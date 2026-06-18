//! ALAN Echo — Rule-based text cleanup engine (Rust port).
//! Filler removal, false starts, repetitions, capitalization, number formatting.

use once_cell::sync::Lazy;
use regex::Regex;

static FILLER_WORDS: &[&str] = &[
    "um", "uh", "uhh", "umm", "erm", "er", "ah", "ahh", "hmm", "hm", "mm", "mmm", "mhm",
];

static FILLER_PHRASES: &[&str] = &[
    "you know what i mean", "you know what i'm saying", "if you will", "so to speak",
    "at the end of the day", "to be honest", "to be fair", "as a matter of fact",
    "long story short", "needless to say", "for what it's worth", "in any case",
    "you know", "kind of like", "sort of like", "kind of", "sort of", "more or less",
];

static SENTENCE_START_FILLERS: &[&str] = &[
    "so yeah", "so anyway", "anyway", "so basically", "well anyway",
    "i mean", "i guess", "like i said", "as i was saying",
    "all right so", "okay so", "right so", "well", "so", "like", "okay", "ok", "right",
];

static ALWAYS_UPPERCASE: &[&str] = &[
    "api", "apis", "url", "urls", "html", "css", "sql", "ai", "ml", "nlp", "llm", "gpt",
    "pdf", "csv", "json", "xml", "ui", "ux", "id", "ids", "roi", "kpi", "okr",
    "saas", "b2b", "b2c", "ceo", "cto", "cfo", "vp", "svp",
    "aws", "gcp", "gpu", "cpu", "ram", "ssd", "usb", "http", "https", "dns",
    "etf", "ipo", "sec", "nyse", "nasdaq", "fyi", "asap", "eta",
];

static ALWAYS_CAPITALIZE: &[&str] = &[
    "monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday",
    "january", "february", "march", "april", "may", "june", "july",
    "august", "september", "october", "november", "december",
    "google", "apple", "microsoft", "amazon", "meta", "nvidia", "tesla",
    "alan", "openai", "anthropic", "python", "javascript",
];

static HALLUCINATION_EXACT: &[&str] = &[
    "", "you", "thank you", "thanks", "bye", "goodbye", "the end",
    "thanks for watching", "thank you for watching",
];

static RE_HALLUCINATIONS: Lazy<Vec<Regex>> = Lazy::new(|| vec![
    Regex::new(r"(?i)thanks?\s+for\s+(watching|listening)").expect("hallucination regex 1"),
    Regex::new(r"(?i)please\s+(like\s+and\s+)?subscribe").expect("hallucination regex 2"),
    Regex::new(r"(?i)see\s+you\s+(in\s+the\s+)?next\s+(video|episode|time)").expect("hallucination regex 3"),
    Regex::new(r"(?i)don'?t\s+forget\s+to\s+(like|subscribe|comment|share)").expect("hallucination regex 4"),
    Regex::new(r"(?i)hit\s+the\s+(bell|notification|like)").expect("hallucination regex 5"),
    Regex::new(r"\[.*?\]").expect("bracket regex"),
    Regex::new(r"\(.*?\)").expect("paren regex"),
]);

// No backreference regex — word repetition handled programmatically in remove_word_repetitions()
static RE_MULTI_SPACE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").expect("multi-space regex"));
static RE_DOUBLE_PERIOD: Lazy<Regex> = Lazy::new(|| Regex::new(r"\.{2,}").expect("double-period regex"));
static RE_SPACE_BEFORE_PUNCT: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+([.,!?;:])").expect("space-before-punct regex"));
static RE_STANDALONE_I: Lazy<Regex> = Lazy::new(|| Regex::new(r"\bi\b").expect("standalone-i regex"));

// Precompiled once. `clean()` runs on every transcription; building these
// ~30-70 regexes per call (one per acronym / correction / phrase) used to be
// pure waste on the dictation hot path. Match the RE_HALLUCINATIONS idiom.
static RE_ACRONYMS: Lazy<Vec<(Regex, String)>> = Lazy::new(|| {
    ALWAYS_UPPERCASE.iter().map(|acr| {
        let re = Regex::new(&format!(r"(?i)\b{}\b", regex::escape(acr))).expect("acronym regex");
        (re, acr.to_uppercase())
    }).collect()
});

static RE_INFORMAL: Lazy<Vec<(Regex, &'static str)>> = Lazy::new(|| {
    const CORRECTIONS: &[(&str, &str)] = &[
        ("gonna", "going to"), ("wanna", "want to"), ("gotta", "got to"),
        ("kinda", "kind of"), ("sorta", "sort of"), ("coulda", "could have"),
        ("woulda", "would have"), ("shoulda", "should have"), ("dunno", "don't know"),
        ("lemme", "let me"), ("gimme", "give me"),
    ];
    CORRECTIONS.iter().map(|(from, to)| {
        let re = Regex::new(&format!(r"(?i)\b{}\b", regex::escape(from))).expect("informal regex");
        (re, *to)
    }).collect()
});

static RE_TIGHTEN: Lazy<Vec<(Regex, &'static str)>> = Lazy::new(|| {
    const REPLACEMENTS: &[(&str, &str)] = &[
        (r"(?i)\bin order to\b", "to"),
        (r"(?i)\bdue to the fact that\b", "because"),
        (r"(?i)\bat this point in time\b", "now"),
        (r"(?i)\bin the event that\b", "if"),
        (r"(?i)\bhas the ability to\b", "can"),
        (r"(?i)\bis able to\b", "can"),
        (r"(?i)\bprior to\b", "before"),
    ];
    REPLACEMENTS.iter().map(|(pattern, replacement)| {
        (Regex::new(pattern).expect("tighten regex"), *replacement)
    }).collect()
});

pub struct TextCleanupEngine {
    level: String,
    /// User-defined find→replace rules, precompiled. Applied as the last
    /// transform so they win over the engine's own casing rules.
    rules: Vec<(Regex, String)>,
}

impl TextCleanupEngine {
    pub fn new(level: &str) -> Self {
        Self { level: level.to_string(), rules: Vec::new() }
    }

    /// Change the cleanup level in place. Kept separate from `new` so a level
    /// change from settings doesn't discard the user's find→replace rules.
    pub fn set_level(&mut self, level: &str) {
        self.level = level.to_string();
    }

    /// Compile deterministic find→replace rules. Each `from` matches whole-word
    /// and case-insensitively (the idiom the acronym/correction passes use);
    /// empty/uncompilable `from` patterns are skipped. The replacement is
    /// inserted literally — `$1` in a user's replacement is not a capture ref.
    pub fn set_rules(&mut self, pairs: &[(String, String)]) {
        self.rules = pairs.iter().filter_map(|(from, to)| {
            if from.trim().is_empty() {
                return None;
            }
            Regex::new(&format!(r"(?i)\b{}\b", regex::escape(from)))
                .ok()
                .map(|re| (re, to.clone()))
        }).collect();
    }

    fn apply_rules(&self, text: &str) -> String {
        let mut result = text.to_string();
        for (re, to) in &self.rules {
            result = re.replace_all(&result, regex::NoExpand(to.as_str())).to_string();
        }
        result
    }

    pub fn clean(&self, raw: &str) -> String {
        let mut text = raw.trim().to_string();
        if text.is_empty() { return String::new(); }

        // Verbatim: the user wants exactly what they said (lawyers, devs,
        // people quoting). Bypass every baseline transform — they are the
        // destructive ones: remove_hallucinations strips [..]/(..) and so
        // kills `arr[i]`, fix_punctuation force-appends '.', and
        // fix_capitalization rewrites case. Return the trimmed text untouched.
        if self.level == "verbatim" {
            return self.apply_rules(&text);
        }

        // Levels in increasing aggression: `light` = baseline only (whitespace,
        // hallucination strip, de-dup, punctuation, capitalization); `standard`
        // adds filler/acronym cleanup; `aggressive` adds informal/tightening.
        // Any unrecognized level behaves as `light`.
        // All levels
        text = self.normalize_whitespace(&text);
        text = self.remove_hallucinations(&text);
        if text.is_empty() { return String::new(); }
        text = self.remove_word_repetitions(&text);
        text = self.fix_punctuation(&text);
        text = self.fix_capitalization(&text);

        // Standard+
        if self.level == "standard" || self.level == "aggressive" {
            text = self.remove_filler_words(&text);
            text = self.remove_filler_phrases(&text);
            text = self.remove_sentence_start_fillers(&text);
            text = self.fix_acronyms(&text);
        }

        // Aggressive
        if self.level == "aggressive" {
            text = self.apply_informal_corrections(&text);
            text = self.tighten_phrasing(&text);
        }

        // Final pass
        text = self.fix_capitalization(&text);
        // User find→replace runs after capitalization so it wins over the
        // engine's casing (e.g. "github" → "GitHub" stays GitHub).
        text = self.apply_rules(&text);
        text = self.final_cleanup(&text);
        text.trim().to_string()
    }

    fn normalize_whitespace(&self, text: &str) -> String {
        RE_MULTI_SPACE.replace_all(&text.replace('\n', " ").replace('\r', " "), " ").trim().to_string()
    }

    fn remove_hallucinations(&self, text: &str) -> String {
        let lower = text.trim().to_lowercase();
        let stripped = lower.trim_end_matches(|c: char| ".!?".contains(c));
        if HALLUCINATION_EXACT.contains(&stripped) {
            return String::new();
        }
        let mut result = text.to_string();
        for re in RE_HALLUCINATIONS.iter() {
            result = re.replace_all(&result, "").to_string();
        }
        result = RE_MULTI_SPACE.replace_all(&result, " ").trim().to_string();
        // Check only stop words remain
        let meaningful: Vec<_> = result.split_whitespace()
            .filter(|w| {
                let l = w.to_lowercase().trim_matches(|c: char| ".,!?;:".contains(c)).to_string();
                !["and","the","a","an","or","but","so","to","of","in","is","it"].contains(&l.as_str())
            })
            .collect();
        if meaningful.is_empty() || result.len() < 3 {
            return String::new();
        }
        result
    }

    fn remove_filler_words(&self, text: &str) -> String {
        let words: Vec<&str> = text.split_whitespace().collect();
        let mut result = Vec::new();
        for w in &words {
            let clean = w.to_lowercase().trim_end_matches(|c: char| ".,!?;:".contains(c)).to_string();
            if FILLER_WORDS.contains(&clean.as_str()) {
                continue;
            }
            result.push(*w);
        }
        result.join(" ")
    }

    fn remove_filler_phrases(&self, text: &str) -> String {
        let mut result = text.to_string();
        // Remove longest phrases first so "kind of like" wins over "kind of"
        let mut phrases: Vec<&&str> = FILLER_PHRASES.iter().collect();
        phrases.sort_by(|a, b| b.len().cmp(&a.len()));
        for phrase in phrases {
            // Re-derive the lowercase view after every removal — indices into a
            // stale lowercase string would mis-slice (or panic) once `result`
            // has been shortened by a previous removal.
            let mut search_from = 0;
            loop {
                let lower = result.to_lowercase();
                if lower.len() != result.len() || search_from >= lower.len() {
                    break; // non-ASCII case folding shifted bytes — bail safely
                }
                while search_from < lower.len() && !lower.is_char_boundary(search_from) {
                    search_from += 1;
                }
                if search_from >= lower.len() {
                    break;
                }
                let Some(rel) = lower[search_from..].find(*phrase) else { break };
                let pos = search_from + rel;
                let end = pos + phrase.len();

                // Whole-word match only: don't eat "you know" out of "you knowing".
                let before_ok = pos == 0
                    || !result[..pos].chars().next_back().map(|c| c.is_alphanumeric()).unwrap_or(false);
                let after_ok = end >= result.len()
                    || !result[end..].chars().next().map(|c| c.is_alphanumeric()).unwrap_or(false);
                if !(before_ok && after_ok) {
                    search_from = pos + 1;
                    continue;
                }

                let after_trimmed = result[end..].trim_start_matches(|c: char| ", ".contains(c));
                result = format!("{}{}", &result[..pos], after_trimmed);
                search_from = pos;
            }
        }
        RE_MULTI_SPACE.replace_all(&result, " ").trim().to_string()
    }

    fn remove_sentence_start_fillers(&self, text: &str) -> String {
        let mut result = text.to_string();
        for filler in SENTENCE_START_FILLERS {
            let lower = result.to_lowercase();
            if lower.starts_with(filler) {
                let rest = &result[filler.len()..];
                let rest = rest.trim_start_matches(|c: char| ", ".contains(c));
                if !rest.is_empty() {
                    result = rest.to_string();
                }
            }
        }
        result
    }

    fn remove_word_repetitions(&self, text: &str) -> String {
        let words: Vec<&str> = text.split_whitespace().collect();
        if words.is_empty() { return text.to_string(); }
        let mut result: Vec<&str> = vec![words[0]];
        for w in &words[1..] {
            if !result.last().map(|prev| prev.eq_ignore_ascii_case(w)).unwrap_or(false) {
                result.push(w);
            }
        }
        result.join(" ")
    }

    fn fix_punctuation(&self, text: &str) -> String {
        let mut t = RE_DOUBLE_PERIOD.replace_all(text, ".").to_string();
        t = RE_SPACE_BEFORE_PUNCT.replace_all(&t, "$1").to_string();
        t = t.trim().to_string();
        if !t.is_empty() && !t.ends_with('.') && !t.ends_with('!') && !t.ends_with('?') {
            t.push('.');
        }
        t
    }

    fn fix_capitalization(&self, text: &str) -> String {
        if text.is_empty() { return String::new(); }
        let mut result = String::new();
        let mut capitalize_next = true;

        for ch in text.chars() {
            if capitalize_next && ch.is_alphabetic() {
                result.push(ch.to_uppercase().next().unwrap_or(ch));
                capitalize_next = false;
            } else {
                result.push(ch);
                if ch == '.' || ch == '!' || ch == '?' {
                    capitalize_next = true;
                }
            }
        }

        // Fix "i" → "I"
        result = RE_STANDALONE_I.replace_all(&result, "I").to_string();

        // Fix known words
        let words: Vec<String> = result.split_whitespace().map(|w| {
            let stripped = w.trim_matches(|c: char| ".,!?;:\"'()".contains(c)).to_lowercase();
            if ALWAYS_UPPERCASE.contains(&stripped.as_str()) {
                w.to_lowercase().replace(&stripped, &stripped.to_uppercase())
            } else if ALWAYS_CAPITALIZE.contains(&stripped.as_str()) {
                let cap = capitalize_word(&stripped);
                w.to_lowercase().replace(&stripped, &cap)
            } else {
                w.to_string()
            }
        }).collect();

        words.join(" ")
    }

    fn fix_acronyms(&self, text: &str) -> String {
        let mut result = text.to_string();
        for (re, upper) in RE_ACRONYMS.iter() {
            result = re.replace_all(&result, upper.as_str()).to_string();
        }
        result
    }

    fn apply_informal_corrections(&self, text: &str) -> String {
        let mut result = text.to_string();
        for (re, to) in RE_INFORMAL.iter() {
            result = re.replace_all(&result, *to).to_string();
        }
        result
    }

    fn tighten_phrasing(&self, text: &str) -> String {
        let mut result = text.to_string();
        for (re, replacement) in RE_TIGHTEN.iter() {
            result = re.replace_all(&result, *replacement).to_string();
        }
        result
    }

    fn final_cleanup(&self, text: &str) -> String {
        let mut t = RE_MULTI_SPACE.replace_all(text, " ").trim().to_string();
        t = RE_SPACE_BEFORE_PUNCT.replace_all(&t, "$1").to_string();
        if !t.is_empty() && !t.ends_with('.') && !t.ends_with('!') && !t.ends_with('?') {
            t.push('.');
        }
        // Fix leading comma from removals
        t = t.trim_start_matches(|c: char| ", ".contains(c)).to_string();
        t
    }
}

fn capitalize_word(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => format!("{}{}", c.to_uppercase().collect::<String>(), chars.collect::<String>()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiple_filler_phrases_no_panic() {
        // Regression: two phrase removals used to slice with stale indices.
        let engine = TextCleanupEngine::new("standard");
        let out = engine.clean("you know I think kind of we should you know just ship it sort of soon");
        assert!(!out.to_lowercase().contains("you know"));
        assert!(!out.to_lowercase().contains("kind of"));
    }

    #[test]
    fn phrase_not_removed_mid_word() {
        let engine = TextCleanupEngine::new("standard");
        let out = engine.clean("they were you knowing nothing about it");
        assert!(out.to_lowercase().contains("knowing"));
    }

    #[test]
    fn unicode_input_no_panic() {
        let engine = TextCleanupEngine::new("aggressive");
        let out = engine.clean("um so the café — naïve approach you know works fine");
        assert!(out.contains("café"));
    }

    #[test]
    fn hallucination_only_input_empty() {
        let engine = TextCleanupEngine::new("standard");
        assert_eq!(engine.clean("Thanks for watching!"), "");
        assert_eq!(engine.clean("you"), "");
    }

    #[test]
    fn levels_visibly_differ_on_settings_sample() {
        // Keep in sync with CLEANUP_SAMPLE in SettingsPanel.jsx — the settings
        // preview must show a visible difference between the two levels.
        let sample = "um so basically we're gonna need to uh move the the meeting to friday in order to hit the api deadline you know";
        let std_out = TextCleanupEngine::new("standard").clean(sample);
        let agg_out = TextCleanupEngine::new("aggressive").clean(sample);
        assert_ne!(std_out, agg_out);
        assert!(agg_out.contains("going to"));
        assert!(!agg_out.to_lowercase().contains("in order to"));
    }

    #[test]
    fn basic_cleanup() {
        let engine = TextCleanupEngine::new("standard");
        let out = engine.clean("um so basically i think the the api is ready");
        assert_eq!(out, "I think the API is ready.");
    }

    #[test]
    fn find_replace_rules_win_over_casing_and_are_case_insensitive() {
        let mut engine = TextCleanupEngine::new("standard");
        engine.set_rules(&[
            ("github".to_string(), "GitHub".to_string()),
            ("k8s".to_string(), "Kubernetes".to_string()),
        ]);
        let out = engine.clean("i pushed it to github and deployed to k8s");
        assert!(out.contains("GitHub"), "got: {out}");
        assert!(out.contains("Kubernetes"), "got: {out}");
        assert!(!out.contains("github"), "casing rule must win: {out}");
    }

    #[test]
    fn find_replace_applies_in_verbatim_mode() {
        let mut engine = TextCleanupEngine::new("verbatim");
        engine.set_rules(&[("todo".to_string(), "TODO".to_string())]);
        // Rule fires, and verbatim still preserves the code/casing around it.
        assert_eq!(engine.clean("todo: fix arr[i]"), "TODO: fix arr[i]");
    }

    #[test]
    fn find_replace_replacement_is_literal_not_a_capture_ref() {
        let mut engine = TextCleanupEngine::new("verbatim");
        engine.set_rules(&[("price".to_string(), "$5".to_string())]);
        // "$5" must be inserted literally, not interpreted as capture group 5.
        assert_eq!(engine.clean("the price"), "the $5");
    }

    #[test]
    fn empty_from_rule_is_ignored() {
        let mut engine = TextCleanupEngine::new("verbatim");
        engine.set_rules(&[(String::new(), "X".to_string())]);
        assert_eq!(engine.clean("hello world"), "hello world");
    }

    #[test]
    fn verbatim_preserves_code_punctuation_and_case() {
        // Verbatim must bypass the destructive baseline transforms:
        // remove_hallucinations strips [..]/(..) (would kill arr[i]),
        // fix_punctuation force-appends '.', fix_capitalization rewrites case.
        let engine = TextCleanupEngine::new("verbatim");
        assert_eq!(engine.clean("arr[i] = compute(x)"), "arr[i] = compute(x)");
        assert_eq!(engine.clean("hello world"), "hello world");
        // ...but it is not raw: surrounding whitespace is still trimmed.
        assert_eq!(engine.clean("  spaced out  "), "spaced out");
    }
}
