CREATE TABLE google_calendar_connections (
    user_id INTEGER PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    refresh_token BLOB NOT NULL
);

CREATE TABLE google_calendar_attempts (
    user_id INTEGER PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    session_hash BLOB NOT NULL REFERENCES sessions (token_hash) ON DELETE CASCADE,
    state_hash BLOB NOT NULL,
    verifier TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    claimed INTEGER NOT NULL DEFAULT 0 CHECK (claimed IN (0, 1))
);
