# Terminal 1 Session Log — macOS Cross-Platform Support

**Date:** 2026-06-11 overnight session
**Scope:** Make ALAN Echo codebase ready for macOS builds (Terminal 1 from handoff)

---

## What shipped

### Cross-platform Rust code (all committed + pushed)

| File | Change |
|------|--------|
| `paste.rs` | macOS paste via `osascript` — captures frontmost PID, re-focuses, simulates Cmd+V via System Events. Requires Accessibility permissions. Error message guides user to System Settings if denied. |
| `whisper.rs` | Platform-aware binary name (`whisper-server` vs `.exe`). `detect_hardware()` split into platform-specific `detect_nvidia_gpu()` — returns (None, None) on macOS. `find_server_binary()` uses `SERVER_BINARY_NAME` constant. |
| `packs.rs` | `probe_nvidia()` gated to Windows (no NVIDIA on modern Macs). `probe_display_adapters()` uses `system_profiler SPDisplaysDataType` on macOS. `pack_server_exe()` and pack verification use platform-aware binary name. |
| `updater.rs` | Downloads `.dmg` on macOS (not `.exe`). Opens with `open` command for drag-install flow (doesn't exit app). Sends `?platform=mac` to version endpoint. SHA-256 verification preserved. |
| `main.rs` | macOS own-window check in `deliver_text()` — compares PID instead of HWND. Language setting handler (from prior session) integrated. |
| `tauri.conf.json` | Added `"dmg"` to bundle targets. Added `macOS.minimumSystemVersion: "10.15"`. |
| `Info.plist` | New file — `NSMicrophoneUsageDescription` for macOS microphone permission prompt. |
| `Cargo.toml` | Added `json` feature to ureq (from prior session fix). |

### Previous session work (also committed — was uncommitted)

| Feature | Files |
|---------|-------|
| Updater SHA-256 verification | `updater.rs`, `UpdateBanner.jsx` |
| ureq 2.x API fix | `updater.rs`, `main.rs` |
| Launch at startup (autostart) | `main.rs`, `Cargo.toml`, `capabilities/default.json`, `Dashboard.jsx` |
| Multi-language groundwork | `whisper.rs` (Mutex<String> language), `main.rs` (set_setting handler) |
| Onboarding + Settings UI improvements | `Onboarding.jsx`, `SettingsPanel.jsx` |

### CI/CD

- `.github/workflows/build-macos.yml` — Builds for arm64 (Apple Silicon) and x86_64 (Intel Mac). Produces DMG artifacts. Creates draft GitHub release on tag push.
- `scripts/prepare-resources-macos.sh` — Compiles whisper.cpp from source (CPU-only), downloads ggml-base.en.bin model.

---

## What was deferred

| Item | Reason |
|------|--------|
| **Actual macOS build + .dmg** | Requires macOS machine or CI runner with whisper binary. CI workflow is ready — needs whisper-server binary staged. |
| **Download page OS detection** | Stock-analyzer repo change — Terminal 2's scope. |
| **Version endpoint dual URLs** | Server-side change in stock-analyzer — Terminal 2's scope. Client sends `?platform=mac` and is ready. |
| **Metal/CoreML GPU acceleration** | Decision D15: CPU-only for v1 Mac release. |
| **Apple code signing** | Decision D16: Ship unsigned with right-click bypass instructions. |
| **whisper.cpp universal binary** | Would need both arm64 and x86_64 builds, lipo-joined. CI workflow builds both architectures separately. |

---

## Verification

- `cargo check` — PASS (Windows, no macOS-specific code compiled)
- `cargo test` — 10/10 PASS
- `npm run build` — PASS
- Smoke tests (all endpoints) — PASS

---

## Architecture notes for Mac testers

### Auto-paste flow on macOS
1. Recording starts → `foreground_window()` runs `osascript` to get frontmost app PID
2. Transcription completes → `paste_into(pid)` runs `osascript` to refocus + keystroke Cmd+V
3. Requires Accessibility permissions — first attempt shows clear error with System Settings path
4. `osascript` approach adds ~50ms latency per paste (acceptable for v1; CGEvent optimization possible in v2)

### Updater flow on macOS
1. App checks `/api/echo/version?platform=mac` on launch
2. If update available, banner shows "Download & install"
3. Downloads `.dmg` to `~/Library/Application Support/ALAN Echo/`
4. Runs `open ALAN-Echo-Update.dmg` to mount
5. Shows message: "Drag the new ALAN Echo to Applications to complete the update, then relaunch"
6. App stays running (user relaunches manually after drag-install)

### What still needs macOS testing
- cpal audio recording (should work — cpal uses CoreAudio on macOS)
- Auto-paste with Accessibility permissions
- System tray / menu bar behavior
- Launch at startup via launchd (autostart plugin)
- DMG install + first launch Gatekeeper bypass
