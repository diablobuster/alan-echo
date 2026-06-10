#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod db;
mod license;
mod settings;
mod text_cleanup;
mod whisper;

use audio::{DeviceInfo, RecordingResult, RecorderHandle};
use db::TranscriptDB;
use license::LicenseManager;
use settings::Settings;
use text_cleanup::TextCleanupEngine;
use whisper::WhisperEngine;

use parking_lot::Mutex;
use std::sync::Arc;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Emitter, Manager, State,
};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

pub struct AppState {
    pub db: Mutex<TranscriptDB>,
    pub settings: Mutex<Settings>,
    pub license: Mutex<LicenseManager>,
    pub cleanup: Mutex<TextCleanupEngine>,
    pub recorder: RecorderHandle,
    pub whisper: Mutex<Option<WhisperEngine>>,
    pub data_dir: std::path::PathBuf,
}

// ── Transcript commands ──────────────────────────────────────────────

#[tauri::command]
fn get_transcripts(state: State<Arc<AppState>>, page: Option<u32>, page_size: Option<u32>) -> Result<serde_json::Value, String> {
    let db = state.db.lock();
    let (transcripts, total) = db.get_page(page.unwrap_or(0), page_size.unwrap_or(50)).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "transcripts": transcripts, "total": total }))
}

#[tauri::command]
fn search_transcripts(state: State<Arc<AppState>>, query: String) -> Result<Vec<db::Transcript>, String> {
    state.db.lock().search(&query, 100).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_stats(state: State<Arc<AppState>>) -> Result<db::Stats, String> {
    state.db.lock().get_stats().map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_transcript(state: State<Arc<AppState>>, id: i64) -> Result<bool, String> {
    state.db.lock().delete(id).map_err(|e| e.to_string())
}

#[tauri::command]
fn update_transcript(state: State<Arc<AppState>>, id: i64, text: String) -> Result<bool, String> {
    state.db.lock().update_text(id, &text).map_err(|e| e.to_string())
}

#[tauri::command]
fn export_transcripts(state: State<Arc<AppState>>, path: String, format: String) -> Result<bool, String> {
    state.db.lock().export(&path, &format).map_err(|e| e.to_string())
}

// ── Settings commands ────────────────────────────────────────────────

#[tauri::command]
fn get_settings(state: State<Arc<AppState>>) -> Result<serde_json::Value, String> {
    Ok(state.settings.lock().to_json())
}

#[tauri::command]
fn set_setting(state: State<Arc<AppState>>, key: String, value: serde_json::Value) -> Result<(), String> {
    let mut s = state.settings.lock();
    s.set(&key, value);
    s.save().map_err(|e| e.to_string())
}

// ── License commands ─────────────────────────────────────────────────

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
    // TODO: re-enable after fixing keygen parity
    // Ok(state.license.lock().is_licensed())
    Ok(true)
}

// ── Audio commands ───────────────────────────────────────────────────

#[tauri::command]
fn list_audio_devices() -> Result<Vec<DeviceInfo>, String> {
    audio::list_input_devices()
}

#[tauri::command]
fn start_recording(state: State<Arc<AppState>>) -> Result<(), String> {
    let device = state.settings.lock().get_str("microphone_device");
    state.recorder.start(device.as_deref())
}

#[tauri::command]
fn stop_recording(state: State<Arc<AppState>>) -> Result<RecordingResult, String> {
    state.recorder.stop()
}

#[tauri::command]
fn is_recording(state: State<Arc<AppState>>) -> Result<bool, String> {
    Ok(state.recorder.is_recording())
}

#[tauri::command]
fn get_audio_level(state: State<Arc<AppState>>) -> Result<f32, String> {
    Ok(state.recorder.current_level())
}

#[tauri::command]
async fn test_microphone() -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(|| {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        use std::sync::{Arc, atomic::{AtomicU32, AtomicBool, Ordering}};

        let host = cpal::default_host();
        let device = host.default_input_device().ok_or("No input device found")?;
        let name = device.name().unwrap_or_default();
        let config = device.default_input_config().map_err(|e| e.to_string())?;
        let fmt = format!("{:?}", config.sample_format());
        let rate = config.sample_rate().0;
        let ch = config.channels();

        let sample_count = Arc::new(AtomicU32::new(0));
        let max_val = Arc::new(parking_lot::Mutex::new(0.0f32));
        let got_data = Arc::new(AtomicBool::new(false));

        let sc = Arc::clone(&sample_count);
        let mv = Arc::clone(&max_val);
        let gd = Arc::clone(&got_data);

        let stream_config: cpal::StreamConfig = config.config();

        let stream = device.build_input_stream(
            &stream_config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                gd.store(true, Ordering::Relaxed);
                sc.fetch_add(data.len() as u32, Ordering::Relaxed);
                let peak = data.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
                let mut m = mv.lock();
                if peak > *m { *m = peak; }
            },
            |err| log::error!("Mic test error: {}", err),
            None,
        ).map_err(|e| format!("Failed to open stream: {}", e))?;

        stream.play().map_err(|e| format!("Failed to play: {}", e))?;
        std::thread::sleep(std::time::Duration::from_secs(2));
        drop(stream);

        let samples = sample_count.load(Ordering::Relaxed);
        let peak = *max_val.lock();
        let got = got_data.load(Ordering::Relaxed);

        Ok(serde_json::json!({
            "device": name,
            "format": fmt,
            "sample_rate": rate,
            "channels": ch,
            "callback_fired": got,
            "samples_received": samples,
            "peak_amplitude": peak,
            "speech_likely": peak > 0.001,
        }))
    }).await.map_err(|e| format!("Task failed: {}", e))?
}

// ── Transcription commands ───────────────────────────────────────────

#[tauri::command]
async fn transcribe(state: State<'_, Arc<AppState>>, wav_path: String) -> Result<serde_json::Value, String> {
    let state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || {
        let whisper_guard = state.whisper.lock();
        let engine = whisper_guard.as_ref().ok_or("Whisper engine not initialized")?;
        let result = engine.transcribe(&wav_path)?;

        let cleaned = state.cleanup.lock().clean(&result.text);

        if cleaned.is_empty() {
            return Ok(serde_json::json!({ "text": "", "raw_text": result.text, "empty": true }));
        }

        let id = state.db.lock().save(&cleaned, Some(&result.text), result.duration_seconds).map_err(|e| e.to_string())?;

        // Clean up the WAV file
        std::fs::remove_file(&wav_path).ok();

        Ok(serde_json::json!({
            "id": id,
            "text": cleaned,
            "raw_text": result.text,
            "duration_seconds": result.duration_seconds,
            "word_count": cleaned.split_whitespace().count(),
            "empty": false,
        }))
    }).await.map_err(|e| format!("Task failed: {}", e))?
}

#[tauri::command]
fn check_whisper_ready(state: State<Arc<AppState>>) -> Result<bool, String> {
    Ok(state.whisper.lock().as_ref().map(|w| w.is_ready()).unwrap_or(false))
}

// ── Text cleanup ─────────────────────────────────────────────────────

#[tauri::command]
fn clean_text(state: State<Arc<AppState>>, text: String) -> Result<String, String> {
    Ok(state.cleanup.lock().clean(&text))
}

// ── Main ─────────────────────────────────────────────────────────────

fn main() {
    env_logger::init();

    let data_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("ALAN Echo");
    std::fs::create_dir_all(&data_dir).ok();
    std::fs::create_dir_all(data_dir.join("backups")).ok();
    std::fs::create_dir_all(data_dir.join("models")).ok();

    let db_path = data_dir.join("transcripts.db");
    let settings_path = data_dir.join("settings.json");

    let settings = Settings::load(&settings_path).unwrap_or_default();
    let license_key = settings.get_str("license_key");
    let cleanup_level = settings.get_str("text_cleanup_level").unwrap_or_else(|| "standard".into());
    let machine_id = license::get_machine_id();

    let whisper_engine = WhisperEngine::new(&data_dir).ok();
    if whisper_engine.is_none() {
        log::warn!("Whisper engine not available — transcription will fail until model is installed");
    }

    let app_state = Arc::new(AppState {
        db: Mutex::new(TranscriptDB::open(&db_path).expect("Failed to open database")),
        settings: Mutex::new(settings),
        license: Mutex::new(LicenseManager::new(license_key, machine_id)),
        cleanup: Mutex::new(TextCleanupEngine::new(&cleanup_level)),
        recorder: RecorderHandle::new(),
        whisper: Mutex::new(whisper_engine),
        data_dir: data_dir.clone(),
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_shell::init())
        .manage(app_state.clone())
        .setup(move |app| {
            // System tray
            let show = MenuItem::with_id(app, "show", "Show Dashboard", true, None::<&str>)?;
            let dictate = MenuItem::with_id(app, "dictate", "Start Dictation", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit ALAN Echo", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &dictate, &quit])?;

            let tray_icon_bytes = include_bytes!("../icons/icon.png");
            let tray_img = image::load_from_memory(tray_icon_bytes).expect("Failed to decode tray icon");
            let tray_rgba = tray_img.to_rgba8();
            let (tw, th) = tray_rgba.dimensions();
            let tray_icon = tauri::image::Image::new_owned(tray_rgba.into_raw(), tw, th);

            let _tray = TrayIconBuilder::new()
                .icon(tray_icon)
                .tooltip("ALAN Echo — Ready")
                .menu(&menu)
                .on_menu_event(move |app, event| {
                    match event.id().as_ref() {
                        "show" => {
                            if let Some(w) = app.get_webview_window("main") {
                                w.show().ok();
                                w.set_focus().ok();
                            }
                        }
                        "dictate" => {
                            if let Some(w) = app.get_webview_window("main") {
                                w.emit("dictate-toggle", ()).ok();
                            }
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .build(app)?;

            // Register global shortcuts
            use tauri_plugin_global_shortcut::ShortcutState;
            app.global_shortcut().on_shortcut("CmdOrCtrl+Shift+Space", move |app, _shortcut, event| {
                if event.state == ShortcutState::Pressed {
                    if let Some(w) = app.get_webview_window("main") {
                        w.emit("dictate-toggle", ()).ok();
                    }
                }
            }).unwrap_or_else(|e| log::error!("Failed to register Ctrl+Shift+Space: {}", e));
            app.global_shortcut().on_shortcut("CmdOrCtrl+Shift+Escape", move |app, _shortcut, event| {
                if event.state == ShortcutState::Pressed {
                    if let Some(w) = app.get_webview_window("main") {
                        w.emit("dictate-cancel", ()).ok();
                    }
                }
            }).unwrap_or_else(|e| log::error!("Failed to register Ctrl+Shift+Escape: {}", e));
            app.global_shortcut().on_shortcut("CmdOrCtrl+Shift+H", move |app, _shortcut, event| {
                if event.state == ShortcutState::Pressed {
                    if let Some(w) = app.get_webview_window("main") {
                        w.show().ok();
                        w.set_focus().ok();
                    }
                }
            }).unwrap_or_else(|e| log::error!("Failed to register Ctrl+Shift+H: {}", e));

            // Set window icon
            if let Some(window) = app.get_webview_window("main") {
                let png_bytes = include_bytes!("../icons/icon.png");
                if let Ok(img) = image::load_from_memory(png_bytes) {
                    let rgba = img.to_rgba8();
                    let (w, h) = rgba.dimensions();
                    let icon = tauri::image::Image::new_owned(rgba.into_raw(), w, h);
                    let _ = window.set_icon(icon);
                }
            }

            // Daily backup
            let state = app.state::<Arc<AppState>>();
            state.db.lock().maybe_daily_backup();

            Ok(())
        })
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
            list_audio_devices,
            start_recording,
            stop_recording,
            is_recording,
            get_audio_level,
            transcribe,
            check_whisper_ready,
            clean_text,
            test_microphone,
        ])
        .run(tauri::generate_context!())
        .expect("error while running ALAN Echo");
}
