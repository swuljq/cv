mod image_sync;

use arboard::Clipboard;
use base64::{engine::general_purpose::STANDARD, Engine};
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use image_sync::NormalizedImage;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::HashSet, fs, path::{Path, PathBuf}, sync::{atomic::{AtomicBool, AtomicIsize, Ordering}, Mutex, OnceLock}, thread, time::{Duration, Instant}};
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tungstenite::{connect, stream::MaybeTlsStream, Message};
use windows::Win32::{Foundation::{POINT, HWND}, UI::WindowsAndMessaging::{GetCursorPos, GetForegroundWindow, SetForegroundWindow}};

static SYNC_RUNNING: AtomicBool = AtomicBool::new(false);
static TARGET_WINDOW: AtomicIsize = AtomicIsize::new(0);
static HISTORY_STATE: OnceLock<Mutex<HistoryState>> = OnceLock::new();
static HISTORY_IO: Mutex<()> = Mutex::new(());
const HISTORY_SHORTCUT: &str = "CommandOrControl+Alt+Z";
const HISTORY_WINDOW_WIDTH: i32 = 380;
const HISTORY_WINDOW_HEIGHT: i32 = 320;
const DEFAULT_HISTORY_LIMIT: usize = 10;
const MAX_HISTORY_LIMIT: usize = 1000;
const HISTORY_FILE_NAME: &str = "clipsync-history.json";
const HISTORY_SCHEMA_VERSION: u8 = 2;
const IMAGE_DIRECTORY_NAME: &str = "clipsync-images";
const THUMBNAIL_DIRECTORY_NAME: &str = "clipsync-thumbnails";

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct HistorySettings {
    history_limit: usize,
    history_directory: String,
    #[serde(default)]
    device_id: String,
}

#[derive(Clone)]
struct HistoryState {
    entries: Vec<HistoryEntry>,
    settings: HistorySettings,
    settings_path: PathBuf,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum HistoryKind {
    Text,
    Image,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryEntry {
    id: String,
    kind: HistoryKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    file_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    thumbnail_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    byte_length: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    height: Option<u32>,
    created_at: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct HistoryDocument {
    version: u8,
    entries: Vec<HistoryEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum StoredHistory {
    Document(HistoryDocument),
    Legacy(Vec<String>),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryItem {
    id: String,
    kind: HistoryKind,
    text: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    created_at: String,
    available: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum ClipboardFingerprint {
    Text(String),
    Image(String),
}

fn should_process_clipboard(last: &mut Option<ClipboardFingerprint>, current: ClipboardFingerprint) -> bool {
    if last.as_ref() == Some(&current) { return false; }
    *last = Some(current);
    true
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

fn now_timestamp() -> String {
    OffsetDateTime::now_utc().format(&Rfc3339).unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

fn text_entry(value: String) -> HistoryEntry {
    HistoryEntry {
        id: uuid::Uuid::new_v4().to_string(),
        kind: HistoryKind::Text,
        text: Some(value),
        file_name: None,
        thumbnail_name: None,
        sha256: None,
        byte_length: None,
        width: None,
        height: None,
        created_at: now_timestamp(),
    }
}

fn image_entry(image: &NormalizedImage) -> HistoryEntry {
    let id = uuid::Uuid::new_v4().to_string();
    HistoryEntry {
        file_name: Some(format!("{id}.png")),
        thumbnail_name: Some(format!("{id}.png")),
        id,
        kind: HistoryKind::Image,
        text: None,
        sha256: Some(image.sha256.clone()),
        byte_length: Some(image.png.len()),
        width: Some(image.width),
        height: Some(image.height),
        created_at: now_timestamp(),
    }
}

fn entry_key(entry: &HistoryEntry) -> Option<String> {
    match entry.kind {
        HistoryKind::Text => entry.text.as_ref().map(|value| format!("text:{value}")),
        HistoryKind::Image => entry.sha256.as_ref().map(|value| format!("image:{value}")),
    }
}

fn validate_history_entry(entry: &HistoryEntry) -> Result<(), String> {
    let id = uuid::Uuid::parse_str(&entry.id).map_err(|_| "历史记录 ID 无效".to_string())?;
    match entry.kind {
        HistoryKind::Text => {
            if entry.text.as_deref().is_none_or(str::is_empty) { return Err("文字历史内容无效".to_string()); }
        }
        HistoryKind::Image => {
            let expected = format!("{id}.png");
            if entry.file_name.as_deref() != Some(expected.as_str()) || entry.thumbnail_name.as_deref() != Some(expected.as_str()) {
                return Err("图片历史文件名无效".to_string());
            }
            let sha = entry.sha256.as_deref().ok_or_else(|| "图片历史指纹缺失".to_string())?;
            if sha.len() != 64 || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) { return Err("图片历史指纹无效".to_string()); }
            if entry.byte_length.is_none_or(|length| length > image_sync::MAX_PNG_BYTES) { return Err("图片历史大小无效".to_string()); }
            image_sync::validate_dimensions(entry.width.ok_or_else(|| "图片宽度缺失".to_string())?, entry.height.ok_or_else(|| "图片高度缺失".to_string())?)?;
        }
    }
    Ok(())
}

fn normalize_history(entries: Vec<HistoryEntry>, limit: usize) -> Vec<HistoryEntry> {
    let mut seen = HashSet::new();
    entries.into_iter().filter(|entry| entry_key(entry).is_some_and(|key| seen.insert(key))).take(limit).collect()
}

fn insert_entry(entries: &mut Vec<HistoryEntry>, entry: HistoryEntry, limit: usize) -> (bool, Vec<HistoryEntry>) {
    let Some(key) = entry_key(&entry) else { return (false, Vec::new()); };
    if entries.first().and_then(entry_key).as_deref() == Some(key.as_str()) { return (false, Vec::new()); }
    entries.retain(|item| entry_key(item).as_deref() != Some(key.as_str()));
    entries.insert(0, entry);
    let removed = if entries.len() > limit { entries.split_off(limit) } else { Vec::new() };
    (true, removed)
}

fn history_file(directory: &Path) -> PathBuf {
    directory.join(HISTORY_FILE_NAME)
}

fn image_directory(directory: &Path) -> PathBuf {
    directory.join(IMAGE_DIRECTORY_NAME)
}

fn thumbnail_directory(directory: &Path) -> PathBuf {
    directory.join(THUMBNAIL_DIRECTORY_NAME)
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

fn write_history(directory: &Path, entries: &[HistoryEntry]) -> Result<(), String> {
    write_json(&history_file(directory), &HistoryDocument { version: HISTORY_SCHEMA_VERSION, entries: entries.to_vec() })
}

fn cleanup_orphan_images(directory: &Path, entries: &[HistoryEntry]) {
    let referenced_images: HashSet<&str> = entries.iter().filter_map(|entry| entry.file_name.as_deref()).collect();
    let referenced_thumbnails: HashSet<&str> = entries.iter().filter_map(|entry| entry.thumbnail_name.as_deref()).collect();
    for (root, referenced) in [(image_directory(directory), referenced_images), (thumbnail_directory(directory), referenced_thumbnails)] {
        let Ok(files) = fs::read_dir(root) else { continue; };
        for file in files.flatten() {
            let path = file.path();
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else { continue; };
            let valid_name = path.extension().and_then(|value| value.to_str()).is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
                && path.file_stem().and_then(|value| value.to_str()).is_some_and(|stem| uuid::Uuid::parse_str(stem).is_ok());
            if valid_name && !referenced.contains(name) { let _ = fs::remove_file(path); }
        }
    }
}

fn load_history(directory: &Path, limit: usize) -> Result<(Vec<HistoryEntry>, bool), String> {
    let path = history_file(directory);
    recover_interrupted_write(&path)?;
    if !path.exists() { return Ok((Vec::new(), false)); }
    let data = fs::read(path).map_err(|error| error.to_string())?;
    let stored = serde_json::from_slice::<StoredHistory>(&data).map_err(|error| format!("历史记录文件格式错误：{error}"))?;
    match stored {
        StoredHistory::Document(document) if document.version == HISTORY_SCHEMA_VERSION => {
            for entry in &document.entries { validate_history_entry(entry)?; }
            let entries = normalize_history(document.entries.clone(), limit);
            let changed = entries != document.entries;
            Ok((entries, changed))
        }
        StoredHistory::Document(document) => Err(format!("不支持的历史记录版本：{}", document.version)),
        StoredHistory::Legacy(values) => {
            let entries = values.into_iter().filter(|value| !value.is_empty()).map(text_entry).collect();
            Ok((normalize_history(entries, limit), true))
        }
    }
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
    let directory = Path::new(&settings.history_directory);
    let (entries, history_changed) = load_history(directory, settings.history_limit)?;
    if history_changed { write_history(directory, &entries)?; }
    cleanup_orphan_images(directory, &entries);
    if settings_changed { write_json(&settings_path, &settings)?; }
    HISTORY_STATE.set(Mutex::new(HistoryState { entries, settings, settings_path })).map_err(|_| "历史记录状态已经初始化".to_string())
}

fn history_state() -> Result<&'static Mutex<HistoryState>, String> {
    HISTORY_STATE.get().ok_or_else(|| "历史记录尚未初始化".to_string())
}

fn remove_entry_files(directory: &Path, entries: &[HistoryEntry]) {
    for entry in entries.iter().filter(|entry| entry.kind == HistoryKind::Image) {
        if let Some(name) = entry.file_name.as_deref() { let _ = fs::remove_file(image_directory(directory).join(name)); }
        if let Some(name) = entry.thumbnail_name.as_deref() { let _ = fs::remove_file(thumbnail_directory(directory).join(name)); }
    }
}

fn remember_text(value: &str) -> Result<bool, String> {
    if value.is_empty() { return Ok(false); }
    let _io = HISTORY_IO.lock().map_err(|_| "无法写入历史记录".to_string())?;
    let (mut entries, limit, directory) = {
        let state = history_state()?.lock().map_err(|_| "无法访问历史记录".to_string())?;
        (state.entries.clone(), state.settings.history_limit, PathBuf::from(&state.settings.history_directory))
    };
    let (changed, removed) = insert_entry(&mut entries, text_entry(value.to_string()), limit);
    if !changed { return Ok(false); }
    write_history(&directory, &entries)?;
    remove_entry_files(&directory, &removed);
    history_state()?.lock().map_err(|_| "无法访问历史记录".to_string())?.entries = entries;
    Ok(true)
}

fn save_image_files(directory: &Path, entry: &HistoryEntry, image: &NormalizedImage) -> Result<(), String> {
    let image_name = entry.file_name.as_deref().ok_or_else(|| "图片文件名缺失".to_string())?;
    let thumbnail_name = entry.thumbnail_name.as_deref().ok_or_else(|| "缩略图文件名缺失".to_string())?;
    write_bytes(&image_directory(directory).join(image_name), &image.png)?;
    let _ = write_bytes(&thumbnail_directory(directory).join(thumbnail_name), &image.thumbnail);
    Ok(())
}

fn remember_image(image: &NormalizedImage) -> Result<bool, String> {
    let _io = HISTORY_IO.lock().map_err(|_| "无法写入历史记录".to_string())?;
    let (mut entries, limit, directory) = {
        let state = history_state()?.lock().map_err(|_| "无法访问历史记录".to_string())?;
        if state.entries.first().and_then(|entry| entry.sha256.as_deref()) == Some(image.sha256.as_str()) { return Ok(false); }
        (state.entries.clone(), state.settings.history_limit, PathBuf::from(&state.settings.history_directory))
    };
    let existing = entries.iter().find(|entry| entry.sha256.as_deref() == Some(image.sha256.as_str())).cloned();
    let entry = existing.unwrap_or_else(|| image_entry(image));
    let is_new = !entries.iter().any(|item| item.id == entry.id);
    if is_new { save_image_files(&directory, &entry, image)?; }
    let (changed, removed) = insert_entry(&mut entries, entry.clone(), limit);
    if !changed { return Ok(false); }
    if let Err(error) = write_history(&directory, &entries) {
        if is_new { remove_entry_files(&directory, &[entry]); }
        return Err(error);
    }
    remove_entry_files(&directory, &removed);
    history_state()?.lock().map_err(|_| "无法访问历史记录".to_string())?.entries = entries;
    Ok(true)
}

#[tauri::command]
fn get_clipboard_history() -> Result<Vec<HistoryItem>, String> {
    history_state()?.lock().map(|state| state.entries.iter().map(|entry| HistoryItem {
        id: entry.id.clone(),
        kind: entry.kind,
        text: entry.text.clone(),
        width: entry.width,
        height: entry.height,
        created_at: entry.created_at.clone(),
        available: entry.kind == HistoryKind::Text || entry.file_name.as_deref().is_some_and(|name| image_directory(Path::new(&state.settings.history_directory)).join(name).is_file()),
    }).collect()).map_err(|_| "无法访问历史记录".to_string())
}

#[tauri::command]
fn get_history_thumbnail(id: String) -> Result<String, String> {
    uuid::Uuid::parse_str(&id).map_err(|_| "图片记录 ID 无效".to_string())?;
    let _io = HISTORY_IO.lock().map_err(|_| "无法读取图片历史".to_string())?;
    let (path, full_path) = {
        let state = history_state()?.lock().map_err(|_| "无法访问历史记录".to_string())?;
        let entry = state.entries.iter().find(|entry| entry.id == id && entry.kind == HistoryKind::Image).ok_or_else(|| "找不到图片记录".to_string())?;
        let thumbnail_name = entry.thumbnail_name.as_deref().ok_or_else(|| "缩略图文件名缺失".to_string())?;
        let image_name = entry.file_name.as_deref().ok_or_else(|| "图片文件名缺失".to_string())?;
        let directory = PathBuf::from(&state.settings.history_directory);
        (thumbnail_directory(&directory).join(thumbnail_name), image_directory(&directory).join(image_name))
    };
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let full = fs::read(full_path).map_err(|error| error.to_string())?;
            let image = image_sync::normalize_encoded(&full)?;
            write_bytes(&path, &image.thumbnail)?;
            image.thumbnail
        }
        Err(error) => return Err(error.to_string()),
    };
    Ok(STANDARD.encode(bytes))
}

#[tauri::command]
fn get_history_settings() -> Result<HistorySettings, String> {
    history_state()?.lock().map(|state| state.settings.clone()).map_err(|_| "无法访问历史记录设置".to_string())
}

struct MigratedFile {
    destination: PathBuf,
    backup: Option<PathBuf>,
}

fn write_migrated_file(destination: PathBuf, data: &[u8]) -> Result<MigratedFile, String> {
    if let Some(parent) = destination.parent() { fs::create_dir_all(parent).map_err(|error| error.to_string())?; }
    let backup = sidecar_file(&destination, "migration-bak");
    if !destination.exists() && backup.exists() {
        fs::rename(&backup, &destination).map_err(|error| error.to_string())?;
    }
    let _ = fs::remove_file(&backup);
    let backup = if destination.exists() {
        fs::rename(&destination, &backup).map_err(|error| error.to_string())?;
        Some(backup)
    } else {
        None
    };
    if let Err(error) = write_bytes(&destination, data) {
        if let Some(path) = backup.as_ref() {
            if let Err(restore) = fs::rename(path, &destination) { return Err(format!("{error}；恢复迁移备份失败：{restore}")); }
        }
        return Err(error);
    }
    Ok(MigratedFile { destination, backup })
}

fn rollback_migrated_files(files: Vec<MigratedFile>) -> Result<(), String> {
    let mut errors = Vec::new();
    for file in files.into_iter().rev() {
        if let Err(error) = fs::remove_file(&file.destination) {
            if error.kind() != std::io::ErrorKind::NotFound { errors.push(error.to_string()); }
        }
        if let Some(backup) = file.backup {
            if let Err(error) = fs::rename(backup, file.destination) { errors.push(error.to_string()); }
        }
    }
    if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
}

fn finish_migrated_files(files: &[MigratedFile]) {
    for file in files { if let Some(backup) = file.backup.as_ref() { let _ = fs::remove_file(backup); } }
}

fn migration_error(error: String, files: Vec<MigratedFile>) -> String {
    match rollback_migrated_files(files) {
        Ok(()) => error,
        Err(rollback) => format!("{error}；回滚失败：{rollback}"),
    }
}

fn apply_history_settings(state: &mut HistoryState, history_limit: usize, history_directory: String) -> Result<(HistorySettings, bool), String> {
    validate_history_limit(history_limit)?;
    if history_directory.trim().is_empty() { return Err("请选择历史记录保存目录".to_string()); }
    let directory = PathBuf::from(history_directory);
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let directory = directory.canonicalize().map_err(|error| error.to_string())?;
    let previous_directory = PathBuf::from(&state.settings.history_directory);
    if previous_directory != directory && (directory.starts_with(&previous_directory) || previous_directory.starts_with(&directory)) {
        return Err("新旧历史记录目录不能互相嵌套".to_string());
    }
    let mut entries = state.entries.clone();
    let removed = if entries.len() > history_limit { entries.split_off(history_limit) } else { Vec::new() };
    let settings = HistorySettings { history_limit, history_directory: directory.to_string_lossy().into_owned(), device_id: state.settings.device_id.clone() };
    let mut copied_files: Vec<MigratedFile> = Vec::new();
    if previous_directory != directory {
        for entry in entries.iter().filter(|entry| entry.kind == HistoryKind::Image) {
            let image_name = entry.file_name.as_deref().ok_or_else(|| "图片历史文件名缺失".to_string())?;
            let thumbnail_name = entry.thumbnail_name.as_deref().ok_or_else(|| "缩略图历史文件名缺失".to_string())?;
            let full_data = match fs::read(image_directory(&previous_directory).join(image_name)) {
                Ok(data) => data,
                Err(error) => return Err(migration_error(error.to_string(), copied_files)),
            };
            let thumbnail_data = match fs::read(thumbnail_directory(&previous_directory).join(thumbnail_name)) {
                Ok(data) => data,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => match image_sync::normalize_encoded(&full_data) {
                    Ok(image) => image.thumbnail,
                    Err(error) => return Err(migration_error(error, copied_files)),
                },
                Err(error) => return Err(migration_error(error.to_string(), copied_files)),
            };
            for (destination_root, name, data) in [
                (image_directory(&directory), image_name, full_data),
                (thumbnail_directory(&directory), thumbnail_name, thumbnail_data),
            ] {
                let destination = destination_root.join(name);
                match write_migrated_file(destination, &data) {
                    Ok(file) => copied_files.push(file),
                    Err(error) => return Err(migration_error(error, copied_files)),
                }
            }
        }
    }
    let history_data = serde_json::to_vec_pretty(&HistoryDocument { version: HISTORY_SCHEMA_VERSION, entries: entries.clone() }).map_err(|error| error.to_string())?;
    match write_migrated_file(history_file(&directory), &history_data) {
        Ok(file) => copied_files.push(file),
        Err(error) => return Err(migration_error(error, copied_files)),
    }
    if let Err(error) = write_json(&state.settings_path, &settings) { return Err(migration_error(error, copied_files)); }
    finish_migrated_files(&copied_files);
    let changed = entries != state.entries;
    state.entries = entries;
    state.settings = settings.clone();
    if previous_directory != directory {
        match fs::remove_file(history_file(&previous_directory)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {}
        }
        let _ = fs::remove_dir_all(image_directory(&previous_directory));
        let _ = fs::remove_dir_all(thumbnail_directory(&previous_directory));
    } else {
        remove_entry_files(&directory, &removed);
    }
    Ok((settings, changed))
}

#[tauri::command]
fn save_history_settings(app: tauri::AppHandle, history_limit: usize, history_directory: String) -> Result<HistorySettings, String> {
    let _io = HISTORY_IO.lock().map_err(|_| "无法写入历史记录设置".to_string())?;
    let mut next = history_state()?.lock().map_err(|_| "无法访问历史记录设置".to_string())?.clone();
    let (settings, changed) = apply_history_settings(&mut next, history_limit, history_directory)?;
    let mut state = history_state()?.lock().map_err(|_| "无法访问历史记录设置".to_string())?;
    state.entries = next.entries;
    state.settings = next.settings;
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
fn select_clipboard_history(app: tauri::AppHandle, id: String) -> Result<(), String> {
    uuid::Uuid::parse_str(&id).map_err(|_| "历史记录 ID 无效".to_string())?;
    let _io = HISTORY_IO.lock().map_err(|_| "无法读取历史记录".to_string())?;
    let (entry, directory) = {
        let state = history_state()?.lock().map_err(|_| "无法访问历史记录".to_string())?;
        let entry = state.entries.iter().find(|entry| entry.id == id).cloned().ok_or_else(|| "找不到历史记录".to_string())?;
        (entry, PathBuf::from(&state.settings.history_directory))
    };
    let is_image = entry.kind == HistoryKind::Image;
    let mut clipboard = Clipboard::new().map_err(|error| error.to_string())?;
    match entry.kind {
        HistoryKind::Text => clipboard.set_text(entry.text.ok_or_else(|| "文字历史内容缺失".to_string())?).map_err(|error| error.to_string())?,
        HistoryKind::Image => {
            let name = entry.file_name.as_deref().ok_or_else(|| "图片历史文件名缺失".to_string())?;
            let bytes = fs::read(image_directory(&directory).join(name)).map_err(|error| error.to_string())?;
            let image = image_sync::normalize_encoded(&bytes)?;
            clipboard.set_image(image_sync::clipboard_data(&image)?).map_err(|error| error.to_string())?;
        }
    }
    drop(clipboard);
    let changed = {
        let (mut entries, history_directory) = {
            let state = history_state()?.lock().map_err(|_| "无法访问历史记录".to_string())?;
            (state.entries.clone(), state.settings.history_directory.clone())
        };
        let index = entries.iter().position(|entry| entry.id == id).ok_or_else(|| "找不到历史记录".to_string())?;
        if index == 0 {
            false
        } else {
            let selected = entries.remove(index);
            entries.insert(0, selected);
            write_history(Path::new(&history_directory), &entries)?;
            history_state()?.lock().map_err(|_| "无法访问历史记录".to_string())?.entries = entries;
            true
        }
    };
    if changed { let _ = app.emit("clipboard-history-changed", ()); }
    let target = TARGET_WINDOW.load(Ordering::Acquire);
    if target != 0 {
        unsafe {
            let _ = SetForegroundWindow(HWND(target as *mut _));
        }
        thread::sleep(if is_image { Duration::from_millis(180) } else { Duration::from_millis(80) });
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
        let mut last_fingerprint: Option<ClipboardFingerprint> = None;
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
                if clipboard.is_some() {
                    let cb = clipboard.as_mut().expect("clipboard checked above");
                    match image_sync::read_clipboard_image(cb) {
                        Some(Ok(image)) => {
                            let fingerprint = ClipboardFingerprint::Image(image.sha256.clone());
                            if should_process_clipboard(&mut last_fingerprint, fingerprint) {
                                match remember_image(&image) {
                                    Ok(true) => { let _ = app.emit("clipboard-history-changed", ()); }
                                    Err(error) => eprintln!("failed to persist image history: {error}"),
                                    Ok(false) => {}
                                }
                                let event_id = format!("{device_id}:{}", uuid::Uuid::new_v4());
                                let data = image_sync::encode_wire(&image);
                                if socket.send(Message::Text(json!({"type":"clipboard","eventId":event_id,"contentType":"image","data":data}).to_string())).is_err() {
                                    continue 'reconnect;
                                }
                            }
                        }
                        Some(Err(error)) => { let _ = app.emit("clipboard-sync-warning", error); }
                        None => if let Ok(text) = cb.get_text() {
                            if !text.is_empty() {
                                let fingerprint = ClipboardFingerprint::Text(text.clone());
                                if should_process_clipboard(&mut last_fingerprint, fingerprint) {
                                    match remember_text(&text) {
                                        Ok(true) => { let _ = app.emit("clipboard-history-changed", ()); }
                                        Err(error) => eprintln!("failed to persist clipboard history: {error}"),
                                        Ok(false) => {}
                                    }
                                    let event_id = format!("{device_id}:{}", uuid::Uuid::new_v4());
                                    if socket.send(Message::Text(json!({"type":"clipboard","eventId":event_id,"contentType":"text","data":text}).to_string())).is_err() {
                                        continue 'reconnect;
                                    }
                                }
                            }
                        }
                    }
                }
                match socket.read() {
                    Ok(Message::Text(raw)) => if let Ok(message) = serde_json::from_str::<serde_json::Value>(&raw) {
                        if message["type"] == "clipboard" {
                            if message["contentType"] == "text" {
                                if let Some(value) = message["data"].as_str() {
                                    last_fingerprint = Some(ClipboardFingerprint::Text(value.to_string()));
                                    let clipboard_result = clipboard.as_mut().ok_or_else(|| "无法访问系统剪贴板".to_string())
                                        .and_then(|cb| cb.set_text(value).map_err(|error| error.to_string()));
                                    match clipboard_result {
                                        Ok(()) => {
                                    match remember_text(value) {
                                                Ok(true) => { let _ = app.emit("clipboard-history-changed", ()); }
                                                Err(error) => eprintln!("failed to persist clipboard history: {error}"),
                                                Ok(false) => {}
                                            }
                                        }
                                        Err(error) => { let _ = app.emit("clipboard-sync-warning", format!("写入文字剪贴板失败：{error}")); }
                                    }
                                }
                            } else if message["contentType"] == "image" {
                                if let Some(value) = message["data"].as_str() {
                                    match image_sync::decode_wire(value) {
                                        Ok(image) => {
                                            last_fingerprint = Some(ClipboardFingerprint::Image(image.sha256.clone()));
                                            let clipboard_result = clipboard.as_mut().ok_or_else(|| "无法访问系统剪贴板".to_string())
                                                .and_then(|cb| image_sync::clipboard_data(&image).and_then(|data| cb.set_image(data).map_err(|error| error.to_string())));
                                            match clipboard_result {
                                                Ok(()) => {
                                                    match remember_image(&image) {
                                                        Ok(true) => { let _ = app.emit("clipboard-history-changed", ()); }
                                                        Err(error) => eprintln!("failed to persist image history: {error}"),
                                                        Ok(false) => {}
                                                    }
                                                }
                                                Err(error) => { let _ = app.emit("clipboard-sync-warning", format!("写入图片剪贴板失败：{error}")); }
                                            }
                                        }
                                        Err(error) => { let _ = app.emit("clipboard-sync-warning", error); }
                                    }
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
    use super::{apply_history_settings, auth_message, cleanup_orphan_images, ensure_device_id, history_file, image_directory, image_entry, insert_entry, load_history, save_image_files, should_process_clipboard, sidecar_file, text_entry, thumbnail_directory, validate_history_limit, write_history, write_json, ClipboardFingerprint, HistoryDocument, HistoryEntry, HistoryKind, HistorySettings, HistoryState, HISTORY_SCHEMA_VERSION, HISTORY_SHORTCUT};
    use crate::image_sync;
    use std::{fs, path::PathBuf, time::{SystemTime, UNIX_EPOCH}};
    use tungstenite::Message;

    fn temp_directory(name: &str) -> PathBuf {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).expect("time after epoch").as_nanos();
        let path = std::env::temp_dir().join(format!("clipsync-{name}-{}-{unique}", std::process::id()));
        fs::create_dir_all(&path).expect("create temp directory");
        path
    }

    fn text_values(entries: &[super::HistoryEntry]) -> Vec<&str> {
        entries.iter().filter_map(|entry| entry.text.as_deref()).collect()
    }

    fn sample_image() -> image_sync::NormalizedImage {
        image_sync::normalize_rgba(2, 2, &[255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255]).expect("sample image")
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
    fn clipboard_history_combines_types_and_removes_duplicates() {
        let mut history = Vec::new();
        for index in 0..11 { insert_entry(&mut history, text_entry(format!("item-{index}")), 10); }
        let image = sample_image();
        let image_history = image_entry(&image);
        insert_entry(&mut history, image_history.clone(), 10);
        assert_eq!(history.len(), 10);
        assert_eq!(history[0].kind, HistoryKind::Image);
        assert!(!text_values(&history).contains(&"item-0"));
        let duplicate = image_entry(&image);
        let (_, removed) = insert_entry(&mut history, duplicate, 10);
        assert!(removed.is_empty());
        assert_eq!(history.iter().filter(|entry| entry.sha256 == image_history.sha256).count(), 1);
    }

    #[test]
    fn clipboard_fingerprints_suppress_loops_but_allow_type_changes() {
        let mut last = None;
        assert!(should_process_clipboard(&mut last, ClipboardFingerprint::Image("image-a".to_string())));
        assert!(!should_process_clipboard(&mut last, ClipboardFingerprint::Image("image-a".to_string())));
        assert!(should_process_clipboard(&mut last, ClipboardFingerprint::Text("text".to_string())));
        assert!(should_process_clipboard(&mut last, ClipboardFingerprint::Image("image-a".to_string())));
    }

    #[test]
    fn history_limit_must_be_between_one_and_one_thousand() {
        assert!(validate_history_limit(1).is_ok());
        assert!(validate_history_limit(1000).is_ok());
        assert!(validate_history_limit(0).is_err());
        assert!(validate_history_limit(1001).is_err());
    }

    #[test]
    fn legacy_history_migrates_to_version_two() {
        let directory = temp_directory("round-trip");
        write_json(&history_file(&directory), &vec!["new".to_string(), "new".to_string(), "".to_string(), "old".to_string()]).expect("write history");
        let (history, migrated) = load_history(&directory, 10).expect("load history");
        assert!(migrated);
        assert_eq!(text_values(&history), vec!["new", "old"]);
        write_history(&directory, &history).expect("write migrated history");
        let (_, migrated_again) = load_history(&directory, 10).expect("reload history");
        assert!(!migrated_again);
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
        let image = sample_image();
        let image_history = image_entry(&image);
        let entries = vec![image_history.clone(), text_entry("one".to_string())];
        save_image_files(&old_directory, &image_history, &image).expect("save image history");
        write_history(&old_directory, &entries).expect("write old history");
        let mut state = HistoryState {
            entries,
            settings: HistorySettings { history_limit: 10, history_directory: old_directory.to_string_lossy().into_owned(), device_id: "desktop-test".to_string() },
            settings_path: root.join("settings.json"),
        };
        let (settings, changed) = apply_history_settings(&mut state, 1, new_directory.to_string_lossy().into_owned()).expect("migrate history");
        assert!(changed);
        assert_eq!(settings.history_limit, 1);
        assert_eq!(settings.device_id, "desktop-test");
        let migrated_directory = PathBuf::from(settings.history_directory);
        let (migrated, _) = load_history(&migrated_directory, 10).expect("load migrated history");
        assert_eq!(migrated.len(), 1);
        assert_eq!(migrated[0].kind, HistoryKind::Image);
        assert!(super::image_directory(&migrated_directory).join(image_history.file_name.as_deref().expect("image name")).exists());
        assert!(!history_file(&old_directory).exists());
        assert!(!super::image_directory(&old_directory).exists());
        fs::remove_dir_all(root).expect("remove temp directory");
    }

    #[test]
    fn orphan_cleanup_only_removes_unreferenced_uuid_png_files() {
        let directory = temp_directory("orphan-cleanup");
        let image = sample_image();
        let entry = image_entry(&image);
        save_image_files(&directory, &entry, &image).expect("save referenced image");
        let orphan = format!("{}.png", uuid::Uuid::new_v4());
        fs::create_dir_all(image_directory(&directory)).expect("create image directory");
        fs::write(image_directory(&directory).join(&orphan), b"orphan").expect("write image orphan");
        fs::create_dir_all(thumbnail_directory(&directory)).expect("create thumbnail directory");
        fs::write(thumbnail_directory(&directory).join(&orphan), b"orphan").expect("write thumbnail orphan");
        fs::write(image_directory(&directory).join("keep.txt"), b"unrelated").expect("write unrelated file");
        cleanup_orphan_images(&directory, &[entry.clone()]);
        assert!(image_directory(&directory).join(entry.file_name.expect("image name")).exists());
        assert!(!image_directory(&directory).join(&orphan).exists());
        assert!(!thumbnail_directory(&directory).join(&orphan).exists());
        assert!(image_directory(&directory).join("keep.txt").exists());
        fs::remove_dir_all(directory).expect("remove temp directory");
    }

    #[test]
    fn reducing_history_limit_removes_trimmed_image_files() {
        let directory = temp_directory("trim-images");
        let image = sample_image();
        let image_history = image_entry(&image);
        save_image_files(&directory, &image_history, &image).expect("save image history");
        let image_path = image_directory(&directory).join(image_history.file_name.as_deref().expect("image name"));
        let thumbnail_path = thumbnail_directory(&directory).join(image_history.thumbnail_name.as_deref().expect("thumbnail name"));
        let mut state = HistoryState {
            entries: vec![text_entry("newer".to_string()), image_history],
            settings: HistorySettings { history_limit: 10, history_directory: directory.canonicalize().expect("canonical directory").to_string_lossy().into_owned(), device_id: "desktop-test".to_string() },
            settings_path: directory.join("settings.json"),
        };
        apply_history_settings(&mut state, 1, directory.to_string_lossy().into_owned()).expect("reduce limit");
        assert!(!image_path.exists());
        assert!(!thumbnail_path.exists());
        fs::remove_dir_all(directory).expect("remove temp directory");
    }

    #[test]
    fn history_rejects_path_traversal_metadata() {
        let directory = temp_directory("invalid-metadata");
        let entry = HistoryEntry {
            id: uuid::Uuid::new_v4().to_string(),
            kind: HistoryKind::Image,
            text: None,
            file_name: Some("..\\outside.png".to_string()),
            thumbnail_name: Some("..\\outside.png".to_string()),
            sha256: Some("0".repeat(64)),
            byte_length: Some(4),
            width: Some(1),
            height: Some(1),
            created_at: "2026-10-02T00:00:00Z".to_string(),
        };
        write_json(&history_file(&directory), &HistoryDocument { version: HISTORY_SCHEMA_VERSION, entries: vec![entry] }).expect("write invalid history");
        assert!(load_history(&directory, 10).is_err());
        fs::remove_dir_all(directory).expect("remove temp directory");
    }

    #[test]
    fn failed_migration_restores_existing_destination_history() {
        let root = temp_directory("migration-rollback");
        let old_directory = root.join("old");
        let new_directory = root.join("new");
        fs::create_dir_all(&old_directory).expect("create old directory");
        fs::create_dir_all(&new_directory).expect("create new directory");
        let current = vec![text_entry("current".to_string())];
        let destination = vec![text_entry("destination".to_string())];
        write_history(&old_directory, &current).expect("write old history");
        write_history(&new_directory, &destination).expect("write destination history");
        let blocked_parent = root.join("blocked");
        fs::write(&blocked_parent, b"not a directory").expect("create blocking file");
        let mut state = HistoryState {
            entries: current,
            settings: HistorySettings { history_limit: 10, history_directory: old_directory.to_string_lossy().into_owned(), device_id: "desktop-test".to_string() },
            settings_path: blocked_parent.join("settings.json"),
        };
        assert!(apply_history_settings(&mut state, 10, new_directory.to_string_lossy().into_owned()).is_err());
        let (restored, _) = load_history(&new_directory, 10).expect("load restored destination");
        assert_eq!(text_values(&restored), vec!["destination"]);
        assert_eq!(text_values(&state.entries), vec!["current"]);
        assert_eq!(state.settings.history_directory, old_directory.to_string_lossy());
        fs::remove_dir_all(root).expect("remove temp directory");
    }

    #[test]
    fn history_directory_migration_rejects_nested_paths() {
        let root = temp_directory("nested-migration");
        let nested = root.join("nested");
        fs::create_dir_all(&nested).expect("create nested directory");
        let mut state = HistoryState {
            entries: vec![text_entry("current".to_string())],
            settings: HistorySettings { history_limit: 10, history_directory: root.canonicalize().expect("canonical root").to_string_lossy().into_owned(), device_id: "desktop-test".to_string() },
            settings_path: root.join("settings.json"),
        };
        assert!(apply_history_settings(&mut state, 10, nested.to_string_lossy().into_owned()).is_err());
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
        .invoke_handler(tauri::generate_handler![start_sync, get_clipboard_history, get_history_thumbnail, get_history_settings, save_history_settings, hide_history, select_clipboard_history])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
