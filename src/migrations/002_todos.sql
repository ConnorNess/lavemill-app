CREATE TABLE todos (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL CHECK (length(trim(title)) > 0),
    description TEXT,
    due TEXT NOT NULL,
    reporter_id INTEGER NOT NULL REFERENCES users (id),
    assignee_id INTEGER REFERENCES users (id),
    status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'completed', 'failed')),
    completed_by INTEGER REFERENCES users (id),
    completed_at INTEGER,
    previous_assignee_id INTEGER REFERENCES users (id),
    CHECK (
        (status = 'completed' AND completed_by IS NOT NULL AND completed_at IS NOT NULL)
        OR (status != 'completed' AND completed_by IS NULL AND completed_at IS NULL
            AND previous_assignee_id IS NULL)
    )
);
