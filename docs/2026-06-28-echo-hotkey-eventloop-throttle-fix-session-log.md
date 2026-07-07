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

- **alan-echo PR [#5](https://github.com/diablobuster/alan-echo/pull/5)** —
  `fix/hotkey-eventloop-throttle`, branched off `origin/main` in an isolated
  worktree (NOT the shared `feat/macos-launch-scaffold` tree, to avoid entangling
  the parallel session's uncommitted `trial.rs`/lockfile WIP). 7 files: main.rs,
  Dashboard.jsx, tauri.conf.json, Cargo.toml, Cargo.lock, package.json (1.2.3 →
  1.2.4), + this session log. **Squash-merged to `main` (`3d6bd03`)** (the
  pre-existing backend CI failure — missing `resources/models` — bypassed with
  `--admin`, confirmed infra not a regression).
- The owner-machine reinstall (§4b) used the working-tree build before the PR;
  the PR carries the identical fix off a clean `main` base.

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

## 4c. v1.2.4 public release — GitHub + website (Windows-only)

Shipped the fix to all users as **v1.2.4**, **Windows-only** (owner decision after
discovering the macOS native code lives only on the unmerged `feat/macos-launch-
scaffold` branch — a Mac 1.2.4 would have meant publishing that whole untested
scaffold; deferred to the proper Mac launch). The release pipeline spans three repos
+ Vercel prod env (all env-driven; no website code change):

1. **Build** — v1.2.4 Windows installer built off `main`+fix in an isolated worktree
   (shared Rust target for a fast incremental build). `ALAN Echo_1.2.4_x64-setup.exe`,
   129.1 MB, **sha256 `8fbff6b26a82f734eae7f39f208a47b3f97bf0b0194a0861cc7cceae62c3b6e4`**.
2. **Release** — `gh release create v1.2.4` in **`diablobuster/alan-echo-releases`**
   (private; the site mints signed asset URLs via `GITHUB_RELEASES_TOKEN`). Assets:
   `ALAN.Echo_1.2.4_x64-setup.exe`, `ALAN.Echo_1.2.4_universal.dmg` (a **re-host of
   the existing 1.2.3 .dmg**, sha `8f82477c…`, since `ECHO_RELEASE_TAG` is shared
   across platforms — the Mac resolver needs a .dmg on the v1.2.4 tag), and
   `SHA256SUMS-124.txt`.
3. **Preflight gate** — `scripts/echo-release-preflight.ts` (downloads the *served*
   binary via the same resolver the app uses, checks SHA) **PASSED for Windows**
   (served .exe == `8fbff6b2…`) and **Mac SHA matched** (`8f82477c…`; the Mac
   version-label "fail" is the intentional 1.2.3 pin under the shared tag).
4. **Prod env** (Vercel project `alan_intelligence`, Production): `ECHO_RELEASE_TAG`
   → `v1.2.4`; `ECHO_INSTALLER_SHA256` + `NEXT_PUBLIC_ECHO_INSTALLER_SHA256` → the new
   exe hash; `NEXT_PUBLIC_ECHO_INSTALLER_VERSION` → `1.2.4`; size/date refreshed.
   **Mac left untouched** — `ECHO_MAC_INSTALLER_VERSION` stays `1.2.3` (overrides the
   tag → no spurious Mac update), `ECHO_MAC_INSTALLER_SHA256` unchanged.
5. **Deploy** — `vercel redeploy` of the current prod deployment OOM'd twice
   (`npm run build` SIGKILL — the known intermittent runner-OOM, NOT this change).
   Resolved by `vercel --prod` from a clean worktree at `origin/main` — which the
   timestamps prove **equals the current prod source** (`64ea3ccc`, deployed 06-26
   01:24, one min after the commit; no drift), so it shipped **no new code**, only
   the env. Built with cache → succeeded.
6. **BOM fix** — the first env set (PowerShell pipe) prepended a UTF-8 BOM to every
   value; trimmed fields (version/sha/tag) were fine but `sizeMb`/`date` showed
   `ï»¿`. Re-set cleanly via `Start-Process -RedirectStandardInput` from a no-BOM
   file (PowerShell's pipe-to-native and `cmd /c` were both unusable here), verified
   byte-clean, redeployed.

**Live verification (prod API):** Windows → `version=1.2.4`, `sha256=8fbff6b2…`,
`129 MB`, `Jun 28, 2026` (all byte-clean); Mac → `version=1.2.3`, `8f82477c…`,
`141 MB` (unchanged). Both `download-free?platform=…` endpoints return **302 →
signed asset**. Existing Windows 1.2.3 users will be offered the in-app update
(`version_gt(1.2.4,1.2.3)`); the download verifies against the advertised SHA.

## 4d. Version-label bug — hardcoded `v1.2.1` in settings/license gate

User reported the settings menu showed `v1.2.1` while the main window showed `1.2.3`.
Root cause: two **hardcoded** version strings frozen at 1.2.1, while the rest of the
UI reads the dynamic `pkg.version` (from package.json):
- `src/components/SettingsPanel.jsx:475` — "Part of ALAN Global Intelligence · v1.2.1"
- `src/components/LicenseGate.jsx:236` — "ALAN Global Intelligence · Echo v1.2.1"
(A repo-wide sweep confirmed these were the only two hardcoded version labels.)

Fix: both now interpolate `v{pkg.version}` (SettingsPanel already imported `pkg`;
added the import to LicenseGate). Also bumped the local working tree to **1.2.4**
(package.json / Cargo.toml / tauri.conf.json) so the owner's app matches the website
AND the in-app updater won't offer the released 1.2.4 (which still carries this
display bug) as a "newer" version. Rebuilt + reinstalled on the owner machine
(v1.2.3 → v1.2.4, relaunched); all version labels now agree.

**Open follow-up:** this display fix is only in the owner's local build, NOT in the
public release — the website's v1.2.4 download still shows `v1.2.1` in settings/
license gate. To correct it for everyone, fold this fix into a clean **v1.2.5**
release (PR to main → build → alan-echo-releases → prod env bump → deploy), the same
pipeline as §4c. Deferred pending owner go-ahead.

## 4e. Residual hotkey fix + v1.2.5 release

After v1.2.4 the owner still reported intermittent delay and "sometimes press
twice." Runtime evidence ruled OUT throttling (the WebView2 anti-throttle flags
were confirmed present on the live `msedgewebview2.exe` browser process) and
confirmed the app was on v1.2.4. A 3-agent workflow (investigate → design both
options → adversarial review) traced the dominant residual cause and recommended
the **narrow Option A** (ship now, escalate to the backend-toggle only if delay
survives):

- **Root cause #1:** the frontend `inflightRef` guard was held across the ENTIRE
  stop→transcribe→reload window (seconds), so a press to start the next dictation
  right after finishing was dropped *before its beep*.
- **Fix:** `stop_recording` now returns the captured paste target; `transcribe`
  takes it as a per-recording param (defusing the shared-`paste_target` landmine).
  `handleToggle` releases the guard the instant `stop_recording` returns and runs
  Whisper in a **serialized background chain** — the next press starts immediately.
  Status split into recorder-truth (`status`) + a derived display badge
  (`panelStatus`). Also fixed a pre-existing out-of-scope `catch` ref in the stop
  path; 1s timer; kept-warm AudioContext.
- Files: `src-tauri/src/main.rs` (stop_recording, transcribe, dropped the now-unused
  `RecordingResult` import), `src/components/Dashboard.jsx`. Plus the §4d version-
  label fix folded in.
- Verification: `cargo check` + `cargo test` (43) green; vite build clean; eslint on
  Dashboard.jsx **14** (down from 16 — caught + fixed a TDZ crash my first draft
  introduced: `transcribeInBackground` referenced `fireToast` before its declaration).
  **Live-tested on the owner machine (v1.2.4 → v1.2.5): owner confirmed "it's better"
  — the press-twice is gone.**

**v1.2.5 public release (Windows-only, same pipeline as §4c):**
- alan-echo PR [#6](https://github.com/diablobuster/alan-echo/pull/6) (`fix/hotkey-residual-v125`,
  off origin/main; SettingsPanel edited in-place to avoid the feat-branch macOS-scaffold
  lines that a file-copy would have dragged in) → **squash-merged to main (`355a56e`)**.
- alan-echo-releases **v1.2.5**: `ALAN.Echo_1.2.5_x64-setup.exe`
  (sha `c748fca7147b7965070afe581f4ad2be1150fc6888069384fdd3b31f9c8cc196`) +
  re-hosted `ALAN.Echo_1.2.5_universal.dmg` (sha `8f82477c…`, Mac unchanged) +
  `SHA256SUMS-125.txt`. **Preflight PASS** (Windows served-SHA verified).
- Prod env: `ECHO_RELEASE_TAG=v1.2.5`, Windows SHA (×2) + version updated via the
  **Start-Process no-BOM method** (verified byte-clean — fixes the v1.2.4 BOM class).
  Mac vars untouched.
- Deployed `origin/main` (== current prod source `64ea3ccc`, drift-free) via
  `vercel --prod`. **Live verified:** Windows `1.2.5` / `c748fca7…` / `129 MB`;
  Mac `1.2.3` / `8f82477c…`. Temp worktrees cleaned up; sources intact.

DEFERRED (only if residual delay is still reported): the "Option B" backend-owned
toggle (move the start/stop DECISION into Rust so the hotkey drives the recorder
directly — fixes the webview round-trip latency and pre-Dashboard-mount drops). And
the cpal cold-open clipping the leading word (needs a pre-warmed mic stream). Both
are larger, dual-OS-test changes; not warranted unless the narrow fix proves
insufficient.

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
