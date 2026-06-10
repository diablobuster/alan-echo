#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod db;
mod license;
mod paste;
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
    pub whisper: Arc<WhisperEngine>,
    pub paste_target: Mutex<Option<isize>>,
    pub hotkeys: Mutex<serde_json::Value>,
    pub cancel_accel: Mutex<Option<String>>,
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
    // Refuse a model the user can't actually run — otherwise the engine
    // silently falls back and the selector appears broken.
    if key == "whisper_model" {
        if let Some(name) = value.as_str() {
            if !name.is_empty() && name != "auto" && !state.whisper.model_available(name) {
                return Err(format!(
                    "That model isn't installed — place ggml-{}.bin in the models folder first",
                    name
                ));
            }
        }
    }
    {
        let mut s = state.settings.lock();
        s.set(&key, value.clone());
        s.save().map_err(|e| e.to_string())?;
    }
    // Some settings must take effect immediately, not on next launch.
    match key.as_str() {
        "text_cleanup_level" => {
            if let Some(level) = value.as_str() {
                *state.cleanup.lock() = TextCleanupEngine::new(level);
            }
        }
        "whisper_model" => {
            // Restart whisper-server with the newly selected model (async).
            state.whisper.reload(value.as_str());
        }
        _ => {}
    }
    Ok(())
}

// ── License commands ─────────────────────────────────────────────────

#[tauri::command]
fn validate_license(state: State<Arc<AppState>>, key: String) -> Result<serde_json::Value, String> {
    let mut lm = state.license.lock();
    let (valid, msg) = lm.activate(&key);
    if valid {
        let mut s = state.settings.lock();
        s.set("license_key", serde_json::Value::String(key.trim().to_uppercase().replace(' ', "")));
        let _ = s.save();
    }
    Ok(serde_json::json!({ "valid": valid, "message": msg }))
}

#[tauri::command]
fn check_license(state: State<Arc<AppState>>) -> Result<bool, String> {
    // Debug builds skip the gate so development isn't blocked by keygen access.
    if cfg!(debug_assertions) {
        return Ok(true);
    }
    Ok(state.license.lock().is_licensed())
}

// ── Audio commands ───────────────────────────────────────────────────

#[tauri::command]
fn list_audio_devices() -> Result<Vec<DeviceInfo>, String> {
    audio::list_input_devices()
}

#[tauri::command]
fn start_recording(app: tauri::AppHandle, state: State<Arc<AppState>>) -> Result<(), String> {
    // Capture the focused app NOW — this is where the transcript gets pasted.
    let target = paste::foreground_window();

    // A recording abandoned mid mic-test (its component unmounted) must not
    // wedge dictation forever: discard the stale capture and start fresh.
    if state.recorder.is_recording() {
        if let Ok(stale) = state.recorder.stop() {
            if let Some(path) = stale.wav_path {
                std::fs::remove_file(path).ok();
            }
        }
    }

    let device = state.settings.lock().get_str("microphone_device");
    state.recorder.start(device.as_deref())?;

    // Commit only after a successful start so a failed start (e.g. a mic test
    // racing a dictation) can never clobber the in-flight paste target.
    *state.paste_target.lock() = Some(target);

    // Register the cancel hotkey only for the duration of the recording —
    // Echo must not swallow Ctrl+Shift+X system-wide while idle in the tray.
    if let Some(accel) = state.cancel_accel.lock().clone() {
        register_emit_hotkey(&app, &accel, "dictate-cancel");
    }
    Ok(())
}

fn unregister_cancel_hotkey(app: &tauri::AppHandle, state: &AppState) {
    if let Some(accel) = state.cancel_accel.lock().clone() {
        app.global_shortcut().unregister(accel.as_str()).ok();
    }
}

#[tauri::command]
fn stop_recording(app: tauri::AppHandle, state: State<Arc<AppState>>) -> Result<RecordingResult, String> {
    unregister_cancel_hotkey(&app, &state);
    state.recorder.stop()
}

#[tauri::command]
fn cancel_recording(app: tauri::AppHandle, state: State<Arc<AppState>>) -> Result<(), String> {
    unregister_cancel_hotkey(&app, &state);
    if let Ok(result) = state.recorder.stop() {
        // Discard the recording — nothing should reach the transcriber.
        if let Some(path) = result.wav_path {
            std::fs::remove_file(path).ok();
        }
    }
    *state.paste_target.lock() = None;
    Ok(())
}

#[tauri::command]
fn discard_recording(state: State<Arc<AppState>>, wav_path: String) -> Result<(), String> {
    // Only delete our own recording WAVs inside the data dir.
    let p = std::path::PathBuf::from(&wav_path);
    let is_ours = p.parent().map(|d| d == state.data_dir).unwrap_or(false)
        && p.file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with("recording_") && n.ends_with(".wav"))
            .unwrap_or(false);
    if is_ours {
        std::fs::remove_file(&p).ok();
    }
    Ok(())
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
async fn transcribe(state: State<'_, Arc<AppState>>, app: tauri::AppHandle, wav_path: String) -> Result<serde_json::Value, String> {
    let state = Arc::clone(state.inner());
    tokio::task::spawn_blocking(move || {
        let result = state.whisper.transcribe(&wav_path);

        // The WAV served its purpose whether transcription succeeded or not —
        // the frontend never retries with the same path.
        std::fs::remove_file(&wav_path).ok();
        let result = result?;

        let cleaned = state.cleanup.lock().clean(&result.text);
        if cleaned.is_empty() {
            return Ok(serde_json::json!({ "text": "", "raw_text": result.text, "empty": true }));
        }

        let id = state.db.lock().save(&cleaned, Some(&result.text), result.duration_seconds).map_err(|e| e.to_string())?;

        let pasted = deliver_text(&app, &state, &cleaned);

        Ok(serde_json::json!({
            "id": id,
            "text": cleaned,
            "raw_text": result.text,
            "duration_seconds": result.duration_seconds,
            "word_count": cleaned.split_whitespace().count(),
            "empty": false,
            "pasted": pasted,
        }))
    }).await.map_err(|e| format!("Task failed: {}", e))?
}

/// Copy the transcript to the clipboard and, if enabled, paste it into the app
/// that was focused when recording started. Returns whether a paste happened.
fn deliver_text(app: &tauri::AppHandle, state: &AppState, text: &str) -> bool {
    use tauri_plugin_clipboard_manager::ClipboardExt;

    let auto_paste = state.settings.lock().get_bool("auto_paste").unwrap_or(true);
    let target = state.paste_target.lock().take();

    let prior_clip = app.clipboard().read_text().ok();
    if let Err(e) = app.clipboard().write_text(text.to_string()) {
        log::warn!("Clipboard write failed: {}", e);
        return false;
    }

    if !auto_paste {
        return false;
    }

    let Some(hwnd) = target else { return false };
    if hwnd == 0 {
        return false;
    }

    // Never paste into our own dashboard (e.g. recording started via the
    // in-app button) — the clipboard copy is enough there.
    #[cfg(target_os = "windows")]
    {
        let own_window = app
            .get_webview_window("main")
            .and_then(|w| w.hwnd().ok())
            .map(|h| h.0 as isize == hwnd)
            .unwrap_or(false);
        if own_window {
            return false;
        }
    }

    match paste::paste_into(hwnd) {
        Ok(()) => {
            // The target reads the clipboard asynchronously when its UI thread
            // processes the injected Ctrl+V — restore the user's clipboard on a
            // detached thread after a generous delay, and only if ours is still
            // there (never clobber something the user copied in the meantime).
            if let Some(prior) = prior_clip {
                let app = app.clone();
                let ours = text.to_string();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(1500));
                    if app.clipboard().read_text().ok().as_deref() == Some(ours.as_str()) {
                        if let Err(e) = app.clipboard().write_text(prior) {
                            log::warn!("Clipboard restore failed: {}", e);
                        }
                    }
                });
            }
            true
        }
        Err(e) => {
            // Text stays in the clipboard so the user can paste manually.
            log::warn!("Auto-paste failed: {}", e);
            false
        }
    }
}

#[tauri::command]
fn check_whisper_ready(state: State<Arc<AppState>>) -> Result<bool, String> {
    Ok(state.whisper.is_ready())
}

#[tauri::command]
fn get_engine_info(state: State<Arc<AppState>>) -> Result<whisper::EngineInfo, String> {
    Ok(state.whisper.info())
}

#[tauri::command]
fn list_models(state: State<Arc<AppState>>) -> Result<serde_json::Value, String> {
    // The retail installer bundles only base.en; bigger models arrive as
    // optional downloads. The picker greys out what isn't on disk.
    let models = [("base", "Basic"), ("small", "Standard"), ("medium", "Enhanced"), ("large-v3", "Ultra")]
        .iter()
        .map(|(name, label)| {
            serde_json::json!({
                "name": name,
                "label": label,
                "available": state.whisper.model_available(name),
            })
        })
        .collect::<Vec<_>>();
    Ok(serde_json::Value::Array(models))
}

#[tauri::command]
fn get_hotkey_info(state: State<Arc<AppState>>) -> Result<serde_json::Value, String> {
    Ok(state.hotkeys.lock().clone())
}

#[tauri::command]
async fn read_wav_base64(wav_path: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let bytes = std::fs::read(&wav_path).map_err(|e| format!("Failed to read WAV: {}", e))?;
        // Mic-test recordings are throwaway; don't litter the data dir.
        std::fs::remove_file(&wav_path).ok();
        use base64::Engine;
        Ok(base64::engine::general_purpose::STANDARD.encode(&bytes))
    }).await.map_err(|e| format!("Task failed: {}", e))?
}

// ── Text cleanup ─────────────────────────────────────────────────────

#[tauri::command]
fn clean_text(state: State<Arc<AppState>>, text: String) -> Result<String, String> {
    Ok(state.cleanup.lock().clean(&text))
}

// ── Hotkeys ──────────────────────────────────────────────────────────

fn register_emit_hotkey(app: &tauri::AppHandle, accel: &str, event: &'static str) -> bool {
    use tauri_plugin_global_shortcut::ShortcutState;
    app.global_shortcut()
        .on_shortcut(accel, move |app, _shortcut, ev| {
            if ev.state == ShortcutState::Pressed {
                if let Some(w) = app.get_webview_window("main") {
                    if event == "show-dashboard" {
                        w.show().ok();
                        w.set_focus().ok();
                    } else {
                        w.emit(event, ()).ok();
                    }
                }
            }
        })
        .map_err(|e| log::warn!("Failed to register {}: {}", accel, e))
        .is_ok()
}

/// "CmdOrCtrl+Shift+X" → "Ctrl + Shift + X" for display.
fn display_accel(accel: &str) -> String {
    accel.replace("CmdOrCtrl", "Ctrl").replace('+', " + ")
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

    // A corrupt settings file must not brick saving forever — keep the path.
    let settings = Settings::load(&settings_path).unwrap_or_else(|e| {
        log::warn!("Settings load failed ({}), starting fresh", e);
        Settings::new(settings_path.clone())
    });
    let license_key = settings.get_str("license_key");
    let cleanup_level = settings.get_str("text_cleanup_level").unwrap_or_else(|| "standard".into());
    let model_pref = settings.get_str("whisper_model");

    // Spawn whisper-server immediately so the model is warm by the time the
    // user dictates. Loading happens on a background thread.
    let whisper_engine = Arc::new(WhisperEngine::new(&data_dir));
    whisper_engine.start(model_pref.as_deref());

    let app_state = Arc::new(AppState {
        db: Mutex::new(TranscriptDB::open(&db_path).expect("Failed to open database")),
        settings: Mutex::new(settings),
        license: Mutex::new(LicenseManager::new(license_key)),
        cleanup: Mutex::new(TextCleanupEngine::new(&cleanup_level)),
        recorder: RecorderHandle::new(),
        whisper: Arc::clone(&whisper_engine),
        paste_target: Mutex::new(None),
        hotkeys: Mutex::new(serde_json::Value::Null),
        cancel_accel: Mutex::new(None),
        data_dir: data_dir.clone(),
    });

    // Sweep recordings orphaned by a crash or force-kill. Age-gated so a
    // second running instance's in-flight recording is left alone.
    if let Ok(entries) = std::fs::read_dir(&data_dir) {
        let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("recording_") && name.ends_with(".wav") {
                let stale = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .map(|t| t < cutoff)
                    .unwrap_or(false);
                if stale {
                    std::fs::remove_file(entry.path()).ok();
                }
            }
        }
    }

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(app_state.clone())
        .on_window_event(|window, event| {
            // X closes to tray; only the tray menu's Quit exits the app.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                window.hide().ok();

                let app = window.app_handle();
                let state = app.state::<Arc<AppState>>();
                let already_notified = {
                    let mut s = state.settings.lock();
                    let shown = s.get_bool("tray_notice_shown") == Some(true);
                    if !shown {
                        s.set("tray_notice_shown", serde_json::Value::Bool(true));
                        s.save().ok();
                    }
                    shown
                };
                if !already_notified {
                    use tauri_plugin_notification::NotificationExt;
                    app.notification()
                        .builder()
                        .title("ALAN Echo is still running")
                        .body("Hotkeys stay active in the background. Right-click the tray icon to quit.")
                        .show()
                        .ok();
                }
            }
        })
        .setup(move |app| {
            // System tray
            let show = MenuItem::with_id(app, "show", "Show Dashboard", true, None::<&str>)?;
            let dictate = MenuItem::with_id(app, "dictate", "Start Dictation", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit ALAN Echo", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &dictate, &quit])?;

            let tray_icon = image::load_from_memory(include_bytes!("../icons/icon.png"))
                .ok()
                .map(|img| {
                    let rgba = img.to_rgba8();
                    let (w, h) = rgba.dimensions();
                    tauri::image::Image::new_owned(rgba.into_raw(), w, h)
                })
                .or_else(|| app.default_window_icon().cloned());

            let mut tray_builder = TrayIconBuilder::new()
                .tooltip("ALAN Echo — Ready")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        button_state: tauri::tray::MouseButtonState::Up,
                        ..
                    } = event
                    {
                        if let Some(w) = tray.app_handle().get_webview_window("main") {
                            w.show().ok();
                            w.set_focus().ok();
                        }
                    }
                })
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
                });
            if let Some(icon) = tray_icon {
                tray_builder = tray_builder.icon(icon);
            }
            tray_builder.build(app)?;

            // Global shortcuts. Ctrl+Shift+Escape is reserved by Windows
            // (Task Manager), so cancel uses X with a Backspace fallback.
            // Cancel is only PROBED here — it gets registered for the duration
            // of each recording so Echo doesn't steal it system-wide while idle.
            let handle = app.handle();
            let toggle_ok = register_emit_hotkey(handle, "CmdOrCtrl+Shift+Space", "dictate-toggle");
            let cancel_accel = ["CmdOrCtrl+Shift+X", "CmdOrCtrl+Shift+Backspace"]
                .iter()
                .find(|a| {
                    if app.global_shortcut().register(**a).is_ok() {
                        app.global_shortcut().unregister(**a).ok();
                        true
                    } else {
                        false
                    }
                })
                .copied();
            let show_ok = register_emit_hotkey(handle, "CmdOrCtrl+Shift+H", "show-dashboard");

            {
                let state = app.state::<Arc<AppState>>();
                *state.cancel_accel.lock() = cancel_accel.map(|a| a.to_string());
                *state.hotkeys.lock() = serde_json::json!({
                    "toggle": if toggle_ok { Some(display_accel("CmdOrCtrl+Shift+Space")) } else { None },
                    "cancel": cancel_accel.map(display_accel),
                    "show": if show_ok { Some(display_accel("CmdOrCtrl+Shift+H")) } else { None },
                });
            }

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
            cancel_recording,
            discard_recording,
            is_recording,
            get_audio_level,
            transcribe,
            check_whisper_ready,
            get_engine_info,
            list_models,
            get_hotkey_info,
            clean_text,
            test_microphone,
            read_wav_base64,
        ])
        .build(tauri::generate_context!())
        .expect("error while building ALAN Echo");

    app.run(move |_app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            // Don't leave an orphaned whisper-server holding the model in VRAM.
            whisper_engine.shutdown();
        }
    });
}
