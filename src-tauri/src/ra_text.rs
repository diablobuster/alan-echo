//! Read Aloud — turn raw UIA word tokens into speakable words and chunks.
use std::borrow::Cow;
use std::ops::Range;

use crate::ra_struct::{self, Brk};

#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub text: String,
    pub src: Vec<usize>,
    pub para_break_before: bool,
    /// The structural boundary before this word (natural-reading spec §1): a source
    /// marker, else `Paragraph` at a line break, else `None`.
    pub brk: Brk,
}

fn is_dash(s: &str) -> bool {
    matches!(s, "-" | "\u{2010}" | "\u{2011}")
}

fn has_alnum(s: &str) -> bool {
    s.chars().any(|c| c.is_alphanumeric())
}

pub fn normalize(raw_in: &[String]) -> Vec<Word> {
    // in-band structure markers (`ra_struct::mark`): stripped here, kept per token
    let mut marks: Vec<Option<Brk>> = Vec::with_capacity(raw_in.len());
    let raw: Vec<String> = raw_in
        .iter()
        .map(|t| {
            let (b, rest) = ra_struct::strip_mark(t);
            marks.push(b);
            rest.to_string()
        })
        .collect();
    let raw = &raw[..];
    let mut out: Vec<Word> = Vec::new();
    let mut pending_para = false;
    // true when the previous raw token ended without whitespace (glued to the next)
    let mut prev_glued = false;
    // a source marker waiting for the next word (a marked token never glues on)
    let mut pending_mark: Option<Brk> = None;
    let mut i = 0;
    while i < raw.len() {
        if let Some(b) = marks[i] {
            pending_mark = Some(b);
            pending_para = true;
            prev_glued = false;
        }
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
        // glued pieces of one written word: `$` `3` `.` `2B` -> "$3.2B" (spec §2)
        if prev_glued && !tok.starts_with(char::is_whitespace) && !pending_para && !breaks_before(raw, i) {
            if let Some(last) = out.last_mut() {
                // a letter glued to a following "." (`U` `.` `S` `.`): an initialism
                let dot_next = glued && raw.get(i + 1).is_some_and(|n| n.starts_with('.'));
                let prev_raw = i.checked_sub(1).and_then(|j| raw.get(j)).map_or("", |s| s.as_str());
                if joins_glued(&last.text, t, dot_next, prev_raw) {
                    // tokens glued on as punctuation (the ".") are part of this word too
                    let from = last.src.last().map_or(i, |s| s + 1);
                    last.src.extend(from..=i);
                    last.text.push_str(t);
                    pending_para |= breaks;
                    prev_glued = glued;
                    i += 1;
                    continue;
                }
            }
        }
        // a currency or number sign after a space starts its own word (`cost $` `3`)
        if !prev_glued && is_prefix_symbol(t) {
            out.push(Word { text: t.to_string(), src: vec![i], para_break_before: pending_para && !out.is_empty(), brk: brk_before(&mut pending_mark, pending_para && !out.is_empty()) });
            pending_para = breaks;
            prev_glued = glued;
            i += 1;
            continue;
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
            // punctuation-only token glues onto the previous word (text only), except a
            // spoken symbol after a space: "dispute ¶ 8", "β = 0.62", "clause (§ 14.2)"
            if !has_alnum(t) && !is_dash(t) && !pending_para && !(!prev_glued && is_spoken_symbol(t)) {
                last.text.push_str(t);
                pending_para |= breaks;
                prev_glued = glued;
                i += 1;
                continue;
            }
        }
        out.push(Word { text: t.to_string(), src: vec![i], para_break_before: pending_para && !out.is_empty(), brk: brk_before(&mut pending_mark, pending_para && !out.is_empty()) });
        pending_para = breaks;
        prev_glued = glued;
        i += 1;
    }
    out
}

/// `Word::brk` for a new word: the source's marker, else `Paragraph` after a line
/// break, else `None`.
fn brk_before(pending_mark: &mut Option<Brk>, para: bool) -> Brk {
    pending_mark.take().unwrap_or(if para { Brk::Paragraph } else { Brk::None })
}

fn breaks_before(raw: &[String], i: usize) -> bool {
    i > 0 && (raw[i - 1].contains('\r') || raw[i - 1].contains('\n'))
}

/// A token that is a symbol read aloud on its own (`¶`, `¶¶`, `=`, `<`, `(§`), possibly
/// opened by a bracket and closed by punctuation.
fn is_spoken_symbol(t: &str) -> bool {
    let core = t.trim_start_matches(['(', '[']).trim_end_matches([',', ';', ':', '.', ')', ']']);
    !core.is_empty() && core.chars().all(|c| matches!(c, '¶' | '§' | '=' | '<' | '>' | '≤' | '≥' | '≈' | '≠' | '×' | '÷' | '±'))
}

fn is_prefix_char(c: char) -> bool {
    matches!(c, '$' | '€' | '£' | '¥' | '₹' | '#' | '§' | '±' | '−')
}

/// A token made only of currency or number signs (`$`, `#`, `§`).
fn is_prefix_symbol(t: &str) -> bool {
    !t.is_empty() && t.chars().all(is_prefix_char)
}

/// Letters that stay glued to a number in one written word: ordinals, clock
/// times, scale suffixes and common units (`21st`, `7:30pm`, `2B`, `5km`).
const NUM_SUFFIX: &[&str] = &[
    "AM", "B", "GB", "GHz", "Hz", "K", "KB", "M", "MB", "MHz", "PM", "T", "TB", "a", "am", "b", "bn", "cm", "g", "h", "k",
    "kB", "kW", "kWh", "kg", "km", "m", "mg", "min", "mm", "mph", "nd", "p", "pm", "rd", "s", "st", "th", "tn", "x",
];

/// Whether glued token `t` continues the written word `prev` (spec §2: digits,
/// `.`, `,`, `$`, `%`, `/`, `:` and letters). `prev_raw` is the raw token just
/// before `t`. Deliberately narrow, so a PDF line end without a space (`end.` +
/// `Next`, `2019` + `the`, `3,` + `2024`, `Q3` + `2025`) never fuses two words.
fn joins_glued(prev: &str, t: &str, dot_next: bool, prev_raw: &str) -> bool {
    let mut back = prev.chars().rev();
    let (Some(a), Some(b)) = (back.next(), t.chars().next()) else { return false };
    let digit_before = back.next().is_some_and(|c| c.is_ascii_digit());
    if b.is_ascii_digit() {
        let digits_only = |s: &str| s.chars().all(|c| c.is_ascii_digit());
        let lead_digits = t.chars().take_while(char::is_ascii_digit).count();
        // a year split by glyph runs: `19` `90` (two other numbers stay two)
        if matches!(prev, "19" | "20") && t.len() == 2 && digits_only(t) {
            return true;
        }
        if a.is_ascii_digit() {
            // never two all-digit tokens (a line end), nor `Q3` + `2025`
            let tail = prev.trim_end_matches(|c: char| c.is_ascii_digit());
            let letter_led = tail.chars().next_back().is_some_and(char::is_alphabetic);
            return !(digits_only(prev) && digits_only(t)) && !(letter_led && lead_digits >= 3);
        }
        if !digit_before {
            // $|3
            return is_prefix_char(a);
        }
        return match a {
            // 1,|000: only a thousands group (`3,` + `2024` is a date at a line end)
            ',' => lead_digits == 3,
            // 3.|2 only when the "." was its own glued token (`2019.` + `2020` ends a sentence)
            '.' => prev_raw == ".",
            // 7:|30, 3/|14, and a range split at a line end: 2019–|2021
            ':' | '/' | '-' | '\u{2013}' => true,
            _ => is_prefix_char(a),
        };
    }
    if !b.is_alphabetic() {
        return false;
    }
    if a.is_ascii_digit() {
        return NUM_SUFFIX.contains(&t) && suffix_fits(prev, t);
    }
    if a != '.' || !t.chars().all(char::is_alphabetic) {
        return false;
    }
    // dotted initialism: U.|S, e.|g, Ph.|D (never "end." + "Next" or "it." + "So").
    // The letter must have its own "." after it: "Plan B." + "I" at a line end
    // is two words.
    if !dot_next {
        return false;
    }
    let head = prev.strip_suffix('.').unwrap_or(prev);
    let run = head.rsplit('.').next().unwrap_or(head);
    let (rn, tn) = (run.chars().count(), t.chars().count());
    let run_letters = run.chars().all(char::is_alphabetic);
    let caps_led = run.chars().next().is_some_and(char::is_uppercase);
    // "U.S." + "It", "Plan B." + "We": a capitalised word after a sentence end
    let word_like = tn == 2 && t.chars().next().is_some_and(char::is_uppercase) && t.chars().nth(1).is_some_and(char::is_lowercase);
    run_letters && tn <= 2 && !word_like && (rn == 1 || (rn == 2 && tn == 1 && caps_led))
}

/// Suffixes that are also words (`a`, `am`, `s`, `m`, ...) merge only into a known
/// pattern, so `2019` + `a` at a PDF line end stays two words: a clock time
/// (`7am`, `7:30pm`, `7:30a`), a decade (`1990s`, `90s`), or money (`$5m`).
fn suffix_fits(prev: &str, t: &str) -> bool {
    let digits: String = prev.chars().rev().take_while(char::is_ascii_digit).collect();
    match t {
        // `7:30a`; a bare `10` + `a` at a line end is "10 a day", never "ten A"
        "a" | "p" => prev.contains(':'),
        "am" | "pm" | "AM" | "PM" => prev.contains(':') || digits.len() <= 2,
        "s" => (digits.len() == 4 || digits.len() == 2) && digits.starts_with('0') && !prev.contains(['.', ',', '$']),
        "m" | "M" | "k" | "K" | "b" | "B" | "bn" | "tn" | "T" => prev.starts_with(is_prefix_char) || t.len() > 1 || t.chars().all(char::is_uppercase),
        _ => true,
    }
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
    if ABBREV.contains(&lower.as_str()) || crate::ra_norm_text::never_ends_sentence(core) {
        return false;
    }
    // single capital initial like "A." or "J."
    let letters: Vec<char> = core.chars().filter(|c| c.is_alphabetic()).collect();
    !(letters.len() == 1 && letters[0].is_uppercase() && core.len() == 2)
}

/// A short month (`Jan.`, `Sept.`) before a word that starts with a digit is a
/// date ("Jan. 5, 2024"), never a sentence end: a cut there reads "Jan" as a name
/// with a falling tone. "I met Jan. She ..." still ends the sentence.
fn month_before_number(w: &str, next: Option<&Word>) -> bool {
    const SHORT_MONTHS: &[&str] =
        &["Jan.", "Feb.", "Mar.", "Apr.", "Jun.", "Jul.", "Aug.", "Sept.", "Sep.", "Oct.", "Nov.", "Dec."];
    SHORT_MONTHS.contains(&w) && next.is_some_and(|n| n.text.starts_with(|c: char| c.is_ascii_digit()))
}

/// `a.m.` / `p.m.` before a time-zone abbreviation ("3 p.m. ET") is one phrase,
/// never a sentence end: a cut there read the zone as a new sentence.
fn clock_before_zone(w: &str, next: Option<&Word>) -> bool {
    let clock = matches!(w.to_ascii_lowercase().as_str(), "a.m." | "p.m.") || w.ends_with("a.m.") || w.ends_with("p.m.");
    clock && next.is_some_and(|n| is_time_zone(&n.text))
}

/// A time-zone abbreviation (`ET`, `PST`, `UTC`), trailing punctuation ignored.
pub(crate) fn is_time_zone(w: &str) -> bool {
    const ZONES: &[&str] = &[
        "ET", "PT", "CT", "MT", "EST", "EDT", "PST", "PDT", "CST", "CDT", "MST", "MDT", "AKST", "AKDT", "HST", "UTC",
        "GMT", "BST", "CET", "CEST", "EET", "EEST", "WET", "IST", "JST", "KST", "AEST", "AEDT", "SGT", "HKT",
    ];
    ZONES.contains(&w.trim_end_matches(|c: char| !c.is_alphanumeric()))
}

/// Conjunctions, prepositions and relatives a phrase can start with: a chunk cut
/// just before one sounds like a natural breath.
fn joins_phrase(w: &str) -> bool {
    const JOIN: &[&str] = &[
        "about", "after", "against", "and", "as", "at", "because", "before", "between", "but", "by", "during",
        "for", "from", "if", "in", "into", "like", "nor", "of", "on", "or", "over", "since", "so", "than",
        "that", "through", "to", "under", "unless", "until", "when", "where", "whereas", "which", "while",
        "who", "whose", "with", "within", "without",
    ];
    JOIN.contains(&w)
}

fn has_digit(s: &str) -> bool {
    s.bytes().any(|b| b.is_ascii_digit())
}

/// A cut before `words[k]` would split "September 2," from its year "2021".
fn date_comma(words: &[Word], k: usize) -> bool {
    const MONTHS: &[&str] = &[
        "Jan", "Jan.", "January", "Feb", "Feb.", "February", "March", "Mar.", "April", "Apr.", "May", "June", "July",
        "Aug", "Aug.", "August", "Sept", "Sept.", "Sep.", "September", "Oct", "Oct.", "October", "Nov", "Nov.",
        "November", "Dec", "Dec.", "December",
    ];
    let day = words[k - 1].text.trim_end_matches(',');
    k >= 2
        && day.len() <= 2
        && day.chars().all(|c| c.is_ascii_digit())
        && MONTHS.contains(&words[k - 2].text.as_str())
        && words.get(k).is_some_and(|w| w.text.len() >= 4 && w.text.chars().take(4).all(|c| c.is_ascii_digit()))
}

/// An abbreviation's period inside a sentence: the next word starts lowercase
/// ("1 tbsp. kosher", "9 a.m. to", "(D-Mass.) called"), or a short abbreviation is
/// followed by a number or "\u{a7}" ("Cat. 5", "H.R. 4366", "U.S.C. \u{a7} 230").
/// A number's period ("in 1990. 2001 was") still ends the sentence.
fn abbrev_continues(w: &str, next: Option<&Word>) -> bool {
    let Some(n) = next else { return false };
    let core = w.trim_end_matches(|c| matches!(c, '"' | '\'' | ')' | ']' | '\u{201D}' | '\u{2019}'));
    let Some(body) = core.strip_suffix('.') else { return false };
    if !body.chars().any(char::is_alphabetic) || body.chars().any(|c| c.is_ascii_digit()) {
        return false;
    }
    let nt = n.text.trim_start_matches(['(', '[', '"', '\'', '\u{201C}']);
    let Some(c) = nt.chars().next() else { return false };
    if c.is_lowercase() {
        return true;
    }
    let letters = body.chars().filter(|c| c.is_alphabetic()).count();
    // "Fed. R. Civ. P. 12": a short abbreviation before a capital initial ("R.", "P.")
    let initial_next = nt.len() == 2 && c.is_ascii_uppercase() && nt.ends_with('.');
    (c.is_ascii_digit() || c == '\u{a7}' || initial_next) && letters <= 4
}

fn ends_clause(w: &str) -> bool {
    w.ends_with([',', ';', ':'])
}

#[cfg_attr(not(test), allow(dead_code))] // fixed limits: tests; the app uses `chunk_ramped`
pub fn chunk(words: &[Word], first_max: usize, max: usize) -> Vec<Range<usize>> {
    let (first_max, max) = (first_max.max(1), max.max(1));
    chunk_with(words, &|out: &[Range<usize>]| if out.is_empty() { first_max } else { max }, &|_: &[Range<usize>]| 1)
}

/// Synth time per word as a share of the word's playback time, with margin. Fitted on
/// the CPU model in release (round 2 gapless bench, chunk silence trimmed): 0.56 at
/// 2.2x and 0.63 at 3.5x; numbers and symbols expand to more sounds per written word,
/// and other apps take CPU, hence the margin.
const RAMP_Q: f64 = 0.7;
/// `RAMP_Q` at 3.5x (`ramp_q`).
const RAMP_Q_FAST: f64 = 0.8;
/// Fixed synth cost per chunk, net of the fixed audio each chunk adds, in words of
/// playback: fitted 1.8 at 2.2x and 1.2 at 3.5x.
const RAMP_O: f64 = 2.0;
/// Smallest ramp chunk: below this the per-chunk cost dominates, and a run of tiny
/// chunks sounds choppy (each chunk ends on a falling tone). In the ramp a shorter
/// sentence joins the next one.
const RAMP_MIN: usize = 6;

/// Smallest first chunk (`RAMP_MIN`), and above `FAST_RATE` this: a 4-word first
/// clause plays for under half a second there, less than any next chunk takes to make.
const FAST_FIRST_MIN: usize = 8;
const FAST_RATE: f32 = 2.5;

/// The synth share at reading speed `rate`: above 2.2x the model speaks faster
/// (`ra_tts::split_rate`) but its cost per word falls less than the words' playback
/// time (fitted 0.56 at 2.2x; 0.80 at 3.5x with the model capped at 1.25), so the share
/// rises to `RAMP_Q_FAST`.
fn ramp_q(rate: f32) -> f64 {
    let t = ((rate as f64 - 2.2) / 1.3).clamp(0.0, 1.0);
    RAMP_Q + (RAMP_Q_FAST - RAMP_Q) * t
}

/// The word limit for the next chunk of a ramp whose earlier chunks have `sizes`
/// words, at reading speed `rate`: the largest chunk the CPU voice can synthesize
/// while the chunks before it play, so the audio never runs dry. With `w0` the first
/// chunk (synthesized before the first audio), chunk n must satisfy
/// `q * (W_n - w0) + n * RAMP_O <= W_(n-1)` (W = words so far, the n-th chunk
/// included on the left; q = `ramp_q(rate)`). At 2.2x: 12 -> 14 -> 17 -> 21 -> 27 -> 30;
/// at 3.5x 12 -> 12 -> 13 -> 13 -> 14 -> 15 -> 16 -> 18 -> ... -> 30.
pub fn ramp_limit(sizes: &[usize], first_max: usize, max: usize, rate: f32) -> usize {
    let Some(&w0) = sizes.first() else { return first_max.max(1) };
    let w: f64 = sizes.iter().sum::<usize>() as f64;
    let n = sizes.len() as f64;
    let budget = (w - n * RAMP_O) / ramp_q(rate) + w0 as f64 - w;
    (budget.max(0.0).floor() as usize).clamp(RAMP_MIN.min(max), max.max(1))
}

/// `chunk` with ramped limits (`ramp_limit`) so playback is gapless from the first
/// word on, at reading speed `rate`. `prior`: sizes of the chunks of the same ramp
/// already made (a re-chunk of the not-yet-submitted tail passes them; a fresh start
/// passes none). While the ramp is below `max`, no chunk is shorter than
/// `RAMP_MIN` words (8 for the first chunk above 2.5x): a shorter clause is not a cut,
/// and a shorter sentence joins the next one (not across a paragraph break). After
/// the ramp, chunks are cut as `chunk` cuts them.
pub fn chunk_ramped(words: &[Word], prior: &[usize], first_max: usize, max: usize, rate: f32) -> Vec<Range<usize>> {
    let sizes = |out: &[Range<usize>]| -> Vec<usize> { prior.iter().copied().chain(out.iter().map(|r| r.len())).collect() };
    chunk_with(
        words,
        &|out: &[Range<usize>]| ramp_limit(&sizes(out), first_max, max, rate),
        &|out: &[Range<usize>]| {
            let sz = sizes(out);
            if sz.is_empty() {
                (if rate > FAST_RATE { FAST_FIRST_MIN } else { RAMP_MIN }).min(first_max)
            } else if ramp_limit(&sz, first_max, max, rate) < max {
                RAMP_MIN.min(max)
            } else {
                1
            }
        },
    )
}

/// Sentences, then each cut to `limit(chunks so far)` words (at a clause when possible).
/// No chunk is shorter than `min_len(chunks so far)` words where it can be avoided: no
/// cut that leaves less, and a shorter sentence joins the next (same paragraph).
fn chunk_with(
    words: &[Word],
    limit: &dyn Fn(&[Range<usize>]) -> usize,
    min_len: &dyn Fn(&[Range<usize>]) -> usize,
) -> Vec<Range<usize>> {
    // 1) sentences
    let mut sentences: Vec<Range<usize>> = Vec::new();
    let mut start = 0;
    for (i, w) in words.iter().enumerate() {
        if i > start && w.para_break_before {
            sentences.push(start..i);
            start = i;
        }
        if ends_sentence(&w.text)
            && !month_before_number(&w.text, words.get(i + 1))
            && !clock_before_zone(&w.text, words.get(i + 1))
            && !abbrev_continues(&w.text, words.get(i + 1))
        {
            sentences.push(start..i + 1);
            start = i + 1;
        }
    }
    if start < words.len() {
        sentences.push(start..words.len());
    }
    // 2) enforce limits (first chunk smaller, cut at a clause when possible)
    let mut out = Vec::new();
    // start of a short chunk that joins the next sentence
    let mut carry: Option<usize> = None;
    for (si, s) in sentences.iter().enumerate() {
        let mut a = carry.take().unwrap_or(s.start);
        while a < s.end {
            let limit = limit(&out).max(1); // a 0 limit would never advance
            let min = min_len(&out).clamp(1, limit);
            if s.end - a <= limit {
                let next_joins = sentences.get(si + 1).is_some_and(|n| !words[n.start].para_break_before);
                if s.end - a < min && next_joins {
                    carry = Some(a);
                } else {
                    out.push(a..s.end);
                }
                break;
            }
            let hard = a + limit;
            let cut = ((a + min).min(hard)..hard)
                .rev()
                .find(|&k| ends_clause(&words[k - 1].text) && !date_comma(words, k))
                .unwrap_or_else(|| {
                    // no clause end: never part a number from its neighbours
                    // (`450` | `GeV`, `in` | `1990`) when a cut nearby avoids it,
                    // and best cut before a joining word (`… references | as most …`)
                    // so the chunk's falling tone lands at a phrase edge
                    // and never right after an abbreviation inside the sentence (`U.S. | evidence`)
                    let quiet = |k: usize| {
                        !has_digit(&words[k - 1].text)
                            && !has_digit(&words[k].text)
                            && !abbrev_continues(&words[k - 1].text, words.get(k))
                    };
                    let near = a + (1 + limit / 2).max(min)..=hard;
                    near.clone()
                        .rev()
                        .find(|&k| quiet(k) && joins_phrase(&words[k].text))
                        .or_else(|| near.rev().find(|&k| quiet(k)))
                        // a short ramp chunk: an earlier quiet gap still beats `3.5 | ft`
                        .or_else(|| (a + 1..hard).rev().find(|&k| quiet(k)))
                        .unwrap_or(hard)
                });
            out.push(a..cut);
            a = cut;
        }
    }
    out
}
/// ALL-CAPS words misaki tends to spell letter by letter in an all-caps sentence.
/// Returns the form to feed G2P. A caps word is lowercased (so it is read as a
/// word) when misaki knows the lowercase word (`known`), the names dictionary
/// knows it (`names_knows`, e.g. PELOSI), or it is 5+ letters (the letter-to-sound
/// model says it). It stays caps (spelled) when it is a common acronym
/// (`KEEP_CAPS`, `ra_names::SPELLED`), has no vowel (HTTPS), the lexicon has the
/// caps spelling itself (`known(upper)`, e.g. NATO), or it is a short unknown (GDP).
/// Leading/trailing punctuation, possessive 'S and hyphen-joined parts are handled.
/// Never panics.
pub fn speakable_caps<'a>(word: &'a str, known: &dyn Fn(&str) -> bool, names_knows: &dyn Fn(&str) -> bool) -> Cow<'a, str> {
    if !word.contains('-') {
        return caps_part(word, known, names_knows);
    }
    let parts: Vec<Cow<str>> = word.split('-').map(|p| caps_part(p, known, names_knows)).collect();
    if parts.iter().all(|p| matches!(p, Cow::Borrowed(_))) {
        Cow::Borrowed(word)
    } else {
        Cow::Owned(parts.join("-"))
    }
}

/// Common acronyms whose lowercase form is also an English word.
pub(crate) const KEEP_CAPS: &[&str] = &["US", "IT", "WHO", "AM", "PM", "OK", "AI", "ID", "TV", "UK", "EU", "UN", "IQ", "PC"];

/// Unknown caps words this long are more likely words or names than acronyms.
const SAY_CAPS_LEN: usize = 5;

fn caps_part<'a>(w: &'a str, known: &dyn Fn(&str) -> bool, names_knows: &dyn Fn(&str) -> bool) -> Cow<'a, str> {
    let Some(start) = w.find(char::is_alphanumeric) else { return Cow::Borrowed(w) };
    let Some((last, ch)) = w.char_indices().rev().find(|(_, c)| c.is_alphanumeric()) else { return Cow::Borrowed(w) };
    let end = last + ch.len_utf8();
    let core = &w[start..end];
    let base = core.strip_suffix("'S").or_else(|| core.strip_suffix("\u{2019}S")).unwrap_or(core);
    if base.chars().count() < 2 || !base.chars().all(|c| c.is_alphabetic() && c.is_uppercase()) {
        return Cow::Borrowed(w);
    }
    // an acronym (spelled list) or a caps run with no vowel (HTTPS, NFLX) is never a word
    let no_vowel = !base.chars().any(|c| matches!(c, 'A' | 'E' | 'I' | 'O' | 'U' | 'Y'));
    if KEEP_CAPS.contains(&base) || crate::ra_names::SPELLED.contains(&base) || no_vowel || known(base) {
        return Cow::Borrowed(w);
    }
    let lower = base.to_lowercase();
    if !(known(&lower) || names_knows(&lower) || base.chars().count() >= SAY_CAPS_LEN) {
        return Cow::Borrowed(w);
    }
    Cow::Owned(format!("{}{}{}", &w[..start], core.to_lowercase(), &w[end..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_markers_set_brk_and_never_glue() {
        let h = |b: Brk, t: &str| format!("{}{t}", ra_struct::mark(b));
        let raw: Vec<String> = vec![
            h(Brk::Heading(2), "Key "),
            "Takeaways".into(), // no trailing space: glued to the next token
            h(Brk::AfterHeading, "The "),
            "company ".into(),
            "grew.
".into(),
            "Then ".into(),
            h(Brk::ListItem, ""),
            "Apples".into(),
        ];
        let w = normalize(&raw);
        let got: Vec<(&str, Brk, bool)> = w.iter().map(|w| (w.text.as_str(), w.brk, w.para_break_before)).collect();
        assert_eq!(
            got,
            vec![
                ("Key", Brk::Heading(2), false),
                ("Takeaways", Brk::None, false),
                ("The", Brk::AfterHeading, true),
                ("company", Brk::None, false),
                ("grew.", Brk::None, false),
                ("Then", Brk::Paragraph, true),
                ("Apples", Brk::ListItem, true),
            ]
        );
        assert_eq!(w[6].src, vec![7], "the empty marked token is not a word; the marker carries on");
    }

    fn known(w: &str) -> bool {
        ["the", "and", "on", "for", "talks", "president", "us", "it", "who", "am", "pm", "ok", "id", "mid", "year", "NATO"]
            .contains(&w)
    }

    fn caps(w: &str) -> String {
        speakable_caps(w, &known, &|_| false).into_owned()
    }

    fn cmu(w: &str) -> bool {
        ["pelosi", "schumer"].contains(&w)
    }

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

    #[test]
    fn caps_dictionary_words_are_lowercased() {
        assert_eq!(caps("THE"), "the");
        assert_eq!(caps("TALKS"), "talks");
        assert_eq!(caps("AND"), "and");
        assert_eq!(caps("THE,"), "the,");
        assert_eq!(caps("\u{201C}FOR"), "\u{201C}for");
        assert_eq!(caps("PRESIDENT'S"), "president's");
        assert_eq!(caps("PRESIDENT\u{2019}S."), "president\u{2019}s.");
        assert_eq!(caps("MID-YEAR"), "mid-year");
        assert_eq!(caps("THE\u{1F600}"), "the\u{1F600}");
    }

    #[test]
    fn caps_acronyms_and_unknowns_stay() {
        for w in ["FBI", "GDP", "CEO", "US", "IT", "WHO", "AM", "PM", "OK", "ID", "US.", "NATO", "NASA", "A", "I"] {
            assert_eq!(caps(w), w, "{w}");
        }
    }

    #[test]
    fn caps_without_a_vowel_or_on_the_spelled_list_stay_caps() {
        // HTTPS is 5+ letters but has no vowel; AWS/SEC/FYI are words to some dictionary
        // every lowercase word is known; no caps spelling is
        let lower_known = |w: &str| !w.chars().any(char::is_uppercase);
        for w in ["HTTPS", "NFLX", "HTTPS,", "AWS", "SEC", "SEC'S", "FYI", "CPU"] {
            assert_eq!(speakable_caps(w, &lower_known, &lower_known), w, "{w}");
        }
        assert_eq!(speakable_caps("SPY", &known, &|w| w == "spy"), "spy", "Y counts as a vowel");
    }

    #[test]
    fn caps_leaves_mixed_and_odd_input_alone() {
        for w in ["iPhone", "McDonald", "The", "the", "COVID19", "3PM", "123", "", "-", "\u{1F600}", "\u{6771}\u{4EAC}", "'S", "--", "\u{00C9}T\u{00C9}"] {
            assert_eq!(caps(w), w, "{w:?}");
        }
    }
    fn raw(v: &[&str]) -> Vec<String> { v.iter().map(|s| s.to_string()).collect() }

    #[test]
    fn a_spoken_symbol_after_a_space_is_its_own_word() {
        // round 3: "dispute ¶ 8", "β = 0.62", "clause (§ 14.2)" were glued onto the
        // previous word as punctuation and never read
        let t = |v: &[&str]| normalize(&raw(v)).into_iter().map(|w| w.text).collect::<Vec<_>>();
        assert_eq!(t(&["dispute ", "¶ ", "8"]), ["dispute", "¶", "8"]);
        assert_eq!(t(&["in ", "¶¶ ", "3-17"]), ["in", "¶¶", "3-17"]);
        assert_eq!(t(&["β ", "= ", "0.62"]), ["β", "=", "0.62"]);
        assert_eq!(t(&["p ", "< ", "0.05"]), ["p", "<", "0.05"]);
        assert_eq!(t(&["clause ", "(§ ", "14.2)"]), ["clause", "(§", "14.2)"]);
        // glued punctuation still glues; plain punctuation after a space too
        assert_eq!(t(&["end", ". ", "Next"]), ["end.", "Next"]);
        assert_eq!(t(&["wait ", "... ", "what"]), ["wait...", "what"]);
    }
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

    // Review Focus 2: UIA splits `$3.2B` into `$` `3` `.` `2B` (glued = no trailing
    // whitespace); the pre-pass must see the whole written word.
    #[test]
    fn glued_number_tokens_merge_into_one_word() {
        let w = normalize(&raw(&["it ", "cost ", "$", "3", ".", "2B", " in ", "Q3", "."]));
        assert_eq!(texts(&w), vec!["it", "cost", "$3.2B", "in", "Q3."]);
        assert_eq!(w[2].src, vec![2, 3, 4, 5], "every token of the merged word is highlighted");
        assert_eq!(texts(&normalize(&raw(&["8", ".", "6", "% "]))), vec!["8.6%"]);
        assert_eq!(texts(&normalize(&raw(&["7", ":", "30", "pm "]))), vec!["7:30pm"]);
        assert_eq!(texts(&normalize(&raw(&["U", ".", "S", ". "]))), vec!["U.S."]);
        assert_eq!(texts(&normalize(&raw(&["about ", "1", ",", "000 ", "people"]))), vec!["about", "1,000", "people"]);
        assert_eq!(texts(&normalize(&raw(&["on ", "3", "/", "14 "]))), vec!["on", "3/14"]);
        assert_eq!(texts(&normalize(&raw(&["the ", "21", "st ", "time"]))), vec!["the", "21st", "time"]);
        assert_eq!(texts(&normalize(&raw(&["Ph", ".", "D", ". "]))), vec!["Ph.D."]);
    }

    #[test]
    fn number_suffixes_merge_only_in_known_patterns() {
        let t = |v: &[&str]| texts(&normalize(&raw(v))).into_iter().map(String::from).collect::<Vec<_>>();
        assert_eq!(t(&["at ", "7:30", "am "]), vec!["at", "7:30am"]);
        assert_eq!(t(&["at ", "7", "pm "]), vec!["at", "7pm"]);
        assert_eq!(t(&["the ", "1990", "s "]), vec!["the", "1990s"]);
        assert_eq!(t(&["the ", "90", "s "]), vec!["the", "90s"]);
        assert_eq!(t(&["raised ", "$5", "m "]), vec!["raised", "$5m"]);
        // a PDF line end without a space: not one word
        assert_eq!(t(&["in ", "2019", "a "]), vec!["in", "2019", "a"]);
        assert_eq!(t(&["in ", "2019", "am "]), vec!["in", "2019", "am"]);
        assert_eq!(t(&["in ", "2019", "s "]), vec!["in", "2019", "s"]);
        assert_eq!(t(&["ran ", "5", "m "]), vec!["ran", "5", "m"]);
        // two all-digit tokens never fuse
        assert_eq!(t(&["page ", "3", "2 "]), vec!["page", "3", "2"]);
        assert_eq!(t(&["about ", "1", ",", "000 "]), vec!["about", "1,000"]);
    }

    #[test]
    fn spaced_tokens_never_merge() {
        assert_eq!(texts(&normalize(&raw(&["cost ", "$ ", "3 ", "2B ", "in"]))), vec!["cost", "$", "3", "2B", "in"]);
        assert_eq!(texts(&normalize(&raw(&["7 ", ": ", "30"]))), vec!["7:", "30"]);
        assert_eq!(texts(&normalize(&raw(&["it ", "2B", " in"]))), vec!["it", "2B", "in"]);
    }

    #[test]
    fn glued_words_that_are_not_one_written_word_stay_apart() {
        // PDF line ends have no trailing space: a sentence end, a year then a word,
        // and two plain words must not fuse.
        assert_eq!(texts(&normalize(&raw(&["the ", "end.", "Next "]))), vec!["the", "end.", "Next"]);
        assert_eq!(texts(&normalize(&raw(&["in ", "2019", "the "]))), vec!["in", "2019", "the"]);
        assert_eq!(texts(&normalize(&raw(&["Hello", "world"]))), vec!["Hello", "world"]);
        assert_eq!(texts(&normalize(&raw(&["U.S.", "The "]))), vec!["U.S.", "The"]);
        // PDF line ends after a sentence-final initialism or one-letter word
        assert_eq!(texts(&normalize(&raw(&["the ", "U.S.", "It "]))), vec!["the", "U.S.", "It"]);
        assert_eq!(texts(&normalize(&raw(&["Plan ", "B.", "We "]))), vec!["Plan", "B.", "We"]);
        // a paragraph break between glued tokens always separates them
        assert_eq!(texts(&normalize(&raw(&["5", "\r", "3 "]))), vec!["5", "3"]);
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
        s.split(' ').enumerate().map(|(i, t)| Word { text: t.into(), src: vec![i], para_break_before: false, brk: Brk::None }).collect()
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
    fn title_abbreviations_do_not_end_a_sentence() {
        // a chunk cut after "Sen." would read "sen" and lose "senator"
        let w = words("Sen. Warren and Gen. Milley met Lt. Dan. Done.");
        assert_eq!(chunk(&w, 12, 50), vec![0..8, 8..9]);
    }

    #[test]
    fn first_chunk_cut_at_clause_within_12() {
        let w = words("Eight times a year, twelve people sit around a long table in Washington and decide.");
        let c = chunk(&w, 12, 50);
        assert_eq!(c[0], 0..4); // "Eight times a year,"
        assert_eq!(c[1].start, 4);
    }

    /// Round 2 gapless: every chunk of a ramp fits the synth budget of the chunks
    /// before it; the first chunk is unchanged (first audio as fast as before).
    #[test]
    fn ramped_chunks_fit_the_synth_budget() {
        let s = "Eight times a year, twelve people sit around a long table in Washington and decide what your money \
                 costs. They do not vote on your rent, your car loan, or your credit card, but every one of those prices \
                 moves within days of their meeting. Most people never watch it happen. The room is quiet, the statement \
                 is short, and the market reacts in seconds and then keeps going for a long while without any pause at all \
                 because nothing in the long sentence has a comma or a clause mark anywhere near its middle part here.";
        let w = words(s);
        let c = chunk_ramped(&w, &[], 12, 30, 2.2);
        assert!(c[0].len() >= RAMP_MIN, "no 4-word first chunk: {c:?}");
        let sizes: Vec<usize> = c.iter().map(|r| r.len()).collect();
        for n in 1..sizes.len() {
            let w_prev: usize = sizes[..n].iter().sum();
            let w_now: usize = sizes[..=n].iter().sum();
            let need = ramp_q(2.2) * (w_now - sizes[0]) as f64 + n as f64 * RAMP_O;
            assert!(need <= w_prev as f64 + 1e-9, "chunk {n} too big: {sizes:?}");
            assert!(sizes[n] >= RAMP_MIN || n + 1 == sizes.len() || sizes[n - 1] >= 30, "chunk {n} too small: {sizes:?}");
        }
        assert!(c.iter().all(|r| r.len() <= 30));
        assert_eq!(c.last().unwrap().end, w.len());
        // long plain text ramps up to the max
        let s = (0..200).map(|i| format!("w{i}")).collect::<Vec<_>>().join(" ");
        let c = chunk_ramped(&words(&s), &[], 12, 30, 2.2);
        let sizes: Vec<usize> = c.iter().map(|r| r.len()).collect();
        assert_eq!(&sizes[..6], &[12, 14, 17, 21, 27, 30], "{sizes:?}");
        // a re-chunk deep into a ramp goes straight to the max
        let c = chunk_ramped(&words(&s), &[12, 14, 17, 21, 27], 12, 30, 2.2);
        assert_eq!(c[0].len(), 30);
        // the limit itself
        assert_eq!(ramp_limit(&[], 12, 30, 2.2), 12);
        assert_eq!(ramp_limit(&[4], 12, 30, 2.2), RAMP_MIN);
        assert_eq!(ramp_limit(&[6], 12, 30, 2.2), RAMP_MIN);
        assert_eq!(ramp_limit(&[12, 14, 17], 12, 30, 2.2), 21);
        assert_eq!(ramp_limit(&[12], 12, 30, 2.2), 14);
        // slower ramp at 3.5x
        let c = chunk_ramped(&words(&s), &[], 12, 30, 3.5);
        let sizes: Vec<usize> = c.iter().map(|r| r.len()).collect();
        assert_eq!(&sizes[..8], &[12, 12, 13, 13, 14, 15, 16, 18], "{sizes:?}");
        // a first clause under 6 words (8 at 3.5x) is not a cut
        let w = words("Eight times a year, twelve people sit around a long table in Washington and decide.");
        assert!(chunk_ramped(&w, &[], 12, 30, 2.2)[0].len() >= 6);
        assert!(chunk_ramped(&w, &[], 12, 30, 3.5)[0].len() >= 8);
        // in the ramp a short sentence joins the next one, not across a paragraph
        let w = words("Rates fell. The Fed cut its rate by a quarter point on Wednesday afternoon.");
        let c = chunk_ramped(&w, &[], 12, 30, 2.2);
        assert!(c[0].start == 0 && c[0].end >= 8, "{c:?}");
        let mut w = words("Rates fell. The Fed cut its rate by a quarter point on Wednesday afternoon.");
        w[2].para_break_before = true;
        assert_eq!(chunk_ramped(&w, &[], 12, 30, 2.2)[0], 0..2);
        // a short ramp chunk never parts a number from its unit
        let w = words("The cable was 1 ft long, but the next one was 3.5 ft long.");
        let c = chunk_ramped(&w, &[], 12, 30, 2.2);
        assert!(c.iter().all(|r| w[r.start].text != "ft"), "{c:?}");
        // after the ramp, sentences are chunks of their own again
        let w = words("Rates fell. The Fed cut.");
        assert_eq!(chunk_ramped(&w, &[12, 15, 19, 25, 30], 12, 30, 2.2), vec![0..2, 2..5]);
    }

    /// Round 2: an abbreviation's period before a lowercase word, or a short one
    /// before a number or "§", does not end the sentence (no chunk cut, no pause).
    #[test]
    fn abbreviation_periods_inside_a_sentence_do_not_cut() {
        for s in [
            "Season with 1 tbsp. kosher salt and pepper.",
            "Expect 6-10 in. of snow above 5,000 ft.",
            "Hurricane Milton is a Cat. 5 storm with 180 mph winds.",
            "Section 230 of the Act, 47 U.S.C. § 230, is at issue.",
            "H.R. 4366 passed by a vote of 286-134.",
            "Open from 9 a.m. to 5 p.m. daily.",
            "The court cited Fed. R. Civ. P. 12(b)(6) in its ruling.",
        ] {
            assert_eq!(chunk(&words(s), 30, 50).len(), 1, "{s}");
        }
        // a length cut does not land right after one either
        let w = words("there is no real U.S. evidence to indicate that the Dutch auction fares any better.");
        assert!(chunk(&w, 5, 50).iter().all(|r| r.start == 0 || !w[r.start - 1].text.ends_with('.')));
        // real sentence ends stay cuts
        assert_eq!(chunk(&words("It closed in 1990. 2001 was the next."), 30, 50).len(), 2);
        assert_eq!(chunk(&words("We met at noon. Then we left."), 30, 50).len(), 2);
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
    fn zero_limits_still_progress() {
        let c = chunk(&words("a b c"), 0, 0);
        assert_eq!(c, vec![0..1, 1..2, 2..3]);
    }

    // ---- Task 6 fix rounds ----
    #[test]
    fn line_end_initial_does_not_swallow_the_next_word() {
        // "Plan B." at a line end, then "I" glued (no space)
        assert_eq!(texts(&normalize(&raw(&["a ", "Plan ", "B.", "I ", "said"]))), vec!["a", "Plan", "B.", "I", "said"]);
        assert_eq!(texts(&normalize(&raw(&["Plan ", "B", ".", "I ", "think"]))), vec!["Plan", "B.", "I", "think"]);
        // real initialisms still join
        assert_eq!(texts(&normalize(&raw(&["the ", "U", ".", "S", ". ", "team"]))), vec!["the", "U.S.", "team"]);
        assert_eq!(texts(&normalize(&raw(&["e", ".", "g", ". ", "this"]))), vec!["e.g.", "this"]);
        assert_eq!(texts(&normalize(&raw(&["a ", "Ph", ".", "D", ". "]))), vec!["a", "Ph.D."]);
    }

    #[test]
    fn glyph_split_year_digits_join() {
        assert_eq!(texts(&normalize(&raw(&["in ", "19", "90 ", "the"]))), vec!["in", "1990", "the"]);
        assert_eq!(texts(&normalize(&raw(&["by ", "20", "25."]))), vec!["by", "2025."]);
        // two separate numbers at a line end stay two
        assert_eq!(texts(&normalize(&raw(&["page ", "12", "34 ", "rows"]))), vec!["page", "12", "34", "rows"]);
        assert_eq!(texts(&normalize(&raw(&["in ", "2019", "2020 "]))), vec!["in", "2019", "2020"]);
    }

    #[test]
    fn no_clause_cut_between_a_date_and_its_year() {
        let w = words("a b c d e f g on September 2, 2021 the x y z.");
        let c = chunk(&w, 12, 50);
        // not after "2,": the hard cut moves before "on", keeping the date whole
        assert_eq!(c[0], 0..7);
        assert_eq!(c[1], 7..15);
    }

    #[test]
    fn hard_cut_does_not_split_a_number_from_its_unit() {
        // no clause end; the hard cut at 12 would fall between "450" and "GeV"
        let w = words("a b c d e f g h i j k 450 GeV before the beam moves on to the next ring");
        let c = chunk(&w, 12, 50);
        // the cut moves back to the nearest gap with no number on either side
        assert_eq!(c[0], 0..10);
        assert_eq!(c[1].start, 10);
    }

    #[test]
    fn hard_cut_prefers_the_gap_before_a_joining_word() {
        // no clause end within 12: cut before "as" / "in" / "with", not mid-phrase
        let w = words("Newer brand labels and logos have dropped those old mentions as most modern cables work fine.");
        assert_eq!(chunk(&w, 12, 50)[0], 0..10);
        let w = words("Device linking got easier once makers added a simpler setup step in Bluetooth v2.1 today.");
        assert_eq!(chunk(&w, 12, 50)[0], 0..11);
        // ... but never next to a number ("iPhone 6 | and")
        let w = words("Several owners reported phones bending slightly over months with the iPhone 6 and 6 Plus, mostly.");
        assert_eq!(chunk(&w, 12, 50)[0], 0..8);
    }

    // ---- final review fixes ----
    #[test]
    fn short_month_before_a_number_never_ends_a_sentence() {
        assert_eq!(chunk(&words("On Jan. 5, 2024, it closed."), 12, 50), vec![0..6]);
        assert_eq!(chunk(&words("On Nov. 6 the board met."), 12, 50), vec![0..6]);
        assert_eq!(chunk(&words("Sept. 30"), 12, 50), vec![0..2]);
        for m in ["Jan.", "Feb.", "Mar.", "Apr.", "Aug.", "Sept.", "Sep.", "Oct.", "Nov.", "Dec."] {
            let s = format!("By {m} 12 it was done.");
            assert_eq!(chunk(&words(&s), 12, 50), vec![0..6], "{m}");
        }
        // Jan the name still ends a sentence
        assert_eq!(chunk(&words("I met Jan. She said hi."), 12, 50), vec![0..3, 3..6]);
        // a month at a real sentence end, before a word
        assert_eq!(chunk(&words("It shipped in Nov. Then it sold."), 12, 50), vec![0..4, 4..7]);
    }

    /// Round 1: "3 p.m. ET" is one phrase; a cut after "p.m." read the zone as a
    /// new sentence with a falling tone before it.
    #[test]
    fn clock_time_before_a_time_zone_never_ends_a_sentence() {
        assert_eq!(chunk(&words("The meeting starts at 3 p.m. ET on Monday."), 12, 50), vec![0..9]);
        assert_eq!(chunk(&words("Markets open at 9:30 a.m. EST and close."), 12, 50), vec![0..8]);
        for z in ["ET", "PT", "CT", "MT", "EST", "EDT", "PST", "PDT", "CST", "CDT", "UTC", "GMT", "BST", "CET", "JST"] {
            let s = format!("It airs at 8 p.m. {z} tonight.");
            assert_eq!(chunk(&words(&s), 12, 50), vec![0..7], "{z}");
        }
        // a real sentence end after a time
        assert_eq!(chunk(&words("We left at 5 p.m. Then it rained."), 12, 50), vec![0..5, 5..8]);
        // the zone at the very end still ends the sentence
        assert_eq!(chunk(&words("It ends at 4 p.m. EST. Then it rained."), 12, 50), vec![0..6, 6..9]);
    }

    #[test]
    fn line_end_punctuated_number_does_not_glue_the_next_number() {
        let t = |v: &[&str]| texts(&normalize(&raw(v))).into_iter().map(String::from).collect::<Vec<_>>();
        // `3,` then `2024` at a PDF line end: a date, two words
        assert_eq!(t(&["Jan. ", "3", ",", "2024 "]), vec!["Jan.", "3,", "2024"]);
        assert_eq!(t(&["Jan. ", "3,", "2024 "]), vec!["Jan.", "3,", "2024"]);
        // `2019.` (one token) then `2020`: a sentence end, not a decimal
        assert_eq!(t(&["in ", "2019.", "2020 ", "was"]), vec!["in", "2019.", "2020", "was"]);
        // `Q3` then `2025`
        assert_eq!(t(&["in ", "Q3", "2025 "]), vec!["in", "Q3", "2025"]);
        // `10` then `a`
        assert_eq!(t(&["about ", "10", "a ", "day"]), vec!["about", "10", "a", "day"]);
        assert_eq!(t(&["at ", "7", "a ", "dog"]), vec!["at", "7", "a", "dog"]);
        // the real merges still hold
        assert_eq!(t(&["about ", "1", ",", "000 ", "people"]), vec!["about", "1,000", "people"]);
        assert_eq!(t(&["1", ",", "000", ",", "000 "]), vec!["1,000,000"]);
        assert_eq!(t(&["$", "3", ".", "2B "]), vec!["$3.2B"]);
        assert_eq!(t(&["8", ".", "6", "% "]), vec!["8.6%"]);
        assert_eq!(t(&["7", ":", "30", "pm "]), vec!["7:30pm"]);
        assert_eq!(t(&["7:30", "am "]), vec!["7:30am"]);
        assert_eq!(t(&["U", ".", "S", ". "]), vec!["U.S."]);
        assert_eq!(t(&["on ", "3", "/", "14 "]), vec!["on", "3/14"]);
    }

    #[test]
    fn line_end_dash_after_a_year_rejoins_the_range() {
        let t = |v: &[&str]| texts(&normalize(&raw(v))).into_iter().map(String::from).collect::<Vec<_>>();
        assert_eq!(t(&["from ", "2019\u{2013}", "2021 ", "it"]), vec!["from", "2019\u{2013}2021", "it"]);
        assert_eq!(t(&["from ", "2019", "\u{2013}", "2021 "]), vec!["from", "2019\u{2013}2021"]);
        assert_eq!(t(&["from ", "2019-", "2021 "]), vec!["from", "2019-2021"]);
        // a dash then a word still stays apart from the next number's rules
        assert_eq!(t(&["the ", "2019\u{2013}", "then "]), vec!["the", "2019\u{2013}", "then"]);
    }

    #[test]
    fn empty_input_no_chunks() {
        assert!(chunk(&[], 12, 50).is_empty());
    }
}
