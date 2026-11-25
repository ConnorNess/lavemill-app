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
            CardHeading {
                icon: "calendar",
                title: "Shared Calendar",
                id: "calendar-title",
                count: "This week",
            }
            div { class: "card-body agenda",
                match agenda.as_ref() {
                    Some(Ok(Some(agenda))) => rsx! {
                        if agenda.incomplete {
                            p { role: "status", "Some shared calendars could not be loaded." }
                        }
                        if !agenda.has_shared_calendars {
                            p { "Your household hasn’t shared a calendar yet." }
                            Link {
                                to: Route::Calendar {
                                    view: None,
                                    date: None,
                                    notice: Some("manage".into()),
                                },
                                "Connect a calendar"
                            }
                        } else if agenda.days.is_empty() && !agenda.incomplete {
                            p { "No shared events this week." }
                        }
                        for day in &agenda.days {
                            section { key: "{day.date}", class: "agenda-group",
                                h3 { class: "agenda-date",
                                    "{day.label}"
                                    span { "{day.date}" }
                                }
                                ul { class: "item-list",
                                    for (index , entry) in day.entries.iter().enumerate() {
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
                    Some(Ok(None)) => rsx! {
                        p { "Calendar linking is unavailable on this server." }
                    },
                    Some(Err(_)) => rsx! {
                        p { role: "alert", "Could not load the shared calendar. Reload to try again." }
                    },
                    None => rsx! {
                        p { "Loading the shared calendar…" }
                    },
                }
            }
        }
    }
}

#[component]
pub fn Calendar(view: Option<String>, date: Option<String>, notice: Option<String>) -> Element {
    let mut manage_open = use_signal(|| notice.is_some());
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
        main { id: "main", class: "dashboard calendar-page",
            document::Title { "Lavemill - Calendar" }
            div { class: "calendar-heading",
                h1 { "Calendar" }
                button {
                    class: "secondary-button",
                    r#type: "button",
                    aria_expanded: manage_open(),
                    aria_controls: "calendar-connections",
                    onclick: move |_| manage_open.toggle(),
                    "Manage calendars"
                }
            }
            section {
                id: "calendar-connections",
                hidden: !manage_open(),
                aria_label: "Calendar connections",
                if manage_open() {
                    if let Some(message) = message {
                        p { role: "status", "{message}" }
                    }
                    div { class: "calendar-connections",
                        CalendarConnection { provider: Provider::Google }
                        CalendarConnection { provider: Provider::Outlook }
                    }
                }
            }
            CalendarPeriod { key: "{view:?}/{date:?}", view, date }
        }
    }
}

fn period_route(view: Option<String>, date: String) -> Route {
    Route::Calendar {
        view,
        date: Some(date),
        notice: None,
    }
}

#[component]
fn CalendarPeriod(view: Option<String>, date: Option<String>) -> Element {
    let query = CalendarViewQuery { view, date };
    let data = use_server_future(move || calendar_view(query.clone()))?;
    let mut selected_day = use_signal(|| None::<String>);
    let data = data.read();
    let Some(result) = data.as_ref() else {
        return rsx! {
            p { "Loading the shared calendar…" }
        };
    };
    let Ok(data) = result else {
        return rsx! {
            p { role: "alert", "Could not load the shared calendar. Reload to try again." }
        };
    };
    let selected = selected_day().unwrap_or_else(|| data.date.clone());
    let week_href = period_route(Some("week".into()), selected.clone()).to_string();
    let month_href = period_route(Some("month".into()), selected.clone()).to_string();
    let view = if data.month { "month" } else { "week" };
    rsx! {
        div { class: "calendar-toolbar",
            h2 { "{data.label}" }
            div { class: "calendar-toolbar-actions",
                nav { class: "calendar-nav", aria_label: "Calendar view",
                    a {
                        href: week_href,
                        aria_current: if !data.month { "true" } else { "false" },
                        "Week"
                    }
                    a {
                        href: month_href,
                        aria_current: if data.month { "true" } else { "false" },
                        "Month"
                    }
                }
                nav { class: "calendar-nav", aria_label: "Calendar dates",
                    a { href: period_route(Some(view.into()), data.previous.clone()).to_string(),
                        "Previous"
                    }
                    a { href: period_route(Some(view.into()), data.today.clone()).to_string(),
                        "Today"
                    }
                    a { href: period_route(Some(view.into()), data.next.clone()).to_string(),
                        "Next"
                    }
                }
            }
        }
        if data.incomplete {
            p { role: "status", "Some shared calendars could not be loaded." }
        }
        if !data.configured {
            p { class: "row-note", "Calendar linking is unavailable on this server." }
        } else if !data.has_shared_calendars {
            p { class: "row-note", "Your household hasn’t shared a calendar yet." }
        }
        if data.month {
            MonthGrid {
                days: data.days.clone(),
                today: data.today.clone(),
                selected,
                on_select: move |day| selected_day.set(Some(day)),
            }
        } else {
            WeekGrid { days: data.days.clone(), today: data.today.clone() }
        }
    }
}

#[component]
fn MonthGrid(
    days: Vec<CalendarDay>,
    today: String,
    selected: String,
    on_select: EventHandler<String>,
) -> Element {
    let selected_day = days.iter().find(|day| day.date == selected);
    rsx! {
        div {
            class: "calendar-month",
            role: "group",
            aria_label: "Month calendar",
            for day in days.iter().take(7) {
                div {
                    key: "heading-{day.weekday}",
                    class: "calendar-month-weekday",
                    "{day.weekday}"
                }
            }
            for day in &days {
                button {
                    key: "{day.date}",
                    class: "calendar-month-day",
                    class: if day.outside_month { "calendar-outside-month" },
                    r#type: "button",
                    aria_current: if day.date == today { "date" } else { "false" },
                    aria_pressed: day.date == selected,
                    aria_controls: "calendar-day-events",
                    aria_label: "{day.label}, {day.events.len()} events",
                    onclick: {
                        let date = day.date.clone();
                        move |_| on_select.call(date.clone())
                    },
                    span { class: "calendar-month-number", "{day.number}" }
                    for (index , event) in day.events.iter().take(3).enumerate() {
                        span { key: "{index}", class: "calendar-month-event", "{event.title}" }
                    }
                    if day.events.len() > 3 {
                        span { class: "calendar-month-more", "+{day.events.len() - 3} more" }
                    }
                }
            }
        }
        if let Some(day) = selected_day {
            section {
                id: "calendar-day-events",
                class: "calendar-day-details",
                aria_labelledby: "calendar-day-title",
                h3 { id: "calendar-day-title", "{day.label}" }
                if day.events.is_empty() {
                    p { class: "row-note", "No shared events on this day." }
                } else {
                    ul { class: "item-list",
                        for (index , event) in day.events.iter().enumerate() {
                            li { key: "{index}", class: "calendar-day-event",
                                span { "{event.title}" }
                                span { class: "row-note", "{event.when}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn WeekGrid(days: Vec<CalendarDay>, today: String) -> Element {
    rsx! {
        div {
            class: "calendar-week-scroll",
            role: "region",
            aria_label: "Hourly week calendar",
            tabindex: "0",
            div { class: "calendar-week",
                div { class: "calendar-week-row calendar-week-head",
                    span {}
                    for day in &days {
                        h3 {
                            key: "{day.date}",
                            aria_label: "{day.label}",
                            class: if day.date == today { "calendar-today" },
                            "{day.weekday} {day.number}"
                        }
                    }
                }
                div { class: "calendar-week-row calendar-all-day",
                    span { class: "calendar-all-day-label", "All day" }
                    for day in &days {
                        ul {
                            key: "{day.date}",
                            class: "item-list",
                            aria_label: "All-day events on {day.label}",
                            for (index , event) in day.events.iter().filter(|event| event.minutes.is_none()).enumerate() {
                                li { key: "{index}", "{event.title}" }
                            }
                        }
                    }
                }
                div { class: "calendar-week-row calendar-hours",
                    div { class: "calendar-hour-labels", aria_hidden: "true",
                        for hour in 0..24 {
                            div { key: "{hour}", "{hour:02}:00" }
                        }
                    }
                    for day in &days {
                        section {
                            key: "{day.date}",
                            class: "calendar-hour-column",
                            aria_label: "Timed events on {day.label}",
                            for (index , block) in scheduled_blocks(&day.events).into_iter().enumerate() {
                                div {
                                    key: "{index}",
                                    class: "calendar-timed-event",
                                    style: block.style(),
                                    title: "{block.event.title} — {block.event.when}",
                                    span { class: "calendar-timed-title", "{block.event.title}" }
                                    span { class: "calendar-timed-time", "{block.event.when}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

struct ScheduledBlock<'a> {
    event: &'a CalendarViewEvent,
    start: u32,
    end: u32,
    lane: usize,
    lanes: usize,
}

impl ScheduledBlock<'_> {
    fn style(&self) -> String {
        let top = f64::from(self.start) / 14.4;
        let height = f64::from(self.end - self.start) / 14.4;
        let width = 100.0 / self.lanes as f64;
        let left = width * self.lane as f64;
        format!("top:{top:.3}%;height:{height:.3}%;left:{left:.3}%;width:{width:.3}%")
    }
}

fn scheduled_blocks(events: &[CalendarViewEvent]) -> Vec<ScheduledBlock<'_>> {
    let mut blocks: Vec<ScheduledBlock<'_>> = Vec::new();
    let mut lane_ends = Vec::new();
    let mut group_start = 0;
    let mut group_end = 0;
    for (event, (start, end)) in events
        .iter()
        .filter_map(|event| Some((event, event.minutes?)))
    {
        let end = end.max(start + 24).min(1440);
        if start >= group_end {
            for block in &mut blocks[group_start..] {
                block.lanes = lane_ends.len();
            }
            group_start = blocks.len();
            lane_ends.clear();
        }
        let lane = lane_ends
            .iter()
            .position(|until| *until <= start)
            .unwrap_or(lane_ends.len());
        if lane == lane_ends.len() {
            lane_ends.push(end);
        } else {
            lane_ends[lane] = end;
        }
        group_end = group_end.max(end);
        blocks.push(ScheduledBlock {
            event,
            start,
            end,
            lane,
            lanes: 1,
        });
    }
    for block in &mut blocks[group_start..] {
        block.lanes = lane_ends.len();
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_routes_round_trip_view_date_and_oauth_notice() {
        assert_eq!(
            "/calendar".parse::<Route>().unwrap(),
            Route::Calendar {
                view: None,
                date: None,
                notice: None,
            }
        );
        let route = period_route(Some("month".into()), "2026-12-31".into());
        assert_eq!(route.to_string().parse::<Route>().unwrap(), route);
        assert_eq!(
            "/calendar?notice=outlook_cancelled"
                .parse::<Route>()
                .unwrap(),
            Route::Calendar {
                view: None,
                date: None,
                notice: Some("outlook_cancelled".into())
            }
        );
    }

    #[test]
    fn week_grid_gives_overlapping_events_separate_lanes() {
        let event = |start, end| CalendarViewEvent {
            minutes: Some((start, end)),
            title: "Event".into(),
            when: String::new(),
        };
        let events = [
            event(540, 630),
            event(570, 600),
            event(600, 660),
            event(720, 750),
        ];
        let placement: Vec<_> = scheduled_blocks(&events)
            .iter()
            .map(|block| (block.lane, block.lanes))
            .collect();
        assert_eq!(placement, [(0, 2), (1, 2), (1, 2), (0, 1)]);
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
                Some(Ok(ConnectionStatus::Connected { selected, shared, choices })) => {
                    rsx! {
                        CalendarSharing {
                            provider,
                            selected: selected.clone(),
                            shared: *shared,
                            choices: choices.clone(),
                        }
                        ConnectButton { provider, reconnect: true }
                        form { action: "/calendar/{key}/disconnect", method: "post",
                            button { class: "undo-button", r#type: "submit", "Disconnect {name}" }
                        }
                    }
                }
                Some(Ok(ConnectionStatus::Disconnected)) => rsx! {
                    ConnectButton { provider, reconnect: false }
                },
                Some(Ok(ConnectionStatus::Unavailable)) => rsx! {
                    p { "{name} linking is unavailable on this server." }
                },
                Some(Err(_)) => rsx! {
                    p { role: "alert", "Could not check your calendar connection. Reload to try again." }
                },
                None => rsx! {
                    p { "Checking your calendar connection…" }
                },
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
                    select {
                        id: "selected-calendar-{key}",
                        name: "calendar_id",
                        required: true,
                        option {
                            value: "",
                            disabled: true,
                            selected: !selected_is_available,
                            "Choose a calendar"
                        }
                        for calendar in calendars {
                            option {
                                key: "{calendar.id}",
                                value: "{calendar.id}",
                                selected: Some(&calendar.id) == selected.as_ref(),
                                "{calendar.name}"
                            }
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
                    button {
                        class: "undo-button",
                        r#type: "submit",
                        name: "shared",
                        value: "false",
                        "Stop sharing"
                    }
                } else {
                    p {
                        "Share this calendar to show event titles and times to signed-in household members. Events marked private appear as Busy."
                    }
                    button {
                        class: "primary-button",
                        r#type: "submit",
                        name: "shared",
                        value: "true",
                        "Share with household"
                    }
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
