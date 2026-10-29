-- Changes are stored as field deltas, never as attachment content or duplicate
-- full-task snapshots. Existing tasks start recording at their next mutation.
CREATE TABLE task_activities (
 id INTEGER PRIMARY KEY AUTOINCREMENT,
 task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
 action TEXT NOT NULL,
 created_at TEXT NOT NULL,
 before_json TEXT NOT NULL,
 after_json TEXT NOT NULL
);
CREATE INDEX task_activities_task ON task_activities(task_id,id DESC);
