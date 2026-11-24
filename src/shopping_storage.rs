use crate::accounts::database_error;
use crate::shopping_data::{ShoppingChange, ShoppingItem};
use rusqlite::{params, Connection, TransactionBehavior};

const RETENTION_SECONDS: i64 = 7 * 24 * 60 * 60;

pub fn list(db: &mut Connection) -> Result<Vec<ShoppingItem>, &'static str> {
    access(db, read_items).map_err(database_error)
}

pub fn apply(
    db: &mut Connection,
    actor: i64,
    change: ShoppingChange,
) -> Result<Option<&'static str>, &'static str> {
    access(db, |db, now| apply_change(db, now, actor, change)).map_err(database_error)
}

fn access<T>(
    db: &mut Connection,
    operation: impl FnOnce(&Connection, i64) -> rusqlite::Result<T>,
) -> rusqlite::Result<T> {
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let now = tx.query_row("SELECT unixepoch()", [], |row| row.get(0))?;
    let result = access_at(&tx, now, operation)?;
    tx.commit()?;
    Ok(result)
}

fn access_at<T>(
    db: &Connection,
    now: i64,
    operation: impl FnOnce(&Connection, i64) -> rusqlite::Result<T>,
) -> rusqlite::Result<T> {
    db.execute(
        "DELETE FROM shopping_items WHERE completed_at <= ?1 OR removed_at <= ?1",
        [now - RETENTION_SECONDS],
    )?;
    operation(db, now)
}

fn read_items(db: &Connection, _: i64) -> rusqlite::Result<Vec<ShoppingItem>> {
    let mut statement = db.prepare(
        "SELECT id, name, quantity, completed_at IS NOT NULL FROM shopping_items
         WHERE removed_at IS NULL ORDER BY completed_at IS NOT NULL, id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(ShoppingItem {
            id: row.get(0)?,
            name: row.get(1)?,
            quantity: row.get(2)?,
            checked: row.get(3)?,
        })
    })?;
    rows.collect()
}

fn apply_change(
    db: &Connection,
    now: i64,
    actor: i64,
    change: ShoppingChange,
) -> rusqlite::Result<Option<&'static str>> {
    let changed = match change {
        ShoppingChange::Add { name, quantity } => {
            if name.trim().is_empty() {
                return Ok(Some("Enter an item name."));
            }
            db.execute(
                "INSERT INTO shopping_items (name, quantity, created_by)
                 VALUES (?1, NULLIF(?2, ''), ?3)",
                params![name.trim(), quantity.trim(), actor],
            )
        }
        ShoppingChange::Edit { id, name, quantity } => {
            if name.trim().is_empty() {
                return Ok(Some("Enter an item name."));
            }
            db.execute(
                "UPDATE shopping_items SET name = ?2, quantity = NULLIF(?3, '')
                 WHERE id = ?1 AND removed_at IS NULL",
                params![id, name.trim(), quantity.trim()],
            )
        }
        ShoppingChange::SetChecked { id, checked } => db.execute(
            "UPDATE shopping_items SET completed_at =
                CASE WHEN ?2 THEN COALESCE(completed_at, ?3) ELSE NULL END
             WHERE id = ?1 AND removed_at IS NULL",
            params![id, checked, now],
        ),
        ShoppingChange::Remove { id } => db.execute(
            "UPDATE shopping_items SET removed_at = ?2, removed_by = ?3
             WHERE id = ?1 AND removed_at IS NULL",
            params![id, now, actor],
        ),
        ShoppingChange::Restore { id } => db.execute(
            "UPDATE shopping_items SET removed_at = NULL, removed_by = NULL
             WHERE id = ?1 AND removed_by = ?2",
            params![id, actor],
        ),
    }?;
    Ok((changed == 0).then_some("This item is no longer available. Reload the list."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(db: &Connection) {
        db.execute_batch(
            "INSERT INTO users (id, username, first_name, last_name, password_hash, is_admin)
             VALUES (1, 'alex', 'Alex', 'One', 'test', 0), (2, 'sam', 'Sam', 'Two', 'test', 0)",
        )
        .unwrap();
    }

    fn change(
        db: &Connection,
        now: i64,
        actor: i64,
        change: ShoppingChange,
    ) -> Option<&'static str> {
        access_at(db, now, |db, now| apply_change(db, now, actor, change)).unwrap()
    }

    fn add(name: &str, quantity: &str) -> ShoppingChange {
        ShoppingChange::Add {
            name: name.into(),
            quantity: quantity.into(),
        }
    }

    #[test]
    fn migration_keeps_existing_accounts_and_persists_items_across_connections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shopping.sqlite");
        let mut db = crate::database::open(&path).unwrap();
        for migration in [
            include_str!("migrations/001_accounts.sql"),
            include_str!("migrations/002_todos.sql"),
            include_str!("migrations/003_todo_recurrence.sql"),
            "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY);
             INSERT INTO schema_migrations VALUES (1), (2), (3);",
        ] {
            db.execute_batch(migration).unwrap();
        }
        setup(&db);
        crate::database::initialize(&path).unwrap();
        assert!(list(&mut db).unwrap().is_empty());
        assert!(apply(&mut db, 1, add(" ", "box")).unwrap().is_some());
        assert_eq!(
            apply(&mut db, 1, add(" Milk ", " a handful ")).unwrap(),
            None
        );
        assert_eq!(apply(&mut db, 2, add("Milk", "")).unwrap(), None);
        drop(db);

        let mut db = crate::database::open(&path).unwrap();
        let items = list(&mut db).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].name, items[1].name);
        assert_eq!(items[0].quantity.as_deref(), Some("a handful"));
        assert_eq!(items[1].quantity, None);
        let creator = db
            .query_row(
                "SELECT created_by FROM shopping_items WHERE id = ?1",
                [items[0].id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap();
        assert_eq!(creator, 1);
    }

    #[test]
    fn checking_retries_keep_expiry_and_unchecking_restarts_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shopping.sqlite");
        crate::database::initialize(&path).unwrap();
        let db = crate::database::open(&path).unwrap();
        setup(&db);
        assert_eq!(change(&db, 100, 1, add("Milk", "")), None);
        assert_eq!(change(&db, 100, 1, add("Bread", "")), None);
        assert_eq!(change(&db, 100, 1, add("Eggs", "")), None);
        let set =
            |now, id, checked| change(&db, now, 2, ShoppingChange::SetChecked { id, checked });
        assert_eq!(set(100, 1, true), None);
        assert_eq!(set(101, 1, true), None);
        assert_eq!(set(100, 2, true), None);
        assert_eq!(set(102, 2, false), None);
        assert_eq!(set(103, 2, true), None);
        let items = access_at(&db, 99 + RETENTION_SECONDS, read_items).unwrap();
        assert_eq!(
            items.iter().map(|item| item.id).collect::<Vec<_>>(),
            [3, 1, 2]
        );
        assert!(set(100 + RETENTION_SECONDS, 1, false).is_some());
        let items = access_at(&db, 100 + RETENTION_SECONDS, read_items).unwrap();
        assert_eq!(items.iter().map(|item| item.id).collect::<Vec<_>>(), [3, 2]);
        let items = access_at(&db, 103 + RETENTION_SECONDS, read_items).unwrap();
        assert_eq!(items.iter().map(|item| item.id).collect::<Vec<_>>(), [3]);
    }

    #[test]
    fn removal_undo_preserves_identity_and_completion_and_expires() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shopping.sqlite");
        crate::database::initialize(&path).unwrap();
        let db = crate::database::open(&path).unwrap();
        setup(&db);
        assert_eq!(change(&db, 100, 1, add("Milk", "2 litres")), None);
        assert_eq!(
            change(
                &db,
                100,
                2,
                ShoppingChange::SetChecked {
                    id: 1,
                    checked: true
                }
            ),
            None
        );
        assert_eq!(change(&db, 101, 2, ShoppingChange::Remove { id: 1 }), None);
        assert!(read_items(&db, 101).unwrap().is_empty());
        assert!(change(&db, 102, 1, ShoppingChange::Restore { id: 1 }).is_some());
        assert_eq!(change(&db, 102, 2, ShoppingChange::Restore { id: 1 }), None);
        let items = read_items(&db, 102).unwrap();
        assert_eq!(items[0].id, 1);
        assert_eq!(items[0].quantity.as_deref(), Some("2 litres"));
        assert!(items[0].checked);
        assert_eq!(change(&db, 103, 2, ShoppingChange::Remove { id: 1 }), None);
        assert!(change(
            &db,
            100 + RETENTION_SECONDS,
            2,
            ShoppingChange::Restore { id: 1 }
        )
        .is_some());
        assert_eq!(
            change(&db, 100 + RETENTION_SECONDS, 2, add("Eggs", "")),
            None
        );
        assert_eq!(
            change(
                &db,
                101 + RETENTION_SECONDS,
                2,
                ShoppingChange::Remove { id: 2 }
            ),
            None
        );
        assert!(change(
            &db,
            101 + 2 * RETENTION_SECONDS,
            2,
            ShoppingChange::Restore { id: 2 }
        )
        .is_some());
    }
}
