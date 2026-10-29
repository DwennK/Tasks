use serde_json::{json, Map, Value};

pub(crate) fn string(description: &str) -> Value {
    json!({"type":"string","description":description})
}
pub(crate) fn nullable_string(description: &str) -> Value {
    json!({"type":["string","null"],"description":description})
}
pub(crate) fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
pub(crate) fn tool(
    name: &str,
    title: &str,
    description: &str,
    schema: Value,
    read_only: bool,
    destructive: bool,
    idempotent: bool,
) -> Value {
    json!({"name":name,"title":title,"description":description,"inputSchema":schema,"annotations":{"readOnlyHint":read_only,"destructiveHint":destructive,"idempotentHint":idempotent,"openWorldHint":false}})
}
fn task_fields() -> Map<String, Value> {
    let mut map = Map::new();
    map.insert(
        "title".into(),
        json!({"type":"string","minLength":1,"maxLength":500,"description":"Task title"}),
    );
    map.insert("notes".into(), string("Plain text or Markdown notes"));
    map.insert(
        "list_id".into(),
        string("List ID; default inbox. Use list_lists to discover IDs."),
    );
    map.insert(
        "parent_id".into(),
        nullable_string(
            "Parent task ID for a subtask; null removes parent. Child inherits parent's list.",
        ),
    );
    map.insert(
        "due_date".into(),
        nullable_string("Local calendar date YYYY-MM-DD; null clears it"),
    );
    map.insert(
        "due_time".into(),
        nullable_string("Local time HH:MM, requires due_date; null clears it"),
    );
    map.insert("reminder_at".into(),nullable_string("RFC3339 instant including offset; null clears reminder. Desktop must be running for notifications."));
    map.insert("priority".into(),json!({"type":"integer","minimum":0,"maximum":3,"description":"0 none, 1 low, 2 medium, 3 high"}));
    map.extend(crate::extended_schema::task_fields());
    map
}
pub fn tools() -> Vec<Value> {
    let id = string("Task ID returned by list_tasks or create_task");
    let id_schema = object(json!({"id":id}), &["id"]);
    let create = task_fields();
    let mut update = create.clone();
    update.insert("id".into(), id.clone());
    update.insert("expected_version".into(),json!({"type":"integer","minimum":1,"description":"Reject a stale edit instead of overwriting a concurrent change"}));
    let mut tools=vec![
        tool("list_tasks","List tasks","List or instantly search local tasks. Defaults to active top-level tasks. Today includes overdue; Upcoming covers seven days. Use include_subtasks to return children; parent_id filters direct children.",object(json!({"view":{"type":"string","enum":["inbox","today","tomorrow","upcoming","all","completed","trash","wont_do"]},"list_id":string("Only this list"),"tag_id":string("Only tasks with this tag"),"section_id":nullable_string("Only tasks in this section; null means unsectioned"),"search":string("Substring in task title or rendered note text; HTML/Markdown formatting and entities are normalized"),"date":string("Local today YYYY-MM-DD; defaults to the computer's local date"),"parent_id":nullable_string("Only direct children of this task; null selects top-level tasks"),"include_subtasks":{"type":"boolean"}}),&[]),true,false,true),
        tool("get_task","Get task","Read a single task, including completed or trashed tasks.",id_schema.clone(),true,false,true),
        tool("create_task","Create task","Create a task in Inbox or a chosen list, or create a subtask. Returns the persistent ID.",object(Value::Object(create),&["title"]),false,false,false),
        tool("update_task","Update task","Update only supplied fields; omitted fields stay unchanged. Null clears nullable values. Moving a task moves its subtree. Use expected_version to guard concurrent edits.",object(Value::Object(update),&["id"]),false,true,true),
        tool("complete_task","Complete or reopen task","Complete a task and its subtasks. Set completed=false to reopen it and required ancestors.",object(json!({"id":id,"completed":{"type":"boolean","default":true}}),&["id"]),false,true,true),
        tool("delete_task","Move task to Trash","Recoverable soft deletion of a task and its subtasks. Restore with restore_task.",id_schema.clone(),false,true,true),
        tool("restore_task","Restore task","Restore a task and its subtree from Trash, including required ancestors.",id_schema,false,false,true),
        tool("reorder_tasks","Reorder tasks","Set relative order of the supplied sibling task IDs. IDs must be unique and share a parent and list.",object(json!({"ids":{"type":"array","items":{"type":"string"},"minItems":1,"uniqueItems":true}}),&["ids"]),false,true,true),
        tool("list_lists","List task lists","List all task lists and their folder IDs, including the protected inbox.",object(json!({}),&[]),true,false,true),
        tool("list_folders","List folders","List all folders, their parent IDs and ordering.",object(json!({}),&[]),true,false,true),
        tool("create_list","Create task list","Create a local task list, optionally inside a folder.",object(json!({"name":string("List name"),"folder_id":nullable_string("Folder ID or null for root"),"color":string("Hex color such as #5b7fff, or empty string for None")}),&["name"]),false,false,false),
        tool("update_list","Update task list","Rename, recolor or move a list to a folder. Null folder_id moves to root.",object(json!({"id":string("List ID"),"name":string("List name"),"folder_id":nullable_string("Folder ID or null for root"),"color":string("Hex color, or empty string for None")}),&["id"]),false,true,true),
        tool("delete_list","Delete task list","Remove a list, preserving all its tasks by moving them to Inbox. Inbox cannot be deleted.",object(json!({"id":string("List ID")}),&["id"]),false,true,false),
        tool("create_folder","Create folder","Create a folder at root or inside another folder.",object(json!({"name":string("Folder name"),"parent_id":nullable_string("Parent folder ID or null for root")}),&["name"]),false,false,false),
        tool("update_folder","Update folder","Rename or move a folder. Cycles are rejected; null parent_id moves to root.",object(json!({"id":string("Folder ID"),"name":string("Folder name"),"parent_id":nullable_string("Parent folder ID or null for root")}),&["id"]),false,true,true),
        tool("delete_folder","Delete folder","Remove a folder, preserving its contents by moving child folders and lists to its parent.",object(json!({"id":string("Folder ID")}),&["id"]),false,true,false),
    ];
    for entry in &mut tools {
        if entry["name"] == "create_list" || entry["name"] == "update_list" {
            entry["inputSchema"]["properties"]
                .as_object_mut()
                .unwrap()
                .extend(crate::extended_schema::list_fields());
        }
    }
    for entry in &mut tools {
        if entry["name"] == "create_folder" || entry["name"] == "update_folder" {
            entry["inputSchema"]["properties"]
                .as_object_mut()
                .unwrap()
                .extend(crate::extended_schema::navigation_fields());
        }
    }
    tools.extend(crate::extended_schema::tools());
    tools
}
// Validate the deliberately small JSON Schema subset used above. Business validation
// remains in tasks-core and cannot be bypassed through CLI/MCP/GUI.
pub fn validate(schema: &Value, value: &Value) -> Result<(), String> {
    validate_at(schema, value, "arguments")
}
fn validate_at(schema: &Value, value: &Value, path: &str) -> Result<(), String> {
    if let Some(kind) = schema.get("type") {
        let valid = |t: &str| match t {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "null" => value.is_null(),
            "boolean" => value.is_boolean(),
            "integer" => value.is_i64() || value.is_u64(),
            _ => false,
        };
        let matches = kind.as_str().is_some_and(valid)
            || kind
                .as_array()
                .is_some_and(|types| types.iter().any(|t| t.as_str().is_some_and(valid)));
        if !matches {
            return Err(format!("{path}: expected {kind}"));
        }
    }
    if let Some(choices) = schema["enum"].as_array() {
        if !choices.contains(value) {
            return Err(format!("{path}: unsupported value"));
        }
    }
    if let Some(n) = value.as_f64() {
        if schema["minimum"].as_f64().is_some_and(|min| n < min)
            || schema["maximum"].as_f64().is_some_and(|max| n > max)
        {
            return Err(format!("{path}: outside allowed range"));
        }
    }
    if let Some(text) = value.as_str() {
        let len = text.chars().count() as u64;
        if schema["minLength"].as_u64().is_some_and(|min| len < min)
            || schema["maxLength"].as_u64().is_some_and(|max| len > max)
        {
            return Err(format!("{path}: invalid string length"));
        }
    }
    if let Some(map) = value.as_object() {
        if let Some(required) = schema["required"].as_array() {
            for k in required.iter().filter_map(Value::as_str) {
                if !map.contains_key(k) {
                    return Err(format!("{path}.{k} is required"));
                }
            }
        }
        let props = schema["properties"].as_object();
        for (key, v) in map {
            match props.and_then(|p| p.get(key)) {
                Some(s) => validate_at(s, v, &format!("{path}.{key}"))?,
                None if schema["additionalProperties"] == false => {
                    return Err(format!("{path}.{key}: unknown field"))
                }
                None => {}
            }
        }
    }
    if let Some(array) = value.as_array() {
        if schema["minItems"]
            .as_u64()
            .is_some_and(|min| array.len() < (min as usize))
        {
            return Err(format!("{path}: too few items"));
        }
        if schema["maxItems"]
            .as_u64()
            .is_some_and(|max| array.len() > (max as usize))
        {
            return Err(format!("{path}: too many items"));
        }
        for (i, v) in array.iter().enumerate() {
            if !schema["items"].is_null() {
                validate_at(&schema["items"], v, &format!("{path}[{i}]"))?;
            }
            if schema["uniqueItems"] == true && array[..i].contains(v) {
                return Err(format!("{path}: duplicate item"));
            }
        }
    }
    Ok(())
}
