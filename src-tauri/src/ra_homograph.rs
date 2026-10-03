//! Read Aloud: homographs (words spelled the same, said two ways), e.g. "read" (reed/red),
//! "record" (REcord/reCORD). misaki's gold lexicon has one reading per word, so this picks
//! between that reading (A, copied from `us_gold.json`) and the other one (B) using
//! misaki's Penn tag plus one word of context on each side. Pure, static, never panics.

/// How the tag chooses between reading A (misaki's gold entry) and reading B.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Rule {
    /// A = noun/adjective (gold), B = verb. Any VB* tag -> B.
    NounVerb,
    /// A = verb (gold), B = noun. Any NN* tag -> B.
    VerbNoun,
    /// A = present/base, B = past (VBD/VBN).
    Past,
    /// B on JJ (and RB for "live"); otherwise misaki's own.
    Adj,
    /// B on NN only.
    Noun,
    /// B on NNP/NNPS (a proper noun), A forced on a verb.
    Proper,
    /// Context words only; the tagger can't tell these apart.
    Context,
}
use Rule::*;

/// word -> (A: misaki gold, B: the other reading, rule). Sorted by word (binary search).
static TABLE: &[(&str, &str, &str, Rule)] = &[
    ("august", "ˈɔːɡəst", "ɔːɡˈʌst", Adj),
    ("bass", "bˈe\u{200D}ɪs", "bˈæs", Context),
    ("bow", "bˈo\u{200D}ʊ", "bˈa\u{200D}ʊ", NounVerb),
    ("close", "klˈo\u{200D}ʊs", "klˈo\u{200D}ʊz", NounVerb),
    ("conduct", "kˈɑːndʌkt", "kəndˈʌkt", NounVerb),
    ("conflict", "kˈɑːnflɪkt", "kənflˈɪkt", NounVerb),
    ("content", "kˈɑːntɛnt", "kəntˈɛnt", Adj),
    ("contract", "kˈɑːntɹækt", "kəntɹˈækt", NounVerb),
    ("convert", "kənvˈɜːt", "kˈɑːnvɜːt", VerbNoun),
    ("decrease", "dˈiːkɹiːs", "dᵻkɹˈiːs", NounVerb),
    ("desert", "dˈɛzɚt", "dᵻzˈɜːt", NounVerb),
    ("entrance", "ˈɛntɹəns", "ɛntɹˈæns", NounVerb),
    ("estimate", "ˈɛstᵻmˌe\u{200D}ɪt", "ˈɛstᵻmət", VerbNoun),
    ("graduate", "ɡɹˈæd\u{200D}ʒuːət", "ɡɹˈæd\u{200D}ʒuːˌe\u{200D}ɪt", NounVerb),
    ("increase", "ˈɪŋkɹiːs", "ɪŋkɹˈiːs", NounVerb),
    ("insult", "ˈɪnsʌlt", "ɪnsˈʌlt", NounVerb),
    ("invalid", "ɪnvˈælɪd", "ˈɪnvəlɪd", Noun),
    ("lead", "lˈiːd", "lˈɛd", Context),
    ("live", "lˈɪv", "lˈa\u{200D}ɪv", Adj),
    ("minute", "mˈɪnɪt", "ma\u{200D}ɪnjˈuːt", Adj),
    ("moderate", "mˈɑːdɚɹət", "mˈɑːdɚɹˌe\u{200D}ɪt", NounVerb),
    ("object", "ˈɑːbd\u{200D}ʒɛkt", "əbd\u{200D}ʒˈɛkt", NounVerb),
    ("perfect", "pˈɜːfɛkt", "pɚfˈɛkt", NounVerb),
    ("permit", "pˈɜːmɪt", "pɚmˈɪt", NounVerb),
    ("polish", "pˈɑːlɪʃ", "pˈOlɪʃ", Proper),
    ("present", "pɹˈɛzənt", "pɹɪzˈɛnt", NounVerb),
    ("produce", "pɹədˈuːs", "pɹˈo\u{200D}ʊduːs", VerbNoun),
    ("progress", "pɹˈɑːɡɹɛs", "pɹəɡɹˈɛs", NounVerb),
    ("project", "pɹˈɑːd\u{200D}ʒɛkt", "pɹəd\u{200D}ʒˈɛkt", NounVerb),
    ("read", "ɹˈiːd", "ɹˈɛd", Past),
    ("rebel", "ɹˈɛbə\u{200D}l", "ɹᵻbˈɛl", NounVerb),
    ("record", "ɹˈɛkɚd", "ɹᵻkˈɔː\u{200D}ɹd", NounVerb),
    ("refuse", "ɹᵻfjˈuːz", "ɹˈɛfjuːs", VerbNoun),
    ("row", "ɹˈo\u{200D}ʊ", "ɹˈa\u{200D}ʊ", Context),
    ("separate", "sˈɛpɹət", "sˈɛpɚɹˌe\u{200D}ɪt", NounVerb),
    ("subject", "sˈʌbd\u{200D}ʒɛkt", "səbd\u{200D}ʒˈɛkt", NounVerb),
    ("suspect", "sˈʌspɛkt", "səspˈɛkt", NounVerb),
    ("tear", "tˈɪ\u{200D}ɹ", "tˈɛ\u{200D}ɹ", NounVerb),
    ("use", "jˈuːs", "jˈuːz", NounVerb),
    ("wind", "wˈɪnd", "wˈa\u{200D}ɪnd", NounVerb),
    ("wound", "wˈuːnd", "wˈa\u{200D}ʊnd", Past),
];

/// Words after which the next word is a bare verb: "to read", "will record".
const VERB_BEFORE: &[&str] = &[
    "to", "will", "would", "can", "could", "must", "should", "shall", "may", "might", "'ll", "wo", "ca",
    "do", "does", "did", "let", "lets", "please",
];
/// Words after which the next word is a noun or adjective: "the record", "a permit".
const NOUN_BEFORE: &[&str] = &[
    "the", "a", "an", "this", "these", "those", "his", "her", "their", "our", "my", "your", "its",
    "every", "each", "no", "any", "some",
];
/// "have read", "was read": past participle -> "red".
const PAST_BEFORE: &[&str] = &[
    "have", "has", "had", "having", "'ve", "is", "are", "was", "were", "be", "been", "being",
];

fn is(word: Option<&str>, set: &[&str]) -> bool {
    word.is_some_and(|w| set.iter().any(|s| s.eq_ignore_ascii_case(w)))
}

/// Word-specific context: Some(true) = B, Some(false) = A, None = let the tag decide.
fn context(word: &str, prev: Option<&str>, next: Option<&str>) -> Option<bool> {
    let hit = |w: Option<&str>, set: &[&str]| is(w, set);
    match word {
        "read" if hit(prev, VERB_BEFORE) => Some(false),
        "read" if hit(prev, PAST_BEFORE) => Some(true),
        "lead" if hit(next, &["pipe", "pipes", "paint", "poisoning", "pencil", "pencils", "exposure", "levels", "contamination", "shot", "weights", "bullet", "bullets"]) => Some(true),
        "lead" => Some(false),
        "live" if hit(next, &["music", "show", "shows", "stream", "streams", "event", "events", "broadcast", "coverage", "performance", "audience", "album", "concert", "feed", "wire", "ammunition", "bait"]) => Some(true),
        "live" if hit(prev, VERB_BEFORE) => Some(false),
        "wind" | "tear" if hit(next, &["up", "down", "apart", "off", "open", "through", "into"]) => Some(true),
        "wound" if hit(next, &["up", "down", "around", "round", "tight", "tightly"]) => Some(true),
        "bass" => Some(hit(next, &["fishing", "fish", "boat", "boats"]) || hit(prev, &["caught", "catch", "catching", "largemouth", "smallmouth", "striped", "sea"])),
        "bow" if hit(next, &["down", "out", "before"]) || hit(prev, &["take", "takes", "took", "taking", "deep"]) => Some(true),
        "row" => Some(hit(next, &["over", "between", "erupted", "broke"]) || hit(prev, &["blazing", "bitter", "diplomatic", "furious", "heated", "public", "political", "ongoing", "almighty"])),
        "content" if hit(prev, &["feel", "feels", "felt", "seem", "seems", "seemed", "remain", "remained", "am", "perfectly", "quite"]) || hit(next, &["with"]) => Some(true),
        "minute" if hit(next, &["detail", "details", "amount", "amounts", "traces", "quantities", "particles", "differences"]) => Some(true),
        "subject" if hit(next, &["to"]) && !hit(prev, VERB_BEFORE) => Some(false),
        "close" if hit(next, &["to", "by"]) => Some(false),
        _ => None,
    }
}

/// Misaki phonemes for the reading this token should get, or `None` to keep misaki's own.
/// `word_lower` is the token text lowercased; `tag` is misaki's Penn tag (`MToken.tag`);
/// `prev`/`next` are the neighbouring tokens' text (any case, may be punctuation).
pub fn reading(word_lower: &str, tag: &str, prev: Option<&str>, next: Option<&str>) -> Option<&'static str> {
    let i = TABLE.binary_search_by(|e| e.0.cmp(word_lower)).ok()?;
    let &(_, a, b, rule) = TABLE.get(i)?;
    let pick = |alt: bool| Some(if alt { b } else { a });
    if let Some(alt) = context(word_lower, prev, next) {
        return pick(alt);
    }
    let verb = tag.starts_with("VB");
    let noun = tag.starts_with("NN");
    match rule {
        NounVerb | VerbNoun => {
            // The tagger often calls a noun-after-"the" a verb and vice versa; one word of
            // context is the stronger signal. B is the verb reading for NounVerb.
            let as_verb = if is(prev, VERB_BEFORE) {
                true
            } else if is(prev, NOUN_BEFORE) {
                false
            } else {
                verb
            };
            let alt = if rule == NounVerb { as_verb } else { !as_verb && (noun || is(prev, NOUN_BEFORE)) };
            alt.then_some(b)
        }
        Past => matches!(tag, "VBD" | "VBN").then_some(b),
        Adj => (tag == "JJ" || (word_lower == "live" && tag == "RB")).then_some(b),
        Noun => (tag == "NN").then_some(b),
        Proper => {
            if tag.starts_with("NNP") {
                Some(b)
            } else {
                verb.then_some(a)
            }
        }
        Context => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use misaki_rs::{Language, MToken, G2P};
    use std::sync::OnceLock;

    fn g2p() -> &'static G2P {
        static G: OnceLock<G2P> = OnceLock::new();
        G.get_or_init(|| G2P::new(Language::EnglishUS))
    }

    /// (sentence, index of the target among the word tokens, expected phonemes).
    /// Sentences avoid apostrophes and hyphens so word tokens line up with the words.
    const CASES: &[(&str, usize, &str)] = &[
        // read
        ("I will read the report tonight", 2, "ɹˈiːd"),
        ("She has read every page of it", 2, "ɹˈɛd"),
        ("I read it yesterday", 1, "ɹˈɛd"),
        ("I read every day", 1, "ɹˈiːd"),
        ("He read the letter aloud and wept", 1, "ɹˈɛd"),
        // lead
        ("Our team took the lead in the final quarter", 4, "lˈiːd"),
        ("Old lead pipes can poison water", 1, "lˈɛd"),
        ("She will lead the project next year", 2, "lˈiːd"),
        ("Children are harmed by lead poisoning", 4, "lˈɛd"),
        // live
        ("They live in a small town", 1, "lˈɪv"),
        ("We saw a live show last night", 3, "lˈa\u{200D}ɪv"),
        ("The band played live music all night", 3, "lˈa\u{200D}ɪv"),
        ("The game will be broadcast live tonight", 5, "lˈa\u{200D}ɪv"),
        // wind
        ("The wind was cold and strong", 1, "wˈɪnd"),
        ("Please wind the clock before bed", 1, "wˈa\u{200D}ɪnd"),
        ("The roads wind through the hills", 2, "wˈa\u{200D}ɪnd"),
        // tear
        ("A single tear rolled down her cheek", 2, "tˈɪ\u{200D}ɹ"),
        ("Do not tear the paper", 2, "tˈɛ\u{200D}ɹ"),
        // record
        ("She set a new world record", 5, "ɹˈɛkɚd"),
        ("We will record the meeting", 2, "ɹᵻkˈɔː\u{200D}ɹd"),
        // object
        ("He held a strange object in his hand", 4, "ˈɑːbd\u{200D}ʒɛkt"),
        ("I object to this plan", 1, "əbd\u{200D}ʒˈɛkt"),
        // project
        ("The project is nearly finished", 1, "pɹˈɑːd\u{200D}ʒɛkt"),
        ("Analysts project strong growth next year", 1, "pɹəd\u{200D}ʒˈɛkt"),
        // present
        ("She gave him a birthday present", 5, "pɹˈɛzənt"),
        ("They will present the results tomorrow", 2, "pɹɪzˈɛnt"),
        // content
        ("The content of the book is great", 1, "kˈɑːntɛnt"),
        ("He seemed content with the result", 2, "kəntˈɛnt"),
        // minute
        ("Wait a minute for me", 2, "mˈɪnɪt"),
        ("There was a minute change in the data", 3, "ma\u{200D}ɪnjˈuːt"),
        // close
        ("Please close the door", 1, "klˈo\u{200D}ʊz"),
        ("We live close to the station", 2, "klˈo\u{200D}ʊs"),
        // use
        ("The use of force was justified", 1, "jˈuːs"),
        ("You can use my phone", 2, "jˈuːz"),
        // wound
        ("The wound healed slowly", 1, "wˈuːnd"),
        ("He wound the rope around the post", 1, "wˈa\u{200D}ʊnd"),
        // bass
        ("He plays bass in a jazz band", 2, "bˈe\u{200D}ɪs"),
        ("We caught a bass in the lake", 3, "bˈæs"),
        // bow
        ("She tied a bow in her hair", 3, "bˈo\u{200D}ʊ"),
        ("The actors bow to the audience", 2, "bˈa\u{200D}ʊ"),
        // row
        ("We sat in the front row", 5, "ɹˈo\u{200D}ʊ"),
        ("The couple had a row over money", 4, "ɹˈa\u{200D}ʊ"),
        // desert
        ("The desert is hot and dry", 1, "dˈɛzɚt"),
        ("Soldiers who desert face prison", 2, "dᵻzˈɜːt"),
        // permit
        ("You need a permit to park here", 3, "pˈɜːmɪt"),
        ("The rules do not permit smoking", 4, "pɚmˈɪt"),
        // refuse
        ("They refuse to pay the fine", 1, "ɹᵻfjˈuːz"),
        ("The truck collects refuse every week", 3, "ɹˈɛfjuːs"),
        // invalid
        ("The password is invalid", 3, "ɪnvˈælɪd"),
        ("She cared for an invalid at home", 4, "ˈɪnvəlɪd"),
        // produce
        ("Farms produce food for the city", 1, "pɹədˈuːs"),
        ("Fresh produce is sold at the market", 1, "pɹˈo\u{200D}ʊduːs"),
        // conduct
        ("His conduct was excellent", 1, "kˈɑːndʌkt"),
        ("They will conduct a survey", 2, "kəndˈʌkt"),
        // conflict
        ("The conflict lasted ten years", 1, "kˈɑːnflɪkt"),
        ("These rules conflict with the law", 2, "kənflˈɪkt"),
        // contract
        ("She signed the contract today", 3, "kˈɑːntɹækt"),
        ("Muscles contract when cold", 1, "kəntɹˈækt"),
        // convert
        ("You can convert the file easily", 2, "kənvˈɜːt"),
        ("He is a recent convert to the cause", 4, "kˈɑːnvɜːt"),
        // decrease
        ("There was a decrease in sales", 3, "dˈiːkɹiːs"),
        ("Prices will decrease next month", 2, "dᵻkɹˈiːs"),
        // increase
        ("The increase in prices hurt families", 1, "ˈɪŋkɹiːs"),
        ("They plan to increase taxes", 3, "ɪŋkɹˈiːs"),
        // insult
        ("That remark was an insult", 4, "ˈɪnsʌlt"),
        ("Do not insult your guests", 2, "ɪnsˈʌlt"),
        // perfect
        ("It was a perfect day", 3, "pˈɜːfɛkt"),
        ("She wants to perfect her technique", 3, "pɚfˈɛkt"),
        // progress
        ("We made good progress this week", 3, "pɹˈɑːɡɹɛs"),
        ("As the talks progress we will learn more", 3, "pɹəɡɹˈɛs"),
        // rebel
        ("The rebel army took the city", 1, "ɹˈɛbə\u{200D}l"),
        ("Teenagers often rebel against rules", 2, "ɹᵻbˈɛl"),
        // subject
        ("Math is my favorite subject", 4, "sˈʌbd\u{200D}ʒɛkt"),
        ("They will subject the samples to heat", 2, "səbd\u{200D}ʒˈɛkt"),
        // suspect
        ("Police arrested the suspect", 3, "sˈʌspɛkt"),
        ("I suspect he is lying", 1, "səspˈɛkt"),
        // polish
        ("Please polish the silver", 1, "pˈɑːlɪʃ"),
        ("She speaks fluent Polish at home", 3, "pˈOlɪʃ"),
        // entrance
        ("Use the side entrance please", 3, "ˈɛntɹəns"),
        ("Her voice will entrance the crowd", 3, "ɛntɹˈæns"),
        // august
        ("The meeting is in August", 4, "ˈɔɡəst"),
        ("It was an august institution", 3, "ɔːɡˈʌst"),
        // moderate
        ("He holds moderate views", 2, "mˈɑːdɚɹət"),
        ("She will moderate the debate", 2, "mˈɑːdɚɹˌe\u{200D}ɪt"),
        // separate
        ("They sleep in separate rooms", 3, "sˈɛpɹət"),
        ("Please separate the eggs", 1, "sˈɛpɚɹˌe\u{200D}ɪt"),
        // estimate
        ("The estimate was too low", 1, "ˈɛstᵻmət"),
        ("We estimate the cost at ten dollars", 1, "ˈɛstᵻmˌe\u{200D}ɪt"),
        // graduate
        ("He is a graduate of Yale", 3, "ɡɹˈæd\u{200D}ʒuːət"),
        ("She will graduate in June", 2, "ɡɹˈæd\u{200D}ʒuːˌe\u{200D}ɪt"),
    ];

    fn is_word(t: &MToken) -> bool {
        t.text.chars().any(|c| c.is_alphanumeric())
    }

    /// Runs a sentence through the real G2P, then applies `reading` the way Task 5 will:
    /// per token, with the neighbouring tokens' text as context.
    fn spoken_at(sentence: &str, idx: usize) -> (String, String, String) {
        let (_, toks) = g2p().g2p(sentence).expect("g2p");
        let words: Vec<usize> = (0..toks.len()).filter(|&i| is_word(&toks[i])).collect();
        let i = words[idx];
        let tk = &toks[i];
        let lower = tk.text.to_lowercase();
        let prev = i.checked_sub(1).map(|j| toks[j].text.as_str());
        let next = toks.get(i + 1).map(|t| t.text.as_str());
        let misaki = tk.phonemes.clone().unwrap_or_default();
        let got = reading(&lower, &tk.tag, prev, next).map(str::to_string).unwrap_or(misaki);
        (tk.text.clone(), tk.tag.clone(), got)
    }

    #[test]
    fn targeted_sentences_pick_the_right_reading() {
        let mut pass = 0;
        let mut fails = Vec::new();
        for &(s, idx, want) in CASES {
            let (word, tag, got) = spoken_at(s, idx);
            if got == want {
                pass += 1;
            } else {
                fails.push(format!("  {s:?} [{word}/{tag}] got {got} want {want}"));
            }
        }
        let n = CASES.len();
        println!("homographs: {pass}/{n} pass ({:.1}%)", 100.0 * pass as f64 / n as f64);
        for f in &fails {
            println!("{f}");
        }
        // Target: at least 85% of the targeted sentences.
        assert!(pass * 100 >= n * 85, "{pass}/{n} pass, below 85%:\n{}", fails.join("\n"));
    }

    #[test]
    fn every_listed_word_is_covered_both_ways() {
        let words = [
            "read", "lead", "live", "wind", "tear", "record", "object", "project", "present", "content",
            "minute", "close", "use", "wound", "bass", "bow", "row", "desert", "permit", "refuse", "invalid",
            "produce", "conduct", "conflict", "contract", "convert", "decrease", "increase", "insult",
            "perfect", "progress", "rebel", "subject", "suspect", "polish", "entrance", "august",
            "moderate", "separate", "estimate", "graduate",
        ];
        for w in words {
            let mut readings: Vec<&str> = CASES
                .iter()
                .filter(|(s, i, _)| s.split_whitespace().nth(*i).map(|x| x.eq_ignore_ascii_case(w)) == Some(true))
                .map(|c| c.2)
                .collect();
            readings.sort();
            readings.dedup();
            assert!(readings.len() >= 2, "{w}: needs a sentence for each reading");
        }
    }

    #[test]
    fn context_rules() {
        assert_eq!(reading("read", "VBD", Some("to"), None), Some("ɹˈiːd"));
        assert_eq!(reading("read", "VB", Some("will"), None), Some("ɹˈiːd"));
        assert_eq!(reading("read", "VBP", Some("have"), None), Some("ɹˈɛd"));
        assert_eq!(reading("read", "VB", Some("had"), None), Some("ɹˈɛd"));
        assert_eq!(reading("lead", "NN", Some("the"), Some("in")), Some("lˈiːd"));
        assert_eq!(reading("lead", "NN", None, Some("paint")), Some("lˈɛd"));
        assert_eq!(reading("lead", "JJ", Some("the"), Some("pipe")), Some("lˈɛd"));
        assert_eq!(reading("live", "VBP", None, Some("stream")), Some("lˈa\u{200D}ɪv"));
        assert_eq!(reading("record", "NN", Some("to"), None), Some("ɹᵻkˈɔː\u{200D}ɹd"));
        assert_eq!(reading("record", "VB", Some("the"), None), None);
    }

    #[test]
    fn table_is_sorted_and_defaults_match_misaki_gold() {
        assert!(TABLE.windows(2).all(|w| w[0].0 < w[1].0), "TABLE must stay sorted for binary search");
        let lx = misaki_rs::Lexicon::new(Language::EnglishUS);
        for &(w, a, _, _) in TABLE {
            let (gold, _) = lx.get_word(w, "NN", None, None).expect("gold entry");
            assert_eq!(gold, a, "{w}: reading A must be misaki's gold entry");
        }
    }

    #[test]
    fn unknown_words_and_odd_input_are_left_alone() {
        assert_eq!(reading("cat", "NN", None, None), None);
        assert_eq!(reading("", "", None, None), None);
        assert_eq!(reading("record", "", Some(""), Some("")), None);
        assert_eq!(reading("rëad", "VBD", Some("日本"), Some("😀")), None);
    }
}
