-- Local, bounded undo receipts. Never included in cloud synchronization.
CREATE TABLE undo_entries (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    token TEXT NOT NULL UNIQUE,
    changes_json TEXT NOT NULL
);
