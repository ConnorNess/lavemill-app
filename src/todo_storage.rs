use crate::accounts::database_error;
use crate::todos::{Change, Member, Recurrence, Repeat, Task, Tasks};
use chrono::{Datelike, Days, Months, NaiveDate, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

pub fn household_date() -> Result<NaiveDate, &'static str> {
    let zone = crate::household_time::time_zone()?;
    Ok(Utc::now().with_timezone(&zone).date_naive())
}

pub fn list(db: &mut Connection, today: NaiveDate) -> Result<Tasks, &'static str> {
    let transaction = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(database_error)?;
    let today = today.to_string();
    catch_up(&transaction, &today)?;
    let tasks = read_tasks(&transaction, today, true)?;
    transaction.commit().map_err(database_error)?;
    Ok(tasks)
}

pub fn snapshot(db: &Connection, today: NaiveDate) -> Result<Tasks, &'static str> {
    read_tasks(db, today.to_string(), false)
}

fn read_tasks(
    db: &Connection,
    today: String,
    include_members: bool,
) -> Result<Tasks, &'static str> {
    let mut statement = db
        .prepare(
            "SELECT t.id, t.title, t.description, t.due, t.assignee_id,
                reporter.first_name || ' ' || reporter.last_name,
                assignee.first_name || ' ' || assignee.last_name, assignee.first_name, t.status,
                t.status = 'completed' AND COALESCE(s.retired, 0) = 0 AND (t.next_due IS NULL OR (
                    t.next_due > ?1 AND EXISTS (
                        SELECT 1 FROM todos successor
                        WHERE successor.series_id = t.series_id AND successor.due = t.next_due
                    )
                )), s.frequency, s.usual_assignee_id
         FROM todos t
         JOIN users reporter ON reporter.id = t.reporter_id
         LEFT JOIN users assignee ON assignee.id = t.assignee_id
         LEFT JOIN todo_series s ON s.id = t.series_id
         WHERE t.status <> 'failed' OR t.missed_on IS NULL OR t.missed_on > date(?1, '-7 days')
         ORDER BY t.due, t.id",
        )
        .map_err(database_error)?;
    let tasks = statement
        .query_map([&today], |row| {
            let frequency: Option<String> = row.get(10)?;
            let usual_assignee_id = row.get(11)?;
            Ok(Task {
                id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                due: row.get(3)?,
                assignee_id: row.get(4)?,
                reporter: row.get(5)?,
                assignee: row.get(6)?,
                assignee_first_name: row.get(7)?,
                status: row.get(8)?,
                can_reopen: row.get(9)?,
                recurrence: frequency.map(|value| Recurrence {
                    frequency: Repeat::from_frequency(&value),
                    usual_assignee_id,
                }),
            })
        })
        .map_err(database_error)?
        .collect::<Result<_, _>>()
        .map_err(database_error)?;
    let mut members = Vec::new();
    if include_members {
        let mut statement = db
            .prepare("SELECT id, first_name FROM users ORDER BY first_name, last_name, username")
            .map_err(database_error)?;
        for member in statement
            .query_map([], |row| {
                Ok(Member {
                    id: row.get(0)?,
                    name: row.get(1)?,
                })
            })
            .map_err(database_error)?
        {
            members.push(member.map_err(database_error)?);
        }
    }
    Ok(Tasks {
        today,
        tasks,
        members,
    })
}

pub fn apply(
    db: &mut Connection,
    today: NaiveDate,
    actor: i64,
    change: Change,
) -> Result<(), &'static str> {
    let mut transaction = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(database_error)?;
    let today = today.to_string();
    catch_up(&transaction, &today)?;
    let savepoint = transaction.savepoint().map_err(database_error)?;
    let result = apply_change(&savepoint, &today, actor, change);
    if result.is_ok() {
        savepoint.commit().map_err(database_error)?;
    } else {
        savepoint.finish().map_err(database_error)?;
    }
    transaction.commit().map_err(database_error)?;
    result
}

fn apply_change(
    db: &Connection,
    today: &str,
    actor: i64,
    change: Change,
) -> Result<(), &'static str> {
    let changed = match change {
        Change::Create {
            title,
            description,
            due,
            assignee,
            repeat,
        } => {
            return create_task(
                db,
                today,
                actor,
                NewTask {
                    title,
                    description,
                    due,
                    assignee,
                    repeat,
                },
            );
        }
        Change::EditRepeat {
            id,
            title,
            description,
            due,
            assignee,
            repeat,
        } => {
            return edit_repeat(
                db,
                today,
                id,
                NewTask {
                    title,
                    description,
                    due,
                    assignee,
                    repeat,
                },
            );
        }
        Change::DeleteRepeat { id } => return delete_repeat(db, id),
        Change::Rename { id, title } => db.execute(
            "UPDATE todos SET title = ?2
             WHERE id = ?1 AND status = 'open' AND series_id IS NULL",
            params![id, validate_title(&title)?],
        ),
        Change::Describe { id, description } => db.execute(
            "UPDATE todos SET description = NULLIF(?2, '') WHERE id = ?1 AND status = 'open'",
            params![id, description.trim()],
        ),
        Change::Assign { id, assignee } => {
            validate_assignee(db, assignee)?;
            db.execute(
                "UPDATE todos SET assignee_id = ?2 WHERE id = ?1 AND status = 'open'",
                params![id, assignee],
            )
        }
        Change::Complete { id } => return complete_task(db, actor, id),
        Change::Reopen { id } => db.execute(
            "UPDATE todos AS t SET status = 'open', assignee_id = previous_assignee_id,
                previous_assignee_id = NULL, completed_by = NULL, completed_at = NULL
             WHERE id = ?1 AND status = 'completed'
             AND NOT EXISTS (SELECT 1 FROM todo_series WHERE id = t.series_id AND retired = 1)
             AND (next_due IS NULL OR (
                next_due > ?2 AND EXISTS (
                    SELECT 1 FROM todos successor
                    WHERE successor.series_id = t.series_id AND successor.due = t.next_due
                )
             ))",
            params![id, today],
        ),
    }
    .map_err(database_error)?;
    ensure_task_changed(changed)
}

struct NewTask {
    title: String,
    description: String,
    due: String,
    assignee: Option<i64>,
    repeat: Repeat,
}

fn create_task(
    db: &Connection,
    today: &str,
    actor: i64,
    task: NewTask,
) -> Result<(), &'static str> {
    let NewTask {
        title,
        description,
        due,
        assignee,
        repeat,
    } = task;
    let title = validate_title(&title)?;
    let date = validate_due(&due)?;
    validate_assignee(db, assignee)?;
    let (series, next_due) = if let Some(frequency) = frequency(repeat) {
        let next_due = next_date(frequency, date, date)
            .ok_or("Choose an earlier first due date for this repeat.")?;
        let series = db
            .query_row(
                "INSERT INTO todo_series (frequency, first_due, usual_assignee_id)
             VALUES (?1, ?2, ?3) RETURNING id",
                params![frequency, due, assignee],
                |row| row.get::<_, i64>(0),
            )
            .map_err(database_error)?;
        (Some(series), Some(next_due.to_string()))
    } else {
        (None, None)
    };
    db.execute(
        "INSERT INTO todos (title, description, due, reporter_id, assignee_id, series_id, next_due)
         VALUES (?1, NULLIF(?2, ''), ?3, ?4, ?5, ?6, ?7)",
        params![
            title,
            description.trim(),
            due,
            actor,
            assignee,
            series,
            next_due
        ],
    )
    .map_err(database_error)?;
    catch_up(db, today)
}

fn edit_repeat(db: &Connection, today: &str, id: i64, task: NewTask) -> Result<(), &'static str> {
    let title = validate_title(&task.title)?;
    let due = validate_due(&task.due)?;
    validate_assignee(db, task.assignee)?;
    let frequency = frequency(task.repeat).ok_or("Choose a repeat frequency.")?;
    let (series_id, old_due, old_frequency): (i64, String, String) = db
        .query_row(
            "SELECT s.id, t.due, s.frequency FROM todos t JOIN todo_series s ON s.id = t.series_id
             WHERE t.id = ?1 AND t.status = 'open' AND s.retired = 0",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(database_error)?
        .ok_or("This task has changed. Refresh and try again.")?;

    if old_due != task.due || old_frequency != frequency {
        let overlaps_history = db
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM todos
                 WHERE series_id = ?1 AND status != 'open' AND due >= ?2)",
                params![series_id, task.due],
                |row| row.get::<_, bool>(0),
            )
            .map_err(database_error)?;
        if overlaps_history {
            return Err("Choose a due date after the last completed or missed occurrence.");
        }
        let following =
            next_date(frequency, due, due).ok_or("Choose an earlier due date for this repeat.")?;
        db.execute(
            "DELETE FROM todos WHERE series_id = ?1 AND status = 'open' AND id != ?2",
            params![series_id, id],
        )
        .map_err(database_error)?;
        db.execute(
            "UPDATE todos SET due = ?2, next_due = ?3 WHERE id = ?1",
            params![id, task.due, following.to_string()],
        )
        .map_err(database_error)?;
        db.execute(
            "UPDATE todo_series SET frequency = ?2, first_due = ?3 WHERE id = ?1",
            params![series_id, frequency, task.due],
        )
        .map_err(database_error)?;
    }
    db.execute(
        "UPDATE todo_series SET usual_assignee_id = ?2 WHERE id = ?1",
        params![series_id, task.assignee],
    )
    .map_err(database_error)?;
    db.execute(
        "UPDATE todos SET title = ?2, description = NULLIF(?3, ''), assignee_id = ?4
         WHERE series_id = ?1 AND status = 'open'",
        params![series_id, title, task.description.trim(), task.assignee],
    )
    .map_err(database_error)?;
    catch_up(db, today)
}

fn delete_repeat(db: &Connection, id: i64) -> Result<(), &'static str> {
    let series_id: i64 = db
        .query_row(
            "UPDATE todo_series SET retired = 1
             WHERE retired = 0 AND id = (
                SELECT series_id FROM todos WHERE id = ?1 AND status = 'open'
             ) RETURNING id",
            [id],
            |row| row.get(0),
        )
        .optional()
        .map_err(database_error)?
        .ok_or("This task has changed. Refresh and try again.")?;
    db.execute(
        "DELETE FROM todos WHERE series_id = ?1 AND status = 'open'",
        [series_id],
    )
    .map_err(database_error)?;
    db.execute(
        "DELETE FROM todo_series WHERE id = ?1 AND NOT EXISTS (
            SELECT 1 FROM todos WHERE series_id = ?1
        )",
        [series_id],
    )
    .map_err(database_error)?;
    Ok(())
}

fn validate_due(due: &str) -> Result<NaiveDate, &'static str> {
    let date = due
        .parse::<NaiveDate>()
        .map_err(|_| "Choose a valid due date.")?;
    if due.len() != 10 || due.starts_with("0000") || date.to_string() != due {
        return Err("Choose a valid due date.");
    }
    Ok(date)
}

fn validate_title(title: &str) -> Result<&str, &'static str> {
    let title = title.trim();
    if title.is_empty() {
        return Err("Enter a task title.");
    }
    Ok(title)
}

fn complete_task(db: &Connection, actor: i64, id: i64) -> Result<(), &'static str> {
    let changed = db
        .execute(
            "UPDATE todos SET
                 previous_assignee_id = assignee_id, assignee_id = ?2,
                 completed_by = ?2, completed_at = unixepoch(), status = 'completed'
             WHERE id = ?1 AND status = 'open'",
            params![id, actor],
        )
        .map_err(database_error)?;
    if changed == 0 {
        let already_complete = db
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM todos WHERE id = ?1 AND status = 'completed')",
                [id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(database_error)?;
        return ensure_task_changed(usize::from(already_complete));
    }
    create_successor(db, id)
}

fn ensure_task_changed(changed: usize) -> Result<(), &'static str> {
    if changed == 0 {
        return Err("This task has changed. Refresh and try again.");
    }
    Ok(())
}

fn frequency(repeat: Repeat) -> Option<&'static str> {
    (repeat != Repeat::Once).then(|| repeat.value())
}

fn next_date(frequency: &str, first: NaiveDate, current: NaiveDate) -> Option<NaiveDate> {
    let next = match frequency {
        "daily" => current.checked_add_days(Days::new(1)),
        "weekly" => current.checked_add_days(Days::new(7)),
        "monthly" => {
            let month = current.with_day(1)?.checked_add_months(Months::new(1))?;
            (1..=first.day()).rev().find_map(|day| month.with_day(day))
        }
        _ => None,
    };
    next.filter(|date| date.year() <= 9999)
}

fn create_successor(db: &Connection, id: i64) -> Result<(), &'static str> {
    let schedule: Option<(String, String, String)> = db
        .query_row(
            "SELECT s.frequency, s.first_due, t.next_due FROM todos t
         JOIN todo_series s ON s.id = t.series_id
         WHERE t.id = ?1 AND t.next_due IS NOT NULL AND s.retired = 0",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(database_error)?;
    let Some((frequency, first, next)) = schedule else {
        return Ok(());
    };
    let parse = |date: &str| {
        date.parse::<NaiveDate>()
            .map_err(|_| "The repeat has an invalid date.")
    };
    let following =
        next_date(&frequency, parse(&first)?, parse(&next)?).map(|date| date.to_string());
    db.execute(
        "INSERT INTO todos (title, description, due, reporter_id, assignee_id, series_id, next_due)
         SELECT t.title, t.description, t.next_due, t.reporter_id, s.usual_assignee_id, t.series_id, ?2
         FROM todos t JOIN todo_series s ON s.id = t.series_id WHERE t.id = ?1
         ON CONFLICT (series_id, due) DO NOTHING",
        params![id, following],
    ).map_err(database_error)?;
    Ok(())
}

fn catch_up(db: &Connection, today: &str) -> Result<(), &'static str> {
    let mut overdue = db
        .prepare(
            "SELECT id, series_id, due FROM todos
         WHERE status = 'open' AND next_due <= ?1 ORDER BY next_due, id LIMIT 1",
        )
        .map_err(database_error)?;
    let mut discard_older_miss = db
        .prepare("DELETE FROM todos WHERE status = 'failed' AND series_id = ?1 AND due < ?2")
        .map_err(database_error)?;
    let mut fail = db
        .prepare(
            "UPDATE todos AS missed SET status = 'failed', missed_on = next_due
             WHERE id = ?1 AND next_due > date(?2, '-7 days') AND NOT EXISTS (
                SELECT 1 FROM todos AS retained
                WHERE retained.status = 'failed' AND retained.series_id = missed.series_id
             )",
        )
        .map_err(database_error)?;
    let mut discard = db
        .prepare("DELETE FROM todos WHERE id = ?1")
        .map_err(database_error)?;
    while let Some((id, series_id, due)) = overdue
        .query_row([today], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .optional()
        .map_err(database_error)?
    {
        create_successor(db, id)?;
        discard_older_miss
            .execute(params![series_id, due])
            .map_err(database_error)?;
        if fail.execute(params![id, today]).map_err(database_error)? == 0 {
            discard.execute([id]).map_err(database_error)?;
        }
    }
    let expired = db
        .execute(
            "DELETE FROM todos WHERE status = 'failed' AND missed_on <= date(?1, '-7 days')",
            [today],
        )
        .map_err(database_error)?;
    if expired > 0 {
        db.execute(
            "DELETE FROM todo_series WHERE retired = 1 AND NOT EXISTS (
                SELECT 1 FROM todos WHERE series_id = todo_series.id
            )",
            [],
        )
        .map_err(database_error)?;
    }
    Ok(())
}

fn validate_assignee(db: &Connection, assignee: Option<i64>) -> Result<(), &'static str> {
    let exists: bool = db
        .query_row(
            "SELECT ?1 IS NULL OR EXISTS (SELECT 1 FROM users WHERE id = ?1)",
            [assignee],
            |row| row.get(0),
        )
        .map_err(database_error)?;
    if !exists {
        return Err("Choose a household member or leave the task unassigned.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guest_snapshot_does_not_advance_overdue_recurring_tasks() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test.sqlite");
        crate::database::initialize(&path).unwrap();
        let mut db = crate::database::open(&path).unwrap();
        db.execute_batch("INSERT INTO users VALUES (1, 'alex', 'Alex', 'One', 'test', 0);")
            .unwrap();
        apply(
            &mut db,
            "2026-11-23".parse().unwrap(),
            1,
            Change::Create {
                title: "Feed hens".into(),
                description: String::new(),
                due: "2026-11-23".into(),
                assignee: Some(1),
                repeat: Repeat::Daily,
            },
        )
        .unwrap();
        db.pragma_update(None, "query_only", true).unwrap();
        let tasks = snapshot(&db, "2026-11-24".parse().unwrap()).unwrap();
        assert!(tasks.members.is_empty());
        assert_eq!(tasks.tasks.len(), 1);
        assert_eq!(tasks.tasks[0].status, "open");
    }

    #[test]
    fn migration_and_one_off_completion_preserve_reporter_and_assignment() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test.sqlite");
        let mut db = crate::database::open(&path).unwrap();
        db.execute_batch("CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY);")
            .unwrap();
        db.execute_batch(include_str!("migrations/001_accounts.sql"))
            .unwrap();
        db.execute_batch(
            "INSERT INTO schema_migrations VALUES (1);
             INSERT INTO users VALUES (1, 'alex', 'Alex', 'One', 'test', 0),
                                      (2, 'sam', 'Sam Lee', 'Two', 'test', 0);",
        )
        .unwrap();
        crate::database::initialize(&path).unwrap();
        let today = "2026-11-24".parse().unwrap();
        apply(
            &mut db,
            today,
            1,
            Change::Create {
                title: "Muck out".into(),
                description: "Bring gloves".into(),
                due: "2026-11-23".into(),
                assignee: Some(1),
                repeat: Repeat::Once,
            },
        )
        .unwrap();
        apply(&mut db, today, 2, Change::Complete { id: 1 }).unwrap();
        db.execute("UPDATE todos SET completed_at = 42", [])
            .unwrap();
        apply(&mut db, today, 1, Change::Complete { id: 1 }).unwrap();
        let completion = db
            .query_row("SELECT completed_by, completed_at FROM todos", [], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })
            .unwrap();
        assert_eq!(completion, (2, 42));
        let tasks = list(&mut db, today).unwrap();
        assert_eq!(tasks.tasks[0].reporter, "Alex One");
        assert_eq!(tasks.tasks[0].assignee_id, Some(2));
        assert_eq!(
            tasks.tasks[0].assignee_first_name.as_deref(),
            Some("Sam Lee")
        );
        assert_eq!(tasks.tasks[0].assignee.as_deref(), Some("Sam Lee Two"));
        assert_eq!(tasks.members[1].id, 2);
        assert_eq!(tasks.members[1].name, "Sam Lee");
        assert_eq!(tasks.tasks[0].description.as_deref(), Some("Bring gloves"));
        apply(&mut db, today, 2, Change::Reopen { id: 1 }).unwrap();
        let task = &list(&mut db, today).unwrap().tasks[0];
        assert_eq!(task.assignee_id, Some(1));
        assert_eq!(task.status, "open");
    }

    #[test]
    fn repeat_copies_one_description_at_a_time_and_keeps_edits_on_undo() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test.sqlite");
        crate::database::initialize(&path).unwrap();
        let mut db = crate::database::open(&path).unwrap();
        db.execute_batch(
            "INSERT INTO users VALUES (1, 'alex', 'Alex', 'One', 'test', 0),
                                      (2, 'sam', 'Sam', 'Two', 'test', 0);",
        )
        .unwrap();
        let monday = "2026-11-23".parse().unwrap();
        let tuesday = "2026-11-24".parse().unwrap();
        let next_monday = "2026-11-30".parse().unwrap();
        apply(
            &mut db,
            monday,
            1,
            Change::Create {
                title: "Muck out".into(),
                description: "Bring gloves".into(),
                due: "2026-11-23".into(),
                assignee: Some(1),
                repeat: Repeat::Weekly,
            },
        )
        .unwrap();
        assert_eq!(list(&mut db, tuesday).unwrap().tasks.len(), 1);
        apply(&mut db, tuesday, 2, Change::Complete { id: 1 }).unwrap();
        let tasks = list(&mut db, tuesday).unwrap().tasks;
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[1].description.as_deref(), Some("Bring gloves"));
        assert_eq!(tasks[1].assignee_id, Some(1));
        assert_eq!(tasks[1].reporter, "Alex One");
        apply(
            &mut db,
            tuesday,
            2,
            Change::Describe {
                id: 2,
                description: "Bring a bucket".into(),
            },
        )
        .unwrap();
        apply(&mut db, tuesday, 2, Change::Reopen { id: 1 }).unwrap();
        assert!(apply(&mut db, next_monday, 2, Change::Complete { id: 1 }).is_err());
        let tasks = list(&mut db, next_monday).unwrap().tasks;
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].status, "failed");
        assert_eq!(tasks[1].description.as_deref(), Some("Bring a bucket"));
        apply(&mut db, next_monday, 2, Change::Complete { id: 2 }).unwrap();
        apply(&mut db, next_monday, 1, Change::Complete { id: 2 }).unwrap();
        let tasks = list(&mut db, next_monday).unwrap().tasks;
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[1].assignee_id, Some(2));
        assert_eq!(tasks[2].description.as_deref(), Some("Bring a bucket"));
        let after_downtime = "2026-12-21".parse().unwrap();
        let tasks = list(&mut db, after_downtime).unwrap().tasks;
        let status: Vec<_> = tasks.iter().map(|task| task.status.as_str()).collect();
        assert_eq!(status, ["completed", "failed", "open"]);
        assert_eq!(tasks[1].due, "2026-12-14");
        assert_eq!(tasks[2].due, "2026-12-21");
        assert!(apply(&mut db, after_downtime, 2, Change::Reopen { id: 2 }).is_err());
        apply(&mut db, after_downtime, 2, Change::Complete { id: 2 }).unwrap();
        assert!(
            list(&mut db, after_downtime).unwrap().tasks == tasks,
            "a completion retry should leave the current occurrence and retained history unchanged"
        );
    }

    #[test]
    fn edits_open_repeats_and_preserves_history_when_rescheduling() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test.sqlite");
        crate::database::initialize(&path).unwrap();
        let mut db = crate::database::open(&path).unwrap();
        db.execute(
            "INSERT INTO users VALUES (1, 'alex', 'Alex', 'One', 'test', 0)",
            [],
        )
        .unwrap();
        let today = "2026-11-23".parse().unwrap();
        apply(
            &mut db,
            today,
            1,
            Change::Create {
                title: "Muck out".into(),
                description: "Bring gloves".into(),
                due: "2026-11-23".into(),
                assignee: Some(1),
                repeat: Repeat::Weekly,
            },
        )
        .unwrap();
        apply(&mut db, today, 1, Change::Complete { id: 1 }).unwrap();
        apply(&mut db, today, 1, Change::Reopen { id: 1 }).unwrap();
        let edit = |id, title: &str, due: &str, repeat| Change::EditRepeat {
            id,
            title: title.into(),
            description: "Bring a bucket".into(),
            due: due.into(),
            assignee: None,
            repeat,
        };

        assert_eq!(
            apply(
                &mut db,
                today,
                1,
                edit(1, " \t ", "2026-11-23", Repeat::Weekly)
            ),
            Err("Enter a task title.")
        );
        apply(
            &mut db,
            today,
            1,
            edit(1, "  Clean the coop  ", "2026-11-23", Repeat::Weekly),
        )
        .unwrap();
        let tasks = list(&mut db, today).unwrap().tasks;
        assert_eq!(tasks[0].title, "Clean the coop");
        assert_eq!(tasks[1].title, "Clean the coop");
        assert_eq!(tasks[0].description.as_deref(), Some("Bring a bucket"));
        assert_eq!(tasks[0].assignee_id, None);

        apply(&mut db, today, 1, Change::Complete { id: 1 }).unwrap();
        assert!(apply(
            &mut db,
            today,
            1,
            edit(1, "Past task", "2026-11-30", Repeat::Weekly)
        )
        .is_err());
        assert!(apply(
            &mut db,
            today,
            1,
            edit(2, "Past task", "2026-11-23", Repeat::Weekly)
        )
        .is_err());
        apply(
            &mut db,
            today,
            1,
            edit(2, "Sweep the coop", "2027-01-31", Repeat::Monthly),
        )
        .unwrap();
        assert!(apply(&mut db, today, 1, Change::Reopen { id: 1 }).is_err());
        apply(&mut db, today, 1, Change::Complete { id: 1 }).unwrap();
        apply(&mut db, today, 1, Change::Complete { id: 2 }).unwrap();
        let mut reopened = crate::database::open(&path).unwrap();
        let tasks = list(&mut reopened, today).unwrap().tasks;
        let titles: Vec<_> = tasks.iter().map(|task| task.title.as_str()).collect();
        assert_eq!(
            titles,
            ["Clean the coop", "Sweep the coop", "Sweep the coop"]
        );
        assert!(!tasks[0].can_reopen);
        assert_eq!(tasks[0].due, "2026-11-23");
        assert_eq!(tasks[1].due, "2027-01-31");
        assert_eq!(tasks[2].due, "2027-02-28");
        assert_eq!(tasks[2].assignee_id, None);
        // Editing details on February's occurrence keeps the January anchor.
        apply(
            &mut reopened,
            today,
            1,
            edit(tasks[2].id, "Sweep the coop", "2027-02-28", Repeat::Monthly),
        )
        .unwrap();
        apply(
            &mut reopened,
            today,
            1,
            Change::Complete { id: tasks[2].id },
        )
        .unwrap();
        assert_eq!(
            list(&mut reopened, today).unwrap().tasks[3].due,
            "2027-03-31"
        );
    }

    #[test]
    fn deleting_a_repeat_removes_open_tasks_and_keeps_history_closed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test.sqlite");
        crate::database::initialize(&path).unwrap();
        let mut db = crate::database::open(&path).unwrap();
        db.execute(
            "INSERT INTO users VALUES (1, 'alex', 'Alex', 'One', 'test', 0)",
            [],
        )
        .unwrap();
        let today = "2026-11-23".parse().unwrap();
        let create = |repeat| Change::Create {
            title: "Put the bins out".into(),
            description: String::new(),
            due: "2026-11-23".into(),
            assignee: Some(1),
            repeat,
        };
        apply(&mut db, today, 1, create(Repeat::Weekly)).unwrap();
        apply(&mut db, today, 1, Change::Complete { id: 1 }).unwrap();
        apply(&mut db, today, 1, Change::Complete { id: 2 }).unwrap();
        apply(&mut db, today, 1, Change::Reopen { id: 2 }).unwrap();
        assert_eq!(list(&mut db, today).unwrap().tasks.len(), 3);

        apply(&mut db, today, 1, Change::DeleteRepeat { id: 2 }).unwrap();
        let tasks = list(&mut db, today).unwrap().tasks;
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, 1);
        assert_eq!(tasks[0].status, "completed");
        assert!(!tasks[0].can_reopen);
        assert!(apply(&mut db, today, 1, Change::Reopen { id: 1 }).is_err());
        apply(&mut db, today, 1, Change::Complete { id: 1 }).unwrap();

        let later = "2027-01-01".parse().unwrap();
        assert_eq!(list(&mut db, later).unwrap().tasks.len(), 1);
        apply(&mut db, today, 1, create(Repeat::Once)).unwrap();
        assert!(apply(&mut db, today, 1, Change::Complete { id: 3 }).is_err());
        let tasks = list(&mut db, later).unwrap().tasks;
        let open = tasks.iter().find(|task| task.status == "open").unwrap();
        assert!(open.id > 3);
        assert!(open.recurrence.is_none());
        assert!(apply(&mut db, today, 1, Change::DeleteRepeat { id: open.id }).is_err());
    }

    #[test]
    fn catch_up_keeps_only_the_latest_miss_and_expires_it_on_day_seven() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test.sqlite");
        crate::database::initialize(&path).unwrap();
        let mut db = crate::database::open(&path).unwrap();
        db.execute(
            "INSERT INTO users VALUES (1, 'alex', 'Alex', 'One', 'test', 0)",
            [],
        )
        .unwrap();
        let start = "2026-11-23".parse().unwrap();
        let create = |title: &str, repeat| Change::Create {
            title: title.into(),
            description: String::new(),
            due: "2026-11-23".into(),
            assignee: None,
            repeat,
        };
        apply(&mut db, start, 1, create("Daily chore", Repeat::Daily)).unwrap();
        apply(&mut db, start, 1, create("Monthly chore", Repeat::Monthly)).unwrap();

        let thursday = "2026-11-26".parse().unwrap();
        let tasks = list(&mut db, thursday).unwrap().tasks;
        let misses: Vec<_> = tasks
            .iter()
            .filter(|task| task.status == "failed")
            .collect();
        assert_eq!(misses.len(), 1);
        assert_eq!(misses[0].due, "2026-11-25");
        let daily_open = tasks
            .iter()
            .find(|task| task.title == "Daily chore" && task.status == "open")
            .unwrap();
        apply(
            &mut db,
            thursday,
            1,
            Change::DeleteRepeat { id: daily_open.id },
        )
        .unwrap();

        // There was no access on December 23 when the monthly task became missed.
        let day_six = "2026-12-29".parse().unwrap();
        let tasks = list(&mut db, day_six).unwrap().tasks;
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].status, "failed");
        assert_eq!(tasks[0].due, "2026-11-23");
        assert_eq!(tasks[1].due, "2026-12-23");
        apply(
            &mut db,
            day_six,
            1,
            Change::DeleteRepeat { id: tasks[1].id },
        )
        .unwrap();
        assert_eq!(list(&mut db, day_six).unwrap().tasks.len(), 1);

        let day_seven = "2026-12-30".parse().unwrap();
        assert!(apply(&mut db, day_seven, 1, Change::Complete { id: tasks[0].id }).is_err());
        let remaining: i64 = db
            .query_row("SELECT count(*) FROM todos", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining, 0);
        assert!(list(&mut db, day_seven).unwrap().tasks.is_empty());
    }

    #[test]
    fn monthly_repeats_return_to_the_original_day_after_short_months() {
        for (january, february, march) in [
            ("2026-01-31", "2026-02-28", "2026-03-31"),
            ("2028-01-31", "2028-02-29", "2028-03-31"),
        ] {
            let first = january.parse().unwrap();
            let next = next_date("monthly", first, first).unwrap();
            assert_eq!(next.to_string(), february);
            assert_eq!(
                next_date("monthly", first, next).unwrap().to_string(),
                march
            );
        }
        let first = "2026-12-31".parse().unwrap();
        assert_eq!(
            next_date("daily", first, first).unwrap().to_string(),
            "2027-01-01"
        );
    }
}
