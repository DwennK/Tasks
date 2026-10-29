CREATE TABLE folders (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL CHECK(length(trim(name)) > 0),
    parent_id TEXT REFERENCES folders(id) ON DELETE RESTRICT,
    position INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK(parent_id IS NULL OR parent_id <> id)
);
CREATE INDEX folders_parent_position ON folders(parent_id, position);
CREATE TABLE lists (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL CHECK(length(trim(name)) > 0),
    folder_id TEXT REFERENCES folders(id) ON DELETE RESTRICT,
    color TEXT NOT NULL,
    position INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX lists_folder_position ON lists(folder_id, position);
CREATE TABLE tasks (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL CHECK(length(trim(title)) > 0),
    notes TEXT NOT NULL DEFAULT '',
    list_id TEXT NOT NULL REFERENCES lists(id) ON DELETE RESTRICT,
    parent_id TEXT REFERENCES tasks(id) ON DELETE RESTRICT,
    due_date TEXT,
    due_time TEXT,
    reminder_at TEXT,
    reminder_notified_at TEXT,
    priority INTEGER NOT NULL DEFAULT 0 CHECK(priority BETWEEN 0 AND 3),
    position INTEGER NOT NULL,
    completed_at TEXT,
    deleted_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    CHECK(parent_id IS NULL OR parent_id <> id),
    CHECK(due_time IS NULL OR due_date IS NOT NULL)
);
CREATE INDEX tasks_list_position ON tasks(list_id, parent_id, position);
CREATE INDEX tasks_due ON tasks(due_date) WHERE deleted_at IS NULL AND completed_at IS NULL;
CREATE INDEX tasks_parent ON tasks(parent_id);
CREATE INDEX tasks_reminder ON tasks(reminder_at) WHERE deleted_at IS NULL AND completed_at IS NULL;
CREATE TABLE settings (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL);
CREATE TABLE metadata (key TEXT PRIMARY KEY NOT NULL, value INTEGER NOT NULL);
INSERT INTO metadata(key, value) VALUES('revision', 0);
