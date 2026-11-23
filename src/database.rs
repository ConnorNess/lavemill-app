use rusqlite::Connection;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

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
    use super::*;

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
