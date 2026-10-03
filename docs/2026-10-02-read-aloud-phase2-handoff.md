# Handoff — Echo Read Aloud phase 2: unusual names + Adobe Acrobat

You are picking up ALAN Echo's "Read Aloud" feature. Phase 1 is built, reviewed and merged on branch **`feat/read-aloud`** in `C:\Users\arowm\alan-echo` (not merged to `main`, not pushed). Your job: fix two gaps Alex (the user) asked for.

1. **Unusual names are spelled letter-by-letter** ("Pelosi" → P-E-L-O-S-I). Make them pronounced like words.
2. **Adobe Acrobat doesn't work** (click → "Nothing readable here"). Make click-to-read + word highlight work in Acrobat (and, by the same path, scanned PDFs/images).

## Talk to Alex like this
Lead with the answer, short bullets, plain words (ELI5), no walls of text, bottom-line every decision with a recommendation. See `C:\Users\arowm\.claude\CLAUDE.md`.

## Read first (in this order)
1. `docs/2026-10-02-read-aloud-session-log.md` — what shipped, files, verification, known limits.
2. `docs/superpowers/specs/2026-10-02-read-aloud-design.md` — the approved spec (§5 UIA rules, §6 voice engine, §7 resources, §10 phases: phase 2 = OCR).
3. `docs/superpowers/plans/2026-10-02-read-aloud.md` — Global Constraints still bind you.
4. `.superpowers/sdd/2026-10-02-read-aloud/progress.md` (git-ignored ledger) — every `Ruling:` and `minor (deferred)` line. Key rulings: "auto" device = CPU (GPU leaks ~300 MB after unload); read source = window under the click; UIA batches 200/400; chunks 12/30 words; diphthong mapping to Kokoro symbols.
5. Code: `src-tauri/src/{read_aloud,ra_tts,ra_uia,ra_text,ra_timing,ra_hook}.rs`.

## Hard rules (learned the hard way this session)
- **Never send keystrokes, clicks or focus changes to Alex's apps** during tests. A test once opened incognito windows in his Chrome and ate his typing. Tests launch and close only their own windows/processes; browsers only with a throwaway `--user-data-dir`.
- Never panic on Read Aloud paths (`panic = "abort"` in release kills all of Echo). 0 MB at rest: nothing loads/spawns until Ctrl+Shift+E. Hotkey handler only posts a message. Everything `cfg(target_os = "windows")`; macOS CI must still build.
- **No GPL dependencies** (no espeak-ng, no GPL dictionaries). Echo is a proprietary product.
- Alex's installed Echo runs in the tray; Echo is single-instance, so the dev build (`npm run tauri dev`) won't start until he quits it. Ask him — don't kill it.
- Process: use the superpowers skills — brainstorming (this is architectural → short spec addendum + Alex's approval), writing-plans, then subagent-driven-development with per-task review and a final whole-branch review. Alex wants **max parallelism** (separate git worktrees per task, controller merges) and high quality. Write a session log in `docs/` before stopping.

## Step 0 — check the live test (still open from phase 1)
Phase 1 has never been run in the real app UI. Ask Alex whether he did the live test (Chrome / Edge / Edge PDF / Word / Notepad: highlight lines up, pill buttons clickable, overlay draws, memory). If not, offer to launch the dev build first — phase 2 builds on it. Record results in the session log.

## Workstream A — unusual names (pronunciation fallback)

**Where it happens:** `misaki-rs` 0.6 (built with `default-features = false`, i.e. no espeak). For out-of-dictionary words it spells letters. It has a hook: `misaki_rs::G2P::with_fallback(Language::EnglishUS, Some(Box<dyn misaki_rs::fallback::Fallback>))`, where `trait Fallback: Send + Sync { fn phonemize(&self, word: &str) -> Result<String, FallbackError>; }` (see `~/.cargo/registry/src/*/misaki-rs-0.6.0/src/fallback.rs` and `g2p.rs` ~line 375). `ra_tts.rs` currently constructs `G2P::new(Language::EnglishUS)` — swap in `with_fallback`.

**Recommended approach (spike to confirm):**
1. **Big dictionary first:** CMUdict (BSD-2-Clause, ~134k words incl. many names) or `open-english-pronouncing-dictionary` crate (check its licence + size). Convert ARPAbet → misaki/Kokoro phoneme symbols (stress marks ˈ ˌ; diphthongs to Kokoro's single symbols A I O W Y ʤ ʧ — reuse the mapping table in `ra_tts.rs`). Embed compressed or load from the model folder (keep binary + RAM small; measure).
2. **Letter-to-sound for anything still unknown** (e.g. "Zelensky", "Buttigieg", brand names): evaluate a small permissive G2P — candidates: `floravox-g2p` crate ("permissively-licensed espeak-ng replacement, pluggable OOV fallback" — verify licence/quality), a tiny ONNX G2P model (e.g. DeepPhonemizer/OpenPhonemizer — verify licence, size, CPU latency < 5 ms/word), or rule-based NRL letter-to-sound. Pick by: licence (no GPL), quality on a name list, latency, size.
3. **Keep spelling for real acronyms:** all-caps 2–5 letters with no vowels pattern / known acronym list (FBI, NATO handled as word? decide) → spell; otherwise pronounce.
4. **Test set:** ~60 names/words (Pelosi, Zelensky, Buttigieg, Nguyen, Siobhan, Schumer, Kamala, Xi, Putin, Netanyahu, Anthropic, Kubernetes, Nvidia, Ozempic, etc.) — produce a WAV per word at 2.2x and let Alex listen to a short A/B (current spelled vs new). Word timing must still align (`ra_timing::align` maps misaki tokens to words — the fallback keeps one token per word, so it should).
5. Unit tests: fallback is used only for OOV; acronym rule; no panics on emoji/CJK/numbers; latency budget.

## Workstream B — Adobe Acrobat (and scans) = phase 2 OCR

**What phase 1 found (spike A, scripts in the old scratchpad):** Acrobat exposes **no UIA TextPattern**. It does expose full text in reading order through **MSAA** (legacy accessibility), but only paragraph-level rectangles — no word boxes.

**Recommended approach (spike to confirm):**
1. `ocr.rs`: capture the clicked window's visible client area (Windows.Graphics.Capture or PrintWindow/BitBlt — test which works for Acrobat's GPU-rendered surface) → `Windows.Media.Ocr` (built into Windows, free, no model) → words + rects in screen px. Same shape as UIA output so the controller can use it.
2. **Hybrid for Acrobat:** MSAA gives the *text beyond the screen* in correct order; OCR gives *word rectangles* for what's visible. Align the OCR words to the MSAA text sequence (reuse `ra_timing::align`-style normalized matching) so reading continues past the fold and the highlight works on visible words. When the next words are off-screen, scroll the window by posting **WM_MOUSEWHEEL to the Acrobat window only via UIA/MSAA scroll APIs** — never global input — then re-OCR. If MSAA scroll isn't available, fall back to "reads to the bottom of the visible page, then stops" (spec §10) and say so.
3. **Generic fallback:** any window with no TextPattern and no MSAA text (scanned PDF, image, canvas app) → OCR-only of the visible area.
4. Plug in at the existing "no TextPattern" branch in `ra_uia.rs` (`NO_TEXT` path → currently the "Nothing readable here" toast). Keep the `Uia` interface (pick/more/rects/scroll_to) so `read_aloud.rs` barely changes — implement an OCR-backed source behind the same interface.
5. Hover outline in pick mode should also work over OCR words (OCR the window once on hotkey press; cache until scroll/resize).
6. Resources: OCR only on demand, no background polling, capture buffers dropped after use. Measure latency (target: first audio ≤ 1.5 s after click in Acrobat).
7. Test: a multi-page PDF opened in Acrobat (launch Acrobat yourself on a generated PDF, close only it), plus a scanned-image PDF. Manual tests `#[ignore]`.

## Deliverables
- Short spec addendum (`docs/superpowers/specs/2026-10-0X-read-aloud-phase2-design.md`) approved by Alex → plan → SDD execution on a branch off `feat/read-aloud` (e.g. `feat/read-aloud-phase2`), reviewed, merged into `feat/read-aloud`.
- WAV A/B for names; Acrobat demo working; session log; update `project_echo-read-aloud` memory.
- Do not merge to `main` or push without Alex's explicit OK.
