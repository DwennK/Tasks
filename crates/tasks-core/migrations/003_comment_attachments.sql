CREATE TABLE comment_attachments (
 comment_id TEXT NOT NULL REFERENCES comments(id) ON DELETE CASCADE,
 attachment_id TEXT NOT NULL REFERENCES attachments(id) ON DELETE CASCADE,
 PRIMARY KEY(comment_id,attachment_id)
);
CREATE INDEX comment_attachments_file ON comment_attachments(attachment_id);
