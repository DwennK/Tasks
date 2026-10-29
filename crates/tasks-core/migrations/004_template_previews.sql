-- Keep saved attachment payloads out of regular UI snapshots.
ALTER TABLE templates ADD COLUMN preview_json TEXT NOT NULL DEFAULT '{}';
