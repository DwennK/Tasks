ALTER TABLE reminders ADD COLUMN generation TEXT NOT NULL DEFAULT '';
UPDATE reminders SET generation=lower(hex(randomblob(16))) WHERE generation='';
ALTER TABLE comments ADD COLUMN version INTEGER NOT NULL DEFAULT 1;
