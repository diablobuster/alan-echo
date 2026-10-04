//! Read Aloud: page structure before a word (natural-reading spec §1).
//!
//! Sources keep the `Vec<String>` token contract and may prefix a token with
//! `mark(brk)` (`U+E0F0` + one code char): the structural boundary before that token.
//! `ra_text::normalize` strips the marker and sets `Word::brk`. Never panics.

/// The structural boundary before a word. Punctuation pauses (comma, clause,
/// sentence) come from the text itself and are not listed here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Brk {
    /// Inside running text.
    #[default]
    None,
    /// A hard line break inside a block (address, poem, untagged PDF line).
    Line,
    ListItem,
    TableCell,
    TableRow,
    Paragraph,
    Caption,
    Quote,
    Code,
    /// A heading starts (level 1-6, 0 = unknown).
    Heading(u8),
    /// The first word after a heading.
    AfterHeading,
    /// A new section (a landmark change, a heading after a long block).
    Section,
}

/// Starts an in-band structure marker.
pub const MARK: char = '\u{E0F0}';

impl Brk {
    fn code(self) -> char {
        match self {
            Brk::None => 'n',
            Brk::Line => 'l',
            Brk::ListItem => 'i',
            Brk::TableCell => 'c',
            Brk::TableRow => 'r',
            Brk::Paragraph => 'p',
            Brk::Caption => 'a',
            Brk::Quote => 'q',
            Brk::Code => 'k',
            Brk::Heading(l) => char::from(b'0' + l.min(9)),
            Brk::AfterHeading => 'h',
            Brk::Section => 's',
        }
    }

    fn from_code(c: char) -> Option<Brk> {
        Some(match c {
            'n' => Brk::None,
            'l' => Brk::Line,
            'i' => Brk::ListItem,
            'c' => Brk::TableCell,
            'r' => Brk::TableRow,
            'p' => Brk::Paragraph,
            'a' => Brk::Caption,
            'q' => Brk::Quote,
            'k' => Brk::Code,
            'h' => Brk::AfterHeading,
            's' => Brk::Section,
            '0'..='9' => Brk::Heading(c as u8 - b'0'),
            _ => return None,
        })
    }

    /// Any boundary other than running text.
    pub fn is_break(self) -> bool {
        self != Brk::None
    }

    pub fn is_heading(self) -> bool {
        matches!(self, Brk::Heading(_))
    }
}

/// The marker to prefix a token with.
pub fn mark(b: Brk) -> String {
    let mut s = String::with_capacity(4);
    s.push(MARK);
    s.push(b.code());
    s
}

/// A token's leading markers stripped: the last valid one wins (a source may add a
/// more specific one after a generic one). An unknown code is stripped and ignored.
pub fn strip_mark(tok: &str) -> (Option<Brk>, &str) {
    let mut rest = tok;
    let mut brk = None;
    while let Some(after) = rest.strip_prefix(MARK) {
        let mut cs = after.chars();
        match cs.next() {
            Some(c) => {
                if let Some(b) = Brk::from_code(c) {
                    brk = Some(b);
                }
                rest = cs.as_str();
            }
            None => {
                rest = after;
                break;
            }
        }
    }
    (brk, rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_round_trip() {
        for b in [
            Brk::None,
            Brk::Line,
            Brk::ListItem,
            Brk::TableCell,
            Brk::TableRow,
            Brk::Paragraph,
            Brk::Caption,
            Brk::Quote,
            Brk::Code,
            Brk::Heading(0),
            Brk::Heading(2),
            Brk::Heading(6),
            Brk::AfterHeading,
            Brk::Section,
        ] {
            let t = format!("{}Word ", mark(b));
            assert_eq!(strip_mark(&t), (Some(b), "Word "), "{b:?}");
        }
    }

    #[test]
    fn plain_tokens_and_bad_marks_are_safe() {
        assert_eq!(strip_mark("plain"), (None, "plain"));
        assert_eq!(strip_mark(""), (None, ""));
        assert_eq!(strip_mark("\u{E0F0}"), (None, ""));
        assert_eq!(strip_mark("\u{E0F0}Zx"), (None, "x"));
        let two = format!("{}{}x", mark(Brk::Paragraph), mark(Brk::Heading(2)));
        assert_eq!(strip_mark(&two), (Some(Brk::Heading(2)), "x"));
    }
}
