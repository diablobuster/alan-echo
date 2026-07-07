#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod activation;
mod audio;
mod db;
mod license;
mod packs;
mod paste;
mod settings;
mod text_cleanup;
mod trial;
mod updater;
mod whisper;

use audio::{DeviceInfo, RecorderHandle};
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
    pub trial_state: Mutex<trial::TrialState>,
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
    let p = std::path::Path::new(&path);
    if p.components().any(|c| c == std::path::Component::ParentDir) {
        return Err("Export path must not contain '..'".into());
    }
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
                // Mutate in place so the user's find→replace rules survive a
                // level change (constructing a fresh engine would drop them).
                state.cleanup.lock().set_level(level);
            }
        }
        "text_replace_rules" => {
            state.cleanup.lock().set_rules(&parse_replace_rules(&value));
        }
        "whisper_model" => {
            state.whisper.reload(value.as_str());
        }
        "language" => {
            if let Some(lang) = value.as_str() {
                state.whisper.set_language(lang);
                state.whisper.reload(None);
            }
        }
        _ => {}
    }
    Ok(())
}

/// Parse the `text_replace_rules` setting — a JSON array of
/// `{"from": "...", "to": "..."}` — into (from, to) pairs. Malformed entries
/// and a missing/non-array value yield no rules (find→replace simply off).
fn parse_replace_rules(value: &serde_json::Value) -> Vec<(String, String)> {
    value.as_array().map(|arr| {
        arr.iter().filter_map(|item| {
            let from = item.get("from")?.as_str()?.to_string();
            let to = item.get("to").and_then(|v| v.as_str()).unwrap_or("").to_string();
            Some((from, to))
        }).collect()
    }).unwrap_or_default()
}

// ── License commands ─────────────────────────────────────────────────

#[tauri::command]
fn validate_license(state: State<Arc<AppState>>, key: String) -> Result<serde_json::Value, String> {
    let mut lm = state.license.lock();
    let (valid, msg) = lm.activate(&key);
    let mut persisted = true;
    if valid {
        let normalized = key.trim().to_uppercase().replace(' ', "");
        let mut s = state.settings.lock();
        s.set("license_key", serde_json::Value::String(normalized.clone()));
        if let Err(e) = s.save() {
            log::error!("License accepted but could not be saved: {}", e);
            persisted = false;
        }
        drop(s);
        drop(lm);
        // Auto-attempt Ed25519 online activation
        match activation::activate_online(&normalized, &state.data_dir) {
            Ok(_) => {
                return Ok(serde_json::json!({
                    "valid": true, "activated": true,
                    "message": "License activated", "persisted": persisted
                }));
            }
            Err(e) => {
                log::warn!("Online activation failed (key stored for retry): {}", e);
                let msg = if e.contains("ureq") || e.contains("connect") || e.contains("timeout") || e.contains("dns") {
                    "Key accepted — connect to the internet to complete activation".to_string()
                } else {
                    e.clone()
                };
                return Ok(serde_json::json!({
                    "valid": true, "activated": false,
                    "message": msg,
                    "persisted": persisted
                }));
            }
        }
    }
    Ok(serde_json::json!({ "valid": valid, "message": msg, "persisted": persisted }))
}

fn require_license(state: &AppState) -> Result<(), String> {
    if cfg!(debug_assertions) {
        return Ok(());
    }
    if state.license.lock().is_licensed() {
        return Ok(());
    }
    if activation::is_activated(&state.data_dir) {
        return Ok(());
    }
    match trial::check_trial(&state.trial_state.lock()) {
        trial::TrialStatus::Allowed => Ok(()),
        trial::TrialStatus::DailyLimitReached => {
            Err("Daily trial limit reached — resets tomorrow, or activate your license for unlimited dictation".into())
        }
        trial::TrialStatus::LifetimeExpired => {
            Err("Trial ended — 50 free dictations used. Purchase a license for unlimited dictation".into())
        }
    }
}

fn increment_trial_count(state: &AppState) {
    let mut ts = state.trial_state.lock();
    trial::increment(&mut ts);
    let mut s = state.settings.lock();
    trial::save(&ts, &mut s, &state.data_dir);
}

#[tauri::command]
fn get_trial_status(state: State<Arc<AppState>>) -> Result<serde_json::Value, String> {
    if state.license.lock().is_licensed() {
        return Ok(serde_json::json!({ "licensed": true }));
    }
    if activation::is_activated(&state.data_dir) {
        return Ok(serde_json::json!({ "licensed": true }));
    }
    Ok(trial::get_status_json(&state.trial_state.lock()))
}

#[tauri::command]
fn quit_app() {
    // EULA declined — exit cleanly before any engine/tray initialization matters.
    std::process::exit(0);
}

#[tauri::command]
fn check_license(state: State<Arc<AppState>>) -> Result<bool, String> {
    if cfg!(debug_assertions) {
        return Ok(true);
    }
    let activated = activation::is_activated(&state.data_dir);
    // Housekeeping, never gating: if the activation token is missing, expired
    // (tokens carry a 400-day exp), or no longer verifies after a machine-
    // fingerprint change, refresh it in the background with the saved key.
    // Previously this was gated behind is_licensed(), which is always false
    // (format-only check) — so the self-heal never ran, leaving expired and
    // post-fingerprint-change tokens un-refreshed. Gate on "a saved key exists"
    // instead. Failures only log — a paying user always boots.
    if !activated {
        if let Some(key) = state.settings.lock().get_str("license_key") {
            let data_dir = state.data_dir.clone();
            std::thread::spawn(move || {
                if let Err(e) = activation::activate_online(&key, &data_dir) {
                    log::info!("Silent re-activation failed (will retry next launch): {}", e);
                }
            });
        }
    }
    Ok(activated)
}

#[tauri::command]
fn activate_online(state: State<Arc<AppState>>, key: String) -> Result<serde_json::Value, String> {
    let token = activation::activate_online(&key, &state.data_dir)?;
    Ok(serde_json::json!({ "activated": true, "token_length": token.len() }))
}

#[tauri::command]
fn get_activation_status(state: State<Arc<AppState>>) -> Result<serde_json::Value, String> {
    let licensed = state.license.lock().is_licensed();
    let activated = activation::is_activated(&state.data_dir);
    let fingerprint = activation::machine_fingerprint();
    Ok(serde_json::json!({
        "licensed": licensed,
        "activated": activated,
        "fingerprint": fingerprint,
    }))
}

// ── Audio commands ───────────────────────────────────────────────────

#[tauri::command]
fn list_audio_devices() -> Result<Vec<DeviceInfo>, String> {
    audio::list_input_devices()
}

#[tauri::command]
async fn start_recording(app: tauri::AppHandle, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    // Run the blocking recorder work OFF the main/event-loop thread. A
    // *synchronous* Tauri command runs on the main thread, and recorder.start()
    // blocks until the cpal/WASAPI input stream cold-opens (hundreds of ms). On
    // Windows the main thread is the loop that pumps WM_HOTKEY, so that block
    // froze global-shortcut delivery: a second press queued during the freeze
    // fired only *after* the command returned (status already 'recording') and
    // immediately stopped the just-started recording — the "press twice and it
    // cancels itself" symptom. spawn_blocking keeps the event loop free.
    let state = Arc::clone(state.inner());
    tokio::task::spawn_blocking(move || {
        require_license(&state)?;

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
            register_cancel_hotkey_on_main(&app, accel);
        }
        Ok(())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

/// Register the per-recording cancel hotkey from a worker thread by marshaling
/// the call back to the main thread. The global-shortcut plugin's hotkey
/// message window is created on the main thread in setup(); (un)registering
/// from a spawn_blocking worker could bind the hotkey to the wrong thread, so
/// always run it on the main thread. Posting is non-blocking.
fn register_cancel_hotkey_on_main(app: &tauri::AppHandle, accel: String) {
    let app_handle = app.clone();
    app.run_on_main_thread(move || {
        register_emit_hotkey(&app_handle, &accel, "dictate-cancel");
    })
    .ok();
}

fn unregister_cancel_hotkey(app: &tauri::AppHandle, state: &AppState) {
    let Some(accel) = state.cancel_accel.lock().clone() else {
        return;
    };
    // Marshal back to the main thread (see register_cancel_hotkey_on_main) —
    // stop/cancel now run on a spawn_blocking worker.
    let app_handle = app.clone();
    app.run_on_main_thread(move || {
        app_handle.global_shortcut().unregister(accel.as_str()).ok();
    })
    .ok();
}

#[tauri::command]
async fn stop_recording(app: tauri::AppHandle, state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    // Off the main thread: recorder.stop() blocks on resample + WAV encode +
    // write (tens-to-hundreds of ms for a long clip), which otherwise froze the
    // hotkey pump and delayed deactivation.
    //
    // Take the paste target HERE (at stop), while it still unambiguously belongs
    // to THIS recording, and hand it back to the frontend. The frontend then
    // transcribes in the background and passes the target to transcribe() — so a
    // NEW dictation can start (overwriting the shared paste_target slot) before
    // this one's transcription finishes, without pasting into the wrong window.
    let state = Arc::clone(state.inner());
    tokio::task::spawn_blocking(move || {
        unregister_cancel_hotkey(&app, &state);
        let r = state.recorder.stop()?;
        let target = state.paste_target.lock().take();
        Ok(serde_json::json!({
            "wav_path": r.wav_path,
            "duration_seconds": r.duration_seconds,
            "has_speech": r.has_speech,
            "paste_target": target,
        }))
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

#[tauri::command]
async fn cancel_recording(app: tauri::AppHandle, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let state = Arc::clone(state.inner());
    tokio::task::spawn_blocking(move || {
        unregister_cancel_hotkey(&app, &state);
        if let Ok(result) = state.recorder.stop() {
            // Discard the recording — nothing should reach the transcriber.
            if let Some(path) = result.wav_path {
                std::fs::remove_file(path).ok();
            }
        }
        *state.paste_target.lock() = None;
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))
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
async fn transcribe(state: State<'_, Arc<AppState>>, app: tauri::AppHandle, wav_path: String, paste_target: Option<isize>) -> Result<serde_json::Value, String> {
    require_license(&state)?;
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

        if !state.license.lock().is_licensed() {
            increment_trial_count(&state);
        }

        // The target was captured at recording start and handed back by
        // stop_recording, so it belongs to THIS recording even if a newer
        // dictation has since started and overwritten the shared slot.
        let pasted = deliver_text(&app, &state, &cleaned, paste_target);

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
fn deliver_text(app: &tauri::AppHandle, state: &AppState, text: &str, target: Option<isize>) -> bool {
    use tauri_plugin_clipboard_manager::ClipboardExt;

    let auto_paste = state.settings.lock().get_bool("auto_paste").unwrap_or(true);

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
    #[cfg(target_os = "macos")]
    {
        if hwnd == std::process::id() as isize {
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

#[tauri::command]
fn has_multilingual_model(state: State<Arc<AppState>>) -> bool {
    state.whisper.has_multilingual_model()
}

#[tauri::command]
fn download_multilingual_model(app: tauri::AppHandle, state: State<Arc<AppState>>) -> Result<(), String> {
    download_model(app, state, "base-multi".into())
}

#[tauri::command]
fn download_model(app: tauri::AppHandle, state: State<Arc<AppState>>, name: String) -> Result<(), String> {
    let filename = match name.as_str() {
        "base" => "ggml-base.en.bin",
        "base-multi" => "ggml-base.bin",
        "small" => "ggml-small.en.bin",
        "medium" => "ggml-medium.en.bin",
        "large-v3" => "ggml-large-v3.bin",
        _ => return Err(format!("Unknown model: {}", name)),
    };
    if state.whisper.model_available(&name) {
        return Ok(());
    }
    let data_dir = state.data_dir.clone();
    let fname = filename.to_string();
    std::thread::Builder::new()
        .name("model-download".into())
        .spawn(move || {
            if let Err(e) = download_model_file(&app, &data_dir, &fname) {
                let _ = app.emit("model_download_progress", serde_json::json!({
                    "stage": "error", "error": e
                }));
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn download_model_file(app: &tauri::AppHandle, data_dir: &std::path::Path, filename: &str) -> Result<(), String> {
    use std::io::{Read as _, Write as _};

    let models_dir = data_dir.join("models");
    std::fs::create_dir_all(&models_dir).map_err(|e| format!("Couldn't create models dir: {}", e))?;

    let dest = models_dir.join(filename);
    let partial = models_dir.join(format!("{}.partial", filename));

    let url = format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}", filename);
    app.emit("model_download_progress", serde_json::json!({
        "stage": "downloading", "percent": 0
    })).ok();

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(15))
        .timeout_read(std::time::Duration::from_secs(60))
        .build();
    let resp = agent.get(&url).call()
        .map_err(|e| format!("Download failed: {}", e))?;

    let total: u64 = resp.header("Content-Length").and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut reader = resp.into_reader();
    let mut file = std::fs::File::create(&partial)
        .map_err(|e| format!("Couldn't create file: {}", e))?;

    let mut downloaded: u64 = 0;
    let mut buf = [0u8; 65536];
    let mut last_pct: u32 = 0;
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("Download interrupted: {}", e))?;
        if n == 0 { break; }
        file.write_all(&buf[..n]).map_err(|e| format!("Write error: {}", e))?;
        downloaded += n as u64;
        if total > 0 {
            let pct = (downloaded as f64 / total as f64 * 100.0) as u32;
            if pct > last_pct {
                last_pct = pct;
                app.emit("model_download_progress", serde_json::json!({
                    "stage": "downloading", "percent": pct,
                    "downloaded_mb": downloaded / (1024 * 1024),
                    "total_mb": total / (1024 * 1024),
                })).ok();
            }
        }
    }
    file.flush().map_err(|e| format!("Flush error: {}", e))?;
    drop(file);

    if downloaded < 50_000_000 {
        std::fs::remove_file(&partial).ok();
        return Err("Download too small — try again".into());
    }

    std::fs::rename(&partial, &dest)
        .map_err(|e| format!("Couldn't finalize model file: {}", e))?;

    app.emit("model_download_progress", serde_json::json!({ "stage": "done" })).ok();
    log::info!("Multilingual model downloaded ({} MB)", downloaded / (1024 * 1024));
    Ok(())
}

// ── Text cleanup ─────────────────────────────────────────────────────

#[tauri::command]
fn clean_text(state: State<Arc<AppState>>, text: String) -> Result<String, String> {
    Ok(state.cleanup.lock().clean(&text))
}

// ── Auto-updater ────────────────────────────────────────────────────

#[tauri::command]
fn check_for_update(app: tauri::AppHandle, state: State<Arc<AppState>>) -> Result<updater::UpdateInfo, String> {
    let version = app.config().version.clone().unwrap_or_else(|| "0.0.0".to_string());
    let mut info = updater::check_for_update(&version)?;
    // Construct authenticated download URL from stored key
    if info.download_url.is_none() && info.available {
        let key = state.settings.lock().get_str("license_key");
        if let Some(k) = key {
            // Percent-encode defensively — the key is normally [A-Z0-9-], but a
            // raw interpolation into a URL is a latent correctness/leak gap.
            let enc: String = k
                .bytes()
                .map(|b| match b {
                    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                        (b as char).to_string()
                    }
                    _ => format!("%{:02X}", b),
                })
                .collect();
            info.download_url = Some(format!(
                "https://www.alanglobalintelligence.com/api/echo/download?key={}",
                enc
            ));
        }
    }
    Ok(info)
}

#[tauri::command]
fn download_update(app: tauri::AppHandle, state: State<Arc<AppState>>, download_url: String, expected_sha256: Option<String>) -> Result<(), String> {
    let data_dir = state.data_dir.clone();
    std::thread::spawn(move || {
        if let Err(e) = updater::download_and_launch_update(&app, &download_url, expected_sha256.as_deref(), &data_dir) {
            let _ = app.emit("update_progress", serde_json::json!({ "stage": "error", "error": e }));
        }
    });
    Ok(())
}

// ── Autostart ───────────────────────────────────────────────────────

#[tauri::command]
fn get_autostart(app: tauri::AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
fn set_autostart(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let mgr = app.autolaunch();
    if enabled {
        mgr.enable().map_err(|e| e.to_string())
    } else {
        mgr.disable().map_err(|e| e.to_string())
    }
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

/// "CmdOrCtrl+Shift+X" → "Cmd + Shift + X" (Mac) or "Ctrl + Shift + X" (Windows).
fn display_accel(accel: &str) -> String {
    let modifier = if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl" };
    accel.replace("CmdOrCtrl", modifier).replace('+', " + ")
}

/// Re-insert the most recent transcript into whatever window is focused right
/// now — no re-recording. The capture happens here in the backend because at
/// hotkey time the user's target app is focused (not Echo's window); routing it
/// through a JS event round-trip would capture Echo instead. Reuses
/// deliver_text, so it honors the auto_paste setting and does the UIPI-safe
/// paste + clipboard restore. (NOTE: with auto_paste off this only re-copies to
/// the clipboard — an explicit-paste override is a candidate follow-up.)
fn paste_last_transcript(app: &tauri::AppHandle) {
    let state = app.state::<Arc<AppState>>();
    let state = Arc::clone(state.inner());

    let newest = state.db.lock()
        .get_page(0, 1)
        .ok()
        .and_then(|(rows, _)| rows.into_iter().next())
        .map(|t| t.text)
        .filter(|t| !t.trim().is_empty());
    let Some(text) = newest else {
        log::info!("paste-last: no transcript to re-paste");
        return;
    };

    // The window focused at the instant the hotkey fired is the paste target.
    // Pass it directly — never via the shared state.paste_target mutex, which
    // belongs to the dictation flow and could be mid-transcription right now.
    let pasted = deliver_text(app, &state, &text, Some(paste::foreground_window()));
    if let Some(w) = app.get_webview_window("main") {
        w.emit("paste-last", serde_json::json!({ "pasted": pasted })).ok();
    }
}

fn register_paste_last_hotkey(app: &tauri::AppHandle, accel: &str) -> bool {
    use tauri_plugin_global_shortcut::ShortcutState;
    app.global_shortcut()
        .on_shortcut(accel, move |app, _shortcut, ev| {
            if ev.state == ShortcutState::Pressed {
                paste_last_transcript(app);
            }
        })
        .map_err(|e| log::warn!("Failed to register paste-last {}: {}", accel, e))
        .is_ok()
}

// ── Main ─────────────────────────────────────────────────────────────

fn setup_logging(data_dir: &std::path::Path) {
    let log_path = data_dir.join("echo.log");

    // Rotate: if the log file exceeds 5 MB, truncate it before opening.
    if let Ok(meta) = std::fs::metadata(&log_path) {
        if meta.len() > 5 * 1024 * 1024 {
            let _ = std::fs::write(&log_path, "");
        }
    }

    let mut dispatch = fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "{} [{}] {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.level(),
                message,
            ))
        })
        .level(log::LevelFilter::Info);

    if let Ok(file) = fern::log_file(&log_path) {
        dispatch = dispatch.chain(file);
    }
    #[cfg(debug_assertions)]
    {
        dispatch = dispatch.chain(std::io::stderr());
    }
    dispatch.apply().ok();
}

fn show_fatal_error(message: &str) {
    #[cfg(target_os = "windows")]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        let wide_msg: Vec<u16> = OsStr::new(message).encode_wide().chain(Some(0)).collect();
        let wide_title: Vec<u16> = OsStr::new("ALAN Echo").encode_wide().chain(Some(0)).collect();
        unsafe {
            #[link(name = "user32")]
            extern "system" {
                fn MessageBoxW(hwnd: *mut std::ffi::c_void, text: *const u16, caption: *const u16, typ: u32) -> i32;
            }
            MessageBoxW(std::ptr::null_mut(), wide_msg.as_ptr(), wide_title.as_ptr(), 0x10);
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        eprintln!("[ALAN Echo FATAL] {}", message);
    }
}

fn main() {
    // Keep the dictation hotkey and its start/stop beep responsive even when
    // Echo is hidden in the tray (its normal resident state). WebView2/Chromium
    // throttles JS, timers, and injected-event delivery for occluded or
    // minimized windows (native window-occlusion tracking + background-timer
    // throttling), which otherwise delays the 'dictate-toggle' event reaching
    // the webview and the beep that confirms it. These switches disable that
    // throttling. Use the env var rather than the additionalBrowserArgs config:
    // WebView2 *appends* the env var to wry's defaults, whereas the config arg
    // *replaces* them and would silently drop wry's
    // --disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection. Must be set
    // before any webview environment is created — and before any thread is
    // spawned below, since set_var is not thread-safe.
    #[cfg(target_os = "windows")]
    std::env::set_var(
        "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
        "--disable-background-timer-throttling --disable-renderer-backgrounding --disable-backgrounding-occluded-windows --disable-features=CalculateNativeWinOcclusion",
    );

    // A corrupt engine binary (e.g. a damaged GPU pack) must make CreateProcess
    // FAIL, not hang the spawn behind a modal "Unsupported 16-Bit Application"
    // system dialog. Error mode is per-process and inherited by children.
    #[cfg(target_os = "windows")]
    unsafe {
        #[link(name = "kernel32")]
        extern "system" {
            fn SetErrorMode(mode: u32) -> u32;
        }
        SetErrorMode(0x0001 | 0x8000); // SEM_FAILCRITICALERRORS | SEM_NOOPENFILEERRORBOX
    }

    let data_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("ALAN Echo");
    std::fs::create_dir_all(&data_dir).ok();
    setup_logging(&data_dir);
    std::fs::create_dir_all(data_dir.join("backups")).ok();
    std::fs::create_dir_all(data_dir.join("models")).ok();

    // Warm the machine-fingerprint cache off the hotkey path. The first license
    // check (inside start_recording) otherwise computes it lazily by spawning
    // three PowerShell/WMI processes (~2s warm, far worse cold), stalling the
    // first beep. Computing it once here, in parallel with the rest of startup,
    // means the cache is hot well before any hotkey press. machine_fingerprint()
    // memoizes, so this is the only place that pays the cost.
    std::thread::spawn(|| {
        let _ = activation::machine_fingerprint();
    });

    let db_path = data_dir.join("transcripts.db");
    let settings_path = data_dir.join("settings.json");

    // A corrupt settings file must not brick saving forever — and it must not
    // silently discard the license key. Try the backup copy first; only then
    // fall back to fresh settings.
    let settings_backup = data_dir.join("backups").join("settings.json");
    let settings = Settings::load(&settings_path)
        .or_else(|e| {
            log::warn!("Settings load failed ({}), trying backup", e);
            if settings_backup.exists() {
                std::fs::copy(&settings_backup, &settings_path)
                    .map_err(|c| -> Box<dyn std::error::Error> { Box::new(c) })?;
                Settings::load(&settings_path)
            } else {
                Err(e)
            }
        })
        .unwrap_or_else(|e| {
            log::warn!("Settings load failed ({}), starting fresh", e);
            let mut fresh = Settings::new(settings_path.clone());
            if let Ok(raw) = std::fs::read_to_string(&settings_path) {
                if let Some(start) = raw.find("\"license_key\"") {
                    if let Some(colon) = raw[start..].find(':') {
                        let after = &raw[start + colon + 1..];
                        let trimmed = after.trim_start();
                        if trimmed.starts_with('"') {
                            if let Some(end) = trimmed[1..].find('"') {
                                let key = &trimmed[1..1 + end];
                                if !key.is_empty() {
                                    log::info!("Salvaged license_key from corrupt settings");
                                    fresh.set("license_key", serde_json::Value::String(key.to_string()));
                                    fresh.save().ok();
                                }
                            }
                        }
                    }
                }
            }
            fresh
        });
    // Keep a daily-ish backup next to the transcript backups — settings.json
    // holds the license key, the only file whose loss costs the customer.
    if settings_path.exists() {
        std::fs::copy(&settings_path, &settings_backup).ok();
    }
    let license_key = settings.get_str("license_key");
    let cleanup_level = settings.get_str("text_cleanup_level").unwrap_or_else(|| "standard".into());
    let replace_rules = settings.get("text_replace_rules")
        .map(parse_replace_rules)
        .unwrap_or_default();
    let model_pref = settings.get_str("whisper_model");
    let language = settings.get_str("language").unwrap_or_else(|| "en".into());

    let whisper_engine = Arc::new(WhisperEngine::new(&data_dir));
    whisper_engine.set_language(&language);
    whisper_engine.start(model_pref.as_deref());

    let initial_trial = trial::load(&settings, &data_dir);

    let app_state = Arc::new(AppState {
        db: Mutex::new(TranscriptDB::open(&db_path).unwrap_or_else(|e| {
            log::error!("Failed to open database: {}", e);
            show_fatal_error(&format!("ALAN Echo could not open its database.\n\n{}\n\nTry deleting:\n{}", e, db_path.display()));
            std::process::exit(1);
        })),
        settings: Mutex::new(settings),
        license: Mutex::new(LicenseManager::new(license_key)),
        cleanup: Mutex::new({
            let mut engine = TextCleanupEngine::new(&cleanup_level);
            engine.set_rules(&replace_rules);
            engine
        }),
        recorder: RecorderHandle::new(),
        whisper: Arc::clone(&whisper_engine),
        paste_target: Mutex::new(None),
        hotkeys: Mutex::new(serde_json::Value::Null),
        cancel_accel: Mutex::new(None),
        trial_state: Mutex::new(initial_trial),
        data_dir: data_dir.clone(),
    });

    // Migrate old trial keys to signed blob on first run
    {
        let state_ref = &app_state;
        let s = state_ref.settings.lock();
        if s.get_str("trial_date").is_some() && s.get_str(trial::SETTINGS_KEY).is_none() {
            drop(s);
            let ts = state_ref.trial_state.lock().clone();
            let mut s = state_ref.settings.lock();
            trial::save(&ts, &mut s, &data_dir);
            trial::cleanup_old_keys(&mut s);
        }
    }

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
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                w.show().ok();
                w.set_focus().ok();
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(app_state.clone())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let state = app.state::<Arc<AppState>>();
                let tray_ok = state.settings.lock().get_bool("_tray_ok") == Some(true);
                if !tray_ok {
                    return;
                }
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
            match tray_builder.build(app) {
                Ok(_) => {
                    let state = app.state::<Arc<AppState>>();
                    let mut s = state.settings.lock();
                    s.set("_tray_ok", serde_json::Value::Bool(true));
                    s.save().ok();
                },
                Err(e) => log::warn!("Failed to create tray icon: {}", e),
            }

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
            // Re-paste the most recent transcript into the focused app. Bound
            // globally (active while idle) so it works from any app. NOTE:
            // Ctrl+Shift+V is also "paste without formatting" in many
            // terminals/editors — registering it globally intercepts that combo
            // system-wide. Kept as the intuitive default; revisit once hotkeys
            // are user-rebindable.
            //
            // Windows-only for now: paste-last fires synchronously while the
            // user may still hold Shift, and only the Windows paste path
            // releases it. On macOS the held Shift would turn Cmd+V into
            // Cmd+Shift+V ("Paste and Match Style") — re-enable once the macOS
            // Shift-release lands (docs/2026-06-17-slice8-macos-parity-spec.md §8d).
            let paste_last_ok = if cfg!(target_os = "windows") {
                register_paste_last_hotkey(handle, "CmdOrCtrl+Shift+V")
            } else {
                false
            };

            {
                let state = app.state::<Arc<AppState>>();
                *state.cancel_accel.lock() = cancel_accel.map(|a| a.to_string());
                *state.hotkeys.lock() = serde_json::json!({
                    "toggle": if toggle_ok { Some(display_accel("CmdOrCtrl+Shift+Space")) } else { None },
                    "cancel": cancel_accel.map(display_accel),
                    "show": if show_ok { Some(display_accel("CmdOrCtrl+Shift+H")) } else { None },
                    "pasteLast": if paste_last_ok { Some(display_accel("CmdOrCtrl+Shift+V")) } else { None },
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

            // Ensure the window is visible on startup
            if let Some(window) = app.get_webview_window("main") {
                window.show().ok();
                window.set_focus().ok();
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
            quit_app,
            validate_license,
            check_license,
            get_trial_status,
            activate_online,
            get_activation_status,
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
            has_multilingual_model,
            download_multilingual_model,
            download_model,
            list_models,
            get_hotkey_info,
            clean_text,
            test_microphone,
            read_wav_base64,
            packs::get_gpu_pack_status,
            packs::download_gpu_pack,
            packs::test_gpu,
            check_for_update,
            download_update,
            get_autostart,
            set_autostart,
        ])
        .build(tauri::generate_context!())
        .unwrap_or_else(|e| {
            log::error!("Failed to build ALAN Echo: {}", e);
            let hint = if cfg!(target_os = "windows") {
                "This usually means WebView2 is missing or corrupted.\nReinstall WebView2 from Microsoft, then try again."
            } else {
                "Try reinstalling the app or check Console.app for details."
            };
            show_fatal_error(&format!("ALAN Echo failed to start.\n\n{}\n\n{}", e, hint));
            std::process::exit(1);
        });

    app.run(move |_app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            // Don't leave an orphaned whisper-server holding the model in VRAM.
            whisper_engine.shutdown();
        }
    });
}
