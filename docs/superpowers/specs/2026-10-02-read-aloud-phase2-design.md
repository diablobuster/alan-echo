# Read Aloud: Phase 2 Design (names + Adobe Acrobat + scans)

Addendum to `2026-10-02-read-aloud-design.md`. Everything in that spec still applies, and so do the plan's Global Constraints, unless this addendum says otherwise.

Branch: `feat/read-aloud-phase2`, cut from `feat/read-aloud` after the live-test fixes (`feat/read-aloud-fixes`) are merged.

## TLDR
- **Names:** unknown words like "Pelosi" are spoken as words instead of spelled out. Lookup order: our hand-fixed list, then CMUdict, then a tiny neural letter-to-sound model (g2p_en, ported to plain Rust). Only real acronyms keep being spelled.
- **Acrobat:** reads through Acrobat's own accessibility interface (`IPDDomNode`), which gives text, word boxes and scrolling. No OCR is needed for normal PDFs.
- **Scans, images, and apps with no text:** Windows' built-in OCR (`Windows.Media.Ocr`) reads the visible part of the window only.
- `read_aloud.rs` barely changes. The new sources live inside the UIA worker, behind the same `pick / more / rects / scroll_to / hover / click` calls.

## 1. Goals and non-goals
**Goals**
- Names and other out-of-dictionary words are pronounced, not spelled. Measure on the 59-name test list (§2.5).
- In Adobe Acrobat / Acrobat Reader, these work like Edge does today:
  - click a word to start
  - highlight the spoken word
  - follow the reading onto the next pages
  - click to jump
  - "Go to word"
- A window with no readable text (a scanned PDF, an image, a canvas app) reads its visible text instead of showing "Nothing readable here".

**Non-goals (this phase)**
- OCR sources scrolling to text that is off screen. The OCR source reads the visible area and then stops, as spec §10 already allows. A toast says "Read to the end of what's on screen".
- Any GPL or share-alike data or code. That rules out espeak-ng and open-english-pronouncing-dictionary (CC-BY-SA), as well as ByT5 weights.
- PDF viewers other than Acrobat that have no TextPattern. They get the OCR source.

## 2. Workstream A: pronouncing unknown words

### 2.1 Where it plugs in
`misaki-rs` 0.6 calls a `Fallback` for a word of 2+ letters that misses its lexicon, special cases, stemming and number rules. `ra_tts.rs` swaps `G2P::new` for `G2P::with_fallback(Language::EnglishUS, Some(Box::new(NameFallback)))`.

The fallback gets the word with its original casing. It must return misaki phoneme symbols with stress marks `ˈ` / `ˌ` placed before the stressed vowel. It may use Kokoro's single-letter diphthongs (`A I O W Y ʤ ʧ`) directly.

**The fallback must never return `Err`.** An `Err` fails the whole chunk's `g2p()` and the chunk goes silent. If every step below fails, it returns letter-by-letter spelling built from the alphabet table, which is today's behaviour.

### 2.2 Lookup order (new module `ra_names.rs`, pure logic + embedded data)
1. **Hand list** (`names_fix.tsv`, ours): famous names that the steps below get wrong. A starting set from the research run includes Buttigieg, Zelensky, Erdogan, Tlaib, Mayorkas, Lukashenko, Youngkin, Milei, Ramaswamy, Navalny, Kishida, Sheinbaum, Kubernetes and Ozempic. Each entry is written directly in misaki symbols.
2. **CMUdict** (BSD-2-Clause; notice kept in `THIRD_PARTY_NOTICES`): about 134k words, embedded compressed (≈0.9 MB gz). It is parsed into a `HashMap` on first use. ARPAbet is mapped to misaki symbols with a fixed 39-entry table (vowels, consonants, stress digits 1 → `ˈ`, 2 → `ˌ`). The table is the one in the research notes and gets a unit test per symbol. The first pronunciation of each word wins.
3. **g2p_en neural letter-to-sound** (Apache-2.0; `checkpoint20.npz` weights ≈3.3 MB): a 2-layer GRU encoder-decoder ported to plain Rust, about 150 lines with no new dependencies. Weights are converted once by a tool script into a compact f32 blob and embedded compressed. Greedy decode with a 30-phoneme cap. Its output is ARPAbet, so it goes through the same mapping table. Budget: under 5 ms per word in a release build.
4. **Spell letters** (last resort, never fails).

**Data load:** CMUdict and the GRU weights are decompressed lazily inside the voice engine's load. They are dropped with the engine on idle unload, so the 0 MB-at-rest rule holds. Load-time and RAM growth get measured (target ≤ +40 MB while loaded, ≤ +150 ms on first chunk).

### 2.3 ALL-CAPS and acronyms
This extends the live-test fix in `ra_text::speakable_caps`, which lowercases ALL-CAPS dictionary words.
- An ALL-CAPS word that misaki doesn't know but CMUdict does is lowercased too, so the fallback can say it (PELOSI → Pelosi).
- An ALL-CAPS word of **≤ 4 letters** that neither knows stays spelled (GDP, CEO, NFL).
- An ALL-CAPS word of **≥ 5 letters** that neither knows gets pronounced by the GRU, since a long unknown caps run is more likely a word or name than an acronym.
- The `KEEP_CAPS` list (US, IT, WHO, AM, PM, …) always wins.

### 2.4 Word timing
The fallback returns one token per word, so `ra_timing::align` is unchanged. A test proves that a chunk containing a fallback word still gives one timing span per word.

### 2.5 Test set and listening check
- `tools/read-aloud/names.txt`: the 59 names from the research plus about 15 common words, as a control.
- An `#[ignore]` test renders `before/` (current build) and `after/` (with fallback) WAVs at 2.2x into the scratchpad, plus a one-page HTML board for listening.
- Alex listens to the A/B. Anything still wrong goes into the hand list.

### 2.6 Unit tests
- The fallback is used only for words the lexicon misses (a lexicon word never reaches it).
- The ARPAbet map covers all 39 symbols and every stress case.
- CMUdict hit; GRU used only on a CMUdict miss.
- Acronym rule cases (§2.3).
- No panic and always `Ok` for emoji, CJK, digits, apostrophes, one-letter words and 60-letter words.
- GRU latency under 5 ms per word (release-mode check in an `#[ignore]` perf test; debug gets a loose bound).

## 3. Workstream B: Acrobat and scans

### 3.1 What the spike measured (2026-10-02, Acrobat Reader 64-bit, on this machine)
- **UIA is useless here:** the page view claims TextPattern, but `DocumentRange` fails and `GetVisibleRanges` is not implemented. There is no ScrollPattern.
- **Acrobat's own interface works.** MSAA `AccessibleObjectFromWindow(AVL_AVView page view)` → `IServiceProvider::QueryService(SID_GetPDDomNode {C0A1D5E9-1142-4cf3-B607-82FC3B96A4DF})` → `IGetPDDomNode` → `IPDDomNode`.
  - **Text:** `GetTextContent` returns the whole document in reading order (732 chars in 0.6 ms).
  - **Word boxes:** Word nodes give `GetLocation` in physical screen pixels (0,0,0,0 when off screen), at 0.2–0.9 ms per word.
  - **Scrolling:** `ScrollTo()` scrolls a node into view in 13–33 ms with no input sent. Page numbers come from `IPDDomNodeExt::GetPageNum`, and pages can be changed with `IPDDomDocument::GoToPage`.
  - Works on both tagged and untagged PDFs. No setup dialog appeared.
- **Gotchas:**
  - A hidden background tab's page view has no DOM, so use the *visible* `AVL_AVView`.
  - The window may answer with a client wrapper (role 10) instead of the document (role 15). QueryService only works on the role-15 document.
  - The document window belongs to the `--type=renderer` child process.
- Spike scripts: session scratchpad `acro-spike/` (`probe.py`, `probe_vis.py`). The interface definitions come from Adobe's public accessibility interface (`AcrobatAccess.idl`, as used by NVDA). We write our own Rust bindings from its GUIDs and method order and do not commit the IDL.

### 3.2 Architecture: sources inside the UIA worker
The worker's `Read` becomes an enum, and `State` keeps one current read:

```
enum Read { Uia(UiaRead), Acro(AcroRead), Ocr(OcrRead) }
```

Each variant implements the same internal operations:

| Operation | What it does |
|---|---|
| `first(point)` | ≤ 200 tokens from the word under the point |
| `more()` | ≤ 400 tokens; empty = end |
| `rects(&[idx])` | `Some(empty)` = off screen, `None` = no answer |
| `scroll_to(idx)` | scroll so that word is on screen |
| `hover(point)` | box of the word under the point |
| `click_ok(point)` | is this a plain-text click (for click-to-jump)? |

`Uia`, `ClickFetcher`, `MoreFetcher`, the channel protocol and `read_aloud.rs` stay as they are. The only controller change: when the OCR source reaches its end, it shows the toast from §1.

**Choosing a source on pick** (the picked window's class/process decides):
1. Window is Acrobat (`AVL_AVView` page view under the point, process `Acrobat.exe` / `AcroRd32.exe`) → try **Acro**. On failure, try **Ocr**.
2. Otherwise **Uia** as today. Where today's code returns `NO_TEXT`, try **Ocr** before giving up.
3. OCR finds no words → `NO_TEXT` → "Nothing readable here", the same as today.

### 3.3 Acro source (`ra_acro.rs`)
- **Bindings:** hand-written `windows::core::interface` declarations for `IGetPDDomNode`, `IPDDomNode`, `IPDDomNodeExt`, `IPDDomWord` (if needed) and `IPDDomDocument`. They are kept to the methods we call, and the vtable order must match the IDL exactly. A test checks the method count against the IDL.
- **Build:** on pick, walk the DOM once in document order and collect **Word** leaf nodes (index = source index). Each node's text comes from `GetTextContent`. The walk is capped (e.g. 20k words) and lazy: `first` walks only until the clicked word plus 200 words, and `more` continues. The clicked word is the Word node whose `GetLocation` contains the point. Only on-screen words have boxes, so that search is cheap.
- **rects:** `GetLocation` on the word node, fresh each time (rule 9).
- **scroll_to:** `ScrollTo()` on the word node. This works with the phase-1 follow-mode rules: Echo scrolls only while follow is ON.
- **hover:** on-screen word under the point.
- **click_ok:** true for any word node. Acrobat links are rare, so there is no link detection this phase.
- **Text normalisation:** word text goes through the same `ra_text::normalize`. PDF line-end hyphens are already handled.

### 3.4 OCR source (`ra_ocr.rs`)
- **Capture:** BitBlt the visible client area of the window from the screen DC (works for GPU-drawn content). Echo's own overlay and pill set `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` so they never appear in the capture.
- **OCR:** `Windows.Media.Ocr`: `OcrEngine::TryCreateFromUserProfileLanguages()` → `SoftwareBitmap::CreateCopyFromBuffer(BGRA)` → `RecognizeAsync().get()`, run on the worker thread (MTA). Lines → words → `BoundingRect` + the capture origin = screen px.
  - If the window is larger than `OcrEngine::MaxImageDimension`, it is cut into tiles.
  - Reading order is the order of the OCR lines (top-to-bottom; columns as Windows OCR orders them).
- **first:** OCR the window (one pass) and start at the word whose box contains the point, or the nearest word on that line.
- **more:** returns the rest of the OCR'd words, then empty (end).
- **rects:** the stored box. If the window moved or scrolled since the OCR (a new 64-px thumbnail checksum of the capture region differs, checked at most every 500 ms), it answers `Some(empty)`. The highlight then disappears instead of drawing in the wrong place.
- **scroll_to:** no-op.
- **Memory:** capture buffers are dropped right after OCR. The OCR engine is created per pick and dropped with the read. No background polling.
- **Latency target:** first audio ≤ 1.5 s after the click on a 1080p window. Measure, and if over target, OCR a band around the click first (±400 px), then the rest.

### 3.5 Hover in pick mode
Hover calls go through the same source switch. For OCR windows the first hover runs the OCR once and caches it until the window moves or scrolls, so hovering doesn't re-OCR on every mouse move. For Acrobat, hover uses the on-screen word boxes.

### 3.6 Tests
**Unit (no apps):**
- source selection by window class
- OCR word ordering and nearest-word-to-click
- tiling maths
- checksum-change invalidation
- Acro DOM walk over a fake tree (a trait over node operations, so it can be unit-tested)
- the vtable method-count check

**Manual `#[ignore]` (each launches and closes only its own windows; never sends input):**
- Acrobat: generate a 3-page text PDF, open it in Acrobat, pick on page 1 (a word box under a known point), read past the page break and confirm `ScrollTo` moved the page, then close only that process tree. If Acrobat is already running with the user's documents, the test skips.
- Scanned PDF: an image-only PDF in Acrobat → OCR source → words found.
- OCR of our own test window that shows a rendered image of text.

**Live with Alex:** Acrobat click, highlight, follow onto page 2, click to jump, Go to word; a scanned PDF; an image in the Photos app.

## 4. Resources (unchanged rules, new numbers to record)
Record each of these in the session log:
- RAM while reading Acrobat
- RAM during and after OCR
- RAM with names data loaded
- RAM after idle unload, which must return to the phase-1 baseline (~20 MB)

## 5. Risks
- **Acrobat may withhold its DOM** unless it thinks a screen reader is running, on other machines or versions. Mitigation: if `QueryService` fails, fall back to OCR (it still works on the visible page).
- **The GRU port misses g2p_en numerically.** Mitigation: a test checks Rust output against Python g2p_en for 30 words, generated once into a fixture.
- **CMUdict pronunciations are flatter** than misaki's (no flap `ɾ`). It is only used for words misaki doesn't know, so common words don't change.
- **OCR mis-orders two-column layouts.** Accepted for phase 2; Acrobat (the main case) uses its own DOM order.
