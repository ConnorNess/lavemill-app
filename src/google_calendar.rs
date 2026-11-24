use crate::{
    accounts::database_error,
    auth::{
        server::{connection, now, run_blocking, SessionHash},
        Viewer,
    },
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use dioxus::server::axum::{
    extract::{Extension, Query},
    http::{header, HeaderMap},
    response::Redirect,
};
use ring::aead::{self, Aad, LessSafeKey, Nonce, UnboundKey};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};
use url::Url;

const CALLBACK_PATH: &str = "/calendar/google/callback";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
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

    pub async fn is_connected(user: i64) -> Result<bool, &'static str> {
        run_blocking(move || {
            connection()?
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM google_calendar_connections WHERE user_id = ?1)",
                    [user],
                    |row| row.get(0),
                )
                .map_err(database_error)
        })
        .await
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
         ON CONFLICT(user_id) DO UPDATE SET refresh_token = excluded.refresh_token",
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
        let (nonce, sealed) = stored.split_at(NONCE_BYTES);
        let nonce = Nonce::try_assume_unique_for_key(nonce).unwrap();
        let mut sealed = sealed.to_vec();
        assert_eq!(
            key.open_in_place(nonce, Aad::from(42i64.to_be_bytes()), &mut sealed)
                .unwrap(),
            b"refresh token"
        );
    }
}
