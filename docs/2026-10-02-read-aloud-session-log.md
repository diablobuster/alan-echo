# 2026-10-02 — Read Aloud (phase 1) session log

Branch: `feat/read-aloud` (not merged to `main`, not pushed). Spec: `docs/superpowers/specs/2026-10-02-read-aloud-design.md`. Plan: `docs/superpowers/plans/2026-10-02-read-aloud.md`. Design: https://claude.ai/artifact/7VdxFzrRDRXjP9nbT4rLP2 (v4, black-and-white).

## 1. What shipped (on the branch)
Ctrl+Shift+E → pick mode → click any word in any visible window → Echo reads aloud from there (Kokoro-82M, local) at 2.2x with a live on-page word highlight, a Now Reading pill, media keys, and a Settings section. Off by default; nothing loaded at rest.

## 2. Files and intent
| File | Intent |
|---|---|
| `src-tauri/src/ra_model.rs` | Model file paths in `%APPDATA%/ALAN Echo/models/kokoro`, `is_installed` (q8 + fp32 + tokenizer + voice) |
| `src-tauri/src/ra_text.rs` | UIA tokens → speakable words (hyphen merge, punctuation, paragraphs); sentence chunking (12 first / 30 max) |
| `src-tauri/src/ra_timing.rs` | Phoneme durations → per-word sample times (−36 ms offset); misaki token ↔ word alignment |
| `src-tauri/src/ra_wsola.rs` | WSOLA pitch-preserving time-stretch, exact output length |
| `src-tauri/src/ra_tts.rs` | Kokoro via ort rc.13: lazy load, CPU default ("auto"), DirectML opt-in with CPU fallback, idle unload, self-reload, `covered` truncation signal, diphthong mapping to Kokoro symbols |
| `src-tauri/src/ra_player.rs` | Streaming cpal output, sample-exact position under one lock, 2 ms fade-in |
| `src-tauri/src/ra_uia.rs` (+ `ra_uia_manual_tests.rs`) | UIA worker thread: re-targets to the window under the point, pick (200) / more (400) batches, fresh rects with coalescing, Chromium scroll via ScrollItemPattern |
| `src-tauri/src/ra_hook.rs` | WH_MOUSE_LL pick hook: swallows click down+up, passes clicks over Echo's own windows |
| `src-tauri/src/read_aloud.rs` | State machine + Session + controller thread: hotkey, pick, synth worker, timeline, overlay/pill windows, media keys, teardown |
| `src-tauri/src/main.rs` | cfg(windows) wiring: AppState field, `ra_*` commands, hotkey registration gated on enabled + installed, live settings toggle |
| `src-tauri/capabilities/read-aloud.json` | Permissions for `ra-overlay` / `ra-pill` |
| `src/main.jsx`, `src/components/ReadAloudOverlay.jsx`, `NowReadingPill.jsx`, `src/readAloud.css` | Transparent overlay + pill UI (black-and-white) |
| `src/components/SettingsPanel.jsx` | "Read Aloud (beta)" section |
| `tools/read-aloud/prepare_model.py`, `README.md` | Download + DirectML-patch the model into the data dir; ort rc.13 call shape |
| `src-tauri/Cargo.toml` | `ort =2.0.0-rc.13` (directml), `misaki-rs` (no espeak), windows features (Windows-only) |

## 3. Commits / PRs
31+ commits on `feat/read-aloud` (spec ec8b0e9, plan ae22a9c, tasks 1–10 + review fixes, final-review fix merge 50ab04d). No PR opened, nothing pushed.

## 4. Verification
- `cargo test`: 143 passed, 0 failed, 14 ignored (ignored = real-model / real-device / real-app manual tests; each was run at least once by its implementer and passed — Notepad x2, hook, Edge, GPU + CPU synth, idle unload, headless pipeline smoke).
- `cargo build`: 0 warnings. `npm run build`: OK.
- Headless smoke (real CPU voice + real audio device, muted): 65 words, first audio 1.32 s, all words highlighted in order.
- Measured: CPU engine ~490 MB while loaded, ~20 MB after unload; GPU ~580 MB and keeps ~300 MB after unload (why "auto" = CPU).
- Every task passed an independent spec + quality review; final whole-branch review → 5 Important fixed + re-reviewed clean.
- **NOT yet done:** the live app test (Task 11 matrix: Chrome / Edge / Edge PDF / Word / Notepad / Acrobat; pill clickable while non-focusable; overlay draws over WebView2; resource check). macOS build not compiled (gating checked by reading).

## 5. Follow-ups / known limitations
- Task 11 real-world matrix + resource check (needs the installed Echo quit first — single instance).
- `DirectML.dll` is not bundled for installer builds (dev runs only).
- Acrobat / scans need phase 2 OCR (currently "Nothing readable here").
- Names not in the dictionary are spelled letter-by-letter (no CMUdict fallback yet).
- Deferred minors are listed in the SDD ledger rulings summary (final message of this session).
- Side effects during testing: a UIA test briefly opened 2 incognito Chrome windows (closed) and swallowed 2 typed characters into a test Notepad; Notepad no longer restores the spike's `notes.txt` tab (file intact); leftover `%TEMP%\RaUiaBrowser50936-profile` folder.
