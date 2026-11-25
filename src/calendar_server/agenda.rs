use super::{
    connection, database_error, run_blocking, CalendarClient, CalendarClients, Connection,
    ProviderError, Url,
};
use crate::{
    calendar::{
        Agenda, AgendaDay, AgendaEntry, CalendarDay, CalendarViewData, CalendarViewEvent, Provider,
    },
    household_time,
};
use chrono::{
    DateTime, Datelike, Days, Months, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta, TimeZone,
    Timelike, Utc,
};
use chrono_tz::Tz;
use serde::Deserialize;
use std::time::Duration;
use tokio::task::JoinSet;

struct SharedCalendar {
    provider: Provider,
    user: i64,
    id: String,
    token: Vec<u8>,
    connection_id: Vec<u8>,
}

#[derive(Clone, Copy)]
struct DateRange {
    zone: Tz,
    today: NaiveDate,
    first: NaiveDate,
    until: NaiveDate,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EventPage {
    #[serde(default)]
    items: Vec<CalendarEvent>,
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
struct CalendarEvent {
    summary: Option<String>,
    visibility: Option<String>,
    status: Option<String>,
    start: Option<EventTime>,
    end: Option<EventTime>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EventTime {
    date: Option<String>,
    date_time: Option<String>,
}

struct DatedEntry {
    day: NaiveDate,
    start: Option<DateTime<Tz>>,
    entry: AgendaEntry,
}

impl SharedCalendar {
    fn same_share(&self, current: &Self) -> bool {
        self.provider == current.provider
            && self.user == current.user
            && self.id == current.id
            && self.connection_id == current.connection_id
    }
}

struct SharedEvents {
    has_shared_calendars: bool,
    incomplete: bool,
    events: Vec<CalendarEvent>,
}

impl CalendarClients {
    fn has_calendar_provider(&self) -> bool {
        self.google.is_some() || self.outlook.is_some()
    }

    pub async fn weekly_agenda(&self) -> Result<Option<Agenda>, &'static str> {
        if !self.has_calendar_provider() {
            return Ok(None);
        }
        let zone = household_time::time_zone()?;
        let week = DateRange::week(zone, Utc::now().with_timezone(&zone).date_naive());
        Ok(Some(self.shared_events(week).await?.agenda(week)))
    }

    pub async fn calendar_view(
        &self,
        month: bool,
        date: Option<&str>,
    ) -> Result<CalendarViewData, &'static str> {
        let zone = household_time::time_zone()?;
        let today = Utc::now().with_timezone(&zone).date_naive();
        let anchor = date
            .and_then(|value| value.parse::<NaiveDate>().ok())
            .filter(|value| (1..=9998).contains(&value.year()))
            .unwrap_or(today);
        let period = ViewPeriod::new(zone, today, anchor, month);
        let mut view = period.data;
        view.configured = self.has_calendar_provider();
        if view.configured {
            let events = self.shared_events(period.range).await?;
            view.has_shared_calendars = events.has_shared_calendars;
            view.incomplete = events.incomplete;
            fill_view_days(&mut view.days, period.range, events.events);
        }
        Ok(view)
    }

    async fn shared_events(&self, range: DateRange) -> Result<SharedEvents, &'static str> {
        let (start, end) = range
            .bounds()
            .ok_or("Could not determine the calendar date range.")?;
        // Google's all-day query boundaries use each source calendar's time zone.
        // Include the largest possible offset difference and filter by date below.
        let start = (start - TimeDelta::hours(26)).to_rfc3339();
        let end = (end + TimeDelta::hours(26)).to_rfc3339();
        let sources = run_blocking(|| list_shared(&connection()?)).await?;
        let mut requests = JoinSet::new();
        for source in sources {
            let Some(client) = self.get(source.provider).cloned() else {
                continue;
            };
            let (start, end) = (start.clone(), end.clone());
            requests.spawn(async move {
                let result = tokio::time::timeout(
                    Duration::from_secs(10),
                    client.events_in_range(&source, &start, &end),
                )
                .await
                .unwrap_or(Err(ProviderError::Unavailable));
                (source, result)
            });
        }
        let mut results = Vec::new();
        let mut incomplete = false;
        while let Some(result) = requests.join_next().await {
            match result {
                Ok(result) => results.push(result),
                Err(_) => incomplete = true,
            }
        }
        // Check sharing again after the providers respond so an in-flight request can
        // drop results for connections that were changed or disconnected.
        let mut current = run_blocking(|| list_shared(&connection()?)).await?;
        current.retain(|source| self.get(source.provider).is_some());
        let mut shared = collect_shared_events(current, results);
        shared.incomplete |= incomplete && shared.has_shared_calendars;
        Ok(shared)
    }
}

impl SharedEvents {
    fn agenda(self, week: DateRange) -> Agenda {
        let entries = self
            .events
            .into_iter()
            .filter_map(|event| week.agenda_entry(event))
            .collect();
        Agenda {
            has_shared_calendars: self.has_shared_calendars,
            incomplete: self.incomplete,
            days: week.group_entries(entries),
        }
    }
}

fn collect_shared_events(
    current: Vec<SharedCalendar>,
    mut results: Vec<(SharedCalendar, Result<Vec<CalendarEvent>, ProviderError>)>,
) -> SharedEvents {
    let mut events = Vec::new();
    let mut incomplete = false;
    for source in &current {
        match results
            .iter_mut()
            .find(|(queried, _)| queried.same_share(source))
        {
            Some((_, Ok(found))) => events.append(found),
            _ => incomplete = true,
        }
    }
    SharedEvents {
        has_shared_calendars: !current.is_empty(),
        incomplete,
        events,
    }
}

#[cfg(test)]
fn assemble_agenda(
    week: DateRange,
    current: Vec<SharedCalendar>,
    results: Vec<(SharedCalendar, Result<Vec<CalendarEvent>, ProviderError>)>,
) -> Agenda {
    collect_shared_events(current, results).agenda(week)
}

impl CalendarClient {
    async fn events_in_range(
        &self,
        source: &SharedCalendar,
        start: &str,
        end: &str,
    ) -> Result<Vec<CalendarEvent>, ProviderError> {
        let access_token = self
            .access_token(source.user, &source.connection_id, &source.token)
            .await?;
        if source.provider == Provider::Outlook {
            let mut url = Url::parse("https://graph.microsoft.com/v1.0/me/calendars/")
                .expect("Graph calendar URL is valid");
            url.path_segments_mut()
                .expect("Graph supports path segments")
                .pop_if_empty()
                .push(&source.id)
                .push("calendarView");
            url.query_pairs_mut().extend_pairs([
                ("startDateTime", start),
                ("endDateTime", end),
                ("$select", "subject,sensitivity,isCancelled,isAllDay,start,end,originalStartTimeZone,originalEndTimeZone"),
            ]);
            let events = self
                .graph_collection::<OutlookEvent>(url, &access_token)
                .await?;
            return Ok(events
                .into_iter()
                .filter_map(OutlookEvent::normalize)
                .collect());
        }

        let mut url = Url::parse("https://www.googleapis.com/calendar/v3/calendars/")
            .expect("Google Calendar API URL is valid");
        url.path_segments_mut()
            .expect("Google Calendar URL supports path segments")
            .pop_if_empty()
            .push(&source.id)
            .push("events");
        let mut events = Vec::new();
        let mut page_token = None;
        loop {
            let mut request = self
                .0
                .http
                .get(url.clone())
                .bearer_auth(&access_token)
                .query(&[
                    ("timeMin", start),
                    ("timeMax", end),
                    ("singleEvents", "true"),
                    ("showDeleted", "false"),
                    (
                        "fields",
                        "items(summary,visibility,status,start,end),nextPageToken",
                    ),
                ]);
            if let Some(token) = page_token {
                request = request.query(&[("pageToken", token)]);
            }
            let page: EventPage = request
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
                .map_err(|_| ProviderError::Unavailable)?
                .json()
                .await
                .map_err(|_| ProviderError::Unavailable)?;
            events.extend(page.items);
            page_token = page.next_page_token;
            if page_token.is_none() {
                return Ok(events);
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OutlookEvent {
    subject: Option<String>,
    sensitivity: Option<String>,
    is_cancelled: bool,
    is_all_day: bool,
    start: Option<OutlookTime>,
    end: Option<OutlookTime>,
    original_start_time_zone: Option<String>,
    original_end_time_zone: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OutlookTime {
    date_time: String,
    time_zone: String,
}

impl OutlookEvent {
    fn normalize(self) -> Option<CalendarEvent> {
        if self.is_cancelled {
            return None;
        }
        let start = self.start?.utc()?;
        let end = self.end?.utc()?;
        let normalize_time = |time: DateTime<Utc>, original_zone: Option<&str>| {
            if self.is_all_day {
                Some(EventTime {
                    date: Some(outlook_date(time, original_zone)?.to_string()),
                    date_time: None,
                })
            } else {
                Some(EventTime {
                    date: None,
                    date_time: Some(time.to_rfc3339()),
                })
            }
        };
        Some(CalendarEvent {
            summary: self.subject,
            visibility: (self.sensitivity.as_deref() != Some("normal")).then(|| "private".into()),
            status: None,
            start: Some(normalize_time(
                start,
                self.original_start_time_zone.as_deref(),
            )?),
            end: Some(normalize_time(
                end,
                self.original_end_time_zone
                    .as_deref()
                    .or(self.original_start_time_zone.as_deref()),
            )?),
        })
    }
}

impl OutlookTime {
    fn utc(self) -> Option<DateTime<Utc>> {
        if let Ok(with_offset) = DateTime::parse_from_rfc3339(&self.date_time) {
            return Some(with_offset.with_timezone(&Utc));
        }
        if !self.time_zone.eq_ignore_ascii_case("UTC") {
            return None;
        }
        NaiveDateTime::parse_from_str(&self.date_time, "%Y-%m-%dT%H:%M:%S%.f")
            .ok()
            .map(|time| time.and_utc())
    }
}

fn outlook_date(utc: DateTime<Utc>, original_zone: Option<&str>) -> Option<NaiveDate> {
    if utc.time() == NaiveTime::MIN {
        return Some(utc.date_naive());
    }
    let zone = original_zone?;
    let zone = zone.parse::<Tz>().ok().or_else(|| {
        include_str!("windows_zones.tsv")
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .find(|(name, _)| name.eq_ignore_ascii_case(zone))
            .and_then(|(_, iana)| iana.parse().ok())
    })?;
    Some(utc.with_timezone(&zone).date_naive())
}

fn list_shared(db: &Connection) -> Result<Vec<SharedCalendar>, &'static str> {
    let mut statement = db
        .prepare(
            "SELECT user_id, calendar_id, refresh_token, id, provider = 'outlook' FROM calendar_connections
             WHERE shared = 1",
        )
        .map_err(database_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok(SharedCalendar {
                user: row.get(0)?,
                id: row.get(1)?,
                token: row.get(2)?,
                connection_id: row.get(3)?,
                provider: if row.get::<_, bool>(4)? {
                    Provider::Outlook
                } else {
                    Provider::Google
                },
            })
        })
        .map_err(database_error)?;
    rows.collect::<rusqlite::Result<_>>()
        .map_err(database_error)
}

impl CalendarEvent {
    fn visible_title(&self) -> Option<String> {
        if self.status.as_deref() == Some("cancelled") {
            return None;
        }
        let title = if matches!(self.visibility.as_deref(), Some("private" | "confidential")) {
            None
        } else {
            self.summary
                .as_ref()
                .filter(|title| !title.trim().is_empty())
        };
        Some(title.cloned().unwrap_or_else(|| "Busy".to_owned()))
    }
}

struct ViewPeriod {
    range: DateRange,
    data: CalendarViewData,
}

impl ViewPeriod {
    fn new(zone: Tz, today: NaiveDate, anchor: NaiveDate, month: bool) -> Self {
        let monday =
            |date: NaiveDate| date - Days::new(date.weekday().num_days_from_monday().into());
        let (first, until, previous, next, label) = if month {
            let month_start = anchor.with_day(1).expect("Every month has a first day");
            let next_month = month_start + Months::new(1);
            let month_end = next_month - Days::new(1);
            (
                monday(month_start),
                monday(month_end) + Days::new(7),
                anchor - Months::new(1),
                anchor + Months::new(1),
                month_start.format("%B %Y").to_string(),
            )
        } else {
            let first = monday(anchor);
            let last = first + Days::new(6);
            (
                first,
                last + Days::new(1),
                anchor - Days::new(7),
                anchor + Days::new(7),
                format!("{} -> {}", first.format("%-d %b"), last.format("%-d %b %Y")),
            )
        };
        let days = first
            .iter_days()
            .take_while(|day| *day < until)
            .map(|day| CalendarDay {
                date: day.to_string(),
                weekday: day.format("%a").to_string(),
                number: day.day(),
                label: day.format("%A %-d %B %Y").to_string(),
                outside_month: month && day.month() != anchor.month(),
                events: Vec::new(),
            })
            .collect();
        Self {
            range: DateRange {
                zone,
                today,
                first,
                until,
            },
            data: CalendarViewData {
                month,
                date: anchor.to_string(),
                today: today.to_string(),
                previous: previous.to_string(),
                next: next.to_string(),
                label,
                configured: false,
                has_shared_calendars: false,
                incomplete: false,
                days,
            },
        }
    }
}

fn fill_view_days(days: &mut [CalendarDay], range: DateRange, events: Vec<CalendarEvent>) {
    let minute = |time: DateTime<Tz>| time.num_seconds_from_midnight() / 60;
    for event in events {
        let Some(title) = event.visible_title() else {
            continue;
        };
        let (Some(start), Some(end)) = (event.start, event.end) else {
            continue;
        };
        if let (Some(start), Some(end)) = (start.date, end.date) {
            let (Ok(start), Ok(end)) = (start.parse::<NaiveDate>(), end.parse::<NaiveDate>())
            else {
                continue;
            };
            for day in start
                .max(range.first)
                .iter_days()
                .take_while(|day| *day < end.min(range.until))
            {
                let index = (day - range.first).num_days() as usize;
                days[index].events.push(CalendarViewEvent {
                    title: title.clone(),
                    when: "All day".into(),
                    minutes: None,
                });
            }
        } else if let (Some(start), Some(end)) = (start.date_time, end.date_time) {
            let (Ok(start), Ok(end)) = (
                DateTime::parse_from_rfc3339(&start),
                DateTime::parse_from_rfc3339(&end),
            ) else {
                continue;
            };
            if end < start {
                continue;
            }
            let (start, end) = (
                start.with_timezone(&range.zone),
                end.with_timezone(&range.zone),
            );
            for day in start
                .date_naive()
                .max(range.first)
                .iter_days()
                .take_while(|day| *day <= end.date_naive() && *day < range.until)
            {
                let starts_today = start.date_naive() == day;
                let ends_today = end.date_naive() == day;
                if !starts_today && ends_today && end.time() == NaiveTime::MIN {
                    continue;
                }
                let first = if starts_today { minute(start) } else { 0 };
                let until = if ends_today { minute(end) } else { 1440 };
                let when = match (starts_today, ends_today) {
                    (true, true) if start == end => start.format("%H:%M").to_string(),
                    (true, true) => format!("{}–{}", start.format("%H:%M"), end.format("%H:%M")),
                    (true, false) => format!("{} onward", start.format("%H:%M")),
                    (false, true) => format!("Until {}", end.format("%H:%M")),
                    (false, false) => "Continues all day".into(),
                };
                let index = (day - range.first).num_days() as usize;
                days[index].events.push(CalendarViewEvent {
                    title: title.clone(),
                    when,
                    minutes: Some((first, until.max(first + 1))),
                });
            }
        }
    }
    for day in days {
        day.events
            .sort_by(|a, b| (a.minutes, &a.title).cmp(&(b.minutes, &b.title)));
    }
}

impl DateRange {
    fn week(zone: Tz, today: NaiveDate) -> Self {
        let first = today - Days::new(today.weekday().num_days_from_monday().into());
        let until = first + Days::new(7);
        Self {
            zone,
            today,
            first,
            until,
        }
    }

    fn bounds(self) -> Option<(DateTime<Tz>, DateTime<Tz>)> {
        let start = |date: NaiveDate| {
            let midnight = date.and_hms_opt(0, 0, 0)?;
            (0..=24 * 60).find_map(|minute| {
                self.zone
                    .from_local_datetime(&(midnight + TimeDelta::minutes(minute)))
                    .earliest()
            })
        };
        Some((start(self.first)?, start(self.until)?))
    }

    fn agenda_entry(self, event: CalendarEvent) -> Option<DatedEntry> {
        let title = event.visible_title()?;
        let (start, end) = (event.start?, event.end?);
        if let (Some(start), Some(end)) = (start.date, end.date) {
            let (start, end): (NaiveDate, NaiveDate) = (start.parse().ok()?, end.parse().ok()?);
            if start >= self.until || end <= self.first || start >= end {
                return None;
            }
            let last = end.pred_opt()?;
            let when = if last > start {
                format!("All day through {}", last.format("%-d %b"))
            } else {
                "All day".to_owned()
            };
            return Some(DatedEntry {
                day: start.max(self.first),
                start: None,
                entry: AgendaEntry { title, when },
            });
        }

        let start = DateTime::parse_from_rfc3339(start.date_time.as_deref()?)
            .ok()?
            .with_timezone(&self.zone);
        let end = DateTime::parse_from_rfc3339(end.date_time.as_deref()?)
            .ok()?
            .with_timezone(&self.zone);
        let day = start.date_naive().max(self.first);
        if day >= self.until
            || end < start
            || end.date_naive() < self.first
            || (end.date_naive() == self.first
                && end.time() == chrono::NaiveTime::MIN
                && start.date_naive() < self.first)
        {
            return None;
        }
        let when = if start.date_naive() < day {
            format!("Ends {}", end.format("%-d %b at %H:%M"))
        } else if end.date_naive() != day {
            format!(
                "{} to {}",
                start.format("%H:%M"),
                end.format("%-d %b at %H:%M")
            )
        } else if end == start {
            start.format("%H:%M").to_string()
        } else {
            format!("{}–{}", start.format("%H:%M"), end.format("%H:%M"))
        };
        Some(DatedEntry {
            day,
            start: Some(start),
            entry: AgendaEntry { title, when },
        })
    }

    fn group_entries(self, mut entries: Vec<DatedEntry>) -> Vec<AgendaDay> {
        entries.sort_by(|a, b| {
            (a.day, a.start, &a.entry.title).cmp(&(b.day, b.start, &b.entry.title))
        });
        let mut days: Vec<AgendaDay> = Vec::new();
        for item in entries {
            let date = item.day.format("%-d %b").to_string();
            if let Some(day) = days.last_mut().filter(|day| day.date == date) {
                day.entries.push(item.entry);
            } else {
                let label = if item.day == self.today {
                    "Today".to_owned()
                } else {
                    item.day.format("%A").to_string()
                };
                days.push(AgendaDay {
                    date,
                    label,
                    entries: vec![item.entry],
                });
            }
        }
        days
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(value: &str) -> NaiveDate {
        value.parse().unwrap()
    }

    fn event(start: &str, end: &str, all_day: bool) -> CalendarEvent {
        let time = |value: &str| EventTime {
            date: all_day.then(|| value.to_owned()),
            date_time: (!all_day).then(|| value.to_owned()),
        };
        CalendarEvent {
            summary: Some("Family event".into()),
            visibility: None,
            status: Some("confirmed".into()),
            start: Some(time(start)),
            end: Some(time(end)),
        }
    }

    #[test]
    fn calendar_month_includes_leap_day_and_full_monday_first_weeks() {
        let zone = chrono_tz::Europe::London;
        let period = ViewPeriod::new(zone, date("2024-02-15"), date("2024-02-15"), true);
        assert_eq!(period.data.days.first().unwrap().date, "2024-01-29");
        assert_eq!(period.data.days.last().unwrap().date, "2024-03-03");
        assert!(period.data.days[0].outside_month);
        assert!(period
            .data
            .days
            .iter()
            .any(|day| day.date == "2024-02-29" && !day.outside_month));
        let december = ViewPeriod::new(zone, date("2024-02-15"), date("2024-12-31"), true);
        assert_eq!(december.data.next, "2025-01-31");
    }

    #[test]
    fn calendar_view_segments_overnight_events_and_preserves_privacy_on_dst_week() {
        let mut period = ViewPeriod::new(
            chrono_tz::Europe::London,
            date("2026-03-29"),
            date("2026-03-29"),
            false,
        );
        assert_eq!(period.data.label, "23 Mar -> 29 Mar 2026");
        let mut private = event("2026-03-28T23:30:00Z", "2026-03-29T02:00:00Z", false);
        private.visibility = Some("private".into());
        let all_day = event("2026-03-28", "2026-03-30", true);
        fill_view_days(&mut period.data.days, period.range, vec![private, all_day]);
        let weekend = &period.data.days[5..];
        assert_eq!(weekend[0].events[0].minutes, None);
        assert_eq!(weekend[0].events[1].minutes, Some((1410, 1440)));
        assert_eq!(weekend[1].events[1].minutes, Some((0, 180)));
        assert_eq!(weekend[1].events[1].title, "Busy");
        assert!(period.data.days[0].events.is_empty());
    }

    #[test]
    fn agenda_redacts_private_titles_and_respects_week_boundaries_and_dst() {
        let week = DateRange::week(chrono_tz::Europe::London, date("2026-03-29"));
        let (start, end) = week.bounds().unwrap();
        assert_eq!(start.to_rfc3339(), "2026-03-23T00:00:00+00:00");
        assert_eq!(end.to_rfc3339(), "2026-03-30T00:00:00+01:00");

        let mut private = event("2026-03-29T10:30:00Z", "2026-03-29T11:30:00Z", false);
        private.visibility = Some("private".into());
        let private = week.agenda_entry(private).unwrap();
        assert_eq!(private.entry.title, "Busy");
        assert_eq!(private.entry.when, "11:30–12:30");

        let all_day = week
            .agenda_entry(event("2026-03-29", "2026-03-31", true))
            .unwrap();
        assert_eq!(all_day.day, date("2026-03-29"));
        assert_eq!(all_day.entry.when, "All day through 30 Mar");
        let days = week.group_entries(vec![private, all_day]);
        assert_eq!(days[0].label, "Today");
        assert_eq!(days[0].entries[0].title, "Family event");
        assert_eq!(days[0].entries[1].title, "Busy");

        let mut cancelled = event("2026-03-29", "2026-03-30", true);
        cancelled.status = Some("cancelled".into());
        assert!(week.agenda_entry(cancelled).is_none());
        assert!(week
            .agenda_entry(event("2026-03-22", "2026-03-23", true))
            .is_none());
        assert!(week
            .agenda_entry(event("2026-03-30", "2026-03-31", true))
            .is_none());
        assert!(week
            .agenda_entry(event("2026-03-29T23:30:00Z", "2026-03-30T01:00:00Z", false))
            .is_none());
        assert!(week
            .agenda_entry(event("2026-03-22T23:30:00Z", "2026-03-23T00:00:00Z", false))
            .is_none());
    }

    #[test]
    fn household_agenda_reads_only_calendars_that_are_currently_shared() {
        use crate::database;
        use rusqlite::params;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("calendar.sqlite3");
        database::initialize(&path).unwrap();
        let db = database::open(&path).unwrap();
        for (user, calendar, shared) in [(1, "family", true), (2, "personal", false)] {
            db.execute(
                "INSERT INTO users (id, username, first_name, last_name, password_hash)
                 VALUES (?1, 'user' || ?1, 'Household', 'Member', 'unused')",
                [user],
            )
            .unwrap();
            db.execute(
                "INSERT INTO calendar_connections (user_id, refresh_token, calendar_id, shared, provider)
                 VALUES (?1, X'0102', ?2, ?3, 'google')",
                params![user, calendar, shared],
            )
            .unwrap();
        }
        let shared = list_shared(&db).unwrap();
        assert_eq!(shared.len(), 1);
        assert_eq!(shared[0].id, "family");
        assert!(super::super::set_shared(&db, "google", 1, "family", false).unwrap());
        assert!(list_shared(&db).unwrap().is_empty());
    }

    #[test]
    fn outlook_events_preserve_all_day_dates_and_redact_sensitive_subjects() {
        let week = DateRange::week(chrono_tz::Europe::London, date("2026-03-29"));
        let outlook_time = |date_time: &str| OutlookTime {
            date_time: date_time.to_owned(),
            time_zone: "UTC".into(),
        };
        let mut event = OutlookEvent {
            subject: Some("Personal plans".into()),
            sensitivity: Some("personal".into()),
            is_cancelled: false,
            is_all_day: true,
            start: Some(outlook_time("2026-03-28T23:00:00.0000000")),
            end: Some(outlook_time("2026-03-29T22:00:00.0000000")),
            original_start_time_zone: Some("W. Europe Standard Time".into()),
            original_end_time_zone: Some("W. Europe Standard Time".into()),
        };
        let entry = week.agenda_entry(event.normalize().unwrap()).unwrap();
        assert_eq!(entry.day, date("2026-03-29"));
        assert_eq!(entry.entry.title, "Busy");
        assert_eq!(entry.entry.when, "All day");

        event = OutlookEvent {
            subject: Some("Recurring appointment".into()),
            sensitivity: Some("normal".into()),
            is_cancelled: false,
            is_all_day: false,
            start: Some(outlook_time("2026-03-29T10:00:00.0000000")),
            end: Some(outlook_time("2026-03-29T11:00:00.0000000")),
            original_start_time_zone: None,
            original_end_time_zone: None,
        };
        let entry = week.agenda_entry(event.normalize().unwrap()).unwrap();
        assert_eq!(entry.entry.title, "Recurring appointment");
        assert_eq!(entry.entry.when, "11:00–12:00");
    }

    #[test]
    fn shared_agenda_keeps_healthy_results_and_drops_unshared_or_replaced_connections() {
        let week = DateRange::week(chrono_tz::Europe::London, date("2026-03-29"));
        let source = |provider, generation, token| SharedCalendar {
            provider,
            user: 1,
            id: "family".into(),
            token: vec![token],
            connection_id: vec![generation],
        };
        let family_event = || vec![event("2026-03-29", "2026-03-30", true)];
        let google = || source(Provider::Google, 1, 1);
        let outlook = || source(Provider::Outlook, 2, 1);
        let results = || {
            vec![
                (google(), Ok(family_event())),
                (outlook(), Ok(family_event())),
            ]
        };

        let agenda = assemble_agenda(week, vec![google(), outlook()], results());
        assert_eq!(agenda.days[0].entries.len(), 2);
        assert!(!agenda.incomplete);

        let agenda = assemble_agenda(week, vec![google()], results());
        assert_eq!(agenda.days[0].entries.len(), 1);
        assert!(!agenda.incomplete);

        let rotated = source(Provider::Outlook, 2, 9);
        let agenda = assemble_agenda(week, vec![rotated], results());
        assert_eq!(agenda.days[0].entries.len(), 1);
        assert!(!agenda.incomplete);

        let reconnected = source(Provider::Outlook, 3, 9);
        let agenda = assemble_agenda(week, vec![google(), reconnected], results());
        assert_eq!(agenda.days[0].entries.len(), 1);
        assert!(agenda.incomplete);

        let agenda = assemble_agenda(
            week,
            vec![google(), outlook()],
            vec![
                (google(), Ok(family_event())),
                (outlook(), Err(ProviderError::Unavailable)),
            ],
        );
        assert_eq!(agenda.days[0].entries.len(), 1);
        assert!(agenda.incomplete);
    }
}
