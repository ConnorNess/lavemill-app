use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
struct ActivitySnapshot {
    configured: bool,
    rows: Vec<ActivityRow>,
}

#[derive(Deserialize, Serialize)]
struct ActivityRow {
    name: String,
    status: String,
}

#[get("/api/activity")]
async fn user_activity() -> Result<ActivitySnapshot, ServerFnError> {
    let context = dioxus::fullstack::FullstackContext::current()
        .ok_or_else(|| ServerFnError::new("Sign in to view activity."))?;
    context
        .extension::<crate::auth::Viewer>()
        .ok_or_else(|| ServerFnError::new("Sign in to view activity."))?;
    let tautulli: crate::tautulli::Tautulli = context
        .extension()
        .ok_or_else(|| ServerFnError::new("Activity unavailable."))?;
    let members = crate::auth::server::run_blocking(load_members)
        .await
        .map_err(ServerFnError::new)?;
    let configured = tautulli.is_configured();
    let activity = if configured && members.iter().any(|member| member.profile_id.is_some()) {
        tautulli.activity().await.ok()
    } else {
        None
    };
    let sessions = activity
        .as_ref()
        .map(|activity| activity.sessions.as_slice());

    let rows = members
        .iter()
        .map(|member| ActivityRow {
            name: if members
                .iter()
                .filter(|other| member.first.eq_ignore_ascii_case(&other.first))
                .count()
                > 1
            {
                format!("{} {}", member.first, member.last)
            } else {
                member.first.clone()
            },
            status: status(member.profile_id, sessions),
        })
        .collect();
    Ok(ActivitySnapshot { configured, rows })
}

#[cfg(feature = "server")]
struct Member {
    first: String,
    last: String,
    profile_id: Option<i64>,
}

#[cfg(feature = "server")]
fn load_members() -> Result<Vec<Member>, &'static str> {
    let db = crate::auth::server::connection()?;
    let mut query = db
        .prepare(
            "SELECT first_name, last_name, external_profile_id
             FROM users LEFT JOIN activity_profile_links
               ON activity_profile_links.user_id = users.id AND provider = 'plex'
             ORDER BY first_name COLLATE NOCASE, last_name COLLATE NOCASE, users.id",
        )
        .map_err(crate::accounts::database_error)?;
    let rows = query
        .query_map([], |row| {
            Ok(Member {
                first: row.get(0)?,
                last: row.get(1)?,
                profile_id: row.get(2)?,
            })
        })
        .map_err(crate::accounts::database_error)?;
    rows.collect::<Result<_, _>>()
        .map_err(crate::accounts::database_error)
}

#[cfg(feature = "server")]
fn status(profile_id: Option<i64>, sessions: Option<&[crate::tautulli::Playback]>) -> String {
    use crate::tautulli::PlaybackState;

    let Some(profile_id) = profile_id else {
        return "Plex not linked".into();
    };
    let Some(sessions) = sessions else {
        return "Activity unavailable".into();
    };
    let current = sessions
        .iter()
        .filter(|session| session.user_id == profile_id && session.state != PlaybackState::Other)
        .max_by_key(|session| {
            (
                session.state,
                std::cmp::Reverse(session.session_id.as_str()),
            )
        });
    match current {
        Some(session) => {
            let title = &session.full_title;
            match session.state {
                PlaybackState::Playing => format!("Watching {title}"),
                PlaybackState::Paused => format!("Paused · {title}"),
                PlaybackState::Buffering => format!("Buffering · {title}"),
                PlaybackState::Other => unreachable!("Unsupported states were filtered out"),
            }
        }
        None => "Not watching".into(),
    }
}

#[component]
pub(super) fn ActivityCard() -> Element {
    let snapshot = use_signal(|| None::<Result<ActivitySnapshot, ServerFnError>>);
    #[cfg(target_arch = "wasm32")]
    use_future(move || {
        let mut snapshot = snapshot;
        async move {
            loop {
                snapshot.set(Some(user_activity().await));
                gloo_timers::future::TimeoutFuture::new(30_000).await;
            }
        }
    });
    let snapshot = snapshot.read();

    rsx! {
        section { class: "dashboard-card activity-card", aria_labelledby: "user-activity-heading",
            div { class: "card-heading",
                h2 { class: "card-heading-copy", id: "user-activity-heading", "User Activity" }
            }
            div { class: "card-body",
                match snapshot.as_ref() {
                    Some(Ok(snapshot)) => rsx! {
                        if !snapshot.configured {
                            p { class: "activity-message", "Plex activity is not configured." }
                        }
                        ul { class: "activity-list",
                            for (index, row) in snapshot.rows.iter().enumerate() {
                                li { key: "{index}",
                                    strong { "{row.name}" }
                                    span { "{row.status}" }
                                }
                            }
                        }
                    },
                    Some(Err(_)) => rsx! { p { class: "activity-message", "Activity unavailable." } },
                    None => rsx! { p { class: "activity-message", "Loading activity…" } },
                }
            }
        }
    }
}

#[cfg(all(test, feature = "server"))]
mod tests {
    use super::status;
    use crate::tautulli::{Playback, PlaybackState::*};

    #[test]
    fn selects_the_current_session_for_each_profile() {
        let playback = |user_id, session_id: &str, full_title: &str, state| Playback {
            user_id,
            session_id: session_id.into(),
            full_title: full_title.into(),
            state,
        };
        let sessions = [
            playback(101, "1", "Paused Movie", Paused),
            playback(101, "2", "Buffering Movie", Buffering),
            playback(101, "4", "Other Movie", Playing),
            playback(101, "3", "The Odyssey", Playing),
            playback(102, "5", "Episode", Paused),
            playback(103, "6", "Live TV", Buffering),
        ];
        assert_eq!(status(Some(101), Some(&sessions)), "Watching The Odyssey");
        assert_eq!(status(Some(102), Some(&sessions)), "Paused · Episode");
        assert_eq!(status(Some(103), Some(&sessions)), "Buffering · Live TV");
        assert_eq!(status(Some(104), Some(&sessions)), "Not watching");
        assert_eq!(status(None, Some(&sessions)), "Plex not linked");
    }

    #[test]
    fn distinguishes_empty_activity_from_unavailable_activity() {
        assert_eq!(status(Some(101), Some(&[])), "Not watching");
        assert_eq!(status(Some(101), None), "Activity unavailable");
        assert_eq!(status(None, None), "Plex not linked");
    }
}
