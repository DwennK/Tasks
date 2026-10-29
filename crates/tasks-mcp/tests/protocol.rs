use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Exercise the real stdio process, including framing and clean process teardown.
struct Client {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    reader: Option<JoinHandle<()>>,
    next_id: u64,
}

impl Client {
    fn start(db: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_tasks-mcp"))
            .arg("--db")
            .arg(db)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start MCP process");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, lines) = mpsc::channel();
        let reader = thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(line) => {
                        if sender.send(line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            child,
            stdin: Some(stdin),
            lines,
            reader: Some(reader),
            next_id: 1,
        }
    }

    fn raw(&mut self, line: &str) {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
    }

    fn receive(&self) -> Value {
        let line = self
            .lines
            .recv_timeout(Duration::from_secs(10))
            .expect("MCP must respond within ten seconds");
        serde_json::from_str(&line)
            .expect("stdout contains only a complete JSON-RPC message per line")
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.raw(
            &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string(),
        );
        let response = self.receive();
        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], id, "notifications must not emit responses");
        response
    }

    fn initialize(&mut self) {
        let response = self.request(
            "initialize",
            json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "tasks-protocol-tests", "version": "1.0.0"}
            }),
        );
        assert_eq!(response["result"]["protocolVersion"], "2025-11-25");
        assert!(response["result"]["serverInfo"]["name"].is_string());
        assert!(response["result"]["capabilities"]["tools"].is_object());
        self.raw(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}).to_string());
        let ping = self.request("ping", json!({}));
        assert_eq!(ping["result"], json!({}));
    }

    fn call(&mut self, name: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({"name": name, "arguments": arguments}))
    }

    fn successful_call(&mut self, name: &str, arguments: Value) -> Value {
        let response = self.call(name, arguments);
        assert!(response.get("error").is_none(), "{response}");
        assert_ne!(response["result"]["isError"], true, "{response}");
        assert_eq!(response["result"]["content"][0]["type"], "text");
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
            .expect("tool text is parseable serialized service data")
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

#[test]
fn real_process_task_lifecycle_and_restart_persistence() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("tasks.sqlite3");
    let mut client = Client::start(&db);
    client.initialize();

    let task = client.successful_call(
        "create_task",
        json!({"title":"MCP integration task","notes":"first line\nsecond line","priority":3}),
    );
    let id = task["id"].as_str().unwrap().to_owned();
    assert_eq!(task["list_id"], "inbox");
    assert_eq!(task["notes"], "first line\nsecond line");

    let updated = client.successful_call("update_task", json!({"id":id,"title":"Updated locally","due_date":"2026-11-03","expected_version":task["version"]}));
    assert_eq!(updated["title"], "Updated locally");
    assert_eq!(updated["due_date"], "2026-11-03");
    assert!(updated["version"].as_i64().unwrap() > task["version"].as_i64().unwrap());

    let listed = client.successful_call("list_tasks", json!({"search":"Updated locally"}));
    assert!(listed
        .as_array()
        .unwrap()
        .iter()
        .any(|task| task["id"] == id));

    let completed = client.successful_call("complete_task", json!({"id":id}));
    assert!(completed["completed_at"].is_string());
    let reopened = client.successful_call("complete_task", json!({"id":id,"completed":false}));
    assert!(reopened["completed_at"].is_null());
    let deleted = client.successful_call("delete_task", json!({"id":id}));
    assert!(deleted["deleted_at"].is_string());
    let trashed = client.successful_call("list_tasks", json!({"view":"trash"}));
    assert!(trashed
        .as_array()
        .unwrap()
        .iter()
        .any(|task| task["id"] == id));
    let restored = client.successful_call("restore_task", json!({"id":id}));
    assert!(restored["deleted_at"].is_null());

    drop(client);
    let mut restarted = Client::start(&db);
    restarted.initialize();
    let persisted = restarted.successful_call("get_task", json!({"id":id}));
    assert_eq!(persisted["title"], "Updated locally");
    assert_eq!(persisted["notes"], "first line\nsecond line");
    assert!(persisted["deleted_at"].is_null());
}

#[test]
fn tool_discovery_exposes_schemas_and_local_safety_annotations() {
    let dir = tempfile::tempdir().unwrap();
    let mut client = Client::start(&dir.path().join("tasks.sqlite3"));
    client.initialize();
    let response = client.request("tools/list", json!({}));
    let tools = response["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 51);
    let mut names = std::collections::HashSet::new();
    for tool in tools {
        assert!(names.insert(tool["name"].as_str().unwrap()));
        assert!(tool["inputSchema"].is_object());
        assert!(tool["annotations"]["readOnlyHint"].is_boolean());
        assert_eq!(tool["annotations"]["openWorldHint"], false);
    }
    let create = tools.iter().find(|t| t["name"] == "create_task").unwrap();
    assert_eq!(
        create["inputSchema"]["properties"]["title"]["maxLength"],
        500
    );
    for name in [
        "list_tasks",
        "get_task",
        "create_task",
        "update_task",
        "complete_task",
        "delete_task",
        "restore_task",
        "list_lists",
        "list_folders",
        "create_list",
        "create_folder",
    ] {
        let tool = tools
            .iter()
            .find(|tool| tool["name"] == name)
            .unwrap_or_else(|| panic!("missing {name}"));
        assert_eq!(tool["inputSchema"]["type"], "object", "{name}");
        assert!(tool["description"]
            .as_str()
            .is_some_and(|text| !text.is_empty()));
        assert_eq!(
            tool["annotations"]["openWorldHint"], false,
            "local tools must not indicate network access: {name}"
        );
        assert_eq!(
            tool["annotations"]["readOnlyHint"],
            name.starts_with("list_") || name == "get_task",
            "{name}"
        );
    }
}

#[test]
fn malformed_requests_recover_and_unknown_operations_use_protocol_errors() {
    let dir = tempfile::tempdir().unwrap();
    let mut client = Client::start(&dir.path().join("tasks.sqlite3"));
    client.initialize();
    client.raw("this is not json");
    let malformed = client.receive();
    assert_eq!(malformed["error"]["code"], -32700);
    assert!(malformed["id"].is_null());

    for invalid in [
        json!({"jsonrpc":"2.0","id":true,"method":"ping"}),
        json!({"jsonrpc":"2.0","id":1.25,"method":"ping"}),
        json!({"jsonrpc":"2.0","id":3}),
    ] {
        client.raw(&invalid.to_string());
        assert_eq!(client.receive()["error"]["code"], -32600);
    }
    let unknown = client.request("not/a/method", json!({}));
    assert_eq!(unknown["error"]["code"], -32601);
    let unknown_tool = client.call("not_a_tool", json!({}));
    assert_eq!(unknown_tool["error"]["code"], -32602);
    client.raw(
        &json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":999,"reason":"already finished"}}).to_string(),
    );
    client.raw(&json!({"jsonrpc":"2.0","id":"unicode-é-✅","method":"ping"}).to_string());
    let string_id = client.receive();
    assert_eq!(string_id["id"], "unicode-é-✅");
    assert_eq!(string_id["result"], json!({}));
    assert_eq!(client.request("ping", json!({}))["result"], json!({}));
}

#[test]
fn service_errors_do_not_break_protocol_or_commit_invalid_changes() {
    let dir = tempfile::tempdir().unwrap();
    let mut client = Client::start(&dir.path().join("tasks.sqlite3"));
    client.initialize();
    let invalid = client.call("create_task", json!({"title":"   "}));
    assert_eq!(invalid["result"]["isError"], true, "{invalid}");
    assert_eq!(client.successful_call("list_tasks", json!({})), json!([]));
    let task = client.successful_call("create_task", json!({"title":"Valid task"}));
    let updated = client.successful_call(
        "update_task",
        json!({"id":task["id"],"title":"Newer edit","expected_version":task["version"]}),
    );
    let stale = client.call(
        "update_task",
        json!({"id":task["id"],"title":"Stale edit","expected_version":task["version"]}),
    );
    assert_eq!(stale["result"]["isError"], true, "{stale}");
    let current = client.successful_call("get_task", json!({"id":task["id"]}));
    assert_eq!(current["title"], updated["title"]);
}

#[test]
fn folder_and_list_tools_share_task_storage() {
    let dir = tempfile::tempdir().unwrap();
    let mut client = Client::start(&dir.path().join("tasks.sqlite3"));
    client.initialize();
    let folder = client.successful_call("create_folder", json!({"name":"Work"}));
    let child = client.successful_call(
        "create_folder",
        json!({"name":"Clients","parent_id":folder["id"]}),
    );
    assert_eq!(child["parent_id"], folder["id"]);
    let list = client.successful_call(
        "create_list",
        json!({"name":"Website","folder_id":child["id"],"color":"#4567ab"}),
    );
    let task = client.successful_call(
        "create_task",
        json!({"title":"Review site","list_id":list["id"]}),
    );
    assert_eq!(task["list_id"], list["id"]);
    let lists = client.successful_call("list_lists", json!({}));
    assert!(lists
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["id"] == list["id"]));
    let folders = client.successful_call("list_folders", json!({}));
    assert_eq!(folders.as_array().unwrap().len(), 2);
}

#[test]
fn tools_require_completed_initialization_before_access() {
    let dir = tempfile::tempdir().unwrap();
    let mut client = Client::start(&dir.path().join("tasks.sqlite3"));
    assert_eq!(client.request("ping", json!({}))["result"], json!({}));
    let early = client.call("create_task", json!({"title":"Must not be created"}));
    assert_eq!(early["error"]["code"], -32002);
    let initialized = client.request(
        "initialize",
        json!({
            "protocolVersion":"2025-11-25", "capabilities":{},
            "clientInfo":{"name":"lifecycle-test","version":"1.0"}
        }),
    );
    assert!(initialized.get("result").is_some());
    let premature = client.request("tools/list", json!({}));
    assert_eq!(premature["error"]["code"], -32002);
    client.raw(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}).to_string());
    assert_eq!(client.successful_call("list_tasks", json!({})), json!([]));
}

#[test]
fn oversized_frame_is_rejected_and_following_messages_still_work() {
    let dir = tempfile::tempdir().unwrap();
    let mut client = Client::start(&dir.path().join("tasks.sqlite3"));
    client.initialize();
    let oversized = json!({
        "jsonrpc":"2.0", "id":1000, "method":"tools/call",
        "params":{"name":"create_task","arguments":{"title":"Must not be created","notes":"x".repeat(1_048_576)}}
    });
    client.raw(&oversized.to_string());
    let error = client.receive();
    assert_eq!(error["error"]["code"], -32700);
    assert!(error["id"].is_null());
    assert_eq!(client.request("ping", json!({}))["result"], json!({}));
    assert_eq!(client.successful_call("list_tasks", json!({})), json!([]));
}

#[test]
fn server_exits_successfully_when_client_closes_stdin() {
    let dir = tempfile::tempdir().unwrap();
    let mut client = Client::start(&dir.path().join("tasks.sqlite3"));
    client.initialize();
    client.stdin.take();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = client.child.try_wait().expect("read child exit status") {
            assert!(
                status.success(),
                "stdio EOF must be a clean shutdown: {status}"
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "MCP child must exit after stdio closes"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn extended_task_fields_recurrence_and_templates_are_shared() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("extended.sqlite3");
    let mut client = Client::start(&db);
    client.initialize();
    let list = client.successful_call(
        "create_list",
        json!({"name":"Launch","view_mode":"board","group_by":"section","sort_by":"priority"}),
    );
    let section = client.successful_call(
        "create_section",
        json!({"list_id":list["id"],"name":"Review"}),
    );
    let tag = client.successful_call("create_tag", json!({"name":"important","color":"#5684ff"}));
    let task=client.successful_call("create_task",json!({
        "title":"Review each day","list_id":list["id"],"section_id":section["id"],"tag_ids":[tag["id"]],
        "due_date":"2026-10-28","due_time":"09:00","start_date":"2026-10-28","start_time":"08:30","duration_minutes":30,
        "timezone":"Europe/Zurich","recurrence":{"frequency":"daily","count":3},"pinned":true,
        "reminders":["2026-10-28T07:30:00Z","2026-10-28T07:55:00Z"],"notes_format":"markdown","notes":"## Review",
        "checklist":[{"id":"check-one","text":"Verify build","completed":false}]
    }));
    assert_eq!(task["duration_minutes"], 30);
    assert_eq!(task["tag_ids"], json!([tag["id"]]));
    assert_eq!(task["reminders"].as_array().unwrap().len(), 2);
    let template = client.successful_call(
        "create_template",
        json!({"name":"Daily review","task_id":task["id"]}),
    );
    let applied = client.successful_call(
        "apply_template",
        json!({"id":template["id"],"list_id":"inbox"}),
    );
    assert_ne!(applied["id"], task["id"]);
    assert_eq!(applied["list_id"], "inbox");
    let completed = client.successful_call("complete_task", json!({"id":task["id"]}));
    assert!(completed["recurrence_next_id"].is_string());
    let next = client.successful_call("get_task", json!({"id":completed["recurrence_next_id"]}));
    assert_eq!(next["due_date"], "2026-10-29");
    assert!(next["completed_at"].is_null());
    let duplicate = client.successful_call(
        "duplicate_task",
        json!({"id":task["id"],"title":"One-off review"}),
    );
    assert!(duplicate["completed_at"].is_null());
    assert_eq!(duplicate["title"], "One-off review");
    let dismissed = client.successful_call("dismiss_task", json!({"id":duplicate["id"]}));
    assert!(dismissed["wont_do_at"].is_string());
    let active = client.successful_call(
        "dismiss_task",
        json!({"id":duplicate["id"],"wont_do":false}),
    );
    assert!(active["wont_do_at"].is_null());
    client.successful_call("archive_list", json!({"id":list["id"]}));
    let active = client.successful_call("list_tasks", json!({}));
    assert!(active
        .as_array()
        .unwrap()
        .iter()
        .all(|t| t["list_id"] != list["id"]));
    client.successful_call("archive_list", json!({"id":list["id"],"archived":false}));
    client.successful_call("set_setting", json!({"key":"theme_color","value":"teal"}));
    assert_eq!(
        client.successful_call("get_settings", json!({}))["theme_color"],
        "teal"
    );
}

#[test]
fn attachments_use_bounded_frames_and_explicit_file_import() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("attachments.sqlite3");
    let mut client = Client::start(&db);
    client.initialize();
    let first = client.successful_call("create_task", json!({"title":"Attachments"}));
    let second = client.successful_call("create_task", json!({"title":"Related"}));
    client.successful_call(
        "link_tasks",
        json!({"id":first["id"],"linked_task_id":second["id"]}),
    );
    let linked = client.successful_call("get_task", json!({"id":second["id"]}));
    assert!(linked["linked_task_ids"]
        .as_array()
        .unwrap()
        .contains(&first["id"]));
    let attachment=client.successful_call("add_attachment",json!({"id":first["id"],"name":"hello.txt","mime_type":"text/plain","content_base64":"aGVsbG8="}));
    let chunk = client.successful_call(
        "read_attachment_chunk",
        json!({"id":attachment["id"],"offset":0,"length":3}),
    );
    assert_eq!(chunk["content_base64"], "aGVs");
    assert_eq!(chunk["next_offset"], 3);
    let final_chunk = client.successful_call(
        "read_attachment_chunk",
        json!({"id":attachment["id"],"offset":3}),
    );
    assert_eq!(final_chunk["content_base64"], "bG8=");
    assert!(final_chunk["next_offset"].is_null());
    let large = dir.path().join("large.bin");
    std::fs::write(&large, vec![42u8; 2 * 1024 * 1024]).unwrap();
    let imported = client.successful_call(
        "import_attachment",
        json!({"task_id":first["id"],"path":large.to_str().unwrap()}),
    );
    assert_eq!(imported["size"], 2 * 1024 * 1024);
    let chunk = client.successful_call("read_attachment_chunk", json!({"id":imported["id"]}));
    assert_eq!(chunk["next_offset"], 262144);
    assert_eq!(chunk["content_base64"].as_str().unwrap().len(), 349528);
    let oversized = client.call(
        "add_attachment",
        json!({"id":first["id"],"name":"too-large.bin","content_base64":"A".repeat(699052)}),
    );
    assert_eq!(oversized["result"]["isError"], true);
    let invalid = client.call(
        "add_attachment",
        json!({"id":first["id"],"name":"invalid.bin","content_base64":"="}),
    );
    assert_eq!(invalid["result"]["isError"], true);
    let oversized_read = client.call(
        "read_attachment_chunk",
        json!({"id":attachment["id"],"length":262145}),
    );
    assert_eq!(oversized_read["result"]["isError"], true);
    client.successful_call("remove_attachment", json!({"id":attachment["id"]}));
    client.successful_call(
        "unlink_tasks",
        json!({"id":first["id"],"linked_task_id":second["id"]}),
    );
}

#[test]
fn local_comments_and_attachment_associations_are_editable() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("comments.sqlite3");
    let mut client = Client::start(&db);
    client.initialize();
    let task = client.successful_call("create_task", json!({"title":"Comment review"}));
    let attachment = client.successful_call(
        "add_attachment",
        json!({"id":task["id"],"name":"note.txt","content_base64":"aGVsbG8="}),
    );
    let comment = client.successful_call(
        "add_comment",
        json!({"id":task["id"],"body":"Review attached note","attachment_ids":[attachment["id"]]}),
    );
    assert_eq!(comment["author"], "You");
    assert_eq!(comment["attachment_ids"], json!([attachment["id"]]));
    let changed = client.successful_call(
        "update_comment",
        json!({"id":comment["id"],"body":"Reviewed"}),
    );
    assert_eq!(changed["body"], "Reviewed");
    assert_eq!(changed["attachment_ids"], json!([attachment["id"]]));
    let detached = client.successful_call(
        "update_comment",
        json!({"id":comment["id"],"attachment_ids":[]}),
    );
    assert_eq!(detached["body"], "Reviewed");
    assert!(detached["attachment_ids"].as_array().unwrap().is_empty());
    assert_eq!(
        client
            .successful_call("list_comments", json!({"id":task["id"]}))
            .as_array()
            .unwrap()
            .len(),
        1
    );
    client.successful_call("delete_comment", json!({"id":comment["id"]}));
    assert!(client
        .successful_call("list_comments", json!({"id":task["id"]}))
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn tag_and_comment_tools_guard_against_stale_writes() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("versions.sqlite3");
    let mut client = Client::start(&db);
    client.initialize();
    let task = client.successful_call("create_task", json!({"title":"Concurrent edits"}));
    let tag = client.successful_call("create_tag", json!({"name":"review"}));
    client.successful_call(
        "set_task_tags",
        json!({"id":task["id"],"tag_ids":[tag["id"]],"expected_version":task["version"]}),
    );
    let stale = client.call(
        "set_task_tags",
        json!({"id":task["id"],"tag_ids":[],"expected_version":task["version"]}),
    );
    assert_eq!(stale["result"]["isError"], true);
    assert_eq!(
        client.successful_call("get_task", json!({"id":task["id"]}))["tag_ids"],
        json!([tag["id"]])
    );
    let comment = client.successful_call("add_comment", json!({"id":task["id"],"body":"Original"}));
    assert!(comment["version"].is_u64());
    let edited = client.successful_call(
        "update_comment",
        json!({"id":comment["id"],"body":"Newer edit","expected_version":comment["version"]}),
    );
    let stale_edit = client.call(
        "update_comment",
        json!({"id":comment["id"],"body":"Stale edit","expected_version":comment["version"]}),
    );
    assert_eq!(stale_edit["result"]["isError"], true);
    let stale_delete = client.call(
        "delete_comment",
        json!({"id":comment["id"],"expected_version":comment["version"]}),
    );
    assert_eq!(stale_delete["result"]["isError"], true);
    let comments = client.successful_call("list_comments", json!({"id":task["id"]}));
    assert_eq!(comments[0]["body"], "Newer edit");
    assert_eq!(comments[0]["version"], edited["version"]);
    client.successful_call(
        "delete_comment",
        json!({"id":comment["id"],"expected_version":edited["version"]}),
    );
    assert!(client
        .successful_call("list_comments", json!({"id":task["id"]}))
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn list_metadata_note_defaults_duplication_and_rendered_search_are_exposed() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("notes.sqlite3");
    let mut client = Client::start(&db);
    client.initialize();
    let folder = client.successful_call(
        "create_folder",
        json!({"name":"Work","icon":"briefcase","pinned":true}),
    );
    assert_eq!(folder["pinned"], true);
    let changed = client.successful_call(
        "update_folder",
        json!({"id":folder["id"],"icon":null,"pinned":false}),
    );
    assert_eq!(changed["icon"], "");
    let list=client.successful_call("create_list",json!({"name":"Notes","folder_id":folder["id"],"icon":"notebook","pinned":true,"show_in_smart_lists":false,"list_type":"note","color":""}));
    assert_eq!(list["color"], "");
    assert_eq!(list["list_type"], "note");
    let section = client.successful_call(
        "create_section",
        json!({"list_id":list["id"],"name":"Ideas"}),
    );
    let source = "<p>Design &amp; <strong>planning</strong></p><p>Second line</p>";
    let note=client.successful_call("create_task",json!({"title":"Hidden note","list_id":list["id"],"section_id":section["id"],"notes_format":"html","notes":source}));
    assert_eq!(note["is_note"], true);
    assert_eq!(note["notes"], source);
    assert!(note["notes_text"]
        .as_str()
        .unwrap()
        .contains("Design & planning"));
    assert!(client
        .successful_call("list_tasks", json!({}))
        .as_array()
        .unwrap()
        .is_empty());
    let found = client.successful_call(
        "list_tasks",
        json!({"search":"Design & planning Second line"}),
    );
    assert_eq!(found.as_array().unwrap().len(), 1);
    assert_eq!(found[0]["id"], note["id"]);
    assert!(client
        .successful_call("list_tasks", json!({"search":"strong"}))
        .as_array()
        .unwrap()
        .is_empty());
    let task = client.successful_call(
        "create_task",
        json!({"title":"Completed task","list_id":list["id"],"is_note":false}),
    );
    assert_eq!(task["is_note"], false);
    client.successful_call("complete_task", json!({"id":task["id"]}));
    let copied = client.successful_call(
        "duplicate_list",
        json!({"id":list["id"],"name":"Copy","folder_id":null}),
    );
    assert_eq!(copied["pinned"], false);
    assert_eq!(copied["icon"], "notebook");
    assert_eq!(copied["list_type"], "note");
    assert!(copied["folder_id"].is_null());
    let notes = client.successful_call("list_tasks", json!({"list_id":copied["id"]}));
    assert_eq!(notes.as_array().unwrap().len(), 1);
    assert_ne!(notes[0]["id"], note["id"]);
    assert_ne!(notes[0]["section_id"], section["id"]);
    assert_eq!(notes[0]["notes_text"], note["notes_text"]);
    assert_eq!(
        client
            .successful_call(
                "list_tasks",
                json!({"list_id":copied["id"],"view":"completed"})
            )
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let changed=client.successful_call("update_list",json!({"id":copied["id"],"icon":null,"show_in_smart_lists":true,"list_type":"task","color":""}));
    assert_eq!(changed["icon"], "");
    assert_eq!(
        client.successful_call("get_task", json!({"id":notes[0]["id"]}))["is_note"],
        true
    );
    let converted =
        client.successful_call("update_task", json!({"id":notes[0]["id"],"is_note":false}));
    assert_eq!(converted["is_note"], false);
    assert_eq!(converted["notes"], source);
    let invalid = client.call(
        "update_task",
        json!({"id":note["id"],"notes_text":"spoofed"}),
    );
    assert_eq!(invalid["result"]["isError"], true);
}

#[test]
fn custom_calendar_recurrence_patterns_pass_through_real_mcp() {
    let directory = tempfile::tempdir().unwrap();
    let mut client = Client::start(&directory.path().join("calendar.sqlite3"));
    client.initialize();
    let cases = [
        (
            "Month dates",
            "2026-01-01",
            json!({"frequency":"monthly","month_days":[1,15,-1],"skip_weekends":true}),
            "2026-01-15",
        ),
        (
            "Month weekday",
            "2026-01-05",
            json!({"frequency":"monthly","ordinal_weekday":{"ordinal":1,"weekday":1}}),
            "2026-02-02",
        ),
        (
            "Last workday",
            "2026-01-30",
            json!({"frequency":"monthly","workday":"last"}),
            "2026-02-27",
        ),
        (
            "Leap year",
            "2024-02-29",
            json!({"frequency":"yearly","year_month":2,"month_days":[29]}),
            "2028-02-29",
        ),
    ];
    for (title, due, recurrence, next_due) in cases {
        let task = client.successful_call(
            "create_task",
            json!({"title":title,"due_date":due,"recurrence":recurrence}),
        );
        let done = client.successful_call("complete_task", json!({"id":task["id"]}));
        let next = client.successful_call("get_task", json!({"id":done["recurrence_next_id"]}));
        assert_eq!(next["due_date"], next_due, "{title}");
    }
    let invalid = client.call("create_task", json!({"title":"Invalid mix","due_date":"2026-01-01","recurrence":{"frequency":"monthly","month_days":[1],"workday":"first"}}));
    assert_eq!(invalid["result"]["isError"], true);
}

#[test]
fn recurrence_preview_tool_uses_shared_pure_rules_without_revision_changes() {
    let directory = tempfile::tempdir().unwrap();
    let db = directory.path().join("preview.sqlite3");
    let mut client = Client::start(&db);
    client.initialize();
    let service = tasks_core::TaskService::open(&db).unwrap();
    let revision = service.execute("revision", json!({})).unwrap();
    let preview = client.successful_call("preview_recurrence", json!({"due_date":"2026-01-01","recurrence":{"frequency":"monthly","month_days":[1,15,-1]},"range_start":"2026-01-01","range_end":"2026-01-31","limit":1}));
    assert_eq!(preview["dates"], json!(["2026-01-15"]));
    assert_eq!(preview["truncated"], true);
    assert_eq!(preview["assumption"], "due_date");
    let invalid = client.call("preview_recurrence", json!({"due_date":"2026-01-01","recurrence":null,"range_start":"2026-01-01","range_end":"2028-01-01"}));
    assert_eq!(invalid["result"]["isError"], true);
    assert_eq!(service.execute("revision", json!({})).unwrap(), revision);
}

#[test]
fn global_window_shortcut_persists_and_conflicts_are_atomic_tool_errors() {
    let directory = tempfile::tempdir().unwrap();
    let db = directory.path().join("global-shortcuts.sqlite3");
    let show = "global_show_hide_shortcut";
    let quick = "global_quick_add_shortcut";
    let mut client = Client::start(&db);
    client.initialize();
    let defaults = client.successful_call("get_settings", json!({}));
    assert_eq!(defaults[show], "");
    assert_eq!(defaults[quick], "");
    client.successful_call("set_setting", json!({"key":show,"value":"mod+shift+e"}));
    client.successful_call("set_setting", json!({"key":quick,"value":"ctrl+shift+a"}));
    drop(client);

    let mut client = Client::start(&db);
    client.initialize();
    let reopened = client.successful_call("get_settings", json!({}));
    assert_eq!(reopened[show], "mod+shift+e");
    assert_eq!(reopened[quick], "ctrl+shift+a");
    let service = tasks_core::TaskService::open(&db).unwrap();
    let before = service.execute("snapshot", json!({})).unwrap();
    for key in [show, quick] {
        for invalid in [
            json!("a"),
            json!("shift+a"),
            json!("g t"),
            json!("mod+q"),
            json!("ctrl+ctrl+a"),
            json!("meta+shift+e"),
            json!(true),
        ] {
            let result = client.call("set_setting", json!({"key":key,"value":invalid}));
            assert!(result.get("error").is_none(), "{result}");
            assert_eq!(result["result"]["isError"], true, "{key}: {invalid}");
            assert_eq!(service.execute("snapshot", json!({})).unwrap(), before);
        }
    }
    for (key, collision) in [(show, "ctrl+shift+a"), (quick, "mod+shift+e")] {
        let result = client.call("set_setting", json!({"key":key,"value":collision}));
        assert!(result.get("error").is_none(), "{result}");
        assert_eq!(result["result"]["isError"], true);
        assert_eq!(service.execute("snapshot", json!({})).unwrap(), before);
    }
    client.successful_call("set_setting", json!({"key":show,"value":""}));
    client.successful_call("set_setting", json!({"key":quick,"value":"mod+shift+e"}));
    client.successful_call("set_setting", json!({"key":show,"value":"ctrl+shift+a"}));
    let swapped = client.successful_call("get_settings", json!({}));
    assert_eq!(swapped[show], "ctrl+shift+a");
    assert_eq!(swapped[quick], "mod+shift+e");
}
