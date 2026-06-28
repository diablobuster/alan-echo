# Session Log — 2026-06-28 — Dictation hotkey latency: event-loop block + webview throttling

## 1. What shipped

Fixed the reported bug: the global dictation hotkey (Ctrl/Cmd+Shift+Space) was
slow to activate, often needed multiple presses, and deactivation was delayed or
needed multiple presses. Three compounding root causes were verified (Tauri v2
docs + plugin source + WebView2/Chromium docs, via a verification + adversarial-
design workflow) and fixed:

1. **Main/event-loop thread block (HIGH).** `start_recording`, `stop_recording`,
   and `cancel_recording` were *synchronous* `#[tauri::command]`s. Tauri v2 runs
   non-async commands on the main thread, which on Windows is the loop that pumps
   `WM_HOTKEY`. `recorder.start()` blocks until the cpal/WASAPI stream cold-opens
   (hundreds of ms); `recorder.stop()` blocks on resample + WAV write. While the
   main thread was frozen, the global-shortcut callback and `emit`-to-webview
   could not run — and a second press queued during the freeze fired only *after*
   the command returned (status already `recording`), immediately **stopping the
   recording it had just started** ("press twice and it cancels itself").
2. **WebView2 background throttling (HIGH).** The hotkey did a Rust→webview→Rust
   round trip (`on_shortcut` emits `dictate-toggle`; JS `handleToggle` decides and
   invokes). In normal use Echo is hidden in the tray, where Chromium's native
   window-occlusion tracking + background-timer throttling delay JS/event delivery
   and suspend the `AudioContext` — so the toggle and the beep lagged in both
   directions.
3. **Beep-confirmation lag (MED).** The start beep was gated *behind*
   `await invoke('start_recording')`, so the user heard nothing during the full
   cold-open and re-pressed.

The full "move the toggle decision into Rust" rewrite was **deliberately deferred**
(largest regression surface under a no-GUI-test constraint with paying customers);
the adversarial review confirmed fixes #1 + #2 + a cheap slice of #3 resolve both
the activation and deactivation symptoms.

## 2. Files touched and intent

- **src-tauri/src/main.rs**
  - *(top of `main()`)* Set `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` (Windows only)
    to `--disable-background-timer-throttling --disable-renderer-backgrounding
    --disable-backgrounding-occluded-windows --disable-features=CalculateNativeWinOcclusion`.
    Chosen over the `additionalBrowserArgs` config because WebView2 **appends** the
    env var to wry's defaults, whereas the config arg **replaces** them and would
    drop wry's `--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection`. Set
    as the first statement in `main()` — before any thread spawn (set_var isn't
    thread-safe) and before the webview environment is created.
  - Rewrote `start_recording` / `stop_recording` / `cancel_recording` as
    `async fn` that `Arc::clone(state.inner())` then run the blocking recorder work
    inside `tokio::task::spawn_blocking` (the proven `transcribe` pattern) — moving
    the cpal open/close off the main/event-loop thread. JS-visible signatures are
    unchanged (no new params; only the injected `app`/`State` types changed), so
    all call sites (Dashboard, Onboarding, SettingsPanel) keep working untouched.
  - Added `register_cancel_hotkey_on_main()` and made `unregister_cancel_hotkey()`
    marshal the global-shortcut register/unregister back to the main thread via
    `app.run_on_main_thread(...)`. The plugin's hotkey message window is created on
    the main thread in `setup()`; calling register/unregister from a
    `spawn_blocking` worker could bind the cancel hotkey to the wrong thread.
  - Preserved all prior semantics: stale-recording self-heal, `paste_target`
    commit-only-after-successful-start, cancel deletes the WAV and clears
    `paste_target`.

- **src-tauri/tauri.conf.json**
  - Added `"backgroundThrottling": "disabled"` to `app.windows[0]` (macOS 14+
    parity for the dual-platform mandate; no-op on Windows/Linux). Verified
    `backgroundThrottling` is a valid `WindowConfig` field in tauri-utils 2.9.3 and
    that the correct enum value is `disabled` (variants: `disabled`/`suspend`/
    `throttle`) — the design review's guessed value `"never"` would have failed the
    build's schema validation.

- **src/components/Dashboard.jsx**
  - `handleToggle` start branch: moved `playBeep('start')` **before**
    `await invoke('start_recording')` so activation is confirmed on the keypress,
    not after the cold-open. `applyStatus('recording')` stays AFTER the await so a
    failed start never shows "recording"; a genuine mic failure still surfaces the
    error toast.
  - `playBeep`: resume the `AudioContext` if it is `suspended`, so the beep is
    never silently dropped when the (previously throttled) webview suspended audio.

## 3. Commits / PRs

- **None — changes are uncommitted in the working tree.** They sit alongside
  unrelated parallel-session edits (`src-tauri/src/trial.rs`, `package-lock.json`)
  on branch `feat/macos-launch-scaffold`. Recommended next step: create a dedicated
  branch (e.g. `fix/hotkey-eventloop-throttle`) and commit only the four files
  above (main.rs, tauri.conf.json, Dashboard.jsx) so the trial.rs/lockfile work is
  not entangled.

## 4. Verification

Done (no-GUI gates — the resident tray app shares the `com.alan.echo` data dir, so
`tauri dev` / running the built binary is off-limits this session):
- `cargo check` (debug): clean.
- `cargo check --release`: clean (release profile; matches the prior session gate).
  Also validates tauri.conf.json (tauri-build reads it), confirming the
  `backgroundThrottling` key is accepted.
- `cargo test`: **43 passed, 0 failed** — resample/activation/trial/cleanup suites
  all green; the threading refactor changed no logic.
- `npm run build` (vite): success.
- eslint on the changed Dashboard.jsx: **no new errors** (the 16 reported are
  pre-existing repo baseline at lines 254/270/304/425/426/448, none in the edited
  regions).

Deferred (requires a NON-resident machine or quitting the resident tray app, then
rebuild + reinstall):
- End-to-end runtime validation with the window hidden in the tray: press hotkey →
  **immediate** start beep, recording starts on the **first** press, a fast
  double-press no longer cancels, and stop responds within one press while hidden.
- macOS path: `backgroundThrottling` + the async commands are compiled-only here
  (macOS is gated on Apple enrollment + real hardware per the macOS-launch memory).
  The global-shortcut handler thread on macOS (Carbon) was not source-verified, so
  the event-loop benefit is likely but unproven there.

## 4b. Deployment (this session)

Rebuilt and reinstalled on the dev/owner machine after the code landed:
- `npm run tauri build -- --bundles nsis` → `ALAN Echo_1.2.3_x64-setup.exe`
  (129 MB; release build ~4 min, deps cached from the prior `cargo check --release`).
- Closed the resident tray app (user-initiated) and killed orphaned
  `whisper-server` children.
- Silent per-user install (`setup.exe /S`, exit 0). Installed
  `%LOCALAPPDATA%\ALAN Echo\alan-echo.exe` replaced: **06/23 23:36 → 06/28 02:27**.
- Relaunched (PID confirmed). Clean engine startup in echo.log: GPU detected
  (RTX 4060), whisper-server ready on port 8178 in ~5s, no errors. User's data dir
  (`%APPDATA%\ALAN Echo`: license, transcripts, CUDA pack, models) untouched.

The running binary now carries the fix. Remaining validation is real-world hotkey
feel (see §4 deferred): hidden-tray press → immediate beep, first-press start, fast
double-press no longer cancels, stop responds in one press.

## 5. Follow-ups / known limitations

- The fix is now installed and running on the owner machine (see §4b). End users
  on the old build still need the next released installer to benefit.
- **WebView2 browser flags are documented by Microsoft as dev/diagnostic** ("apps
  in production shouldn't use them"). The specific Chromium switches are stable and
  in wide production use (Electron/Tauri), but re-verify after a WebView2 runtime
  upgrade.
- **Deferred (own change + own tests):** move the toggle *decision* into Rust
  (`on_shortcut` starts/stops the recorder directly, emitting UI-only events) — the
  most robust end state (works even if the webview is throttled/crashed/not-loaded,
  de-dupes presses against the recorder's own truth, kills the residual start-beep
  lag). Not required once the above lands.
- **Deferred (distinct defect, do NOT bundle):** the frontend `inflightRef` guard
  is held across the entire stop→transcribe→reload await, so a new dictation can't
  start until transcription finishes. Landmine: `paste_target` is a single shared
  mutex set at start and consumed at the end of `transcribe`; narrowing the guard
  to allow overlap would paste the OLD transcript into the NEW target window. The
  correct fix threads the paste target per-recording first.
- If the beep is still occasionally missed when hidden (AudioContext suspension
  racing `resume()`), the robust fix is a native/Rust-emitted "recording-started"
  signal — folded into the deferred backend-toggle work.
