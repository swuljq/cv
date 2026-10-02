// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use arboard::Clipboard;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use serde_json::json;
use std::{sync::{atomic::{AtomicBool, AtomicIsize, Ordering}, Mutex, OnceLock}, thread, time::{Duration, Instant}};
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tungstenite::{connect, stream::MaybeTlsStream, Message};
use windows::Win32::{Foundation::{POINT, HWND}, UI::WindowsAndMessaging::{GetCursorPos, GetForegroundWindow, SetForegroundWindow}};

static SYNC_RUNNING: AtomicBool = AtomicBool::new(false);
static TARGET_WINDOW: AtomicIsize = AtomicIsize::new(0);
static CLIPBOARD_HISTORY: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
const HISTORY_SHORTCUT: &str = "CommandOrControl+Alt+Z";
const HISTORY_WINDOW_WIDTH: i32 = 430;
const HISTORY_WINDOW_HEIGHT: i32 = 360;

fn bounded_history_position(cursor_x: i32, cursor_y: i32, area_x: i32, area_y: i32, area_width: u32, area_height: u32) -> (i32, i32) {
    let preferred_x = cursor_x - HISTORY_WINDOW_WIDTH / 2;
    let preferred_y = cursor_y - HISTORY_WINDOW_HEIGHT / 2;
    let max_x = area_x + area_width as i32 - HISTORY_WINDOW_WIDTH;
    let max_y = area_y + area_height as i32 - HISTORY_WINDOW_HEIGHT;
    (preferred_x.clamp(area_x, max_x.max(area_x)), preferred_y.clamp(area_y, max_y.max(area_y)))
}

fn history_store() -> &'static Mutex<Vec<String>> {
    CLIPBOARD_HISTORY.get_or_init(|| Mutex::new(Vec::new()))
}

fn remember_clipboard(value: &str) {
    if value.is_empty() { return; }
    if let Ok(mut history) = history_store().lock() {
        history.retain(|item| item != value);
        history.insert(0, value.to_string());
        history.truncate(10);
    }
}

#[tauri::command]
fn get_clipboard_history() -> Vec<String> {
    history_store().lock().map(|history| history.clone()).unwrap_or_default()
}

#[tauri::command]
fn select_clipboard_history(value: String) -> Result<(), String> {
    let mut clipboard = Clipboard::new().map_err(|error| error.to_string())?;
    clipboard.set_text(value.clone()).map_err(|error| error.to_string())?;
    remember_clipboard(&value);
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

fn auth_message(username: &str, password: &str) -> Message {
    Message::Text(json!({
        "type": "auth",
        "username": username,
        "password": password,
        "deviceId": "windows-tauri",
        "deviceName": "Windows desktop"
    }).to_string())
}

#[tauri::command]
fn start_sync(url: String, username: String, password: String) -> Result<(), String> {
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
            let _ = socket.send(auth_message(&username, &password));
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
                            remember_clipboard(&text);
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
                                if let Some(value) = message["data"].as_str() { if let Some(cb) = clipboard.as_mut() { let _ = cb.set_text(value); last = value.to_string(); } remember_clipboard(value); }
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
    use super::{auth_message, get_clipboard_history, remember_clipboard, HISTORY_SHORTCUT};
    use tungstenite::Message;

    #[test]
    fn auth_message_includes_fixed_device_identity() {
        let Message::Text(raw) = auth_message("user", "secret") else {
            panic!("auth payload must be text");
        };
        let value: serde_json::Value = serde_json::from_str(&raw).expect("valid JSON");
        assert_eq!(value["type"], "auth");
        assert_eq!(value["username"], "user");
        assert_eq!(value["password"], "secret");
        assert_eq!(value["deviceId"], "windows-tauri");
    }

    #[test]
    fn clipboard_history_keeps_latest_ten_items_without_duplicates() {
        for index in 0..12 { remember_clipboard(&format!("item-{index}")); }
        remember_clipboard("item-5");
        let history = get_clipboard_history();
        assert_eq!(history.len(), 10);
        assert_eq!(history[0], "item-5");
        assert!(!history.contains(&"item-0".to_string()));
    }

    #[test]
    fn history_shortcut_uses_control_alt_z() {
        assert_eq!(HISTORY_SHORTCUT, "CommandOrControl+Alt+Z");
    }

    #[test]
    fn history_position_is_kept_inside_monitor_work_area() {
        assert_eq!(super::bounded_history_position(1900, 1050, 0, 0, 1920, 1080), (1490, 720));
        assert_eq!(super::bounded_history_position(20, 20, 0, 0, 1920, 1080), (0, 0));
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            use tauri_plugin_global_shortcut::GlobalShortcutExt;
            let history_window = WebviewWindowBuilder::new(app, "history", WebviewUrl::App("index.html?history".into()))
                .title("ClipBridge 最近复制")
                .inner_size(HISTORY_WINDOW_WIDTH as f64, HISTORY_WINDOW_HEIGHT as f64)
                .resizable(false)
                .transparent(true)
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
        .invoke_handler(tauri::generate_handler![start_sync, get_clipboard_history, select_clipboard_history])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
