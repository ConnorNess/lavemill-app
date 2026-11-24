use crate::{CardHeading, Route};
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Google,
    Outlook,
}

impl Provider {
    pub fn key(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Outlook => "outlook",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Google => "Google Calendar",
            Self::Outlook => "Outlook",
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub enum ConnectionStatus {
    Unavailable,
    Disconnected,
    Connected {
        selected: Option<String>,
        shared: bool,
        choices: CalendarChoices,
    },
}

#[derive(Clone, Deserialize, PartialEq, Serialize)]
pub enum CalendarChoices {
    Available(Vec<CalendarChoice>),
    Reconnect,
    Unavailable,
}

#[derive(Clone, Deserialize, PartialEq, Serialize)]
pub struct CalendarChoice {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Agenda {
    pub has_shared_calendars: bool,
    pub incomplete: bool,
    pub days: Vec<AgendaDay>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct AgendaDay {
    pub date: String,
    pub label: String,
    pub entries: Vec<AgendaEntry>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct AgendaEntry {
    pub title: String,
    pub when: String,
}

#[derive(Clone, Deserialize, PartialEq, Serialize)]
pub struct CalendarViewData {
    pub month: bool,
    pub date: String,
    pub today: String,
    pub previous: String,
    pub next: String,
    pub label: String,
    pub configured: bool,
    pub has_shared_calendars: bool,
    pub incomplete: bool,
    pub days: Vec<CalendarDay>,
}

#[derive(Clone, Deserialize, PartialEq, Serialize)]
pub struct CalendarDay {
    pub date: String,
    pub weekday: String,
    pub number: u32,
    pub label: String,
    pub outside_month: bool,
    pub events: Vec<CalendarViewEvent>,
}

#[derive(Clone, Deserialize, PartialEq, Serialize)]
pub struct CalendarViewEvent {
    pub title: String,
    pub when: String,
    pub minutes: Option<(u32, u32)>,
}

#[derive(Clone, Deserialize, Serialize)]
struct CalendarViewQuery {
    view: Option<String>,
    date: Option<String>,
}

#[get("/api/calendar/view?:query")]
async fn calendar_view(query: CalendarViewQuery) -> Result<CalendarViewData, ServerFnError> {
    let (_, clients) = calendar_context()?;
    clients
        .calendar_view(
            query.view.as_deref() == Some("month"),
            query.date.as_deref(),
        )
        .await
        .map_err(ServerFnError::new)
}

#[get("/api/calendar/connection")]
async fn connection_status() -> Result<ConnectionStatus, ServerFnError> {
    provider_status(Provider::Google).await
}

#[get("/api/calendar/outlook/connection")]
async fn outlook_status() -> Result<ConnectionStatus, ServerFnError> {
    provider_status(Provider::Outlook).await
}

#[cfg(feature = "server")]
async fn provider_status(provider: Provider) -> Result<ConnectionStatus, ServerFnError> {
    let (viewer, clients) = calendar_context()?;
    let Some(client) = clients.get(provider) else {
        return Ok(ConnectionStatus::Unavailable);
    };
    client.status(viewer.id).await.map_err(ServerFnError::new)
}

#[get("/api/calendar/agenda")]
async fn shared_agenda() -> Result<Option<Agenda>, ServerFnError> {
    let (_, clients) = calendar_context()?;
    clients.weekly_agenda().await.map_err(ServerFnError::new)
}

#[cfg(feature = "server")]
fn calendar_context(
) -> Result<(crate::auth::Viewer, crate::calendar_server::CalendarClients), ServerFnError> {
    let context = dioxus::fullstack::FullstackContext::current()
        .ok_or_else(|| ServerFnError::new("Sign in to use Calendar."))?;
    let viewer = context
        .extension()
        .ok_or_else(|| ServerFnError::new("Sign in to use Calendar."))?;
    let clients = context
        .extension()
        .ok_or_else(|| ServerFnError::new("Calendar is unavailable."))?;
    Ok((viewer, clients))
}

#[component]
pub fn CalendarCard() -> Element {
    let agenda = use_server_future(shared_agenda)?;
    let agenda = agenda.read();
    rsx! {
        section { class: "dashboard-card", aria_labelledby: "calendar-title",
            CardHeading { icon: "calendar", title: "Shared Calendar", id: "calendar-title", count: "This week" }
            div { class: "card-body agenda",
                match agenda.as_ref() {
                    Some(Ok(Some(agenda))) => rsx! {
                        if agenda.incomplete {
                            p { role: "status", "Some shared calendars could not be loaded." }
                        }
                        if !agenda.has_shared_calendars {
                            p { "Your household hasn’t shared a calendar yet." }
                            Link { to: Route::Calendar { notice: None }, "Connect a calendar" }
                        } else if agenda.days.is_empty() && !agenda.incomplete {
                            p { "No shared events this week." }
                        }
                        for day in &agenda.days {
                            section { key: "{day.date}", class: "agenda-group",
                                h3 { class: "agenda-date", "{day.label}", span { "{day.date}" } }
                                ul { class: "item-list",
                                    for (index, entry) in day.entries.iter().enumerate() {
                                        li { key: "{index}", class: "agenda-row",
                                            span { class: "agenda-marker", aria_hidden: "true" }
                                            div { class: "row-copy",
                                                span { class: "row-title", "{entry.title}" }
                                                span { class: "row-note", "{entry.when}" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    },
                    Some(Ok(None)) => rsx! { p { "Calendar linking is unavailable on this server." } },
                    Some(Err(_)) => rsx! { p { role: "alert", "Could not load the shared calendar. Reload to try again." } },
                    None => rsx! { p { "Loading the shared calendar…" } },
                }
            }
        }
    }
}

#[component]
pub fn Calendar(notice: Option<String>) -> Element {
    let (notice_provider, notice) = match notice.as_deref() {
        Some(notice) if notice.starts_with("outlook_") => {
            (Provider::Outlook, notice.strip_prefix("outlook_"))
        }
        notice => (Provider::Google, notice),
    };
    let provider = notice_provider.label();
    let message = match notice {
        Some("connected") => Some(format!("{provider} connected.")),
        Some("disconnected") => Some(format!("{provider} disconnected.")),
        Some("cancelled") => Some(format!("{provider} connection was cancelled.")),
        Some("origin") => Some("Open Lavemill at its configured address and try again.".into()),
        Some("failed") => Some(format!(
            "Could not update the {provider} connection. Try again."
        )),
        Some("reconnect") => Some(format!("Reconnect {provider} to continue.")),
        Some("selected") => Some("Calendar selection saved.".into()),
        Some("shared") => Some("Calendar sharing is on.".into()),
        Some("unshared") => Some("Calendar sharing is off.".into()),
        Some("selection_failed") => {
            Some("Could not update your calendar selection. Reload and try again.".into())
        }
        _ => None,
    };
    rsx! {
        main { id: "main", class: "dashboard",
            document::Title { "Lavemill ? Calendar" }
            h1 { "Calendar" }
            if let Some(message) = message {
                p { role: "status", "{message}" }
            }
            CalendarConnection { provider: Provider::Google }
            CalendarConnection { provider: Provider::Outlook }
        }
    }
}

#[component]
fn CalendarConnection(provider: Provider) -> Element {
    let status = use_server_future(move || async move {
        match provider {
            Provider::Google => connection_status().await,
            Provider::Outlook => outlook_status().await,
        }
    })?;
    let status = status.read();
    let name = provider.label();
    let key = provider.key();
    rsx! {
            section { class: "add-task-panel calendar-settings",
                h2 { "{name}" }
                match status.as_ref() {
                    Some(Ok(ConnectionStatus::Connected { selected, shared, choices })) => rsx! {
                        CalendarSharing { provider, selected: selected.clone(), shared: *shared, choices: choices.clone() }
                        ConnectButton { provider, reconnect: true }
                        form { action: "/calendar/{key}/disconnect", method: "post",
                            button { class: "undo-button", r#type: "submit", "Disconnect {name}" }
                        }
                    },
                    Some(Ok(ConnectionStatus::Disconnected)) => rsx! {
                        ConnectButton { provider, reconnect: false }
                    },
                    Some(Ok(ConnectionStatus::Unavailable)) => rsx! {
                        p { "{name} linking is unavailable on this server." }
                    },
                    Some(Err(_)) => rsx! {
                        p { role: "alert", "Could not check your calendar connection. Reload to try again." }
                    },
                    None => rsx! { p { "Checking your calendar connection…" } },
                }
            }
    }
}

#[component]
fn CalendarSharing(
    provider: Provider,
    selected: Option<String>,
    shared: bool,
    choices: CalendarChoices,
) -> Element {
    let name = provider.label();
    let key = provider.key();
    let (selected_is_available, share_status) = match &choices {
        CalendarChoices::Available(calendars) => {
            let exists = calendars
                .iter()
                .any(|calendar| Some(&calendar.id) == selected.as_ref());
            (
                exists,
                if selected.is_some() && !exists {
                    Some("Your selected calendar is no longer available.".to_owned())
                } else {
                    None
                },
            )
        }
        CalendarChoices::Reconnect => (
            false,
            Some(format!("Reconnect {name} to access your calendars.")),
        ),
        CalendarChoices::Unavailable => (
            false,
            Some(format!(
                "Could not load your calendars from {name}. Reload to try again."
            )),
        ),
    };
    rsx! {
        if let Some(message) = share_status {
            p { role: "status", "{message}" }
        }
        if let CalendarChoices::Available(calendars) = choices {
            if calendars.is_empty() {
                p { "There are no readable calendars on this account." }
            } else {
                form { action: "/calendar/{key}/select", method: "post",
                    label { r#for: "selected-calendar-{key}", "Calendar" }
                    select { id: "selected-calendar-{key}", name: "calendar_id", required: true,
                        option { value: "", disabled: true, selected: !selected_is_available, "Choose a calendar" }
                        for calendar in calendars {
                            option { key: "{calendar.id}", value: "{calendar.id}", selected: Some(&calendar.id) == selected.as_ref(), "{calendar.name}" }
                        }
                    }
                    button { class: "primary-button", r#type: "submit", "Save calendar selection" }
                }
            }
        }
        if let Some(id) = selected.filter(|_| shared || selected_is_available) {
            form { action: "/calendar/{key}/share", method: "post",
                input { r#type: "hidden", name: "calendar_id", value: "{id}" }
                if shared {
                    p { "This calendar is shared with your household." }
                    button { class: "undo-button", r#type: "submit", name: "shared", value: "false", "Stop sharing" }
                } else {
                    p { "Share this calendar to show event titles and times to signed-in household members. Events marked private appear as Busy." }
                    button { class: "primary-button", r#type: "submit", name: "shared", value: "true", "Share with household" }
                }
            }
        }
    }
}

#[component]
fn ConnectButton(provider: Provider, reconnect: bool) -> Element {
    let key = provider.key();
    let action = if reconnect { "Reconnect" } else { "Connect" };
    let name = provider.label();
    rsx! {
        form { action: "/calendar/{key}/connect", method: "post",
            button { class: "primary-button", r#type: "submit", "{action} {name}" }
        }
    }
}
