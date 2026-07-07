# ALAN Echo — Debug Journal

## 2026-06-28 — Hotkey slow/multi-press to activate AND deactivate

### Issue: Ctrl+Shift+Space lagged, needed multiple presses, and a fast double-press cancelled the recording it just started
- **Category**: ARCHITECTURE (latency-critical path routed through the main event loop + a throttled webview)
- **Symptom**: Press to start → ~1s of nothing, or had to press several times; press to stop → delayed or needed several presses. Distinct from the 2026-06-17 fingerprint fix, which only touched the *start* path's license check and never the stop path.
- **Root cause** (verified vs Tauri v2 docs + global-hotkey/plugin source + WebView2/Chromium docs):
  1. `start_recording`/`stop_recording`/`cancel_recording` were *synchronous* `#[tauri::command]`s. Tauri v2 runs non-async commands on the **main thread**, which on Windows pumps `WM_HOTKEY`. `recorder.start()`/`stop()` block it (cpal cold-open; resample+WAV write), freezing both the global-shortcut callback and `emit`-to-webview. A second press queued during the freeze fired *after* the command returned (status already `recording`) → it stopped the just-started recording.
  2. The hotkey did a Rust→webview→Rust round trip; with Echo hidden in the tray, Chromium occlusion + background-timer throttling delayed the JS `handleToggle` and suspended the `AudioContext` beep.
  3. The start beep was gated behind `await invoke('start_recording')`, so confirmation lagged the full cold-open and trained re-presses.
- **Evidence**: Tauri v2 docs — "Commands without the async keyword are executed on the main thread"; global-hotkey `windows/mod.rs` — `WM_HOTKEY` dispatched on the main-thread message pump; `emit_js → eval → EvaluateScript` queued to the (blocked) event loop. Chromium occlusion docs — "rendering stops, and js is throttled" for occluded/minimized windows.
- **Fix**: (a) Make the three recorder commands `async fn` + `tokio::task::spawn_blocking` so the blocking work leaves the event loop; marshal the cancel-hotkey register/unregister back via `app.run_on_main_thread` (the plugin's hotkey window lives on the main thread). (b) Set `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` (env-var path — appends to wry defaults) to disable background-timer/renderer/occlusion throttling; add `backgroundThrottling: "disabled"` for macOS parity. (c) Beep before the await + `AudioContext.resume()` when suspended.
- **Lesson**: Never put a blocking call in a *synchronous* Tauri command — it runs on the main/event-loop thread and starves the global-shortcut pump *and* `emit`. Use `async fn` + `spawn_blocking` (as `transcribe` already did). A global hotkey routed through a hidden webview is also subject to WebView2 occlusion throttling — disable it, or own the action in Rust. Verify config enum literals against the installed crate (`backgroundThrottling` value is `disabled`, not `never`).

## 2026-06-17 — Hotkey-to-beep latency (~15s) / "recording too short"

### Issue: First beep lagged the hotkey by seconds; mashing the key reported "Recording too short"
- **Category**: PERFORMANCE (expensive syscall on the hot path)
- **Symptom**: Press dictation hotkey → up to ~15s before the start beep. Or press repeatedly → "Recording too short — try a full sentence."
- **Root cause**: The frontend gates the start beep behind `await invoke('start_recording')` (Dashboard.jsx). `start_recording`'s first line is `require_license`, which — because `LicenseManager::is_licensed()` is hardcoded `false` (crypto moved to Ed25519 activation) — always falls through to `activation::is_activated()`. With an `activation.jwt` present (every activated/owner machine), `is_activated()` recomputed `machine_fingerprint()` on **every** call. On Windows that spawns three sequential PowerShell `Get-CimInstance` processes — measured **2.077s warm**, far worse cold / under Defender (the reported ~15s). The "too short" cascade follows: presses are dropped while `inflightRef` is true, then the first press after start finally completes immediately stops a <0.5s recording.
- **Evidence**: Timed the three CIM queries directly (2.1s warm). A failing-first unit test (`machine_fingerprint_is_memoized`) measured the 2nd call at 2.077s before the fix.
- **Fix**: Memoize `machine_fingerprint()` with a process-lifetime `OnceLock` (hardware IDs are immutable while running) — fixes every caller at once (start_recording, transcribe, trial, status). Plus a startup warm-up thread in `main()` so the cache is hot before the first hotkey press. Extracted `fingerprint_from()` and pinned the SHA-256(`|`-join) algorithm with a known-vector test so the value can never silently change (which would brick existing activation tokens).
- **Lesson**: Never put a process-spawning syscall on a latency-critical UI path. If a value is derived from immutable hardware, compute it once and memoize; warm it off the hot path at startup.

## 2026-06-12 — Security hardening + 20-pass ultra audit

### Issue: display_accel() showed "Ctrl" on Mac instead of "Cmd"
- **Category**: OVERSIGHT
- **What happened**: `display_accel()` hardcoded "Ctrl" as the replacement for "CmdOrCtrl". Mac users would see "Ctrl + Shift + Space" instead of "Cmd + Shift + Space".
- **Root cause**: Original implementation assumed Windows-only deployment.
- **Fix**: `if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl" }`.
- **Lesson**: Every user-facing string that mentions a keyboard modifier must be platform-conditional.

### Issue: WebView2 error message shown on Mac
- **Category**: OVERSIGHT
- **What happened**: Fatal startup error said "WebView2 is missing or corrupted" regardless of platform. macOS uses WKWebView, not WebView2.
- **Fix**: Platform-conditional hint string.
- **Lesson**: Every user-facing error message referencing system components must be cfg-gated.

### Issue: Activation URL used non-www domain causing 307 redirect failure
- **Category**: LOGIC-ERROR
- **What happened**: `ACTIVATE_URL` was `https://alanglobalintelligence.com/api/echo/activate` (no www). The non-www domain returns a 307 redirect. While `ureq` follows redirects for GET, the POST body can be lost on redirect. Combined with CSRF middleware blocking the request (no Origin/Referer from desktop app), activation failed with "Failed to read JSON."
- **Root cause**: Two bugs compounding: (1) wrong domain, (2) missing CSRF exemption.
- **Fix**: Changed URL to www, added `/api/echo/activate` to CSRF exempt list.
- **Lesson**: Every API URL in a non-browser client must use the canonical domain (www). CSRF exemptions must be added for desktop app endpoints.

### Issue: Windows-specific text in platform-agnostic components
- **Category**: OVERSIGHT
- **What happened**: "AppData" referenced in LicenseGate.jsx, "Windows allows this app" in SettingsPanel.jsx and Onboarding.jsx.
- **Fix**: Replaced with platform-generic language.
- **Lesson**: Search all user-facing strings for "Windows", "AppData", "registry", "tray icon" etc. before Mac launch.

### Issue: License keys emailed in plaintext despite D19 decision
- **Category**: MISREAD (D19 not propagated to email.ts)
- **What happened**: D19 says "Keys NEVER sent via email — display only at /echo/keys after login." But `lib/echo/email.ts` still embeds the key in HTML (line 92) and plaintext (line 189), plus a download URL with the key in a query parameter (line 51).
- **Root cause**: D19 was decided after the email code was written. The email code was not updated.
- **Fix**: Pending user decision — the email should link to /echo/keys instead of showing the key.
- **Lesson**: When a decision changes the behavior of existing code, grep for ALL references and update them.

### Issue: All Rust API URLs used non-www domain
- **Category**: COPY-PASTE
- **What happened**: After fixing activation.rs, discovered packs.rs (GPU download), main.rs (key download), and updater.rs (version check) also used the non-www domain. Each adds an unnecessary 307 redirect hop.
- **Fix**: Changed all 4 remaining URLs to www.
- **Lesson**: When fixing a domain/URL pattern, grep for ALL occurrences across the entire codebase, not just the one that caused the bug.

### Verified this session
- All 17 cargo tests pass after HMAC key change (random bytes, trial state roundtrips correctly)
- Binary size: 11.52 MB (down from ~19MB with LTO+strip)
- `strings` verification: no "arowm", no "ALAN_ECHO_TRIAL", no HMAC key fragments
- Mac CI build succeeds on GitHub Actions (6m20s, 134.78 MB DMG)
- Activation endpoint returns proper JSON errors (not "Forbidden")
- Stock-analyzer builds with all changes (settings overlay, middleware, testimonials)

## 2026-06-10 — Ship-readiness overhaul session

### Issue: Handoff claimed whisper-server.exe was "already downloaded" at models/ root
- **Category**: ASSUMPTION
- **What happened**: The flat models dir only had whisper-cli; the server binaries live in `models/cuda_release/Release/` (CUDA) and `models/Release/` (CPU).
- **Root cause**: Handoff doc described intent, not disk state.
- **Fix**: Engine searches both build dirs, picking CUDA vs CPU by nvidia-smi detection.
- **Lesson**: Verify on-disk artifacts before designing around them; `Get-ChildItem` first.

### Issue: Handoff claimed Rust/Python license checksum algorithms "differ slightly"
- **Category**: STALE-KNOWLEDGE (in the handoff, not the code)
- **What happened**: Pinned Python-generated vectors (`AAAAA-BBBBB-CCCCC → FEMJB`) in Rust unit tests — they match exactly.
- **Root cause**: The original bypass was precautionary, not evidence-based.
- **Fix**: Tests prove parity; gate re-enabled for release builds; machine binding now persisted in settings (`license_binding`) — previously it was never saved, so binding was a no-op across restarts.
- **Lesson**: Reproduce a claimed mismatch with fixed test vectors before "fixing" it.

### Issue: remove_filler_phrases sliced `result` with byte positions from a stale lowercase copy
- **Category**: LOGIC-ERROR (latent crash)
- **What happened**: After the first phrase removal shortened the string, later positions could exceed bounds or split UTF-8 → panic.
- **Fix**: Re-derive the lowercase view per removal, char-boundary + whole-word guards, loop until no match.
- **Lesson**: Never index string A with offsets computed from string B after mutating A.

### Issue: Double beep on every record start/stop
- **Category**: OVERSIGHT
- **What happened**: Backend emitted `play-beep` events AND the frontend played beeps directly in handleToggle — both fired.
- **Fix**: Removed backend emits + listener; frontend owns beeps, gated on `sound_enabled`.

### Issue: Orphaned whisper-server found listening on 8178 during smoke test
- **What happened**: A leftover server from earlier manual testing answered the port instantly, masking real model-load timing.
- **Fix in design**: app picks a verified-free port before spawning, kills child on RunEvent::Exit, generation counter supersedes stale init threads.
- **Lesson**: When a service is "ready" implausibly fast, check `Get-NetTCPConnection` for who actually owns the port.

### Verified empirically this session
- whisper-server (CUDA, RTX 4060): model load ~1.6s, inference 0.45–0.85s for a 7s clip; port binds only AFTER model load (TCP connect = ready is sound for this build).
- Flash attention is default-ON in this whisper.cpp build (`-fa [true]`) — do not pass extra flags.
- `Ctrl+Shift+Escape` can never be a global hotkey on Windows (Task Manager owns it) — cancel is `Ctrl+Shift+X` with `Ctrl+Shift+Backspace` fallback.

### Adversarial review round (26-agent workflow) — 12 distinct issues confirmed & fixed
- whisper-server resurrection: transcribe()'s crash-retry racing app exit could respawn an orphan server → `Status::Stopped` guard in start().
- SetForegroundWindow failure ignored → verified `GetForegroundWindow() == h` (with one retry) before SendInput; never paste into an unchosen window.
- UIPI: SendInput at elevated windows "succeeds" silently → integrity-level check up front, Err → transcript stays in clipboard.
- Clipboard restore at fixed 300ms raced the target's paste → detached thread, 1.5s, restore only if clipboard still holds our text.
- Abandoned mic-test/onboarding recording bricked dictation until restart → backend self-heal in start_recording + unmount cleanup via cancel_recording + hard 310s sample cap in the recorder thread.
- start_recording committed paste_target before recorder.start() could fail → commit only on success.
- Ctrl+Shift+X was swallowed system-wide while idle → cancel hotkey now probed at startup, registered only during a recording.
- Dashboard state machine lacked a synchronous re-entrancy guard (statusRef only updated at render) → inflightRef + applyStatus.
- WAV leaks on too-short/no-speech/transcribe-error paths → discard_recording command + unconditional delete in transcribe + startup sweep (10-min age gate).
- Engine Failed status was sticky → one restart attempt per transcribe when Failed.
- Model switch silently fell back when the file was missing → set_setting validates model_available, SettingsPanel shows the error.
- LicenseGate maxLength=29 truncated pasted keys with leading junk; orphanable poll interval in SettingsPanel; dead toggle-hotkey masked by hardcoded labels across 5 components — all fixed.

### Other fixes this session (watch for regressions)
- WAV files now deleted after: transcription, cancel (`cancel_recording`), mic test (`read_wav_base64`).
- 5-minute recording cap actually enforced (frontend effect) — was display-only.
- Settings: corrupt settings.json no longer loses the save path (Settings::new keeps path).
- Backups pruned to 7. Google Fonts removed (offline/privacy claim). LicenseGate no longer activates on IPC error.
