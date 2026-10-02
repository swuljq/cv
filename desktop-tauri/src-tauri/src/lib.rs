// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use arboard::Clipboard;
use serde_json::json;
use std::{thread, time::Duration};
use tungstenite::{connect, stream::MaybeTlsStream, Message};

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
    thread::Builder::new().name("clipbridge-sync".into()).spawn(move || {
        let Ok((mut socket, _)) = connect(&url) else { return; };
        let _ = socket.send(auth_message(&username, &password));
        match socket.get_mut() {
            MaybeTlsStream::Plain(stream) => {
                let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
            }
            #[allow(unreachable_patterns)]
            _ => {}
        }
        let mut clipboard = Clipboard::new().ok();
        let mut last = String::new();
        loop {
            if let Some(cb) = clipboard.as_mut() {
                if let Ok(text) = cb.get_text() {
                    if !text.is_empty() && text != last {
                        last = text.clone();
                        let event_id = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|v| v.as_millis()).unwrap_or_default();
                        let _ = socket.send(Message::Text(json!({"type":"clipboard","eventId":format!("tauri-{event_id}"),"contentType":"text","data":text}).to_string()));
                    }
                }
            }
            match socket.read() {
                Ok(Message::Text(raw)) => if let Ok(message) = serde_json::from_str::<serde_json::Value>(&raw) {
                    if message["type"] == "clipboard" && message["contentType"] == "text" {
                        if let Some(value) = message["data"].as_str() { if let Some(cb) = clipboard.as_mut() { let _ = cb.set_text(value); last = value.to_string(); } }
                    }
                },
                Err(tungstenite::Error::Io(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {},
                Err(_) => break,
                _ => {}
            }
            thread::sleep(Duration::from_millis(400));
        }
    }).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::auth_message;
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
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![start_sync])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
