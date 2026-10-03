//! Read Aloud: pronunciations for words misaki doesn't know (names, brands).
//! Order: acronym rule -> hand list (names_fix.tsv) -> CMUdict (BSD-2) -> CamelCase /
//! compound parts -> optional letter-to-sound model -> spelled letters / digits and
//! symbols read out. Never panics; the misaki fallback is always Ok.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::sync::Arc;

use misaki_rs::fallback::{Fallback, FallbackError};

static CMU_GZ: &[u8] = include_bytes!("../data/cmudict.tsv.gz");
static FIX_TSV: &str = include_str!("../data/names_fix.tsv");

/// ARPAbet (no stress digit) -> misaki/Kokoro symbols. AH/ER depend on stress.
fn arpa_symbol(base: &str, stress: Option<char>) -> Option<&'static str> {
    Some(match (base, stress) {
        ("AH", Some('0')) => "ə",
        ("AH", _) => "ʌ",
        ("ER", Some('0')) => "ɚ",
        ("ER", _) => "ɜɹ",
        ("AA", _) => "ɑ", ("AE", _) => "æ", ("AO", _) => "ɔ", ("AW", _) => "W",
        ("AY", _) => "I", ("EH", _) => "ɛ", ("EY", _) => "A", ("IH", _) => "ɪ",
        ("IY", _) => "i", ("OW", _) => "O", ("OY", _) => "Y", ("UH", _) => "ʊ", ("UW", _) => "u",
        ("B", _) => "b", ("CH", _) => "ʧ", ("D", _) => "d", ("DH", _) => "ð", ("F", _) => "f",
        ("G", _) => "ɡ", ("HH", _) => "h", ("JH", _) => "ʤ", ("K", _) => "k", ("L", _) => "l",
        ("M", _) => "m", ("N", _) => "n", ("NG", _) => "ŋ", ("P", _) => "p", ("R", _) => "ɹ",
        ("S", _) => "s", ("SH", _) => "ʃ", ("T", _) => "t", ("TH", _) => "θ", ("V", _) => "v",
        ("W", _) => "w", ("Y", _) => "j", ("Z", _) => "z", ("ZH", _) => "ʒ",
        _ => return None,
    })
}

/// Space-separated ARPAbet with stress digits -> misaki string. Unknown symbols are dropped.
pub fn arpa_to_misaki(arpa: &str) -> String {
    let mut out = String::new();
    for tok in arpa.split_whitespace() {
        let (base, stress) = match tok.chars().last() {
            Some(c) if c.is_ascii_digit() => (&tok[..tok.len() - 1], Some(c)), // ASCII digit: 1 byte
            _ => (tok, None),
        };
        let Some(sym) = arpa_symbol(base, stress) else { continue };
        match stress {
            Some('1') => out.push('ˈ'),
            Some('2') => out.push('ˌ'),
            _ => {}
        }
        out.push_str(sym);
    }
    out
}

fn letter_name(c: char) -> Option<&'static str> {
    Some(match c {
        'a' => "ˈA", 'b' => "bˈi", 'c' => "sˈi", 'd' => "dˈi", 'e' => "ˈi", 'f' => "ˈɛf",
        'g' => "ʤˈi", 'h' => "ˈAʧ", 'i' => "ˈI", 'j' => "ʤˈA", 'k' => "kˈA", 'l' => "ˈɛl",
        'm' => "ˈɛm", 'n' => "ˈɛn", 'o' => "ˈO", 'p' => "pˈi", 'q' => "kjˈu", 'r' => "ˈɑɹ",
        's' => "ˈɛs", 't' => "tˈi", 'u' => "jˈu", 'v' => "vˈi", 'w' => "dˈʌbəljˌu",
        'x' => "ˈɛks", 'y' => "wˈI", 'z' => "zˈi",
        _ => return None,
    })
}

/// Russian Cyrillic (33 letters, BGN/PCGN-style) plus Ukrainian і ї є ґ, as
/// lowercase Latin: "Путин" -> "putin", "Хрущёв" -> "khrushchyov". The hard and soft
/// signs are silent (""). `None` for anything that is not Cyrillic.
fn cyrillic(c: char) -> Option<&'static str> {
    let lc = c.to_lowercase().next()?;
    Some(match lc {
        'а' => "a",
        'б' => "b",
        'в' => "v",
        'г' => "g",
        'д' => "d",
        'е' => "e",
        'ё' => "yo",
        'ж' => "zh",
        'з' => "z",
        'и' => "i",
        'й' => "y",
        'к' => "k",
        'л' => "l",
        'м' => "m",
        'н' => "n",
        'о' => "o",
        'п' => "p",
        'р' => "r",
        'с' => "s",
        'т' => "t",
        'у' => "u",
        'ф' => "f",
        'х' => "kh",
        'ц' => "ts",
        'ч' => "ch",
        'ш' => "sh",
        'щ' => "shch",
        'ъ' | 'ь' => "",
        'ы' => "y",
        'э' => "e",
        'ю' => "yu",
        'я' => "ya",
        'і' => "i",
        'ї' => "yi",
        'є' => "ye",
        'ґ' => "g",
        _ => return None,
    })
}

/// The lowercase ASCII letters for a non-ASCII letter: Latin accent fold (`ğ` -> "g",
/// `Ł` -> "l", `Æ` -> "ae") or Cyrillic transliteration. `None` for anything else.
fn latin_of(c: char) -> Option<&'static str> {
    crate::ra_norm::fold(c).or_else(|| cyrillic(c))
}

/// Appends the lowercase ASCII letters `c` spells: itself for a-z / A-Z, `latin_of`
/// otherwise. False when `c` is not a letter we can say (digits, symbols, CJK).
fn letters_of(c: char, out: &mut String) -> bool {
    if c.is_ascii_alphabetic() {
        out.push(c.to_ascii_lowercase());
        return true;
    }
    match latin_of(c) {
        Some(f) => {
            out.extend(f.chars().map(|x| x.to_ascii_lowercase()));
            true
        }
        None => false,
    }
}

fn letter_names(word: &str) -> Vec<&'static str> {
    let mut s = String::new();
    for c in word.chars() {
        letters_of(c, &mut s);
    }
    s.chars().filter_map(letter_name).collect()
}

/// Letter-by-letter misaki phonemes. Empty only when the word has no usable letter.
pub fn spell(word: &str) -> String {
    letter_names(word).join(" ")
}

/// Letter names run together the way misaki spells an acronym: one word, secondary
/// stress on every letter but the last (`SEC` -> "ˌɛsˌisˈi").
pub fn spell_tight(word: &str) -> String {
    let names = letter_names(word);
    let mut out = String::new();
    for (i, n) in names.iter().enumerate() {
        if i + 1 == names.len() {
            out.push_str(n);
        } else {
            out.extend(n.chars().map(|c| if c == 'ˈ' { 'ˌ' } else { c }));
        }
    }
    out
}

/// Optional letter-to-sound model: lowercase word -> ARPAbet with stress digits.
pub trait Lts: Send + Sync {
    fn arpabet(&self, word_lower: &str) -> Option<String>;
}

pub struct Names {
    map: HashMap<String, String>,
    /// Keys that came from the hand list (they override misaki too).
    hand: HashSet<String>,
    /// CMUdict keys whose entry only spells the letters (fbi, gdp, ceo).
    acronyms: HashSet<String>,
    lts: Option<Box<dyn Lts>>,
}

/// A pronunciation with stress and spaces removed and ɡ read as ʤ (CMUdict has
/// some hard-G letter spellings, e.g. gdp), for comparing against `spell`.
fn bare(p: &str) -> String {
    p.chars().filter(|c| !matches!(c, 'ˈ' | 'ˌ' | ' ')).map(|c| if c == 'ɡ' { 'ʤ' } else { c }).collect()
}

/// Longest word checked for being a letter-spelled acronym.
const ACRONYM_MAX: usize = 6;

impl Names {
    /// Decompress the embedded data. On failure logs and keeps only the hand list.
    pub fn load(lts: Option<Box<dyn Lts>>) -> Names {
        let mut map: HashMap<String, String> = HashMap::with_capacity(130_000);
        let mut text = String::new();
        match flate2::read::GzDecoder::new(CMU_GZ).read_to_string(&mut text) {
            Ok(_) => {
                for line in text.lines() {
                    if line.starts_with('#') {
                        continue;
                    }
                    if let Some((w, arpa)) = line.split_once('\t') {
                        let m = arpa_to_misaki(arpa);
                        if !m.is_empty() {
                            map.insert(w.to_string(), m);
                        }
                    }
                }
            }
            Err(e) => log::warn!("ra_names: CMUdict decompress failed: {e}"),
        }
        let mut hand = HashSet::new();
        for line in FIX_TSV.lines() {
            if line.starts_with('#') {
                continue;
            }
            if let Some((w, ph)) = line.split_once('\t') {
                let ph = ph.trim();
                // the phonemes go inside misaki's `[word](/ph/)` override: no syntax chars
                if ph.is_empty() || ph.contains(['[', ']', '(', ')', '/']) {
                    log::warn!("ra_names: bad hand-list entry skipped: {line:?}");
                    continue;
                }
                let w = w.trim().to_lowercase();
                map.insert(w.clone(), ph.to_string());
                hand.insert(w);
            }
        }
        let acronyms = map
            .iter()
            .filter(|(w, p)| w.len() <= ACRONYM_MAX && bare(p) == bare(&spell(w)))
            .map(|(w, _)| w.clone())
            .collect();
        Names { map, hand, acronyms, lts }
    }

    /// Knows nothing (the "before" side of the names A/B board).
    #[cfg(test)]
    pub fn empty() -> Names {
        Names { map: HashMap::new(), hand: HashSet::new(), acronyms: HashSet::new(), lts: None }
    }

    /// Hand-list pronunciation of `word` (any case, optional possessive 's), or None.
    /// Hand-list words override misaki's own reading (e.g. Xi, Nguyen).
    pub fn hand(&self, word: &str) -> Option<String> {
        let lower = lower(word);
        if self.hand.contains(lower.as_ref()) {
            return self.map.get(lower.as_ref()).cloned();
        }
        let stem = lower.strip_suffix("'s").or_else(|| lower.strip_suffix("\u{2019}s"))?;
        if !self.hand.contains(stem) {
            return None;
        }
        self.map.get(stem).map(|v| format!("{v}{}", possessive(v)))
    }

    pub fn lookup(&self, word: &str) -> Option<String> {
        let lower = lower(word);
        if let Some(v) = self.map.get(lower.as_ref()) {
            return Some(v.clone());
        }
        let stem = lower.strip_suffix("'s").or_else(|| lower.strip_suffix("\u{2019}s"))?;
        self.map.get(stem).map(|v| format!("{v}{}", possessive(v)))
    }

    /// True when the dictionary says `word_lower` as a word. CMUdict also lists
    /// acronyms as their spelled letters (fbi, gdp, ceo); those don't count, so an
    /// ALL-CAPS acronym stays spelled. No allocation (the acronym set is built at load).
    pub fn knows(&self, word_lower: &str) -> bool {
        self.map.contains_key(word_lower) && !self.acronyms.contains(word_lower)
    }
}

/// `word` lowercased with Latin accents folded (`Erdoğan` -> "erdogan", `Æ` -> "ae")
/// and Cyrillic transliterated ("Путин" -> "putin"),
/// without allocating when there is nothing to change (the common case: `g2p_words`
/// asks the hand list about every word of every chunk).
fn lower(word: &str) -> Cow<'_, str> {
    if !word.chars().any(|c| c.is_uppercase() || (!c.is_ascii() && latin_of(c).is_some())) {
        return Cow::Borrowed(word);
    }
    let mut out = String::with_capacity(word.len());
    for c in word.chars().flat_map(char::to_lowercase) {
        match latin_of(c) {
            Some(f) => out.extend(f.chars().map(|x| x.to_ascii_lowercase())),
            None => out.push(c),
        }
    }
    Cow::Owned(out)
}

/// English possessive ending after a pronunciation: ɪz after a sibilant, s after
/// a voiceless consonant, z otherwise (vowels and voiced consonants).
fn possessive(ph: &str) -> &'static str {
    match ph.chars().rev().find(|c| !matches!(c, 'ˈ' | 'ˌ')) {
        Some('s' | 'z' | 'ʃ' | 'ʒ' | 'ʧ' | 'ʤ') => "ɪz",
        Some('p' | 't' | 'k' | 'f' | 'θ') => "s",
        _ => "z",
    }
}

impl Lts for crate::ra_g2pen::G2pEn {
    fn arpabet(&self, w: &str) -> Option<String> {
        self.predict(w)
    }
}

/// misaki fallback: acronym -> dictionary (hand list, CMUdict) -> CamelCase / compound
/// parts -> letter-to-sound model -> spelled letters, or the digits and symbols read
/// out. Always Ok; never empty for a token with a Latin letter, a digit or a symbol
/// from `symbol_word` (CJK / emoji alone stay silent: Kokoro cannot say them).
pub struct NameFallback(pub Arc<Names>);

impl Fallback for NameFallback {
    fn phonemize(&self, word: &str) -> Result<String, FallbackError> {
        Ok(self.phonemize_via(word).0)
    }
}

impl NameFallback {
    /// The reading and which rule gave it ("acronym", "dict", "compound", "gru",
    /// "spell", "chars"); the glitch hunt counts what still reaches the GRU.
    pub(crate) fn phonemize_via(&self, word: &str) -> (String, &'static str) {
        let n = &self.0;
        let none = Lex::none();
        if let Some(p) = n.acronym(word, &none) {
            return (p, "acronym");
        }
        if let Some(p) = n.lookup(word).filter(|p| !p.is_empty()) {
            return (p, "dict");
        }
        if let Some(p) = n.compound(word, &none) {
            return (p, "compound");
        }
        if let Some(p) = n.lts_say(word) {
            return (p, "gru");
        }
        if !word.chars().any(|c| c.is_ascii_digit() || symbol_word(c).is_some() || quiet_symbol(c)) {
            let s = spell(word);
            if !s.is_empty() {
                return (s, "spell");
            }
        }
        (n.read_chars(word), "chars")
    }
}

/// What the letter-to-sound model is asked for `word`: its lowercase a-z form with a
/// possessive `'s` / `’s` removed (true when one was), accented letters folded to
/// ASCII (é -> e, ñ -> n, ł -> l) and apostrophes dropped (D'Agostino -> dagostino).
/// `None` when any other character is left (digits, CJK), no letter is left, or the
/// stem is longer than 40 letters.
fn lts_key(word: &str) -> Option<(String, bool)> {
    let low = lower(word);
    let (stem, poss) = match low.strip_suffix("'s").or_else(|| low.strip_suffix("\u{2019}s")) {
        Some(s) => (s, true),
        None => (low.as_ref(), false),
    };
    if stem.is_empty() || stem.chars().count() > 40 {
        return None;
    }
    // an apostrophe is not a sound: the model gets the letters around it
    let key: String = stem.chars().filter(|c| !matches!(c, '\'' | '\u{2019}')).collect();
    if key.is_empty() || !key.bytes().all(|b| b.is_ascii_lowercase()) {
        return None;
    }
    Some((key, poss))
}

/// What the misaki lexicon knows, for the rules that run before misaki (`g2p_words`
/// passes the real lexicon; the misaki fallback has none and passes `Lex::none()`).
pub struct Lex<'a> {
    /// The lexicon has a curated entry for this word itself (not only for a stem).
    pub knows: &'a dyn Fn(&str) -> bool,
    /// The lexicon's phonemes for this word, stems included (nonprofits).
    pub phonemes: &'a dyn Fn(&str) -> Option<String>,
}

impl Lex<'static> {
    pub fn none() -> Lex<'static> {
        Lex { knows: &|_| false, phonemes: &|_| None }
    }
}

/// ALL-CAPS words read letter by letter even where a dictionary has them as a word:
/// misaki's gold lexicon says SEC as "sec"; CMUdict has AWS-like and FYI ("for your
/// information") readings. Decisions per word are in the phase 3 task 3 report.
pub const SPELLED: &[&str] = &[
    "API", "AWS", "COO", "CPU", "DOE", "ESG", "ETF", "FYI", "GPU", "IPO", "ROI", "SAP", "SEC", "UPS",
];

/// Two-letter acronyms whose lowercase is a word (ra_text's KEEP_CAPS): spelled when
/// they carry a plural or possessive (PCs, TVs, AI's).
const LETTER_WORDS: &[&str] = &["AI", "EU", "ID", "IQ", "OK", "PC", "TV", "UK", "UN"];

fn is_vowel(c: char) -> bool {
    matches!(c.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u' | 'y')
}

/// `w` without a possessive `'s` / `’s` / `'S` / `’S`, if it has one.
fn strip_possessive(w: &str) -> Option<&str> {
    ["'s", "\u{2019}s", "'S", "\u{2019}S"].iter().find_map(|s| w.strip_suffix(s))
}

/// `core` as stem + whether a suffix (possessive, or a plural `s` after a stem with
/// two or more capitals: GPUs, PhDs) was taken off.
fn suffix_split(core: &str) -> (&str, bool) {
    if let Some(s) = strip_possessive(core) {
        return (s, true);
    }
    match core.strip_suffix('s') {
        Some(s) if s.bytes().filter(u8::is_ascii_uppercase).count() >= 2 => (s, true),
        _ => (core, false),
    }
}

/// ASCII-alphanumeric `w` cut at case and digit boundaries: lower->Upper (`Tech|Review`),
/// UPPER->Upper+lower (`JP|Morgan`, `Postgre|SQL`; not before a final plural s:
/// `Cr|RNAs`) and letter<->digit (`USB|4`).
/// `None` when there is no boundary or more than 6 parts.
fn camel_parts(w: &str) -> Option<Vec<&str>> {
    let b = w.as_bytes();
    let mut cuts = vec![0];
    for i in 1..b.len() {
        let (p, c) = (b[i - 1], b[i]);
        // a final plural s is not a new word: RNAs stays whole in CrRNAs
        let plural_s = i + 2 == b.len() && b[i + 1] == b's';
        let next_lower = b.get(i + 1).is_some_and(u8::is_ascii_lowercase) && !plural_s;
        if (p.is_ascii_lowercase() && c.is_ascii_uppercase())
            || (p.is_ascii_uppercase() && c.is_ascii_uppercase() && next_lower)
            || (p.is_ascii_digit() != c.is_ascii_digit())
        {
            cuts.push(i);
        }
    }
    if cuts.len() < 2 || cuts.len() > 6 {
        return None;
    }
    cuts.push(b.len());
    // b is ASCII (checked by the caller), so every cut is a char boundary
    Some(cuts.windows(2).filter_map(|c| w.get(c[0]..c[1])).collect())
}

/// Spoken words for a run of ASCII digits: a cardinal up to 9 digits ("19" ->
/// "nineteen"), digit by digit otherwise or after a decimal point.
fn number_words(run: &str, after_point: bool) -> String {
    if after_point || run.len() > 9 {
        crate::ra_numwords::digits(run)
    } else {
        crate::ra_numwords::integer(run)
    }
}

/// The name of a Greek letter (`ε` -> "epsilon"), for the number pass (`ε4`).
pub(crate) fn greek_name(c: char) -> Option<&'static str> {
    if matches!(c, '\u{0370}'..='\u{03FF}' | 'µ') {
        symbol_word(c)
    } else {
        None
    }
}

/// The word said for a symbol or Greek letter that misaki would drop or pause on.
fn symbol_word(c: char) -> Option<&'static str> {
    Some(match c {
        '$' => "dollar",
        '€' => "euro",
        '£' => "pound",
        '¥' => "yen",
        '₹' => "rupee",
        '₩' => "won",
        '₿' => "bitcoin",
        '¢' => "cent",
        '%' => "percent",
        '&' => "and",
        '+' => "plus",
        '=' => "equals",
        '@' => "at",
        '#' => "number",
        '×' => "times",
        '÷' => "divided by",
        '−' => "minus",
        '±' => "plus or minus",
        '≈' => "about",
        '≠' => "not equal to",
        '<' => "less than",
        '>' => "greater than",
        '≤' => "at most",
        '≥' => "at least",
        '°' => "degrees",
        '§' => "section",
        '¶' => "paragraph",
        '√' => "root",
        '∞' => "infinity",
        '²' => "squared",
        '³' => "cubed",
        '½' => "one half",
        '¼' => "one quarter",
        '¾' => "three quarters",
        '©' => "copyright",
        '®' => "registered",
        'α' | 'Α' => "alpha",
        'β' | 'Β' => "beta",
        'γ' | 'Γ' => "gamma",
        'δ' | 'Δ' => "delta",
        'ε' | 'Ε' => "epsilon",
        'ζ' | 'Ζ' => "zeta",
        'η' | 'Η' => "eta",
        'θ' | 'Θ' => "theta",
        'ι' | 'Ι' => "iota",
        'ν' | 'Ν' => "nu",
        'ξ' | 'Ξ' => "xi",
        'ο' | 'Ο' => "omicron",
        'υ' | 'Υ' => "upsilon",
        'κ' | 'Κ' => "kappa",
        'λ' | 'Λ' => "lambda",
        'μ' | 'Μ' | 'µ' => "mu",
        'π' | 'Π' => "pi",
        'ρ' | 'Ρ' => "rho",
        'σ' | 'ς' | 'Σ' => "sigma",
        'τ' | 'Τ' => "tau",
        'φ' | 'Φ' => "phi",
        'χ' | 'Χ' => "chi",
        'ψ' | 'Ψ' => "psi",
        'ω' | 'Ω' => "omega",
        _ => return None,
    })
}

/// Symbols with no word of their own: `→` and `•` are a pause (","), `~` says
/// "about" before a number, `* ^ | ™` are silent. Listed so a token holding one is
/// read character by character (`Acme™` -> "acme") rather than spelled.
fn quiet_symbol(c: char) -> bool {
    matches!(c, '→' | '•' | '~' | '*' | '^' | '|' | '™')
}

/// Compounds are only looked for in words up to this long (bounds the O(n²) search).
const COMPOUND_MAX: usize = 30;

impl Names {
    /// The spelled reading of `core` when it is an acronym, else `None`:
    /// - an ALL-CAPS stem on the `SPELLED` list (SEC, AWS), with or without a suffix;
    /// - an ALL-CAPS stem of 5 letters or fewer, or with no vowel, with or without a
    ///   plural / possessive (UTR, GPUs, SEC's, FTX's, HTTPS), that no dictionary says
    ///   as a word (NASA, AIDS stay words; `g2p_words` leaves plain caps words with a
    ///   vowel to misaki and the caps rule, so this mostly fires in the fallback);
    /// - a short mixed-case code: 4 letters or fewer, 2+ capitals, one small letter
    ///   (IoT, NVMe, PCIe, mRNA), that no dictionary says as a word. (5-letter ones
    ///   go through CamelCase: My|SQL, No|SQL.)
    /// The suffix follows the last letter's sound (`z` / `s` / `ɪz`).
    pub fn acronym(&self, core: &str, lex: &Lex) -> Option<String> {
        // a word that ends in s is not a plural (VIRUS, BUS, STATUS)
        let (stem, suf) = if self.says_as_word(core, lex) { (core, false) } else { suffix_split(core) };
        let n = stem.len();
        if !(2..=12).contains(&n) || !stem.bytes().all(|b| b.is_ascii_alphabetic()) {
            return None;
        }
        let upper = stem.bytes().filter(u8::is_ascii_uppercase).count();
        let spelled = if upper == n {
            SPELLED.contains(&stem)
                || (suf && LETTER_WORDS.contains(&stem))
                || (!self.says_as_word(stem, lex) && (n <= 5 || !stem.chars().any(is_vowel)))
        } else {
            n <= 4 && upper >= 2 && n - upper == 1 && !self.says_as_word(stem, lex)
        };
        if !spelled {
            return None;
        }
        let p = spell_tight(stem);
        Some(if suf { format!("{p}{}", possessive(&p)) } else { p })
    }

    /// The lexicon or CMUdict reads `stem` as a word, not letter by letter (NASA,
    /// AIDS, COVID; not FBI, CEO).
    fn says_as_word(&self, stem: &str, lex: &Lex) -> bool {
        let n = stem.chars().count();
        let word = |p: &str| p.matches(['ˈ', 'ˌ']).count() < n;
        if (lex.knows)(stem) && (lex.phonemes)(stem).map_or(true, |p| word(&p)) {
            return true;
        }
        let low = stem.to_ascii_lowercase();
        self.knows(&low) && self.map.get(&low).is_some_and(|p| word(p))
    }

    /// The reading of a word no dictionary knows whole, from its parts:
    /// - CamelCase / digit boundaries (`TechReview`, `JPMorgan`, `iHeartMedia`, `USB4`),
    ///   each part read hand list -> spelled (caps acronym) -> lexicon -> CMUdict -> GRU;
    /// - an all-lowercase word of 6+ letters as 2 (or 3) dictionary words (`elonmusk`,
    ///   `techreview`), fewest parts first, then the longest first part.
    /// `None` when the lexicon or CMUdict knows the word whole (McDonald, iPhone,
    /// YouTube are never split), or no split applies. An `'s` is kept as a suffix.
    pub fn compound(&self, core: &str, lex: &Lex) -> Option<String> {
        let (stem, poss) = match strip_possessive(core) {
            Some(s) => (s, true),
            None => (core, false),
        };
        if stem.is_empty() || stem.len() > 40 || !stem.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return None;
        }
        let parts = camel_parts(stem);
        let lowercase = parts.is_none() && stem.len() >= 6 && stem.len() <= COMPOUND_MAX && stem.bytes().all(|b| b.is_ascii_lowercase());
        if parts.is_none() && !lowercase {
            return None;
        }
        // never split a word a dictionary knows whole
        if (lex.knows)(stem) || (lex.phonemes)(stem).is_some() || self.lookup(stem).is_some() {
            return None;
        }
        let p = match parts {
            Some(parts) => parts.iter().map(|p| self.part(p, lex)).collect::<String>(),
            None => self.segment(stem, lex)?,
        };
        if p.is_empty() {
            return None;
        }
        Some(if poss { format!("{p}{}", possessive(&p)) } else { p })
    }

    /// One CamelCase part (ASCII alphanumeric, non-empty).
    fn part(&self, p: &str, lex: &Lex) -> String {
        if p.bytes().all(|b| b.is_ascii_digit()) {
            return self.say_words(&number_words(p, false));
        }
        if let Some(h) = self.hand(p) {
            return h;
        }
        if let Some(a) = self.acronym(p, lex) {
            return a; // RNAs, IoT, HTTPS
        }
        let n = p.len();
        if n >= 2 && p.bytes().all(|b| b.is_ascii_uppercase()) {
            // a 2-letter caps part is letters even where a dictionary has a word (OS, US)
            if n == 2 || SPELLED.contains(&p) || !p.chars().any(is_vowel) {
                return spell_tight(p);
            }
            if (lex.knows)(p) {
                if let Some(x) = (lex.phonemes)(p) {
                    return x;
                }
            }
            if n <= 4 {
                return spell_tight(p);
            }
        }
        if n == 1 || !p.chars().any(is_vowel) {
            return spell_tight(p);
        }
        if (lex.knows)(p) {
            if let Some(x) = (lex.phonemes)(p) {
                return x;
            }
        }
        self.say_run(p)
    }

    /// A run of letters: CMUdict -> GRU (only with a vowel) -> spelled.
    fn say_run(&self, p: &str) -> String {
        self.lookup(p).filter(|x| !x.is_empty()).or_else(|| self.lts_say(p)).unwrap_or_else(|| spell_tight(p))
    }

    /// Spoken words (space separated) as phonemes, one dictionary word each, joined
    /// with spaces. Words CMUdict lacks go through the GRU or are spelled.
    fn say_words(&self, words: &str) -> String {
        words.split_whitespace().map(|w| self.say_run(w)).collect::<Vec<_>>().join(" ")
    }

    /// `w` (lowercase ASCII, 6..=COMPOUND_MAX letters) as 2 or 3 known words: fewest
    /// parts first, then the longest first part. Parts are 2+ letters (3+ in a 3-way
    /// split: ag|ent|ic is not "agentic"). Splits whose parts are all curated lexicon
    /// words are tried before ones that need CMUdict, whose many names make odd splits
    /// (autos|leep vs auto|sleep). At most ~2n² lookups; runs only on a dictionary miss.
    fn segment(&self, w: &str, lex: &Lex) -> Option<String> {
        let n = w.len();
        if n < 6 {
            return None; // also keeps the n - 6 / n - 2 bounds below from underflowing
        }
        let say = |p: &str| (lex.knows)(p).then(|| (lex.phonemes)(p)).flatten().or_else(|| self.lookup(p));
        let join = |ps: &[&str]| ps.iter().map(|p| say(p)).collect::<Option<String>>();
        let curated = |p: &str| (lex.knows)(p);
        let any = |p: &str| (lex.knows)(p) || self.knows(p);
        for known in [&curated as &dyn Fn(&str) -> bool, &any] {
            let two = |p: &str| p.len() >= 2 && known(p);
            for i in (2..=n - 2).rev() {
                if two(&w[..i]) && two(&w[i..]) {
                    return join(&[&w[..i], &w[i..]]);
                }
            }
        }
        for known in [&curated as &dyn Fn(&str) -> bool, &any] {
            let three = |p: &str| p.len() >= 3 && known(p);
            for i in (3..=n - 6).rev() {
                if !three(&w[..i]) {
                    continue;
                }
                for j in (i + 3..=n - 3).rev() {
                    if three(&w[i..j]) && three(&w[j..]) {
                        return join(&[&w[..i], &w[i..j], &w[j..]]);
                    }
                }
            }
        }
        None
    }

    /// The letter-to-sound model's reading, only for a key with a vowel (a token
    /// with no vowel - nm, kb, QQQ - is never guessed as a word).
    fn lts_say(&self, word: &str) -> Option<String> {
        let lts = self.lts.as_ref()?;
        let (key, poss) = lts_key(word)?;
        if !key.chars().any(is_vowel) {
            return None;
        }
        let p = arpa_to_misaki(&lts.arpabet(&key)?);
        if p.is_empty() {
            return None;
        }
        Some(if poss { format!("{p}{}", possessive(&p)) } else { p })
    }

    /// Every character read out: digit runs as numbers ("3.2" -> three point two),
    /// symbols and Greek letters as words, letter runs as words or spelled. The last
    /// resort that keeps a token from going silent. Empty only when nothing in it is
    /// readable (emoji, CJK).
    fn read_chars(&self, word: &str) -> String {
        let chars: Vec<char> = word.chars().collect();
        let mut out: Vec<String> = Vec::new();
        let mut i = 0;
        let mut after_point = false;
        while i < chars.len() {
            let c = chars[i];
            let start = i;
            if c.is_ascii_digit() {
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                let run: String = chars[start..i].iter().collect();
                out.push(self.say_words(&number_words(&run, after_point)));
                after_point = false;
                continue;
            }
            let mut letters = String::new();
            while i < chars.len() && symbol_word(chars[i]).is_none() && letters_of(chars[i], &mut letters) {
                i += 1;
            }
            if i > start {
                // one letter is its name ("A→B"), CMUdict would say "a" as "uh"
                match letters.len() {
                    0 => {}
                    1 => out.push(spell_tight(&letters)),
                    _ => out.push(self.say_run(&letters)),
                }
                continue;
            }
            let next_digit = chars.get(i + 1).is_some_and(char::is_ascii_digit);
            let between_digits = i > 0 && next_digit && chars[i - 1].is_ascii_digit();
            if c == '.' && between_digits {
                out.push(self.say_run("point"));
                after_point = true;
            } else if matches!(c, '→' | '•') {
                out.push(",".to_string());
            } else if matches!(c, 'ξ' | 'Ξ') {
                out.push("ksˈI".to_string()); // Greek xi; the hand list's "xi" is the name Xi
            } else if c == '~' && next_digit {
                out.push(self.say_run("about"));
            } else if let Some(w) = symbol_word(c) {
                out.push(self.say_words(w));
            }
            i += 1;
        }
        out.retain(|s| !s.is_empty());
        out.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arpa_vowels_consonants_and_stress() {
        assert_eq!(arpa_to_misaki("P AH0 L OW1 S IY0"), "pəlˈOsi");
        assert_eq!(arpa_to_misaki("HH AW1 S"), "hˈWs");
        assert_eq!(arpa_to_misaki("CH ER1 CH"), "ʧˈɜɹʧ");
        assert_eq!(arpa_to_misaki("JH AH1 JH"), "ʤˈʌʤ");
        assert_eq!(arpa_to_misaki("B ER0 D"), "bɚd");
        assert_eq!(arpa_to_misaki("AY1 D IY2 AH0"), "ˈIdˌiə");
    }

    #[test]
    fn every_arpabet_symbol_maps() {
        let all = "AA AE AH AO AW AY B CH D DH EH ER EY F G HH IH IY JH K L M N NG OW OY P R S SH T TH UH UW V W Y Z ZH";
        for s in all.split(' ') {
            let out = arpa_to_misaki(&format!("{s}1"));
            assert!(!out.is_empty() && !out.contains(|c: char| c.is_ascii_uppercase() && !"AIOWY".contains(c)), "{s} -> {out}");
        }
        assert_eq!(arpa_to_misaki("XX1 ??"), "", "unknown symbols are dropped, not panicked on");
    }

    #[test]
    fn spell_never_empty_for_letters_and_never_panics() {
        assert!(!spell("FBI").is_empty());
        assert!(!spell("é").is_empty());
        assert_eq!(spell("123"), "");
        for w in ["🙂", "東京", "", "'", &"a".repeat(60)] { let _ = spell(w); }
    }

    #[test]
    fn hand_list_beats_cmu_and_lookup_is_case_insensitive() {
        let n = Names::load(None);
        assert_eq!(n.lookup("Buttigieg").as_deref(), Some("bˈudəʤˌɛʤ"));
        assert!(n.lookup("PELOSI").is_some());
        assert!(n.lookup("pelosi").is_some());
        assert!(n.knows("putin"));
        assert!(n.lookup("qzxv").is_none());
    }

    #[test]
    fn knows_skips_cmu_entries_that_just_spell_the_letters() {
        let n = Names::load(None);
        for w in ["fbi", "gdp", "ceo", "nfl", "cia"] {
            assert!(!n.knows(w), "{w} is an acronym in CMUdict, not a word");
        }
        for w in ["pelosi", "putin", "musk", "gaza", "buttigieg"] {
            assert!(n.knows(w), "{w}");
        }
        assert!(!n.knows("qzxv"));
    }

    #[test]
    fn hand_returns_only_hand_list_entries() {
        let n = Names::load(None);
        assert_eq!(n.hand("Xi").as_deref(), Some("ʃˈi"));
        assert_eq!(n.hand("XI").as_deref(), Some("ʃˈi"));
        assert_eq!(n.hand("Nguyen").as_deref(), Some("wˈɪn"));
        // Task 6: said names, not spelled letters
        assert_eq!(n.hand("IEEE").as_deref(), Some("ˌI tɹˈɪpəl ˈi"));
        assert_eq!(n.hand("Ng").as_deref(), Some("ˈɪŋ"));
        assert_eq!(n.hand("Xi's").as_deref(), Some("ʃˈiz"));
        assert_eq!(n.hand("Ozempic\u{2019}s").as_deref(), Some("Ozˈɛmpɪks"));
        assert_eq!(n.hand("pelosi"), None, "CMUdict word, not a hand entry");
        assert_eq!(n.hand("the"), None);
        for w in ["", "'", "'s", "🙂", "東京", "[xi]", &"x".repeat(60)] {
            let _ = n.hand(w);
        }
        assert!(Names::empty().hand("xi").is_none());
    }

    #[test]
    fn possessive_suffix_follows_the_last_sound() {
        let n = Names::load(None);
        let base = |w: &str| n.lookup(w).unwrap_or_default();
        // hand-list words (CMUdict has no "'s" entry for them, so the suffix rule runs):
        // vowel / voiced -> z, voiceless -> s, sibilant -> ɪz
        for w in ["milei's", "ozempic's", "mayorkas's", "sheinbaum\u{2019}s"] {
            assert!(!n.map.contains_key(w), "{w} must not be a dictionary entry");
        }
        assert_eq!(n.lookup("Milei's"), Some(format!("{}z", base("milei"))));
        assert_eq!(n.lookup("Ozempic's"), Some(format!("{}s", base("ozempic"))));
        assert_eq!(n.lookup("Mayorkas's"), Some(format!("{}ɪz", base("mayorkas"))));
        assert_eq!(n.lookup("Sheinbaum\u{2019}s"), Some(format!("{}z", base("sheinbaum"))));
        assert_eq!(possessive("bˈʊʃ"), "ɪz");
        assert_eq!(possessive("ʧˈɜɹʧ"), "ɪz");
        assert_eq!(possessive("pˈæθ"), "s");
        assert_eq!(possessive("ˈ"), "z");
        assert!(n.lookup("qzxv's").is_none());
    }

    struct FakeLts;
    impl Lts for FakeLts {
        fn arpabet(&self, w: &str) -> Option<String> { (w == "zorblat").then(|| "Z AO1 R B L AE0 T".into()) }
    }

    #[test]
    fn fallback_order_and_always_ok() {
        use misaki_rs::fallback::Fallback;
        let f = NameFallback(std::sync::Arc::new(Names::load(Some(Box::new(FakeLts)))));
        assert_eq!(f.phonemize("Pelosi").unwrap(), arpa_to_misaki("P EH0 L OW1 S IY0"));
        assert_eq!(f.phonemize("Zorblat").unwrap(), "zˈɔɹblæt");
        assert_eq!(f.phonemize("Qzxv").unwrap(), spell("Qzxv"));
        for w in ["🙂", "東京", "", "x'", &"q".repeat(60)] { assert!(f.phonemize(w).is_ok()); }
    }

    struct NameLts;
    impl Lts for NameLts {
        fn arpabet(&self, w: &str) -> Option<String> {
            match w {
                "mamdani" => Some("M AA0 M D AA1 N IY0".into()),
                "pena" => Some("P EY1 N Y AH0".into()),
                "nunez" => Some("N UW1 N EH0 Z".into()),
                "dagostino" => Some("D AA0 G OW0 S T IY1 N OW0".into()),
                _ => None,
            }
        }
    }

    /// Only the letter-to-sound model knows these names (no dictionary entries).
    fn lts_only() -> NameFallback {
        NameFallback(Arc::new(Names {
            map: HashMap::new(),
            hand: HashSet::new(),
            acronyms: HashSet::new(),
            lts: Some(Box::new(NameLts)),
        }))
    }

    #[test]
    fn possessive_names_only_the_lts_model_knows_are_said_not_spelled() {
        use misaki_rs::fallback::Fallback;
        let f = lts_only();
        let base = arpa_to_misaki("M AA0 M D AA1 N IY0");
        assert_eq!(f.phonemize("Mamdani").unwrap(), base);
        assert_eq!(f.phonemize("Mamdani's").unwrap(), format!("{base}z"));
        assert_eq!(f.phonemize("Mamdani\u{2019}s").unwrap(), format!("{base}z"));
        assert_eq!(f.phonemize("MAMDANI'S").unwrap(), format!("{base}z"));
        // an unknown stem with 's is still spelled (whole word), never panics
        assert_eq!(f.phonemize("Qzxv's").unwrap(), spell("Qzxv's"));
        for w in ["'s", "\u{2019}s", "'", "s'", "o'neil"] {
            assert!(f.phonemize(w).is_ok(), "{w}");
        }
    }

    #[test]
    fn inner_apostrophe_names_are_said_not_spelled() {
        use misaki_rs::fallback::Fallback;
        let f = lts_only();
        let base = arpa_to_misaki("D AA0 G OW0 S T IY1 N OW0");
        assert_eq!(f.phonemize("D'Agostino").unwrap(), base);
        assert_eq!(f.phonemize("D\u{2019}Agostino").unwrap(), base);
        assert_eq!(f.phonemize("D'AGOSTINO'S").unwrap(), format!("{base}z"));
        // apostrophes only: nothing for the model, spelled / Ok, never panics
        for w in ["''", "'\u{2019}'", "'"] {
            assert!(f.phonemize(w).is_ok(), "{w}");
        }
    }

    #[test]
    fn accented_names_fold_to_ascii_for_the_lts_model() {
        use misaki_rs::fallback::Fallback;
        let f = lts_only();
        assert_eq!(f.phonemize("Peña").unwrap(), arpa_to_misaki("P EY1 N Y AH0"));
        assert_eq!(f.phonemize("Núñez").unwrap(), arpa_to_misaki("N UW1 N EH0 Z"));
        assert_eq!(f.phonemize("Peña's").unwrap(), format!("{}z", arpa_to_misaki("P EY1 N Y AH0")));
        // no obvious fold (CJK, ß): spelled / Ok, not sent to the model
        for w in ["東京", "Straße", "🙂"] {
            assert!(f.phonemize(w).is_ok(), "{w}");
        }
    }

    #[test]
    fn real_lts_says_mamdanis_and_pena() {
        use misaki_rs::fallback::Fallback;
        let lts = crate::ra_g2pen::G2pEn::load().map(|g| Box::new(g) as Box<dyn Lts>);
        assert!(lts.is_some(), "embedded g2p_en loads");
        let f = NameFallback(Arc::new(Names::load(lts)));
        let plain = f.phonemize("Mamdani").unwrap();
        assert_ne!(plain, spell("Mamdani"), "Mamdani is said, not spelled");
        for w in ["Mamdani's", "Mamdani\u{2019}s"] {
            let p = f.phonemize(w).unwrap();
            assert_ne!(p, spell(w), "{w} is said, not spelled");
            assert_eq!(p, format!("{plain}{}", possessive(&plain)), "{w}");
        }
        let pena = f.phonemize("Peña").unwrap();
        assert_ne!(pena, spell("Peña"), "Peña is said, not spelled: {pena}");
    }

    #[test]
    fn lowercase_words_are_not_reallocated() {
        assert!(matches!(lower("xi"), std::borrow::Cow::Borrowed("xi")));
        assert!(matches!(lower("ozempic\u{2019}s"), std::borrow::Cow::Borrowed(_)));
        assert_eq!(lower("XI's"), "xi's");
        assert_eq!(lower("Peña"), "pena", "accents fold");
        let n = Names::load(None);
        assert_eq!(n.hand("xi").as_deref(), Some("ʃˈi"));
        assert_eq!(n.hand("XI").as_deref(), Some("ʃˈi"));
    }

    // ---- phase 3 task 3: acronyms, CamelCase, fold, never-empty, compounds ----

    /// A letter-to-sound model that must never be asked.
    struct PanicLts;
    impl Lts for PanicLts {
        fn arpabet(&self, w: &str) -> Option<String> {
            panic!("the GRU was asked about {w:?}")
        }
    }

    fn cmu_fallback() -> NameFallback {
        NameFallback(Arc::new(Names::load(None)))
    }

    #[test]
    fn spell_tight_has_one_primary_stress_and_no_spaces() {
        assert_eq!(spell_tight("SEC"), "ˌɛsˌisˈi");
        assert_eq!(spell_tight("ab"), "ˌAbˈi");
        assert_eq!(spell_tight(""), "");
        assert_eq!(spell_tight("12"), "");
    }

    #[test]
    fn acronyms_are_spelled_with_plural_and_possessive_suffix() {
        use misaki_rs::fallback::Fallback;
        let f = cmu_fallback();
        for (w, stem, suf) in [
            ("SEC's", "SEC", "z"),
            ("AWS", "AWS", ""),
            ("GPUs", "GPU", "z"),
            ("ETFs", "ETF", "s"),
            ("HTTPS", "HTTPS", ""),
            ("FTX's", "FTX", "ɪz"),
            ("SVB's", "SVB", "z"),
            ("SVB\u{2019}s", "SVB", "z"),
            ("QQQ", "QQQ", ""),
            ("NVMe", "NVMe", ""),
            ("PCIe", "PCIe", ""),
            ("IoT", "IoT", ""),
            ("FYI", "FYI", ""),
            ("CEOs", "CEO", "z"),
            ("mRNA", "mRNA", ""),
            ("LLMs", "LLM", "z"),
            ("UTR", "UTR", ""),
            ("TSMC", "TSMC", ""),
        ] {
            assert_eq!(f.phonemize(w).unwrap(), format!("{}{suf}", spell_tight(stem)), "{w}");
        }
    }

    #[test]
    fn word_acronyms_and_names_are_not_acronyms() {
        let n = Names::load(None);
        let none = Lex::none();
        for w in ["NASA", "NATO", "AIDS", "NASA's", "MAMDANI", "MAMDANI'S", "Pelosi", "D'AGOSTINO'S", "Xi", "TSMC2"] {
            assert_eq!(n.acronym(w, &none), None, "{w}");
        }
        // the lexicon knows COVID as a word: not spelled even with a suffix
        let knows = |w: &str| w == "COVID";
        let ph = |w: &str| (w == "COVID").then(|| "kˈOvɪd".to_string());
        let lex = Lex { knows: &knows, phonemes: &ph };
        assert_eq!(n.acronym("COVID's", &lex), None);
        assert_eq!(n.acronym("COVID", &lex), None);
        // the spelled list wins over a dictionary word (CMU has "sec")
        assert_eq!(n.acronym("SEC", &none), Some(spell_tight("SEC")));
        assert!(n.knows("sec"));
    }

    #[test]
    fn tokens_without_a_vowel_never_reach_the_gru() {
        use misaki_rs::fallback::Fallback;
        let f = NameFallback(Arc::new(Names::load(Some(Box::new(PanicLts)))));
        for w in ["nm", "kb", "QQQ", "HTTPS", "Qzxv", "Qzxv's", "NFLX", "pfft"] {
            let p = f.phonemize(w).unwrap();
            assert!(!p.is_empty(), "{w}");
        }
    }

    #[test]
    fn camel_case_reads_the_parts() {
        use misaki_rs::fallback::Fallback;
        let n = Names::load(None);
        let f = cmu_fallback();
        let cmu = |w: &str| n.lookup(w).unwrap();
        assert_eq!(f.phonemize("TechReview").unwrap(), format!("{}{}", cmu("tech"), cmu("review")));
        assert_eq!(f.phonemize("UnitedHealth").unwrap(), format!("{}{}", cmu("united"), cmu("health")));
        assert_eq!(f.phonemize("DeepSeek").unwrap(), format!("{}{}", cmu("deep"), cmu("seek")));
        assert_eq!(f.phonemize("JPMorgan").unwrap(), format!("{}{}", spell_tight("JP"), cmu("morgan")));
        assert_eq!(f.phonemize("USB4").unwrap(), format!("{}{}", spell_tight("USB"), cmu("four")));
        assert_eq!(f.phonemize("iHeartMedia").unwrap(), format!("{}{}{}", spell_tight("i"), cmu("heart"), cmu("media")));
        assert_eq!(f.phonemize("OpenAI's").unwrap(), format!("{}{}z", cmu("open"), spell_tight("AI")));
        assert_eq!(f.phonemize("ChatGPT").unwrap(), format!("{}{}", cmu("chat"), spell_tight("GPT")));
        // a 5-letter mixed-case word is CamelCase, not a letter code
        assert_eq!(f.phonemize("MySQL").unwrap(), format!("{}{}", cmu("my"), spell_tight("SQL")));
        assert_eq!(f.phonemize("NoSQL").unwrap(), format!("{}{}", cmu("no"), spell_tight("SQL")));
        // the lexicon closure is asked before CMUdict
        let knows = |w: &str| w == "Tech";
        let ph = |w: &str| (w == "Tech").then(|| "tˈɛk".to_string());
        let lex = Lex { knows: &knows, phonemes: &ph };
        assert_eq!(n.compound("TechReview", &lex), Some(format!("tˈɛk{}", cmu("review"))));
    }

    #[test]
    fn camel_case_never_splits_a_known_word() {
        let n = Names::load(None);
        let none = Lex::none();
        // CMUdict knows these whole
        for w in ["McDonald", "iPhone", "LinkedIn", "McDonald's", "PayPal"] {
            assert!(n.lookup(w).is_some(), "{w} is in CMUdict");
            assert_eq!(n.compound(w, &none), None, "{w}");
        }
        // the lexicon knows this one whole
        let knows = |w: &str| w == "YouTube";
        let lex = Lex { knows: &knows, phonemes: &|_| None };
        assert_eq!(n.compound("YouTube", &lex), None);
        // no case or digit boundary: nothing to split
        for w in ["Mamdani", "MAMDANI", "a", "", "B", "🙂", "東京"] {
            assert_eq!(n.compound(w, &none), None, "{w}");
        }
    }

    #[test]
    fn accented_letters_fold_before_cmu_and_the_gru() {
        use misaki_rs::fallback::Fallback;
        struct FoldLts;
        impl Lts for FoldLts {
            fn arpabet(&self, w: &str) -> Option<String> {
                match w {
                    "calin" => Some("K AH0 L IY1 N".into()),
                    "lukasz" => Some("W UW1 K AA0 SH".into()),
                    _ => None,
                }
            }
        }
        let n = Names::load(Some(Box::new(FoldLts)));
        let hand = n.lookup("erdogan").unwrap();
        let f = NameFallback(Arc::new(n));
        assert_eq!(f.phonemize("Erdoğan").unwrap(), hand, "folded to the hand-list entry");
        assert_eq!(f.phonemize("Erdoğan's").unwrap(), format!("{hand}z"));
        assert_eq!(f.phonemize("Călin").unwrap(), arpa_to_misaki("K AH0 L IY1 N"));
        assert_eq!(f.phonemize("Łukasz").unwrap(), arpa_to_misaki("W UW1 K AA0 SH"));
        assert_eq!(f.0.hand("Erdoğan"), Some(hand));
        assert!(spell("Łukasz").starts_with("ˈɛl"), "Ł is a letter, not dropped: {}", spell("Łukasz"));
    }

    #[test]
    fn fallback_is_never_empty_for_digits_and_symbols() {
        use misaki_rs::fallback::Fallback;
        let f = cmu_fallback();
        let n = Names::load(None);
        let cmu = |w: &str| n.lookup(w).unwrap();
        for w in ["3.2", "$", "€", "2B", "μ", "£", "¥", "%", "=", "7", "1.5%", "μClinux", "x²"] {
            let p = f.phonemize(w).unwrap();
            assert!(!p.trim().is_empty(), "{w} is silent");
        }
        let p = f.phonemize("3.2").unwrap();
        assert!(p.contains(&cmu("three")) && p.contains(&cmu("point")) && p.contains(&cmu("two")), "{p}");
        assert!(f.phonemize("€").unwrap().contains(&cmu("euro")));
        assert!(f.phonemize("μ").unwrap().contains(&cmu("mu")));
        // still never panics on odd input
        for w in ["", "'", "''", "🙂", "東京", "\u{200D}", &"9".repeat(80), &"Ab".repeat(40)] {
            assert!(f.phonemize(w).is_ok(), "{w:?}");
        }
    }

    #[test]
    fn lowercase_compounds_split_into_dictionary_words() {
        use misaki_rs::fallback::Fallback;
        let n = Names::load(None);
        let cmu = |w: &str| n.lookup(w).unwrap();
        let none = Lex::none();
        assert_eq!(n.compound("techreview", &none), Some(format!("{}{}", cmu("tech"), cmu("review"))));
        assert_eq!(n.compound("deepseek", &none), Some(format!("{}{}", cmu("deep"), cmu("seek"))));
        // "elon" is only in the misaki lexicon
        let knows = |w: &str| w == "elon";
        let ph = |w: &str| (w == "elon").then(|| "ˈilɑn".to_string());
        let lex = Lex { knows: &knows, phonemes: &ph };
        assert_eq!(n.compound("elonmusk", &lex), Some(format!("ˈilɑn{}", cmu("musk"))));
        // three parts at most, fewest parts first
        assert_eq!(
            n.compound("deeptechreview", &none),
            Some(format!("{}{}{}", cmu("deep"), cmu("tech"), cmu("review")))
        );
        // known words are never segmented; short words and non-letters are left alone
        for w in ["together", "carpet", "kingdom", "manage", "pelosi"] {
            assert!(n.lookup(w).is_some(), "{w}");
            assert_eq!(n.compound(w, &none), None, "{w}");
        }
        assert_eq!(n.compound("carpet", &Lex { knows: &|w| w == "carpet", phonemes: &|_| None }), None);
        assert_eq!(n.compound("abcde", &none), None, "under 6 letters");
        assert_eq!(n.compound("qzxvqzxv", &none), None, "no dictionary parts");
        let f = NameFallback(Arc::new(Names::load(None)));
        assert_eq!(f.phonemize("techreview").unwrap(), format!("{}{}", cmu("tech"), cmu("review")));
    }

    #[test]
    fn compound_parts_spell_short_caps_and_suffixed_acronyms() {
        use misaki_rs::fallback::Fallback;
        let n = Names::load(None);
        let f = cmu_fallback();
        let cmu = |w: &str| n.lookup(w).unwrap();
        // CMUdict (and misaki gold) read "os" as a word; inside a compound it is O S
        assert_eq!(f.phonemize("ChromeOS").unwrap(), format!("{}{}", cmu("chrome"), spell_tight("OS")));
        let knows = |w: &str| w == "OS" || w == "watch";
        let ph = |w: &str| match w {
            "OS" => Some("ˈɑs".to_string()),
            "watch" => Some("wˈɑʧ".to_string()),
            _ => None,
        };
        let lex = Lex { knows: &knows, phonemes: &ph };
        assert_eq!(n.compound("watchOS", &lex), Some(format!("wˈɑʧ{}", spell_tight("OS"))));
        // a plural acronym part keeps its letters and its plural
        assert_eq!(f.phonemize("CrRNAs").unwrap(), format!("{}{}z", spell_tight("Cr"), spell_tight("RNA")));
    }

    #[test]
    fn compound_split_prefers_curated_words_and_long_three_way_parts() {
        let n = Names::load(None);
        // the lexicon (curated) has auto, autos, sleep; CMUdict also has "leep" (a name):
        // auto|sleep (all curated) beats the longer first part autos|leep
        let gold = ["auto", "autos", "sleep"];
        let knows = |w: &str| gold.contains(&w);
        let ph = |w: &str| gold.contains(&w).then(|| format!("<{w}>"));
        let lex = Lex { knows: &knows, phonemes: &ph };
        assert!(n.knows("leep") && n.knows("autos"));
        assert_eq!(n.compound("autosleep", &lex), Some("<auto><sleep>".to_string()));
        // a three-way split needs parts of 3+ letters (2-letter pieces make junk splits)
        let gold = ["qx", "qy", "qz", "qxq", "yqz", "xqy"];
        let knows = |w: &str| gold.contains(&w);
        let ph = |w: &str| gold.contains(&w).then(|| format!("<{w}>"));
        let lex = Lex { knows: &knows, phonemes: &ph };
        assert_eq!(n.compound("qxqyqz", &lex), Some("<qxq><yqz>".to_string()), "two parts first");
        assert_eq!(n.compound("qxqzqy", &lex), None, "qx|qz|qy: 2-letter parts in a 3-way split");
        // two-way splits keep the 2-letter minimum
        let gold = ["pull", "up"];
        let knows = |w: &str| gold.contains(&w);
        let ph = |w: &str| gold.contains(&w).then(|| format!("<{w}>"));
        let lex = Lex { knows: &knows, phonemes: &ph };
        assert_eq!(n.compound("pullup", &lex), Some("<pull><up>".to_string()));
    }

    /// A 30-letter miss whose first and middle parts are all "known" and whose last
    /// parts never are: both 2-way and 3-way loops run to the end (worst case).
    fn worst_case_segment(n: &Names, calls: &std::cell::Cell<usize>) -> Option<String> {
        let w = format!("{}z", "a".repeat(COMPOUND_MAX - 1));
        let knows = |p: &str| {
            calls.set(calls.get() + 1);
            !p.ends_with('z')
        };
        let lex = Lex { knows: &knows, phonemes: &|_| None };
        n.compound(&w, &lex)
    }

    #[test]
    fn compound_search_runs_both_loops_and_stays_quadratic() {
        let n = Names::load(None);
        let calls = std::cell::Cell::new(0usize);
        assert_eq!(worst_case_segment(&n, &calls), None);
        let len = COMPOUND_MAX;
        // the 3-way loop ran (far more lookups than the 2-way loop alone) ...
        assert!(calls.get() > 4 * len, "{} lookups: the 3-way loop did not run", calls.get());
        // ... and stayed O(n²)
        assert!(calls.get() <= 4 * len * len, "{} lookups", calls.get());
        // generous bound, debug build
        let t = std::time::Instant::now();
        for _ in 0..20 {
            let _ = worst_case_segment(&n, &std::cell::Cell::new(0));
        }
        assert!(t.elapsed() < std::time::Duration::from_secs(2), "{:?}", t.elapsed());
    }

    /// Release timing (cargo test --release compound_search_release -- --ignored).
    #[test]
    #[ignore]
    fn compound_search_release_under_50ms() {
        let n = Names::load(None);
        let t = std::time::Instant::now();
        for _ in 0..100 {
            let _ = worst_case_segment(&n, &std::cell::Cell::new(0));
        }
        assert!(t.elapsed() < std::time::Duration::from_millis(50), "{:?} for 100 worst-case words", t.elapsed());
    }

    #[test]
    fn segment_guards_short_words_itself() {
        let n = Names::load(None);
        let none = Lex::none();
        for w in ["", "a", "ab", "abcde", "techr"] {
            assert_eq!(n.segment(w, &none), None, "{w:?}");
        }
    }

    #[test]
    fn caps_words_ending_in_s_are_not_plurals() {
        use misaki_rs::fallback::Fallback;
        let n = Names::load(None);
        let f = cmu_fallback();
        for w in ["VIRUS", "STATUS", "CRISIS", "BUS", "Virus", "BONUS"] {
            assert_eq!(n.acronym(w, &Lex::none()), None, "{w}");
            assert_eq!(f.phonemize(w).unwrap(), n.lookup(w).unwrap(), "{w}");
        }
        // LED is mostly the verb in headlines; UPS stays spelled
        assert_eq!(n.acronym("LED", &Lex::none()), None);
        assert_eq!(n.acronym("UPS", &Lex::none()), Some(spell_tight("UPS")));
    }

    #[test]
    fn cyrillic_is_transliterated() {
        use misaki_rs::fallback::Fallback;
        let n = Names::load(None);
        let f = cmu_fallback();
        assert_eq!(f.phonemize("Путин").unwrap(), n.lookup("putin").unwrap());
        assert_eq!(f.phonemize("ПУТИН").unwrap(), n.lookup("putin").unwrap());
        assert_eq!(f.phonemize("Путин's").unwrap(), n.lookup("putin's").unwrap());
        assert_eq!(lower("Москва"), "moskva");
        assert_eq!(lower("Щука"), "shchuka");
        assert_eq!(lower("Ольга"), "olga", "soft sign is silent");
        assert_eq!(lower("Хрущёв"), "khrushchyov");
        assert_eq!(lower("Цой"), "tsoy");
        assert_eq!(lower("Юлия"), "yuliya");
        assert_eq!(spell("Жук"), spell("zhuk"));
        // every letter of the 33-letter alphabet (but the two signs) is said
        for c in "абвгдеёжзийклмнопрстуфхцчшщыэюяАБВГДЕЁЖЗИЙКЛМНОПРСТУФХЦЧШЩЫЭЮЯ".chars() {
            let w = c.to_string();
            assert!(!f.phonemize(&w).unwrap().trim().is_empty(), "{c}");
        }
        for w in ["ъ", "ь", "Ъ"] {
            assert!(f.phonemize(w).is_ok());
        }
    }

    #[test]
    fn greek_letters_are_never_silent() {
        use misaki_rs::fallback::Fallback;
        let f = cmu_fallback();
        for c in "αβγδεζηθικλμνξοπρσςτυφχψωΑΒΓΔΕΖΗΘΙΚΛΜΝΞΟΠΡΣΤΥΦΧΨΩ".chars() {
            assert!(!f.phonemize(&c.to_string()).unwrap().trim().is_empty(), "{c}");
        }
    }

    #[test]
    fn extra_symbols_read_only_where_it_helps() {
        use misaki_rs::fallback::Fallback;
        let n = Names::load(None);
        let f = cmu_fallback();
        let cmu = |w: &str| n.lookup(w).unwrap();
        // pauses, not words
        assert_eq!(f.phonemize("→").unwrap(), ",");
        assert_eq!(f.phonemize("•").unwrap(), ",");
        assert_eq!(f.phonemize("A→B").unwrap(), format!("{} , {}", spell_tight("a"), spell_tight("b")));
        // words
        assert!(f.phonemize("©").unwrap().contains(&cmu("copyright")));
        assert_eq!(f.phonemize("~5").unwrap(), format!("{} {}", cmu("about"), cmu("five")));
        assert!(f.phonemize("20°").unwrap().ends_with(&cmu("degrees")));
        // silent: the word around them is still read as a word
        assert_eq!(f.phonemize("Acme™").unwrap(), cmu("acme"));
        assert_eq!(f.phonemize("~x").unwrap(), spell_tight("x"));
        for w in ["*", "^", "|", "™", "**"] {
            assert!(f.phonemize(w).is_ok(), "{w}");
        }
        assert_eq!(f.phonemize("x^2").unwrap(), format!("{} {}", spell_tight("x"), cmu("two")));
    }

    #[test]
    fn load_is_fast_enough() {
        let t = std::time::Instant::now();
        let n = Names::load(None);
        assert!(n.knows("the"));
        assert!(t.elapsed() < std::time::Duration::from_millis(2500), "{:?}", t.elapsed());
    }
}
