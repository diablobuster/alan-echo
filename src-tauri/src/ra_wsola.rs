//! Read Aloud — WSOLA time-stretch (speeds speech up without raising pitch).
#![allow(dead_code)]

pub fn stretch(input: &[f32], rate: f32, sr: u32) -> Vec<f32> {
    if !(rate > 1.0001) || !rate.is_finite() {
        return input.to_vec();
    }
    let rate = rate.min(4.0);
    let n = ((sr as f32 * 0.030) as usize) & !1; // ~30 ms window, even
    if n < 8 {
        return input.to_vec();
    }
    let tol = (sr as f32 * 0.008) as usize; // +-8 ms search
    let out_len = (input.len() as f32 / rate) as usize;
    if input.len() < n + tol * 2 {
        // too short for WSOLA: linear-interpolation resample to exactly out_len
        return (0..out_len)
            .map(|j| {
                let p = j as f32 * rate;
                let i = p as usize;
                let f = p - i as f32;
                let a = input[i.min(input.len() - 1)];
                let b = input[(i + 1).min(input.len() - 1)];
                a + (b - a) * f
            })
            .collect();
    }
    // Synthesis hop: 50% overlap normally; at high rates shrink it so the analysis
    // hop (hs * rate) stays <= 0.8 * n and no input falls between windows.
    let hs = (((n / 2) as f32).min(0.8 * n as f32 / rate) as usize).max(1);
    let ha = ((hs as f32 * rate).round() as usize).max(1);
    let win: Vec<f32> = (0..n).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos()).collect();
    // zero-pad so the tail is analysed and the final frames never run off the end
    let mut x = Vec::with_capacity(input.len() + n + tol + ha);
    x.extend_from_slice(input);
    x.resize(input.len() + n + tol + ha, 0.0);
    let mut out = vec![0.0f32; out_len + n];
    let mut norm = vec![0.0f32; out_len + n];
    let mut prev: usize = 0; // input position used for the previous frame
    let mut k = 0usize;
    loop {
        let out_pos = k * hs;
        if out_pos >= out_len {
            break;
        }
        let nominal = k * ha;
        let pos = if k == 0 {
            0
        } else {
            // best match to the natural continuation of the previous frame
            let target = prev + hs;
            let hi = (nominal + tol).min(x.len() - n);
            let lo = nominal.saturating_sub(tol).min(hi);
            let mut best = nominal.min(hi);
            let mut best_c = f32::MIN;
            let mut c = lo;
            while c <= hi {
                let mut s = 0.0f32;
                let mut i = 0;
                while i < n && target + i < x.len() {
                    s += x[c + i] * x[target + i];
                    i += 4;
                }
                if s > best_c {
                    best_c = s;
                    best = c;
                }
                c += 2;
            }
            best
        };
        for i in 0..n {
            out[out_pos + i] += x[pos + i] * win[i];
            norm[out_pos + i] += win[i];
        }
        prev = pos;
        k += 1;
    }
    for i in 0..out_len {
        if norm[i] > 1e-3 {
            out[i] /= norm[i];
        }
    }
    out.truncate(out_len);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sine(f: f32, secs: f32, sr: u32) -> Vec<f32> {
        (0..(secs * sr as f32) as usize).map(|i| (2.0 * std::f32::consts::PI * f * i as f32 / sr as f32).sin() * 0.5).collect()
    }
    fn zero_crossings(x: &[f32]) -> usize {
        x.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count()
    }

    #[test]
    fn length_scales_with_rate() {
        let x = sine(220.0, 2.0, 24_000);
        let y = stretch(&x, 2.2, 24_000);
        let expect = x.len() as f32 / 2.2;
        assert!((y.len() as f32 - expect).abs() / expect < 0.03, "{} vs {}", y.len(), expect);
    }

    #[test]
    fn pitch_preserved() {
        let x = sine(440.0, 2.0, 24_000);
        let y = stretch(&x, 2.0, 24_000);
        let fx = zero_crossings(&x) as f32 / (x.len() as f32 / 24_000.0);
        let fy = zero_crossings(&y) as f32 / (y.len() as f32 / 24_000.0);
        assert!((fx - fy).abs() / fx < 0.05, "{fx} vs {fy}");
    }

    #[test]
    fn rate_one_is_identity() {
        let x = sine(300.0, 0.5, 24_000);
        assert_eq!(stretch(&x, 1.0, 24_000), x);
    }

    #[test]
    fn tiny_and_empty_inputs_safe() {
        assert!(stretch(&[], 2.2, 24_000).is_empty());
        let y = stretch(&[0.1; 100], 2.2, 24_000);
        assert!(y.len() <= 100);
    }

    #[test]
    fn no_clipping() {
        let x = sine(200.0, 1.0, 24_000);
        assert!(stretch(&x, 2.2, 24_000).iter().all(|v| v.abs() <= 0.51));
    }

    #[test]
    fn garbage_rate_and_sr_are_safe() {
        let x = sine(300.0, 0.2, 24_000);
        for r in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.0, -2.0, 0.5] {
            assert_eq!(stretch(&x, r, 24_000), x, "rate {r}");
        }
        assert_eq!(stretch(&x, 2.2, 0), x);
        assert_eq!(stretch(&x, 2.2, 10), x);
        assert!(stretch(&x, 1000.0, 24_000).len() <= x.len() / 4 + 1);
    }

    #[test]
    fn length_and_pitch_at_3_5() {
        let x = sine(440.0, 2.0, 24_000);
        let y = stretch(&x, 3.5, 24_000);
        let expect = x.len() as f32 / 3.5;
        assert!((y.len() as f32 - expect).abs() / expect < 0.03);
        let fx = zero_crossings(&x) as f32 / 2.0;
        let fy = zero_crossings(&y) as f32 / (y.len() as f32 / 24_000.0);
        assert!((fx - fy).abs() / fx < 0.05, "{fx} vs {fy}");
    }

    fn goertzel(x: &[f32], f: f32, sr: f32) -> f32 {
        let (mut re, mut im) = (0.0f32, 0.0f32);
        for (i, v) in x.iter().enumerate() {
            let a = 2.0 * std::f32::consts::PI * f * i as f32 / sr;
            re += v * a.cos();
            im += v * a.sin();
        }
        (re * re + im * im).sqrt() / x.len().max(1) as f32
    }

    #[test]
    fn tail_burst_preserved() {
        let sr = 24_000u32;
        for rate in [1.2f32, 2.2, 3.5] {
            let mut x = sine(300.0, 1.0, sr);
            let burst: Vec<f32> = (0..(0.05 * sr as f32) as usize)
                .map(|i| (2.0 * std::f32::consts::PI * 1500.0 * i as f32 / sr as f32).sin() * 0.5)
                .collect();
            x.extend(burst);
            let y = stretch(&x, rate, sr);
            let win = ((60.0 / rate + 15.0) / 1000.0 * sr as f32) as usize;
            let tail = &y[y.len() - win..];
            let before = &y[y.len() - 2 * win..y.len() - win];
            let et = goertzel(tail, 1500.0, sr as f32);
            let eb = goertzel(before, 1500.0, sr as f32);
            assert!(et > 0.02 && et > 5.0 * eb, "rate {rate}: tail {et} before {eb}");
            // no zero run > 5 ms in the last 100 ms
            let last = &y[y.len() - (0.1 * sr as f32) as usize..];
            let (mut run, mut maxrun) = (0usize, 0usize);
            for v in last {
                if v.abs() < 1e-7 { run += 1; maxrun = maxrun.max(run); } else { run = 0; }
            }
            assert!(maxrun <= (0.005 * sr as f32) as usize, "rate {rate}: zero run {maxrun}");
        }
    }

    #[test]
    fn tiny_input_interpolates_to_exact_length() {
        let y = stretch(&[0.1; 100], 2.0, 24_000);
        assert_eq!(y.len(), 50);
        assert!(y.iter().all(|v| (v - 0.1).abs() < 1e-6));
    }

    #[test]
    fn perf_7_5s_release_only() {
        let x = sine(180.0, 7.5, 24_000);
        let t = std::time::Instant::now();
        let y = stretch(&x, 2.2, 24_000);
        eprintln!("PERF stretch 7.5s@2.2x: {:?} (out {})", t.elapsed(), y.len());
    }

    #[test]
    #[ignore]
    fn wsola_wav_out() {
        let path = std::env::var("RA_REF_WAV").or_else(|_| std::env::var("RAA_REF_WAV")).expect("set RA_REF_WAV");
        let mut r = hound::WavReader::open(&path).unwrap();
        let spec = r.spec();
        let x: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => r.samples::<f32>().map(|s| s.unwrap()).collect(),
            hound::SampleFormat::Int => {
                let m = (1i64 << (spec.bits_per_sample - 1)) as f32;
                r.samples::<i32>().map(|s| s.unwrap() as f32 / m).collect()
            }
        };
        for rate in [2.2f32, 3.0] {
        let y = stretch(&x, rate, spec.sample_rate);
        let ratio = x.len() as f32 / y.len() as f32;
        let max_jump = y.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        let max_jump_in = x.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        let peak = y.iter().fold(0.0f32, |a, v| a.max(v.abs()));
        eprintln!("rate {rate} WAV in {} out {} ratio {ratio:.3} peak {peak:.3} max_jump out {max_jump:.3} in {max_jump_in:.3}", x.len(), y.len());
        assert!((ratio - rate).abs() < 0.05);
        let out_spec = hound::WavSpec { channels: 1, sample_rate: spec.sample_rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create(format!("target/ra_wsola_{rate:.1}.wav"), out_spec).unwrap();
        for v in &y {
            w.write_sample((v.clamp(-1.0, 1.0) * 32767.0) as i16).unwrap();
        }
        w.finalize().unwrap();
        }
    }
}
