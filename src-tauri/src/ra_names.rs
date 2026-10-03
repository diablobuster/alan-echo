//! Read Aloud: pronunciations for words misaki doesn't know (names, brands).
//! Order: hand list (names_fix.tsv) -> CMUdict (BSD-2) -> optional letter-to-sound
//! model -> spelled letters. Never panics; the misaki fallback is always Ok.

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

/// Fold common accented Latin letters to their base ASCII letter.
fn fold(c: char) -> Option<char> {
    if c.is_ascii_alphabetic() {
        return Some(c.to_ascii_lowercase());
    }
    Some(match c.to_lowercase().next()? {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => 'a',
        'ç' | 'ć' | 'č' => 'c',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' => 'e',
        'ì' | 'í' | 'î' | 'ï' | 'ī' => 'i',
        'ñ' | 'ń' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' => 'o',
        'ś' | 'š' => 's',
        'ù' | 'ú' | 'û' | 'ü' | 'ū' => 'u',
        'ý' | 'ÿ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        _ => return None,
    })
}

/// Letter-by-letter misaki phonemes. Empty only when the word has no usable letter.
pub fn spell(word: &str) -> String {
    let parts: Vec<&str> = word.chars().filter_map(fold).filter_map(letter_name).collect();
    parts.join(" ")
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

/// `word` lowercased, without allocating when it has no uppercase letter (the
/// common case: `g2p_words` asks the hand list about every word of every chunk).
fn lower(word: &str) -> Cow<'_, str> {
    if word.chars().any(char::is_uppercase) {
        Cow::Owned(word.to_lowercase())
    } else {
        Cow::Borrowed(word)
    }
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

/// misaki fallback: lookup -> LTS -> spelled letters. Always Ok.
pub struct NameFallback(pub Arc<Names>);

impl Fallback for NameFallback {
    fn phonemize(&self, word: &str) -> Result<String, FallbackError> {
        if let Some(p) = self.0.lookup(word).filter(|p| !p.is_empty()) {
            return Ok(p);
        }
        if let Some(lts) = &self.0.lts {
            if let Some((key, poss)) = lts_key(word) {
                if let Some(a) = lts.arpabet(&key) {
                    let p = arpa_to_misaki(&a);
                    if !p.is_empty() {
                        return Ok(if poss { format!("{p}{}", possessive(&p)) } else { p });
                    }
                }
            }
        }
        Ok(spell(word))
    }
}

/// What the letter-to-sound model is asked for `word`: its lowercase a-z form with a
/// possessive `'s` / `’s` removed (true when one was), accented letters folded to
/// ASCII (é -> e, ñ -> n). `None` when any other character is left (digits, an inner
/// apostrophe, CJK, ß) or the stem is empty or longer than 40 letters.
fn lts_key(word: &str) -> Option<(String, bool)> {
    let low = lower(word);
    let (stem, poss) = match low.strip_suffix("'s").or_else(|| low.strip_suffix("\u{2019}s")) {
        Some(s) => (s, true),
        None => (low.as_ref(), false),
    };
    if stem.is_empty() || stem.chars().count() > 40 {
        return None;
    }
    let key = stem.chars().map(fold).collect::<Option<String>>()?;
    Some((key, poss))
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
        assert_eq!(lower("Peña"), "peña");
        let n = Names::load(None);
        assert_eq!(n.hand("xi").as_deref(), Some("ʃˈi"));
        assert_eq!(n.hand("XI").as_deref(), Some("ʃˈi"));
    }

    #[test]
    fn load_is_fast_enough() {
        let t = std::time::Instant::now();
        let n = Names::load(None);
        assert!(n.knows("the"));
        assert!(t.elapsed() < std::time::Duration::from_millis(2500), "{:?}", t.elapsed());
    }
}
