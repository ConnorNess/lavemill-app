use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct User {
    id: i64,
    username: String,
    first_name: String,
    last_name: String,
    is_admin: bool,
}

#[get("/api/users")]
async fn list_users() -> Result<Vec<User>, ServerFnError> {
    require_admin()?;
    crate::auth::server::run_blocking(|| {
        let db = crate::auth::server::connection()?;
        let mut query = db
            .prepare(
                "SELECT id, username, first_name, last_name, is_admin
             FROM users ORDER BY is_admin DESC, username",
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
                })
            })
            .map_err(crate::accounts::database_error)?;
        rows.collect::<Result<_, _>>()
            .map_err(crate::accounts::database_error)
    })
    .await
    .map_err(ServerFnError::new)
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
            value: "{password}", required: true, minlength: "15", maxlength: "128",
            oninput: move |event| password.set(event.value()) }
        small { class: "password-hint", "Use at least 15 characters." }
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
