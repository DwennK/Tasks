use serde_json::{json, Value};
use std::{
    path::Path,
    process::{Command, Output},
};
fn run(db: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tasks"))
        .arg("--db")
        .arg(db)
        .arg("--json")
        .args(args)
        .output()
        .unwrap()
}
fn success(db: &Path, args: &[&str]) -> Value {
    let output = run(db, args);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
#[test]
fn task_lifecycle_persists_between_processes() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("tasks.sqlite3");
    let task = success(
        &db,
        &[
            "add",
            "Prepare release",
            "--notes",
            "Check signing",
            "--priority",
            "3",
            "--due",
            "2026-10-28",
        ],
    );
    let id = task["id"].as_str().unwrap();
    let edited = success(&db, &["edit", id, "--title", "Ship release", "--clear-due"]);
    assert_eq!(edited["title"], "Ship release");
    assert!(edited["due_date"].is_null());
    let list = success(&db, &["list", "--search", "Ship release"]);
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert!(!success(&db, &["complete", id])["completed_at"].is_null());
    assert!(success(&db, &["list"]).as_array().unwrap().is_empty());
    assert!(success(&db, &["reopen", id])["completed_at"].is_null());
    assert!(!success(&db, &["delete", id])["deleted_at"].is_null());
    assert_eq!(
        success(&db, &["list", "--view", "trash"])
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(success(&db, &["restore", id])["deleted_at"].is_null());
    assert_eq!(success(&db, &["get", id])["notes"], "Check signing");
}
#[test]
fn generic_call_and_lists_share_one_database() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("tasks.sqlite3");
    let folder = success(&db, &["folders", "add", "Work"]);
    let folder_id = folder["id"].as_str().unwrap();
    let list = success(&db, &["lists", "add", "Release", "--folder", folder_id]);
    let list_id = list["id"].as_str().unwrap();
    let input = json!({"title":"Review build","list_id":list_id}).to_string();
    let task = success(&db, &["call", "tasks.create", &input]);
    success(&db, &["lists", "delete", list_id]);
    assert_eq!(
        success(&db, &["get", task["id"].as_str().unwrap()])["list_id"],
        "inbox"
    );
    let folders = success(&db, &["folders"]);
    assert_eq!(folders.as_array().unwrap().len(), 1);
}
#[test]
fn errors_have_nonzero_status_and_no_success_json() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("tasks.sqlite3");
    for args in [
        &["add", ""][..],
        &["get", "missing"][..],
        &["call", "tasks.create", "not-json"][..],
        &["add", "Task", "--priority", "4"][..],
    ] {
        let output = run(&db, args);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}

#[test]
fn advanced_cli_uses_shared_task_extensions() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("extended.sqlite3");
    let list = success(
        &db,
        &[
            "lists",
            "add",
            "Release",
            "--view-mode",
            "board",
            "--group-by",
            "section",
        ],
    );
    let list_id = list["id"].as_str().unwrap();
    let section = success(&db, &["sections", "add", list_id, "Review"]);
    let section_id = section["id"].as_str().unwrap();
    let tag = success(&db, &["tags", "add", "important"]);
    let tag_id = tag["id"].as_str().unwrap();
    let task = success(
        &db,
        &[
            "add",
            "Recurring review",
            "--list",
            list_id,
            "--section",
            section_id,
            "--tags",
            tag_id,
            "--due",
            "2026-10-28",
            "--time",
            "09:00",
            "--timezone",
            "Europe/Zurich",
            "--duration",
            "30",
            "--repeat",
            r#"{"frequency":"daily","count":2}"#,
            "--pinned",
            "true",
        ],
    );
    let id = task["id"].as_str().unwrap();
    assert_eq!(task["pinned"], true);
    assert_eq!(task["duration_minutes"], 30);
    let template = success(&db, &["templates", "add", "Daily review", id]);
    let applied = success(
        &db,
        &[
            "templates",
            "apply",
            template["id"].as_str().unwrap(),
            "--list",
            "inbox",
        ],
    );
    assert_eq!(applied["list_id"], "inbox");
    let completed = success(&db, &["complete", id]);
    assert!(completed["recurrence_next_id"].is_string());
    let next_id = completed["recurrence_next_id"].as_str().unwrap();
    let edited = success(
        &db,
        &[
            "edit",
            next_id,
            "--fields",
            r#"{"recurrence":null,"duration_minutes":null}"#,
        ],
    );
    assert!(edited["recurrence"].is_null());
    let dismissed = success(&db, &["wont-do", next_id]);
    assert!(dismissed["wont_do_at"].is_string());
    assert_eq!(
        success(&db, &["list", "--view", "wont-do"])
            .as_array()
            .unwrap()
            .len(),
        1
    );
    success(&db, &["wont-do", next_id, "--undo"]);
    let file = dir.path().join("note.txt");
    std::fs::write(&file, b"hello").unwrap();
    let attachment = success(
        &db,
        &["attachments", "import", next_id, file.to_str().unwrap()],
    );
    assert_eq!(
        success(
            &db,
            &["attachments", "read", attachment["id"].as_str().unwrap()]
        )["content_base64"],
        "aGVsbG8="
    );
    success(&db, &["settings", "set", "theme_color", r#""teal""#]);
    assert_eq!(success(&db, &["settings"])["theme_color"], "teal");
}

fn stdin_call(db: &Path, alias: &str, action: &str, input: &[u8]) -> Output {
    use std::{io::Write, process::Stdio};
    let mut child = Command::new(env!("CARGO_BIN_EXE_tasks"))
        .arg("--db")
        .arg(db)
        .args(["--json", alias, action, "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let write_result = child.stdin.take().unwrap().write_all(input);
    // An oversized stream may be rejected before its writer reaches EOF.
    if let Err(error) = write_result {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
    }
    child.wait_with_output().unwrap()
}

#[test]
fn raw_and_call_read_large_json_from_stdin() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("stdin.sqlite3");
    let task = success(&db, &["add", "Large attachment"]);
    // 800 KiB base64 decodes to600 KiB, beyond typical macOS argv limits.
    let input = json!({"id":task["id"],"name":"large.bin","content_base64":"A".repeat(800*1024)})
        .to_string();
    assert!(input.len() > 512 * 1024);
    let output = stdin_call(&db, "call", "attachments.add", input.as_bytes());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let attachment: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(attachment["size"], 600 * 1024);
    let get = json!({"id":task["id"]}).to_string();
    let output = stdin_call(&db, "raw", "tasks.get", get.as_bytes());
    assert!(output.status.success());
    let persisted: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(persisted["attachments"][0]["id"], attachment["id"]);
}

#[test]
fn stdin_json_limit_and_invalid_input_are_rejected_before_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("stdin-limit.sqlite3");
    let before = success(&db, &["snapshot"]);
    let output = stdin_call(
        &db,
        "call",
        "tasks.create",
        &vec![b' '; 32 * 1024 * 1024 + 1],
    );
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("32 MiB limit"));
    for input in [b"not-json".as_slice(), b"[]".as_slice(), b"\xff".as_slice()] {
        let output = stdin_call(&db, "raw", "tasks.create", input);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
    assert_eq!(success(&db, &["snapshot"])["revision"], before["revision"]);
}

#[test]
fn dedicated_tag_and_comment_commands_reject_stale_versions() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("versions.sqlite3");
    let task = success(&db, &["add", "Concurrent edits"]);
    let task_id = task["id"].as_str().unwrap();
    let version = task["version"].to_string();
    let tag = success(&db, &["tags", "add", "review"]);
    let tag_id = tag["id"].as_str().unwrap();
    success(
        &db,
        &[
            "tags",
            "set",
            task_id,
            tag_id,
            "--expected-version",
            &version,
        ],
    );
    let stale = run(
        &db,
        &["tags", "set", task_id, "--expected-version", &version],
    );
    assert!(!stale.status.success());
    assert!(stale.stdout.is_empty());
    assert_eq!(success(&db, &["get", task_id])["tag_ids"], json!([tag_id]));
    let comment = success(&db, &["comments", "add", task_id, "Original"]);
    let comment_id = comment["id"].as_str().unwrap();
    let version = comment["version"].to_string();
    let edited = success(
        &db,
        &[
            "comments",
            "edit",
            comment_id,
            "--body",
            "Newer edit",
            "--expected-version",
            &version,
        ],
    );
    for args in [
        vec![
            "comments",
            "edit",
            comment_id,
            "--body",
            "Stale edit",
            "--expected-version",
            &version,
        ],
        vec![
            "comments",
            "delete",
            comment_id,
            "--expected-version",
            &version,
        ],
    ] {
        let output = run(&db, &args);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
    let comments = success(&db, &["comments", "list", task_id]);
    assert_eq!(comments[0]["body"], "Newer edit");
    assert_eq!(comments[0]["version"], edited["version"]);
    success(
        &db,
        &[
            "comments",
            "delete",
            comment_id,
            "--expected-version",
            &edited["version"].to_string(),
        ],
    );
    assert!(success(&db, &["comments", "list", task_id])
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn note_lists_metadata_duplication_and_rendered_search_share_service() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("notes.sqlite3");
    let folder = success(
        &db,
        &[
            "folders",
            "add",
            "Work",
            "--icon",
            "briefcase",
            "--pinned",
            "true",
        ],
    );
    let folder_id = folder["id"].as_str().unwrap();
    assert_eq!(folder["icon"], "briefcase");
    assert_eq!(folder["pinned"], true);
    let changed = success(
        &db,
        &[
            "folders",
            "edit",
            folder_id,
            "--clear-icon",
            "--pinned",
            "false",
        ],
    );
    assert_eq!(changed["icon"], "");
    assert_eq!(changed["pinned"], false);
    let list = success(
        &db,
        &[
            "lists",
            "add",
            "Notes",
            "--folder",
            folder_id,
            "--list-type",
            "note",
            "--icon",
            "notebook",
            "--pinned",
            "true",
            "--color",
            "None",
            "--show-in-smart-lists",
            "false",
        ],
    );
    let list_id = list["id"].as_str().unwrap();
    assert_eq!(list["color"], "");
    assert_eq!(list["list_type"], "note");
    let section = success(&db, &["sections", "add", list_id, "Ideas"]);
    let section_id = section["id"].as_str().unwrap();
    let html = "<p>Design &amp; <strong>planning</strong></p><p>Second line</p>";
    let note = success(
        &db,
        &[
            "add",
            "Hidden note",
            "--list",
            list_id,
            "--section",
            section_id,
            "--notes-format",
            "html",
            "--notes",
            html,
        ],
    );
    assert_eq!(note["is_note"], true);
    assert_eq!(note["notes"], html);
    assert!(note["notes_text"]
        .as_str()
        .unwrap()
        .contains("Design & planning"));
    assert!(success(&db, &["list"]).as_array().unwrap().is_empty());
    assert_eq!(
        success(&db, &["list", "--search", "Design & planning Second line"])
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(success(&db, &["list", "--search", "strong"])
        .as_array()
        .unwrap()
        .is_empty());
    let task = success(
        &db,
        &[
            "add",
            "Finished task",
            "--list",
            list_id,
            "--is-note",
            "false",
        ],
    );
    assert_eq!(task["is_note"], false);
    success(&db, &["complete", task["id"].as_str().unwrap()]);
    let copied = success(
        &db,
        &["lists", "duplicate", list_id, "--name", "Copy", "--root"],
    );
    let copied_id = copied["id"].as_str().unwrap();
    assert_eq!(copied["list_type"], "note");
    assert_eq!(copied["pinned"], false);
    assert_eq!(copied["icon"], "notebook");
    assert!(copied["folder_id"].is_null());
    let notes = success(&db, &["list", "--list", copied_id]);
    assert_eq!(notes.as_array().unwrap().len(), 1);
    assert_ne!(notes[0]["id"], note["id"]);
    assert_eq!(notes[0]["notes_text"], note["notes_text"]);
    assert_ne!(notes[0]["section_id"], note["section_id"]);
    let completed = success(&db, &["list", "--view", "completed", "--list", copied_id]);
    assert_eq!(completed.as_array().unwrap().len(), 1);
    let converted = success(
        &db,
        &["edit", note["id"].as_str().unwrap(), "--is-note", "false"],
    );
    assert_eq!(converted["is_note"], false);
    assert_eq!(converted["notes"], html);
    let input = json!({"id":note["id"],"notes_text":"spoofed"}).to_string();
    assert!(!run(&db, &["call", "tasks.update", &input]).status.success());
}

#[test]
fn recurrence_preview_never_creates_database_or_parent_directory() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().join("must-not-exist");
    let db = parent.join("tasks.sqlite3");
    let input = json!({"due_date":"2026-01-01","recurrence":{"frequency":"daily","count":3},"range_start":"2026-01-01","range_end":"2026-01-31"}).to_string();
    let expected =
        json!({"dates":["2026-01-02","2026-01-03"],"truncated":false,"assumption":"due_date"});
    for alias in ["raw", "call"] {
        assert_eq!(
            success(&db, &[alias, "recurrence.preview", &input]),
            expected
        );
        let output = stdin_call(&db, alias, "recurrence.preview", input.as_bytes());
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap(),
            expected
        );
        for invalid in [
            "not-json",
            "[]",
            "{}",
            r#"{"due_date":"2026-13-01","recurrence":null,"range_start":"2026-01-01","range_end":"2026-01-31"}"#,
        ] {
            for output in [
                run(&db, &[alias, "recurrence.preview", invalid]),
                stdin_call(&db, alias, "recurrence.preview", invalid.as_bytes()),
            ] {
                assert!(!output.status.success());
                assert!(output.stdout.is_empty());
                assert!(!output.stderr.is_empty());
            }
        }
        assert!(!db.exists());
        assert!(
            !parent.exists(),
            "Pure preview must not create even the database parent directory"
        );
    }
}

#[test]
fn global_window_shortcut_persists_and_rejects_conflicts_atomically() {
    let directory = tempfile::tempdir().unwrap();
    let db = directory.path().join("global-shortcuts.sqlite3");
    let show = "global_show_hide_shortcut";
    let quick = "global_quick_add_shortcut";
    let defaults = success(&db, &["settings"]);
    assert_eq!(defaults[show], "");
    assert_eq!(defaults[quick], "");

    success(&db, &["settings", "set", show, r#""mod+shift+e""#]);
    success(&db, &["settings", "set", quick, r#""ctrl+shift+a""#]);
    // Every invocation is a new executable and database connection.
    let reopened = success(&db, &["settings"]);
    assert_eq!(reopened[show], "mod+shift+e");
    assert_eq!(reopened[quick], "ctrl+shift+a");
    let before = success(&db, &["snapshot"]);

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
            let output = run(&db, &["settings", "set", key, &invalid.to_string()]);
            assert!(!output.status.success(), "{key}: {invalid}");
            assert!(output.stdout.is_empty());
            assert!(!output.stderr.is_empty());
            assert_eq!(success(&db, &["snapshot"]), before);
        }
    }
    for (key, collision) in [(show, "ctrl+shift+a"), (quick, "mod+shift+e")] {
        let output = run(
            &db,
            &["settings", "set", key, &json!(collision).to_string()],
        );
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
        assert_eq!(success(&db, &["snapshot"]), before);
    }

    // Removing a binding releases its chord for the other action.
    success(&db, &["settings", "set", show, r#""""#]);
    success(&db, &["settings", "set", quick, r#""mod+shift+e""#]);
    success(&db, &["settings", "set", show, r#""ctrl+shift+a""#]);
    let swapped = success(&db, &["settings"]);
    assert_eq!(swapped[show], "ctrl+shift+a");
    assert_eq!(swapped[quick], "mod+shift+e");
}
