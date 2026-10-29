//! Transactional task activities. Only changed fields are persisted, and file
//! bytes are never selected. Capturing the whole task graph also covers implicit
//! ancestor, descendant, recurring-instance and relationship changes.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) type Snapshot = BTreeMap<String, Map<String, Value>>;

pub(super) fn records_action(action: &str) -> bool {
    action.starts_with("tasks.")
        || action.starts_with("attachments.")
        || action.starts_with("comments.")
        || action.starts_with("links.")
        || action.starts_with("sections.")
        || matches!(
            action,
            "tags.delete"
                | "templates.apply"
                | "lists.delete"
                | "lists.duplicate"
                | "folders.delete"
                | "reminders.resolve"
                | "sync.merge"
                | "sync.resolve"
        )
}

pub(super) fn capture(conn: &Connection) -> Result<Snapshot> {
    let mut snapshot = Snapshot::new();
    // Deliberately do not hydrate each task: a fixed number of queries avoids
    // issuing many queries per task for every autosaved title or note change.
    let mut statement = conn.prepare(&format!("SELECT {TASK_COLUMNS} FROM tasks"))?;
    let tasks = statement.query_map([], task_row)?;
    for task in tasks {
        let task = task?;
        let value = serde_json::to_value(&task)?;
        let mut fields = value
            .as_object()
            .cloned()
            .expect("Task serializes as an object");
        for key in [
            "id",
            "notes_text",
            "created_at",
            "updated_at",
            "version",
            "progress_mode",
        ] {
            fields.remove(key);
        }
        // Keep one coherent content version, only when any of its three fields
        // changes. Scheduling/priority edits never duplicate the task's notes.
        let content = json!({
            "title": fields.remove("title").unwrap_or(Value::Null),
            "notes": fields.remove("notes").unwrap_or(Value::Null),
            "notes_format": fields.remove("notes_format").unwrap_or(json!("plain")),
        });
        fields.insert("content".into(), content);
        fields.insert("comment_attachments".into(), json!([]));
        snapshot.insert(task.id, fields);
    }
    fill_relation(
        conn,
        &mut snapshot,
        "tag_ids",
        "SELECT task_id,tag_id FROM task_tags ORDER BY task_id,tag_id",
        |row| Ok(json!(row.get::<_, String>(1)?)),
    )?;
    fill_relation(conn, &mut snapshot, "linked_task_ids", "SELECT task_id,linked_task_id FROM task_links UNION SELECT linked_task_id,task_id FROM task_links ORDER BY 1,2", |row| {
        Ok(json!(row.get::<_, String>(1)?))
    })?;
    fill_relation(
        conn,
        &mut snapshot,
        "reminders",
        "SELECT task_id,at FROM reminders ORDER BY task_id,at",
        |row| Ok(json!(row.get::<_, String>(1)?)),
    )?;
    fill_relation(
        conn,
        &mut snapshot,
        "attachments",
        "SELECT task_id,id,name,mime_type,size FROM attachments ORDER BY task_id,id",
        |row| {
            Ok(
                json!({"id":row.get::<_,String>(1)?,"name":row.get::<_,String>(2)?,"mime_type":row.get::<_,String>(3)?,"size":row.get::<_,i64>(4)?}),
            )
        },
    )?;
    fill_relation(
        conn,
        &mut snapshot,
        "comments",
        "SELECT task_id,id,body FROM comments ORDER BY task_id,id",
        |row| Ok(json!({"id":row.get::<_,String>(1)?,"body":row.get::<_,String>(2)?})),
    )?;
    fill_relation(conn, &mut snapshot, "comment_attachments", "SELECT c.task_id,ca.comment_id,ca.attachment_id FROM comment_attachments ca JOIN comments c ON c.id=ca.comment_id ORDER BY c.task_id,ca.comment_id,ca.attachment_id", |row| {
        Ok(json!({"comment_id":row.get::<_,String>(1)?,"attachment_id":row.get::<_,String>(2)?}))
    })?;
    Ok(snapshot)
}

fn fill_relation(
    conn: &Connection,
    snapshot: &mut Snapshot,
    key: &str,
    sql: &str,
    value: impl Fn(&Row<'_>) -> rusqlite::Result<Value>,
) -> Result<()> {
    let mut statement = conn.prepare(sql)?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        if let Some(fields) = snapshot.get_mut(&id) {
            if let Some(items) = fields.get_mut(key).and_then(Value::as_array_mut) {
                items.push(value(row)?);
            }
        }
    }
    Ok(())
}

pub(super) fn record(conn: &Connection, action: &str, before: Snapshot) -> Result<()> {
    let after = capture(conn)?;
    let time = now();
    let mut insert = conn.prepare("INSERT INTO task_activities(task_id,action,created_at,before_json,after_json) VALUES(?1,?2,?3,?4,?5)")?;
    for (id, current) in after {
        let (old, new) = if let Some(previous) = before.get(&id) {
            let keys: BTreeSet<_> = previous.keys().chain(current.keys()).collect();
            let mut old = Map::new();
            let mut new = Map::new();
            for key in keys {
                if previous.get(key) != current.get(key) {
                    old.insert(
                        key.clone(),
                        previous.get(key).cloned().unwrap_or(Value::Null),
                    );
                    new.insert(
                        key.clone(),
                        current.get(key).cloned().unwrap_or(Value::Null),
                    );
                }
            }
            if new.is_empty() {
                continue;
            }
            (Value::Object(old), Value::Object(new))
        } else {
            (Value::Null, Value::Object(current))
        };
        insert.execute(params![
            id,
            action,
            time,
            serde_json::to_string(&old)?,
            serde_json::to_string(&new)?
        ])?;
    }
    Ok(())
}

fn positive_integer(data: &Map<String, Value>, key: &str) -> Result<i64> {
    data.get(key)
        .and_then(Value::as_i64)
        .filter(|n| *n > 0)
        .ok_or_else(|| ServiceError::Invalid(format!("{key} must be a positive integer.")))
}

pub(super) fn read(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    fields(data, &["id", "before_id", "limit"])?;
    let id = required(data, "id")?;
    let task = get_task(conn, id)?;
    let limit = if data.contains_key("limit") {
        positive_integer(data, "limit")?
    } else {
        30
    };
    if limit > 100 {
        return invalid("limit must not exceed 100.");
    }
    let before_id = if data.contains_key("before_id") {
        Some(positive_integer(data, "before_id")?)
    } else {
        None
    };
    let mut stmt = conn.prepare("SELECT id,action,created_at,before_json,after_json FROM task_activities WHERE task_id=?1 AND (?2 IS NULL OR id<?2) ORDER BY id DESC LIMIT ?3")?;
    let mut rows = stmt.query(params![id, before_id, limit + 1])?;
    let mut items = Vec::new();
    while let Some(row) = rows.next()? {
        let before: Value = serde_json::from_str(&row.get::<_, String>(3)?)?;
        let after: Value = serde_json::from_str(&row.get::<_, String>(4)?)?;
        let changed_fields: Vec<_> = after
            .as_object()
            .into_iter()
            .flat_map(|map| map.keys())
            .collect();
        items.push(json!({
            "id":row.get::<_,i64>(0)?,
            "action":row.get::<_,String>(1)?,
            "created_at":row.get::<_,String>(2)?,
            "changed_fields":changed_fields,
            "before":before,
            "after":after,
        }));
    }
    let has_more = items.len() > limit as usize;
    items.truncate(limit as usize);
    let next_before_id = if has_more {
        items.last().map(|item| item["id"].clone())
    } else {
        None
    };
    Ok(
        json!({"items":items,"next_before_id":next_before_id,"task_version":task.version,"task_deleted":task.deleted_at.is_some()}),
    )
}

pub(super) fn restore_content(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    fields(data, &["id", "activity_id", "side", "expected_version"])?;
    let id = required(data, "id")?;
    let activity_id = positive_integer(data, "activity_id")?;
    let expected_version = positive_integer(data, "expected_version")?;
    let column = match required(data, "side")? {
        "before" => "before_json",
        "after" => "after_json",
        _ => return invalid("side must be before or after."),
    };
    let task = get_task(conn, id)?;
    if task.version != expected_version {
        return Err(ServiceError::Conflict);
    }
    if task.deleted_at.is_some() {
        return invalid("Restore the task from Trash before restoring its content.");
    }
    let serialized: String = conn
        .query_row(
            &format!("SELECT {column} FROM task_activities WHERE id=?1 AND task_id=?2"),
            params![activity_id, id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| ServiceError::NotFound("Task activity".into()))?;
    let snapshot: Value = serde_json::from_str(&serialized)?;
    let content = snapshot
        .get("content")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ServiceError::Invalid("This activity has no content version to restore.".into())
        })?;
    let mut patch = Map::new();
    patch.insert("id".into(), json!(id));
    patch.insert("expected_version".into(), json!(expected_version));
    for key in ["title", "notes", "notes_format"] {
        patch.insert(
            key.into(),
            content.get(key).cloned().ok_or_else(|| {
                ServiceError::Invalid(
                    "This activity contains an incomplete content version.".into(),
                )
            })?,
        );
    }
    // All normal validation, note sanitization and conflict handling still apply.
    // The lifecycle, dates, priority, relations and checklist are never replayed.
    update_task(conn, &patch)
}
