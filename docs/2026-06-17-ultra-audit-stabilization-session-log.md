# Session log — Ultra-audit stabilization (Slices 1–8)

**Date:** 2026-06-17
**Branch:** `feat/ultra-audit-stabilization` (created off `fix/hotkey-fingerprint-latency` @ `489e2fc`, the shipped fingerprint fix)
**Driver:** `docs/2026-06-17-ultra-audit-handoff.md` §4/§5/§6/§9. User directive: "queue all changes and do it in whatever order you find logical and optimal."
**Method:** serial vertical slices (overlapping files → no parallel writes), TDD per change, verified on Windows where possible. 9 commits, `cargo test` 40/40 green.

---

## 1. What shipped

| # | Change | Type |
|---|--------|------|
| 1 | Precompile text-cleanup regexes once instead of per `clean()` | perf P0 |
| 2 | Real `verbatim` cleanup mode (bypasses destructive baseline transforms) | feature |
| 3 | Deterministic find→replace rules in the cleanup engine + settings wiring | feature |
| 4a | `verify_token` forgery/malformed coverage + extracted testable `check_claims` | test/correctness |
| 4b | Fingerprint-collapse guard (UUID fallback for <2 resolved components) | correctness/security |
| 5 | `resample()` coverage; verified the handoff's empty-input "panic" is a FALSE POSITIVE | test |
| 6 | Re-paste-last global hotkey (`CmdOrCtrl+Shift+V`) | feature |
| 7 | CI gate: windows+macos `cargo test` + frontend build | infra |
| 8 | macOS parity (osascript hot-path + Metal) — **specced, not coded** | deferred |

---

## 2. Files touched + intent

- **`src-tauri/src/text_cleanup.rs`** — (1) moved `fix_acronyms`/`apply_informal_corrections`/`tighten_phrasing` regex sets to `once_cell::Lazy` statics (`RE_ACRONYMS`/`RE_INFORMAL`/`RE_TIGHTEN`); (2) `verbatim` early-return at top of `clean()`; (3) `rules: Vec<(Regex,String)>` field + `set_rules`/`set_level`/`apply_rules`, applied after final capitalization and in verbatim. 9 new tests.
- **`src-tauri/src/main.rs`** — `text_replace_rules` startup load + `set_setting` arm + `parse_replace_rules` helper; `text_cleanup_level` arm now uses `set_level` (preserves rules); `paste_last_transcript` worker + `register_paste_last_hotkey` + setup() registration + `pasteLast` in hotkeys JSON.
- **`src-tauri/src/activation.rs`** — extracted `check_claims`; routed `machine_fingerprint` through `fingerprint_with_fallback` (+`resolved_component_count`, `read_or_create_fallback_id`, `fallback_id_path`). 13 new tests.
- **`src-tauri/src/audio.rs`** — first unit tests for `resample` (empty/single/identity); no production change.
- **`src/components/Dashboard.jsx`** — `paste-last` event listener → confirmation toast (mirrors existing `dictate-toggle` idiom).
- **`src/components/SettingsPanel.jsx`** — added `verbatim` to the cleanup-level `Seg` options.
- **`.github/workflows/ci.yml`** — new. backend matrix (windows-latest, macos-14) hard-gates `cargo test` (frontend built first for `generate_context!`); frontend job hard-gates `npm run build`; fmt/clippy/lint informational.
- **`docs/2026-06-17-slice8-macos-parity-spec.md`** — new. file:line spec for the deferred macOS work.

---

## 3. Commits / PRs

Commits `489e2fc..HEAD` (9): `ada9fe6` regex precompile · `8d3112e` verbatim · `07e3629` find/replace · `b41abe6` verify_token tests · `00ab2d7` fingerprint guard · `a0d7704` resample coverage · `ca1a242` re-paste-last · `175bc07` CI · `eae9e56` Slice 8 spec.

**No PR opened, nothing pushed** — work sits on the local branch for review. The parallel session's working-tree changes (`package-lock.json`, `src-tauri/Cargo.lock`, untracked docs) were left untouched and never staged.

---

## 4. Verification status

**Tested (Windows):**
- `cargo test` 40/40 green (was 19 at session start; +21 new tests).
- Each behavioral change followed TDD: watched the test fail for the right reason, then pass. Notably the fingerprint guard's collapse was reproduced (identical hash regardless of machine) before fixing.
- `vite build` exit 0 (frontend edits bundle); no new eslint errors on my added lines.
- Working tree clean of my changes; commits scoped to single concerns.

**Deferred / NOT verified:**
- **Re-paste-last end-to-end** — needs a running GUI; the resident tray app blocks `tauri dev`. Requires manual test after rebuild/reinstall.
- **Frontend visuals** — verbatim `Seg` now has 4 options (overflow check), paste-last toast rendering.
- **CI** — never executed; first GitHub push/PR is the real validation.
- **Slice 8 (macOS)** — not coded; no Mac toolchain/Apple Silicon/Metal available. Spec only.
- **`cargo fmt`/`clippy`/`npm lint`** — pre-existing debt; intentionally informational in CI.

---

## 5. Follow-ups / known limitations

- **Find/replace has no Settings UI** — backend fully wired; `text_replace_rules` is currently only settable via `settings.json`. Add an editor (planned with rebindable hotkeys).
- **Re-paste-last UX decisions to confirm:** (a) `Ctrl+Shift+V` globally intercepts "paste without formatting" in terminals/editors — intuitive but intrusive; (b) reuses `deliver_text`, so with `auto_paste` off it only re-copies. Both easy to change; proper fix is user-rebindable hotkeys.
- **Slice 8 macOS parity** — execute `docs/2026-06-17-slice8-macos-parity-spec.md` on a Mac; the new `macos-14` CI job will compile the `cfg(macos)` code.
- **Audit false positive corrected:** the handoff's `resample` empty-input panic does not exist (subtraction is inside a closure that never runs for empty input).
- **Adversarial self-review** (6 dimensions → per-finding verification) was launched at session end; confirmed findings, if any, to be addressed in a follow-up and folded into this log.
- Untouched from the handoff backlog (next sessions): macOS `osascript` is the same class as items already done; honest progress bar; download integrity check (§6 High); CSV formula-injection (§6); custom vocabulary; rebindable hotkeys; push-to-talk.
