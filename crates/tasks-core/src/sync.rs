//! Personal-device sync protocol, independent of any cloud provider.
//!
//! Each field is a causal multi-value register. Disjoint edits merge, concurrent
//! alternatives remain available, and a deterministic winner supplies the live
//! SQLite view. Deletion is itself a register; tombstones are never discarded.
//! The transport writes one file per device and may retry/reorder packets freely.
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use rusqlite::types::{Value as SqlValue, ValueRef};
use std::collections::{BTreeMap, BTreeSet};

const MAX_PACKET_BYTES: usize = 100 * 1024 * 1024;
const MAX_ATTACHMENT_BYTES: usize = 20 * 1024 * 1024;
const MAX_ENTITIES: usize = 100_000;
const MAX_DEVICES: usize = 128;
const MAX_VERSIONS: usize = 128;
const MAX_CLOCK: i64 = 9_000_000_000_000;
const EXISTS: &str = "$exists";
type Vector = BTreeMap<String, i64>;
type Key = (String, String);
type Rows = BTreeMap<Key, Map<String, Value>>;
type Registers = BTreeMap<String, Vec<Version>>;
type Replica = BTreeMap<Key, Entity>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Version {
    device_id: String,
    clock: i64,
    context: Vector,
    value: Value,
}
impl Version {
    fn id(&self) -> String {
        format!("{}:{}", self.device_id, self.clock)
    }
    fn sees(&self, other: &Self) -> bool {
        self.context.get(&other.device_id).copied().unwrap_or(0) >= other.clock
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Entity {
    kind: String,
    id: String,
    fields: Registers,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Packet {
    protocol: String,
    version: i64,
    device_id: String,
    clock: i64,
    vector: Vector,
    entities: Vec<Entity>,
}
#[derive(Clone, Copy)]
struct Schema {
    table: &'static str,
    columns: &'static [&'static str],
    key: &'static [&'static str],
}
const SCHEMAS: &[Schema] = &[
    Schema {
        table: "folders",
        columns: &[
            "id",
            "name",
            "parent_id",
            "position",
            "created_at",
            "updated_at",
            "icon",
            "pinned",
        ],
        key: &["id"],
    },
    Schema {
        table: "lists",
        columns: &[
            "id",
            "name",
            "folder_id",
            "color",
            "position",
            "created_at",
            "updated_at",
            "archived_at",
            "preferences_json",
            "icon",
            "pinned",
            "show_in_smart_lists",
            "list_type",
        ],
        key: &["id"],
    },
    Schema {
        table: "tasks",
        columns: &[
            "id",
            "title",
            "notes",
            "list_id",
            "parent_id",
            "due_date",
            "due_time",
            "priority",
            "position",
            "completed_at",
            "deleted_at",
            "created_at",
            "updated_at",
            "details_json",
            "recurrence_source_id",
        ],
        key: &["id"],
    },
    Schema {
        table: "tags",
        columns: &[
            "id",
            "name",
            "color",
            "position",
            "created_at",
            "updated_at",
        ],
        key: &["id"],
    },
    Schema {
        table: "sections",
        columns: &[
            "id",
            "list_id",
            "name",
            "position",
            "created_at",
            "updated_at",
        ],
        key: &["id"],
    },
    Schema {
        table: "templates",
        columns: &[
            "id",
            "name",
            "task_json",
            "created_at",
            "updated_at",
            "preview_json",
        ],
        key: &["id"],
    },
    Schema {
        table: "comments",
        columns: &["id", "task_id", "body", "created_at", "updated_at"],
        key: &["id"],
    },
    Schema {
        table: "attachments",
        columns: &[
            "id",
            "task_id",
            "name",
            "mime_type",
            "size",
            "content",
            "created_at",
        ],
        key: &["id"],
    },
    Schema {
        table: "task_tags",
        columns: &["task_id", "tag_id"],
        key: &["task_id", "tag_id"],
    },
    Schema {
        table: "task_links",
        columns: &["task_id", "linked_task_id"],
        key: &["task_id", "linked_task_id"],
    },
    Schema {
        table: "reminders",
        columns: &["task_id", "at"],
        key: &["task_id", "at"],
    },
    Schema {
        table: "comment_attachments",
        columns: &["comment_id", "attachment_id"],
        key: &["comment_id", "attachment_id"],
    },
    Schema {
        table: "settings",
        columns: &["key", "value"],
        key: &["key"],
    },
];
fn schema(kind: &str) -> Result<Schema> {
    SCHEMAS
        .iter()
        .copied()
        .find(|schema| schema.table == kind)
        .ok_or_else(|| ServiceError::Invalid(format!("Unsupported sync entity: {kind}")))
}
fn meta(conn: &Connection, key: &str) -> Result<String> {
    let value = conn
        .query_row(
            "SELECT value FROM sync_metadata WHERE key=?1",
            [key],
            |row| row.get(0),
        )
        .optional()?;
    match value {
        Some(value) => Ok(value),
        None if key == "enabled" || key.starts_with("checkpoint_clock:") => Ok("0".into()),
        None if key.starts_with("checkpoint:") => Ok(String::new()),
        None => invalid("Missing local sync metadata."),
    }
}
fn put_meta(conn: &Connection, key: &str, value: impl ToString) -> Result<()> {
    conn.execute("INSERT INTO sync_metadata(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key,value.to_string()])?;
    Ok(())
}
pub(super) fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch(include_str!("../migrations/009_personal_sync.sql"))?;
    put_meta(conn, "device_id", Uuid::new_v4())?;
    put_meta(conn, "clock", 0)?;
    put_meta(conn, "vector", "{}")?;
    put_meta(conn, "last_merge_at", "")?;
    put_meta(conn, "revision", 0)?;
    put_meta(conn, "pending_conflicts", 0)?;
    put_meta(conn, "enabled", 0)?;
    Ok(())
}
fn activate(conn: &Connection) -> Result<()> {
    put_meta(conn, "enabled", 1)?;
    record_local(conn)
}
fn identity(schema: Schema, row: &Map<String, Value>) -> Result<String> {
    if schema.key.len() == 1 {
        return row
            .get(schema.key[0])
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| ServiceError::Invalid("Sync identity must be a string.".into()));
    }
    let values: Vec<_> = schema
        .key
        .iter()
        .map(|column| row.get(*column).cloned().unwrap_or(Value::Null))
        .collect();
    Ok(serde_json::to_string(&values)?)
}
fn snapshot_rows(conn: &Connection) -> Result<Rows> {
    snapshot_rows_with_baseline(conn, None)
}
fn snapshot_rows_with_baseline(conn: &Connection, baseline: Option<&Rows>) -> Result<Rows> {
    let mut result = Rows::new();
    for schema in SCHEMAS {
        let columns = schema
            .columns
            .iter()
            .map(|column| {
                if baseline.is_some() && schema.table == "attachments" && *column == "content" {
                    "NULL"
                } else {
                    *column
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        let mut sql = format!("SELECT {columns} FROM {}", schema.table);
        if schema.table == "settings" {
            sql.push_str(" WHERE key IN ('saved_filters','summary_templates')");
        }
        let mut statement = conn.prepare(&sql)?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let mut value = Map::new();
            for (index, column) in schema.columns.iter().enumerate() {
                if schema.table == "attachments" && *column == "content" && baseline.is_some() {
                    let id: String = row.get(0)?;
                    let cached = baseline
                        .and_then(|rows| rows.get(&("attachments".into(), id.clone())))
                        .and_then(|row| row.get("content"));
                    let content = if let Some(cached) = cached {
                        cached.clone()
                    } else {
                        let bytes: Vec<u8> = conn.query_row(
                            "SELECT content FROM attachments WHERE id=?1",
                            [id],
                            |row| row.get(0),
                        )?;
                        json!(STANDARD.encode(bytes))
                    };
                    value.insert("content".into(), content);
                    continue;
                }
                let value_ref = row.get_ref(index)?;
                let mut field = match value_ref {
                    ValueRef::Null => Value::Null,
                    ValueRef::Integer(number) => json!(number),
                    ValueRef::Text(text) => {
                        json!(std::str::from_utf8(text).map_err(|_| ServiceError::Invalid(
                            "Sync cannot encode invalid UTF-8.".into()
                        ))?)
                    }
                    ValueRef::Blob(bytes) if *column == "content" => json!(STANDARD.encode(bytes)),
                    _ => return invalid("Unsupported value in a sync entity."),
                };
                if matches!(
                    *column,
                    "details_json" | "preferences_json" | "task_json" | "preview_json"
                ) || (schema.table == "settings" && *column == "value")
                {
                    field = json!(canonical_json(column, field.as_str().unwrap())?);
                }
                value.insert((*column).into(), field);
            }
            result.insert((schema.table.into(), identity(*schema, &value)?), value);
        }
    }
    Ok(result)
}
fn canonical_json(column: &str, text: &str) -> Result<String> {
    let mut parsed: Value = serde_json::from_str(text)?;
    // Defaulted fields and key order must not turn an unrelated title edit into
    // a new schedule/checklist version that discards an unresolved conflict.
    let defaults = match column {
        "details_json" => Some(serde_json::to_value(serde_json::from_value::<
            extensions::TaskDetails,
        >(parsed.clone())?)?),
        "preferences_json" => Some(serde_json::to_value(serde_json::from_value::<
            extensions::ListPreferences,
        >(parsed.clone())?)?),
        _ => None,
    };
    if let (Some(object), Some(defaults)) = (
        parsed.as_object_mut(),
        defaults.and_then(|value| value.as_object().cloned()),
    ) {
        for (key, value) in defaults {
            object.entry(key).or_insert(value);
        }
    }
    Ok(serde_json::to_string(&parsed)?)
}
fn load_projection(conn: &Connection) -> Result<Rows> {
    let mut statement =
        conn.prepare("SELECT kind,id,value_json FROM sync_projection ORDER BY kind,id")?;
    let values = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    values
        .into_iter()
        .map(|(kind, id, text)| Ok(((kind, id), serde_json::from_str(&text)?)))
        .collect()
}
fn load_replica(conn: &Connection) -> Result<Replica> {
    let mut statement =
        conn.prepare("SELECT kind,id,fields_json FROM sync_entities ORDER BY kind,id")?;
    let values = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    values
        .into_iter()
        .map(|(kind, id, text)| {
            Ok((
                (kind.clone(), id.clone()),
                Entity {
                    kind,
                    id,
                    fields: serde_json::from_str(&text)?,
                },
            ))
        })
        .collect()
}
fn load_entity(conn: &Connection, key: &Key) -> Result<Entity> {
    let text: Option<String> = conn
        .query_row(
            "SELECT fields_json FROM sync_entities WHERE kind=?1 AND id=?2",
            params![key.0, key.1],
            |row| row.get(0),
        )
        .optional()?;
    Ok(Entity {
        kind: key.0.clone(),
        id: key.1.clone(),
        fields: text
            .map(|text| serde_json::from_str(&text))
            .transpose()?
            .unwrap_or_default(),
    })
}
fn save_entity(conn: &Connection, entity: &Entity) -> Result<()> {
    conn.execute("INSERT INTO sync_entities(kind,id,fields_json) VALUES(?1,?2,?3) ON CONFLICT(kind,id) DO UPDATE SET fields_json=excluded.fields_json", params![entity.kind,entity.id,serde_json::to_string(&entity.fields)?])?;
    Ok(())
}
fn save_projection(conn: &Connection, rows: &Rows) -> Result<()> {
    conn.execute("DELETE FROM sync_projection", [])?;
    let mut insert =
        conn.prepare("INSERT INTO sync_projection(kind,id,value_json) VALUES(?1,?2,?3)")?;
    for ((kind, id), row) in rows {
        insert.execute(params![
            kind,
            id,
            serde_json::to_string(&projection_row(row))?
        ])?;
    }
    Ok(())
}
// Large values already live in the domain table and causal register. The local
// edit baseline keeps only a fingerprint, avoiding a third attachment copy.
fn projection_value(value: &Value) -> Value {
    if let Some(text) = value.as_str().filter(|text| text.len() > 4096) {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hash);
        json!({"local_fingerprint":format!("{:016x}",hash.finish()),"bytes":text.len()})
    } else {
        value.clone()
    }
}
fn projection_row(row: &Map<String, Value>) -> Map<String, Value> {
    row.iter()
        .map(|(column, value)| (column.clone(), projection_value(value)))
        .collect()
}
fn bump_revision(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE sync_metadata SET value=CAST(value AS INTEGER)+1 WHERE key='revision'",
        [],
    )?;
    Ok(())
}
fn next_stamp(conn: &Connection, vector: &mut Vector) -> Result<(String, i64)> {
    let device = meta(conn, "device_id")?;
    let clock: i64 = meta(conn, "clock")?
        .parse()
        .map_err(|_| ServiceError::Invalid("Invalid local sync clock.".into()))?;
    let clock = clock
        .max(vector.values().copied().max().unwrap_or(0))
        .checked_add(1)
        .ok_or_else(|| ServiceError::Invalid("Sync clock exhausted.".into()))?;
    if clock > MAX_CLOCK {
        return invalid("Sync clock exceeds the supported range.");
    }
    vector.insert(device.clone(), clock);
    put_meta(conn, "clock", clock)?;
    put_meta(conn, "vector", serde_json::to_string(vector)?)?;
    Ok((device, clock))
}
pub(super) fn record_local(conn: &Connection) -> Result<()> {
    if meta(conn, "enabled")? != "1" {
        return Ok(());
    }
    let previous = load_projection(conn)?;
    // Attachment bytes are immutable for a given ID. Only newly added IDs need
    // reading/encoding during autosave; normal edits never reread existing BLOBs.
    let current = snapshot_rows_with_baseline(conn, Some(&previous))?;
    let projected: Rows = current
        .iter()
        .map(|(key, row)| (key.clone(), projection_row(row)))
        .collect();
    if projected == previous {
        return Ok(());
    }
    let mut conflicts: i64 = meta(conn, "pending_conflicts")?.parse().unwrap_or(0);
    let mut vector: Vector = serde_json::from_str(&meta(conn, "vector")?)?;
    let (device_id, clock) = next_stamp(conn, &mut vector)?;
    let version = |value| Version {
        device_id: device_id.clone(),
        clock,
        context: vector.clone(),
        value,
    };
    let keys: BTreeSet<_> = current.keys().chain(previous.keys()).cloned().collect();
    for key in keys {
        if projected.get(&key) == previous.get(&key) {
            continue;
        }
        let mut entity = load_entity(conn, &key)?;
        let new_entity = entity.fields.is_empty();
        // A fresh installation's empty presets are a neutral default, not an
        // authored concurrent deletion of another device's saved preferences.
        if new_entity
            && key.0 == "settings"
            && current.get(&key).is_some_and(|row| row["value"] == "[]")
        {
            continue;
        }
        conflicts -= entity
            .fields
            .iter()
            .filter(|(field, versions)| is_conflict(field, versions))
            .count() as i64;
        entity.fields.insert(
            EXISTS.into(),
            vec![version(json!(current.contains_key(&key)))],
        );
        if let Some(row) = current.get(&key) {
            for (column, value) in row {
                if new_entity
                    || previous.get(&key).and_then(|row| row.get(column))
                        != Some(&projection_value(value))
                {
                    entity
                        .fields
                        .insert(column.clone(), vec![version(value.clone())]);
                }
            }
        }
        conflicts += entity
            .fields
            .iter()
            .filter(|(field, versions)| is_conflict(field, versions))
            .count() as i64;
        save_entity(conn, &entity)?;
    }
    save_projection(conn, &current)?;
    put_meta(conn, "pending_conflicts", conflicts)?;
    bump_revision(conn)
}
fn winner<'a>(field: &str, versions: &'a [Version]) -> &'a Version {
    versions
        .iter()
        .max_by_key(|version| {
            (
                field == EXISTS && version.value == false,
                version.clock,
                &version.device_id,
            )
        })
        .expect("validated nonempty register")
}
fn merge_register(left: &[Version], right: &[Version]) -> Result<Vec<Version>> {
    let mut union: BTreeMap<(String, i64), Version> = BTreeMap::new();
    for version in left.iter().chain(right) {
        let key = (version.device_id.clone(), version.clock);
        if union.get(&key).is_some_and(|existing| existing != version) {
            return invalid("Conflicting sync values reuse the same device clock.");
        }
        union.insert(key, version.clone());
    }
    let values: Vec<_> = union.values().collect();
    let active: Vec<_> = values
        .iter()
        .filter(|candidate| {
            !values.iter().any(|other| {
                (other.device_id != candidate.device_id || other.clock != candidate.clock)
                    && other.sees(candidate)
            })
        })
        .map(|version| (**version).clone())
        .collect();
    if active.is_empty() || active.len() > MAX_VERSIONS {
        return invalid("Invalid or excessively concurrent sync register.");
    }
    Ok(active)
}

fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 200 && !value.chars().any(char::is_control)
}
fn validate_vector(vector: &Vector, ceiling: i64) -> Result<()> {
    if vector.len() > MAX_DEVICES
        || vector.iter().any(|(device, clock)| {
            Uuid::parse_str(device).is_err() || *clock <= 0 || *clock > ceiling
        })
    {
        return invalid("Invalid sync causal vector.");
    }
    Ok(())
}
fn validate_details(value: &str) -> Result<()> {
    let value: Value = serde_json::from_str(value)?;
    let object = value
        .as_object()
        .ok_or_else(|| ServiceError::Invalid("Task details must be an object.".into()))?;
    fields(
        object,
        &[
            "start_date",
            "start_time",
            "duration_minutes",
            "timezone",
            "recurrence",
            "recurrence_next_id",
            "is_note",
            "notes_format",
            "checklist",
            "progress",
            "progress_mode",
            "pinned",
            "wont_do_at",
            "section_id",
        ],
    )?;
    let details: extensions::TaskDetails = serde_json::from_value(value.clone())?;
    recurrence::validate_timezone(&details.timezone)?;
    if let Some(date) = &details.start_date {
        parse_date(date)?;
    }
    if let Some(time) = &details.start_time {
        NaiveTime::parse_from_str(time, "%H:%M")
            .map_err(|_| ServiceError::Invalid("Invalid synced start time.".into()))?;
    }
    if let Some(rule) = &details.recurrence {
        recurrence::validate_rule(rule)?;
    }
    if !(0..=100).contains(&details.progress)
        || details
            .duration_minutes
            .is_some_and(|n| !(1..=525600).contains(&n))
        || !["plain", "markdown", "html"].contains(&details.notes_format.as_str())
        || details.checklist.len() > 1000
    {
        return invalid("Invalid synced task details.");
    }
    let mut ids = BTreeSet::new();
    for item in details.checklist {
        let item = item
            .as_object()
            .ok_or_else(|| ServiceError::Invalid("Invalid synced checklist.".into()))?;
        fields(item, &["id", "text", "completed"])?;
        let id = required(item, "id")?;
        if !valid_id(id) || !ids.insert(id.to_owned()) {
            return invalid("Invalid synced checklist identity.");
        }
        clean_name(required(item, "text")?, "text")?;
        if !item.get("completed").is_some_and(Value::is_boolean) {
            return invalid("Invalid synced checklist state.");
        }
    }
    for id in [details.recurrence_next_id, details.section_id]
        .into_iter()
        .flatten()
    {
        if !valid_id(&id) {
            return invalid("Invalid synced reference.");
        }
    }
    if let Some(time) = details.wont_do_at {
        normalized_instant(&time)?;
    }
    Ok(())
}
fn validate_template(value: &Value, depth: usize, count: &mut usize) -> Result<()> {
    *count += 1;
    if depth > 100 || *count > 10_000 {
        return invalid("Synced template is too large.");
    }
    let payload = value
        .as_object()
        .ok_or_else(|| ServiceError::Invalid("Invalid synced template.".into()))?;
    fields(payload, &["data", "children", "files"])?;
    let data = payload
        .get("data")
        .and_then(Value::as_object)
        .ok_or_else(|| ServiceError::Invalid("Synced template requires task data.".into()))?;
    extensions::validate_task_fields(data, false)?;
    clean_name(required(data, "title")?, "title")?;
    if data.get("notes").is_some_and(|value| {
        !value
            .as_str()
            .is_some_and(|text| text.len() <= 1_000_000 && !text.contains('\0'))
    }) {
        return invalid("Invalid synced template notes.");
    }
    if let Some(children) = payload.get("children") {
        let children = children
            .as_array()
            .ok_or_else(|| ServiceError::Invalid("Invalid synced template children.".into()))?;
        for child in children {
            validate_template(child, depth + 1, count)?;
        }
    }
    if let Some(files) = payload.get("files") {
        let files = files
            .as_array()
            .ok_or_else(|| ServiceError::Invalid("Invalid synced template files.".into()))?;
        for file in files {
            let object = file.as_object().ok_or_else(|| {
                ServiceError::Invalid("Invalid synced template attachment.".into())
            })?;
            fields(
                object,
                &[
                    "id",
                    "task_id",
                    "name",
                    "mime_type",
                    "size",
                    "created_at",
                    "content_base64",
                ],
            )?;
            clean_name(required(object, "name")?, "name")?;
            let encoded = object
                .get("content_base64")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ServiceError::Invalid("Attachment content must be a base64 string.".into())
                })?;
            if encoded.len() > MAX_ATTACHMENT_BYTES.div_ceil(3) * 4
                || STANDARD
                    .decode(encoded)
                    .map_or(true, |bytes| bytes.len() > MAX_ATTACHMENT_BYTES)
            {
                return invalid("Invalid or oversized synced template attachment.");
            }
        }
    }
    Ok(())
}
fn validate_field(kind: &str, column: &str, value: &Value) -> Result<()> {
    if column == EXISTS {
        if !value.is_boolean() {
            return invalid("Sync existence must be boolean.");
        }
        return Ok(());
    }
    let nullable = matches!(
        column,
        "parent_id"
            | "folder_id"
            | "due_date"
            | "due_time"
            | "completed_at"
            | "deleted_at"
            | "archived_at"
            | "recurrence_source_id"
    );
    if value.is_null() && nullable {
        return Ok(());
    }
    if matches!(
        column,
        "priority" | "position" | "size" | "pinned" | "show_in_smart_lists"
    ) {
        let number = value.as_i64().ok_or_else(|| {
            ServiceError::Invalid("Sync numeric field must be an integer.".into())
        })?;
        if (column == "priority" && !(0..=3).contains(&number))
            || (matches!(column, "pinned" | "show_in_smart_lists") && !(0..=1).contains(&number))
            || (column == "size" && !(0..=MAX_ATTACHMENT_BYTES as i64).contains(&number))
        {
            return invalid("Sync numeric field is out of range.");
        }
        return Ok(());
    }
    let text = value
        .as_str()
        .ok_or_else(|| ServiceError::Invalid(format!("Sync field {column} must be a string.")))?;
    if kind == "settings" {
        if column == "key" && !["saved_filters", "summary_templates"].contains(&text) {
            return invalid("Only saved filters and summary templates may sync.");
        }
        if column == "value" {
            let _: Vec<Value> = serde_json::from_str(text)?;
        }
    }
    if text.contains('\0') {
        return invalid("Sync strings cannot contain null bytes.");
    }
    if column == "content" {
        if text.len() > MAX_ATTACHMENT_BYTES.div_ceil(3) * 4
            || STANDARD
                .decode(text)
                .map_or(true, |bytes| bytes.len() > MAX_ATTACHMENT_BYTES)
        {
            return invalid("Invalid or oversized synced attachment.");
        }
    } else if column == "task_json" {
        let payload: Value = serde_json::from_str(text)?;
        validate_template(&payload, 0, &mut 0)?;
    } else if text.len() > 1_000_000 {
        return invalid("Synced field exceeds the maximum length.");
    }
    match column {
        "id"
        | "parent_id"
        | "folder_id"
        | "list_id"
        | "task_id"
        | "tag_id"
        | "linked_task_id"
        | "comment_id"
        | "attachment_id"
        | "recurrence_source_id" => {
            if !valid_id(text) {
                return invalid("Invalid synced ID.");
            }
        }
        "title" | "name" => {
            clean_name(text, column)?;
        }
        "created_at" | "updated_at" | "completed_at" | "deleted_at" | "archived_at" | "at" => {
            if normalized_instant(text)? != text {
                return invalid("Synced timestamps must use canonical UTC milliseconds.");
            }
        }
        "due_date" => {
            parse_date(text)?;
        }
        "due_time" => {
            NaiveTime::parse_from_str(text, "%H:%M")
                .map_err(|_| ServiceError::Invalid("Invalid synced due time.".into()))?;
        }
        "details_json" => validate_details(text)?,
        "preferences_json" => {
            let object: Map<String, Value> = serde_json::from_str(text)?;
            fields(&object, &["view_mode", "group_by", "sort_by", "sort_order"])?;
            extensions::preferences(&object, &extensions::ListPreferences::default())?;
        }
        "preview_json" => {
            let _: Map<String, Value> = serde_json::from_str(text)?;
        }
        "list_type" if !["task", "note"].contains(&text) => {
            return invalid("Invalid synced list type.")
        }
        "color"
            if !(kind == "lists" && text.is_empty()
                || text.len() == 7
                    && text.starts_with('#')
                    && text[1..].bytes().all(|c| c.is_ascii_hexdigit())) =>
        {
            return invalid("Invalid synced color.")
        }
        _ => {}
    }
    Ok(())
}
fn validate_packet(packet: &Packet) -> Result<()> {
    if packet.protocol != "tasks-sync" || packet.version != 1 {
        return invalid("Unsupported Tasks sync protocol. Update Tasks on all devices.");
    }
    if Uuid::parse_str(&packet.device_id).is_err()
        || !(1..=MAX_CLOCK).contains(&packet.clock)
        || packet.entities.len() > MAX_ENTITIES
    {
        return invalid("Invalid sync packet identity or size.");
    }
    validate_vector(&packet.vector, packet.clock)?;
    let mut keys = BTreeSet::new();
    for entity in &packet.entities {
        let schema = schema(&entity.kind)?;
        if !keys.insert((&entity.kind, &entity.id))
            || entity.fields.len() != schema.columns.len() + 1
            || entity.id.len() > 1000
        {
            return invalid("Duplicate or incomplete sync entity.");
        }
        for column in schema.columns.iter().copied().chain([EXISTS]) {
            let versions = entity
                .fields
                .get(column)
                .ok_or_else(|| ServiceError::Invalid(format!("Missing sync field {column}.")))?;
            if versions.is_empty() || versions.len() > MAX_VERSIONS {
                return invalid("Invalid sync version count.");
            }
            let mut dots = BTreeSet::new();
            for version in versions {
                if version.clock < 1
                    || version.clock > packet.clock
                    || !dots.insert((&version.device_id, version.clock))
                    || version.context.get(&version.device_id) != Some(&version.clock)
                {
                    return invalid("Invalid sync version stamp.");
                }
                validate_vector(&version.context, version.clock)?;
                if version
                    .context
                    .iter()
                    .any(|(device, clock)| packet.vector.get(device).copied().unwrap_or(0) < *clock)
                {
                    return invalid("Sync packet omits causal history.");
                }
                validate_field(&entity.kind, column, &version.value)?;
                if schema.key.len() == 1 && column == schema.key[0] && version.value != entity.id {
                    return invalid("Synced entity ID cannot change.");
                }
                if entity.kind == "lists"
                    && entity.id == "inbox"
                    && column == EXISTS
                    && version.value == false
                {
                    return invalid("The Inbox cannot be deleted by sync.");
                }
                if entity.kind == "settings" && column == EXISTS && version.value == false {
                    return invalid("Portable settings cannot be removed by sync.");
                }
            }
            merge_register(&[], versions)?;
        }
        let row: Map<String, Value> = schema
            .columns
            .iter()
            .map(|column| {
                (
                    (*column).into(),
                    winner(column, &entity.fields[*column]).value.clone(),
                )
            })
            .collect();
        if identity(schema, &row)? != entity.id {
            return invalid("Synced relation identity does not match its fields.");
        }
        if entity.kind == "attachments" {
            let bytes = STANDARD
                .decode(row["content"].as_str().unwrap())
                .map_err(|_| ServiceError::Invalid("Invalid synced attachment.".into()))?;
            if row["size"].as_i64() != Some(bytes.len() as i64) {
                return invalid("Synced attachment length does not match its content.");
            }
        }
    }
    Ok(())
}

fn known_reference(replica: &Replica, kind: &str, value: &Value) -> Result<()> {
    if let Some(id) = value.as_str() {
        if !replica.contains_key(&(kind.into(), id.into())) {
            return invalid(format!("Synced {kind} reference {id} is missing."));
        }
    }
    Ok(())
}
fn alive(rows: &Rows, kind: &str, value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|id| rows.contains_key(&(kind.into(), id.into())))
}
fn break_cycles(rows: &mut Rows, kind: &str) {
    let ids: Vec<String> = rows
        .keys()
        .filter(|key| key.0 == kind)
        .map(|key| key.1.clone())
        .collect();
    for id in ids {
        let mut chain = Vec::new();
        let mut cursor = id;
        loop {
            if let Some(offset) = chain.iter().position(|id| id == &cursor) {
                // Every device breaks the same edge, independent of packet order.
                let detach = chain[offset..].iter().max().cloned().unwrap();
                rows.get_mut(&(kind.into(), detach))
                    .unwrap()
                    .insert("parent_id".into(), Value::Null);
                break;
            }
            chain.push(cursor.clone());
            let Some(next) = rows
                .get(&(kind.into(), cursor))
                .and_then(|row| row.get("parent_id"))
                .and_then(Value::as_str)
            else {
                break;
            };
            cursor = next.into();
        }
    }
}
fn materialized(replica: &Replica) -> Result<Rows> {
    let mut rows = Rows::new();
    for (key, entity) in replica {
        if winner(EXISTS, &entity.fields[EXISTS]).value == false {
            continue;
        }
        let schema = schema(&entity.kind)?;
        let row = schema
            .columns
            .iter()
            .map(|column| {
                (
                    (*column).into(),
                    winner(column, &entity.fields[*column]).value.clone(),
                )
            })
            .collect();
        rows.insert(key.clone(), row);
    }
    for key in ["saved_filters", "summary_templates"] {
        rows.entry(("settings".into(), key.into()))
            .or_insert_with(|| {
                json!({"key":key,"value":"[]"})
                    .as_object()
                    .expect("object")
                    .clone()
            });
    }
    if !rows.contains_key(&("lists".into(), "inbox".into())) {
        return invalid("Sync packet does not contain an active Inbox.");
    }
    // A concurrent deletion can orphan a reference. Preserve its raw register,
    // but project the surviving task into Inbox / a root instead of losing it.
    for (key, row) in rows.clone() {
        let current = rows.get_mut(&key).unwrap();
        match key.0.as_str() {
            "folders" => {
                known_reference(replica, "folders", &row["parent_id"])?;
                if !reference_alive(replica, "folders", &row["parent_id"]) {
                    current.insert("parent_id".into(), Value::Null);
                }
            }
            "lists" => {
                known_reference(replica, "folders", &row["folder_id"])?;
                if !reference_alive(replica, "folders", &row["folder_id"]) || key.1 == "inbox" {
                    current.insert("folder_id".into(), Value::Null);
                }
                if key.1 == "inbox" {
                    current.insert("list_type".into(), json!("task"));
                }
            }
            "tasks" => {
                known_reference(replica, "lists", &row["list_id"])?;
                known_reference(replica, "tasks", &row["parent_id"])?;
                known_reference(replica, "tasks", &row["recurrence_source_id"])?;
                if !reference_alive(replica, "lists", &row["list_id"]) {
                    current.insert("list_id".into(), json!("inbox"));
                }
                if !reference_alive(replica, "tasks", &row["parent_id"])
                    || row["parent_id"] == key.1
                {
                    current.insert("parent_id".into(), Value::Null);
                }
                if !reference_alive(replica, "tasks", &row["recurrence_source_id"])
                    || row["recurrence_source_id"] == key.1
                {
                    current.insert("recurrence_source_id".into(), Value::Null);
                }
                if row["due_date"].is_null() {
                    current.insert("due_time".into(), Value::Null);
                }
            }
            _ => {}
        }
    }
    break_cycles(&mut rows, "folders");
    break_cycles(&mut rows, "tasks");
    // Parent/list edits on different devices can disagree. The child's chosen
    // list remains authoritative; detaching avoids moving its entire subtree.
    for (key, row) in rows.clone().into_iter().filter(|(key, _)| key.0 == "tasks") {
        if let Some(parent) = row["parent_id"].as_str() {
            if rows
                .get(&("tasks".into(), parent.into()))
                .is_some_and(|parent| {
                    parent["list_id"] != row["list_id"]
                        || (row["deleted_at"].is_null() && !parent["deleted_at"].is_null())
                })
            {
                rows.get_mut(&key)
                    .unwrap()
                    .insert("parent_id".into(), Value::Null);
            }
        }
    }
    // Unique tag names and recurrence source IDs can collide after offline
    // creation. Keep all records, using stable disambiguation in the live view.
    let mut names = BTreeSet::new();
    let mut sources = BTreeSet::new();
    for (key, row) in &mut rows {
        if key.0 == "tags" {
            let name = row["name"].as_str().unwrap().to_owned();
            if !names.insert(name.to_ascii_lowercase()) {
                let mut candidate = format!("{name} · {}", key.1);
                while !names.insert(candidate.to_ascii_lowercase()) {
                    candidate.push('·');
                }
                row.insert("name".into(), json!(candidate));
            }
        }
        if key.0 == "tasks" {
            if let Some(source) = row["recurrence_source_id"].as_str() {
                if !sources.insert(source.to_owned()) {
                    row.insert("recurrence_source_id".into(), Value::Null);
                }
            }
        }
    }
    let mut remove = Vec::new();
    for (key, row) in rows.clone() {
        let references: &[(&str, &str)] = match key.0.as_str() {
            "sections" => &[("lists", "list_id")],
            "comments" | "attachments" | "reminders" => &[("tasks", "task_id")],
            "task_tags" => &[("tasks", "task_id"), ("tags", "tag_id")],
            "task_links" => &[("tasks", "task_id"), ("tasks", "linked_task_id")],
            "comment_attachments" => {
                &[("comments", "comment_id"), ("attachments", "attachment_id")]
            }
            _ => &[],
        };
        for (kind, column) in references {
            known_reference(replica, kind, &row[*column])?;
            if !alive(&rows, kind, &row[*column]) {
                remove.push(key.clone());
            }
        }
        if key.0 == "task_links" && row["task_id"].as_str() >= row["linked_task_id"].as_str() {
            return invalid("Synced task links must use distinct IDs in canonical order.");
        }
    }
    for key in remove {
        rows.remove(&key);
    }
    // Second pass removes comment links whose attachment/comment disappeared in
    // the preceding projection, including orphaned relations from stale packets.
    let dangling: Vec<_> = rows
        .iter()
        .filter(|(key, row)| {
            key.0 == "comment_attachments"
                && (!alive(&rows, "comments", &row["comment_id"])
                    || !alive(&rows, "attachments", &row["attachment_id"]))
        })
        .map(|(key, _)| key.clone())
        .collect();
    for key in dangling {
        rows.remove(&key);
    }
    for (key, row) in &rows {
        if key.0 == "comment_attachments" {
            let comment = &rows[&(
                "comments".into(),
                row["comment_id"].as_str().unwrap().into(),
            )];
            let attachment = &rows[&(
                "attachments".into(),
                row["attachment_id"].as_str().unwrap().into(),
            )];
            if comment["task_id"] != attachment["task_id"] {
                return invalid("A synced comment attachment must belong to the same task.");
            }
        }
    }
    let active: Vec<_> = rows
        .iter()
        .filter(|(key, row)| {
            key.0 == "tasks"
                && row["deleted_at"].is_null()
                && row["completed_at"].is_null()
                && serde_json::from_str::<Value>(row["details_json"].as_str().unwrap())
                    .is_ok_and(|details| details["wont_do_at"].is_null())
        })
        .map(|(key, _)| key.1.clone())
        .collect();
    for id in active {
        let mut cursor = rows[&("tasks".into(), id)]["parent_id"]
            .as_str()
            .map(str::to_owned);
        while let Some(parent) = cursor {
            let row = rows.get_mut(&("tasks".into(), parent)).unwrap();
            let mut details: Map<String, Value> =
                serde_json::from_str(row["details_json"].as_str().unwrap())?;
            let mut changed = false;
            if !row["completed_at"].is_null() {
                row.insert("completed_at".into(), Value::Null);
                details.insert("progress".into(), json!(0));
                changed = true;
            }
            if details
                .get("wont_do_at")
                .is_some_and(|value| !value.is_null())
            {
                details.insert("wont_do_at".into(), Value::Null);
                changed = true;
            }
            if changed {
                row.insert(
                    "details_json".into(),
                    json!(serde_json::to_string(&details)?),
                );
            }
            cursor = row["parent_id"].as_str().map(str::to_owned);
        }
    }
    for (key, row) in rows.clone().into_iter().filter(|(key, _)| key.0 == "tasks") {
        let mut details: Map<String, Value> =
            serde_json::from_str(row["details_json"].as_str().unwrap())?;
        if let Some(section) = details.get("section_id").filter(|value| !value.is_null()) {
            known_reference(replica, "sections", section)?;
            if !rows
                .get(&("sections".into(), section.as_str().unwrap().into()))
                .is_some_and(|section| section["list_id"] == row["list_id"])
            {
                details.insert("section_id".into(), Value::Null);
            }
        }
        if let Some(next) = details
            .get("recurrence_next_id")
            .filter(|value| !value.is_null())
        {
            known_reference(replica, "tasks", next)?;
            if !alive(&rows, "tasks", next) {
                details.insert("recurrence_next_id".into(), Value::Null);
            }
        }
        if row["due_date"].is_null() {
            details.insert("recurrence".into(), Value::Null);
        }
        if details.get("start_date").is_none_or(Value::is_null) {
            details.insert("start_time".into(), Value::Null);
        }
        if let (Some(start), Some(due)) = (
            details.get("start_date").and_then(Value::as_str),
            row["due_date"].as_str(),
        ) {
            if start > due
                || (start == due
                    && details
                        .get("start_time")
                        .and_then(Value::as_str)
                        .zip(row["due_time"].as_str())
                        .is_some_and(|(start, due)| start > due))
            {
                details.insert("start_date".into(), Value::Null);
                details.insert("start_time".into(), Value::Null);
            }
        }
        rows.get_mut(&key).unwrap().insert(
            "details_json".into(),
            json!(serde_json::to_string(&details)?),
        );
    }
    if let Some(row) = rows.get(&("settings".into(), "saved_filters".into())) {
        let mut filters: Vec<Value> = serde_json::from_str(row["value"].as_str().unwrap())?;
        for filter in &mut filters {
            for (field, kind) in [("list_ids", "lists"), ("tag_ids", "tags")] {
                if let Some(ids) = filter.get_mut(field).and_then(Value::as_array_mut) {
                    for id in ids.iter() {
                        known_reference(replica, kind, id)?;
                    }
                    ids.retain(|id| alive(&rows, kind, id));
                }
            }
        }
        rows.get_mut(&("settings".into(), "saved_filters".into()))
            .unwrap()
            .insert("value".into(), json!(serde_json::to_string(&filters)?));
    }
    Ok(rows)
}
fn reference_alive(replica: &Replica, kind: &str, value: &Value) -> bool {
    value
        .as_str()
        .and_then(|id| replica.get(&(kind.into(), id.into())))
        .is_some_and(|entity| winner(EXISTS, &entity.fields[EXISTS]).value == true)
}

fn sql_value(value: &Value, column: &str) -> Result<SqlValue> {
    if column == "content" {
        return Ok(SqlValue::Blob(
            STANDARD
                .decode(value.as_str().unwrap_or(""))
                .map_err(|_| ServiceError::Invalid("Invalid attachment encoding.".into()))?,
        ));
    }
    Ok(match value {
        Value::Null => SqlValue::Null,
        Value::String(value) => SqlValue::Text(value.clone()),
        Value::Number(value) => SqlValue::Integer(
            value
                .as_i64()
                .ok_or_else(|| ServiceError::Invalid("Sync integer is out of range.".into()))?,
        ),
        _ => return invalid("Unsupported SQL value in sync projection."),
    })
}
fn apply_projection(conn: &Connection, rows: &Rows) -> Result<bool> {
    let before = snapshot_rows(conn)?;
    if before == *rows {
        return Ok(false);
    }
    conn.pragma_update(None, "defer_foreign_keys", true)?;
    let mut affected_tasks = BTreeSet::new();
    let mut affected_comments = BTreeSet::new();
    for (key, row) in before.iter().chain(rows) {
        if before.get(key) == rows.get(key) {
            continue;
        }
        if key.0 == "comment_attachments" {
            if let Some(id) = row["comment_id"].as_str() {
                affected_comments.insert(id.to_owned());
                if let Some(comment) = rows
                    .get(&("comments".into(), id.into()))
                    .or_else(|| before.get(&("comments".into(), id.into())))
                {
                    if let Some(task) = comment["task_id"].as_str() {
                        affected_tasks.insert(task.to_owned());
                    }
                }
            }
        }
    }
    for (key, row) in &before {
        if rows.get(key) == Some(row) {
            continue;
        }
        if key.0 == "tags" {
            conn.execute(
                "UPDATE tags SET name=?2 WHERE id=?1",
                params![key.1, format!("__sync_{}", Uuid::new_v4())],
            )?;
        }
        if key.0 == "tasks" {
            conn.execute(
                "UPDATE tasks SET recurrence_source_id=NULL WHERE id=?1",
                [&key.1],
            )?;
            if !rows.contains_key(key) {
                conn.execute("UPDATE tasks SET parent_id=NULL WHERE id=?1", [&key.1])?;
            }
        }
        if key.0 == "folders" && !rows.contains_key(key) {
            conn.execute("UPDATE folders SET parent_id=NULL WHERE id=?1", [&key.1])?;
        }
        if let Some(id) = row.get("task_id").and_then(Value::as_str) {
            affected_tasks.insert(id.to_owned());
        }
        if let Some(id) = row.get("linked_task_id").and_then(Value::as_str) {
            affected_tasks.insert(id.to_owned());
        }
    }
    // The schema is a compile-time allowlist; all external values are bound.
    for schema in SCHEMAS {
        for (key, row) in rows.iter().filter(|(key, _)| key.0 == schema.table) {
            if before.get(key) == Some(row) {
                continue;
            }
            let mut columns: Vec<_> = schema
                .columns
                .iter()
                .map(|column| (*column).to_owned())
                .collect();
            let mut values: Vec<_> = schema
                .columns
                .iter()
                .map(|column| sql_value(&row[*column], column))
                .collect::<Result<_>>()?;
            if schema.table == "reminders" && !before.contains_key(key) {
                columns.push("generation".into());
                values.push(SqlValue::Text(Uuid::new_v4().to_string()));
            }
            let mut updates: Vec<_> = schema
                .columns
                .iter()
                .filter(|column| !schema.key.contains(column))
                .map(|column| format!("{column}=excluded.{column}"))
                .collect();
            if matches!(schema.table, "tasks" | "comments") {
                updates.push("version=version+1".into());
            }
            let placeholders = (1..=columns.len())
                .map(|index| format!("?{index}"))
                .collect::<Vec<_>>()
                .join(",");
            let conflict = if updates.is_empty() {
                "DO NOTHING".into()
            } else {
                format!("DO UPDATE SET {}", updates.join(","))
            };
            conn.execute(
                &format!(
                    "INSERT INTO {}({}) VALUES({placeholders}) ON CONFLICT({}) {conflict}",
                    schema.table,
                    columns.join(","),
                    schema.key.join(",")
                ),
                rusqlite::params_from_iter(values),
            )?;
            if let Some(id) = row.get("task_id").and_then(Value::as_str) {
                affected_tasks.insert(id.to_owned());
            }
            if let Some(id) = row.get("linked_task_id").and_then(Value::as_str) {
                affected_tasks.insert(id.to_owned());
            }
        }
    }
    for schema in SCHEMAS.iter().rev() {
        for (key, row) in before
            .iter()
            .filter(|(key, _)| key.0 == schema.table && !rows.contains_key(*key))
        {
            let predicate = schema
                .key
                .iter()
                .enumerate()
                .map(|(index, column)| format!("{column}=?{}", index + 1))
                .collect::<Vec<_>>()
                .join(" AND ");
            let values = schema
                .key
                .iter()
                .map(|column| sql_value(&row[*column], column))
                .collect::<Result<Vec<_>>>()?;
            conn.execute(
                &format!("DELETE FROM {} WHERE {predicate}", schema.table),
                rusqlite::params_from_iter(values),
            )?;
            if key.0 == "tasks" {
                affected_tasks.remove(&key.1);
            }
        }
    }
    for id in affected_tasks {
        if rows.get(&("tasks".into(), id.clone())) == before.get(&("tasks".into(), id.clone())) {
            conn.execute("UPDATE tasks SET version=version+1 WHERE id=?1", [id])?;
        }
    }
    for id in affected_comments {
        if rows.get(&("comments".into(), id.clone()))
            == before.get(&("comments".into(), id.clone()))
        {
            conn.execute("UPDATE comments SET version=version+1 WHERE id=?1", [id])?;
        }
    }
    conn.execute("UPDATE tasks SET reminder_at=(SELECT min(at) FROM reminders WHERE task_id=tasks.id),reminder_notified_at=(SELECT notified_at FROM reminders WHERE task_id=tasks.id ORDER BY at LIMIT 1)",[])?;
    conn.execute("DELETE FROM pending_reminders WHERE NOT EXISTS(SELECT 1 FROM reminders r WHERE r.task_id=pending_reminders.task_id AND r.at=pending_reminders.at AND r.generation=pending_reminders.generation)",[])?;
    for (key, row) in rows.iter().filter(|(key, _)| key.0 == "settings") {
        let value: Value = serde_json::from_str(row["value"].as_str().unwrap())?;
        extensions::set_setting(
            conn,
            json!({"key":key.1,"value":value})
                .as_object()
                .expect("object"),
        )?;
    }
    let violations: i64 =
        conn.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })?;
    if violations > 0 {
        return invalid("Sync would leave invalid relationships; no changes were saved.");
    }
    Ok(true)
}

fn is_conflict(field: &str, versions: &[Version]) -> bool {
    // Timestamp disagreements describe the edit, not lost user content.
    !matches!(field, "id" | "created_at" | "updated_at" | "preview_json")
        && versions
            .first()
            .is_some_and(|first| versions.iter().any(|version| version.value != first.value))
}
fn conflict_count(replica: &Replica) -> usize {
    replica
        .values()
        .map(|entity| {
            entity
                .fields
                .iter()
                .filter(|(field, versions)| is_conflict(field, versions))
                .count()
        })
        .sum()
}
fn status(conn: &Connection) -> Result<Value> {
    let merged = meta(conn, "last_merge_at")?;
    let entities: i64 =
        conn.query_row("SELECT count(*) FROM sync_entities", [], |row| row.get(0))?;
    Ok(json!({
        "enabled":meta(conn,"enabled")? == "1",
        "device_id":meta(conn,"device_id")?,
        "clock":meta(conn,"clock")?.parse::<i64>().unwrap_or(0),
        "revision":meta(conn,"revision")?.parse::<i64>().unwrap_or(0),
        "entities":entities,
        "pending_conflicts":meta(conn,"pending_conflicts")?.parse::<i64>().unwrap_or(0),
        "last_merge_at":if merged.is_empty() { None } else { Some(merged) },
    }))
}
pub(super) fn read(conn: &Connection, action: &str, data: &Map<String, Value>) -> Result<Value> {
    if action == "sync.checkpoint" {
        fields(data, &["provider"])?;
        let provider = checkpoint_provider(required(data, "provider")?)?;
        let token = meta(conn, &format!("checkpoint:{provider}"))?;
        return Ok(json!({"token":if token.is_empty(){None}else{Some(token)}}));
    }
    fields(data, &[])?;
    if action == "sync.status" {
        return status(conn);
    }
    if action != "sync.conflicts" {
        return invalid("Unknown sync query.");
    }
    let replica = load_replica(conn)?;
    let mut items = Vec::new();
    for entity in replica.values() {
        let label = entity
            .fields
            .get("title")
            .or_else(|| entity.fields.get("name"))
            .map(|versions| winner("title", versions).value.clone())
            .unwrap_or(json!(entity.id));
        for (field, versions) in &entity.fields {
            if !is_conflict(field, versions) {
                continue;
            }
            let selected = winner(field, versions);
            let mut distinct: Vec<&Value> = Vec::new();
            let mut alternatives = Vec::new();
            // Winning version first makes the current visible value explicit.
            let mut ordered: Vec<_> = versions.iter().collect();
            ordered.sort_by_key(|version| (version.id() != selected.id(), version.id()));
            for version in ordered {
                if distinct.contains(&&version.value) {
                    continue;
                }
                distinct.push(&version.value);
                let serialized = serde_json::to_string(&version.value)?;
                let truncated = serialized.len() > 4096 || field == "content";
                let value = if field == "content" {
                    json!(format!(
                        "Attachment content ({} encoded bytes)",
                        version.value.as_str().map(str::len).unwrap_or(0)
                    ))
                } else if truncated {
                    json!(version
                        .value
                        .as_str()
                        .unwrap_or(&serialized)
                        .chars()
                        .take(1000)
                        .collect::<String>())
                } else {
                    version.value.clone()
                };
                alternatives.push(json!({"id":version.id(),"device_id":version.device_id,"clock":version.clock,"value":value,"truncated":truncated}));
            }
            items.push(json!({"kind":entity.kind,"id":entity.id,"field":field,"label":label,"winner_id":selected.id(),"versions":alternatives}));
        }
    }
    Ok(json!({"items":items}))
}
fn export(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    fields(data, &[])?;
    activate(conn)?;
    let packet = Packet {
        protocol: "tasks-sync".into(),
        version: 1,
        device_id: meta(conn, "device_id")?,
        clock: meta(conn, "clock")?
            .parse()
            .map_err(|_| ServiceError::Invalid("Invalid local sync clock.".into()))?,
        vector: serde_json::from_str(&meta(conn, "vector")?)?,
        entities: load_replica(conn)?.into_values().collect(),
    };
    // Validate local data too: an unsupported legacy value must fail explicitly,
    // never vanish from a successful-looking cloud upload.
    validate_packet(&packet)?;
    let value = serde_json::to_value(packet)?;
    if serde_json::to_vec(&value)?.len() > MAX_PACKET_BYTES {
        return invalid("Sync packet exceeds 100 MiB. Reduce attachments before syncing.");
    }
    Ok(value)
}
fn merge(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    fields(
        data,
        &["packets", "checkpoint_provider", "expected_checkpoint"],
    )?;
    let checkpoint = match (
        data.get("checkpoint_provider"),
        data.get("expected_checkpoint"),
    ) {
        (None, None) => None,
        (Some(provider), Some(token)) => {
            let provider = checkpoint_provider(provider.as_str().unwrap_or(""))?;
            let token = token
                .as_str()
                .ok_or_else(|| ServiceError::Invalid("Invalid sync checkpoint token.".into()))?;
            validate_checkpoint_token(token)?;
            Some((provider, token))
        }
        _ => return invalid("Supply checkpoint_provider and expected_checkpoint together."),
    };
    let input = data
        .get("packets")
        .and_then(Value::as_array)
        .filter(|packets| packets.len() <= 32)
        .ok_or_else(|| {
            ServiceError::Invalid("packets must contain at most 32 device packets.".into())
        })?;
    let mut packets = Vec::new();
    let mut size = 0usize;
    let local_clock: i64 = meta(conn, "clock")?.parse().unwrap_or(0);
    for value in input {
        let bytes = serde_json::to_vec(value)?;
        size = size.saturating_add(bytes.len());
        if bytes.len() > MAX_PACKET_BYTES || size > MAX_PACKET_BYTES * 2 {
            return invalid("Sync packet batch exceeds the safe size limit.");
        }
        let packet: Packet = serde_json::from_value(value.clone())?;
        validate_packet(&packet)?;
        if packet.clock > local_clock.saturating_add(1_000_000) {
            return invalid("Remote sync clock advanced implausibly far; no changes were saved.");
        }
        packets.push(packet);
    }
    if packets.is_empty() {
        return Ok(
            json!({"merged":0,"changed":false,"visible_changes":false,"status":status(conn)?}),
        );
    }
    // Persist any local edit first, in this same transaction, before applying
    // cloud input. A malformed later packet rolls back this step as well.
    activate(conn)?;
    recover_restored_identity(conn, &packets, checkpoint)?;
    let mut replica = load_replica(conn)?;
    let original = replica.clone();
    let mut vector: Vector = serde_json::from_str(&meta(conn, "vector")?)?;
    for packet in &packets {
        for (device, clock) in &packet.vector {
            vector
                .entry(device.clone())
                .and_modify(|local| *local = (*local).max(*clock))
                .or_insert(*clock);
        }
        for incoming in &packet.entities {
            let key = (incoming.kind.clone(), incoming.id.clone());
            if let Some(existing) = replica.get_mut(&key) {
                for (field, versions) in &incoming.fields {
                    let merged = merge_register(
                        existing.fields.get(field).map(Vec::as_slice).unwrap_or(&[]),
                        versions,
                    )?;
                    existing.fields.insert(field.clone(), merged);
                }
            } else {
                let mut incoming = incoming.clone();
                for versions in incoming.fields.values_mut() {
                    *versions = merge_register(&[], versions)?;
                }
                replica.insert(key, incoming);
            }
        }
    }
    if replica.len() > MAX_ENTITIES || vector.len() > MAX_DEVICES {
        return invalid("Sync workspace exceeds the supported size.");
    }
    let projection = materialized(&replica)?;
    let visible_changes = apply_projection(conn, &projection)?;
    let changed = replica != original;
    if changed {
        for (key, entity) in &replica {
            if original.get(key) != Some(entity) {
                save_entity(conn, entity)?;
            }
        }
        bump_revision(conn)?;
        put_meta(conn, "pending_conflicts", conflict_count(&replica))?;
    }
    let clock = vector.values().copied().max().unwrap_or(0);
    put_meta(conn, "clock", clock)?;
    put_meta(conn, "vector", serde_json::to_string(&vector)?)?;
    if !packets.is_empty() {
        put_meta(conn, "last_merge_at", now())?;
    }
    if visible_changes {
        save_projection(conn, &snapshot_rows(conn)?)?;
    }
    Ok(
        json!({"merged":packets.len(),"changed":changed,"visible_changes":visible_changes,"status":status(conn)?}),
    )
}
fn resolve(conn: &Connection, data: &Map<String, Value>) -> Result<Value> {
    fields(data, &["kind", "id", "field", "version_id"])?;
    record_local(conn)?;
    let kind = required(data, "kind")?;
    let id = required(data, "id")?;
    let field = required(data, "field")?;
    let version_id = required(data, "version_id")?;
    schema(kind)?;
    let mut replica = load_replica(conn)?;
    let entity = replica
        .get_mut(&(kind.into(), id.into()))
        .ok_or_else(|| ServiceError::NotFound("Sync entity".into()))?;
    let versions = entity
        .fields
        .get(field)
        .ok_or_else(|| ServiceError::NotFound("Sync field".into()))?;
    if !is_conflict(field, versions) {
        return invalid("This sync conflict has already been resolved.");
    }
    let selected = versions
        .iter()
        .find(|version| version.id() == version_id)
        .ok_or_else(|| ServiceError::NotFound("Sync version".into()))?
        .value
        .clone();
    let mut context: Vector = serde_json::from_str(&meta(conn, "vector")?)?;
    let (device_id, clock) = next_stamp(conn, &mut context)?;
    entity.fields.insert(
        field.into(),
        vec![Version {
            device_id,
            clock,
            context,
            value: selected,
        }],
    );
    let resolved = entity.clone();
    let projection = materialized(&replica)?;
    let visible_changes = apply_projection(conn, &projection)?;
    save_entity(conn, &resolved)?;
    put_meta(conn, "pending_conflicts", conflict_count(&replica))?;
    if visible_changes {
        save_projection(conn, &snapshot_rows(conn)?)?;
    }
    bump_revision(conn)?;
    Ok(json!({"ok":true,"status":status(conn)?}))
}
pub(super) fn mutate(conn: &Connection, action: &str, data: &Map<String, Value>) -> Result<Value> {
    match action {
        "sync.export" => export(conn, data),
        "sync.merge" => merge(conn, data),
        "sync.resolve" => resolve(conn, data),
        "sync.checkpoint.set" => {
            fields(data, &["provider", "token"])?;
            let provider = checkpoint_provider(required(data, "provider")?)?;
            let token = required(data, "token")?;
            validate_checkpoint_token(token)?;
            put_meta(conn, &format!("checkpoint:{provider}"), token)?;
            let vector: Vector = serde_json::from_str(&meta(conn, "vector")?)?;
            let clock = vector.get(&meta(conn, "device_id")?).copied().unwrap_or(0);
            put_meta(conn, &format!("checkpoint_clock:{provider}"), clock)?;
            Ok(json!({"ok":true}))
        }
        _ => invalid("Unknown sync action."),
    }
}
fn checkpoint_provider(provider: &str) -> Result<&str> {
    if ["google_drive", "dropbox"].contains(&provider) {
        Ok(provider)
    } else {
        invalid("Invalid sync checkpoint provider.")
    }
}
fn validate_checkpoint_token(token: &str) -> Result<()> {
    if token.len() == 43
        && token
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
    {
        Ok(())
    } else {
        invalid("Invalid sync checkpoint token.")
    }
}
fn recover_restored_identity(
    conn: &Connection,
    packets: &[Packet],
    checkpoint: Option<(&str, &str)>,
) -> Result<()> {
    let device = meta(conn, "device_id")?;
    let mut vector: Vector = serde_json::from_str(&meta(conn, "vector")?)?;
    let local_counter = vector.get(&device).copied().unwrap_or(0);
    let replica = load_replica(conn)?;
    let explicit_restore = checkpoint.is_some_and(|(provider, expected)| {
        meta(conn, &format!("checkpoint:{provider}")).is_ok_and(|actual| actual != expected)
    });
    let own_remote_ahead = packets
        .iter()
        .any(|packet| packet.vector.get(&device).copied().unwrap_or(0) > local_counter);
    let reused_dot = packets
        .iter()
        .flat_map(|packet| &packet.entities)
        .any(|incoming| {
            replica
                .get(&(incoming.kind.clone(), incoming.id.clone()))
                .is_some_and(|local| {
                    incoming.fields.iter().any(|(field, versions)| {
                        versions
                            .iter()
                            .filter(|version| version.device_id == device)
                            .any(|incoming| {
                                local.fields.get(field).is_some_and(|versions| {
                                    versions.iter().any(|local| {
                                        local.device_id == device
                                            && local.clock == incoming.clock
                                            && local != incoming
                                    })
                                })
                            })
                    })
                })
        });
    if !explicit_restore && !own_remote_ahead && !reused_dot {
        return Ok(());
    }
    let acknowledged = if let Some((provider, _)) = checkpoint {
        meta(conn, &format!("checkpoint_clock:{provider}"))?
            .parse::<i64>()
            .unwrap_or(0)
    } else {
        ["google_drive", "dropbox"]
            .into_iter()
            .filter_map(|provider| {
                meta(conn, &format!("checkpoint_clock:{provider}"))
                    .ok()?
                    .parse::<i64>()
                    .ok()
            })
            .max()
            .unwrap_or(0)
    };
    let replacement = Uuid::new_v4().to_string();
    let mut rebased = false;
    for mut entity in replica.into_values() {
        let mut changed = false;
        for versions in entity.fields.values_mut() {
            for version in versions {
                if version.device_id == device && version.clock > acknowledged {
                    version.device_id = replacement.clone();
                    if acknowledged == 0 {
                        version.context.remove(&device);
                    } else {
                        version.context.insert(device.clone(), acknowledged);
                    }
                    version.context.insert(replacement.clone(), version.clock);
                    changed = true;
                    rebased = true;
                }
            }
        }
        if changed {
            save_entity(conn, &entity)?;
        }
    }
    if acknowledged == 0 {
        vector.remove(&device);
    } else {
        vector.insert(device, acknowledged);
    }
    if rebased {
        vector.insert(replacement.clone(), local_counter);
    }
    put_meta(conn, "device_id", replacement)?;
    put_meta(conn, "vector", serde_json::to_string(&vector)?)?;
    for provider in ["google_drive", "dropbox"] {
        put_meta(conn, &format!("checkpoint:{provider}"), "")?;
        put_meta(conn, &format!("checkpoint_clock:{provider}"), 0)?;
    }
    bump_revision(conn)
}
