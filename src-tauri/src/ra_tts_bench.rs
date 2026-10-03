//! Task 8 benches for the voice engine (real Kokoro CPU model, release build):
//! cargo test --release ra_tts::bench -- --ignored --nocapture --test-threads=1
//!
//! `tts_bench`: time to first audio (cold: load + first chunk; warm), per-chunk synth
//! time and real-time factor at 2.2x, CPU used while synthesizing, RAM at rest /
//! loaded / after idle unload. `tts_sweep`: the same chunks under several CPU session
//! settings (intra-op threads, spinning).

use super::*;
use crate::ra_perf::{cores, mem, ms, pct, process_cpu};

const PASSAGE: &str = "Eight times a year, twelve people sit around a long table in Washington and decide \
what your money costs. They do not vote on your rent, your car loan, or your credit card, but every one \
of those prices moves within days of their meeting. Most people never watch it happen. The room is quiet, \
the statement is short, and the market reacts in seconds. Traders read every word for a hint about the \
next move, and a single changed phrase can send stocks up or down by hundreds of billions of dollars. \
Behind the statement sits a forecast that nobody outside the building can check, built from data that \
arrives late and is revised for months. When the forecast is wrong, the cost lands on families who never \
heard the debate. A mortgage that looked affordable in the spring can cost hundreds more a month by the \
fall. A small business that planned to hire may decide to wait. None of this is secret, but almost none \
of it is explained in plain words, and that is the gap this story is about.";

fn chunks() -> Vec<Vec<&'static str>> {
    let w: Vec<&str> = PASSAGE.split_whitespace().collect();
    let mut out = vec![w[..12].to_vec()];
    out.extend(w[12..].chunks(30).map(|c| c.to_vec()));
    out
}

/// "threads:spin[:mem_pattern[:opt]]", e.g. "8:0:0:2".
fn parse_opts(c: &str) -> Option<CpuOpts> {
    let mut it = c.split(':');
    let intra = it.next()?.parse().ok()?;
    let spin = it.next()? == "1";
    let mem_pattern = it.next().map_or(true, |m| m == "1");
    let opt = it.next().map_or(Some(0), |o| o.parse().ok())?;
    Some(CpuOpts { intra, spin, mem_pattern, opt })
}

fn data_dir() -> PathBuf {
    dirs::data_dir().unwrap().join("ALAN Echo")
}

struct ChunkStat {
    wall: f64,
    cpu: f64,
    audio: f64,
}

fn run_chunk(l: &mut Loaded, words: &[&str]) -> ChunkStat {
    let (t, c) = (Instant::now(), process_cpu());
    let a = Engine::synth_inner(l, words, 2.2, true).expect("synth");
    let wall = ms(t.elapsed());
    let cpu = ms(process_cpu() - c);
    ChunkStat { wall, cpu, audio: a.pcm.len() as f64 / ra_timing::SR as f64 * 1000.0 }
}

fn summary(label: &str, st: &[ChunkStat]) -> (f64, f64) {
    let wall: f64 = st.iter().map(|s| s.wall).sum();
    let cpu: f64 = st.iter().map(|s| s.cpu).sum();
    let audio: f64 = st.iter().map(|s| s.audio).sum();
    let walls: Vec<f64> = st.iter().map(|s| s.wall).collect();
    println!(
        "{label}: per 30-word chunk {}; RTF@2.2x {:.3} (wall/played audio); cores busy while synthesizing {:.2}; \
         CPU per played second {:.0} ms = {:.1}% of the machine over the read",
        pct(&walls),
        wall / audio,
        cpu / wall,
        cpu / audio * 1000.0,
        cpu / audio * 100.0 / cores() as f64
    );
    (wall / audio, cpu / audio)
}

/// cargo test --release ra_tts::bench::tts_bench -- --ignored --nocapture --test-threads=1
#[test]
#[ignore]
fn tts_bench() {
    let ch = chunks();
    let rest = mem();
    println!("RAM at rest (test process, nothing loaded): ws {:.1} MB, private {:.1} MB", rest.0, rest.1);
    let e = Engine::with_idle(data_dir(), Duration::from_secs(2), Duration::from_millis(200));
    let t0 = Instant::now();
    e.preload("af_heart", "cpu");
    while !e.is_loaded() {
        assert!(t0.elapsed() < Duration::from_secs(60), "load");
        std::thread::sleep(Duration::from_millis(2));
    }
    let load = ms(t0.elapsed());
    let t1 = Instant::now();
    let a = e.synth(&ch[0], 2.2).expect("first chunk");
    let first = ms(t1.elapsed());
    println!(
        "cold: load {load:.0} ms + first chunk (12 words) {first:.0} ms = time to first audio {:.0} ms ({:.0} ms audio)",
        load + first,
        a.pcm.len() as f64 / ra_timing::SR as f64 * 1000.0
    );
    let mut warm = Vec::new();
    for _ in 0..5 {
        let t = Instant::now();
        e.synth(&ch[0], 2.2).unwrap();
        warm.push(ms(t.elapsed()));
    }
    println!("warm first chunk (12 words): {}", pct(&warm));
    let eng = e.acquire().unwrap();
    let mut st = Vec::new();
    {
        let mut l = eng.lock();
        for c in &ch[1..] {
            st.push(run_chunk(&mut l, c));
        }
        // a second pass chunked differently: reading never repeats an input length
        let w: Vec<&str> = PASSAGE.split_whitespace().collect();
        for c in w[27..].chunks(29) {
            st.push(run_chunk(&mut l, c));
        }
    }
    let loaded = mem();
    drop(eng);
    summary("default session", &st);
    println!("RAM while loaded (engine + names + g2pen): ws {:.1} MB, private {:.1} MB, peak ws {:.1} MB", loaded.0, loaded.1, loaded.2);
    e.touch();
    let t = Instant::now();
    while e.is_loaded() {
        assert!(t.elapsed() < Duration::from_secs(10), "idle unload");
        std::thread::sleep(Duration::from_millis(50));
    }
    std::thread::sleep(Duration::from_millis(300));
    let after = mem();
    println!(
        "RAM after idle unload: ws {:.1} MB, private {:.1} MB (rest + {:.1} MB private)",
        after.0,
        after.1,
        after.1 - rest.1
    );
}

/// Same chunks under several CPU session settings, interleaved in rounds (this machine
/// is shared, so each config sees the same background load). RA_SWEEP="8:1,4:0" picks
/// configs (threads:spin[:mem_pattern]); RA_ROUNDS sets the rounds (default 3).
/// cargo test --release ra_tts::bench::tts_sweep -- --ignored --nocapture --test-threads=1
#[test]
#[ignore]
fn tts_sweep() {
    let ch = chunks();
    let e = Engine::new(data_dir());
    e.preload("af_heart", "cpu");
    let eng = e.acquire().expect("engine");
    let mut l = eng.lock();
    let f = ra_model::files(&data_dir());
    let configs: Vec<CpuOpts> = std::env::var("RA_SWEEP")
        .unwrap_or_else(|_| "8:1,8:0,6:0,5:0,4:0".into())
        .split(',')
        .filter_map(parse_opts)
        .collect();
    let rounds: usize = std::env::var("RA_ROUNDS").ok().and_then(|v| v.parse().ok()).unwrap_or(3);
    println!("physical cores {}, logical {}, {rounds} rounds", num_cpus::get_physical(), cores());
    let mut sessions = Vec::new();
    for o in &configs {
        let t = Instant::now();
        let mut s = open_session_with(&f.fp32, false, *o).expect("session");
        let open = ms(t.elapsed());
        std::mem::swap(&mut l.session, &mut s);
        let t = Instant::now();
        Engine::synth_inner(&mut l, &ch[0], 2.2, true).unwrap(); // first run on a new session
        let first_run = ms(t.elapsed());
        std::mem::swap(&mut l.session, &mut s);
        println!("{o:?}: open {open:.0} ms, first run {first_run:.0} ms");
        sessions.push(s);
    }
    let mut warm: Vec<Vec<f64>> = vec![Vec::new(); configs.len()];
    let mut st: Vec<Vec<ChunkStat>> = configs.iter().map(|_| Vec::new()).collect();
    for _ in 0..rounds {
        for (i, s) in sessions.iter_mut().enumerate() {
            std::mem::swap(&mut l.session, s);
            for _ in 0..2 {
                let t = Instant::now();
                Engine::synth_inner(&mut l, &ch[0], 2.2, true).unwrap();
                warm[i].push(ms(t.elapsed()));
            }
            for c in &ch[1..] {
                st[i].push(run_chunk(&mut l, c));
            }
            std::mem::swap(&mut l.session, s);
        }
    }
    for (i, o) in configs.iter().enumerate() {
        println!("--- {o:?}: warm 12-word first chunk {}", pct(&warm[i]));
        summary("    ", &st[i]);
    }
}

/// Does a tiny warm-up synth right after the session opens take the first-run cost off
/// the first real chunk? Fresh session each time: (a) 12-word chunk straight away,
/// (b) warm-up words, then the 12-word chunk. RA_WARM sets the warm-up text.
/// cargo test --release ra_tts::bench::tts_warmup_ab -- --ignored --nocapture --test-threads=1
#[test]
#[ignore]
fn tts_warmup_ab() {
    let ch = chunks();
    let e = Engine::new(data_dir());
    e.preload("af_heart", "cpu");
    let eng = e.acquire().expect("engine");
    let mut l = eng.lock();
    let f = ra_model::files(&data_dir());
    let warm_text = std::env::var("RA_WARM").unwrap_or_else(|_| "Hello there.".into());
    let warm_words: Vec<&str> = warm_text.split_whitespace().collect();
    let (mut a, mut b, mut wu) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..4 {
        l.session = open_session(&f.fp32, false).unwrap();
        let t = Instant::now();
        Engine::synth_inner(&mut l, &ch[0], 2.2, true).unwrap();
        a.push(ms(t.elapsed()));
        l.session = open_session(&f.fp32, false).unwrap();
        let t = Instant::now();
        Engine::synth_inner(&mut l, &warm_words, 2.2, true).unwrap();
        wu.push(ms(t.elapsed()));
        let t = Instant::now();
        Engine::synth_inner(&mut l, &ch[0], 2.2, true).unwrap();
        b.push(ms(t.elapsed()));
    }
    println!("first chunk on a fresh session:          {}", pct(&a));
    println!("warm-up {warm_text:?}:                    {}", pct(&wu));
    println!("first chunk after the warm-up:           {}", pct(&b));
}

/// Reading never repeats a chunk, so every chunk is a new input length. Per config and
/// round, a fresh session reads a 12-word first chunk and then 12 distinct chunks (each
/// length seen once); configs are interleaved round by round (shared machine).
/// RA_SWEEP as in `tts_sweep` (threads:spin:mem_pattern), RA_ROUNDS (default 2).
/// cargo test --release ra_tts::bench::tts_shapes -- --ignored --nocapture --test-threads=1
#[test]
#[ignore]
fn tts_shapes() {
    let words: Vec<&str> = PASSAGE.split_whitespace().collect();
    // 12 chunks of 18..=29 words at different offsets: all different id lengths
    let ch: Vec<Vec<&str>> = (0..12).map(|k| words[k * 3..k * 3 + 18 + k].to_vec()).collect();
    let first = chunks()[0].clone();
    let e = Engine::new(data_dir());
    e.preload("af_heart", "cpu");
    let eng = e.acquire().expect("engine");
    let mut l = eng.lock();
    let f = ra_model::files(&data_dir());
    let configs: Vec<CpuOpts> = std::env::var("RA_SWEEP")
        .unwrap_or_else(|_| "8:1:1,8:0:1,8:0:0".into())
        .split(',')
        .filter_map(parse_opts)
        .collect();
    let rounds: usize = std::env::var("RA_ROUNDS").ok().and_then(|v| v.parse().ok()).unwrap_or(2);
    let mut firsts: Vec<Vec<f64>> = vec![Vec::new(); configs.len()];
    let mut st: Vec<Vec<ChunkStat>> = configs.iter().map(|_| Vec::new()).collect();
    for _ in 0..rounds {
        for (i, o) in configs.iter().enumerate() {
            l.session = open_session_with(&f.fp32, false, *o).unwrap();
            let t = Instant::now();
            Engine::synth_inner(&mut l, &first, 2.2, true).unwrap();
            firsts[i].push(ms(t.elapsed()));
            st[i].extend(ch.iter().map(|c| run_chunk(&mut l, c)));
        }
    }
    for (i, o) in configs.iter().enumerate() {
        println!("--- {o:?}: first chunk (12 words, fresh session) {}", pct(&firsts[i]));
        summary("    new lengths", &st[i]);
    }
}
