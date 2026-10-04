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

/// Silence kept before a chunk's first sound, at 1x (divided by the reading speed),
/// but never less than `KEEP_LEAD_MIN_MS` whatever the speed: a quiet first sound
/// ("h", "f", "s") fades in below the threshold and must not be clipped.
pub const KEEP_LEAD_MS: f32 = 40.0;
pub const KEEP_LEAD_MIN_MS: f32 = 30.0;

/// Closing quotes and brackets, skipped when looking at a word's last punctuation.
const CLOSERS: [char; 6] = ['"', '\'', ')', ']', '\u{201D}', '\u{2019}'];

/// The reading speed for pauses: below 1x (or junk) counts as 1x.
fn speed(rate: f32) -> f32 {
    rate.max(1.0)
}

/// The silence the engine keeps before a chunk's first sound at `rate`.
pub fn lead_ms(rate: f32) -> f32 {
    (KEEP_LEAD_MS / speed(rate)).max(KEEP_LEAD_MIN_MS)
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

/// The whole silence to leave after a chunk ending in `last_word`, before a word whose
/// boundary is `next_brk`. Running text (`Brk::None`) keeps today's audible gap: the
/// punctuation pause plus the next chunk's lead. `None` (no next word yet, the end of
/// the read so far) is a sentence's gap. A structural break never pauses less than the
/// punctuation gap.
pub fn after_ms(last_word: &str, next_brk: Option<Brk>, rate: f32) -> f32 {
    let punct = match next_brk {
        None => SENTENCE_MS / speed(rate),
        Some(_) => punct_ms(last_word, rate),
    } + lead_ms(rate);
    pause_ms(next_brk.unwrap_or_default(), rate).max(punct)
}

/// How `word` is spoken before a boundary `next_brk` (never stored in `Word::text`).
/// Before a heading, list item, row, paragraph, caption, quote, code, section or line:
/// a final `,` or dash is replaced by "." and a word not ending in `.` `!` `?` `:` `;`
/// gets a "." (a falling end). Before a table cell: a final dash is replaced by "," and
/// a word ending in a letter or digit gets a ",". Closing quotes and brackets are looked
/// past and kept after the mark. A word with no letter or digit is left alone, and so
/// is one the abbreviation table would expand once dotted (`St`, `Dr`, `no`).
pub fn speak_form(word: &str, next_brk: Brk) -> Cow<'_, str> {
    if next_brk == Brk::None || !word.chars().any(char::is_alphanumeric) {
        return Cow::Borrowed(word);
    }
    let body = core(word);
    let closers = word.get(body.len()..).unwrap_or("");
    let Some(last) = body.chars().last() else { return Cow::Borrowed(word) };
    let dash = matches!(last, '\u{2014}' | '\u{2013}');
    let (cut, mark) = if next_brk == Brk::TableCell {
        if dash {
            (true, ',')
        } else if last.is_alphanumeric() {
            (false, ',')
        } else {
            return Cow::Borrowed(word);
        }
    } else if dash || last == ',' {
        (true, '.')
    } else if matches!(last, '.' | '!' | '?' | ':' | ';') {
        return Cow::Borrowed(word);
    } else {
        (false, '.')
    };
    let keep = if cut { body.get(..body.len() - last.len_utf8()).unwrap_or(body) } else { body };
    // final-review ruling: a "." the normalizer would expand ("St." = saint) is never
    // added; the pause alone marks the break
    if mark == '.' && crate::ra_norm_text::expands_when_dotted(keep.trim_start_matches(|c: char| !c.is_alphanumeric())) {
        return Cow::Borrowed(word);
    }
    let mut s = String::with_capacity(word.len() + 1);
    s.push_str(keep);
    s.push(mark);
    s.push_str(closers);
    Cow::Owned(s)
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
        // end of the words so far: a sentence pause (plus the lead, as today)
        assert_eq!(after_ms("developers", None, 1.0), 200.0 + KEEP_LEAD_MS);
        assert_eq!(after_ms("developers", None, 2.0), 100.0 + KEEP_LEAD_MIN_MS);
        // running text: today's audible gap, the punctuation trail plus the next lead
        assert_eq!(after_ms("developers", Some(Brk::None), 1.0), 60.0 + KEEP_LEAD_MS);
        assert_eq!(after_ms("year,", Some(Brk::None), 1.0), 150.0 + KEEP_LEAD_MS);
        for rate in [1.0f32, 2.2, 3.0, 3.5] {
            assert_eq!(after_ms("ended.", Some(Brk::None), rate), punct_ms("ended.", rate) + lead_ms(rate));
        }
        assert_eq!(lead_ms(1.0), 40.0);
        assert_eq!(lead_ms(2.2), 30.0);
        assert_eq!(lead_ms(f32::NAN), 40.0);
        // a break: its pause
        assert_eq!(after_ms("Takeaways", Some(Brk::AfterHeading), 1.0), 650.0);
        assert_eq!(after_ms("grew.", Some(Brk::Paragraph), 2.2), pause_ms(Brk::Paragraph, 2.2));
        // never less than the punctuation gap
        for rate in [1.0f32, 2.2, 3.0, 3.5] {
            for b in ALL {
                assert!(after_ms("ended.", Some(b), rate) >= punct_ms("ended.", rate) + lead_ms(rate), "{b:?} {rate}");
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
        assert_eq!(speak_form("\"Hello\"", Brk::Paragraph), "\"Hello.\"", "the mark goes inside the closers");
        assert_eq!(speak_form("\"Hello.\"", Brk::Paragraph), "\"Hello.\"");
        assert_eq!(speak_form("(see\u{201D})", Brk::Quote), "(see.\u{201D})");
        assert_eq!(speak_form("Price", Brk::TableCell), "Price,");
        assert_eq!(speak_form("Price:", Brk::TableCell), "Price:");
        assert_eq!(speak_form("$4.20", Brk::TableCell), "$4.20,");
        // a trailing comma or dash is replaced, never stacked
        assert_eq!(speak_form("apples,", Brk::ListItem), "apples.");
        assert_eq!(speak_form("so\u{2014}", Brk::Paragraph), "so.");
        assert_eq!(speak_form("then\u{2013}", Brk::AfterHeading), "then.");
        assert_eq!(speak_form("\"apples,\"", Brk::Paragraph), "\"apples.\"");
        assert_eq!(speak_form("(so\u{2014})", Brk::Section), "(so.)");
        assert_eq!(speak_form("so\u{2014}", Brk::TableCell), "so,");
        assert_eq!(speak_form("so\u{2013}", Brk::TableCell), "so,");
        assert_eq!(speak_form("apples,", Brk::TableCell), "apples,");
        assert_eq!(speak_form("Price.", Brk::TableCell), "Price.");
        // only punctuation: untouched
        assert_eq!(speak_form(",", Brk::Paragraph), ",");
        assert_eq!(speak_form("\u{2013}", Brk::TableCell), "\u{2013}");
        assert_eq!(speak_form("\"\u{2014}\"", Brk::Heading(1)), "\"\u{2014}\"");
        // never: running text, no letters or digits
        assert_eq!(speak_form("Takeaways", Brk::None), "Takeaways");
        assert_eq!(speak_form("\u{2014}", Brk::Paragraph), "\u{2014}");
        assert_eq!(speak_form("\u{2022}", Brk::ListItem), "\u{2022}");
        assert_eq!(speak_form("", Brk::Heading(1)), "");
        assert!(matches!(speak_form("grew.", Brk::Paragraph), Cow::Borrowed(_)));
    }

    /// Final-review ruling: a word the abbreviation table would expand once dotted
    /// ("St." = saint, "Dr." = doctor, "No." = number) never gets the ".": the pause
    /// alone marks the break. Matched as written in the table, or ALL CAPS; a lowercase
    /// common word ("sun", "no") still falls.
    #[test]
    fn speak_form_never_dots_an_abbreviation() {
        let line: Vec<String> = ["Main", "St"].iter().map(|w| speak_form(w, Brk::None).into_owned()).collect();
        assert_eq!(line, ["Main", "St"]);
        assert_eq!(speak_form("St", Brk::Line), "St", "14 Main St / Springfield");
        assert_eq!(speak_form("Dr", Brk::Line), "Dr");
        assert_eq!(speak_form("No", Brk::ListItem), "No");
        assert_eq!(speak_form("Mr", Brk::Line), "Mr");
        assert_eq!(speak_form("ST", Brk::Paragraph), "ST");
        assert_eq!(speak_form("DR", Brk::Line), "DR");
        // controller ruling: never the lowercase common word
        let the_sun: Vec<String> = [("the", Brk::None), ("sun", Brk::Paragraph)].iter().map(|(w, b)| speak_form(w, *b).into_owned()).collect();
        assert_eq!(the_sun, ["the", "sun."]);
        let say_no: Vec<String> = [("Say", Brk::None), ("no", Brk::Paragraph)].iter().map(|(w, b)| speak_form(w, *b).into_owned()).collect();
        assert_eq!(say_no, ["Say", "no."]);
        for w in ["cat", "art", "wed", "st", "dr", "mar"] {
            assert_eq!(speak_form(w, Brk::Paragraph), format!("{w}."), "{w}");
        }
        assert_eq!(speak_form("sT", Brk::Paragraph), "sT.", "mixed case is not the table form");
        assert_eq!(speak_form("Ave", Brk::AfterHeading), "Ave");
        assert_eq!(speak_form("Inc", Brk::Heading(2)), "Inc");
        assert_eq!(speak_form("(Jr)", Brk::Paragraph), "(Jr)");
        assert_eq!(speak_form("St,", Brk::Line), "St,", "a comma is not replaced by an expanding dot");
        assert_eq!(speak_form("St", Brk::TableCell), "St,", "a cell comma never expands");
        assert_eq!(speak_form("Street", Brk::Line), "Street.");
        assert_eq!(speak_form("Takeaways", Brk::AfterHeading), "Takeaways.");
    }
}
