# Read Aloud glitch hunt

Measures how often Read Aloud says a word wrong, on real prose plus our own stress
sentences. Phase 3 spec §6.

## Run it

```bash
# static pass only (seconds, no audio): the gate after every task
GH_STATIC_ONLY=1 bash tools/read-aloud/glitch-hunt/run_hunt.sh

# full run: static + real CPU voice model + Whisper round trip (minutes)
bash tools/read-aloud/glitch-hunt/run_hunt.sh

# freeze this run as the baseline the next runs compare with
bash tools/read-aloud/glitch-hunt/run_hunt.sh --set-baseline
```

Use `GH_CORPUS=<path to a frozen corpus.tsv>` to rerun on the exact corpus of an
earlier run. Without it, `build_corpus.py` builds `corpus.tsv` once (fetches are
cached in `cache/`, so later builds are offline and identical).

## What each piece does

| File | Job |
|---|---|
| `src-tauri/src/ra_glitch_hunt.rs` | `#[ignore]` tests that drive the real private pipeline: `glitch_static` (normalize, chunk, pre-pass, misaki, align; writes `static.jsonl`) and `glitch_synth` (real CPU model; writes `wav/`) |
| `build_corpus.py` | Fetches Wikipedia / Wikinews / Gutenberg prose into `cache/` and adds `stress.txt` |
| `stress.txt` | Our own stress sentences (money, numbers, acronyms, brands, homographs, punctuation). The only corpus text that is committed |
| `static_flags.py` | Flags glitches in `static.jsonl` (spelled words, silent words, years read as cardinals, pauses inside words, ...) |
| `categorize.py` | Surface category of a written word (shared) |
| `make_sample.py` | Picks the round-trip sample (seeded) |
| `transcribe.py` | Sends the WAVs to our own whisper-server on port 8199 (never 8178, the app's) |
| `speechnorm.py` | Turns reference text into spoken words for the Whisper comparison |
| `roundtrip.py` | Aligns reference vs Whisper transcript and scores mismatches |
| `cluster.py` | Groups glitches by cause, writes `current.json`, compares with `baseline.json` |

## Round 2 corpus and checks

- `build_round2.py` builds `corpus_round2.tsv`: public-domain US-government text
  (Federal Reserve, BEA, NASA, White House, Federal Register abstracts; SEC, BLS and
  Treasury pages refuse unnamed tools and are skipped) fetched into `cache_r2/`, plus
  our own 700 lines from `stress_round2.txt` (`gen_stress_round2.py` writes them:
  finance, earnings calls, tech, crypto, politics, sports, weather, recipes). Run the
  hunt on it with `GH_CORPUS=<frozen copy of corpus_round2.tsv>`.
- `static_flags.py` also flags `SHORTHAND_UNEXPANDED` (bps, Y/Y, FY25, 1H24,
  NASDAQ:AAPL, (D-Mass.), 5.1M, W 3-1, ...) and `FALSE_SENTENCE_BREAK` (a chunk cut
  right after an abbreviation's period: `tbsp. | kosher`, `U.S.C. | §`).
- Gapless playback: `src-tauri/src/ra_gapless_bench.rs` (`cargo test --release
  gapless_bench -- --ignored --nocapture --test-threads=1`) drives the real `Session`
  queue with the real CPU model on a virtual clock: first audio, underruns and the
  silence at every chunk boundary, at 2.2x / 3x / 3.5x, for a start, a click-jump and
  a skip.
- `glitch_synth` takes `GH_RATE` (default 1.0) and `GH_GAP_MS` (default 100); the
  round-2 board lines use `GH_RATE=2.2 GH_GAP_MS=0` (chunks back to back).

## Round 3 corpus

- `gen_stress_round3.py` writes `stress_round3.txt` (committed, ours) and
  `corpus_round3.tsv` (git-ignored): 1,500 self-written lines of legal / court text,
  medical and health news, academic abstracts with citations and statistics,
  Reddit / forum style, product reviews and code-ish text. Run the hunt on it with
  `GH_CORPUS=<frozen copy of corpus_round3.tsv>`.
- The round-3 board lines are `../board_round3.tsv` (25 lines, 2.2x, `GH_RATE=2.2
  GH_GAP_MS=0`); `make_glitch_board.py --r3-before/--r3-after` adds the section.
- The round-trip scorer's reference (`speechnorm.py`) does not know the round-3
  expansions (ftw, bpm, kg/m2, the paragraph sign, C sharp), so many round-3
  "glitches" are Whisper hearing the right words; read `roundtrip.csv` before fixing.

## Licences

- **Corpus:** Wikipedia text is CC BY-SA and Wikinews is CC BY 2.5. Neither is ever
  committed. `cache/`, `corpus.tsv`, `corpus.txt`, `wav/` and every output file are
  git-ignored; `build_corpus.py` re-fetches them.
- **`num2words` (LGPL) is a dev-only dependency.** Only `speechnorm.py`, the local
  Python spoken-form normaliser for scoring, uses it. It is never shipped and never
  linked into the app. The app's own number words are `src-tauri/src/ra_numwords.rs`.
- Other Python deps (dev only): `requests`, `jellyfish`.

## Reading the numbers

- **Static rate:** glitch words per 1,000 words. Informational flags (`GRU`,
  `INFO_*`, e.g. an initialism correctly spelled out) are not glitches.
- A word inside a pre-pass span (`billion` in `$5 billion`) has fed text `""`: the
  span's first word says the whole span, so it is never flagged on its own.
- Gate (phase 3 final): static <= 5 per 1,000 with no cause above 1 per 1,000;
  round-trip random controls <= 8 per 1,000.
