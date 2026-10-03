# Read Aloud Phase 3 (speaking real-world text) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Read numbers, money, dates, abbreviations, URLs, symbols, acronyms, CamelCase brands and homographs correctly, so that Read Aloud never spells a real word, never reads an acronym as a word and never goes silent.

**Architecture:** A pure normalisation pre-pass (`ra_norm*`, `ra_numwords`) turns runs of source words into spoken text. `ra_tts::g2p_words` phonemises that text with the same G2P and feeds misaki a `[word](/phonemes/)` override. That gives one highlight per source word, and words inside a span get zero length. `ra_names` gains acronym, CamelCase and accent-fold rules. A homograph table picks readings using misaki's POS tags.

**Tech Stack:** Rust (Tauri v2) and misaki-rs 0.6 (no espeak). There are no new crates. Tool scripts are Python 3 and are used locally only.

**Spec:** `docs/superpowers/specs/2026-10-03-read-aloud-phase3-design.md`. Section 3 is the **reading-rules contract**: every row is a required test case. Parent specs: phase 1 and phase 2 (same folder).

**Evidence / harness (spike):**
- Report: `C:\Users\arowm\AppData\Local\Temp\claude\C--Users-arowm\73bf4adb-ff19-45f8-84e8-dda9f5f4c1b4\scratchpad\p3\baseline-report.md`
- Harness: `...\scratchpad\p3\run_hunt.sh`, plus `C:\Users\arowm\alan-echo-p3-spike\src-tauri\src\ra_glitch_hunt.rs` (uncommitted)
- GRU-routed word list: `...\scratchpad\p3\static_summary.json`

## Global Constraints
- **Windows only:** every new module gets `#[cfg(target_os = "windows")]` on its `mod` line in `main.rs`. macOS `cargo test` must still pass.
- **0 MB at rest:** tables are `static`. Nothing loads before Ctrl+Shift+E.
- **Never panic on any Read Aloud path:** no `unwrap`/`expect`/unchecked indexing or byte slicing on user text outside tests. Slice only at `char_indices` boundaries or ASCII positions you have checked.
- **The misaki `Fallback::phonemize` must always return `Ok`** and must never return an empty string for a token that contains a letter, digit or currency/math symbol.
- **No GPL, LGPL or share-alike code or data shipped.** Number words are our own code (no num2words). The Wikipedia corpus is never committed.
- **One timing entry per source word.** The highlight never moves backwards.
- **Pre-pass budget:** ≤ 1 ms per 30-word chunk in a release build.
- **Never send input to the user's apps.** Never kill `alan-echo.exe`.
- **Commits:** prefix `feat(read-aloud):` / `test(read-aloud):` / `chore(read-aloud):`. Every message ends with:
  ```
  Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01KN3Tceh8DdXc6dVqVEDh7j
  ```
- **Build checks:** `cargo test` (in `src-tauri`) all pass with ignored tests still ignored; `cargo build` 0 warnings.
- **Worktrees** need the git-ignored `src-tauri/resources/models/` and `dist/index.html` placeholders.

## Review Focus
1. **A year vs a quantity.** "In 1500 the city…" should read as a year; "1500 workers" should read as a cardinal. Pinned in Task 1 (both readings).
2. **Raw UIA token splits of money and decimals** (`$`,`3`,`.`,`2B`) must merge into one word before the pre-pass. Pinned in Task 5 (fixture of split tokens).
3. **A multi-word span** (e.g. `$5 billion`, `World War II`, `St. Louis`) must keep one timing entry per word, give zero length inside the span and never move backwards. Pinned in Task 5.
4. **Sentence-final abbreviations and lone symbols** ("…in the U.S." at a sentence end, a lone `$`, a lone `.`) must never produce silence and never swallow the sentence pause. Pinned in Tasks 2 and 5.
5. **CamelCase must not split real dictionary words** (McDonald, iPhone, YouTube) when the hand list or CMU already knows them. Pinned in Task 3.

---

## File map
| File | Task | Responsibility |
|---|---|---|
| `src-tauri/src/ra_numwords.rs` | 1 | `cardinal(u64)`, `ordinal(u64)`, `year(u32)`, `digits(&str)`, `decimal(&str)`: words, hyphen-free |
| `src-tauri/src/ra_norm_num.rs` | 1 | `num_spans(&[&str]) -> Vec<(usize, usize, String)>`: number-family spans |
| `src-tauri/src/ra_norm_text.rs` | 2 | `text_spans(&[&str]) -> Vec<(usize, usize, String)>`: abbreviations, initialisms, URLs/emails/handles/hashtags, symbols, units, Roman numerals, ellipses |
| `src-tauri/src/ra_norm.rs` | 2 | `pub struct Span {start,end,spoken}`, `spans(&[&str]) -> Vec<Span>` (merge, numbers win), `prepare(&str) -> Cow<str>` (apostrophes, NB hyphen), `fold(char) -> Option<&'static str>` |
| `src-tauri/src/ra_homograph.rs` | 4 | `reading(word, tag, prev, next) -> Option<&'static str>` (misaki phonemes) |
| `src-tauri/src/ra_names.rs` | 3 | Acronym stems, CamelCase split, full fold, never-empty |
| `src-tauri/data/names_fix.tsv` | 3 | Brand list |
| `src-tauri/src/ra_text.rs` | 5 | Merge glued tokens into written words |
| `src-tauri/src/ra_tts.rs` | 5 | Spans → overrides; collapse in-token pauses; homograph apply |
| `src-tauri/src/ra_glitch_hunt.rs`, `tools/read-aloud/glitch-hunt/*` | 5 | Committed harness (static `#[ignore]` test + scripts + self-written stress sentences) |

**Waves:**
- **Wave A:** Tasks 1, 2 and 4, in parallel worktrees off `feat/read-aloud-phase3` at `a62584a`. They only add new files. Task 2's `ra_norm.rs` merges Task 1's output by the tuple type, so it doesn't compile-depend on Task 1. Its tests use a local stub, and the controller wires the real call at merge.
- **Wave B:** Task 3, after phase 2 Task 8 is merged into phase 3, because it touches `ra_names.rs`.
- **Wave C:** Task 5, after Tasks 1–4.
- **Then:** Task 6 (controller): run the gate, then fix rounds by category.

---

### Task 1: Number words + number spans

**Files:**
- Create: `src-tauri/src/ra_numwords.rs`, `src-tauri/src/ra_norm_num.rs`
- Modify: `src-tauri/src/main.rs` (two cfg-gated `mod` lines)

**Interfaces:**
- Produces:
  - `ra_numwords::{cardinal(n: u64) -> String, ordinal(n: u64) -> String, year(y: u32) -> String, digits(s: &str) -> String, decimal(int_part: &str, frac: &str) -> String}`
  - `ra_norm_num::num_spans(words: &[&str]) -> Vec<(usize, usize, String)>`, where `start..end` is a source-word index range (end exclusive) and the `String` is the spoken text. Spans don't overlap and are sorted.
- **Word style:** lowercase, words separated by single spaces, **no hyphens** ("twenty five", not "twenty-five"), no "and" ("one hundred five"). The letters of a spoken acronym are uppercase and space-separated ("P M", "U S").

- [ ] **Step 1: Write the failing tests.** Every number-family row of spec §3 is one `#[test]` assertion: years, decades, decimals, cardinals with commas, currency (all six examples), percent, ordinals, times, dates (all three), ranges/scores. Plus:
  - "In 1500 the city fell" → year; "1500 workers" → cardinal (Review Focus 1)
  - "since 1990" → year; "page 2025 of" → cardinal
  - `-5` / `−5` → "minus five"
  - `1/2` → "one half"; `3/4` → "three quarters"
  - `v2.1` → "version two point one"
  - phone `555-123-4567` → "five five five, one two three, four five six seven"
  - a 60-digit number → digits read one by one (no overflow)
  - `$` alone → no span (left to Task 2's symbols)
  - `1e10` → "one E ten"
  - every result is non-empty for any token containing a digit
  - no panic on emoji or CJK neighbours

  The suffix `B` after a currency amount means billion only directly after a currency amount or with the `$`. A plain `3B` (a vitamin, a pencil) reads "three B".
- [ ] **Step 2:** Run `cargo test ra_norm_num ra_numwords`. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - **Number words:** `cardinal` via a ones/teens/tens table and scale words up to 10^18 (above that, fall back to `digits`).
  - **Years:** 2000–2009 → "two thousand X"; 2010–2099 → "twenty ten" … "twenty ninety-nine" (hyphen-free); 1100–1999 → two two-digit halves; `X00` → "nineteen hundred".
  - **Ordinals:** irregular table (first, second, third, fifth, eighth, ninth, twelfth, and the `-ty` → `-tieth` rule).
  - **`num_spans`:** a single left-to-right scan with a small classifier per word (ASCII checks only, no regex crate). Context looks at `words[i-1]` and `words[i+1]` for months, "in/since/by/from/until", unit and count nouns, and currency scale words ("million", "billion", "bn"). Month names come from a static table.
- [ ] **Step 4:** `cargo test` (all) and `cargo build` (0 warnings). Add an `#[ignore]` perf test: 10k chunk-sized calls in release, average < 0.5 ms per call.
- [ ] **Step 5: Commit.** `feat(read-aloud): number normaliser (years, money, decimals, ordinals, dates, times)`

---

### Task 2: Text spans + the `ra_norm` merger

**Files:**
- Create: `src-tauri/src/ra_norm_text.rs`, `src-tauri/src/ra_norm.rs`
- Modify: `main.rs` (two cfg-gated `mod` lines)

**Interfaces:**
- Produces:
  - `ra_norm_text::text_spans(words: &[&str]) -> Vec<(usize, usize, String)>`, with the same conventions as Task 1.
  - `ra_norm::Span { pub start: usize, pub end: usize, pub spoken: String }`
  - `ra_norm::spans(words: &[&str]) -> Vec<Span>`: calls `crate::ra_norm_num::num_spans` and `text_spans`, merges, and the number span wins on overlap. Until Task 1 is merged, keep that call behind a `fn num(words) -> Vec<(usize,usize,String)>`; the controller swaps the real call in at merge. Write it as `crate::ra_norm_num::num_spans(words)` and add a stub `ra_norm_num.rs` that returns `vec![]` **only in this worktree, uncommitted**. The controller merges Task 1's real file.
  - `ra_norm::prepare(word: &str) -> Cow<str>`: maps ’ ‘ ʼ to ', and U+2010/U+2011 to -.
  - `ra_norm::fold(c: char) -> Option<&'static str>`: full Latin accent fold (à–ÿ, ğ ă ș ț ł ő ű ø æ œ ß đ þ ñ ç).
- **Spoken style:** as in Task 1. Letters to be spelled are uppercase and space-separated.

- [ ] **Step 1: Write the failing tests.** Every text row of spec §3 is one assertion: initialisms, abbreviations including `St. Louis` / `Main St.`, the domain, the URL, the email, the handle, the hashtag, Roman numerals, units, math, curly apostrophe through `prepare`. Plus:
  - "…moved to the U.S." at a sentence end: the span keeps a trailing "." in `spoken`, so the sentence pause survives (Review Focus 4)
  - "Sen." at a sentence end with no capital after it: no expansion
  - `No. 5` → "number five"; "said no." → untouched
  - `Dr. Smith` → "Doctor Smith"; `Elm Dr.` → "Elm Drive"
  - `I` (pronoun) is never a Roman numeral; `MIX` and `CIVIL` are never Roman numerals
  - a glued `word...word` splits with a pause
  - `and/or` → "and or"; `P/E` → "P E"; `km/h` → "kilometers per hour"
  - `&` → "and"; `+` → "plus"; `%` alone → "percent"
  - a lone `$` → "dollars"; a lone `.` → no span
  - emoji / CJK → no span and no panic
  - `prepare` and `fold` tables tested by sampling
- [ ] **Step 2:** Run the tests. Expected: FAIL.
- [ ] **Step 3: Implement** with static sorted tables:
  - abbreviations (~80, with each one's context rule as an enum)
  - units (~60, applied only when the previous word is a number or a number-ending token)
  - symbols
  - Roman numerals: parse I–MMMCMXCIX, but expand only after a trigger word (War, Henry, Louis, Elizabeth, Charles, George, Edward, Richard, John, Paul, Benedict, Pius, Leo, Chapter, Part, Act, Volume, Vol., Super Bowl, Pope, King, Queen, Title, Phase, Stage, Type, Class), or in `XIV` form ≥ 4 letters after a capitalised proper noun
  - "Henry VIII" → "Henry the eighth" (regnal) vs "Chapter IV" → "Chapter four" (cardinal)
  - URL/email/handle/hashtag tokenisation by hand (split on `.` `/` `@` `#` `-` `_`, plus a CamelCase split for hashtags and handles)
- [ ] **Step 4:** `cargo test` and `cargo build` (0 warnings). Perf `#[ignore]` < 0.5 ms per chunk.
- [ ] **Step 5: Commit** (without the stub). `feat(read-aloud): text normaliser (abbreviations, initialisms, URLs, symbols, units, Roman numerals)`

---

### Task 3: Acronyms, CamelCase, fold, never-empty, brand list (Wave B)

**Files:**
- Modify: `src-tauri/src/ra_names.rs`, `src-tauri/src/ra_text.rs` (only `speakable_caps`/`caps_part` if the acronym rule lives there), `src-tauri/data/names_fix.tsv`
- Create: `tools/read-aloud/brand_candidates.py` (helper that turns the spike's GRU list into a review TSV)

**Interfaces:**
- Consumes: `ra_norm::fold` (Task 2). If it's not merged yet, add a local fold and the controller dedupes at merge. Also consumes the existing `Names`, `NameFallback`, `spell`, `arpa_to_misaki` and `lts_key`.
- Produces: unchanged public API. `NameFallback::phonemize` behaviour changes as follows.

- [ ] **Step 1: Write the failing tests.**
  - **Acronyms:** `SEC's`, `AWS`, `GPUs`, `ETFs`, `HTTPS`, `FTX's`, `SVB's`, `QQQ`, `NVMe`, `PCIe`, `IoT`, `FYI` → spelled letters with a plural/possessive suffix (`ˈɛs` letters + `z` / `ɪz`). `NASA`, `NATO`, `COVID`, `AIDS` → words, as today.
  - **No vowel:** any no-vowel token (`nm`, `kb`, `QQQ`) never reaches the GRU (use a fake `Lts` that panics if called).
  - **CamelCase:** `TechReview`, `LinkedIn`, `PayPal`, `OpenAI`, `JPMorgan`, `PostgreSQL`, `iHeartMedia`, `UnitedHealth`, `DocuSign`, `DeepSeek`, `USB4` → the parts' phonemes joined. `McDonald`, `iPhone`, `YouTube` → hand or CMU entry, **not split** (Review Focus 5).
  - **Fold:** `Erdoğan`, `Călin`, `Łukasz` → folded before CMU and the GRU.
  - **Never empty:** `phonemize` is never empty for `"3.2"`, `"$"`, `"€"`, `"2B"`, `"μ"`.
  - **Microsoft:** reads as CMU "mˈIkɹəsˌɔft"-like, not spelled. This is the regression test from Alex's report.
- [ ] **Step 2: Brand list.**
  1. Run the helper over `static_summary.json`'s GRU list plus a curated list of about 200 tech/finance/news brands. It writes `tools/read-aloud/brand_review.tsv` with: word, CamelCase+CMU result, GRU result.
  2. For each brand where both are wrong, hand-write the misaki phonemes into `names_fix.tsv`, in the same symbol style as the existing entries.
  3. Target: the top 300 by corpus frequency are covered. Record the count in the report.
- [ ] **Step 3: Implement.**
  - **Acronym stem rule:** in `NameFallback::phonemize`, before `lookup`. Also in `speakable_caps`, so that misaki's gold "sec" word reading is bypassed. Use a `SPELLED` list for caps words misaki knows as words but that are acronyms in practice: SEC, AWS, ESG, GAAP, IPO, ETF, GPU, CPU, API, ICE, DOGE?
    - Decide DOGE (and similar) case by case and list every decision in the report.
  - **CamelCase split:** on lower→Upper, Upper→Upper+lower (`JPMorgan` → `JP|Morgan`) and letter↔digit boundaries. Each part goes: hand → misaki lexicon (via a lookup closure passed in, the same `lexicon_knows` + G2P) → CMU → caps ≤ 4 spelled → GRU. Join the parts with no space; one stress per part is fine.
  - **Never-empty guard:** digits are read via `ra_numwords::digits` when Task 1 is merged. Otherwise use a local digit table and the controller dedupes.
- [ ] **Step 4:** `cargo test` and `cargo build`. Run the static glitch hunt once, as in the spike's `GH_STATIC_ONLY=1 bash run_hunt.sh` with `GH_SRC` pointing at your worktree; copy `ra_glitch_hunt.rs` and its 3-line hook in **uncommitted**. Record the before/after rates for the GRU, acronym and CamelCase categories.
- [ ] **Step 5: Commit.** `feat(read-aloud): acronym, CamelCase and accent rules; brand pronunciations`

---

### Task 4: Homograph table (`ra_homograph.rs`)

**Files:**
- Create: `src-tauri/src/ra_homograph.rs`
- Modify: `main.rs` (cfg-gated `mod`)

**Interfaces:**
- Produces: `ra_homograph::reading(word_lower: &str, tag: &str, prev: Option<&str>, next: Option<&str>) -> Option<&'static str>`, which returns misaki phonemes for the chosen reading, or `None` to leave misaki's default. `tag` is misaki's Penn tag (`VBD`, `VBN`, `NN`, `JJ`, …), the same tags that the `MToken.tag` values in misaki-rs output use.
- Task 5 calls it per token after G2P and replaces that token's phonemes.

- [ ] **Step 1: Write the failing tests.** A table of 50 targeted sentences, written as `(sentence, word index, expected phonemes)`. Run them through the real misaki G2P to get tags (use the existing test G2P helper in `ra_tts` tests, or construct `G2P::new`), then `reading`. Cover every word in spec §2's list, both readings each where the language has two. Target: ≥ 85% pass, asserted as a count, with the failures printed.
- [ ] **Step 2:** Run. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - A static table: word → (reading A phonemes, reading B phonemes, rule).
  - Rules: POS-based (VBD/VBN vs VB/VBP/VBZ/NN/JJ), plus context overrides: `to`/`will`/`can`/`must` + read → reed; `have`/`has`/`had` + read → red; `the lead` → leed (noun, as in "take the lead"); `lead` + `pipe/paint/poisoning` → led; `live` + `music/show/stream/event` → laiv; noun/verb stress pairs (REcord / reCORD) by tag.
  - Phonemes use misaki symbols, consistent with misaki gold entries. Copy the existing reading from misaki's lexicon for the default reading.
- [ ] **Step 4:** `cargo test` and `cargo build`.
- [ ] **Step 5: Commit.** `feat(read-aloud): homograph readings by part of speech and context`

---

### Task 5: Integration + committed glitch hunt (Wave C)

**Files:**
- Modify: `src-tauri/src/ra_text.rs` (merge glued tokens), `src-tauri/src/ra_tts.rs` (`g2p_words`, `token_phonemes`, homograph apply, `synth_inner` wiring)
- Create: `src-tauri/src/ra_glitch_hunt.rs` (from the spike, cleaned up and made a sibling `#[cfg(test)]` module with the same hook), `tools/read-aloud/glitch-hunt/` (the spike's scripts, plus `stress.txt` with only self-written sentences, plus README). Add a `.gitignore` entry for `tools/read-aloud/glitch-hunt/cache/`, `corpus.tsv`, `wav/` and outputs.

**Interfaces:**
- Consumes: `ra_norm::{spans, prepare, Span}`, `ra_homograph::reading`, and Task 3's `NameFallback` behaviour.
- Produces: unchanged `Engine::synth` contract. `ChunkAudio.words` has exactly one entry per input word.

- [ ] **Step 1: Write the failing tests.**
  - **(Review Focus 2)** `ra_text::normalize` on raw tokens `["it ", "cost ", "$", "3", ".", "2B", " in ", "Q3", "."]`. Mirror the existing raw-token test style, with glued = no trailing whitespace. Expect words `["it","cost","$3.2B","in","Q3."]`. Also `["8", ".", "6", "% "]` → `"8.6%"`, `["7", ":", "30", "pm "]` → `"7:30pm"`, and `["U", ".", "S", ". "]` → `"U.S."`.
    - Tokens with a space between them are never merged.
    - The existing hyphen tests must stay green.
  - **(Review Focus 3)** `synth_words` timing on "It cost $5 billion in World War II." with the real G2P and a fake duration model. Use the existing `word_times` path: run the duration/time functions with synthetic durations, as the existing timing tests do. Assert:
    - there are 8 entries;
    - the "billion" and "II" entries have zero length at the span end;
    - times are non-decreasing.
  - `token_phonemes` collapses whitespace from empty subtokens owned by the same word. "cost [$3.2B](/…/) today" has no run of spaces longer than 1 between "dollars" and "today".
  - The homograph is applied: "I have read it" contains the red phonemes.
  - **(Review Focus 4)** "…in the U.S." at the end of a chunk keeps the final "." token, so the sentence-end prosody survives.
  - **Perf `#[ignore]`:** `g2p_words` + spans on 30-word chunks of corpus-like text adds ≤ 1 ms per chunk in release.
- [ ] **Step 2:** Run. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - **In `g2p_words`:** apply `prepare` per word, then `spans`. For each span:
    - phonemise `spoken` with the same G2P, at recursion depth 1, so the pre-pass doesn't run again;
    - build the override for word `start`;
    - make the words `start+1..end` overrides with **no phonemes** (or skip them and give alignment zero-length entries; choose the approach that keeps `align` simple and document it).
  - Keep the existing hand override, caps and hyphen logic for words outside spans.
  - Apply homographs after `g2p()` per token, by owner word.
  - Port the glitch hunt and make it pass against this branch.
- [ ] **Step 4:** Run `cargo test`, `cargo build` (0 warnings) and `npm run build`. Run the static hunt and record the rate. Target ≤ 5 per 1,000. If it's above that, list the top remaining categories in the report. Don't widen scope; Task 6 handles it.
- [ ] **Step 5: Commit.** `feat(read-aloud): text normalisation pre-pass wired into synthesis; glitch hunt harness`

---

### Task 6: Gate, fix rounds, listening board (controller)
- [ ] Merge everything into `feat/read-aloud-phase3`. Run the full checks, then the full glitch hunt: static plus round trip, about 35 min.
- [ ] While the gate fails (static > 5 per 1,000, round-trip controls > 8 per 1,000, or any static category > 1 per 1,000): dispatch ONE fix agent per round with the top categories and their examples, re-run the static gate after each round, and run the full gate at the end. At most 3 rounds, then adjudicate.
- [ ] Final whole-branch review on the most capable model. One fix wave, then a scoped re-review.
- [ ] Generate `tools/read-aloud/glitch_board.html` (residual glitches with audio, plus 20 random good sentences). Record the before/after rates in the session log, and update the memory.
