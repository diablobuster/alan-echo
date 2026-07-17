//! ALAN Echo — Rust-side dictation state machine.
//!
//! The hotkey's critical path must never depend on the webview. WebView2 can
//! be throttled, suspended (memory pressure / efficiency mode), or crashed
//! while Echo idles in the tray — v1.2.4/v1.2.5 patched around that with
//! anti-throttle flags and async commands but kept the
//! hotkey → emit → JS → invoke round-trip. This module removes it: the hotkey
//! handler drives start/stop/cancel/transcribe/paste entirely in Rust, and the
//! frontend merely mirrors state from `dictation` events.

use crate::AppState;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// Matches the UI's advertised 5:00 cap (the recorder itself has a 310s
/// hard backstop in audio.rs).
const MAX_RECORDING_SECS: u64 = 300;

/// A Starting/Stopping phase older than this is a wedged worker (e.g. a mic
/// driver hanging the cpal open). The next hotkey press resets to Idle and
/// recovers instead of being silently dropped forever — the failure mode that
/// used to require killing the app.
const TRANSITION_STALE_SECS: u64 = 10;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Phase {
    Idle,
    Starting,
    Recording,
    Stopping,
}

pub struct Dictation {
    phase: Mutex<(Phase, Instant)>,
    /// Bumped on every recording start; lets the max-duration watchdog verify
    /// it is stopping *its own* recording, not a later one.
    generation: AtomicU64,
    /// Transcriptions in flight (stop returns before transcription finishes).
    pending: AtomicU32,
    /// whisper-server exposes a single /inference endpoint — serialize.
    transcribe_lock: Mutex<()>,
}

impl Dictation {
    pub fn new() -> Self {
        Self {
            phase: Mutex::new((Phase::Idle, Instant::now())),
            generation: AtomicU64::new(0),
            pending: AtomicU32::new(0),
            transcribe_lock: Mutex::new(()),
        }
    }

    pub fn is_recording(&self) -> bool {
        matches!(self.phase.lock().0, Phase::Recording)
    }

    pub fn pending_count(&self) -> u32 {
        self.pending.load(Ordering::SeqCst)
    }

    /// Serializes all transcriptions (hotkey flow AND the transcribe command)
    /// against whisper-server's single /inference endpoint.
    pub fn transcribe_lock(&self) -> &Mutex<()> {
        &self.transcribe_lock
    }

    fn set_phase(&self, p: Phase) {
        *self.phase.lock() = (p, Instant::now());
    }
}

fn emit(app: &AppHandle, payload: serde_json::Value) {
    if let Some(w) = app.get_webview_window("main") {
        w.emit("dictation", payload).ok();
    }
}

fn emit_error(app: &AppHandle, message: String) {
    emit(app, serde_json::json!({ "type": "error", "message": message }));
}

fn emit_pending(app: &AppHandle, count: u32) {
    emit(app, serde_json::json!({ "type": "pending", "count": count }));
}

/// Toggle dictation from any thread (hotkey callback, tray menu, command).
/// Returns immediately; all blocking work happens on a worker thread.
pub fn toggle(app: &AppHandle) {
    let app = app.clone();
    std::thread::Builder::new()
        .name("dictation-toggle".into())
        .spawn(move || toggle_blocking(&app))
        .ok();
}

/// Cancel an in-progress recording (nothing reaches the transcriber).
pub fn cancel(app: &AppHandle) {
    let app = app.clone();
    std::thread::Builder::new()
        .name("dictation-cancel".into())
        .spawn(move || cancel_blocking(&app))
        .ok();
}

enum Action {
    Start,
    Stop,
    Ignore,
}

fn toggle_blocking(app: &AppHandle) {
    let state = Arc::clone(app.state::<Arc<AppState>>().inner());
    let action = {
        let mut ph = state.dictation.phase.lock();
        match ph.0 {
            Phase::Idle => {
                *ph = (Phase::Starting, Instant::now());
                Action::Start
            }
            Phase::Recording => {
                *ph = (Phase::Stopping, Instant::now());
                Action::Stop
            }
            Phase::Starting | Phase::Stopping => {
                // Debounce genuine double-presses, but never stay wedged.
                if ph.1.elapsed() > Duration::from_secs(TRANSITION_STALE_SECS) {
                    log::warn!("Dictation phase {:?} wedged for >{}s — resetting", ph.0, TRANSITION_STALE_SECS);
                    *ph = (Phase::Idle, Instant::now());
                }
                Action::Ignore
            }
        }
    };

    match action {
        Action::Start => start_flow(app, &state),
        Action::Stop => stop_flow(app, &state),
        Action::Ignore => {}
    }
}

fn start_flow(app: &AppHandle, state: &Arc<AppState>) {
    // Beep on the keypress, not after the mic cold-opens.
    crate::audio::play_beep(sound_enabled(state), crate::audio::Beep::Start);

    if let Err(e) = crate::require_license(state) {
        state.dictation.set_phase(Phase::Idle);
        emit_error(app, e);
        return;
    }

    match crate::begin_recording(app, state) {
        Ok(()) => {
            let gen = state.dictation.generation.fetch_add(1, Ordering::SeqCst) + 1;
            state.dictation.set_phase(Phase::Recording);
            emit(app, serde_json::json!({ "type": "recording-started" }));
            spawn_watchdog(app.clone(), gen);
        }
        Err(e) => {
            state.dictation.set_phase(Phase::Idle);
            emit_error(app, format!("Microphone error — {}", e));
        }
    }
}

/// Auto-stop at the 5:00 cap. Generation-checked so it can never stop a
/// recording that started after its own.
fn spawn_watchdog(app: AppHandle, gen: u64) {
    std::thread::Builder::new()
        .name("dictation-watchdog".into())
        .spawn(move || {
            std::thread::sleep(Duration::from_secs(MAX_RECORDING_SECS));
            let state = Arc::clone(app.state::<Arc<AppState>>().inner());
            let still_ours = {
                let ph = state.dictation.phase.lock();
                ph.0 == Phase::Recording && state.dictation.generation.load(Ordering::SeqCst) == gen
            };
            if still_ours {
                log::info!("Recording hit the {}s cap — auto-stopping", MAX_RECORDING_SECS);
                toggle_blocking(&app);
            }
        })
        .ok();
}

fn stop_flow(app: &AppHandle, state: &Arc<AppState>) {
    crate::audio::play_beep(sound_enabled(state), crate::audio::Beep::Stop);

    let stopped = crate::end_recording(app, state);
    state.dictation.set_phase(Phase::Idle);
    emit(app, serde_json::json!({ "type": "recording-stopped" }));

    let (rec, target) = match stopped {
        Ok(r) => r,
        Err(e) => {
            emit_error(app, format!("Recording failed — {}", e));
            return;
        }
    };

    let discard = |path: &Option<String>| {
        if let Some(p) = path {
            std::fs::remove_file(p).ok();
        }
    };

    if rec.duration_seconds < 0.5 {
        discard(&rec.wav_path);
        emit_error(app, "Recording too short — try a full sentence".into());
        return;
    }
    if !rec.has_speech {
        discard(&rec.wav_path);
        emit_error(app, "No speech detected — try again".into());
        return;
    }
    let Some(wav_path) = rec.wav_path else {
        emit_error(app, "Recording failed".into());
        return;
    };

    // Transcribe on this worker; phase is already Idle so the next dictation
    // can start while this one transcribes in the background.
    let count = state.dictation.pending.fetch_add(1, Ordering::SeqCst) + 1;
    emit_pending(app, count);
    let result = {
        let _serialize = state.dictation.transcribe_lock.lock();
        crate::transcribe_and_deliver(app, state, &wav_path, target)
    };
    let count = state.dictation.pending.fetch_sub(1, Ordering::SeqCst) - 1;
    emit_pending(app, count);

    match result {
        Ok(mut payload) => {
            payload["type"] = serde_json::Value::String("transcript".into());
            emit(app, payload);
        }
        Err(e) => emit_error(app, format!("Transcription failed — {}", e)),
    }
}

fn cancel_blocking(app: &AppHandle) {
    let state = Arc::clone(app.state::<Arc<AppState>>().inner());
    {
        let mut ph = state.dictation.phase.lock();
        if ph.0 != Phase::Recording {
            return;
        }
        *ph = (Phase::Stopping, Instant::now());
    }
    crate::unregister_cancel_hotkey(app, &state);
    if let Ok(result) = state.recorder.stop() {
        if let Some(path) = result.wav_path {
            std::fs::remove_file(path).ok();
        }
    }
    *state.paste_target.lock() = None;
    state.dictation.set_phase(Phase::Idle);
    emit(app, serde_json::json!({ "type": "recording-stopped" }));
}

fn sound_enabled(state: &AppState) -> bool {
    state.settings.lock().get_bool("sound_enabled").unwrap_or(true)
}
