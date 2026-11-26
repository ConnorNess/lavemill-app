use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Viewer {
    pub id: i64,
    pub first_name: String,
    pub is_admin: bool,
}

#[get("/api/session")]
pub async fn current_viewer() -> Result<Option<Viewer>, ServerFnError> {
    Ok(request_viewer())
}

#[cfg(feature = "server")]
pub(crate) fn request_viewer() -> Option<Viewer> {
    dioxus::fullstack::FullstackContext::current().and_then(|context| context.extension())
}

#[cfg(feature = "server")]
pub(crate) fn require_member() -> Result<Viewer, ServerFnError> {
    request_viewer().ok_or_else(|| ServerFnError::new("Sign in to continue."))
}

#[component]
pub fn Login(error: Option<String>) -> Element {
    let message = match error.as_deref() {
        Some("credentials") => Some("The username or password is incorrect."),
        Some("retry") => Some("Too many attempts. Try again in a minute."),
        Some("unavailable") => Some("Sign-in is temporarily unavailable. Try again."),
        _ => None,
    };
    rsx! {
        main { id: "main", class: "dashboard login-page",
            document::Title { "Lavemill · Sign in" }
            section { class: "add-task-panel",
                h1 { "Sign in to Lavemill" }
                form { action: "/auth/login", method: "post",
                    label { r#for: "login-username", "Username" }
                    input { id: "login-username", name: "username", autocomplete: "username", maxlength: "40", required: true }
                    label { r#for: "login-password", "Password" }
                    input { id: "login-password", name: "password", r#type: "password", autocomplete: "current-password", required: true }
                    if let Some(message) = message {
                        p { class: "form-error", role: "alert", "{message}" }
                    }
                    button { class: "primary-button", r#type: "submit", "Sign in" }
                }
            }
        }
    }
}

#[cfg(feature = "server")]
pub mod server {
    use super::Viewer;
    use crate::{accounts, database, App};
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use dioxus::server::{
        axum::{
            extract::{DefaultBodyLimit, Request, State},
            http::{header, HeaderMap, HeaderValue, Method, StatusCode},
            middleware::{self, Next},
            response::{IntoResponse, Redirect, Response},
            routing::{get, post},
            Form, Router,
        },
        DioxusRouterExt, ServeConfig,
    };
    use rusqlite::{params, OptionalExtension};
    use serde::Deserialize;
    use sha2::{Digest, Sha256};
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };
    use tokio::sync::Semaphore;
    use url::Url;

    const SESSION_SECONDS: i64 = 7 * 24 * 60 * 60;

    #[derive(Clone)]
    struct AuthState {
        origin: String,
        dev_port: Option<u16>,
        secure: bool,
        dummy_hash: Arc<str>,
        attempts: Arc<Mutex<Attempts>>,
        verification_slots: Arc<Semaphore>,
        calendars: crate::calendar_server::CalendarClients,
        tautulli: crate::tautulli::Tautulli,
        steam: crate::steam::Steam,
    }

    #[derive(Clone, Copy)]
    pub struct SessionHash(pub [u8; 32]);

    struct Attempts {
        since: Instant,
        total: u32,
        by_username: HashMap<String, u32>,
    }

    impl Attempts {
        fn allow(&mut self, username: &str) -> bool {
            if self.since.elapsed() >= Duration::from_secs(60) {
                self.since = Instant::now();
                self.total = 0;
                self.by_username.clear();
            }
            if self.total >= 90 {
                return false;
            }
            let count = self.by_username.entry(username.to_owned()).or_default();
            if *count >= 8 {
                return false;
            }
            *count += 1;
            self.total += 1;
            true
        }
    }

    impl AuthState {
        fn new(origin: &str) -> Result<Self, &'static str> {
            let url = Url::parse(origin)
                .map_err(|_| "Set LAVEMILL_ORIGIN to the browser-facing origin.")?;
            let secure = url.scheme() == "https";
            if url.host().is_none()
                || (!secure
                    && (url.scheme() != "http"
                        || !matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))))
                || !url.username().is_empty()
                || url.password().is_some()
                || url.path() != "/"
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(
                    "LAVEMILL_ORIGIN must be an HTTPS origin or an explicit loopback HTTP origin.",
                );
            }
            let dummy_hash = accounts::hash_password("unused dummy credential")?;
            let origin = url.origin().ascii_serialization();
            Ok(Self {
                calendars: crate::calendar_server::CalendarClients::configured(&origin)?,
                tautulli: crate::tautulli::Tautulli::configured(),
                steam: crate::steam::Steam::configured(),
                origin,
                dev_port: url.port_or_known_default().filter(|_| !secure),
                secure,
                dummy_hash: dummy_hash.into(),
                attempts: Arc::new(Mutex::new(Attempts {
                    since: Instant::now(),
                    total: 0,
                    by_username: HashMap::new(),
                })),
                verification_slots: Arc::new(Semaphore::new(2)),
            })
        }

        fn accepts_origin(&self, headers: &HeaderMap) -> bool {
            let Some(origin) = headers
                .get(header::ORIGIN)
                .and_then(|value| value.to_str().ok())
            else {
                return false;
            };
            if origin == self.origin {
                return true;
            }
            let (Some(port), Some(authority)) = (self.dev_port, origin.strip_prefix("http://"))
            else {
                return false;
            };
            if !headers
                .get(header::HOST)
                .is_some_and(|host| host == authority)
            {
                return false;
            }
            Url::parse(origin).is_ok_and(|url| {
                matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
                    && url.port_or_known_default() == Some(port)
                    && url.origin().ascii_serialization() == origin
            })
        }

        fn cookie_name(&self) -> &'static str {
            if self.secure {
                "__Host-lavemill_session"
            } else {
                "lavemill_dev_session"
            }
        }

        fn cookie(&self, value: &str, max_age: i64) -> HeaderValue {
            let secure = if self.secure { "; Secure" } else { "" };
            HeaderValue::from_str(&format!(
                "{}={value}; Path=/; Max-Age={max_age}; HttpOnly; SameSite=Lax{secure}",
                self.cookie_name()
            ))
            .expect("session cookie only contains safe ASCII")
        }

        fn token(&self, headers: &HeaderMap) -> Option<[u8; 32]> {
            let token = headers
                .get_all(header::COOKIE)
                .iter()
                .filter_map(|value| value.to_str().ok())
                .flat_map(|value| value.split(';'))
                .filter_map(|value| value.trim().split_once('='))
                .find_map(|(name, value)| (name == self.cookie_name()).then_some(value))?;
            if token.len() != 43 {
                return None;
            }
            let mut bytes = [0; 32];
            if URL_SAFE_NO_PAD.decode_slice(token, &mut bytes).ok()? != bytes.len() {
                return None;
            }
            Some(Sha256::digest(bytes).into())
        }
    }

    pub fn router(origin: &str) -> Result<Router, &'static str> {
        let state = AuthState::new(origin)?;
        Ok(Router::new()
            .route("/healthz", get(|| async { StatusCode::NO_CONTENT }))
            .route("/auth/login", post(sign_in))
            .route("/auth/logout", post(sign_out))
            .route(
                "/calendar/{provider}/connect",
                post(crate::calendar_server::connect),
            )
            .route(
                "/calendar/{provider}/callback",
                get(crate::calendar_server::callback),
            )
            .route(
                "/calendar/{provider}/disconnect",
                post(crate::calendar_server::disconnect),
            )
            .route(
                "/calendar/{provider}/select",
                post(crate::calendar_server::select),
            )
            .route(
                "/calendar/{provider}/share",
                post(crate::calendar_server::share),
            )
            .layer(DefaultBodyLimit::max(8 * 1024))
            .with_state(state.clone())
            .serve_dioxus_application(ServeConfig::new(), App)
            .layer(middleware::from_fn_with_state(state, guard)))
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum RequestKind {
        HouseholdPage,
        AdminPage,
        HouseholdApi,
        AdminApi,
        ActivityApi,
        Calendar,
        Session,
        PublicAuth,
        Other,
    }

    impl RequestKind {
        fn classify(path: &str) -> Self {
            match path.trim_end_matches('/') {
                "/users" => return Self::AdminPage,
                "" | "/todos" | "/shopping" | "/calendar" => return Self::HouseholdPage,
                _ => {}
            }
            if path == "/recipes" || path.starts_with("/recipes/") {
                Self::HouseholdPage
            } else if path.starts_with("/api/users") {
                Self::AdminApi
            } else if path == "/api/session" {
                Self::Session
            } else if path == "/api/activity" {
                Self::ActivityApi
            } else if path.starts_with("/calendar/google/")
                || path.starts_with("/calendar/outlook/")
                || path == "/api/calendar"
                || path.starts_with("/api/calendar/")
            {
                Self::Calendar
            } else if ["/api/todos", "/api/shopping", "/api/recipes"]
                .iter()
                .any(|prefix| {
                    path.strip_prefix(prefix)
                        .is_some_and(|suffix| suffix.is_empty() || suffix.starts_with('/'))
                })
            {
                Self::HouseholdApi
            } else if path == "/login" || path.starts_with("/auth/") {
                Self::PublicAuth
            } else {
                Self::Other
            }
        }
    }

    async fn guard(State(state): State<AuthState>, mut request: Request, next: Next) -> Response {
        let kind = RequestKind::classify(request.uri().path());
        let account_request = kind != RequestKind::Other;
        if account_request
            && request.method() != Method::GET
            && request.method() != Method::HEAD
            && !state.accepts_origin(request.headers())
        {
            return private((
                StatusCode::FORBIDDEN,
                format!("This request came from an unexpected address. Open Lavemill at {} and try again.", state.origin),
            ).into_response());
        }

        if account_request && kind != RequestKind::PublicAuth {
            let token = state.token(request.headers());
            let viewer = match token {
                Some(token) => match run_blocking(move || lookup_session(&token)).await {
                    Ok(viewer) => viewer,
                    Err(_) => return private(StatusCode::SERVICE_UNAVAILABLE.into_response()),
                },
                None => None,
            };
            match viewer {
                Some(viewer) => {
                    if matches!(kind, RequestKind::AdminPage | RequestKind::AdminApi)
                        && !viewer.is_admin
                    {
                        return private(StatusCode::FORBIDDEN.into_response());
                    }
                    request.extensions_mut().insert(viewer);
                    if let Some(token) = token {
                        request.extensions_mut().insert(SessionHash(token));
                    }
                    request.extensions_mut().insert(state.calendars.clone());
                    request.extensions_mut().insert(state.tautulli.clone());
                    if kind == RequestKind::ActivityApi {
                        request.extensions_mut().insert(state.steam.clone());
                    }
                }
                None if matches!(kind, RequestKind::HouseholdPage | RequestKind::AdminPage) => {
                    return private(Redirect::to("/login").into_response());
                }
                None if kind != RequestKind::Session => {
                    return private(StatusCode::UNAUTHORIZED.into_response());
                }
                None => {}
            }
        }
        let response = next.run(request).await;
        if account_request {
            private(response)
        } else {
            response
        }
    }

    #[cfg(test)]
    #[test]
    fn classifies_request_paths() {
        use RequestKind::*;
        for (path, expected) in [
            ("/", HouseholdPage),
            ("/todos/", HouseholdPage),
            ("/shopping", HouseholdPage),
            ("/recipes/42/edit", HouseholdPage),
            ("/calendar/", HouseholdPage),
            ("/users/", AdminPage),
            ("/api/users/create", AdminApi),
            ("/api/users-extra", AdminApi),
            ("/api/todos", HouseholdApi),
            ("/api/shopping/change", HouseholdApi),
            ("/api/recipes/42", HouseholdApi),
            ("/api/todos-extra", Other),
            ("/api/calendar/view", Calendar),
            ("/calendar/google/callback", Calendar),
            ("/calendar/outlook/share", Calendar),
            ("/api/activity", ActivityApi),
            ("/api/session", Session),
            ("/api/session/", Other),
            ("/login", PublicAuth),
            ("/auth/logout", PublicAuth),
            ("/healthz", Other),
        ] {
            assert_eq!(RequestKind::classify(path), expected, "{path}");
        }
    }

    fn private(mut response: Response) -> Response {
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        response
    }

    #[derive(Deserialize)]
    struct Credentials {
        username: String,
        password: String,
    }

    async fn sign_in(
        State(state): State<AuthState>,
        headers: HeaderMap,
        Form(credentials): Form<Credentials>,
    ) -> Response {
        let username = accounts::username(&credentials.username).unwrap_or_default();
        if !state.attempts.lock().unwrap().allow(&username) {
            return Redirect::to("/login?error=retry").into_response();
        }
        let Ok(slot) = state.verification_slots.clone().try_acquire_owned() else {
            return Redirect::to("/login?error=retry").into_response();
        };
        let previous = state.token(&headers);
        let dummy_hash = state.dummy_hash.clone();
        let result = run_blocking(move || {
            let _slot = slot;
            issue_session(&username, &credentials.password, &dummy_hash, previous)
        })
        .await;
        match result {
            Ok(Some(token)) => (
                [(header::SET_COOKIE, state.cookie(&token, SESSION_SECONDS))],
                Redirect::to("/"),
            )
                .into_response(),
            Ok(None) => Redirect::to("/login?error=credentials").into_response(),
            Err(_) => Redirect::to("/login?error=unavailable").into_response(),
        }
    }

    async fn sign_out(State(state): State<AuthState>, headers: HeaderMap) -> Response {
        if let Some(token) = state.token(&headers) {
            let result = run_blocking(move || {
                connection()?
                    .execute("DELETE FROM sessions WHERE token_hash = ?1", [&token[..]])
                    .map_err(accounts::database_error)?;
                Ok(())
            })
            .await;
            if result.is_err() {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Could not sign out. Please try again.",
                )
                    .into_response();
            }
        }
        (
            [(header::SET_COOKIE, state.cookie("", 0))],
            Redirect::to("/login"),
        )
            .into_response()
    }

    pub async fn run_blocking<T: Send + 'static>(
        operation: impl FnOnce() -> Result<T, &'static str> + Send + 'static,
    ) -> Result<T, &'static str> {
        tokio::task::spawn_blocking(operation)
            .await
            .map_err(|_| "The account operation could not finish.")?
    }

    pub fn connection() -> Result<rusqlite::Connection, &'static str> {
        database::open(&database::configured_path()).map_err(|error| {
            eprintln!("Unable to open account database: {error}");
            "The account database is unavailable. Try again."
        })
    }

    pub fn now() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
    }

    fn lookup_session(token: &[u8; 32]) -> Result<Option<Viewer>, &'static str> {
        connection()?.query_row(
            "SELECT users.id, first_name, is_admin FROM sessions JOIN users ON users.id = user_id WHERE token_hash = ?1 AND expires_at > ?2",
            params![&token[..], now()],
            |row| Ok(Viewer { id: row.get(0)?, first_name: row.get(1)?, is_admin: row.get(2)? }),
        ).optional().map_err(accounts::database_error)
    }

    fn issue_session(
        username: &str,
        password: &str,
        dummy_hash: &str,
        previous: Option<[u8; 32]>,
    ) -> Result<Option<String>, &'static str> {
        let mut db = connection()?;
        let account: Option<(i64, String)> = db
            .query_row(
                "SELECT id, password_hash FROM users WHERE username = ?1",
                [username],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(accounts::database_error)?;
        let hash = account
            .as_ref()
            .map_or(dummy_hash, |(_, hash)| hash.as_str());
        if !accounts::verify_password(password, hash) {
            return Ok(None);
        }
        let Some((id, hash)) = account else {
            return Ok(None);
        };

        let mut secret = [0; 32];
        getrandom::fill(&mut secret).map_err(|_| "Could not generate a session token.")?;
        let token_hash = Sha256::digest(secret);
        let transaction = db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(accounts::database_error)?;
        let issued_at = now();
        let added = transaction
            .execute(
                "INSERT INTO sessions (token_hash, user_id, expires_at)
             SELECT ?1, id, ?2 FROM users WHERE id = ?3 AND password_hash = ?4 AND username = ?5",
                params![
                    &token_hash[..],
                    issued_at + SESSION_SECONDS,
                    id,
                    hash,
                    username
                ],
            )
            .map_err(accounts::database_error)?;
        if added == 0 {
            return Ok(None);
        }
        transaction
            .execute(
                "DELETE FROM sessions WHERE expires_at <= ?1 OR token_hash = ?2",
                params![issued_at, previous.as_ref().map(|token| &token[..])],
            )
            .map_err(accounts::database_error)?;
        transaction.commit().map_err(accounts::database_error)?;
        Ok(Some(URL_SAFE_NO_PAD.encode(secret)))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn loopback_alias_requires_matching_host_and_port() {
            let state = AuthState::new("http://127.0.0.1:8080").unwrap();
            let mut headers = HeaderMap::new();
            assert!(!state.accepts_origin(&headers));
            headers.insert(header::HOST, HeaderValue::from_static("localhost:8080"));
            headers.insert(
                header::ORIGIN,
                HeaderValue::from_static("http://localhost:8080"),
            );
            assert!(state.accepts_origin(&headers));

            headers.insert(header::HOST, HeaderValue::from_static("127.0.0.1:8080"));
            assert!(!state.accepts_origin(&headers));
            headers.insert(header::HOST, HeaderValue::from_static("localhost:8081"));
            headers.insert(
                header::ORIGIN,
                HeaderValue::from_static("http://localhost:8081"),
            );
            assert!(!state.accepts_origin(&headers));
            headers.insert(header::HOST, HeaderValue::from_static("example.com:8080"));
            headers.insert(
                header::ORIGIN,
                HeaderValue::from_static("http://example.com:8080"),
            );
            assert!(!state.accepts_origin(&headers));
        }

        #[test]
        fn https_only_accepts_its_configured_origin() {
            let state = AuthState::new("https://lavemill.example.com").unwrap();
            let mut headers = HeaderMap::new();
            headers.insert(header::HOST, HeaderValue::from_static("localhost:8080"));
            headers.insert(
                header::ORIGIN,
                HeaderValue::from_static("http://localhost:8080"),
            );
            assert!(!state.accepts_origin(&headers));
            headers.insert(
                header::ORIGIN,
                HeaderValue::from_static("https://example.com"),
            );
            assert!(!state.accepts_origin(&headers));
            headers.insert(
                header::ORIGIN,
                HeaderValue::from_static("https://lavemill.example.com"),
            );
            assert!(state.accepts_origin(&headers));
        }
    }
}
