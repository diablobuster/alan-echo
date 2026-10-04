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
    /// `Some(None)`: drop the character.
    fn map(c: char) -> Option<Option<char>> {
        Some(match c {
            '\u{2019}' | '\u{2018}' | '\u{02BC}' => Some('\''),
            '\u{2010}' | '\u{2011}' => Some('-'),
            // round 2: list bullets are a pause, not a silent symbol
            '\u{2022}' | '\u{25CF}' | '\u{25AA}' | '\u{25E6}' | '\u{2023}' => Some(','),
            // an em dash is a pause ("The market\u{2014}already shaky\u{2014}fell" ran on)
            '\u{2014}' => Some(','),
            // the Hawaiian okina (Hawai\u{02BB}i) is silent for an English reader
            '\u{02BB}' => None,
            // trademark signs are not read
            '\u{ae}' | '\u{2122}' => None,
            // transliteration marks (Devgh\u{0101}\u{1E6D}), never in an English dictionary
            '\u{0101}' => Some('a'),
            '\u{0100}' => Some('A'),
            '\u{0113}' => Some('e'),
            '\u{012B}' => Some('i'),
            '\u{014D}' => Some('o'),
            '\u{016B}' => Some('u'),
            '\u{016A}' => Some('U'),
            '\u{1E6D}' => Some('t'),
            '\u{1E6C}' => Some('T'),
            '\u{1E0D}' => Some('d'),
            '\u{1E47}' => Some('n'),
            '\u{1E63}' => Some('s'),
            '\u{1E62}' => Some('S'),
            '\u{1E25}' => Some('h'),
            '\u{1E5B}' => Some('r'),
            '\u{1E37}' => Some('l'),
            '\u{1E43}' => Some('m'),
            _ => return None,
        })
    }
    // a dash standing alone between words is a pause too
    if matches!(word, "-" | "\u{2013}" | "--") {
        return Cow::Borrowed(",");
    }
    let pm = word.contains("+/-");
    // trademark markers glued to a brand: "Intel(R)", "Office(TM)", "Windows\u{ae}"
    let mark = word.len() > 3 && (word.contains("(R)") || word.contains("(TM)") || word.contains("(tm)"));
    if !pm && !mark && !word.chars().any(|c| map(c).is_some()) {
        return Cow::Borrowed(word);
    }
    // "+/-2%" is the plus-or-minus sign
    let w = if pm { word.replace("+/-", "\u{b1}") } else { word.to_string() };
    let w = if mark { w.replace("(R)", "").replace("(TM)", "").replace("(tm)", "") } else { w };
    Cow::Owned(w.chars().filter_map(|c| map(c).unwrap_or(Some(c))).collect())
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

/// Round 2: the next layer of glitches from the finance / tech / politics / sports /
/// weather / recipe corpus, end to end through `spans` (numbers and text merged).
#[cfg(test)]
mod round2_tests {
    use super::*;

    /// Source words with each span replaced by its spoken text.
    fn render(text: &str) -> String {
        // as `ra_tts::g2p_chunk` feeds them: prepared words
        let raw: Vec<&str> = text.split_whitespace().collect();
        let prepared: Vec<Cow<str>> = raw.iter().map(|x| prepare(x)).collect();
        let w: Vec<&str> = prepared.iter().map(|c| c.as_ref()).collect();
        let sp = spans(&w);
        let (mut out, mut i, mut k) = (Vec::new(), 0, 0);
        while i < w.len() {
            if k < sp.len() && sp[k].start == i {
                out.push(sp[k].spoken.clone());
                i = sp[k].end;
                k += 1;
            } else {
                out.push(w[i].to_string());
                i += 1;
            }
        }
        out.join(" ")
    }

    fn check(cases: &[(&str, &str)]) {
        let mut bad = Vec::new();
        for (src, want) in cases {
            let got = render(src);
            if !got.contains(want) {
                bad.push(format!("{src:?}\n   got  {got:?}\n   want ..{want:?}.."));
            }
        }
        assert!(bad.is_empty(), "{} wrong:\n{}", bad.len(), bad.join("\n"));
    }

    #[test]
    fn earnings_shorthand() {
        check(&[
            ("Revenue grew 12% Y/Y and 3% Q/Q.", "twelve percent year over year and three percent quarter over quarter."),
            ("Core PCE rose 0.3% M/M in August.", "zero point three percent month over month in"),
            ("up 21% YoY and 4% QoQ, with MoM flat", "year over year and four percent quarter over quarter, with month over month flat"),
            ("The yield fell 7 bps to 4.12%.", "fell seven basis points to"),
            ("Margins widened 50bp last year", "fifty basis points last"),
            ("a 1 bp move", "one basis point move"),
            ("Margin fell 2.1 pp to 18.4%.", "two point one percentage points to"),
            ("raised its FY25 guidance", "raised its fiscal twenty five guidance"),
            ("FY2024 revenue was flat", "fiscal twenty twenty four revenue"),
            ("1H24 revenue came in", "first half twenty four revenue"),
            ("ramping through 2H25.", "through second half twenty five."),
            ("3Q24 sales rose", "third quarter twenty four sales"),
            ("Q4 FY24 revenue", "fiscal twenty four revenue"),
            ("Guidance implies +/-2% growth", "plus or minus two percent growth"),
        ]);
    }

    #[test]
    fn scale_suffixes_on_bare_numbers() {
        check(&[
            ("Netflix added 5.1M paid subscribers", "added five point one million paid"),
            ("payrolls rose by 254K in September", "two hundred fifty four thousand in"),
            ("now has 200M weekly users", "two hundred million weekly"),
            ("serves 4M+ customers", "four million plus customers"),
            ("costs $0.15 per 1M input tokens", "per one million input"),
            ("passed 1.2B users", "one point two billion users"),
            // not scales: a 4K screen, a model size
            ("a 4K TV", "a four K TV"),
            ("Llama 3.1 70B beats it", "seventy B"),
        ]);
    }

    #[test]
    fn tickers_with_exchanges() {
        check(&[
            ("Apple (NASDAQ:AAPL) slipped", "Nasdaq, A A P L"),
            ("AT&T (NYSE:T) reports", "N Y S E, T"),
            ("Snowflake (NYSE: SNOW) reports", "N Y S E, S N O W"),
            ("Meta (NASDAQ: META) rose", "Nasdaq, M E T A"),
        ]);
    }

    #[test]
    fn product_model_numbers_read_in_pairs() {
        check(&[
            ("The RTX 4090 draws up to", "forty ninety"),
            ("an RTX 5080 card", "fifty eighty"),
            ("AMD's Ryzen 9 9950X boosts", "ninety nine fifty X"),
            ("the RX 7900 XTX is", "seventy nine hundred"),
            ("a GTX 1080 Ti", "ten eighty"),
            ("Core i9-14900K support", "fourteen nine hundred K"),
        ]);
    }

    #[test]
    fn units_weather_and_recipes() {
        check(&[
            ("The pressure dropped to 897 mb.", "eight hundred ninety seven millibars."),
            ("Makes 12 servings; 240 kcal per serving.", "two hundred forty calories per"),
            ("a 200MP camera", "two hundred megapixels"),
            ("Stir in 2 Tbsp honey", "two tablespoons honey"),
            ("Season with 1 tbsp. kosher salt", "one tablespoon kosher"),
            ("Expect 6-10 in. of snow", "six to ten inches of snow"),
            ("Stir in 1/3 c. milk", "one third of a cup milk"),
            ("Winds NW 15-25 mph", "Winds northwest fifteen to twenty five"),
            ("Preheat the oven to 425 degrees F.", "degrees Fahrenheit."),
            ("about 20 degrees C today", "degrees Celsius today"),
            ("Milton is a Cat. 5 storm", "a category five storm"),
            ("runs at ~30 tokens/s on a GPU", "tokens per second"),
            ("Gold hit a record $2,685/oz on Friday", "dollars per ounce"),
            ("Roast 45 min\u{2013}1 hr, until golden", "forty five minutes to one hour,"),
            ("Rainfall totals of 1-3\" are likely", "one to three inches are"),
            ("with locally 5\".", "five inches."),
        ]);
    }

    #[test]
    fn sports_shorthand() {
        check(&[
            ("Arsenal W 3-1 vs. Spurs", "Arsenal won three to one"),
            ("Yankees L 7-6", "Yankees lost seven to six"),
            ("Chelsea D 1-1 at Villa", "Chelsea drew one to one"),
            ("Mahomes was 24/36 for 291 yds", "twenty four of thirty six for"),
            ("took P1 by 0.214s ahead", "zero point two one four seconds ahead"),
            ("jumped 12% in 24h.", "twenty four hours."),
        ]);
    }

    #[test]
    fn politics_and_legal() {
        check(&[
            ("Sen. Elizabeth Warren (D-Mass.) called", "Democrat of Massachusetts"),
            ("Rep. Jim Jordan (R-OH) chairs", "Republican of Ohio"),
            ("Sen. Ted Cruz (R-Texas) said", "Republican of Texas"),
            ("Gov. Josh Shapiro (D) signed", "Democrat"),
            ("The court cited Fed. R. Civ. P. 12(b)(6).", "Federal Rule of Civil Procedure"),
            ("employees in a 401(k) plan", "four oh one K plan"),
            ("Under 26 U.S.C. § 401(k), employees", "four oh one K"),
        ]);
    }

    #[test]
    fn urls_emails_and_years_in_brackets() {
        check(&[
            ("Read the post (https://blog.example.org/2024/ai-update) for details.", "example dot org"),
            ("Contact sandra.p.jones@nasa.gov/ today", "nasa dot gov"),
            ("basis (2017 reference year) are shown", "twenty seventeen"),
        ]);
    }

    /// Review fix round 1: the new rules must not fire outside their context.
    fn check_not(cases: &[(&str, &str)]) {
        let mut bad = Vec::new();
        for (src, never) in cases {
            let got = render(src);
            if got.contains(never) {
                bad.push(format!("{src:?}\n   got  {got:?}\n   must not contain {never:?}"));
            }
        }
        assert!(bad.is_empty(), "{} wrong:\n{}", bad.len(), bad.join("\n"));
    }

    #[test]
    fn slash_before_for_is_a_score_only_in_sports_context() {
        check(&[
            ("Taxes are due 4/15 for most filers.", "April fifteenth for"),
            ("I give it 9/10 for effort.", "nine out of ten for"),
            ("Mahomes was 24/36 for 291 yds", "twenty four of thirty six for"),
        ]);
    }

    #[test]
    fn result_letters_need_a_team() {
        check(&[("Arsenal W 3-1 vs. Spurs", "Arsenal won three to one"), ("The Yankees L 7-6", "Yankees lost")]);
        check_not(&[
            ("Take vitamin D 1-2 times a day.", "drew"),
            ("Boarding at Gate L 1-10 now.", "lost"),
            ("Seats in Row W 10-12 are open.", "won"),
            ("See Appendix D 1-3 for details.", "drew"),
        ]);
    }

    #[test]
    fn party_letters_only_after_a_person() {
        check(&[("Gov. Josh Shapiro (D) signed it", "Shapiro Democrat signed")]);
        check(&[("Sen. Ted Cruz (R) spoke", "Cruz Republican spoke")]);
        check_not(&[("See Appendix (D) for the data.", "Democrat"), ("Plan (R) was chosen", "Republican")]);
        // round 3: a title abbreviation before a one-word name
        check(&[
            ("Rep. Smith (R) voted no", "Smith Republican voted"),
            ("Sen. Warren (D) said", "Warren Democrat said"),
            ("Gov. Newsom (D), who", "Newsom Democrat"),
            ("Del. Norton (D) asked", "Norton Democrat asked"),
        ]);
        check_not(&[("In Exhibit (R) the totals", "Republican"), ("Mr. Smith (R) is a typo", "Republican")]);
        assert_eq!(prepare("Intel(R)"), "Intel");
        assert_eq!(prepare("Windows\u{ae}"), "Windows");
        assert_eq!(prepare("iPhone\u{2122}"), "iPhone");
        assert_eq!(prepare("Office(TM)"), "Office");
        assert_eq!(prepare("(R)"), "(R)", "a lone (R) is left to the context rules");
    }

    #[test]
    fn pp_is_pages_unless_it_is_about_percent() {
        check(&[
            ("The book runs 352 pp. and has an index.", "three hundred fifty two pages"),
            ("See pp 12-15 for the table.", "pages"),
            ("Margin fell 2.1 pp to 18.4%.", "percentage points"),
            ("The rate rose 0.5 pp this year.", "percentage points"),
            ("The spread widened 30 pp", "percentage points"),
        ]);
        check_not(&[("The book runs 352 pp. and has an index.", "percentage")]);
    }

    #[test]
    fn mb_is_millibars_only_for_weather() {
        check(&[
            ("a 500 mb file", "five hundred megabytes file"),
            ("The pressure dropped to 897 mb.", "millibars"),
            ("A 990 mb low moves in", "millibars"),
            ("central pressure 960 mb, per the hPa chart", "millibars"),
        ]);
        // round 3: a weather word within 3 words, and no file / storage word
        check(&[
            ("500 mb file, high quality", "five hundred megabytes file"),
            ("a 700 mb download with high speed", "megabytes"),
            ("The storm's 950 mb center", "millibars"),
        ]);
        check_not(&[
            ("500 mb file, high quality", "millibars"),
            ("a 700 mb download with high speed", "millibars"),
            ("high resolution photos take 12 mb each", "millibars"),
            ("The storm photos were a 20 mb storage hog", "millibars"),
        ]);
    }

    #[test]
    fn in_and_c_at_a_sentence_end_stay_words() {
        check(&[("He scored 3 in. Then he left.", "three in."), ("Expect 6-10 in. of snow", "inches of snow")]);
        check_not(&[("He scored 3 in.", "inches"), ("She got a 3 c. That was it.", "cup")]);
    }

    #[test]
    fn prepare_folds_bullets_okina_and_transliteration_marks() {
        assert_eq!(prepare("flat\u{2022}"), "flat,");
        assert_eq!(prepare("\u{2022}"), ",");
        assert_eq!(prepare("points:\u{2022}"), "points:,");
        assert_eq!(prepare("Hawai\u{02BB}i"), "Hawaii");
        assert_eq!(prepare("Devgh\u{0101}\u{1E6D}"), "Devghat");
        assert_eq!(prepare("caf\u{e9}"), "caf\u{e9}", "everyday accents are left to the dictionary");
        assert_eq!(prepare("+/-2%"), "\u{b1}2%");
        assert_eq!(prepare("market\u{2014}already"), "market,already");
        assert_eq!(prepare("\u{2014}"), ",");
        assert_eq!(prepare("-"), ",");
        assert_eq!(prepare("\u{2013}"), ",");
        assert_eq!(prepare("2019\u{2013}2021"), "2019\u{2013}2021", "a range keeps its dash");
    }
}


/// Round 3: legal, medical, academic, forum, review and code-ish text.
#[cfg(test)]
mod round3_tests {
    use super::*;

    fn render(text: &str) -> String {
        let raw: Vec<&str> = text.split_whitespace().collect();
        let prepared: Vec<Cow<str>> = raw.iter().map(|x| prepare(x)).collect();
        let w: Vec<&str> = prepared.iter().map(|c| c.as_ref()).collect();
        let sp = spans(&w);
        let (mut out, mut i, mut k) = (Vec::new(), 0, 0);
        while i < w.len() {
            if k < sp.len() && sp[k].start == i {
                out.push(sp[k].spoken.clone());
                i = sp[k].end;
                k += 1;
            } else {
                out.push(w[i].to_string());
                i += 1;
            }
        }
        out.join(" ")
    }

    fn check(cases: &[(&str, &str)]) {
        let bad: Vec<String> = cases
            .iter()
            .filter_map(|(src, want)| {
                let got = render(src);
                (!got.contains(want)).then(|| format!("{src:?}
   got  {got:?}
   want ..{want:?}.."))
            })
            .collect();
        assert!(bad.is_empty(), "{} wrong:
{}", bad.len(), bad.join("
"));
    }

    fn check_not(cases: &[(&str, &str)]) {
        let bad: Vec<String> = cases
            .iter()
            .filter_map(|(src, not)| {
                let got = render(src);
                got.contains(not).then(|| format!("{src:?}
   got  {got:?}
   must not have {not:?}"))
            })
            .collect();
        assert!(bad.is_empty(), "{} wrong:
{}", bad.len(), bad.join("
"));
    }

    #[test]
    fn paragraph_signs_and_symbols_read() {
        check(&[
            ("The parties dispute ¶ 8 of the complaint.", "paragraph eight"),
            ("the facts in ¶¶ 3-17 of the", "paragraphs three"),
            ("force majeure clause (§ 14.2) excuses", "section fourteen point two"),
            ("β = 0.62, SE = 0.08", "equals"),
        ]);
    }

    #[test]
    fn forum_shorthand_is_expanded() {
        check(&[
            ("ngl that ending made me cry", "not gonna lie that"),
            ("the rest tbh", "to be honest"),
            ("smh people really", "shaking my head"),
            ("brb gotta feed the cat", "be right back"),
            ("edit: nvm figured it out", "never mind"),
            ("bc the update", "because the"),
            ("idk why ppl are mad", "I don't know why people"),
            ("imo the first season", "in my opinion"),
            ("iirc the deadline", "if I recall correctly"),
            ("afaik it's not legal", "as far as I know"),
            ("fwiw i've had mine", "for what it's worth"),
            ("Salt &amp; cheese ftw", "for the win"),
            ("ios 18.1 here", "I O S"),
        ]);
        // capitalised or other words are left alone
        check_not(&[("BC Hydro raised rates", "because"), ("The Smh river", "shaking")]);
    }

    #[test]
    fn mb_nearest_context_word_decides() {
        check(&[
            ("940 mb before landfall, per data from the NHC", "millibars"),
            ("a 500 mb file", "megabytes"),
            ("The 990 mb low brought data outages", "millibars"),
            ("high resolution photos take 12 mb each", "megabytes"),
        ]);
    }

    #[test]
    fn round3_minor_shorthand() {
        check(&[
            ("It was built in 44 bc by the Romans", "B C"),
            ("b/c the landlord never fixed it", "because the"),
            ("Imo the first season is better", "In my opinion"),
            ("Tbh it was fine", "To be honest"),
            ("Ngl that hurt", "Not gonna lie"),
            ("Idk what happened", "I don't know"),
            ("Ppl are mad. Smh honestly", "People are"),
        ]);
        check_not(&[("It was built in 44 bc by the Romans", "because"), ("Mr. Imo said", "opinion")]);
    }

    #[test]
    fn medical_and_review_units() {
        check(&[
            ("Heart rate was 141 bpm and", "one hundred forty one beats per minute"),
            ("Patients took 420 mcg of metformin", "micrograms"),
            ("been using it for 6 wks and", "six weeks"),
            ("Holds 1.5 qt, which", "quarts"),
            ("a BMI of 30 kg/m2 or higher", "kilograms per square meter"),
        ]);
    }

    #[test]
    fn code_names_read() {
        check(&[
            ("a C# front end", "C sharp front"),
            ("the frontend uses F#.", "F sharp"),
            ("Open setup.cfg and change", "setup dot C F G"),
            ("loads main.js and styles.css.", "styles dot C S S"),
            ("Open app.tsx and", "app dot T S X"),
            ("Open server.go and", "server dot go"),
            ("The v1 endpoint /api/users returns", "slash api slash users"),
            ("has a user_id and an access_token field", "user id and an access token"),
            ("Set NODE_ENV=production in .env.", "NODE ENV equals production"),
            ("a C# front end on .NET 8.", "dot net"),
            ("Both .NET. And more", "dot net."),
            ("hmu if u want the link", "hit me up"),
        ]);
    }

    #[test]
    fn statistics_letters_read() {
        check(&[
            ("β = 0.62, SE = 0.08", "beta equals"),
            ("The scale was reliable (α = .81).", "alpha equals"),
            ("high (κ = 0.87).", "kappa"),
            ("84.9% (Δ = 13.6 pp).", "delta"),
            ("of the variance (R² = 0.41).", "R squared equals"),
        ]);
    }

    #[test]
    fn paths_and_settings_with_digits_read() {
        check(&[
            ("The v3 endpoint /v1/orders returns", "slash v1 slash orders"),
            ("Set PORT=http://localhost:8080 in .env.", "PORT equals localhost"),
            ("Set DEBUG=1 in .env.", "DEBUG equals one"),
        ]);
    }

    #[test]
    fn legal_citations_read() {
        check(&[
            ("Apple Inc., No. 21-cv-6242 (D. Md. 2020).", "Maryland"),
            ("Cf. Chevron U.S.A. Inc. v. NRDC", "compare"),
        ]);
    }
}
