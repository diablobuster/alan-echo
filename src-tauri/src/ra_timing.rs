//! Read Aloud — Kokoro phoneme durations -> per-word sample times.
#![allow(dead_code)] // consumed by later Read Aloud tasks
pub const SR: u32 = 24_000;
const SAMPLES_PER_FRAME: f64 = 600.0;

#[derive(Debug, Clone, PartialEq)]
pub struct WordTime {
    pub word: usize,
    pub start: u64,
    pub end: u64,
}

pub fn word_times(durations: &[f32], spans: &[(usize, usize, usize)], n_samples: usize, offset_ms: f32, rate: f32) -> Vec<WordTime> {
    let frames: Vec<f64> = durations.iter().map(|d| (d.round() as f64).max(1.0)).collect();
    let total: f64 = frames.iter().sum();
    let spf = if total > 0.0 { n_samples as f64 / total } else { SAMPLES_PER_FRAME };
    let mut cum = Vec::with_capacity(frames.len() + 1);
    cum.push(0.0);
    for f in &frames {
        cum.push(cum.last().unwrap() + f);
    }
    let off = offset_ms as f64 / 1000.0 * SR as f64;
    let rate = rate.max(0.01) as f64;
    let mut starts: Vec<(usize, u64)> = Vec::new();
    let mut floor = 0u64;
    for &(w, s, e) in spans {
        debug_assert!(e + 2 <= durations.len(), "span end {e} beyond durations ({})", durations.len());
        if e <= s || s + 1 >= cum.len() {
            continue;
        }
        let raw = (cum[s + 1] * spf - off).max(0.0) / rate; // +1 skips the leading pad
        let v = (raw as u64).max(floor);
        floor = v;
        starts.push((w, v));
    }
    let end_all = (n_samples as f64 / rate) as u64;
    (0..starts.len())
        .map(|k| WordTime {
            word: starts[k].0,
            start: starts[k].1,
            end: if k + 1 < starts.len() { starts[k + 1].1 } else { end_all },
        })
        .collect()
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

pub fn align(words: &[&str], tokens: &[&str]) -> Vec<Option<usize>> {
    let wn: Vec<String> = words.iter().map(|w| norm(w)).collect();
    let mut wi = 0usize;
    let mut off = 0usize;
    let mut out = Vec::with_capacity(tokens.len());
    for t in tokens {
        let tn = norm(t);
        if tn.is_empty() {
            out.push(None);
            continue;
        }
        // skip words with nothing speakable
        while wi < wn.len() && wn[wi].is_empty() {
            wi += 1;
            off = 0;
        }
        if wi >= wn.len() {
            out.push(None);
            continue;
        }
        if wn[wi][off..].starts_with(&tn) {
            out.push(Some(wi));
            off += tn.len();
            if off >= wn[wi].len() {
                wi += 1;
                off = 0;
            }
            continue;
        }
        // look ahead up to 3 words for a fresh match
        if let Some(j) = (wi + 1..(wi + 4).min(wn.len()))
            .find(|&j| wn[j] == tn || (tn.chars().count() >= 3 && wn[j].starts_with(&tn)))
        {
            out.push(Some(j));
            wi = j;
            off = tn.len();
            if off >= wn[wi].len() {
                wi += 1;
                off = 0;
            }
            continue;
        }
        // unmatched (e.g. "$5" -> "five","dollars"): credit the current word but do
        // not advance; the next matching token resyncs the cursor
        out.push(Some(wi));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_from_cumulative_frames() {
        let d = [1.0, 2.0, 2.0, 1.0, 3.0, 1.0, 1.0];
        let spans = [(0, 0, 2), (1, 3, 5)];
        let n = 11 * 600;
        let t = word_times(&d, &spans, n, 0.0, 1.0);
        assert_eq!(t[0].start, 600);
        assert_eq!(t[1].start, (1 + 2 + 2 + 1) * 600);
        assert_eq!(t[0].end, t[1].start);
        assert_eq!(t[1].end, n as u64);
    }

    #[test]
    fn offset_and_rate_scale() {
        let d = [1.0, 2.0, 2.0, 1.0, 3.0, 1.0, 1.0];
        let spans = [(0, 0, 2), (1, 3, 5)];
        let n = 11 * 600;
        let t = word_times(&d, &spans, n, 36.0, 2.0);
        let off = (0.036 * SR as f32) as u64;
        assert_eq!(t[1].start, ((6 * 600) as u64 - off) / 2);
        assert_eq!(t[1].end, (n / 2) as u64);
    }

    #[test]
    fn rounds_and_clamps_durations() {
        let d = [0.2, 1.6, 0.4, 0.0];
        let spans = [(0, 0, 2)];
        let t = word_times(&d, &spans, 5 * 600, 0.0, 1.0);
        assert_eq!(t[0].start, 600);
    }

    #[test]
    fn monotonic_never_negative() {
        let d = [1.0, 1.0, 1.0, 1.0];
        let spans = [(0, 0, 1), (1, 1, 2)];
        let t = word_times(&d, &spans, 4 * 600, 500.0, 1.0);
        assert_eq!(t[0].start, 0);
        assert!(t[1].start >= t[0].start);
    }

    #[test]
    fn align_simple() {
        let a = align(&["Hello,", "world."], &["Hello", ",", "world", "."]);
        assert_eq!(a, vec![Some(0), None, Some(1), None]);
    }

    #[test]
    fn align_splits_hyphenated() {
        let a = align(&["a", "hyphen-ated", "word"], &["a", "hyphen", "-", "ated", "word"]);
        assert_eq!(a, vec![Some(0), Some(1), None, Some(1), Some(2)]);
    }

    #[test]
    fn align_skips_punctuation_only_tokens() {
        let a = align(&["—", "next"], &["—", "next"]);
        assert_eq!(a, vec![None, Some(1)]);
    }

    #[test]
    fn align_split_rewrites_resync() {
        let a = align(&["costs", "$5", "today"], &["costs", "five", "dollars", "today"]);
        assert_eq!(a, vec![Some(0), Some(1), Some(1), Some(2)]);
        let b = align(
            &["about", "1,000", "people", "came"],
            &["about", "one", "thousand", "people", "came"],
        );
        assert_eq!(b, vec![Some(0), Some(1), Some(1), Some(2), Some(3)]);
    }

    #[test]
    fn align_short_token_does_not_skip() {
        let a = align(&["a", "big", "apple"], &["a", "big", "apple"]);
        assert_eq!(a, vec![Some(0), Some(1), Some(2)]);
        let b = align(&["two", "apples"], &["2", "apples"]);
        assert_eq!(b, vec![Some(0), Some(1)]);
    }

    #[test]
    fn align_non_ascii() {
        let a = align(&["École", "straße"], &["École", "straße"]);
        assert_eq!(a, vec![Some(0), Some(1)]);
    }

    #[test]
    #[should_panic]
    fn word_times_span_beyond_durations_asserts() {
        word_times(&[1.0, 1.0, 1.0], &[(0, 0, 2)], 1800, 0.0, 1.0);
    }

    #[test]
    fn align_recovers_after_mismatch() {
        let a = align(&["costs", "$5", "today"], &["costs", "five dollars", "today"]);
        assert_eq!(a[0], Some(0));
        assert_eq!(a[2], Some(2));
    }
}
