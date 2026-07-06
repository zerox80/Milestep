-- Keep subtask order deterministic even after concurrent inserts.
-- Existing rows are normalized first so adding the unique index is safe.
WITH ranked AS (
  SELECT
    id,
    (ROW_NUMBER() OVER (PARTITION BY task_id ORDER BY position, id) - 1)::INT AS new_position
  FROM subtasks
)
UPDATE subtasks s
SET position = ranked.new_position
FROM ranked
WHERE s.id = ranked.id
  AND s.position <> ranked.new_position;

CREATE UNIQUE INDEX IF NOT EXISTS subtasks_task_position_key
  ON subtasks (task_id, position);
