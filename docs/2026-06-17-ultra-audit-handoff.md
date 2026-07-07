# ALAN Echo — Ultra-Audit Handoff

**Date:** 2026-06-17
**Purpose:** Drive the next session(s) to (a) optimize ALAN Echo's performance and (b) add high-value, differentiating features — grounded in a real 8-dimension code audit + deep competitor research already performed.
**Scope:** The desktop app only (`C:\Users\arowm\alan-echo`). NOT the marketing website (separate repo).

This handoff is **pre-loaded with verified findings** so the next session can start executing immediately. Every file:line below was checked against the live tree. Full raw findings are in `docs/_audit-raw/` (`audit-findings.json`, `audit-summary.md`, competitive research in the workflow output). When in doubt, re-verify against code — some line numbers drift as the tree changes.

---

## 0. How this handoff was produced

Two parallel multi-agent workflows ran on 2026-06-17:
1. **8-dimension code audit** (architecture, performance, backend correctness, frontend, build/release, parity, features, testing) — each dimension's findings were independently **pressure-tested by a skeptic agent** (confirmed / adjusted / rejected). ~59 findings survived.
2. **Competitive research** (47 agents) — 9 competitor clusters, a feature matrix, 4 diverse ideation lenses, and **adversarial scoring** of 32 candidate features for value × differentiation × feasibility-in-code.

A latency fix already shipped this session (see §2) — use it as the **proven pattern** for the performance work.

---

## 1. Mission & scope

Make ALAN Echo **faster** and **stand out**. The wedge (validated by the research) is:

> **The dictation app you *own* instead of *rent*: 100% local speech-to-text that never leaves your machine, GPU-fast on both Windows (CUDA) and Mac (Metal), for one honest ~$89 — not a $144/yr Wispr subscription or a $96/yr Aqua rental.**

Cloud leaders (Wispr Flow, Aqua Voice, Otter, Fireflies) require internet for every word, fail offline, and upload audio to third parties (Wispr was caught uploading active-window screenshots; Otter faces the *Brewer v. Otter.ai* class action over training on conversations). Echo's architecture (whisper.cpp on `127.0.0.1`, audio deleted on every code path, no telemetry) is the structural answer to the category's loudest complaint. **Every feature must preserve that local-first, one-time-price, dual-platform posture.**

**In scope:** performance, features, correctness, packaging, parity, tests.
**Out of scope / dismissed:** cloud-LLM post-processing (BYOK), meeting-bot/diarization, enterprise SaaS — all reintroduce the recurring-cost/cloud round-trip users are fleeing.

---

## 2. Current state — what just shipped

**Hotkey latency fix (`fix/hotkey-fingerprint-latency`, pushed, reinstalled).** `machine_fingerprint()` spawned 3 PowerShell/WMI processes on *every* `require_license()` call (~2s warm, ~15s cold), and `start_recording()` runs `require_license()` first while the frontend gates the start beep behind the awaited invoke → the hotkey took up to 15s to beep. Fixed by **memoizing the fingerprint (`OnceLock`)** + **warming it at startup** off the hotkey path. Tests: 2.077s → <50ms; 19/19 pass; release built; installed.

> **This is the template for the performance work below: find process spawns / blocking syscalls / redundant recomputation on a latency-critical path, then memoize or move them off it.**

---

## 3. How ALAN Echo works (codebase + runtime map)

**Stack:** Tauri 2; Rust backend (`src-tauri/src/`, ~4,650 LOC), React 19 + Vite frontend (`src/`, ~3,000 LOC). Local STT via a bundled **whisper.cpp server** (`whisper-server.exe`, CUDA or CPU on Windows; CPU-only on Mac today — see §7).

**Dictation pipeline:**
1. Global hotkey `Ctrl/Cmd+Shift+Space` registered in `main.rs` `setup()` (`register_emit_hotkey`, ~`main.rs:1043`) → backend emits `dictate-toggle`.
2. Frontend `Dashboard.jsx` `listen('dictate-toggle')` → `handleToggle()` (state machine: `ready`/`recording`/`processing`, serialized by `inflightRef`).
3. `invoke('start_recording')` (`main.rs:267`) → `require_license()` → `paste::foreground_window()` (captures paste target) → `recorder.start()` (cpal on a dedicated thread, `audio.rs:130`).
4. Stop → `invoke('stop_recording')` → resample (`audio.rs:354`) → write WAV.
5. `invoke('transcribe')` (`main.rs:409`) → `whisper.transcribe()` HTTP POST to the server (`whisper.rs:~515`) → `text_cleanup.clean()` (`text_cleanup.rs:70`) → save to SQLite (`db.rs`) → `deliver_text()` (`main.rs:448`) clipboard-writes + auto-pastes into the previously-focused app (`paste::paste_into`).

**Threading:** recorder thread (cpal, channel-driven), whisper-server child process (port-picked, generation-superseded, killed on exit), plus spawned threads for downloads/clipboard-restore. **Engine lifecycle:** `whisper.rs` picks a free port (`free_port:552`), spawns the server, polls TCP for readiness, restarts on failure (no cap/backoff today — see §6).

**Licensing:** `license.rs` is format-only (`is_licensed()` is hardcoded `false` by design); real gating is Ed25519 activation (`activation.rs`), with `machine_fingerprint()` binding the token. Trial mode in `trial.rs`.

---

## 4. Performance backlog

The fingerprint fix is shipped. Remaining items, by priority. **Apply the systematic-debugging skill (root cause first) and TDD (failing test first) to each.**

### P0 — highest impact, low risk
- **`clean()` recompiles ~30–70 regexes on every transcription.** `text_cleanup.rs:259-299` (`fix_acronyms`, `apply_informal_corrections`) compile their regex sets per call; `clean()` runs on every dictation. **Fix:** precompile once via `once_cell::Lazy` (the file already does this for `RE_HALLUCINATIONS` — match the idiom). *Effort: small. Dual-platform: none (pure Rust).*
- **macOS: `osascript` spawned synchronously on the hotkey→record→beep path** — the exact sibling of the fingerprint bug. `paste.rs:142-157` (`mac::foreground_window`) shells out to `osascript`, called from `start_recording` (`main.rs:272`) before the beep. **Fix:** replace the macOS capture with a non-subprocess API — `NSWorkspace.shared.frontmostApplication.processIdentifier` via `objc2`/`cocoa` (synchronous, no spawn). Keep osascript only for the actual paste keystroke. *Effort: medium. Dual-platform: macOS-only change; Windows already uses fast `GetForegroundWindow`.*

### P1
- **Audio callback computes RMS every frame for a `get_audio_level` that's barely polled.** `audio.rs:193-195/220-222/247`. Gate the RMS work or downsample it. *Effort: small.*
- **O(n) linear resampler runs synchronously in the Stop handler** (`audio.rs:354-365`) and **panics on empty input** (`samples.len()-1` underflow). Add the empty guard (correctness) and consider moving off the IPC worker. *Effort: small.*
- **Post-dictation does two sequential IPC round-trips** (`Dashboard.jsx:216-217`, `loadTranscripts` then `loadStats`). Combine or parallelize. *Effort: small.*
- **Transcript list renders every row without virtualization** (`Dashboard.jsx:374-376`); "Load more" grows it unbounded. Add windowing for large histories. *Effort: medium.*

### P2 (low)
- WAV read twice per transcription (`whisper.rs:321`). `get_page` issues a `COUNT(*)` on every page incl. load-more (`db.rs:107-125`). Recorder `level`/`is_recording` use sync channel round-trips under a Mutex (`audio.rs:92-127`). Auto-paste fixed 120ms sleeps + 1500ms restore (`paste.rs:104,108`). Engine readiness discovered by 3 uncoordinated frontend polling loops (`main.jsx:66-87`) — replace with an event. `is_activated()` re-reads token + re-verifies Ed25519 on every check (cache it). `Sync start/stop/cancel_recording` block a Tauri worker for the mpsc round-trip (`main.rs:267-322`).

---

## 5. Standout feature roadmap

Full competitive matrix and 32 scored features in `docs/_audit-raw/`. Highlights:

### Build-now (P0) — high value × differentiation, dual-platform-safe
1. **Re-paste last transcript hotkey** *(the user's seed idea — ship first).* A dedicated global hotkey (`CmdOrCtrl+Shift+V`, probed for collisions like the cancel accel) re-inserts the most recent transcript into the now-focused app, no re-recording. **Every dependency exists.** Add `#[tauri::command] fn paste_last(app, state)` near `deliver_text` (`main.rs:448`): capture the live target via `paste::foreground_window()`, set `*state.paste_target.lock() = Some(target)`, read newest row via `state.db.lock().get_page(0,1)` (`db.rs:107`), then call `deliver_text()` (which already does clipboard-write + UIPI-safe paste + restore + `auto_paste` respect). **Do the capture in the Rust command**, not the JS event round-trip, so you don't capture Echo's own window. Register a 4th hotkey in `setup()` (`main.rs:~1053`), add to `invoke_handler!`, add `listen('paste-last', …)` in `Dashboard.jsx:125`. *Effort: small. Dual-platform: low (reuses shipping infra).*
2. **Custom vocabulary / dictionary** — the #1 accuracy complaint across the category; Echo has none. Add a `custom_vocabulary` setting; **primary (certain) path:** pass whisper-server the `--prompt`/`-p` CLI flag in `spawn_server` (`whisper.rs:~497`), taking effect via the existing `reload()` machinery (`main.rs:116-123`). **Secondary (validate against the bundled server first):** add a `prompt` multipart field to `post_inference` for restart-free per-request injection — do NOT assume the bundled server accepts it. Plus a deterministic casing post-pass in `text_cleanup.rs`. Cap ~200 tokens. *Effort: small–medium. Dual-platform: none.*
3. **Deterministic find-and-replace rules** — user `from→to` table applied before paste (`'github'→'GitHub'`, canned expansions). Add a precompiled `rules: Vec<(Regex,String)>` to `TextCleanupEngine`, invoked in `clean()` after `fix_capitalization`. `clean()` is the single chokepoint for both live dictation and the `clean_text` command, so it covers auto-paste *and* re-paste. *Effort: small. Dual-platform: none.*
4. **Verbatim / raw mode** — a real `"verbatim"` early-return at the top of `clean()` (`text_cleanup.rs:70`) that bypasses the **baseline** transforms (`remove_hallucinations` strips `[..]`/`(..)` — kills `arr[i]`; `fix_punctuation` force-appends `.`; `fix_capitalization`). NB: "just use `raw_text`" is NOT verbatim — the harmful transforms live in the baseline. Also fix the dead `'light'` option that silently falls through. The trust angle for lawyers/devs/quoters that cloud rewriters can't match. *Effort: small.*
5. **Rebindable hotkeys** — accelerators are hardcoded literals (`main.rs:~1053`) with no `set_hotkey` command; the Settings "Hotkeys" section is read-only. This is the direct fix for the "hotkey unavailable — use the tray menu" dead-end shown in 3 places. Add `set_hotkey(action, accel)` (unregister old → register via `register_emit_hotkey` → persist → update `state.hotkeys`); key-capture UI in `SettingsPanel.jsx`. Persist the raw `CmdOrCtrl+…` form so `display_accel` per-OS still works. *Effort: small. Dual-platform: yes (plugin is cross-platform).*

### P0 parity (also a perf/positioning win)
- **macOS Metal GPU acceleration.** Echo's headline is "GPU-fast, cross-platform," yet GPU only exists on Windows — a Mac buyer pays ~$89 for a CPU experience next to Metal-fast MacWhisper/superwhisper. **The fix is a build-flag change, not a download pack:** `scripts/prepare-resources-macos.sh:31-37` compiles the server with `-DWHISPER_NO_METAL=1` and even comments *"Remove this flag in a future release to enable Metal."* Metal links a framework present on every Mac since 2014. Then add a `'metal'` arm to `binary_kind()`/`engine_kind` + an Apple-Silicon probe (`detect_nvidia_gpu` returns `None` on mac today, `whisper.rs:600-605`), and fix `gpuVerdictText` ("No dedicated GPU found" is wrong on Apple Silicon, `SettingsPanel.jsx:553`). **Must validate on real Apple Silicon.** *Effort: medium.*

### P1 backlog
- **Push-to-talk (hold) vs toggle** — `register_emit_hotkey` drops `ShortcutState::Released` (`main.rs:720`); emit `dictate-start`/`dictate-stop` on Pressed/Released. Needs a PTT-specific safety timeout for missed key-ups.
- **Spoken formatting commands** ("new line", "comma", "bullet") — a pass at the *end* of `clean()`; **critical:** `normalize_whitespace` (`text_cleanup.rs:103`) strips `\n`, so the command pass must run last or it's a no-op. Verify newlines survive both paste backends.
- **On-device translate-to-English** — `translate` per-request form field (not a reload-forcing flag); cloud peers translate by *uploading* — strong differentiation. Quality wants medium/large model.
- **Transcript pins/tags** — start with a one-column `pinned` boolean (additive ALTER, template at `db.rs:84-87`) + a star toggle; tags/folders later.
- **Honest progress bar** (cheap slice of "streaming") — replace the fixed-30% indeterminate bar (`StatusPanel.jsx:61-64`) with a duration-based estimate using `recording.duration_seconds` (already returned) × a realtime factor keyed off `engine_kind`/`cpu_cores`. *Small, high-value — the most-watched moment.*
- **Recent-transcripts quick-pick palette / tray submenu** — the multi-item sibling of P0-1; ship after re-paste-last proves the deliver-by-id command. Cost is the focus dance + a new capability file.

### Quick wins to ship first
Re-paste-last (#1) → Verbatim + fix dead `'light'` (#4) → find/replace (#3) → "0 audio at rest" privacy counter (already true: WAV deleted on transcribe/cancel/discard/startup-sweep) → broaden export (TXT → +SRT/JSON/MD).

### Explicitly dismissed (don't build)
BYOK cloud-LLM post-processing (breaks local-first), undo-paste via backspace injection (false premise — Echo pastes via clipboard+Ctrl/Cmd+V, not keystrokes; over/under-deletes the user's real doc), SQLCipher encrypted vault (large, partial guarantee — raw WAVs + clipboard still leak), meeting capture/diarization (the Otter/Fireflies lawsuit territory Echo contrasts *against*), macOS Metal *download pack* (use the build flag).

---

## 6. Correctness / robustness / security register

**High:**
- **Fingerprint collapses to a shared constant when all hardware probes fail** (`activation.rs:132-153`): every "all-UNKNOWN" machine (locked-down corp boxes, VMs) gets `SHA256("UNKNOWN|UNKNOWN|UNKNOWN")` — token binding defeated for that class + per-machine accounting collides. **Fix additively** (only change the mostly-UNKNOWN case — re-deriving existing fingerprints would brick live tokens): fall back to a persisted UUID v4 (already a dep) when <2 of 3 components resolve.
- **GPU pack + HuggingFace model downloads have no integrity check** (`packs.rs:489-547`, `main.rs:596-655`) — only a size floor, yet the downloaded packs are native executables launched as the engine (RCE vector). The installer path already SHA-verifies (`updater.rs:104-123`) — reuse that streaming-hash block before extract/rename. `ed25519-dalek`+`sha2` already deps.
- **Transcribe ("processing") phase is un-cancelable with no progress** (`Dashboard.jsx:190-234`) — pair with the honest-progress-bar feature.

**Medium/low:** engine restart-on-failure has no cap/backoff/breaker (`whisper.rs:69-77`); updater download has no per-read timeout (`updater.rs:72-99`) — mirror `packs.rs`'s `AgentBuilder` timeouts; `version_gt` silently drops non-numeric segments (`updater.rs:162`) so `1.2.3-beta`→`[1,2]` (a server "-beta" tag would suppress a real update); license key embedded **unencoded** in the download URL query string (`main.rs:672`) — percent-encode + validate; `free_port()` TOCTOU (`whisper.rs:552`); CSV **formula-injection** on the text field (`db.rs:227-238`) — prefix cells starting with `=+-@`; backend delete failure still removes the row in UI (desync, `Dashboard.jsx:289`); status/toasts/errors not announced to assistive tech (no `aria-live`, `Dashboard.jsx:359-368`); beep oscillator nodes never disconnected + `AudioContext` never resumed (`Dashboard.jsx:96-120`).

---

## 7. Build / release / packaging

- **Installer-hash CRITICAL (tracked):** the updater verifies the download against a **server-supplied** SHA with **no app-pinned signing key** (`updater.rs:104-123`) — catches transport corruption only, not a compromised origin. The hash is hand-synced across 2 site env vars + `SHA256SUMS.txt` + release notes; drift hard-fails the update and deletes the installer. The specific *stale-hash instance* was already resolved (per `docs/2026-06-13-legal-audit-handoff.md`), but the **structural fragility + no pinned key remain.** **Fix:** single-source the hash (site API reads the uploaded `SHA256SUMS.txt`), and move to real artifact signing — `tauri-plugin-updater` or a detached ed25519 signature against a const pubkey compiled into `updater.rs` (app already depends on ed25519-dalek). Note the mac path is a custom DMG drag-install, so `tauri-plugin-updater` is not a drop-in.
- **Binaries are unsigned/unnotarized on both platforms.** Windows is a deliberate, checksum-mitigated decision; **macOS is silently absent** — an unsigned, un-notarized, internet-downloaded `.app` is likely Gatekeeper-*blocked*, not just warned. Decide the mac posture explicitly: Developer ID codesign + `notarytool` + `stapler` in CI, or ship explicit `xattr -dr com.apple.quarantine` instructions. `TAURI_SIGNING_PRIVATE_KEY:""` in CI is the *updater* key, not an Apple identity.
- **CI gap:** `.github/workflows/build-macos.yml` is the **only** workflow (macOS arm64 only, tag-triggered) and never runs `cargo test`/`clippy`/`fmt`/`lint`. **There is no Windows CI at all** despite Windows being primary — the shipped Windows payload is hand-copied from a dev machine (`prepare-resources.ps1`). This is the single highest-leverage fix (see §9).

---

## 8. Cross-platform parity gaps (Windows + macOS)

The **dual-platform mandate is non-negotiable** (memory). Confirmed gaps where macOS is weaker:
- **No GPU on Mac** (Metal — §5 parity item) — the biggest gap.
- **`osascript` on the hotkey hot path** (§4 P0) — the macOS latency sibling of the fingerprint bug.
- **macOS unsigned/un-notarized** (§7) — likely a hard launch failure.
- **Intel Macs get no build** — CI targets `aarch64` only, despite `minimumSystemVersion 10.15` implying Intel support (and a stale session log *falsely* claims it builds both). Decide: Apple-Silicon-only (and say so) or add the x86_64 leg.
- Per-app identity: Windows persists only the HWND (not PID); macOS persists the PID — relevant to per-app paste profiles.

---

## 9. Testing strategy

Current coverage: **19 Rust unit tests across 4 of 14 backend modules** (text_cleanup, trial, license, activation). **No frontend test runner. No CI gate.** Priorities:
1. **Add CI** (`ci.yml` on push/PR): matrix `cargo test` + `clippy -D warnings` + `fmt --check` on windows-latest + macos-14, and `npm ci && lint && build`. **This is the only automated way a `cfg(target_os)` Windows compile break is caught at PR time** instead of at tag-build. Highest-leverage single change.
2. **Test `verify_token`** (`activation.rs:33`) — the revenue gate, currently zero tests: malformed/short-sig/tampered/mfp-mismatch/exp-boundary/legacy-no-exp. All testable offline with the embedded pubkey.
3. **Add a fingerprint-quality guard + test** (§6) and a `resample` empty-input test (currently panics).
4. **Add vitest + @testing-library/react + jsdom**; mock `@tauri-apps/api`; cover the Dashboard state machine first (rapid-toggle re-entrancy). **This lets you test the most bug-prone code WITHOUT a GUI**, honoring the resident-app constraint.
5. **DB tests** must use a temp/in-memory DB — **never** the real data dir (the resident app holds `transcripts.db` open; a test could corrupt it).

---

## 10. Execution plan for the ultra audit

Run as a sequence of Workflow phases, staying in the loop between them. Use **TDD** (failing test first) and **systematic-debugging** (root cause before fix) on every change.

1. **Stabilize first (P0 perf + correctness):** regex precompile, macOS osascript replacement, fingerprint-quality guard, download integrity, resample empty guard. Each: failing test → fix → verify.
2. **Ship the quick-win features:** re-paste-last → verbatim+`light` fix → find/replace → privacy counter → export formats. Pipeline them (independent).
3. **Then the bigger bets:** custom vocabulary, rebindable hotkeys, push-to-talk, Metal parity, honest progress bar.
4. **Harden release:** CI gate (windows+mac), signing decision, installer-hash single-sourcing.
5. **Deepen research where a feature needs it** (e.g., live-validate the whisper-server `prompt` form field; survey Apple-Silicon Metal build specifics) with a targeted research workflow.

**Acceptance per change:** failing test written and watched fail; minimal fix; `cargo test` (and frontend vitest once it exists) green; compiles on **both** targets; dual-platform note in the commit; no regression to the resident app's data.

**Suggested workflow patterns:** multi-modal sweep for discovery, adversarial-verify for each correctness fix, pipeline for the independent feature backlog, loop-until-dry for "what did we miss."

---

## 11. Guardrails & non-negotiables

- **Dual-platform always.** Every change must work on Windows AND macOS. Flag every `cfg(target_os)`, PowerShell/WMI, ioreg/diskutil, SendInput, osascript, WASAPI path.
- **Never GUI-test via `tauri dev` while the resident tray app runs** — it shares the `com.alan.echo` data dir. Match processes by **path**, not name. To deploy a fix: build → quit the running app + whisper child → silent reinstall (`*-setup.exe /S`, currentUser = no UAC) → relaunch.
- **A parallel Claude session edits/commits these repos concurrently.** Verify git state before editing/committing; isolate your work on a dedicated branch; never sweep unrelated working-tree changes (e.g. lock files, legal docs) into a commit.
- **Open CRITICALs:** (1) installer-hash signing weakness (§7); (2) copyright registration filing due **2026-09-10**.
- **Preserve the wedge:** no cloud round-trips, no subscriptions, no telemetry. Local-first or it doesn't ship.
- **Verify before claiming done.** If a fix needs a rebuild+reinstall to take effect for the user, say so.

---

## 12. Appendix — raw materials

- `docs/_audit-raw/audit-findings.json` — all 7 pressure-tested dimensions, structured.
- `docs/_audit-raw/audit-summary.md` — readable digest with detail/fix/impact/skeptic notes per finding.
- `docs/_audit-raw/competitive-roadmap.md` — full positioning thesis, competitive matrix, scored roadmap (note: contains some mojibake from a console write; the JSON in the workflow output is clean).
- Competitor clusters profiled: Wispr Flow, superwhisper, MacWhisper/Aqua Voice, Talon/Serenade, Nuance Dragon, OS built-ins (Win+H / Apple Dictation), OSS local-whisper (VoiceInk, Whispering, Vibe, Willow, Buzz, Handy), cloud/meeting (Otter, Fireflies), and market/pricing.
