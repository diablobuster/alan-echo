# 2026-10-04: Read Aloud natural reading: session log

Final head: `feat/read-aloud-natural` @ 11f367f (+ this log).

Branch `feat/read-aloud-natural` (worktree `C:\Users\arowm\alan-echo-nat`), merged into `feat/read-aloud`. Not merged to `main`. Nothing pushed.

- Spec: `docs/superpowers/specs/2026-10-04-read-aloud-natural-reading-design.md`
- Plan: `docs/superpowers/plans/2026-10-04-read-aloud-natural-reading.md`
- Ledger, task reports and reviews (git-ignored): `.superpowers/sdd/2026-10-04-read-aloud-natural-reading/`

## TLDR
- Headings, bullets, table cells and paragraphs no longer run together.
  - Fused boundaries (a structural break with < 150 ms of silence) on 8 self-written fixtures at 2.2x and 3x: **199 of 206 → 6** with page structure, **→ 25 (24 of them plain-text table cells)** from plain text alone.
- Every chunk now plays at the same loudness: the per-chunk spread went from **3.85 dB → 0.38 dB** max, with 0 clicks at joins.
- Page junk ("Advertisement", "Skip to content", social-button rows) and footnote markers like `[1]` are no longer read.
- Glitch gates and Whisper word error rate are unchanged. Idle gapless bench: **0 underruns** at 2.2/3/3.5x.

## 1. What shipped
| Area | What | Files |
|---|---|---|
| Structure model | `Brk` kinds (Line, ListItem, TableCell/Row, Paragraph, Caption, Quote, Code, Heading, AfterHeading, Section), in-band `U+E0F0` markers on raw tokens | `ra_struct.rs`, `ra_text.rs` |
| Raw-text fallback | Headings (short unpunctuated line after a sentence end), bullets and item numbers, soft wraps (→ no pause), short lines (→ Line), blank line = Paragraph | `ra_text.rs` |
| Edge/Chrome/Word | UIA structure walk at block starts (role, heading level, list, table, quote, caption, code; position-checked, ≤ 150 ms per batch). Edge gives no line breaks, so this is the only source of browser paragraph pauses | `ra_uia_struct.rs`, `ra_uia.rs` |
| Acrobat | Paragraph breaks + tagged structure types (`IPDDomElement::GetStdName`), untagged PDFs get breaks from page-text line ends | `ra_acro.rs`, `ra_text::line_breaks` |
| OCR | Headings by line height (≥ 1.3× median), list lines, decided breaks marked | `ra_ocr.rs` |
| Pause model | `base / speed^0.6`, floor 150 ms; exact silence at chunk edges and inside chunks (declicked); punctuation edges unchanged | `ra_pause.rs`, `ra_tts.rs`, `read_aloud.rs` |
| Speak form | Headings and list lines end with "." for a falling tone (never on St/Dr/Mr…); cells with "," | `ra_pause.rs` |
| Chunking | Structural breaks split chunks after the ramp; during the ramp short blocks join (the inner silence keeps the pause) | `ra_text.rs` |
| Loudness | Per-chunk speech-RMS levelling (gain 0.5–2.0), soft limiter, 5 ms edge fades | `ra_level.rs` |
| Junk | Whole-block junk phrases, footnotes, long URL → "link to example dot com" | `ra_junk.rs`, `ra_norm_text.rs`, `read_aloud.rs` |
| Dev tools | Natural-reading bench + 8 fixtures, style-row spike, board section | `ra_natural_bench.rs`, `tools/read-aloud/natural/`, `tools/read-aloud/make_natural_board.py` |

## 2. Verification
- `cargo test`: 664 passed, 0 failed; `cargo build` 0 warnings.
- Static glitch gates p3 / r2 / r3: 2.04 / 0.56 / 2.58, identical (baseline 2.04 / 0.56 / 2.58).
- Clarity WER (100 sentences, at 47ff2bf): 2.2x 6.89% / 3.0x 8.39% / 3.5x 11.23% (round 2: 6.85 / 8.39 / 11.43).
- Idle gapless bench: 0 underruns in 18 runs at 2.2/3/3.5x; first audio 0.70–0.92 s; median RTF 0.54–0.91.
- Every task: implementer → task review → fix rounds → scoped re-review. Whole-branch review (most capable model) → one fix wave → scoped re-review, clean.

### Window tests (run while Alex slept, all windows closed)
| Test | Result |
|---|---|
| follow_reflow_manual | pass (word on screen 95.6%) |
| uia_browser_manual | failed 2/2 at 03:40 under load; **passed 2/2** at 05:30 on the idle merged branch |
| uia_notepad_manual / two_notepads / notepad_bench | pass |
| acro_live, acro_struct_live (tagged + untagged) | pass |
| ocr_live | pass |
| pick_hook_manual / watch_hook_manual | pass |
| uia_struct_live_manual (Edge), notepad, Word | pass |
- The first gapless bench (03:30) showed 3.5x underruns; that was CPU contention from parallel builds. The idle rerun had 0.

## 3. Not done / known limits
- Plain-text tables: cells can't be told apart without page structure (23 of 24 still run together in plain mode; fine in Edge).
- Chromium blockquote detection matches the English label only.
- A few real paragraphs after a bare image or a block link get no browser pause (the safe direction).
- Heading at the very start of a read could, in a rare case next to numbers, be cut by the ramp's last-resort cut.
- The Word live test added `RaStructWord.docx` to Word's recent-files list.
- macOS build gating was checked by reading the code only (no macOS target here).
- Style row: kept as today (no clear win); the A/B is on the board.

## 4. Rulings
See the ledger (`progress.md`); every `Ruling:` line is listed in the morning message.
