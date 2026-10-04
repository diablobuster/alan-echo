//! Read Aloud: number spans for the text pre-pass (years, money, decimals,
//! ordinals, dates, times, ranges, fractions, phone numbers, versions).
//! Pure, panic-free, no regex. Spoken text style: see `ra_numwords`.
//!
//! Leading and trailing punctuation of a source word (`(`, `,`, `.`, `)` ...)
//! is kept on the spoken text so the sentence pause survives the override.
//! Every word that contains an ASCII digit gets a non-empty span.

use crate::ra_numwords::{cardinal, decimal, digits, integer, ordinal, ordinalize, year};

static MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September",
    "October", "November", "December",
];
/// Lowercase month spellings -> month number. Sorted (binary search).
static MONTH_KEYS: [(&str, u32); 24] = [
    ("apr", 4), ("april", 4), ("aug", 8), ("august", 8), ("dec", 12), ("december", 12),
    ("feb", 2), ("february", 2), ("jan", 1), ("january", 1), ("jul", 7), ("july", 7),
    ("jun", 6), ("june", 6), ("mar", 3), ("march", 3), ("may", 5), ("nov", 11),
    ("november", 11), ("oct", 10), ("october", 10), ("sep", 9), ("sept", 9), ("september", 9),
];
/// Words before a 4-digit number that make it a year. Sorted.
static YEAR_PREV: [&str; 25] = [
    "after", "autumn", "before", "between", "by", "c", "ca", "circa", "during", "early",
    "fall", "from", "in", "late", "mid", "since", "spring", "summer", "through", "thru",
    "till", "until", "winter", "year", "years",
];
/// Words before a number that make it a quantity or a label (never a year). Sorted.
static CARD_PREV: [&str; 38] = [
    "about", "almost", "approx", "approximately", "apt", "bus", "channel", "exactly", "ext",
    "flight", "gate", "highway", "interstate", "just", "least", "model", "more", "most",
    "nearly", "no", "nos", "number", "only", "over", "p", "page", "pages", "pp", "rank",
    "room", "roughly", "route", "some", "suite", "than", "total", "unit", "version",
];
/// Determiners before a year-shaped number that make it a year even when the
/// sentence has other digits ("its 2012 ranking", "the 2026 race"). Sorted.
static YEAR_DET: [&str; 9] = ["her", "his", "its", "our", "that", "the", "their", "this", "whose"];
/// Nouns after a model size that make its K/M/B/T suffix a scale word
/// ("405B parameters" -> "four hundred five billion parameters"). Sorted.
static AI_NOUNS: [&str; 6] = ["parameter", "parameters", "params", "token", "tokens", "weights"];
/// Lowercase words between a number and a count noun that keep it a quantity
/// ("2050 new jobs"). Sorted.
static COUNT_ADJ: [&str; 7] = ["additional", "extra", "fewer", "more", "new", "other", "total"];
/// Unit and count nouns after a number that make it a quantity. Sorted.
static COUNT_NEXT: [&str; 84] = [
    "acres", "aircraft", "beds", "buildings", "calories", "cars", "cases", "characters",
    "children", "cm", "companies", "copies", "customers", "days", "deaths", "doctors",
    "dollars", "employees", "euros", "families", "feet", "firms", "ft", "hectares", "homes",
    "hours", "households", "houses", "items", "jobs", "kg", "kids", "kilometers", "kilometres",
    "km", "lbs", "lines", "liters", "litres", "members", "men", "meters", "metres", "migrants",
    "miles", "minutes", "mm", "months", "officers", "pages", "patients", "people", "persons",
    "planes", "points", "police", "pounds", "refugees", "residents", "rooms", "rows",
    "schools", "seats", "seconds", "shares", "ships", "soldiers", "staff", "stores",
    "students", "times", "tonnes", "tons", "troops", "units", "users", "vehicles", "votes",
    "weeks", "women", "words", "workers", "yards", "years",
];
/// Glued units only the number pass knows: (suffix, singular, plural). Every other
/// glued unit (`5km`, `3kWh`, `20°C`) comes from the text pass's unit table
/// (`ra_norm_text::glued_unit`), so the two passes read units alike.
static UNITS: [(&str, &str, &str); 2] = [
    ("km/h", "kilometer per hour", "kilometers per hour"),
    ("x", "times", "times"),
];

fn has_digit(s: &str) -> bool {
    s.bytes().any(|b| b.is_ascii_digit())
}

fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn is_apos(c: char) -> bool {
    matches!(c, '\'' | '’' | 'ʼ')
}

fn is_lead(c: char) -> bool {
    matches!(c, '(' | '[' | '{' | '"' | '“' | '‘' | '«' | '\'' | '’' | 'ʼ')
}

fn is_trail(c: char) -> bool {
    matches!(
        c,
        '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '"' | '”' | '’' | '\'' | '»' | '…'
    )
}

/// `90s`, `90's` right after a leading apostrophe.
fn is_short_decade(rest: &str) -> bool {
    let b = rest.as_bytes();
    b.len() >= 3 && b[0].is_ascii_digit() && b[1].is_ascii_digit() && !b[2].is_ascii_digit()
}

/// Split a source word into (leading punctuation, core, trailing punctuation).
fn split_punct(w: &str) -> (&str, &str, &str) {
    let mut start = 0;
    for (i, c) in w.char_indices() {
        let next = i + c.len_utf8();
        if is_lead(c) && !(is_apos(c) && is_short_decade(&w[next..])) {
            start = next;
        } else {
            break;
        }
    }
    let rest = &w[start..];
    let mut end = rest.len();
    for (i, c) in rest.char_indices().rev() {
        if is_trail(c) {
            end = i;
        } else {
            break;
        }
    }
    (&w[..start], &rest[..end], &rest[end..])
}

fn lc_core(w: &str) -> String {
    split_punct(w).1.to_lowercase()
}

/// Month number for a capitalised month name or abbreviation ("Sept", "March").
fn month_of(core: &str) -> Option<u32> {
    if !core.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
        return None;
    }
    let lc = core.to_ascii_lowercase();
    MONTH_KEYS.binary_search_by(|(k, _)| k.cmp(&lc.as_str())).ok().map(|i| MONTH_KEYS[i].1)
}

/// `21st` -> Some("21"). The prefix must be digits (commas allowed).
fn strip_ordinal_suffix(core: &str) -> Option<&str> {
    if core.len() < 3 || !core.is_char_boundary(core.len() - 2) {
        return None;
    }
    let (p, s) = core.split_at(core.len() - 2);
    let s = s.to_ascii_lowercase();
    let ok_suffix = matches!(s.as_str(), "st" | "nd" | "rd" | "th");
    let ok_prefix = has_digit(p) && p.bytes().all(|b| b.is_ascii_digit() || b == b',');
    (ok_suffix && ok_prefix).then_some(p)
}

/// A day of the month: `5`, `14th`, `1st` (1..=31).
fn day_of(core: &str) -> Option<u64> {
    let body = strip_ordinal_suffix(core).unwrap_or(core);
    if !all_digits(body) || body.len() > 2 {
        return None;
    }
    let d: u64 = body.parse().ok()?;
    (1..=31).contains(&d).then_some(d)
}

fn in_sorted(list: &[&str], w: &str) -> bool {
    list.binary_search(&w).is_ok()
}

/// `am`, `pm`, `a.m`, `p.m` (any case) -> the spoken letters.
fn ampm(core: &str) -> Option<&'static str> {
    if core.len() > 4 {
        return None;
    }
    let s: String = core.chars().filter(|c| *c != '.').collect::<String>().to_ascii_lowercase();
    match s.as_str() {
        "am" => Some("A M"),
        "pm" => Some("P M"),
        _ => None,
    }
}

/// A glued am/pm suffix: "7:30pm" -> ("7:30", "P M").
fn split_ampm(core: &str) -> Option<(&str, &'static str)> {
    let lc = core.to_ascii_lowercase();
    for suf in ["a.m", "p.m", "am", "pm"] {
        if lc.ends_with(suf) && core.is_char_boundary(core.len() - suf.len()) {
            let body = &core[..core.len() - suf.len()];
            if has_digit(body) {
                return ampm(suf).map(|a| (body, a));
            }
        }
    }
    None
}

fn pluralize(words: &str) -> String {
    let (head, last) = match words.rsplit_once(' ') {
        Some((h, l)) => (Some(h), l),
        None => (None, words),
    };
    let last = if let Some(stem) = last.strip_suffix('y') {
        format!("{stem}ies")
    } else if last.ends_with('x') {
        format!("{last}es")
    } else {
        format!("{last}s")
    };
    match head {
        Some(h) => format!("{h} {last}"),
        None => last,
    }
}

/// A two-digit year or seconds tail: "21" -> "twenty one", "09" -> "oh nine".
fn two_digit_year(s: &str) -> String {
    match s.parse::<u64>() {
        Ok(n) if n < 10 && s.len() == 2 => format!("oh {}", cardinal(n)),
        _ => integer(s),
    }
}

/// What the neighbours of a word say about how to read it.
#[derive(Clone, Copy, Default)]
struct Ctx {
    /// Plain 4-digit numbers may read as years at all (only at the top level).
    allow_year: bool,
    /// The context says "year" (in/since/by, a month, parentheses ...).
    year_hint: bool,
    /// The context says "quantity" (page/about/over before, a count noun after).
    card_hint: bool,
    /// Another word in the same sentence contains a digit.
    other_digits: bool,
    /// `3/4` should read as a date (after "on", a weekday ...).
    date_hint: bool,
    /// An AI-size noun follows (`405B parameters`): a K/M/B/T suffix is a scale word.
    ai_scale: bool,
    /// A month name, a year or a date word is near: `5/10` stays a date, not a rating.
    near_date: bool,
    /// Blood pressure (`120/80 mmHg`, "blood pressure was 90/60"): `a/b` reads "a over b".
    pressure: bool,
    /// The word before is exactly one ("1", "one"): a unit here is singular (`1 km2`).
    after_one: bool,
    /// "for" follows: `24/36 for 291 yards` is "twenty four of thirty six".
    of_next: bool,
    /// "per" before: `per 1M tokens` is a quantity ("one million").
    per_prev: bool,
    /// "percentage", "point", "percent" or "of" follows: `1/4` is a fraction.
    frac_next: bool,
}

fn plain() -> Ctx {
    Ctx::default()
}

fn is_year_num(s: &str) -> bool {
    s.len() == 4 && all_digits(s) && s.parse::<u32>().is_ok_and(|n| (1000..=2999).contains(&n))
}

fn ends_sentence(w: &str) -> bool {
    w.ends_with(['.', '!', '?'])
}

fn build_ctx(words: &[&str], i: usize, lead: &str, trail: &str) -> Ctx {
    let prev_core = if i >= 1 { split_punct(words[i - 1]).1 } else { "" };
    let prev2_core = if i >= 2 { split_punct(words[i - 2]).1 } else { "" };
    let prev = prev_core.to_lowercase();
    let next = words.get(i + 1).map(|w| lc_core(w)).unwrap_or_default();

    let after_month = month_of(prev_core).is_some();
    let after_month_day = month_of(prev2_core).is_some() && day_of(prev_core).is_some();
    let range_tail = matches!(prev.as_str(), "to" | "and" | "through" | "until" | "-" | "–")
        && is_year_num(prev2_core);
    let modern_now = words.get(i).is_some_and(|w| {
        let c = split_punct(w).1;
        is_year_num(c) && c.parse::<u32>().is_ok_and(|n| (1900..=2099).contains(&n))
    });
    // "(2017 reference year)": an opening bracket before a modern year is enough
    let parens = lead.contains('(') && (trail.contains(')') || modern_now);
    let modern = words.get(i).is_some_and(|w| {
        let c = split_punct(w).1;
        is_year_num(c) && c.parse::<u32>().is_ok_and(|n| (1900..=2099).contains(&n))
    });
    // "Windows Server 2012", "Mars 2020": a capitalised name (not the sentence's
    // first word, not a label like Room) before a modern year-shaped number.
    let prev_is_name = i >= 1
        && !ends_sentence(if i >= 2 { words[i - 2] } else { "." })
        && prev_core.chars().next().is_some_and(char::is_uppercase)
        && prev_core.chars().skip(1).all(|c| c.is_alphabetic())
        && prev_core.len() >= 2
        && split_punct(words[i - 1]).2.is_empty()
        && month_of(prev_core).is_none();
    // "since about 2019", "in around 1990"
    let about_year = matches!(prev.as_str(), "about" | "around" | "circa" | "approximately")
        && in_sorted(&YEAR_PREV, &prev2_core.to_lowercase());
    // round 2: "for 2026", "September of 1814" (a month, then "of")
    let month_of_year = prev == "of" && month_of(prev2_core).is_some() && is_year_num(split_punct(words[i]).1);
    let modern_cue = modern && (prev_is_name || in_sorted(&YEAR_DET, &prev) || prev == "of" || prev == "for");
    let year_hint = in_sorted(&YEAR_PREV, &prev)
        || month_of_year
        || after_month
        || after_month_day
        || range_tail
        || parens
        || about_year
        || (modern_cue && !in_sorted(&COUNT_NEXT, &next));
    let card_hint = (in_sorted(&CARD_PREV, &prev) && !about_year)
        || prev == "#"
        || in_sorted(&COUNT_NEXT, &next);
    // "2050 new jobs": a lowercase adjective, then a count noun. This overrides only the
    // default year reading, not an explicit cue ("In 2020 new cases rose").
    let next_raw = words.get(i + 1).map(|w| split_punct(w).1).unwrap_or("");
    let next2 = words.get(i + 2).map(|w| lc_core(w)).unwrap_or_default();
    let adj_count = next_raw.chars().next().is_some_and(|c| c.is_lowercase())
        && in_sorted(&COUNT_ADJ, next_raw)
        && in_sorted(&COUNT_NEXT, &next2);
    let card_hint = card_hint || (adj_count && !year_hint);

    // The sentence around word i: back to the last terminal, forward through the next one.
    let mut s = i;
    while s > 0 && !ends_sentence(words[s - 1]) {
        s -= 1;
    }
    let mut e = i + 1;
    while e < words.len() && !ends_sentence(words[e - 1]) {
        e += 1;
    }
    // Only plain quantities count ("3", "1,200", "2.5"); years, ordinals and codes don't.
    let other_digits = (s..e.min(words.len())).any(|k| {
        let c = split_punct(words[k]).1;
        k != i && !is_year_num(c) && number_parts(c).is_some()
    });

    let date_hint = matches!(
        prev.as_str(),
        "on" | "since" | "by" | "until" | "before" | "after" | "date" | "dated" | "monday"
            | "tuesday" | "wednesday" | "thursday" | "friday" | "saturday" | "sunday"
    );
    let ai_scale = in_sorted(&AI_NOUNS, &next);
    let lo = i.saturating_sub(4);
    let hi = (i + 5).min(words.len());
    let near_date = (lo..hi).any(|k| {
        let c = split_punct(words[k]).1;
        k != i
            && (month_of(c).is_some()
                || is_year_num(c)
                || matches!(c.to_lowercase().as_str(), "year" | "years" | "date" | "dated" | "day"))
    });
    let pressure = next == "mmhg"
        || (next == "blood" && next2 == "pressure")
        || (i.saturating_sub(3)..i).any(|k| matches!(lc_core(words[k]).as_str(), "pressure" | "bp"));
    let after_one = i >= 1 && matches!(prev.as_str(), "1" | "one") && split_punct(words[i - 1]).2.is_empty();
    let of_next = next == "for" && words.get(i + 2).is_some_and(|w| split_punct(w).1.starts_with(|c: char| c.is_ascii_digit()));
    // "by 1/4 percentage point": a fraction, not a date
    let frac_next = matches!(next.as_str(), "percentage" | "point" | "points" | "percent" | "of");
    let per_prev = prev == "per";
    Ctx { allow_year: true, year_hint, card_hint, other_digits, date_hint, ai_scale, near_date, pressure, after_one, of_next, per_prev, frac_next }
}

// ---- currency ----

struct Currency {
    one: &'static str,
    many: &'static str,
    /// Subunit (singular, plural) said for a two-digit fraction, if any.
    sub: Option<(&'static str, &'static str)>,
}

const CENTS: Option<(&str, &str)> = Some(("cent", "cents"));

static CURRENCIES: [(&str, Currency); 9] = [
    ("US$", Currency { one: "U S dollar", many: "U S dollars", sub: CENTS }),
    ("HK$", Currency { one: "Hong Kong dollar", many: "Hong Kong dollars", sub: CENTS }),
    ("C$", Currency { one: "Canadian dollar", many: "Canadian dollars", sub: CENTS }),
    ("A$", Currency { one: "Australian dollar", many: "Australian dollars", sub: CENTS }),
    ("$", Currency { one: "dollar", many: "dollars", sub: CENTS }),
    ("€", Currency { one: "euro", many: "euros", sub: CENTS }),
    ("£", Currency { one: "pound", many: "pounds", sub: Some(("penny", "pence")) }),
    ("¥", Currency { one: "yen", many: "yen", sub: None }),
    ("₹", Currency { one: "rupee", many: "rupees", sub: None }),
];

/// `$480` -> (dollars, "480"); `5€` -> (euros, "5").
fn currency_prefix(core: &str) -> Option<(&'static Currency, &str)> {
    for (p, c) in CURRENCIES.iter() {
        if let Some(rest) = core.strip_prefix(p) {
            return Some((c, rest));
        }
    }
    for (p, c) in CURRENCIES.iter().filter(|(p, _)| p.chars().count() == 1) {
        if let Some(rest) = core.strip_suffix(p) {
            return Some((c, rest));
        }
    }
    None
}

fn scale_suffix(s: &str) -> Option<&'static str> {
    Some(match s {
        "K" | "k" => "thousand",
        "M" | "m" | "mn" | "MM" | "mm" => "million",
        "B" | "b" | "bn" | "Bn" | "BN" => "billion",
        "T" | "tn" | "Tn" => "trillion",
        _ => return None,
    })
}

fn scale_word(lc: &str) -> Option<&'static str> {
    Some(match lc {
        "thousand" => "thousand",
        "million" | "mn" | "m" => "million",
        "billion" | "bn" => "billion",
        "trillion" | "tn" => "trillion",
        _ => return None,
    })
}

/// Split `3.2B` into ("3.2", Some("billion")); the amount must be a valid number.
fn amount_and_scale(rest: &str) -> Option<(&str, Option<&'static str>)> {
    let end = rest
        .char_indices()
        .find(|(_, c)| !(c.is_ascii_digit() || *c == '.' || *c == ','))
        .map(|(i, _)| i)
        .unwrap_or(rest.len());
    let (amt, suf) = rest.split_at(end);
    if !has_digit(amt) {
        return None;
    }
    let scale = if suf.is_empty() { None } else { Some(scale_suffix(suf)?) };
    number_parts(amt)?;
    Some((amt, scale))
}

fn say_currency(cur: &Currency, amt: &str, scale: Option<&str>) -> Option<String> {
    let (ip, fp) = number_parts(amt)?;
    if let Some(sc) = scale {
        return Some(format!("{} {sc} {}", decimal(ip, fp), cur.many));
    }
    let int_words = if ip.is_empty() { "zero".to_string() } else { integer(ip) };
    let name = if ip == "1" { cur.one } else { cur.many };
    if fp.is_empty() {
        return Some(format!("{int_words} {name}"));
    }
    if let (Some((s1, sn)), 2) = (cur.sub, fp.len()) {
        let cents: u64 = fp.parse().ok()?;
        let cents_words = format!("{} {}", cardinal(cents), if cents == 1 { s1 } else { sn });
        if ip.bytes().all(|b| b == b'0' || b == b',') {
            return Some(cents_words);
        }
        if cents == 0 {
            return Some(format!("{int_words} {name}"));
        }
        return Some(format!("{int_words} {name} {cents_words}"));
    }
    Some(format!("{} {}", decimal(ip, fp), cur.many))
}

// ---- plain numbers ----

/// Validate `7,428,973.25` / `2.5` / `.5` and split it into (int, frac).
fn number_parts(s: &str) -> Option<(&str, &str)> {
    let (ip, fp) = match s.split_once('.') {
        Some((a, b)) if all_digits(b) => (a, b),
        Some(_) => return None,
        None => (s, ""),
    };
    if ip.is_empty() {
        return (!fp.is_empty()).then_some((ip, fp));
    }
    if ip.contains(',') {
        let mut groups = ip.split(',');
        let first = groups.next()?;
        if !all_digits(first) || first.len() > 3 {
            return None;
        }
        if !groups.all(|g| g.len() == 3 && all_digits(g)) {
            return None;
        }
    } else if !all_digits(ip) {
        return None;
    }
    Some((ip, fp))
}

fn plain_number(s: &str) -> Option<String> {
    let (ip, fp) = number_parts(s)?;
    Some(decimal(ip, fp))
}

/// `1990s`, `'90s`, `80s`, `1990's` -> plural words.
fn decade(core: &str) -> Option<String> {
    let body = core
        .strip_suffix("'s")
        .or_else(|| core.strip_suffix("’s"))
        .or_else(|| core.strip_suffix('s'))?;
    let body = body.strip_prefix(['\'', '’', 'ʼ']).unwrap_or(body);
    if !all_digits(body) || body.len() > 4 {
        return None;
    }
    let n: u32 = body.parse().ok()?;
    let words = if body.len() == 4 { year(n) } else { integer(body) };
    let words = match words.strip_prefix("one ") {
        Some(rest) if n == 100 || n == 1000 => rest.to_string(),
        _ => words,
    };
    Some(pluralize(&words))
}

fn sign_word(c: char) -> Option<&'static str> {
    Some(match c {
        '-' | '−' => "minus",
        '+' => "plus",
        '±' => "plus or minus",
        '~' | '≈' => "about",
        '<' => "less than",
        '>' => "more than",
        '≤' => "at most",
        '≥' => "at least",
        '#' | '№' => "number",
        '§' => "section",
        '¶' => "paragraph",
        '√' => "square root of",
        _ => return None,
    })
}

fn time_words(h: u64, m: u64, ap: Option<&str>) -> String {
    let hw = if ap.is_some() && h == 0 { "twelve".to_string() } else { cardinal(h) };
    let mw = match m {
        0 => match ap {
            Some(_) => String::new(),
            None if (1..=12).contains(&h) => "o'clock".into(),
            None => "hundred".into(),
        },
        1..=9 => format!("oh {}", cardinal(m)),
        _ => cardinal(m),
    };
    let mut out = hw;
    for part in [mw.as_str(), ap.unwrap_or("")] {
        if !part.is_empty() {
            out.push(' ');
            out.push_str(part);
        }
    }
    out
}

/// `H:MM` with an optional am/pm, if it is a valid clock time.
fn clock(core: &str, ap: Option<&str>) -> Option<String> {
    let (h, m) = core.split_once(':')?;
    if !all_digits(h) || h.len() > 2 || m.len() != 2 || !all_digits(m) {
        return None;
    }
    let h: u64 = h.parse().ok()?;
    let m: u64 = m.parse().ok()?;
    let max_h = if ap.is_some() { 12 } else { 23 };
    (h <= max_h && m <= 59).then(|| time_words(h, m, ap))
}

/// `7:30`, `22:37`, `7:30pm`, `1:30:45`, `3:1` (a ratio or score).
fn colon_number(core: &str) -> Option<String> {
    let (body, ap) = match split_ampm(core) {
        Some((b, a)) => (b, Some(a)),
        None => (core, None),
    };
    let parts: Vec<&str> = body.split(':').collect();
    if parts.len() < 2 || !parts.iter().all(|p| all_digits(p)) {
        return None;
    }
    let with_ap = |mut s: String| {
        if let Some(a) = ap {
            s.push(' ');
            s.push_str(a);
        }
        s
    };
    if parts.len() == 2 {
        if let Some(t) = clock(body, ap) {
            return Some(t);
        }
    }
    if parts.len() == 3 && parts[2].len() == 2 {
        let hm = format!("{}:{}", parts[0], parts[1]);
        if let Some(t) = clock(&hm, None) {
            return Some(with_ap(format!("{t} {}", two_digit_year(parts[2]))));
        }
    }
    let words: Vec<String> = parts.iter().map(|p| integer(p)).collect();
    Some(with_ap(words.join(" to ")))
}

fn date_words(m: u64, d: u64, y: Option<&str>) -> Option<String> {
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let base = format!("{} {}", MONTHS[(m - 1) as usize], ordinal(d));
    Some(match y {
        None => base,
        Some(y) if y.len() == 4 => format!("{base}, {}", year(y.parse().ok()?)),
        Some(y) if y.len() == 2 => format!("{base}, {}", two_digit_year(y)),
        Some(_) => return None,
    })
}

fn fraction_words(num: u64, den: u64) -> String {
    let den_word = match (den, num == 1) {
        (2, true) => "half".to_string(),
        (2, false) => "halves".to_string(),
        (4, true) => "quarter".to_string(),
        (4, false) => "quarters".to_string(),
        (_, true) => ordinal(den),
        (_, false) => format!("{}s", ordinal(den)),
    };
    format!("{} {den_word}", cardinal(num))
}

/// A day-month date (`25/12` -> "the twenty fifth of December"): only when the
/// day is above 12, so it cannot be a US month/day.
fn day_month_words(d: u64, m: u64, y: Option<&str>) -> Option<String> {
    if !(13..=31).contains(&d) || !(1..=12).contains(&m) {
        return None;
    }
    let base = format!("the {} of {}", ordinal(d), MONTHS[(m - 1) as usize]);
    Some(match y {
        None => base,
        Some(y) if y.len() == 4 => format!("{base}, {}", year(y.parse().ok()?)),
        Some(y) if y.len() == 2 => format!("{base}, {}", two_digit_year(y)),
        Some(_) => return None,
    })
}

/// Units a fraction can measure, as written after it (lowercase key, sorted) ->
/// the singular said ("3/4 cup" -> "three quarters of a cup").
static FRACTION_UNITS: [(&str, &str); 53] = [
    ("acre", "acre"), ("acres", "acre"), ("c", "cup"), ("cm", "centimeter"), ("cup", "cup"), ("cups", "cup"),
    ("feet", "foot"), ("foot", "foot"), ("ft", "foot"), ("gal", "gallon"), ("gallon", "gallon"),
    ("gallons", "gallon"), ("hour", "hour"), ("hours", "hour"), ("hr", "hour"), ("in", "inch"),
    ("inch", "inch"), ("inches", "inch"), ("kg", "kilogram"), ("lb", "pound"), ("lbs", "pound"),
    ("liter", "liter"), ("liters", "liter"), ("litre", "liter"), ("litres", "liter"), ("mi", "mile"),
    ("mile", "mile"), ("miles", "mile"), ("min", "minute"), ("minute", "minute"), ("minutes", "minute"),
    ("mm", "millimeter"), ("ounce", "ounce"), ("ounces", "ounce"), ("oz", "ounce"), ("pint", "pint"),
    ("pints", "pint"), ("pound", "pound"), ("pounds", "pound"), ("qt", "quart"), ("quart", "quart"),
    ("quarts", "quart"), ("second", "second"), ("stick", "stick"), ("sticks", "stick"),
    ("tablespoon", "tablespoon"), ("tablespoons", "tablespoon"), ("tbsp", "tablespoon"),
    ("teaspoon", "teaspoon"), ("teaspoons", "teaspoon"), ("tsp", "teaspoon"), ("yard", "yard"),
    ("yards", "yard"),
];

/// A vulgar fraction character as (a, b).
fn vulgar(core: &str) -> Option<(u64, u64)> {
    let mut cs = core.chars();
    let c = cs.next()?;
    if cs.next().is_some() {
        return None;
    }
    Some(match c {
        '½' => (1, 2),
        '⅓' => (1, 3),
        '⅔' => (2, 3),
        '¼' => (1, 4),
        '¾' => (3, 4),
        '⅕' => (1, 5),
        '⅖' => (2, 5),
        '⅗' => (3, 5),
        '⅘' => (4, 5),
        '⅙' => (1, 6),
        '⅚' => (5, 6),
        '⅛' => (1, 8),
        '⅜' => (3, 8),
        '⅝' => (5, 8),
        '⅞' => (7, 8),
        _ => return None,
    })
}

/// The fraction after a whole number: "a half", "an eighth", "three quarters".
fn mixed_tail(a: u64, b: u64) -> String {
    if a != 1 {
        return fraction_words(a, b);
    }
    let w = match b {
        2 => "half".to_string(),
        4 => "quarter".to_string(),
        _ => ordinal(b),
    };
    let art = if w.starts_with(['a', 'e', 'i', 'o', 'u']) { "an" } else { "a" };
    format!("{art} {w}")
}

/// `a/b` with 1 <= a < b <= 64 (a common fraction), as (a, b); a vulgar
/// fraction character too.
fn common_fraction(core: &str) -> Option<(u64, u64)> {
    if let Some(f) = vulgar(core) {
        return Some(f);
    }
    let (a, b) = core.split_once('/')?;
    if a.is_empty() || b.is_empty() || a.len() > 2 || b.len() > 2 || !all_digits(a) || !all_digits(b) {
        return None;
    }
    let (a, b) = (a.parse::<u64>().ok()?, b.parse::<u64>().ok()?);
    (a >= 1 && a < b && b <= 64).then_some((a, b))
}

/// A fraction measuring a unit: "thirteen sixteenths of an inch", "half a cup".
fn fraction_of_unit(a: u64, b: u64, unit: &str) -> String {
    let art = if unit.starts_with(['a', 'e', 'i', 'o', 'u']) || unit == "hour" { "an" } else { "a" };
    if (a, b) == (1, 2) {
        format!("half {art} {unit}")
    } else {
        format!("{} of {art} {unit}", fraction_words(a, b))
    }
}

/// `1/2`, `3/14`, `3/14/2025`, `24/7`; ratings (`9/10` -> "nine out of ten") and
/// blood pressure (`120/80` -> "one hundred twenty over eighty").
fn slash_number(core: &str, ctx: &Ctx) -> Option<String> {
    let parts: Vec<&str> = core.split('/').collect();
    if parts.len() < 2 || !parts.iter().all(|p| all_digits(p)) {
        return None;
    }
    let n: Vec<u64> = parts.iter().map(|p| p.parse::<u64>().unwrap_or(u64::MAX)).collect();
    if parts.len() == 2 {
        let (a, b) = (n[0], n[1]);
        let short = parts[0].len() <= 2 && parts[1].len() <= 2;
        // 120/80, 90/60: two 2-3 digit numbers, the first larger
        let pressure_shape = (2..=3).contains(&parts[0].len()) && (2..=3).contains(&parts[1].len()) && a > b;
        let over = || format!("{} over {}", integer(parts[0]), integer(parts[1]));
        if ctx.pressure && pressure_shape {
            return Some(over());
        }
        // completions: "24/36 for 291 yards"
        if ctx.of_next && a <= b && parts[0].len() <= 3 && parts[1].len() <= 3 {
            return Some(format!("{} of {}", integer(parts[0]), integer(parts[1])));
        }
        if ctx.frac_next && a >= 1 && a < b && b <= 64 {
            return Some(fraction_words(a, b));
        }
        if ctx.date_hint && short {
            if let Some(d) = date_words(a, b, None) {
                return Some(d);
            }
        }
        // a rating: 9/10, 10/10, 4/5 (a month or a year nearby keeps the date)
        if matches!(b, 5 | 10) && a <= b {
            if !ctx.near_date {
                return Some(format!("{} out of {}", integer(parts[0]), integer(parts[1])));
            }
            if let Some(d) = date_words(a, b, None).filter(|_| short) {
                return Some(d);
            }
        }
        if (2..=10).contains(&b) && a >= 1 && a < b {
            return Some(fraction_words(a, b));
        }
        if a == 24 && b == 7 {
            return Some("twenty four seven".into());
        }
        if short {
            if let Some(d) = date_words(a, b, None).or_else(|| day_month_words(a, b, None)) {
                return Some(d);
            }
        }
        // no cue: only a 3-digit top number (120/80); 60/40 is a split, not a pressure
        if pressure_shape && parts[0].len() == 3 {
            return Some(over());
        }
        // 50/50, 24/7, and a two-digit split with no other reading (60/40, 70/30)
        let split = parts[0].len() == 2 && parts[1].len() == 2;
        if a == b || (a == 24 && b == 7) || split {
            return Some(format!("{} {}", integer(parts[0]), integer(parts[1])));
        }
    }
    if parts.len() == 3 && parts[0].len() <= 2 && parts[1].len() <= 2 {
        if let Some(d) = date_words(n[0], n[1], Some(parts[2])).or_else(|| day_month_words(n[0], n[1], Some(parts[2]))) {
            return Some(d);
        }
    }
    let words: Vec<String> = parts.iter().map(|p| integer(p)).collect();
    Some(words.join(" slash "))
}

fn is_dash(c: char) -> bool {
    matches!(c, '-' | '–' | '—' | '‐' | '‑')
}

fn starts_numberish(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '.')
        || currency_prefix(s).is_some_and(|(_, rest)| rest.starts_with(|c: char| c.is_ascii_digit()))
}

/// `24-17`, `2019–2021`, `2017-09-25`, `555-123-4567`, `30th-largest`, `COVID-19`.
fn dash_number(core: &str, ctx: &Ctx) -> Option<String> {
    if !core.chars().any(is_dash) {
        return None;
    }
    // Empty parts ("5-", "1--2") carry nothing to say.
    let parts: Vec<&str> = core.split(is_dash).filter(|p| !p.is_empty()).collect();
    match parts.len() {
        0 => return None,
        1 => return Some(read_core(parts[0], &plain())),
        _ => {}
    }
    if parts.iter().all(|p| all_digits(p)) {
        if let [y, m, d] = parts[..] {
            if is_year_num(y) && m.len() == 2 && d.len() == 2 {
                let mw = date_words(m.parse().ok()?, d.parse().ok()?, Some(y));
                if mw.is_some() {
                    return mw;
                }
            }
        }
        if parts.len() >= 3 {
            // A phone number or another code: digit groups.
            let groups: Vec<String> = parts.iter().map(|p| digits(p)).collect();
            return Some(groups.join(", "));
        }
        let (a, b) = (parts[0], parts[1]);
        let years = ctx.allow_year
            && !ctx.card_hint
            && is_year_num(a)
            && ((is_year_num(b) && b > a) || b.len() == 2);
        if years {
            let bw = if b.len() == 2 { two_digit_year(b) } else { year(b.parse().ok()?) };
            return Some(format!("{} to {bw}", year(a.parse().ok()?)));
        }
        return Some(format!("{} to {}", integer(a), integer(b)));
    }
    if parts.len() == 2 && starts_numberish(parts[0]) && starts_numberish(parts[1]) {
        return Some(format!("{} to {}", read_core(parts[0], &plain()), read_core(parts[1], &plain())));
    }
    let words: Vec<String> = parts.iter().map(|p| read_part(p)).collect();
    Some(words.join(" "))
}

/// One piece of a hyphenated compound: numbers are read, a lone letter is uppercased, words kept.
fn read_part(p: &str) -> String {
    if has_digit(p) {
        return read_core(p, &plain());
    }
    let mut ch = p.chars();
    match (ch.next(), ch.next()) {
        (Some(c), None) if c.is_ascii_alphabetic() => c.to_ascii_uppercase().to_string(),
        _ => p.to_string(),
    }
}

/// `v2.1` -> "version two point one". A capital V needs a dot (so `V8` is not a version).
fn version(core: &str) -> Option<String> {
    let rest = core
        .strip_prefix('v')
        .or_else(|| core.strip_prefix('V').filter(|r| r.contains('.')))?;
    let groups: Vec<&str> = rest.split('.').collect();
    if !groups.iter().all(|g| all_digits(g)) {
        return None;
    }
    let words: Vec<String> = groups.iter().map(|g| integer(g)).collect();
    Some(format!("version {}", words.join(" point ")))
}

/// `20°C`, `5km`, `8TB`, `10x`.
fn number_unit(core: &str) -> Option<String> {
    let end = core
        .char_indices()
        .find(|(_, c)| !(c.is_ascii_digit() || *c == '.' || *c == ','))
        .map(|(i, _)| i)?;
    let (num, suf) = core.split_at(end);
    let (one, many) = UNITS
        .iter()
        .find(|(s, _, _)| *s == suf)
        .map(|(_, o, m)| (*o, *m))
        .or_else(|| crate::ra_norm_text::glued_unit(suf))
        .or_else(|| match suf {
            // "0.214s" (a lap time), "24h"; a whole number + s is a plural ("5s")
            "s" if num.contains('.') => Some(("second", "seconds")),
            "h" => Some(("hour", "hours")),
            _ => None,
        })?;
    let words = plain_number(num)?;
    Some(format!("{words} {}", if words == "one" { one } else { many }))
}

/// `1.23×1010`, `6.02×10^23`, `3×10^8`: a mantissa times ten to a power. A
/// glued `×1010` (a lost superscript) counts only after a decimal mantissa, so a
/// plain product (`3×1010`) stays a product.
fn times_ten(core: &str) -> Option<String> {
    let (mant, rest) = core.split_once('×').or_else(|| core.split_once('x'))?;
    let (ip, fp) = number_parts(mant)?;
    let caret = rest.starts_with("10^");
    let exp = if let Some(e) = rest.strip_prefix("10^") {
        e
    } else {
        let e = rest.strip_prefix("10")?;
        if fp.is_empty() {
            return None;
        }
        e
    };
    let e = exp.strip_prefix(['-', '−']);
    let neg = e.is_some();
    let e = e.unwrap_or(exp);
    if e.is_empty() || e.len() > 2 || !all_digits(e) {
        return None;
    }
    let n: u64 = e.parse().ok()?;
    // a glued `x100` / `x101` is a plain product, not ten to the 0th / 1st
    if !caret && !neg && n < 2 {
        return None;
    }
    let sign = if neg { "minus " } else { "" };
    Some(format!("{} times ten to the {sign}{}", decimal(ip, fp), ordinal(n)))
}

/// `2^10` -> "two to the tenth" (a trailing `=` is said: `2^10=`).
fn power(core: &str) -> Option<String> {
    let (base, rest) = core.split_once('^')?;
    let (exp, eq) = match rest.strip_suffix('=') {
        Some(e) => (e, " equals"),
        None => (rest, ""),
    };
    let (ip, fp) = number_parts(base)?;
    if exp.is_empty() || exp.len() > 2 || !all_digits(exp) {
        return None;
    }
    let n: u64 = exp.parse().ok()?;
    Some(format!("{} to the {}{eq}", decimal(ip, fp), ordinal(n)))
}

/// `km2`, `m3` (a lost superscript after a length unit) -> "square kilometers"
/// ("square kilometer" after exactly one, `one`).
fn square_unit(core: &str, one: bool) -> Option<String> {
    let (unit, pow) = (core.get(..core.len().checked_sub(1)?)?, core.get(core.len() - 1..)?);
    let kind = match pow {
        "2" => "square",
        "3" => "cubic",
        _ => return None,
    };
    let (single, many) = match unit {
        "km" => ("kilometer", "kilometers"),
        "m" => ("meter", "meters"),
        "cm" => ("centimeter", "centimeters"),
        "mm" => ("millimeter", "millimeters"),
        "mi" => ("mile", "miles"),
        "ft" => ("foot", "feet"),
        "yd" => ("yard", "yards"),
        _ => return None,
    };
    Some(format!("{kind} {}", if one { single } else { many }))
}

/// `405B` before "parameters": the K/M/B/T suffix is a scale word. `70B-parameter`
/// carries its own noun.
fn model_size(core: &str, ctx: &Ctx) -> Option<String> {
    let (size, noun) = match core.split_once('-') {
        Some((a, b)) if in_sorted(&AI_NOUNS, &b.to_ascii_lowercase()) => (a, Some(b)),
        Some(_) => return None,
        None if ctx.ai_scale => (core, None),
        None => return None,
    };
    let (amt, Some(sc)) = amount_and_scale(size)? else { return None };
    if !size.ends_with(['K', 'M', 'B', 'T']) {
        return None;
    }
    let (ip, fp) = number_parts(amt)?;
    let words = format!("{} {sc}", decimal(ip, fp));
    Some(match noun {
        Some(n) => format!("{words} {}", n.to_ascii_lowercase()),
        None => words,
    })
}

/// Last resort: digit runs are read as numbers, letter runs kept (a lone
/// letter is said as its name), a few symbols named, everything else dropped.
fn generic(core: &str) -> String {
    let chars: Vec<(usize, char)> = core.char_indices().collect();
    let byte_at = |j: usize| chars.get(j).map(|(p, _)| *p).unwrap_or(core.len());
    let mut out: Vec<String> = Vec::new();
    let mut k = 0;
    while k < chars.len() {
        let (pos, c) = chars[k];
        if c.is_ascii_digit() {
            let mut j = k;
            while j < chars.len() {
                let cj = chars[j].1;
                let joins = (cj == '.' || cj == ',')
                    && chars.get(j + 1).is_some_and(|(_, n)| n.is_ascii_digit());
                if cj.is_ascii_digit() || joins {
                    j += 1;
                } else {
                    break;
                }
            }
            let run = &core[pos..byte_at(j)];
            let w = plain_number(run).unwrap_or_else(|| {
                let parts: Vec<String> =
                    run.split([',', '.']).filter(|p| !p.is_empty()).map(integer).collect();
                parts.join(" ")
            });
            out.push(w);
            k = j;
        } else if c.is_alphabetic() {
            let mut j = k;
            while j < chars.len() && chars[j].1.is_alphabetic() {
                j += 1;
            }
            let run = &core[pos..byte_at(j)];
            let between_digits = k > 0
                && chars[k - 1].1.is_ascii_digit()
                && chars.get(j).is_some_and(|(_, n)| n.is_ascii_digit());
            let has_greek = run.chars().any(|c| crate::ra_names::greek_name(c).is_some());
            if (run == "x" || run == "×") && between_digits {
                out.push("by".into());
            } else if has_greek {
                // `ε4`, `Aβ42`: Greek letters by name, Latin runs as they are
                let mut latin = String::new();
                for ch in run.chars() {
                    if let Some(g) = crate::ra_names::greek_name(ch) {
                        if !latin.is_empty() {
                            out.push(std::mem::take(&mut latin));
                        }
                        out.push(g.to_string());
                    } else {
                        latin.push(ch);
                    }
                }
                if !latin.is_empty() {
                    out.push(latin);
                }
            } else if j == k + 1 && c.is_ascii_alphabetic() {
                out.push(c.to_ascii_uppercase().to_string());
            } else {
                out.push(run.to_string());
            }
            k = j;
        } else {
            let w = match c {
                '%' => "percent",
                '+' => "plus",
                '&' => "and",
                '#' => "number",
                '=' => "equals",
                '°' => "degrees",
                '×' => "times",
                '÷' => "divided by",
                '½' => "and a half",
                '¼' => "and a quarter",
                '¾' => "and three quarters",
                _ => "",
            };
            if !w.is_empty() {
                // "½" alone is "a half", after a number "and a half".
                let w = if out.is_empty() { w.trim_start_matches("and ") } else { w };
                out.push(w.to_string());
            }
            k += 1;
        }
    }
    let s = out.join(" ");
    if s.chars().any(|c| c.is_alphabetic()) {
        s
    } else {
        digits(core)
    }
}

/// Spoken text for one word's core (punctuation already split off).
///
/// Recursion stays shallow: signs and `%` are stripped in loops here, and the
/// only re-entry is `dash_number` -> `read_core` on a part that has no dashes.
fn read_core(core: &str, ctx: &Ctx) -> String {
    if !has_digit(core) {
        return core.to_string();
    }
    // Sign or comparison prefixes: -5, −5, ±2, ~5, #1. A run of one sign is said once.
    // Byte positions of the first and last digit: one scan, so the loops stay linear.
    let first_digit = core.bytes().position(|b| b.is_ascii_digit()).unwrap_or(0);
    let last_digit = core.bytes().rposition(|b| b.is_ascii_digit()).unwrap_or(0);
    let mut signs: Vec<&'static str> = Vec::new();
    let mut start = 0;
    while let Some(c) = core[start..].chars().next() {
        let Some(w) = sign_word(c) else { break };
        let next = start + c.len_utf8();
        if next > first_digit || core[next..].starts_with(char::is_alphabetic) {
            break;
        }
        if signs.last() != Some(&w) {
            signs.push(w);
        }
        start = next;
    }
    // Percent suffixes: 8.6%, 5%% (said once).
    let mut end = core.len();
    let mut pct = false;
    while end > last_digit + 1 && core.as_bytes().get(end - 1) == Some(&b'%') {
        pct = true;
        end -= 1;
    }
    let rest = &core[start..end];
    if signs.is_empty() && !pct {
        return read_body(core, ctx);
    }
    let mut out = signs.join(" ");
    if !out.is_empty() {
        out.push(' ');
    }
    out.push_str(&read_body(rest, &plain()));
    if pct {
        out.push_str(" percent");
    }
    out
}

/// Per-unit words after a slash: `$20/hr` -> "per hour".
fn per_unit(u: &str) -> Option<&'static str> {
    Some(match u.to_ascii_lowercase().as_str() {
        "hr" | "hrs" | "h" | "hour" => "hour",
        "mo" | "mos" | "month" => "month",
        "yr" | "yrs" | "year" | "annum" => "year",
        "wk" | "wks" | "week" => "week",
        "day" | "d" => "day",
        "share" | "sh" => "share",
        "oz" | "ounce" => "ounce",
        "lb" | "lbs" | "pound" => "pound",
        "kg" => "kilogram",
        "bbl" | "barrel" => "barrel",
        "gal" | "gallon" => "gallon",
        "ton" | "tonne" | "t" => "ton",
        "kwh" => "kilowatt hour",
        "unit" => "unit",
        "person" => "person",
        "night" => "night",
        _ => return None,
    })
}

/// A currency amount followed by something the plain parse rejects: `$20/hr`,
/// `$9.99/mo`, `$1.5-trillion`, `$10-a-share`. None for anything else, so
/// `$5-$10` still reads as a range. Never recurses.
fn currency_with_tail(cur: &Currency, rest: &str) -> Option<String> {
    let end = rest
        .char_indices()
        .find(|(_, c)| !(c.is_ascii_digit() || *c == '.' || *c == ','))
        .map(|(i, _)| i)?;
    let (amt, suf) = rest.split_at(end);
    let (ip, fp) = number_parts(amt)?;
    let alpha_end = |s: &str| s.find(|c: char| !c.is_alphabetic()).unwrap_or(s.len());
    // A scale glued (`5mm/yr`) or after a dash (`1.5-trillion`).
    let mut scale = None;
    let mut tail = suf;
    let a = alpha_end(suf);
    if a > 0 {
        if let Some(sc) = scale_suffix(&suf[..a]) {
            scale = Some(sc);
            tail = &suf[a..];
        }
    } else if let Some(d) = suf.chars().next().filter(|c| is_dash(*c)) {
        let after = &suf[d.len_utf8()..];
        let a2 = alpha_end(after);
        if let Some(sc) = scale_word(&after[..a2].to_ascii_lowercase()) {
            scale = Some(sc);
            tail = &after[a2..];
        }
    }
    let ok_tail = match tail.chars().next() {
        None | Some('/') => true,
        Some(c) if c.is_alphabetic() => true,
        Some(c) if is_dash(c) => tail[c.len_utf8()..].starts_with(char::is_alphabetic),
        _ => false,
    };
    if !ok_tail {
        return None;
    }
    let money = if tail.starts_with('/') && scale.is_none() {
        // A rate reads the amount as a plain decimal: "nine point nine nine dollars per month".
        let name = if ip == "1" && fp.is_empty() { cur.one } else { cur.many };
        format!("{} {name}", decimal(ip, fp))
    } else {
        say_currency(cur, amt, scale)?
    };
    if tail.is_empty() {
        return Some(money);
    }
    if let Some(u) = tail.strip_prefix('/') {
        let unit = per_unit(u).map(str::to_string).unwrap_or_else(|| generic(u));
        return Some(format!("{money} per {unit}").trim_end().to_string());
    }
    // Text after the amount: words kept as they are, digits read.
    let more = if has_digit(tail) {
        generic(tail)
    } else {
        let w: Vec<&str> = tail.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
        w.join(" ")
    };
    Some(format!("{money} {more}").trim_end().to_string())
}

/// Fiscal and half/quarter codes: "FY25" -> "fiscal twenty five", "1H24" -> "first
/// half twenty four", "3Q24" -> "third quarter twenty four".
fn period_code(core: &str) -> Option<String> {
    let yr = |y: &str| -> Option<String> {
        match y.len() {
            2 if all_digits(y) => Some(two_digit_year(y)),
            4 if all_digits(y) => Some(year(y.parse().ok()?)),
            _ => None,
        }
    };
    if let Some(y) = core.strip_prefix("FY") {
        return Some(format!("fiscal {}", yr(y)?));
    }
    let b = core.as_bytes();
    if b.len() >= 4 && b[0].is_ascii_digit() && matches!(b[1], b'H' | b'Q') {
        let n = (b[0] - b'0') as u64;
        let what = match (b[1], n) {
            (b'H', 1..=2) => "half",
            (b'Q', 1..=4) => "quarter",
            _ => return None,
        };
        return Some(format!("{} {what} {}", ordinal(n), yr(&core[2..])?));
    }
    None
}

/// A bare number with a scale letter: "5.1M" -> "five point one million", "254K",
/// "4M+". K and M from 10 up, with a decimal, a "+", after "per" or before a count
/// noun (a 4K screen stays "four K", the 3M company "three M"); B only with a decimal
/// or before a count noun (a 70B model stays "seventy B").
fn bare_scale(core: &str, ctx: &Ctx) -> Option<String> {
    let (body, plus) = match core.strip_suffix('+') {
        Some(b) => (b, true),
        None => (core, false),
    };
    let (amt, Some(sc)) = amount_and_scale(body)? else { return None };
    let suf = &body[amt.len()..];
    let dec = amt.contains('.');
    let big = dec || plus || ctx.per_prev || ctx.card_hint || amt.replace(',', "").parse::<f64>().is_ok_and(|v| v >= 10.0);
    let ok = match suf {
        "M" | "mn" | "K" | "k" => big,
        "B" | "bn" | "Bn" => dec || suf != "B" || ctx.card_hint,
        _ => false,
    };
    if !ok {
        return None;
    }
    let (ip, fp) = number_parts(amt)?;
    Some(format!("{} {sc}{}", decimal(ip, fp), if plus { " plus" } else { "" }))
}

/// A product model number read in pairs, as people say it: "4090" -> "forty ninety",
/// "7900" -> "seventy nine hundred", "14900K" -> "fourteen nine hundred K",
/// "9950X3D" -> "ninety nine fifty X three D".
fn model_pairs(core: &str) -> Option<String> {
    let end = core.find(|c: char| !c.is_ascii_digit()).unwrap_or(core.len());
    let (num, suf) = core.split_at(end);
    if !(4..=5).contains(&num.len()) || num.starts_with('0') || !suf.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    let (hi, lo) = num.split_at(num.len() - 2);
    let lo_n: u64 = lo.parse().ok()?;
    let mut out = if num.len() == 5 {
        format!("{} {}", integer(&num[..2]), integer(&num[2..]))
    } else if lo_n == 0 {
        format!("{} hundred", integer(hi))
    } else if lo_n < 10 {
        format!("{} oh {}", integer(hi), cardinal(lo_n))
    } else {
        format!("{} {}", integer(hi), cardinal(lo_n))
    };
    for c in suf.chars() {
        out.push(' ');
        out.push_str(&if c.is_ascii_digit() { cardinal(c as u64 - '0' as u64) } else { c.to_ascii_uppercase().to_string() });
    }
    Some(out)
}

/// "401(k)" -> "four oh one K"; "529" plans and "457(b)" alike.
fn plan_name(w: &str) -> Option<String> {
    let w = w.trim_end_matches([',', '.', ';', ':']);
    let (num, rest) = w.split_once('(')?;
    let letter = rest.strip_suffix(')')?;
    if num.len() != 3 || !all_digits(num) || letter.len() != 1 || !letter.chars().all(|c| c.is_ascii_lowercase()) {
        return None;
    }
    let d: Vec<u64> = num.bytes().map(|b| (b - b'0') as u64).collect();
    let tail = if d[1] == 0 { format!("oh {}", cardinal(d[2])) } else { cardinal(d[1] * 10 + d[2]) };
    Some(format!("{} {tail} {}", cardinal(d[0]), letter.to_ascii_uppercase()))
}

/// Brands whose model numbers are read in pairs (`model_pairs`).
static MODEL_PREFIX: [&str; 6] = ["GT", "GTX", "MX", "RTX", "RX", "Threadripper"];

/// `read_core` after the sign and percent handling.
fn read_body(core: &str, ctx: &Ctx) -> String {
    if !has_digit(core) {
        return core.to_string();
    }
    if let Some(s) = decade(core) {
        return s;
    }
    if let Some((cur, rest)) = currency_prefix(core) {
        if let Some(s) = amount_and_scale(rest).and_then(|(a, sc)| say_currency(cur, a, sc)) {
            return s;
        }
        if let Some(s) = times_ten(rest) {
            return format!("{s} {}", cur.many);
        }
        if let Some(s) = currency_with_tail(cur, rest) {
            return s;
        }
    }
    if let Some(p) = strip_ordinal_suffix(core) {
        return ordinalize(&integer(p));
    }
    if core.contains(':') {
        if let Some(s) = colon_number(core) {
            return s;
        }
    }
    if let Some((body, ap)) = split_ampm(core) {
        if let Some(h) = body.parse::<u64>().ok().filter(|h| (1..=12).contains(h)) {
            return time_words(h, 0, Some(ap));
        }
    }
    if let Some(s) = version(core) {
        return s;
    }
    if let Some(s) = times_ten(core) {
        return s;
    }
    if let Some(s) = power(core) {
        return s;
    }
    if let Some(s) = square_unit(core, ctx.after_one) {
        return s;
    }
    if let Some(s) = model_size(core, ctx) {
        return s;
    }
    if let Some(s) = period_code(core) {
        return s;
    }
    if let Some(s) = bare_scale(core, ctx) {
        return s;
    }
    if core.contains('/') {
        // `GeV/c2`, `MeV/c²`: a mass unit
        if let Some((unit, "c2" | "c²")) = core.split_once('/') {
            if !unit.is_empty() && unit.chars().all(|c| c.is_ascii_alphabetic()) {
                return format!("{unit} per c squared");
            }
        }
        if let Some(s) = slash_number(core, ctx) {
            return s;
        }
    }
    // "3-3/4" (a whole number and a fraction joined by a hyphen)
    if let Some((w, f)) = core.split_once('-') {
        if let (true, Some((a, b))) = ((1..=3).contains(&w.len()) && all_digits(w), common_fraction(f)) {
            if f.contains('/') {
                return format!("{} and {}", integer(w), mixed_tail(a, b));
            }
        }
    }
    // "49ers", "76ers": the team names
    if let Some(n) = core.strip_suffix("ers").filter(|n| !n.is_empty() && all_digits(n) && n.len() <= 3) {
        let words = integer(n);
        return if words.ends_with('e') { format!("{words}rs") } else { format!("{words}ers") };
    }
    if let Some(s) = dash_number(core, ctx) {
        return s;
    }
    if ctx.allow_year && is_year_num(core) && !ctx.card_hint {
        if let Ok(n) = core.parse::<u32>() {
            let default_year = (1100..=2099).contains(&n) && !ctx.other_digits;
            if ctx.year_hint || default_year {
                return year(n);
            }
        }
    }
    if let Some(s) = plain_number(core) {
        return s;
    }
    if core.split('.').all(all_digits) {
        // 1.2.3
        let words: Vec<String> = core.split('.').map(integer).collect();
        return words.join(" point ");
    }
    if let Some(s) = number_unit(core) {
        return s;
    }
    generic(core)
}

/// A clock time followed by a separate am/pm word ("7:30" + "p.m.", "7" + "pm").
fn time_before_ampm(core: &str, ap: &str) -> Option<String> {
    if let Some(h) = core.parse::<u64>().ok().filter(|h| (1..=12).contains(h)) {
        if all_digits(core) {
            return Some(time_words(h, 0, Some(ap)));
        }
    }
    clock(core, Some(ap))
}

/// The span starting at word `i`: (end, spoken).
fn span_at(words: &[&str], i: usize) -> Option<(usize, String)> {
    let (lead, core, trail) = split_punct(words.get(i)?);
    let next = words.get(i + 1).map(|w| split_punct(w));

    // Month name + day: "Sept. 5", "March 14th,".
    if let Some(m) = month_of(core) {
        if let (Some((l2, c2, t2)), true) = (next, trail.is_empty() || trail == ".") {
            if let (Some(d), true) = (day_of(c2), l2.is_empty()) {
                let name = MONTHS[(m - 1) as usize];
                return Some((i + 2, format!("{lead}{name} {}{t2}", ordinal(d))));
            }
        }
        return None;
    }
    if !has_digit(core) && vulgar(core).is_none() {
        return None;
    }
    // "45 min\u{2013}1 hr": "minutes to one"
    if let Some(s) = crate::ra_norm_text::unit_range(core) {
        return Some((i + 1, format!("{lead}{s}{trail}")));
    }
    // inches written as a double quote after a number ("1-3\"", "5\".") when no quote
    // opened earlier in the chunk
    if trail.starts_with('"') && core.ends_with(|c: char| c.is_ascii_digit()) && !lead.contains('"') {
        let opened = words[..i].iter().any(|w| w.contains(['"', '\u{201C}']));
        if !opened {
            let ctx = build_ctx(words, i, lead, "");
            let body = read_core(core, &ctx);
            let unit = if core == "1" { "inch" } else { "inches" };
            return Some((i + 1, format!("{lead}{body} {unit}{}", &trail[1..])));
        }
    }
    // "401(k)", "457(b)"
    if let Some(p) = plan_name(words.get(i)?) {
        let (_, _, t) = split_punct(words.get(i)?);
        let t = t.trim_start_matches(')');
        return Some((i + 1, format!("{lead}{p}{t}")));
    }
    // product model numbers: "RTX 4090", "Ryzen 9 9950X", "Core i9-14900K"
    let prev = if i >= 1 { split_punct(words[i - 1]).1 } else { "" };
    let prev2 = if i >= 2 { split_punct(words[i - 2]).1 } else { "" };
    let after_brand = MODEL_PREFIX.contains(&prev) || (prev2 == "Ryzen" && prev.len() == 1 && all_digits(prev));
    if after_brand {
        if let Some(m) = model_pairs(core) {
            return Some((i + 1, format!("{lead}{m}{trail}")));
        }
    }
    if let Some((cpu, num)) = core.split_once('-') {
        if cpu.len() == 2 && cpu.starts_with('i') && cpu[1..].bytes().all(|b| b.is_ascii_digit()) {
            if let Some(m) = model_pairs(num) {
                return Some((i + 1, format!("{lead}I {} {m}{trail}", cardinal((cpu.as_bytes()[1] - b'0') as u64))));
            }
        }
    }
    if let Some((l2, c2, t2)) = next {
        // A whole number and a fraction: "2 1/2", "1 ½" -> "two and a half".
        let whole = (1..=3).contains(&core.len()) && all_digits(core) && trail.is_empty() && l2.is_empty();
        if let (true, Some((a, b))) = (whole, common_fraction(c2).filter(|_| whole)) {
            let n = core.parse::<u64>().unwrap_or(0);
            return Some((i + 2, format!("{lead}{} and {}{t2}", cardinal(n), mixed_tail(a, b))));
        }
        // An area code + a local number: "(555) 123-4567".
        let local = c2.split_once('-').filter(|(a, b)| {
            a.len() == 3 && b.len() == 4 && all_digits(a) && all_digits(b)
        });
        if let (Some((a, b)), true, true) = (local, lead == "(" && trail == ")", core.len() == 3) {
            if all_digits(core) && l2.is_empty() {
                let d = format!("({}) {}, {}{t2}", digits(core), digits(a), digits(b));
                return Some((i + 2, d));
            }
        }
        let glued = trail.is_empty() && l2.is_empty();
        // Day + full month name: "5 March" -> "the fifth of" (the month stays a word).
        if let (Some(d), true) = (day_of(core), glued && c2.len() > 3 && month_of(c2).is_some()) {
            return Some((i + 1, format!("{lead}the {} of", ordinal(d))));
        }
        // A fraction before a unit or "of": "13/16 inch", "3/4 cup", "2/3 of".
        if let (Some((a, b)), true, true) = (common_fraction(core), glued, lead.is_empty()) {
            let key = c2.to_ascii_lowercase();
            if key == "of" {
                return Some((i + 1, fraction_words(a, b)));
            }
            // a bare "in" is a unit only before a lowercase word ("3/8 in thick", not "5/10 in 2024")
            let bare_in = (key == "in" && t2.is_empty()) || (key == "c" && !t2.starts_with('.'));
            let in_ok = !bare_in
                || words.get(i + 2).is_some_and(|w| {
                    let c = split_punct(w).1;
                    c.chars().next().is_some_and(char::is_lowercase)
                        && !matches!(c, "the" | "a" | "an" | "my" | "his" | "her" | "its" | "our" | "their" | "this" | "that")
                });
            if let (Ok(k), true) = (FRACTION_UNITS.binary_search_by(|e| e.0.cmp(key.as_str())), in_ok) {
                let unit = FRACTION_UNITS[k].1;
                // an abbreviation's period ("in.") ends the sentence only before a capital or nothing
                let abbr = !c2.eq_ignore_ascii_case(unit) && !key.starts_with(unit);
                let next_lower = words
                    .get(i + 2)
                    .and_then(|w| split_punct(w).1.chars().next())
                    .is_some_and(|c| !c.is_uppercase());
                let t2 = if abbr && next_lower { t2.strip_prefix('.').unwrap_or(t2) } else { t2 };
                return Some((i + 2, format!("{}{t2}", fraction_of_unit(a, b, unit))));
            }
        }
        // Currency amount + a scale word: "$5 billion", "€18.5 million".
        if let (Some((cur, rest)), true) = (currency_prefix(core), glued) {
            if let (Some((amt, None)), Some(sc)) =
                (amount_and_scale(rest), scale_word(&c2.to_ascii_lowercase()))
            {
                if let Some(s) = say_currency(cur, amt, Some(sc)) {
                    return Some((i + 2, format!("{lead}{s}{t2}")));
                }
            }
        }
        // A time + a separate am/pm word: "7:30 p.m.", "7 pm".
        if let (Some(ap), true) = (ampm(c2), glued) {
            if let Some(t) = time_before_ampm(core, ap) {
                // "p.m." keeps its period only when it also ends the sentence.
                let next_lower = words
                    .get(i + 2)
                    .and_then(|w| split_punct(w).1.chars().next())
                    .is_some_and(|c| c.is_lowercase());
                // "3 p.m. ET": a time zone carries on the phrase
                let zone = words.get(i + 2).is_some_and(|w| crate::ra_text::is_time_zone(w));
                let t2 = if next_lower || zone { t2.strip_prefix('.').unwrap_or(t2) } else { t2 };
                return Some((i + 2, format!("{lead}{t}{t2}")));
            }
        }
    }
    if !has_digit(core) {
        return None; // a lone vulgar fraction: the text pass says it
    }
    let ctx = build_ctx(words, i, lead, trail);
    let body = read_core(core, &ctx);
    Some((i + 1, format!("{lead}{body}{trail}")))
}

/// Number spans over source words: (start, end exclusive, spoken). Sorted and
/// non-overlapping. Every word with an ASCII digit is covered by a non-empty span.
pub fn num_spans(words: &[&str]) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < words.len() {
        // a URL or email with digits is the text pass's ("example.org/2024/ai-update")
        if words[i].contains("://") || words[i].contains("www.") || words[i].contains('@') {
            i += 1;
            continue;
        }
        match span_at(words, i) {
            Some((end, spoken)) if end > i && end <= words.len() && !spoken.trim().is_empty() => {
                out.push((i, end, spoken));
                i = end;
            }
            _ => {
                if has_digit(words[i]) {
                    // Guard: a digit token never goes silent.
                    out.push((i, i + 1, digits(words[i])));
                }
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spans for a whitespace-split sentence.
    fn spans(s: &str) -> Vec<(usize, usize, String)> {
        let w: Vec<&str> = s.split_whitespace().collect();
        num_spans(&w)
    }

    /// The spoken text of a one-word input (panics in tests if no span).
    fn one(s: &str) -> String {
        let v = num_spans(&[s]);
        assert_eq!(v.len(), 1, "expected one span for {s:?}, got {v:?}");
        assert_eq!((v[0].0, v[0].1), (0, 1), "{s:?}");
        v[0].2.clone()
    }

    /// The spoken text of the span starting at word `start` in sentence `s`.
    fn at(s: &str, start: usize) -> (usize, String) {
        let v = spans(s);
        let sp = v.iter().find(|x| x.0 == start);
        let sp = sp.unwrap_or_else(|| panic!("no span at {start} in {s:?}: {v:?}"));
        (sp.1, sp.2.clone())
    }

    // ---- §3: years and decades ----
    #[test]
    fn year_standalone_2007() {
        assert_eq!(one("2007"), "two thousand seven");
    }

    #[test]
    fn years_1990_1683_2025() {
        assert_eq!(one("1990"), "nineteen ninety");
        assert_eq!(one("1683"), "sixteen eighty three");
        assert_eq!(one("2025"), "twenty twenty five");
    }

    #[test]
    fn decades() {
        assert_eq!(one("1990s"), "nineteen nineties");
        assert_eq!(one("'90s"), "nineties");
        assert_eq!(one("’90s"), "nineties");
        assert_eq!(one("1900s"), "nineteen hundreds");
        assert_eq!(one("80s"), "eighties");
    }

    // ---- §3: decimals and cardinals ----
    #[test]
    fn decimals() {
        assert_eq!(one("2.5"), "two point five");
        assert_eq!(one("0.35"), "zero point three five");
    }

    #[test]
    fn cardinal_with_commas() {
        assert_eq!(
            one("7,428,973"),
            "seven million four hundred twenty eight thousand nine hundred seventy three"
        );
    }

    // ---- §3: currency (all six) ----
    #[test]
    fn currency_dollars_480() {
        assert_eq!(one("$480"), "four hundred eighty dollars");
    }

    #[test]
    fn currency_scale_suffix_b() {
        assert_eq!(one("$3.2B"), "three point two billion dollars");
    }

    #[test]
    fn currency_scale_word_follows() {
        assert_eq!(spans("$5 billion"), vec![(0, 2, "five billion dollars".to_string())]);
    }

    #[test]
    fn currency_us_dollars() {
        assert_eq!(one("US$32"), "thirty two U S dollars");
    }

    #[test]
    fn currency_euros_million_word() {
        assert_eq!(
            spans("€18.5 million"),
            vec![(0, 2, "eighteen point five million euros".to_string())]
        );
    }

    #[test]
    fn currency_pounds_m_suffix() {
        assert_eq!(one("£149m"), "one hundred forty nine million pounds");
    }

    #[test]
    fn currency_extras() {
        assert_eq!(one("C$19.4B"), "nineteen point four billion Canadian dollars");
        assert_eq!(one("$1"), "one dollar");
        assert_eq!(one("$4.50"), "four dollars fifty cents");
        assert_eq!(one("¥500"), "five hundred yen");
        assert_eq!(one("₹20"), "twenty rupees");
        assert_eq!(one("$10bn"), "ten billion dollars");
        assert_eq!(one("$2K"), "two thousand dollars");
    }

    #[test]
    fn plain_scale_letter_is_a_letter() {
        // B is billion only with a currency amount: a vitamin or a pencil stays B.
        assert_eq!(one("3B"), "three B");
    }

    #[test]
    fn lone_dollar_sign_is_not_a_number_span() {
        assert!(num_spans(&["$"]).is_empty());
        assert!(spans("costs $ more").is_empty());
    }

    // ---- §3: percent ----
    #[test]
    fn percent() {
        assert_eq!(one("8.6%"), "eight point six percent");
    }

    // ---- §3: ordinals ----
    #[test]
    fn ordinals() {
        assert_eq!(one("21st"), "twenty first");
        assert_eq!(one("2nd"), "second");
        assert_eq!(one("30th-largest"), "thirtieth largest");
    }

    // ---- §3: times ----
    #[test]
    fn times() {
        assert_eq!(one("7:30pm"), "seven thirty P M");
        assert_eq!(one("22:37"), "twenty two thirty seven");
        assert_eq!(spans("at 7:30 p.m. today")[0], (1, 3, "seven thirty P M".to_string()));
        assert_eq!(one("7pm"), "seven P M");
        assert_eq!(one("9:05"), "nine oh five");
    }

    // ---- §3: dates (all three) ----
    #[test]
    fn date_month_day_no_year() {
        assert_eq!(one("3/14"), "March fourteenth");
    }

    #[test]
    fn date_month_day_year() {
        assert_eq!(one("3/14/2025"), "March fourteenth, twenty twenty five");
    }

    #[test]
    fn date_iso() {
        assert_eq!(one("2017-09-25"), "September twenty fifth, twenty seventeen");
    }

    #[test]
    fn date_month_word_then_day() {
        assert_eq!(at("on Sept. 5 the", 1), (3, "September fifth".to_string()));
        assert_eq!(at("on March 14th, 2025 we", 1), (3, "March fourteenth,".to_string()));
        assert_eq!(at("on March 14th, 2025 we", 3), (4, "twenty twenty five".to_string()));
    }

    // ---- §3: ranges and scores ----
    #[test]
    fn score() {
        assert_eq!(one("24-17"), "twenty four to seventeen");
    }

    #[test]
    fn year_range_en_dash() {
        assert_eq!(one("2019–2021"), "twenty nineteen to twenty twenty one");
        assert_eq!(one("2019-2021"), "twenty nineteen to twenty twenty one");
    }

    // ---- Review Focus 1: year vs quantity ----
    #[test]
    fn in_1500_is_a_year() {
        assert_eq!(at("In 1500 the city fell", 1), (2, "fifteen hundred".to_string()));
    }

    #[test]
    fn fifteen_hundred_workers_is_a_cardinal() {
        assert_eq!(at("1500 workers", 0), (1, "one thousand five hundred".to_string()));
    }

    #[test]
    fn since_1990_year_page_2025_cardinal() {
        assert_eq!(at("since 1990", 1), (2, "nineteen ninety".to_string()));
        assert_eq!(at("page 2025 of", 1), (2, "two thousand twenty five".to_string()));
    }

    #[test]
    fn other_digits_in_sentence_make_it_a_cardinal() {
        // Spec §3 default: a lone 1100-2099 is a year only when the sentence has no other digits.
        assert_eq!(at("Dial 1200 then 3 more.", 1), (2, "one thousand two hundred".to_string()));
        assert_eq!(at("The 1990 election.", 1), (2, "nineteen ninety".to_string()));
    }

    // ---- extra edge cases from the brief ----
    #[test]
    fn negatives() {
        assert_eq!(one("-5"), "minus five");
        assert_eq!(one("−5"), "minus five");
    }

    #[test]
    fn fractions() {
        assert_eq!(one("1/2"), "one half");
        assert_eq!(one("3/4"), "three quarters");
        assert_eq!(one("2/3"), "two thirds");
    }

    /// Round 2: a fraction before a unit or "of" reads as a fraction, with
    /// "of a/an <unit>" for a unit; any denominator up to 64.
    #[test]
    fn fractions_before_a_unit_or_of() {
        let t = |s: &str, start: usize| at(s, start);
        assert_eq!(t("a 13/16 inch bolt", 1), (3, "thirteen sixteenths of an inch".to_string()));
        assert_eq!(t("cut 3/8 in. thick", 1), (3, "three eighths of an inch".to_string()));
        assert_eq!(t("add 3/4 cup sugar", 1), (3, "three quarters of a cup".to_string()));
        assert_eq!(t("add 3/4 cups sugar", 1), (3, "three quarters of a cup".to_string()));
        assert_eq!(t("add 1/2 tsp salt", 1), (3, "half a teaspoon".to_string()));
        assert_eq!(t("about 1/2 inch.", 1), (3, "half an inch.".to_string()));
        assert_eq!(t("a 5/32 mile stretch", 1), (3, "five thirty seconds of a mile".to_string()));
        assert_eq!(t("only 7/64 oz", 1), (3, "seven sixty fourths of an ounce".to_string()));
        assert_eq!(t("won 2/3 of the vote", 1), (2, "two thirds".to_string()));
        assert_eq!(t("about 11/16 of it", 1), (2, "eleven sixteenths".to_string()));
        assert!(FRACTION_UNITS.windows(2).all(|p| p[0].0 < p[1].0), "FRACTION_UNITS sorted");
        // a bare "in" before a year or a name is the preposition
        assert_eq!(t("due 5/10 in 2024", 1), (2, "May tenth".to_string()));
        // without a unit or "of", unchanged
        assert_eq!(one("13/16"), "thirteen sixteen");
        assert_eq!(one("3/4"), "three quarters");
    }

    /// Round 2: a whole number and a fraction ("2 1/2 cups", "1 ½ cups") is one
    /// mixed number; a lone vulgar fraction before a unit is "half a cup".
    #[test]
    fn mixed_numbers_and_vulgar_fractions() {
        assert_eq!(at("and 2 1/2 cups of milk", 1), (3, "two and a half".to_string()));
        assert_eq!(at("add 1 ½ cups flour", 1), (3, "one and a half".to_string()));
        assert_eq!(at("a 2 3/4 inch gap", 1), (3, "two and three quarters".to_string()));
        assert_eq!(at("about 3 1/3 miles", 1), (3, "three and a third".to_string()));
        assert_eq!(at("then 1 ⅛ tsp salt", 1), (3, "one and an eighth".to_string()));
        assert_eq!(at("Pour in ½ cup of milk.", 2), (4, "half a cup".to_string()));
        assert_eq!(at("add ¾ cup sugar", 1), (3, "three quarters of a cup".to_string()));
        // not a mixed number
        assert_eq!(at("won 3 1/2 of", 1), (3, "three and a half".to_string()));
        assert_eq!(at("in 2024 1/2 the votes", 1), (2, "twenty twenty four".to_string()));
    }

    /// Round 2: a day above 12 reads as a day-month date (UK); US month/day stays the default.
    #[test]
    fn day_month_dates_when_the_day_is_above_twelve() {
        assert_eq!(one("25/12"), "the twenty fifth of December");
        assert_eq!(one("31/1"), "the thirty first of January");
        assert_eq!(one("25/12/2025"), "the twenty fifth of December, twenty twenty five");
        assert_eq!(one("13/04/24"), "the thirteenth of April, twenty four");
        // US default
        assert_eq!(one("12/10"), "December tenth");
        assert_eq!(one("3/14"), "March fourteenth");
        assert_eq!(one("3/14/2025"), "March fourteenth, twenty twenty five");
        // not dates
        assert_eq!(one("60/40"), "sixty forty");
        assert_eq!(one("20/20"), "twenty twenty");
    }

    #[test]
    fn ratings_out_of_ten_or_five() {
        assert_eq!(one("10/10"), "ten out of ten");
        assert_eq!(one("9/10"), "nine out of ten");
        assert_eq!(at("I give it 10/10 honestly", 3), (4, "ten out of ten".to_string()));
        assert_eq!(at("rated 4/5 by critics", 1), (2, "four out of five".to_string()));
        assert_eq!(at("a solid 5/5.", 2), (3, "five out of five.".to_string()));
        // a date context keeps the date
        assert_eq!(at("on 10/10 we met", 1), (2, "October tenth".to_string()));
        assert_eq!(at("due 5/10 in 2024 at noon", 1), (2, "May tenth".to_string()));
        assert_eq!(at("the March 4/5 sessions", 2), (3, "April fifth".to_string()));
        // other fractions and dates are unchanged
        assert_eq!(one("3/14"), "March fourteenth");
        assert_eq!(one("1/2"), "one half");
        assert_eq!(one("2/3"), "two thirds");
        assert_eq!(one("12/10"), "December tenth");
    }

    #[test]
    fn blood_pressure_reads_over() {
        assert_eq!(at("BP was 120/80 mmHg today", 2), (3, "one hundred twenty over eighty".to_string()));
        assert_eq!(at("a 140/90 blood pressure", 1), (2, "one hundred forty over ninety".to_string()));
        assert_eq!(at("his blood pressure was 90/60 at rest", 4), (5, "ninety over sixty".to_string()));
        assert_eq!(one("120/80"), "one hundred twenty over eighty");
        // a short pair that is a real date stays a date
        assert_eq!(one("11/5"), "November fifth");
        // first number smaller: not a reading
        assert_eq!(one("80/120"), "eighty slash one hundred twenty");
        // a 60/40 split is not a pressure without a cue
        assert!(!one("60/40").contains("over"), "{}", one("60/40"));
        assert!(!at("Maybe 60/40.", 1).1.contains("over"));
    }

    /// Round 1: a split with no cue (60/40, 70/30) is two plain numbers, not "slash".
    #[test]
    fn split_pairs_read_as_two_numbers() {
        assert_eq!(one("60/40"), "sixty forty");
        assert_eq!(one("70/30"), "seventy thirty");
        assert_eq!(one("80/20"), "eighty twenty");
        assert_eq!(one("55/45"), "fifty five forty five");
        assert_eq!(at("Maybe 60/40.", 1).1, "sixty forty.");
        // the other readings are unchanged
        assert_eq!(one("50/50"), "fifty fifty");
        assert_eq!(one("9/10"), "nine out of ten");
        assert_eq!(one("4/5"), "four out of five");
        assert_eq!(one("120/80"), "one hundred twenty over eighty");
        assert_eq!(at("BP was 120/80 mmHg today", 2).1, "one hundred twenty over eighty");
        assert_eq!(one("3/14"), "March fourteenth");
        assert_eq!(one("12/25"), "December twenty fifth");
        assert_eq!(one("1/2"), "one half");
        assert_eq!(one("24/7"), "twenty four seven");
        assert_eq!(one("80/120"), "eighty slash one hundred twenty");
    }

    #[test]
    fn version() {
        assert_eq!(one("v2.1"), "version two point one");
    }

    #[test]
    fn phone_number() {
        assert_eq!(one("555-123-4567"), "five five five, one two three, four five six seven");
    }

    #[test]
    fn sixty_digit_number_reads_digits() {
        let n = "1234567890".repeat(6);
        let s = one(&n);
        assert_eq!(s.split(' ').count(), 60, "{s}");
        assert!(s.starts_with("one two three"));
    }

    #[test]
    fn scientific_e() {
        assert_eq!(one("1e10"), "one E ten");
    }

    #[test]
    fn trailing_punctuation_is_kept_for_the_pause() {
        assert_eq!(one("1990,"), "nineteen ninety,");
        assert_eq!(one("$5."), "five dollars.");
        assert_eq!(one("(2022)"), "(twenty twenty two)");
    }

    #[test]
    fn mixed_tokens() {
        assert_eq!(one("COVID-19"), "COVID nineteen");
        assert_eq!(one("20°C"), "twenty degrees Celsius");
        assert_eq!(one("±2"), "plus or minus two");
    }

    #[test]
    fn every_digit_token_gets_a_nonempty_span() {
        let toks = [
            "0", "1", "42", "1,2", "1.2.3", "3:1", "1:30:45", "5-", "-", "1/0", "0/0", "99/99",
            "12:99", "13/45", "x1", "1x", "4x4", "$1.", "$.5", ".5", "5.", "1.", "%5", "5%%",
            "1,000,", "'9", "'99", "9'", "#1", "1st-", "2k", "3B", "10-K", "USB4", "H1N1", "5🙂",
            "東京5", "1e", "e1", "1e10e", "$$5", "US$", "5€", "1-800-555-1234", "2017-13-45",
            "--5", "+-5", "0x1F", "1_000", "١٢", "５", "1½", "9999999999999999999999", "000",
            "007", "1.0e-5", "2nd.", "(5)", "[1]", "\"7\"", "“8”", "3/14/", "/3", "3//4",
        ];
        for t in toks {
            let v = num_spans(&[t]);
            if t.chars().any(|c| c.is_ascii_digit()) {
                assert_eq!(v.len(), 1, "{t:?} -> {v:?}");
                assert!(!v[0].2.trim().is_empty(), "{t:?} -> empty");
                assert!(v[0].2.chars().any(|c| c.is_alphabetic()), "{t:?} -> {:?}", v[0].2);
            }
        }
    }

    #[test]
    fn no_panic_on_emoji_and_cjk_neighbours() {
        let s = ["🙂", "東京", "2025", "年", "🎉🎉", "$", "€", "½", "", " ", "١٢٣"];
        let v = num_spans(&s);
        assert!(v.iter().any(|x| x.0 == 2), "{v:?}");
        let _ = num_spans(&[]);
        let _ = num_spans(&["🙂"]);
    }

    #[test]
    fn spans_sorted_and_disjoint() {
        let s = "On March 5, 2025 the $3.2B deal for 1500 workers closed at 7:30 p.m. and 8.6% rose 24-17 in 2019–2021 since 1990s";
        let v = spans(s);
        let n = s.split_whitespace().count();
        let mut last = 0;
        for (a, b, t) in &v {
            assert!(a < b && *b <= n, "{v:?}");
            assert!(*a >= last, "{v:?}");
            assert!(!t.is_empty());
            last = *b;
        }
    }

    #[test]
    fn output_style_lowercase_no_hyphens() {
        for t in ["21st", "1683", "7,428,973", "$3.2B", "24-17", "2017-09-25", "30th-largest"] {
            let s = one(t);
            assert!(!s.contains('-'), "{t} -> {s}");
            assert!(!s.contains("  "), "{t} -> {s}");
            assert!(!s.contains(" and "), "{t} -> {s}");
        }
    }

    #[test]
    fn lookup_tables_are_sorted() {
        fn sorted(t: &[&str]) -> bool {
            t.windows(2).all(|w| w[0] < w[1])
        }
        assert!(sorted(&YEAR_PREV));
        assert!(sorted(&CARD_PREV));
        assert!(sorted(&COUNT_NEXT));
        let keys: Vec<&str> = MONTH_KEYS.iter().map(|(k, _)| *k).collect();
        assert!(sorted(&keys));
    }

    #[test]
    fn more_context_cases() {
        // day before a month, then the year after the month
        assert_eq!(at("on 5 March 2025 we", 1), (2, "the fifth of".to_string()));
        assert_eq!(at("on 5 March 2025 we", 3), (4, "twenty twenty five".to_string()));
        // a year range in words
        assert_eq!(at("from 2019 to 2021 we", 1), (2, "twenty nineteen".to_string()));
        assert_eq!(at("from 2019 to 2021 we", 3), (4, "twenty twenty one".to_string()));
        // a count noun beats "in"
        assert_eq!(at("in 1500 years", 1), (2, "one thousand five hundred".to_string()));
        assert_eq!(one("$0.50"), "fifty cents");
        assert_eq!(one("4x4"), "four by four");
        assert_eq!(one("10x"), "ten times");
        assert_eq!(one("1/0"), "one slash zero");
        assert_eq!(one("24/7"), "twenty four seven");
        assert_eq!(at("on 3/4 we", 1), (2, "March fourth".to_string()));
        assert_eq!(one("2019–21"), "twenty nineteen to twenty one");
        assert_eq!(one("12:00"), "twelve o'clock");
        assert_eq!(at("closes at 7:30 p.m.", 2), (4, "seven thirty P M.".to_string()));
        assert_eq!(at("at 7 pm", 1), (3, "seven P M".to_string()));
        assert_eq!(one("V8"), "V eight");
        assert_eq!(one("5-10%"), "five to ten percent");
    }

    #[test]
    fn ordinals_and_codes_are_not_other_quantities() {
        let s = "The 2007 crisis, the 21st century, and the 2nd quarter of 2020.";
        assert_eq!(at(s, 11), (12, "twenty twenty.".to_string()));
        assert_eq!(at("rose 12% in Q3 2023.", 4), (5, "twenty twenty three.".to_string()));
    }

    #[test]
    fn area_code_phone_number() {
        assert_eq!(
            spans("call (555) 123-4567 now"),
            vec![(1, 3, "(five five five) one two three, four five six seven".to_string())]
        );
    }

    #[test]
    fn bit_rates() {
        assert_eq!(one("40Gbps"), "forty gigabits per second");
        assert_eq!(one("100Mbps"), "one hundred megabits per second");
    }

    #[test]
    fn brute_force_short_tokens_never_panic_and_never_go_silent() {
        let alphabet: Vec<char> = "0159$€.,:/-–%'’smpBkx(°C)🙂東½−±".chars().collect();
        let mut tok = String::new();
        let n = alphabet.len();
        for a in 0..n {
            for b in 0..=n {
                for c in 0..=n {
                    tok.clear();
                    tok.push(alphabet[a]);
                    if b < n {
                        tok.push(alphabet[b]);
                    }
                    if c < n {
                        tok.push(alphabet[c]);
                    }
                    let v = num_spans(&[tok.as_str(), "March", tok.as_str(), "pm"]);
                    let mut last = 0;
                    for (s, e, t) in &v {
                        assert!(s < e && *e <= 4 && *s >= last, "{tok:?} {v:?}");
                        assert!(t.chars().any(|c| c.is_alphabetic()), "{tok:?} {v:?}");
                        last = *e;
                    }
                    if tok.chars().any(|c| c.is_ascii_digit()) {
                        assert!(v.iter().any(|x| x.0 == 0), "{tok:?} {v:?}");
                    }
                }
            }
        }
    }

    // ---- review fix round 1 ----
    #[test]
    fn many_signs_do_not_overflow_the_stack() {
        let t = format!("{}5", "-".repeat(50_000));
        let v = num_spans(&[&t]);
        assert_eq!(v.len(), 1);
        assert!(v[0].2.ends_with("five"), "{}", &v[0].2[..40.min(v[0].2.len())]);
        let t = format!("5{}", "%".repeat(50_000));
        let v = num_spans(&[&t]);
        assert_eq!(v.len(), 1);
        assert!(v[0].2.starts_with("five percent"));
        let t = format!("{}5", "−±".repeat(25_000));
        assert_eq!(num_spans(&[&t]).len(), 1);
        let t = format!("{}5{}-5{}", "+".repeat(30_000), "%".repeat(30_000), "%".repeat(30_000));
        assert_eq!(num_spans(&[&t]).len(), 1);
        let t = "$5".repeat(30_000);
        assert_eq!(num_spans(&[&t]).len(), 1);
    }

    #[test]
    fn currency_before_a_slash_or_text() {
        assert_eq!(one("$20/hr"), "twenty dollars per hour");
        assert_eq!(one("$9.99/mo"), "nine point nine nine dollars per month");
        assert_eq!(one("$5mm"), "five million dollars");
        assert_eq!(one("$1.5-trillion"), "one point five trillion dollars");
        assert_eq!(one("$1/yr"), "one dollar per year");
        assert_eq!(one("€30/day"), "thirty euros per day");
        assert_eq!(one("$4/share"), "four dollars per share");
        assert_eq!(one("$10-a-share"), "ten dollars a share");
        // still ranges
        assert_eq!(one("$5-$10"), "five dollars to ten dollars");
        // and plain mm is still millimeters
        assert_eq!(one("5mm"), "five millimeters");
    }

    #[test]
    fn adjective_then_count_noun_is_a_quantity() {
        assert_eq!(at("2050 new jobs", 0), (1, "two thousand fifty".to_string()));
        assert_eq!(at("1683 new cases", 0), (1, "one thousand six hundred eighty three".to_string()));
        assert_eq!(at("about 1990 more workers", 1).1, "one thousand nine hundred ninety");
        // an explicit year cue still wins
        assert_eq!(at("In 2020 new cases rose", 1), (2, "twenty twenty".to_string()));
        // an adjective alone is not enough
        assert_eq!(at("The 2050 new plan", 1), (2, "twenty fifty".to_string()));
    }

    #[test]
    fn label_words_make_a_cardinal() {
        for w in [
            "suite", "apt", "unit", "gate", "bus", "ext", "flight", "route", "highway",
            "interstate", "channel", "model", "version",
        ] {
            let s = format!("{w} 1500");
            assert_eq!(at(&s, 1).1, "one thousand five hundred", "{s}");
        }
    }

    // ---- Task 6 fix rounds ----
    #[test]
    fn year_after_a_name_or_a_determiner_beats_other_digits() {
        // a year right after a month and day, and a product year after a name
        let s = "On September 4, 2012, the team shipped Windows Server 2012.";
        assert_eq!(at(s, 3).1, "twenty twelve,");
        assert_eq!(at(s, 9).1, "twenty twelve.");
        // a determiner before a year-shaped number, other digits in the sentence
        assert_eq!(at("climbing 11 spots past its 2012 position", 5).1, "twenty twelve");
        assert_eq!(at("She finished 1st out of 22 runners in the 2026 race.", 9).1, "twenty twenty six");
        assert_eq!(at("challenge the 2020 national vote, 4 charges", 2).1, "twenty twenty");
        // "of" and "since about"
        assert_eq!(at("By the close of 1946 the club had signed up 39 members.", 4).1, "nineteen forty six");
        assert_eq!(at("(since about 2019) has found 3 things", 2).1, "twenty nineteen)");
        // quantities stay quantities
        assert_eq!(at("the 1500 workers and 3 more", 1).1, "one thousand five hundred");
        assert_eq!(at("one of 1950 residents and 3 more", 2).1, "one thousand nine hundred fifty");
        assert_eq!(at("Room 1204, Floor 12", 1).1, "one thousand two hundred four,");
        assert_eq!(at("Dial 1200 then 3 more.", 1).1, "one thousand two hundred");
        assert_eq!(at("Four of these are federal, 1261 are local, and 53 are private.", 5).1, "one thousand two hundred sixty one");
    }

    /// Round 2 (round-trip audit): FOMC statements say "by 1/4 percentage point" and
    /// "3-3/4 to 4 percent"; sports say "49ers".
    #[test]
    fn fed_fractions_and_team_numbers() {
        assert_eq!(at("raise the range by 1/4 percentage point", 4).1, "one quarter");
        assert_eq!(at("lower it by 1/2 point today", 3).1, "one half");
        assert_eq!(at("by 1/4 of a point", 1).1, "one quarter");
        assert_eq!(at("on 1/4 we met", 1).1, "January fourth", "a date stays a date");
        assert_eq!(at("to 3-3/4 to 4 percent", 1).1, "three and three quarters");
        assert_eq!(at("to 4-1/2 percent", 1).1, "four and a half");
        assert_eq!(one("49ers"), "forty niners");
        assert_eq!(one("76ers"), "seventy sixers");
    }

    /// Round 2: year cues that beat another number in the sentence.
    #[test]
    fn years_after_for_and_month_of() {
        assert_eq!(at("remain at 2 percent for 2026.", 5).1, "twenty twenty six.");
        assert_eq!(at("In September of 1814, 1,000 patriots", 3).1, "eighteen fourteen,");
        // a count stays a count
        assert_eq!(at("enough for 2026 people and 3 dogs", 2).1, "two thousand twenty six");
    }

    #[test]
    fn model_sizes_say_billion_only_before_ai_nouns() {
        assert_eq!(at("has 405B parameters", 1).1, "four hundred five billion");
        assert_eq!(at("a 7B model with 2T tokens", 4).1, "two trillion");
        assert_eq!(at("a 70B-parameter model", 1).1, "seventy billion parameter");
        // no AI noun: letter name (the spoken style for model names)
        assert_eq!(at("the 70B version", 1).1, "seventy B");
        assert_eq!(at("a 3M company", 1).1, "three M");
    }

    #[test]
    fn greek_letters_inside_a_token_are_named() {
        assert_eq!(one("ε4"), "epsilon four");
        assert_eq!(one("α2"), "alpha two");
        assert_eq!(one("Aβ42"), "A beta forty two");
    }

    #[test]
    fn mass_over_c_squared() {
        assert_eq!(one("GeV/c2"), "GeV per c squared");
        assert_eq!(one("MeV/c2."), "MeV per c squared.");
    }

    #[test]
    fn scientific_notation_with_times_ten() {
        assert_eq!(one("$1.23×1010"), "one point two three times ten to the tenth dollars");
        assert_eq!(one("6.02×1023"), "six point zero two times ten to the twenty third");
        assert_eq!(one("3×10^8"), "three times ten to the eighth");
        // a plain product stays a product
        assert_eq!(one("3×4"), "three times four");
        assert!(!one("2.5x100").contains("to the"), "{}", one("2.5x100"));
    }

    /// Round 1 (Alex): number-pass units are plural after any number but exactly 1.
    #[test]
    fn number_pass_units_are_plural_after_any_number_but_one() {
        let last = |s: &str| spans(s).last().map(|x| x.2.clone()).unwrap_or_default();
        let mut bad = Vec::new();
        for (u, one_u, many) in [
            ("km2", "square kilometer", "square kilometers"),
            ("m2", "square meter", "square meters"),
            ("cm2", "square centimeter", "square centimeters"),
            ("mm2", "square millimeter", "square millimeters"),
            ("mi2", "square mile", "square miles"),
            ("ft2", "square foot", "square feet"),
            ("yd2", "square yard", "square yards"),
            ("m3", "cubic meter", "cubic meters"),
            ("km3", "cubic kilometer", "cubic kilometers"),
        ] {
            for n in ["59,000", "2", "1.5", "1.0", "0.5", "1/2"] {
                let got = last(&format!("{n} {u}"));
                if got != many {
                    bad.push(format!("{n} {u}: {got:?} (want {many:?})"));
                }
            }
            for n in ["1", "one"] {
                let got = last(&format!("{n} {u}"));
                if got != one_u {
                    bad.push(format!("{n} {u}: {got:?} (want {one_u:?})"));
                }
            }
        }
        for (u, one_u, many) in UNITS {
            for n in ["59,000", "2", "1.5", "1.0", "0.5"] {
                let got = one(&format!("{n}{u}"));
                if !got.ends_with(&format!(" {many}")) {
                    bad.push(format!("{n}{u}: {got:?} (want .. {many:?})"));
                }
            }
            let got = one(&format!("1{u}"));
            if got != format!("one {one_u}") {
                bad.push(format!("1{u}: {got:?}"));
            }
        }
        assert!(bad.is_empty(), "{} wrong:\n{}", bad.len(), bad.join("\n"));
    }

    #[test]
    fn powers_and_square_units() {
        assert_eq!(one("2^10"), "two to the tenth");
        assert_eq!(one("2^10="), "two to the tenth equals");
        assert_eq!(one("10^3"), "ten to the third");
        assert_eq!(one("km2)"), "square kilometers)");
        assert_eq!(one("m2"), "square meters");
        assert_eq!(one("m3"), "cubic meters");
        // not a unit: a code stays a code
        assert_eq!(one("A2"), "A two");
    }

    /// cargo test --release ra_norm_num::tests::perf -- --ignored --nocapture
    #[test]
    #[ignore]
    fn perf() {
        let s = "On March 5, 2025 the company said its $3.2B deal for 1500 workers closed at 7:30 p.m. and shares rose 8.6% to $480 after a 24-17 vote in 2019–2021 versus 1990s levels of 7,428,973";
        let w: Vec<&str> = s.split_whitespace().collect();
        assert!(w.len() >= 30);
        let w = &w[..30];
        let n = 10_000;
        let t = std::time::Instant::now();
        let mut total = 0usize;
        for _ in 0..n {
            total += num_spans(std::hint::black_box(w)).len();
        }
        let per = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
        println!("num_spans: {per:.4} ms per 30-word call ({total} spans)");
        assert!(per < 0.5, "{per} ms");
    }
}
