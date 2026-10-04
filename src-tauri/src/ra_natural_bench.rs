//! Natural-reading bench (spec §8, dev only): structure fixtures rendered through the
//! real `Session` queue + the real CPU voice model, pauses included, exactly as the
//! player gets the PCM, with objective pause metrics per structural boundary.
//!
//! cargo test --release natural_bench -- --ignored --nocapture --test-threads=1
//! NB_OUT   output dir (default C:/Users/arowm/alan-echo-nat-bench/run): WAVs + metrics.json
//! NB_MODE  `plain` (default: no markers, glyphs as a page shows them) | `marked`
//! NB_RATES rates (default "2.2,3.0"); NB_ONLY a fixture-name filter.

use super::*;

use crate::ra_struct::{self, Brk};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

/// How a fixture's markup reaches the tokens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// Structure markers from the markup, no glyphs (a UIA source that knows the roles).
    Marked,
    /// No markers; bullet glyphs and item numbers kept as the page shows them.
    Plain,
}

/// A fixture as a source would give it: raw tokens, and each structural boundary as
/// (raw index of the block's first token, kind). The boundaries come from the markup
/// whatever the mode.
#[derive(Debug, Default, PartialEq)]
struct Loaded {
    tokens: Vec<String>,
    bounds: Vec<(usize, Brk)>,
}

/// One markup line.
enum Line<'a> {
    Heading(u8, &'a str),
    Bullet(&'a str),
    /// Item number as written ("2."), then the text.
    Numbered(&'a str, &'a str),
    Row(Vec<&'a str>),
    Quote(&'a str),
    Caption(&'a str),
    Text(&'a str),
    Blank,
    /// A table separator row (`|---|---|`): not text.
    Skip,
}

/// Reads one markup line.
fn classify(line: &str) -> Line<'_> {
    let t = line.trim();
    if t.is_empty() {
        return Line::Blank;
    }
    let hashes = t.bytes().take_while(|&c| c == b'#').count();
    if (1..=6).contains(&hashes) {
        if let Some(rest) = t.get(hashes..).and_then(|r| r.strip_prefix(' ')) {
            return Line::Heading(hashes as u8, rest.trim());
        }
    }
    for g in ["- ", "* ", "\u{2022} "] {
        if let Some(r) = t.strip_prefix(g) {
            return Line::Bullet(r.trim());
        }
    }
    let digits = t.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 {
        if let (Some(num), Some(r)) = (t.get(..digits + 1), t.get(digits..).and_then(|r| r.strip_prefix(". "))) {
            return Line::Numbered(num, r.trim());
        }
    }
    if let Some(r) = t.strip_prefix('|') {
        let cells: Vec<&str> = r.strip_suffix('|').unwrap_or(r).split('|').map(str::trim).collect();
        if cells.iter().all(|c| c.chars().all(|ch| matches!(ch, '-' | ':' | ' '))) {
            return Line::Skip;
        }
        return Line::Row(cells);
    }
    if let Some(r) = t.strip_prefix("> ") {
        return Line::Quote(r.trim());
    }
    if let Some(r) = t.strip_prefix("Caption: ") {
        return Line::Caption(r.trim());
    }
    Line::Text(t)
}

/// Fixture markup → raw tokens as a source would give them (word + " ", the line's
/// last word + "\n"; a table row has "\n" only after its last cell). Markup: `#`..
/// heading lines, `- ` `* ` `• ` bullets, `1. ` numbered, `| a | b |` rows, `> `
/// quote, `Caption: `, a blank line = paragraph, a plain line break = line.
fn load(text: &str, mode: Mode) -> Loaded {
    let mut out = Loaded::default();
    let (mut blank, mut after_heading, mut in_quote) = (false, false, false);
    for line in text.lines() {
        let l = classify(line);
        // (boundary kind, glyph shown in plain mode, words) per block or cell
        let mut segs: Vec<(Brk, Option<&str>, &str)> = Vec::new();
        match &l {
            Line::Blank => {
                blank = true;
                continue;
            }
            Line::Skip => continue,
            Line::Heading(n, r) => segs.push((Brk::Heading(*n), None, r)),
            Line::Bullet(r) => segs.push((Brk::ListItem, Some("\u{2022}"), r)),
            Line::Numbered(num, r) => segs.push((Brk::ListItem, Some(num), r)),
            Line::Row(cells) => {
                for (k, c) in cells.iter().enumerate() {
                    segs.push((if k == 0 { Brk::TableRow } else { Brk::TableCell }, None, c));
                }
            }
            Line::Quote(r) => segs.push((if in_quote && !blank { Brk::Line } else { Brk::Quote }, None, r)),
            Line::Caption(r) => segs.push((Brk::Caption, None, r)),
            Line::Text(r) => {
                let b = if after_heading {
                    Brk::AfterHeading
                } else if blank {
                    Brk::Paragraph
                } else {
                    Brk::Line
                };
                segs.push((b, None, r));
            }
        }
        let line_start = out.tokens.len();
        for (brk, glyph, words) in segs {
            let mut ws: Vec<&str> = Vec::new();
            if mode == Mode::Plain {
                ws.extend(glyph);
            }
            ws.extend(words.split_whitespace());
            for (k, w) in ws.iter().enumerate() {
                if k == 0 {
                    if !out.tokens.is_empty() {
                        out.bounds.push((out.tokens.len(), brk));
                    }
                    if mode == Mode::Marked {
                        out.tokens.push(format!("{}{w} ", ra_struct::mark(brk)));
                        continue;
                    }
                }
                out.tokens.push(format!("{w} "));
            }
        }
        // the line's last token ends the block
        if let Some(last) = out.tokens.get_mut(line_start..).and_then(|t| t.last_mut()) {
            last.pop();
            last.push('\n');
        }
        blank = false;
        after_heading = matches!(l, Line::Heading(..));
        in_quote = matches!(l, Line::Quote(_));
    }
    out
}

/// One 10 ms window at 24 kHz.
const WIN: usize = 240;

/// 10 ms windows (240 samples, the last one partial): true when the peak is under -40 dBFS.
fn silent_windows(pcm: &[f32]) -> Vec<bool> {
    pcm.chunks(WIN).map(|w| w.iter().all(|x| x.abs() <= 0.01)).collect()
}

/// The longest run of silent windows that overlaps samples `from..=to`, in ms (the run
/// may extend past the range). 0 when no silent window touches it.
fn silence_at(silent: &[bool], from: u64, to: u64) -> u64 {
    let a = (from / WIN as u64) as usize;
    let b = ((to / WIN as u64) as usize).min(silent.len().saturating_sub(1));
    if silent.is_empty() || a > b {
        return 0;
    }
    let mut best = 0usize;
    let mut k = a;
    while k <= b {
        if silent.get(k) != Some(&true) {
            k += 1;
            continue;
        }
        let lo = silent.get(..k).map_or(0, |s| s.iter().rev().take_while(|&&x| x).count());
        let hi = silent.get(k..).map_or(0, |s| s.iter().take_while(|&&x| x).count());
        best = best.max(lo + hi);
        k += hi.max(1);
    }
    best as u64 * 10
}

/// The words either side of the boundary at raw token `tok`: the last word starting
/// before it, and the first spoken word (has a letter or digit) from it on.
fn bound_words(words: &[Word], tok: usize) -> Option<(usize, usize)> {
    let first = |w: &Word| w.src.iter().min().copied().unwrap_or(0);
    let prev = words.iter().rposition(|w| first(w) < tok)?;
    let next = words.iter().position(|w| first(w) >= tok && w.text.chars().any(char::is_alphanumeric))?;
    Some((prev, next))
}

/// Speech RMS of one chunk in dB (silent windows left out); `None` under 10 speech windows.
fn speech_db(pcm: &[f32]) -> Option<f64> {
    let speech: Vec<&[f32]> = pcm.chunks(WIN).filter(|w| w.iter().any(|x| x.abs() > 0.01)).collect();
    if speech.len() < 10 {
        return None;
    }
    let (sum, n) = speech.iter().flat_map(|w| w.iter()).fold((0.0f64, 0usize), |(s, n), &x| (s + f64::from(x) * f64::from(x), n + 1));
    Some(10.0 * (sum / n.max(1) as f64).max(1e-12).log10())
}

/// Spread (max - min, dB) of per-chunk speech RMS (silent windows left out). Chunks
/// with under 10 speech windows are skipped; 0 with fewer than two chunks.
fn loudness_spread_db(chunks: &[Vec<f32>]) -> f64 {
    let dbs: Vec<f64> = chunks.iter().filter_map(|c| speech_db(c)).collect();
    if dbs.len() < 2 {
        return 0.0;
    }
    let hi = dbs.iter().copied().fold(f64::MIN, f64::max);
    let lo = dbs.iter().copied().fold(f64::MAX, f64::min);
    hi - lo
}

/// Largest sample jump where one non-empty chunk's PCM meets the next.
fn max_join_jump(chunks: &[Vec<f32>]) -> f32 {
    let mut last: Option<f32> = None;
    let mut max = 0.0f32;
    for c in chunks.iter().filter(|c| !c.is_empty()) {
        if let (Some(l), Some(f)) = (last, c.first()) {
            max = max.max((f - l).abs());
        }
        last = c.last().copied();
    }
    max
}

/// Spec §2 target pause at `rate`: base at 1x / rate^0.6, floored at 150 ms (0 for
/// running text).
fn target_ms(b: Brk, rate: f32) -> f64 {
    let base = match b {
        Brk::None => return 0.0,
        Brk::Line | Brk::TableCell => 250.0,
        Brk::ListItem => 400.0,
        Brk::TableRow | Brk::Caption | Brk::Quote | Brk::Code | Brk::Heading(_) => 450.0,
        Brk::Paragraph => 550.0,
        Brk::AfterHeading => 650.0,
        Brk::Section => 900.0,
    };
    (base / f64::from(rate.max(0.1)).powf(0.6)).max(150.0)
}

/// Short name of a boundary kind (heading levels pooled).
fn kind_name(b: Brk) -> &'static str {
    match b {
        Brk::None => "None",
        Brk::Line => "Line",
        Brk::ListItem => "ListItem",
        Brk::TableCell => "TableCell",
        Brk::TableRow => "TableRow",
        Brk::Paragraph => "Paragraph",
        Brk::Caption => "Caption",
        Brk::Quote => "Quote",
        Brk::Code => "Code",
        Brk::Heading(_) => "Heading",
        Brk::AfterHeading => "AfterHeading",
        Brk::Section => "Section",
    }
}

/// A structural boundary with less silence than this is fused (runs together).
const FUSED_MS: u64 = 150;

/// Measured pauses → (fused count, per kind: (n, mean ms, fused)).
fn summarize(p: &[(Brk, u64)]) -> (usize, BTreeMap<&'static str, (usize, f64, usize)>) {
    let mut sums: BTreeMap<&'static str, (usize, u64, usize)> = BTreeMap::new();
    for &(b, ms) in p {
        let e = sums.entry(kind_name(b)).or_default();
        e.0 += 1;
        e.1 += ms;
        e.2 += usize::from(ms < FUSED_MS);
    }
    let fused = sums.values().map(|v| v.2).sum();
    (fused, sums.into_iter().map(|(k, (n, s, f))| (k, (n, s as f64 / n.max(1) as f64, f))).collect())
}

/// The committed fixtures (`tools/read-aloud/natural/*.txt`), by name, sorted.
fn fixtures() -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tools/read-aloud/natural");
    let mut out: Vec<(String, String)> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "txt"))
                .filter_map(|p| Some((p.file_stem()?.to_string_lossy().into_owned(), std::fs::read_to_string(&p).ok()?)))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// What the player got for one read: the PCM, per chunk, and each word's start / end
/// sample (start from the session timeline, end from the chunk's word times).
#[derive(Default)]
struct Render {
    pcm: Vec<f32>,
    chunks: Vec<Vec<f32>>,
    starts: HashMap<usize, u64>,
    ends: HashMap<usize, u64>,
}

/// The one engine call (as the synth worker makes it). Task 2 adds `pause_after_ms`
/// from the `Job` here.
fn synth(e: &Arc<Engine>, ctl: u64, j: &Job) -> Result<ChunkAudio, String> {
    let words: Vec<&str> = j.words.iter().map(String::as_str).collect();
    e.synth_job(&words, j.rate, ctl, j.gen)
}

/// Reads `words` start to end through the real `Session` (`set_rate`, `start`,
/// `jobs`, engine synth, `on_chunk`) and concatenates the pushed PCM in order.
fn render(e: &Arc<Engine>, words: Vec<Word>, raw_len: usize, rate: f32) -> Result<Render, String> {
    let mut s = Session::new(Arc::new(AtomicU64::new(0)));
    s.set_rate(rate);
    s.start(words, raw_len, true);
    let ctl = e.controller_gen();
    let mut r = Render::default();
    let mut times: HashMap<usize, Vec<crate::ra_timing::WordTime>> = HashMap::new();
    for _ in 0..10_000 {
        let pos = r.pcm.len() as u64;
        if s.finished(pos) {
            break;
        }
        let jobs = s.jobs(pos, rate);
        if jobs.is_empty() {
            return Err("stalled: no jobs and not finished".into());
        }
        for j in jobs {
            if j.gen != s.gen() {
                continue; // queued past a split chunk: re-cut and re-issued
            }
            let a = synth(e, ctl, &j)?;
            times.insert(j.idx, a.words.clone());
            let first = s.next_push;
            let (pcm, _) = s.on_chunk(j.gen, j.idx, a);
            for (k, p) in pcm.into_iter().enumerate() {
                let off = r.pcm.len() as u64;
                let idx = first + k;
                if let (Some(rg), Some(ws)) = (s.chunks.get(idx), times.remove(&idx)) {
                    for w in ws.iter().filter(|w| w.word < rg.len()) {
                        r.ends.insert(rg.start + w.word, off + w.end);
                    }
                }
                r.pcm.extend_from_slice(&p);
                r.chunks.push(p);
            }
        }
    }
    r.starts = s.timeline.iter().map(|&(t, w)| (w, t)).collect();
    Ok(r)
}

/// 16-bit mono WAV at the engine rate.
fn write_wav(path: &std::path::Path, pcm: &[f32]) -> Result<(), String> {
    let spec = hound::WavSpec { channels: 1, sample_rate: crate::ra_timing::SR, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?;
    for &x in pcm {
        w.write_sample((x.clamp(-1.0, 1.0) * 32767.0) as i16).map_err(|e| e.to_string())?;
    }
    w.finalize().map_err(|e| e.to_string())
}

/// Silence (ms) heard at each structural boundary: the longest silent run touching
/// the span from late in the previous word (its end - 50 ms, not before its start) to
/// 100 ms into the next word's start.
fn boundary_pauses(words: &[Word], bounds: &[(usize, Brk)], r: &Render) -> Vec<(Brk, usize, u64)> {
    let silent = silent_windows(&r.pcm);
    bounds
        .iter()
        .filter(|b| b.1.is_break())
        .filter_map(|&(tok, kind)| {
            let (p, n) = bound_words(words, tok)?;
            let to = r.starts.get(&n)? + 2400;
            let ps = r.starts.get(&p).copied().unwrap_or(0);
            let from = r.ends.get(&p).map_or(ps, |&e| e.saturating_sub(1200).max(ps)).min(to);
            Some((kind, n, silence_at(&silent, from, to)))
        })
        .collect()
}

#[test]
#[ignore]
fn natural_bench() {
    let out = PathBuf::from(std::env::var("NB_OUT").unwrap_or_else(|_| "C:/Users/arowm/alan-echo-nat-bench/run".into()));
    let mode = match std::env::var("NB_MODE").as_deref() {
        Ok("marked") => Mode::Marked,
        _ => Mode::Plain,
    };
    let rates: Vec<f32> = std::env::var("NB_RATES")
        .ok()
        .map(|v| v.split(',').filter_map(|x| x.trim().parse().ok()).collect())
        .unwrap_or_else(|| vec![2.2, 3.0]);
    let only = std::env::var("NB_ONLY").unwrap_or_default();
    std::fs::create_dir_all(&out).expect("NB_OUT");
    let e = Engine::new(dirs::data_dir().expect("data dir").join("ALAN Echo"));
    e.preload("af_heart", "cpu");
    let t0 = Instant::now();
    while !e.is_loaded() {
        assert!(t0.elapsed() < Duration::from_secs(60), "load");
        std::thread::sleep(Duration::from_millis(5));
    }
    e.synth(&["Warm", "up", "the", "engine."], 2.2).expect("warm up");
    println!("natural bench: mode {mode:?}, rates {rates:?}, out {}", out.display());
    let mut all = Vec::new();
    for (name, text) in fixtures().into_iter().filter(|f| f.0.contains(&only)) {
        let l = load(&text, mode);
        let words = ra_text::normalize(&l.tokens);
        for &rate in &rates {
            let r = render(&e, words.clone(), l.tokens.len(), rate).expect("render");
            write_wav(&out.join(format!("{name}-{rate}.wav")), &r.pcm).expect("wav");
            let pauses = boundary_pauses(&words, &l.bounds, &r);
            let p2: Vec<(Brk, u64)> = pauses.iter().map(|p| (p.0, p.2)).collect();
            let (fused, by) = summarize(&p2);
            let spread = loudness_spread_db(&r.chunks);
            let jump = max_join_jump(&r.chunks);
            let untimed = (0..words.len()).filter(|w| !r.starts.contains_key(w)).count();
            let kinds: Vec<String> = by.iter().map(|(k, v)| format!("{k} {:.0}/{:.0}ms n{} f{}", v.1, target_ms(pauses.iter().find(|p| kind_name(p.0) == *k).map_or(Brk::None, |p| p.0), rate), v.0, v.2)).collect();
            println!(
                "{name} @{rate}x: {} words, {} chunks, {:.1} s; fused {fused}/{}; loudness spread {spread:.1} dB; max join jump {jump:.3}; untimed {untimed}\n    {}",
                words.len(),
                r.chunks.len(),
                r.pcm.len() as f64 / 24000.0,
                pauses.len(),
                kinds.join("; ")
            );
            all.push(serde_json::json!({
                "fixture": name, "rate": rate, "mode": format!("{mode:?}").to_lowercase(),
                "words": words.len(), "chunks": r.chunks.len(), "audio_s": r.pcm.len() as f64 / 24000.0,
                "fused": fused, "boundaries_n": pauses.len(),
                "loudness_spread_db": spread, "max_join_jump": jump, "untimed_words": untimed,
                "chunk_speech_db": r.chunks.iter().map(|c| speech_db(c)).collect::<Vec<_>>(),
                "by_kind": by.iter().map(|(k, v)| (k.to_string(), serde_json::json!({"n": v.0, "mean_ms": v.1, "fused": v.2}))).collect::<serde_json::Map<_, _>>(),
                "boundaries": pauses.iter().map(|p| serde_json::json!({
                    "kind": format!("{:?}", p.0), "word": p.1, "text": words.get(p.1).map(|w| w.text.as_str()),
                    "silence_ms": p.2, "target_ms": target_ms(p.0, rate),
                })).collect::<Vec<_>>(),
            }));
        }
    }
    let json = serde_json::to_string_pretty(&serde_json::json!({ "mode": format!("{mode:?}").to_lowercase(), "runs": all })).expect("json");
    std::fs::write(out.join("metrics.json"), json).expect("metrics.json");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(b: Brk, t: &str) -> String {
        format!("{}{t}", ra_struct::mark(b))
    }

    #[test]
    fn loads_headings_and_bullets_in_both_modes() {
        let text = "# Key Takeaways\nThe company grew.\n- Apples\n- Pears\n";
        let marked = load(text, Mode::Marked);
        assert_eq!(
            marked.tokens,
            vec![m(Brk::Heading(1), "Key "), "Takeaways\n".into(), m(Brk::AfterHeading, "The "), "company ".into(), "grew.\n".into(), m(Brk::ListItem, "Apples\n"), m(Brk::ListItem, "Pears\n")]
        );
        assert_eq!(marked.bounds, vec![(2, Brk::AfterHeading), (5, Brk::ListItem), (6, Brk::ListItem)]);
        let plain = load(text, Mode::Plain);
        assert_eq!(plain.tokens, ["Key ", "Takeaways\n", "The ", "company ", "grew.\n", "• ", "Apples\n", "• ", "Pears\n"]);
        assert_eq!(plain.bounds, vec![(2, Brk::AfterHeading), (5, Brk::ListItem), (7, Brk::ListItem)]);
    }

    #[test]
    fn loads_table_cells_with_a_line_break_only_at_row_end() {
        let text = "| a | b |\n|---|---|\n| c d | e |\n";
        let plain = load(text, Mode::Plain);
        assert_eq!(plain.tokens, ["a ", "b\n", "c ", "d ", "e\n"]);
        assert_eq!(plain.bounds, vec![(1, Brk::TableCell), (2, Brk::TableRow), (4, Brk::TableCell)]);
        let marked = load(text, Mode::Marked);
        assert_eq!(marked.tokens, vec![m(Brk::TableRow, "a "), m(Brk::TableCell, "b\n"), m(Brk::TableRow, "c "), "d ".into(), m(Brk::TableCell, "e\n")]);
        assert_eq!(marked.bounds, plain.bounds);
    }

    #[test]
    fn loads_paragraphs_lines_quotes_captions_and_numbers() {
        let text = "Hi all,\n\nFirst line\nsecond line\n> q one\n> q two\nCaption: Fig 1.\n2. Step\n";
        let plain = load(text, Mode::Plain);
        assert_eq!(plain.tokens, ["Hi ", "all,\n", "First ", "line\n", "second ", "line\n", "q ", "one\n", "q ", "two\n", "Fig ", "1.\n", "2. ", "Step\n"]);
        let kinds: Vec<Brk> = plain.bounds.iter().map(|b| b.1).collect();
        assert_eq!(kinds, [Brk::Paragraph, Brk::Line, Brk::Quote, Brk::Line, Brk::Caption, Brk::ListItem]);
        assert_eq!(plain.bounds.last(), Some(&(12, Brk::ListItem)));
        let marked = load(text, Mode::Marked);
        assert_eq!(marked.tokens.last().map(String::as_str), Some(format!("{}Step\n", ra_struct::mark(Brk::ListItem)).as_str()));
    }

    #[test]
    fn odd_markup_never_panics() {
        for t in ["", "\n\n", "#", "# ", "|", "||", "| |", "1.", "1. ", "- ", "####### x", "> ", "Caption: ", "\u{2022} ", "•x"] {
            for mode in [Mode::Marked, Mode::Plain] {
                let l = load(t, mode);
                assert!(l.bounds.iter().all(|b| b.0 < l.tokens.len()), "{t:?}");
            }
        }
    }

    #[test]
    fn marked_tokens_set_word_brk_through_normalize() {
        let text = "# Key Takeaways\nThe company grew.\n- Apples\n| a | b |\n";
        let l = load(text, Mode::Marked);
        let w = ra_text::normalize(&l.tokens);
        assert_eq!(w[0].brk, Brk::Heading(1));
        for &(tok, kind) in &l.bounds {
            let (_, next) = bound_words(&w, tok).expect("bound");
            assert_eq!(w[next].brk, kind, "{tok}");
        }
    }

    #[test]
    fn bound_words_skip_a_bullet_glyph() {
        let l = load("It grew.\n- Apples\n", Mode::Plain);
        let w = ra_text::normalize(&l.tokens);
        let (p, n) = bound_words(&w, l.bounds[0].0).expect("bound");
        assert_eq!((w[p].text.as_str(), w[n].text.as_str()), ("grew.", "Apples"));
        assert_eq!(bound_words(&w, 0), None, "nothing before the first word");
        assert_eq!(bound_words(&w, 99), None);
        assert_eq!(bound_words(&[], 1), None);
    }

    #[test]
    fn finds_silent_windows() {
        let mut pcm = vec![0.0f32; 480];
        pcm.extend(std::iter::repeat_n(0.5, 240));
        pcm.extend(std::iter::repeat_n(0.005, 100));
        assert_eq!(silent_windows(&pcm), [true, true, false, true]);
        assert!(silent_windows(&[]).is_empty());
    }

    #[test]
    fn silence_at_takes_the_longest_overlapping_run() {
        let s = [false, false, true, true, true, false, false, true, false];
        assert_eq!(silence_at(&s, 240, 480), 30, "the run 2..5 touches window 2");
        assert_eq!(silence_at(&s, 0, 240), 0);
        assert_eq!(silence_at(&s, 240, 7 * 240), 30, "longest of two runs");
        assert_eq!(silence_at(&s, 7 * 240, 99_999), 10);
        assert_eq!(silence_at(&s, 99_999, 9), 0, "reversed / past the end");
        assert_eq!(silence_at(&[], 0, 100), 0);
    }

    #[test]
    fn loudness_spread_ignores_silence() {
        let a = vec![0.1f32; 2400];
        let mut b = vec![0.2f32; 2400];
        b.extend(vec![0.0; 4800]);
        let tiny = vec![0.9f32; 240];
        let s = loudness_spread_db(&[a.clone(), b, tiny]);
        assert!((s - 6.0206).abs() < 0.01, "{s}");
        assert_eq!(loudness_spread_db(&[a]), 0.0);
        assert_eq!(loudness_spread_db(&[]), 0.0);
    }

    #[test]
    fn join_jump_skips_empty_chunks() {
        let c = vec![vec![0.0, 0.1], vec![0.5, 0.0], vec![], vec![-0.2]];
        assert!((max_join_jump(&c) - 0.4).abs() < 1e-6);
        assert_eq!(max_join_jump(&[]), 0.0);
    }

    #[test]
    fn summary_counts_fused_and_means_per_kind() {
        let p = [(Brk::Heading(2), 100), (Brk::Heading(1), 300), (Brk::ListItem, 149), (Brk::ListItem, 150), (Brk::TableCell, 0)];
        let (fused, by) = summarize(&p);
        assert_eq!(fused, 3);
        assert_eq!(by.get("Heading"), Some(&(2, 200.0, 1)));
        assert_eq!(by.get("ListItem"), Some(&(2, 149.5, 1)));
        assert_eq!(by.get("TableCell"), Some(&(1, 0.0, 1)));
        assert_eq!(summarize(&[]), (0, BTreeMap::new()));
    }

    #[test]
    fn fixtures_are_complete() {
        let fx = fixtures();
        let names: Vec<&str> = fx.iter().map(|f| f.0.as_str()).collect();
        assert_eq!(names, ["01-news", "02-docs", "03-recipe", "04-table", "05-forum", "06-email", "07-report", "08-control"]);
        for (name, text) in &fx {
            let plain = load(text, Mode::Plain);
            let marked = load(text, Mode::Marked);
            let spoken = ra_text::normalize(&plain.tokens).iter().filter(|w| w.text.chars().any(char::is_alphanumeric)).count();
            assert!((120..=250).contains(&spoken), "{name}: {spoken} words");
            let kinds = |l: &Loaded| l.bounds.iter().map(|b| b.1).collect::<Vec<_>>();
            assert_eq!(kinds(&plain), kinds(&marked), "{name}");
            let w = ra_text::normalize(&marked.tokens);
            for &(tok, kind) in &marked.bounds {
                let (_, next) = bound_words(&w, tok).expect("bound");
                assert_eq!(w[next].brk, kind, "{name} at {}", w[next].text);
            }
        }
        // the "Key Takeaways" example: an unpunctuated sentence end, then an H2
        let news = load(&fx[0].1, Mode::Plain);
        let w = ra_text::normalize(&news.tokens);
        let key = news.bounds.iter().find_map(|&(t, k)| k.is_heading().then(|| bound_words(&w, t))).flatten().expect("h2");
        assert_eq!((w[key.0].text.as_str(), w[key.1].text.as_str()), ("2024", "Key"));
        let table = load(&fx[3].1, Mode::Plain);
        let count = |k: Brk| table.bounds.iter().filter(|b| b.1 == k).count();
        assert_eq!((count(Brk::TableRow), count(Brk::TableCell)), (4, 12), "header + 3 rows, 4 columns");
    }

    #[test]
    fn targets_follow_the_pause_model() {
        assert!((target_ms(Brk::Paragraph, 2.2) - 342.6).abs() < 1.0);
        assert!((target_ms(Brk::AfterHeading, 2.2) - 405.0).abs() < 1.0);
        assert_eq!(target_ms(Brk::Line, 3.0), 150.0);
        assert_eq!(target_ms(Brk::Heading(2), 1.0), 450.0);
        assert_eq!(target_ms(Brk::None, 2.2), 0.0);
    }
}
