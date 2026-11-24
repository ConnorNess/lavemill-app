use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Deserialize, Serialize)]
enum ConnectionStatus {
    Unavailable,
    Disconnected,
    Connected,
}

#[get("/api/calendar/connection")]
async fn connection_status() -> Result<ConnectionStatus, ServerFnError> {
    use crate::{auth::Viewer, google_calendar::GoogleCalendar};
    let context = dioxus::fullstack::FullstackContext::current()
        .ok_or_else(|| ServerFnError::new("Sign in to manage your calendar."))?;
    let viewer: Viewer = context
        .extension()
        .ok_or_else(|| ServerFnError::new("Sign in to manage your calendar."))?;
    if context.extension::<GoogleCalendar>().is_none() {
        return Ok(ConnectionStatus::Unavailable);
    }
    Ok(
        if GoogleCalendar::is_connected(viewer.id)
            .await
            .map_err(ServerFnError::new)?
        {
            ConnectionStatus::Connected
        } else {
            ConnectionStatus::Disconnected
        },
    )
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
        _ => None,
    };
    rsx! {
        main { id: "main", class: "dashboard",
            document::Title { "Lavemill · Calendar" }
            section { class: "add-task-panel",
                h1 { "Google Calendar" }
                if let Some(message) = message {
                    p { role: "status", "{message}" }
                }
                match status.as_ref() {
                    Some(Ok(ConnectionStatus::Connected)) => rsx! {
                        p { "Google Calendar is connected." }
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
fn ConnectButton(label: &'static str) -> Element {
    rsx! {
        form { action: "/calendar/google/connect", method: "post",
            button { class: "primary-button", r#type: "submit", "{label}" }
        }
    }
}
