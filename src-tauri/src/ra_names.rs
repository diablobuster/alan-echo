//! Read Aloud: pronunciations for words misaki doesn't know (names, brands).
//! Order: hand list (names_fix.tsv) -> CMUdict (BSD-2) -> optional letter-to-sound
//! model -> spelled letters. Never panics; the misaki fallback is always Ok.

// Not wired into ra_tts yet (Task 3 removes this allow).
#![allow(dead_code)]
use std::collections::HashMap;
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
    lts: Option<Box<dyn Lts>>,
}

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
            Err(e) => eprintln!("[ra_names] CMUdict decompress failed: {e}"),
        }
        for line in FIX_TSV.lines() {
            if line.starts_with('#') {
                continue;
            }
            if let Some((w, ph)) = line.split_once('\t') {
                let ph = ph.trim();
                if !ph.is_empty() {
                    map.insert(w.trim().to_lowercase(), ph.to_string());
                }
            }
        }
        Names { map, lts }
    }

    pub fn lookup(&self, word: &str) -> Option<String> {
        let lower = word.to_lowercase();
        if let Some(v) = self.map.get(&lower) {
            return Some(v.clone());
        }
        let stem = lower.strip_suffix("'s").or_else(|| lower.strip_suffix("\u{2019}s"))?;
        self.map.get(stem).map(|v| format!("{v}z"))
    }

    pub fn knows(&self, word_lower: &str) -> bool {
        self.map.contains_key(word_lower)
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
            if word.chars().count() <= 40 && word.chars().all(|c| c.is_ascii_alphabetic() || c == '\'') {
                if let Some(a) = lts.arpabet(&word.to_lowercase()) {
                    let p = arpa_to_misaki(&a);
                    if !p.is_empty() {
                        return Ok(p);
                    }
                }
            }
        }
        Ok(spell(word))
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

    #[test]
    fn load_is_fast_enough() {
        let t = std::time::Instant::now();
        let n = Names::load(None);
        assert!(n.knows("the"));
        assert!(t.elapsed() < std::time::Duration::from_millis(2500), "{:?}", t.elapsed());
    }
}
