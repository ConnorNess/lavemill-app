use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct User {
    id: i64,
    username: String,
    first_name: String,
    last_name: String,
    is_admin: bool,
    plex_profile_id: Option<i64>,
}

#[get("/api/users")]
async fn list_users() -> Result<Vec<User>, ServerFnError> {
    require_admin()?;
    crate::auth::server::run_blocking(|| {
        let db = crate::auth::server::connection()?;
        let mut query = db
            .prepare(
                "SELECT users.id, username, first_name, last_name, is_admin, external_profile_id
                 FROM users LEFT JOIN activity_profile_links
                   ON activity_profile_links.user_id = users.id AND provider = 'plex'
                 ORDER BY is_admin DESC, username",
            )
            .map_err(crate::accounts::database_error)?;
        let rows = query
            .query_map([], |row| {
                Ok(User {
                    id: row.get(0)?,
                    username: row.get(1)?,
                    first_name: row.get(2)?,
                    last_name: row.get(3)?,
                    is_admin: row.get(4)?,
                    plex_profile_id: row.get(5)?,
                })
            })
            .map_err(crate::accounts::database_error)?;
        rows.collect::<Result<_, _>>()
            .map_err(crate::accounts::database_error)
    })
    .await
    .map_err(ServerFnError::new)
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct PlexProfile {
    id: i64,
    name: String,
}

#[get("/api/users/plex-profiles")]
async fn plex_profiles() -> Result<Option<Vec<PlexProfile>>, ServerFnError> {
    require_admin()?;
    let profiles = tautulli()?.profiles().await.map_err(ServerFnError::new)?;
    Ok(profiles.map(|profiles| {
        let mut profiles: Vec<_> = profiles
            .into_iter()
            .map(|profile| PlexProfile {
                id: profile.user_id,
                name: profile.friendly_name,
            })
            .collect();
        profiles.sort_by_cached_key(|profile| (profile.name.to_lowercase(), profile.id));
        profiles
    }))
}

#[post("/api/users/plex-profile")]
async fn save_plex_profile(
    id: i64,
    profile_id: Option<i64>,
) -> Result<Option<String>, ServerFnError> {
    require_admin()?;
    if let Some(profile_id) = profile_id {
        match tautulli()?.profiles().await {
            Ok(Some(profiles)) if profiles.iter().any(|profile| profile.user_id == profile_id) => {}
            Ok(Some(_)) => return Ok(Some("That Plex profile is no longer available.".into())),
            Ok(None) => return Ok(Some("Tautulli is not configured.".into())),
            Err(error) => return Ok(Some(error.into())),
        }
    }
    let result = crate::auth::server::run_blocking(move || {
        write_plex_profile(&crate::auth::server::connection()?, id, profile_id)
    })
    .await;
    Ok(result.err().map(str::to_owned))
}

#[cfg(feature = "server")]
fn tautulli() -> Result<crate::tautulli::Tautulli, ServerFnError> {
    dioxus::fullstack::FullstackContext::current()
        .and_then(|context| context.extension())
        .ok_or_else(|| ServerFnError::new("Tautulli configuration is unavailable."))
}

#[cfg(feature = "server")]
fn write_plex_profile(
    db: &rusqlite::Connection,
    id: i64,
    profile_id: Option<i64>,
) -> Result<(), &'static str> {
    if let Some(profile_id) = profile_id {
        db.execute(
            "INSERT INTO activity_profile_links (user_id, provider, external_profile_id)
             VALUES (?1, 'plex', ?2)
             ON CONFLICT (user_id, provider)
             DO UPDATE SET external_profile_id = excluded.external_profile_id",
            rusqlite::params![id, profile_id],
        )
        .map_err(|error| match error {
            rusqlite::Error::SqliteFailure(code, _)
                if code.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE =>
            {
                "That Plex profile is already linked to another user."
            }
            rusqlite::Error::SqliteFailure(code, _)
                if code.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY =>
            {
                "User not found."
            }
            _ => crate::accounts::database_error(error),
        })?;
    } else {
        db.execute(
            "DELETE FROM activity_profile_links WHERE user_id = ?1 AND provider = 'plex'",
            [id],
        )
        .map_err(crate::accounts::database_error)?;
    }
    Ok(())
}

#[post("/api/users/create")]
async fn create_user(
    username: String,
    first: String,
    last: String,
    password: String,
) -> Result<Option<String>, ServerFnError> {
    require_admin()?;
    let result = crate::auth::server::run_blocking(move || {
        let (username, first, last) = crate::accounts::details(&username, &first, &last)?;
        let hash = crate::accounts::hash_password(&password)?;
        crate::auth::server::connection()?.execute(
            "INSERT INTO users (username, first_name, last_name, password_hash) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![username, first, last, hash],
        ).map_err(crate::accounts::save_error)?;
        Ok(())
    }).await;
    Ok(result.err().map(str::to_owned))
}

#[post("/api/users/edit")]
async fn edit_user(
    id: i64,
    username: String,
    first: String,
    last: String,
) -> Result<Option<String>, ServerFnError> {
    require_admin()?;
    let result = crate::auth::server::run_blocking(move || {
        let (username, first, last) = crate::accounts::details(&username, &first, &last)?;
        let changed = crate::auth::server::connection()?
            .execute(
                "UPDATE users SET username = ?1, first_name = ?2, last_name = ?3 WHERE id = ?4",
                rusqlite::params![username, first, last, id],
            )
            .map_err(crate::accounts::save_error)?;
        if changed == 0 {
            return Err("User not found.");
        }
        Ok(())
    })
    .await;
    Ok(result.err().map(str::to_owned))
}

#[post("/api/users/reset-password")]
async fn reset_password(id: i64, password: String) -> Result<Option<String>, ServerFnError> {
    require_admin()?;
    let result = crate::auth::server::run_blocking(move || {
        let hash = crate::accounts::hash_password(&password)?;
        crate::accounts::replace_password(&mut crate::auth::server::connection()?, id, &hash)
    })
    .await;
    Ok(result.err().map(str::to_owned))
}

#[cfg(feature = "server")]
fn require_admin() -> Result<(), ServerFnError> {
    let viewer: Option<crate::auth::Viewer> =
        dioxus::fullstack::FullstackContext::current().and_then(|context| context.extension());
    if viewer.is_some_and(|viewer| viewer.is_admin) {
        return Ok(());
    }
    Err(ServerFnError::new("Administrator access required."))
}

#[component]
pub fn Users() -> Element {
    let mut users = use_server_future(list_users)?;
    let profiles = use_resource(plex_profiles);
    let result = users.read();
    rsx! {
        main { id: "main", class: "dashboard users-page", tabindex: "-1",
            document::Title { "Lavemill · Users" }
            h1 { "Users" }
            match result.as_ref() {
                Some(Ok(accounts)) => rsx! {
                    div { class: "users-grid",
                        section { class: "add-task-panel",
                            h2 { "Add user" }
                            UserDetails { on_saved: move |_| users.restart() }
                        }
                        div { class: "user-list",
                            for user in accounts {
                                section { key: "{user.id}", class: "add-task-panel",
                                    h2 {
                                        if user.is_admin { "Your admin account" }
                                        else { "Account details" }
                                    }
                                    UserDetails { user: user.clone(), on_saved: |_| {} }
                                    PlexProfileForm { id: user.id, initial: user.plex_profile_id, profiles }
                                    PasswordReset { id: user.id, is_admin: user.is_admin }
                                }
                            }
                        }
                    }
                },
                Some(Err(_)) => rsx! { p { class: "form-error", role: "alert", "Could not load users. Check your session and reload." } },
                None => rsx! { p { "Loading users…" } },
            }
        }
    }
}

#[component]
fn PlexProfileForm(
    id: i64,
    initial: Option<i64>,
    profiles: Resource<Result<Option<Vec<PlexProfile>>, ServerFnError>>,
) -> Element {
    let mut saved = use_signal(|| initial);
    let mut selected = use_signal(|| initial);
    let mut ready = use_signal(|| false);
    let mut pending = use_signal(|| false);
    let mut notice = use_signal(|| None);
    let profiles = profiles.read();
    let (choices, hint) = match profiles.as_ref() {
        Some(Ok(Some(choices))) => (choices.as_slice(), None),
        Some(Ok(None)) => (&[][..], Some("Tautulli is not configured.")),
        Some(Err(_)) => (&[][..], Some("Could not load Plex profiles.")),
        None => (&[][..], Some("Loading Plex profiles…")),
    };
    let missing = saved().filter(|id| !choices.iter().any(|profile| profile.id == *id));
    let value = selected().map(|id| id.to_string()).unwrap_or_default();
    let can_save = selected() != saved();

    rsx! {
        form { class: "reset-form", method: "post", onmounted: move |_| ready.set(true), onsubmit: move |event| {
            event.prevent_default();
            async move {
                if pending() || selected() == saved() { return; }
                let profile_id = selected();
                pending.set(true);
                notice.set(None);
                let result = save_plex_profile(id, profile_id).await;
                pending.set(false);
                if matches!(result, Ok(None)) {
                    saved.set(profile_id);
                }
                notice.set(Some(form_notice(result, "Plex profile saved.")));
            }
        },
            label { r#for: "plex-profile-{id}", "Plex profile" }
            select { id: "plex-profile-{id}", value,
                disabled: !ready() || pending() || (choices.is_empty() && saved().is_none()),
                onchange: move |event| selected.set(event.value().parse().ok()),
                option { value: "", "No Plex profile" }
                if let Some(id) = missing {
                    option { value: "{id}", "Linked profile ({id})" }
                }
                for profile in choices {
                    option { key: "{profile.id}", value: "{profile.id}",
                        if choices.iter().filter(|other| other.name.eq_ignore_ascii_case(&profile.name)).count() > 1 {
                            "{profile.name} ({profile.id})"
                        } else {
                            "{profile.name}"
                        }
                    }
                }
            }
            if let Some(hint) = hint {
                p { "{hint}" }
            }
            button { class: "undo-button", r#type: "submit", disabled: !ready() || pending() || !can_save, "Save Plex profile" }
            FormNotice { notice: notice() }
        }
    }
}

#[component]
fn UserDetails(#[props(default)] user: Option<User>, on_saved: EventHandler<()>) -> Element {
    let id = user.as_ref().map(|user| user.id);
    let form_id = id.map_or_else(|| "new-user".to_owned(), |id| format!("user-{id}"));
    let mut username = use_signal(|| {
        user.as_ref()
            .map_or_else(String::new, |user| user.username.clone())
    });
    let mut first = use_signal(|| {
        user.as_ref()
            .map_or_else(String::new, |user| user.first_name.clone())
    });
    let mut last = use_signal(|| {
        user.as_ref()
            .map_or_else(String::new, |user| user.last_name.clone())
    });
    let mut password = use_signal(String::new);
    let mut ready = use_signal(|| false);
    let mut pending = use_signal(|| false);
    let mut notice = use_signal(|| None);

    rsx! {
        form { method: "post", onmounted: move |_| ready.set(true), onsubmit: move |event| {
            event.prevent_default();
            async move {
                if pending() { return; }
                pending.set(true);
                notice.set(None);
                let result = match id {
                    Some(id) => edit_user(id, username(), first(), last()).await,
                    None => create_user(username(), first(), last(), password()).await,
                };
                pending.set(false);
                if matches!(result, Ok(None)) {
                    if id.is_none() {
                        username.set(String::new());
                        first.set(String::new());
                        last.set(String::new());
                        password.set(String::new());
                    } else {
                        username.set(username().trim().to_ascii_lowercase());
                        first.set(first().trim().to_owned());
                        last.set(last().trim().to_owned());
                    }
                    on_saved.call(());
                }
                let success = if id.is_some() { "Details saved." } else { "User added." };
                notice.set(Some(form_notice(result, success)));
            }
        },
            label { r#for: "{form_id}-username", "Username" }
            input { id: "{form_id}-username", name: "username", autocomplete: "off", value: "{username}", required: true, maxlength: "40", autocapitalize: "none", spellcheck: "false",
                oninput: move |event| username.set(event.value()) }
            label { r#for: "{form_id}-first", "First name" }
            input { id: "{form_id}-first", name: "first_name", autocomplete: "off", value: "{first}", required: true, maxlength: "80",
                oninput: move |event| first.set(event.value()) }
            label { r#for: "{form_id}-last", "Last name" }
            input { id: "{form_id}-last", name: "last_name", autocomplete: "off", value: "{last}", required: true, maxlength: "80",
                oninput: move |event| last.set(event.value()) }
            if id.is_none() {
                PasswordField { id: "new-user-password".to_owned(), label: "Password", password }
            }
            button { class: "primary-button", r#type: "submit", disabled: !ready() || pending(),
                if id.is_some() { "Save details" } else { "Add user" }
            }
            FormNotice { notice: notice() }
        }
    }
}

#[component]
fn PasswordReset(id: i64, is_admin: bool) -> Element {
    let mut password = use_signal(String::new);
    let mut ready = use_signal(|| false);
    let mut notice = use_signal(|| None);
    let mut pending = use_signal(|| false);
    let navigator = use_navigator();
    rsx! {
        form { class: "reset-form", method: "post", onmounted: move |_| ready.set(true),
            onsubmit: move |event| {
                event.prevent_default();
                async move {
                    if pending() { return; }
                    pending.set(true);
                    notice.set(None);
                    let result = reset_password(id, password()).await;
                    pending.set(false);
                    if matches!(result, Ok(None)) {
                        password.set(String::new());
                        if is_admin {
                            navigator.replace(crate::Route::Login { error: None });
                            return;
                        }
                    }
                    notice.set(Some(form_notice(result, "Password reset. All sessions for this user were signed out.")));
                }
            },
            PasswordField { id: format!("reset-password-{id}"), label: "New password", password }
            button { class: "undo-button", r#type: "submit", disabled: !ready() || pending(), "Reset password" }
            FormNotice { notice: notice() }
        }
    }
}

#[component]
fn PasswordField(id: String, label: &'static str, mut password: Signal<String>) -> Element {
    rsx! {
        label { r#for: "{id}", "{label}" }
        input { id: "{id}", name: "password", r#type: "password", autocomplete: "new-password",
            value: "{password}", required: true,
            oninput: move |event| password.set(event.value()) }
    }
}

fn form_notice(result: Result<Option<String>, ServerFnError>, success: &str) -> (bool, String) {
    match result {
        Ok(None) => (true, success.to_owned()),
        Ok(Some(message)) => (false, message),
        Err(_) => (
            false,
            "Could not save. Check your session and try again.".to_owned(),
        ),
    }
}

#[component]
fn FormNotice(notice: Option<(bool, String)>) -> Element {
    match notice {
        Some((true, message)) => rsx! { p { class: "form-feedback", role: "status", "{message}" } },
        Some((false, message)) => rsx! { p { class: "form-error", role: "alert", "{message}" } },
        None => rsx! {},
    }
}

#[cfg(all(test, feature = "server"))]
mod tests {
    use super::write_plex_profile;

    #[test]
    fn plex_links_persist_and_reject_duplicate_profiles() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("household.sqlite");
        crate::database::initialize(&path).unwrap();
        let db = crate::database::open(&path).unwrap();
        db.execute_batch(
            "INSERT INTO users (id, username, first_name, last_name, password_hash)
             VALUES (1, 'connor', 'Connor', 'Example', 'unused'),
                    (2, 'member', 'Household', 'Member', 'unused');",
        )
        .unwrap();
        write_plex_profile(&db, 1, Some(100)).unwrap();
        write_plex_profile(&db, 2, Some(200)).unwrap();
        assert!(write_plex_profile(&db, 2, Some(100)).is_err());
        drop(db);

        let db = crate::database::open(&path).unwrap();
        let profile = |id| {
            db.query_row(
                "SELECT external_profile_id FROM activity_profile_links WHERE user_id = ?1",
                [id],
                |row| row.get::<_, i64>(0),
            )
        };
        assert_eq!(profile(1).unwrap(), 100);
        assert_eq!(profile(2).unwrap(), 200);
        write_plex_profile(&db, 1, Some(300)).unwrap();
        assert_eq!(profile(1).unwrap(), 300);
        write_plex_profile(&db, 1, None).unwrap();
        db.execute("DELETE FROM users WHERE id = 2", []).unwrap();
        assert!(profile(1).is_err() && profile(2).is_err());
    }
}
