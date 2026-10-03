//! Read Aloud text normaliser: the non-number spans of the phase 3 pre-pass
//! (spec §3). Abbreviations, dotted initialisms, domains, URLs, emails, @handles,
//! #hashtags, symbols and math, units after a number, Roman numerals in context and
//! glued ellipses.
//!
//! `text_spans(words)` returns `(start, end, spoken)` over source-word indices (end
//! exclusive), sorted and non-overlapping. Spoken style: lowercase words, single
//! spaces, no hyphens; letters to be spelled are uppercase and space-separated
//! ("U S"). A trailing `,;:!?` on the source word is kept on `spoken`, and so is a
//! sentence-final `.` (so the sentence pause survives).
//!
//! Pure: static sorted tables (binary search), no regex, no allocation at rest, and
//! no byte slicing except at `char_indices` boundaries via `str::get`.

use std::borrow::Cow;

/// When an abbreviation expands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Rule {
    /// Unambiguous: always expand.
    Always,
    /// A title: only when the next word is a capitalised name (`Sen. Warren`).
    Title,
    /// Only before a number or a Roman numeral (`p. 12`, `Vol. III`).
    Number,
    /// Only before a digit (`No. 5`, `Jan. 5`); never before a letter (`No. C.`).
    Digit,
    /// `Sat.` / `Sun.`: only before a number or a month (they are also words).
    DateNext,
    /// `Dr.`: Drive after a street name, else Doctor.
    Dr,
    /// `St.`: Street after a street name, else Saint.
    St,
    /// `v.`: only between two capitalised words (`Roe v. Wade`).
    Versus,
}

/// Written form (exact case, with its period) → spoken, rule. Sorted by bytes.
static ABBREVS: &[(&str, &str, Rule)] = &[
    // BEGIN SORTED
    ("A.M.", "A M", Rule::Always),
    ("Adm.", "admiral", Rule::Title),
    ("Amb.", "ambassador", Rule::Title),
    ("Approx.", "approximately", Rule::Always),
    ("Apr.", "april", Rule::Always),
    ("Art.", "article", Rule::Number),
    ("Assn.", "association", Rule::Always),
    ("Atty.", "attorney", Rule::Title),
    ("Aug.", "august", Rule::Always),
    ("Ave.", "avenue", Rule::Always),
    ("Blvd.", "boulevard", Rule::Always),
    ("Bros.", "brothers", Rule::Always),
    ("Capt.", "captain", Rule::Title),
    ("Ch.", "chapter", Rule::Number),
    ("Cmdr.", "commander", Rule::Title),
    ("Co.", "company", Rule::Always),
    ("Col.", "colonel", Rule::Title),
    ("Corp.", "corporation", Rule::Always),
    ("Cpl.", "corporal", Rule::Title),
    ("Ct.", "court", Rule::Always),
    ("Dec.", "december", Rule::Always),
    ("Dept.", "department", Rule::Always),
    ("Dr.", "doctor", Rule::Dr),
    ("E.g.", "for example", Rule::Always),
    ("Esq.", "esquire", Rule::Always),
    ("Feb.", "february", Rule::Always),
    ("Fig.", "figure", Rule::Number),
    ("Fr.", "father", Rule::Title),
    ("Fri.", "friday", Rule::Always),
    ("Ft.", "fort", Rule::Title),
    ("Gen.", "general", Rule::Title),
    ("Gov.", "governor", Rule::Title),
    ("Hon.", "honorable", Rule::Title),
    ("Hwy.", "highway", Rule::Always),
    ("I.e.", "that is", Rule::Always),
    ("Inc.", "incorporated", Rule::Always),
    ("Jan.", "january", Rule::Digit),
    ("Jr.", "junior", Rule::Always),
    ("Jul.", "july", Rule::Always),
    ("Jun.", "june", Rule::Digit),
    ("Ln.", "lane", Rule::Always),
    ("Lt.", "lieutenant", Rule::Title),
    ("Ltd.", "limited", Rule::Always),
    ("Maj.", "major", Rule::Title),
    ("Mar.", "march", Rule::Always),
    ("Mme.", "madame", Rule::Title),
    ("Mon.", "monday", Rule::Always),
    ("Mr.", "mister", Rule::Always),
    ("Mrs.", "missus", Rule::Always),
    ("Ms.", "miz", Rule::Always),
    ("Mt.", "mount", Rule::Title),
    ("Mtn.", "mountain", Rule::Always),
    ("No.", "number", Rule::Digit),
    ("Nos.", "numbers", Rule::Digit),
    ("Nov.", "november", Rule::Always),
    ("Oct.", "october", Rule::Always),
    ("P.M.", "P M", Rule::Always),
    ("Pkwy.", "parkway", Rule::Always),
    ("Pres.", "president", Rule::Title),
    ("Prof.", "professor", Rule::Title),
    ("Rd.", "road", Rule::Always),
    ("Rep.", "representative", Rule::Title),
    ("Rev.", "reverend", Rule::Title),
    ("Rte.", "route", Rule::Number),
    ("Sat.", "saturday", Rule::DateNext),
    ("Sec.", "section", Rule::Number),
    ("Sen.", "senator", Rule::Title),
    ("Sep.", "september", Rule::Always),
    ("Sept.", "september", Rule::Always),
    ("Sgt.", "sergeant", Rule::Title),
    ("Sr.", "senior", Rule::Always),
    ("St.", "saint", Rule::St),
    ("Sun.", "sunday", Rule::DateNext),
    ("Supt.", "superintendent", Rule::Title),
    ("Thu.", "thursday", Rule::Always),
    ("Thurs.", "thursday", Rule::Always),
    ("Tue.", "tuesday", Rule::Always),
    ("Tues.", "tuesday", Rule::Always),
    ("Univ.", "university", Rule::Always),
    ("Vol.", "volume", Rule::Number),
    ("Vs.", "versus", Rule::Always),
    ("Wed.", "wednesday", Rule::Always),
    ("a.m.", "A M", Rule::Always),
    ("approx.", "approximately", Rule::Always),
    ("avg.", "average", Rule::Always),
    ("cf.", "compare", Rule::Always),
    ("ch.", "chapter", Rule::Number),
    ("dept.", "department", Rule::Always),
    ("e.g.", "for example", Rule::Always),
    ("est.", "estimated", Rule::Always),
    ("etc.", "et cetera", Rule::Always),
    ("excl.", "excluding", Rule::Always),
    ("ext.", "extension", Rule::Number),
    ("fig.", "figure", Rule::Number),
    ("govt.", "government", Rule::Always),
    ("hr.", "hour", Rule::Always),
    ("hrs.", "hours", Rule::Always),
    ("i.e.", "that is", Rule::Always),
    ("incl.", "including", Rule::Always),
    ("intl.", "international", Rule::Always),
    ("max.", "maximum", Rule::Always),
    ("mins.", "minutes", Rule::Always),
    ("mos.", "months", Rule::Always),
    ("natl.", "national", Rule::Always),
    ("no.", "number", Rule::Digit),
    ("nos.", "numbers", Rule::Digit),
    ("p.", "page", Rule::Number),
    ("p.m.", "P M", Rule::Always),
    ("pct.", "percent", Rule::Always),
    ("pp.", "pages", Rule::Number),
    ("secs.", "seconds", Rule::Always),
    ("v.", "versus", Rule::Versus),
    ("vol.", "volume", Rule::Number),
    ("vs.", "versus", Rule::Always),
    ("yr.", "year", Rule::Always),
    ("yrs.", "years", Rule::Always),
    // END SORTED
];

/// Unit (exact case) → singular, plural, and whether it may be glued to the
/// number (`5km`). Only used after a number. Sorted by bytes.
static UNITS: &[(&str, &str, &str, bool)] = &[
    // BEGIN SORTED
    ("GB", "gigabyte", "gigabytes", true),
    ("GHz", "gigahertz", "gigahertz", true),
    ("GW", "gigawatt", "gigawatts", true),
    ("GWh", "gigawatt hour", "gigawatt hours", true),
    ("Gb", "gigabit", "gigabits", true),
    ("Gbit", "gigabit", "gigabits", true),
    ("Gbps", "gigabit per second", "gigabits per second", true),
    ("Hz", "hertz", "hertz", true),
    ("KB", "kilobyte", "kilobytes", true),
    ("L", "liter", "liters", false),
    ("MB", "megabyte", "megabytes", true),
    ("MHz", "megahertz", "megahertz", true),
    ("MW", "megawatt", "megawatts", true),
    ("MWh", "megawatt hour", "megawatt hours", true),
    ("Mb", "megabit", "megabits", true),
    ("Mbit", "megabit", "megabits", true),
    ("Mbps", "megabit per second", "megabits per second", true),
    ("PB", "petabyte", "petabytes", true),
    ("TB", "terabyte", "terabytes", true),
    ("TWh", "terawatt hour", "terawatt hours", true),
    ("V", "volt", "volts", true),
    ("W", "watt", "watts", true),
    ("cc", "cubic centimeter", "cubic centimeters", true),
    ("cm", "centimeter", "centimeters", true),
    ("dB", "decibel", "decibels", true),
    ("fps", "frame per second", "frames per second", true),
    ("ft", "foot", "feet", true),
    ("g", "gram", "grams", false),
    ("gal", "gallon", "gallons", true),
    ("ha", "hectare", "hectares", false),
    ("hr", "hour", "hours", true),
    ("hrs", "hours", "hours", true),
    ("kB", "kilobyte", "kilobytes", true),
    ("kHz", "kilohertz", "kilohertz", true),
    ("kW", "kilowatt", "kilowatts", true),
    ("kWh", "kilowatt hour", "kilowatt hours", true),
    ("kb", "kilobit", "kilobits", true),
    ("kbit", "kilobit", "kilobits", true),
    ("kbps", "kilobit per second", "kilobits per second", true),
    ("kg", "kilogram", "kilograms", true),
    ("km", "kilometer", "kilometers", true),
    ("km²", "square kilometer", "square kilometers", true),
    ("kph", "kilometer per hour", "kilometers per hour", true),
    ("lb", "pound", "pounds", true),
    ("lbs", "pounds", "pounds", true),
    ("m", "meter", "meters", false),
    ("mAh", "milliamp hour", "milliamp hours", true),
    ("mL", "milliliter", "milliliters", true),
    ("mg", "milligram", "milligrams", true),
    ("mi", "mile", "miles", true),
    ("min", "minute", "minutes", true),
    ("mins", "minutes", "minutes", true),
    ("ml", "milliliter", "milliliters", true),
    ("mm", "millimeter", "millimeters", true),
    ("mpg", "mile per gallon", "miles per gallon", true),
    ("mph", "mile per hour", "miles per hour", true),
    ("ms", "millisecond", "milliseconds", true),
    ("m²", "square meter", "square meters", true),
    ("nm", "nanometer", "nanometers", true),
    ("oz", "ounce", "ounces", true),
    ("psi", "P S I", "P S I", true),
    ("px", "pixel", "pixels", true),
    ("rpm", "R P M", "R P M", true),
    ("sec", "second", "seconds", true),
    ("secs", "seconds", "seconds", true),
    ("tbsp", "tablespoon", "tablespoons", true),
    ("tsp", "teaspoon", "teaspoons", true),
    ("yd", "yard", "yards", true),
    ("yds", "yards", "yards", true),
    ("°", "degree", "degrees", true),
    ("°C", "degree Celsius", "degrees Celsius", true),
    ("°F", "degree Fahrenheit", "degrees Fahrenheit", true),
    ("µm", "micrometer", "micrometers", true),
    ("μm", "micrometer", "micrometers", true),
    // END SORTED
];

/// Denominators of `unit/unit` that are not units on their own (`km/h`).
static PER: &[(&str, &str)] = &[
    // BEGIN SORTED
    ("L", "liter"),
    ("d", "day"),
    ("day", "day"),
    ("h", "hour"),
    ("l", "liter"),
    ("mo", "month"),
    ("s", "second"),
    ("wk", "week"),
    ("yr", "year"),
    // END SORTED
];

/// A symbol standing alone as a word. Sorted by bytes.
static SYMBOLS: &[(&str, &str)] = &[
    // BEGIN SORTED
    ("#", "number"),
    ("$", "dollars"),
    ("%", "percent"),
    ("&", "and"),
    ("+", "plus"),
    ("/", "slash"),
    ("<", "less than"),
    ("=", "equals"),
    (">", "greater than"),
    ("@", "at"),
    ("~", "about"),
    ("£", "pounds"),
    ("¥", "yen"),
    ("§", "section"),
    ("§§", "sections"),
    ("°", "degrees"),
    ("±", "plus or minus"),
    ("×", "times"),
    ("÷", "divided by"),
    ("€", "euros"),
    ("₹", "rupees"),
    ("→", "to"),
    ("∆", "delta"),
    ("−", "minus"),
    ("√", "square root of"),
    ("≈", "approximately"),
    ("≠", "does not equal"),
    ("≤", "less than or equal to"),
    ("≥", "greater than or equal to"),
    // END SORTED
];

/// How a top-level domain is read. Only these make a word a domain. Sorted.
static TLDS: &[(&str, &str)] = &[
    // BEGIN SORTED
    ("ai", "A I"),
    ("app", "app"),
    ("au", "A U"),
    ("biz", "biz"),
    ("ca", "C A"),
    ("cn", "C N"),
    ("co", "co"),
    ("com", "com"),
    ("de", "D E"),
    ("dev", "dev"),
    ("edu", "E D U"),
    ("es", "E S"),
    ("eu", "E U"),
    ("fr", "F R"),
    ("gg", "G G"),
    ("gov", "gov"),
    ("ie", "I E"),
    ("in", "I N"),
    ("info", "info"),
    ("int", "int"),
    ("io", "I O"),
    ("jp", "J P"),
    ("ly", "L Y"),
    ("me", "me"),
    ("mil", "mil"),
    ("net", "net"),
    ("news", "news"),
    ("nl", "N L"),
    ("nz", "N Z"),
    ("org", "org"),
    ("ru", "R U"),
    ("tech", "tech"),
    ("tv", "T V"),
    ("uk", "U K"),
    ("us", "U S"),
    ("xyz", "X Y Z"),
    // END SORTED
];

/// File extensions read after a file name (`Node.js`, `data.json`). Sorted.
static FILE_EXTS: &[(&str, &str)] = &[
    // BEGIN SORTED
    ("csv", "C S V"),
    ("db", "D B"),
    ("dll", "D L L"),
    ("exe", "E X E"),
    ("html", "H T M L"),
    ("jpg", "J P G"),
    ("js", "J S"),
    ("json", "jason"),
    ("md", "M D"),
    ("pdf", "P D F"),
    ("png", "P N G"),
    ("py", "P Y"),
    ("rs", "R S"),
    ("sh", "S H"),
    ("ts", "T S"),
    ("txt", "T X T"),
    ("xml", "X M L"),
    ("yaml", "yammel"),
    ("zip", "zip"),
    // END SORTED
];

/// Roman numerals read as cardinals after these (`World War II`, `Chapter IV`).
static ROMAN_CARDINAL: &[&str] = &[
    // BEGIN SORTED
    "Act", "Article", "Book", "Chapter", "Class", "Part", "Phase", "Section", "Stage", "Title", "Type",
    "Vol.", "Volume", "War",
    // END SORTED
];

/// Roman numerals read as regnal ordinals after these (`Henry VIII` → the eighth).
static ROMAN_REGNAL: &[&str] = &[
    // BEGIN SORTED
    "Alexander",
    "Alfonso",
    "Benedict",
    "Catherine",
    "Charles",
    "Clement",
    "Edward",
    "Elizabeth",
    "Ferdinand",
    "Frederick",
    "George",
    "Gregory",
    "Henry",
    "Innocent",
    "James",
    "John",
    "King",
    "Leo",
    "Louis",
    "Mary",
    "Nicholas",
    "Paul",
    "Peter",
    "Philip",
    "Pius",
    "Pope",
    "Queen",
    "Richard",
    "William",
    // END SORTED
];

/// Capitalised imperatives that start recipe-style sentences (`Add 5 g salt`):
/// a single-letter unit after them is still a unit. Sorted.
static IMPERATIVES: &[&str] = &[
    // BEGIN SORTED
    "Add", "Cut", "Heat", "Mix", "Pour", "Take", "Use",
    // END SORTED
];

/// Units that are also words or letters (`one gal`, `Route 66 W`): they expand
/// only after digits, never after a number word. Single letters are always in
/// this class. Sorted.
static UNITS_DIGITS_ONLY: &[&str] = &[
    // BEGIN SORTED
    "gal", "ha", "in", "mi", "min", "s",
    // END SORTED
];

/// Valid numerals that are usually words or initialisms, never expanded.
static ROMAN_NEVER: &[&str] = &["CIV", "CLI", "DIV", "DIX", "MIX"];

/// Capitalised words that usually start a sentence. After an abbreviation's
/// period they mean the period also ended the sentence. Sorted.
static STARTERS: &[&str] = &[
    // BEGIN SORTED
    "A",
    "After",
    "An",
    "And",
    "As",
    "At",
    "Before",
    "Both",
    "But",
    "Each",
    "For",
    "He",
    "Her",
    "His",
    "However",
    "I",
    "If",
    "In",
    "It",
    "Its",
    "Many",
    "Meanwhile",
    "Most",
    "My",
    "On",
    "One",
    "Our",
    "She",
    "So",
    "Some",
    "That",
    "The",
    "Their",
    "Then",
    "There",
    "These",
    "They",
    "This",
    "Those",
    "To",
    "We",
    "When",
    "While",
    "Yet",
    // END SORTED
];

/// Number words that can precede a unit (`five km`).
static NUMBER_WORDS: &[&str] = &[
    // BEGIN SORTED
    "dozen", "eight", "eighty", "eleven", "few", "fifteen", "fifty", "five", "forty", "four",
    "hundred", "million", "nine", "ninety", "one", "seven", "seventy", "several", "six", "sixty",
    "ten", "thirty", "thousand", "three", "twelve", "twenty", "two",
    // END SORTED
];

static MONTHS: &[&str] = &[
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

const OPENERS: &[char] = &['(', '[', '{', '"', '\u{201C}', '\u{00AB}'];
const CLOSERS: &[char] = &[
    ',', ';', ':', '!', '?', ')', ']', '}', '"', '\u{201D}', '\u{00BB}',
];
const PAUSES: &[char] = &[',', ';', ':', '!', '?'];

fn find<'t, T>(table: &'t [(&'static str, T)], key: &str) -> Option<&'t T> {
    table
        .binary_search_by(|e| e.0.cmp(key))
        .ok()
        .and_then(|i| table.get(i))
        .map(|e| &e.1)
}

fn has(table: &[&str], key: &str) -> bool {
    table.binary_search(&key).is_ok()
}

fn abbrev(key: &str) -> Option<(&'static str, Rule)> {
    let i = ABBREVS.binary_search_by(|e| e.0.cmp(key)).ok()?;
    ABBREVS.get(i).map(|e| (e.1, e.2))
}

fn unit(key: &str) -> Option<(&'static str, &'static str, bool)> {
    let i = UNITS.binary_search_by(|e| e.0.cmp(key)).ok()?;
    UNITS.get(i).map(|e| (e.1, e.2, e.3))
}

/// A unit that may be glued to its number (`5km`, `3kWh`): (singular, plural).
/// The number pass (`ra_norm_num`) reads glued units with this table, so glued and
/// spaced units share one list.
pub fn glued_unit(key: &str) -> Option<(&'static str, &'static str)> {
    unit(key).filter(|u| u.2).map(|u| (u.0, u.1))
}

/// An abbreviation that never ends a sentence: a title (`Sen.`, `Gen.`), `Dr.`,
/// `St.`, `v.`, or one that needs a number after it (`Fig.`, `Vol.`). Used by
/// `ra_text::chunk`, so a chunk never ends on one and loses its expansion.
pub fn never_ends_sentence(word: &str) -> bool {
    abbrev(split(word).core)
        .is_some_and(|(_, r)| matches!(r, Rule::Title | Rule::Dr | Rule::St | Rule::Versus | Rule::Number))
}

// ---------------------------------------------------------------- words

/// A source word split into its core and the closing punctuation after it.
/// Opening brackets and quotes are dropped. The core keeps any `.`.
#[derive(Clone, Copy)]
struct Tok<'a> {
    core: &'a str,
    trail: &'a str,
}

fn split(word: &str) -> Tok<'_> {
    let start = word
        .char_indices()
        .find(|(_, c)| !OPENERS.contains(c))
        .map_or(word.len(), |(i, _)| i);
    let rest = word.get(start..).unwrap_or("");
    let end = rest
        .char_indices()
        .rev()
        .find(|(_, c)| !CLOSERS.contains(c))
        .map_or(0, |(i, c)| i + c.len_utf8());
    let mut core = rest.get(..end).unwrap_or("");
    // `°F).`, `GHz".`: closers before the sentence's period belong to the trail
    // (`finish` turns a trailing `.` back into the period).
    if let Some(body) = core.strip_suffix('.') {
        let trimmed = body.trim_end_matches(CLOSERS);
        if trimmed.len() < body.len() && !trimmed.is_empty() {
            core = trimmed;
        }
    }
    Tok { core, trail: rest.get(core.len()..).unwrap_or("") }
}

fn tok<'a>(w: &[&'a str], i: usize) -> Option<Tok<'a>> {
    w.get(i).map(|x| split(x))
}

/// `body` without one trailing `.`, and whether there was one.
fn strip_period(core: &str) -> (&str, bool) {
    match core.strip_suffix('.') {
        Some(b) => (b, true),
        None => (core, false),
    }
}

fn first_char(s: &str) -> Option<char> {
    s.chars().next()
}

fn is_cap(s: &str) -> bool {
    first_char(s).is_some_and(char::is_uppercase)
}

/// Capitalised word with lowercase rest (`Henry`, `Apollo`), a proper-noun shape.
fn is_proper(s: &str) -> bool {
    let mut cs = s.chars();
    cs.next().is_some_and(|c| c.is_ascii_uppercase())
        && s.len() >= 2
        && cs.all(|c| c.is_ascii_lowercase())
}

fn next_core<'a>(w: &[&'a str], i: usize) -> Option<&'a str> {
    w.get(i + 1).map(|x| split(x).core)
}

fn next_is_cap(w: &[&str], i: usize) -> bool {
    next_core(w, i).is_some_and(is_cap)
}

fn next_is_starter(w: &[&str], i: usize) -> bool {
    next_core(w, i).is_some_and(|c| has(STARTERS, c))
}

fn next_is_number(w: &[&str], i: usize) -> bool {
    next_core(w, i).is_some_and(|c| {
        let c = c.strip_prefix('#').unwrap_or(c);
        first_char(c).is_some_and(|x| x.is_ascii_digit())
    })
}

/// The word after `i` ends the sentence: none, or a usual sentence starter.
fn sentence_end(w: &[&str], i: usize) -> bool {
    i + 1 >= w.len() || next_is_starter(w, i)
}

/// The word before `i` is a number or ends in a digit, with no pause after it.
fn prev_is_number(w: &[&str], i: usize) -> bool {
    let Some(p) = i.checked_sub(1).and_then(|j| tok(w, j)) else {
        return false;
    };
    if !p.trail.is_empty() {
        return false;
    }
    p.core.chars().last().is_some_and(|c| c.is_ascii_digit()) || has(NUMBER_WORDS, p.core)
}

/// The word before `i` looks like a street name (`Main`, `5th`), no pause after it.
fn prev_is_street_name(w: &[&str], i: usize) -> bool {
    let Some(p) = i.checked_sub(1).and_then(|j| tok(w, j)) else {
        return false;
    };
    p.trail.is_empty()
        && !has(STARTERS, p.core)
        && (is_proper(p.core) || first_char(p.core).is_some_and(|c| c.is_ascii_digit()))
}

/// Appends the pause of the source word (`,;:!?`) or, failing that, a period.
fn finish(mut spoken: String, trail: &str, period: bool) -> String {
    let pause: String = trail.chars().filter(|c| PAUSES.contains(c)).collect();
    if !pause.is_empty() {
        spoken.push_str(&pause);
    } else if period || trail.ends_with('.') {
        spoken.push('.');
    }
    spoken
}

// ---------------------------------------------------------------- number words

use crate::ra_numwords::{cardinal, digits as digit_words, ordinal};

/// Words for a plain written number: `5`, `1,000`, `2.5`, `.5`. `None` otherwise
/// (stray or doubled commas, a bare `.`, a non-digit fraction). The words come
/// from `ra_numwords`; numbers too long for it are read digit by digit.
fn number_words(s: &str) -> Option<String> {
    let (int, frac) = match s.split_once('.') {
        Some((a, b)) => (a, Some(b)),
        None => (s, None),
    };
    if int.is_empty() && frac.is_none_or(str::is_empty) {
        return None;
    }
    if int.starts_with(',') || int.ends_with(',') || int.contains(",,") {
        return None;
    }
    if !int.chars().all(|c| c.is_ascii_digit() || c == ',') {
        return None;
    }
    match frac {
        Some(f) if f.is_empty() || !f.chars().all(|c| c.is_ascii_digit()) => None,
        Some(f) => Some(crate::ra_numwords::decimal(int, f)),
        None => Some(crate::ra_numwords::integer(int)),
    }
}

// ---------------------------------------------------------------- pieces

fn has_vowel(s: &str) -> bool {
    s.chars()
        .any(|c| matches!(c.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u' | 'y'))
}

fn spell(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphabetic)
        .map(|c| c.to_ascii_uppercase().to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

/// One alphabetic piece: short or vowel-less caps are spelled (`BLM` → "B L M"),
/// longer caps are kept (`NASA`), anything else is lowercased.
fn caps_piece(p: &str) -> String {
    let caps = p.chars().all(|c| c.is_ascii_uppercase());
    if caps && (p.len() <= 3 || !has_vowel(p)) {
        spell(p)
    } else if caps {
        p.to_string()
    } else {
        p.to_ascii_lowercase()
    }
}

/// Splits an ASCII word on case and digit boundaries (`MacronLeaks` → macron
/// leaks, `JPMorgan` → J P morgan, `user42` → user forty two).
fn camel_words(s: &str) -> String {
    let cs: Vec<char> = s.chars().collect();
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    for (k, &c) in cs.iter().enumerate() {
        let prev = k.checked_sub(1).and_then(|j| cs.get(j)).copied();
        let next = cs.get(k + 1).copied();
        let boundary = match prev {
            None => false,
            Some(p) => {
                (p.is_ascii_lowercase() && c.is_ascii_uppercase())
                    || (p.is_ascii_uppercase()
                        && c.is_ascii_uppercase()
                        && next.is_some_and(|n| n.is_ascii_lowercase()))
                    || (p.is_ascii_alphabetic() && c.is_ascii_digit())
                    || (p.is_ascii_digit() && c.is_ascii_alphabetic())
            }
        };
        if boundary && !cur.is_empty() {
            parts.push(std::mem::take(&mut cur));
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    parts
        .iter()
        .map(|p| {
            if p.chars().all(|c| c.is_ascii_digit()) {
                if p.len() <= 2 {
                    number_words(p).unwrap_or_else(|| digit_words(p))
                } else {
                    digit_words(p)
                }
            } else {
                caps_piece(p)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_ascii_word(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Reads ASCII text whose pieces are separated by `.` `-` `_` `+`, naming each
/// separator. Pieces with letters are case-split. `None` on anything else.
fn separated_words(s: &str, label: fn(&str) -> String) -> Option<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur_start = 0usize;
    for (k, c) in s.char_indices() {
        let sep = match c {
            '.' => "dot",
            '-' => "dash",
            '_' => "underscore",
            '+' => "plus",
            _ => continue,
        };
        let piece = s.get(cur_start..k)?;
        if !is_ascii_word(piece) {
            return None;
        }
        out.push(label(piece));
        out.push(sep.to_string());
        cur_start = k + c.len_utf8();
    }
    let piece = s.get(cur_start..)?;
    if !is_ascii_word(piece) {
        return None;
    }
    out.push(label(piece));
    Some(out.join(" "))
}

/// A domain label: vowel-less lowercase short labels are spelled (`bbc`, `cnn`).
fn label_words(p: &str) -> String {
    if p.len() <= 5 && p.chars().all(|c| c.is_ascii_lowercase()) && !has_vowel(p) {
        spell(p)
    } else {
        camel_words(p)
    }
}

/// `amazon.com` → "amazon dot com". Needs a known TLD and a letter up front.
fn domain_words(s: &str) -> Option<String> {
    let (host, tld) = s.rsplit_once('.')?;
    let tld_said = find(TLDS, &tld.to_ascii_lowercase())?;
    if host.is_empty() || !host.chars().any(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let mut out = String::new();
    for label in host.split('.') {
        if label.is_empty() || !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return None;
        }
        let words = separated_words(label, label_words)?;
        out.push_str(&words);
        out.push_str(" dot ");
    }
    out.push_str(tld_said);
    Some(out)
}

/// `Node.js` → "node dot J S", `data.json` → "data dot jason": one name label
/// and a known file extension (lowercase, as file names are written).
fn file_words(s: &str) -> Option<String> {
    let (name, ext) = s.rsplit_once('.')?;
    let ext_said = find(FILE_EXTS, ext)?;
    if name.is_empty()
        || !name.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    let words = separated_words(name, label_words)?;
    Some(format!("{words} dot {ext_said}"))
}

/// `https://www.example.org/path?q` → "example dot org slash path". Also a bare
/// `example.org/path`. The scheme, `www.`, query and fragment are not read.
fn url_words(s: &str) -> Option<String> {
    let lower = s.to_ascii_lowercase();
    let mut rest = s;
    let mut schemed = false;
    for scheme in ["https://", "http://", "ftp://"] {
        if lower.starts_with(scheme) {
            rest = s.get(scheme.len()..)?;
            schemed = true;
            break;
        }
    }
    let mut www = false;
    if rest
        .get(..4)
        .is_some_and(|p| p.eq_ignore_ascii_case("www."))
    {
        rest = rest.get(4..)?;
        www = true;
    }
    if !schemed && !www && !rest.contains('/') {
        return None;
    }
    let cut = rest.find(['?', '#']).unwrap_or(rest.len());
    let rest = rest.get(..cut)?.trim_end_matches('/');
    let (host, path) = match rest.split_once('/') {
        Some((h, p)) => (h, Some(p)),
        None => (rest, None),
    };
    let mut out = domain_words(host)?;
    if let Some(path) = path {
        for seg in path.split('/').filter(|x| !x.is_empty()) {
            out.push_str(" slash ");
            out.push_str(&separated_words(seg, camel_words)?);
        }
    }
    Some(out)
}

/// `press@example.org` → "press at example dot org".
fn email_words(s: &str) -> Option<String> {
    let (local, domain) = s.split_once('@')?;
    if local.is_empty() || domain.contains('@') {
        return None;
    }
    let local = separated_words(local, camel_words)?;
    Some(format!("{local} at {}", domain_words(domain)?))
}

/// `@ElonMusk` → "at elon musk", `#MacronLeaks` → "hashtag macron leaks",
/// `#1` → "number one".
fn tag_words(s: &str) -> Option<String> {
    let (lead, rest) = if let Some(r) = s.strip_prefix('@') {
        ("at", r)
    } else if let Some(r) = s.strip_prefix('#') {
        ("hashtag", r)
    } else {
        return None;
    };
    if lead == "hashtag" && !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
        return Some(format!("number {}", number_words(rest)?));
    }
    if !rest.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let mut parts = Vec::new();
    for piece in rest.split('_') {
        if !piece.is_empty() {
            if !is_ascii_word(piece) {
                return None;
            }
            parts.push(camel_words(piece));
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(format!("{lead} {}", parts.join(" underscore ")))
}

/// `U.S.` → "U S", `Ph.D.` → "P H D", with a `'s` kept (`U S's`).
/// Returns the spoken letters and whether the core ended in the period.
fn initialism(core: &str) -> Option<(String, bool)> {
    let (body, suffix) = if let Some(b) = core.strip_suffix("'s") {
        (b, "'s")
    } else {
        (core, "")
    };
    let (body, period) = strip_period(body);
    if !body.contains('.') {
        return None;
    }
    let segs: Vec<&str> = body.split('.').collect();
    let ok = segs.len() >= 2
        && segs
            .iter()
            .all(|g| (1..=2).contains(&g.len()) && g.chars().all(|c| c.is_ascii_alphabetic()))
        && segs.iter().any(|g| g.len() == 1);
    if !ok {
        return None;
    }
    Some((format!("{}{suffix}", spell(body)), period))
}

// ---------------------------------------------------------------- roman

fn roman_value(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > 15 {
        return None;
    }
    let val = |c: char| match c {
        'I' => Some(1),
        'V' => Some(5),
        'X' => Some(10),
        'L' => Some(50),
        'C' => Some(100),
        'D' => Some(500),
        'M' => Some(1000),
        _ => None,
    };
    let vs: Vec<u32> = s.chars().map(val).collect::<Option<_>>()?;
    let mut total = 0i64;
    for (k, &v) in vs.iter().enumerate() {
        if vs.get(k + 1).is_some_and(|&n| n > v) {
            total -= i64::from(v);
        } else {
            total += i64::from(v);
        }
    }
    let total = u32::try_from(total).ok()?;
    // Canonical form only (rejects IIII, VX, IL ...).
    (1..=3999)
        .contains(&total)
        .then_some(total)
        .filter(|&t| to_roman(t) == s)
}

fn to_roman(mut n: u32) -> String {
    const T: [(u32, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut s = String::new();
    for (v, r) in T {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

/// A Roman numeral in context: after a trigger word (`War II`, `Henry VIII`,
/// `Super Bowl LVIII`), or 4+ letters after a capitalised name (`Apollo XIII`).
fn roman(w: &[&str], i: usize, numeral: &str) -> Option<String> {
    if has(ROMAN_NEVER, numeral) {
        return None;
    }
    let value = roman_value(numeral)?;
    let prev = i.checked_sub(1).and_then(|j| tok(w, j))?;
    if !prev.trail.is_empty() {
        return None;
    }
    let single = numeral.len() == 1;
    let super_bowl = prev.core == "Bowl"
        && i.checked_sub(2)
            .and_then(|j| tok(w, j))
            .is_some_and(|t| t.core == "Super");
    // `CHAPTER XV`, `class II`: triggers match in any case
    let mut title = String::new();
    for (k, c) in prev.core.chars().enumerate() {
        if k == 0 {
            title.extend(c.to_uppercase());
        } else {
            title.extend(c.to_lowercase());
        }
    }
    if super_bowl || has(ROMAN_CARDINAL, &title) {
        // Single letters: only I, V and X (never `Type C`, `Class D`).
        if single && !matches!(numeral, "I" | "V" | "X") {
            return None;
        }
        // A lone I is usually the pronoun (`the Cold War I moved`, `Part I think`).
        // Read it only after World/Great War or when it ends the clause.
        if numeral == "I" {
            let named_war = prev.core == "War"
                && i.checked_sub(2)
                    .and_then(|j| tok(w, j))
                    .is_some_and(|t| t.core == "World" || t.core == "Great");
            let me = tok(w, i);
            let clause_end = i + 1 >= w.len()
                || me.is_some_and(|t| !t.trail.is_empty() || t.core.ends_with('.'))
                || (next_is_cap(w, i) && !next_is_starter(w, i));
            if !named_war && !clause_end {
                return None;
            }
        }
        return Some(cardinal(value.into()));
    }
    // `Apple I,` / `Henry I.`: a lone I after a name that ends the clause is a
    // numeral (the pronoun almost never ends a clause straight after a name)
    let i_clause_end = numeral == "I"
        && tok(w, i).is_some_and(|t| !t.trail.is_empty() || t.core.ends_with('.'))
        && is_proper(prev.core)
        && !has(STARTERS, prev.core);
    if has(ROMAN_REGNAL, prev.core) {
        // `Henry I think`: a lone I after a name is the pronoun.
        if single && !matches!(numeral, "V" | "X") && !i_clause_end {
            return None;
        }
        return Some(format!("the {}", ordinal(value.into())));
    }
    if i_clause_end {
        return Some("one".into());
    }
    // `Apollo XIII`, and `Apple II` / `Artemis III` (II and III are never words)
    let long_or_ii = numeral.len() >= 4 || matches!(numeral, "II" | "III");
    if long_or_ii && is_proper(prev.core) && !has(STARTERS, prev.core) {
        return Some(cardinal(value.into()));
    }
    None
}

// ---------------------------------------------------------------- symbols, units

/// `km/h` → kilometers per hour, `and/or` → "and or", `P/E` → "P E".
fn slash_words(s: &str) -> Option<String> {
    match s {
        "w/" | "W/" => return Some("with".into()),
        "w/o" | "W/O" => return Some("without".into()),
        "c/o" => return Some("care of".into()),
        "n/a" | "N/A" => return Some("not applicable".into()),
        _ => {}
    }
    let (a, b) = s.split_once('/')?;
    if matches!(b, "c2" | "c²") && is_ascii_word(a) {
        return Some(format!("{a} per c squared"));
    }
    if !is_ascii_word(a) || !is_ascii_word(b) {
        return None;
    }
    if a.chars().chain(b.chars()).any(|c| c.is_ascii_digit()) {
        return None;
    }
    if let Some((_, many, _)) = unit(a) {
        let per = unit(b).map(|u| u.0).or_else(|| find(PER, b).copied());
        if let Some(per) = per {
            return Some(format!("{many} per {per}"));
        }
    }
    Some(format!("{} {}", caps_piece(a), caps_piece(b)))
}

/// Math symbols that are read when glued to a letter word.
fn glued_math(c: char) -> bool {
    matches!(c, '=' | '×' | '÷' | '<' | '>' | '≤' | '≥' | '≈' | '≠' | '±' | '+' | '∆')
}

/// Letters and glued math symbols, no digits: each symbol said by its word, a
/// lone letter said by its name (`a>` → "A greater than"), a unit after a number
/// said as the unit (`8.34 mm×` → "millimeters times").
fn glued_symbols(w: &[&str], i: usize, body: &str) -> Option<String> {
    if !body.chars().any(glued_math) || !body.chars().any(char::is_alphabetic) {
        return None;
    }
    if !body.chars().all(|c| c.is_alphabetic() || glued_math(c)) {
        return None;
    }
    let mut out: Vec<String> = Vec::new();
    let mut letters = String::new();
    let flush = |letters: &mut String, out: &mut Vec<String>| {
        if letters.is_empty() {
            return;
        }
        let l = std::mem::take(letters);
        let said = match unit(&l) {
            Some((one_u, many, _)) if out.is_empty() && prev_is_number(w, i) => {
                let singular = i.checked_sub(1).and_then(|j| tok(w, j)).is_some_and(|p| p.core == "1");
                (if singular { one_u } else { many }).to_string()
            }
            _ if l.chars().count() == 1 => l.to_uppercase(),
            _ => l,
        };
        out.push(said);
    };
    for c in body.chars() {
        if glued_math(c) {
            flush(&mut letters, &mut out);
            let mut buf = [0u8; 4];
            out.push(find(SYMBOLS, c.encode_utf8(&mut buf))?.to_string());
        } else {
            letters.push(c);
        }
    }
    flush(&mut letters, &mut out);
    Some(out.join(" "))
}

/// `AT&T` → "A T and T", `R&D` → "R and D".
fn ampersand_words(s: &str) -> Option<String> {
    if !s.contains('&') {
        return None;
    }
    let mut parts = Vec::new();
    for p in s.split('&') {
        if !is_ascii_word(p) {
            return None;
        }
        parts.push(caps_piece(p));
    }
    Some(parts.join(" and "))
}

/// `approach...we` → "approach, we".
fn ellipsis_words(s: &str) -> Option<String> {
    let (k, len) = match (s.find("..."), s.find('…')) {
        (Some(a), _) => (a, 3),
        (None, Some(b)) => (b, '…'.len_utf8()),
        _ => return None,
    };
    let left = s.get(..k)?;
    let right = s.get(k + len..)?.trim_start_matches(['.', '…']);
    let lettered = |x: &str| x.chars().any(char::is_alphabetic);
    if !lettered(left) || !lettered(right) {
        return None;
    }
    Some(format!("{left}, {right}"))
}

// ---------------------------------------------------------------- the scan

/// The span starting at word `i`, if any: (end, spoken).
fn at(w: &[&str], i: usize) -> Option<(usize, String)> {
    let t = tok(w, i)?;
    let core = t.core;
    if core.is_empty() {
        return None;
    }
    let one = |spoken: String, period: bool| Some((i + 1, finish(spoken, t.trail, period)));

    // Two words: `et al.`
    if core == "et" {
        if let Some(n) = tok(w, i + 1) {
            if n.core == "al." || n.core == "al" {
                let period = n.core.ends_with('.') && sentence_end(w, i + 1);
                return Some((i + 2, finish("et al".into(), n.trail, period)));
            }
        }
    }

    // A symbol on its own.
    if let Some(said) = find(SYMBOLS, core) {
        return one((*said).to_string(), false);
    }
    // `3 x 4`
    if core == "x" && prev_is_number(w, i) && next_is_number(w, i) {
        return one("times".into(), false);
    }

    // A math symbol glued to a letter word: `x×`, `y=`, `b≥`, `was≈`, `∆v`.
    if let Some(said) = glued_symbols(w, i, strip_period(core).0) {
        return one(said, strip_period(core).1 && sentence_end(w, i));
    }

    // Abbreviations (the period is part of the abbreviation).
    // `Sec. Blinken` is the Secretary (`Sec. 12` stays a section); `est. 1967` is
    // "established" before a year, "estimated" otherwise.
    if core == "Sec." && next_is_cap(w, i) && !next_is_starter(w, i) {
        return one("secretary".into(), false);
    }
    if core == "est." {
        let year_next = next_core(w, i).is_some_and(|c| {
            c.len() == 4 && c.bytes().all(|b| b.is_ascii_digit()) && matches!(c.as_bytes()[0], b'1' | b'2')
        });
        if year_next {
            return one("established".into(), false);
        }
    }
    if let Some((said, rule)) = abbrev(core) {
        let said = match rule {
            Rule::Always => Some(said),
            Rule::Title => (next_is_cap(w, i) && !next_is_starter(w, i)).then_some(said),
            Rule::Number => (next_is_number(w, i)
                || next_core(w, i)
                    .map(|c| strip_period(c).0)
                    .is_some_and(|c| c.len() >= 2 && roman_value(c).is_some()))
            .then_some(said),
            Rule::Digit => next_is_number(w, i).then_some(said),
            Rule::DateNext => (next_is_number(w, i)
                || next_core(w, i).is_some_and(|c| MONTHS.contains(&c)))
            .then_some(said),
            Rule::Versus => {
                (i > 0 && tok(w, i - 1).is_some_and(|p| is_cap(p.core)) && next_is_cap(w, i))
                    .then_some(said)
            }
            Rule::Dr | Rule::St => {
                let name_follows = next_is_cap(w, i) && !next_is_starter(w, i);
                if prev_is_street_name(w, i) && !name_follows {
                    Some(if rule == Rule::Dr { "drive" } else { "street" })
                } else {
                    Some(said)
                }
            }
        };
        if let Some(said) = said {
            return one(said.to_string(), sentence_end(w, i));
        }
    }

    // Dotted initialisms, also glued to a hyphenated word (`U.S.-China`).
    if let Some((said, period)) = initialism(core) {
        // Period only at a sentence end: `U.S. Senate`, `J.K. Rowling` must not
        // pause; a missed pause in `the U.S. Trump said` is the accepted cost.
        return one(said, period && sentence_end(w, i));
    }
    if let Some((left, right)) = core.split_once('-') {
        if let Some((said, _)) = initialism(left) {
            if right.chars().any(char::is_alphabetic) {
                return one(format!("{said} {}", right.replace('-', " ")), false);
            }
        }
    }

    // Everything below treats a final `.` as the sentence's own period.
    let (body, period) = strip_period(core);
    if body.is_empty() {
        return None;
    }

    if let Some(said) = tag_words(body)
        .or_else(|| {
            if body.contains('@') {
                email_words(body)
            } else {
                None
            }
        })
        .or_else(|| url_words(body))
        .or_else(|| {
            if body.contains('.') {
                domain_words(body).or_else(|| file_words(body)).or_else(|| {
                    // `Amazon.com's`
                    let stem = body.strip_suffix("'s")?;
                    Some(format!("{}'s", domain_words(stem)?))
                })
            } else {
                None
            }
        })
    {
        return one(said, period);
    }

    if let Some(said) = ellipsis_words(core) {
        return one(said, false);
    }
    if let Some(said) = ampersand_words(body) {
        return one(said, period);
    }
    if body.contains('/') {
        if let Some(said) = slash_words(body) {
            return one(said, period);
        }
    }

    // `per km²` -> "per square kilometer" (never a single letter or a word-like unit)
    let after_per = i
        .checked_sub(1)
        .and_then(|j| tok(w, j))
        .is_some_and(|p| p.core.eq_ignore_ascii_case("per") && p.trail.is_empty());
    if after_per && body.chars().count() > 1 && !has(UNITS_DIGITS_ONLY, body) {
        if let Some((one_u, _, _)) = unit(body) {
            return one(one_u.to_string(), period);
        }
    }

    // Units after a number, including `sq ft`.
    if prev_is_number(w, i) {
        if body == "sq" || body == "sq." {
            if let Some(n) = tok(w, i + 1) {
                let (nb, np) = strip_period(n.core);
                if let Some((_, many, _)) = unit(nb) {
                    return Some((i + 2, finish(format!("square {many}"), n.trail, np)));
                }
            }
        }
        let prev = tok(w, i - 1);
        let prev_money = prev.is_some_and(|p| p.core.contains(['$', '€', '£', '¥', '₹']));
        let prev_digits =
            prev.is_some_and(|p| p.core.chars().last().is_some_and(|c| c.is_ascii_digit()));
        let single = body.chars().count() == 1;
        let ambiguous = single || has(UNITS_DIGITS_ONLY, body);
        // `take Route 66 W`: a single letter after a number that follows a name is
        // a direction or a label, not a unit. A capital that is the chunk's first
        // word or an imperative (`Add 5 g salt`) is not a name.
        let after_name = i.checked_sub(2).is_some_and(|j| {
            j > 0
                && tok(w, j).is_some_and(|p| {
                    is_cap(p.core) && p.trail.is_empty() && !has(IMPERATIVES, p.core)
                })
        });
        let ok = !(ambiguous && !prev_digits)
            && !(single && after_name)
            && !has(ROMAN_NEVER, body)
            && !(body == "m" && prev_money);
        if let Some((one_u, many, _)) = unit(body).filter(|_| ok) {
            let singular = prev.is_some_and(|p| p.core == "1" || p.core == "one");
            return one((if singular { one_u } else { many }).to_string(), period);
        }
    }
    // A symbol or unit glued to a number (`±2`, `§230`, `5km`, `3×4`) is a number
    // span: the number pass reads it, and numbers win the merge (`ra_norm`).

    if let Some(said) = roman(w, i, body) {
        return one(said, period);
    }
    None
}

/// Non-number spans for one chunk of source words: `(start, end, spoken)`,
/// end exclusive, sorted and non-overlapping. Curly apostrophes and Unicode
/// hyphens are normalised first (`ra_norm::prepare`).
pub fn text_spans(words: &[&str]) -> Vec<(usize, usize, String)> {
    let prepared: Vec<Cow<'_, str>> = words.iter().map(|x| crate::ra_norm::prepare(x)).collect();
    let w: Vec<&str> = prepared.iter().map(|c| c.as_ref()).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < w.len() {
        match at(&w, i) {
            Some((end, spoken)) if end > i && end <= w.len() && !spoken.trim().is_empty() => {
                out.push((i, end, spoken));
                i = end;
            }
            _ => i += 1,
        }
    }
    out
}

#[cfg(test)]
fn check_tables_sorted() {
    fn sorted<T>(name: &str, keys: impl Iterator<Item = T>)
    where
        T: Ord + std::fmt::Debug,
    {
        let v: Vec<T> = keys.collect();
        for pair in v.windows(2) {
            assert!(
                pair[0] < pair[1],
                "{name} not sorted/unique at {:?} / {:?}",
                pair[0],
                pair[1]
            );
        }
    }
    sorted("ABBREVS", ABBREVS.iter().map(|e| e.0));
    sorted("UNITS", UNITS.iter().map(|e| e.0));
    sorted("PER", PER.iter().map(|e| e.0));
    sorted("SYMBOLS", SYMBOLS.iter().map(|e| e.0));
    sorted("TLDS", TLDS.iter().map(|e| e.0));
    sorted("FILE_EXTS", FILE_EXTS.iter().map(|e| e.0));
    sorted("ROMAN_CARDINAL", ROMAN_CARDINAL.iter());
    sorted("ROMAN_REGNAL", ROMAN_REGNAL.iter());
    sorted("ROMAN_NEVER", ROMAN_NEVER.iter());
    sorted("STARTERS", STARTERS.iter());
    sorted("IMPERATIVES", IMPERATIVES.iter());
    sorted("UNITS_DIGITS_ONLY", UNITS_DIGITS_ONLY.iter());
    sorted("NUMBER_WORDS", NUMBER_WORDS.iter());
    assert!(ABBREVS.len() >= 80, "{} abbreviations", ABBREVS.len());
    assert!(UNITS.len() >= 60, "{} units", UNITS.len());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sp(s: &str) -> Vec<(usize, usize, String)> {
        let w: Vec<&str> = s.split_whitespace().collect();
        text_spans(&w)
    }

    /// Source words with each span replaced by its spoken text.
    fn render(s: &str) -> String {
        let w: Vec<&str> = s.split_whitespace().collect();
        let spans = text_spans(&w);
        let mut out: Vec<String> = Vec::new();
        let mut i = 0;
        let mut k = 0;
        while i < w.len() {
            if k < spans.len() && spans[k].0 == i {
                out.push(spans[k].2.clone());
                i = spans[k].1;
                k += 1;
            } else {
                out.push(w[i].to_string());
                i += 1;
            }
        }
        out.join(" ")
    }

    fn check_invariants(words: &[&str]) {
        let spans = text_spans(words);
        let mut last_end = 0;
        for (s, e, spoken) in &spans {
            assert!(s < e, "empty range {s}..{e} in {words:?}");
            assert!(*e <= words.len(), "range past end in {words:?}");
            assert!(*s >= last_end, "overlap/unsorted in {words:?}");
            assert!(!spoken.trim().is_empty(), "empty spoken in {words:?}");
            last_end = *e;
        }
    }

    // ---- §3: initialisms ----
    #[test]
    fn initialisms() {
        assert_eq!(render("the U.S. economy"), "the U S economy");
        assert_eq!(render("e.g. this"), "for example this");
        assert_eq!(render("i.e. that"), "that is that");
        assert_eq!(render("a Ph.D. student"), "a P H D student");
        assert_eq!(render("an M.B.A. holder"), "an M B A holder");
        assert_eq!(render("the U.S.'s debt"), "the U S's debt");
        assert_eq!(render("the U.S., and"), "the U S, and");
    }

    // Review Focus 4: the sentence pause survives.
    #[test]
    fn initialism_at_sentence_end_keeps_period() {
        assert_eq!(render("he moved to the U.S."), "he moved to the U S.");
        assert_eq!(render("in the U.S. The rest"), "in the U S. The rest");
    }

    // ---- §3: abbreviations ----
    #[test]
    fn abbreviations() {
        assert_eq!(render("Gen. Smith"), "general Smith");
        assert_eq!(render("Sen. Warren"), "senator Warren");
        assert_eq!(render("Kim vs. Lee"), "Kim versus Lee");
        assert_eq!(render("approx. 5 people"), "approximately 5 people");
        assert_eq!(render("in St. Louis"), "in saint Louis");
        assert_eq!(render("St. Louis"), "saint Louis");
        assert_eq!(render("on Main St."), "on Main street.");
        assert_eq!(render("on Main St. in town"), "on Main street in town");
        assert_eq!(render("Apple Inc. said"), "Apple incorporated said");
        assert_eq!(render("Lt. Col. Smith"), "lieutenant colonel Smith");
        assert_eq!(render("Roe v. Wade"), "Roe versus Wade");
    }

    #[test]
    fn title_at_sentence_end_is_not_expanded() {
        assert_eq!(render("he met the Sen."), "he met the Sen.");
        assert_eq!(render("the Sen. said so"), "the Sen. said so");
    }

    #[test]
    fn number_abbreviation_needs_a_number() {
        assert_eq!(render("No. 5"), "number 5");
        assert_eq!(render("he said no."), "he said no.");
        assert_eq!(render("No. I won't"), "No. I won't");
    }

    #[test]
    fn doctor_vs_drive() {
        assert_eq!(render("Dr. Smith"), "doctor Smith");
        assert_eq!(render("with Dr. Smith"), "with doctor Smith");
        assert_eq!(render("on Elm Dr."), "on Elm drive.");
        assert_eq!(render("Elm Dr. is closed"), "Elm drive is closed");
    }

    // ---- §3: domains, URLs, emails, handles, hashtags ----
    #[test]
    fn domain() {
        assert_eq!(render("amazon.com"), "amazon dot com");
        assert_eq!(
            render("visit Amazon.com today"),
            "visit amazon dot com today"
        );
        assert_eq!(render("see bbc.co.uk."), "see B B C dot co dot U K.");
    }

    #[test]
    fn url() {
        assert_eq!(
            render("https://www.example.org/path"),
            "example dot org slash path"
        );
        assert_eq!(render("http://example.com/"), "example dot com");
        assert_eq!(
            render("www.example.com/a-b?x=1"),
            "example dot com slash a dash b"
        );
    }

    #[test]
    fn email() {
        assert_eq!(render("press@example.org"), "press at example dot org");
        assert_eq!(
            render("john.doe@example.com"),
            "john dot doe at example dot com"
        );
    }

    #[test]
    fn handle() {
        assert_eq!(render("@ElonMusk"), "at elon musk");
        // Controller ruling: a lowercase blob has no case boundary, so text_spans
        // does NOT meet the §3 "at elon musk" row here. Dictionary segmentation
        // ("elonmusk" -> "elon musk") happens in ra_names' NameFallback (Task 3).
        assert_eq!(sp("@elonmusk"), vec![(0, 1, "at elonmusk".to_string())]);
        assert_eq!(render("@john_doe"), "at john underscore doe");
    }

    #[test]
    fn hashtag() {
        assert_eq!(render("#MacronLeaks"), "hashtag macron leaks");
        assert_eq!(render("#BLM"), "hashtag B L M");
    }

    // ---- §3: Roman numerals ----
    #[test]
    fn roman_numerals() {
        assert_eq!(render("World War II"), "World War two");
        assert_eq!(render("World War I ended"), "World War one ended");
        assert_eq!(render("Henry VIII"), "Henry the eighth");
        assert_eq!(render("Louis XIV"), "Louis the fourteenth");
        assert_eq!(render("Pope Leo XIV"), "Pope Leo the fourteenth");
        assert_eq!(render("Super Bowl LVIII"), "Super Bowl fifty eight");
        assert_eq!(render("Chapter IV"), "Chapter four");
        assert_eq!(render("Vol. III"), "volume three");
        assert_eq!(render("Apollo XIII flew"), "Apollo thirteen flew");
        assert_eq!(render("in World War II."), "in World War two.");
    }

    #[test]
    fn roman_false_positives() {
        assert_eq!(render("I read it"), "I read it");
        assert_eq!(render("so said I"), "so said I");
        assert_eq!(render("Henry I think"), "Henry I think");
        assert_eq!(render("the MIX tape"), "the MIX tape");
        assert_eq!(render("Henry MIX"), "Henry MIX");
        assert_eq!(render("the CIVIL WAR"), "the CIVIL WAR");
        assert_eq!(render("Chapter CIVIL"), "Chapter CIVIL");
        assert_eq!(render("Apollo XI"), "Apollo XI");
        assert_eq!(render("Washington DC"), "Washington DC");
    }

    // ---- §3: units ----
    #[test]
    fn units_after_a_number() {
        assert_eq!(render("5 km"), "5 kilometers");
        assert_eq!(render("1 km"), "1 kilometer");
        assert_eq!(render("8 TB"), "8 terabytes");
        assert_eq!(render("3 GHz"), "3 gigahertz");
        // glued units belong to the number pass (ra_norm::glued_and_spaced_units_read_alike)
        assert_eq!(render("20°C"), "20°C");
        assert_eq!(render("2.5GHz chip"), "2.5GHz chip");
        assert_eq!(render("10 sq ft"), "10 square feet");
        assert_eq!(render("five km away"), "five kilometers away");
    }

    #[test]
    fn units_need_a_number() {
        assert_eq!(render("the km sign"), "the km sign");
        assert_eq!(render("5 in the box"), "5 in the box");
        assert_eq!(render("a 5G phone"), "a 5G phone");
    }

    // ---- §3: math ----
    #[test]
    fn math() {
        assert_eq!(render("a = b"), "a equals b");
        assert_eq!(render("3 × 4"), "3 times 4");
        // a sign glued to a number belongs to the number pass (ra_norm tests)
        assert_eq!(render("±2"), "±2");
        assert_eq!(render("x ≈ 5"), "x approximately 5");
        assert_eq!(render("6 ÷ 2"), "6 divided by 2");
        assert_eq!(render("§ 230"), "section 230");
        assert_eq!(render("a < b"), "a less than b");
    }

    #[test]
    fn glued_ellipsis_splits_with_a_pause() {
        assert_eq!(render("approach...we"), "approach, we");
        assert_eq!(render("wait…what"), "wait, what");
        assert_eq!(render("and then..."), "and then...");
    }

    #[test]
    fn slashes() {
        assert_eq!(render("and/or"), "and or");
        assert_eq!(render("P/E"), "P E");
        assert_eq!(render("km/h"), "kilometers per hour");
        assert_eq!(render("w/o sugar"), "without sugar");
    }

    #[test]
    fn symbols() {
        assert_eq!(render("A & B"), "A and B");
        assert_eq!(render("+"), "plus");
        assert_eq!(render("%"), "percent");
        assert_eq!(render("$"), "dollars");
        assert_eq!(render("AT&T"), "A T and T");
        assert_eq!(render("R&D spend"), "R and D spend");
        assert!(sp(".").is_empty());
        assert!(sp("…").is_empty());
        assert!(sp("—").is_empty());
    }

    #[test]
    fn curly_apostrophe_through_prepare() {
        // ra_norm::prepare runs first; text_spans must still see the ASCII form.
        assert_eq!(render("the U.S.’s debt"), "the U S's debt");
    }

    #[test]
    fn emoji_and_cjk_have_no_span_and_no_panic() {
        assert!(sp("🎉 東京 😀 你好").is_empty());
        assert!(sp("👍🏽 ‍").is_empty());
    }

    #[test]
    fn spans_are_valid_on_nasty_input() {
        let nasty = [
            "",
            "@",
            "#",
            "@@",
            "##",
            "http://",
            "https://",
            "www.",
            ".com",
            "a..b",
            "...",
            "U.",
            ".U.S.",
            "St.",
            "Dr.",
            "No.",
            "$",
            "%",
            "/",
            "//",
            "a/",
            "/b",
            "&",
            "&&",
            "A&",
            "®",
            "東京.com",
            "é.fr",
            "@東京",
            "#東京",
            "x@y",
            "a@b.c",
            "@.",
            "#1",
            "MMMM",
            "IIII",
            "VX",
            "Super",
            "Bowl",
            "km/",
            "/h",
            "20°",
            "°C",
            "±",
            "√",
            "1e10",
            "…a",
            "a…",
            "Ⅻ",
            "İ",
        ];
        for a in nasty {
            for b in nasty {
                check_invariants(&[a, b]);
                check_invariants(&["Henry", a, b, "5"]);
            }
        }
        check_invariants(&[]);
    }

    #[test]
    fn mixed_sentence_invariants() {
        let s = "Dr. Smith of the U.S. met Sen. Warren at 5 km/h near St. Louis, e.g. amazon.com \
                 & @ElonMusk #AI World War II Henry VIII 20°C ± 3 = x approach...we No. 5 Main St.";
        let w: Vec<&str> = s.split_whitespace().collect();
        check_invariants(&w);
        assert!(text_spans(&w).len() >= 15);
    }

    #[test]
    fn roman_edges() {
        for n in 1..=3999 {
            assert_eq!(roman_value(&to_roman(n)), Some(n));
        }
        for bad in ["IIII", "VX", "IL", "IC", "XM", "VV", "MMMM", "", "ii"] {
            assert_eq!(roman_value(bad), None, "{bad}");
        }
        assert_eq!(render("Type C cable"), "Type C cable");
        assert_eq!(render("Class D"), "Class D");
        assert_eq!(render("George V"), "George the fifth");
        assert_eq!(render("Type II diabetes"), "Type two diabetes");
        assert_eq!(render("War, I think"), "War, I think");
    }

    #[test]
    fn unit_edges() {
        assert_eq!(render("5 m tall"), "5 meters tall");
        assert_eq!(render("$5 m deal"), "$5 m deal");
        assert_eq!(render("6 ft."), "6 feet.");
        assert_eq!(render("-5°C"), "-5°C", "the number pass owns it");
        assert_eq!(render("m/s"), "meters per second");
    }

    #[test]
    fn number_words_local() {
        assert_eq!(cardinal(58), "fifty eight");
        assert_eq!(cardinal(105), "one hundred five");
        assert_eq!(cardinal(3999), "three thousand nine hundred ninety nine");
        assert_eq!(
            cardinal(7_428_973),
            "seven million four hundred twenty eight thousand nine hundred seventy three"
        );
        assert_eq!(ordinal(8), "eighth");
        assert_eq!(ordinal(20), "twentieth");
        assert_eq!(ordinal(21), "twenty first");
        assert_eq!(ordinal(12), "twelfth");
        assert_eq!(number_words("1,000"), Some("one thousand".into()));
        assert_eq!(number_words("0.35"), Some("zero point three five".into()));
        assert_eq!(number_words("1,,0"), None);
        assert_eq!(number_words("."), None);
        assert_eq!(
            number_words(&"9".repeat(60)).map(|s| s.split(' ').count()),
            Some(60)
        );
    }

    #[test]
    fn lone_i_after_a_trigger_is_usually_the_pronoun() {
        assert_eq!(render("the Cold War I moved"), "the Cold War I moved");
        assert_eq!(render("Before the War I lived"), "Before the War I lived");
        assert_eq!(render("Part I think"), "Part I think");
        assert_eq!(render("In Class I learned"), "In Class I learned");
        assert_eq!(render("World War I ended"), "World War one ended");
        assert_eq!(render("after Great War I"), "after Great War one");
        assert_eq!(render("Chapter I."), "Chapter one.");
        assert_eq!(render("Part I, the start"), "Part one, the start");
        assert_eq!(render("Part I Introduction"), "Part one Introduction");
        assert_eq!(render("in Part I"), "in Part one");
    }

    #[test]
    fn no_needs_a_digit() {
        assert_eq!(render("Is it A? No. C."), "Is it A? No. C.");
        assert_eq!(render("No. IV"), "No. IV");
        assert_eq!(render("No. 5"), "number 5");
        assert_eq!(render("Vol. III"), "volume three");
    }

    #[test]
    fn ambiguous_units_need_digits() {
        assert_eq!(render("one gal told me"), "one gal told me");
        assert_eq!(render("take Route 66 W"), "take Route 66 W");
        assert_eq!(render("then Route 66 W today"), "then Route 66 W today");
        // Sentence-initial or imperative capitals are not names (fix round 2).
        assert_eq!(render("Add 5 g salt"), "Add 5 grams salt");
        assert_eq!(render("then Mix 5 g sugar"), "then Mix 5 grams sugar");
        assert_eq!(render("ten W"), "ten W");
        assert_eq!(render("60 W bulb"), "60 watts bulb");
        assert_eq!(render("5 gal tank"), "5 gallons tank");
        assert_eq!(render("five km away"), "five kilometers away");
    }

    #[test]
    fn jan_and_jun_need_a_day() {
        assert_eq!(render("Jan. 5"), "january 5");
        assert_eq!(render("Jun. 12th"), "june 12th");
        assert_eq!(render("met Jan. today"), "met Jan. today");
        assert_eq!(render("said Jun."), "said Jun.");
        assert_eq!(render("Feb. 3"), "february 3");
    }

    #[test]
    fn no_false_pause_before_a_capital() {
        // Ruling (fix round 2): a missed pause ("U.S. Trump said") is acceptable,
        // a false pause ("U.S. Senate") is not. Period only at a sentence end.
        assert_eq!(render("the U.S. Trump said"), "the U S Trump said");
        assert_eq!(render("the U.S. Senate"), "the U S Senate");
        assert_eq!(render("the U.S. Army"), "the U S Army");
        assert_eq!(render("by J.K. Rowling"), "by J K Rowling");
        assert_eq!(render("T.S. Eliot wrote"), "T S Eliot wrote");
        assert_eq!(render("the D.C. Circuit"), "the D C Circuit");
        assert_eq!(
            render("Apple Inc. Shares rose"),
            "Apple incorporated Shares rose"
        );
        assert_eq!(render("at 5 p.m. Eastern"), "at 5 P M Eastern");
        assert_eq!(render("in the U.S. The rest"), "in the U S. The rest");
        assert_eq!(render("Mr. Smith"), "mister Smith");
        assert_eq!(render("the U.S. economy"), "the U S economy");
    }

    // ---- Task 6 fix rounds ----
    #[test]
    fn math_symbols_glued_to_a_letter_are_said() {
        assert_eq!(render("so x× y= 21"), "so X times Y equals 21");
        assert_eq!(render("and x÷ y"), "and X divided by y");
        assert_eq!(render("If a> b and b≥ c,"), "If A greater than b and B greater than or equal to c,");
        assert_eq!(render("it was≈ 5"), "it was approximately 5");
        assert_eq!(render("∆v is"), "delta V is");
        // a plain word with no symbol is untouched
        assert_eq!(render("so x y"), "so x y");
    }

    #[test]
    fn units_before_closing_brackets_and_quotes() {
        assert_eq!(render("(82–86 °F)."), "(82–86 degrees Fahrenheit.");
        assert_eq!(render("above 6 GHz\"."), "above 6 gigahertz.");
        assert_eq!(render("3/4 tsp of salt"), "3/4 teaspoons of salt");
        assert_eq!(render("1 tbsp sugar"), "1 tablespoon sugar");
        assert_eq!(render("a 64 kb DRAM"), "a 64 kilobits DRAM");
    }

    #[test]
    fn roman_one_after_a_name_at_a_clause_end() {
        assert_eq!(render("was the Apple I, designed by"), "was the Apple one, designed by");
        assert_eq!(render("King Henry I. Then"), "King Henry the first. Then");
        // the pronoun stays
        assert_eq!(render("said John I think"), "said John I think");
        assert_eq!(render("Anna and I, too"), "Anna and I, too");
        assert_eq!(render("When I, too"), "When I, too");
    }

    #[test]
    fn titles_and_est_by_context() {
        assert_eq!(render("Sec. Blinken and Amb. Thomas-Greenfield spoke"), "secretary Blinken and ambassador Thomas-Greenfield spoke");
        assert_eq!(render("see Sec. 12 now"), "see section 12 now");
        assert_eq!(render("a firm est. 1967 in"), "a firm established 1967 in");
        assert_eq!(render("cost est. 5 million"), "cost estimated 5 million");
    }

    #[test]
    fn bit_rate_units() {
        assert_eq!(render("is 5.9 Gbit/s now"), "is 5.9 gigabits per second now");
        assert_eq!(render("at 100 Mbit/s and"), "at 100 megabits per second and");
        assert_eq!(render("a 10 Gbit link"), "a 10 gigabits link");
    }

    #[test]
    fn unit_after_per_is_singular() {
        assert_eq!(render("density: 40 per km²."), "density: 40 per square kilometer.");
        assert_eq!(render("10 per kg of"), "10 per kilogram of");
        // a plain word after "per" is untouched
        assert_eq!(render("per m of"), "per m of");
    }

    #[test]
    fn mass_unit_over_c_squared() {
        assert_eq!(render("a MeV/c² mass"), "a MeV per c squared mass");
    }

    #[test]
    fn possessive_domain_and_file_names() {
        assert_eq!(render("Amazon.com's profits"), "amazon dot com's profits");
        assert_eq!(render("in Node.js today"), "in node dot J S today");
        assert_eq!(render("the \"consolidated.db\" file"), "the consolidated dot D B file");
        assert_eq!(render("open data.json now"), "open data dot jason now");
        // a sentence glued to the next one is not a file
        assert_eq!(render("it ended.The next"), "it ended.The next");
    }

    #[test]
    fn roman_two_and_three_after_a_name() {
        assert_eq!(render("the Apple II in"), "the Apple two in");
        assert_eq!(render("the Phenom II processor"), "the Phenom two processor");
        assert_eq!(render("Artemis III (late"), "Artemis three (late");
        assert_eq!(render("class II MHC"), "class two MHC");
        assert_eq!(render("CHAPTER XV The"), "CHAPTER fifteen The");
        assert_eq!(render("Article II, Section IV and"), "Article two, Section four and");
        // still not: initialisms and sizes
        assert_eq!(render("the Pixel 9 Pro XL were"), "the Pixel 9 Pro XL were");
        assert_eq!(render("both cited ML data"), "both cited ML data");
        assert_eq!(render("on the Amazon Console and CLI."), "on the Amazon Console and CLI.");
        assert_eq!(render("Jacob II"), "Jacob two");
    }

    #[test]
    fn tables_are_sorted() {
        super::check_tables_sorted();
    }
}
