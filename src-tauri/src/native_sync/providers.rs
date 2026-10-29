//! Bounded transport for per-device sync packets, never the live database.
//! The caller supplies a client with redirects disabled and a finite timeout.
//!
//! Provider protocol references (checked 2026-10-29):
//! https://developers.google.com/workspace/drive/api/guides/appdata
//! https://developers.google.com/workspace/drive/api/guides/manage-uploads
//! https://docs.dropboxapi.com/dropbox-api/api-reference/user-endpoints/files/list-folder
//! https://docs.dropboxapi.com/dropbox-api/api-reference/user-endpoints/files/download
//! https://docs.dropboxapi.com/dropbox-api/api-reference/user-endpoints/files/upload

use reqwest::blocking::{Client, RequestBuilder, Response};
use reqwest::{header, Url};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::Read;

const MAX_PACKET: usize = 64 * 1024 * 1024;
// Keep in step with the core's maximum number of packets per merge.
const MAX_FILES: usize = 32;
const MAX_METADATA: usize = 1024 * 1024;
const MAX_PAGES: usize = 20;
const SMALL_GOOGLE_UPLOAD: usize = 5_000_000;
const GOOGLE_FILES: &str = "https://www.googleapis.com/drive/v3/files";
const GOOGLE_FILE_FIELDS: &str = "id,name,version,modifiedTime,size";
const GOOGLE_UPLOAD: &str = "https://www.googleapis.com/upload/drive/v3/files";
const DROPBOX_LIST: &str = "https://api.dropboxapi.com/2/files/list_folder";
const DROPBOX_CONTINUE: &str = "https://api.dropboxapi.com/2/files/list_folder/continue";
const DROPBOX_DOWNLOAD: &str = "https://content.dropboxapi.com/2/files/download";
const DROPBOX_UPLOAD: &str = "https://content.dropboxapi.com/2/files/upload";

#[derive(Clone, Debug)]
pub(super) struct RemoteFile {
    pub id: String,
    pub name: String,
    /// Provider-issued content revision, used only with this file ID.
    pub revision: String,
    pub size: Option<u64>,
}

fn safe_identifier(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn packet_name(name: &str) -> bool {
    name.strip_prefix("tasks-sync-")
        .and_then(|name| name.strip_suffix(".json"))
        .is_some_and(|device| safe_identifier(device, 128))
}

fn validate_file(provider: &str, file: &RemoteFile) -> Result<(), String> {
    let valid_id = match provider {
        "google_drive" => safe_identifier(&file.id, 256),
        "dropbox" => file
            .id
            .strip_prefix("id:")
            .is_some_and(|id| safe_identifier(id, 256)),
        _ => return Err("Unsupported sync provider.".into()),
    };
    if !packet_name(&file.name)
        || !valid_id
        || file.revision.is_empty()
        || file.revision.len() > 256
        || file.revision.chars().any(char::is_control)
    {
        return Err("Invalid cloud sync file metadata.".into());
    }
    Ok(())
}

fn retry_after_seconds(value: Option<&str>, now: chrono::DateTime<chrono::Utc>) -> u64 {
    let delay = value
        .and_then(|value| {
            value.trim().parse::<u64>().ok().or_else(|| {
                chrono::DateTime::parse_from_rfc2822(value.trim())
                    .ok()
                    .map(|date| {
                        date.with_timezone(&chrono::Utc)
                            .signed_duration_since(now)
                            .num_seconds()
                            .max(1) as u64
                    })
            })
        })
        .unwrap_or(300);
    delay.clamp(1, 3600)
}

fn send(
    request: RequestBuilder,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<Response, String> {
    // Check at the request boundary, including every page and resumable phase.
    // Cancellation therefore waits for at most the caller's single-request timeout.
    check()?;
    // Do not return reqwest errors: URLs (including session secrets), server
    // response text, and tokens must not reach UI diagnostics or application logs.
    let response = request
        .send()
        .map_err(|_| "Cloud request could not be completed.".to_owned())?;
    if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let delay = retry_after_seconds(
            response
                .headers()
                .get(header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok()),
            chrono::Utc::now(),
        );
        return Err(format!(
            "Cloud provider rate limit. Retry after {delay} seconds."
        ));
    }
    if !response.status().is_success() {
        return Err(format!(
            "Cloud request failed (HTTP {}).",
            response.status().as_u16()
        ));
    }
    Ok(response)
}

fn read_bounded(reader: impl Read, maximum: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cloud response could not be read.".to_owned())?;
    if bytes.len() > maximum {
        return Err("Cloud response exceeds the supported size limit.".into());
    }
    Ok(bytes)
}

fn response_bytes(response: Response, maximum: usize) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|size| size > maximum as u64)
    {
        return Err("Cloud response exceeds the supported size limit.".into());
    }
    read_bounded(response, maximum)
}

fn response_json(response: Response) -> Result<Value, String> {
    serde_json::from_slice(&response_bytes(response, MAX_METADATA)?)
        .map_err(|_| "Cloud response contains invalid metadata.".to_owned())
}

fn append_file(
    files: &mut Vec<RemoteFile>,
    provider: &str,
    id: &str,
    name: &str,
    revision: String,
    size: Option<u64>,
) -> Result<(), String> {
    let file = RemoteFile {
        id: id.into(),
        name: name.into(),
        revision,
        size,
    };
    validate_file(provider, &file)?;
    if files
        .iter()
        .any(|other| other.id == file.id || other.name == file.name)
    {
        return Err("Duplicate device sync files were found in cloud storage.".into());
    }
    if files.len() >= MAX_FILES {
        return Err("Cloud sync supports at most 32 device files.".into());
    }
    files.push(file);
    Ok(())
}

fn file_revision(provider: &str, entry: &Value) -> Result<(String, Option<u64>), String> {
    let size = match &entry["size"] {
        Value::Null => None,
        Value::Number(number) => Some(
            number
                .as_u64()
                .ok_or("Cloud file has invalid size metadata.")?,
        ),
        Value::String(number) => Some(
            number
                .parse::<u64>()
                .map_err(|_| "Cloud file has invalid size metadata.")?,
        ),
        _ => return Err("Cloud file has invalid size metadata.".into()),
    };
    if size.is_some_and(|size| size > MAX_PACKET as u64) {
        return Err("Cloud sync file exceeds the supported size limit.".into());
    }
    let revision = match provider {
        "google_drive" => {
            let version = entry["version"]
                .as_str()
                .filter(|value| {
                    !value.is_empty()
                        && value.len() <= 40
                        && value.bytes().all(|byte| byte.is_ascii_digit())
                })
                .ok_or("Cloud file has invalid revision metadata.")?;
            let modified = entry["modifiedTime"]
                .as_str()
                .filter(|value| {
                    value.len() <= 64 && chrono::DateTime::parse_from_rfc3339(value).is_ok()
                })
                .ok_or("Cloud file has invalid revision metadata.")?;
            format!("{version}:{modified}")
        }
        "dropbox" => entry["rev"]
            .as_str()
            .filter(|value| safe_identifier(value, 128))
            .ok_or("Cloud file has invalid revision metadata.")?
            .to_owned(),
        _ => return Err("Unsupported sync provider.".into()),
    };
    Ok((revision, size))
}

fn next_page(value: &Value, seen: &mut HashSet<String>) -> Result<Option<String>, String> {
    match value {
        Value::Null => Ok(None),
        Value::String(token) if token.is_empty() => Ok(None),
        Value::String(token) if token.len() <= 4096 && seen.insert(token.clone()) => {
            Ok(Some(token.clone()))
        }
        _ => Err("Cloud response contains invalid pagination metadata.".into()),
    }
}

pub(super) fn list_files(
    client: &Client,
    provider: &str,
    token: &str,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<Vec<RemoteFile>, String> {
    match provider {
        "google_drive" => google_list(client, token, check),
        "dropbox" => dropbox_list(client, token, check),
        _ => Err("Unsupported sync provider.".into()),
    }
}

fn google_list(
    client: &Client,
    token: &str,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<Vec<RemoteFile>, String> {
    let mut files = Vec::new();
    let mut page: Option<String> = None;
    let mut seen = HashSet::new();
    for _ in 0..MAX_PAGES {
        let mut request = client.get(GOOGLE_FILES).bearer_auth(token).query(&[
            ("spaces", "appDataFolder"),
            ("q", "name contains 'tasks-sync-' and trashed = false"),
            (
                "fields",
                "nextPageToken,files(id,name,version,modifiedTime,size)",
            ),
            ("pageSize", "100"),
        ]);
        if let Some(page) = &page {
            request = request.query(&[("pageToken", page)]);
        }
        let result = response_json(send(request, check)?)?;
        let entries = result["files"]
            .as_array()
            .ok_or("Cloud response is missing its file list.")?;
        for entry in entries {
            let name = entry["name"]
                .as_str()
                .ok_or("Cloud response contains invalid file metadata.")?;
            if packet_name(name) {
                let id = entry["id"]
                    .as_str()
                    .ok_or("Cloud response contains invalid file metadata.")?;
                let (revision, size) = file_revision("google_drive", entry)?;
                append_file(&mut files, "google_drive", id, name, revision, size)?;
            }
        }
        page = next_page(&result["nextPageToken"], &mut seen)?;
        if page.is_none() {
            return Ok(files);
        }
    }
    Err("Cloud file listing exceeded its pagination limit.".into())
}

fn dropbox_list(
    client: &Client,
    token: &str,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<Vec<RemoteFile>, String> {
    let mut files = Vec::new();
    let mut cursor: Option<String> = None;
    let mut seen = HashSet::new();
    for _ in 0..MAX_PAGES {
        let request = if let Some(cursor) = &cursor {
            client
                .post(DROPBOX_CONTINUE)
                .json(&json!({"cursor": cursor}))
        } else {
            client.post(DROPBOX_LIST).json(&json!({
                "path": "", "recursive": false, "include_deleted": false,
                "include_non_downloadable_files": false, "limit": 100
            }))
        };
        let result = response_json(send(request.bearer_auth(token), check)?)?;
        let entries = result["entries"]
            .as_array()
            .ok_or("Cloud response is missing its file list.")?;
        for entry in entries {
            if entry[".tag"].as_str() != Some("file") {
                continue;
            }
            let name = entry["name"]
                .as_str()
                .ok_or("Cloud response contains invalid file metadata.")?;
            if packet_name(name) {
                let id = entry["id"]
                    .as_str()
                    .ok_or("Cloud response contains invalid file metadata.")?;
                // list_folder is non-recursive; reject any unexpected nested path.
                let expected_path = format!("/{name}");
                let root_file = entry["path_display"].as_str() == Some(expected_path.as_str())
                    || entry["path_lower"].as_str()
                        == Some(expected_path.to_ascii_lowercase().as_str());
                if !root_file {
                    return Err("Cloud response contains a sync file outside the app root.".into());
                }
                let (revision, size) = file_revision("dropbox", entry)?;
                append_file(&mut files, "dropbox", id, name, revision, size)?;
            }
        }
        match result["has_more"].as_bool() {
            Some(false) => return Ok(files),
            Some(true) => {
                cursor = next_page(&result["cursor"], &mut seen)?;
                if cursor.is_none() {
                    return Err("Cloud response is missing its pagination cursor.".into());
                }
            }
            None => return Err("Cloud response contains invalid pagination metadata.".into()),
        }
    }
    Err("Cloud file listing exceeded its pagination limit.".into())
}

pub(super) fn download(
    client: &Client,
    provider: &str,
    token: &str,
    file: &RemoteFile,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<Vec<u8>, String> {
    validate_file(provider, file)?;
    if file.size.is_some_and(|size| size > MAX_PACKET as u64) {
        return Err("Cloud sync file exceeds the supported size limit.".into());
    }
    let request = match provider {
        "google_drive" => client
            .get(format!("{GOOGLE_FILES}/{}", file.id))
            .query(&[("alt", "media")]),
        "dropbox" => client
            .post(DROPBOX_DOWNLOAD)
            .header("Dropbox-API-Arg", json!({"path": file.id}).to_string()),
        _ => return Err("Unsupported sync provider.".into()),
    };
    response_bytes(send(request.bearer_auth(token), check)?, MAX_PACKET)
}

pub(super) fn upload(
    client: &Client,
    provider: &str,
    token: &str,
    name: &str,
    existing: Option<&RemoteFile>,
    bytes: Vec<u8>,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<Option<RemoteFile>, String> {
    if !packet_name(name) {
        return Err("Invalid device sync filename.".into());
    }
    if bytes.is_empty() || bytes.len() > MAX_PACKET {
        return Err("Sync packet exceeds the supported size limit or is empty.".into());
    }
    if let Some(existing) = existing {
        validate_file(provider, existing)?;
        if existing.name != name {
            return Err("Cannot overwrite another device's sync file.".into());
        }
    }
    match provider {
        "google_drive" => google_upload(client, token, name, existing, bytes, check),
        "dropbox" => {
            let response = send(
                client
                    .post(DROPBOX_UPLOAD)
                    .bearer_auth(token)
                    .header(header::CONTENT_TYPE, "application/octet-stream")
                    .header(
                        "Dropbox-API-Arg",
                        json!({
                            "path": format!("/{name}"), "mode": "overwrite", "autorename": false,
                            "mute": true, "strict_conflict": false
                        })
                        .to_string(),
                    )
                    .body(bytes),
                check,
            )?;
            uploaded_file(response, "dropbox", name)
        }
        _ => Err("Unsupported sync provider.".into()),
    }
}

fn uploaded_file(
    response: Response,
    provider: &str,
    expected_name: &str,
) -> Result<Option<RemoteFile>, String> {
    let bytes = response_bytes(response, MAX_METADATA)?;
    if bytes.is_empty() {
        return Ok(None);
    }
    let entry: Value =
        serde_json::from_slice(&bytes).map_err(|_| "Cloud response contains invalid metadata.")?;
    let (Some(id), Some(name)) = (entry["id"].as_str(), entry["name"].as_str()) else {
        return Ok(None);
    };
    if name != expected_name {
        return Err("Cloud upload returned an unexpected device file.".into());
    }
    if (provider == "google_drive"
        && (entry["version"].is_null() || entry["modifiedTime"].is_null()))
        || (provider == "dropbox" && entry["rev"].is_null())
    {
        return Ok(None);
    }
    let (revision, size) = file_revision(provider, &entry)?;
    let file = RemoteFile {
        id: id.into(),
        name: name.into(),
        revision,
        size,
    };
    validate_file(provider, &file)?;
    Ok(Some(file))
}

fn multipart_packet(name: &str, bytes: Vec<u8>) -> (String, Vec<u8>) {
    let metadata =
        json!({"name": name, "parents": ["appDataFolder"], "mimeType": "application/json"})
            .to_string();
    let mut suffix = 0;
    let boundary = loop {
        let candidate = format!("tasks-sync-packet-boundary-{suffix}");
        if !metadata.contains(&candidate)
            && !bytes
                .windows(candidate.len())
                .any(|chunk| chunk == candidate.as_bytes())
        {
            break candidate;
        }
        suffix += 1;
    };
    let mut body = format!("--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{metadata}\r\n--{boundary}\r\nContent-Type: application/json\r\n\r\n").into_bytes();
    body.extend(bytes);
    body.extend(format!("\r\n--{boundary}--\r\n").bytes());
    (boundary, body)
}

fn resumable_url(location: &str, expected_path: &str) -> Result<Url, String> {
    let url = Url::parse(location).map_err(|_| "Invalid cloud upload session.".to_owned())?;
    let query: Vec<_> = url.query_pairs().collect();
    let allowed_query = query.iter().all(|(key, value)| match key.as_ref() {
        "uploadType" => value == "resumable",
        "fields" => value == GOOGLE_FILE_FIELDS,
        "upload_id" => {
            !value.is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control)
        }
        _ => false,
    });
    if location.len() > 8192
        || url.scheme() != "https"
        || url.host_str() != Some("www.googleapis.com")
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.path() != expected_path
        || !allowed_query
        || query.iter().filter(|(key, _)| key == "uploadType").count() != 1
        || query.iter().filter(|(key, _)| key == "upload_id").count() != 1
    {
        return Err("Invalid cloud upload session.".into());
    }
    Ok(url)
}

fn google_upload(
    client: &Client,
    token: &str,
    name: &str,
    existing: Option<&RemoteFile>,
    bytes: Vec<u8>,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<Option<RemoteFile>, String> {
    if bytes.len() <= SMALL_GOOGLE_UPLOAD {
        let request = if let Some(file) = existing {
            client
                .patch(format!("{GOOGLE_UPLOAD}/{}", file.id))
                .query(&[("uploadType", "media"), ("fields", GOOGLE_FILE_FIELDS)])
                .header(header::CONTENT_TYPE, "application/json")
                .body(bytes)
        } else {
            let (boundary, body) = multipart_packet(name, bytes);
            client
                .post(GOOGLE_UPLOAD)
                .query(&[("uploadType", "multipart"), ("fields", GOOGLE_FILE_FIELDS)])
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/related; boundary={boundary}"),
                )
                .body(body)
        };
        return uploaded_file(
            send(request.bearer_auth(token), check)?,
            "google_drive",
            name,
        );
    }
    // Drive documents resumable uploads for payloads over 5 MB. No session URL
    // may escape Google's exact upload endpoint, even if the server supplies it.
    let (endpoint, metadata) = match existing {
        Some(file) => (format!("{GOOGLE_UPLOAD}/{}", file.id), json!({})),
        None => (
            GOOGLE_UPLOAD.to_owned(),
            json!({"name": name, "parents": ["appDataFolder"], "mimeType": "application/json"}),
        ),
    };
    let request = if existing.is_some() {
        client.patch(&endpoint)
    } else {
        client.post(&endpoint)
    };
    let response = send(
        request
            .bearer_auth(token)
            .query(&[("uploadType", "resumable"), ("fields", GOOGLE_FILE_FIELDS)])
            .header("X-Upload-Content-Type", "application/json")
            .header("X-Upload-Content-Length", bytes.len().to_string())
            .json(&metadata),
        check,
    )?;
    let expected_path = endpoint
        .strip_prefix("https://www.googleapis.com")
        .ok_or("Invalid cloud upload endpoint.")?;
    let session = resumable_url(
        response
            .headers()
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .ok_or("Cloud response is missing its upload session.")?,
        expected_path,
    )?;
    response_bytes(response, MAX_METADATA)?;
    let response = send(
        client
            .put(session)
            .bearer_auth(token)
            .header(header::CONTENT_TYPE, "application/json")
            .body(bytes),
        check,
    )?;
    uploaded_file(response, "google_drive", name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_names_and_file_ids_cannot_escape_their_namespace() {
        assert!(packet_name(
            "tasks-sync-550e8400-e29b-41d4-a716-446655440000.json"
        ));
        for name in [
            "tasks-sync-.json",
            "tasks-sync-../other.json",
            "tasks-sync-a/b.json",
            "tasks-sync-a%2fb.json",
            "tasks-sync-a\r\n.json",
            "tasks-sync-é.json",
            "other.json",
        ] {
            assert!(!packet_name(name));
        }
        for id in ["../other", "id?alt=media", "https://example.org", "a%2fb"] {
            assert!(validate_file(
                "google_drive",
                &RemoteFile {
                    id: id.into(),
                    name: "tasks-sync-device.json".into(),
                    revision: "1".into(),
                    size: Some(10),
                }
            )
            .is_err());
        }
        assert!(validate_file(
            "dropbox",
            &RemoteFile {
                id: "id:a4ayc_80_OEAAAAAAAAAXw".into(),
                name: "tasks-sync-device.json".into(),
                revision: "1".into(),
                size: Some(10),
            }
        )
        .is_ok());
    }

    #[test]
    fn body_reader_stops_after_one_byte_over_the_limit() {
        struct Counter {
            count: std::rc::Rc<std::cell::Cell<usize>>,
        }
        impl Read for Counter {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                self.count.set(self.count.get() + buf.len());
                buf.fill(b'a');
                Ok(buf.len())
            }
        }
        let count = std::rc::Rc::new(std::cell::Cell::new(0));
        assert!(read_bounded(
            Counter {
                count: count.clone()
            },
            16
        )
        .is_err());
        assert_eq!(count.get(), 17);
        assert_eq!(read_bounded(&b"abcd"[..], 4).unwrap(), b"abcd");
    }

    #[test]
    fn listing_rejects_duplicates_excess_devices_and_repeated_cursors() {
        let mut files = Vec::new();
        for i in 0..MAX_FILES {
            append_file(
                &mut files,
                "google_drive",
                &format!("id{i}"),
                &format!("tasks-sync-device{i}.json"),
                "1".into(),
                Some(10),
            )
            .unwrap();
        }
        assert!(append_file(
            &mut files,
            "google_drive",
            "extra",
            "tasks-sync-extra.json",
            "1".into(),
            Some(10)
        )
        .is_err());
        assert!(append_file(
            &mut Vec::from([RemoteFile {
                id: "a".into(),
                name: "tasks-sync-a.json".into(),
                revision: "1".into(),
                size: Some(10),
            }]),
            "google_drive",
            "b",
            "tasks-sync-a.json",
            "1".into(),
            Some(10)
        )
        .is_err());
        let mut cursors = HashSet::new();
        assert_eq!(
            next_page(&json!("page"), &mut cursors).unwrap(),
            Some("page".into())
        );
        assert!(next_page(&json!("page"), &mut cursors).is_err());
    }

    #[test]
    fn resumable_sessions_cannot_redirect_credentials_or_inject_queries() {
        let path = "/upload/drive/v3/files/abc";
        assert!(resumable_url("https://www.googleapis.com/upload/drive/v3/files/abc?uploadType=resumable&upload_id=secret", path).is_ok());
        for url in [
            "https://www.googleapis.com.evil.test/upload/drive/v3/files/abc?uploadType=resumable&upload_id=secret",
            "http://www.googleapis.com/upload/drive/v3/files/abc?uploadType=resumable&upload_id=secret",
            "https://user@www.googleapis.com/upload/drive/v3/files/abc?uploadType=resumable&upload_id=secret",
            "https://www.googleapis.com:444/upload/drive/v3/files/abc?uploadType=resumable&upload_id=secret",
            "https://www.googleapis.com/other?uploadType=resumable&upload_id=secret",
            "https://www.googleapis.com/upload/drive/v3/files/abc?uploadType=resumable&upload_id=secret&access_token=secret",
            "https://www.googleapis.com/upload/drive/v3/files/abc?uploadType=resumable&upload_id=secret&upload_id=second",
        ] {
            assert_eq!(resumable_url(url, path).unwrap_err(), "Invalid cloud upload session.");
        }
    }

    #[test]
    fn multipart_uses_app_data_parent_and_avoids_boundary_collisions() {
        let payload = br#"{"message":"tasks-sync-packet-boundary-0"}"#.to_vec();
        let (boundary, body) = multipart_packet("tasks-sync-a.json", payload.clone());
        assert_eq!(boundary, "tasks-sync-packet-boundary-1");
        let text = String::from_utf8(body).unwrap();
        assert!(text.contains("\"parents\":[\"appDataFolder\"]"));
        assert!(text.contains(std::str::from_utf8(&payload).unwrap()));
        assert!(text.ends_with(&format!("\r\n--{boundary}--\r\n")));
    }
    #[test]
    fn revisions_change_with_provider_version_and_sizes_are_bounded() {
        let first = json!({"version":"1","modifiedTime":"2026-10-29T10:00:00Z","size":"123"});
        let second = json!({"version":"2","modifiedTime":"2026-10-29T10:00:00Z","size":"123"});
        assert_ne!(
            file_revision("google_drive", &first).unwrap().0,
            file_revision("google_drive", &second).unwrap().0
        );
        assert_eq!(file_revision("google_drive", &first).unwrap().1, Some(123));
        assert_eq!(
            file_revision("dropbox", &json!({"rev":"a1c10ce0dd78", "size": 100})).unwrap(),
            ("a1c10ce0dd78".into(), Some(100))
        );
        assert!(file_revision("dropbox", &json!({"rev":"a1", "size": MAX_PACKET + 1})).is_err());
        assert!(file_revision("google_drive", &json!({"version":"1"})).is_err());
        assert!(file_revision("dropbox", &json!({"size":1})).is_err());
    }

    #[test]
    fn retry_after_accepts_seconds_and_http_dates_with_bounded_fallback() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-29T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(retry_after_seconds(Some("120"), now), 120);
        assert_eq!(
            retry_after_seconds(Some("Thu, 29 Oct 2026 12:10:00 GMT"), now),
            600
        );
        assert_eq!(
            retry_after_seconds(Some("Thu, 29 Oct 2026 11:00:00 GMT"), now),
            1
        );
        assert_eq!(retry_after_seconds(Some("999999"), now), 3600);
        assert_eq!(retry_after_seconds(Some("bad secret value"), now), 300);
        assert_eq!(retry_after_seconds(None, now), 300);
    }
    #[test]
    fn cancelled_requests_return_before_attempting_http() {
        let client = Client::new();
        let checks = std::cell::Cell::new(0);
        let cancelled = || {
            checks.set(checks.get() + 1);
            Err("Sync cancelled.".to_owned())
        };
        // An invalid URL also prevents accidental network traffic in this test.
        let result = send(client.get("not a URL"), &cancelled);
        assert_eq!(result.unwrap_err(), "Sync cancelled.");
        assert_eq!(checks.get(), 1);
    }
}
