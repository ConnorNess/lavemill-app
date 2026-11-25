use reqwest::Client;
use serde::Deserialize;
use std::{
    collections::HashMap,
    env,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use url::Url;

const PLAYER_SUMMARIES_URL: &str = "https://api.steampowered.com/ISteamUser/GetPlayerSummaries/v2/";
const CACHE_TTL: Duration = Duration::from_secs(30);
const BATCH_SIZE: usize = 100;

#[derive(Clone)]
pub(crate) struct Steam(Option<Arc<Connection>>);

struct Connection {
    http: Client,
    endpoint: Url,
    key: String,
    cache: Mutex<Option<Snapshot>>,
}

struct Snapshot {
    ids: Vec<i64>,
    fetched_at: Instant,
    games: HashMap<i64, String>,
}

#[derive(Deserialize)]
struct PlayerSummaries {
    response: Players,
}

#[derive(Deserialize)]
struct Players {
    players: Vec<Player>,
}

#[derive(Deserialize)]
struct Player {
    steamid: String,
    gameextrainfo: Option<String>,
}

impl Steam {
    pub(crate) fn configured() -> Self {
        let key = env::var("LAVEMILL_STEAM_API_KEY").unwrap_or_default();
        let key = key.trim();
        if key.is_empty() {
            return Self(None);
        }
        let endpoint = Url::parse(PLAYER_SUMMARIES_URL).expect("valid Steam Web API URL");
        let connection = Connection::new(endpoint, key);
        if connection.is_err() {
            eprintln!("Could not initialize the Steam activity client.");
        }
        Self(connection.ok().map(Arc::new))
    }

    pub(crate) async fn activity(&self, mut ids: Vec<i64>) -> HashMap<i64, String> {
        let Some(connection) = &self.0 else {
            return HashMap::new();
        };
        ids.sort_unstable();
        ids.dedup();

        // Hold the lock through refresh so concurrent viewers share the same lookup.
        let mut cache = connection.cache.lock().await;
        if ids.is_empty() {
            *cache = None;
            return HashMap::new();
        }
        if let Some(snapshot) = cache.as_ref() {
            if snapshot.ids == ids && snapshot.fetched_at.elapsed() < CACHE_TTL {
                return snapshot.games.clone();
            }
        }

        let mut games = HashMap::new();
        for batch in ids.chunks(BATCH_SIZE) {
            if let Ok(summaries) = connection.fetch(batch).await {
                games.extend(summaries.into_games(batch));
            }
        }
        *cache = Some(Snapshot {
            ids,
            fetched_at: Instant::now(),
            games: games.clone(),
        });
        games
    }
}

impl Connection {
    fn new(endpoint: Url, key: &str) -> Result<Self, reqwest::Error> {
        let http = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(3))
            .build()?;
        Ok(Self {
            http,
            endpoint,
            key: key.to_owned(),
            cache: Mutex::new(None),
        })
    }

    async fn fetch(&self, batch: &[i64]) -> Result<PlayerSummaries, reqwest::Error> {
        let ids = batch
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(",");
        self.http
            .get(self.endpoint.clone())
            .query(&[("key", self.key.as_str()), ("steamids", &ids)])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
    }
}

impl PlayerSummaries {
    fn into_games(self, requested_ids: &[i64]) -> impl Iterator<Item = (i64, String)> + '_ {
        self.response.players.into_iter().filter_map(move |player| {
            let id = player.steamid.parse().ok()?;
            let title = player.gameextrainfo?;
            let title = title.trim();
            (requested_ids.contains(&id) && !title.is_empty()).then(|| (id, title.to_owned()))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dioxus::server::axum::{http::StatusCode, routing::get, Router};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn shares_a_snapshot_and_refreshes_when_links_change_or_it_expires() {
        const PLAYING: i64 = 76561198000000001;
        const HIDDEN: i64 = 76561198000000002;
        const BLANK: i64 = 76561198000000003;
        const UNREQUESTED: i64 = 76561198000000004;
        const PLAYERS: &str = r#"{"response":{"players":[
            {"steamid":"76561198000000001","gameextrainfo":" Hades II "},
            {"steamid":"76561198000000002"},
            {"steamid":"76561198000000003","gameextrainfo":" "},
            {"steamid":"76561198000000004","gameextrainfo":"Other Game"}
        ]}}"#;

        let calls = Arc::new(AtomicUsize::new(0));
        let request_count = calls.clone();
        let router = Router::new().route(
            "/",
            get(move || async move {
                let request = request_count.fetch_add(1, Ordering::Relaxed);
                if request >= 2 {
                    (StatusCode::SERVICE_UNAVAILABLE, "")
                } else {
                    (StatusCode::OK, PLAYERS)
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let server =
            tokio::spawn(async move { dioxus::server::axum::serve(listener, router).await });
        let connection = Arc::new(Connection::new(endpoint, "test-key").unwrap());
        let steam = Steam(Some(connection.clone()));

        assert!(steam.activity(Vec::new()).await.is_empty());
        let (first, second) = tokio::join!(
            steam.activity(vec![PLAYING, HIDDEN, BLANK]),
            steam.activity(vec![BLANK, PLAYING, HIDDEN]),
        );
        assert_eq!(first, HashMap::from([(PLAYING, "Hades II".into())]));
        assert_eq!(second, first);
        assert!(!first.contains_key(&UNREQUESTED));
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert!(steam.activity(vec![HIDDEN]).await.is_empty());
        assert_eq!(calls.load(Ordering::Relaxed), 2);

        connection.cache.lock().await.as_mut().unwrap().fetched_at -= CACHE_TTL;
        assert!(steam.activity(vec![HIDDEN]).await.is_empty());
        assert!(steam.activity(vec![HIDDEN]).await.is_empty());
        assert_eq!(calls.load(Ordering::Relaxed), 3);
        server.abort();
    }
}
