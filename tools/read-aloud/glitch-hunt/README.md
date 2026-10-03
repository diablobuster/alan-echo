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
