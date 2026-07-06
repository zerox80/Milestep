ALTER TABLE tasks ADD COLUMN recurrence TEXT
  CHECK (recurrence IN ('daily', 'weekly', 'biweekly', 'monthly'));
