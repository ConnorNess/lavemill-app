use super::{
    connection, database_error, run_blocking, Connection, GoogleCalendar, GoogleError, Url,
};
use crate::{
    calendar::{Agenda, AgendaDay, AgendaEntry},
    household_time,
};
use chrono::{DateTime, Datelike, Days, NaiveDate, TimeDelta, TimeZone, Utc};
use chrono_tz::Tz;
use serde::Deserialize;
use std::time::Duration;
use tokio::task::JoinSet;

#[derive(PartialEq)]
struct SharedCalendar {
    user: i64,
    id: String,
    token: Vec<u8>,
}

#[derive(Clone, Copy)]
struct Week {
    zone: Tz,
    today: NaiveDate,
    first: NaiveDate,
    until: NaiveDate,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EventPage {
    #[serde(default)]
    items: Vec<GoogleEvent>,
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
struct GoogleEvent {
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

impl GoogleCalendar {
    pub async fn weekly_agenda(&self) -> Result<Agenda, &'static str> {
        let zone = household_time::time_zone()?;
        let week = Week::new(zone, Utc::now().with_timezone(&zone).date_naive());
        let (start, end) = week
            .bounds()
            .ok_or("Could not determine the calendar week.")?;
        // Google's all-day query boundaries use each source calendar's time zone.
        // Include the largest possible offset difference and filter by date below.
        let start = (start - TimeDelta::hours(26)).to_rfc3339();
        let end = (end + TimeDelta::hours(26)).to_rfc3339();
        let sources = run_blocking(|| list_shared(&connection()?)).await?;
        let mut requests = JoinSet::new();
        for source in sources {
            let google = self.clone();
            let (start, end) = (start.clone(), end.clone());
            requests.spawn(async move {
                let result = tokio::time::timeout(
                    Duration::from_secs(10),
                    google.weekly_events(&source, &start, &end),
                )
                .await
                .unwrap_or(Err(GoogleError::Unavailable));
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
        // Check sharing again after Google responds so an in-flight request can
        // drop results for connections that were changed or disconnected.
        let current = run_blocking(|| list_shared(&connection()?)).await?;
        let mut entries = Vec::new();
        for source in &current {
            match results.iter_mut().find(|(queried, _)| queried == source) {
                Some((_, Ok(events))) => entries.extend(
                    events
                        .drain(..)
                        .filter_map(|event| week.agenda_entry(event)),
                ),
                _ => incomplete = true,
            }
        }
        Ok(Agenda {
            has_shared_calendars: !current.is_empty(),
            incomplete,
            days: week.group_entries(entries),
        })
    }

    async fn weekly_events(
        &self,
        source: &SharedCalendar,
        start: &str,
        end: &str,
    ) -> Result<Vec<GoogleEvent>, GoogleError> {
        let access_token = self.access_token(source.user, &source.token).await?;
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
                .map_err(|_| GoogleError::Unavailable)?
                .json()
                .await
                .map_err(|_| GoogleError::Unavailable)?;
            events.extend(page.items);
            page_token = page.next_page_token;
            if page_token.is_none() {
                return Ok(events);
            }
        }
    }
}

fn list_shared(db: &Connection) -> Result<Vec<SharedCalendar>, &'static str> {
    let mut statement = db
        .prepare(
            "SELECT user_id, calendar_id, refresh_token FROM google_calendar_connections
             WHERE shared = 1",
        )
        .map_err(database_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok(SharedCalendar {
                user: row.get(0)?,
                id: row.get(1)?,
                token: row.get(2)?,
            })
        })
        .map_err(database_error)?;
    rows.collect::<rusqlite::Result<_>>()
        .map_err(database_error)
}

impl Week {
    fn new(zone: Tz, today: NaiveDate) -> Self {
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

    fn agenda_entry(self, event: GoogleEvent) -> Option<DatedEntry> {
        if event.status.as_deref() == Some("cancelled") {
            return None;
        }
        let title = if matches!(
            event.visibility.as_deref(),
            Some("private" | "confidential")
        ) {
            "Busy".to_owned()
        } else {
            event
                .summary
                .filter(|title| !title.trim().is_empty())
                .unwrap_or_else(|| "Busy".to_owned())
        };
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

    fn event(start: &str, end: &str, all_day: bool) -> GoogleEvent {
        let time = |value: &str| EventTime {
            date: all_day.then(|| value.to_owned()),
            date_time: (!all_day).then(|| value.to_owned()),
        };
        GoogleEvent {
            summary: Some("Family event".into()),
            visibility: None,
            status: Some("confirmed".into()),
            start: Some(time(start)),
            end: Some(time(end)),
        }
    }

    #[test]
    fn agenda_redacts_private_titles_and_respects_week_boundaries_and_dst() {
        let week = Week::new(chrono_tz::Europe::London, date("2026-03-29"));
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
                "INSERT INTO google_calendar_connections (user_id, refresh_token, calendar_id, shared)
                 VALUES (?1, X'0102', ?2, ?3)",
                params![user, calendar, shared],
            )
            .unwrap();
        }
        let shared = list_shared(&db).unwrap();
        assert_eq!(shared.len(), 1);
        assert_eq!(shared[0].id, "family");
        assert!(super::super::set_shared(&db, 1, "family", false).unwrap());
        assert!(list_shared(&db).unwrap().is_empty());
    }
}
