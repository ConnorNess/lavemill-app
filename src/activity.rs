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
    let tautulli: crate::tautulli::Tautulli = context
        .extension()
        .ok_or_else(|| ServerFnError::new("Activity unavailable."))?;
    let steam: crate::steam::Steam = context
        .extension()
        .ok_or_else(|| ServerFnError::new("Activity unavailable."))?;
    let members = crate::auth::server::run_blocking(load_members)
        .await
        .map_err(ServerFnError::new)?;
    let steam_ids = members
        .iter()
        .filter_map(|member| member.steam_id)
        .collect();
    let (activity, games) = tokio::join!(
        async {
            if tautulli.is_configured() && members.iter().any(|member| member.profile_id.is_some())
            {
                tautulli.activity().await.ok()
            } else {
                None
            }
        },
        steam.activity(steam_ids),
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
        .filter_map(|member| {
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
            if activities.is_empty() {
                return None;
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
            Some(ActivityRow {
                member_id: member.id,
                name,
                activities,
            })
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
    let snapshot = use_signal(|| None::<ActivitySnapshot>);
    #[cfg(target_arch = "wasm32")]
    use_future(move || {
        let mut snapshot = snapshot;
        async move {
            loop {
                snapshot.set(user_activity().await.ok());
                gloo_timers::future::TimeoutFuture::new(30_000).await;
            }
        }
    });
    let snapshot = snapshot.read();
    let Some(snapshot) = snapshot
        .as_ref()
        .filter(|snapshot| !snapshot.rows.is_empty())
    else {
        return rsx! {};
    };

    rsx! {
        section { class: "activity-section", aria_labelledby: "user-activity-heading",
            div { class: "activity-heading",
                h2 { id: "user-activity-heading", "Activity" }
                span { class: "activity-live-count", "{snapshot.rows.len()} active" }
            }
            ul { class: "activity-list", tabindex: "0", aria_labelledby: "user-activity-heading",
                for row in &snapshot.rows {
                    li { class: "activity-person", key: "{row.member_id}",
                        strong { class: "activity-person-name", "{row.name}" }
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
    fn omits_members_without_reported_activity() {
        let mut members = [Member {
            id: 1,
            first: "Household".into(),
            last: "Member".into(),
            profile_id: Some(101),
            steam_id: Some(76561198000000001),
        }];
        let mut sessions = [Playback {
            user_id: 101,
            session_id: "1".into(),
            full_title: "The Odyssey".into(),
            state: Other,
        }];
        let games = HashMap::new();
        assert!(activity_rows(&members, Some(&[]), &games).is_empty());
        assert!(activity_rows(&members, Some(&sessions), &games).is_empty());
        assert!(activity_rows(&members, None, &games).is_empty());

        sessions[0].state = Playing;
        members[0].profile_id = None;
        members[0].steam_id = None;
        assert!(activity_rows(&members, Some(&sessions), &games).is_empty());
    }
}
