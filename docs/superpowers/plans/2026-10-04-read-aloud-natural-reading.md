# Plan: Read Aloud natural reading (2026-10-04)

Spec: `docs/superpowers/specs/2026-10-04-read-aloud-natural-reading-design.md` (binding).
Branch: `feat/read-aloud-natural` (worktree `C:\Users\arowm\alan-echo-nat`), base commit
038251b (spec + `ra_struct::Brk` + `Word::brk` + markers stripped in `normalize`).
Each task runs in its own worktree on its own branch off 038251b; the controller merges.

## Global Constraints (every task)
- Rust, `src-tauri/src/`. TDD: write the failing test first, then the code.
- Never panic on Read Aloud paths (`panic = "abort"` in release kills Echo): no
  `unwrap()`/indexing that can fail on page input; use `get`, `checked_*`, `saturating_*`.
- Exactly one timing entry per source word; the highlight never moves backwards.
- 0 MB at rest: nothing loads until Ctrl+Shift+E. Pre-pass ≤ 1 ms per chunk.
- No GPL/LGPL/share-alike code or data shipped (dev-only tools may use them, say so).
- No global keyboard hook; hooks observe-only.
- Don't change the device default (CPU). Model speed cap stays 1.25.
- `cargo build` 0 warnings at the end of the task; `cargo test` all pass (543 + yours).
- Build setup in a fresh worktree: create `<worktree>/dist/` (empty) and
  `<worktree>/src-tauri/resources/models/` (empty) or the build script fails. Use the
  `CARGO_TARGET_DIR` given in your dispatch (a warm target dir; never another task's).
- Never touch `C:\Users\arowm\alan-echo` (Alex's running dev app watches it), never kill
  `alan-echo.exe`, never use port 8178.
- Commit trailer:
  ```
  Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01KN3Tceh8DdXc6dVqVEDh7j
  ```
- Match the surrounding code: doc comment on every fn, terse, plain words.

### Task 1: Raw-text structure fallback + structure-aligned chunking (`ra_text.rs`)
Spec §1 (raw-text fallback) and §3.
- After `normalize` builds words (and only where the source gave no marker, i.e.
  `brk == Paragraph` from a line break, or the first word), classify blocks:
  - Block = words from one break to the next. A block of ≤ 12 words whose last word has
    no terminal punctuation (`.` `!` `?` `:` `;` `,` after closing quotes/brackets), followed by a block
    whose first char is uppercase or a digit → its first word `Brk::Heading(0)` (unless
    it is the very first word of the read with no break before it: then only the
    following word gets `AfterHeading`), and the next block's first word `Brk::AfterHeading`.
    Don't classify a block that is a list item as heading.
  - A block whose first token is a bullet glyph (`•` `◦` `▪` `‣` `●` `○` `■` `▫` `–` `-` `*` `·`)
    or starts with an item number (`1.` `12)` `a)` `b.` `iv.` (roman ≤ 4 chars, lowercase or upper)
    as its own word or glued like "1." ) → `Brk::ListItem`. A lone bullet-glyph word is
    removed from the word list and its `src` indices are prepended to the next word's
    `src` (the glyph is never spoken; the highlight still covers it). A glyph glued to the
    word ("•Apples") is stripped from `text` only. Item numbers stay spoken.
  - A block of a line that ends with ":" followed by list items keeps `Paragraph` before
    the first item → that item becomes `ListItem` (ListItem wins).
  - Source markers (`brk` set by a marker) are never overwritten, except that a
    `Paragraph` marker may be refined to `ListItem` by a bullet glyph.
  - Add a public helper in `ra_text`: `pub fn classify_blocks(words: &mut Vec<Word>, marked: &[bool])`
    or equivalent; normalize must know which words got their brk from a marker (track it).
- `chunk_with`: every word with `brk.is_break()` (not only `para_break_before`) starts a
  new sentence. A short sentence never carries across a structural break, EXCEPT:
  a `ListItem` or `TableCell` block shorter than `min_len` may join the next block when
  the next block's brk is the same kind (`ListItem`→`ListItem`, `TableCell`→`TableCell`
  or `TableRow`); the speak form (Task 2) adds the pause there.
- A heading block (from `Heading(_)` to the word before `AfterHeading`/next break) is never
  cut inside unless it exceeds the hard limit.
- Mid-sentence cuts (no sentence end within the limit): prefer a word ending in `;` `:`
  `—` (em dash) over one ending in `,`; keep all existing exclusions (`date_comma`, digits,
  abbreviations). Existing tests must keep passing; adjust a test only if the spec
  requires the new behaviour, and say so in the report.
- Tests (at least): "In 2024\nKey Takeaways\nThe company grew." → `Key` Heading(0), `The`
  AfterHeading, and chunks never join "2024 Key" or "Takeaways The"; bullet list with
  "•" tokens (src folded, one entry per raw index preserved overall); numbered list
  "1. Apples\n2. Pears"; a sentence that wraps across lines in OCR style ("The cat sat on
  the\n" + "mat.") must NOT become a heading (next block starts lowercase); a
  title-case line followed by lowercase stays Paragraph; chunk tests for list joining
  and the clause-over-comma preference. Pre-pass cost unchanged (normalize is per batch).

### Task 2: Pause model, speak form, exact pauses at chunk edges
Files: new `ra_pause.rs`, `read_aloud.rs` (`Session::job`, `Job`, synth worker),
`ra_tts.rs` (`trim_silence`, `synth_job`/`synth_opts`/`synth_inner_with` signature).
Spec §2.
- `ra_pause::pause_ms(brk: Brk, rate: f32) -> f32`: bases at 1x — Line 250,
  TableCell 250, ListItem 400, TableRow 450, Caption 450, Quote 450, Code 450,
  Paragraph 550, Heading(_) 450, AfterHeading 650, Section 900; `None` → 0.
  Value = base / rate.max(1.0).powf(0.6), floor 150 ms for any non-None brk.
  Constants named, documented.
- `ra_pause::after_ms(last_word: &str, next_brk: Option<Brk>, rate: f32) -> f32`: the
  silence to leave after a chunk. `next_brk` `None` = end of the read's words so far
  (use the sentence pause). If next_brk is a break: `pause_ms(next_brk)`, but never
  less than the punctuation pause. Else punctuation pause = today's rule (sentence
  200 / comma-clause-dash 150 / phrase 60 ms at 1x ÷ rate) — move `Tail`/`tail_of`
  logic here (or call it) so there is one source of truth.
- `ra_pause::speak_form<'a>(word: &'a str, next_brk: Brk) -> Cow<'a, str>`: if next_brk
  is Heading/AfterHeading/ListItem/TableRow/Paragraph/Caption/Quote/Code/Section/Line and the
  word (ignoring closing quotes/brackets) does not end in `.` `!` `?` `:` `;` → append
  "."; if next_brk is TableCell and the word ends in no punctuation → append ",".
  Never touches `Word::text`. Words whose text has no alphanumeric char are left alone.
- `Session::job`: build the job's words with `speak_form(word, next word's brk)` for
  each word except the chunk's last word only when the next chunk exists (then the
  last word also gets speak_form with the next chunk's first word brk — that gives
  headings their falling tone); add `pause_after_ms: f32` to `Job` from `after_ms`
  (next word = first word of the next chunk; `None` when there is no next word yet).
  Keep `Job` constructible in tests.
- Engine: `synth_job(words, rate, pause_after_ms, ctl, gen)` → `trim_silence` keeps
  the lead as today and makes the trailing silence exactly
  `max(0, pause_after_ms − lead_ms(rate))` in total: trim the model's trailing silence
  to that, and pad with zeros when the model left less (only add silence at the very
  end; word times unchanged by the padding). Remove the old `Tail` enum if fully
  replaced. Every existing caller (glitch hunt, benches, tests) updated: they pass the
  old-equivalent value (`after_ms(last_word, None, rate)`) so their outputs don't change.
- Tests: pause table monotonic order at 1x, 2.2x, 3x, 3.5x
  (Line ≤ ListItem ≤ Paragraph ≤ AfterHeading ≤ Section; floor holds); speak_form cases
  ("Takeaways"+Heading→"Takeaways.", "grew."→unchanged, "Price"+TableCell→"Price,",
  `"Hello"` closing quote handled, "—" alone unchanged); trim pads to exact length
  (sample-exact within one 10 ms window) and never cuts speech; `Session::job` gives
  the heading's last word a "." and `pause_after_ms` from the next chunk's brk.

### Task 3: Loudness consistency + edge fades (`ra_level.rs`, new; one call in `ra_tts.rs`)
Spec §4.
- `ra_level::level(pcm: &mut [f32])`: RMS over speech-active 10 ms windows (window
  peak > chunk peak × 0.0316, i.e. −30 dB re peak); gain = TARGET_RMS / rms clamped
  0.5–2.0 (TARGET_RMS: measure the median per-chunk speech RMS of real Kokoro chunks
  from the existing `#[ignore]` real-model tests' approach or pick 0.08 and justify in
  the report); apply gain; soft limiter: samples above 0.9 compressed smoothly so
  |x| ≤ 0.95 (tanh-style knee); 5 ms linear fade-in and fade-out at the edges. No
  allocation beyond the input. Silent/empty input untouched.
- Call it in `synth_inner_with` after `trim_silence` (the padded silence stays zero).
- Tests: two chunks at gains 0.3 and 1.5 of the same signal end within 1 dB RMS of
  each other; limiter bound; fades zero the first/last sample; empty/silent safe;
  cost ≤ 0.2 ms per second of audio in release (`#[ignore]` perf test ok).
- Measurement (`#[ignore]`, real CPU model, release): synth ~30 varied sentences at
  2.2x, report per-chunk speech RMS spread (max−min dB and std) before (no `level`)
  vs after, and the max sample jump at the join of consecutive chunks. Put numbers
  in the report.

### Task 4: UIA structure markers (Edge/Chrome/Word) (`ra_uia.rs`)
Spec §5 UIA. Window tests allowed tonight (Alex asleep) — throwaway Edge only
(`--user-data-dir` in a temp dir), never Alex's Chrome, close what you open.
- Spike first (`#[ignore]` manual test, like `uia_browser_manual` in
  `ra_uia_manual_tests.rs`): write a local HTML fixture (news article: h1, h2, p, ul/li,
  ol/li, table with th/td, blockquote, figure/figcaption, nav menu, footer, an "aside"
  related-links box, a cookie banner div with role=dialog) and open it in throwaway Edge.
  For the token at each block start, read: enclosing element (`RangeFromChild` /
  `GetEnclosingElement`) control type, `AriaRole`, `LocalizedControlType`, heading level
  (`UIA_HeadingLevelPropertyId` if available in the windows crate), `StyleId` attribute
  (`GetAttributeValue(UIA_StyleIdAttributeId)`), landmark type
  (`UIA_LandmarkTypePropertyId`) on ancestors up to the document. Print a table and
  the cost per 200/400-token batch with and without the extra reads. Record in the report.
- Implement in `read_forward` (and the pick's first batch): only at block starts (the
  first token, and a token right after one containing "\r" or "\n"), get the
  structural kind and prefix the token with `ra_struct::mark(brk)`: heading (level),
  list item, table cell / row (first cell of a row = TableRow), caption, block quote,
  code (`pre`). Unknown → no marker (the raw-text fallback in `ra_text` handles it).
  Cache per enclosing element if useful. Budget: ≤ 150 ms extra per 400-token batch in
  Edge on the fixture; if a read is over budget, stop reading attributes for the rest
  of that batch.
- Junk: if a block start's ancestors include landmark navigation/banner/contentinfo/
  complementary, or role dialog with "cookie" text, prefix `ra_struct::mark(...)`?
  No — there is no junk Brk. Instead expose the finding in the report only (Task 7 owns
  junk). Keep it out of this task's code.
- Pure logic (mapping control type / role / style id / heading level → Brk) is a
  separate fn with unit tests. Live test (`#[ignore]`) prints the marked tokens of the
  fixture and asserts headings/list items/cells are marked in Edge.
- Notepad and Word: Notepad has no structure (no markers, no cost); Word only if
  installed (skip otherwise).

### Task 5: Acrobat paragraph breaks + structure types (`ra_acro.rs`)
Spec §5 Acrobat.
- Today Acrobat tokens carry no line breaks at all, so paragraphs and headings fuse.
  In the DOM walk, the last word of each paragraph-level node (any node whose children
  are text/word nodes, or the structure element owning a run of words) ends with "\n"
  (replace its trailing space; keep one token per word, same `src` indexing).
- The first word of a node whose structure type / name is H, H1–H6, LI/LBody, TD/TH, TR
  (first cell), Caption, BlockQuote, Code gets `ra_struct::mark(brk)` prefixed. Find
  how to read the type: `GetType` (CPDDomNode type enum) and `GetName` (tag name) on
  the parent chain; read the IPDDomNode docs in the code comments and the existing
  constants. If the structure name isn't reachable cheaply, do paragraph breaks only
  and report why.
- Unit tests with the existing fake DOM (`F(DOC, ...)` helpers): two paragraphs → the
  first's last word ends "\n"; a heading node → marker on its first word; untagged
  doc → only "\n". Keep the time-sliced `more()` contract and the cost budget.
- `acro_live` (`#[ignore]`) may be run only if Acrobat is NOT already open; otherwise
  skip and say so. Use `tools/read-aloud/mkpdf.py` for a tagged PDF with headings and a list if it supports tags.

### Task 6: OCR headings by line height (`ra_ocr.rs`)
Spec §5 OCR.
- OCR paragraphs already end with "\n". Add: a line whose median word height is
  ≥ 1.3× the page's median word height (over lines with ≥ 3 words) and has ≤ 12 words
  → its first word gets `ra_struct::mark(Brk::Heading(0))` and the line's last word
  ends with "\n"; the next line's first word gets `mark(Brk::AfterHeading)`.
- A line starting with a bullet glyph or item number after a line break: ensure the
  previous line's last word ends "\n" (so `ra_text` sees a break and its list rule
  applies), even without a vertical gap.
- Unit tests with synthetic OCR lines/rects (follow the existing test style near line
  1000). Don't change the stale-check/fingerprint logic.

### Task 7: Page junk, footnote markers, long URLs (`ra_junk.rs` new; `ra_norm_text.rs` url)
Spec §6, §7. Depends on Task 2 for the speak-form wiring; do the pure parts first.
- `ra_junk::is_junk_block(words: &[&str]) -> bool`: exact-match (case-insensitive,
  trailing punctuation ignored) junk phrases: "advertisement", "ad", "sponsored",
  "skip to content", "skip to main content", "share this article", "share this",
  "share", "sign up for our newsletter", "subscribe to our newsletter",
  "related articles", "related", "read more", "recommended", "most popular",
  "you may also like", "continue reading" — and a block made only of social names
  (facebook, twitter, x, linkedin, email, reddit, pinterest, whatsapp, print, copy link) with ≥ 2 of them.
- `ra_junk::strip_footnote(w: &str) -> Cow<str>`: removes bracketed numeric footnote
  markers glued to a word ("evidence.[1]" → "evidence.", "[12]" alone → "") and
  `[citation needed]`.
- Wiring (after Task 2 merged, in `Session::job` via speak form): words of a junk block
  (block = words between `brk.is_break()` boundaries) that is not the block the read
  started in are spoken as nothing — verify how the pipeline handles an empty word
  string (each word must keep exactly one timing entry, zero length); if empty strings
  are unsafe for g2p, drop them from the fed text inside `ra_tts` at the same
  place the existing empty-span words are handled. Footnote markers stripped in the speak form.
- Long URL (`ra_norm_text::url_words` or its caller): a bare URL longer than 40 chars
  reads "link to <host words>" (e.g. "link to example dot com"). Shorter URLs unchanged.
- Tests for each rule, including: a real sentence containing "share" is never junk;
  junk block in the middle of an article is skipped, the first block never.

### Task 8: Natural-reading bench, fixtures, board renderer (`ra_natural_bench.rs`, `tools/read-aloud/natural/`)
Spec §8.
- Fixtures (self-written, committed): `tools/read-aloud/natural/*.txt` in a small
  markup: `# `/`## ` heading lines, `- `/`* `/`• ` bullets, `1. ` numbered, `| a | b |`
  table rows, `> ` block quote, `Caption: ` lines, blank line = paragraph, plain line
  break = line. 8 fixtures: news article with H2s and bullets (the "Key Takeaways"
  example first), docs page, recipe, table (3×4), forum thread, email, PDF-like report
  with figure caption, plain-paragraph control. ~120–250 words each.
- Loader: fixture → raw tokens as a source would give them (word + " ", block ends
  with "\n"). Two modes: `marked` (markers from the markup: Heading(level), ListItem,
  TableCell/TableRow, Quote, Caption) and `plain` (no markers, glyphs kept like a page
  shows them: "•", "1.", cells separated by spaces). Tables: cells with "\n" only at row end.
- Renderer (`#[ignore]`, release, real CPU model): drive the real `Session`
  (`set_rate`, `start`, `jobs`, engine synth, `on_chunk`) and concatenate the pushed
  PCM exactly as the player would get it → `<out>/<fixture>-<rate>.wav` for rates 2.2
  and 3.0. Env: `NB_OUT` (output dir), `NB_MODE` (`marked`|`plain`).
  The code must compile against the current branch API; keep the synth call in ONE
  helper fn so it is a one-line change if Task 2 changes `synth_job`'s signature.
- Metrics per fixture & rate, written to `<out>/metrics.json` and printed: for each
  structural boundary (from the fixture markup, regardless of mode) the silence
  measured at that point in the output (map word → sample via the session timeline;
  silence = run of 10 ms windows under −40 dBFS around the boundary); count of fused
  boundaries (structural boundary with < 150 ms silence); per-chunk speech RMS spread (dB);
  max sample jump at chunk joins; first audio not needed.
- Run it now on this task's base (feat/read-aloud-natural @ 038251b) in `plain` mode → the
  "before" set: `NB_OUT=C:/Users/arowm/alan-echo-nat-bench/before`. Report the
  metrics table. The model files: the real-model tests find them the way
  `ra_gapless_bench.rs` does — copy that approach.

### Task 9: Style-row spike (measurement only) (`ra_tts.rs` test)
Spec §4 style row.
- `#[ignore]` release test: synth the same ~20 sentences, each as its own chunk, at
  2.2x with (a) today's `voice[len(ids)]` row and (b) a fixed row (try 3 fixed rows:
  the median chunk length's row, 100, 200). Per chunk measure spectral centroid
  (simple FFT or zero-crossing proxy is fine — no new deps unless already in Cargo.toml),
  speech RMS, F0 median if cheap (autocorrelation). Report variance across chunks per
  variant, and Whisper WER via our own whisper-server on port 8199 if it's running
  (start it as `tools/read-aloud/glitch-hunt/README.md` / `transcribe.py` describe; never 8178);
  skip WER if it can't run.
- Write WAVs for a listening A/B to `C:/Users/arowm/alan-echo-nat-bench/style/`.
- Recommendation in the report: keep or change, with numbers. Code change only via
  a test-only override hook; do not change product behaviour in this task.
