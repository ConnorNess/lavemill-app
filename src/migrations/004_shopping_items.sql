CREATE TABLE shopping_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    quantity TEXT,
    created_by INTEGER NOT NULL REFERENCES users (id),
    completed_at INTEGER,
    removed_at INTEGER,
    removed_by INTEGER REFERENCES users (id),
    CHECK ((removed_at IS NULL) = (removed_by IS NULL))
);

CREATE INDEX shopping_items_completed_at ON shopping_items (completed_at)
    WHERE completed_at IS NOT NULL;
CREATE INDEX shopping_items_removed_at ON shopping_items (removed_at)
    WHERE removed_at IS NOT NULL;
