use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use tasks_core::TaskService;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
pub fn open_notification_settings(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let url = "x-apple.systempreferences:com.apple.Notifications-Settings.extension";
    #[cfg(not(target_os = "macos"))]
    let url = "ms-settings:notifications";
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_external_link(app: tauri::AppHandle, url: String) -> Result<(), String> {
    let parsed = tauri::Url::parse(&url).map_err(|_| "Invalid link".to_string())?;
    if !matches!(parsed.scheme(), "https" | "http" | "mailto") {
        return Err("Only web and email links can be opened.".into());
    }
    app.opener()
        .open_url(parsed.as_str(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn copy_text(text: String) -> Result<(), String> {
    if text.len() > 8 * 1024 * 1024 {
        return Err("The text exceeds 8 MiB.".into());
    }
    #[cfg(target_os = "macos")]
    {
        tauri::async_runtime::spawn_blocking(move || {
            use std::process::{Command, Stdio};
            let mut process = Command::new("/usr/bin/pbcopy")
                .env("LANG", "en_US.UTF-8")
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| e.to_string())?;
            let result = process
                .stdin
                .take()
                .ok_or_else(|| "Could not open the clipboard.".to_string())?
                .write_all(text.as_bytes());
            let status = process.wait().map_err(|e| e.to_string())?;
            result.map_err(|e| e.to_string())?;
            if !status.success() {
                return Err("Could not copy the text.".into());
            }
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())?
    }
    #[cfg(not(target_os = "macos"))]
    Err("Native clipboard is supported on macOS.".into())
}

#[tauri::command]
pub async fn import_attachments(
    app: tauri::AppHandle,
    service: tauri::State<'_, TaskService>,
    activities: tauri::State<'_, crate::quit::Activities>,
    task_id: String,
) -> Result<Value, String> {
    let service = service.inner().clone();
    let activity = activities.start();
    tauri::async_runtime::spawn_blocking(move || {
        let _activity = activity;
        let window = app
            .get_webview_window("main")
            .ok_or("The Tasks main window is unavailable")?;
        let Some(paths) = app
            .dialog()
            .file()
            .set_parent(&window)
            .set_title("Add Attachments")
            .blocking_pick_files()
        else {
            return Ok(json!({"attachments":[],"errors":[]}));
        };
        let mut attachments = Vec::new();
        let mut errors = Vec::new();
        for file in paths {
            match file.into_path() {
                Ok(path) => match service
                    .execute("attachments.import", json!({"task_id":task_id,"path":path}))
                {
                    Ok(attachment) => attachments.push(attachment),
                    Err(error) => errors.push(format!(
                        "{}: {error}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    )),
                },
                Err(error) => errors.push(error.to_string()),
            }
        }
        Ok(json!({"attachments":attachments,"errors":errors}))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn attachment_bytes(service: &TaskService, id: &str) -> Result<(String, Vec<u8>), String> {
    let attachment = service
        .execute("attachments.get", json!({"id":id}))
        .map_err(|e| e.to_string())?;
    let original = attachment["name"]
        .as_str()
        .ok_or("Attachment has no filename")?;
    let name = Path::new(original)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("attachment")
        .to_owned();
    let bytes = STANDARD
        .decode(
            attachment["content_base64"]
                .as_str()
                .ok_or("Attachment has no content")?,
        )
        .map_err(|e| e.to_string())?;
    Ok((name, bytes))
}

fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Invalid destination")?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    file.write_all(bytes)
        .and_then(|()| file.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn export_text(
    app: tauri::AppHandle,
    activities: tauri::State<'_, crate::quit::Activities>,
    name: String,
    content: String,
    format: String,
) -> Result<bool, String> {
    let (label, extension) = match format.as_str() {
        "markdown" => ("Markdown", "md"),
        "text" => ("Plain Text", "txt"),
        _ => return Err("Unsupported export format".into()),
    };
    if content.len() > 8 * 1024 * 1024 {
        return Err("The export exceeds 8 MiB.".into());
    }
    let stem: String = name
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '/' | '\\' | ':'))
        .take(100)
        .collect();
    let stem = stem.trim().trim_matches('.');
    let filename = format!(
        "{}.{}",
        if stem.is_empty() { "Task" } else { stem },
        extension
    );
    let activity = activities.start();
    tauri::async_runtime::spawn_blocking(move || {
        let _activity = activity;
        let window = app
            .get_webview_window("main")
            .ok_or("The Tasks main window is unavailable")?;
        let Some(file) = app
            .dialog()
            .file()
            .set_parent(&window)
            .set_title("Export Task")
            .set_file_name(filename)
            .add_filter(label, &[extension])
            .blocking_save_file()
        else {
            return Ok(false);
        };
        let path = file.into_path().map_err(|e| e.to_string())?;
        write_atomically(&path, content.as_bytes())?;
        Ok(true)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn save_attachment(
    app: tauri::AppHandle,
    service: tauri::State<'_, TaskService>,
    activities: tauri::State<'_, crate::quit::Activities>,
    id: String,
) -> Result<bool, String> {
    let service = service.inner().clone();
    let activity = activities.start();
    tauri::async_runtime::spawn_blocking(move || {
        let _activity = activity;
        let (name, bytes) = attachment_bytes(&service, &id)?;
        let window = app
            .get_webview_window("main")
            .ok_or("The Tasks main window is unavailable")?;
        let Some(file) = app
            .dialog()
            .file()
            .set_parent(&window)
            .set_title("Save Attachment")
            .set_file_name(name)
            .blocking_save_file()
        else {
            return Ok(false);
        };
        let path = file.into_path().map_err(|e| e.to_string())?;
        write_atomically(&path, &bytes)?;
        Ok(true)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn open_attachment(
    app: tauri::AppHandle,
    service: tauri::State<'_, TaskService>,
    activities: tauri::State<'_, crate::quit::Activities>,
    id: String,
) -> Result<(), String> {
    let service = service.inner().clone();
    let activity = activities.start();
    tauri::async_runtime::spawn_blocking(move || {
        let _activity = activity;
        let (name, bytes) = attachment_bytes(&service, &id)?;
        let extension = Path::new(&name)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !matches!(
            extension.as_str(),
            "pdf"
                | "png"
                | "jpg"
                | "jpeg"
                | "gif"
                | "webp"
                | "heic"
                | "txt"
                | "md"
                | "csv"
                | "json"
                | "mp3"
                | "m4a"
                | "wav"
                | "mp4"
                | "mov"
        ) {
            return Err(
                "Save this attachment to open it with an application of your choice.".into(),
            );
        }
        // The database-generated ID is validated independently before it is used
        // as a cache directory. No uploaded filename can escape its directory.
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err("Invalid attachment ID".into());
        }
        let directory: PathBuf = app
            .path()
            .app_cache_dir()
            .map_err(|e| e.to_string())?
            .join("attachments")
            .join(id);
        std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
        let path = directory.join(name);
        write_atomically(&path, &bytes)?;
        app.opener()
            .open_path(path.to_string_lossy(), None::<&str>)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
