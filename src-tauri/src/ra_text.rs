//! Read Aloud — turn raw UIA word tokens into speakable words and chunks.
use std::borrow::Cow;
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
        // glued pieces of one written word: `$` `3` `.` `2B` -> "$3.2B" (spec §2)
        if prev_glued && !tok.starts_with(char::is_whitespace) && !pending_para && !breaks_before(raw, i) {
            if let Some(last) = out.last_mut() {
                if joins_glued(&last.text, t) {
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
            out.push(Word { text: t.to_string(), src: vec![i], para_break_before: pending_para && !out.is_empty() });
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
/// `.`, `,`, `$`, `%`, `/`, `:` and letters). Deliberately narrow, so a PDF line
/// end without a space (`end.` + `Next`, `2019` + `the`) never fuses two words.
fn joins_glued(prev: &str, t: &str) -> bool {
    let mut back = prev.chars().rev();
    let (Some(a), Some(b)) = (back.next(), t.chars().next()) else { return false };
    let digit_before = back.next().is_some_and(|c| c.is_ascii_digit());
    if b.is_ascii_digit() {
        // 3.|2, 1,|000, 7:|30, 3/|14, $|3, $3|2; never two all-digit tokens (a line end)
        let digits_only = |s: &str| s.chars().all(|c| c.is_ascii_digit());
        return (a.is_ascii_digit() && !(digits_only(prev) && digits_only(t)))
            || (matches!(a, '.' | ',' | ':' | '/') && digit_before)
            || is_prefix_char(a);
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
    // dotted initialism: U.|S, e.|g, Ph.|D (never "end." + "Next" or "it." + "So")
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
/// (`7am`, `7:30pm`), a decade (`1990s`, `90s`), or money (`$5m`).
fn suffix_fits(prev: &str, t: &str) -> bool {
    let digits: String = prev.chars().rev().take_while(char::is_ascii_digit).collect();
    match t {
        "a" | "p" | "am" | "pm" | "AM" | "PM" => prev.contains(':') || digits.len() <= 2,
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

fn ends_clause(w: &str) -> bool {
    w.ends_with([',', ';', ':'])
}

pub fn chunk(words: &[Word], first_max: usize, max: usize) -> Vec<Range<usize>> {
    // a 0 limit would never advance
    let (first_max, max) = (first_max.max(1), max.max(1));
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
const KEEP_CAPS: &[&str] = &["US", "IT", "WHO", "AM", "PM", "OK", "AI", "ID", "TV", "UK", "EU", "UN", "IQ", "PC"];

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

    #[test]
    fn empty_input_no_chunks() {
        assert!(chunk(&[], 12, 50).is_empty());
    }
}
