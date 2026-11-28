use crate::{
    accounts::database_error,
    auth::{
        server::{connection, now, run_blocking, SessionHash},
        MemberViewer,
    },
    calendar::{CalendarChoice, CalendarChoices, ConnectionStatus, Provider},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use dioxus::server::axum::{
    extract::{Extension, Path, Query},
    http::{header, HeaderMap},
    response::Redirect,
    Form,
};
use ring::aead::{self, Aad, LessSafeKey, Nonce, UnboundKey};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};
use url::Url;

mod agenda;

const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const CALENDAR_LIST_URL: &str = "https://www.googleapis.com/calendar/v3/users/me/calendarList";
const GOOGLE_SCOPES: &str = "https://www.googleapis.com/auth/calendar.calendarlist.readonly https://www.googleapis.com/auth/calendar.events.readonly";
const OUTLOOK_SCOPES: &str = "https://graph.microsoft.com/Calendars.ReadBasic offline_access";
const OUTLOOK_TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const ATTEMPT_SECONDS: i64 = 10 * 60;
const NONCE_BYTES: usize = 12;
const CONNECTION_ERROR: &str = "Could not protect your calendar connection.";

#[derive(Clone)]
pub struct CalendarClient(Arc<Config>);

struct Config {
    provider: Provider,
    origin: String,
    client_id: String,
    client_secret: String,
    token_key: LessSafeKey,
    http: reqwest::Client,
}

struct LinkedCalendar {
    token: Vec<u8>,
    id: Vec<u8>,
    calendar_id: Option<String>,
    shared: bool,
}

enum ProviderError {
    Reconnect,
    Unavailable,
}

#[derive(Clone)]
pub struct CalendarClients {
    pub google: Option<CalendarClient>,
    pub outlook: Option<CalendarClient>,
}

impl CalendarClients {
    pub fn configured(origin: &str) -> Result<Self, &'static str> {
        let google = CalendarClient::configured(origin, Provider::Google)?;
        let outlook =
            CalendarClient::configured(origin, Provider::Outlook).unwrap_or_else(|error| {
                eprintln!("Outlook Calendar is unavailable: {error}");
                None
            });
        Ok(Self { google, outlook })
    }

    pub fn get(&self, provider: Provider) -> Option<&CalendarClient> {
        match provider {
            Provider::Google => self.google.as_ref(),
            Provider::Outlook => self.outlook.as_ref(),
        }
    }
}

impl CalendarClient {
    fn configured(origin: &str, provider: Provider) -> Result<Option<Self>, &'static str> {
        let prefix = match provider {
            Provider::Google => "LAVEMILL_GOOGLE",
            Provider::Outlook => "LAVEMILL_OUTLOOK",
        };
        let read = |suffix| {
            std::env::var(format!("{prefix}_{suffix}"))
                .ok()
                .filter(|value| !value.is_empty())
        };
        let settings = (read("CLIENT_ID"), read("CLIENT_SECRET"), read("TOKEN_KEY"));
        let (client_id, client_secret, token_key) = match settings {
            (Some(id), Some(secret), Some(key)) => (id, secret, key),
            (None, None, None) => return Ok(None),
            _ => return Err(
                "Set the client ID, client secret, and token key to enable this calendar provider.",
            ),
        };
        let key_error = "The calendar token key must be 32 bytes encoded as unpadded base64url.";
        let key = URL_SAFE_NO_PAD.decode(token_key).map_err(|_| key_error)?;
        let token_key = UnboundKey::new(&aead::AES_256_GCM, &key).map_err(|_| key_error)?;
        let http = reqwest::Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(8))
            .build()
            .map_err(|_| "Could not initialize the Calendar HTTP client.")?;
        Ok(Some(Self(Arc::new(Config {
            provider,
            origin: origin.to_owned(),
            client_id,
            client_secret,
            token_key: LessSafeKey::new(token_key),
            http,
        }))))
    }

    pub async fn status(&self, user: i64) -> Result<ConnectionStatus, &'static str> {
        let Some(link) = linked_calendar(self.0.provider.key(), user).await? else {
            return Ok(ConnectionStatus::Disconnected);
        };
        let choices = match self.calendar_choices(user, &link).await {
            Ok(choices) => CalendarChoices::Available(choices),
            Err(ProviderError::Reconnect) => CalendarChoices::Reconnect,
            Err(ProviderError::Unavailable) => CalendarChoices::Unavailable,
        };
        Ok(ConnectionStatus::Connected {
            selected: link.calendar_id,
            shared: link.shared,
            choices,
        })
    }

    async fn access_token(
        &self,
        user: i64,
        connection_id: &[u8],
        sealed: &[u8],
    ) -> Result<String, ProviderError> {
        #[derive(Deserialize)]
        struct Token {
            access_token: String,
            refresh_token: Option<String>,
        }
        #[derive(Deserialize)]
        struct TokenError {
            error: String,
        }

        let token = decrypt_token(&self.0.token_key, user, sealed)?;
        let response = self
            .0
            .http
            .post(self.token_url())
            .form(&[
                ("client_id", self.0.client_id.as_str()),
                ("client_secret", self.0.client_secret.as_str()),
                ("grant_type", "refresh_token"),
                ("refresh_token", token.as_str()),
            ])
            .send()
            .await
            .map_err(|_| ProviderError::Unavailable)?;
        if response.status() == reqwest::StatusCode::BAD_REQUEST {
            return Err(match response.json::<TokenError>().await {
                Ok(error) if error.error == "invalid_grant" => ProviderError::Reconnect,
                _ => ProviderError::Unavailable,
            });
        }
        let token: Token = response
            .error_for_status()
            .map_err(|_| ProviderError::Unavailable)?
            .json()
            .await
            .map_err(|_| ProviderError::Unavailable)?;
        if self.0.provider == Provider::Outlook {
            if let Some(replacement) = token.refresh_token.filter(|value| !value.is_empty()) {
                let replacement = encrypt_token(&self.0.token_key, user, &replacement)
                    .map_err(|_| ProviderError::Unavailable)?;
                let previous = sealed.to_vec();
                let connection_id = connection_id.to_vec();
                run_blocking(move || {
                    replace_outlook_token(
                        &connection()?,
                        user,
                        &connection_id,
                        &previous,
                        &replacement,
                    )
                })
                .await
                .map_err(|_| ProviderError::Unavailable)?;
            }
        }
        Ok(token.access_token)
    }

    async fn calendar_choices(
        &self,
        user: i64,
        link: &LinkedCalendar,
    ) -> Result<Vec<CalendarChoice>, ProviderError> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Page {
            #[serde(default)]
            items: Vec<GoogleChoice>,
            next_page_token: Option<String>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct GoogleChoice {
            id: String,
            summary: String,
            summary_override: Option<String>,
            access_role: String,
        }

        let access_token = self.access_token(user, &link.id, &link.token).await?;
        if self.0.provider == Provider::Outlook {
            #[derive(Deserialize)]
            struct OutlookChoice {
                id: String,
                name: String,
            }

            let mut url = Url::parse("https://graph.microsoft.com/v1.0/me/calendars")
                .expect("Graph calendar URL is valid");
            url.query_pairs_mut().append_pair("$select", "id,name");
            let calendars = self
                .graph_collection::<OutlookChoice>(url, &access_token)
                .await?;
            return Ok(calendars
                .into_iter()
                .map(|choice| CalendarChoice {
                    id: choice.id,
                    name: choice.name,
                })
                .collect());
        }
        let mut choices = Vec::new();
        let mut page_token = None;
        loop {
            let mut request = self
                .0
                .http
                .get(CALENDAR_LIST_URL)
                .bearer_auth(&access_token)
                .query(&[(
                    "fields",
                    "items(id,summary,summaryOverride,accessRole),nextPageToken",
                )]);
            if let Some(token) = page_token {
                request = request.query(&[("pageToken", token)]);
            }
            let page: Page = request
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
                .map_err(|_| ProviderError::Unavailable)?
                .json()
                .await
                .map_err(|_| ProviderError::Unavailable)?;
            choices.extend(page.items.into_iter().filter_map(|choice| {
                matches!(
                    choice.access_role.as_str(),
                    "owner" | "writer" | "reader" | "writerWithoutPrivateAccess"
                )
                .then(|| CalendarChoice {
                    id: choice.id,
                    name: choice.summary_override.unwrap_or(choice.summary),
                })
            }));
            page_token = page.next_page_token;
            if page_token.is_none() {
                break;
            }
        }
        Ok(choices)
    }

    async fn graph_collection<T: serde::de::DeserializeOwned>(
        &self,
        mut url: Url,
        access_token: &str,
    ) -> Result<Vec<T>, ProviderError> {
        #[derive(Deserialize)]
        struct Page<T> {
            value: Vec<T>,
            #[serde(rename = "@odata.nextLink")]
            next_link: Option<String>,
        }

        let mut values = Vec::new();
        loop {
            if !is_graph_url(&url) {
                return Err(ProviderError::Unavailable);
            }
            let page: Page<T> = self
                .0
                .http
                .get(url)
                .bearer_auth(access_token)
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
                .map_err(|_| ProviderError::Unavailable)?
                .json()
                .await
                .map_err(|_| ProviderError::Unavailable)?;
            values.extend(page.value);
            let Some(next) = page.next_link else {
                return Ok(values);
            };
            url = Url::parse(&next).map_err(|_| ProviderError::Unavailable)?;
        }
    }

    fn token_url(&self) -> &'static str {
        match self.0.provider {
            Provider::Google => GOOGLE_TOKEN_URL,
            Provider::Outlook => OUTLOOK_TOKEN_URL,
        }
    }

    fn scopes(&self) -> &'static str {
        match self.0.provider {
            Provider::Google => GOOGLE_SCOPES,
            Provider::Outlook => OUTLOOK_SCOPES,
        }
    }

    fn authorization_url(&self, state: &str, verifier: &str) -> Url {
        let authorization_url = match self.0.provider {
            Provider::Google => "https://accounts.google.com/o/oauth2/v2/auth",
            Provider::Outlook => {
                "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize"
            }
        };
        let mut url = Url::parse(authorization_url).expect("Calendar authorization URL is valid");
        url.query_pairs_mut().extend_pairs([
            ("client_id", self.0.client_id.as_str()),
            ("redirect_uri", &self.callback_url()),
            ("response_type", "code"),
            ("scope", self.scopes()),
            ("state", state),
            ("code_challenge", &URL_SAFE_NO_PAD.encode(hash(verifier))),
            ("code_challenge_method", "S256"),
        ]);
        match self.0.provider {
            Provider::Google => url.query_pairs_mut().extend_pairs([
                ("access_type", "offline"),
                ("prompt", "consent select_account"),
            ]),
            Provider::Outlook => url
                .query_pairs_mut()
                .extend_pairs([("response_mode", "query"), ("prompt", "select_account")]),
        };
        url
    }

    fn callback_url(&self) -> String {
        format!(
            "{}/calendar/{}/callback",
            self.0.origin,
            self.0.provider.key()
        )
    }

    async fn exchange_code(&self, code: &str, verifier: &str) -> Result<String, &'static str> {
        #[derive(Deserialize)]
        struct Token {
            refresh_token: String,
            scope: String,
        }

        let error = "Could not authorize the calendar provider.";
        let token: Token = self
            .0
            .http
            .post(self.token_url())
            .form(&[
                ("client_id", self.0.client_id.as_str()),
                ("client_secret", self.0.client_secret.as_str()),
                ("redirect_uri", &self.callback_url()),
                ("grant_type", "authorization_code"),
                ("code", code),
                ("code_verifier", verifier),
            ])
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|_| error)?
            .json()
            .await
            .map_err(|_| error)?;
        if token.refresh_token.is_empty() || !has_calendar_scope(self.0.provider, &token.scope) {
            return Err(error);
        }
        Ok(token.refresh_token)
    }
}

pub async fn connect(
    Path(provider): Path<Provider>,
    Extension(viewer): Extension<MemberViewer>,
    Extension(session): Extension<SessionHash>,
    Extension(clients): Extension<CalendarClients>,
    headers: HeaderMap,
) -> Redirect {
    let Some(client) = clients.get(provider) else {
        return calendar(provider, "failed");
    };
    if headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        != Some(client.0.origin.as_str())
    {
        return calendar(provider, "origin");
    }
    let Ok((state, verifier)) = random_bytes::<32>().and_then(|state| {
        Ok((
            URL_SAFE_NO_PAD.encode(state),
            URL_SAFE_NO_PAD.encode(random_bytes::<32>()?),
        ))
    }) else {
        return calendar(provider, "failed");
    };
    let url = client.authorization_url(&state, &verifier);
    let saved = run_blocking(move || {
        start_attempt(
            &connection()?,
            provider.key(),
            viewer.id,
            session,
            &state,
            &verifier,
        )
    })
    .await;
    match saved {
        Ok(_) => Redirect::to(url.as_str()),
        Err(_) => calendar(provider, "failed"),
    }
}

#[derive(Deserialize)]
pub struct Callback {
    state: Option<String>,
    code: Option<String>,
    error: Option<String>,
}

pub async fn callback(
    Path(provider): Path<Provider>,
    Extension(viewer): Extension<MemberViewer>,
    Extension(session): Extension<SessionHash>,
    Extension(clients): Extension<CalendarClients>,
    Query(query): Query<Callback>,
) -> Redirect {
    let (Some(client), Some(state)) = (clients.get(provider), query.state) else {
        return calendar(provider, "failed");
    };
    let user = viewer.id;
    let state = hash(&state);
    let Ok(Some(verifier)) =
        run_blocking(move || claim_attempt(&connection()?, provider.key(), user, session, state))
            .await
    else {
        return calendar(provider, "failed");
    };
    if query.error.as_deref() == Some("access_denied") {
        return calendar(provider, "cancelled");
    }
    let Some(code) = query.code else {
        return calendar(provider, "failed");
    };
    let Ok(token) = client
        .exchange_code(&code, &verifier)
        .await
        .and_then(|token| encrypt_token(&client.0.token_key, user, &token))
    else {
        return calendar(provider, "failed");
    };
    match run_blocking(move || {
        save_connection(&mut connection()?, provider.key(), user, state, &token)
    })
    .await
    {
        Ok(true) => calendar(provider, "connected"),
        _ => calendar(provider, "failed"),
    }
}

pub async fn disconnect(
    Path(provider): Path<Provider>,
    Extension(viewer): Extension<MemberViewer>,
) -> Redirect {
    match run_blocking(move || remove_connection(&mut connection()?, provider.key(), viewer.id))
        .await
    {
        Ok(()) => calendar(provider, "disconnected"),
        Err(_) => calendar(provider, "failed"),
    }
}

#[derive(Deserialize)]
pub struct CalendarSelection {
    calendar_id: String,
}

pub async fn select(
    Path(provider): Path<Provider>,
    Extension(viewer): Extension<MemberViewer>,
    Extension(clients): Extension<CalendarClients>,
    Form(selection): Form<CalendarSelection>,
) -> Redirect {
    let (Some(client), Ok(Some(link))) = (
        clients.get(provider),
        linked_calendar(provider.key(), viewer.id).await,
    ) else {
        return calendar(provider, "failed");
    };
    let id = selection.calendar_id;
    match client.calendar_choices(viewer.id, &link).await {
        Ok(choices) if choices.iter().any(|choice| choice.id == id) => {}
        Err(ProviderError::Reconnect) => return calendar(provider, "reconnect"),
        _ => return calendar(provider, "selection_failed"),
    }
    let user = viewer.id;
    let changed =
        run_blocking(move || save_selection(&connection()?, provider.key(), user, &link.id, &id))
            .await;
    match changed {
        Ok(true) => calendar(provider, "selected"),
        _ => calendar(provider, "selection_failed"),
    }
}

#[derive(Deserialize)]
pub struct CalendarSharing {
    calendar_id: String,
    shared: bool,
}

pub async fn share(
    Path(provider): Path<Provider>,
    Extension(viewer): Extension<MemberViewer>,
    Extension(clients): Extension<CalendarClients>,
    Form(sharing): Form<CalendarSharing>,
) -> Redirect {
    if sharing.shared && clients.get(provider).is_none() {
        return calendar(provider, "failed");
    }
    let user = viewer.id;
    let shared = sharing.shared;
    let saved = run_blocking(move || {
        set_shared(
            &connection()?,
            provider.key(),
            user,
            &sharing.calendar_id,
            shared,
        )
    })
    .await;
    match saved {
        Ok(true) if shared => calendar(provider, "shared"),
        Ok(true) => calendar(provider, "unshared"),
        _ => calendar(provider, "selection_failed"),
    }
}

async fn linked_calendar(
    provider: &'static str,
    user: i64,
) -> Result<Option<LinkedCalendar>, &'static str> {
    run_blocking(move || {
        connection()?.query_row(
            "SELECT refresh_token, calendar_id, shared, id FROM calendar_connections WHERE user_id = ?1 AND provider = ?2",
            params![user, provider],
            |row| Ok(LinkedCalendar { token: row.get(0)?, calendar_id: row.get(1)?, shared: row.get(2)?, id: row.get(3)? }),
        ).optional().map_err(database_error)
    }).await
}

fn save_selection(
    db: &Connection,
    provider: &str,
    user: i64,
    connection_id: &[u8],
    id: &str,
) -> Result<bool, &'static str> {
    db.execute(
        "UPDATE calendar_connections
         SET calendar_id = ?3, shared = CASE WHEN calendar_id = ?3 THEN shared ELSE 0 END
         WHERE user_id = ?1 AND id = ?2 AND provider = ?4",
        params![user, connection_id, id, provider],
    )
    .map(|rows| rows == 1)
    .map_err(database_error)
}

fn set_shared(
    db: &Connection,
    provider: &str,
    user: i64,
    id: &str,
    shared: bool,
) -> Result<bool, &'static str> {
    db.execute(
        "UPDATE calendar_connections SET shared = ?3 WHERE user_id = ?1 AND calendar_id = ?2 AND provider = ?4",
        params![user, id, shared, provider],
    ).map(|rows| rows == 1).map_err(database_error)
}

fn start_attempt(
    db: &Connection,
    provider: &str,
    user: i64,
    session: SessionHash,
    state: &str,
    verifier: &str,
) -> Result<usize, &'static str> {
    db.execute(
        "INSERT INTO calendar_attempts (user_id, session_hash, state_hash, verifier, expires_at, provider)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(user_id, provider) DO UPDATE SET session_hash = excluded.session_hash,
           state_hash = excluded.state_hash, verifier = excluded.verifier,
           expires_at = excluded.expires_at, claimed = 0",
        params![user, &session.0[..], &hash(state)[..], verifier, now() + ATTEMPT_SECONDS, provider],
    ).map_err(database_error)
}

fn claim_attempt(
    db: &Connection,
    provider: &str,
    user: i64,
    session: SessionHash,
    state: [u8; 32],
) -> Result<Option<String>, &'static str> {
    db.query_row(
        "UPDATE calendar_attempts SET claimed = 1
         WHERE user_id = ?1 AND session_hash = ?2 AND state_hash = ?3
           AND expires_at > ?4 AND claimed = 0 AND provider = ?5 RETURNING verifier",
        params![user, &session.0[..], &state[..], now(), provider],
        |row| row.get(0),
    )
    .optional()
    .map_err(database_error)
}

fn save_connection(
    db: &mut Connection,
    provider: &str,
    user: i64,
    state: [u8; 32],
    token: &[u8],
) -> Result<bool, &'static str> {
    let tx = db.transaction().map_err(database_error)?;
    let claimed = tx
        .execute(
            "DELETE FROM calendar_attempts
         WHERE user_id = ?1 AND state_hash = ?2 AND expires_at > ?3 AND claimed = 1 AND provider = ?4
           AND EXISTS (SELECT 1 FROM sessions WHERE token_hash = session_hash AND expires_at > ?3)",
            params![user, &state[..], now(), provider],
        )
        .map_err(database_error)?;
    if claimed == 0 {
        return Ok(false);
    }
    tx.execute(
        "INSERT INTO calendar_connections (user_id, refresh_token, provider) VALUES (?1, ?2, ?3)
         ON CONFLICT(user_id, provider) DO UPDATE SET refresh_token = excluded.refresh_token, id = excluded.id,
           calendar_id = NULL, shared = 0",
        params![user, token, provider],
    )
    .map_err(database_error)?;
    tx.commit().map_err(database_error)?;
    Ok(true)
}

fn remove_connection(db: &mut Connection, provider: &str, user: i64) -> Result<(), &'static str> {
    let tx = db.transaction().map_err(database_error)?;
    tx.execute(
        "DELETE FROM calendar_attempts WHERE user_id = ?1 AND provider = ?2",
        params![user, provider],
    )
    .map_err(database_error)?;
    tx.execute(
        "DELETE FROM calendar_connections WHERE user_id = ?1 AND provider = ?2",
        params![user, provider],
    )
    .map_err(database_error)?;
    tx.commit().map_err(database_error)
}

fn encrypt_token(key: &LessSafeKey, user: i64, token: &str) -> Result<Vec<u8>, &'static str> {
    let nonce = random_bytes::<NONCE_BYTES>()?;
    let mut sealed = token.as_bytes().to_vec();
    key.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce),
        Aad::from(user.to_be_bytes()),
        &mut sealed,
    )
    .map_err(|_| CONNECTION_ERROR)?;
    let mut stored = nonce.to_vec();
    stored.extend(sealed);
    Ok(stored)
}

fn decrypt_token(key: &LessSafeKey, user: i64, stored: &[u8]) -> Result<String, ProviderError> {
    let (nonce, sealed) = stored
        .split_at_checked(NONCE_BYTES)
        .ok_or(ProviderError::Reconnect)?;
    let nonce = Nonce::try_assume_unique_for_key(nonce).map_err(|_| ProviderError::Reconnect)?;
    let mut sealed = sealed.to_vec();
    let plaintext = key
        .open_in_place(nonce, Aad::from(user.to_be_bytes()), &mut sealed)
        .map_err(|_| ProviderError::Reconnect)?;
    String::from_utf8(plaintext.to_vec()).map_err(|_| ProviderError::Reconnect)
}

fn random_bytes<const N: usize>() -> Result<[u8; N], &'static str> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).map_err(|_| CONNECTION_ERROR)?;
    Ok(bytes)
}

fn hash(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
}

fn calendar(provider: Provider, notice: &str) -> Redirect {
    match provider {
        Provider::Google => Redirect::to(&format!("/calendar?notice={notice}")),
        Provider::Outlook => Redirect::to(&format!("/calendar?notice=outlook_{notice}")),
    }
}

fn is_graph_url(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("graph.microsoft.com")
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
}

fn has_calendar_scope(provider: Provider, granted: &str) -> bool {
    match provider {
        Provider::Google => GOOGLE_SCOPES
            .split_whitespace()
            .all(|scope| granted.split_whitespace().any(|value| scope == value)),
        Provider::Outlook => granted.split_whitespace().any(|value| {
            value
                .rsplit('/')
                .next()
                .is_some_and(|scope| scope.eq_ignore_ascii_case("Calendars.ReadBasic"))
        }),
    }
}

fn replace_outlook_token(
    db: &Connection,
    user: i64,
    id: &[u8],
    previous: &[u8],
    replacement: &[u8],
) -> Result<bool, &'static str> {
    db.execute(
        "UPDATE calendar_connections SET refresh_token = ?4
         WHERE user_id = ?1 AND provider = 'outlook' AND id = ?2 AND refresh_token = ?3",
        params![user, id, previous, replacement],
    )
    .map(|rows| rows == 1)
    .map_err(database_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    const GOOGLE: &str = "google";
    use crate::database;

    #[test]
    fn oauth_attempt_is_bound_to_member_and_session_and_can_only_be_used_once() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("calendar.sqlite3");
        database::initialize(&path).unwrap();
        let mut db = database::open(&path).unwrap();
        for user in [1i64, 2] {
            db.execute(
                "INSERT INTO users (id, username, first_name, last_name, password_hash)
                 VALUES (?1, 'test' || ?1, 'Test', 'Member', 'unused')",
                [user],
            )
            .unwrap();
        }
        let session = SessionHash([4; 32]);
        let state = hash("test state");
        db.execute(
            "INSERT INTO sessions (token_hash, user_id, expires_at) VALUES (?1, 1, ?2)",
            params![&session.0[..], now() + 120],
        )
        .unwrap();
        let start = |db: &Connection| {
            db.execute(
                "INSERT INTO calendar_attempts
                 (user_id, session_hash, state_hash, verifier, expires_at, provider)
                 VALUES (1, ?1, ?2, 'test verifier', ?3, 'google')",
                params![&session.0[..], &state[..], now() + 60],
            )
            .unwrap();
        };
        start(&db);
        assert_eq!(
            claim_attempt(&db, "outlook", 1, session, state).unwrap(),
            None
        );
        for (user, session, state) in [
            (2, session, state),
            (1, SessionHash([5; 32]), state),
            (1, session, hash("different state")),
        ] {
            assert_eq!(
                claim_attempt(&db, GOOGLE, user, session, state).unwrap(),
                None
            );
        }
        assert_eq!(
            claim_attempt(&db, GOOGLE, 1, session, state)
                .unwrap()
                .as_deref(),
            Some("test verifier")
        );
        assert_eq!(claim_attempt(&db, GOOGLE, 1, session, state).unwrap(), None);
        assert!(save_connection(&mut db, GOOGLE, 1, state, b"sealed token").unwrap());
        drop(db);
        let mut db = database::open(&path).unwrap();
        let token: Vec<u8> = db
            .query_row(
                "SELECT refresh_token FROM calendar_connections WHERE user_id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(token, b"sealed token");
        let id: Vec<u8> = db
            .query_row(
                "SELECT id FROM calendar_connections WHERE user_id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!set_shared(&db, GOOGLE, 2, "family", true).unwrap());
        assert!(!set_shared(&db, GOOGLE, 1, "family", true).unwrap());
        assert!(!save_selection(&db, GOOGLE, 1, b"old token", "family").unwrap());
        assert!(save_selection(&db, GOOGLE, 1, &id, "family").unwrap());
        assert!(set_shared(&db, GOOGLE, 1, "family", true).unwrap());
        assert!(save_selection(&db, GOOGLE, 1, &id, "family").unwrap());
        let shared = || {
            db.query_row(
                "SELECT shared FROM calendar_connections WHERE user_id = 1",
                [],
                |row| row.get::<_, bool>(0),
            )
            .unwrap()
        };
        assert!(shared());
        assert!(save_selection(&db, GOOGLE, 1, &id, "personal").unwrap());
        assert!(!shared());
        assert!(!set_shared(&db, GOOGLE, 1, "family", true).unwrap());
        assert!(set_shared(&db, GOOGLE, 1, "personal", true).unwrap());
        assert!(set_shared(&db, GOOGLE, 1, "personal", false).unwrap());
        assert!(!shared());

        let outlook = Provider::Outlook.key();
        let cancelled = "cancelled outlook authorization";
        start_attempt(&db, outlook, 1, session, cancelled, "verifier").unwrap();
        assert!(claim_attempt(&db, outlook, 1, session, hash(cancelled))
            .unwrap()
            .is_some());
        assert!(claim_attempt(&db, outlook, 1, session, hash(cancelled))
            .unwrap()
            .is_none());
        let outlook_state = "outlook authorization";
        start_attempt(&db, outlook, 1, session, outlook_state, "verifier").unwrap();
        assert!(claim_attempt(&db, outlook, 1, session, hash(outlook_state))
            .unwrap()
            .is_some());
        assert!(save_connection(
            &mut db,
            outlook,
            1,
            hash(outlook_state),
            b"initial outlook token"
        )
        .unwrap());
        let outlook_id: Vec<u8> = db
            .query_row(
                "SELECT id FROM calendar_connections WHERE user_id = 1 AND provider = 'outlook'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(replace_outlook_token(
            &db,
            1,
            &outlook_id,
            b"initial outlook token",
            b"replacement"
        )
        .unwrap());
        assert!(!replace_outlook_token(
            &db,
            1,
            &outlook_id,
            b"initial outlook token",
            b"late replacement"
        )
        .unwrap());
        remove_connection(&mut db, outlook, 1).unwrap();
        assert!(
            !replace_outlook_token(&db, 1, &outlook_id, b"replacement", b"late replacement")
                .unwrap()
        );
        assert!(set_shared(&db, GOOGLE, 1, "personal", true).unwrap());

        start(&db);
        assert!(claim_attempt(&db, GOOGLE, 1, session, state)
            .unwrap()
            .is_some());
        remove_connection(&mut db, GOOGLE, 1).unwrap();
        assert!(!save_connection(&mut db, GOOGLE, 1, state, b"stale token").unwrap());
        let rows: i64 = db
            .query_row("SELECT COUNT(*) FROM calendar_connections", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(rows, 0);
    }

    #[test]
    fn outlook_accepts_only_the_calendar_grant_and_graph_origin() {
        assert!(has_calendar_scope(Provider::Outlook, "Calendars.ReadBasic"));
        assert!(has_calendar_scope(
            Provider::Outlook,
            "https://graph.microsoft.com/Calendars.ReadBasic"
        ));
        assert!(!has_calendar_scope(Provider::Outlook, "offline_access"));
        assert!(is_graph_url(
            &Url::parse("https://graph.microsoft.com/v1.0/me/calendars").unwrap()
        ));
        assert!(!is_graph_url(
            &Url::parse("https://graph.microsoft.com.evil.invalid/v1.0/me/calendars").unwrap()
        ));
    }

    #[test]
    fn refresh_token_is_encrypted_for_its_owner() {
        let key = LessSafeKey::new(UnboundKey::new(&aead::AES_256_GCM, &[8; 32]).unwrap());
        let stored = encrypt_token(&key, 42, "refresh token").unwrap();
        assert_ne!(stored, encrypt_token(&key, 42, "refresh token").unwrap());
        assert_eq!(
            decrypt_token(&key, 42, &stored).ok().as_deref(),
            Some("refresh token")
        );
        assert!(matches!(
            decrypt_token(&key, 43, &stored),
            Err(ProviderError::Reconnect)
        ));
    }
}
