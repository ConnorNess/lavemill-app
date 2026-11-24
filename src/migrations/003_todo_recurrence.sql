CREATE TABLE todo_series (
    id INTEGER PRIMARY KEY,
    frequency TEXT NOT NULL CHECK (frequency IN ('daily', 'weekly', 'monthly')),
    first_due TEXT NOT NULL,
    usual_assignee_id INTEGER REFERENCES users (id)
);

ALTER TABLE todos ADD COLUMN series_id INTEGER REFERENCES todo_series (id);
ALTER TABLE todos ADD COLUMN next_due TEXT;

CREATE UNIQUE INDEX todos_series_due ON todos (series_id, due);
CREATE INDEX todos_open_next_due ON todos (next_due)
    WHERE status = 'open' AND next_due IS NOT NULL;
