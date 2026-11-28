CREATE TABLE guest_sessions (
    token_hash BLOB PRIMARY KEY NOT NULL,
    expires_at INTEGER NOT NULL
);

CREATE INDEX guest_sessions_by_expiry ON guest_sessions (expires_at);
