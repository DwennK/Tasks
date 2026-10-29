ALTER TABLE tasks ADD COLUMN details_json TEXT NOT NULL DEFAULT '{}';
ALTER TABLE tasks ADD COLUMN recurrence_source_id TEXT REFERENCES tasks(id);
CREATE UNIQUE INDEX tasks_recurrence_source ON tasks(recurrence_source_id) WHERE recurrence_source_id IS NOT NULL;
ALTER TABLE lists ADD COLUMN archived_at TEXT;
ALTER TABLE lists ADD COLUMN preferences_json TEXT NOT NULL DEFAULT '{}';
CREATE TABLE tags (
 id TEXT PRIMARY KEY NOT NULL, name TEXT NOT NULL COLLATE NOCASE UNIQUE,
 color TEXT NOT NULL, position INTEGER NOT NULL,
 created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE TABLE task_tags (
 task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
 tag_id TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
 PRIMARY KEY(task_id,tag_id)
);
CREATE INDEX task_tags_tag ON task_tags(tag_id);
CREATE TABLE task_links (
 task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
 linked_task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
 PRIMARY KEY(task_id,linked_task_id), CHECK(task_id<>linked_task_id)
);
CREATE INDEX task_links_target ON task_links(linked_task_id);
CREATE TABLE attachments (
 id TEXT PRIMARY KEY NOT NULL, task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
 name TEXT NOT NULL, mime_type TEXT NOT NULL, size INTEGER NOT NULL,
 content BLOB NOT NULL, created_at TEXT NOT NULL
);
CREATE INDEX attachments_task ON attachments(task_id);
CREATE TABLE reminders (
 task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
 at TEXT NOT NULL, notified_at TEXT, PRIMARY KEY(task_id,at)
);
INSERT INTO reminders(task_id,at,notified_at)
 SELECT id,reminder_at,reminder_notified_at FROM tasks WHERE reminder_at IS NOT NULL;
CREATE INDEX reminders_due ON reminders(at) WHERE notified_at IS NULL;
CREATE TABLE sections (
 id TEXT PRIMARY KEY NOT NULL, list_id TEXT NOT NULL REFERENCES lists(id) ON DELETE CASCADE,
 name TEXT NOT NULL, position INTEGER NOT NULL,
 created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE INDEX sections_list ON sections(list_id,position);
CREATE INDEX tasks_section ON tasks(json_extract(details_json,'$.section_id'));
CREATE TABLE templates (
 id TEXT PRIMARY KEY NOT NULL, name TEXT NOT NULL, task_json TEXT NOT NULL,
 created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE TABLE comments (
 id TEXT PRIMARY KEY NOT NULL, task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
 body TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE INDEX comments_task ON comments(task_id,created_at);
