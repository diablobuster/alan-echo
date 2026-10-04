//! Page junk and footnote markers (natural-reading spec §6). Pure text rules:
//! whole blocks only, never mid-sentence. Wired into `Session::job` in part B.

use crate::ra_text::Word;
use std::borrow::Cow;

/// A block that is exactly one of these is page furniture, not text to read.
const JUNK_PHRASES: &[&str] = &[
    "advertisement",
    "ad",
    "sponsored",
    "skip to content",
    "skip to main content",
    "share this article",
    "share this",
    "share",
    "sign up for our newsletter",
    "subscribe to our newsletter",
    "related articles",
    "related",
    "read more",
    "recommended",
    "most popular",
    "you may also like",
    "continue reading",
];

/// Names on a share-button row ("Facebook Twitter LinkedIn Email"). "copy link" is
/// one button and is handled apart.
const SOCIAL: &[&str] = &["facebook", "twitter", "x", "linkedin", "email", "reddit", "pinterest", "whatsapp", "print"];

/// Lowercased letters and digits of `w`, so trailing punctuation never matters.
fn core(w: &str) -> String {
    w.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// True when the block's words are exactly a junk phrase (case and trailing
/// punctuation ignored) or a run of 2+ social-button names ("copy link" is one).
#[cfg_attr(not(test), allow(dead_code))] // wired in part B
pub fn is_junk_block(words: &[&str]) -> bool {
    let cores: Vec<String> = words.iter().map(|w| core(w)).filter(|c| !c.is_empty()).collect();
    if cores.is_empty() || cores.len() > 12 {
        return false;
    }
    if JUNK_PHRASES.contains(&cores.join(" ").as_str()) {
        return true;
    }
    let mut names = 0usize;
    let mut i = 0usize;
    while let Some(c) = cores.get(i) {
        if c == "copy" && cores.get(i + 1).is_some_and(|n| n == "link") {
            i += 2;
        } else if SOCIAL.contains(&c.as_str()) {
            i += 1;
        } else {
            return false;
        }
        names += 1;
    }
    names >= 2
}

/// Removes numeric footnote markers (`[1]`, `[12]`) and `[citation needed]` from `w`:
/// "evidence.[1]" -> "evidence.", "[12]" -> "". Borrows when there is nothing to strip.
#[cfg_attr(not(test), allow(dead_code))] // wired in part B
pub fn strip_footnote(w: &str) -> Cow<'_, str> {
    if !w.contains('[') {
        return Cow::Borrowed(w);
    }
    let mut out = String::with_capacity(w.len());
    let mut rest = w;
    let mut changed = false;
    while let Some(open) = rest.find('[') {
        let (head, tail) = rest.split_at(open);
        let Some(close) = tail.find(']') else { break };
        let inner = tail.get(1..close).unwrap_or("");
        let marker = (!inner.is_empty() && inner.len() <= 4 && inner.bytes().all(|b| b.is_ascii_digit()))
            || inner.eq_ignore_ascii_case("citation needed");
        out.push_str(head);
        if marker {
            changed = true;
        } else {
            out.push_str(tail.get(..=close).unwrap_or(tail));
        }
        rest = tail.get(close + 1..).unwrap_or("");
    }
    if !changed {
        return Cow::Borrowed(w);
    }
    out.push_str(rest);
    Cow::Owned(out)
}

/// One flag per word: true for the words of a junk block. A block runs from a word
/// with `brk.is_break()` to the next one. With `protect_first` the first block (the
/// one the read starts in, which Alex clicked) is never junk.
#[cfg_attr(not(test), allow(dead_code))] // wired in part B
pub fn junk_mask(words: &[Word], protect_first: bool) -> Vec<bool> {
    let mut mask = vec![false; words.len()];
    let mut start = 0usize;
    let mut first = true;
    while start < words.len() {
        let mut end = start + 1;
        while words.get(end).is_some_and(|w| !w.brk.is_break()) {
            end += 1;
        }
        if !(first && protect_first) {
            let texts: Vec<&str> = words.get(start..end).unwrap_or(&[]).iter().map(|w| w.text.as_str()).collect();
            if is_junk_block(&texts) {
                if let Some(m) = mask.get_mut(start..end) {
                    m.fill(true);
                }
            }
        }
        first = false;
        start = end;
    }
    mask
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ra_struct::Brk;

    fn w(text: &str, brk: Brk) -> Word {
        Word { text: text.into(), src: vec![0], para_break_before: false, brk }
    }

    fn block(s: &str) -> bool {
        is_junk_block(&s.split_whitespace().collect::<Vec<_>>())
    }

    #[test]
    fn junk_phrases_match_ignoring_case_and_punctuation() {
        for s in [
            "Advertisement",
            "ADVERTISEMENT:",
            "Skip to content",
            "skip to main content",
            "Share this article",
            "Sign up for our newsletter!",
            "Related Articles",
            "Read more »",
            "Ad",
            "Continue reading...",
            "You may also like",
        ] {
            assert!(block(s), "{s}");
        }
    }

    #[test]
    fn real_sentences_are_never_junk() {
        for s in [
            "Share prices fell today.",
            "I will share this with you.",
            "Read more books.",
            "Advertisement revenue fell",
            "The ad was funny",
            "Share your thoughts below",
            "Please share this article with friends today",
            "",
        ] {
            assert!(!block(s), "{s}");
        }
    }

    #[test]
    fn social_runs_need_two_names() {
        assert!(block("Facebook Twitter LinkedIn Email"));
        assert!(block("Twitter Facebook"));
        assert!(block("Email Print Copy link"));
        assert!(block("copy link Facebook"));
        assert!(!block("Twitter"));
        assert!(!block("Facebook launched today"));
        assert!(!block("X"));
        assert!(!block("copy link"));
        assert!(!block("Facebook and Twitter"));
    }

    #[test]
    fn footnotes_strip() {
        assert_eq!(strip_footnote("evidence.[1]"), "evidence.");
        assert_eq!(strip_footnote("[12]"), "");
        assert_eq!(strip_footnote("claim[3][4],"), "claim,");
        assert_eq!(strip_footnote("[citation needed]"), "");
        assert_eq!(strip_footnote("fact.[Citation Needed]"), "fact.");
        assert!(matches!(strip_footnote("plain"), Cow::Borrowed(_)));
        // not markers: kept as written
        assert_eq!(strip_footnote("[note]"), "[note]");
        assert!(matches!(strip_footnote("[note]"), Cow::Borrowed(_)));
        assert_eq!(strip_footnote("a[b"), "a[b");
        assert_eq!(strip_footnote("x]y["), "x]y[");
        assert_eq!(strip_footnote("arr[0]"), "arr");
        assert_eq!(strip_footnote("[]"), "[]");
        assert_eq!(strip_footnote("[12345]"), "[12345]");
        assert_eq!(strip_footnote("[\u{fc}n\u{ef}]\u{e9}"), "[\u{fc}n\u{ef}]\u{e9}");
    }

    #[test]
    fn mid_article_junk_block_is_masked_first_block_never() {
        let ws = [
            w("Advertisement", Brk::None),
            w("Real", Brk::Paragraph),
            w("text", Brk::None),
            w("Advertisement", Brk::Paragraph),
            w("More", Brk::Paragraph),
            w("Share", Brk::Paragraph),
            w("this", Brk::None),
            w("article", Brk::None),
        ];
        assert_eq!(junk_mask(&ws, true), vec![false, false, false, true, false, true, true, true]);
        // unprotected: the first block may be junk too
        assert!(junk_mask(&ws, false)[0]);
        assert!(junk_mask(&[], true).is_empty());
    }

    #[test]
    fn sentence_with_share_in_a_block_is_kept() {
        let ws = [w("Hello", Brk::None), w("Share", Brk::Paragraph), w("prices", Brk::None), w("rose.", Brk::None)];
        assert_eq!(junk_mask(&ws, true), vec![false; 4]);
    }
}
