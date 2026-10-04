//! Round-2 "gapless playback" bench: the real CPU voice model driven by the real
//! `Session` queue logic (`jobs`, `on_chunk`, `seek`), on a virtual clock.
//!
//! Each synth job runs for real (release build, warm CPU engine) and its measured wall
//! time is the worker's busy time; the player consumes pushed samples at 24 kHz. An
//! underrun is time the player had nothing to play after the first audio and before
//! the end (the natural pauses inside the model's audio are not underruns). The
//! controller pumps on a 33 ms tick and after every finished chunk, like `Ctl`.
//!
//! cargo test --release gapless_bench -- --ignored --nocapture --test-threads=1
//! RA_ROUNDS (default 2) repeats every scenario.

use super::*;

use std::collections::VecDeque;

const ARTICLE: &str = "Eight times a year, twelve people sit around a long table in Washington and decide \
what your money costs. They do not vote on your rent, your car loan, or your credit card, but every one \
of those prices moves within days of their meeting. Most people never watch it happen. The room is quiet, \
the statement is short, and the market reacts in seconds. Traders read every word for a hint about the \
next move, and a single changed phrase can send stocks up or down by hundreds of billions of dollars. \
Behind the statement sits a forecast that nobody outside the building can check, built from data that \
arrives late and is revised for months. When the forecast is wrong, the cost lands on families who never \
heard the debate. A mortgage that looked affordable in the spring can cost hundreds more a month by the \
fall. A small business that planned to hire may decide to wait. None of this is secret, but almost none \
of it is explained in plain words, and that is the gap this story is about. On Wednesday the committee \
cut its benchmark rate by a quarter point, to a range of 4.25% to 4.5%, the third cut this year. Stocks \
rose 1.2% in the hour after the decision, while the yield on the 10-year note fell to 4.1%. Two members \
dissented, one in favor of a larger cut and one who wanted no change at all. The chair said the labor \
market had cooled but was not weak, and that inflation, now at 2.7%, was still above the 2% goal. He \
declined to say whether another cut would come in January. Economists at three large banks said they now \
expect two more cuts next year, down from three. For savers, the move means lower yields on money market \
funds within weeks. For borrowers with variable loans, it means slightly smaller payments, but only after \
the next reset date, which for many credit cards is the following statement.";

/// Pump cadence of the controller (`TICK`).
const TICK_S: f64 = 0.033;

struct Run {
    first_audio: f64,
    underruns: Vec<(f64, f64)>,
    gap_at: Vec<usize>,
    chunks: Vec<(usize, f64, f64)>, // words, synth s, audio s
    total_audio: f64,
    /// Per pushed chunk: leading silence s, trailing silence s, ends a sentence.
    sil: Vec<(f64, f64, bool)>,
}

/// Leading and trailing silence of a chunk's PCM (10 ms windows under -40 dBFS).
fn silences(pcm: &[f32]) -> (f64, f64) {
    let win = 240usize; // 10 ms
    let loud = |w: &[f32]| w.iter().any(|x| x.abs() > 0.01);
    let n = pcm.len() / win;
    let lead = (0..n).take_while(|&k| !loud(&pcm[k * win..(k + 1) * win])).count();
    let trail = (0..n).rev().take_while(|&k| !loud(&pcm[k * win..(k + 1) * win])).count();
    if lead == n {
        return (pcm.len() as f64 / 24000.0, 0.0);
    }
    (lead as f64 * 0.01, (trail * win + pcm.len() % win) as f64 / 24000.0)
}

/// Silence heard at each chunk boundary: the chunk's trailing silence + any underrun
/// + the next chunk's leading silence. Returns (all boundaries, sentence-end boundaries).
fn boundary_gaps(r: &Run) -> (Vec<f64>, Vec<f64>) {
    let (mut all, mut sent) = (Vec::new(), Vec::new());
    for k in 0..r.sil.len().saturating_sub(1) {
        let under: f64 = r.gap_at.iter().zip(&r.underruns).filter(|(g, _)| **g == k + 1).map(|(_, u)| u.1).sum();
        let g = r.sil[k].1 + under + r.sil[k + 1].0;
        all.push(g);
        if r.sil[k].2 {
            sent.push(g);
        }
    }
    (all, sent)
}

fn stats(v: &[f64]) -> String {
    if v.is_empty() {
        return "-".into();
    }
    let mut v = v.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let q = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize] * 1000.0;
    format!("median {:.0} / p95 {:.0} / max {:.0} ms (n={})", q(0.5), q(0.95), q(1.0), v.len())
}

/// One read: `start` (or the caller's seek) already happened on `s`; plays to the end.
fn play(e: &Arc<Engine>, s: &mut Session, rate: f32) -> Run {
    let sr = crate::ra_timing::SR as f64;
    let mut t = 0.0f64;
    let mut queue: VecDeque<Job> = VecDeque::new();
    let mut busy: Option<(f64, Job, ChunkAudio, f64)> = None; // (end, job, audio, synth s)
    let (mut pushed, mut consumed) = (0f64, 0f64);
    let mut first: Option<f64> = None;
    let mut under: Vec<(f64, f64)> = Vec::new();
    let mut gap_at: Vec<usize> = Vec::new();
    let mut chunks = Vec::new();
    let mut sil: Vec<(f64, f64, bool)> = Vec::new();
    let mut ends: std::collections::HashMap<usize, bool> = std::collections::HashMap::new();
    let mut next_tick = 0.0f64;
    let start_job = |q: &mut VecDeque<Job>, at: f64, gen: u64| -> Option<(f64, Job, ChunkAudio, f64)> {
        while let Some(j) = q.pop_front() {
            if j.gen != gen {
                continue;
            }
            let words: Vec<&str> = j.words.iter().map(String::as_str).collect();
            let t0 = Instant::now();
            let a = e.synth(&words, j.rate).expect("synth");
            let w = t0.elapsed().as_secs_f64();
            return Some((at + w, j, a, w));
        }
        None
    };
    let pump = |s: &mut Session, q: &mut VecDeque<Job>, consumed: f64| {
        q.extend(s.jobs(consumed as u64, rate));
    };
    pump(s, &mut queue, consumed);
    busy = busy.or_else(|| start_job(&mut queue, t, s.gen()));
    for _ in 0..200_000 {
        let next = busy.as_ref().map_or(next_tick, |b| b.0.min(next_tick));
        // advance the player to `next`
        let dt = next - t;
        if first.is_some() {
            let can = (pushed - consumed).max(0.0);
            let want = dt * sr;
            if want > can + 1e-9 {
                let gap_from = t + can / sr;
                if let Some(last) = under.last_mut().filter(|u| (u.0 + u.1 - gap_from).abs() < 1e-6) {
                    last.1 += next - gap_from;
                } else {
                    under.push((gap_from, next - gap_from));
                    gap_at.push(chunks.len()); // the chunk the player is waiting for
                }
                consumed = pushed;
            } else {
                consumed += want;
            }
        }
        t = next;
        if busy.as_ref().is_some_and(|b| b.0 <= t + 1e-12) {
            let (end, job, a, w) = busy.take().unwrap();
            let worker_free_at = end;
            chunks.push((job.words.len(), w, a.pcm.len() as f64 / sr));
            let last = job.words.last().map(|x| x.trim_end_matches(['"', '\'', ')', '\u{201D}'])).unwrap_or("");
            ends.insert(job.idx, last.ends_with(['.', '!', '?']));
            let before = s.next_push;
            let (pcm, _) = s.on_chunk(job.gen, job.idx, a);
            for (k, p) in pcm.iter().enumerate() {
                let (l, t) = silences(p);
                sil.push((l, t, ends.get(&(before + k)).copied().unwrap_or(false)));
            }
            for p in pcm {
                if first.is_none() && !p.is_empty() {
                    first = Some(t);
                }
                pushed += p.len() as f64;
            }
            pump(s, &mut queue, consumed);
            busy = start_job(&mut queue, worker_free_at, s.gen());
        }
        if t + 1e-12 >= next_tick {
            pump(s, &mut queue, consumed);
            if busy.is_none() {
                busy = start_job(&mut queue, t, s.gen());
            }
            next_tick += TICK_S;
        }
        if first.is_some() && s.finished(consumed as u64) && busy.is_none() && queue.is_empty() {
            break;
        }
    }
    // an underrun that runs to the end is the end of the read, not a gap
    if let Some(l) = under.last() {
        if (l.0 + l.1 - t).abs() < 1e-6 && s.finished(consumed as u64) {
            under.pop();
            gap_at.pop();
        }
    }
    Run { first_audio: first.unwrap_or(f64::NAN), underruns: under, gap_at, chunks, total_audio: pushed / sr, sil }
}

fn report(label: &str, r: &Run) {
    let n = r.underruns.len();
    let worst = r.underruns.iter().map(|u| u.1).fold(0.0, f64::max);
    let total: f64 = r.underruns.iter().map(|u| u.1).sum();
    println!(
        "{label}: first audio {:.0} ms; underruns {n} (worst {:.0} ms, total {:.0} ms, before chunks {:?}) over {:.1} s audio; chunks {:?}",
        r.first_audio * 1000.0,
        worst * 1000.0,
        total * 1000.0,
        r.gap_at,
        r.total_audio,
        r.chunks.iter().map(|c| format!("{}w:{:.0}/{:.0}ms", c.0, c.1 * 1000.0, c.2 * 1000.0)).collect::<Vec<_>>()
    );
    let (all, sent) = boundary_gaps(r);
    let rtf: Vec<f64> = r.chunks.iter().filter(|c| c.2 > 0.0).map(|c| c.1 / c.2).collect();
    println!(
        "    {label} gaps: every chunk boundary {}; sentence ends {}; RTF (synth/played) {}",
        stats(&all),
        stats(&sent),
        stats(&rtf).replace(" ms", "").replace("median", "x1000: median")
    );
}

fn article_words(from: usize) -> Vec<Word> {
    let raw: Vec<String> = ARTICLE.split_whitespace().skip(from).map(String::from).collect();
    ra_text::normalize(&raw)
}

#[test]
#[ignore]
fn gapless_bench() {
    let e = Engine::new(dirs::data_dir().unwrap().join("ALAN Echo"));
    e.preload("af_heart", "cpu");
    let t0 = Instant::now();
    while !e.is_loaded() {
        assert!(t0.elapsed() < Duration::from_secs(60), "load");
        std::thread::sleep(Duration::from_millis(5));
    }
    e.synth(&["Warm", "up", "the", "engine."], 2.2).unwrap();
    let n = ARTICLE.split_whitespace().count();
    println!("article: {n} words; first chunk {FIRST_MAX}, ramp to {CHUNK_MAX}; ahead {AHEAD} chunks / {AHEAD_WORDS} words (cap {AHEAD_MAX})");
    let rounds: usize = std::env::var("RA_ROUNDS").ok().and_then(|v| v.parse().ok()).unwrap_or(2);
    for round in 0..rounds {
        // RA_RATES="2.2,3.5" picks rates (default 2.2, 3.0, 3.5)
        let rates: Vec<f32> = std::env::var("RA_RATES")
            .ok()
            .map(|v| v.split(',').filter_map(|x| x.trim().parse().ok()).collect())
            .unwrap_or_else(|| vec![2.2, 3.0, 3.5]);
        for &rate in &rates {
            let gen = Arc::new(AtomicU64::new(0));
            let mut s = Session::new(Arc::clone(&gen));
            s.set_rate(rate);
            let words = article_words(0);
            let len = words.len();
            s.start(words, len, true, false);
            report(&format!("r{round} start @{rate}x"), &play(&e, &mut s, rate));
            // click-to-jump: a fresh read from the click (mid-article)
            let mut s = Session::new(Arc::clone(&gen));
            s.set_rate(rate);
            let words = article_words(150);
            let len = words.len();
            s.start(words, len, true, false);
            report(&format!("r{round} jump  @{rate}x"), &play(&e, &mut s, rate));
            // skip / speed change: seek inside an existing read
            let mut s = Session::new(Arc::clone(&gen));
            s.set_rate(rate);
            let words = article_words(0);
            let len = words.len();
            s.start(words, len, true, false);
            let c = s.chunk_of(150).unwrap_or(0);
            s.seek(c);
            report(&format!("r{round} skip  @{rate}x"), &play(&e, &mut s, rate));
        }
    }
}

/// Where a chunk's synth time goes: G2P (prepare), model, WSOLA.
#[test]
#[ignore]
fn gapless_breakdown() {
    let e = Engine::new(dirs::data_dir().unwrap().join("ALAN Echo"));
    e.preload("af_heart", "cpu");
    while !e.is_loaded() {
        std::thread::sleep(Duration::from_millis(5));
    }
    let w: Vec<&str> = ARTICLE.split_whitespace().take(26).collect();
    for _ in 0..3 {
        let t = Instant::now();
        let a1 = e.synth(&w, 1.0).unwrap();
        let s1 = t.elapsed().as_secs_f64();
        let t = Instant::now();
        let st = crate::ra_wsola::stretch(&a1.pcm, 3.5, crate::ra_timing::SR);
        let ws = t.elapsed().as_secs_f64();
        println!("26 words: synth@1x {:.0} ms for {:.1} s audio; wsola 3.5x {:.1} ms -> {} samples", s1 * 1e3, a1.pcm.len() as f64 / 24000.0, ws * 1e3, st.len());
    }
}
