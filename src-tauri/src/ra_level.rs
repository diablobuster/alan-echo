//! Read Aloud — per-chunk loudness leveling, soft peak limiter and edge fades, so every
//! synthesized chunk plays at the same level with no clicks at the joins.

use crate::ra_timing::SR;

/// Speech RMS every chunk is brought to (median of real Kokoro chunks, see the task-3 report).
pub const TARGET_RMS: f32 = 0.08;
/// Gain limits, so a near-silent or odd chunk is never blown up or crushed.
const GAIN_MIN: f32 = 0.5;
const GAIN_MAX: f32 = 2.0;
/// A 10 ms window counts as speech when its peak is above chunk peak x this (-30 dB).
const ACTIVE_RATIO: f32 = 0.0316;
/// The limiter starts bending here and never passes `LIMIT_KNEE + LIMIT_SPAN` (0.95).
const LIMIT_KNEE: f32 = 0.9;
const LIMIT_SPAN: f32 = 0.05;

/// RMS over the speech-active 10 ms windows (window peak > chunk peak x -30 dB); 0 if none.
pub(crate) fn speech_rms(pcm: &[f32]) -> f32 {
    let win = (SR as usize / 100).max(1);
    let peak = pcm.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
    if peak <= 0.0 || !peak.is_finite() {
        return 0.0;
    }
    let (mut sum, mut n) = (0.0f64, 0usize);
    for w in pcm.chunks(win) {
        let wp = w.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
        if wp > peak * ACTIVE_RATIO {
            sum += w.iter().map(|&x| f64::from(x) * f64::from(x)).sum::<f64>();
            n += w.len();
        }
    }
    if n == 0 { 0.0 } else { (sum / n as f64).sqrt() as f32 }
}

/// Smooth peak limiter: identity below 0.9, a tanh knee above it, never past 0.95.
fn soft_limit(x: f32) -> f32 {
    let a = x.abs();
    if a <= LIMIT_KNEE {
        x
    } else {
        (LIMIT_KNEE + LIMIT_SPAN * ((a - LIMIT_KNEE) / LIMIT_SPAN).tanh()).copysign(x)
    }
}

/// Level one chunk in place: gain to `TARGET_RMS` (clamped 0.5-2), soft limit, 5 ms edge
/// fades (first and last sample end at zero). Empty or silent input is left untouched.
pub fn level(pcm: &mut [f32]) {
    let rms = speech_rms(pcm);
    if !(rms > 0.0) {
        return;
    }
    let gain = (TARGET_RMS / rms).clamp(GAIN_MIN, GAIN_MAX);
    for x in pcm.iter_mut() {
        *x = soft_limit(*x * gain);
    }
    let n = pcm.len();
    let fade = (SR as usize / 200).min(n / 2);
    for i in 0..fade {
        let g = i as f32 / fade as f32;
        pcm[i] *= g;
        pcm[n - 1 - i] *= g;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 220 Hz tone with a 4 Hz envelope (speech-ish), `secs` long, peak-scaled by `amp`.
    fn sig(secs: f32, amp: f32) -> Vec<f32> {
        (0..(SR as f32 * secs) as usize)
            .map(|i| {
                let t = i as f32 / SR as f32;
                amp * (6.2832 * 220.0 * t).sin() * (0.6 + 0.4 * (6.2832 * 4.0 * t).sin())
            })
            .collect()
    }

    fn db(a: f32, b: f32) -> f32 {
        20.0 * (a / b).log10()
    }

    /// Different input gains end at the same speech level (within 1 dB). The brief's
    /// 0.3 vs 1.5 spans 5x, wider than the 4x clamp window, so 0.5 vs 1.5 is used.
    #[test]
    fn equalizes_chunks() {
        let (mut a, mut b) = (sig(1.0, 0.5 * 0.2), sig(1.0, 1.5 * 0.2));
        level(&mut a);
        level(&mut b);
        let d = db(speech_rms(&a), speech_rms(&b)).abs();
        assert!(d < 1.0, "{d} dB apart");
        assert!((db(speech_rms(&a), TARGET_RMS)).abs() < 1.0);
    }

    /// The gain is clamped: a very quiet chunk is only doubled, a loud one only halved.
    #[test]
    fn gain_is_clamped() {
        let mut q = sig(0.5, 0.001);
        let r = speech_rms(&q);
        level(&mut q);
        assert!((speech_rms(&q) / r - 2.0).abs() < 0.1);
        let mut l = sig(0.5, 0.5);
        let r = speech_rms(&l);
        level(&mut l);
        assert!((speech_rms(&l) / r - 0.5).abs() < 0.05);
    }

    /// Nothing passes 0.95, and the limiter is smooth and monotonic.
    #[test]
    fn limiter_bound() {
        let mut v = sig(0.5, 20.0);
        level(&mut v);
        assert!(v.iter().all(|x| x.abs() <= 0.95 && x.is_finite()));
        let mut prev = 0.0;
        for i in 0..2000 {
            let y = soft_limit(i as f32 * 0.005);
            assert!(y >= prev && y <= 0.95);
            prev = y;
        }
        assert_eq!(soft_limit(0.5), 0.5);
        assert_eq!(soft_limit(-0.5), -0.5);
        assert!(soft_limit(-5.0) >= -0.95);
    }

    /// First and last sample are zero; the fade is 5 ms; the middle is untouched by it.
    #[test]
    fn fades_zero_the_edges() {
        let mut v = vec![0.3f32; 4800];
        level(&mut v);
        assert_eq!(v[0], 0.0);
        assert_eq!(v[v.len() - 1], 0.0);
        assert!(v[119] > 0.0 && v[120] > v[60]);
        assert!(v[2400] > 0.0);
        let mut tiny = vec![0.3f32; 5];
        level(&mut tiny);
        assert_eq!(tiny[0], 0.0);
        assert_eq!(tiny[4], 0.0);
    }

    /// Empty, silent and one-sample inputs never panic.
    #[test]
    fn empty_and_silent_safe() {
        let mut e: Vec<f32> = Vec::new();
        level(&mut e);
        assert!(e.is_empty());
        let mut s = vec![0.0f32; 4800];
        level(&mut s);
        assert!(s.iter().all(|&x| x == 0.0));
        let mut one = vec![0.2f32];
        level(&mut one);
        assert_eq!(one.len(), 1);
    }

    /// Quiet gaps (below -30 dB re peak) do not drag the speech RMS down.
    #[test]
    fn gaps_do_not_count() {
        let mut v = sig(0.5, 0.2);
        let speech = speech_rms(&v);
        v.extend(std::iter::repeat(0.0).take(24_000));
        assert!((speech_rms(&v) / speech - 1.0).abs() < 0.01);
    }

    /// Cost: <= 0.2 ms per second of audio in release.
    /// cargo test --release ra_level::tests::perf -- --ignored --nocapture
    #[test]
    #[ignore]
    fn perf() {
        let base = sig(10.0, 0.2);
        let t = std::time::Instant::now();
        for _ in 0..20 {
            let mut v = base.clone();
            level(&mut v);
        }
        let per_sec = t.elapsed().as_secs_f64() * 1000.0 / 20.0 / 10.0;
        println!("level: {per_sec:.4} ms per second of audio (incl. clone)");
        assert!(per_sec <= 0.2);
    }
}
