//! Read Aloud: our own number -> English words (no num2words, which is LGPL).
//! Style: lowercase, single spaces, no hyphens, no "and" ("one hundred five").
//! Pure and panic-free.

static ONES: [&str; 20] = [
    "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
    "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen",
    "nineteen",
];
static TENS: [&str; 10] =
    ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"];
/// Scale words by power of 1000 (index 1 = thousand ... 6 = quintillion = 10^18).
static SCALES: [&str; 7] =
    ["", "thousand", "million", "billion", "trillion", "quadrillion", "quintillion"];

/// 1..=999 -> words appended to `out` (nothing for 0).
fn push_under_1000(n: u64, out: &mut Vec<&'static str>) {
    let h = (n / 100) as usize;
    let r = (n % 100) as usize;
    if h > 0 {
        out.push(ONES[h]);
        out.push("hundred");
    }
    if r >= 20 {
        out.push(TENS[r / 10]);
        if r % 10 > 0 {
            out.push(ONES[r % 10]);
        }
    } else if r > 0 {
        out.push(ONES[r]);
    }
}

/// Cardinal words: 7428973 -> "seven million four hundred twenty eight thousand nine hundred seventy three".
pub fn cardinal(n: u64) -> String {
    if n == 0 {
        return ONES[0].to_string();
    }
    // Split into base-1000 groups, most significant first. u64 has at most 7 groups.
    let mut groups = [0u64; 7];
    let mut m = n;
    let mut k = 0;
    while m > 0 && k < groups.len() {
        groups[k] = m % 1000;
        m /= 1000;
        k += 1;
    }
    let mut out: Vec<&'static str> = Vec::with_capacity(16);
    for idx in (0..k).rev() {
        let g = groups[idx];
        if g == 0 {
            continue;
        }
        push_under_1000(g, &mut out);
        if idx > 0 {
            out.push(SCALES[idx]);
        }
    }
    out.join(" ")
}

/// The ordinal form of one cardinal word ("one" -> "first", "twenty" -> "twentieth").
fn ordinal_word(w: &str) -> String {
    match w {
        "one" => "first".into(),
        "two" => "second".into(),
        "three" => "third".into(),
        "five" => "fifth".into(),
        "eight" => "eighth".into(),
        "nine" => "ninth".into(),
        "twelve" => "twelfth".into(),
        _ => match w.strip_suffix('y') {
            Some(stem) => format!("{stem}ieth"),
            None => format!("{w}th"),
        },
    }
}

/// Make the last word of a cardinal phrase ordinal.
pub(crate) fn ordinalize(words: &str) -> String {
    match words.rsplit_once(' ') {
        Some((head, last)) => format!("{head} {}", ordinal_word(last)),
        None => ordinal_word(words),
    }
}

/// Ordinal words: 21 -> "twenty first", 30 -> "thirtieth".
pub fn ordinal(n: u64) -> String {
    ordinalize(&cardinal(n))
}

/// Two-digit pair as said in a year: 5 -> "oh five", 83 -> "eighty three", 0 -> "hundred".
fn year_half(n: u32) -> String {
    match n {
        0 => "hundred".into(),
        1..=9 => format!("oh {}", ONES[n as usize]),
        _ => cardinal(n as u64),
    }
}

/// Year words: 2007 -> "two thousand seven", 2025 -> "twenty twenty five",
/// 1683 -> "sixteen eighty three", 1900 -> "nineteen hundred". Outside 1000..=9999: cardinal.
pub fn year(y: u32) -> String {
    if !(1000..=9999).contains(&y) {
        return cardinal(y as u64);
    }
    let hi = y / 100;
    let lo = y % 100;
    if y % 1000 == 0 || (2000..=2009).contains(&y) || (1000..=1009).contains(&y) {
        // 2000, 2007, 3000, 1005: "two thousand seven"
        return cardinal(y as u64);
    }
    format!("{} {}", cardinal(hi as u64), year_half(lo))
}

/// Each ASCII digit as a word, space separated; everything else is skipped.
pub fn digits(s: &str) -> String {
    let mut out: Vec<&'static str> = Vec::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_digit() {
            out.push(ONES[(b - b'0') as usize]);
        }
    }
    out.join(" ")
}

/// An integer string (ASCII digits, commas ignored) as words. Leading zeros
/// ("007") or anything that doesn't fit u64 is read digit by digit.
pub(crate) fn integer(s: &str) -> String {
    let d: String = s.bytes().filter(|b| b.is_ascii_digit()).map(|b| b as char).collect();
    if d.is_empty() {
        return String::new();
    }
    // leading zeros ("007"), or a long ungrouped run (an ISBN, a DOI, an ID)
    if (d.len() > 1 && d.starts_with('0')) || (d.len() >= 10 && !s.contains(',')) {
        return digits(&d);
    }
    match d.parse::<u64>() {
        Ok(n) => cardinal(n),
        Err(_) => digits(&d),
    }
}

/// "2" + "5" -> "two point five"; the fraction is read digit by digit.
/// An empty integer part gives "point five"; an empty fraction gives the integer.
pub fn decimal(int_part: &str, frac: &str) -> String {
    let i = integer(int_part);
    let f = digits(frac);
    match (i.is_empty(), f.is_empty()) {
        (_, true) => i,
        (true, false) => format!("point {f}"),
        (false, false) => format!("{i} point {f}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_plain_digit_runs_are_read_digit_by_digit() {
        // an ID (ISBN, DOI) is digits; a grouped amount stays a number
        assert_eq!(integer("9780226821665"), "nine seven eight zero two two six eight two one six six five");
        assert_eq!(integer("1234567890"), "one two three four five six seven eight nine zero");
        assert_eq!(integer("123456789"), "one hundred twenty three million four hundred fifty six thousand seven hundred eighty nine");
        assert!(integer("1,234,567,890").starts_with("one billion"));
    }

    #[test]
    fn cardinal_small_and_tens() {
        assert_eq!(cardinal(0), "zero");
        assert_eq!(cardinal(7), "seven");
        assert_eq!(cardinal(13), "thirteen");
        assert_eq!(cardinal(25), "twenty five");
        assert_eq!(cardinal(40), "forty");
        assert_eq!(cardinal(99), "ninety nine");
    }

    #[test]
    fn cardinal_hundreds_no_and() {
        assert_eq!(cardinal(100), "one hundred");
        assert_eq!(cardinal(105), "one hundred five");
        assert_eq!(cardinal(480), "four hundred eighty");
        assert_eq!(cardinal(1500), "one thousand five hundred");
    }

    #[test]
    fn cardinal_scales() {
        assert_eq!(
            cardinal(7_428_973),
            "seven million four hundred twenty eight thousand nine hundred seventy three"
        );
        assert_eq!(cardinal(1_000_000), "one million");
        assert_eq!(cardinal(2_000_000_005), "two billion five");
        assert_eq!(cardinal(1_000_000_000_000_000_000), "one quintillion");
        // u64::MAX must not panic and must be words
        assert!(cardinal(u64::MAX).starts_with("eighteen quintillion"));
    }

    #[test]
    fn ordinals_irregular_and_tieth() {
        assert_eq!(ordinal(1), "first");
        assert_eq!(ordinal(2), "second");
        assert_eq!(ordinal(3), "third");
        assert_eq!(ordinal(5), "fifth");
        assert_eq!(ordinal(8), "eighth");
        assert_eq!(ordinal(9), "ninth");
        assert_eq!(ordinal(12), "twelfth");
        assert_eq!(ordinal(14), "fourteenth");
        assert_eq!(ordinal(21), "twenty first");
        assert_eq!(ordinal(25), "twenty fifth");
        assert_eq!(ordinal(30), "thirtieth");
        assert_eq!(ordinal(100), "one hundredth");
        assert_eq!(ordinal(112), "one hundred twelfth");
    }

    #[test]
    fn years() {
        assert_eq!(year(2007), "two thousand seven");
        assert_eq!(year(2000), "two thousand");
        assert_eq!(year(1990), "nineteen ninety");
        assert_eq!(year(1683), "sixteen eighty three");
        assert_eq!(year(2025), "twenty twenty five");
        assert_eq!(year(2010), "twenty ten");
        assert_eq!(year(2099), "twenty ninety nine");
        assert_eq!(year(1900), "nineteen hundred");
        assert_eq!(year(1500), "fifteen hundred");
        assert_eq!(year(1905), "nineteen oh five");
        assert_eq!(year(1066), "ten sixty six");
        assert_eq!(year(2100), "twenty one hundred");
        assert_eq!(year(476), "four hundred seventy six");
    }

    #[test]
    fn digits_and_decimals() {
        assert_eq!(digits("555"), "five five five");
        assert_eq!(digits("0"), "zero");
        assert_eq!(digits("a1b2"), "one two");
        assert_eq!(digits(""), "");
        assert_eq!(decimal("2", "5"), "two point five");
        assert_eq!(decimal("0", "35"), "zero point three five");
        assert_eq!(decimal("18", "5"), "eighteen point five");
        assert_eq!(decimal("1,234", "05"), "one thousand two hundred thirty four point zero five");
        assert_eq!(decimal("", "5"), "point five");
        assert_eq!(decimal("3", ""), "three");
    }

    #[test]
    fn no_hyphens_anywhere() {
        for n in [21u64, 99, 345, 7_428_973, u64::MAX] {
            assert!(!cardinal(n).contains('-'), "{n}");
            assert!(!ordinal(n).contains('-'), "{n}");
        }
        for y in [1100u32, 1999, 2021, 2099] {
            assert!(!year(y).contains('-'), "{y}");
        }
    }
}
