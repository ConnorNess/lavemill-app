CREATE TABLE todos_with_stable_ids (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL CHECK (length(trim(title)) > 0),
    description TEXT,
    due TEXT NOT NULL,
    reporter_id INTEGER NOT NULL REFERENCES users (id),
    assignee_id INTEGER REFERENCES users (id),
    status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'completed', 'failed')),
    completed_by INTEGER REFERENCES users (id),
    completed_at INTEGER,
    previous_assignee_id INTEGER REFERENCES users (id),
    series_id INTEGER REFERENCES todo_series (id),
    next_due TEXT,
    CHECK (
        (status = 'completed' AND completed_by IS NOT NULL AND completed_at IS NOT NULL)
        OR (status != 'completed' AND completed_by IS NULL AND completed_at IS NULL
            AND previous_assignee_id IS NULL)
    )
);

INSERT INTO todos_with_stable_ids (
    id, title, description, due, reporter_id, assignee_id, status,
    completed_by, completed_at, previous_assignee_id, series_id, next_due
)
SELECT id, title, description, due, reporter_id, assignee_id, status,
    completed_by, completed_at, previous_assignee_id, series_id, next_due
FROM todos;

DROP TABLE todos;
ALTER TABLE todos_with_stable_ids RENAME TO todos;

CREATE UNIQUE INDEX todos_series_due ON todos (series_id, due);
CREATE INDEX todos_open_next_due ON todos (next_due)
    WHERE status = 'open' AND next_due IS NOT NULL;
