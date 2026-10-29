//! Task relationships and local attachments. Attachment bytes live in SQLite so
//! importing/removing them has the same atomicity and backup boundary as tasks.
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::io::Read;

const MAX_ATTACHMENT_BYTES: usize = 20 * 1024 * 1024;
const MAX_TAGS_PER_TASK: usize = 100;

pub(super) fn mutate(conn: &Connection, action: &str, data: &Map<String, Value>) -> Result<Value> {
    match action {
        "tags.create" => create_tag(conn, data),
        "tags.update" => update_tag(conn, data),
        "tags.delete" => delete_tag(conn, data),
        "tasks.tags" => {
            fields(data, &["id", "tag_ids", "expected_version"])?;
            let id = required(data, "id")?;
            editable_task(conn, id)?;
            if let Some(value) = data.get("expected_version") {
                let version = value.as_i64().ok_or_else(|| {
                    ServiceError::Invalid("expected_version must be an integer.".into())
                })?;
                if get_task(conn, id)?.version != version {
                    return Err(ServiceError::Conflict);
                }
            }
            let values = data
                .get("tag_ids")
                .ok_or_else(|| ServiceError::Invalid("tag_ids is required.".into()))?;
            replace_tags(conn, id, values)?;
            touch_task(conn, id)?;
            Ok(json!(get_task(conn, id)?))
        }
        "links.add" | "links.remove" => change_link(conn, action, data),
        "attachments.add" => add_attachment(conn, data),
        "attachments.import" => import_attachment(conn, data),
        "attachments.remove" => remove_attachment(conn, data),
        _ => invalid(format!("Unknown relationship action: {action}")),
    }
}

pub(super) fn read(conn: &Connection, action: &str, data: &Map<String, Value>) -> Result<Value> {
    match action {
        "tags.list" => {
            fields(data, &[])?;
            tags(conn)
        }
        "attachments.get" => {
            fields(data, &["id"])?;
            let id = required(data, "id")?;
            let mut attachment = attachment(conn, id)?;
            let content: Vec<u8> =
                conn.query_row("SELECT content FROM attachments WHERE id=?1", [id], |row| {
                    row.get(0)
                })?;
            attachment["content_base64"] = json!(STANDARD.encode(content));
            Ok(attachment)
        }
        "attachments.read_chunk" => {
            fields(data, &["id", "offset", "length"])?;
            let id = required(data, "id")?;
            let mut item = attachment(conn, id)?;
            let size = item["size"]
                .as_i64()
                .ok_or_else(|| ServiceError::Invalid("Attachment size is invalid.".into()))?;
            let offset = match data.get("offset") {
                None => 0,
                Some(value) => value
                    .as_i64()
                    .filter(|n| (0..=size).contains(n))
                    .ok_or_else(|| {
                        ServiceError::Invalid(
                            "offset must be an integer between 0 and the attachment size.".into(),
                        )
                    })?,
            };
            let length = match data.get("length") {
                None => 262_144,
                Some(value) => value
                    .as_i64()
                    .filter(|n| (1..=262_144).contains(n))
                    .ok_or_else(|| {
                        ServiceError::Invalid(
                            "length must be an integer between 1 and 262144 bytes.".into(),
                        )
                    })?,
            };
            let content: Vec<u8> = conn.query_row(
                "SELECT substr(content,?2,?3) FROM attachments WHERE id=?1",
                params![id, offset + 1, length],
                |row| row.get(0),
            )?;
            let next = offset + content.len() as i64;
            item["content_base64"] = json!(STANDARD.encode(content));
            item["offset"] = json!(offset);
            item["next_offset"] = if next < size {
                json!(next)
            } else {
                Value::Null
            };
            Ok(item)
        }
        _ => invalid(format!("Unknown relationship query: {action}")),
    }
}

pub(super) fn tags(conn: &Connection) -> Result<Value> {
    let mut stmt = conn.prepare(
        "SELECT id,name,color,position,created_at,updated_at FROM tags ORDER BY position,name,id",
    )?;
    let items = stmt
        .query_map([], tag_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(json!(items))
}

fn tag_row(row: &Row) -> rusqlite::Result<Value> {
    Ok(json!({
        "id": row.get::<_,String>(0)?, "name": row.get::<_,String>(1)?,
        "color": row.get::<_,Option<String>>(2)?, "position": row.get::<_,i64>(3)?,
        "created_at": row.get::<_,String>(4)?, "updated_at": row.get::<_,String>(5)?,
    }))
}

fn tag(conn: &Connection, id: &str) -> Result<Value> {
    conn.query_row(
        "SELECT id,name,color,position,created_at,updated_at FROM tags WHERE id=?1",
        [id],
        tag_row,
    )
    .optional()?
    .ok_or_else(|| ServiceError::NotFound("Tag".into()))
}

fn ensure_tag_name(conn: &Connection, name: &str, excluding: Option<&str>) -> Result<()> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM tags WHERE name=?1 COLLATE NOCASE AND (?2 IS NULL OR id<>?2))",
        params![name, excluding],
        |r| r.get(0),
    )?;
    if exists {
        return invalid("A tag with this name already exists.");
    }
    Ok(())
}

fn create_tag(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    fields(data, &["name", "color"])?;
    let name = clean_name(required(data, "name")?, "name")?;
    ensure_tag_name(conn, &name, None)?;
    let color = color(data, "#4772fa")?;
    let id = Uuid::new_v4().to_string();
    let time = now();
    conn.execute("INSERT INTO tags(id,name,color,position,created_at,updated_at) VALUES(?1,?2,?3,(SELECT COALESCE(MAX(position),-1)+1 FROM tags),?4,?4)", params![id,name,color,time])?;
    tag(conn, &id)
}

fn update_tag(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    fields(data, &["id", "name", "color"])?;
    let id = required(data, "id")?;
    let previous = tag(conn, id)?;
    let name = if data.contains_key("name") {
        clean_name(required(data, "name")?, "name")?
    } else {
        previous["name"].as_str().unwrap_or_default().into()
    };
    ensure_tag_name(conn, &name, Some(id))?;
    let color = color(data, previous["color"].as_str().unwrap_or("#4772fa"))?;
    conn.execute(
        "UPDATE tags SET name=?2,color=?3,updated_at=?4 WHERE id=?1",
        params![id, name, color, now()],
    )?;
    tag(conn, id)
}

fn delete_tag(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    fields(data, &["id"])?;
    let id = required(data, "id")?;
    tag(conn, id)?;
    conn.execute("UPDATE tasks SET updated_at=?2,version=version+1 WHERE id IN (SELECT task_id FROM task_tags WHERE tag_id=?1)", params![id,now()])?;
    conn.execute("DELETE FROM tags WHERE id=?1", [id])?;
    conn.execute(
        "UPDATE settings SET value='null' WHERE key='default_tag_id' AND value=?1",
        [json!(id).to_string()],
    )?;
    Ok(json!({"ok":true}))
}

pub(super) fn replace_tags(conn: &Connection, task_id: &str, values: &Value) -> Result<()> {
    let values = values
        .as_array()
        .ok_or_else(|| ServiceError::Invalid("tag_ids must be an array of tag IDs.".into()))?;
    if values.len() > MAX_TAGS_PER_TASK {
        return invalid("A task can have at most 100 tags.");
    }
    let mut ids = Vec::with_capacity(values.len());
    let mut seen = HashSet::new();
    // Validate everything before replacing, even when called by a test without a transaction.
    for value in values {
        let id = value.as_str().filter(|id| !id.is_empty()).ok_or_else(|| {
            ServiceError::Invalid("Each tag ID must be a non-empty string.".into())
        })?;
        tag(conn, id)?;
        if seen.insert(id) {
            ids.push(id);
        }
    }
    conn.execute("DELETE FROM task_tags WHERE task_id=?1", [task_id])?;
    for id in ids {
        conn.execute(
            "INSERT INTO task_tags(task_id,tag_id) VALUES(?1,?2)",
            params![task_id, id],
        )?;
    }
    Ok(())
}

fn editable_task(conn: &Connection, id: &str) -> Result<()> {
    let deleted: Option<Option<String>> = conn
        .query_row("SELECT deleted_at FROM tasks WHERE id=?1", [id], |row| {
            row.get(0)
        })
        .optional()?;
    match deleted {
        None => Err(ServiceError::NotFound("Task".into())),
        Some(Some(_)) => {
            invalid("Restore the task from Trash before changing its attachments, tags, or links.")
        }
        Some(None) => Ok(()),
    }
}

fn touch_task(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE tasks SET updated_at=?2,version=version+1 WHERE id=?1",
        params![id, now()],
    )?;
    Ok(())
}

fn change_link(conn: &Connection, action: &str, data: &Map<String, Value>) -> Result<Value> {
    fields(data, &["id", "linked_task_id"])?;
    let id = required(data, "id")?;
    let linked_id = required(data, "linked_task_id")?;
    if id == linked_id {
        return invalid("A task cannot be linked to itself.");
    }
    if action == "links.add" {
        editable_task(conn, id)?;
        editable_task(conn, linked_id)?;
    } else {
        get_task(conn, id)?;
        get_task(conn, linked_id)?;
    }
    let (first, second) = if id < linked_id {
        (id, linked_id)
    } else {
        (linked_id, id)
    };
    let changed = if action == "links.add" {
        conn.execute(
            "INSERT OR IGNORE INTO task_links(task_id,linked_task_id) VALUES(?1,?2)",
            params![first, second],
        )?
    } else {
        conn.execute(
            "DELETE FROM task_links WHERE task_id=?1 AND linked_task_id=?2",
            params![first, second],
        )?
    };
    if changed > 0 {
        touch_task(conn, id)?;
        touch_task(conn, linked_id)?;
    }
    Ok(json!({"ok":true}))
}

fn filename(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty()
        || name.chars().count() > 255
        || matches!(name, "." | "..")
        || name
            .chars()
            .any(|c| c.is_control() || c == '/' || c == '\\')
    {
        return invalid(
            "Attachment name must be a filename of 1–255 characters without path separators.",
        );
    }
    Ok(name.into())
}

fn mime_type(value: &str) -> Result<String> {
    let value = value.trim();
    if value.len() > 255
        || !value.contains('/')
        || value.starts_with('/')
        || value.ends_with('/')
        || !value.bytes().all(|b| b.is_ascii_graphic())
    {
        return invalid("mime_type must be a valid media type such as application/pdf.");
    }
    Ok(value.into())
}

fn attachment_row(row: &Row) -> rusqlite::Result<Value> {
    Ok(
        json!({"id":row.get::<_,String>(0)?,"task_id":row.get::<_,String>(1)?,"name":row.get::<_,String>(2)?,"mime_type":row.get::<_,String>(3)?,"size":row.get::<_,i64>(4)?,"created_at":row.get::<_,String>(5)?}),
    )
}

fn attachment(conn: &Connection, id: &str) -> Result<Value> {
    conn.query_row(
        "SELECT id,task_id,name,mime_type,size,created_at FROM attachments WHERE id=?1",
        [id],
        attachment_row,
    )
    .optional()?
    .ok_or_else(|| ServiceError::NotFound("Attachment".into()))
}

fn store_attachment(
    conn: &Connection,
    task_id: &str,
    name: &str,
    mime: &str,
    content: Vec<u8>,
) -> Result<Value> {
    if content.len() > MAX_ATTACHMENT_BYTES {
        return invalid("Attachments must be 20 MiB or smaller.");
    }
    let id = Uuid::new_v4().to_string();
    let time = now();
    conn.execute("INSERT INTO attachments(id,task_id,name,mime_type,size,content,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)", params![id,task_id,name,mime,content.len() as i64,content,time])?;
    touch_task(conn, task_id)?;
    attachment(conn, &id)
}

fn add_attachment(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    fields(data, &["id", "name", "mime_type", "content_base64"])?;
    let id = required(data, "id")?;
    editable_task(conn, id)?;
    let name = filename(required(data, "name")?)?;
    let mime = mime_type(&text_field(data, "mime_type", "application/octet-stream")?)?;
    let encoded = text_field(data, "content_base64", "")?;
    if !data.contains_key("content_base64") {
        return invalid("content_base64 is required.");
    }
    if encoded.len() > MAX_ATTACHMENT_BYTES.div_ceil(3) * 4 {
        return invalid("Attachments must be 20 MiB or smaller.");
    }
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| ServiceError::Invalid("content_base64 must be valid Base64.".into()))?;
    store_attachment(conn, id, &name, &mime, bytes)
}

fn import_attachment(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    fields(data, &["task_id", "path"])?;
    let task_id = required(data, "task_id")?;
    editable_task(conn, task_id)?;
    let path = Path::new(required(data, "path")?);
    let file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return invalid("Attachments must be regular files.");
    }
    if metadata.len() > MAX_ATTACHMENT_BYTES as u64 {
        return invalid("Attachments must be 20 MiB or smaller.");
    }
    let name = path
        .file_name()
        .ok_or_else(|| ServiceError::Invalid("Attachment path has no filename.".into()))?
        .to_string_lossy();
    let name = filename(&name)?;
    let mime = match path
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "txt" | "log" => "text/plain",
        "md" => "text/markdown",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "json" => "application/json",
        "csv" => "text/csv",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    };
    // A file may grow after metadata is read. Bound the actual read as well.
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_ATTACHMENT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    store_attachment(conn, task_id, &name, mime, bytes)
}

fn remove_attachment(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    fields(data, &["id"])?;
    let id = required(data, "id")?;
    let previous = attachment(conn, id)?;
    let task_id = previous["task_id"]
        .as_str()
        .ok_or_else(|| ServiceError::Invalid("Attachment has no task.".into()))?;
    conn.execute("DELETE FROM attachments WHERE id=?1", [id])?;
    touch_task(conn, task_id)?;
    Ok(json!({"ok":true}))
}

pub(super) fn task_metadata(
    conn: &Connection,
    task_id: &str,
) -> Result<(Vec<String>, Vec<Value>, Vec<String>)> {
    let mut stmt = conn.prepare("SELECT tt.tag_id FROM task_tags tt JOIN tags t ON t.id=tt.tag_id WHERE tt.task_id=?1 ORDER BY t.position,t.name,t.id")?;
    let tag_ids = stmt
        .query_map([task_id], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    let mut stmt = conn.prepare("SELECT id,task_id,name,mime_type,size,created_at FROM attachments WHERE task_id=?1 ORDER BY created_at,id")?;
    let attachments = stmt
        .query_map([task_id], attachment_row)?
        .collect::<rusqlite::Result<Vec<Value>>>()?;
    let mut stmt = conn.prepare("SELECT CASE WHEN task_id=?1 THEN linked_task_id ELSE task_id END AS linked FROM task_links WHERE task_id=?1 OR linked_task_id=?1 ORDER BY linked")?;
    let linked_ids = stmt
        .query_map([task_id], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok((tag_ids, attachments, linked_ids))
}

pub(super) fn duplicate_metadata(conn: &Connection, old_id: &str, new_id: &str) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO task_tags(task_id,tag_id) SELECT ?2,tag_id FROM task_tags WHERE task_id=?1",
        params![old_id, new_id],
    )?;
    let mut stmt =
        conn.prepare("SELECT id FROM attachments WHERE task_id=?1 ORDER BY created_at,id")?;
    let ids = stmt
        .query_map([old_id], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for id in ids {
        conn.execute("INSERT INTO attachments(id,task_id,name,mime_type,size,content,created_at) SELECT ?2,?3,name,mime_type,size,content,?4 FROM attachments WHERE id=?1", params![id,Uuid::new_v4().to_string(),new_id,now()])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, TaskService, String, String) {
        let dir = tempfile::tempdir().unwrap();
        let service = TaskService::open(dir.path().join("relations.sqlite3")).unwrap();
        let first = service
            .execute("tasks.create", json!({"title":"First"}))
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let second = service
            .execute("tasks.create", json!({"title":"Second"}))
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        (dir, service, first, second)
    }

    #[test]
    fn tags_validate_before_replacing_and_delete_updates_tasks() {
        let (_dir, service, first, _) = setup();
        let tag = service
            .execute("tags.create", json!({"name":"Work","color":"#123456"}))
            .unwrap();
        assert!(service
            .execute("tags.create", json!({"name":"work"}))
            .is_err());
        service
            .execute(
                "tasks.tags",
                json!({"id":first,"tag_ids":[tag["id"],tag["id"]]}),
            )
            .unwrap();
        let before = service.execute("tasks.get", json!({"id":first})).unwrap();
        assert!(service
            .execute("tasks.tags", json!({"id":first,"tag_ids":["missing"]}))
            .is_err());
        let conn = service.connect().unwrap();
        assert_eq!(
            task_metadata(&conn, &first).unwrap().0,
            vec![tag["id"].as_str().unwrap().to_owned()]
        );
        service
            .execute("tags.delete", json!({"id":tag["id"]}))
            .unwrap();
        assert!(task_metadata(&conn, &first).unwrap().0.is_empty());
        let after = service.execute("tasks.get", json!({"id":first})).unwrap();
        assert!(after["version"].as_i64().unwrap() > before["version"].as_i64().unwrap());
    }

    #[test]
    fn links_are_symmetric_idempotent_and_independent_of_parenthood() {
        let (_dir, service, first, second) = setup();
        service
            .execute("links.add", json!({"id":first,"linked_task_id":second}))
            .unwrap();
        service
            .execute("links.add", json!({"id":second,"linked_task_id":first}))
            .unwrap();
        let conn = service.connect().unwrap();
        assert_eq!(
            task_metadata(&conn, &first).unwrap().2,
            vec![second.clone()]
        );
        assert_eq!(
            task_metadata(&conn, &second).unwrap().2,
            vec![first.clone()]
        );
        assert!(get_task(&conn, &first).unwrap().parent_id.is_none());
        assert!(service
            .execute("links.add", json!({"id":first,"linked_task_id":first}))
            .is_err());
        service
            .execute("tasks.complete", json!({"id":first}))
            .unwrap();
        assert!(get_task(&conn, &second).unwrap().completed_at.is_none());
        service
            .execute("links.remove", json!({"id":second,"linked_task_id":first}))
            .unwrap();
        assert!(task_metadata(&conn, &first).unwrap().2.is_empty());
        assert!(task_metadata(&conn, &second).unwrap().2.is_empty());
    }

    #[test]
    fn attachments_round_trip_without_content_in_snapshot_and_duplicate_independently() {
        let (_dir, service, first, second) = setup();
        let attachment = service.execute("attachments.add", json!({"id":first,"name":"note.txt","mime_type":"text/plain","content_base64":STANDARD.encode("résumé\n")})).unwrap();
        assert!(attachment.get("content_base64").is_none());
        let read = service
            .execute("attachments.get", json!({"id":attachment["id"]}))
            .unwrap();
        assert_eq!(read["content_base64"], STANDARD.encode("résumé\n"));
        let mut conn = service.connect().unwrap();
        let tx = conn.transaction().unwrap();
        duplicate_metadata(&tx, &first, &second).unwrap();
        tx.commit().unwrap();
        let copies = task_metadata(&conn, &second).unwrap().1;
        assert_eq!(copies.len(), 1);
        assert_ne!(copies[0]["id"], attachment["id"]);
        assert!(copies[0].get("content_base64").is_none());
        service
            .execute("attachments.remove", json!({"id":attachment["id"]}))
            .unwrap();
        assert!(service
            .execute("attachments.get", json!({"id":attachment["id"]}))
            .is_err());
        assert_eq!(
            service
                .execute("attachments.get", json!({"id":copies[0]["id"]}))
                .unwrap()["content_base64"],
            STANDARD.encode("résumé\n")
        );
    }

    #[test]
    fn attachment_import_owns_bytes_and_rejects_invalid_inputs() {
        let (dir, service, first, _) = setup();
        let path = dir.path().join("local.txt");
        std::fs::write(&path, b"Original content").unwrap();
        let attachment = service
            .execute("attachments.import", json!({"task_id":first,"path":path}))
            .unwrap();
        std::fs::write(&path, b"Changed outside").unwrap();
        assert_eq!(
            service
                .execute("attachments.get", json!({"id":attachment["id"]}))
                .unwrap()["content_base64"],
            STANDARD.encode(b"Original content")
        );
        for input in [
            json!({"id":first,"name":"../escape","content_base64":"YQ=="}),
            json!({"id":first,"name":"okay","content_base64":"%%%"}),
            json!({"id":first,"name":"okay","mime_type":"invalid","content_base64":"YQ=="}),
        ] {
            assert!(service.execute("attachments.add", input).is_err());
        }
        assert!(service
            .execute(
                "attachments.import",
                json!({"task_id":first,"path":dir.path()})
            )
            .is_err());
        let conn = service.connect().unwrap();
        assert_eq!(task_metadata(&conn, &first).unwrap().1.len(), 1);
    }

    #[test]
    fn attachment_chunks_are_bounded_and_reassemble_original_bytes() {
        let (_dir, service, first, _) = setup();
        let bytes: Vec<u8> = (0..700_003).map(|index| (index % 251) as u8).collect();
        let attachment = service
            .execute(
                "attachments.add",
                json!({"id":first,"name":"payload.bin","content_base64":STANDARD.encode(&bytes)}),
            )
            .unwrap();
        let mut offset = 0;
        let mut recovered = Vec::new();
        loop {
            let chunk = service
                .execute(
                    "attachments.read_chunk",
                    json!({"id":attachment["id"],"offset":offset}),
                )
                .unwrap();
            let decoded = STANDARD
                .decode(chunk["content_base64"].as_str().unwrap())
                .unwrap();
            assert!(decoded.len() <= 262_144);
            assert_eq!(chunk["offset"], offset);
            recovered.extend(decoded);
            match chunk["next_offset"].as_i64() {
                Some(next) => {
                    assert!(next > offset);
                    offset = next;
                }
                None => break,
            }
        }
        assert_eq!(recovered, bytes);
        let eof = service
            .execute(
                "attachments.read_chunk",
                json!({"id":attachment["id"],"offset":bytes.len()}),
            )
            .unwrap();
        assert_eq!(eof["content_base64"], "");
        assert!(eof["next_offset"].is_null());
        for invalid in [
            json!({"offset":-1}),
            json!({"offset":bytes.len()+1}),
            json!({"length":0}),
            json!({"length":262_145}),
            json!({"length":1.5}),
        ] {
            let mut data = invalid;
            data["id"] = attachment["id"].clone();
            assert!(service.execute("attachments.read_chunk", data).is_err());
        }
    }
}
