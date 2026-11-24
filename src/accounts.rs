use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use rusqlite::{params, Connection, ErrorCode, OptionalExtension, TransactionBehavior};

pub fn username(value: &str) -> Option<String> {
    let value = value.trim();
    if !(1..=40).contains(&value.len())
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return None;
    }
    Some(value.to_ascii_lowercase())
}

pub fn details(
    username_input: &str,
    first: &str,
    last: &str,
) -> Result<(String, String, String), &'static str> {
    let username = username(username_input)
        .ok_or("Use 1–40 letters, digits, dots, hyphens or underscores for the username.")?;
    let first = first.trim();
    let last = last.trim();
    if [first, last]
        .iter()
        .any(|name| !(1..=80).contains(&name.chars().count()) || name.chars().any(char::is_control))
    {
        return Err("Enter a first and last name of 1–80 characters each.");
    }
    Ok((username, first.to_owned(), last.to_owned()))
}

pub fn hash_password(password: &str) -> Result<String, &'static str> {
    if password.len() > 1024 || !(15..=128).contains(&password.chars().count()) {
        return Err("Use a password of 15–128 characters.");
    }
    let mut random = [0; 16];
    getrandom::fill(&mut random).map_err(|_| "Could not generate password salt.")?;
    let salt = SaltString::encode_b64(&random).map_err(|_| "Could not encode password salt.")?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| "Could not hash the password.")
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    password.len() <= 1024
        && PasswordHash::new(hash).is_ok_and(|hash| {
            Argon2::default()
                .verify_password(password.as_bytes(), &hash)
                .is_ok()
        })
}

pub fn save_error(error: rusqlite::Error) -> &'static str {
    if error.sqlite_error_code() == Some(ErrorCode::ConstraintViolation) {
        return "That username is already in use.";
    }
    database_error(error)
}

pub fn database_error(error: rusqlite::Error) -> &'static str {
    eprintln!("Account database operation failed: {error}");
    "The account database is unavailable. Try again."
}

fn create_admin(
    connection: &mut Connection,
    details: (String, String, String),
    hash: &str,
) -> Result<(), &'static str> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(database_error)?;
    let exists: bool = transaction
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM users WHERE is_admin = 1)",
            [],
            |row| row.get(0),
        )
        .map_err(database_error)?;
    if exists {
        return Err("The admin account already exists.");
    }
    transaction.execute(
        "INSERT INTO users (username, first_name, last_name, password_hash, is_admin) VALUES (?1, ?2, ?3, ?4, 1)",
        params![details.0, details.1, details.2, hash],
    ).map_err(save_error)?;
    transaction.commit().map_err(database_error)
}

pub fn replace_password(
    connection: &mut Connection,
    id: i64,
    hash: &str,
) -> Result<(), &'static str> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(database_error)?;
    let changed = transaction
        .execute(
            "UPDATE users SET password_hash = ?1 WHERE id = ?2",
            params![hash, id],
        )
        .map_err(database_error)?;
    if changed == 0 {
        return Err("User not found.");
    }
    transaction
        .execute("DELETE FROM sessions WHERE user_id = ?1", [id])
        .map_err(database_error)?;
    transaction.commit().map_err(database_error)
}

pub fn admin_command(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let path = crate::database::configured_path();
    let mut connection = crate::database::open(&path)?;
    match args {
        [command, username, first, last] if command == "init" => {
            let details = details(username, first, last)?;
            create_admin(&mut connection, details, &prompt_hash()?)?;
            println!("Admin account created.");
        }
        [command] if command == "reset-password" => {
            let id = connection
                .query_row("SELECT id FROM users WHERE is_admin = 1", [], |row| {
                    row.get(0)
                })
                .optional()?
                .ok_or("Run admin init first.")?;
            replace_password(&mut connection, id, &prompt_hash()?)?;
            println!("Admin password reset and sessions revoked.");
        }
        _ => {
            return Err(
                "Usage: admin init USERNAME FIRST_NAME LAST_NAME | admin reset-password".into(),
            )
        }
    }
    Ok(())
}

fn prompt_hash() -> Result<String, Box<dyn std::error::Error>> {
    let password = rpassword::prompt_password("Password: ")?;
    let confirmation = rpassword::prompt_password("Confirm password: ")?;
    if password != confirmation {
        return Err("Passwords do not match.".into());
    }
    Ok(hash_password(&password)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_persists_one_admin_and_reset_revokes_sessions() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("accounts.sqlite3");
        let old_hash = hash_password("first lengthy password").unwrap();
        let new_hash = hash_password("second lengthy password").unwrap();
        crate::database::initialize(&path).unwrap();
        let mut db = crate::database::open(&path).unwrap();
        create_admin(
            &mut db,
            details("Connor", "Connor", "Example").unwrap(),
            &old_hash,
        )
        .unwrap();
        assert!(create_admin(
            &mut db,
            details("other", "Other", "Example").unwrap(),
            &old_hash
        )
        .is_err());
        assert!(db.execute("INSERT INTO users (username, first_name, last_name, password_hash) VALUES ('CONNOR', 'A', 'B', 'unused')", []).is_err());
        db.execute("INSERT INTO sessions VALUES (X'01', 1, 99)", [])
            .unwrap();
        drop(db);
        crate::database::initialize(&path).unwrap();
        let mut db = crate::database::open(&path).unwrap();
        replace_password(&mut db, 1, &new_hash).unwrap();
        let hash: String = db
            .query_row("SELECT password_hash FROM users WHERE id = 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(!verify_password("first lengthy password", &hash));
        assert!(verify_password("second lengthy password", &hash));
        assert_eq!(
            db.query_row("SELECT count(*) FROM sessions", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}
