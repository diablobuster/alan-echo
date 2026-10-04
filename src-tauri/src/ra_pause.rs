//! Read Aloud: the pause after a chunk and the spoken form of a word before a
//! structural boundary (natural-reading spec §2). Pure, never panics.

use crate::ra_struct::Brk;
use std::borrow::Cow;

/// Structural pauses at 1x, in ms (the whole silence between the two words).
const LINE_MS: f32 = 250.0;
const TABLE_CELL_MS: f32 = 250.0;
const LIST_ITEM_MS: f32 = 400.0;
const TABLE_ROW_MS: f32 = 450.0;
/// Caption, quote and code blocks.
const BLOCK_MS: f32 = 450.0;
const PARAGRAPH_MS: f32 = 550.0;
/// Before a heading starts.
const HEADING_MS: f32 = 450.0;
/// After a heading, before its first body word.
const AFTER_HEADING_MS: f32 = 650.0;
const SECTION_MS: f32 = 900.0;
/// Structural pauses shrink with `rate^SCALE_EXP`: slower than speed, so structure
/// stays audible fast.
const SCALE_EXP: f32 = 0.6;
/// No structural pause is shorter than this, at any speed.
pub const PAUSE_FLOOR_MS: f32 = 150.0;

/// Punctuation pauses at a chunk edge at 1x (divided by speed, like the model's own
/// pauses inside a chunk): sentence end, comma/clause/dash, cut mid-phrase.
const SENTENCE_MS: f32 = 200.0;
const COMMA_MS: f32 = 150.0;
const PHRASE_MS: f32 = 60.0;

/// Closing quotes and brackets, skipped when looking at a word's last punctuation.
const CLOSERS: [char; 6] = ['"', '\'', ')', ']', '\u{201D}', '\u{2019}'];

/// The reading speed for pauses: below 1x (or junk) counts as 1x.
fn speed(rate: f32) -> f32 {
    rate.max(1.0)
}

/// The pause for a structural boundary at `rate`: `base / rate^0.6`, never under
/// `PAUSE_FLOOR_MS`. `Brk::None` has no pause (0).
pub fn pause_ms(brk: Brk, rate: f32) -> f32 {
    let base = match brk {
        Brk::None => return 0.0,
        Brk::Line => LINE_MS,
        Brk::TableCell => TABLE_CELL_MS,
        Brk::ListItem => LIST_ITEM_MS,
        Brk::TableRow => TABLE_ROW_MS,
        Brk::Caption | Brk::Quote | Brk::Code => BLOCK_MS,
        Brk::Paragraph => PARAGRAPH_MS,
        Brk::Heading(_) => HEADING_MS,
        Brk::AfterHeading => AFTER_HEADING_MS,
        Brk::Section => SECTION_MS,
    };
    (base / speed(rate).powf(SCALE_EXP)).max(PAUSE_FLOOR_MS)
}

/// The word without its closing quotes and brackets.
fn core(word: &str) -> &str {
    word.trim_end_matches(CLOSERS)
}

/// The punctuation pause after a chunk ending in `last` (no structure): a sentence
/// end (or no word) 200 ms, a comma/semicolon/colon/dash 150 ms, else 60 ms; at 1x,
/// divided by speed.
pub fn punct_ms(last: &str, rate: f32) -> f32 {
    let w = core(last);
    let base = if w.is_empty() || w.ends_with(['.', '!', '?']) {
        SENTENCE_MS
    } else if w.ends_with([',', ';', ':', '\u{2014}', '\u{2013}']) {
        COMMA_MS
    } else {
        PHRASE_MS
    };
    base / speed(rate)
}

/// The silence to leave after a chunk ending in `last_word`, before a word whose
/// boundary is `next_brk` (`None`: no next word yet, the end of the read so far: a
/// sentence pause). A structural break never pauses less than the punctuation.
pub fn after_ms(last_word: &str, next_brk: Option<Brk>, rate: f32) -> f32 {
    match next_brk {
        None => SENTENCE_MS / speed(rate),
        Some(b) => pause_ms(b, rate).max(punct_ms(last_word, rate)),
    }
}

/// How `word` is spoken before a boundary `next_brk` (never stored in `Word::text`):
/// before a heading, list item, row, paragraph, caption, quote, code, section or line
/// a word without a final `.` `!` `?` `:` `;` gets a "." (a falling end); before a
/// table cell a word ending in a letter or digit gets a ",". Closing quotes and
/// brackets are looked past. A word with no letter or digit is left alone.
pub fn speak_form(word: &str, next_brk: Brk) -> Cow<'_, str> {
    if !word.chars().any(char::is_alphanumeric) {
        return Cow::Borrowed(word);
    }
    let last = core(word).chars().last();
    let add = match next_brk {
        Brk::None => None,
        Brk::TableCell => last.filter(|c| c.is_alphanumeric()).map(|_| ','),
        _ => (!last.is_some_and(|c| matches!(c, '.' | '!' | '?' | ':' | ';'))).then_some('.'),
    };
    match add {
        Some(c) => {
            let mut s = String::with_capacity(word.len() + 1);
            s.push_str(word);
            s.push(c);
            Cow::Owned(s)
        }
        None => Cow::Borrowed(word),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Brk; 11] = [
        Brk::Line,
        Brk::TableCell,
        Brk::ListItem,
        Brk::TableRow,
        Brk::Caption,
        Brk::Quote,
        Brk::Code,
        Brk::Paragraph,
        Brk::Heading(2),
        Brk::AfterHeading,
        Brk::Section,
    ];

    #[test]
    fn bases_at_1x_are_the_spec_values() {
        let want = [250.0, 250.0, 400.0, 450.0, 450.0, 450.0, 450.0, 550.0, 450.0, 650.0, 900.0];
        for (b, w) in ALL.iter().zip(want) {
            assert_eq!(pause_ms(*b, 1.0), w, "{b:?}");
        }
        assert_eq!(pause_ms(Brk::None, 1.0), 0.0);
        assert_eq!(pause_ms(Brk::None, 3.0), 0.0);
        // below 1x (or junk) the 1x pause
        assert_eq!(pause_ms(Brk::Paragraph, 0.5), 550.0);
        assert_eq!(pause_ms(Brk::Paragraph, f32::NAN), 550.0);
        assert_eq!(pause_ms(Brk::Paragraph, f32::INFINITY), PAUSE_FLOOR_MS);
    }

    #[test]
    fn pauses_scale_less_than_speed_keep_their_order_and_floor() {
        for rate in [1.0f32, 2.2, 3.0, 3.5] {
            let p = |b| pause_ms(b, rate);
            assert!(p(Brk::Line) <= p(Brk::ListItem), "{rate}");
            assert!(p(Brk::ListItem) <= p(Brk::Paragraph), "{rate}");
            assert!(p(Brk::Paragraph) <= p(Brk::AfterHeading), "{rate}");
            assert!(p(Brk::AfterHeading) <= p(Brk::Section), "{rate}");
            for b in ALL {
                assert!(p(b) >= PAUSE_FLOOR_MS, "{b:?} at {rate}x");
            }
        }
        // spec: at 2.2x Paragraph ~340 ms, AfterHeading ~405 ms
        assert!((pause_ms(Brk::Paragraph, 2.2) - 344.0).abs() < 5.0, "{}", pause_ms(Brk::Paragraph, 2.2));
        assert!((pause_ms(Brk::AfterHeading, 2.2) - 406.0).abs() < 5.0);
        assert_eq!(pause_ms(Brk::Line, 3.5), PAUSE_FLOOR_MS);
    }

    #[test]
    fn punctuation_pause_is_todays_rule() {
        assert_eq!(punct_ms("ended.", 1.0), 200.0);
        assert_eq!(punct_ms("ended.\u{201D}", 2.0), 100.0);
        assert_eq!(punct_ms("", 1.0), 200.0);
        assert_eq!(punct_ms("year,", 1.0), 150.0);
        assert_eq!(punct_ms("said;", 1.0), 150.0);
        assert_eq!(punct_ms("so\u{2014}", 1.0), 150.0);
        assert_eq!(punct_ms("developers", 1.0), 60.0);
        assert!((punct_ms("year,", 2.2) - 150.0 / 2.2).abs() < 1e-3);
    }

    #[test]
    fn after_ms_takes_the_longer_of_break_and_punctuation() {
        // end of the words so far: a sentence pause whatever the last word
        assert_eq!(after_ms("developers", None, 1.0), 200.0);
        assert_eq!(after_ms("developers", None, 2.0), 100.0);
        // running text: the punctuation pause
        assert_eq!(after_ms("developers", Some(Brk::None), 1.0), 60.0);
        assert_eq!(after_ms("year,", Some(Brk::None), 1.0), 150.0);
        // a break: its pause
        assert_eq!(after_ms("Takeaways", Some(Brk::AfterHeading), 1.0), 650.0);
        assert_eq!(after_ms("grew.", Some(Brk::Paragraph), 2.2), pause_ms(Brk::Paragraph, 2.2));
        // never less than the punctuation pause (a sentence end before a table cell at 1x
        // is 250 vs 200; at 1x a cell pause wins, the floor keeps it above a comma)
        for rate in [1.0f32, 2.2, 3.0, 3.5] {
            for b in ALL {
                assert!(after_ms("ended.", Some(b), rate) >= punct_ms("ended.", rate), "{b:?} {rate}");
            }
        }
    }

    #[test]
    fn speak_form_gives_boundaries_a_falling_end() {
        assert_eq!(speak_form("Takeaways", Brk::AfterHeading), "Takeaways.");
        assert_eq!(speak_form("Takeaways", Brk::Heading(2)), "Takeaways.");
        assert_eq!(speak_form("milk", Brk::ListItem), "milk.");
        assert_eq!(speak_form("grew.", Brk::Paragraph), "grew.");
        assert_eq!(speak_form("why?", Brk::Section), "why?");
        assert_eq!(speak_form("Note:", Brk::Line), "Note:");
        assert_eq!(speak_form("\"Hello\"", Brk::Paragraph), "\"Hello\".");
        assert_eq!(speak_form("\"Hello.\"", Brk::Paragraph), "\"Hello.\"");
        assert_eq!(speak_form("(see\u{201D})", Brk::Quote), "(see\u{201D}).");
        assert_eq!(speak_form("Price", Brk::TableCell), "Price,");
        assert_eq!(speak_form("Price:", Brk::TableCell), "Price:");
        assert_eq!(speak_form("$4.20", Brk::TableCell), "$4.20,");
        // never: running text, no letters or digits
        assert_eq!(speak_form("Takeaways", Brk::None), "Takeaways");
        assert_eq!(speak_form("\u{2014}", Brk::Paragraph), "\u{2014}");
        assert_eq!(speak_form("\u{2022}", Brk::ListItem), "\u{2022}");
        assert_eq!(speak_form("", Brk::Heading(1)), "");
        assert!(matches!(speak_form("grew.", Brk::Paragraph), Cow::Borrowed(_)));
    }
}
