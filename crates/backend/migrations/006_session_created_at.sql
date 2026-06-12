-- Sliding session expiry needs the creation time to enforce a hard cap.
-- Idempotent: some databases already have this column (added out of band or
-- by an earlier run that did not record the migration), so only add and
-- back-fill it when it is actually missing — re-running must not fail or
-- clobber existing created_at values.
DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM information_schema.columns
    WHERE table_name = 'sessions' AND column_name = 'created_at'
  ) THEN
    ALTER TABLE sessions ADD COLUMN created_at TIMESTAMPTZ NOT NULL DEFAULT now();
    -- Existing sessions were issued with a fixed 14-day TTL; approximate.
    UPDATE sessions SET created_at = expires_at - INTERVAL '14 days';
  END IF;
END $$;
