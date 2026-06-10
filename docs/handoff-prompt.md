# ALAN Echo — Comprehensive Handoff Prompt

> Use this prompt to hand off ALAN Echo to another AI for audit-fix-audit-fix cycles until the product is ship-ready.

---

## Project Overview

**ALAN Echo** is a local, offline voice-to-text dictation desktop app for Windows, built with **Tauri 2** (Rust backend + React frontend). It captures audio, transcribes via whisper.cpp, cleans up the text with a rule-based NLP engine, and pastes the result into the focused application.

**Repository:** `C:\Users\arowm\alan-echo\`
**Tech stack:** Tauri 2.11, Rust 1.95, React 18, Vite 8, whisper.cpp v1.8.6
**GPU:** NVIDIA RTX 4060 (8GB VRAM, CUDA 13.1)
**Whisper binary:** CUDA-accelerated (`whisper-cublas-12.4.0-bin-x64`)
**Models available:** ggml-medium.bin (1.5GB), ggml-large-v3.bin (3.1GB)
**App data:** `%APPDATA%/ALAN Echo/` (models, db, settings, backups, recordings)

---

## Architecture

```
alan-echo/
├── src/                          # React frontend (Vite)
│   ├── main.jsx                  # App flow: splash → dashboard
│   ├── tokens.css                # Design tokens (warm light-mode palette)
│   └── components/
│       ├── Dashboard.jsx         # Main UI: dictation state machine, transcript list
│       ├── Splash.jsx            # Loading screen
│       ├── LicenseGate.jsx       # License key activation (currently bypassed)
│       ├── SettingsPanel.jsx     # Engine, mic, behavior, hotkeys
│       ├── TitleBar.jsx          # Custom ALAN wordmark + status + window controls
│       ├── StatusPanel.jsx       # Ready/Recording/Processing states
│       ├── QuickStats.jsx        # Stats bar
│       ├── SearchBar.jsx         # FTS search
│       ├── TranscriptCard.jsx    # Card component
│       ├── DetailPanel.jsx       # Full text view
│       ├── FooterBar.jsx         # Hotkey hints + privacy badge
│       ├── Icons.jsx             # SVG icons + Monogram
│       └── logoData.js           # Base64-embedded ALAN mark PNGs
├── src-tauri/                    # Rust backend
│   ├── src/
│   │   ├── main.rs              # Tauri commands, tray, hotkeys, setup
│   │   ├── audio.rs             # cpal recording (dedicated thread, Send-safe)
│   │   ├── db.rs                # SQLite + FTS5
│   │   ├── license.rs           # HMAC license keys + machine binding
│   │   ├── text_cleanup.rs      # Rule-based NLP cleanup
│   │   ├── whisper.rs           # whisper.cpp CLI sidecar
│   │   └── settings.rs          # JSON persistence
│   ├── tauri.conf.json          # App config
│   ├── capabilities/default.json # Permissions
│   └── icons/                   # App icons (ALAN mark)
├── public/assets/               # ALAN mark PNGs
└── package.json                 # Node dependencies
```

---

## Current State — What Works

1. **Window launches** with ALAN branding, custom title bar, system tray icon
2. **Ctrl+Shift+Space** toggles recording (global hotkey via Tauri plugin)
3. **Audio recording** works via cpal on a dedicated thread (F32/I16/U16 format handling)
4. **Whisper transcription** works via whisper.cpp CLI sidecar (GPU-accelerated, ~2s after model cached)
5. **Text cleanup engine** processes raw Whisper output (filler removal, capitalization, etc.)
6. **SQLite + FTS5** stores transcripts with full-text search
7. **Dashboard UI** shows transcript cards, detail panel, search, stats, export
8. **Settings panel** opens with engine/mic/behavior/hotkey sections
9. **Beep sounds** play on record start/stop via Web Audio API
10. **System tray** with menu (Show Dashboard, Start Dictation, Quit)
11. **Auto-backup** runs daily (keeps 7)

## Known Issues to Fix

### CRITICAL — Must Fix

1. **First transcription is slow (~14s)** because whisper.cpp CLI reloads the model from disk every time. The fix is to run `whisper-server` (persistent HTTP server mode) instead of `whisper-cli` (one-shot CLI). whisper.cpp ships with `whisper-server.exe` which loads the model once and serves transcription requests via HTTP. This would make every transcription ~2s including the first one.

2. **Model switching in Settings doesn't work.** The UI shows Standard/Enhanced/Ultra but changing the selection only saves a preference — it doesn't tell the whisper engine to use a different model. The `whisper.rs` module's `find_model()` function picks the first available model file, ignoring the user's preference. Fix: pass the selected model name from settings to the whisper engine.

3. **License key system is bypassed** (`check_license` returns `Ok(true)` unconditionally). The HMAC key generation in Python doesn't produce keys that the Rust validator accepts because the checksum algorithms differ slightly. Fix: ensure `compute_check()` in license.rs produces the same output as the Python keygen, then re-enable the license gate.

4. **Ctrl+Shift+Escape (cancel) fails to register** because another app holds that shortcut. Need a fallback hotkey or show a warning in the UI. Consider changing to `Ctrl+Shift+X` or `Ctrl+Shift+Backspace`.

5. **No auto-paste after transcription.** The Dashboard copies text to clipboard but doesn't simulate Ctrl+V. Need to either: (a) use Tauri's shell plugin to simulate keypress, or (b) implement a Rust-side Ctrl+V injection using Windows SendInput API.

### HIGH — Feature Gaps

6. **Settings changes don't persist across restarts.** The `set_setting` command saves to JSON, but the app doesn't re-read settings on startup for all values. Verify: mic device, cleanup level, sound toggle, auto-paste toggle all load from saved settings.

7. **Export button says "coming soon."** Wire it to a file-save dialog (`@tauri-apps/plugin-dialog`) → call `export_transcripts` with the user-selected path and format.

8. **Window close (X) should minimize to tray, not quit.** Currently the window just hides. Add proper "X minimizes, tray Quit actually quits" behavior. Show a one-time notification "ALAN Echo is still running in the background."

9. **No first-run onboarding.** New users need: (a) mic permission/selection, (b) a test recording, (c) hotkey tutorial. Build a simple 3-step wizard shown once on first launch.

10. **Whisper server mode** (see #1). The `whisper-server.exe` binary is already downloaded in the models directory. Change `whisper.rs` to: start whisper-server on app launch (port 8178), send transcription requests via HTTP POST, keep model loaded permanently. This eliminates the ~5s model reload on first transcription.

### MEDIUM — Polish

11. **GPU auto-detection in Settings.** The "Status: Engine ready" text should show the GPU name and whether CUDA is active. Read this from the whisper.cpp output or from the `nvidia-smi` command.

12. **Recording timer doesn't show in title bar.** When recording, the title bar status pill shows "Recording" but no elapsed time. Pass the elapsed time to the TitleBar component.

13. **Transcript editing.** Clicking on a transcript in the detail panel should allow inline text editing. The `update_transcript` Tauri command already exists — just need a UI text editor with save/cancel.

14. **Cleanup level feedback.** When changing text cleanup level, show a before/after preview of a sample transcript so the user understands the difference.

15. **Dark mode.** The app uses a warm light-mode design. Add a dark mode toggle that swaps CSS variables. The Claude Design handoff includes dark mode tokens from the original ALAN design system.

### LOW — Cosmetic

16. **SettingsPanel slide animation** uses vertical rise (`echo-rise`) instead of horizontal slide-in from right. Add a `@keyframes echo-slide-in { from { transform: translateX(100%); } to { transform: none; } }` animation.

17. **Empty detail panel** shows a chevron icon that doesn't clearly communicate "select a transcript." Replace with a more descriptive empty state.

18. **Footer hotkey display** shows `⇧` character which may not render on all systems. Use text "Shift" instead.

---

## GPU / CPU Detection Requirements

The app must auto-detect the best compute path:

1. **Check for NVIDIA GPU** at startup (via `nvidia-smi` or whisper.cpp's own detection)
2. **If GPU found:** Use CUDA-accelerated whisper-cli/whisper-server with `--gpu` flag
3. **If no GPU:** Fall back to CPU-only binary (whisper-cli-cpu.exe is in the models dir)
4. **Display in Settings:** "GPU: NVIDIA RTX 4060 (CUDA)" or "CPU only"
5. **Model recommendations:** With GPU, default to Enhanced (medium). Without GPU, default to Standard (small) for speed.

The CUDA binary and DLLs are at:
```
%APPDATA%/ALAN Echo/models/
├── whisper-cli.exe          # CUDA version (current)
├── whisper-cli-cpu.exe      # CPU fallback
├── whisper-server.exe       # Persistent server mode
├── ggml-medium.bin          # Enhanced model (1.5GB)
├── ggml-large-v3.bin        # Ultra model (3.1GB)
├── ggml-cuda.dll            # CUDA runtime
├── cublas64_12.dll          # cuBLAS
├── cublasLt64_12.dll        # cuBLAS Light
└── cudart64_12.dll          # CUDA runtime
```

---

## Audit Checklist

Run through every item below. For each one: verify it works, fix if broken, verify again.

### Recording Pipeline
- [ ] Ctrl+Shift+Space starts recording (beep plays, status turns red)
- [ ] Audio level is captured (check current_level() returns > 0)
- [ ] Ctrl+Shift+Space stops recording (double beep, status turns yellow)
- [ ] WAV file is created in %APPDATA%/ALAN Echo/ with unique UUID filename
- [ ] WAV is 16kHz mono 16-bit (resampled from device's native rate)
- [ ] Silence detection works (no speech → "No speech detected" error, not a crash)
- [ ] Recording timer counts up correctly in the status panel
- [ ] Recording respects 5-minute max (auto-stops)

### Transcription Pipeline
- [ ] whisper.cpp runs with GPU (check for "CUDA" in output, not "CPU")
- [ ] Transcription completes in < 3 seconds for a 10-second clip on GPU
- [ ] Text cleanup engine runs (filler words removed, capitalization fixed)
- [ ] Result is saved to SQLite with both cleaned and raw text
- [ ] Result appears in the transcript list with flash animation
- [ ] WAV file is cleaned up after transcription
- [ ] Errors during transcription show user-friendly message, not raw error

### UI Components
- [ ] Title bar: wordmark, status pill (green/red/yellow), settings gear, min/max/close
- [ ] Quick stats: shows correct count, words, minutes
- [ ] Status panel: all 3 states render correctly (ready/recording/processing)
- [ ] Transcript cards: relative timestamps, 2-line preview, duration + word count
- [ ] Detail panel: shows full text, copy button works, delete button works
- [ ] Search: filters transcripts in real-time via FTS5
- [ ] Settings panel: opens on gear click, all sections render
- [ ] Mic test in settings: records via cpal, plays back (no browser permission prompt)
- [ ] Model selector: Standard/Enhanced/Ultra saves preference
- [ ] Toggles: auto-paste, sound feedback, text cleanup level all persist
- [ ] Footer: hotkey hints display correctly
- [ ] Toast notifications: appear on copy/delete

### System Integration
- [ ] System tray icon shows ALAN mark (visible on both light and dark taskbars)
- [ ] Tray right-click menu: Show Dashboard, Start Dictation, Quit
- [ ] Tray "Show Dashboard" brings window to front
- [ ] Tray "Start Dictation" triggers recording
- [ ] Tray "Quit" actually exits the app
- [ ] Global hotkey Ctrl+Shift+Space works from any app
- [ ] Global hotkey Ctrl+Shift+H shows/focuses dashboard
- [ ] Window icon shows ALAN mark in taskbar
- [ ] App doesn't crash when window is closed and reopened via tray

### Data & Persistence
- [ ] Transcripts persist across app restarts
- [ ] Settings persist across app restarts
- [ ] Database backup exists in %APPDATA%/ALAN Echo/backups/
- [ ] FTS5 index works (search for a word that exists in a transcript)
- [ ] Delete removes transcript from both main table and FTS index
- [ ] Export produces valid TXT/JSON/CSV/Markdown files

### Error Handling
- [ ] No microphone connected → shows clear error, doesn't crash
- [ ] Whisper model missing → shows warning, doesn't crash
- [ ] Database locked → retries, doesn't crash
- [ ] Very long recording (5 min) → transcribes correctly
- [ ] Very short recording (< 1 sec) → shows "too short" message
- [ ] Rapid toggle (spam Ctrl+Shift+Space) → doesn't crash or double-record

---

## How to Run

### Development mode:
```bash
cd C:\Users\arowm\alan-echo
npx tauri dev
```
This starts Vite on port 1420 and launches the Tauri window.

### Production build:
```bash
cd C:\Users\arowm\alan-echo
npx tauri build
```
This produces an NSIS installer at `src-tauri/target/release/bundle/nsis/`.

### Generate license keys (debug builds only):
```python
# From Python (matches Rust HMAC):
python -c "
import hmac, hashlib, secrets
SECRET = b'ALAN_ECHO_v1_GLOBAL_INTELLIGENCE_2026'
CHARSET = 'ABCDEFGHJKMNPQRSTUVWXYZ23456789'
def compute_check(payload):
    mac = hmac.new(SECRET, payload.encode(), hashlib.sha256).digest()
    return ''.join(CHARSET[b % len(CHARSET)] for b in mac[:5])
segments = [''.join(secrets.choice(CHARSET) for _ in range(5)) for _ in range(3)]
payload = '-'.join(segments)
key = f'ECHO-{payload}-{compute_check(payload)}'
print(key)
"
```

---

## CRITICAL: Transcription Speed Optimization

This is the #1 priority. If transcription is slow, users will abandon the product. Every decision must optimize for minimum latency on ALL hardware.

### The Problem
Whisper.cpp CLI reloads the 1.5GB model from disk on every single transcription. First call: ~14s (5s load + 9s encode on CPU). Even on GPU after model is cached: the CLI still re-initializes every call. This is unacceptable.

### The Solution: whisper-server (Persistent Mode)
`whisper-server.exe` is ALREADY DOWNLOADED at `%APPDATA%/ALAN Echo/models/whisper-server.exe`. It loads the model ONCE and serves HTTP transcription requests. This eliminates model reload entirely.

**Implementation:**
1. On app launch, spawn `whisper-server.exe` as a child process:
   ```
   whisper-server.exe -m ggml-medium.bin --port 8178 --host 127.0.0.1
   ```
2. Replace the CLI call in `whisper.rs` with an HTTP POST to `http://127.0.0.1:8178/inference`
3. Send the WAV file as multipart form data
4. Parse the JSON response for the transcription text
5. On app quit, kill the whisper-server child process
6. If whisper-server crashes, auto-restart it

**Expected latency after this change:**
- GPU (RTX 4060): ~1-2s for 30s of speech (every time, including first)
- CPU (modern 8-core): ~5-8s for 30s of speech
- CPU (4-core laptop): ~10-15s for 30s of speech

### Speed Optimization Ladder (implement ALL of these)

**Tier 1 — Model selection by hardware (auto-detect at startup):**
| Hardware | Model | Expected Speed (30s clip) |
|----------|-------|--------------------------|
| NVIDIA GPU ≥6GB VRAM | large-v3 (Ultra) | ~2-3s |
| NVIDIA GPU ≥4GB VRAM | medium (Enhanced) | ~1-2s |
| NVIDIA GPU <4GB VRAM | small (Standard) | ~0.5-1s |
| CPU only, 8+ cores | medium (Enhanced) | ~5-8s |
| CPU only, 4 cores | small (Standard) | ~3-5s |
| CPU only, 2 cores | base | ~2-3s |

Auto-detect: run `nvidia-smi` at startup. If it returns GPU info, read VRAM. If no GPU, count CPU cores. Select the fastest model that fits.

**Tier 2 — whisper.cpp threading optimization:**
- Pass `--threads N` where N = physical CPU cores (not logical/hyperthreaded)
- On CPU-only: use `--processors 1` (single-pass is faster for short clips)
- On GPU: use `--flash-attn` flag for faster attention computation

**Tier 3 — Audio optimization:**
- Record at 16kHz natively if the mic supports it (skip resampling)
- Use VAD (Voice Activity Detection) to trim silence before/after speech — shorter audio = faster transcription
- whisper-server supports `--vad-threshold` flag

**Tier 4 — Model quantization:**
- Use quantized models (ggml-medium-q5_0.bin is ~60% the size, ~90% the accuracy, 40% faster)
- Download quantized variants from HuggingFace and offer as default
- URL: `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium-q5_0.bin` (~600MB vs 1.5GB)

**Tier 5 — Streaming transcription (future):**
- whisper.cpp supports `--stream` mode where it transcribes in real-time as audio comes in
- This would show partial results while the user is still speaking
- Complex but would make the app feel instant

### Cross-Platform Speed Notes
- **Windows with GPU:** Use CUDA build (already set up)
- **Windows without GPU:** Use CPU build with OpenBLAS (`whisper-blas-bin-x64.zip` — already available in whisper.cpp releases, ~2x faster than vanilla CPU)
- **macOS (Apple Silicon):** Use CoreML/Metal build of whisper.cpp — M1/M2/M3 chips are extremely fast for ML inference, often matching NVIDIA GPUs
- **macOS (Intel):** Use Accelerate framework build — slower but workable
- **Linux:** Use CUDA build if NVIDIA GPU, otherwise OpenBLAS CPU build

### What NOT to do
- Do NOT use the Python `faster-whisper` library (requires PyTorch = 2GB+ overhead)
- Do NOT download models on first launch (bundle them or download during install)
- Do NOT run whisper on the main/UI thread (already fixed — uses spawn_blocking)
- Do NOT reload the model per transcription (use whisper-server)

---

## CRITICAL: Auto-Paste Must Work

Currently transcribed text is copied to the clipboard but NOT pasted into the focused application. The user has to manually Ctrl+V. This defeats the core purpose of the product.

### The Problem
The Dashboard does `navigator.clipboard.writeText(result.text)` which copies to clipboard, but never sends a Ctrl+V keypress to the previously focused window. Also, when the ALAN Echo window is focused (after clicking Stop), the "previously focused app" has lost focus.

### The Solution
Auto-paste must happen at the Rust level, NOT the frontend level:

1. **Before recording starts:** Save the handle of the currently focused window (using `GetForegroundWindow()` Win32 API)
2. **After transcription completes:** 
   a. Copy text to clipboard (`SetClipboardData` or the Tauri clipboard plugin)
   b. Set focus back to the saved window handle (`SetForegroundWindow`)
   c. Simulate Ctrl+V keypress (`SendInput` Win32 API)
   d. After a short delay (200ms), restore the original clipboard contents

**Rust implementation sketch:**
```rust
// In main.rs or a new paste.rs module:
use windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT, INPUT_KEYBOARD, KEYEVENTF_KEYUP, VK_CONTROL, VK_V};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, SetForegroundWindow};

fn save_focused_window() -> HWND {
    unsafe { GetForegroundWindow() }
}

fn paste_to_window(hwnd: HWND, text: &str) {
    // 1. Copy text to clipboard
    // 2. SetForegroundWindow(hwnd)  
    // 3. Sleep 100ms
    // 4. SendInput: Ctrl down, V down, V up, Ctrl up
    // 5. Sleep 200ms
    // 6. Restore original clipboard
}
```

Add the `windows` crate to Cargo.toml:
```toml
[target.'cfg(windows)'.dependencies]
windows = { version = "0.58", features = ["Win32_UI_Input_KeyboardAndMouse", "Win32_UI_WindowsAndMessaging", "Win32_System_DataExchange", "Win32_Foundation"] }
```

### Flow
1. User presses Ctrl+Shift+Space → `save_focused_window()` records the active app
2. User speaks, presses Ctrl+Shift+Space again → recording stops
3. Whisper transcribes → text cleanup runs → saved to DB
4. `paste_to_window(saved_hwnd, cleaned_text)` runs automatically
5. Text appears in the user's original app as if they typed it

This is exactly how the original Python version worked (via pyautogui).

---

## Instructions for the AI

**Your job:** Audit every file, test every feature, fix every bug. Loop until nothing is broken.

**Process:**
1. Read every source file in `src/` and `src-tauri/src/`
2. For each file, check for: correctness bugs, crashes (unwrap/panic), missing error handling, dead code, broken wiring between frontend and backend
3. Fix everything you find
4. Run `cargo check` and `npx vite build` to verify compilation
5. Test the app with `npx tauri dev`
6. Go through the audit checklist above
7. Repeat from step 1 until you find zero issues

**Rules:**
- Never use `window.__TAURI__` — always import from `@tauri-apps/api/core`, `@tauri-apps/api/event`, `@tauri-apps/api/window`
- Never use `unwrap()` in Rust on user-facing data — use `?` or `.unwrap_or_default()`
- Never use regex backreferences (`\1`) in Rust — the `regex` crate doesn't support them
- All Tauri commands that block (whisper, mic test) must be `async` with `spawn_blocking`
- Test mic must use Rust cpal backend, NOT browser navigator.mediaDevices
- The app runs on Windows only for now — use `#[cfg(target_os = "windows")]` for platform-specific code

**Priority:**
1. Make transcription fast (whisper-server mode)
2. Make every button and feature work
3. Make it not crash under any circumstance
4. Make it look and feel premium
