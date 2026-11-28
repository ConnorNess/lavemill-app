ALTER TABLE todos ADD COLUMN missed_on TEXT;

DELETE FROM todos AS missed
WHERE missed.status = 'failed' AND missed.series_id IS NOT NULL
  AND EXISTS (
      SELECT 1 FROM todos AS newer
      WHERE newer.series_id = missed.series_id AND newer.status = 'failed' AND newer.due > missed.due
  );

UPDATE todos SET missed_on = COALESCE(next_due, due) WHERE status = 'failed';

CREATE UNIQUE INDEX todos_latest_miss ON todos (series_id)
    WHERE status = 'failed' AND series_id IS NOT NULL;
CREATE INDEX todos_miss_expiry ON todos (missed_on) WHERE status = 'failed';
