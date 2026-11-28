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
    steam_id: Option<String>,
}

#[get("/api/users")]
async fn list_users() -> Result<Vec<User>, ServerFnError> {
    require_admin()?;
    crate::auth::server::run_blocking(|| {
        let db = crate::auth::server::connection()?;
        let mut query = db
            .prepare(
                "SELECT users.id, username, first_name, last_name, is_admin,
                        plex.external_profile_id, CAST(steam.external_profile_id AS TEXT)
                 FROM users
                 LEFT JOIN activity_profile_links AS plex
                   ON plex.user_id = users.id AND plex.provider = 'plex'
                 LEFT JOIN activity_profile_links AS steam
                   ON steam.user_id = users.id AND steam.provider = 'steam'
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
                    steam_id: row.get(6)?,
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
    write_activity_profile(db, id, "plex", profile_id)
}

#[post("/api/users/steam-profile")]
async fn save_steam_profile(id: i64, steam_id: String) -> Result<Option<String>, ServerFnError> {
    require_admin()?;
    let result = crate::auth::server::run_blocking(move || {
        write_steam_profile(&crate::auth::server::connection()?, id, &steam_id)
    })
    .await;
    Ok(result.err().map(str::to_owned))
}

#[cfg(feature = "server")]
fn write_steam_profile(
    db: &rusqlite::Connection,
    id: i64,
    steam_id: &str,
) -> Result<(), &'static str> {
    let steam_id = steam_id.trim();
    let profile_id = if steam_id.is_empty() {
        None
    } else if steam_id.len() == 17
        && !steam_id.starts_with('0')
        && steam_id.bytes().all(|byte| byte.is_ascii_digit())
    {
        steam_id.parse().ok()
    } else {
        return Err("Enter a 17-digit SteamID64.");
    };
    write_activity_profile(db, id, "steam", profile_id)
}

#[cfg(feature = "server")]
fn write_activity_profile(
    db: &rusqlite::Connection,
    id: i64,
    provider: &str,
    profile_id: Option<i64>,
) -> Result<(), &'static str> {
    if let Some(profile_id) = profile_id {
        db.execute(
            "INSERT INTO activity_profile_links (user_id, provider, external_profile_id)
             VALUES (?1, ?2, ?3)
             ON CONFLICT (user_id, provider)
             DO UPDATE SET external_profile_id = excluded.external_profile_id",
            rusqlite::params![id, provider, profile_id],
        )
        .map_err(|error| match error {
            rusqlite::Error::SqliteFailure(code, _)
                if code.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE =>
            {
                if provider == "steam" {
                    "That Steam account is already linked to another user."
                } else {
                    "That Plex profile is already linked to another user."
                }
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
            "DELETE FROM activity_profile_links WHERE user_id = ?1 AND provider = ?2",
            rusqlite::params![id, provider],
        )
        .map_err(crate::accounts::database_error)?;
    }
    Ok(())
}

#[derive(Deserialize, Serialize)]
enum CreateUser {
    Created(User),
    Invalid(String),
}

#[post("/api/users/create")]
async fn create_user(
    username: String,
    first: String,
    last: String,
    password: String,
) -> Result<CreateUser, ServerFnError> {
    require_admin()?;
    let result = crate::auth::server::run_blocking(move || {
        let (username, first, last) = crate::accounts::details(&username, &first, &last)?;
        let hash = crate::accounts::hash_password(&password)?;
        let id = crate::auth::server::connection()?.query_row(
            "INSERT INTO users (username, first_name, last_name, password_hash) VALUES (?1, ?2, ?3, ?4) RETURNING id",
            rusqlite::params![username, first, last, hash],
            |row| row.get(0),
        ).map_err(crate::accounts::save_error)?;
        Ok(User { id, username, first_name: first, last_name: last, is_admin: false, plex_profile_id: None, steam_id: None })
    }).await;
    Ok(match result {
        Ok(user) => CreateUser::Created(user),
        Err(error) => CreateUser::Invalid(error.into()),
    })
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
    if crate::auth::request_viewer().is_some_and(|viewer| viewer.is_admin) {
        return Ok(());
    }
    Err(ServerFnError::new("Administrator access required."))
}

#[component]
pub fn Users() -> Element {
    let users = use_server_future(list_users)?;
    let profiles = use_resource(plex_profiles);
    let mut selected = use_signal(|| None::<i64>);
    let mut adding = use_signal(|| false);
    let mut added = use_signal(|| false);
    let mut focus_after_add = use_signal(|| None::<&'static str>);
    let pending = use_signal(|| false);
    let directory = use_context_provider(|| UserDirectory(users));
    use_context_provider(|| pending);
    use_effect(move || {
        if let Some(id) = focus_after_add() {
            focus_after_add.set(None);
            let _ = document::eval(&format!("document.getElementById({id:?})?.focus()"));
        }
    });
    let result = users.read();
    rsx! {
        main { id: "main", class: "dashboard interior-page users-page", tabindex: "-1",
            document::Title { "Lavemill · Users" }
            h1 { "Users" }
            match result.as_ref() {
                Some(Ok(accounts)) => rsx! {
                    div { class: "user-list",
                        div { class: "users-toolbar",
                            div { class: "user-selector",
                                label { r#for: "users-account", "User" }
                                crate::select_field::SelectField {
                                    id: "users-account",
                                    label: "User",
                                    placeholder: "Choose a user",
                                    value: selected().map(|id| id.to_string()).unwrap_or_default(),
                                    options: accounts.iter().map(|user| (
                                        user.id.to_string(),
                                        format!("{} {} ({})", user.first_name, user.last_name, user.username),
                                    )).collect(),
                                    disabled: pending() || adding(),
                                    on_change: move |value: String| {
                                        if !*pending.peek() && !*adding.peek() {
                                            selected.set(value.parse().ok());
                                            added.set(false);
                                        }
                                    },
                                }
                            }
                            button {
                                id: "users-add-action", class: "primary-button", r#type: "button",
                                aria_expanded: adding(), aria_controls: "users-workspace", disabled: pending() || adding(),
                                onclick: move |_| {
                                    if !*pending.peek() {
                                        added.set(false);
                                        adding.set(true);
                                    }
                                },
                                "Add user"
                            }
                        }
                        if added() { p { class: "form-feedback", role: "status", "User added." } }
                        div { id: "users-workspace",
                            if adding() {
                                section { class: "user-add-card add-task-panel surface-panel",
                                    h2 { "Add user" }
                                    UserDetails { on_created: move |user: User| {
                                        let id = user.id;
                                        directory.insert(user);
                                        selected.set(Some(id));
                                        adding.set(false);
                                        added.set(true);
                                        focus_after_add.set(Some("users-account"));
                                    } }
                                    button { class: "secondary-button users-cancel-add", r#type: "button", disabled: pending(),
                                        onclick: move |_| {
                                            if !*pending.peek() {
                                                adding.set(false);
                                                focus_after_add.set(Some("users-add-action"));
                                            }
                                        },
                                        "Cancel"
                                    }
                                }
                            } else if let Some(user) = accounts.iter().find(|user| Some(user.id) == selected()) {
                                section { key: "{user.id}", class: "user-account-card add-task-panel surface-panel", aria_labelledby: "user-account-title",
                                    header { class: "users-card-heading",
                                        h2 { id: "user-account-title", "{user.first_name} {user.last_name}" }
                                        if user.is_admin { p { "Your admin account" } }
                                    }
                                    div { class: "user-account-sections",
                                        section {
                                            h3 { "Account details" }
                                            UserDetails { user: user.clone(), on_created: |_| {} }
                                        }
                                        section { class: "user-profiles",
                                            h3 { "Linked profiles" }
                                            PlexProfileForm { id: user.id, initial: user.plex_profile_id, profiles }
                                            SteamProfileForm { id: user.id, initial: user.steam_id.clone() }
                                        }
                                        section { class: "user-password-reset",
                                            h3 { "Reset password" }
                                            PasswordReset { id: user.id, is_admin: user.is_admin }
                                        }
                                    }
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

#[derive(Clone, Copy)]
struct UserDirectory(Resource<Result<Vec<User>, ServerFnError>>);

impl UserDirectory {
    fn insert(mut self, user: User) {
        if let Some(Ok(accounts)) = self.0.write().as_mut() {
            accounts.push(user);
            sort_users(accounts);
        }
    }

    fn update(mut self, id: i64, update: impl FnOnce(&mut User)) {
        let mut result = self.0.write();
        if let Some(Ok(accounts)) = result.as_mut() {
            if let Some(user) = accounts.iter_mut().find(|user| user.id == id) {
                update(user);
                sort_users(accounts);
            }
        }
    }
}

fn sort_users(accounts: &mut [User]) {
    accounts.sort_by(|a, b| {
        b.is_admin
            .cmp(&a.is_admin)
            .then_with(|| a.username.cmp(&b.username))
    });
}

#[component]
fn PlexProfileForm(
    id: i64,
    initial: Option<i64>,
    profiles: Resource<Result<Option<Vec<PlexProfile>>, ServerFnError>>,
) -> Element {
    let mut saved = use_signal(|| initial);
    let mut selected = use_signal(|| initial);
    let submission = use_form_submission();
    let directory = use_context::<UserDirectory>();
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
    let mut options = vec![(String::new(), "No Plex profile".to_owned())];
    if let Some(id) = missing {
        options.push((id.to_string(), format!("Linked profile ({id})")));
    }
    options.extend(choices.iter().map(|profile| {
        let name = if choices
            .iter()
            .filter(|other| other.name.eq_ignore_ascii_case(&profile.name))
            .count()
            > 1
        {
            format!("{} ({})", profile.name, profile.id)
        } else {
            profile.name.clone()
        };
        (profile.id.to_string(), name)
    }));

    rsx! {
        form { class: "reset-form", method: "post", onmounted: move |_| submission.mounted(), onsubmit: move |event| {
            event.prevent_default();
            async move {
                let profile_id = selected();
                if profile_id == saved() || !submission.begin() { return; }
                let result = save_plex_profile(id, profile_id).await;
                submission.finish();
                if matches!(result, Ok(None)) {
                    saved.set(profile_id);
                    directory.update(id, |user| user.plex_profile_id = profile_id);
                }
                submission.report(result, "Plex profile saved.");
            }
        },
            label { r#for: "plex-profile-{id}", "Plex profile" }
            crate::select_field::SelectField { id: format!("plex-profile-{id}"), label: "Plex profile", value, options,
                disabled: submission.disabled() || (choices.is_empty() && saved().is_none()),
                on_change: move |value: String| selected.set(value.parse().ok()),
            }
            if let Some(hint) = hint {
                p { "{hint}" }
            }
            button { class: "undo-button", r#type: "submit", disabled: submission.disabled() || !can_save, "Save Plex profile" }
            FormNotice { notice: (submission.notice)() }
        }
    }
}

#[component]
fn SteamProfileForm(id: i64, initial: Option<String>) -> Element {
    let mut saved = use_signal(|| initial.unwrap_or_default());
    let mut input = use_signal(|| saved());
    let submission = use_form_submission();
    let directory = use_context::<UserDirectory>();

    rsx! {
        form { class: "reset-form", method: "post", onmounted: move |_| submission.mounted(), onsubmit: move |event| {
            event.prevent_default();
            async move {
                let steam_id = input().trim().to_owned();
                if steam_id == saved() || !submission.begin() { return; }
                let result = save_steam_profile(id, steam_id.clone()).await;
                submission.finish();
                if matches!(result, Ok(None)) {
                    directory.update(id, |user| user.steam_id = (!steam_id.is_empty()).then(|| steam_id.clone()));
                    input.set(steam_id.clone());
                    saved.set(steam_id);
                }
                submission.report(result, "Steam profile saved.");
            }
        },
            label { r#for: "steam-id-{id}", "SteamID64" }
            input { id: "steam-id-{id}", inputmode: "numeric", value: "{input}",
                aria_describedby: "steam-id-help-{id}", disabled: submission.disabled(),
                oninput: move |event| input.set(event.value()),
            }
            p { id: "steam-id-help-{id}", "Link at the member’s request to share their visible playing activity with the household. Clear the field and save to unlink." }
            button { class: "undo-button", r#type: "submit",
                disabled: submission.disabled() || input().trim() == saved(), "Save Steam profile"
            }
            FormNotice { notice: (submission.notice)() }
        }
    }
}

#[component]
fn UserDetails(#[props(default)] user: Option<User>, on_created: EventHandler<User>) -> Element {
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
    let submission = use_form_submission();
    let directory = use_context::<UserDirectory>();

    rsx! {
        form { method: "post", onmounted: move |_| submission.mounted(), onsubmit: move |event| {
            event.prevent_default();
            async move {
                if !submission.begin() { return; }
                let (submitted_username, submitted_first, submitted_last) = (username(), first(), last());
                let Some(id) = id else {
                    let result = create_user(submitted_username, submitted_first, submitted_last, password()).await;
                    submission.finish();
                    match result {
                        Ok(CreateUser::Created(user)) => {
                            password.set(String::new());
                            on_created.call(user);
                        }
                        Ok(CreateUser::Invalid(message)) => submission.report(Ok(Some(message)), "User added."),
                        Err(error) => submission.report(Err(error), "User added."),
                    }
                    return;
                };
                let result = edit_user(id, submitted_username.clone(), submitted_first.clone(), submitted_last.clone()).await;
                submission.finish();
                if matches!(result, Ok(None)) {
                    let saved_username = submitted_username.trim().to_ascii_lowercase();
                    let saved_first = submitted_first.trim().to_owned();
                    let saved_last = submitted_last.trim().to_owned();
                    directory.update(id, |user| {
                        user.username = saved_username.clone();
                        user.first_name = saved_first.clone();
                        user.last_name = saved_last.clone();
                    });
                    username.set(saved_username);
                    first.set(saved_first);
                    last.set(saved_last);
                }
                submission.report(result, "Details saved.");
            }
        },
            label { r#for: "{form_id}-username", "Username" }
            input { id: "{form_id}-username", name: "username", autocomplete: "off", value: "{username}", required: true, maxlength: "40", autocapitalize: "none", spellcheck: "false", readonly: (submission.pending)(),
                onmounted: move |event| async move { if id.is_none() { let _ = event.set_focus(true).await; } },
                oninput: move |event| username.set(event.value()) }
            label { r#for: "{form_id}-first", "First name" }
            input { id: "{form_id}-first", name: "first_name", autocomplete: "off", value: "{first}", required: true, maxlength: "80", readonly: (submission.pending)(),
                oninput: move |event| first.set(event.value()) }
            label { r#for: "{form_id}-last", "Last name" }
            input { id: "{form_id}-last", name: "last_name", autocomplete: "off", value: "{last}", required: true, maxlength: "80", readonly: (submission.pending)(),
                oninput: move |event| last.set(event.value()) }
            if id.is_none() {
                PasswordField { id: "new-user-password".to_owned(), label: "Password", password, readonly: (submission.pending)() }
            }
            button { class: "primary-button", r#type: "submit", disabled: submission.disabled(),
                if id.is_some() { "Save details" } else { "Add user" }
            }
            FormNotice { notice: (submission.notice)() }
        }
    }
}

#[component]
fn PasswordReset(id: i64, is_admin: bool) -> Element {
    let mut password = use_signal(String::new);
    let submission = use_form_submission();
    let navigator = use_navigator();
    rsx! {
        form { class: "reset-form", method: "post", onmounted: move |_| submission.mounted(),
            onsubmit: move |event| {
                event.prevent_default();
                async move {
                    if !submission.begin() { return; }
                    let result = reset_password(id, password()).await;
                    submission.finish();
                    if matches!(result, Ok(None)) {
                        password.set(String::new());
                        if is_admin {
                            navigator.replace(crate::Route::Login { error: None });
                            return;
                        }
                    }
                    submission.report(result, "Password reset. All sessions for this user were signed out.");
                }
            },
            PasswordField { id: format!("reset-password-{id}"), label: "New password", password, readonly: (submission.pending)() }
            button { class: "undo-button", r#type: "submit", disabled: submission.disabled(), "Reset password" }
            FormNotice { notice: (submission.notice)() }
        }
    }
}

#[component]
fn PasswordField(
    id: String,
    label: &'static str,
    mut password: Signal<String>,
    readonly: bool,
) -> Element {
    rsx! {
        label { r#for: "{id}", "{label}" }
        input { id: "{id}", name: "password", r#type: "password", autocomplete: "new-password",
            value: "{password}", required: true, readonly,
            oninput: move |event| password.set(event.value()) }
    }
}

#[derive(Clone, Copy)]
struct FormSubmission {
    ready: Signal<bool>,
    pending: Signal<bool>,
    notice: Signal<Option<(bool, String)>>,
}

fn use_form_submission() -> FormSubmission {
    FormSubmission {
        ready: use_signal(|| false),
        pending: use_context::<Signal<bool>>(),
        notice: use_signal(|| None),
    }
}

impl FormSubmission {
    fn mounted(mut self) {
        self.ready.set(true);
    }

    fn disabled(self) -> bool {
        !(self.ready)() || (self.pending)()
    }

    fn begin(mut self) -> bool {
        if self.disabled() {
            return false;
        }
        self.pending.set(true);
        self.notice.set(None);
        true
    }

    fn finish(mut self) {
        self.pending.set(false);
    }

    fn report(mut self, result: Result<Option<String>, ServerFnError>, success: &str) {
        self.notice.set(Some(form_notice(result, success)));
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
    use super::{write_plex_profile, write_steam_profile};

    #[test]
    fn steam_links_validate_and_leave_plex_links_intact() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("household.sqlite");
        crate::database::initialize(&path).unwrap();
        let db = crate::database::open(&path).unwrap();
        db.execute_batch(
            "INSERT INTO users (id, username, first_name, last_name, password_hash)
             VALUES (1, 'admin', 'Admin', 'Example', 'unused'),
                    (2, 'member', 'Household', 'Member', 'unused');",
        )
        .unwrap();
        let profile = |provider: &str| {
            db.query_row(
                "SELECT external_profile_id FROM activity_profile_links WHERE user_id = 1 AND provider = ?1",
                [provider],
                |row| row.get::<_, i64>(0),
            )
        };
        write_plex_profile(&db, 1, Some(100)).unwrap();
        write_steam_profile(&db, 1, " 76561198000000001 ").unwrap();
        assert_eq!(profile("steam").unwrap(), 76561198000000001);
        assert!(write_steam_profile(&db, 2, "76561198000000001").is_err());
        assert!(write_steam_profile(&db, 1, "not-a-steam-id").is_err());
        write_steam_profile(&db, 1, "76561198000000002").unwrap();
        assert_eq!(profile("steam").unwrap(), 76561198000000002);
        write_steam_profile(&db, 1, " ").unwrap();
        assert!(profile("steam").is_err());
        assert_eq!(profile("plex").unwrap(), 100);
    }

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
