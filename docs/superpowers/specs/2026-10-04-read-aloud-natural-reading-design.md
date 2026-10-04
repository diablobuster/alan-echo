# Read Aloud: natural reading (spec addendum, 2026-10-04)

Status: self-approved under the autonomous overnight ruling (handoff
`docs/2026-10-04-read-aloud-natural-reading-handoff.md`). Branch
`feat/read-aloud-natural` off `feat/read-aloud`, merged back into `feat/read-aloud`.

## Goal
Listening should feel like a person reading the page: headers, bullets, table cells,
captions and paragraphs never run together ("…in 2024 Key Takeaways The company…"),
pauses fit the structure, and chunks sound like one voice at one loudness.

## Non-goals tonight
- No new settings UI. Every optional behaviour ("first, second…", "quote … end
  quote", table header prefix, code-block summary) stays at its default (off / read)
  and is not built unless cheap.
- No device-default change, no `main`, no push.

## 1. Structure model (`ra_struct.rs`, new)
`Brk`: the structural boundary *before* a word.

| Brk | Meaning |
|---|---|
| `None` | inside running text (punctuation still gives comma/clause/sentence pauses) |
| `Line` | a hard line break inside a block (address, poem, untagged PDF line) |
| `ListItem` | a list item starts |
| `TableCell` / `TableRow` | a new cell / row starts |
| `Paragraph` | a new paragraph |
| `Caption`, `Quote`, `Code` | a caption / block quote / code block starts |
| `Heading(level)` | a heading starts (level 1–6, 0 = unknown) |
| `AfterHeading` | the first word after a heading |
| `Section` | a new section (a heading after a long block, a landmark change) |

- `Word` gains `brk: Brk` (default `None`). `para_break_before` stays (true for every
  `Brk` other than `None`), so existing logic keeps working.
- **In-band markers.** Sources keep the `Vec<String>` token contract. A token may be
  prefixed with `ra_struct::mark(brk)` = `U+E0F0` + one code char. `ra_text::normalize`
  strips the marker and sets `Word::brk`. Ruling: in-band beats changing every
  channel type (pick, click pick, more, fetchers, rollback) for one optional hint.
- **Raw-text fallback** (no marker, any source): at a line/paragraph break,
  - a block of ≤ 12 words with no terminal punctuation, followed by a block starting
    with a capital or digit, is a heading → `Heading(0)` before it and `AfterHeading`
    after it;
  - a block starting with a bullet glyph (`•` `◦` `▪` `‣` `●` `○` `■` `–` `-` `*`) or
    an item number (`1.` `2)` `a)` `iv.`) is a `ListItem`; a lone bullet-glyph token is
    folded into the next word (its `src` kept, its text dropped);
  - otherwise `Paragraph`.
  A marker from the source always wins over the fallback.

## 2. Pause model (`ra_pause.rs`, new)
- `pause_ms(brk, rate)`: structural pauses scale *less* than speed:
  `base_1x / rate^0.6`, floored. Bases at 1x (tuned by listening + metrics):
  Line 250, TableCell 250, ListItem 400, TableRow 450, Caption/Quote/Code 450,
  Paragraph 550, Heading (before) 450, AfterHeading 650, Section 900. Floor 150 ms.
  At 2.2x: Paragraph ≈ 340 ms, AfterHeading ≈ 405 ms (today: ~27–91 ms).
- Punctuation pauses at a chunk edge (no structure) keep today's linear rule
  (sentence 200 / comma 150 / phrase 60 ms at 1x ÷ speed) so they match the model's
  own pauses inside a chunk.
- **Chunk edges:** the engine trims the model's edge silence (as today) and then pads
  the tail with zeros so the silence after the chunk is exactly the boundary's pause
  (minus the next chunk's kept lead-in). The `Job` carries `pause_after_ms`, computed
  by the `Session` from the last word and the next word's `Brk`.
- **Speak form** (`ra_pause::speak_form`, applied in `Session::job`, never to
  `Word::text`): a word followed by a heading/list/row/paragraph/caption boundary that
  has no terminal punctuation gets a "." so the model gives a falling end; a word
  before a `TableCell` gets ",". Headings therefore end with a falling tone even when
  the page has no period. (Final-review ruling: a word the abbreviation table would
  expand once dotted, such as "St", "Dr", "no", gets no "."; the pause alone marks
  the break, so "14 Main St" / "Springfield" never reads "Main saint".)

## 3. Chunking aligned to structure (`ra_text::chunk_with`)
- Every `Brk` except `None` splits sentences (as `para_break_before` does today).
- During the ramp (limit below max), a block shorter than the ramp minimum joins the
  next one across any structural break (heading, paragraph, list item, cell), as
  before this branch, so a read starting on a 2-word heading does not make a 2-word
  first chunk. The exact pause is rendered inside the chunk (Task 10 inner silence)
  and the speak form gives the falling end. After the ramp every break ends a chunk.
  (Final-review ruling; replaces "only list items and cells join".)
- A heading is never cut mid-heading by a sentence end, and a heading carried into a
  ramp chunk is never cut. A heading longer than the chunk limit is still cut at the
  limit (gapless playback wins; final-review note).
- Mid-sentence ramp cuts prefer `;` `:` `—` over `,`, then the existing rules.

## 4. Audio consistency
- **Loudness:** each chunk is gain-normalised on its speech-active 10 ms windows to a
  target RMS (gain clamped 0.5–2.0), then a soft peak limiter (≤ 0.95). Measured: the
  spread of per-chunk speech RMS (dB) before vs after.
- **Joins:** 5 ms fade-in / fade-out at chunk edges (the edges are near-silent after
  the trim; the fades remove residual clicks). Measured: sample jump at joins.
- **Style row:** spike only. Compare `voice[len(ids)]` (today, the Kokoro reference
  behaviour) with a fixed row on per-chunk spectral centroid / RMS variance and
  Whisper WER. Change only on a clear win; otherwise keep and log the ruling.

## 5. Sources
- **UIA (Chrome/Edge/Word):** at each block start only (the token after a line break,
  and the first token), read the enclosing element's control type / `AriaRole` /
  heading level / `StyleId` and emit a marker. Spike first: which attributes Edge,
  Chrome (only if already safe to open a throwaway instance — never Alex's Chrome),
  Word and Notepad return, and the cost per 200/400-token batch. Budget: ≤ 150 ms
  extra per batch in Edge. Junk landmarks (navigation, banner, contentinfo,
  complementary, `aria-hidden`) found on the same walk mark the block as junk
  (see §6) only when cheap.
- **Acrobat:** the DOM walk already visits paragraph-level nodes. The last word of a
  paragraph-like node ends with "\n" (Acrobat tokens have no line breaks today), and
  the node's structure type (H1–H6, LI, TD/TR, Caption, BlockQuote) gives a marker.
  Untagged PDFs fall back to §1's raw-text rules.
- **OCR:** paragraphs already end with "\n". A line whose word height is ≥ 1.3× the
  page's median word height is a heading; the bullet glyph and item-number rules come
  from §1.

## 6. Skip page junk (conservative, forward only)
- Text rules (`ra_junk.rs`, new), applied to whole blocks only: a block that is
  exactly a junk phrase ("Advertisement", "Skip to content", "Share this article",
  "Sign up for our newsletter", "Related articles", "Read more", social-button runs
  like "Facebook Twitter LinkedIn Email") is not spoken. Footnote markers glued to a
  word (`evidence.[1]`, `[12]`) are not spoken.
- A skipped word keeps its one timing entry (zero length); the highlight passes over it.
- Never skips the block the read starts on (Alex clicked it). Only a new read (a pick
  or a click-to-jump) protects a block; skip forward/back, device recovery and a speed
  change step past chunks that are all junk (to the first spoken word, or the end).

## 7. Small naturalness rules
- A bare URL longer than 40 chars reads "link to example dot com" (host only).
- Parentheses and spaced em dashes get a comma-length pause when the model gives none
  (speak form adds "," before "(" / after ")" when no punctuation is there), only if
  measurement shows the model does not already pause.

## 8. Measurement (dev only)
- `ra_natural_bench.rs` (`#[ignore]`): renders structure fixtures through the real
  `Session` + CPU engine (what Alex hears, pauses included) at 2.2x and 3x to WAVs,
  and reports per fixture: pause at each boundary kind vs target, fused boundaries
  (structural boundary with < 150 ms silence), per-chunk loudness spread, join clicks.
- Fixtures (`tools/read-aloud/natural/`, self-written): news article with H2s and
  bullets, docs page, recipe, table, forum thread, email, PDF-like report, plus a
  plain paragraph control. Written as marked text; the "before" render feeds the
  same text without markers through the base branch.
- Gates: glitch gates no worse (p3 2.04 / r2 0.56 / r3 2.58 per 1,000 words), WER on
  the round trip no worse, 0 gapless underruns at 2.2/3/3.5x (idle), first audio
  ≤ 1.0 s, pre-pass ≤ 1 ms per chunk, every word exactly one timing entry.
- Board: a "Natural reading" section first: before/after per fixture at 2.2x and 3x,
  with a "what changed" line.

## Hard rules carried over
No panics on Read Aloud paths; 0 MB at rest; one timing entry per source word, the
highlight never moves backwards; no GPL/LGPL/share-alike shipped; hooks observe-only.
