CREATE TABLE calendar_connections (
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    provider TEXT NOT NULL CHECK (provider IN ('google', 'outlook')),
    id BLOB NOT NULL DEFAULT (randomblob(16)),
    refresh_token BLOB NOT NULL,
    calendar_id TEXT,
    shared INTEGER NOT NULL DEFAULT 0
        CHECK (shared IN (0, 1) AND (shared = 0 OR calendar_id IS NOT NULL)),
    PRIMARY KEY (user_id, provider)
);

CREATE TABLE calendar_attempts (
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    provider TEXT NOT NULL CHECK (provider IN ('google', 'outlook')),
    session_hash BLOB NOT NULL REFERENCES sessions (token_hash) ON DELETE CASCADE,
    state_hash BLOB NOT NULL,
    verifier TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    claimed INTEGER NOT NULL DEFAULT 0 CHECK (claimed IN (0, 1)),
    PRIMARY KEY (user_id, provider)
);

INSERT INTO calendar_connections (user_id, provider, refresh_token, calendar_id, shared)
SELECT user_id, 'google', refresh_token, calendar_id, shared FROM google_calendar_connections;

INSERT INTO calendar_attempts (user_id, provider, session_hash, state_hash, verifier, expires_at, claimed)
SELECT user_id, 'google', session_hash, state_hash, verifier, expires_at, claimed FROM google_calendar_attempts;

DROP TABLE google_calendar_attempts;
DROP TABLE google_calendar_connections;
