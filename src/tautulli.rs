use reqwest::{header::HeaderValue, Client};
use serde::{de::DeserializeOwned, Deserialize, Deserializer};
use std::{env, time::Duration};
use url::Url;

#[derive(Clone)]
pub(crate) struct Tautulli(Result<Option<Connection>, &'static str>);

#[derive(Clone)]
struct Connection {
    http: Client,
    endpoint: Url,
    key: HeaderValue,
}

#[derive(Deserialize)]
struct Envelope<T> {
    response: ApiResponse<T>,
}

#[derive(Deserialize)]
struct ApiResponse<T> {
    result: String,
    data: T,
}

#[derive(Deserialize)]
pub(crate) struct Profile {
    #[serde(deserialize_with = "profile_id")]
    pub user_id: i64,
    pub friendly_name: String,
}

#[derive(Deserialize)]
pub(crate) struct Activity {
    pub sessions: Vec<Playback>,
}

#[derive(Deserialize)]
pub(crate) struct Playback {
    #[serde(deserialize_with = "profile_id")]
    pub user_id: i64,
    pub session_id: String,
    pub full_title: String,
    pub state: PlaybackState,
}

#[derive(Clone, Copy, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PlaybackState {
    Paused,
    Buffering,
    Playing,
    #[serde(other)]
    Other,
}

fn profile_id<'de, D: Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Id {
        Number(i64),
        Text(String),
    }
    match Id::deserialize(deserializer)? {
        Id::Number(id) => Ok(id),
        Id::Text(id) => id.parse().map_err(serde::de::Error::custom),
    }
}

impl Tautulli {
    pub(crate) fn configured() -> Self {
        let url = env::var("LAVEMILL_TAUTULLI_URL").unwrap_or_default();
        let key = env::var("LAVEMILL_TAUTULLI_API_KEY").unwrap_or_default();
        if url.trim().is_empty() && key.trim().is_empty() {
            return Self(Ok(None));
        }
        Self(Connection::new(url.trim(), key.trim()).map(Some))
    }

    pub(crate) async fn profiles(&self) -> Result<Option<Vec<Profile>>, &'static str> {
        let Some(connection) = self.0.as_ref().map_err(|error| *error)? else {
            return Ok(None);
        };
        let mut profiles: Vec<Profile> = connection.get("get_user_names").await?;
        profiles.retain(|profile| profile.user_id > 0);
        Ok(Some(profiles))
    }

    pub(crate) fn is_configured(&self) -> bool {
        !matches!(&self.0, Ok(None))
    }

    pub(crate) async fn activity(&self) -> Result<Activity, &'static str> {
        let connection = self
            .0
            .as_ref()
            .map_err(|error| *error)?
            .as_ref()
            .ok_or("Tautulli is not configured.")?;
        connection.get("get_activity").await
    }
}

impl Connection {
    fn new(url: &str, key: &str) -> Result<Self, &'static str> {
        const INVALID: &str = "Check the Tautulli URL and API key configuration.";
        let mut endpoint = Url::parse(url).map_err(|_| INVALID)?;
        if !matches!(endpoint.scheme(), "http" | "https")
            || endpoint.host().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || key.is_empty()
        {
            return Err(INVALID);
        }
        endpoint
            .path_segments_mut()
            .map_err(|_| INVALID)?
            .pop_if_empty()
            .extend(["api", "v2"]);
        let mut key = HeaderValue::from_str(key).map_err(|_| INVALID)?;
        key.set_sensitive(true);
        let http = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(3))
            .build()
            .map_err(|_| INVALID)?;
        Ok(Self {
            http,
            endpoint,
            key,
        })
    }

    async fn get<T: DeserializeOwned>(&self, command: &str) -> Result<T, &'static str> {
        const UNAVAILABLE: &str = "Tautulli is unavailable. Try again.";
        let mut endpoint = self.endpoint.clone();
        endpoint.query_pairs_mut().append_pair("cmd", command);
        let envelope: Envelope<T> = self
            .http
            .get(endpoint)
            .header("X-Api-Key", self.key.clone())
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|_| UNAVAILABLE)?
            .json()
            .await
            .map_err(|_| UNAVAILABLE)?;
        if envelope.response.result != "success" {
            return Err(UNAVAILABLE);
        }
        Ok(envelope.response.data)
    }
}
