ALTER TABLE project_statuses ADD COLUMN is_done BOOLEAN NOT NULL DEFAULT false;

-- Preserve the previous convention where the last (fourth) column was "done".
UPDATE project_statuses SET is_done = true WHERE position = 3;

CREATE INDEX sessions_expires_idx ON sessions(expires_at);
CREATE INDEX sessions_user_idx ON sessions(user_id);
