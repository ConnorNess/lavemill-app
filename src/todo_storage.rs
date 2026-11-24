use crate::accounts::database_error;
use crate::todos::{Change, Member, Task, Tasks};
use chrono::{NaiveDate, Utc};
use rusqlite::{params, Connection};

pub fn household_date() -> Result<NaiveDate, &'static str> {
    let zone = std::env::var("LAVEMILL_TIME_ZONE").unwrap_or_else(|_| "Europe/London".into());
    let zone: chrono_tz::Tz = zone.parse().map_err(|_| "Invalid household time zone.")?;
    Ok(Utc::now().with_timezone(&zone).date_naive())
}

pub fn list(db: &Connection, today: NaiveDate) -> Result<Tasks, &'static str> {
    let mut statement = db
        .prepare(
            "SELECT t.id, t.title, t.description, t.due, t.assignee_id,
                reporter.first_name || ' ' || reporter.last_name,
                assignee.first_name || ' ' || assignee.last_name, t.status
         FROM todos t
         JOIN users reporter ON reporter.id = t.reporter_id
         LEFT JOIN users assignee ON assignee.id = t.assignee_id
         ORDER BY t.due, t.id",
        )
        .map_err(database_error)?;
    let tasks = statement
        .query_map([], |row| {
            Ok(Task {
                id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                due: row.get(3)?,
                assignee_id: row.get(4)?,
                reporter: row.get(5)?,
                assignee: row.get(6)?,
                status: row.get(7)?,
            })
        })
        .map_err(database_error)?
        .collect::<Result<_, _>>()
        .map_err(database_error)?;
    let mut statement = db.prepare(
        "SELECT id, first_name || ' ' || last_name FROM users ORDER BY first_name, last_name, username",
    ).map_err(database_error)?;
    let members = statement
        .query_map([], |row| {
            Ok(Member {
                id: row.get(0)?,
                name: row.get(1)?,
            })
        })
        .map_err(database_error)?
        .collect::<Result<_, _>>()
        .map_err(database_error)?;
    Ok(Tasks {
        today: today.to_string(),
        tasks,
        members,
    })
}

pub fn apply(db: &Connection, actor: i64, change: Change) -> Result<(), &'static str> {
    let changed = match change {
        Change::Create { title, description, due, assignee } => {
            if title.trim().is_empty() {
                return Err("Enter a task title.");
            }
            let date = due.parse::<NaiveDate>().map_err(|_| "Choose a valid due date.")?;
            if due.len() != 10 || due.starts_with("0000") || date.to_string() != due {
                return Err("Choose a valid due date.");
            }
            validate_assignee(db, assignee)?;
            db.execute(
                "INSERT INTO todos (title, description, due, reporter_id, assignee_id)
                 VALUES (?1, NULLIF(?2, ''), ?3, ?4, ?5)",
                params![title.trim(), description.trim(), due, actor, assignee],
            )
        }
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
        Change::Complete { id } => db.execute(
            "UPDATE todos SET
                 previous_assignee_id = CASE WHEN status = 'open' THEN assignee_id ELSE previous_assignee_id END,
                 assignee_id = CASE WHEN status = 'open' THEN ?2 ELSE assignee_id END,
                 completed_by = COALESCE(completed_by, ?2),
                 completed_at = COALESCE(completed_at, unixepoch()), status = 'completed'
             WHERE id = ?1 AND status IN ('open', 'completed')",
            params![id, actor],
        ),
        Change::Reopen { id } => db.execute(
            "UPDATE todos SET status = 'open', assignee_id = previous_assignee_id,
                previous_assignee_id = NULL, completed_by = NULL, completed_at = NULL
             WHERE id = ?1 AND status = 'completed'",
            [id],
        ),
    }.map_err(database_error)?;
    if changed == 0 {
        return Err("This task has changed. Refresh and try again.");
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
    fn migration_and_one_off_completion_preserve_reporter_and_assignment() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test.sqlite");
        let db = crate::database::open(&path).unwrap();
        db.execute_batch("CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY);")
            .unwrap();
        db.execute_batch(include_str!("migrations/001_accounts.sql"))
            .unwrap();
        db.execute_batch(
            "INSERT INTO schema_migrations VALUES (1);
             INSERT INTO users VALUES (1, 'alex', 'Alex', 'One', 'test', 0),
                                      (2, 'sam', 'Sam', 'Two', 'test', 0);",
        )
        .unwrap();
        crate::database::initialize(&path).unwrap();
        apply(
            &db,
            1,
            Change::Create {
                title: "Muck out".into(),
                description: "Bring gloves".into(),
                due: "2026-11-23".into(),
                assignee: Some(1),
            },
        )
        .unwrap();
        apply(&db, 2, Change::Complete { id: 1 }).unwrap();
        db.execute("UPDATE todos SET completed_at = 42", [])
            .unwrap();
        apply(&db, 1, Change::Complete { id: 1 }).unwrap();
        let completion = db
            .query_row("SELECT completed_by, completed_at FROM todos", [], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })
            .unwrap();
        assert_eq!(completion, (2, 42));
        let today = "2026-11-24".parse().unwrap();
        let tasks = list(&db, today).unwrap();
        assert_eq!(tasks.tasks[0].reporter, "Alex One");
        assert_eq!(tasks.tasks[0].assignee_id, Some(2));
        assert_eq!(tasks.tasks[0].description.as_deref(), Some("Bring gloves"));
        apply(&db, 2, Change::Reopen { id: 1 }).unwrap();
        let task = &list(&db, today).unwrap().tasks[0];
        assert_eq!(task.assignee_id, Some(1));
        assert_eq!(task.status, "open");
    }
}
