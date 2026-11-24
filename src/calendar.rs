use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

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

#[get("/api/calendar/connection")]
async fn connection_status() -> Result<ConnectionStatus, ServerFnError> {
    use crate::{auth::Viewer, google_calendar::GoogleCalendar};
    let context = dioxus::fullstack::FullstackContext::current()
        .ok_or_else(|| ServerFnError::new("Sign in to manage your calendar."))?;
    let viewer: Viewer = context
        .extension()
        .ok_or_else(|| ServerFnError::new("Sign in to manage your calendar."))?;
    let Some(google) = context.extension::<GoogleCalendar>() else {
        return Ok(ConnectionStatus::Unavailable);
    };
    google.status(viewer.id).await.map_err(ServerFnError::new)
}

#[component]
pub fn Calendar(notice: Option<String>) -> Element {
    let status = use_server_future(connection_status)?;
    let status = status.read();
    let message = match notice.as_deref() {
        Some("connected") => Some("Google Calendar connected."),
        Some("disconnected") => Some("Google Calendar disconnected."),
        Some("cancelled") => Some("Google Calendar connection was cancelled."),
        Some("origin") => Some("Open Lavemill at its configured address and try again."),
        Some("failed") => Some("Could not update the Google Calendar connection. Try again."),
        Some("reconnect") => Some("Reconnect Google Calendar to continue."),
        Some("selected") => Some("Calendar selection saved."),
        Some("shared") => Some("Calendar sharing is on."),
        Some("unshared") => Some("Calendar sharing is off."),
        Some("selection_failed") => {
            Some("Could not update your calendar selection. Reload and try again.")
        }
        _ => None,
    };
    rsx! {
        main { id: "main", class: "dashboard",
            document::Title { "Lavemill · Calendar" }
            section { class: "add-task-panel calendar-settings",
                h1 { "Google Calendar" }
                if let Some(message) = message {
                    p { role: "status", "{message}" }
                }
                match status.as_ref() {
                    Some(Ok(ConnectionStatus::Connected { selected, shared, choices })) => rsx! {
                        CalendarSharing { selected: selected.clone(), shared: *shared, choices: choices.clone() }
                        ConnectButton { label: "Reconnect Google Calendar" }
                        form { action: "/calendar/google/disconnect", method: "post",
                            button { class: "undo-button", r#type: "submit", "Disconnect Google Calendar" }
                        }
                    },
                    Some(Ok(ConnectionStatus::Disconnected)) => rsx! {
                        ConnectButton { label: "Connect Google Calendar" }
                    },
                    Some(Ok(ConnectionStatus::Unavailable)) => rsx! {
                        p { "Google Calendar linking is unavailable on this server." }
                    },
                    Some(Err(_)) => rsx! {
                        p { role: "alert", "Could not check your calendar connection. Reload to try again." }
                    },
                    None => rsx! { p { "Checking your calendar connection…" } },
                }
            }
        }
    }
}

#[component]
fn CalendarSharing(selected: Option<String>, shared: bool, choices: CalendarChoices) -> Element {
    let (selected_is_available, share_status) = match &choices {
        CalendarChoices::Available(calendars) => {
            let exists = calendars
                .iter()
                .any(|calendar| Some(&calendar.id) == selected.as_ref());
            (
                exists,
                if selected.is_some() && !exists {
                    Some("Your selected calendar is no longer available.")
                } else {
                    None
                },
            )
        }
        CalendarChoices::Reconnect => (
            false,
            Some("Reconnect Google Calendar to access your calendars."),
        ),
        CalendarChoices::Unavailable => (
            false,
            Some("Could not load your calendars from Google. Reload to try again."),
        ),
    };
    rsx! {
        if let Some(message) = share_status {
            p { role: "status", "{message}" }
        }
        if let CalendarChoices::Available(calendars) = choices {
            if calendars.is_empty() {
                p { "There are no readable calendars on this Google account." }
            } else {
                form { action: "/calendar/google/select", method: "post",
                    label { r#for: "selected-calendar", "Google calendar" }
                    select { id: "selected-calendar", name: "calendar_id", required: true,
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
            form { action: "/calendar/google/share", method: "post",
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
fn ConnectButton(label: &'static str) -> Element {
    rsx! {
        form { action: "/calendar/google/connect", method: "post",
            button { class: "primary-button", r#type: "submit", "{label}" }
        }
    }
}
