# Read Aloud Phase 2 (names + Acrobat + OCR) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Read Aloud says unknown names as words, works in Adobe Acrobat (with highlight, follow, click-to-jump), and reads the visible text of scans and images through Windows OCR.

**Architecture:**
- **Names:** a misaki `Fallback` (`ra_names.rs`) checks three sources in order: a hand-fixed list, embedded CMUdict, and a plain-Rust port of the g2p_en GRU letter-to-sound model (`ra_g2pen.rs`). It falls back to spelling letters, so it never errors.
- **Acrobat and OCR:** the UIA worker's single `Read` becomes an enum over three sources: UIA (today), Acrobat `IPDDomNode` (`ra_acro.rs`), and Windows OCR (`ra_ocr.rs`). The public `Uia` channel API stays the same, so `read_aloud.rs` barely changes.

**Tech Stack:** Rust (Tauri v2), `windows` 0.58 (Win32 + WinRT `Media_Ocr`), `misaki-rs` 0.6 (no espeak), `flate2` (already in Cargo.lock via `zip`), Python 3 tool scripts (build-time only, not shipped).

**Spec:** `docs/superpowers/specs/2026-10-02-read-aloud-phase2-design.md`. Parent spec: `docs/superpowers/specs/2026-10-02-read-aloud-design.md`.

**Branch:** `feat/read-aloud-phase2`, cut from `feat/read-aloud-fixes`. Each task runs in its own git worktree off that branch, and the controller merges.

**Research artifacts:**

| What | Where |
|---|---|
| Names research | `C:\Users\arowm\AppData\Local\Temp\claude\C--Users-arowm\73bf4adb-ff19-45f8-84e8-dda9f5f4c1b4\scratchpad\names\`: `cmudict.dict`, `run_g2pen.py`, `wh/x/g2p_en/` (g2p_en source + `checkpoint20.npz`) |
| Acrobat interface definition | `...\scratchpad\ocr\acro.idl` |
| Acrobat spike | `...\scratchpad\acro-spike\` (`probe.py`, `probe_vis.py`, `mkpdf.py`, `venv\` with comtypes) |

## Global Constraints

- **Windows only:** every new module is `#[cfg(target_os = "windows")]`-gated at its `mod` line in `main.rs`. `cargo test` on macOS CI must still pass. Pure-logic modules (`ra_names`, `ra_g2pen`) are gated the same way, following the existing `ra_*` pattern.
- **0 MB at rest:** nothing loads until Ctrl+Shift+E. Names data is decompressed inside the voice-engine load and dropped on idle unload. The OCR engine and capture buffers exist only during a pick or read.
- **Never panic** on any Read Aloud path (`panic = "abort"` in release kills all of Echo). No `unwrap`/`expect`/unchecked indexing or byte slicing on user text. Use `Result` + `log`.
- **The misaki `Fallback::phonemize` must always return `Ok`.** An `Err` silences the whole chunk.
- **No GPL, LGPL or share-alike** code or data. CMUdict is BSD-2 (keep the notice). g2p_en is Apache-2.0 (keep the notice). Do not commit `acro.idl`.
- **Never send keystrokes, clicks, focus changes or scrolls** to the user's apps in tests:
  - Manual tests are `#[ignore]`.
  - Tests launch and close only their own processes or windows.
  - If Acrobat is already running, the Acrobat test skips (prints "skipped: Acrobat already running").
- **UIA worker contract:** one thread (`ra-uia`) owns all COM objects. Callers use channels with the existing timeouts: pick 5 s, rects 100 ms, hover 50 ms.
- **Commit prefix:** `feat(read-aloud):` / `test(read-aloud):` / `chore(read-aloud):`. Every commit message ends with:
  ```
  Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01KN3Tceh8DdXc6dVqVEDh7j
  ```
- **Build checks, all required before a commit:**
  - `cargo test` (run in `src-tauri`): everything passes, ignored tests stay ignored
  - `cargo build`: 0 warnings
  - `npm run build` if any frontend file changed
- **Worktree builds** need `src-tauri/resources/models/` (empty dir) and `dist/index.html` (placeholder). Both are git-ignored; create them if missing. Revert any regenerated `src-tauri/gen/schemas` files.
- **Don't kill the running dev build** of Echo (`alan-echo.exe` from `C:\Users\arowm\alan-echo-wt-text\src-tauri\target\debug`). Build with `CARGO_TARGET_DIR` set to your own worktree's `target`, which is the default.

## Review Focus

1. **Acrobat with two or more tabs open:** only the *visible* page view has a DOM. Picking in the visible tab must work, and a hidden tab's view must never be chosen. Pinned in Task 4 (`visible_page_view` selection test over fake windows).
2. **A word misaki half-knows** (in misaki's stem rules but not its lexicon) must not reach the fallback twice or get letters spelled inside a word. Pinned in Task 3 (real-G2P test: "Pelosi's", "Zelensky-led", "PELOSI").
3. **OCR on a window that moves or scrolls after the pick:** the highlight must disappear, not draw in the wrong place. Pinned in Task 5 (`stale` checksum test).
4. **A window larger than `MaxImageDimension`** (a 4K screen, a maximised window): OCR must still return words with correct screen coordinates. Pinned in Task 5 (`tiles` maths test, with offsets).
5. **Text with no letters**, plus emoji, CJK, numbers and 60-letter tokens through the fallback: always `Ok` and never empty unless the input has no letters. Pinned in Task 1 and Task 3 tests.

---

## File map

| File | Task | Responsibility |
|---|---|---|
| `tools/read-aloud/build_names.py` | 1 | Download CMUdict, keep the first pronunciation, write `src-tauri/data/cmudict.tsv.gz` |
| `src-tauri/data/cmudict.tsv.gz` | 1 | Embedded CMUdict (`word\tARPABET`), about 1 MB |
| `src-tauri/data/names_fix.tsv` | 1 | Hand list: `word\tmisaki phonemes` |
| `src-tauri/src/ra_names.rs` | 1 (3 wires) | ARPAbet→misaki map, letter spelling, `Names` (hand + CMU + optional LTS), `NameFallback` |
| `tools/read-aloud/convert_g2pen.py` | 2 | `checkpoint20.npz` → `src-tauri/data/g2pen.bin.gz` (+ fixture) |
| `src-tauri/data/g2pen.bin.gz` | 2 | GRU weights (f32 LE, fixed order) |
| `src-tauri/src/ra_g2pen.rs` | 2 | GRU encoder-decoder, greedy decode → ARPAbet |
| `src-tauri/src/ra_g2pen_fixture.tsv` | 2 | 30 words: Python g2p_en output (test oracle) |
| `src-tauri/src/ra_tts.rs` | 3 | `with_fallback`; load names data in `load()` |
| `src-tauri/src/ra_text.rs` | 3 | `speakable_caps` extension (CMU-known caps + long-unknown caps) |
| `src-tauri/src/ra_acro.rs` | 4 | Acrobat COM bindings + `AcroRead` (pick/more/rects/scroll/hover) |
| `src-tauri/src/ra_ocr.rs` | 5 | Capture + Windows OCR + `OcrRead` |
| `src-tauri/src/ra_uia.rs` | 6 | `Read` enum + source selection |
| `src-tauri/src/read_aloud.rs` | 6 | OCR end-of-screen toast, overlay/pill excluded from capture |
| `THIRD_PARTY_NOTICES.md` | 1, 2 | CMUdict BSD-2 + g2p_en Apache-2.0 notices |
| `src-tauri/Cargo.toml` | 1, 5 | `flate2` (1); windows WinRT features (5) |

**Waves:**
- **Wave 1, in parallel and in separate worktrees:** Tasks 1, 2, 4 and 5. They touch disjoint files. Two exceptions, both resolved by the controller at merge: `Cargo.toml` (Tasks 1 and 5) and `main.rs` `mod` lines (all four).
- **Wave 2:** Task 3 (needs 1+2) and Task 6 (needs 4+5), in parallel.
- **Last:** Task 7, the live test and the session log, run by the controller.

---

### Task 1: Names dictionary: CMUdict + hand list + ARPAbet map + `NameFallback`

**Files:**
- Create: `tools/read-aloud/build_names.py`, `src-tauri/data/cmudict.tsv.gz`, `src-tauri/data/names_fix.tsv`, `src-tauri/src/ra_names.rs`, `THIRD_PARTY_NOTICES.md` (create, or append if present)
- Modify: `src-tauri/Cargo.toml` (add `flate2 = "1"` under `[target.'cfg(windows)'.dependencies]`), `src-tauri/src/main.rs` (add `#[cfg(target_os = "windows")] mod ra_names;` beside the other `ra_*` mods)

**Interfaces:**
- Produces:
  - `pub fn arpa_to_misaki(arpa: &str) -> String`: space-separated ARPAbet with stress digits → misaki string.
  - `pub fn spell(word: &str) -> String`: letter-by-letter misaki phonemes, never empty when the word has a letter.
  - `pub trait Lts: Send + Sync { fn arpabet(&self, word_lower: &str) -> Option<String>; }`
  - `pub struct Names`, with:
    - `Names::load(lts: Option<Box<dyn Lts>>) -> Names` (decompresses the embedded data; on failure it logs and returns an empty map, never panics)
    - `Names::lookup(&self, word: &str) -> Option<String>` (hand list, then CMU; case-insensitive; misaki symbols)
    - `Names::knows(&self, word_lower: &str) -> bool` (hand list or CMU contains it)
  - `pub struct NameFallback(pub std::sync::Arc<Names>)`, implementing `misaki_rs::fallback::Fallback`: lookup → LTS → `spell`, always `Ok`.

- [ ] **Step 1: Build the data**

Write `tools/read-aloud/build_names.py`:

```python
"""Build src-tauri/data/cmudict.tsv.gz from CMUdict (BSD-2-Clause, cmusphinx).
Keeps the first pronunciation per word, lowercase words, drops comments and
entries with non a-z' characters. Run: py tools/read-aloud/build_names.py"""
import gzip, io, re, urllib.request, pathlib
URL = "https://raw.githubusercontent.com/cmusphinx/cmudict/master/cmudict.dict"
out = pathlib.Path(__file__).resolve().parents[2] / "src-tauri" / "data" / "cmudict.tsv.gz"
raw = urllib.request.urlopen(URL, timeout=60).read().decode("utf-8")
seen, rows = set(), []
for line in raw.splitlines():
    line = line.split("#", 1)[0].strip()
    if not line:
        continue
    word, *ph = line.split()
    word = re.sub(r"\(\d+\)$", "", word).lower()
    if word in seen or not re.fullmatch(r"[a-z][a-z']*", word):
        continue
    seen.add(word)
    rows.append(f"{word}\t{' '.join(ph)}")
out.parent.mkdir(parents=True, exist_ok=True)
with gzip.open(out, "wb", compresslevel=9) as f:
    f.write(("\n".join(rows) + "\n").encode("utf-8"))
print(len(rows), "words ->", out, out.stat().st_size, "bytes")
```

Run: `py tools/read-aloud/build_names.py`
Expected: about 125–135k words, file under 1.2 MB. If there's no network, use the research copy at `...\scratchpad\names\cmudict.dict` by passing its path (add an optional `sys.argv[1]` local-file path).

Write `src-tauri/data/names_fix.tsv` (misaki symbols; diphthongs as Kokoro single letters `A I O W Y`, `ʤ ʧ`). First line is a comment:

```
# word<TAB>misaki phonemes. Hand fixes for names the CMU/LTS steps get wrong. Lowercase keys.
buttigieg	bˈudəʤˌɛʤ
zelensky	zəlˈɛnski
erdogan	ˈɛɹdəwˌɑn
tlaib	təlˈib
mayorkas	mIˈɔɹkəs
lukashenko	lˌukəʃˈɛŋkO
youngkin	jˈʌŋkɪn
milei	milˈA
ramaswamy	ɹˌɑməswˈɑmi
navalny	nəvˈɑlni
kishida	kiʃˈidə
sheinbaum	ʃˈInbWm
kubernetes	kˌubəɹnˈɛtiz
ozempic	Ozˈɛmpɪk
nguyen	wˈɪn
siobhan	ʃəvˈɔn
xi	ʃˈi
```

- [ ] **Step 2: Write the failing tests** (`src-tauri/src/ra_names.rs`, `#[cfg(test)] mod tests`)

```rust
#[test]
fn arpa_vowels_consonants_and_stress() {
    assert_eq!(arpa_to_misaki("P AH0 L OW1 S IY0"), "pəlˈOsi");
    assert_eq!(arpa_to_misaki("HH AW1 S"), "hˈWs");
    assert_eq!(arpa_to_misaki("CH ER1 CH"), "ʧˈɜɹʧ");
    assert_eq!(arpa_to_misaki("JH AH1 JH"), "ʤˈʌʤ");
    assert_eq!(arpa_to_misaki("B ER0 D"), "bɚd");
    assert_eq!(arpa_to_misaki("AY1 D IY2 AH0"), "ˈIdˌiə");
}

#[test]
fn every_arpabet_symbol_maps() {
    let all = "AA AE AH AO AW AY B CH D DH EH ER EY F G HH IH IY JH K L M N NG OW OY P R S SH T TH UH UW V W Y Z ZH";
    for s in all.split(' ') {
        let out = arpa_to_misaki(&format!("{s}1"));
        assert!(!out.is_empty() && !out.contains(|c: char| c.is_ascii_uppercase() && !"AIOWY".contains(c)), "{s} -> {out}");
    }
    assert_eq!(arpa_to_misaki("XX1 ??"), "", "unknown symbols are dropped, not panicked on");
}

#[test]
fn spell_never_empty_for_letters_and_never_panics() {
    assert!(!spell("FBI").is_empty());
    assert!(!spell("é").is_empty());
    assert_eq!(spell("123"), "");
    for w in ["🙂", "東京", "", "'", &"a".repeat(60)] { let _ = spell(w); }
}

#[test]
fn hand_list_beats_cmu_and_lookup_is_case_insensitive() {
    let n = Names::load(None);
    assert_eq!(n.lookup("Buttigieg").as_deref(), Some("bˈudəʤˌɛʤ"));
    assert!(n.lookup("PELOSI").is_some());
    assert!(n.lookup("pelosi").is_some());
    assert!(n.knows("putin"));
    assert!(n.lookup("qzxv").is_none());
}

struct FakeLts;
impl Lts for FakeLts {
    fn arpabet(&self, w: &str) -> Option<String> { (w == "zorblat").then(|| "Z AO1 R B L AE0 T".into()) }
}

#[test]
fn fallback_order_and_always_ok() {
    use misaki_rs::fallback::Fallback;
    let f = NameFallback(std::sync::Arc::new(Names::load(Some(Box::new(FakeLts)))));
    assert_eq!(f.phonemize("Pelosi").unwrap(), arpa_to_misaki("P AH0 L OW1 S IY0"));
    assert_eq!(f.phonemize("Zorblat").unwrap(), "zˈɔɹblæt");
    assert_eq!(f.phonemize("Qzxv").unwrap(), spell("Qzxv"));
    for w in ["🙂", "東京", "", "x'", &"q".repeat(60)] { assert!(f.phonemize(w).is_ok()); }
}

#[test]
fn load_is_fast_enough() {
    let t = std::time::Instant::now();
    let n = Names::load(None);
    assert!(n.knows("the"));
    // debug build; release is ~5x faster. Spec budget: <=150 ms added to first chunk in release.
    assert!(t.elapsed() < std::time::Duration::from_millis(2500), "{:?}", t.elapsed());
}
```

Run: `cargo test ra_names` → expected FAIL (module empty).

- [ ] **Step 3: Implement** `ra_names.rs`

```rust
//! Read Aloud — pronunciations for words misaki doesn't know (names, brands).
//! Order: hand list (names_fix.tsv) -> CMUdict (BSD-2) -> optional letter-to-sound
//! model -> spelled letters. Never panics; the misaki fallback is always Ok.

use std::collections::HashMap;
use std::io::Read;
use std::sync::Arc;

static CMU_GZ: &[u8] = include_bytes!("../data/cmudict.tsv.gz");
static FIX_TSV: &str = include_str!("../data/names_fix.tsv");

/// ARPAbet (no stress digit) -> misaki/Kokoro symbols. AH/ER depend on stress.
fn arpa_symbol(base: &str, stress: Option<char>) -> Option<&'static str> {
    Some(match (base, stress) {
        ("AH", Some('0')) => "ə",
        ("AH", _) => "ʌ",
        ("ER", Some('0')) => "ɚ",
        ("ER", _) => "ɜɹ",
        ("AA", _) => "ɑ", ("AE", _) => "æ", ("AO", _) => "ɔ", ("AW", _) => "W",
        ("AY", _) => "I", ("EH", _) => "ɛ", ("EY", _) => "A", ("IH", _) => "ɪ",
        ("IY", _) => "i", ("OW", _) => "O", ("OY", _) => "Y", ("UH", _) => "ʊ", ("UW", _) => "u",
        ("B", _) => "b", ("CH", _) => "ʧ", ("D", _) => "d", ("DH", _) => "ð", ("F", _) => "f",
        ("G", _) => "ɡ", ("HH", _) => "h", ("JH", _) => "ʤ", ("K", _) => "k", ("L", _) => "l",
        ("M", _) => "m", ("N", _) => "n", ("NG", _) => "ŋ", ("P", _) => "p", ("R", _) => "ɹ",
        ("S", _) => "s", ("SH", _) => "ʃ", ("T", _) => "t", ("TH", _) => "θ", ("V", _) => "v",
        ("W", _) => "w", ("Y", _) => "j", ("Z", _) => "z", ("ZH", _) => "ʒ",
        _ => return None,
    })
}

pub fn arpa_to_misaki(arpa: &str) -> String {
    let mut out = String::new();
    for tok in arpa.split_whitespace() {
        let (base, stress) = match tok.chars().last() {
            Some(c) if c.is_ascii_digit() => (&tok[..tok.len() - 1], Some(c)), // ASCII digit: 1 byte
            _ => (tok, None),
        };
        let Some(sym) = arpa_symbol(base, stress) else { continue };
        match stress {
            Some('1') => out.push('ˈ'),
            Some('2') => out.push('ˌ'),
            _ => {}
        }
        out.push_str(sym);
    }
    out
}
```

Then add:

- **`spell(word)`:** for each ASCII letter, push a fixed per-letter misaki string, with letter names as misaki writes them:

  ```
  a→ˈA b→bˈi c→sˈi d→dˈi e→ˈi f→ˈɛf g→ʤˈi h→ˈAʧ i→ˈI j→ʤˈA k→kˈA l→ˈɛl m→ˈɛm
  n→ˈɛn o→ˈO p→pˈi q→kjˈu r→ˈɑɹ s→ˈɛs t→tˈi u→jˈu v→vˈi w→dˈʌbəljˌu x→ˈɛks y→wˈI z→zˈi
  ```

  - Join letters with a space.
  - Non-ASCII letters: fold to ASCII by base letter where it's obvious (é→e); otherwise skip.
  - Digits and symbols are skipped.
- **`Names { map: HashMap<String, String> }`:** the value is the misaki string. CMU entries are converted with `arpa_to_misaki` at load; hand entries are inserted after CMU, so they win.
- **`Names::load`:** `flate2::read::GzDecoder::new(CMU_GZ).read_to_string(..)`. On `Err`, log and continue with the hand list only. Skip lines without a tab and lines starting with `#`.
- **`lookup`:** `self.map.get(&word.to_lowercase()).cloned()`, stripping a trailing `'s` / `’s` and adding `z` to the result when stripped.
- **`NameFallback::phonemize`:** in order, take the first that is non-empty:
  1. `lookup(word)`
  2. `lts.arpabet(lower)` mapped with `arpa_to_misaki`
  3. `spell(word)`

  Return `Ok(that)`. If the result is empty (no letters), return `Ok(String::new())`. Before the LTS step, guard `word.chars().count() <= 40 && word.chars().all(|c| c.is_ascii_alphabetic() || c == '\'')`, so non-ASCII words go straight to spelling.

`Names` holds `lts: Option<Box<dyn Lts>>`. The `Lts` trait is `Send + Sync`, so `Arc<Names>` is `Send + Sync`, as misaki's `Fallback` requires.

Add `THIRD_PARTY_NOTICES.md` entries with the CMUdict BSD-2 licence text (copy it from the CMUdict repo's `LICENSE`; the research copy is at `...\scratchpad\names\LICENSE`).

- [ ] **Step 4: Run tests**

`cargo test ra_names`: all PASS. Also run the full `cargo test` and `cargo build` (0 warnings).

The `arpa_vowels…` expectations were derived by hand from the table. If a test disagrees with the table, the table is right, so fix the test. Note it in the report.

- [ ] **Step 5: Commit**

```bash
git add tools/read-aloud/build_names.py src-tauri/data/cmudict.tsv.gz src-tauri/data/names_fix.tsv src-tauri/src/ra_names.rs src-tauri/src/main.rs src-tauri/Cargo.toml src-tauri/Cargo.lock THIRD_PARTY_NOTICES.md
git commit -m "feat(read-aloud): CMUdict + hand-list pronunciation fallback for unknown words"
```

---

### Task 2: g2p_en GRU letter-to-sound, ported to Rust

**Files:**
- Create: `tools/read-aloud/convert_g2pen.py`, `src-tauri/data/g2pen.bin.gz`, `src-tauri/src/ra_g2pen.rs`, `src-tauri/src/ra_g2pen_fixture.tsv`
- Modify: `src-tauri/src/main.rs` (`#[cfg(target_os = "windows")] mod ra_g2pen;`), `THIRD_PARTY_NOTICES.md` (append the g2p_en Apache-2.0 notice; if Task 1 also creates the file, the controller merges)

**Interfaces:**
- Consumes: nothing. This task must not depend on `ra_names`.
- Produces:
  - `pub struct G2pEn`
  - `G2pEn::load() -> Option<G2pEn>`: decompresses embedded weights; `None` on any failure, logged.
  - `G2pEn::predict(&self, word_lower: &str) -> Option<String>`: ARPAbet tokens with stress digits, space-separated (e.g. `"P AH0 L OW1 S IY0"`). `None` if the word is empty, longer than 30 chars, or contains anything other than a-z.
- Task 3 wraps it as `ra_names::Lts`. Don't implement the trait here.

- [ ] **Step 1: Read the reference model**

Read `...\scratchpad\names\wh\x\g2p_en\g2p.py`, functions `__init__`, `load_variables`, `grucell`, `gru`, `encode` and `predict`. The model has:

- **Graphemes:** `["<pad>","<unk>","</s>"] + a..z`.
- **Phonemes:** `["<pad>","<unk>","<s>","</s>"] + ARPAbet list`.
- **Embeddings:** `enc_emb`, `dec_emb`.
- **Encoder GRU:** `enc_w_ih`, `enc_w_hh`, `enc_b_ih`, `enc_b_hh`.
- **Decoder GRU:** `dec_w_ih`, `dec_w_hh`, `dec_b_ih`, `dec_b_hh`.
- **Output projection:** `fc_w`, `fc_b`.
- **Encode:** feeds the graphemes + `</s>` through the encoder GRU and takes the last hidden state.
- **Decode:** starts from `<s>` and runs greedy argmax for up to 20 steps, stopping at `</s>`.

Copy the exact GRU equations from `grucell`:
- `rzn_ih = x @ w_ih.T + b_ih`
- `rzn_hh = h @ w_hh.T + b_hh`
- `r` / `z` = sigmoid of the first and second thirds of the sum
- `n = tanh(rzn_ih[2/3:] + r * rzn_hh[2/3:])`
- `h' = (1 - z) * n + z * h`

- [ ] **Step 2: Write the converter + fixture**

`tools/read-aloud/convert_g2pen.py`:
1. Load `checkpoint20.npz` (path in `argv[1]`).
2. Write a header: magic `b"G2PE"`, a u32 count, then for each array in a fixed name order: a u8 name length, the name, a u32 ndim, u32 dims, then f32 little-endian data.
3. gzip level 9 → `src-tauri/data/g2pen.bin.gz`.
4. Run the Python model (reuse the stub-import trick in `run_g2pen.py`) on these 30 words and write `src-tauri/src/ra_g2pen_fixture.tsv` as `word\tARPAbet`:
   pelosi zelensky buttigieg nguyen schumer kamala putin netanyahu anthropic kubernetes nvidia ozempic erdogan bezos tlaib ramaswamy manchin sinema youngkin mayorkas navalny lukashenko sunak kishida milei sheinbaum houthi kharkiv bakhmut moderna

Run: `<scratch>\acro-spike\venv\Scripts\python.exe tools/read-aloud/convert_g2pen.py <scratch>\names\wh\x\g2p_en\checkpoint20.npz`. The venv may need `pip install numpy`; it's in the scratchpad, not the repo.

Expected: `g2pen.bin.gz` under 3.5 MB, and a fixture with 30 lines.

- [ ] **Step 3: Write the failing tests** (in `ra_g2pen.rs`)

```rust
#[test]
fn matches_python_g2p_en_on_fixture() {
    let g = G2pEn::load().expect("weights load");
    let mut bad = Vec::new();
    for line in include_str!("ra_g2pen_fixture.tsv").lines().filter(|l| !l.is_empty()) {
        let (w, want) = line.split_once('\t').unwrap();
        let got = g.predict(w).unwrap_or_default();
        if got != want { bad.push(format!("{w}: got {got} want {want}")); }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn rejects_bad_input_without_panicking() {
    let g = G2pEn::load().unwrap();
    for w in ["", "Pelosi", "o'neil", "東京", "🙂", &"a".repeat(31), "abc1"] {
        assert!(g.predict(w).is_none(), "{w}");
    }
}

#[test]
#[ignore] // perf: run in release: cargo test --release ra_g2pen -- --ignored
fn under_5ms_per_word_release() {
    let g = G2pEn::load().unwrap();
    let t = std::time::Instant::now();
    for _ in 0..10 { for w in ["zelensky", "lukashenko", "kubernetes"] { g.predict(w); } }
    assert!(t.elapsed().as_secs_f64() * 1000.0 / 30.0 < 5.0, "{:?}", t.elapsed());
}
```

The fixture test uses `unwrap` only inside tests, which is allowed. Run `cargo test ra_g2pen` → FAIL.

- [ ] **Step 4: Implement** `ra_g2pen.rs`
- Parse the blob with bounds-checked reads. Any malformed header gives `None`, never a panic.
- Store matrices as `Vec<f32>` row-major with their dims.
- Implement `matvec(w, rows, cols, x) -> Vec<f32>` with `chunks_exact` and plain loops (no deps), plus `gru_step`, `encode` and `decode` exactly as in Python.
- Index lookups use `.get()`.
- If an index is out of range, return `None`.
- Cap the number of decode steps at 30.

- [ ] **Step 5: Run tests**

`cargo test ra_g2pen`: PASS. Also run `cargo test --release ra_g2pen -- --ignored` and record the ms/word figure. Then `cargo build` with 0 warnings.

If a few fixture words differ only by float rounding (argmax ties), use f64 accumulation in `matvec`. Do **not** loosen the test.

- [ ] **Step 6: Commit**

```bash
git add tools/read-aloud/convert_g2pen.py src-tauri/data/g2pen.bin.gz src-tauri/src/ra_g2pen.rs src-tauri/src/ra_g2pen_fixture.tsv src-tauri/src/main.rs THIRD_PARTY_NOTICES.md
git commit -m "feat(read-aloud): g2p_en GRU letter-to-sound in plain Rust"
```

---

### Task 3: Wire names into the voice engine + caps rule (Wave 2; needs 1 + 2)

**Files:**
- Modify: `src-tauri/src/ra_tts.rs` (`load()` ~line 384, `Loaded` ~line 83, `g2p_words` ~line 566, the test G2P at ~line 667), `src-tauri/src/ra_names.rs` (impl `Lts` for `G2pEn` lives here, behind a `use crate::ra_g2pen::G2pEn`), `src-tauri/src/ra_text.rs` (`speakable_caps` / `caps_part`)
- Create: `tools/read-aloud/names.txt`, `tools/read-aloud/names_ab.html` (generated listening board; template inline in the test)

**Interfaces:**
- Consumes:
  - `ra_names::{Names, NameFallback, Lts}` (Task 1)
  - `ra_g2pen::G2pEn::{load, predict}` (Task 2)
  - Existing `ra_text::speakable_caps(word, known: &dyn Fn(&str) -> bool) -> Cow<str>`
- Produces:
  - `ra_text::speakable_caps(word, known, names_knows: &dyn Fn(&str) -> bool) -> Cow<str>`: the new third parameter. Update every caller and test.
  - `Loaded` gains `names: Arc<Names>`.

- [ ] **Step 1: Write the failing tests**

In `ra_text.rs` tests, add:

```rust
fn cmu(w: &str) -> bool { ["pelosi", "schumer"].contains(&w) }

#[test]
fn caps_names_known_to_cmu_are_lowercased() {
    assert_eq!(speakable_caps("PELOSI", &known, &cmu), "pelosi");
    assert_eq!(speakable_caps("SCHUMER's", &known, &cmu), "SCHUMER's"); // mixed-case suffix: untouched
    assert_eq!(speakable_caps("PELOSI'S", &known, &cmu), "pelosi's");
}

#[test]
fn short_unknown_caps_stay_spelled_long_ones_are_pronounced() {
    assert_eq!(speakable_caps("GDP", &known, &cmu), "GDP");
    assert_eq!(speakable_caps("NFLX", &known, &cmu), "NFLX");
    assert_eq!(speakable_caps("ZELENSKY", &known, &cmu), "zelensky");
    assert_eq!(speakable_caps("US", &known, &cmu), "US");
}
```

In `ra_tts.rs` tests, add a real-G2P test using the shared test G2P (rebuilt with the fallback):

```rust
#[test]
fn names_are_pronounced_not_spelled() {
    let g = test_g2p(); // G2P::with_fallback(EnglishUS, Some(Box::new(NameFallback(names))))
    for (text, word) in [("Pelosi spoke", "Pelosi"), ("PELOSI SPOKE", "PELOSI"), ("Pelosi's bill", "Pelosi's"),
                         ("a Zelensky-led talk", "Zelensky-led"), ("Buttigieg won", "Buttigieg")] {
        let words: Vec<&str> = text.split(' ').collect();
        let fed = g2p_words(g, &words);
        let fed: Vec<&str> = fed.iter().map(|c| c.as_ref()).collect();
        let (_, toks) = g.g2p(&fed.join(" ")).unwrap();
        let ph: String = toks.iter().filter_map(|t| t.phonemes.clone()).collect();
        // spelled names have a stress mark per letter; a word has at most ~4
        let marks = ph.matches(['ˈ', 'ˌ']).count();
        assert!(marks <= words.len() + 4, "{text}: {ph}");
        assert!(!ph.contains("  "), "{word}: no multi-space gap: {ph}");
    }
}

#[test]
fn fallback_word_keeps_one_timing_span_per_word() {
    let g = test_g2p();
    let words = ["Senator", "Pelosi", "said", "no"];
    let (_, toks) = g.g2p(&words.join(" ")).unwrap();
    let tt: Vec<&str> = toks.iter().map(|t| t.text.as_str()).collect();
    let owner = ra_timing::align(&words, &tt);
    for w in 0..words.len() { assert!(owner.contains(&Some(w)), "word {w} has no token"); }
}
```

Run: FAIL (signature mismatch / spelled output).

- [ ] **Step 2: Implement**
- **`ra_names.rs`:** add
  ```rust
  impl Lts for crate::ra_g2pen::G2pEn { fn arpabet(&self, w: &str) -> Option<String> { self.predict(w) } }
  ```
- **`ra_tts::load`:**
  - Build `names = Arc::new(Names::load(G2pEn::load().map(|g| Box::new(g) as Box<dyn Lts>)))` inside the existing `ra-tts-g2p` thread, so it overlaps the ONNX session open.
  - That thread returns `(G2P, Arc<Names>)`, where `G2P::with_fallback(Language::EnglishUS, Some(Box::new(NameFallback(names.clone()))))`.
  - The spawn-failed path builds the same thing inline.
  - Store `names` in `Loaded`. The drop on idle unload frees it.
- **`g2p_words`:** pass `&|w| l.names.knows(w)` as the third argument. Thread `names` in by changing `g2p_words(g, names, words)`.
- **`caps_part` new rule, applied after the existing checks.** Let `lower = base.to_lowercase()`. Then:
  - `KEEP_CAPS` contains `base` → keep.
  - `known(base)` → keep (misaki has the caps spelling, e.g. NATO).
  - `known(&lower)` or `names_knows(&lower)` → lowercase.
  - `base.chars().count() >= 5` → lowercase (the GRU says it).
  - Otherwise keep (spelled acronym).
- Possessive handling is unchanged.

- [ ] **Step 3: Run tests**

`cargo test`: all PASS. `cargo build`: 0 warnings.

- [ ] **Step 4: A/B listening board (`#[ignore]` test `names_ab_board`)**
- **Input:** `tools/read-aloud/names.txt`, one word per line: the 59 names from spec §2.5 plus 15 control words (the, report, president, government, Tuesday, Washington, America, economy, mid-year, inflation, NATO, FBI, GDP, iPhone, McDonald's).
- **Render:** each word inside the carrier sentence "Today, WORD spoke." at 2.2x with the real CPU model, twice:
  - **before:** `G2P::new` without fallback, and the old caps rule, by calling `speakable_caps` with `names_knows = |_| false`
  - **after:** the full pipeline
- **Output:** WAVs to `%TEMP%\echo-names-ab\{before,after}\NN_word.wav` (use the existing WAV writer or `hound` if present; otherwise write the 44-byte PCM16 header by hand), plus `names_ab.html` with two `<audio>` players per row and a "phonemes" column.

Run: `cargo test --release names_ab_board -- --ignored --nocapture`. It prints the HTML path.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/ra_tts.rs src-tauri/src/ra_text.rs src-tauri/src/ra_names.rs tools/read-aloud/names.txt
git commit -m "feat(read-aloud): pronounce unknown names via CMUdict + g2p_en fallback; caps names read as words"
```

---

### Task 4: Acrobat source (`ra_acro.rs`)

**Files:**
- Create: `src-tauri/src/ra_acro.rs`
- Modify: `src-tauri/src/main.rs` (`#[cfg(target_os = "windows")] mod ra_acro;`). Do **not** touch `ra_uia.rs`; Task 6 wires it in.

**Interfaces:**
- Consumes: `crate::ra_uia::Rect` (`{x,y,w,h: f64}`, physical screen px).
- Produces (all calls on the `ra-uia` thread, COM already initialised MTA):
  - `pub fn is_acrobat_view(hwnd: isize) -> bool`: the window under the point (or an ancestor) has class `AVL_AVView` and its process image is `Acrobat.exe` or `AcroRd32.exe`.
  - `pub struct AcroRead`, with:
    - `pub fn pick(x: i32, y: i32) -> Result<(AcroRead, Vec<String>), String>`: ≤ 200 tokens starting at the clicked word.
    - `pub fn more(&mut self) -> Result<Vec<String>, String>`: ≤ 400 tokens; `Ok(empty)` = end.
    - `pub fn rects(&self, src: &[usize]) -> Option<Vec<Rect>>`: `Some(empty)` = off screen.
    - `pub fn scroll_to(&self, idx: usize)`
    - `pub fn hwnd(&self) -> isize` (the top-level frame, for `source_hwnd`)
  - `pub fn hover(x: i32, y: i32) -> Option<Vec<Rect>>`: box of the on-screen word under the point.
  - Tokens are each Word node's `GetTextContent()`, trailing space included as Acrobat gives it. The phase-1 `ra_text::normalize` handles spacing and hyphens.

- [ ] **Step 1: COM bindings**

Write `windows::core::interface` declarations in `ra_acro.rs`, using the GUIDs and **exact vtable order** from `...\scratchpad\ocr\acro.idl`:
- `IGetPDDomNode : IUnknown` `{f9f2fe81-f764-4bd0-afa5-5de841ddb625}`: method `get_PDDomNode(VARIANT varChild, *mut Option<IPDDomNode>)`. Check the IDL lines 385–396 for the exact signature.
- `IPDDomNode : IDispatch` `{5007373a-20d7-458f-9ffb-abc900e3a831}`, methods in IDL order: `GetParent, GetType, GetChildCount, GetChild, GetName, GetValue, IsSame, GetTextContent, GetLocation, GetFontInfo, GetFromID, GetIAccessible, ScrollTo, GetTextInLines`. Declare **all 14** so the vtable offsets are right, even the ones we don't call.
- `IPDDomNodeExt : IUnknown` `{4A894040-247E-4aff-BB08-3489E9905235}`: `GetState, Navigate, SetFocus, GetIndex, GetPageNum, DoDefaultAction, ScrollToEx, Relationship`.
- `SID_GetPDDomNode = {C0A1D5E9-1142-4cf3-B607-82FC3B96A4DF}`.
- Access path:
  ```
  AccessibleObjectFromWindow(hwnd, OBJID_CLIENT, &IAccessible::IID)
    → if accRole(CHILDID_SELF) != ROLE_SYSTEM_DOCUMENT (15), look at its children for the role-15 one
    → cast to IServiceProvider
    → QueryService(&SID_GetPDDomNode, &IGetPDDomNode::IID)
    → get_PDDomNode(VARIANT(CHILDID_SELF=0))
  ```
- Node types: `Word = 5` (IDL enum; verify the value in `acro.idl` and record it in a const).

- [ ] **Step 2: Write the failing tests (pure logic over a fake tree)**

Put the DOM walk behind a small trait so it can be tested without Acrobat:

```rust
pub(crate) trait Node: Sized {
    fn kind(&self) -> i32;
    fn children(&self) -> Vec<Self>;
    fn text(&self) -> String;
    fn location(&self) -> Option<Rect>; // None = off screen (0,0,0,0) or error
}
/// Depth-first document-order iterator over Word leaves; stops after `cap` words.
pub(crate) struct Words<N: Node> { stack: Vec<std::vec::IntoIter<N>>, cap: usize, seen: usize }
pub(crate) fn words<N: Node>(root: N, cap: usize) -> Words<N>;
/// Index (in the iterator's order) of the first word whose location contains (x, y).
pub(crate) fn word_at<N: Node>(ws: &[N], x: f64, y: f64) -> Option<usize>;
/// Pick the visible page view among AVL_AVView candidates: (hwnd, visible, area).
pub(crate) fn visible_page_view(c: &[(isize, bool, i64)]) -> Option<isize>;
```

Tests:
- Words come out in document order across nested Struct/Text/Word nodes.
- The `cap` stops the walk.
- `word_at` finds a word by point; an off-screen word (None) is never matched.
- `visible_page_view` ignores hidden views and picks the largest visible one (Review Focus 1).
- The method count of the `IPDDomNode` vtable equals 14. Assert with a const array of method names that must match the IDL list above.

Run: FAIL.

- [ ] **Step 3: Implement**
- **Real node:** `impl Node for IPDDomNode`.
  - `location`: `GetLocation`, mapping `(0,0,0,0)` to None.
  - `text`: `GetTextContent`.
- **`AcroRead`** holds:
  - the root `IPDDomNode`
  - a lazy `Words` iterator state (the stack)
  - `nodes: Vec<IPDDomNode>` (index = source index)
  - a cursor
- **`pick`:**
  1. Find the frame: `GA_ROOT` of `WindowFromPoint`.
  2. Find the page view: enumerate child windows of class `AVL_AVView` with `EnumChildWindows`, then `visible_page_view` (visible = `IsWindowVisible` and a non-zero rect). Use the page-view child whose rect contains the point.
  3. Get the DOM root.
  4. Walk words, collecting nodes, until a node's location contains the point. If none does after the walk passes the visible region, use the first on-screen word below or after the point. If there's still no match, `Err(NO_TEXT_ACRO)`.
  5. Collect the next 200 words.
- Cap the total at 20 000 words, and cap the time per call at 3 s with an `Instant` check in the loop. Return what you have if it hits the cap.
- **`more`:** continue the iterator for 400.
- **`rects`:** `GetLocation` per index; any `None` → `Some(vec![])`.
- **`scroll_to`:** `ScrollTo()` on `nodes[idx]` via `.get()`.
- **`hover`:** a short-lived `AcroRead`-free path: root → walk on-screen words only (skip subtrees whose location is None *and* that have no on-screen descendants), stopping at the first containing word. Cap at 50 ms.
- **Errors:** strings, no panics. Log COM HRESULTs at `debug`.

- [ ] **Step 4: Manual test (`#[ignore]`, `acro_live`)**
1. If any `Acrobat.exe` or `AcroRd32.exe` is running, print "skipped: Acrobat already running" and return.
2. Generate a 3-page PDF into `%TEMP%`. Port `...\scratchpad\acro-spike\mkpdf.py` to Rust, or call `py` on a copy placed in `tools/read-aloud/mkpdf.py`; the latter is fine, since it's a test tool.
3. Launch Acrobat (`C:\Program Files\Adobe\Acrobat DC\Acrobat\Acrobat.exe <pdf>`) and wait ≤ 40 s for an `AVL_AVView`.
4. Find an on-screen word through the DOM, then call `AcroRead::pick` at its centre. Assert the first token equals that word's text.
5. `more()` until a word on page 3. Call `scroll_to` on it. Within 1 s, `rects` of that word must be non-empty.
6. Close **only** the launched process tree:
   - `taskkill /PID <pid> /T /F`, after `WM_CLOSE` fails within 3 s.
   - The close prompt "close all tabs?" is expected, so go to the kill after the timeout.
   - Never touch other processes.
7. Never send input.

Run it once: `cargo test acro_live -- --ignored --nocapture`. Record the timings (pick ms, per-word rect ms) in the report.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/ra_acro.rs src-tauri/src/main.rs tools/read-aloud/mkpdf.py
git commit -m "feat(read-aloud): Adobe Acrobat source via IPDDomNode (text, word boxes, ScrollTo)"
```

---

### Task 5: OCR source (`ra_ocr.rs`)

**Files:**
- Create: `src-tauri/src/ra_ocr.rs`
- Modify:
  - `src-tauri/Cargo.toml`: add windows features `"Foundation"`, `"Foundation_Collections"`, `"Globalization"`, `"Graphics_Imaging"`, `"Media_Ocr"`, `"Storage_Streams"`. Verify each name exists in windows 0.58. `"Win32_Graphics_Gdi"` and `"Win32_UI_WindowsAndMessaging"` are already present.
  - `src-tauri/src/main.rs`: add `mod ra_ocr`.

**Interfaces:**
- Consumes: `crate::ra_uia::Rect`.
- Produces (called on the `ra-uia` thread, MTA):
  - `pub struct OcrRead`, with:
    - `pub fn pick(hwnd: isize, x: i32, y: i32) -> Result<(OcrRead, Vec<String>), String>`: OCR the window's visible client area, then ≤ 200 tokens from the word at or nearest the point. `Err(NO_TEXT_OCR)` when no words are found.
    - `pub fn more(&mut self) -> Result<Vec<String>, String>`: the rest (≤ 400 per call), then `Ok(empty)`.
    - `pub fn rects(&self, src: &[usize]) -> Option<Vec<Rect>>`: the stored boxes, or `Some(empty)` when `stale()`.
    - `pub fn scroll_to(&self, _idx: usize)`: no-op.
    - `pub fn hwnd(&self) -> isize`
    - `pub fn at_end(&self) -> bool`: true once `more` returned empty; Task 6 uses it for the toast.
  - `pub fn hover_cached(cache: &mut Option<OcrRead>, hwnd: isize, x: i32, y: i32) -> Option<Vec<Rect>>`: OCRs once per window. A stale cache is re-OCR'd at most every 500 ms.
  - Tokens are word texts with a trailing `" "`, and `"\n"`-terminated at line ends, to match how UIA tokens look to `ra_text::normalize`. Check `ra_text.rs` for the exact paragraph convention and mirror it.

- [ ] **Step 1: Write the failing tests (pure)**

```rust
/// OCR words in reading order: lines as given, words left->right in each line.
pub(crate) struct OcrWord { pub text: String, pub rect: Rect, pub line: usize }
pub(crate) fn start_index(words: &[OcrWord], x: f64, y: f64) -> Option<usize>; // containing word, else nearest on the closest line, else None
pub(crate) fn tiles(w: u32, h: u32, max: u32) -> Vec<(u32, u32, u32, u32)>;   // (x, y, w, h), overlap 64 px, every pixel covered
pub(crate) fn thumb_sum(bgra: &[u8], w: u32, h: u32) -> u64;                    // 64x64 sampled checksum
pub(crate) fn dedupe_tiles(words: Vec<OcrWord>) -> Vec<OcrWord>;               // drop words duplicated in the overlap (same text, rects overlapping > 50%)
```

Tests:
- `start_index`: a containing word; a point between words on a line → nearest; a point below all lines → the last line's nearest; empty → None.
- `tiles(3840, 2160, 2600)`: covers everything, each tile ≤ 2600 on both sides, and offsets are right (Review Focus 4).
- `tiles(800, 600, 2600)` is one tile.
- `thumb_sum` changes when one sampled region changes and is stable otherwise; zero-size input doesn't panic.
- `dedupe_tiles` removes overlap duplicates and keeps distinct same-text words that are far apart.
- **stale logic** (Review Focus 3): a pure `fn is_stale(orig: (Rect, u64), now: (Rect, u64)) -> bool`. Stale if the window rect moved or resized, or the checksum differs.

Run: FAIL.

- [ ] **Step 2: Implement capture + OCR**
- **Capture:**
  - `GetClientRect` + `ClientToScreen` → screen rect, clipped to the virtual screen.
  - `GetDC(None)` → `CreateCompatibleDC` / `CreateCompatibleBitmap` → `BitBlt(SRCCOPY)` → `GetDIBits` as top-down 32-bit BGRA.
  - Free every GDI object on all paths with a small guard struct whose `Drop` releases it.
- **OCR:**
  1. `OcrEngine::TryCreateFromUserProfileLanguages()`. If that fails or gives null, return `Err("Windows OCR unavailable")`.
  2. Read `max = OcrEngine::MaxImageDimension()`.
  3. For each tile: `DataWriter::new()` → `WriteBytes` → `DetachBuffer` → `SoftwareBitmap::CreateCopyFromBuffer(&buf, BitmapPixelFormat::Bgra8, w, h)` → `engine.RecognizeAsync(&bmp)?.get()?`.
  4. Walk `Lines()` → `Words()` → `Text()`, `BoundingRect()`, adding the tile offset and the capture origin.
  5. `dedupe_tiles`. Drop the BGRA buffer right after.
- Keep `(window rect, thumb_sum)` from the capture for `stale()`.
- **`stale()`:** re-capture only a 64×64-sampled thumbnail, at most every 500 ms (cache the last result with an `Instant`).
- **Latency:** log pick total ms. If it's over 1.5 s on a 1080p window in the manual test, implement the band-first variant: OCR ±400 px around the click first and return those tokens; `more` then OCRs the rest above and below in reading order. Record the decision in the report.

- [ ] **Step 3: Manual test (`#[ignore]`, `ocr_live`)**
1. Create our own top-level window: a plain Win32 window that paints a few lines of large known text with `DrawTextW`.
2. `OcrRead::pick` at a known word. Assert ≥ 80% of the expected words came back, in order.
3. Destroy the window.
4. Never touch other windows.

Run once: `cargo test ocr_live -- --ignored --nocapture`, and record the ms.

- [ ] **Step 4: Run all checks and commit**

```bash
git add src-tauri/src/ra_ocr.rs src-tauri/src/main.rs src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "feat(read-aloud): Windows OCR source for windows with no text (visible area)"
```

---

### Task 6: Source switch in the UIA worker + OCR end toast (Wave 2; needs 4 + 5)

**Files:**
- Modify: `src-tauri/src/ra_uia.rs` (`Read`, `State`, `run`, `pick`, `more`, `rects`, `scroll`, `hover`, `click_on_text`), `src-tauri/src/read_aloud.rs` (end-of-screen toast; overlay/pill capture exclusion)

**Interfaces:**
- Consumes:
  - `ra_acro::{is_acrobat_view, AcroRead, hover}` (Task 4)
  - `ra_ocr::{OcrRead, hover_cached}` (Task 5)
- Produces:
  - The unchanged public `Uia` API, plus `pub fn at_end_of_screen(&self) -> bool`. It asks the worker through a new `Req::AtEnd(Sender<bool>)` with a 50 ms timeout, and is true only for an OCR read whose `at_end()` is true.
  - A new `pub const NOTHING_ON_SCREEN_END: &str = "Read to the end of what's on screen";` in `read_aloud.rs`.

- [ ] **Step 1: Write the failing tests**
- **`choose_source` (pure):**
  ```rust
  pub(crate) enum Src { Uia, Acro, Ocr }
  pub(crate) fn choose_source(is_acrobat: bool, uia_result: Option<&str>) -> Vec<Src>
  ```
  It returns the try order:
  - Acrobat → `[Acro, Ocr]`
  - else → `[Uia, Ocr]`

  OCR is only tried after UIA returns exactly `NO_TEXT`; other UIA errors (timeouts) don't trigger OCR. Test all branches.
- **The `Read` enum delegates** `rects` / `scroll` / `more`: test with the UIA variant's existing tests still passing (rename only).
- **`read_aloud.rs`:** when `more()` returns empty and `uia.at_end_of_screen()` is true, the End event shows the toast. Test the pure decision fn `end_toast(at_end_of_screen: bool) -> Option<&'static str>`.

Run: FAIL.

- [ ] **Step 2: Implement**
- `enum Read { Uia(UiaRead), Acro(AcroRead), Ocr(OcrRead) }`. Rename today's struct to `UiaRead`, and implement `Default` as `Read::Uia(UiaRead::default())`.
- **`pick(st, x, y, retry)`:**
  1. `root = root_at(x, y)` and `is_acrobat_view`.
  2. Follow `choose_source`:
     - **Acro:** `AcroRead::pick`.
     - **Uia:** today's code path.
     - **Ocr:** `OcrRead::pick(root, x, y)`.
  3. Store the resulting `Read` and update `st.source`.
- **`Req::More`, `Req::Rects`, `Req::Scroll`:** match on the variant. Keep the existing truncate/rollback semantics for UIA; for Acro/Ocr, roll back by restoring the cursor (store a clone or index).
- **`Req::Hover`:**
  - Acrobat → `ra_acro::hover`.
  - UIA hit → as today.
  - UIA `NO_TEXT` → `ra_ocr::hover_cached(&mut st.ocr_hover, root, x, y)`. Clear `ocr_hover` when the pick finishes and when the window changes.
- **`click_on_text`** for Acro/Ocr reads: always Ok (no control tree).
- **`ClickPick` staging:** works unchanged, because `Read` is moved as a whole.
- **`read_aloud.rs`:**
  - On `show_overlay` and pill creation, call `SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)`. Get the HWND via the Tauri window's `hwnd()`, Windows only, and ignore errors.
  - On End, if `uia.at_end_of_screen()`, show the toast `NOTHING_ON_SCREEN_END` with the existing toast mechanism instead of the silent end.

- [ ] **Step 3: Run all checks**

`cargo test`, `cargo build` (0 warnings), `npm run build` (no frontend change expected).

Re-run the phase-1 manual Notepad test (`cargo test notepad -- --ignored`, the one in `ra_uia_manual_tests.rs`; check its exact name) to prove UIA still works after the refactor. It opens and closes its own Notepad window only. Before running, make sure no Notepad instance belongs to the user (`Get-Process notepad`); if one does, skip it and say so.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/ra_uia.rs src-tauri/src/read_aloud.rs
git commit -m "feat(read-aloud): pick Acrobat / UIA / OCR source per window; end-of-screen toast for OCR"
```

---

### Task 7: Live test, measurements, session log (controller)

- [ ] Merge everything into `feat/read-aloud-phase2`. Full `cargo test`, `cargo build` (0 warnings), `npm run build`.
- [ ] Final whole-branch review (fresh reviewer). Fix any Critical/Important findings, then re-review.
- [ ] Restart the dev build from the phase-2 worktree. The user tests:
  - **Acrobat:** click, highlight, follow onto page 2, click-to-jump, Go to word.
  - **A scanned PDF in Acrobat:** OCR source, plus the end-of-screen toast.
  - **An image** in the Photos app.
  - **The names A/B board:** listen; anything wrong goes into `names_fix.tsv`.
- [ ] Record the RAM figures (spec §4): during the read, after the read, and after the 2-minute unload.
- [ ] Write `docs/2026-10-0X-read-aloud-phase2-session-log.md`. Update the memory file `project_echo-read-aloud.md`. Merge `feat/read-aloud-phase2` into `feat/read-aloud` only after the user's OK on the live test. Never merge to `main` or push.
