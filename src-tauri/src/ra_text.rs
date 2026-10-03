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
/// Returns the form to feed G2P: lowercased when `known(lower)` says it is a
/// dictionary word, unless it is a common acronym (`KEEP_CAPS`) or the lexicon
/// has the uppercase spelling itself (`known(upper)`, e.g. NATO). Leading/trailing
/// punctuation, possessive 's and hyphen-joined parts are handled. Never panics.
pub fn speakable_caps<'a>(word: &'a str, known: &dyn Fn(&str) -> bool) -> Cow<'a, str> {
    if !word.contains('-') {
        return caps_part(word, known);
    }
    let parts: Vec<Cow<str>> = word.split('-').map(|p| caps_part(p, known)).collect();
    if parts.iter().all(|p| matches!(p, Cow::Borrowed(_))) {
        Cow::Borrowed(word)
    } else {
        Cow::Owned(parts.join("-"))
    }
}

/// Common acronyms whose lowercase form is also an English word.
const KEEP_CAPS: &[&str] = &["US", "IT", "WHO", "AM", "PM", "OK", "AI", "ID", "TV", "UK", "EU", "UN", "IQ", "PC"];

fn caps_part<'a>(w: &'a str, known: &dyn Fn(&str) -> bool) -> Cow<'a, str> {
    let Some(start) = w.find(char::is_alphanumeric) else { return Cow::Borrowed(w) };
    let Some((last, ch)) = w.char_indices().rev().find(|(_, c)| c.is_alphanumeric()) else { return Cow::Borrowed(w) };
    let end = last + ch.len_utf8();
    let core = &w[start..end];
    let base = core.strip_suffix("'S").or_else(|| core.strip_suffix("\u{2019}S")).unwrap_or(core);
    if base.chars().count() < 2 || !base.chars().all(|c| c.is_alphabetic() && c.is_uppercase()) {
        return Cow::Borrowed(w);
    }
    if KEEP_CAPS.contains(&base) || known(base) || !known(&base.to_lowercase()) {
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
        speakable_caps(w, &known).into_owned()
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
    fn zero_limits_still_progress() {
        let c = chunk(&words("a b c"), 0, 0);
        assert_eq!(c, vec![0..1, 1..2, 2..3]);
    }

    #[test]
    fn empty_input_no_chunks() {
        assert!(chunk(&[], 12, 50).is_empty());
    }
}
