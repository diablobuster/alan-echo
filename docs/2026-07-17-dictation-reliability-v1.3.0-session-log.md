# Session log — 2026-07-17 — Echo v1.3.0: dictation reliability + audit burn-down

## What shipped

v1.3.0, addressing the two field-reported bugs (hotkey unreliability; window-minimize/no-paste at transcription end), plus a two-agent full-app audit burn-down. PR #7 squash-merged to main. Installer published to `diablobuster/alan-echo-releases` tag `v1.3.0`; site env flipped and production redeployed.

### Bug 1 — hotkey doesn't fire / doesn't stop (root cause + fix)
The toggle's critical path was `WM_HOTKEY → Rust emit → WebView2 IPC → JS listener → invoke`. WebView2 suspension (memory pressure/EcoQoS — the anti-throttle flags from v1.2.4 don't cover it) or a wedged JS inflight guard ate presses. v1.2.4/v1.2.5 patched around the chain; this release removes it: the whole dictation state machine now lives in Rust (`dictation.rs`) — phase machine (Idle/Starting/Recording/Stopping), generation-checked 5:00 watchdog, 10s transitional-wedge recovery, Rust-side cpal beeps. The frontend only mirrors `dictation` events.

### Bug 2 — window "minimizes" + paste doesn't land (root cause + fix)
Log-confirmed on this machine (`echo.log` 2026-07-16 19:10:18: "Auto-paste failed: Could not focus the target window"): `SetForegroundWindow` is denied to background processes, and `paste.rs` blind-injected `Shift-up, Alt-up` before Ctrl+V — a stray Alt-up mid-keystroke can push target apps into menu mode; the focus-yank displaced the user's current window. Rewrite: never call SetForegroundWindow; inject Ctrl+V only when the captured target (or same-PID window) is already foreground, after polling `GetAsyncKeyState` until physical Ctrl/Shift/Alt/Win release (2s cap). Otherwise clipboard-only + honest toast. Same no-focus-steal policy on macOS.

## Files touched (intent)

**App (alan-echo):**
- `src-tauri/src/dictation.rs` (new) — Rust dictation state machine; hotkey path webview-free
- `src-tauri/src/paste.rs` — no-focus-steal rewrite; modifier-release wait; same-PID paste; macOS parity
- `src-tauri/src/main.rs` — shared `begin_recording`/`end_recording`/`transcribe_and_deliver`; hotkey/tray wired to `dictation::toggle/cancel`; new commands `toggle_dictation`/`cancel_dictation`/`get_dictation_state`; path containment for `transcribe`/`read_wav_base64`; engine shutdown in `quit_app`
- `src-tauri/src/audio.rs` — cpal beep synth (start/stop chirps) so confirmation sounds don't depend on a throttled webview
- `src-tauri/src/text_cleanup.rs` — word-boundary guard on sentence-start fillers; hallucination stripping whole-utterance-only; ambiguous words removed from force-recase lists (+4 regression tests)
- `src-tauri/src/trial.rs` — 3-day clock-skew grace; far-future ratchet heals instead of bricking (+1 test, 1 updated)
- `src-tauri/src/updater.rs` — download URL pinned to alanglobalintelligence.com; engine shutdown before installer `process::exit`
- `src-tauri/src/packs.rs` — **pinned SHA-256 for both hosted pack zips (armed the fail-open integrity gate — audit CRITICAL)**; engine stop + retry before live-pack-dir replace; unix exec bit on extract; Vulkan pack not offered on macOS; progress-channel wedge fix
- `src-tauri/src/db.rs` — u64 pagination offset (u32 wrap)
- `src-tauri/src/whisper.rs` — model label falls back to "Custom", never a raw ggml filename
- `src/components/Dashboard.jsx` — pure mirror of Rust `dictation` events; JS beeps/inflight-guard/txn-chain removed; mount-time state sync
- `src/components/SettingsPanel.jsx` — brand-name scrub (GPU vendors, CUDA, engine tech → capability language); language commit deferred until model download succeeds; download-kind-aware progress box; `has_multilingual_model` re-query on done; listener-leak fix; optimistic-setting rollback; show-hotkey "Unavailable" state
- `src/components/StatusPanel.jsx` — `Btn` forwards `disabled` (onboarding "Warming up" was clickable)
- `src/components/UpdateBanner.jsx` — error state gets Retry + dismiss (was undismissable dead-end)
- `src/components/DetailPanel.jsx` — in-app two-step delete confirm (`window.confirm` is a no-op on macOS); saveEdit rejection guard
- `src/components/LicenseGate.jsx` — Enter double-submit guard
- `src/components/TitleBar.jsx` — hover uses `currentTarget`
- `Cargo.toml`/`tauri.conf.json`/`package.json` — 1.3.0

**Site (stock-analyzer):** no code changes. Vercel prod env flipped: `ECHO_RELEASE_TAG=v1.3.0`, `ECHO_INSTALLER_SHA256`/`NEXT_PUBLIC_ECHO_INSTALLER_SHA256=1c3e0b3d519e62a00927c99c98a783e711964a4785d3b614ea92ee58e8fdf5a8`, `NEXT_PUBLIC_ECHO_INSTALLER_VERSION=1.3.0`, `NEXT_PUBLIC_ECHO_INSTALLER_MB=129`, `NEXT_PUBLIC_ECHO_RELEASE_DATE="Jul 17, 2026"`; **removed** `ECHO_DOWNLOAD_URL` (pointed at a private-repo v1.2.1 asset that 404s anonymously — the June-outage failure mode; null falls back to a clean redirect). Local `prod.env` snapshot synced. `vercel redeploy` of production issued (NEXT_PUBLIC_ vars are build-time).

## Commits / PRs

- alan-echo PR **#7** (squash-merged → main `6e802df`): four commits — core state machine + paste rewrite; audit burn-down; pack-hash pinning + version bump.
- Release: https://github.com/diablobuster/alan-echo-releases/releases/tag/v1.3.0 (asset `ALAN.Echo_1.3.0_x64-setup.exe`, SHA-256 `1c3e0b3d…a8`, 129 MB).

## Verification

- 47/47 Rust tests pass (7 new/updated covering the corruption and trial fixes); `npm run build` clean; `cargo check` clean.
- Installer rebuilt after hash pinning; confirmed both pinned pack hashes and version 1.3.0 embedded in the shipped exe (byte-scan + VersionInfo).
- gh release asset uploaded; env flip confirmed via `vercel env ls`.
- **Deferred/in-flight:** Vercel production build was still deploying at log time; a live probe (`/api/echo/version` → 1.3.0) is armed. Served-binary hash-vs-advertised check should be done after the flip (the local `echo-release-preflight.ts` can't run without `GITHUB_RELEASES_TOKEN`, which exists only in Vercel).
- **Not GUI-tested:** the resident Echo (v1.2.5) runs in the tray on this machine sharing the data dir — per project rule, no `tauri dev` GUI testing. Real-keyboard hotkey/paste behavior of 1.3.0 needs a manual pass after the user updates.

## Follow-ups / known limitations

1. **Verify served binary hash** once the deploy is live (download via `/api/echo/download-free`, compare to `1c3e0b3d…`).
2. **User should update their installed Echo** (tray app is 1.2.5; in-app updater will offer 1.3.0, or run the new installer).
3. macOS: paste modifier-release wait not implemented (osascript path; noted in `paste.rs`); Mac build still gated on Apple enrollment + hardware.
4. Paste behavior change to document in support copy: if focus moved to a different app during transcription, Echo no longer yanks focus — transcript stays on the clipboard ("Copied to clipboard" toast).
5. Audit LOWs deliberately deferred: whisper-server port TOCTOU, silent row-drop in db decode, `reg add` fire-and-forget race, download size caps, a11y batch (dialog semantics/focus traps/aria-live), Splash raw `model_label` (now "Custom", acceptable).
6. Re-pin `packs.rs` hashes whenever a new pack zip is published (see memory: echo-release-process).
