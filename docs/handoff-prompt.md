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
