-- Expired invites must no longer block re-inviting the same address, and a
-- workspace should hold at most one pending invite per email. Clean up first
-- (nothing enforced uniqueness before), then add the constraint.

-- Drop expired leftovers; they are no longer redeemable.
DELETE FROM workspace_invites WHERE expires_at <= now();

-- Collapse any duplicate (workspace_id, email) rows, keeping the newest.
DELETE FROM workspace_invites a
USING workspace_invites b
WHERE a.workspace_id = b.workspace_id
  AND a.email = b.email
  AND (a.created_at, a.id) < (b.created_at, b.id);

-- One pending invite per (workspace, email); re-invites refresh this row via
-- ON CONFLICT instead of stacking dead tokens.
ALTER TABLE workspace_invites
  ADD CONSTRAINT workspace_invites_workspace_email_key UNIQUE (workspace_id, email);
