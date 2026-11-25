CREATE TABLE activity_profile_links (
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    external_profile_id INTEGER NOT NULL CHECK (external_profile_id > 0),
    PRIMARY KEY (user_id, provider),
    UNIQUE (provider, external_profile_id)
);
