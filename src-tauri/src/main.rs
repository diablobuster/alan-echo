#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod db;
mod license;
mod settings;
mod text_cleanup;
mod audio;

use db::TranscriptDB;
use license::LicenseManager;
use settings::Settings;
use text_cleanup::TextCleanupEngine;

use parking_lot::Mutex;
use std::sync::Arc;
use tauri::{Manager, State};

pub struct AppState {
    pub db: Mutex<TranscriptDB>,
    pub settings: Mutex<Settings>,
    pub license: Mutex<LicenseManager>,
    pub cleanup: Mutex<TextCleanupEngine>,
    pub recording: Mutex<bool>,
}

// --- Tauri commands (frontend ↔ backend bridge) ---

#[tauri::command]
fn get_transcripts(state: State<Arc<AppState>>, page: Option<u32>, page_size: Option<u32>) -> Result<serde_json::Value, String> {
    let db = state.db.lock();
    let p = page.unwrap_or(0);
    let ps = page_size.unwrap_or(50);
    let (transcripts, total) = db.get_page(p, ps).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "transcripts": transcripts, "total": total, "page": p }))
}

#[tauri::command]
fn search_transcripts(state: State<Arc<AppState>>, query: String) -> Result<Vec<db::Transcript>, String> {
    let db = state.db.lock();
    db.search(&query, 100).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_stats(state: State<Arc<AppState>>) -> Result<db::Stats, String> {
    let db = state.db.lock();
    db.get_stats().map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_transcript(state: State<Arc<AppState>>, id: i64) -> Result<bool, String> {
    let db = state.db.lock();
    db.delete(id).map_err(|e| e.to_string())
}

#[tauri::command]
fn update_transcript(state: State<Arc<AppState>>, id: i64, text: String) -> Result<bool, String> {
    let db = state.db.lock();
    db.update_text(id, &text).map_err(|e| e.to_string())
}

#[tauri::command]
fn export_transcripts(state: State<Arc<AppState>>, path: String, format: String) -> Result<bool, String> {
    let db = state.db.lock();
    db.export(&path, &format).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_settings(state: State<Arc<AppState>>) -> Result<serde_json::Value, String> {
    let s = state.settings.lock();
    Ok(s.to_json())
}

#[tauri::command]
fn set_setting(state: State<Arc<AppState>>, key: String, value: serde_json::Value) -> Result<(), String> {
    let mut s = state.settings.lock();
    s.set(&key, value);
    s.save().map_err(|e| e.to_string())
}

#[tauri::command]
fn validate_license(state: State<Arc<AppState>>, key: String) -> Result<serde_json::Value, String> {
    let mut lm = state.license.lock();
    let (valid, msg) = lm.activate(&key);
    if valid {
        let mut s = state.settings.lock();
        s.set("license_key", serde_json::Value::String(key));
        let _ = s.save();
    }
    Ok(serde_json::json!({ "valid": valid, "message": msg }))
}

#[tauri::command]
fn check_license(state: State<Arc<AppState>>) -> Result<bool, String> {
    let lm = state.license.lock();
    Ok(lm.is_licensed())
}

#[tauri::command]
fn clean_text(state: State<Arc<AppState>>, text: String) -> Result<String, String> {
    let engine = state.cleanup.lock();
    Ok(engine.clean(&text))
}

#[tauri::command]
fn list_audio_devices() -> Result<Vec<audio::DeviceInfo>, String> {
    audio::list_input_devices().map_err(|e| e.to_string())
}

fn main() {
    env_logger::init();

    let data_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("ALAN Echo");
    std::fs::create_dir_all(&data_dir).ok();
    std::fs::create_dir_all(data_dir.join("backups")).ok();

    let db_path = data_dir.join("transcripts.db");
    let settings_path = data_dir.join("settings.json");

    let settings = Settings::load(&settings_path).unwrap_or_default();
    let license_key = settings.get_str("license_key");
    let cleanup_level = settings.get_str("text_cleanup_level").unwrap_or_else(|| "standard".into());
    let machine_id = license::get_machine_id();

    let app_state = Arc::new(AppState {
        db: Mutex::new(TranscriptDB::open(&db_path).expect("Failed to open database")),
        settings: Mutex::new(settings),
        license: Mutex::new(LicenseManager::new(license_key, machine_id)),
        cleanup: Mutex::new(TextCleanupEngine::new(&cleanup_level)),
        recording: Mutex::new(false),
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_shell::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_transcripts,
            search_transcripts,
            get_stats,
            delete_transcript,
            update_transcript,
            export_transcripts,
            get_settings,
            set_setting,
            validate_license,
            check_license,
            clean_text,
            list_audio_devices,
        ])
        .run(tauri::generate_context!())
        .expect("error while running ALAN Echo");
}
