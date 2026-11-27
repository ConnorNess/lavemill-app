use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

const STEAM_ICON: Asset = asset!("/assets/steam.svg");

#[derive(Deserialize, Serialize)]
struct ActivitySnapshot {
    rows: Vec<ActivityRow>,
}

#[derive(Deserialize, Serialize)]
struct ActivityRow {
    member_id: i64,
    name: String,
    activities: Vec<ActivityItem>,
}

#[derive(Deserialize, Serialize)]
struct ActivityItem {
    provider: String,
    status: String,
}

#[get("/api/activity")]
async fn user_activity() -> Result<ActivitySnapshot, ServerFnError> {
    let context = dioxus::fullstack::FullstackContext::current()
        .ok_or_else(|| ServerFnError::new("Sign in to view activity."))?;
    context
        .extension::<crate::auth::Viewer>()
        .ok_or_else(|| ServerFnError::new("Sign in to view activity."))?;
    let tautulli = context.extension::<crate::tautulli::Tautulli>();
    let steam = context.extension::<crate::steam::Steam>();
    let members = crate::auth::server::run_blocking(load_members)
        .await
        .map_err(ServerFnError::new)?;
    let steam_ids = members
        .iter()
        .filter_map(|member| member.steam_id)
        .collect();
    let (activity, games) = tokio::join!(
        async {
            if let Some(tautulli) = tautulli.filter(|client| {
                client.is_configured() && members.iter().any(|member| member.profile_id.is_some())
            }) {
                tautulli.activity().await.ok()
            } else {
                None
            }
        },
        async {
            match steam {
                Some(steam) => steam.activity(steam_ids).await,
                None => Default::default(),
            }
        },
    );
    let sessions = activity
        .as_ref()
        .map(|activity| activity.sessions.as_slice());

    let rows = activity_rows(&members, sessions, &games);
    Ok(ActivitySnapshot { rows })
}

#[cfg(feature = "server")]
fn activity_rows(
    members: &[Member],
    sessions: Option<&[crate::tautulli::Playback]>,
    games: &std::collections::HashMap<i64, String>,
) -> Vec<ActivityRow> {
    members
        .iter()
        .map(|member| {
            let mut activities = Vec::with_capacity(3);
            if let Some(status) = plex_status(member.profile_id, sessions) {
                activities.push(ActivityItem {
                    provider: "Plex".into(),
                    status,
                });
            }
            if let Some(game) = member.steam_id.and_then(|id| games.get(&id).cloned()) {
                activities.push(ActivityItem {
                    provider: "Steam".into(),
                    status: format!("Playing {game}"),
                });
            }
            let name = if members
                .iter()
                .filter(|other| member.first.eq_ignore_ascii_case(&other.first))
                .count()
                > 1
            {
                format!("{} {}", member.first, member.last)
            } else {
                member.first.clone()
            };
            ActivityRow {
                member_id: member.id,
                name,
                activities,
            }
        })
        .collect()
}

#[cfg(feature = "server")]
struct Member {
    id: i64,
    first: String,
    last: String,
    profile_id: Option<i64>,
    steam_id: Option<i64>,
}

#[cfg(feature = "server")]
fn load_members() -> Result<Vec<Member>, &'static str> {
    let db = crate::auth::server::connection()?;
    let mut query = db
        .prepare(
            "SELECT first_name, last_name, plex.external_profile_id, steam.external_profile_id, users.id
             FROM users
             LEFT JOIN activity_profile_links AS plex
               ON plex.user_id = users.id AND plex.provider = 'plex'
             LEFT JOIN activity_profile_links AS steam
               ON steam.user_id = users.id AND steam.provider = 'steam'
             ORDER BY first_name COLLATE NOCASE, last_name COLLATE NOCASE, users.id",
        )
        .map_err(crate::accounts::database_error)?;
    let rows = query
        .query_map([], |row| {
            Ok(Member {
                id: row.get(4)?,
                first: row.get(0)?,
                last: row.get(1)?,
                profile_id: row.get(2)?,
                steam_id: row.get(3)?,
            })
        })
        .map_err(crate::accounts::database_error)?;
    rows.collect::<Result<_, _>>()
        .map_err(crate::accounts::database_error)
}

#[cfg(feature = "server")]
fn plex_status(
    profile_id: Option<i64>,
    sessions: Option<&[crate::tautulli::Playback]>,
) -> Option<String> {
    use crate::tautulli::PlaybackState;

    let profile_id = profile_id?;
    let session = sessions?
        .iter()
        .filter(|session| session.user_id == profile_id && session.state != PlaybackState::Other)
        .max_by_key(|session| {
            (
                session.state,
                std::cmp::Reverse(session.session_id.as_str()),
            )
        })?;
    let title = &session.full_title;
    Some(match session.state {
        PlaybackState::Playing => format!("Watching {title}"),
        PlaybackState::Paused => format!("Paused · {title}"),
        PlaybackState::Buffering => format!("Buffering · {title}"),
        PlaybackState::Other => unreachable!("Unsupported states were filtered out"),
    })
}

#[component]
pub(super) fn ActivitySection() -> Element {
    let initial = use_server_future(user_activity)?;
    let refreshed = use_signal(|| None::<ActivitySnapshot>);
    let refresh_failed = use_signal(|| false);
    #[cfg(target_arch = "wasm32")]
    use_future(move || {
        let mut refreshed = refreshed;
        let mut refresh_failed = refresh_failed;
        async move {
            loop {
                gloo_timers::future::TimeoutFuture::new(30_000).await;
                match user_activity().await {
                    Ok(snapshot) => {
                        refreshed.set(Some(snapshot));
                        refresh_failed.set(false);
                    }
                    Err(_) => refresh_failed.set(true),
                }
            }
        }
    });
    let initial = initial.read();
    let refreshed = refreshed.read();
    let Some(snapshot) = refreshed
        .as_ref()
        .or_else(|| initial.as_ref()?.as_ref().ok())
    else {
        return rsx! {
            section { class: "activity-section", aria_labelledby: "user-activity-heading",
                h2 { id: "user-activity-heading", class: "sr-only", "Activity" }
                p { class: "activity-placeholder", role: "status",
                    if initial.is_some() {
                        span { class: "activity-empty", "NO ACTIVITY" }
                    } else {
                        "Loading activity…"
                    }
                }
            }
        };
    };
    rsx! {
        section { class: "activity-section", aria_labelledby: "user-activity-heading",
            h2 { id: "user-activity-heading", class: "sr-only", "Activity" }
            ul { class: "activity-list", tabindex: "0", aria_labelledby: "user-activity-heading",
                for row in &snapshot.rows {
                    li { class: "activity-person", key: "{row.member_id}",
                        strong { class: "activity-person-name", "{row.name}" }
                        if refresh_failed() || row.activities.is_empty() {
                            span { class: "activity-empty", "NO ACTIVITY" }
                        } else {
                            ul { class: "activity-stream", aria_label: "{row.name} activity",
                                for activity in &row.activities {
                                    li { class: "activity-item",
                                        if activity.provider == "Plex" {
                                            span { class: "activity-provider activity-provider-plex",
                                                crate::Icon { kind: "Plex" }
                                                "Plex"
                                            }
                                        }
                                        if activity.provider == "Steam" {
                                            span { class: "activity-provider activity-provider-steam",
                                                img { src: STEAM_ICON, alt: "" }
                                                "Steam"
                                            }
                                        }
                                        if activity.provider != "Plex" && activity.provider != "Steam" {
                                            span { class: "activity-provider activity-provider-other",
                                                span { class: "activity-provider-initial", aria_hidden: "true",
                                                    "{activity.provider.chars().next().unwrap_or(' ')}"
                                                }
                                                "{activity.provider}"
                                            }
                                        }
                                        span { class: "activity-description", "{activity.status}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(all(test, feature = "server"))]
mod tests {
    use super::{activity_rows, plex_status, Member};
    use crate::tautulli::{Playback, PlaybackState::*};
    use std::collections::HashMap;

    #[test]
    fn keeps_member_identity_when_names_match_and_order_changes() {
        let member = |id| Member {
            id,
            first: "Same".into(),
            last: "Name".into(),
            profile_id: None,
            steam_id: Some(id),
        };
        let mut members = [member(1), member(2)];
        let games = HashMap::from([(1, "Game One".into()), (2, "Game Two".into())]);
        let before = activity_rows(&members, None, &games);
        members.reverse();
        let after = activity_rows(&members, None, &games);
        assert_eq!(before[0].name, before[1].name);
        assert_ne!(before[0].member_id, before[1].member_id);
        assert_eq!(before[0].member_id, after[1].member_id);
        assert_eq!(before[1].member_id, after[0].member_id);
    }

    #[test]
    fn pairs_steam_and_plex_activity_and_keeps_either_when_the_other_is_unavailable() {
        let steam_id = 76561198000000001;
        let members = [Member {
            id: 1,
            first: "Household".into(),
            last: "Member".into(),
            profile_id: Some(101),
            steam_id: Some(steam_id),
        }];
        let sessions = [Playback {
            user_id: 101,
            session_id: "1".into(),
            full_title: "The Odyssey".into(),
            state: Playing,
        }];
        let games = HashMap::from([(steam_id, "Hades II".into())]);
        let both = activity_rows(&members, Some(&sessions), &games);
        assert_eq!(both[0].activities.len(), 2);
        assert_eq!(both[0].activities[0].status, "Watching The Odyssey");
        assert_eq!(both[0].activities[1].status, "Playing Hades II");

        let plex_only = activity_rows(&members, Some(&sessions), &HashMap::new());
        assert_eq!(plex_only[0].activities.len(), 1);
        assert_eq!(plex_only[0].activities[0].status, "Watching The Odyssey");

        let steam_only = activity_rows(&members, None, &games);
        assert_eq!(steam_only[0].activities.len(), 1);
        assert_eq!(steam_only[0].activities[0].status, "Playing Hades II");
    }

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
        assert_eq!(
            plex_status(Some(101), Some(&sessions)).as_deref(),
            Some("Watching The Odyssey")
        );
        assert_eq!(
            plex_status(Some(102), Some(&sessions)).as_deref(),
            Some("Paused · Episode")
        );
        assert_eq!(
            plex_status(Some(103), Some(&sessions)).as_deref(),
            Some("Buffering · Live TV")
        );
        assert_eq!(plex_status(Some(104), Some(&sessions)), None);
    }

    #[test]
    fn lists_idle_members_with_and_without_linked_profiles() {
        let members = [1, 2, 3, 4].map(|id| Member {
            id,
            first: format!("Member {id}"),
            last: "Household".into(),
            profile_id: (id == 1).then_some(101),
            steam_id: (id == 1).then_some(76561198000000001),
        });
        let mut sessions = [Playback {
            user_id: 101,
            session_id: "1".into(),
            full_title: "The Odyssey".into(),
            state: Other,
        }];
        let games = HashMap::new();
        for inactive_sessions in [None, Some(&sessions[..])] {
            let rows = activity_rows(&members, inactive_sessions, &games);
            assert_eq!(rows.len(), 4);
            assert!(rows.iter().all(|row| row.activities.is_empty()));
        }

        sessions[0].state = Playing;
        let rows = activity_rows(&members, Some(&sessions), &games);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].activities[0].status, "Watching The Odyssey");
        assert!(rows[1..].iter().all(|row| row.activities.is_empty()));
    }
}
