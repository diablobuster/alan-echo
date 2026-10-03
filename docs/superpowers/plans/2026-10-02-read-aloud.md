# Echo Read Aloud — Implementation Plan (Phase 1: core loop)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Press `Ctrl+Shift+E`, click any word in Chrome/Edge/Edge-PDF/Word/Notepad, and Echo reads aloud from there at 2.2x with a live on-page word highlight and a Now Reading pill.

**Architecture:** New flat Rust modules in `src-tauri/src/` (Echo's one-file-per-feature pattern). Pure logic (text normalization, chunking, timing, WSOLA, state machine) is unit-tested. Windows-only integration (UIA, low-level mouse hook, ONNX Runtime + DirectML, cpal output) runs on dedicated worker threads and is cfg-gated to Windows. Two on-demand Tauri windows (`ra-overlay`, `ra-pill`) render React components; they talk to Rust through events + commands.

**Tech Stack:** Rust (Tauri 2), `ort` 2.0.0-rc.13 (DirectML), `misaki-rs` 0.6 (no default features), `windows` 0.58 (UIA, hooks), `cpal` 0.15, React 19.

**Spec:** `docs/superpowers/specs/2026-10-02-read-aloud-design.md` (read it first; this plan argues from it).

**Design:** https://claude.ai/artifact/7VdxFzrRDRXjP9nbT4rLP2 (Version 4: black-and-white stock-analyzer look).

## Global Constraints

- Windows only: every new module is `#[cfg(target_os = "windows")]`-gated at its `mod` line; the Settings section is hidden on macOS. `cargo test` on macOS CI must still pass.
- Read Aloud is **off by default** (`read_aloud_enabled = false`). When off: hotkey not registered, nothing loads.
- **0 MB at rest:** no model, window, hook or thread exists until `Ctrl+Shift+E`. The model starts loading **on the hotkey press**, never at app start.
- Idle unload: 2 minutes after the last audio ends, drop the ONNX session.
- Hotkey handler only flips state and posts to a thread. No UIA, model load or process spawn inside it (debug-journal rule).
- Never panic on any Read Aloud path (`panic = "abort"` in release kills all of Echo). Use `Result` + log + toast.
- Speed: synthesize at 1.0x, then WSOLA time-stretch **per chunk**. Never use Kokoro's `speed` input above 1.0.
- Word timing: rounded phoneme durations × 600 samples, **−36 ms** offset, word end = next word start.
- Chunks: first ≤12 words (cut at a clause), then ≤50 words; synthesis stays at most 2 chunks ahead of playback.
- Model: patched `model_quantized.onnx` (q8) on DirectML; CPU fallback uses fp32. Never `q8f16`.
- Phonemizer: `misaki-rs` with `default-features = false` (no espeak/GPL).
- Speed range 1.0–3.5, step 0.1, default 2.2.
- UI look (overlay hint + pill): black-and-white per design v4 — bg `#000`, raised `#0c0c0f`, border `rgba(255,255,255,0.14)`, text `#fff`, muted `#a6a6ab`, primary button white fill + black icon, fonts Geist / Geist Mono / Instrument Serif (Echo already ships them). Live word: brass `rgba(200,151,63,0.42)` (default) or Mono (3px black underline + 1px white halo). Settings section follows Echo's existing app theme tokens.
- Commit prefix: `feat(read-aloud):` / `test(read-aloud):`. Branch `feat/read-aloud`.

## Review Focus

1. **Clicking while the model is still loading** (first press after idle): the click must be accepted and audio must start when the first chunk is ready, pill in "warming" state — never a dropped click or a hang. → test in Task 8 (`picking_click_while_loading_goes_to_warming`).
2. **Text with no letters** (a click on "—", a number-only cell, emoji): no phonemes → must skip to the next speakable word, not stall or panic. → test in Task 3 (`align_skips_punctuation_only_tokens`) and Task 2 (`punctuation_attaches_to_previous_word`).
3. **A very long sentence with no punctuation** (code, tables, run-on PDF text): must split at 50 words and stay within the model's 510-token limit. → test in Task 2 (`hard_split_long_sentence`) and Task 5 (`ids_truncated_to_510`).
4. **Pressing the hotkey again mid-read** (jump): old audio stops instantly, old synthesis results are discarded (generation counter), new read starts. → test in Task 8 (`jump_bumps_generation`).
5. **End of document / source window closed:** reading ends in Idle with both windows destroyed and hooks removed. → test in Task 8 (`every_error_ends_idle`).

---

## Execution waves (for parallel subagents)

| Wave | Tasks (parallel inside a wave) | Depends on |
|---|---|---|
| A | 1 (deps + model + ort smoke), 2 (text), 3 (timing/align), 4 (WSOLA), 6 (player), 7 (UIA) | — |
| B | 5 (tts engine), 9 (overlay + pill frontend), 10 (settings UI) | A |
| C | 8 (read_aloud wiring) | A, B |
| D | 11 (manual matrix, resource check, session log) | C |

Each task touches its own new file, so Wave A/B tasks do not conflict. Only Tasks 1 and 8 edit `main.rs`/`Cargo.toml`; Task 1 lands first in Wave A (others rebase onto it).

---

### Task 1: Dependencies, model prep script, ort DirectML smoke test

**Files:**
- Modify: `src-tauri/Cargo.toml` (deps, windows features)
- Create: `tools/read-aloud/prepare_model.py`
- Create: `tools/read-aloud/README.md`
- Create: `src-tauri/src/ra_model.rs` (model paths only)
- Modify: `src-tauri/src/main.rs:1-14` (mod lines)

**Interfaces:**
- Produces: `ra_model::model_dir(data_dir: &Path) -> PathBuf` (= `data_dir/models/kokoro`), `ra_model::Files { q8_dml: PathBuf, fp32: PathBuf, tokenizer: PathBuf, voices_dir: PathBuf }`, `ra_model::files(data_dir: &Path) -> Files`, `ra_model::is_installed(data_dir: &Path) -> bool` (q8_dml + tokenizer + `voices/af_heart.bin` exist).

- [ ] **Step 1: Add dependencies** to `src-tauri/Cargo.toml`:

```toml
[target.'cfg(windows)'.dependencies]
windows = { version = "0.58", features = [
    "Win32_Foundation",
    "Win32_Security",
    "Win32_System_Threading",
    "Win32_UI_Input_KeyboardAndMouse",
    "Win32_UI_WindowsAndMessaging",
    "Win32_UI_Accessibility",
    "Win32_System_Com",
    "Win32_System_Ole",
    "Win32_System_Variant",
    "Win32_Graphics_Gdi",
    "Win32_UI_HiDpi",
] }
ort = { version = "=2.0.0-rc.13", features = ["directml"] }
misaki-rs = { version = "0.6", default-features = false }
```

(`ort`/`misaki-rs` are Windows-only so macOS CI is untouched.)

- [ ] **Step 2: Write `tools/read-aloud/prepare_model.py`** (dev-time, personal build; downloads + patches into Echo's data dir):

```python
"""Download Kokoro-82M (timestamped ONNX) and patch it for DirectML.
Usage: py -3.11 -m pip install huggingface_hub onnx && py -3.11 tools/read-aloud/prepare_model.py
Writes to %APPDATA%/ALAN Echo/models/kokoro/."""
import os, sys, shutil
from huggingface_hub import hf_hub_download
import onnx
from onnx import helper

REPO = "onnx-community/Kokoro-82M-v1.0-ONNX-timestamped"
VOICES = ["af_heart", "am_michael", "bf_emma"]
dst = os.path.join(os.environ["APPDATA"], "ALAN Echo", "models", "kokoro")
os.makedirs(os.path.join(dst, "voices"), exist_ok=True)

def get(name):
    return hf_hub_download(REPO, name)

def patch(src, out):
    # DirectML fails on grouped ConvTranspose with output_padding.
    # Equivalent rewrite: drop output_padding, shrink the end pad by the same amount.
    m = onnx.load(src)
    n_patched = 0
    for n in m.graph.node:
        if n.op_type == "ConvTranspose":
            a = {x.name: x for x in n.attribute}
            if "output_padding" in a:
                op = list(a["output_padding"].ints); pads = list(a["pads"].ints)
                k = len(op); pads = pads[:k] + [pads[k + i] - op[i] for i in range(k)]
                n.attribute.remove(a["output_padding"]); n.attribute.remove(a["pads"])
                n.attribute.append(helper.make_attribute("pads", pads))
                n_patched += 1
    onnx.save(m, out)
    print(f"patched {n_patched} ConvTranspose nodes -> {out}")

patch(get("onnx/model_quantized.onnx"), os.path.join(dst, "model_q8_dml.onnx"))
shutil.copy(get("onnx/model.onnx"), os.path.join(dst, "model_fp32.onnx"))
shutil.copy(get("tokenizer.json"), os.path.join(dst, "tokenizer.json"))
for v in VOICES:
    shutil.copy(get(f"voices/{v}.bin"), os.path.join(dst, "voices", f"{v}.bin"))
print("done:", dst)
```

`tools/read-aloud/README.md`: 5 lines — what the script does, the command, licence (model Apache-2.0), and "re-run after wiping the data dir".

- [ ] **Step 3: Run it**

Run: `py -3.11 -m pip install huggingface_hub onnx && py -3.11 tools/read-aloud/prepare_model.py`
Expected: `patched N ConvTranspose nodes` with N ≥ 1, then `done: ...\ALAN Echo\models\kokoro`.

- [ ] **Step 4: Write `ra_model.rs` with a failing test, then implement**

```rust
//! Read Aloud — Kokoro model file locations (data dir, not bundled).
use std::path::{Path, PathBuf};

pub struct Files {
    pub q8_dml: PathBuf,
    pub fp32: PathBuf,
    pub tokenizer: PathBuf,
    pub voices_dir: PathBuf,
}

pub fn model_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("models").join("kokoro")
}

pub fn files(data_dir: &Path) -> Files {
    let d = model_dir(data_dir);
    Files {
        q8_dml: d.join("model_q8_dml.onnx"),
        fp32: d.join("model_fp32.onnx"),
        tokenizer: d.join("tokenizer.json"),
        voices_dir: d.join("voices"),
    }
}

pub fn is_installed(data_dir: &Path) -> bool {
    let f = files(data_dir);
    f.q8_dml.exists() && f.tokenizer.exists() && f.voices_dir.join("af_heart.bin").exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_live_under_models_kokoro() {
        let f = files(Path::new("C:/x"));
        assert!(f.q8_dml.ends_with("models/kokoro/model_q8_dml.onnx"));
        assert!(f.voices_dir.ends_with("models/kokoro/voices"));
    }

    #[test]
    fn not_installed_in_empty_dir() {
        let d = std::env::temp_dir().join(format!("ra-empty-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        assert!(!is_installed(&d));
    }

    /// Smoke: DirectML session loads the patched model and runs one call.
    /// Ignored by default (needs the prepared model). Run with:
    /// cargo test ra_model::tests::ort_dml_smoke -- --ignored --nocapture
    #[test]
    #[ignore]
    fn ort_dml_smoke() {
        use ort::{execution_providers::DirectMLExecutionProvider, session::Session, value::Tensor};
        let data = dirs::data_dir().unwrap().join("ALAN Echo");
        let f = files(&data);
        let mut s = Session::builder().unwrap()
            .with_execution_providers([DirectMLExecutionProvider::default().build().error_on_failure()]).unwrap()
            .with_memory_pattern(false).unwrap()
            .with_parallel_execution(false).unwrap()
            .commit_from_file(&f.q8_dml).unwrap();
        for o in s.outputs.iter() { println!("output: {}", o.name); }
        for i in s.inputs.iter() { println!("input: {}", i.name); }
        let ids: Vec<i64> = vec![0, 50, 83, 54, 156, 0];
        let style = vec![0.0f32; 256];
        let out = s.run(ort::inputs![
            "input_ids" => Tensor::from_array(([1usize, ids.len()], ids)).unwrap(),
            "style" => Tensor::from_array(([1usize, 256usize], style)).unwrap(),
            "speed" => Tensor::from_array(([1usize], vec![1.0f32])).unwrap(),
        ]).unwrap();
        assert!(out.len() >= 2, "expected waveform + durations");
    }
}
```

Add to `main.rs` after the existing `mod` lines:

```rust
#[cfg(target_os = "windows")]
mod ra_model;
```

- [ ] **Step 5: Run tests**

Run: `cd src-tauri && cargo test ra_model -- --nocapture` → 2 pass.
Run: `cargo test ra_model::tests::ort_dml_smoke -- --ignored --nocapture` → PASS; **record the printed input/output names** in `tools/read-aloud/README.md` (Task 5 uses them; expected `waveform` and `durations`). If the ort API differs in rc.13, fix the call shape here and copy the working shape into Task 5.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/ra_model.rs src-tauri/src/main.rs tools/read-aloud
git commit -m "feat(read-aloud): deps, model prep script, DirectML smoke test"
```

---

### Task 2: `ra_text.rs` — normalize UIA tokens, merge hyphens, chunk sentences

**Files:**
- Create: `src-tauri/src/ra_text.rs`
- Modify: `src-tauri/src/main.rs` (add `#[cfg(target_os = "windows")] mod ra_text;`)

**Interfaces:**
- Consumes: raw UIA word tokens as `&[String]` (text exactly as `GetText` returned it, including trailing whitespace / `\r`).
- Produces:
  - `pub struct Word { pub text: String, pub src: Vec<usize>, pub para_break_before: bool }` — `src` = indices into the raw token list (for highlight rects).
  - `pub fn normalize(raw: &[String]) -> Vec<Word>`
  - `pub fn chunk(words: &[Word], first_max: usize, max: usize) -> Vec<std::ops::Range<usize>>`

- [ ] **Step 1: Write failing tests** (bottom of `ra_text.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    fn raw(v: &[&str]) -> Vec<String> { v.iter().map(|s| s.to_string()).collect() }
    fn texts(w: &[Word]) -> Vec<&str> { w.iter().map(|x| x.text.as_str()).collect() }

    #[test]
    fn plain_words_trim_whitespace() {
        let w = normalize(&raw(&["The ", "rate ", "rose"]));
        assert_eq!(texts(&w), vec!["The", "rate", "rose"]);
        assert_eq!(w[1].src, vec![1]);
    }

    #[test]
    fn html_three_token_hyphen_merges() {
        let w = normalize(&raw(&["a ", "hyphen", "-", "ated ", "word"]));
        assert_eq!(texts(&w), vec!["a", "hyphen-ated", "word"]);
        assert_eq!(w[1].src, vec![1, 2, 3]);
    }

    #[test]
    fn pdf_line_end_hyphen_merges() {
        let w = normalize(&raw(&["a ", "hyphen-", "ated ", "word"]));
        assert_eq!(texts(&w), vec!["a", "hyphen-ated", "word"]);
        assert_eq!(w[1].src, vec![1, 2]);
    }

    #[test]
    fn spaced_dash_is_not_merged() {
        let w = normalize(&raw(&["yes ", "- ", "no"]));
        assert_eq!(texts(&w), vec!["yes", "-", "no"]);
    }

    #[test]
    fn punctuation_attaches_to_previous_word() {
        let w = normalize(&raw(&["Hello", ", ", "world", ". "]));
        assert_eq!(texts(&w), vec!["Hello,", "world."]);
        assert_eq!(w[0].src, vec![0]);
    }

    #[test]
    fn carriage_return_marks_paragraph() {
        let w = normalize(&raw(&["end.\r", "Next "]));
        assert_eq!(texts(&w), vec!["end.", "Next"]);
        assert!(w[1].para_break_before);
    }

    #[test]
    fn empty_tokens_skipped() {
        let w = normalize(&raw(&["  ", "\r", "word"]));
        assert_eq!(texts(&w), vec!["word"]);
    }

    fn words(s: &str) -> Vec<Word> {
        s.split(' ').enumerate().map(|(i, t)| Word { text: t.into(), src: vec![i], para_break_before: false }).collect()
    }

    #[test]
    fn splits_on_sentence_end() {
        let w = words("One two. Three four. Five.");
        assert_eq!(chunk(&w, 12, 50), vec![0..2, 2..4, 4..5]);
    }

    #[test]
    fn abbreviations_do_not_split() {
        let w = words("Dr. Smith met the U.S. team. Done.");
        assert_eq!(chunk(&w, 12, 50), vec![0..6, 6..7]);
    }

    #[test]
    fn first_chunk_cut_at_clause_within_12() {
        let w = words("Eight times a year, twelve people sit around a long table in Washington and decide.");
        let c = chunk(&w, 12, 50);
        assert_eq!(c[0], 0..4); // "Eight times a year,"
        assert_eq!(c[1].start, 4);
    }

    #[test]
    fn first_chunk_hard_cut_without_clause() {
        let s = (0..20).map(|i| format!("w{i}")).collect::<Vec<_>>().join(" ");
        let c = chunk(&words(&s), 12, 50);
        assert_eq!(c[0], 0..12);
    }

    #[test]
    fn hard_split_long_sentence() {
        let s = (0..130).map(|i| format!("w{i}")).collect::<Vec<_>>().join(" ");
        let c = chunk(&words(&s), 12, 50);
        assert!(c.iter().all(|r| r.len() <= 50));
        assert_eq!(c.last().unwrap().end, 130);
    }

    #[test]
    fn paragraph_break_splits() {
        let mut w = words("no period here next para");
        w[3].para_break_before = true;
        assert_eq!(chunk(&w, 12, 50), vec![0..3, 3..5]);
    }

    #[test]
    fn empty_input_no_chunks() {
        assert!(chunk(&[], 12, 50).is_empty());
    }
}
```

- [ ] **Step 2: Run to verify fail**

Run: `cd src-tauri && cargo test ra_text` → FAIL (`normalize`/`chunk` not found).

- [ ] **Step 3: Implement** (top of `ra_text.rs`):

```rust
//! Read Aloud — turn raw UIA word tokens into speakable words and chunks.
use std::ops::Range;

#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub text: String,
    pub src: Vec<usize>,
    pub para_break_before: bool,
}

fn is_dash(s: &str) -> bool {
    matches!(s, "-" | "\u{2010}" | "\u{2011}")
}

fn has_alnum(s: &str) -> bool {
    s.chars().any(|c| c.is_alphanumeric())
}

pub fn normalize(raw: &[String]) -> Vec<Word> {
    let mut out: Vec<Word> = Vec::new();
    let mut pending_para = false;
    // true when the previous raw token ended without whitespace (glued to the next)
    let mut prev_glued = false;
    let mut i = 0;
    while i < raw.len() {
        let tok = &raw[i];
        let breaks = tok.contains('\r') || tok.contains('\n');
        let t = tok.trim();
        let glued = !tok.is_empty() && !tok.ends_with(char::is_whitespace);
        if t.is_empty() {
            pending_para |= breaks;
            prev_glued = false;
            i += 1;
            continue;
        }
        // HTML form: word, "-", word all glued -> one word
        if is_dash(t) && prev_glued && glued && i + 1 < raw.len() && has_alnum(raw[i + 1].trim()) {
            if let Some(last) = out.last_mut() {
                last.text.push('-');
                last.text.push_str(raw[i + 1].trim());
                last.src.push(i);
                last.src.push(i + 1);
                let next = &raw[i + 1];
                pending_para |= next.contains('\r') || next.contains('\n');
                prev_glued = !next.ends_with(char::is_whitespace);
                i += 2;
                continue;
            }
        }
        // PDF form: "hyphen-" at line end (no trailing space), then "ated"
        if let Some(last) = out.last_mut() {
            if last.text.ends_with('-') && last.text.len() > 1 && prev_glued && has_alnum(t) && !breaks_before(raw, i) {
                last.text.push_str(t);
                last.src.push(i);
                pending_para |= breaks;
                prev_glued = glued;
                i += 1;
                continue;
            }
            // punctuation-only token glues onto the previous word (text only)
            if !has_alnum(t) && !is_dash(t) && !pending_para {
                last.text.push_str(t);
                pending_para |= breaks;
                prev_glued = glued;
                i += 1;
                continue;
            }
        }
        out.push(Word { text: t.to_string(), src: vec![i], para_break_before: pending_para && !out.is_empty() });
        pending_para = breaks;
        prev_glued = glued;
        i += 1;
    }
    out
}

fn breaks_before(raw: &[String], i: usize) -> bool {
    i > 0 && (raw[i - 1].contains('\r') || raw[i - 1].contains('\n'))
}

const ABBREV: &[&str] = &[
    "dr.", "mr.", "mrs.", "ms.", "st.", "vs.", "jr.", "sr.", "prof.", "inc.", "co.", "e.g.", "i.e.", "etc.", "no.", "fig.", "u.s.", "u.k.",
];

fn ends_sentence(w: &str) -> bool {
    let core = w.trim_end_matches(|c| matches!(c, '"' | '\'' | ')' | ']' | '\u{201D}' | '\u{2019}'));
    if !core.ends_with(['.', '!', '?']) {
        return false;
    }
    let lower = core.to_lowercase();
    if ABBREV.contains(&lower.as_str()) {
        return false;
    }
    // single capital initial like "A." or "J."
    let letters: Vec<char> = core.chars().filter(|c| c.is_alphabetic()).collect();
    !(letters.len() == 1 && letters[0].is_uppercase() && core.len() == 2)
}

fn ends_clause(w: &str) -> bool {
    w.ends_with([',', ';', ':'])
}

pub fn chunk(words: &[Word], first_max: usize, max: usize) -> Vec<Range<usize>> {
    // 1) sentences
    let mut sentences: Vec<Range<usize>> = Vec::new();
    let mut start = 0;
    for (i, w) in words.iter().enumerate() {
        if i > start && w.para_break_before {
            sentences.push(start..i);
            start = i;
        }
        if ends_sentence(&w.text) {
            sentences.push(start..i + 1);
            start = i + 1;
        }
    }
    if start < words.len() {
        sentences.push(start..words.len());
    }
    // 2) enforce limits (first chunk smaller, cut at a clause when possible)
    let mut out = Vec::new();
    for s in sentences {
        let mut a = s.start;
        while a < s.end {
            let limit = if out.is_empty() { first_max } else { max };
            if s.end - a <= limit {
                out.push(a..s.end);
                break;
            }
            let hard = a + limit;
            let cut = (a + 1..hard).rev().find(|&k| ends_clause(&words[k - 1].text)).unwrap_or(hard);
            out.push(a..cut);
            a = cut;
        }
    }
    out
}
```

- [ ] **Step 4: Run tests** → `cargo test ra_text` all PASS. If `first_chunk_cut_at_clause_within_12` picks a later comma, keep the rule "last clause boundary ≤ limit" (as coded) and fix the test expectation only if the sentence has a later comma inside 12 words.

- [ ] **Step 5: Commit** `git commit -am "feat(read-aloud): text normalization, hyphen merge, sentence chunking"` (add the new file first).

---

### Task 3: `ra_timing.rs` — durations → word times, token ↔ word alignment

**Files:**
- Create: `src-tauri/src/ra_timing.rs`
- Modify: `src-tauri/src/main.rs` (mod line, cfg windows)

**Interfaces:**
- Produces:
  - `pub const SR: u32 = 24_000;`
  - `pub struct WordTime { pub word: usize, pub start: u64, pub end: u64 }` — samples at 24 kHz, relative to chunk start; `word` = index into the chunk's words.
  - `pub fn word_times(durations: &[f32], spans: &[(usize, usize, usize)], n_samples: usize, offset_ms: f32, rate: f32) -> Vec<WordTime>` — `spans` = `(word_idx, first_id, end_id)` over ids **without** the leading/trailing pad; `rate` = stretch factor (2.2 → output is 2.2x shorter).
  - `pub fn align(words: &[&str], tokens: &[&str]) -> Vec<Option<usize>>` — which chunk word each misaki token belongs to.

- [ ] **Step 1: Failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_from_cumulative_frames() {
        // pad(1) + w0 ids(2) + space(1) + w1 ids(2) + pad(1) = 7 durations
        let d = [1.0, 2.0, 2.0, 1.0, 3.0, 1.0, 1.0];
        let spans = [(0, 0, 2), (1, 3, 5)];
        let n = 11 * 600; // sum of frames = 11
        let t = word_times(&d, &spans, n, 0.0, 1.0);
        assert_eq!(t[0].start, 600);          // after the 1-frame pad
        assert_eq!(t[1].start, (1 + 2 + 2 + 1) * 600);
        assert_eq!(t[0].end, t[1].start);     // end = next start
        assert_eq!(t[1].end, n as u64);
    }

    #[test]
    fn offset_and_rate_scale() {
        let d = [1.0, 2.0, 2.0, 1.0, 3.0, 1.0, 1.0];
        let spans = [(0, 0, 2), (1, 3, 5)];
        let n = 11 * 600;
        let t = word_times(&d, &spans, n, 36.0, 2.0);
        let off = (0.036 * SR as f32) as u64; // 864
        assert_eq!(t[1].start, ((6 * 600) as u64 - off) / 2);
        assert_eq!(t[1].end, (n / 2) as u64);
    }

    #[test]
    fn rounds_and_clamps_durations() {
        let d = [0.2, 1.6, 0.4, 0.0];
        let spans = [(0, 0, 2)];
        let t = word_times(&d, &spans, 5 * 600, 0.0, 1.0); // frames: 1,2,1,1 = 5
        assert_eq!(t[0].start, 600);
    }

    #[test]
    fn monotonic_never_negative() {
        let d = [1.0, 1.0, 1.0, 1.0];
        let spans = [(0, 0, 1), (1, 1, 2)];
        let t = word_times(&d, &spans, 4 * 600, 500.0, 1.0); // huge offset
        assert_eq!(t[0].start, 0);
        assert!(t[1].start >= t[0].start);
    }

    #[test]
    fn align_simple() {
        let a = align(&["Hello,", "world."], &["Hello", ",", "world", "."]);
        assert_eq!(a, vec![Some(0), None, Some(1), None]);
    }

    #[test]
    fn align_splits_hyphenated() {
        let a = align(&["a", "hyphen-ated", "word"], &["a", "hyphen", "-", "ated", "word"]);
        assert_eq!(a, vec![Some(0), Some(1), None, Some(1), Some(2)]);
    }

    #[test]
    fn align_skips_punctuation_only_tokens() {
        let a = align(&["—", "next"], &["—", "next"]);
        assert_eq!(a, vec![None, Some(1)]);
    }

    #[test]
    fn align_recovers_after_mismatch() {
        // misaki may rewrite numbers; the next real word must still align
        let a = align(&["costs", "$5", "today"], &["costs", "five dollars", "today"]);
        assert_eq!(a[0], Some(0));
        assert_eq!(a[2], Some(2));
    }
}
```

- [ ] **Step 2: Run** `cargo test ra_timing` → FAIL.

- [ ] **Step 3: Implement**

```rust
//! Read Aloud — Kokoro phoneme durations -> per-word sample times.
pub const SR: u32 = 24_000;
const SAMPLES_PER_FRAME: f64 = 600.0;

#[derive(Debug, Clone, PartialEq)]
pub struct WordTime {
    pub word: usize,
    pub start: u64,
    pub end: u64,
}

pub fn word_times(durations: &[f32], spans: &[(usize, usize, usize)], n_samples: usize, offset_ms: f32, rate: f32) -> Vec<WordTime> {
    let frames: Vec<f64> = durations.iter().map(|d| (d.round() as f64).max(1.0)).collect();
    let total: f64 = frames.iter().sum();
    let spf = if total > 0.0 { n_samples as f64 / total } else { SAMPLES_PER_FRAME };
    let mut cum = Vec::with_capacity(frames.len() + 1);
    cum.push(0.0);
    for f in &frames {
        cum.push(cum.last().unwrap() + f);
    }
    let off = offset_ms as f64 / 1000.0 * SR as f64;
    let rate = rate.max(0.01) as f64;
    let mut starts: Vec<(usize, u64)> = Vec::new();
    let mut floor = 0u64;
    for &(w, s, e) in spans {
        if e <= s || s + 1 >= cum.len() {
            continue;
        }
        let raw = (cum[s + 1] * spf - off).max(0.0) / rate; // +1 skips the leading pad
        let v = (raw as u64).max(floor);
        floor = v;
        starts.push((w, v));
    }
    let end_all = (n_samples as f64 / rate) as u64;
    (0..starts.len())
        .map(|k| WordTime {
            word: starts[k].0,
            start: starts[k].1,
            end: if k + 1 < starts.len() { starts[k + 1].1 } else { end_all },
        })
        .collect()
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

pub fn align(words: &[&str], tokens: &[&str]) -> Vec<Option<usize>> {
    let wn: Vec<String> = words.iter().map(|w| norm(w)).collect();
    let mut wi = 0usize;
    let mut off = 0usize;
    let mut out = Vec::with_capacity(tokens.len());
    for t in tokens {
        let tn = norm(t);
        if tn.is_empty() {
            out.push(None);
            continue;
        }
        // skip words with nothing speakable
        while wi < wn.len() && wn[wi].is_empty() {
            wi += 1;
            off = 0;
        }
        if wi >= wn.len() {
            out.push(None);
            continue;
        }
        if wn[wi][off..].starts_with(&tn) {
            out.push(Some(wi));
            off += tn.len();
            if off >= wn[wi].len() {
                wi += 1;
                off = 0;
            }
            continue;
        }
        // look ahead up to 3 words for a fresh match
        if let Some(j) = (wi + 1..(wi + 4).min(wn.len())).find(|&j| wn[j].starts_with(&tn)) {
            out.push(Some(j));
            wi = j;
            off = tn.len();
            if off >= wn[wi].len() {
                wi += 1;
                off = 0;
            }
            continue;
        }
        // unmatched (e.g. "$5" -> "five dollars"): credit the current word, move on
        out.push(Some(wi));
        wi += 1;
        off = 0;
    }
    out
}
```

- [ ] **Step 4: Run** `cargo test ra_timing` → PASS.
- [ ] **Step 5: Commit** `feat(read-aloud): phoneme-duration word timing and token alignment`.

---

### Task 4: `ra_wsola.rs` — pitch-preserving time-stretch

**Files:** Create `src-tauri/src/ra_wsola.rs`; mod line in `main.rs` (cfg windows).

**Interfaces:** Produces `pub fn stretch(input: &[f32], rate: f32, sr: u32) -> Vec<f32>` (rate 2.2 → output ≈ len/2.2; rate ≤ 1.0001 returns a copy).

- [ ] **Step 1: Failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    fn sine(f: f32, secs: f32, sr: u32) -> Vec<f32> {
        (0..(secs * sr as f32) as usize).map(|i| (2.0 * std::f32::consts::PI * f * i as f32 / sr as f32).sin() * 0.5).collect()
    }
    fn zero_crossings(x: &[f32]) -> usize {
        x.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count()
    }

    #[test]
    fn length_scales_with_rate() {
        let x = sine(220.0, 2.0, 24_000);
        let y = stretch(&x, 2.2, 24_000);
        let expect = x.len() as f32 / 2.2;
        assert!((y.len() as f32 - expect).abs() / expect < 0.03, "{} vs {}", y.len(), expect);
    }

    #[test]
    fn pitch_preserved() {
        let x = sine(440.0, 2.0, 24_000);
        let y = stretch(&x, 2.0, 24_000);
        let fx = zero_crossings(&x) as f32 / (x.len() as f32 / 24_000.0);
        let fy = zero_crossings(&y) as f32 / (y.len() as f32 / 24_000.0);
        assert!((fx - fy).abs() / fx < 0.05, "{fx} vs {fy}");
    }

    #[test]
    fn rate_one_is_identity() {
        let x = sine(300.0, 0.5, 24_000);
        assert_eq!(stretch(&x, 1.0, 24_000), x);
    }

    #[test]
    fn tiny_and_empty_inputs_safe() {
        assert!(stretch(&[], 2.2, 24_000).is_empty());
        let y = stretch(&[0.1; 100], 2.2, 24_000);
        assert!(y.len() <= 100);
    }

    #[test]
    fn no_clipping() {
        let x = sine(200.0, 1.0, 24_000);
        assert!(stretch(&x, 2.2, 24_000).iter().all(|v| v.abs() <= 0.51));
    }
}
```

- [ ] **Step 2: Run** `cargo test ra_wsola` → FAIL.

- [ ] **Step 3: Implement**

```rust
//! Read Aloud — WSOLA time-stretch (speeds speech up without raising pitch).
pub fn stretch(input: &[f32], rate: f32, sr: u32) -> Vec<f32> {
    if rate <= 1.0001 {
        return input.to_vec();
    }
    let n = (sr as f32 * 0.030) as usize; // 30 ms window
    let hs = n / 2; // synthesis hop
    let ha = (hs as f32 * rate) as usize; // analysis hop
    let tol = (sr as f32 * 0.008) as usize; // ±8 ms search
    if input.len() < n + tol * 2 {
        let out_len = (input.len() as f32 / rate) as usize;
        return input.iter().step_by(rate.ceil() as usize).take(out_len).copied().collect();
    }
    let win: Vec<f32> = (0..n).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos()).collect();
    let out_len = (input.len() as f32 / rate) as usize;
    let mut out = vec![0.0f32; out_len + n];
    let mut norm = vec![0.0f32; out_len + n];
    let mut prev: usize = 0; // input position used for the previous frame
    let mut k = 0usize;
    loop {
        let out_pos = k * hs;
        let nominal = k * ha;
        if nominal + n + tol >= input.len() || out_pos + n > out.len() {
            break;
        }
        let pos = if k == 0 {
            0
        } else {
            // best match to the natural continuation of the previous frame
            let target = prev + hs;
            let lo = nominal.saturating_sub(tol);
            let hi = (nominal + tol).min(input.len() - n);
            let mut best = nominal.min(hi);
            let mut best_c = f32::MIN;
            let step = 2;
            let mut c = lo;
            while c <= hi {
                let mut s = 0.0f32;
                let mut i = 0;
                while i < n && target + i < input.len() {
                    s += input[c + i] * input[target + i];
                    i += 4;
                }
                if s > best_c {
                    best_c = s;
                    best = c;
                }
                c += step;
            }
            best
        };
        for i in 0..n {
            out[out_pos + i] += input[pos + i] * win[i];
            norm[out_pos + i] += win[i];
        }
        prev = pos;
        k += 1;
    }
    for i in 0..out.len() {
        if norm[i] > 1e-3 {
            out[i] /= norm[i];
        }
    }
    out.truncate(out_len);
    out
}
```

- [ ] **Step 4: Run** `cargo test ra_wsola` → PASS. Also A/B by ear: add an `#[ignore]` test `wsola_wav_out` that reads `ref_1.0x.wav` via `hound` from the spike folder path in an env var `RA_REF_WAV` and writes `target/ra_wsola_2.2.wav`; listen once against spike file `B_s1.0_wsola2.2.wav`.
- [ ] **Step 5: Commit** `feat(read-aloud): WSOLA time-stretch`.

---

### Task 5: `ra_tts.rs` — Kokoro engine (lazy load, DML→CPU, idle unload)

**Files:** Create `src-tauri/src/ra_tts.rs`; mod line in `main.rs` (cfg windows).

**Interfaces:**
- Consumes: `ra_model::files`, `ra_timing::{word_times, align, WordTime, SR}`, `ra_wsola::stretch`, `misaki_rs::{G2P, Language}`.
- Produces:
  - `pub enum Device { Gpu, Cpu }`
  - `pub struct ChunkAudio { pub pcm: Vec<f32>, pub words: Vec<WordTime>, pub device: Device }` (24 kHz mono, already stretched)
  - `pub struct Engine` with `pub fn new(data_dir: PathBuf) -> Arc<Engine>`, `pub fn preload(self: &Arc<Self>, voice: &str, pref: &str)` (spawns a thread; returns immediately), `pub fn synth(&self, words: &[&str], rate: f32) -> Result<ChunkAudio, String>` (blocks until loaded), `pub fn device(&self) -> Option<Device>`, `pub fn touch(&self)`, `pub fn is_loaded(&self) -> bool`.
  - `pub fn should_unload(last_used: Instant, now: Instant, busy: bool) -> bool` (pure; 120 s).
  - `pub fn build_ids(phonemes: &[(usize, String, bool)], vocab: &HashMap<char, i64>) -> (Vec<i64>, Vec<(usize, usize, usize)>)` (pure; caps at 510 ids).

- [ ] **Step 1: Failing tests** (pure parts only; the model test is `#[ignore]`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn vocab() -> HashMap<char, i64> {
        [('h', 1), ('ə', 2), ('l', 3), ('o', 4), (' ', 16)].into_iter().collect()
    }

    #[test]
    fn ids_and_spans() {
        let (ids, spans) = build_ids(&[(0, "həl".into(), true), (1, "lo".into(), false)], &vocab());
        assert_eq!(ids, vec![1, 2, 3, 16, 3, 4]);
        assert_eq!(spans, vec![(0, 0, 3), (1, 4, 6)]);
    }

    #[test]
    fn unknown_phoneme_chars_dropped() {
        let (ids, _) = build_ids(&[(0, "hxz".into(), false)], &vocab());
        assert_eq!(ids, vec![1]);
    }

    #[test]
    fn ids_truncated_to_510() {
        let long: String = std::iter::repeat('h').take(600).collect();
        let (ids, spans) = build_ids(&[(0, long, false)], &vocab());
        assert_eq!(ids.len(), 510);
        assert_eq!(spans[0].2, 510);
    }

    #[test]
    fn unload_after_two_idle_minutes() {
        let t0 = Instant::now();
        assert!(!should_unload(t0, t0 + Duration::from_secs(119), false));
        assert!(should_unload(t0, t0 + Duration::from_secs(121), false));
        assert!(!should_unload(t0, t0 + Duration::from_secs(600), true));
    }

    /// cargo test ra_tts::tests::synth_real -- --ignored --nocapture
    #[test]
    #[ignore]
    fn synth_real() {
        let e = Engine::new(dirs::data_dir().unwrap().join("ALAN Echo"));
        e.preload("af_heart", "auto");
        let t = Instant::now();
        let a = e.synth(&["Eight", "times", "a", "year,"], 2.2).unwrap();
        println!("first chunk {:?} on {:?}, {} samples, words {:?}", t.elapsed(), e.device(), a.pcm.len(), a.words);
        assert_eq!(a.words.len(), 4);
        assert!(a.words.windows(2).all(|w| w[0].start <= w[1].start));
    }
}
```

- [ ] **Step 2: Run** `cargo test ra_tts` → FAIL.

- [ ] **Step 3: Implement**

```rust
//! Read Aloud — Kokoro-82M via ONNX Runtime. Loaded on demand (hotkey press),
//! DirectML first with CPU fallback, dropped after 2 idle minutes.
use crate::{ra_model, ra_timing::{self, WordTime}, ra_wsola};
use misaki_rs::{G2P, Language};
use ort::{execution_providers::DirectMLExecutionProvider, session::Session, value::Tensor};
use parking_lot::{Condvar, Mutex};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

const IDLE: Duration = Duration::from_secs(120);
const MAX_IDS: usize = 510;
const OFFSET_MS: f32 = 36.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Device { Gpu, Cpu }

pub struct ChunkAudio {
    pub pcm: Vec<f32>,
    pub words: Vec<WordTime>,
    pub device: Device,
}

struct Loaded {
    session: Session,
    device: Device,
    vocab: HashMap<char, i64>,
    voice: Vec<f32>, // rows of 256
    g2p: G2P,
}

enum Slot { Empty, Loading, Ready(Box<Loaded>), Failed(String) }

pub struct Engine {
    data_dir: PathBuf,
    slot: Mutex<Slot>,
    ready: Condvar,
    last_used: Mutex<Instant>,
    busy: Mutex<u32>,
}

pub fn should_unload(last_used: Instant, now: Instant, busy: bool) -> bool {
    !busy && now.duration_since(last_used) > IDLE
}

pub fn build_ids(ph: &[(usize, String, bool)], vocab: &HashMap<char, i64>) -> (Vec<i64>, Vec<(usize, usize, usize)>) {
    let mut ids = Vec::new();
    let mut spans = Vec::new();
    let space = vocab.get(&' ').copied();
    for (w, p, ws) in ph {
        if ids.len() >= MAX_IDS { break; }
        let s = ids.len();
        for c in p.chars() {
            if ids.len() >= MAX_IDS { break; }
            if let Some(&id) = vocab.get(&c) { ids.push(id); }
        }
        if ids.len() > s { spans.push((*w, s, ids.len())); }
        if *ws && ids.len() < MAX_IDS { if let Some(sp) = space { ids.push(sp); } }
    }
    (ids, spans)
}

fn load_vocab(path: &std::path::Path) -> Result<HashMap<char, i64>, String> {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let map = v["model"]["vocab"].as_object().ok_or("tokenizer.json: no model.vocab")?;
    Ok(map.iter().filter_map(|(k, v)| Some((k.chars().next()?, v.as_i64()?))).collect())
}

fn load_voice(path: &std::path::Path) -> Result<Vec<f32>, String> {
    let b = std::fs::read(path).map_err(|e| format!("voice: {e}"))?;
    Ok(b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
}

fn open_session(path: &std::path::Path, gpu: bool) -> Result<Session, String> {
    let b = Session::builder().map_err(|e| e.to_string())?;
    let b = if gpu {
        b.with_execution_providers([DirectMLExecutionProvider::default().build().error_on_failure()])
            .and_then(|b| b.with_memory_pattern(false))
            .and_then(|b| b.with_parallel_execution(false))
            .map_err(|e| e.to_string())?
    } else {
        b.with_intra_threads(num_cpus::get_physical().min(8)).map_err(|e| e.to_string())?
    };
    b.commit_from_file(path).map_err(|e| e.to_string())
}

impl Engine {
    pub fn new(data_dir: PathBuf) -> Arc<Self> {
        Arc::new(Self { data_dir, slot: Mutex::new(Slot::Empty), ready: Condvar::new(), last_used: Mutex::new(Instant::now()), busy: Mutex::new(0) })
    }

    pub fn is_loaded(&self) -> bool { matches!(*self.slot.lock(), Slot::Ready(_)) }

    pub fn device(&self) -> Option<Device> {
        if let Slot::Ready(l) = &*self.slot.lock() { Some(l.device) } else { None }
    }

    pub fn touch(&self) { *self.last_used.lock() = Instant::now(); }

    /// Non-blocking: start loading if not loaded. `pref` = "auto" | "gpu" | "cpu".
    pub fn preload(self: &Arc<Self>, voice: &str, pref: &str) {
        self.touch();
        {
            let mut s = self.slot.lock();
            if matches!(*s, Slot::Ready(_) | Slot::Loading) { return; }
            *s = Slot::Loading;
        }
        let me = Arc::clone(self);
        let voice = voice.to_string();
        let pref = pref.to_string();
        std::thread::Builder::new().name("ra-tts-load".into()).spawn(move || {
            let r = me.load(&voice, &pref);
            let mut s = me.slot.lock();
            *s = match r { Ok(l) => Slot::Ready(Box::new(l)), Err(e) => { log::warn!("read-aloud: engine load failed: {e}"); Slot::Failed(e) } };
            me.ready.notify_all();
            drop(s);
            me.spawn_idle_watch();
        }).ok();
    }

    fn load(&self, voice: &str, pref: &str) -> Result<Loaded, String> {
        let f = ra_model::files(&self.data_dir);
        let vocab = load_vocab(&f.tokenizer)?;
        let voice = load_voice(&f.voices_dir.join(format!("{voice}.bin")))
            .or_else(|_| load_voice(&f.voices_dir.join("af_heart.bin")))?;
        let g2p = G2P::new(Language::EnglishUS);
        let t = Instant::now();
        let (session, device) = match pref {
            "cpu" => (open_session(&f.fp32, false)?, Device::Cpu),
            _ => match open_session(&f.q8_dml, true) {
                Ok(s) => (s, Device::Gpu),
                Err(e) if pref != "gpu" => {
                    log::warn!("read-aloud: DirectML failed ({e}); using CPU");
                    (open_session(&f.fp32, false)?, Device::Cpu)
                }
                Err(e) => return Err(e),
            },
        };
        log::info!("read-aloud: engine loaded on {:?} in {:?}", device, t.elapsed());
        Ok(Loaded { session, device, vocab, voice, g2p })
    }

    fn spawn_idle_watch(self: &Arc<Self>) {
        let me = Arc::clone(self);
        std::thread::Builder::new().name("ra-tts-idle".into()).spawn(move || loop {
            std::thread::sleep(Duration::from_secs(15));
            let last = *me.last_used.lock();
            let busy = *me.busy.lock() > 0;
            if should_unload(last, Instant::now(), busy) {
                *me.slot.lock() = Slot::Empty;
                log::info!("read-aloud: engine unloaded after idle");
                return;
            }
            if !me.is_loaded() { return; }
        }).ok();
    }

    /// Blocks until loaded (or failed). Synthesizes one chunk at 1.0x, then stretches to `rate`.
    pub fn synth(&self, words: &[&str], rate: f32) -> Result<ChunkAudio, String> {
        *self.busy.lock() += 1;
        let r = self.synth_inner(words, rate);
        *self.busy.lock() -= 1;
        self.touch();
        r
    }

    fn synth_inner(&self, words: &[&str], rate: f32) -> Result<ChunkAudio, String> {
        let mut s = self.slot.lock();
        while matches!(*s, Slot::Loading) { self.ready.wait(&mut s); }
        let l = match &mut *s {
            Slot::Ready(l) => l,
            Slot::Failed(e) => return Err(e.clone()),
            Slot::Empty => return Err("engine not loaded".into()),
            Slot::Loading => unreachable!(),
        };
        let text = words.join(" ");
        let (_, tokens) = l.g2p.g2p(&text).map_err(|e| format!("g2p: {e:?}"))?;
        let tok_text: Vec<&str> = tokens.iter().map(|t| t.text.as_str()).collect();
        let owner = ra_timing::align(words, &tok_text);
        // one entry per token that has phonemes and belongs to a word
        let ph: Vec<(usize, String, bool)> = tokens.iter().zip(owner.iter())
            .filter_map(|(t, o)| Some((o.as_ref().copied()?, t.phonemes.clone()?, !t.whitespace.is_empty())))
            .collect();
        let (ids, spans) = build_ids(&ph, &l.vocab);
        if ids.is_empty() {
            return Ok(ChunkAudio { pcm: Vec::new(), words: Vec::new(), device: l.device });
        }
        let row = ids.len().min(l.voice.len() / 256 - 1);
        let style = l.voice[row * 256..row * 256 + 256].to_vec();
        let mut input = Vec::with_capacity(ids.len() + 2);
        input.push(0i64);
        input.extend_from_slice(&ids);
        input.push(0);
        let n_in = input.len();
        let out = l.session.run(ort::inputs![
            "input_ids" => Tensor::from_array(([1usize, n_in], input)).map_err(|e| e.to_string())?,
            "style" => Tensor::from_array(([1usize, 256usize], style)).map_err(|e| e.to_string())?,
            "speed" => Tensor::from_array(([1usize], vec![1.0f32])).map_err(|e| e.to_string())?,
        ]).map_err(|e| e.to_string())?;
        let (_, wav) = out["waveform"].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
        let (_, dur) = out["durations"].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
        let wav = wav.to_vec();
        let dur = dur.to_vec();
        // merge spans of tokens that belong to the same word (first start wins)
        let mut merged: Vec<(usize, usize, usize)> = Vec::new();
        for sp in spans {
            match merged.last_mut() { Some(m) if m.0 == sp.0 => m.2 = sp.2, _ => merged.push(sp) }
        }
        let words_t = ra_timing::word_times(&dur, &merged, wav.len(), OFFSET_MS, rate);
        let pcm = ra_wsola::stretch(&wav, rate, ra_timing::SR);
        Ok(ChunkAudio { pcm, words: words_t, device: l.device })
    }
}
```

Notes for the implementer:
- Use the input/output names Task 1 printed. If durations come back as `i64`, extract as `i64` and cast to `f32`.
- If `misaki_rs::G2P::g2p` returns tokens whose `phonemes` is `None` for an unknown word, that word simply gets no span (it is skipped by the highlight, audio still plays the rest). misaki-rs without espeak already spells unknown words letter-by-letter (README), so no extra fallback code is needed for phase 1; note any unknown-word mispronunciations during Task 11.
- On a DirectML **runtime** error inside `run`, return `Err("dml: …")`; Task 8 handles the one-time CPU rebuild by calling `engine.reset_to_cpu()` — add it:

```rust
    /// After a DirectML runtime error: drop the session and reload on CPU (blocking).
    pub fn reset_to_cpu(self: &Arc<Self>, voice: &str) {
        *self.slot.lock() = Slot::Empty;
        self.preload(voice, "cpu");
    }
```

- [ ] **Step 4: Run** `cargo test ra_tts` → 4 PASS. Then `cargo test ra_tts::tests::synth_real -- --ignored --nocapture` → PASS, prints `Gpu` and first-chunk time (record it; target ≤ 2.5 s cold incl. load).
- [ ] **Step 5: Commit** `feat(read-aloud): Kokoro engine with DirectML, CPU fallback, idle unload`.

---

### Task 6: `ra_player.rs` — streaming audio out with a sample-accurate position

**Files:** Create `src-tauri/src/ra_player.rs`; mod line (cfg windows).

**Interfaces:**
- Produces: `pub struct Player` with `pub fn start() -> Result<Player, String>`, `pub fn push(&self, pcm: &[f32])`, `pub fn clear(&self)` (drops queued audio, position resets to 0), `pub fn set_paused(&self, p: bool)`, `pub fn is_paused(&self) -> bool`, `pub fn position(&self) -> u64` (24 kHz samples played since last clear), `pub fn queued_end(&self) -> u64` (24 kHz samples pushed since last clear), `pub fn failed(&self) -> bool` (device error). Dropping `Player` stops the stream thread.
- Pure helper: `pub fn mix_into(out: &mut [f32], channels: usize, step: f64, src: &mut Source) -> u64` (returns 24 kHz samples consumed).

- [ ] **Step 1: Failing tests** (pure resampling/consumption):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consumes_at_ratio_and_duplicates_channels() {
        let mut src = Source::default();
        src.buf.extend((0..100).map(|i| i as f32));
        let mut out = vec![0.0f32; 2 * 40]; // 40 stereo frames at 48k
        let used = mix_into(&mut out, 2, 0.5, &mut src); // 24k -> 48k
        assert_eq!(used, 20);
        assert_eq!(out[0], out[1]);
    }

    #[test]
    fn underrun_outputs_silence() {
        let mut src = Source::default();
        let mut out = vec![1.0f32; 8];
        assert_eq!(mix_into(&mut out, 2, 0.5, &mut src), 0);
        assert!(out.iter().all(|v| *v == 0.0));
    }

    #[test]
    fn paused_outputs_silence_and_keeps_buffer() {
        let mut src = Source::default();
        src.paused = true;
        src.buf.extend([0.5f32; 10]);
        let mut out = vec![1.0f32; 8];
        assert_eq!(mix_into(&mut out, 2, 0.5, &mut src), 0);
        assert_eq!(src.buf.len(), 10);
    }
}
```

- [ ] **Step 2: Run** `cargo test ra_player` → FAIL.

- [ ] **Step 3: Implement**

```rust
//! Read Aloud — streaming playback on the default output device.
//! cpal::Stream isn't Send, so it lives on its own thread (same pattern as audio.rs).
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};

#[derive(Default)]
pub struct Source {
    pub buf: VecDeque<f32>,
    pub frac: f64,
    pub paused: bool,
}

/// Fill `out` (interleaved, `channels`) from 24 kHz mono `src`, advancing `step` source samples per frame.
pub fn mix_into(out: &mut [f32], channels: usize, step: f64, src: &mut Source) -> u64 {
    let mut used = 0u64;
    for frame in out.chunks_mut(channels) {
        let v = if src.paused || src.buf.is_empty() {
            0.0
        } else {
            let a = src.buf[0];
            let b = *src.buf.get(1).unwrap_or(&a);
            let v = a + (b - a) * src.frac as f32;
            src.frac += step;
            while src.frac >= 1.0 && !src.buf.is_empty() {
                src.buf.pop_front();
                src.frac -= 1.0;
                used += 1;
            }
            v
        };
        for s in frame.iter_mut() { *s = v; }
    }
    used
}

pub struct Player {
    src: Arc<Mutex<Source>>,
    pos: Arc<AtomicU64>,
    pushed: Arc<AtomicU64>,
    failed: Arc<AtomicBool>,
    stop_tx: mpsc::Sender<()>,
}

impl Player {
    pub fn start() -> Result<Player, String> {
        let src = Arc::new(Mutex::new(Source::default()));
        let pos = Arc::new(AtomicU64::new(0));
        let failed = Arc::new(AtomicBool::new(false));
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let (ok_tx, ok_rx) = mpsc::channel::<Result<(), String>>();
        let (s2, p2, f2) = (Arc::clone(&src), Arc::clone(&pos), Arc::clone(&failed));
        std::thread::Builder::new().name("ra-player".into()).spawn(move || {
            let run = || -> Result<cpal::Stream, String> {
                let dev = cpal::default_host().default_output_device().ok_or("no output device")?;
                let cfg = dev.default_output_config().map_err(|e| e.to_string())?;
                if cfg.sample_format() != cpal::SampleFormat::F32 {
                    return Err(format!("unsupported output format {:?}", cfg.sample_format()));
                }
                let ch = cfg.channels() as usize;
                let step = 24_000.0 / cfg.sample_rate().0 as f64;
                let f3 = Arc::clone(&f2);
                let stream = dev.build_output_stream(&cfg.config(), move |out: &mut [f32], _| {
                    let used = mix_into(out, ch, step, &mut s2.lock());
                    p2.fetch_add(used, Ordering::Relaxed);
                }, move |e| { log::warn!("read-aloud: output stream error: {e}"); f3.store(true, Ordering::Relaxed); }, None)
                    .map_err(|e| e.to_string())?;
                stream.play().map_err(|e| e.to_string())?;
                Ok(stream)
            };
            match run() {
                Ok(stream) => { ok_tx.send(Ok(())).ok(); let _ = stop_rx.recv(); drop(stream); }
                Err(e) => { ok_tx.send(Err(e)).ok(); }
            }
        }).map_err(|e| e.to_string())?;
        ok_rx.recv().map_err(|e| e.to_string())??;
        Ok(Player { src, pos, pushed: Arc::new(AtomicU64::new(0)), failed, stop_tx })
    }

    pub fn push(&self, pcm: &[f32]) {
        self.src.lock().buf.extend(pcm.iter().copied());
        self.pushed.fetch_add(pcm.len() as u64, Ordering::Relaxed);
    }
    pub fn clear(&self) {
        let mut s = self.src.lock();
        s.buf.clear();
        s.frac = 0.0;
        self.pos.store(0, Ordering::Relaxed);
        self.pushed.store(0, Ordering::Relaxed);
    }
    pub fn set_paused(&self, p: bool) { self.src.lock().paused = p; }
    pub fn is_paused(&self) -> bool { self.src.lock().paused }
    pub fn position(&self) -> u64 { self.pos.load(Ordering::Relaxed) }
    pub fn queued_end(&self) -> u64 { self.pushed.load(Ordering::Relaxed) }
    pub fn failed(&self) -> bool { self.failed.load(Ordering::Relaxed) }
}

impl Drop for Player {
    fn drop(&mut self) { let _ = self.stop_tx.send(()); }
}
```

- [ ] **Step 4: Run** `cargo test ra_player` → PASS.
- [ ] **Step 5: Commit** `feat(read-aloud): streaming player with sample position`.

---

### Task 7: `ra_uia.rs` — UIA worker thread + pick-mode mouse hook

**Files:** Create `src-tauri/src/ra_uia.rs`, `src-tauri/src/ra_hook.rs`; mod lines (cfg windows).

**Interfaces:**
- Produces (`ra_uia`):
  - `pub struct Rect { pub x: f64, pub y: f64, pub w: f64, pub h: f64 }` (physical screen px), `#[derive(Clone, Debug, serde::Serialize)]`
  - `pub struct Uia` (owns a worker thread; all COM lives there) with:
    - `pub fn start(hwnd: isize) -> Uia` (non-blocking; worker warms the connection immediately — rule 1)
    - `pub fn hover(&self, x: i32, y: i32) -> Option<Vec<Rect>>` (word rects under the point, ≤ 50 ms timeout, else `None`)
    - `pub fn pick(&self, x: i32, y: i32) -> Result<Vec<String>, String>` (validates rule 2/3; returns raw tokens from the clicked word forward, up to 2000; worker keeps their ranges, index-aligned)
    - `pub fn more(&self) -> Vec<String>` (next ≤2000 raw tokens, empty at end; indices continue)
    - `pub fn rects(&self, src: Vec<usize>) -> Vec<Rect>` (rule 9: fresh every call)
    - `pub fn scroll_to(&self, src_idx: usize)` (rule 4: paragraph scroll for Chromium)
    - `pub fn source_hwnd(&self) -> isize`
  - Pure: `pub fn contains(rects: &[Rect], x: f64, y: f64) -> bool`
- Produces (`ra_hook`): `pub struct PickHook` — `pub fn install(on_move: impl Fn(i32,i32)+Send+'static, on_click: impl Fn(i32,i32)+Send+'static) -> PickHook`; dropping it uninstalls the WH_MOUSE_LL hook and ends its message loop. Clicks (left down + its up) are **swallowed**; moves pass through.

- [ ] **Step 1: Failing test for the pure helper**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn contains_checks_any_rect() {
        let r = vec![Rect { x: 10.0, y: 10.0, w: 20.0, h: 10.0 }, Rect { x: 0.0, y: 30.0, w: 5.0, h: 5.0 }];
        assert!(contains(&r, 15.0, 15.0));
        assert!(contains(&r, 2.0, 32.0));
        assert!(!contains(&r, 15.0, 25.0));
        assert!(!contains(&[], 0.0, 0.0));
    }
}
```

- [ ] **Step 2: Run** `cargo test ra_uia` → FAIL.

- [ ] **Step 3: Implement `ra_uia.rs`**

```rust
//! Read Aloud — UI Automation text access. One worker thread owns all COM objects.
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;
use windows::core::Interface;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use windows::Win32::System::Ole::{SafeArrayGetLBound, SafeArrayGetUBound, SafeArrayGetElement};
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::WindowsAndMessaging::GetClassNameW;

#[derive(Clone, Debug, serde::Serialize, PartialEq)]
pub struct Rect { pub x: f64, pub y: f64, pub w: f64, pub h: f64 }

pub fn contains(rects: &[Rect], x: f64, y: f64) -> bool {
    rects.iter().any(|r| x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h)
}

enum Req {
    Hover(i32, i32, Sender<Option<Vec<Rect>>>),
    Pick(i32, i32, Sender<Result<Vec<String>, String>>),
    More(Sender<Vec<String>>),
    Rects(Vec<usize>, Sender<Vec<Rect>>),
    Scroll(usize),
}

pub struct Uia { tx: Sender<Req>, hwnd: isize }

const BATCH: usize = 2000;

impl Uia {
    pub fn start(hwnd: isize) -> Uia {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new().name("ra-uia".into()).spawn(move || worker(hwnd, rx)).ok();
        Uia { tx, hwnd }
    }
    pub fn source_hwnd(&self) -> isize { self.hwnd }
    pub fn hover(&self, x: i32, y: i32) -> Option<Vec<Rect>> {
        let (t, r) = mpsc::channel();
        self.tx.send(Req::Hover(x, y, t)).ok()?;
        r.recv_timeout(Duration::from_millis(50)).ok().flatten()
    }
    pub fn pick(&self, x: i32, y: i32) -> Result<Vec<String>, String> {
        let (t, r) = mpsc::channel();
        self.tx.send(Req::Pick(x, y, t)).map_err(|e| e.to_string())?;
        r.recv_timeout(Duration::from_secs(5)).map_err(|_| "text lookup timed out".to_string())?
    }
    pub fn more(&self) -> Vec<String> {
        let (t, r) = mpsc::channel();
        if self.tx.send(Req::More(t)).is_err() { return Vec::new(); }
        r.recv_timeout(Duration::from_secs(5)).unwrap_or_default()
    }
    pub fn rects(&self, src: Vec<usize>) -> Vec<Rect> {
        let (t, r) = mpsc::channel();
        if self.tx.send(Req::Rects(src, t)).is_err() { return Vec::new(); }
        r.recv_timeout(Duration::from_millis(100)).unwrap_or_default()
    }
    pub fn scroll_to(&self, idx: usize) { let _ = self.tx.send(Req::Scroll(idx)); }
}

struct State {
    uia: IUIAutomation,
    root: Option<IUIAutomationElement>,
    chromium: bool,
    ranges: Vec<IUIAutomationTextRange>,
    cursor: Option<IUIAutomationTextRange>, // last fetched word range
}

fn worker(hwnd: isize, rx: Receiver<Req>) {
    unsafe { let _ = CoInitializeEx(None, COINIT_MULTITHREADED); }
    let uia: IUIAutomation = match unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) } {
        Ok(u) => u,
        Err(e) => { log::warn!("read-aloud: UIA init failed: {e}"); return; }
    };
    let h = HWND(hwnd as *mut _);
    let root = unsafe { uia.ElementFromHandle(h) }.ok();
    // Warm-up (rule 1): touching the tree makes Chromium build its a11y tree now.
    if let Some(r) = &root { unsafe { let _ = r.CurrentName(); } }
    let chromium = {
        let mut buf = [0u16; 64];
        let n = unsafe { GetClassNameW(h, &mut buf) } as usize;
        String::from_utf16_lossy(&buf[..n]).starts_with("Chrome_WidgetWin")
    };
    let mut st = State { uia, root, chromium, ranges: Vec::new(), cursor: None };
    while let Ok(req) = rx.recv() {
        match req {
            Req::Hover(x, y, t) => { let _ = t.send(word_at(&st, x, y).ok().map(|(_, r)| r)); }
            Req::Pick(x, y, t) => { let _ = t.send(pick(&mut st, x, y)); }
            Req::More(t) => { let _ = t.send(read_forward(&mut st, BATCH)); }
            Req::Rects(src, t) => {
                let v = src.iter().filter_map(|&i| st.ranges.get(i)).flat_map(|r| rects_of(r)).collect();
                let _ = t.send(v);
            }
            Req::Scroll(i) => if let Some(r) = st.ranges.get(i) { scroll(&st, r); },
        }
    }
}

/// Element under the point that has a TextPattern; retry once (Chromium's first query can be empty).
fn text_pattern_at(st: &State, x: i32, y: i32) -> Result<(IUIAutomationTextPattern, Vec<Rect>), String> {
    for attempt in 0..2 {
        let mut el = unsafe { st.uia.ElementFromPoint(POINT { x, y }) }.map_err(|e| e.to_string())?;
        let walker = unsafe { st.uia.ControlViewWalker() }.map_err(|e| e.to_string())?;
        for _ in 0..30 {
            if let Ok(tp) = unsafe { el.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) } {
                let b = unsafe { el.CurrentBoundingRectangle() }.map_err(|e| e.to_string())?;
                let er = Rect { x: b.left as f64, y: b.top as f64, w: (b.right - b.left) as f64, h: (b.bottom - b.top) as f64 };
                return Ok((tp, vec![er]));
            }
            match unsafe { walker.GetParentElement(&el) } { Ok(p) => el = p, Err(_) => break }
        }
        if attempt == 0 { std::thread::sleep(Duration::from_millis(150)); }
    }
    Err("no TextPattern under the click".into())
}

fn word_at(st: &State, x: i32, y: i32) -> Result<(IUIAutomationTextRange, Vec<Rect>), String> {
    let (tp, elem) = text_pattern_at(st, x, y)?;
    // rule 3: ignore points outside the text element (browser chrome)
    if !contains(&elem, x as f64, y as f64) { return Err("outside text element".into()); }
    let r = unsafe { tp.RangeFromPoint(POINT { x, y }) }.map_err(|e| e.to_string())?;
    unsafe { r.ExpandToEnclosingUnit(TextUnit_Word) }.map_err(|e| e.to_string())?;
    let rects = rects_of(&r);
    // rule 2: the click must land inside the returned word
    if !contains(&rects, x as f64, y as f64) { return Err("click not on a word".into()); }
    Ok((r, rects))
}

fn pick(st: &mut State, x: i32, y: i32) -> Result<Vec<String>, String> {
    let (r, _) = word_at(st, x, y)?;
    st.ranges.clear();
    st.cursor = None;
    // rule 7: everything below walks from this one range/source
    let first_text = unsafe { r.GetText(-1) }.map(|b| b.to_string()).unwrap_or_default();
    st.ranges.push(r.clone());
    st.cursor = Some(r);
    let mut out = vec![first_text];
    out.extend(read_forward(st, BATCH - 1));
    Ok(out)
}

/// Rule 8: always walk forward from the cursor range, never re-query visible ranges.
fn read_forward(st: &mut State, n: usize) -> Vec<String> {
    let mut out = Vec::new();
    let Some(cur) = st.cursor.clone() else { return out; };
    let Ok(r) = (unsafe { cur.Clone() }) else { return out; };
    for _ in 0..n {
        let moved = unsafe { r.Move(TextUnit_Word, 1) }.unwrap_or(0);
        if moved == 0 { break; }
        if unsafe { r.ExpandToEnclosingUnit(TextUnit_Word) }.is_err() { break; }
        let text = unsafe { r.GetText(-1) }.map(|b| b.to_string()).unwrap_or_default();
        let Ok(keep) = (unsafe { r.Clone() }) else { break; };
        st.ranges.push(keep);
        out.push(text);
    }
    if let Ok(c) = unsafe { r.Clone() } { st.cursor = Some(c); }
    out
}

fn rects_of(r: &IUIAutomationTextRange) -> Vec<Rect> {
    let Ok(sa) = (unsafe { r.GetBoundingRectangles() }) else { return Vec::new(); };
    let mut v = Vec::new();
    unsafe {
        let (Ok(lo), Ok(hi)) = (SafeArrayGetLBound(sa, 1), SafeArrayGetUBound(sa, 1)) else { return v; };
        let mut nums = Vec::new();
        for i in lo..=hi {
            let mut d = 0f64;
            if SafeArrayGetElement(sa, &i, &mut d as *mut f64 as *mut _).is_ok() { nums.push(d); }
        }
        for c in nums.chunks_exact(4) { if c[2] > 0.0 && c[3] > 0.0 { v.push(Rect { x: c[0], y: c[1], w: c[2], h: c[3] }); } }
        let _ = windows::Win32::System::Ole::SafeArrayDestroy(sa);
    }
    v
}

fn scroll(st: &State, r: &IUIAutomationTextRange) {
    unsafe {
        if st.chromium {
            // rule 4: Chromium ignores word-range scroll; paragraph works
            if let Ok(p) = r.Clone() { let _ = p.ExpandToEnclosingUnit(TextUnit_Paragraph); let _ = p.ScrollIntoView(true); }
        } else {
            let _ = r.ScrollIntoView(true);
        }
    }
}
```

(The `windows` 0.58 signatures for `SafeArray*` and `GetBoundingRectangles` may differ slightly — adjust to compile; behaviour must match Spike A's `probe.py`. If `ElementFromPoint` returns the overlay window, confirm the overlay has `set_ignore_cursor_events(true)` — it always does in this design.)

- [ ] **Step 4: Implement `ra_hook.rs`**

```rust
//! Read Aloud — low-level mouse hook, alive only during pick mode.
//! Swallows the left click (down + up) so the target app never sees it.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use parking_lot::Mutex;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::*;

type Cb = Box<dyn Fn(i32, i32) + Send>;
static MOVE: OnceLock<Mutex<Option<Cb>>> = OnceLock::new();
static CLICK: OnceLock<Mutex<Option<Cb>>> = OnceLock::new();
static SWALLOW_UP: AtomicBool = AtomicBool::new(false);

pub struct PickHook { thread_id: u32 }

unsafe extern "system" fn proc(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code >= 0 {
        let info = &*(l.0 as *const MSLLHOOKSTRUCT);
        let (x, y) = (info.pt.x, info.pt.y);
        match w.0 as u32 {
            WM_MOUSEMOVE => if let Some(cb) = MOVE.get().and_then(|m| m.lock().as_ref().map(|c| c as *const Cb)) { (*cb)(x, y); },
            WM_LBUTTONDOWN => {
                if let Some(cb) = CLICK.get().and_then(|m| m.lock().as_ref().map(|c| c as *const Cb)) { (*cb)(x, y); }
                SWALLOW_UP.store(true, Ordering::Relaxed);
                return LRESULT(1);
            }
            WM_LBUTTONUP if SWALLOW_UP.swap(false, Ordering::Relaxed) => return LRESULT(1),
            _ => {}
        }
    }
    CallNextHookEx(None, code, w, l)
}

impl PickHook {
    pub fn install(on_move: impl Fn(i32, i32) + Send + 'static, on_click: impl Fn(i32, i32) + Send + 'static) -> PickHook {
        *MOVE.get_or_init(|| Mutex::new(None)).lock() = Some(Box::new(on_move));
        *CLICK.get_or_init(|| Mutex::new(None)).lock() = Some(Box::new(on_click));
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new().name("ra-hook".into()).spawn(move || unsafe {
            let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(proc), None, 0);
            tx.send(GetCurrentThreadId()).ok();
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
            if let Ok(h) = hook { let _ = UnhookWindowsHookEx(h); }
        }).ok();
        PickHook { thread_id: rx.recv().unwrap_or(0) }
    }
}

impl Drop for PickHook {
    fn drop(&mut self) {
        unsafe { let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)); }
        if let Some(m) = MOVE.get() { *m.lock() = None; }
        if let Some(m) = CLICK.get() { *m.lock() = None; }
    }
}
```

Hook callbacks must return in well under 300 ms (Windows drops slow LL hooks): `on_move`/`on_click` only send to a channel; never call UIA inside them.

- [ ] **Step 5: Manual check** — add an `#[ignore]` test `uia_notepad_manual` that launches `notepad.exe` with a temp file of 3 paragraphs, finds its HWND by `FindWindowW`, starts `Uia`, takes the rect of the 5th word via `pick` at the element centre, asserts `pick` returns ≥ 10 tokens and `rects(vec![0])` is non-empty, then kills Notepad. Run: `cargo test ra_uia -- --ignored --nocapture` → PASS.
- [ ] **Step 6: Run** `cargo test ra_uia` → PASS. **Commit** `feat(read-aloud): UIA text worker and pick-mode mouse hook`.

---

### Task 8: `read_aloud.rs` — session state machine, wiring, hotkey, media keys, commands

**Files:**
- Create: `src-tauri/src/read_aloud.rs`
- Create: `src-tauri/capabilities/read-aloud.json`
- Modify: `src-tauri/src/main.rs` — mod line; `AppState` gains `pub read_aloud: read_aloud::ReadAloud` (cfg windows); register commands; in setup, `read_aloud::register_hotkey(handle)` when `read_aloud_enabled`; `set_setting` arm for `read_aloud_enabled` that registers/unregisters live; `hotkeys` JSON gains `"readAloud"`.

**Interfaces:**
- Consumes: everything from Tasks 2–7.
- Produces:
  - Pure core: `pub enum Phase { Idle, Picking { reading: bool }, Warming, Reading, Paused }`, `pub enum Ev { Hotkey, Click, ClickRejected, FirstAudio, Esc, PlayPause, Stop, End, Error }`, `pub enum Act { EnterPick, ExitPick, StartRead, ShowWarming, ShowReading, ShowPaused, Teardown, Toast(&'static str), BumpGen }`, `pub fn step(p: &Phase, ev: Ev) -> (Phase, Vec<Act>)`.
  - Commands (frontend uses exactly these names): `ra_toggle()`, `ra_skip(dir: i32)`, `ra_set_speed(speed: f32)`, `ra_stop()`, `ra_save_pill_pos(x: f64, y: f64)`, `ra_get_state() -> serde_json::Value`.
  - Events (exact payloads):
    - to `ra-overlay`: `"ra-overlay-mode"` `{ "mode": "pick" | "read" }`; `"ra-rects"` `{ "kind": "hover" | "word", "rects": [{x,y,w,h}], "style": "brass" | "mono" | "off" }` (CSS px relative to the overlay); `"ra-pick-rejected"` `{}`.
    - to `ra-pill`: `"ra-state"` `{ "state": "warming" | "reading" | "paused" | "error", "speed": f32, "device": "gpu" | "cpu" | null, "message": string | null }`; `"ra-sentence"` `{ "words": [string], "current": number, "progress": number /*0..1*/ }`.

- [ ] **Step 1: Failing tests for the pure state machine**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use Phase::*;

    #[test]
    fn hotkey_from_idle_enters_pick() {
        let (p, a) = step(&Idle, Ev::Hotkey);
        assert_eq!(p, Picking { reading: false });
        assert!(a.contains(&Act::EnterPick));
    }

    #[test]
    fn picking_click_while_loading_goes_to_warming() {
        let (p, a) = step(&Picking { reading: false }, Ev::Click);
        assert_eq!(p, Warming);
        assert!(a.contains(&Act::StartRead) && a.contains(&Act::ShowWarming) && a.contains(&Act::ExitPick));
        let (p2, a2) = step(&p, Ev::FirstAudio);
        assert_eq!(p2, Reading);
        assert!(a2.contains(&Act::ShowReading));
    }

    #[test]
    fn rejected_click_stays_in_pick() {
        let (p, a) = step(&Picking { reading: false }, Ev::ClickRejected);
        assert_eq!(p, Picking { reading: false });
        assert!(a.is_empty());
    }

    #[test]
    fn esc_in_pick_returns_to_previous() {
        assert_eq!(step(&Picking { reading: false }, Ev::Esc).0, Idle);
        assert_eq!(step(&Picking { reading: true }, Ev::Esc).0, Reading);
    }

    #[test]
    fn jump_bumps_generation() {
        let (p, _) = step(&Reading, Ev::Hotkey);
        assert_eq!(p, Picking { reading: true });
        let (_, a) = step(&p, Ev::Click);
        assert!(a.contains(&Act::BumpGen));
    }

    #[test]
    fn play_pause_toggles() {
        assert_eq!(step(&Reading, Ev::PlayPause).0, Paused);
        assert_eq!(step(&Paused, Ev::PlayPause).0, Reading);
    }

    #[test]
    fn every_error_ends_idle() {
        for p in [Picking { reading: false }, Picking { reading: true }, Warming, Reading, Paused] {
            let (n, a) = step(&p, Ev::Error);
            assert_eq!(n, Idle, "{p:?}");
            assert!(a.contains(&Act::Teardown));
            let (n, a) = step(&p, Ev::Stop);
            assert_eq!(n, Idle);
            assert!(a.contains(&Act::Teardown));
        }
        assert_eq!(step(&Reading, Ev::End).0, Idle);
    }

    #[test]
    fn idle_ignores_controls() {
        for ev in [Ev::PlayPause, Ev::Esc, Ev::Click, Ev::End, Ev::FirstAudio] {
            assert_eq!(step(&Idle, ev).0, Idle);
        }
    }
}
```

- [ ] **Step 2: Run** `cargo test read_aloud` → FAIL.

- [ ] **Step 3: Implement the pure core**

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase { Idle, Picking { reading: bool }, Warming, Reading, Paused }

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ev { Hotkey, Click, ClickRejected, FirstAudio, Esc, PlayPause, Stop, End, Error }

#[derive(Debug, Clone, PartialEq)]
pub enum Act { EnterPick, ExitPick, StartRead, ShowWarming, ShowReading, ShowPaused, Teardown, Toast(&'static str), BumpGen }

pub fn step(p: &Phase, ev: Ev) -> (Phase, Vec<Act>) {
    use Act::*;
    use Phase::*;
    match (p, ev) {
        (_, Ev::Error) if *p != Idle => (Idle, vec![Teardown]),
        (_, Ev::Stop) if *p != Idle => (Idle, vec![Teardown]),
        (Idle, Ev::Hotkey) => (Picking { reading: false }, vec![EnterPick]),
        (Reading | Paused | Warming, Ev::Hotkey) => (Picking { reading: true }, vec![EnterPick]),
        (Picking { .. }, Ev::Hotkey) => (*p, vec![]),
        (Picking { .. }, Ev::Click) => (Warming, vec![ExitPick, BumpGen, StartRead, ShowWarming]),
        (Picking { .. }, Ev::ClickRejected) => (*p, vec![]),
        (Picking { reading: false }, Ev::Esc) => (Idle, vec![ExitPick, Teardown]),
        (Picking { reading: true }, Ev::Esc) => (Reading, vec![ExitPick]),
        (Warming, Ev::FirstAudio) => (Reading, vec![ShowReading]),
        (Reading, Ev::PlayPause) => (Paused, vec![ShowPaused]),
        (Paused, Ev::PlayPause) => (Reading, vec![ShowReading]),
        (Reading | Paused | Warming, Ev::End) => (Idle, vec![Teardown]),
        _ => (*p, vec![]),
    }
}
```

- [ ] **Step 4: Run** `cargo test read_aloud` → PASS. Commit `feat(read-aloud): session state machine`.

- [ ] **Step 5: Implement the runtime** (same file, below the core). Structure — one controller thread owns all non-Send pieces and receives `Msg`s; Tauri commands/hotkeys only send `Msg`s:

```rust
use crate::{ra_hook::PickHook, ra_player::Player, ra_text, ra_timing, ra_tts::{self, Engine}, ra_uia::{Rect, Uia}};
use std::sync::{mpsc, Arc};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub enum Msg { Hotkey, Hover(i32, i32), Click(i32, i32), Esc, PlayPause, Skip(i32), Speed(f32), Stop, Tick, Chunk(u64, usize, Result<ra_tts::ChunkAudio, String>) }

pub struct ReadAloud { tx: parking_lot::Mutex<Option<mpsc::Sender<Msg>>>, engine: Arc<Engine> }

impl ReadAloud {
    pub fn new(data_dir: std::path::PathBuf) -> Self { Self { tx: parking_lot::Mutex::new(None), engine: Engine::new(data_dir) } }
    /// Lazily starts the controller thread on first use (0 threads at rest).
    pub fn send(&self, app: &AppHandle, m: Msg) {
        let mut g = self.tx.lock();
        if g.is_none() {
            let (tx, rx) = mpsc::channel();
            let (app2, eng, tx2) = (app.clone(), Arc::clone(&self.engine), tx.clone());
            std::thread::Builder::new().name("ra-ctl".into()).spawn(move || Controller::new(app2, eng, tx2).run(rx)).ok();
            *g = Some(tx);
        }
        let _ = g.as_ref().unwrap().send(m);
    }
}
```

`Controller` fields: `phase: Phase`, `gen: Arc<AtomicU64>`, `uia: Option<Uia>`, `hook: Option<PickHook>`, `player: Option<Player>`, `words: Vec<ra_text::Word>`, `raw_len: usize`, `chunks: Vec<Range<usize>>`, `next_chunk: usize`, `inflight: usize`, `timeline: Vec<(u64 /*global start sample*/, usize /*global word idx*/)>`, `chunk_starts: Vec<usize>` (word index per chunk start, for skip), `pushed_samples: u64`, `cur_word: Option<usize>`, `speed: f32`, `style: String`, `voice: String`, `device_pref: String`, `source_hwnd: isize`, `overlay_origin: (f64, f64, f64)` (monitor x, y, scale), `used_cpu_retry: bool`.

`run(rx)` loop: `rx.recv_timeout(Duration::from_millis(33))` → on timeout, when `phase` is `Reading`, handle `Tick`; otherwise handle the message. **No timer exists outside Reading** (tick only fires while reading). When `phase == Idle` and the engine is unloaded and windows are gone, the thread keeps blocking on `recv()` (zero CPU).

Act handlers (exact behaviour):
- **EnterPick:** `hwnd = paste::foreground_window()` (existing helper; skip if it is an Echo window — then use the previous source). If `uia` is `None` or its hwnd differs, `uia = Some(Uia::start(hwnd))` (warm-up starts now). `engine.preload(&voice, &device_pref)` (load starts now). `ensure_overlay(hwnd)` (create if missing, see Step 6), emit `ra-overlay-mode {mode:"pick"}`. Register `Escape` as a global shortcut that sends `Msg::Esc`. Install `PickHook` whose callbacks send `Msg::Hover`/`Msg::Click` (throttle Hover to one per 33 ms inside the callback using an `AtomicU64` timestamp).
- **Hover(x,y):** `if let Some(r) = uia.hover(x,y)` emit `ra-rects {kind:"hover", rects: to_css(r)}`, else emit empty hover rects.
- **Click(x,y):** `match uia.pick(x,y)`: `Ok(raw)` → `words = ra_text::normalize(&raw)`; if `words` empty → treat as rejected; else feed `Ev::Click`. `Err(_)` → emit `ra-pick-rejected`, feed `Ev::ClickRejected`.
- **ExitPick:** drop `hook`, unregister `Escape`, emit `ra-overlay-mode {mode:"read"}` and clear hover rects.
- **BumpGen:** `gen += 1`; `player.clear()`; reset `timeline`, `pushed_samples`, `next_chunk`, `inflight = 0`, `cur_word = None`.
- **StartRead:** `chunks = ra_text::chunk(&words, 12, 50)`; `player` = `Player::start()` if `None` (error → `Ev::Error` + toast "No audio output device"); register media keys `MediaPlayPause`, `MediaTrackNext`, `MediaTrackPrevious` (→ `PlayPause`, `Skip(1)`, `Skip(-1)`); `ensure_pill()`; then `pump()`.
- **pump():** while `inflight + (chunks already queued but not yet played) < 2` and `next_chunk < chunks.len()`: spawn a thread that calls `engine.synth(&texts, speed)` and sends `Msg::Chunk(gen, idx, result)`; `inflight += 1; next_chunk += 1`. If `next_chunk == chunks.len()` and `raw_len` was a full batch, call `uia.more()`, append `normalize(more)` (re-index `src` by `+raw_len`), re-chunk the tail. **Chunks must be pushed in order:** keep a `BTreeMap<usize, ChunkAudio>` of finished chunks and push only the next expected index.
- **Chunk(g, idx, res):** ignore if `g != gen` (stale after a jump). `Err(e)` starting with `"dml"` and `!used_cpu_retry` → `engine.reset_to_cpu(&voice)`, `used_cpu_retry = true`, emit state message "Voice engine hiccup — using CPU", resubmit the same chunk; other `Err` → `Ev::Error` + toast "Voice engine failed". `Ok(a)` → for each `WordTime w` push `(pushed_samples + w.start, chunks[idx].start + w.word)` to `timeline`; `player.push(&a.pcm)`; `pushed_samples += a.pcm.len()`; `inflight -= 1`; if `phase == Warming` feed `Ev::FirstAudio`; `pump()`.
- **Tick:** `pos = player.position()`; binary-search `timeline` for the last entry with start ≤ pos → `wi`. If `wi != cur_word`: `cur_word = Some(wi)`; emit `ra-sentence` (words of the chunk containing `wi`, `current` = offset in it, `progress = wi / words.len()`); **source visible check:** if `GetForegroundWindow()`'s root owner == `source_hwnd` and `!IsIconic(source_hwnd)` → `rects = uia.rects(words[wi].src.clone())`; if `rects` empty → `uia.scroll_to(words[wi].src[0])` and retry once; emit `ra-rects {kind:"word", rects: to_css(rects), style}`; else emit empty word rects (overlay hides highlight). If `pos >= pushed_samples` and `next_chunk >= chunks.len()` and `inflight == 0` → `Ev::End`. If `player.failed()` → `Ev::PlayPause` into Paused + state message "Audio device changed — press play" and on next PlayPause recreate `Player` (push nothing old; restart from `cur_word`'s chunk via BumpGen + StartRead from that word).
- **Skip(dir):** find the chunk containing `cur_word`; target = that chunk ± 1 (clamped); rebuild: `words = words[target_start..]` is NOT done — instead BumpGen, set `next_chunk = target`, `pump()` (timeline uses global word indices so highlights stay correct).
- **Speed(s):** clamp 1.0–3.5, round to 0.1, persist `read_aloud_speed`; apply to **future** chunks (already queued audio keeps its rate — max ~2 chunks of lag, acceptable).
- **ShowWarming/ShowReading/ShowPaused:** `player.set_paused(phase == Paused)`; emit `ra-state`.
- **Teardown:** drop `hook`; unregister `Escape` and media keys; `player = None`; `uia = None`; close `ra-overlay` and `ra-pill` windows; clear words/timeline; `engine.touch()` (idle unload starts counting). Every error path goes through here.
- **Toast(msg):** emit to `ra-pill` if it exists, else show the overlay hint for 3 s; also `log::info!`.

`to_css(rects)`: `(r.x - mon_x) / scale`, `(r.y - mon_y) / scale`, `w/scale`, `h/scale`, where `mon_x/mon_y/scale` come from the overlay window's `current_monitor()`.

- [ ] **Step 6: Windows** (in `read_aloud.rs`):

```rust
fn ensure_overlay(app: &AppHandle, hwnd: isize) -> Option<(f64, f64, f64)> {
    let mon = app.get_webview_window("main")?.available_monitors().ok()?.into_iter()
        .find(|m| monitor_contains(m, hwnd)).or_else(|| app.primary_monitor().ok().flatten())?;
    let (pos, size, scale) = (mon.position(), mon.size(), mon.scale_factor());
    let w = match app.get_webview_window("ra-overlay") {
        Some(w) => w,
        None => WebviewWindowBuilder::new(app, "ra-overlay", WebviewUrl::App("index.html".into()))
            .transparent(true).decorations(false).always_on_top(true).skip_taskbar(true)
            .focused(false).shadow(false).resizable(false).visible(false)
            .build().ok()?,
    };
    let _ = w.set_position(tauri::PhysicalPosition::new(pos.x, pos.y));
    let _ = w.set_size(tauri::PhysicalSize::new(size.width, size.height));
    let _ = w.set_ignore_cursor_events(true); // always click-through; the LL hook captures clicks
    let _ = w.show();
    Some((pos.x as f64, pos.y as f64, scale))
}

fn ensure_pill(app: &AppHandle, saved: Option<(f64, f64)>) {
    if app.get_webview_window("ra-pill").is_some() { return; }
    let mut b = WebviewWindowBuilder::new(app, "ra-pill", WebviewUrl::App("index.html".into()))
        .transparent(true).decorations(false).always_on_top(true).skip_taskbar(true)
        .focused(false).resizable(false).shadow(false).inner_size(540.0, 132.0);
    b = match saved { Some((x, y)) => b.position(x, y), None => b.position(40.0, 40.0) };
    if let Ok(w) = b.build() { let _ = w.show(); }
}
```

`monitor_contains` uses `MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST)` + `GetMonitorInfoW` and compares the rect with `m.position()`.

- [ ] **Step 7: Capability** `src-tauri/capabilities/read-aloud.json`:

```json
{
  "$schema": "https://raw.githubusercontent.com/nickel-0/tauri/v2/crates/tauri-utils/schema.json",
  "identifier": "read-aloud",
  "description": "Read Aloud overlay and pill windows",
  "windows": ["ra-overlay", "ra-pill"],
  "permissions": [
    "core:default",
    "core:event:default",
    "core:event:allow-listen",
    "core:window:allow-start-dragging",
    "core:window:allow-outer-position"
  ]
}
```

- [ ] **Step 8: Wire `main.rs`** — add commands (each `#[tauri::command] fn ra_toggle(app: AppHandle, state: State<Arc<AppState>>) { state.read_aloud.send(&app, read_aloud::Msg::PlayPause) }`, likewise `ra_skip(dir)` → `Skip`, `ra_set_speed(speed)` → `Speed`, `ra_stop` → `Stop`, `ra_save_pill_pos(x,y)` → persist `read_aloud_pill_pos` `[x,y]`, `ra_get_state` → `{enabled, speed, voice, device, highlight, installed: ra_model::is_installed}`) to `generate_handler!` behind `#[cfg(target_os = "windows")]` (use a cfg'd `ra_commands!` list or separate handler vector — follow how `packs::` is listed). Hotkey:

```rust
#[cfg(target_os = "windows")]
pub fn register_hotkey(app: &AppHandle) -> bool {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
    app.global_shortcut().on_shortcut("CmdOrCtrl+Shift+E", |app, _s, ev| {
        if ev.state == ShortcutState::Pressed {
            // hotkey path: only post a message (debug-journal rule)
            app.state::<Arc<crate::AppState>>().read_aloud.send(app, Msg::Hotkey);
        }
    }).map_err(|e| log::warn!("read-aloud hotkey: {e}")).is_ok()
}
```

In setup: register only if `settings.get_bool("read_aloud_enabled") == Some(true)` **and** `ra_model::is_installed(&data_dir)`. In `set_setting`: `"read_aloud_enabled"` → register or `unregister("CmdOrCtrl+Shift+E")` + send `Msg::Stop`. `"read_aloud_speed"|"read_aloud_highlight"|"read_aloud_voice"|"read_aloud_device"` → controller reads settings at each `StartRead` (no message needed).

- [ ] **Step 9: Build + run tests** `cd src-tauri && cargo build && cargo test` → all pass; no warnings introduced in existing modules.
- [ ] **Step 10: Commit** `feat(read-aloud): session controller, hotkey, media keys, windows`.

---

### Task 9: Frontend — overlay + Now Reading pill (black-and-white)

**Files:**
- Create: `src/components/ReadAloudOverlay.jsx`, `src/components/NowReadingPill.jsx`, `src/readAloud.css`
- Modify: `src/main.jsx` (route by window label **before** `App` runs)

**Interfaces:** Consumes Task 8's event names/payloads and command names exactly.

- [ ] **Step 1: Route by window label** — top of `src/main.jsx`, replace the final render line:

```jsx
import { getCurrentWindow } from '@tauri-apps/api/window'
import ReadAloudOverlay from './components/ReadAloudOverlay'
import NowReadingPill from './components/NowReadingPill'

const label = getCurrentWindow().label
const root = createRoot(document.getElementById('root'))
if (label === 'ra-overlay') root.render(<ReadAloudOverlay />)
else if (label === 'ra-pill') root.render(<NowReadingPill />)
else root.render(<App />)
```

- [ ] **Step 2: `src/readAloud.css`** (scoped under `.ra-*`; transparent windows):

```css
html.ra-window, html.ra-window body, html.ra-window #root { background: transparent !important; border: none !important; height: 100vh; overflow: hidden; }
.ra-hint { position: fixed; top: 40px; left: 50%; transform: translateX(-50%); display: flex; align-items: center; gap: 12px; padding: 10px 14px 10px 12px; background: #000; color: #fff; border: 1px solid rgba(255,255,255,0.14); border-radius: 10px; box-shadow: 0 10px 30px rgba(0,0,0,0.45); font: 14px var(--font-sans); }
.ra-hint kbd { font: 11px var(--font-mono); padding: 2px 6px; border: 1px solid rgba(255,255,255,0.22); border-radius: 4px; color: #e4e4e6; }
.ra-hint.shake { animation: ra-shake 0.3s; }
@keyframes ra-shake { 25% { transform: translateX(calc(-50% - 6px)); } 75% { transform: translateX(calc(-50% + 6px)); } }
.ra-dim { position: fixed; inset: 0; background: rgba(0,0,0,0.05); }
.ra-box { position: fixed; pointer-events: none; border-radius: 3px; transition: left 60ms linear, top 60ms linear, width 60ms linear; }
.ra-box.hover { outline: 2px solid #fff; box-shadow: 0 0 0 4px rgba(0,0,0,0.55); }
.ra-box.brass { background: rgba(200,151,63,0.42); box-shadow: 0 0 0 2px rgba(200,151,63,0.42); }
.ra-box.mono { box-shadow: inset 0 -3px 0 #000, 0 1px 0 #fff; }
.ra-pill { margin: 6px; background: #0c0c0f; color: #fff; border: 1px solid rgba(255,255,255,0.14); border-radius: 12px; box-shadow: 0 14px 40px rgba(0,0,0,0.5); padding: 10px 12px; display: flex; flex-direction: column; gap: 8px; font-family: var(--font-sans); }
.ra-row { display: flex; align-items: center; gap: 6px; }
.ra-grip { width: 14px; height: 32px; cursor: grab; opacity: 0.4; }
.ra-btn { width: 44px; height: 44px; border: none; background: transparent; color: #e4e4e6; border-radius: 8px; display: flex; align-items: center; justify-content: center; cursor: pointer; }
.ra-btn:hover { background: rgba(255,255,255,0.06); }
.ra-btn.primary { background: #fff; color: #000; border-radius: 22px; }
.ra-speed { display: flex; align-items: center; height: 36px; border: 1px solid rgba(255,255,255,0.14); border-radius: 8px; font: 14px var(--font-mono); }
.ra-speed button { width: 32px; height: 34px; border: none; background: transparent; color: #a6a6ab; cursor: pointer; font-size: 16px; }
.ra-speed span { min-width: 46px; text-align: center; }
.ra-sentence { font: 15px/1.45 var(--font-display), Georgia, serif; color: #a6a6ab; padding: 0 6px 0 22px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.ra-sentence .done { color: #e4e4e6; }
.ra-sentence .cur { color: #000; background: rgba(200,151,63,0.85); border-radius: 3px; box-shadow: 0 0 0 2px rgba(200,151,63,0.85); }
.ra-sentence .cur.mono { color: #fff; background: none; box-shadow: inset 0 -2px 0 #fff; }
.ra-progress { height: 3px; margin: 0 6px 0 22px; background: rgba(255,255,255,0.08); border-radius: 2px; overflow: hidden; }
.ra-progress div { height: 3px; background: #fff; }
.ra-status { font: 11px var(--font-mono); letter-spacing: 0.14em; text-transform: uppercase; color: #a6a6ab; margin-left: 6px; }
.ra-spin { width: 20px; height: 20px; border-radius: 10px; border: 2.5px solid rgba(255,255,255,0.12); border-top-color: #fff; animation: ra-rot 0.9s linear infinite; }
@keyframes ra-rot { to { transform: rotate(360deg); } }
```

- [ ] **Step 3: `ReadAloudOverlay.jsx`**

```jsx
import { useEffect, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import '../readAloud.css'

export default function ReadAloudOverlay() {
  const [mode, setMode] = useState('pick')
  const [hover, setHover] = useState([])
  const [word, setWord] = useState({ rects: [], style: 'brass' })
  const [shake, setShake] = useState(false)

  useEffect(() => {
    document.documentElement.classList.add('ra-window')
    const offs = [
      listen('ra-overlay-mode', e => { setMode(e.payload.mode); setHover([]) }),
      listen('ra-rects', e => {
        if (e.payload.kind === 'hover') setHover(e.payload.rects)
        else setWord({ rects: e.payload.rects, style: e.payload.style })
      }),
      listen('ra-pick-rejected', () => { setShake(true); setTimeout(() => setShake(false), 320) }),
    ]
    return () => offs.forEach(p => p.then(f => f()))
  }, [])

  const box = (r, cls, i) => (
    <div key={i} className={`ra-box ${cls}`} style={{ left: r.x - 2, top: r.y - 1, width: r.w + 4, height: r.h + 2 }} />
  )

  return (
    <>
      {mode === 'pick' && <div className="ra-dim" />}
      {mode === 'pick' && (
        <div className={`ra-hint${shake ? ' shake' : ''}`} role="status">
          <span>Click any word to start reading</span>
          <span><kbd>Esc</kbd> cancel</span>
        </div>
      )}
      {mode === 'pick' && hover.map((r, i) => box(r, 'hover', i))}
      {mode === 'read' && word.style !== 'off' && word.rects.map((r, i) => box(r, word.style, i))}
    </>
  )
}
```

- [ ] **Step 4: `NowReadingPill.jsx`**

```jsx
import { useEffect, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import '../readAloud.css'

const Icon = {
  back: <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M19 20 9 12l10-8v16z"/><path d="M5 19V5"/></svg>,
  fwd: <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="m5 4 10 8-10 8V4z"/><path d="M19 5v14"/></svg>,
  pause: <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="5" width="4" height="14" rx="1"/><rect x="14" y="5" width="4" height="14" rx="1"/></svg>,
  play: <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor"><path d="M7 4.5v15l13-7.5z"/></svg>,
  x: <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>,
}

export default function NowReadingPill() {
  const [st, setSt] = useState({ state: 'warming', speed: 2.2, device: null, message: null })
  const [sent, setSent] = useState({ words: [], current: 0, progress: 0 })
  const [mono, setMono] = useState(false)

  useEffect(() => {
    document.documentElement.classList.add('ra-window')
    invoke('ra_get_state').then(s => setMono(s?.highlight === 'mono')).catch(() => {})
    const offs = [
      listen('ra-state', e => setSt(e.payload)),
      listen('ra-sentence', e => setSent(e.payload)),
    ]
    return () => offs.forEach(p => p.then(f => f()))
  }, [])

  const drag = async () => {
    const w = getCurrentWindow()
    await w.startDragging()
    const p = await w.outerPosition()
    invoke('ra_save_pill_pos', { x: p.x, y: p.y }).catch(() => {})
  }
  const speed = d => invoke('ra_set_speed', { speed: Math.round((st.speed + d) * 10) / 10 })

  if (st.state === 'warming') {
    return (
      <div className="ra-pill"><div className="ra-row">
        <div className="ra-grip" onMouseDown={drag} aria-hidden="true">⋮⋮</div>
        <div className="ra-spin" />
        <div style={{ flex: 1, marginLeft: 8 }}>Starting voice…</div>
        <button className="ra-btn" aria-label="Stop reading" onClick={() => invoke('ra_stop')}>{Icon.x}</button>
      </div></div>
    )
  }

  const paused = st.state === 'paused'
  return (
    <div className="ra-pill">
      <div className="ra-row">
        <div className="ra-grip" onMouseDown={drag} aria-hidden="true">⋮⋮</div>
        <button className="ra-btn" aria-label="Back one sentence" onClick={() => invoke('ra_skip', { dir: -1 })}>{Icon.back}</button>
        <button className="ra-btn primary" aria-label={paused ? 'Play' : 'Pause'} onClick={() => invoke('ra_toggle')}>{paused ? Icon.play : Icon.pause}</button>
        <button className="ra-btn" aria-label="Forward one sentence" onClick={() => invoke('ra_skip', { dir: 1 })}>{Icon.fwd}</button>
        {paused && <span className="ra-status">Paused</span>}
        {st.message && <span className="ra-status">{st.message}</span>}
        <div style={{ flex: 1 }} />
        <div className="ra-speed">
          <button aria-label="Slower" onClick={() => speed(-0.1)}>−</button>
          <span>{st.speed.toFixed(1)}×</span>
          <button aria-label="Faster" onClick={() => speed(0.1)}>+</button>
        </div>
        <button className="ra-btn" aria-label="Stop reading" onClick={() => invoke('ra_stop')}>{Icon.x}</button>
      </div>
      <div className="ra-sentence">
        {sent.words.map((w, i) => (
          <span key={i} className={i === sent.current ? `cur${mono ? ' mono' : ''}` : i < sent.current ? 'done' : ''}>{w}{' '}</span>
        ))}
      </div>
      <div className="ra-progress"><div style={{ width: `${Math.round(sent.progress * 100)}%` }} /></div>
    </div>
  )
}
```

- [ ] **Step 5: Build** `npm run build` → no errors. **Commit** `feat(read-aloud): overlay and Now Reading pill (black-and-white)`.

---

### Task 10: Settings section "Read Aloud (beta)"

**Files:** Modify `src/components/SettingsPanel.jsx` (add one section, Windows only).

**Interfaces:** Consumes `get_settings`, `set_setting`, `ra_get_state` (for `installed`).

- [ ] **Step 1: Add the section** following the panel's existing row components and token classes (read 2–3 existing sections first and reuse their row/label/toggle components — do not invent new primitives). Content, in order:
  1. Eyebrow "Settings · Beta", title "Read Aloud", one-line description "Press the shortcut, click any word, and Echo reads from there. Nothing loads until you use it."
  2. Toggle **Enable Read Aloud** → `set_setting('read_aloud_enabled', bool)`. If `ra_get_state().installed === false`, disable the toggle and show: "Voice files not installed — run `tools/read-aloud/prepare_model.py`".
  3. Read-only **Shortcut** chip `Ctrl + Shift + E`.
  4. **Speed** range 1.0–3.5 step 0.1 → `read_aloud_speed`, label shows `2.2×`.
  5. **Voice** select: Heart · US (`af_heart`), Michael · US (`am_michael`), Emma · UK (`bf_emma`) → `read_aloud_voice`.
  6. **Word highlight** radio: Brass fill (`brass`) / Mono underline (`mono`) / Off (`off`) → `read_aloud_highlight`.
  7. **Run voice on** segmented: Auto (`auto`) / GPU (`gpu`) / CPU (`cpu`) → `read_aloud_device`, helper text "Unloads after 2 min idle."
- [ ] **Step 2: Hide on macOS** using the same platform check the panel already uses for Windows-only rows.
- [ ] **Step 3:** `npm run build` → OK. **Commit** `feat(read-aloud): settings section`.

---

### Task 11: Real-world verification, resource check, session log

**Files:** Create `docs/2026-10-02-read-aloud-session-log.md`; update `.claude/debug-journal.md` with any non-obvious bug found.

- [ ] **Step 1:** `npm run tauri dev`; enable Read Aloud in Settings.
- [ ] **Step 2: Manual matrix** — for each of Chrome article, Edge article, Edge PDF, Word .docx, Notepad: start from a word mid-paragraph, jump with a second `Ctrl+Shift+E`, confirm highlight lines up, page auto-scrolls past the fold, switch to another app and back (highlight hides/returns, audio continues), media Play/Pause and Next work, ✕ tears everything down. Also: Acrobat → "Nothing readable here" toast (phase 2 adds OCR); a mixed-DPI second monitor if available. Fill the table in the session log.
- [ ] **Step 3: Resource check** — Task Manager + `nvidia-smi`: (a) Echo idle with Read Aloud enabled before first use: within ~5 MB of baseline, no ra-* threads, 0 VRAM; (b) while reading: RAM added < 500 MB, VRAM < ~1 GB; (c) 2+ minutes after ✕: back to baseline RAM and VRAM. Record numbers.
- [ ] **Step 4:** `cd src-tauri && cargo test` → all pass. Record count.
- [ ] **Step 5:** Write the session log (what shipped, files, commits, verification, follow-ups: phase 2 OCR, hotkey rebinding, WinEvent-hook visibility if the foreground check proves too coarse on multi-monitor). Commit `docs(read-aloud): phase 1 session log`.

---

## Known deviations from the spec (decided here)

- **Source visibility:** phase 1 checks "source window is foreground and not minimized" on each word change instead of installing WinEvent hooks. Cheaper and simpler; downside: on a second monitor the highlight hides while you work in another app even though the page is visible. Revisit with hooks if that annoys in real use.
- **Pick-mode clicks:** captured with a `WH_MOUSE_LL` hook (alive only in pick mode) while the overlay stays click-through. This keeps UIA `ElementFromPoint` hitting the real app instead of our overlay.
- **Model files:** installed into `%APPDATA%/ALAN Echo/models/kokoro` by a dev script, not bundled in the installer (personal build). A customer build would add a download-on-enable flow.
- **Speed changes** apply from the next synthesized chunk (≤ ~2 chunks of lag).
