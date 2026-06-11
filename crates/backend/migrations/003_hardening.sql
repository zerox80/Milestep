-- Invites become single-use random tokens instead of being redeemable by
-- whoever registers the invited email address (no email verification exists).
-- The table only ever held rows created via the API-only invite endpoint, so
-- clearing it is safe.
DELETE FROM workspace_invites;

ALTER TABLE workspace_invites ADD COLUMN token_hash TEXT NOT NULL UNIQUE;
ALTER TABLE workspace_invites ADD COLUMN expires_at TIMESTAMPTZ NOT NULL;
