//! Personal file replication. Credentials never enter the task database or WebView.
//! Provider APIs are deliberately isolated from the causal merge engine in tasks-core.
//! OAuth references: https://developers.google.com/identity/protocols/oauth2/native-app
//! and https://docs.dropboxapi.com/dropbox-api/docs/oauth (checked 2026-10-29).
mod providers;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::Utc;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};
use tasks_core::TaskService;
use tauri::{Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

const REDIRECT_URI: &str = "http://127.0.0.1:49764/oauth/callback";
const KEYCHAIN_SERVICE: &str = "com.dwenn.tasks.cloud-sync";
const KEYCHAIN_ACCOUNT: &str = "personal-cloud-sync-v1";
const MAX_PACKET: usize = 64 * 1024 * 1024;
const MAX_TOTAL: usize = 128 * 1024 * 1024;
const TOKEN_LIMIT: u64 = 64 * 1024;

// No Debug implementation: OAuth credentials must never be formatted into logs.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct SavedRecord {
    provider: Option<String>,
    client_id: String,
    client_secret: Option<String>,
    auto_sync: bool,
    tokens: Option<Tokens>,
    last_sync_at: Option<String>,
    device_id: Option<String>,
    remote_revisions: BTreeMap<String, String>,
    last_uploaded_hash: Option<String>,
    last_revision: Option<u64>,
    cache_checkpoint: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Tokens {
    access_token: String,
    refresh_token: String,
    expires_at: i64,
}
#[derive(Default)]
struct Inner {
    loaded: bool,
    saved: SavedRecord,
    last_error: Option<String>,
}
#[derive(Default)]
struct Shared {
    inner: Mutex<Inner>,
    busy: AtomicBool,
    cancellation: AtomicU64,
    retry_after: Mutex<Option<Instant>>,
}
#[derive(Clone, Default)]
pub struct CloudSyncState(Arc<Shared>);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudStatus {
    provider: Option<String>,
    configured: bool,
    connected: bool,
    busy: bool,
    auto_sync: bool,
    last_sync_at: Option<String>,
    last_error: Option<String>,
    redirect_uri: &'static str,
    client_id: String,
    has_client_secret: bool,
    supported: bool,
    disabled_reason: Option<String>,
    available_providers: BTreeMap<&'static str, bool>,
}
impl CloudSyncState {
    fn load(&self) -> Result<(), String> {
        supported()?;
        let mut inner = self
            .0
            .inner
            .lock()
            .map_err(|_| "Cloud sync state is unavailable.")?;
        if inner.loaded {
            return Ok(());
        }
        inner.saved = keychain_read()?;
        inner.loaded = true;
        Ok(())
    }
    fn record(&self) -> Result<SavedRecord, String> {
        self.load()?;
        Ok(self
            .0
            .inner
            .lock()
            .map_err(|_| "Cloud sync state is unavailable.")?
            .saved
            .clone())
    }
    fn store(&self, saved: SavedRecord) -> Result<(), String> {
        // Persist before publishing: a Keychain denial must never look like success.
        keychain_write(&saved)?;
        let mut inner = self
            .0
            .inner
            .lock()
            .map_err(|_| "Cloud sync state is unavailable.")?;
        inner.saved = saved;
        inner.loaded = true;
        Ok(())
    }
    fn status(&self) -> CloudStatus {
        let guard = self.0.inner.lock().unwrap_or_else(|e| e.into_inner());
        let saved = &guard.saved;
        CloudStatus {
            provider: saved.provider.clone(),
            configured: saved.provider.is_some() && !saved.client_id.is_empty(),
            connected: saved.tokens.is_some(),
            busy: self.0.busy.load(Ordering::Acquire),
            auto_sync: saved.auto_sync,
            last_sync_at: saved.last_sync_at.clone(),
            last_error: guard.last_error.clone(),
            redirect_uri: REDIRECT_URI,
            client_id: saved.client_id.clone(),
            has_client_secret: saved.client_secret.is_some(),
            supported: environment_allowed(),
            disabled_reason: supported().err(),
            available_providers: BTreeMap::from([
                ("google_drive", !built_in_client("google_drive").is_empty()),
                ("dropbox", !built_in_client("dropbox").is_empty()),
            ]),
        }
    }
    fn error(&self, value: Option<String>) {
        if let Ok(mut inner) = self.0.inner.lock() {
            inner.last_error = value;
        }
    }
}
fn publish(app: &tauri::AppHandle, state: &CloudSyncState) {
    let _ = app.emit("cloud-sync-status", state.status());
}
fn supported() -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("Personal cloud sync requires the macOS Keychain on this platform.".into());
    }
    if !environment_allowed() {
        return Err("Cloud sync is disabled in development builds and custom database sessions to keep test data isolated.".into());
    }
    Ok(())
}
fn allowed_environment(macos: bool, debug: bool, custom_database: bool) -> bool {
    macos && !debug && !custom_database
}
fn environment_allowed() -> bool {
    allowed_environment(
        cfg!(target_os = "macos"),
        cfg!(debug_assertions),
        std::env::var_os("TASKS_DB").is_some() || std::env::var_os("TASKS_DEV_DB").is_some(),
    )
}
fn built_in_client(provider: &str) -> &'static str {
    match provider {
        "google_drive" => option_env!("TASKS_GOOGLE_CLIENT_ID").unwrap_or("").trim(),
        "dropbox" => option_env!("TASKS_DROPBOX_APP_KEY").unwrap_or("").trim(),
        _ => "",
    }
}
fn default_record() -> SavedRecord {
    let provider =
        if built_in_client("google_drive").is_empty() && !built_in_client("dropbox").is_empty() {
            "dropbox"
        } else {
            "google_drive"
        };
    SavedRecord {
        provider: Some(provider.into()),
        client_id: built_in_client(provider).into(),
        client_secret: if provider == "google_drive" {
            option_env!("TASKS_GOOGLE_CLIENT_SECRET")
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        } else {
            None
        },
        ..Default::default()
    }
}
#[cfg(target_os = "macos")]
fn keychain_read() -> Result<SavedRecord, String> {
    supported()?;
    use security_framework::passwords::{generic_password, PasswordOptions};
    match generic_password(PasswordOptions::new_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)) {
        Ok(bytes) => {
            if bytes.len() > 128 * 1024 { return Err("The cloud sync Keychain entry is invalid.".into()); }
            serde_json::from_slice(&bytes).map_err(|_| "The cloud sync Keychain entry is invalid.".into())
        }
        Err(e) if e.code() == -25300 => Ok(default_record()),
        Err(_) => Err("Tasks could not read its cloud sync credentials from Keychain. Allow Keychain access and retry.".into()),
    }
}
#[cfg(not(target_os = "macos"))]
fn keychain_read() -> Result<SavedRecord, String> {
    Ok(default_record())
}
#[cfg(target_os = "macos")]
fn keychain_write(saved: &SavedRecord) -> Result<(), String> {
    supported()?;
    let bytes = serde_json::to_vec(saved)
        .map_err(|_| "Could not prepare the cloud sync Keychain entry.")?;
    security_framework::passwords::set_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT, &bytes)
        .map_err(|_| "Tasks could not save cloud sync credentials in Keychain. Allow Keychain access and retry.".into())
}
#[cfg(not(target_os = "macos"))]
fn keychain_write(_: &SavedRecord) -> Result<(), String> {
    supported()
}

struct Operation {
    state: CloudSyncState,
    generation: u64,
    _activity: crate::quit::ActivityGuard,
}
impl Drop for Operation {
    fn drop(&mut self) {
        self.state.0.busy.store(false, Ordering::Release);
    }
}
impl Operation {
    fn begin(app: &tauri::AppHandle) -> Result<Self, String> {
        supported()?;
        if crate::quit::is_pending(app) {
            return Err("Tasks is saving before quitting.".into());
        }
        let state = app.state::<CloudSyncState>().inner().clone();
        state.load()?;
        let generation = state.0.cancellation.load(Ordering::Acquire);
        if state
            .0
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err("A cloud sync operation is already running.".into());
        }
        let activity = app.state::<crate::quit::Activities>().start();
        state.error(None);
        publish(app, &state);
        Ok(Self {
            state,
            generation,
            _activity: activity,
        })
    }
    fn check(&self, app: &tauri::AppHandle) -> Result<(), String> {
        if self.generation != self.state.0.cancellation.load(Ordering::Acquire)
            || crate::quit::is_pending(app)
        {
            Err("Cloud sync was cancelled.".into())
        } else {
            Ok(())
        }
    }
}
fn client() -> Result<Client, String> {
    Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .user_agent("Tasks-Desktop/0.1")
        .build()
        .map_err(|_| "Could not initialize the secure cloud connection.".into())
}
fn validate_config(provider: &str, client_id: &str, secret: Option<&str>) -> Result<(), String> {
    if !matches!(provider, "google_drive" | "dropbox") {
        return Err("Choose Google Drive or Dropbox.".into());
    }
    if client_id.is_empty()
        || client_id.len() > 512
        || !client_id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
    {
        return Err("Enter a valid OAuth client ID or Dropbox app key.".into());
    }
    if provider == "google_drive" && !client_id.ends_with(".apps.googleusercontent.com") {
        return Err(
            "Use a Google Desktop OAuth client ID ending in .apps.googleusercontent.com.".into(),
        );
    }
    if let Some(secret) = secret {
        if secret.len() > 512 || secret.chars().any(char::is_control) {
            return Err("The OAuth client secret is invalid.".into());
        }
        if provider == "dropbox" && !secret.is_empty() {
            return Err("Dropbox uses PKCE and does not need an app secret.".into());
        }
    }
    Ok(())
}
fn random_url_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| "Secure randomness is unavailable.")?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}
fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}
fn auth_url(record: &SavedRecord, state: &str, verifier: &str) -> Result<tauri::Url, String> {
    let provider = record
        .provider
        .as_deref()
        .ok_or("Configure cloud sync first.")?;
    let mut url = tauri::Url::parse(if provider == "google_drive" {
        "https://accounts.google.com/o/oauth2/v2/auth"
    } else {
        "https://www.dropbox.com/oauth2/authorize"
    })
    .map_err(|_| "Invalid OAuth endpoint.")?;
    let code_challenge = challenge(verifier);
    let mut query = url.query_pairs_mut();
    query.extend_pairs([
        ("client_id", record.client_id.as_str()),
        ("redirect_uri", REDIRECT_URI),
        ("response_type", "code"),
        ("state", state),
        ("code_challenge", code_challenge.as_str()),
        ("code_challenge_method", "S256"),
    ]);
    if provider == "google_drive" {
        query.extend_pairs([
            ("scope", "https://www.googleapis.com/auth/drive.appdata"),
            ("access_type", "offline"),
            ("prompt", "consent"),
        ]);
    } else {
        query.extend_pairs([
            (
                "scope",
                "files.metadata.read files.content.read files.content.write",
            ),
            ("token_access_type", "offline"),
        ]);
    }
    drop(query);
    Ok(url)
}
fn constant_eq(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.bytes()
        .zip(right.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}
// None is an unrelated or invalid request; it cannot terminate the active login.
fn callback_code(request: &str, expected_state: &str) -> Option<Result<String, String>> {
    if !request.contains("\r\n\r\n") {
        return None;
    }
    let mut lines = request.split("\r\n");
    let mut first = lines.next()?.split_whitespace();
    if first.next()? != "GET" {
        return None;
    }
    let target = first.next()?;
    if first.next()? != "HTTP/1.1"
        || first.next().is_some()
        || !target.starts_with("/oauth/callback?")
    {
        return None;
    }
    let host = lines.find_map(|line| {
        line.split_once(':')
            .filter(|(name, _)| name.eq_ignore_ascii_case("host"))
            .map(|(_, value)| value.trim())
    });
    if host != Some("127.0.0.1:49764") {
        return None;
    }
    let url = tauri::Url::parse(&format!("http://127.0.0.1:49764{target}")).ok()?;
    if url.path() != "/oauth/callback" {
        return None;
    }
    let pairs: Vec<_> = url.query_pairs().collect();
    let states: Vec<_> = pairs.iter().filter(|(key, _)| key == "state").collect();
    if states.len() != 1 || !constant_eq(&states[0].1, expected_state) {
        return None;
    }
    if pairs.iter().any(|(key, _)| key == "error") {
        return Some(Err(
            "Authorization was declined. No cloud account was connected.".into(),
        ));
    }
    let codes: Vec<_> = pairs.iter().filter(|(key, _)| key == "code").collect();
    if codes.len() != 1
        || codes[0].1.is_empty()
        || codes[0].1.len() > 4096
        || codes[0].1.chars().any(char::is_control)
    {
        return None;
    }
    Some(Ok(codes[0].1.to_string()))
}
fn reply(stream: &mut TcpStream, valid: bool) {
    let body = if valid {
        "<!doctype html><meta charset=utf-8><title>Tasks</title><p>Return to Tasks to finish connecting. You can close this browser tab.</p>"
    } else {
        "Invalid callback. Return to Tasks and retry the connection."
    };
    let status = if valid { "200 OK" } else { "400 Bad Request" };
    let response = format!("HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nContent-Security-Policy: default-src 'none'; frame-ancestors 'none'\r\nConnection: close\r\n\r\n{body}", body.len());
    let _ = stream.set_write_timeout(Some(Duration::from_millis(250)));
    let _ = stream.write_all(response.as_bytes());
}
fn wait_callback(
    app: &tauri::AppHandle,
    op: &Operation,
    listener: TcpListener,
    expected_state: &str,
) -> Result<String, String> {
    let deadline = Instant::now() + Duration::from_secs(180);
    listener
        .set_nonblocking(true)
        .map_err(|_| "Could not prepare the local sign-in callback.")?;
    loop {
        op.check(app)?;
        if Instant::now() >= deadline {
            return Err("Sign-in timed out. Choose Connect to try again.".into());
        }
        match listener.accept() {
            Ok((mut stream, peer)) => {
                if !peer.ip().is_loopback() {
                    continue;
                }
                let _ = stream.set_read_timeout(Some(Duration::from_millis(250)));
                let mut request = Vec::new();
                let mut block = [0u8; 1024];
                let read_deadline = deadline.min(Instant::now() + Duration::from_secs(2));
                while request.len() < 8192 {
                    op.check(app)?;
                    if Instant::now() >= read_deadline {
                        break;
                    }
                    match stream.read(&mut block) {
                        Ok(0) | Err(_) => break,
                        Ok(count) => {
                            request.extend_from_slice(&block[..count]);
                            if request.windows(4).any(|value| value == b"\r\n\r\n") {
                                break;
                            }
                        }
                    }
                }
                let code = if request.len() <= 8192 {
                    std::str::from_utf8(&request)
                        .ok()
                        .and_then(|value| callback_code(value, expected_state))
                } else {
                    None
                };
                reply(&mut stream, code.is_some());
                if let Some(code) = code {
                    return code;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100))
            }
            Err(_) => {
                return Err(
                    "The local sign-in callback stopped unexpectedly. Retry Connect.".into(),
                )
            }
        }
    }
}
fn token_endpoint(provider: &str) -> &'static str {
    if provider == "google_drive" {
        "https://oauth2.googleapis.com/token"
    } else {
        "https://api.dropboxapi.com/oauth2/token"
    }
}
fn token_request(
    client: &Client,
    record: &SavedRecord,
    values: &[(String, String)],
    existing_refresh: Option<&str>,
) -> Result<Tokens, String> {
    let provider = record
        .provider
        .as_deref()
        .ok_or("Configure cloud sync first.")?;
    let mut form = values.to_vec();
    form.push(("client_id".into(), record.client_id.clone()));
    if provider == "google_drive" {
        if let Some(secret) = &record.client_secret {
            form.push(("client_secret".into(), secret.clone()));
        }
    }
    let response = client
        .post(token_endpoint(provider))
        .form(&form)
        .send()
        .map_err(|_| {
            "Could not reach the cloud sign-in service. Check your connection and retry."
        })?;
    let status = response.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let delay = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| {
                value.parse::<u64>().ok().or_else(|| {
                    chrono::DateTime::parse_from_rfc2822(value)
                        .ok()
                        .map(|date| (date.timestamp() - Utc::now().timestamp()).max(1) as u64)
                })
            })
            .unwrap_or(300)
            .clamp(1, 3600);
        return Err(format!(
            "Cloud provider rate limit. Retry after {delay} seconds."
        ));
    }
    if !status.is_success() {
        return Err(if status.as_u16() == 400 || status.as_u16() == 401 { "Cloud authorization expired or the OAuth client configuration was rejected. Check the client details and connect again." } else { "The cloud sign-in service is unavailable. Try again later." }.into());
    }
    if response
        .content_length()
        .is_some_and(|size| size > TOKEN_LIMIT)
    {
        return Err("The sign-in response was too large.".into());
    }
    let mut bytes = Vec::new();
    response
        .take(TOKEN_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read the sign-in response.")?;
    if bytes.len() as u64 > TOKEN_LIMIT {
        return Err("The sign-in response was too large.".into());
    }
    parse_token_response(&bytes, existing_refresh)
}
fn parse_token_response(bytes: &[u8], existing_refresh: Option<&str>) -> Result<Tokens, String> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| "The cloud sign-in response was invalid.")?;
    let access = value["access_token"]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 8192 && !v.chars().any(char::is_control))
        .ok_or("The cloud service did not issue an access token.")?;
    let refresh = value["refresh_token"]
        .as_str()
        .or(existing_refresh)
        .filter(|v| !v.is_empty() && v.len() <= 8192 && !v.chars().any(char::is_control))
        .ok_or(
            "The cloud service did not grant offline access. Reconnect and approve offline access.",
        )?;
    let seconds = value["expires_in"]
        .as_i64()
        .filter(|value| *value > 0 && *value <= 31_536_000)
        .ok_or("The cloud sign-in expiry was invalid.")?;
    if value["token_type"]
        .as_str()
        .is_some_and(|value| !value.eq_ignore_ascii_case("bearer"))
    {
        return Err("The cloud service returned an unsupported token type.".into());
    }
    Ok(Tokens {
        access_token: access.into(),
        refresh_token: refresh.into(),
        expires_at: Utc::now().timestamp() + seconds,
    })
}
fn connect(app: &tauri::AppHandle, op: &Operation) -> Result<(), String> {
    let mut record = op.state.record()?;
    let provider = record
        .provider
        .as_deref()
        .ok_or("Choose a provider and save its OAuth client configuration first.")?;
    validate_config(provider, &record.client_id, record.client_secret.as_deref())?;
    let listener = TcpListener::bind("127.0.0.1:49764").map_err(|_| {
        "The local sign-in port 49764 is busy. Close another Tasks sign-in attempt and retry."
    })?;
    let state = random_url_token()?;
    let verifier = random_url_token()?;
    let url = auth_url(&record, &state, &verifier)?;
    op.check(app)?;
    app.opener()
        .open_url(url.as_str(), None::<&str>)
        .map_err(|_| "Could not open the system browser for sign-in.")?;
    let code = wait_callback(app, op, listener, &state)?;
    op.check(app)?;
    let tokens = token_request(
        &client()?,
        &record,
        &[
            ("grant_type".into(), "authorization_code".into()),
            ("code".into(), code),
            ("code_verifier".into(), verifier),
            ("redirect_uri".into(), REDIRECT_URI.into()),
        ],
        None,
    )?;
    op.check(app)?;
    record.tokens = Some(tokens);
    // A new authorization may select a different account. Never reuse metadata
    // from the previous account, even when the OAuth client is unchanged.
    record.remote_revisions.clear();
    record.last_uploaded_hash = None;
    record.last_revision = None;
    record.last_sync_at = None;
    record.cache_checkpoint = None;
    op.state.store(record)
}
fn current_token(
    app: &tauri::AppHandle,
    op: &Operation,
    client: &Client,
) -> Result<(SavedRecord, String), String> {
    let mut record = op.state.record()?;
    let tokens = record
        .tokens
        .as_ref()
        .ok_or("Connect a cloud account first.")?;
    if tokens.expires_at > Utc::now().timestamp() + 60 {
        return Ok((record.clone(), tokens.access_token.clone()));
    }
    op.check(app)?;
    let refreshed = token_request(
        client,
        &record,
        &[
            ("grant_type".into(), "refresh_token".into()),
            ("refresh_token".into(), tokens.refresh_token.clone()),
        ],
        Some(&tokens.refresh_token),
    );
    let next = match refreshed {
        Ok(next) => next,
        Err(error) => {
            if error.starts_with("Cloud authorization expired") {
                record.tokens = None;
                op.state.store(record)?;
            }
            return Err(error);
        }
    };
    op.check(app)?;
    let access = next.access_token.clone();
    record.tokens = Some(next);
    op.state.store(record.clone())?;
    Ok((record, access))
}
fn packet_filename(packet: &Value) -> Result<String, String> {
    let device = packet["device_id"]
        .as_str()
        .ok_or("The local sync packet has no device identity.")?;
    if device.len() != 36
        || !device.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
    {
        return Err("The local sync device identity is invalid.".into());
    }
    Ok(format!("tasks-sync-{device}.json"))
}
fn cache_matches_database(
    record: &SavedRecord,
    device: &str,
    revision: Option<u64>,
    checkpoint: Option<&str>,
) -> bool {
    record.device_id.as_deref() == Some(device)
        && record.cache_checkpoint.is_some()
        && record.cache_checkpoint.as_deref() == checkpoint
        && revision
            .zip(record.last_revision)
            .is_some_and(|(current, previous)| current >= previous)
}
fn synchronize(app: &tauri::AppHandle, op: &Operation) -> Result<(), String> {
    if let Some(deadline) = *op
        .state
        .0
        .retry_after
        .lock()
        .map_err(|_| "Cloud retry status is unavailable.")?
    {
        if deadline > Instant::now() {
            return Err(format!(
                "Cloud provider rate limit. Retry after {} seconds.",
                deadline
                    .saturating_duration_since(Instant::now())
                    .as_secs()
                    .max(1)
            ));
        }
    }
    let client = client()?;
    let (mut record, access) = current_token(app, op, &client)?;
    let provider = record
        .provider
        .clone()
        .ok_or("Configure cloud sync first.")?;
    let service = app.state::<TaskService>();
    let initial = service
        .execute("sync.status", json!({}))
        .map_err(|_| "Could not read local sync status.")?;
    let checkpoint = service
        .execute("sync.checkpoint", json!({"provider":provider}))
        .map_err(|_| "Could not validate the local sync checkpoint.")?;
    let device_id = initial["device_id"]
        .as_str()
        .ok_or("The local sync device identity is missing.")?;
    let expected_checkpoint = record
        .cache_checkpoint
        .clone()
        .filter(|_| record.device_id.as_deref() == Some(device_id));
    // A restored database can carry the same device UUID but an older revision.
    // Re-read all remote packets before publishing so a restore cannot replace
    // the device's newer cloud packet with its stale local snapshot.
    if !cache_matches_database(
        &record,
        device_id,
        initial["revision"].as_u64(),
        checkpoint["token"].as_str(),
    ) {
        record.device_id = Some(device_id.into());
        record.remote_revisions.clear();
        record.last_uploaded_hash = None;
        record.last_revision = None;
    }
    op.check(app)?;
    let files = providers::list_files(&client, &provider, &access, &|| op.check(app))?;
    if files.len() > 32 {
        return Err("This cloud folder contains more than 32 device files. Sync stopped without changing your data.".into());
    }
    let mut packets = Vec::new();
    let mut total = 0usize;
    let mut revisions = BTreeMap::new();
    for file in &files {
        op.check(app)?;
        let key = format!("{}:{}", file.id, file.name);
        revisions.insert(key.clone(), file.revision.clone());
        if record.remote_revisions.get(&key) == Some(&file.revision) {
            continue;
        }
        if file.size.is_some_and(|size| {
            size > MAX_PACKET as u64 || size.saturating_add(total as u64) > MAX_TOTAL as u64
        }) {
            return Err("Cloud sync exceeds the current limit of 64 MiB per device and 128 MiB per sync. No remote data was merged.".into());
        }
        let bytes = providers::download(&client, &provider, &access, file, &|| op.check(app))?;
        total = total
            .checked_add(bytes.len())
            .ok_or("Cloud sync data exceeded the safe size limit.")?;
        if bytes.len() > MAX_PACKET || total > MAX_TOTAL {
            return Err("Cloud sync exceeds the current limit of 64 MiB per device and 128 MiB per sync. No remote data was merged.".into());
        }
        let packet: Value = serde_json::from_slice(&bytes).map_err(|_| {
            "A cloud device file is not a valid Tasks sync packet. No remote data was merged."
        })?;
        if packet_filename(&packet)? != file.name {
            return Err("A cloud device file does not match its device identity. No remote data was merged.".into());
        }
        packets.push(packet);
    }
    op.check(app)?;
    if !packets.is_empty()
        || expected_checkpoint
            .as_deref()
            .is_some_and(|expected| checkpoint["token"].as_str() != Some(expected))
    {
        let mut merge = json!({"packets": packets});
        if let Some(expected) = expected_checkpoint {
            merge["checkpoint_provider"] = json!(provider);
            merge["expected_checkpoint"] = json!(expected);
        }
        let result = service.execute("sync.merge", merge).map_err(|_| "Remote task changes could not be merged. Your existing data is preserved; review sync conflicts and retry.")?;
        // The core merge is an atomic transaction. The activity guard prevents
        // quit during this boundary; cancellation is checked before export.
        if result["changed"].as_bool().unwrap_or(true) {
            let _ = app.emit("tasks-changed", ());
        }
    }
    op.check(app)?;
    // Capture revision before export: an edit arriving during network I/O must
    // remain eligible for the next debounced upload, never marked uploaded early.
    let local = service
        .execute("sync.status", json!({}))
        .map_err(|_| "Could not read local sync status.")?;
    let packet = service
        .execute("sync.export", json!({}))
        .map_err(|_| "Could not prepare local tasks for sync.")?;
    let filename = packet_filename(&packet)?;
    let bytes =
        serde_json::to_vec(&packet).map_err(|_| "Could not encode local tasks for sync.")?;
    if bytes.len() > MAX_PACKET {
        return Err("This device's sync packet exceeds 64 MiB. Large attachments are included in that limit.".into());
    }
    let fingerprint = URL_SAFE_NO_PAD.encode(Sha256::digest(&bytes));
    op.check(app)?;
    let existing = files.iter().find(|file| file.name == filename);
    if record.last_uploaded_hash.as_deref() != Some(&fingerprint) || existing.is_none() {
        if let Some(uploaded) = providers::upload(
            &client,
            &provider,
            &access,
            &filename,
            existing,
            bytes,
            &|| op.check(app),
        )? {
            revisions.insert(
                format!("{}:{}", uploaded.id, uploaded.name),
                uploaded.revision,
            );
        }
        record.last_uploaded_hash = Some(fingerprint);
    }
    op.check(app)?;
    record.remote_revisions = revisions;
    // The core may rotate the replica identity when reconciling a restored
    // database. Its exported identity, not the pre-merge one, owns this upload.
    record.device_id = packet["device_id"].as_str().map(str::to_owned);
    record.last_revision = local["revision"].as_u64();
    record.last_sync_at = Some(Utc::now().to_rfc3339());
    // Both stores must agree before a cached provider revision is trusted.
    // Restoring SQLite restores its older token, even if subsequent local edits
    // increase the revision beyond the last uploaded counter. A crash between
    // these writes leaves a safe mismatch and causes a full reconciliation.
    let checkpoint = random_url_token()?;
    service
        .execute(
            "sync.checkpoint.set",
            json!({"provider":provider,"token":checkpoint}),
        )
        .map_err(|_| "Could not save the local sync checkpoint.")?;
    record.cache_checkpoint = Some(checkpoint);
    op.state.store(record)
}

fn perform(
    app: tauri::AppHandle,
    action: fn(&tauri::AppHandle, &Operation) -> Result<(), String>,
) -> Result<CloudStatus, String> {
    let op = Operation::begin(&app)?;
    let state = op.state.clone();
    let result = action(&app, &op);
    if let Err(error) = &result {
        state.error(Some(error.clone()));
        if let Some(seconds) = retry_after(error) {
            if let Ok(mut until) = state.0.retry_after.lock() {
                *until = Some(Instant::now() + Duration::from_secs(seconds));
            }
        }
        if error == "Cloud request failed (HTTP 401)." {
            if let Ok(mut record) = state.record() {
                record.tokens = None;
                let _ = state.store(record);
            }
        }
    }
    drop(op);
    publish(&app, &state);
    result.map(|_| state.status())
}
#[tauri::command]
pub async fn cloud_sync_status(app: tauri::AppHandle) -> Result<CloudStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<CloudSyncState>();
        if let Err(error) = state.load() {
            state.error(Some(error));
        }
        Ok(state.status())
    })
    .await
    .map_err(|_| "Could not read cloud sync status.".to_string())?
}
#[tauri::command]
pub async fn cloud_sync_configure(
    app: tauri::AppHandle,
    provider: String,
    client_id: String,
    client_secret: Option<String>,
    auto_sync: bool,
    clear_client_secret: Option<bool>,
) -> Result<CloudStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let op = Operation::begin(&app)?;
        let state = op.state.clone();
        let result = (|| {
            let client_id = if client_id.trim().is_empty() {
                built_in_client(&provider).to_owned()
            } else {
                client_id.trim().to_string()
            };
            let secret = client_secret
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            validate_config(&provider, &client_id, secret.as_deref())?;
            let mut record = state.record()?;
            if record.provider.as_deref() != Some(&provider) || record.client_id != client_id {
                record.tokens = None;
                record.client_secret = None;
                record.last_sync_at = None;
                record.remote_revisions.clear();
                record.last_uploaded_hash = None;
                record.last_revision = None;
                record.cache_checkpoint = None;
            }
            record.provider = Some(provider.clone());
            record.client_id = client_id;
            record.auto_sync = auto_sync;
            if clear_client_secret.unwrap_or(false) || provider == "dropbox" {
                record.client_secret = None;
            }
            if let Some(secret) = secret {
                record.client_secret = Some(secret);
            }
            if provider == "google_drive"
                && record.client_secret.is_none()
                && record.client_id == built_in_client("google_drive")
                && !clear_client_secret.unwrap_or(false)
            {
                record.client_secret = option_env!("TASKS_GOOGLE_CLIENT_SECRET")
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned);
            }
            op.check(&app)?;
            state.store(record)
        })();
        if let Err(error) = &result {
            state.error(Some(error.clone()));
        }
        drop(op);
        publish(&app, &state);
        result.map(|_| state.status())
    })
    .await
    .map_err(|_| "Could not save cloud sync configuration.".to_string())?
}
#[tauri::command]
pub async fn cloud_sync_connect(app: tauri::AppHandle) -> Result<CloudStatus, String> {
    tauri::async_runtime::spawn_blocking(move || perform(app, connect))
        .await
        .map_err(|_| "The cloud sign-in operation stopped unexpectedly.".to_string())?
}
#[tauri::command]
pub async fn cloud_sync_run(app: tauri::AppHandle) -> Result<CloudStatus, String> {
    tauri::async_runtime::spawn_blocking(move || perform(app, synchronize))
        .await
        .map_err(|_| "The cloud sync operation stopped unexpectedly.".to_string())?
}
#[tauri::command]
pub async fn cloud_sync_disconnect(app: tauri::AppHandle) -> Result<CloudStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        perform(app, |app, op| {
            let mut record = op.state.record()?;
            record.tokens = None;
            record.auto_sync = false;
            op.check(app)?;
            // Only the local token is removed. No provider delete/revoke operation
            // can affect data or other devices linked to the same app client.
            op.state.store(record)
        })
    })
    .await
    .map_err(|_| "Could not disconnect cloud sync.".to_string())?
}
#[tauri::command]
pub fn cloud_sync_cancel(app: tauri::AppHandle) -> CloudStatus {
    let state = app.state::<CloudSyncState>();
    state.0.cancellation.fetch_add(1, Ordering::AcqRel);
    state.status()
}
pub fn start(app: tauri::AppHandle, stopping: Arc<AtomicBool>) {
    if !environment_allowed() {
        return;
    }
    std::thread::spawn(move || {
        let state = app.state::<CloudSyncState>();
        if let Err(error) = state.load() {
            state.error(Some(error));
            publish(&app, &state);
        }
        let mut next_remote = Instant::now() + Duration::from_secs(300 + jitter());
        let mut next_allowed = Instant::now();
        let mut last_attempt: Option<Instant> = None;
        let mut last_activity = Instant::now();
        let mut observed_revision = None;
        let mut dirty_since: Option<Instant> = None;
        let mut failures = 0u32;
        // Local status is cheap and never sends a cloud request. Remote metadata
        // checks back off to 15 minutes at rest; task edits trigger a debounced
        // upload. Unchanged packets and unchanged remote files transfer no body.
        while !stopping.load(Ordering::Acquire) {
            for _ in 0..10 {
                if stopping.load(Ordering::Acquire) {
                    return;
                }
                std::thread::sleep(Duration::from_secs(1));
            }
            if crate::quit::is_pending(&app) {
                continue;
            }
            let state = app.state::<CloudSyncState>();
            if state.0.busy.load(Ordering::Acquire) {
                continue;
            }
            // A Keychain refusal is retried by an explicit settings/status
            // request, never by ten-second background permission prompts.
            if !state
                .0
                .inner
                .lock()
                .map(|inner| inner.loaded)
                .unwrap_or(false)
            {
                continue;
            }
            let Ok(record) = state.record() else {
                continue;
            };
            if !record.auto_sync || record.tokens.is_none() {
                observed_revision = None;
                dirty_since = None;
                next_remote = Instant::now() + Duration::from_secs(300 + jitter());
                continue;
            }
            let Ok(local) = app.state::<TaskService>().execute("sync.status", json!({})) else {
                continue;
            };
            let Some(revision) = local["revision"].as_u64() else {
                continue;
            };
            let now = Instant::now();
            if record.last_revision != Some(revision) {
                if observed_revision != Some(revision) || dirty_since.is_none() {
                    dirty_since = Some(now);
                    last_activity = now;
                }
            } else {
                dirty_since = None;
            }
            observed_revision = Some(revision);
            let local_due = dirty_since
                .is_some_and(|since| now.duration_since(since) >= Duration::from_secs(15))
                && last_attempt
                    .is_none_or(|last| now.duration_since(last) >= Duration::from_secs(60));
            if now < next_allowed || (!local_due && now < next_remote) {
                continue;
            }
            last_attempt = Some(now);
            match perform(app.clone(), synchronize) {
                Ok(_) => {
                    failures = 0;
                    dirty_since = None;
                    let interval = if now.duration_since(last_activity) > Duration::from_secs(600) {
                        900
                    } else {
                        300
                    };
                    next_remote = Instant::now() + Duration::from_secs(interval + jitter());
                    next_allowed = Instant::now();
                }
                Err(error) => {
                    failures = failures.saturating_add(1);
                    next_allowed = Instant::now()
                        + Duration::from_secs(backoff_seconds(&error, failures) + jitter());
                    next_remote = next_allowed;
                }
            }
        }
    });
}
fn jitter() -> u64 {
    let mut bytes = [0u8; 2];
    if getrandom::fill(&mut bytes).is_ok() {
        u16::from_le_bytes(bytes) as u64 % 31
    } else {
        0
    }
}
fn retry_after(error: &str) -> Option<u64> {
    error
        .strip_prefix("Cloud provider rate limit. Retry after ")?
        .strip_suffix(" seconds.")?
        .parse::<u64>()
        .ok()
        .map(|seconds| seconds.clamp(1, 3600))
}
fn backoff_seconds(error: &str, failures: u32) -> u64 {
    let exponential = 60u64.saturating_mul(1u64 << failures.min(6)).min(3600);
    exponential.max(retry_after(error).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pkce_matches_rfc7636_vector() {
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }
    #[test]
    fn authorization_urls_use_minimum_scopes_and_pkce() {
        for provider in ["google_drive", "dropbox"] {
            let record = SavedRecord {
                provider: Some(provider.into()),
                client_id: "client".into(),
                ..Default::default()
            };
            let url = auth_url(&record, "state", "verifier").unwrap();
            let pairs: std::collections::HashMap<_, _> = url.query_pairs().collect();
            assert_eq!(url.scheme(), "https");
            assert_eq!(pairs["redirect_uri"], REDIRECT_URI);
            assert_eq!(pairs["code_challenge_method"], "S256");
            assert_eq!(pairs["state"], "state");
            assert!(!url.as_str().contains("client_secret"));
            if provider == "google_drive" {
                assert_eq!(
                    pairs["scope"],
                    "https://www.googleapis.com/auth/drive.appdata"
                );
            } else {
                assert_eq!(
                    pairs["scope"],
                    "files.metadata.read files.content.read files.content.write"
                );
            }
        }
    }
    #[test]
    fn callback_requires_local_host_path_and_single_matching_state() {
        let request = |query: &str| {
            format!("GET /oauth/callback?{query} HTTP/1.1\r\nHost: 127.0.0.1:49764\r\n\r\n")
        };
        assert_eq!(
            callback_code(&request("code=abc&state=good"), "good")
                .unwrap()
                .unwrap(),
            "abc"
        );
        for query in [
            "code=abc&state=bad",
            "code=abc",
            "code=abc&state=good&state=good",
            "code=a&code=b&state=good",
        ] {
            assert!(callback_code(&request(query), "good").is_none());
        }
        assert!(callback_code(
            &request("code=abc&state=good")
                .replace("Host: 127.0.0.1:49764", "Host: attacker.example"),
            "good"
        )
        .is_none());
        assert!(callback_code(
            &request("code=abc&state=good").replace("GET /oauth/callback?", "GET /wrong?"),
            "good"
        )
        .is_none());
        assert!(callback_code(
            "GET /oauth/callback?code=abc&state=good HTTP/1.1\r\nHost: 127.0.0.1:49764",
            "good"
        )
        .is_none());
        assert!(
            callback_code(&request("error=access_denied&state=good"), "good")
                .unwrap()
                .is_err()
        );
    }
    #[test]
    fn token_refresh_keeps_refresh_token_and_never_returns_provider_error_text() {
        let token = parse_token_response(
            br#"{"access_token":"access","expires_in":3600,"token_type":"Bearer"}"#,
            Some("refresh"),
        )
        .unwrap();
        assert_eq!(token.refresh_token, "refresh");
        assert!(token.expires_at > Utc::now().timestamp());
        let error = parse_token_response(br#"{"error_description":"secret-value"}"#, None)
            .err()
            .unwrap();
        assert!(!error.contains("secret-value"));
        assert!(parse_token_response(
            br#"{"access_token":"a","refresh_token":"r","expires_in":-1}"#,
            None
        )
        .is_err());
    }
    #[test]
    fn packet_name_cannot_escape_app_folder() {
        assert_eq!(
            packet_filename(&json!({"device_id":"12345678-1234-1234-1234-123456789abc"})).unwrap(),
            "tasks-sync-12345678-1234-1234-1234-123456789abc.json"
        );
        for id in ["../file", "", "a/b", "12345678-1234-1234-1234-123456789abz"] {
            assert!(packet_filename(&json!({"device_id":id})).is_err());
        }
    }
    #[test]
    fn status_has_no_tokens_or_secret_values() {
        let state = CloudSyncState::default();
        {
            let mut inner = state.0.inner.lock().unwrap();
            inner.saved.client_secret = Some("secret-value".into());
            inner.saved.tokens = Some(Tokens {
                access_token: "access-value".into(),
                refresh_token: "refresh-value".into(),
                expires_at: 99,
            });
        }
        let status = serde_json::to_string(&state.status()).unwrap();
        for value in [
            "secret-value",
            "access-value",
            "refresh-value",
            "access_token",
            "refresh_token",
        ] {
            assert!(!status.contains(value));
        }
        assert!(state.status().has_client_secret);
    }
    #[test]
    fn client_validation_rejects_urls_and_unsupported_providers() {
        assert!(validate_config("google_drive", "123.apps.googleusercontent.com", None).is_ok());
        assert!(validate_config("dropbox", "appKey123", None).is_ok());
        assert!(validate_config("google_drive", "https://evil", None).is_err());
        assert!(validate_config("dropbox", "appKey123", Some("secret")).is_err());
        assert!(validate_config("other", "key", None).is_err());
    }
    #[test]
    fn automatic_retries_respect_provider_cooldown_and_back_off() {
        assert_eq!(
            retry_after("Cloud provider rate limit. Retry after 600 seconds."),
            Some(600)
        );
        assert_eq!(retry_after("Cloud request failed (HTTP 500)."), None);
        assert_eq!(
            backoff_seconds("Cloud provider rate limit. Retry after 600 seconds.", 1),
            600
        );
        assert_eq!(
            backoff_seconds("Cloud request could not be completed.", 1),
            120
        );
        assert_eq!(
            backoff_seconds("Cloud request could not be completed.", 20),
            3600
        );
    }
    #[test]
    fn metadata_cache_is_account_local_and_not_exposed_to_the_webview() {
        let state = CloudSyncState::default();
        {
            let mut inner = state.0.inner.lock().unwrap();
            inner
                .saved
                .remote_revisions
                .insert("provider-private-file-id".into(), "revision".into());
            inner.saved.last_uploaded_hash = Some("internal-packet-fingerprint".into());
        }
        let status = serde_json::to_string(&state.status()).unwrap();
        assert!(!status.contains("provider-private-file-id"));
        assert!(!status.contains("internal-packet-fingerprint"));
        assert!(!status.contains("remoteRevisions"));
    }
    #[test]
    fn development_and_custom_database_sessions_cannot_access_production_sync() {
        assert!(allowed_environment(true, false, false));
        assert!(!allowed_environment(true, true, false));
        assert!(!allowed_environment(true, false, true));
        assert!(!allowed_environment(true, true, true));
        assert!(!allowed_environment(false, false, false));
    }
    #[test]
    fn restored_database_cannot_reuse_cloud_cache_after_more_local_edits() {
        let record = SavedRecord {
            device_id: Some("device".into()),
            last_revision: Some(110),
            cache_checkpoint: Some("uploaded-checkpoint".into()),
            ..Default::default()
        };
        assert!(cache_matches_database(
            &record,
            "device",
            Some(111),
            Some("uploaded-checkpoint")
        ));
        // A backup at revision100 plus11new edits reaches111. The monotonic
        // counter alone would accept it, but its checkpoint still identifies
        // the old backup, so every cloud packet must be read before publishing.
        assert!(!cache_matches_database(
            &record,
            "device",
            Some(111),
            Some("backup-checkpoint")
        ));
        assert!(!cache_matches_database(
            &record,
            "device",
            Some(100),
            Some("uploaded-checkpoint")
        ));
        assert!(!cache_matches_database(
            &record,
            "replacement-device",
            Some(111),
            Some("uploaded-checkpoint")
        ));
        assert!(!cache_matches_database(&record, "device", Some(111), None));
    }
}
