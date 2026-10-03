//! Read Aloud normalisation pre-pass (spec §2, phase 3).
//!
//! `spans(words)` turns runs of source words into spoken text, as non-overlapping,
//! sorted spans over word indices (end exclusive). Number spans (`ra_norm_num`) win
//! over text spans (`ra_norm_text`) when they overlap. `prepare` and `fold` are the
//! character fixes callers apply before G2P. Pure, static tables, never panics.

use std::borrow::Cow;

/// One run of source words `start..end` (end exclusive) and what to say for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub spoken: String,
}

/// Number-family spans (Task 1).
fn num(words: &[&str]) -> Vec<(usize, usize, String)> {
    crate::ra_norm_num::num_spans(words)
}

/// All spans for one chunk: numbers first, then text spans that don't overlap them.
pub fn spans(words: &[&str]) -> Vec<Span> {
    if words.is_empty() {
        return Vec::new();
    }
    merge(
        words.len(),
        num(words),
        crate::ra_norm_text::text_spans(words),
    )
}

/// Keeps valid spans only (non-empty range inside `0..len`, non-blank spoken),
/// takes number spans first (earliest wins among themselves), then every text span
/// that overlaps nothing already taken. Returns them sorted by start.
fn merge(
    len: usize,
    num: Vec<(usize, usize, String)>,
    text: Vec<(usize, usize, String)>,
) -> Vec<Span> {
    // `taken[i]` marks word i as owned by a kept span.
    let mut taken = vec![false; len];
    let mut out: Vec<Span> = Vec::with_capacity(num.len() + text.len());
    for (source, mut list) in [("num", num), ("text", text)] {
        list.sort_by_key(|(s, e, _)| (*s, *e));
        for (start, end, spoken) in list {
            let valid = start < end && end <= len && !spoken.trim().is_empty();
            if !valid {
                log::debug!("ra_norm: dropped invalid {source} span {start}..{end} of {len}");
                continue;
            }
            let free = taken.get(start..end).is_some_and(|r| r.iter().all(|t| !t));
            if !free {
                log::debug!("ra_norm: dropped overlapping {source} span {start}..{end}");
                continue;
            }
            if let Some(r) = taken.get_mut(start..end) {
                r.iter_mut().for_each(|t| *t = true);
            }
            out.push(Span { start, end, spoken });
        }
    }
    out.sort_by_key(|s| s.start);
    out
}

/// Character fixes before G2P: ’ ‘ ʼ → `'`, and U+2010 / U+2011 (hyphen,
/// non-breaking hyphen) → `-`. Borrows when nothing changes.
pub fn prepare(word: &str) -> Cow<'_, str> {
    fn map(c: char) -> Option<char> {
        match c {
            '\u{2019}' | '\u{2018}' | '\u{02BC}' => Some('\''),
            '\u{2010}' | '\u{2011}' => Some('-'),
            _ => None,
        }
    }
    if !word.chars().any(|c| map(c).is_some()) {
        return Cow::Borrowed(word);
    }
    Cow::Owned(word.chars().map(|c| map(c).unwrap_or(c)).collect())
}

/// Latin accent fold for one character: Latin-1 letters (À–ÿ), Latin Extended-A
/// (Ā–ſ) and Romanian comma-below Ș ș Ț ț, to ASCII. Keeps the case
/// (`É` → "E", `Æ` → "AE", `ß` → "ss"). `None` for ASCII and everything else.
pub fn fold(c: char) -> Option<&'static str> {
    let lower = match c {
        '\u{C0}'..='\u{C5}' | '\u{E0}'..='\u{E5}' | '\u{100}'..='\u{105}' => "a",
        '\u{C6}' | '\u{E6}' => "ae",
        '\u{C7}' | '\u{E7}' | '\u{106}'..='\u{10D}' => "c",
        '\u{D0}' | '\u{F0}' | '\u{10E}'..='\u{111}' => "d",
        '\u{C8}'..='\u{CB}' | '\u{E8}'..='\u{EB}' | '\u{112}'..='\u{11B}' => "e",
        '\u{11C}'..='\u{123}' => "g",
        '\u{124}'..='\u{127}' => "h",
        '\u{CC}'..='\u{CF}' | '\u{EC}'..='\u{EF}' | '\u{128}'..='\u{131}' => "i",
        '\u{132}' | '\u{133}' => "ij",
        '\u{134}' | '\u{135}' => "j",
        '\u{136}'..='\u{138}' => "k",
        '\u{139}'..='\u{142}' => "l",
        '\u{D1}' | '\u{F1}' | '\u{143}'..='\u{14B}' => "n",
        '\u{D2}'..='\u{D6}' | '\u{D8}' | '\u{F2}'..='\u{F6}' | '\u{F8}' | '\u{14C}'..='\u{151}' => {
            "o"
        }
        '\u{152}' | '\u{153}' => "oe",
        '\u{154}'..='\u{159}' => "r",
        '\u{15A}'..='\u{161}' | '\u{17F}' | '\u{218}' | '\u{219}' => "s",
        '\u{DF}' | '\u{1E9E}' => "ss",
        '\u{162}'..='\u{167}' | '\u{21A}' | '\u{21B}' => "t",
        '\u{DE}' | '\u{FE}' => "th",
        '\u{D9}'..='\u{DC}' | '\u{F9}'..='\u{FC}' | '\u{168}'..='\u{173}' => "u",
        '\u{174}' | '\u{175}' => "w",
        '\u{DD}' | '\u{FD}' | '\u{FF}' | '\u{176}'..='\u{178}' => "y",
        '\u{179}'..='\u{17E}' => "z",
        _ => return None,
    };
    if !c.is_uppercase() {
        return Some(lower);
    }
    Some(match lower {
        "a" => "A",
        "ae" => "AE",
        "c" => "C",
        "d" => "D",
        "e" => "E",
        "g" => "G",
        "h" => "H",
        "i" => "I",
        "ij" => "IJ",
        "j" => "J",
        "k" => "K",
        "l" => "L",
        "n" => "N",
        "o" => "O",
        "oe" => "OE",
        "r" => "R",
        "s" => "S",
        "ss" => "SS",
        "t" => "T",
        "th" => "TH",
        "u" => "U",
        "w" => "W",
        "y" => "Y",
        "z" => "Z",
        other => other,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(start: usize, end: usize, spoken: &str) -> (usize, usize, String) {
        (start, end, spoken.to_string())
    }
    fn span(start: usize, end: usize, spoken: &str) -> Span {
        Span {
            start,
            end,
            spoken: spoken.to_string(),
        }
    }

    #[test]
    fn merge_numbers_win_on_overlap() {
        let num = vec![s(0, 2, "five billion dollars")];
        let text = vec![s(1, 2, "x"), s(2, 3, "kilometers")];
        assert_eq!(
            merge(3, num, text),
            vec![span(0, 2, "five billion dollars"), span(2, 3, "kilometers")]
        );
    }

    #[test]
    fn merge_sorts_and_keeps_disjoint_text() {
        let num = vec![s(3, 4, "five")];
        let text = vec![s(4, 5, "kilometers"), s(0, 1, "doctor")];
        assert_eq!(
            merge(5, num, text),
            vec![
                span(0, 1, "doctor"),
                span(3, 4, "five"),
                span(4, 5, "kilometers")
            ]
        );
    }

    #[test]
    fn merge_drops_invalid_spans() {
        let num = vec![
            s(2, 1, "bad"),
            s(0, 9, "past end"),
            s(0, 1, "  "),
            s(1, 2, "two"),
        ];
        let text = vec![
            s(1, 1, "empty range"),
            s(2, 3, "ok"),
            s(1, 3, "overlaps two"),
        ];
        assert_eq!(
            merge(3, num, text),
            vec![span(1, 2, "two"), span(2, 3, "ok")]
        );
    }

    #[test]
    fn merge_drops_overlapping_number_spans() {
        let num = vec![s(0, 2, "first"), s(1, 3, "second")];
        assert_eq!(merge(3, num, vec![]), vec![span(0, 2, "first")]);
    }

    #[test]
    fn spans_end_to_end_text_only() {
        let w = ["he", "moved", "to", "St.", "Louis", "in", "the", "U.S."];
        assert_eq!(spans(&w), vec![span(3, 4, "saint"), span(7, 8, "U S.")]);
        assert!(spans(&[]).is_empty());
    }

    /// Numbers win on overlap, so a symbol glued to a number is read by the number
    /// pass and must keep the symbol's word.
    #[test]
    fn prefix_symbols_survive_the_numbers_win_merge() {
        for (w, want) in [
            ("±2", "plus or minus two"),
            ("§230", "section two hundred thirty"),
            ("#1", "number one"),
            ("√2", "square root of two"),
            ("¶4", "paragraph four"),
            ("№5", "number five"),
        ] {
            assert_eq!(spans(&["see", w, "here"]), vec![span(1, 2, want)], "{w}");
        }
        // glued math reads like the spaced form (controller ruling: times, not by)
        assert_eq!(spans(&["3×4"]), vec![span(0, 1, "three times four")]);
        assert_eq!(spans(&["6÷2"]), vec![span(0, 1, "six divided by two")]);
        assert_eq!(spans(&["-5°C"]), vec![span(0, 1, "minus five degrees Celsius")]);
        assert_eq!(spans(&["under", "§230."]), vec![span(1, 2, "section two hundred thirty.")]);
    }

    /// One unit table: glued units come from the number pass, which also knows
    /// every glueable unit of the text pass's table.
    #[test]
    fn glued_and_spaced_units_read_alike() {
        for (glued, spaced, want) in [
            ("5mAh", ["5", "mAh"], "milliamp hours"),
            ("3kWh", ["3", "kWh"], "kilowatt hours"),
            ("60fps", ["60", "fps"], "frames per second"),
            ("20°C", ["20", "°C"], "degrees Celsius"),
            ("8TB", ["8", "TB"], "terabytes"),
            ("12dB", ["12", "dB"], "decibels"),
        ] {
            let g = spans(&[glued]);
            assert_eq!(g.len(), 1, "{glued}: {g:?}");
            assert!(g[0].spoken.ends_with(want), "{glued}: {g:?}");
            let s = spans(&spaced);
            assert_eq!(s.last().map(|x| x.spoken.as_str()), Some(want), "{spaced:?}: {s:?}");
        }
    }

    /// Round 1: mmHg is said, not spelled, and the reading stays "over".
    #[test]
    fn mmhg_is_millimeters_of_mercury() {
        let s = spans(&["was", "120/80", "mmHg."]);
        assert_eq!(s, vec![span(1, 2, "one hundred twenty over eighty"), span(2, 3, "millimeters of mercury.")]);
    }

    /// Round 1: "3 p.m. ET" has no sentence period after "p m"; the zone is spelled
    /// by the caps rule. A clock time at a real sentence end keeps its period.
    #[test]
    fn clock_time_before_a_zone_has_no_period() {
        let s = spans(&["at", "3", "p.m.", "ET", "on", "Monday."]);
        assert!(s.iter().all(|x| !x.spoken.ends_with('.')), "{s:?}");
        let s = spans(&["at", "9:30", "a.m.", "PT,", "or", "noon."]);
        assert!(s.iter().all(|x| !x.spoken.ends_with('.')), "{s:?}");
        let s = spans(&["We", "left", "at", "5", "p.m."]);
        assert!(s.iter().any(|x| x.spoken.ends_with('.')), "{s:?}");
    }

    /// A short month + day + year in one chunk (the chunker keeps them together).
    #[test]
    fn short_month_dates_read_as_dates() {
        let w = ["On", "Jan.", "5,", "2024,", "it", "closed."];
        assert_eq!(spans(&w), vec![span(1, 3, "January fifth,"), span(3, 4, "twenty twenty four,")]);
        let w = ["On", "Nov.", "6", "the", "board", "met."];
        assert_eq!(spans(&w), vec![span(1, 3, "November sixth")]);
        assert_eq!(spans(&["Sept.", "30"]), vec![span(0, 2, "September thirtieth")]);
        // a year after "Month D," is a year even with other numbers around
        let w = ["Sales", "of", "3", "units", "on", "Jan.", "5,", "2024", "and", "Feb.", "9,", "1998."];
        let s = spans(&w);
        assert!(s.contains(&span(7, 8, "twenty twenty four")), "{s:?}");
        assert!(s.contains(&span(11, 12, "nineteen ninety eight.")), "{s:?}");
        // Jan the name is untouched
        assert!(spans(&["I", "met", "Jan.", "She", "said", "hi."]).is_empty());
    }

    #[test]
    fn spans_handle_curly_apostrophes() {
        let w = ["the", "U.S.’s", "debt"];
        assert_eq!(spans(&w), vec![span(1, 2, "U S's")]);
    }

    #[test]
    fn prepare_maps_apostrophes_and_hyphens() {
        assert_eq!(prepare("company’s"), "company's");
        assert_eq!(prepare("‘quoted’"), "'quoted'");
        assert_eq!(prepare("Hawaiʼi"), "Hawai'i");
        assert_eq!(prepare("Type\u{2011}C"), "Type-C");
        assert_eq!(prepare("well\u{2010}known"), "well-known");
        assert!(matches!(prepare("plain"), Cow::Borrowed(_)));
        assert!(matches!(prepare("東京😀"), Cow::Borrowed(_)));
    }

    #[test]
    fn fold_samples() {
        let cases = [
            ('à', "a"),
            ('á', "a"),
            ('å', "a"),
            ('ÿ', "y"),
            ('ğ', "g"),
            ('ă', "a"),
            ('ș', "s"),
            ('ş', "s"),
            ('ț', "t"),
            ('ţ', "t"),
            ('ł', "l"),
            ('ő', "o"),
            ('ű', "u"),
            ('ø', "o"),
            ('æ', "ae"),
            ('œ', "oe"),
            ('ß', "ss"),
            ('đ', "d"),
            ('þ', "th"),
            ('ñ', "n"),
            ('ç', "c"),
            ('é', "e"),
            ('ü', "u"),
            ('ı', "i"),
            ('ž', "z"),
            ('č', "c"),
            ('ř', "r"),
            ('É', "E"),
            ('Ł', "L"),
            ('Æ', "AE"),
            ('Ø', "O"),
            ('Ș', "S"),
            ('Þ', "TH"),
            ('İ', "I"),
            ('Ğ', "G"),
            ('Œ', "OE"),
            ('ð', "d"),
            ('ý', "y"),
            ('ś', "s"),
            ('ź', "z"),
        ];
        for (c, want) in cases {
            assert_eq!(fold(c), Some(want), "fold({c})");
        }
        for c in ['a', 'Z', '1', '東', '×', '÷', '😀', ' ', 'µ'] {
            assert_eq!(fold(c), None, "fold({c})");
        }
    }

    #[test]
    fn fold_covers_latin1_and_extended_a() {
        for cp in 0xC0u32..=0x17F {
            let Some(c) = char::from_u32(cp) else {
                continue;
            };
            if c == '×' || c == '÷' {
                continue;
            }
            let f = fold(c);
            assert!(f.is_some(), "no fold for U+{cp:04X} {c}");
            assert!(f.unwrap_or("").chars().all(|x| x.is_ascii_alphabetic()));
        }
    }

    #[test]
    #[ignore]
    fn perf_spans_per_chunk() {
        let s = "Dr. Smith of the U.S. met Sen. Warren at 5 km/h near St. Louis, e.g. amazon.com \
                 & @ElonMusk #MacronLeaks World War II and Henry VIII; 20°C ± 3 = x No. 5 Main St.";
        let w: Vec<&str> = s.split_whitespace().collect();
        let n = 10_000;
        let t = std::time::Instant::now();
        let mut total = 0usize;
        for _ in 0..n {
            total += spans(std::hint::black_box(&w)).len();
        }
        let per = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
        println!(
            "ra_norm::spans: {} words, {per:.4} ms per chunk ({total})",
            w.len()
        );
        assert!(per < 0.5, "{per} ms per chunk");
    }
}
