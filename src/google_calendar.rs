use crate::{
    accounts::database_error,
    auth::{
        server::{connection, now, run_blocking, SessionHash},
        Viewer,
    },
    calendar::{CalendarChoice, CalendarChoices, ConnectionStatus},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use dioxus::server::axum::{
    extract::{Extension, Query},
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

const CALLBACK_PATH: &str = "/calendar/google/callback";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const CALENDAR_LIST_URL: &str = "https://www.googleapis.com/calendar/v3/users/me/calendarList";
const SCOPES: &str = "https://www.googleapis.com/auth/calendar.calendarlist.readonly https://www.googleapis.com/auth/calendar.events.readonly";
const ATTEMPT_SECONDS: i64 = 10 * 60;
const NONCE_BYTES: usize = 12;
const CONNECTION_ERROR: &str = "Could not protect your Google Calendar connection.";

#[derive(Clone)]
pub struct GoogleCalendar(Arc<Config>);

struct Config {
    origin: String,
    client_id: String,
    client_secret: String,
    token_key: LessSafeKey,
    http: reqwest::Client,
}

struct LinkedCalendar {
    token: Vec<u8>,
    calendar_id: Option<String>,
    shared: bool,
}

enum GoogleError {
    Reconnect,
    Unavailable,
}

impl GoogleCalendar {
    pub fn configured(origin: &str) -> Result<Option<Self>, &'static str> {
        let read = |name| std::env::var(name).ok().filter(|value| !value.is_empty());
        let settings = (
            read("LAVEMILL_GOOGLE_CLIENT_ID"),
            read("LAVEMILL_GOOGLE_CLIENT_SECRET"),
            read("LAVEMILL_GOOGLE_TOKEN_KEY"),
        );
        let (client_id, client_secret, token_key) = match settings {
            (Some(id), Some(secret), Some(key)) => (id, secret, key),
            (None, None, None) => return Ok(None),
            _ => return Err("Configure all three LAVEMILL_GOOGLE settings to enable Calendar."),
        };
        let key_error = "LAVEMILL_GOOGLE_TOKEN_KEY must be 32 bytes encoded as unpadded base64url.";
        let key = URL_SAFE_NO_PAD.decode(token_key).map_err(|_| key_error)?;
        let token_key = UnboundKey::new(&aead::AES_256_GCM, &key).map_err(|_| key_error)?;
        let http = reqwest::Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(8))
            .build()
            .map_err(|_| "Could not initialize the Google Calendar HTTP client.")?;
        Ok(Some(Self(Arc::new(Config {
            origin: origin.to_owned(),
            client_id,
            client_secret,
            token_key: LessSafeKey::new(token_key),
            http,
        }))))
    }

    pub async fn status(&self, user: i64) -> Result<ConnectionStatus, &'static str> {
        let Some(link) = linked_calendar(user).await? else {
            return Ok(ConnectionStatus::Disconnected);
        };
        let choices = match self.calendar_choices(user, &link.token).await {
            Ok(choices) => CalendarChoices::Available(choices),
            Err(GoogleError::Reconnect) => CalendarChoices::Reconnect,
            Err(GoogleError::Unavailable) => CalendarChoices::Unavailable,
        };
        Ok(ConnectionStatus::Connected {
            selected: link.calendar_id,
            shared: link.shared,
            choices,
        })
    }

    async fn access_token(&self, user: i64, sealed: &[u8]) -> Result<String, GoogleError> {
        #[derive(Deserialize)]
        struct Token {
            access_token: String,
        }
        #[derive(Deserialize)]
        struct TokenError {
            error: String,
        }

        let token = decrypt_token(&self.0.token_key, user, sealed)?;
        let response = self
            .0
            .http
            .post(TOKEN_URL)
            .form(&[
                ("client_id", self.0.client_id.as_str()),
                ("client_secret", self.0.client_secret.as_str()),
                ("grant_type", "refresh_token"),
                ("refresh_token", token.as_str()),
            ])
            .send()
            .await
            .map_err(|_| GoogleError::Unavailable)?;
        if response.status() == reqwest::StatusCode::BAD_REQUEST {
            return Err(match response.json::<TokenError>().await {
                Ok(error) if error.error == "invalid_grant" => GoogleError::Reconnect,
                _ => GoogleError::Unavailable,
            });
        }
        let token: Token = response
            .error_for_status()
            .map_err(|_| GoogleError::Unavailable)?
            .json()
            .await
            .map_err(|_| GoogleError::Unavailable)?;
        Ok(token.access_token)
    }

    async fn calendar_choices(
        &self,
        user: i64,
        sealed: &[u8],
    ) -> Result<Vec<CalendarChoice>, GoogleError> {
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

        let access_token = self.access_token(user, sealed).await?;
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
                .map_err(|_| GoogleError::Unavailable)?
                .json()
                .await
                .map_err(|_| GoogleError::Unavailable)?;
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

    fn authorization_url(&self, state: &str, verifier: &str) -> Url {
        let mut url = Url::parse("https://accounts.google.com/o/oauth2/v2/auth")
            .expect("Google authorization URL is valid");
        url.query_pairs_mut().extend_pairs([
            ("client_id", self.0.client_id.as_str()),
            ("redirect_uri", &self.callback_url()),
            ("response_type", "code"),
            ("scope", SCOPES),
            ("access_type", "offline"),
            ("prompt", "consent select_account"),
            ("state", state),
            ("code_challenge", &URL_SAFE_NO_PAD.encode(hash(verifier))),
            ("code_challenge_method", "S256"),
        ]);
        url
    }

    fn callback_url(&self) -> String {
        format!("{}{CALLBACK_PATH}", self.0.origin)
    }

    async fn exchange_code(&self, code: &str, verifier: &str) -> Result<String, &'static str> {
        #[derive(Deserialize)]
        struct Token {
            refresh_token: String,
            scope: String,
        }

        let error = "Could not authorize Google Calendar.";
        let token: Token = self
            .0
            .http
            .post(TOKEN_URL)
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
        if token.refresh_token.is_empty()
            || !SCOPES.split_whitespace().all(|scope| {
                token
                    .scope
                    .split_whitespace()
                    .any(|granted| granted == scope)
            })
        {
            return Err(error);
        }
        Ok(token.refresh_token)
    }
}

pub async fn connect(
    Extension(viewer): Extension<Viewer>,
    Extension(session): Extension<SessionHash>,
    google: Option<Extension<GoogleCalendar>>,
    headers: HeaderMap,
) -> Redirect {
    let Some(Extension(google)) = google else {
        return calendar("failed");
    };
    if headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        != Some(google.0.origin.as_str())
    {
        return calendar("origin");
    }
    let Ok((state, verifier)) = random_bytes::<32>().and_then(|state| {
        Ok((
            URL_SAFE_NO_PAD.encode(state),
            URL_SAFE_NO_PAD.encode(random_bytes::<32>()?),
        ))
    }) else {
        return calendar("failed");
    };
    let url = google.authorization_url(&state, &verifier);
    let saved = run_blocking(move || {
        connection()?.execute(
            "INSERT INTO google_calendar_attempts (user_id, session_hash, state_hash, verifier, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(user_id) DO UPDATE SET session_hash = excluded.session_hash,
               state_hash = excluded.state_hash, verifier = excluded.verifier,
               expires_at = excluded.expires_at, claimed = 0",
            params![viewer.id, &session.0[..], &hash(&state)[..], verifier, now() + ATTEMPT_SECONDS],
        ).map_err(database_error)
    }).await;
    match saved {
        Ok(_) => Redirect::to(url.as_str()),
        Err(_) => calendar("failed"),
    }
}

#[derive(Deserialize)]
pub struct Callback {
    state: Option<String>,
    code: Option<String>,
    error: Option<String>,
}

pub async fn callback(
    Extension(viewer): Extension<Viewer>,
    Extension(session): Extension<SessionHash>,
    google: Option<Extension<GoogleCalendar>>,
    Query(query): Query<Callback>,
) -> Redirect {
    let (Some(Extension(google)), Some(state)) = (google, query.state) else {
        return calendar("failed");
    };
    let user = viewer.id;
    let state = hash(&state);
    let Ok(Some(verifier)) =
        run_blocking(move || claim_attempt(&connection()?, user, session, state)).await
    else {
        return calendar("failed");
    };
    if query.error.as_deref() == Some("access_denied") {
        return calendar("cancelled");
    }
    let Some(code) = query.code else {
        return calendar("failed");
    };
    let Ok(token) = google
        .exchange_code(&code, &verifier)
        .await
        .and_then(|token| encrypt_token(&google.0.token_key, user, &token))
    else {
        return calendar("failed");
    };
    match run_blocking(move || save_connection(&mut connection()?, user, state, &token)).await {
        Ok(true) => calendar("connected"),
        _ => calendar("failed"),
    }
}

pub async fn disconnect(Extension(viewer): Extension<Viewer>) -> Redirect {
    match run_blocking(move || remove_connection(&mut connection()?, viewer.id)).await {
        Ok(()) => calendar("disconnected"),
        Err(_) => calendar("failed"),
    }
}

#[derive(Deserialize)]
pub struct CalendarSelection {
    calendar_id: String,
}

pub async fn select(
    Extension(viewer): Extension<Viewer>,
    google: Option<Extension<GoogleCalendar>>,
    Form(selection): Form<CalendarSelection>,
) -> Redirect {
    let (Some(Extension(google)), Ok(Some(link))) = (google, linked_calendar(viewer.id).await)
    else {
        return calendar("failed");
    };
    let id = selection.calendar_id;
    match google.calendar_choices(viewer.id, &link.token).await {
        Ok(choices) if choices.iter().any(|choice| choice.id == id) => {}
        Err(GoogleError::Reconnect) => return calendar("reconnect"),
        _ => return calendar("selection_failed"),
    }
    let user = viewer.id;
    let changed =
        run_blocking(move || save_selection(&connection()?, user, &link.token, &id)).await;
    match changed {
        Ok(true) => calendar("selected"),
        _ => calendar("selection_failed"),
    }
}

#[derive(Deserialize)]
pub struct CalendarSharing {
    calendar_id: String,
    shared: bool,
}

pub async fn share(
    Extension(viewer): Extension<Viewer>,
    google: Option<Extension<GoogleCalendar>>,
    Form(sharing): Form<CalendarSharing>,
) -> Redirect {
    if sharing.shared && google.is_none() {
        return calendar("failed");
    }
    let user = viewer.id;
    let shared = sharing.shared;
    let saved =
        run_blocking(move || set_shared(&connection()?, user, &sharing.calendar_id, shared)).await;
    match saved {
        Ok(true) if shared => calendar("shared"),
        Ok(true) => calendar("unshared"),
        _ => calendar("selection_failed"),
    }
}

async fn linked_calendar(user: i64) -> Result<Option<LinkedCalendar>, &'static str> {
    run_blocking(move || {
        connection()?.query_row(
            "SELECT refresh_token, calendar_id, shared FROM google_calendar_connections WHERE user_id = ?1",
            [user],
            |row| Ok(LinkedCalendar { token: row.get(0)?, calendar_id: row.get(1)?, shared: row.get(2)? }),
        ).optional().map_err(database_error)
    }).await
}

fn save_selection(
    db: &Connection,
    user: i64,
    token: &[u8],
    id: &str,
) -> Result<bool, &'static str> {
    db.execute(
        "UPDATE google_calendar_connections
         SET calendar_id = ?3, shared = CASE WHEN calendar_id = ?3 THEN shared ELSE 0 END
         WHERE user_id = ?1 AND refresh_token = ?2",
        params![user, token, id],
    )
    .map(|rows| rows == 1)
    .map_err(database_error)
}

fn set_shared(db: &Connection, user: i64, id: &str, shared: bool) -> Result<bool, &'static str> {
    db.execute(
        "UPDATE google_calendar_connections SET shared = ?3 WHERE user_id = ?1 AND calendar_id = ?2",
        params![user, id, shared],
    ).map(|rows| rows == 1).map_err(database_error)
}

fn claim_attempt(
    db: &Connection,
    user: i64,
    session: SessionHash,
    state: [u8; 32],
) -> Result<Option<String>, &'static str> {
    db.query_row(
        "UPDATE google_calendar_attempts SET claimed = 1
         WHERE user_id = ?1 AND session_hash = ?2 AND state_hash = ?3
           AND expires_at > ?4 AND claimed = 0 RETURNING verifier",
        params![user, &session.0[..], &state[..], now()],
        |row| row.get(0),
    )
    .optional()
    .map_err(database_error)
}

fn save_connection(
    db: &mut Connection,
    user: i64,
    state: [u8; 32],
    token: &[u8],
) -> Result<bool, &'static str> {
    let tx = db.transaction().map_err(database_error)?;
    let claimed = tx
        .execute(
            "DELETE FROM google_calendar_attempts
         WHERE user_id = ?1 AND state_hash = ?2 AND expires_at > ?3 AND claimed = 1
           AND EXISTS (SELECT 1 FROM sessions WHERE token_hash = session_hash AND expires_at > ?3)",
            params![user, &state[..], now()],
        )
        .map_err(database_error)?;
    if claimed == 0 {
        return Ok(false);
    }
    tx.execute(
        "INSERT INTO google_calendar_connections (user_id, refresh_token) VALUES (?1, ?2)
         ON CONFLICT(user_id) DO UPDATE SET refresh_token = excluded.refresh_token,
           calendar_id = NULL, shared = 0",
        params![user, token],
    )
    .map_err(database_error)?;
    tx.commit().map_err(database_error)?;
    Ok(true)
}

fn remove_connection(db: &mut Connection, user: i64) -> Result<(), &'static str> {
    let tx = db.transaction().map_err(database_error)?;
    tx.execute(
        "DELETE FROM google_calendar_attempts WHERE user_id = ?1",
        [user],
    )
    .map_err(database_error)?;
    tx.execute(
        "DELETE FROM google_calendar_connections WHERE user_id = ?1",
        [user],
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

fn decrypt_token(key: &LessSafeKey, user: i64, stored: &[u8]) -> Result<String, GoogleError> {
    let (nonce, sealed) = stored
        .split_at_checked(NONCE_BYTES)
        .ok_or(GoogleError::Reconnect)?;
    let nonce = Nonce::try_assume_unique_for_key(nonce).map_err(|_| GoogleError::Reconnect)?;
    let mut sealed = sealed.to_vec();
    let plaintext = key
        .open_in_place(nonce, Aad::from(user.to_be_bytes()), &mut sealed)
        .map_err(|_| GoogleError::Reconnect)?;
    String::from_utf8(plaintext.to_vec()).map_err(|_| GoogleError::Reconnect)
}

fn random_bytes<const N: usize>() -> Result<[u8; N], &'static str> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).map_err(|_| CONNECTION_ERROR)?;
    Ok(bytes)
}

fn hash(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
}

fn calendar(notice: &str) -> Redirect {
    Redirect::to(&format!("/calendar?notice={notice}"))
}

#[cfg(test)]
mod tests {
    use super::*;
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
                "INSERT INTO google_calendar_attempts
                 (user_id, session_hash, state_hash, verifier, expires_at)
                 VALUES (1, ?1, ?2, 'test verifier', ?3)",
                params![&session.0[..], &state[..], now() + 60],
            )
            .unwrap();
        };
        start(&db);
        for (user, session, state) in [
            (2, session, state),
            (1, SessionHash([5; 32]), state),
            (1, session, hash("different state")),
        ] {
            assert_eq!(claim_attempt(&db, user, session, state).unwrap(), None);
        }
        assert_eq!(
            claim_attempt(&db, 1, session, state).unwrap().as_deref(),
            Some("test verifier")
        );
        assert_eq!(claim_attempt(&db, 1, session, state).unwrap(), None);
        assert!(save_connection(&mut db, 1, state, b"sealed token").unwrap());
        drop(db);
        let mut db = database::open(&path).unwrap();
        let token: Vec<u8> = db
            .query_row(
                "SELECT refresh_token FROM google_calendar_connections WHERE user_id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(token, b"sealed token");
        assert!(!set_shared(&db, 2, "family", true).unwrap());
        assert!(!set_shared(&db, 1, "family", true).unwrap());
        assert!(!save_selection(&db, 1, b"old token", "family").unwrap());
        assert!(save_selection(&db, 1, &token, "family").unwrap());
        assert!(set_shared(&db, 1, "family", true).unwrap());
        assert!(save_selection(&db, 1, &token, "family").unwrap());
        let shared = || {
            db.query_row(
                "SELECT shared FROM google_calendar_connections WHERE user_id = 1",
                [],
                |row| row.get::<_, bool>(0),
            )
            .unwrap()
        };
        assert!(shared());
        assert!(save_selection(&db, 1, &token, "personal").unwrap());
        assert!(!shared());
        assert!(!set_shared(&db, 1, "family", true).unwrap());
        assert!(set_shared(&db, 1, "personal", true).unwrap());
        assert!(set_shared(&db, 1, "personal", false).unwrap());
        assert!(!shared());

        start(&db);
        assert!(claim_attempt(&db, 1, session, state).unwrap().is_some());
        remove_connection(&mut db, 1).unwrap();
        assert!(!save_connection(&mut db, 1, state, b"stale token").unwrap());
        let rows: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM google_calendar_connections",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(rows, 0);
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
            Err(GoogleError::Reconnect)
        ));
    }
}
