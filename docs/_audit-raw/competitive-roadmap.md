All pointers verified against the actual code. Here is the section.

---

# Standout Feature Strategy & Roadmap

## 1. Positioning Thesis

**ALAN Echo is the dictation app you *own* instead of *rent*: 100% local speech-to-text that never leaves your machine, GPU-fast on Windows (CUDA) and parity-fast on Mac (Metal), for one honest ~$89 — not a $144/yr Wispr subscription, a $96/yr Aqua rental, or superwhisper's rug-pulled $849 lifetime tier.** The cloud leaders (Wispr Flow, Aqua Voice, Otter, Fireflies, Willow) require internet for every word, fail on a plane, and upload your audio to third parties — Wispr was caught uploading active-window screenshots and Otter faces a class action (*Brewer v. Otter.ai*) for training on conversations; Echo's entire architecture (whisper.cpp on `127.0.0.1`, audio deleted on every code path, no telemetry) is the structural answer to the single loudest complaint in the category. Echo's defensible wedge is the *bundle no one else combines*: turnkey GPU + true Win/macOS parity + no API keys + searchable/exportable history — capabilities the free OSS peers (Handy, Whispering) and the OS built-ins (Win+H, Apple Dictation) structurally lack, delivered without the recurring cost users are actively fleeing.

---

## 2. Competitive Matrix (condensed)

| Capability | Who has it | ALAN today | Action |
|---|---|---|---|
| Global hotkey → speak → auto-paste at cursor | Wispr, superwhisper, VoiceInk, Handy, Dragon | ✅ Yes | Hold |
| Fully local / offline transcription | superwhisper, VoiceInk, Handy, MacWhisper | ✅ Yes | **Spearhead positioning** |
| GPU acceleration (Windows CUDA) | Handy, Vibe, Buzz | ✅ Yes | Hold |
| GPU acceleration (macOS Metal) | superwhisper, VoiceInk, MacWhisper | ❌ CPU-only on Mac | **P0 — flip build flag** |
| Cross-platform Win + macOS | Wispr, Aqua, Handy, Willow | ✅ Yes | Defend parity |
| One-time / lifetime price | VoiceInk, Whisperstream, MacWhisper | ✅ Yes (~$89) | **Anti-subscription wedge** |
| Custom vocabulary / term boosting | Wispr, superwhisper, Aqua, VoiceInk, Dragon | ❌ No | **P0 — #1 accuracy gap** |
| Deterministic find/replace rules | superwhisper, VoiceInk, Aqua | ❌ No | **P0 — cheap, local** |
| Re-paste last transcript hotkey | Wispr, WhisperType | ❌ No | **P0 — the seed idea** |
| Verbatim / raw mode | Handy, superwhisper | ⚠️ Partial | P0 — trust angle |
| Push-to-talk (hold) vs toggle | Handy, superwhisper, VoiceInk, Whispering | ⚠️ Partial | P1 |
| Recent-transcripts quick-pick palette | Talon, Wispr Scratchpad | ❌ No | P1 |
| On-device translate-to-English | (cloud only, by upload) | ❌ No | P1 |
| Per-app cleanup profiles | superwhisper Modes, VoiceInk Power Mode | ❌ No | P2 |
| Transcript history search + export | superwhisper, MacWhisper, Buzz | ✅ / ⚠️ export partial | P1 — broaden formats |
| BYOK cloud-LLM post-processing | superwhisper, MacWhisper, VoiceInk | ❌ No | **Dismissed (breaks local-first)** |
| Enterprise SOC2/HIPAA via cloud | Wispr, Otter, Dragon Medical | ❌ No | **Dismissed (cloud-only)** |

---

## 3. Build-Now Roadmap (P0)

The four features below scored highest on **value × differentiation × feasibility** and carry **low/no dual-platform risk**. Every file/function pointer below is verified against the current tree.

### P0-1 — Re-paste Last Transcript hotkey *(the seed idea)*
**What it is.** A dedicated global hotkey (`CmdOrCtrl+Shift+V`, with the cancel-style fallback-probe for collisions) re-inserts the most recent transcript into whatever app is focused *now*, with no re-recording.

**Why it differentiates.** 100% local from the SQLite store you already own — Wispr does this in the cloud or via add-ons; the OS built-ins (Win+H, Apple Dictation) are fire-and-forget with no recall. It turns the existing history into a one-key recall/recovery surface and fixes the universal "my text went into the wrong window" failure by re-firing over the correct window. Market it as a **reliability/recall convenience**, not a moat (Wispr ships it too).

**Implementation pointers.**
- Add `#[tauri::command] fn paste_last(app, state)` near `deliver_text` (`src-tauri/src/main.rs:448`). Capture the live target with `paste::foreground_window()` (`paste.rs:28` Win / `:142` mac), set `*state.paste_target.lock() = Some(target)`, read the newest row via `state.db.lock().get_page(0,1)` (`db.rs:107`, `ORDER BY timestamp DESC`), no-op toast if empty, then call `deliver_text(&app, &state, &text)`.
- **Critical**: `deliver_text` consumes the target via `state.paste_target.lock().take()` (verified at `main.rs:452`) and already does clipboard-write + UIPI-safe paste + clipboard-restore + `auto_paste` respect — so setting `paste_target` immediately before calling it is exactly right and reuses all guards for free. Do the capture **in the Rust command** (not via the JS event round-trip in `register_emit_hotkey` at `main.rs:716`, which only `emit()`s) to avoid capturing Echo's own window.
- Register a 4th hotkey in the `setup()` block (`main.rs:1053–1065`); add `paste_last` to `invoke_handler!`; add `listen('paste-last', () => invoke('paste_last'))` in `src/components/Dashboard.jsx:125`.

**Effort:** small (<1 day). **Cut from v1:** the cleaned-vs-raw and auto-paste-interaction toggles (scope creep); ship cleaned-text + `auto_paste`-respecting only.
**Dual-platform:** Low. Reuses already-shipping infra on both OSes; a global-shortcut press doesn't change the frontmost app, so re-capturing foreground at press time is correct on Win (instant `GetForegroundWindow`) and mac (osascript, ~100–200 ms, same as today's dictation paste; needs existing Accessibility permission).

### P0-2 — Custom Vocabulary / term boosting *(local, no key, no cloud)*
**What it is.** A user-managed word/phrase list (names, brands, jargon, code identifiers, coworkers) in `settings.json`, fed to whisper as an initial-prompt bias **plus** a deterministic post-fix casing pass.

**Why it differentiates.** Custom dictionary is the **#1 accuracy complaint** across the whole category and a flat "no" for Echo today; every serious paid peer has it. Doing it **fully on-device with no API key** is something superwhisper/Wispr/Aqua cannot honestly call "private," and the OS keyboards structurally can't learn jargon. Closes a visible matrix gap while *reinforcing* the local-first pillar.

**Implementation pointers.**
- **Layer 1 (verified feasible against the bundled binary):** thread a `vocabulary: Mutex<String>` through `WhisperEngine` (mirror the existing `language: Mutex<String>`), and append a `prompt` multipart part in `post_inference` (`src-tauri/src/whisper.rs:515`) — prefer the **per-request `prompt` form field over a `--prompt` launch flag** so word-list edits don't force a multi-second engine reload. (The bundled `whisper-server` parses the `prompt` multipart field — confirmed via the binary's `httplib::MultipartFormData` strings and `--prompt`/`carry_initial_prompt`.)
- **Layer 2 (the reliable half):** a deterministic post-pass in `text_cleanup.rs` that fixes near-miss spellings/casing of the user's terms (model `fix_acronyms`'s `(?i)\b{regex::escape}\b` idiom; cache the per-term regexes — `fix_acronyms` compiles per call today).
- Persist under a new `custom_vocabulary` key (generic JSON store in `settings.rs` — no schema change); add a `set_setting` arm in `main.rs:110–126` that calls `set_vocabulary(...)`; build a textarea editor next to the "Text cleanup" row in `SettingsPanel.jsx` (debounce/apply-on-blur).

**Honest scope:** whisper's initial-prompt is a *soft probabilistic* bias bounded by ~224 tokens — **cap/dedupe/recency-order the prompt** (an unbounded prompt tanks accuracy/latency), ship empty-by-default, and label it "improves," not "fixes." Layer 2 is the guaranteed win.
**Effort:** medium. **Dual-platform:** Low — pure Rust + React, no OS-specific paths; pre-ship check that the macOS `whisper-server` exposes the same `prompt` field (near-certain).

### P0-3 — Deterministic find-and-replace rules *(text substitution table)*
**What it is.** A user table of `from → to` rules applied to every transcript before paste: correction rules (`'alan eco' → 'ALAN Echo'`, `'github' → 'GitHub'`) and short canned expansions (`'my email' → address`). Case-insensitive match, exact-case output, whole-word boundaries.

**Why it differentiates.** Deterministic, local, no-LLM, no-key — the user can read exactly what changes (no AI guessing), and CSV/JSON import/export feeds the cross-platform "own your data, move it between Windows and macOS" story. Pairs with the dictionary: vocabulary *biases* recognition, replacement rules *guarantee* the final string. (Parity with superwhisper/VoiceInk/Aqua, so it closes a gap rather than being a wedge — but it's cheap and high daily value.)

**Implementation pointers.**
- `text_cleanup.rs`: add a precompiled `rules: Vec<(Regex,String)>` field to `TextCleanupEngine` (compile **once**, not per `clean()`), modeled on `apply_informal_corrections()`; invoke `apply_user_replacements()` inside `clean()` **after** the final `fix_capitalization` and **before** `final_cleanup`'s period-append so user casing is preserved and emails/URLs don't get a spurious trailing `.`.
- `clean()` is the single chokepoint hit by both live dictation (`transcribe`, `main.rs:421`) and the `clean_text` command (`main.rs:661`), so rules apply to auto-paste **and** re-paste for free.
- Wire a `user_replacements` arm into the `set_setting` match (`main.rs:110–126`, mirroring the live `text_cleanup_level` rebuild); store under a new key in `settings.rs`; build the editor + CSV/JSON import beside the cleanup-level UI in `SettingsPanel.jsx` (reuse the existing `clean_text` preview call so rules show live).
- **Isolate multi-line snippet expansion** (signatures) as a separate last-stage pass that bypasses `normalize_whitespace`/`fix_capitalization`, or it gets newline-flattened and re-cased.

**Effort:** small. **Dual-platform:** None — pure Rust + React.

### P0-4 — Verbatim / raw mode *(transcribe exactly, skip cleanup)* + macOS Metal parity
Two cheap, high-trust correctness fixes bundled:

**(a) Verbatim mode.** Add a real `"verbatim"` early-return at the top of `TextCleanupEngine::clean()` (`text_cleanup.rs:70`) that bypasses the **unconditional baseline** — `remove_hallucinations` (strips all `[..]`/`(..)`, *kills `arr[i]`, `[sic]`, and returns empty on stopword-only input*), `fix_punctuation` (force-appends `.`), and `fix_capitalization` (`i→I`, brand-casing). **Note:** "just use `raw_text`" is *not* verbatim — the harmful transforms live in the baseline, not the standard/aggressive arms. Also fix the already-shipping but dead `'light'` option that silently falls through. Add `'verbatim'` to the `SettingsPanel.jsx` options array; add regression tests asserting brackets/lowercase-`i`/no-forced-period survive. This is the **trust angle for lawyers/devs/quoters** that cloud rewriters (Wispr/Aqua) structurally can't match. **Effort:** small. **Dual-platform:** None.

**(b) macOS Metal acceleration.** Echo's headline asset is "GPU-fast, cross-platform," yet GPU only exists on Windows — a Mac buyer pays ~$89 for a CPU experience next to Metal-fast MacWhisper/VoiceInk. **The fix is a build-flag change, not a download pack:** `scripts/prepare-resources-macos.sh:31–37` compiles the retail server with `-DWHISPER_NO_METAL=1` and its own comment says *"Remove this flag in a future release to enable Metal acceleration."* Metal links the system framework (present on every Mac since 2014) and adds negligible size — the `packs.rs` download machinery exists only because of the Windows 2 GB NSIS limit, which macOS doesn't have. Scope: (1) drop `WHISPER_NO_METAL`, build `-DGGML_METAL=1`, stage the `.metallib` so the signed `.app` finds it; (2) add a `'metal'` arm to `binary_kind()` + a Mac Metal probe so `engine_kind` reports `'metal'`; (3) surface it in `EngineStatus.computeText`/`gpuVerdictText` in `SettingsPanel.jsx`. **Effort:** medium. **Must validate on a real Apple-Silicon device** (build targets `aarch64-apple-darwin` only — decide Apple-Silicon-only, leave Intel on CPU). Skip the download-pack variant — large effort for zero benefit over the flag.

---

## 4. Backlog (P1 / P2)

**Writing quality**
- **On-device translate-to-English** (P1) — `translate` per-request form field in `post_inference` (not a reload-forcing flag); reuse the multilingual model download. Strong differentiation (cloud peers translate by *uploading*), narrow audience (X→English only; quality wants medium/large, current path pulls only `base`).
- **Spoken formatting commands** ("new line", "bullet point", "comma") (P2) — new pass in `clean()`, but must emit a sentinel token that survives `normalize_whitespace` (which strips `\n`) and suppress the forced trailing period on list/break lines. Parity with Apple Dictation/Win+H, not a wedge.
- **Reprocess past transcript at a different level** (P2) — `raw_text` is persisted, so feasible; make it **non-destructive** (preview/new-row, not overwrite) and strike the phantom "dictionary + replacement rules" claim.
- **Code-casing formatters** (camelCase/snake_case/kebab/PascalCase) (P2) — pure string transforms on `raw_text`; ride on the P0-1 re-paste scaffolding with **one** "paste as…" picker hotkey, not 5 IDE-colliding chords.

**Privacy / trust**
- **Audio Privacy panel** ("recordings deleted — here's the proof") (P1) — *verified true*: WAV deleted on transcribe/cancel/discard/startup-sweep; "0 at rest" counter + "Verify now" is cheap and on-brand. **Keep copy to ALAN's own behavior** (legal-audit branch); let the marketing site do competitor contrast; be precise that only **audio** is deleted (transcript text stays in SQLite).
- **Transcript retention controls + Delete-all with VACUUM** (P1) — `purge_older_than`/`wipe_all`/VACUUM are clean siblings of existing `db.rs` methods; surfaces the hidden `backups/` dir that silently duplicates transcripts. **Make Panic Wipe a confirm-modal, never an instant hotkey** (irreversible footgun).
- **Privacy-first onboarding + fingerprint disclosure** (P1) — **only after correcting the network-surface inventory**: the "3 endpoints" claim is false (6+: activation, version, installer, plus third-party `huggingface.co` model fetch and GPU pack downloads). The "offline activation path" does not exist — drop or scope separately.

**Reach / accessibility**
- **Push-to-talk (hold) vs toggle** (P1) — high-value parity; `register_emit_hotkey` (`main.rs:716`) drops `ShortcutState::Released` today. Needs auto-repeat suppression, a **short PTT-specific safety timeout** (not the 300 s cap) for missed key-ups, and mode-aware tray menu + cancel-hotkey lifecycle. Per-OS keyup testing required.
- **Hands-free continuous mode** (silence auto-stop) (P2) — strong accessibility value; needs a **real VAD with hangover** (the 0.001 RMS flag is binary, will clip pauses), an `AppHandle`/channel plumbed into `recorder_thread` (it has none today), and **trial-count handling** (`transcribe` increments per call — continuous burns the 50-dictation trial in one session).
- **Low-end "Lite"/tiny model tier** (P2) — engine already tiers `tiny`→`Lite` in `resolve_model`/`model_label`; just expose it in `list_models`/`download_model`/Seg. The auto-recommend onboarding logic is net-new and accuracy-risky.

**Power-user**
- **Recent-transcripts quick-pick palette** (P1) — the multi-item sibling of P0-1; ship *after* re-paste-last proves the deliver-by-id command. The cost is the **focus dance** (overlay must capture foreground *before* it steals focus, then hand it back) + a new capability file (current capabilities scope only the `main` window).
- **Tray submenu → recent transcripts** (P1) — lowest-friction history surface for mouse-only/low-dexterity users; reuses `get_page` + `deliver_text`.
- **Per-app cleanup profiles** (P2) — Echo's answer to superwhisper Modes / VoiceInk Power Mode on a *reliable OS handle* (not OCR), but **large**: requires a real "raw" level (doesn't exist), a process-name resolver written twice (none today), and a profiles schema. macOS PID→app-name is the weak leg.
- **Import/export of vocab + replacements + settings** (P2) — cheap JSON portability that supports the user's own Win↔Mac workflow.

---

## 5. Explicitly Dismissed

| Feature | Reason |
|---|---|
| **BYOK cloud-LLM post-processing** (summarize/rewrite via OpenAI/Anthropic/Groq keys) | Reintroduces the recurring per-token cost and cloud round-trip users are *fleeing*; directly undercuts the local-first/one-time wedge. superwhisper/MacWhisper/VoiceInk do this — that's the trap, not the goal. |
| **Undo / "scratch that" backspace injection** | Built on a **false premise**: Echo pastes via clipboard + Ctrl/Cmd+V, not keystroke typing, so `grapheme_count(text) ≠ backspaces_needed` (IDE auto-indent, autocorrect, IME all transform it). Over/under-deleting silently eats the user's *real* document — strictly worse than the mis-paste. Native Ctrl/Cmd+Z already does this correctly. The cited UIPI guard is Windows-only. |
| **Encrypted-at-rest SQLCipher vault** | Large, high-blast-radius (corrupt key handling bricks all history), and a **partial guarantee that the target HIPAA buyer sees through**: raw WAVs hit disk and transcript text hits the clipboard regardless. `bundled-sqlcipher` fights the Windows CUDA pipeline + macOS universal build. Cheaper honest wins (WAV zeroize, retention cap, DPAPI/Keychain obfuscation) beat it without a false "HIPAA" promise. |
| **System-audio / meeting capture, speaker diarization, mobile companion, whole-OS voice control** | Out of desktop-dictation scope and/or large lifts that don't reinforce the core wedge; the meeting-bot space is exactly the Otter/Fireflies lawsuit territory Echo contrasts *against*. |
| **Enterprise SOC2/HIPAA/SSO via cloud** | Cloud-dependent by construction — contradicts the offline guarantee. Echo's compliance angle is *"nothing leaves the machine,"* delivered locally, not via a backend. |
| **"Offline-verified / 3-endpoint network audit" badge (as proposed)** | The "3 self-owned endpoints" claim is **false** — `main.rs:605` fetches models from third-party `huggingface.co`; `packs.rs` adds more. An honest audit would contradict the badge. Ship only a static "audio never leaves this device" line; fix the network inventory first. |
| **macOS Metal *download pack*** | Solves a Windows-only 2 GB-NSIS problem that macOS doesn't have. Use the build flag (P0-4b) instead. |

---

## 6. Quick Wins (ship first)

1. **Re-paste Last Transcript hotkey (P0-1)** — *trivial-to-small*, the user's seed idea, every dependency already exists; the anchor for the recent-transcripts siblings.
2. **Verbatim mode + fix the dead `'light'` option (P0-4a)** — small; fixes a latent "why did it change my words?" surprise *and* lands the lawyer/dev/quote trust angle.
3. **Deterministic find-and-replace rules (P0-3)** — small, pure-Rust, zero platform risk, compounding daily value for heavy users.
4. **Audio Privacy "0 at rest" counter + Verify now** — small; converts an *already-correct* implementation detail into a headline trust touch (ALAN-only copy).
5. **Broaden transcript export** (TXT → +SRT/JSON/MD) — low-effort; strengthens the "own your data, free to export" story vs Fireflies (pay-to-export) and Otter.

Sequence: ship #1–#3 in the first release as the differentiating bundle; #4–#5 as the trust/data-ownership follow-on. P0-2 (vocabulary) and P0-4b (Metal) land next as the two highest-value "close a visible gap" items.