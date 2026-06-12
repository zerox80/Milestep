-- Sliding session expiry needs the creation time to enforce a hard cap.
ALTER TABLE sessions ADD COLUMN created_at TIMESTAMPTZ NOT NULL DEFAULT now();
-- Existing sessions were issued with a fixed 14-day TTL; approximate.
UPDATE sessions SET created_at = expires_at - INTERVAL '14 days';
