ALTER TABLE google_calendar_connections ADD COLUMN calendar_id TEXT;
ALTER TABLE google_calendar_connections ADD COLUMN shared INTEGER NOT NULL DEFAULT 0
    CHECK (shared IN (0, 1) AND (shared = 0 OR calendar_id IS NOT NULL));
