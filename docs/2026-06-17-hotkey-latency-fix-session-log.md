# Session Log — 2026-06-17 — Hotkey-to-beep latency fix

## 1. What shipped

Fixed the reported bug: pressing the dictation hotkey took up to ~15s before the
start beep, and mashing the key produced "Recording too short — try a full
sentence." Root cause was an expensive, process-spawning syscall
(`machine_fingerprint()`) sitting on the hotkey-to-record path and being
recomputed on every call. Fix memoizes the fingerprint for the process lifetime
and warms the cache at startup, off the hotkey path.

Diagnosed with systematic-debugging (evidence-first) and implemented with TDD
(failing test → fix → green).

## 2. Files touched and intent

- **src-tauri/src/activation.rs**
  - Added `use std::sync::OnceLock;`.
  - Memoized `machine_fingerprint()` via a process-lifetime `OnceLock`. The
    hardware IDs (ProcessorId / BaseBoard serial / DiskDrive serial on Windows;
    ioreg/sysctl/diskutil on macOS) are immutable while the process runs, so it
    is now computed exactly once instead of on every license check. This fixes
    every caller at once: `is_activated` (→ `require_license` → `start_recording`
    and `transcribe`), `get_activation_status`, trial machine binding.
  - Extracted the pure hash step into `fingerprint_from(components)` so the
    algorithm is unit-testable.
  - Added `#[cfg(test)] mod tests`:
    - `machine_fingerprint_is_memoized` — the failing-first test; 2nd call must
      be served from cache (<50ms).
    - `fingerprint_from_is_stable_sha256_of_pipe_join` — pins SHA-256 of the
      `'|'`-joined components against a known vector, so caching/refactors can
      never silently change the fingerprint value (which would brick every
      already-issued Ed25519 activation token).

- **src-tauri/src/main.rs**
  - Added a startup warm-up thread (right after `setup_logging`) that calls
    `activation::machine_fingerprint()` once in parallel with the rest of
    startup, so the cache is hot before any hotkey press — the first press is
    fast too, not just subsequent ones.

- **.claude/debug-journal.md**
  - Added a 2026-06-17 entry documenting symptom, root cause, evidence, fix,
    and lesson.

No frontend changes. `Dashboard.jsx` correctly beeps only after a confirmed
`start_recording`; with the backend now fast (<1s), the existing UX is right and
the "too short" cascade disappears on its own.

## 3. Commits / PRs

- None. Changes are **uncommitted**, currently in the working tree on the
  unrelated `legal/audit-app-safe-fixes` branch. Recommended next step: move to a
  dedicated branch (e.g. `fix/hotkey-fingerprint-latency`) and commit.

## 4. Verification

Tested:
- Reproduced the cost empirically: the three PowerShell `Get-CimInstance`
  queries `machine_fingerprint` runs measured **2.1s warm** on this machine
  (cold / under Defender accounts for the reported ~15s).
- Failing-first unit test measured the 2nd `machine_fingerprint()` call at
  **2.077s** before the fix (RED), **<50ms** after (GREEN).
- Full Rust suite: **19 passed, 0 failed**.
- `cargo check --release`: clean compile.

Deferred / not done:
- No full installer build (`npm run tauri build`) and no GUI run — the resident
  tray app is running the old release binary and shares the `com.alan.echo`
  data dir; launching a dev/build instance would clash. End-to-end "press hotkey
  → immediate beep" must be confirmed after a rebuild + reinstall.
- macOS path is shared Rust (cache + warm-up are platform-agnostic; the
  macOS-specific component gatherers are unchanged) but was not built/run this
  session.

## 5. Follow-ups / known limitations

- **Action required for the user to benefit:** rebuild and reinstall. The fix is
  backend Rust; the running tray app won't pick it up until replaced.
- After reinstall, verify first-press latency on a cold start (worst case) and
  confirm "Recording too short" no longer triggers on normal use.
- Optional hardening (not done — out of scope, no failing test): a frontend
  timeout/visual "starting…" state so any future backend stall degrades
  gracefully instead of silently. Current architecture doesn't need it once the
  fingerprint is cached.
- Note: `LicenseManager::is_licensed()` returning a hardcoded `false` means the
  license fast-path is never taken; `require_license` relies entirely on
  `is_activated()` (token verify + now-cached fingerprint). That's by design
  (Ed25519 activation) and is now cheap, but worth remembering — any future
  change that makes `is_activated()` expensive again would re-introduce this
  class of latency.
