// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use arboard::Clipboard;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::HashSet, fs, path::{Path, PathBuf}, sync::{atomic::{AtomicBool, AtomicIsize, Ordering}, Mutex, OnceLock}, thread, time::{Duration, Instant}};
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tungstenite::{connect, stream::MaybeTlsStream, Message};
use windows::Win32::{Foundation::{POINT, HWND}, UI::WindowsAndMessaging::{GetCursorPos, GetForegroundWindow, SetForegroundWindow}};

static SYNC_RUNNING: AtomicBool = AtomicBool::new(false);
static TARGET_WINDOW: AtomicIsize = AtomicIsize::new(0);
static HISTORY_STATE: OnceLock<Mutex<HistoryState>> = OnceLock::new();
const HISTORY_SHORTCUT: &str = "CommandOrControl+Alt+Z";
const HISTORY_WINDOW_WIDTH: i32 = 380;
const HISTORY_WINDOW_HEIGHT: i32 = 320;
const DEFAULT_HISTORY_LIMIT: usize = 10;
const MAX_HISTORY_LIMIT: usize = 1000;
const HISTORY_FILE_NAME: &str = "clipsync-history.json";

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct HistorySettings {
    history_limit: usize,
    history_directory: String,
    #[serde(default)]
    device_id: String,
}

struct HistoryState {
    entries: Vec<String>,
    settings: HistorySettings,
    settings_path: PathBuf,
}

fn bounded_history_position(cursor_x: i32, cursor_y: i32, area_x: i32, area_y: i32, area_width: u32, area_height: u32) -> (i32, i32) {
    let preferred_x = cursor_x - HISTORY_WINDOW_WIDTH / 2;
    let preferred_y = cursor_y - HISTORY_WINDOW_HEIGHT / 2;
    let max_x = area_x + area_width as i32 - HISTORY_WINDOW_WIDTH;
    let max_y = area_y + area_height as i32 - HISTORY_WINDOW_HEIGHT;
    (preferred_x.clamp(area_x, max_x.max(area_x)), preferred_y.clamp(area_y, max_y.max(area_y)))
}

fn validate_history_limit(limit: usize) -> Result<(), String> {
    if (1..=MAX_HISTORY_LIMIT).contains(&limit) { Ok(()) } else { Err(format!("历史记录条数必须在 1 到 {MAX_HISTORY_LIMIT} 之间")) }
}

fn ensure_device_id(settings: &mut HistorySettings) -> bool {
    if !settings.device_id.is_empty() { return false; }
    settings.device_id = format!("desktop-{}", uuid::Uuid::new_v4());
    true
}

fn normalize_history(entries: Vec<String>, limit: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    entries.into_iter().filter(|item| !item.is_empty() && seen.insert(item.clone())).take(limit).collect()
}

fn insert_history(entries: &mut Vec<String>, value: &str, limit: usize) -> bool {
    if value.is_empty() || entries.first().map(String::as_str) == Some(value) { return false; }
    entries.retain(|item| item != value);
    entries.insert(0, value.to_string());
    entries.truncate(limit);
    true
}

fn history_file(directory: &Path) -> PathBuf {
    directory.join(HISTORY_FILE_NAME)
}

fn sidecar_file(path: &Path, suffix: &str) -> PathBuf {
    let name = path.file_name().and_then(|value| value.to_str()).unwrap_or("clipsync-data");
    path.with_file_name(format!("{name}.{suffix}"))
}

fn recover_interrupted_write(path: &Path) -> Result<(), String> {
    if path.exists() { return Ok(()); }
    let backup = sidecar_file(path, "bak");
    let temporary = sidecar_file(path, "tmp");
    if backup.exists() {
        fs::rename(backup, path).map_err(|error| error.to_string())?;
        let _ = fs::remove_file(temporary);
    } else if temporary.exists() {
        fs::rename(temporary, path).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn write_bytes(path: &Path, data: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() { fs::create_dir_all(parent).map_err(|error| error.to_string())?; }
    let temporary = sidecar_file(path, "tmp");
    let backup = sidecar_file(path, "bak");
    fs::write(&temporary, data).map_err(|error| error.to_string())?;
    if !path.exists() { return fs::rename(temporary, path).map_err(|error| error.to_string()); }
    let _ = fs::remove_file(&backup);
    fs::rename(path, &backup).map_err(|error| error.to_string())?;
    match fs::rename(&temporary, path) {
        Ok(()) => {
            let _ = fs::remove_file(backup);
            Ok(())
        }
        Err(error) => {
            let _ = fs::rename(backup, path);
            let _ = fs::remove_file(temporary);
            Err(error.to_string())
        }
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let data = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    write_bytes(path, &data)
}

fn load_history(directory: &Path, limit: usize) -> Result<Vec<String>, String> {
    let path = history_file(directory);
    recover_interrupted_write(&path)?;
    if !path.exists() { return Ok(Vec::new()); }
    let data = fs::read(path).map_err(|error| error.to_string())?;
    let entries = serde_json::from_slice::<Vec<String>>(&data).map_err(|error| format!("历史记录文件格式错误：{error}"))?;
    Ok(normalize_history(entries, limit))
}

fn initialize_history(settings_path: PathBuf, default_directory: PathBuf) -> Result<(), String> {
    fs::create_dir_all(&default_directory).map_err(|error| error.to_string())?;
    recover_interrupted_write(&settings_path)?;
    let settings_existed = settings_path.exists();
    let mut settings = if settings_existed {
        let data = fs::read(&settings_path).map_err(|error| error.to_string())?;
        serde_json::from_slice::<HistorySettings>(&data).map_err(|error| format!("设置文件格式错误：{error}"))?
    } else {
        HistorySettings { history_limit: DEFAULT_HISTORY_LIMIT, history_directory: default_directory.to_string_lossy().into_owned(), device_id: String::new() }
    };
    let mut settings_changed = !settings_existed || ensure_device_id(&mut settings);
    validate_history_limit(settings.history_limit)?;
    let directory = PathBuf::from(&settings.history_directory);
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let canonical_directory = directory.canonicalize().map_err(|error| error.to_string())?.to_string_lossy().into_owned();
    if settings.history_directory != canonical_directory { settings_changed = true; }
    settings.history_directory = canonical_directory;
    let entries = load_history(Path::new(&settings.history_directory), settings.history_limit)?;
    if settings_changed { write_json(&settings_path, &settings)?; }
    HISTORY_STATE.set(Mutex::new(HistoryState { entries, settings, settings_path })).map_err(|_| "历史记录状态已经初始化".to_string())
}

fn history_state() -> Result<&'static Mutex<HistoryState>, String> {
    HISTORY_STATE.get().ok_or_else(|| "历史记录尚未初始化".to_string())
}

fn remember_clipboard(value: &str) -> Result<bool, String> {
    let mut state = history_state()?.lock().map_err(|_| "无法访问历史记录".to_string())?;
    let previous = state.entries.clone();
    let limit = state.settings.history_limit;
    if !insert_history(&mut state.entries, value, limit) { return Ok(false); }
    if let Err(error) = write_json(&history_file(Path::new(&state.settings.history_directory)), &state.entries) {
        state.entries = previous;
        return Err(error);
    }
    Ok(true)
}

#[tauri::command]
fn get_clipboard_history() -> Result<Vec<String>, String> {
    history_state()?.lock().map(|state| state.entries.clone()).map_err(|_| "无法访问历史记录".to_string())
}

#[tauri::command]
fn get_history_settings() -> Result<HistorySettings, String> {
    history_state()?.lock().map(|state| state.settings.clone()).map_err(|_| "无法访问历史记录设置".to_string())
}

fn apply_history_settings(state: &mut HistoryState, history_limit: usize, history_directory: String) -> Result<(HistorySettings, bool), String> {
    validate_history_limit(history_limit)?;
    if history_directory.trim().is_empty() { return Err("请选择历史记录保存目录".to_string()); }
    let directory = PathBuf::from(history_directory);
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let directory = directory.canonicalize().map_err(|error| error.to_string())?;
    let previous_directory = PathBuf::from(&state.settings.history_directory);
    let mut entries = state.entries.clone();
    entries.truncate(history_limit);
    let settings = HistorySettings { history_limit, history_directory: directory.to_string_lossy().into_owned(), device_id: state.settings.device_id.clone() };
    let destination_file = history_file(&directory);
    let destination_previous = match fs::read(&destination_file) {
        Ok(data) => Some(data),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.to_string()),
    };
    write_json(&destination_file, &entries)?;
    if let Err(error) = write_json(&state.settings_path, &settings) {
        if let Some(data) = destination_previous {
            let _ = write_bytes(&destination_file, &data);
        } else {
            let _ = fs::remove_file(&destination_file);
        }
        return Err(error);
    }
    let changed = entries != state.entries;
    state.entries = entries;
    state.settings = settings.clone();
    if previous_directory != directory {
        match fs::remove_file(history_file(&previous_directory)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {}
        }
    }
    Ok((settings, changed))
}

#[tauri::command]
fn save_history_settings(app: tauri::AppHandle, history_limit: usize, history_directory: String) -> Result<HistorySettings, String> {
    let mut state = history_state()?.lock().map_err(|_| "无法访问历史记录设置".to_string())?;
    let (settings, changed) = apply_history_settings(&mut state, history_limit, history_directory)?;
    drop(state);
    if changed { let _ = app.emit("clipboard-history-changed", ()); }
    Ok(settings)
}

#[tauri::command]
fn hide_history<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
    let window = app.get_webview_window("history").ok_or_else(|| "history window not found".to_string())?;
    window.hide().map_err(|error| error.to_string())
}

#[tauri::command]
fn select_clipboard_history(app: tauri::AppHandle, value: String) -> Result<(), String> {
    let mut clipboard = Clipboard::new().map_err(|error| error.to_string())?;
    clipboard.set_text(value.clone()).map_err(|error| error.to_string())?;
    if remember_clipboard(&value)? { let _ = app.emit("clipboard-history-changed", ()); }
    let target = TARGET_WINDOW.load(Ordering::Acquire);
    if target != 0 {
        unsafe {
            let _ = SetForegroundWindow(HWND(target as *mut _));
        }
        thread::sleep(Duration::from_millis(80));
        let mut enigo = Enigo::new(&Settings::default()).map_err(|error| error.to_string())?;
        enigo.key(Key::Control, Direction::Press).map_err(|error| error.to_string())?;
        enigo.key(Key::V, Direction::Click).map_err(|error| error.to_string())?;
        enigo.key(Key::Control, Direction::Release).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn show_history_shortcut<R: tauri::Runtime>(app: &tauri::AppHandle<R>, _: &tauri_plugin_global_shortcut::Shortcut, event: tauri_plugin_global_shortcut::ShortcutEvent) {
    if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
        unsafe { TARGET_WINDOW.store(GetForegroundWindow().0 as isize, Ordering::Release); }
        if let Some(window) = app.get_webview_window("history") {
            let mut point = POINT::default();
            unsafe { let _ = GetCursorPos(&mut point); }
            let position = window.monitor_from_point(point.x as f64, point.y as f64)
                .ok()
                .flatten()
                .map(|monitor| {
                    let work_area = monitor.work_area();
                    bounded_history_position(
                        point.x,
                        point.y,
                        work_area.position.x,
                        work_area.position.y,
                        work_area.size.width,
                        work_area.size.height,
                    )
                })
                .unwrap_or((point.x - HISTORY_WINDOW_WIDTH / 2, point.y - HISTORY_WINDOW_HEIGHT / 2));
            let _ = window.set_position(tauri::PhysicalPosition::new(position.0, position.1));
            let _ = window.show();
            let _ = window.set_focus();
        }
        let _ = app.emit("clipboard-history-open", ());
    }
}

fn auth_message(username: &str, password: &str, device_id: &str) -> Message {
    Message::Text(json!({
        "type": "auth",
        "username": username,
        "password": password,
        "deviceId": device_id,
        "deviceName": "ClipSync desktop"
    }).to_string())
}

#[tauri::command]
fn start_sync(app: tauri::AppHandle, url: String, username: String, password: String) -> Result<(), String> {
    let device_id = history_state()?.lock().map_err(|_| "无法访问设备设置".to_string())?.settings.device_id.clone();
    if SYNC_RUNNING.swap(true, Ordering::AcqRel) {
        return Ok(());
    }
    thread::Builder::new().name("clipbridge-sync".into()).spawn(move || {
        let mut clipboard = Clipboard::new().ok();
        let mut last = String::new();
        let mut retry_delay = Duration::from_secs(1);
        'reconnect: loop {
            let Ok((mut socket, _)) = connect(&url) else {
                thread::sleep(retry_delay);
                retry_delay = (retry_delay * 2).min(Duration::from_secs(30));
                continue;
            };
            retry_delay = Duration::from_secs(1);
            let _ = socket.send(auth_message(&username, &password, &device_id));
            match socket.get_mut() {
                MaybeTlsStream::Plain(stream) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
                }
                #[allow(unreachable_patterns)]
                _ => {}
            }
            let mut last_ping = Instant::now();
            loop {
                if last_ping.elapsed() >= Duration::from_secs(20) {
                    if socket.send(Message::Ping(Vec::new().into())).is_err() {
                        continue 'reconnect;
                    }
                    last_ping = Instant::now();
                }
                if let Some(cb) = clipboard.as_mut() {
                    if let Ok(text) = cb.get_text() {
                        if !text.is_empty() && text != last {
                            last = text.clone();
                            match remember_clipboard(&text) {
                                Ok(true) => { let _ = app.emit("clipboard-history-changed", ()); }
                                Err(error) => eprintln!("failed to persist clipboard history: {error}"),
                                Ok(false) => {}
                            }
                            let event_id = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|v| v.as_millis()).unwrap_or_default();
                            if socket.send(Message::Text(json!({"type":"clipboard","eventId":format!("tauri-{event_id}"),"contentType":"text","data":text}).to_string())).is_err() {
                                continue 'reconnect;
                            }
                        }
                    }
                }
                match socket.read() {
                    Ok(Message::Text(raw)) => if let Ok(message) = serde_json::from_str::<serde_json::Value>(&raw) {
                        if message["type"] == "clipboard" && message["contentType"] == "text" {
                                if let Some(value) = message["data"].as_str() {
                                    if let Some(cb) = clipboard.as_mut() { let _ = cb.set_text(value); last = value.to_string(); }
                                    match remember_clipboard(value) {
                                        Ok(true) => { let _ = app.emit("clipboard-history-changed", ()); }
                                        Err(error) => eprintln!("failed to persist clipboard history: {error}"),
                                        Ok(false) => {}
                                    }
                                }
                        }
                    },
                    Err(tungstenite::Error::Io(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {},
                    Err(_) => continue 'reconnect,
                    _ => {}
                }
                thread::sleep(Duration::from_millis(400));
            }
        }
    }).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{apply_history_settings, auth_message, ensure_device_id, history_file, insert_history, load_history, sidecar_file, validate_history_limit, write_json, HistorySettings, HistoryState, HISTORY_SHORTCUT};
    use std::{fs, path::PathBuf, time::{SystemTime, UNIX_EPOCH}};
    use tungstenite::Message;

    fn temp_directory(name: &str) -> PathBuf {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).expect("time after epoch").as_nanos();
        let path = std::env::temp_dir().join(format!("clipsync-{name}-{}-{unique}", std::process::id()));
        fs::create_dir_all(&path).expect("create temp directory");
        path
    }

    #[test]
    fn auth_message_includes_persisted_device_identity() {
        let Message::Text(raw) = auth_message("user", "secret", "desktop-test") else {
            panic!("auth payload must be text");
        };
        let value: serde_json::Value = serde_json::from_str(&raw).expect("valid JSON");
        assert_eq!(value["type"], "auth");
        assert_eq!(value["username"], "user");
        assert_eq!(value["password"], "secret");
        assert_eq!(value["deviceId"], "desktop-test");
        assert_eq!(value["deviceName"], "ClipSync desktop");
    }

    #[test]
    fn legacy_settings_receive_a_stable_unique_device_id() {
        let mut first: HistorySettings = serde_json::from_str(r#"{"historyLimit":10,"historyDirectory":"history"}"#).expect("legacy settings");
        let mut second = first.clone();
        assert!(ensure_device_id(&mut first));
        assert!(ensure_device_id(&mut second));
        assert!(first.device_id.starts_with("desktop-"));
        assert_ne!(first.device_id, second.device_id);
        let device_id = first.device_id.clone();
        assert!(!ensure_device_id(&mut first));
        assert_eq!(first.device_id, device_id);
    }

    #[test]
    fn clipboard_history_respects_limit_and_removes_duplicates() {
        let mut history = Vec::new();
        for index in 0..12 { insert_history(&mut history, &format!("item-{index}"), 10); }
        insert_history(&mut history, "item-5", 10);
        assert_eq!(history.len(), 10);
        assert_eq!(history[0], "item-5");
        assert!(!history.contains(&"item-0".to_string()));
        assert!(!insert_history(&mut history, "item-5", 10));
        assert!(insert_history(&mut history, "new-item", 10));
    }

    #[test]
    fn history_limit_must_be_between_one_and_one_thousand() {
        assert!(validate_history_limit(1).is_ok());
        assert!(validate_history_limit(1000).is_ok());
        assert!(validate_history_limit(0).is_err());
        assert!(validate_history_limit(1001).is_err());
    }

    #[test]
    fn history_file_round_trip_normalizes_entries() {
        let directory = temp_directory("round-trip");
        write_json(&history_file(&directory), &vec!["new".to_string(), "new".to_string(), "".to_string(), "old".to_string()]).expect("write history");
        let history = load_history(&directory, 10).expect("load history");
        assert_eq!(history, vec!["new", "old"]);
        assert!(!sidecar_file(&history_file(&directory), "tmp").exists());
        assert!(!sidecar_file(&history_file(&directory), "bak").exists());
        fs::remove_dir_all(directory).expect("remove temp directory");
    }

    #[test]
    fn changing_directory_migrates_and_trims_history() {
        let root = temp_directory("migration");
        let old_directory = root.join("old");
        let new_directory = root.join("new");
        fs::create_dir_all(&old_directory).expect("create old directory");
        write_json(&history_file(&old_directory), &vec!["two".to_string(), "one".to_string()]).expect("write old history");
        let mut state = HistoryState {
            entries: vec!["two".to_string(), "one".to_string()],
            settings: HistorySettings { history_limit: 10, history_directory: old_directory.to_string_lossy().into_owned(), device_id: "desktop-test".to_string() },
            settings_path: root.join("settings.json"),
        };
        let (settings, changed) = apply_history_settings(&mut state, 1, new_directory.to_string_lossy().into_owned()).expect("migrate history");
        assert!(changed);
        assert_eq!(settings.history_limit, 1);
        assert_eq!(settings.device_id, "desktop-test");
        assert_eq!(load_history(PathBuf::from(settings.history_directory).as_path(), 10).expect("load migrated history"), vec!["two"]);
        assert!(!history_file(&old_directory).exists());
        fs::remove_dir_all(root).expect("remove temp directory");
    }

    #[test]
    fn failed_migration_restores_existing_destination_history() {
        let root = temp_directory("migration-rollback");
        let old_directory = root.join("old");
        let new_directory = root.join("new");
        fs::create_dir_all(&old_directory).expect("create old directory");
        fs::create_dir_all(&new_directory).expect("create new directory");
        write_json(&history_file(&old_directory), &vec!["current".to_string()]).expect("write old history");
        write_json(&history_file(&new_directory), &vec!["destination".to_string()]).expect("write destination history");
        let blocked_parent = root.join("blocked");
        fs::write(&blocked_parent, b"not a directory").expect("create blocking file");
        let mut state = HistoryState {
            entries: vec!["current".to_string()],
            settings: HistorySettings { history_limit: 10, history_directory: old_directory.to_string_lossy().into_owned(), device_id: "desktop-test".to_string() },
            settings_path: blocked_parent.join("settings.json"),
        };
        assert!(apply_history_settings(&mut state, 10, new_directory.to_string_lossy().into_owned()).is_err());
        assert_eq!(load_history(&new_directory, 10).expect("load restored destination"), vec!["destination"]);
        assert_eq!(state.entries, vec!["current"]);
        assert_eq!(state.settings.history_directory, old_directory.to_string_lossy());
        fs::remove_dir_all(root).expect("remove temp directory");
    }

    #[test]
    fn history_shortcut_uses_control_alt_z() {
        assert_eq!(HISTORY_SHORTCUT, "CommandOrControl+Alt+Z");
    }

    #[test]
    fn history_position_is_kept_inside_monitor_work_area() {
        assert_eq!(super::bounded_history_position(1900, 1050, 0, 0, 1920, 1080), (1540, 760));
        assert_eq!(super::bounded_history_position(20, 20, 0, 0, 1920, 1080), (0, 0));
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            use tauri_plugin_global_shortcut::GlobalShortcutExt;
            let settings_path = app.path().app_config_dir()?.join("settings.json");
            let default_directory = app.path().app_data_dir()?;
            initialize_history(settings_path, default_directory).map_err(std::io::Error::other)?;
            let history_window = WebviewWindowBuilder::new(app, "history", WebviewUrl::App("index.html?history".into()))
                .title("ClipSync 最近复制")
                .inner_size(HISTORY_WINDOW_WIDTH as f64, HISTORY_WINDOW_HEIGHT as f64)
                .resizable(false)
                .decorations(false)
                .transparent(true)
                .always_on_top(true)
                .visible(false)
                .build()?;
            let history_window_handle = history_window.clone();
            history_window.on_window_event(move |event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = history_window_handle.hide();
                }
            });
            app.global_shortcut().on_shortcut(HISTORY_SHORTCUT, show_history_shortcut)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![start_sync, get_clipboard_history, get_history_settings, save_history_settings, hide_history, select_clipboard_history])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
