use rusqlite::Connection;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

// Append numbered migrations using include_str! when feature schemas are added.
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("migrations/001_accounts.sql")),
    (2, include_str!("migrations/002_todos.sql")),
    (3, include_str!("migrations/003_todo_recurrence.sql")),
    (4, include_str!("migrations/004_shopping_items.sql")),
    (5, include_str!("migrations/005_google_calendar.sql")),
    (
        6,
        include_str!("migrations/006_google_calendar_sharing.sql"),
    ),
    (7, include_str!("migrations/007_calendar_providers.sql")),
    (8, include_str!("migrations/008_activity_profile_links.sql")),
    (9, include_str!("migrations/009_recipes.sql")),
    (10, include_str!("migrations/010_recipe_ids.sql")),
    (11, include_str!("migrations/011_stable_todo_ids.sql")),
    (12, include_str!("migrations/012_retired_todo_series.sql")),
    (13, include_str!("migrations/013_todo_miss_retention.sql")),
    (14, include_str!("migrations/014_guest_sessions.sql")),
];

pub fn initialize(path: &Path) -> Result<()> {
    migrate(&mut open(path)?, MIGRATIONS)
}

fn migrate(connection: &mut Connection, migrations: &[(i64, &str)]) -> Result<()> {
    if migrations.iter().any(|(version, _)| *version <= 0)
        || migrations.windows(2).any(|pair| pair[0].0 >= pair[1].0)
    {
        return Err("migration versions must be positive and strictly increasing".into());
    }
    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    transaction.execute_batch("CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);")?;
    let applied = {
        let mut statement =
            transaction.prepare("SELECT version FROM schema_migrations ORDER BY version")?;
        let rows = statement.query_map([], |row| row.get::<_, i64>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    if applied.len() > migrations.len()
        || applied
            .iter()
            .zip(migrations)
            .any(|(version, migration)| *version != migration.0)
    {
        return Err(
            "database migration history is unknown or is not a prefix of this build".into(),
        );
    }
    for (version, sql) in &migrations[applied.len()..] {
        transaction.execute_batch(sql)?;
        transaction.execute(
            "INSERT INTO schema_migrations(version) VALUES (?1)",
            [version],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

pub fn configured_path() -> PathBuf {
    std::env::var_os("LAVEMILL_DATABASE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/lavemill.sqlite3"))
}

pub fn open(path: &Path) -> Result<Connection> {
    if path.as_os_str().is_empty() || path == Path::new(":memory:") {
        return Err("a persistent database path is required".into());
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let connection = Connection::open(path)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.pragma_update(None, "foreign_keys", true)?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    Ok(connection)
}

#[cfg(test)]
mod tests {
    #[test]
    fn recipe_id_migration_preserves_and_tracks_existing_ids() {
        let mut db = rusqlite::Connection::open_in_memory().unwrap();
        super::migrate(&mut db, &super::MIGRATIONS[..9]).unwrap();
        db.execute("INSERT INTO recipes (id, title) VALUES (42, 'Toast')", [])
            .unwrap();
        super::migrate(&mut db, super::MIGRATIONS).unwrap();
        let (id, last_id): (i64, i64) = db
            .query_row(
                "SELECT recipes.id, recipe_id_sequence.last_id
                 FROM recipes CROSS JOIN recipe_id_sequence",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!((id, last_id), (42, 42));
    }

    #[test]
    fn calendar_provider_migration_preserves_google_connections_and_attempts() {
        let mut db = rusqlite::Connection::open_in_memory().unwrap();
        super::migrate(&mut db, &super::MIGRATIONS[..6]).unwrap();
        db.execute_batch(
            "INSERT INTO users (id, username, first_name, last_name, password_hash)
             VALUES (1, 'member', 'Household', 'Member', 'unused');
             INSERT INTO sessions (token_hash, user_id, expires_at) VALUES (X'01', 1, 9999999999);
             INSERT INTO google_calendar_connections VALUES (1, X'BEEF', 'family', 1);
             INSERT INTO google_calendar_attempts VALUES (1, X'01', X'02', 'verifier', 9999999999, 1);",
        ).unwrap();
        super::migrate(&mut db, super::MIGRATIONS).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT hex(refresh_token), calendar_id, shared, length(id)
                 FROM calendar_connections WHERE user_id = 1 AND provider = 'google'",
                [],
                |row| Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, bool>(2)?,
                    row.get::<_, i64>(3)?
                )),
            )
            .unwrap(),
            ("BEEF".into(), "family".into(), true, 16)
        );
        let claimed: bool = db.query_row(
            "SELECT claimed FROM calendar_attempts
             WHERE user_id = 1 AND provider = 'google' AND state_hash = X'02' AND verifier = 'verifier'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert!(claimed);
    }

    use super::*;

    #[test]
    fn todo_miss_migration_preserves_only_the_latest_miss_per_series() {
        let mut db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "foreign_keys", true).unwrap();
        migrate(&mut db, &MIGRATIONS[..12]).unwrap();
        db.execute_batch(
            "INSERT INTO users VALUES (1, 'member', 'Household', 'Member', 'unused', 0);
             INSERT INTO todo_series (id, frequency, first_due)
             VALUES (1, 'weekly', '2026-11-02'), (2, 'daily', '2026-11-15');
             INSERT INTO todos (id, title, due, reporter_id, series_id, next_due, status) VALUES
                 (3, 'Weekly chore', '2026-11-09', 1, 1, '2026-11-16', 'failed'),
                 (4, 'Weekly chore', '2026-11-02', 1, 1, '2026-11-09', 'failed'),
                 (5, 'Weekly chore', '2026-11-16', 1, 1, '2026-11-23', 'open'),
                 (6, 'Daily chore', '2026-11-15', 1, 2, '2026-11-16', 'failed'),
                 (7, 'One-off', '2026-11-16', 1, NULL, NULL, 'failed');
             INSERT INTO todos (id, title, due, reporter_id, status, completed_by, completed_at)
             VALUES (8, 'Done', '2026-11-09', 1, 'completed', 1, 42);",
        )
        .unwrap();
        migrate(&mut db, MIGRATIONS).unwrap();
        let mut statement = db
            .prepare("SELECT id, missed_on FROM todos ORDER BY id")
            .unwrap();
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(
            rows,
            [
                (3, Some("2026-11-16".into())),
                (5, None),
                (6, Some("2026-11-16".into())),
                (7, Some("2026-11-16".into())),
                (8, None),
            ]
        );
    }

    #[test]
    fn migrations_upgrade_once_and_preserve_data() {
        let mut connection = Connection::open_in_memory().unwrap();
        migrate(&mut connection, &[]).unwrap();
        let migrations = [
            (
                1,
                "CREATE TABLE example(value TEXT); INSERT INTO example VALUES ('saved');",
            ),
            (2, "ALTER TABLE example ADD COLUMN extra TEXT;"),
        ];
        migrate(&mut connection, &migrations[..1]).unwrap();
        migrate(&mut connection, &migrations).unwrap();
        migrate(&mut connection, &migrations).unwrap();
        let count: i64 = connection
            .query_row(
                "SELECT count(*) FROM example WHERE value = 'saved'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        assert!(migrate(&mut connection, &migrations[..1]).is_err());
    }

    #[test]
    fn failed_migration_rolls_back_schema_and_ledger() {
        let mut connection = Connection::open_in_memory().unwrap();
        migrate(&mut connection, &[]).unwrap();
        assert!(migrate(
            &mut connection,
            &[(1, "CREATE TABLE example(value TEXT);"), (2, "INVALID SQL")]
        )
        .is_err());
        let count: i64 = connection
            .query_row("SELECT count(*) FROM schema_migrations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
        assert!(connection.prepare("SELECT * FROM example").is_err());
        assert!(migrate(&mut connection, &[(2, "SELECT 1"), (1, "SELECT 1")]).is_err());
    }

    #[test]
    fn file_survives_reopening_with_connection_settings() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nested/test.sqlite3");
        let connection = open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE example(value TEXT); INSERT INTO example VALUES ('saved');",
            )
            .unwrap();
        drop(connection);
        let connection = open(&path).unwrap();
        let value: String = connection
            .query_row("SELECT value FROM example", [], |r| r.get(0))
            .unwrap();
        assert_eq!(value, "saved");
        for (pragma, expected) in [("foreign_keys", 1), ("busy_timeout", 5000)] {
            let value: i64 = connection
                .pragma_query_value(None, pragma, |r| r.get(0))
                .unwrap();
            assert_eq!(value, expected);
        }
        let mode: String = connection
            .pragma_query_value(None, "journal_mode", |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
        assert!(open(directory.path()).is_err());
        assert!(open(Path::new("")).is_err());
    }
}
