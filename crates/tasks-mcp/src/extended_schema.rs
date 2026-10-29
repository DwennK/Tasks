use crate::schema::{nullable_string, object, string, tool};
use serde_json::{json, Map, Value};
fn choice(values: &[&str]) -> Value {
    json!({"type":"string","enum":values})
}
fn ids() -> Value {
    json!({"type":"array","items":{"type":"string"},"uniqueItems":true})
}
fn name() -> Value {
    json!({"type":"string","minLength":1,"maxLength":500})
}
pub fn task_fields() -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("is_note".into(),json!({"type":"boolean","description":"Note instead of task; omitted creation value inherits list type. Conversion preserves content and relationships."}));
    m.insert(
        "start_date".into(),
        nullable_string("Local start date YYYY-MM-DD"),
    );
    m.insert(
        "start_time".into(),
        nullable_string("Local start time HH:MM"),
    );
    m.insert(
        "duration_minutes".into(),
        json!({"type":["integer","null"],"minimum":1,"maximum":525600}),
    );
    m.insert(
        "timezone".into(),
        string("IANA timezone; e.g. Europe/Zurich. Defaults to system timezone."),
    );
    m.insert("recurrence".into(),json!({"type":["object","null"],"properties":{
        "frequency":choice(&["daily","weekly","monthly","yearly"]),"interval":{"type":"integer","minimum":1,"maximum":365},
        "weekdays":{"type":"array","items":{"type":"integer","minimum":1,"maximum":7},"uniqueItems":true,"maxItems":7},
        "month_days":{"type":"array","items":{"type":"integer","enum":[-1,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31]},"uniqueItems":true,"maxItems":32,"description":"Monthly dates; -1 is Last Day. Yearly accepts one date 1..31. Missing dates skip their period."},
        "ordinal_weekday":{"type":["object","null"],"properties":{"ordinal":{"type":"integer","enum":[-1,1,2,3,4,5]},"weekday":{"type":"integer","minimum":1,"maximum":7}},"required":["ordinal","weekday"],"additionalProperties":false,"description":"Monthly/yearly nth weekday; ordinal -1 is Last, weekdays are ISO Monday1 to Sunday7."},
        "workday":{"type":["string","null"],"enum":["first","last",null],"description":"Monthly first/last Monday–Friday; excludes regional holiday calendars."},
        "year_month":{"type":["integer","null"],"minimum":1,"maximum":12,"description":"Annual month, combined with month_days or ordinal_weekday."},
        "skip_weekends":{"type":"boolean","description":"Omit weekend dates in monthly calendar-date rules. Cannot combine with ordinal_weekday or workday."},
        "end_date":nullable_string("Inclusive end date YYYY-MM-DD"),"anchor_date":nullable_string("Original due date preserving month-end intent; normally omit to use automatic anchor"),"count":{"type":["integer","null"],"minimum":1,"maximum":1000000},
        "mode":choice(&["due_date","completion_date"])
    },"required":["frequency"],"additionalProperties":false,"description":"Repeat rule; due_date required. Completing creates exactly one next occurrence. month_days, ordinal_weekday and workday are mutually exclusive. Null clears recurrence."}));
    m.insert("reminders".into(),json!({"type":"array","items":{"type":"string"},"maxItems":20,"description":"Replace all reminders with RFC3339 instants; [] clears. Do not combine with legacy reminder_at."}));
    let mut tags = ids();
    tags["maxItems"] = json!(100);
    m.insert("tag_ids".into(), tags);
    m.insert(
        "notes_format".into(),
        choice(&["plain", "markdown", "html"]),
    );
    m.insert("checklist".into(),json!({"type":"array","maxItems":1000,"items":object(json!({"id":string("Stable checklist item identifier"),"text":{"type":"string","minLength":1,"maxLength":500},"completed":{"type":"boolean"}}),&["id","text","completed"])}));
    m.insert("pinned".into(), json!({"type":"boolean"}));
    m.insert(
        "section_id".into(),
        nullable_string("Section in the task's list; null clears"),
    );
    m
}
pub fn navigation_fields() -> Map<String, Value> {
    json!({"icon":{"type":["string","null"],"maxLength":64,"description":"Sidebar icon, empty string or null clears; no control characters"},"pinned":{"type":"boolean"}}).as_object().unwrap().clone()
}
pub fn list_fields() -> Map<String, Value> {
    let mut fields=json!({"view_mode":choice(&["list","board"]),"group_by":choice(&["none","section","date","priority","list","tag"]),"sort_by":choice(&["manual","due_date","priority","title","created_at"]),"sort_order":choice(&["asc","desc"]),"show_in_smart_lists":{"type":"boolean","description":"Include active tasks in smart views; explicit list and global search still expose them"},"list_type":choice(&["task","note"])}).as_object().unwrap().clone();
    fields.extend(navigation_fields());
    fields
}
pub fn tools() -> Vec<Value> {
    let id = string("Persistent entity ID");
    let one = object(json!({"id":id}), &["id"]);
    let links = object(
        json!({"id":id,"linked_task_id":string("Related task ID")}),
        &["id", "linked_task_id"],
    );
    let reorder = object(
        json!({"ids":{"type":"array","items":{"type":"string"},"uniqueItems":true,"minItems":1}}),
        &["ids"],
    );
    vec![
        tool("preview_recurrence","Preview recurrence","Calculate future draft occurrences without database reads or writes. Range is inclusive, at most 366 days; initial due date is excluded. Completion-date rules assume completion on each due date and report that assumption.",object(json!({"due_date":string("Initial due date YYYY-MM-DD"),"recurrence":task_fields()["recurrence"],"timezone":string("Optional IANA timezone"),"range_start":string("Inclusive range start YYYY-MM-DD"),"range_end":string("Inclusive range end YYYY-MM-DD"),"limit":{"type":"integer","minimum":1,"maximum":512,"default":128}}),&["due_date","recurrence","range_start","range_end"]),true,false,true),
        tool("duplicate_task","Duplicate task","Clone a task with subtasks, checklist, tags and attachments. New IDs; completion and trash states reset.",object(json!({"id":id,"list_id":string("Optional destination list"),"title":name()}),&["id"]),false,false,false),
        tool("dismiss_task","Mark Won't Do","Dismiss a task and its subtree. wont_do=false reactivates the task and required ancestors.",object(json!({"id":id,"wont_do":{"type":"boolean","default":true}}),&["id"]),false,true,true),
        tool("set_task_tags","Set task tags","Replace task tag associations; [] clears them.",object(json!({"id":id,"tag_ids":ids(),"expected_version":{"type":"integer","minimum":1,"description":"Expected task version; rejects a stale tag replacement"}}),&["id","tag_ids"]),false,true,true),
        tool("list_tags","List tags","Discover tags and their IDs.",object(json!({}),&[]),true,false,true),
        tool("create_tag","Create tag","Create a tag with a case-insensitively unique name.",object(json!({"name":name(),"color":string("Hex color")}),&["name"]),false,false,false),
        tool("update_tag","Update tag","Rename or recolor a tag.",object(json!({"id":id,"name":name(),"color":string("Hex color")}),&["id"]),false,true,true),
        tool("delete_tag","Delete tag","Delete tag and remove its task associations; preserves tasks.",one.clone(),false,true,false),
        tool("link_tasks","Link related tasks","Create a bidirectional link between two tasks; no self links.",links.clone(),false,false,true),
        tool("unlink_tasks","Unlink tasks","Remove the bidirectional link; preserves both tasks.",links,false,true,true),
        tool("add_attachment","Add small attachment","Store base64 bytes locally in SQLite. At most 512 KiB decoded through this tool. For files up to20 MiB use import_attachment with an explicit local path.",object(json!({"id":string("Task ID"),"name":{"type":"string","minLength":1,"maxLength":255},"mime_type":{"type":"string","maxLength":255},"content_base64":{"type":"string","maxLength":699052,"description":"Canonical base64, max512 KiB decoded; no data URL prefix"}}),&["id","name","content_base64"]),false,false,false),
        tool("import_attachment","Import local attachment","Read the explicit local file path and copy up to20 MiB into task SQLite storage. Only use a path the user intends to attach. Does not upload anywhere.",object(json!({"task_id":string("Task ID"),"path":string("Explicit local filesystem path to a regular file, maximum20 MiB")}),&["task_id","path"]),false,false,false),
        tool("read_attachment_chunk","Read attachment chunk","Read up to256 KiB raw attachment bytes, returned as base64 with metadata and next_offset. Continue with next_offset until null. Offsets count decoded bytes.",object(json!({"id":string("Attachment ID from task attachments metadata"),"offset":{"type":"integer","minimum":0,"default":0},"length":{"type":"integer","minimum":1,"maximum":262144,"default":262144}}),&["id"]),true,false,true),
        tool("remove_attachment","Remove attachment","Permanently remove one attachment's bytes and metadata from the task database.",one.clone(),false,true,false),
        tool("duplicate_list","Duplicate list","Copy list metadata, sections and nontrashed items with fresh IDs, preserving item states and remapping internal links. Copy starts unpinned and unarchived.",object(json!({"id":string("Source list ID"),"name":name(),"folder_id":nullable_string("Destination folder ID, null for root; omit to preserve source folder")}),&["id"]),false,false,false),
        tool("archive_list","Archive or unarchive list","Archive list from active views; archived=false restores it. Tasks are preserved. Inbox cannot be archived.",object(json!({"id":id,"archived":{"type":"boolean","default":true}}),&["id"]),false,true,true),
        tool("reorder_lists","Reorder lists","Reorder lists in the same folder.",reorder.clone(),false,true,true),
        tool("reorder_folders","Reorder folders","Reorder sibling folders.",reorder.clone(),false,true,true),
        tool("list_sections","List sections","Discover all list sections and their list IDs.",object(json!({}),&[]),true,false,true),
        tool("create_section","Create section","Create a section within a task list.",object(json!({"list_id":string("List ID"),"name":name()}),&["list_id","name"]),false,false,false),
        tool("update_section","Rename section","Rename a list section.",object(json!({"id":id,"name":name()}),&["id","name"]),false,true,true),
        tool("delete_section","Delete section","Remove section and clear task section assignments; preserves tasks.",one.clone(),false,true,false),
        tool("reorder_sections","Reorder sections","Reorder sections within one list.",reorder,false,true,true),
        tool("list_templates","List templates","List reusable task templates.",object(json!({}),&[]),true,false,true),
        tool("create_template","Save task as template","Capture the task and subtasks as a reusable template.",object(json!({"name":name(),"task_id":string("Source task ID")}),&["name","task_id"]),false,false,false),
        tool("update_template","Rename template","Rename a saved template.",object(json!({"id":id,"name":name()}),&["id","name"]),false,true,true),
        tool("delete_template","Delete template","Remove a template; existing tasks are preserved.",one.clone(),false,true,false),
        tool("apply_template","Create task from template","Create task and subtasks with new IDs from template, optionally in another list.",object(json!({"id":id,"list_id":string("Destination list ID")}),&["id"]),false,false,false),
        tool("get_template","Get template","Read one reusable template preview without attachment binary content.",one.clone(),true,false,true),
        tool("list_comments","List task comments","Read task comments and attachment associations stored locally.",object(json!({"id":string("Task ID")}),&["id"]),true,false,true),
        tool("add_comment","Add task comment","Add a local comment authored by You with text or attachment IDs. No message is sent to anyone.",object(json!({"id":string("Task ID"),"body":{"type":"string","maxLength":10000},"attachment_ids":{"type":"array","items":{"type":"string"},"uniqueItems":true,"maxItems":20}}),&["id"]),false,false,false),
        tool("update_comment","Update task comment","Edit body or attachment associations on a local comment.",object(json!({"id":string("Comment ID"),"expected_version":{"type":"integer","minimum":1,"description":"Expected comment version (not task version), returned by list_comments"},"body":{"type":"string","maxLength":10000},"attachment_ids":{"type":"array","items":{"type":"string"},"uniqueItems":true,"maxItems":20}}),&["id"]),false,true,true),
        tool("delete_comment","Delete task comment","Permanently delete one local comment; task is preserved.",object(json!({"id":string("Comment ID"),"expected_version":{"type":"integer","minimum":1,"description":"Expected comment version (not task version), returned by list_comments"}}),&["id"]),false,true,false),
        tool("get_settings","Read settings","Read persisted application preferences.",object(json!({}),&[]),true,false,true),
        tool("set_setting","Set application preference","Persist one preference. Values are validated by the shared service; get_settings shows current keys/types. Changes apply to desktop and all entry points.",object(json!({"key":string("Known settings key from get_settings"),"value":{}}),&["key","value"]),false,true,true),
    ]
}
pub fn action(name: &str) -> Option<(&'static str, Option<&'static str>)> {
    Some(match name {
        "preview_recurrence" => ("recurrence.preview", None),
        "duplicate_task" => ("tasks.duplicate", None),
        "dismiss_task" => ("tasks.wont_do", None),
        "set_task_tags" => ("tasks.tags", None),
        "list_tags" => ("tags.list", None),
        "create_tag" => ("tags.create", None),
        "update_tag" => ("tags.update", None),
        "delete_tag" => ("tags.delete", None),
        "link_tasks" => ("links.add", None),
        "unlink_tasks" => ("links.remove", None),
        "add_attachment" => ("attachments.add", None),
        "import_attachment" => ("attachments.import", None),
        "read_attachment_chunk" => ("attachments.read_chunk", None),
        "remove_attachment" => ("attachments.remove", None),
        "duplicate_list" => ("lists.duplicate", None),
        "archive_list" => ("lists.archive", None),
        "reorder_lists" => ("lists.reorder", None),
        "reorder_folders" => ("folders.reorder", None),
        "list_sections" => ("snapshot", Some("sections")),
        "create_section" => ("sections.create", None),
        "update_section" => ("sections.update", None),
        "delete_section" => ("sections.delete", None),
        "reorder_sections" => ("sections.reorder", None),
        "list_templates" => ("templates.list", None),
        "get_template" => ("templates.get", None),
        "list_comments" => ("comments.list", None),
        "add_comment" => ("comments.add", None),
        "update_comment" => ("comments.update", None),
        "delete_comment" => ("comments.delete", None),
        "create_template" => ("templates.create", None),
        "update_template" => ("templates.update", None),
        "delete_template" => ("templates.delete", None),
        "apply_template" => ("templates.apply", None),
        "get_settings" => ("settings.get", None),
        "set_setting" => ("settings.set", None),
        _ => return None,
    })
}
