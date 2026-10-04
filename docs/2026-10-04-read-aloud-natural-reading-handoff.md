# Handoff: Echo Read Aloud, overnight run to make reading sound natural

You are picking up ALAN Echo's **Read Aloud** feature. Alex (the user) is going to sleep and wants **major improvements overnight** so that listening feels like a person reading the page to him, not a machine reading a string.

Run **autonomously until done**. Don't wait for approvals: Alex pre-approved this run. Record every decision as a `Ruling:` line in the ledger. Before you stop, write a session log and give Alex a short morning summary.

## Alex's ask (verbatim)
> "lets make some major improvements overnight making this as natural as possible. for example dont just go off of punctuation, but also pay attention to where headers and bullet points are to dictate when you take a pause between sentences so things dont get jumbled together. think of other things to improve like that to make it a more natural reading experience."

## How to talk to Alex
- Lead with the answer.
- Use short bullets and plain words (ELI5). No walls of text.
- Bottom-line every decision with a recommendation.
- See `C:\Users\arowm\.claude\CLAUDE.md`.

## Read first, in this order
1. `docs/2026-10-03-read-aloud-phase2-3-session-log.md`: what exists and where. Rounds 2 and 3 landed after it; see their reports below.
2. Specs in `docs/superpowers/specs/`:
   - `2026-10-02-read-aloud-design.md` (phase 1)
   - `…-phase2-design.md`
   - `2026-10-03-read-aloud-phase3-design.md`, which includes the logged spec exception on model speed (cap 1.25).
3. Round reports (untracked):
   - `C:\Users\arowm\alan-echo-r2\round-2-report.md`: gapless playback, the chunk ramp, silence trim with boundary-aware tails (sentence 200 ms / comma 150 ms / mid-phrase 60 ms at 1x, ÷ speed).
   - `C:\Users\arowm\alan-echo-r3\round-3-report.md` and `follow-off-diagnosis.md`: follow evidence rule, keeping the word on screen.
4. Ledger with every ruling so far: `C:\Users\arowm\alan-echo-p3\.superpowers\sdd\2026-10-03-read-aloud-phase3\progress.md`
5. Code, all in `src-tauri/src/`:
   - pipeline: `read_aloud.rs` (controller, chunk queue, follow), `ra_text.rs` (normalize, glued merge, `chunk`), `ra_tts.rs` (g2p_words, spans, synth, trim, speed split)
   - text normalisation: `ra_norm*.rs`, `ra_numwords.rs`, `ra_names.rs`, `ra_homograph.rs`
   - sources: `ra_uia.rs` (UIA/Acro/OCR `Read` enum), `ra_acro.rs`, `ra_ocr.rs`
   - follow logic: `ra_follow.rs`

## Current state (2026-10-04 ~01:45)
- **Branch:** `feat/read-aloud` = 5f9d5bd. It holds phase 1–3 plus rounds 1–3: 541 unit tests, 45 ignored.
  - **Not on `main`, never pushed.** Merging into `feat/read-aloud` is authorised. `main` and pushing need Alex's explicit OK.
- **Running app:** Alex's Echo is a `npm run tauri dev -- --release` session from `C:\Users\arowm\alan-echo`, started by the previous session.
  - Its file watcher **rebuilds and restarts Echo on every merge** (~4 min). That's fine overnight. During the day, ask before merging.
  - If you need it restarted fresh: quit the old dev session's processes (`target\release\alan-echo.exe`, the vite on :1420, whisper-server on :8178), then rerun the same command.
- **Glitch-hunt harness:** `tools/read-aloud/glitch-hunt/` (README inside).
  - Static pass, plus a Kokoro→Whisper round trip on **your own** whisper-server on port 8199. Never touch 8178.
  - The frozen corpora live in `C:\Users\arowm\AppData\Local\Temp\claude\C--Users-arowm\73bf4adb-ff19-45f8-84e8-dda9f5f4c1b4\scratchpad\p3\`, and round-2/round-3 corpora sit next to the build scripts.
  - Current gates: p3 2.04 / round-2 0.56 / round-3 2.58 glitches per 1,000 words.
- **Listening board:** `tools/read-aloud/board/glitch_board.html` (git-ignored). Alex opens it in the morning, so add a "Natural reading" section.
- **Engine:** CPU by Alex's choice. **Don't change the device default.** Kokoro-82M, af_heart, 2.2x default (range 1.0–3.5).

## Hard rules (learned the hard way)
- **Windows / UI tests: only while Alex is asleep.**
  - Tonight he is, so the window-based `#[ignore]` manual tests are allowed overnight: Edge (throwaway `--user-data-dir`), Notepad, Acrobat, OCR test windows.
  - Run them in one batch, close every window you open, and finish well before ~8am.
  - **Never send keystrokes, clicks or focus to his apps.** Tests launch and close only their own windows. If Acrobat or Notepad is already open with his files, skip the test.
  - Pending window tests to run tonight: `follow_reflow_manual`, `uia_browser_manual`, `uia_notepad_manual`, `uia_two_notepads_manual`, `uia_notepad_bench`, `acro_live`, `ocr_live`, `pick_hook_manual`, `watch_hook_manual`, plus the gapless bench on an **idle** machine. The last bench had underruns at 3–3.5x while Alex was using the PC.
- **Never panic** on Read Aloud paths (`panic = "abort"` in release kills all of Echo).
- **0 MB at rest:** nothing loads until Ctrl+Shift+E.
- **Exactly one timing entry per source word.** The highlight never moves backwards.
- Pre-pass ≤ 1 ms per chunk.
- **No GPL/LGPL/share-alike code or data shipped.** Dev-only tools may use them locally; say so in a README.
- No global keyboard hook. Hooks stay observe-only and exist only during a read.
- **Never kill `alan-echo.exe` mid-read** unless restarting it as described above.
- **Process: superpowers skills.**
  - Brainstorming: this is architectural, so write a short spec addendum and self-approve under the autonomous ruling.
  - Then writing-plans, then subagent-driven-development.
  - Each task gets an implementer, then a task review, then a fix loop. Finish with a whole-branch review (most capable model), then one fix wave and a scoped re-review.
  - Max parallelism: one git worktree per independent task; the controller merges.
  - TDD for all logic.
  - Commit trailer:
    ```
    Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_01KN3Tceh8DdXc6dVqVEDh7j
    ```

## The goal: natural reading

### A. Structure-aware pauses (Alex's explicit example)
Today, pauses come only from punctuation plus the chunk-boundary trim. Headers, bullets, table cells and captions run together ("…in 2024 Key Takeaways The company…").

**1. Capture structure from every source** and attach a `Boundary` kind to each source word: `None | Comma | Clause | Sentence | ListItem | Paragraph | Heading{level} | TableCell | TableRow | Caption | BlockQuote | CodeBlock | Section`.
- **UIA (Chrome/Edge/Word):** read per-range text attributes while walking `read_forward`:
  - `UIA_StyleIdAttributeId`: `StyleId_Heading1..9`, `StyleId_BulletedList`, `StyleId_NumberedList`, `StyleId_Quote`
  - `UIA_BulletStyleAttributeId`
  - `UIA_IsHiddenAttributeId`
  - the enclosing element's control type (`ListItem`, `Header`/`HeaderItem`, `DataItem`, `Table`) and `AriaRole` / heading level (Chromium exposes these)

  Spike it first: measure which attributes Chromium, Edge, Word and Notepad actually return, and how much each costs per batch. Keep the 200/400-token batch latency budget: pick ≤ ~150 ms in Chrome.
- **Acrobat:** the IPDDomNode structure types (H1–H6, P, LI, L, Table/TR/TD, Caption, BlockQuote, Figure) through `GetType`/`GetName` on the parent chain of Word nodes. Untagged PDFs fall back to layout: a line-height jump means heading, a leading bullet glyph or a number + "." means list item, a large vertical gap means paragraph.
- **OCR:** layout heuristics. A larger line height than the body (heading); bullet glyphs (•, ◦, ▪, –, *, 1., a)); indentation; vertical gaps (paragraph); column/table alignment.
- **Raw text fallback:** a line break with no terminal punctuation followed by a capitalised line means an implicit sentence/heading break. That stops "Key Takeaways The company" from fusing.

**2. A pause model.** Tune these for listening at speed:
- Natural pause per boundary: word 0 < comma < clause (;:—) < sentence < list item < paragraph < heading-after < section.
- Scale pauses **less than linearly** with speed. At 3x, listeners still need paragraph and heading breaks. For example: `pause / speed^0.6`, with a floor.
- **Headings:** a short pause before, a longer one after, and add a sentence-final cue so Kokoro gives falling intonation ("Key Takeaways." even without a period).
- **List items:** each item ends like a sentence. An intro line ending in ":" gets a clause pause. Optionally say "first, second…" for numbered lists. Make that a setting, off by default; just pause.
- **Tables:** read row by row, a pause between cells, a longer pause between rows. Optionally prefix the column header for short tables, as a setting.
- **Captions, figures, block quotes:** pause-wrapped. Optionally "quote … end quote" (setting, default off).
- **Code blocks:** skip or summarise ("code block") as a setting, default "read". Inline code stays as normalised by phase 3.

**3. Chunking aligned to structure.** Never cut a synth chunk in the middle of a heading. Prefer chunk cuts at boundaries with the largest natural pause. The ramp (12 → 16 → 24 → 30) currently cuts at commas, which produces a falling tone mid-sentence. Prefer sentence or structure boundaries, using lookahead, and keep the gapless guarantee.

**4. Silence comes from the boundary kind,** not a model accident: trim the model's silence at chunk edges, then insert exactly the pause the boundary calls for. Pauses inside a chunk come from punctuation the model sees. For structural boundaries inside a chunk, inject a token Kokoro pauses on (e.g. "." or ";"), or split the chunk.

### B. Other naturalness work (pick by impact; spike, measure, then build)
1. **Prosody across chunks.** Chunks are synthesised separately, so intonation resets.
   - Check whether the style vector row (ra_tts picks `voice[ids.len()]`, so the token-count-dependent style changes with chunk length!) causes timbre or energy jumps between chunks. Compare a fixed row with a length-matched row, measured by the per-chunk spectral centroid/energy variance and a listening A/B.
   - Also try ending non-final chunks without a falling tone (avoid cutting at commas; carry trailing context).
2. **Loudness consistency.** RMS/LUFS-normalise each chunk to a target, with a limiter and a 5–10 ms crossfade at chunk joins, so there are no volume jumps or clicks. Measure the per-chunk loudness spread before and after.
3. **Skip page junk.** Don't read nav menus, cookie banners, "Advertisement", "Share this", "Related articles", "Sign up for our newsletter", social buttons, image alt-text junk, footnote markers [1], "Skip to content", or bylines/timestamps repeated mid-article.
   - Use UIA: `IsOffscreen`, `IsHidden`, landmark types (navigation, banner, complementary, contentinfo), `aria-hidden`, and the ad-iframe heuristics.
   - The reading starts where Alex clicks, so only skip *forward* junk.
   - Keep it conservative and test on real saved pages (local HTML fixtures you write that mirror common news-site layouts).
4. **Links and URLs.** Read link text as text. A bare URL longer than about 40 chars reads as "link to example dot com". Make this a setting.
5. **Parentheticals and asides:** a slight pause before and after (…), —…—, and footnote-style asides.
6. **Dialogue and quotes:** a pause at quote boundaries and attribution ("she said") with a natural flow.
7. **Repeated-name consistency:** the same pronunciation for a name across a read. Cache any GRU result per read.
8. **Sentence-start capitals after headings and bullets:** don't treat them as acronyms or headline mode.
9. **Speed-adaptive behaviour:** at ≥ 3x, slightly longer structural pauses relative to speech, and slightly lower model speed if clarity drops (WER gate).
10. **Anything else you find** in a listening pass of 20 real article types: news, blog, docs page, recipe, Wikipedia-like, forum thread, PDF report, email.

### C. How to measure "natural" without Alex
- **Structure fixtures:** local HTML pages you write (news article with H2s and bullets, docs page, recipe, table, forum), a tagged and an untagged PDF (`tools/read-aloud/mkpdf.py` makes PDFs), and an OCR image.
- **Objective metrics per fixture:**
  - pause length at each boundary kind vs target, from the synth durations plus inserted silences
  - "fused boundary" count (heading/list/paragraph with < X ms pause)
  - per-chunk loudness spread
  - chunk-join clicks
  - WER (word error rate) on the round trip, with no regression
  - glitch gates, with no regression
  - gapless underruns (0 at 2.2/3/3.5x on an idle machine)
  - first audio ≤ 1.0 s
- **Optional quality score:** a dev-only MOS predictor (an automatic estimate of how natural speech sounds). For example UTMOS: check the licence, run it locally only, never ship it. Use it to compare A/B variants of the pause model and the style-row choice.
- **Listening board:** a "Natural reading" section with before/after for each fixture at 2.2x and 3x (headers + bullets example first), and a short "what changed" line per row.

## Deliverables by morning
1. **Spec addendum** `docs/superpowers/specs/2026-10-04-read-aloud-natural-reading-design.md`, then a plan, then SDD on a branch off `feat/read-aloud` (e.g. `feat/read-aloud-natural`). Reviewed, then merged into `feat/read-aloud`.
2. **Overnight window-test batch** run and recorded. This includes the idle-machine gapless bench, and the live Acrobat/OCR/Edge-follow tests that were pending.
3. **Board** updated, with the "Natural reading" section first.
4. **Session log** `docs/2026-10-04-read-aloud-natural-reading-session-log.md`.
5. **Memory** `project_echo-read-aloud.md` updated.
6. **A morning message to Alex:** TLDR, what to listen to first, before/after numbers, any decisions he needs to make (each with a recommendation), and anything that didn't land.

**Do not merge to `main` or push.** Do not change the device default. Don't open windows after Alex says he's back.
