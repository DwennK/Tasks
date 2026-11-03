//! Transactional undo receipts, authored by the service rather than the client.
//! Only supported task operations are recorded; attachment bytes stay in place.
use super::*;
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize)]
struct Change {
    before: Option<Map<String, Value>>,
    after: Map<String, Value>,
}
type Changes = BTreeMap<String, Change>;

pub(super) fn supported(action: &str) -> bool {
    matches!(
        action,
        "tasks.create"
            | "tasks.update"
            | "tasks.complete"
            | "tasks.delete"
            | "tasks.restore"
            | "tasks.reorder"
            | "tasks.tags"
            | "tasks.duplicate"
            | "tasks.wont_do"
            | "templates.apply"
    )
}

pub(super) fn capture(conn: &Connection) -> Result<history::Snapshot> {
    let mut state = history::capture(conn)?;
    let mut statement = conn
        .prepare("SELECT id,details_json,recurrence_source_id,parent_id FROM tasks ORDER BY id")?;
    let mut rows = statement.query([])?;
    let mut children: BTreeMap<String, Vec<String>> = BTreeMap::new();
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let fields = state.get_mut(&id).expect("captured task");
        // Preserve stored manual progress, not its derived display value.
        fields.insert("_details".into(), json!(row.get::<_, String>(1)?));
        fields.insert("_source".into(), json!(row.get::<_, Option<String>>(2)?));
        if let Some(parent) = row.get::<_, Option<String>>(3)? {
            children.entry(parent).or_default().push(id);
        }
    }
    for (id, fields) in &mut state {
        fields.insert(
            "_children".into(),
            json!(children.remove(id).unwrap_or_default()),
        );
    }
    Ok(state)
}

pub(super) fn record(conn: &Connection, before: history::Snapshot) -> Result<Value> {
    let after = capture(conn)?;
    let changes: Changes = after
        .into_iter()
        .filter_map(|(id, after)| {
            let old = before.get(&id);
            (old != Some(&after)).then(|| {
                (
                    id,
                    Change {
                        before: old.cloned(),
                        after,
                    },
                )
            })
        })
        .collect();
    if changes.is_empty() {
        return Ok(json!({"token":null,"changed":false}));
    }
    let token = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO undo_entries(token,changes_json) VALUES(?1,?2)",
        params![token, serde_json::to_string(&changes)?],
    )?;
    // Bound both count and storage, including large rich-text notes. An expired
    // receipt fails explicitly instead of silently undoing a different action.
    conn.execute("DELETE FROM undo_entries WHERE sequence NOT IN (SELECT sequence FROM undo_entries ORDER BY sequence DESC LIMIT 100)", [])?;
    conn.execute("DELETE FROM undo_entries WHERE sequence IN (SELECT sequence FROM (SELECT sequence,SUM(length(CAST(changes_json AS BLOB))) OVER (ORDER BY sequence DESC) AS bytes FROM undo_entries) WHERE bytes>16777216)", [])?;
    Ok(json!({"token":token,"changed":true}))
}

pub(super) fn apply(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    fields(data, &["tokens"])?;
    let tokens = data
        .get("tokens")
        .and_then(Value::as_array)
        .filter(|tokens| !tokens.is_empty() && tokens.len() <= 100)
        .ok_or_else(|| ServiceError::Invalid("Supply between 1 and 100 undo tokens.".into()))?;
    let mut seen = HashSet::new();
    // Undo a multi-selection as one transaction, newest receipt first.
    for value in tokens.iter().rev() {
        let token = value
            .as_str()
            .filter(|token| seen.insert(*token))
            .ok_or_else(|| ServiceError::Invalid("Undo tokens must be unique strings.".into()))?;
        let stored: Option<String> = conn
            .query_row(
                "SELECT changes_json FROM undo_entries WHERE token=?1",
                [token],
                |row| row.get(0),
            )
            .optional()?;
        let changes: Changes = serde_json::from_str(&stored.ok_or_else(|| {
            ServiceError::Invalid("This action is no longer available to undo.".into())
        })?)?;
        let current = capture(conn)?;
        for (id, change) in &changes {
            if current.get(id) != Some(&change.after) {
                return invalid("Cannot undo: an affected task changed since this action. Your current changes have been kept.");
            }
        }
        // References may temporarily point at a newly created occurrence while
        // its source is being restored. All constraints are checked at commit.
        conn.pragma_update(None, "defer_foreign_keys", true)?;
        for (id, change) in &changes {
            if let Some(before) = &change.before {
                restore(conn, id, before)?;
            }
        }
        for (id, change) in &changes {
            if change.before.is_none() {
                // Newly created/duplicated occurrences have no previous state.
                // Their full metadata/relations and children were checked above.
                conn.execute("DELETE FROM tasks WHERE id=?1", [id])?;
            }
        }
        validate_graph(conn)?;
        conn.execute("DELETE FROM undo_entries WHERE token=?1", [token])?;
    }
    Ok(json!({"ok":true}))
}

fn restore(conn: &Connection, id: &str, fields: &Map<String, Value>) -> Result<()> {
    let value = |key: &str| -> rusqlite::types::Value {
        match &fields[key] {
            Value::String(value) => value.clone().into(),
            Value::Number(value) => value.as_i64().expect("stored integer").into(),
            Value::Null => rusqlite::types::Value::Null,
            _ => unreachable!("stored scalar"),
        }
    };
    conn.execute("UPDATE tasks SET title=?2,notes=?3,list_id=?4,parent_id=?5,due_date=?6,due_time=?7,reminder_at=?8,priority=?9,position=?10,completed_at=?11,deleted_at=?12,details_json=?13,recurrence_source_id=?14,updated_at=?15,version=version+1,reminder_notified_at=CASE WHEN reminder_at IS ?8 THEN reminder_notified_at ELSE NULL END WHERE id=?1", params![
        id, fields["content"]["title"].as_str(), fields["content"]["notes"].as_str(),
        value("list_id"),value("parent_id"),value("due_date"),value("due_time"),value("reminder_at"),
        value("priority"),value("position"),value("completed_at"),value("deleted_at"),value("_details"),value("_source"),now()
    ])?;
    relations::replace_tags(conn, id, &fields["tag_ids"])?;
    let desired = fields["reminders"].as_array().expect("stored reminders");
    let previous = get_task(conn, id)?.reminders;
    for at in previous {
        if !desired.contains(&json!(at)) {
            conn.execute(
                "DELETE FROM reminders WHERE task_id=?1 AND at=?2",
                params![id, at],
            )?;
        }
    }
    for at in desired {
        conn.execute(
            "INSERT OR IGNORE INTO reminders(task_id,at,generation) VALUES(?1,?2,?3)",
            params![id, at.as_str(), Uuid::new_v4().to_string()],
        )?;
    }
    Ok(())
}

fn validate_graph(conn: &Connection) -> Result<()> {
    // A referenced list/section or ancestor may have changed independently.
    // Validate the final graph before committing any part of the undo.
    let state = capture(conn)?;
    for (id, task) in &state {
        let mut seen = HashSet::from([id.as_str()]);
        let mut node = task;
        while let Some(parent_id) = node["parent_id"].as_str() {
            if !seen.insert(parent_id) {
                return invalid("Cannot undo: the task hierarchy has changed.");
            }
            let parent = state.get(parent_id).ok_or(ServiceError::Conflict)?;
            if parent["list_id"] != task["list_id"]
                || (task["deleted_at"].is_null() && !parent["deleted_at"].is_null())
                || (task["deleted_at"].is_null()
                    && task["completed_at"].is_null()
                    && task["wont_do_at"].is_null()
                    && (!parent["completed_at"].is_null() || !parent["wont_do_at"].is_null()))
            {
                return invalid("Cannot undo: the task hierarchy has changed.");
            }
            node = parent;
        }
        if let Some(section) = task["section_id"].as_str() {
            let list: Option<String> = conn
                .query_row("SELECT list_id FROM sections WHERE id=?1", [section], |r| {
                    r.get(0)
                })
                .optional()?;
            if list.as_deref() != task["list_id"].as_str() {
                return invalid("Cannot undo: the task section has changed.");
            }
        }
    }
    Ok(())
}
