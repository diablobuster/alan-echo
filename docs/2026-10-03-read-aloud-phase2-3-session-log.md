# 2026-10-02/03: Read Aloud live-test fixes, phase 2, optimisation, phase 3: session log

Branch: `feat/read-aloud`. Phase 1 plus everything below is merged into it. Nothing is merged to `main` and nothing is pushed.

Specs:
- `docs/superpowers/specs/2026-10-02-read-aloud-phase2-design.md`
- `docs/superpowers/specs/2026-10-03-read-aloud-phase3-design.md`

Plans: the matching files in `docs/superpowers/plans/`.

## TLDR
- **Live-test fixes (Chrome):**
  - ALL-CAPS words read as words.
  - No pause inside "mid-year".
  - Free scrolling plus a "Go to word" button.
  - A plain click jumps reading to that spot.
- **Phase 2:**
  - Unknown names are pronounced. The lookup order is a hand list, then CMUdict, then a g2p_en GRU port.
  - Adobe Acrobat works through its own `IPDDomNode` accessibility API: text, word boxes and ScrollTo, with no OCR.
  - Scans and images are read with Windows OCR (visible area only).
- **Optimisation:**
  - CPU per second of speech: 5.4 s → 2.8 s (34% → 17% of the machine).
  - Acrobat hover p50: 17 → 4 ms.
  - OCR re-scans: 1.7 → 0.25 per second.
- **Phase 3:**
  - A text-normalisation pre-pass reads numbers, money, dates, times, ordinals, abbreviations, initialisms, URLs, symbols, units and Roman numerals.
  - Acronym, CamelCase and brand rules, plus a homograph table.
  - Glitches per 1,000 words, measured by the TTS→Whisper glitch hunt:
    - static: 36.2 → 2.1
    - audible random controls: about 32 → about 3 real (scorer: 9.2)

## 1. What shipped
| Area | What |
|---|---|
| Live-test fixes | `ra_text::speakable_caps` (ALL-CAPS dictionary words); intra-word hyphen pause dropped; follow mode (manual scroll turns follow off, "Go to word" pill button turns it back on); observe-only mouse watch hook gives click-to-jump (links and buttons are left alone; double-click and drag excluded); non-committing click lookups |
| Phase 2 names | `ra_names.rs` (hand list, CMUdict BSD-2, ARPAbet→misaki), `ra_g2pen.rs` (g2p_en Apache-2.0 GRU in plain Rust, 1.1 ms/word), misaki `[w](/ph/)` hand overrides |
| Phase 2 Acrobat | `ra_acro.rs`: hand-written COM bindings (vtable checked against the IDL), lazy DOM walk, time-sliced `more()`, page-view-only detection, hover box cache |
| Phase 2 OCR | `ra_ocr.rs`: BitBlt capture, `Windows.Media.Ocr`, tiling for large windows, stale-check fingerprint on the words' rows, hover back-off with settle detection; overlay and pill excluded from capture |
| Source switch | The `ra_uia.rs` `Read` enum: Uia / Acro / Ocr. Chromium never falls back to OCR. Neutral "end of what's on screen" notice. |
| Optimisation | ORT: no idle spinning, per-shape memory plan off (CPU), benches in `ra_tts_bench.rs` / `ra_perf.rs` (test-only) |
| Phase 3 | `ra_numwords.rs`, `ra_norm_num.rs`, `ra_norm_text.rs`, `ra_norm.rs`, `ra_homograph.rs`; extended `ra_names.rs` (acronyms, CamelCase, compound split, Cyrillic/Greek, never-empty), `names_fix.tsv` (+67 brands); `ra_text` glued-token merge; `ra_tts::g2p_words` spans → overrides, context-aware caps rule (headline vs emphasis vs acronym) |
| Glitch hunt | `tools/read-aloud/glitch-hunt/` (static + Kokoro→Whisper round trip, own whisper-server on :8199), `make_glitch_board.py`, self-written `stress.txt` (Wikipedia corpus is local only, CC BY-SA) |

## 2. Verification
- `cargo test` on `feat/read-aloud`: 454 passed, 0 failed, 41 ignored (3 runs in a row). The debug wall-clock test `caps_rewrite_is_cheap` moved to a release `#[ignore]` perf test (f5f2c8c): 0.24 ms per chunk.
- `cargo build`: 0 warnings. `npm run build`: OK.
- Manual tests, run live:
  - Notepad UIA (×2), `acro_live` (Acrobat Reader: pick, more to page 3, ScrollTo), `ocr_live` (100% of words, pick ~160 ms).
  - Release benches as listed in the task reports.
- Every task passed a spec+quality review. Each phase passed a whole-branch review, and every Critical or Important finding was fixed and re-reviewed. The ledgers live in `.superpowers/sdd/*/progress.md` (git-ignored).
- **Alex live:**
  - Chrome before the fixes: found the 4 issues above. Edge: works.
  - Phase 3 board listening: "most of this is really good", "(59,000 km2)… saying this properly". He found the "ISS" → "iss" acronym issue, which round 1 fixed.

## 3. Not yet done / Alex's call
- **Live tests still to do:** Acrobat (click, highlight, follow to page 2, click-to-jump, Go to word), a scanned PDF, the Photos app, the Edge PDF, Word, and a retest of the Chrome fixes.
- **Decisions for Alex:**
  - "auto" device: CPU (current; 0 MB after idle) or GPU (faster, but keeps about 300 MB).
  - A shorter second chunk to cut the ~0.6 s gap after the first sentence (adds one extra break).
- **Known limits:**
  - Rare unknown names still go to the GRU.
  - Homographs are about 94% right.
  - OCR reads the visible area only.
  - Kokoro adds its own small pause after some number words.
  - Other scripts (CJK and others) are silent.
- **Next:** improvement round 2 (headline threshold to 80%, ambiguous acronym words, "Columbus, OH", fractions like 13/16, UK dates) and further corpus hunts.
- **Housekeeping:** many worktrees under `C:\Users\arowm\alan-echo-*` and `.claude/worktrees` can be removed once Alex is happy.
