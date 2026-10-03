//! Read Aloud — Kokoro-82M via ONNX Runtime. Loaded on demand (hotkey press),
//! DirectML first with CPU fallback, dropped after 2 idle minutes. Nothing is
//! loaded (0 MB) until `preload` is called; the idle-watch thread exits once
//! the engine is unloaded or replaced.
#![allow(dead_code)] // consumed by the Read Aloud controller (later task)
use crate::{
    ra_model,
    ra_timing::{self, WordTime},
    ra_wsola,
};
use misaki_rs::{Language, G2P};
use ort::{ep, session::Session, value::Tensor};
use parking_lot::{Condvar, Mutex};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const IDLE: Duration = Duration::from_secs(120);
const IDLE_POLL: Duration = Duration::from_secs(5);
const LOAD_WAIT: Duration = Duration::from_secs(60);
const MAX_IDS: usize = 510;
const OFFSET_MS: f32 = 36.0;
const STYLE_DIM: usize = 256;
/// Marks a phoneme token (punctuation) that belongs to no word: fed to the model
/// for prosody, but gets no span.
const NO_WORD: usize = usize::MAX;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Device {
    Gpu,
    Cpu,
}

pub struct ChunkAudio {
    pub pcm: Vec<f32>,
    pub words: Vec<WordTime>,
    pub device: Device,
}

struct Loaded {
    session: Session,
    device: Device,
    vocab: HashMap<char, i64>,
    voice: Vec<f32>, // rows of 256
    g2p: G2P,
}

enum Slot {
    Empty,
    Loading,
    /// The model sits behind its own lock so status calls / preload never wait on inference.
    Ready { eng: Arc<Mutex<Loaded>>, device: Device, voice: String, gen: u64 },
    Failed(String),
}

pub struct Engine {
    data_dir: PathBuf,
    slot: Mutex<Slot>,
    ready: Condvar,
    last_used: Mutex<Instant>,
    busy: AtomicU32,
    /// Bumped on every load; stale loaders / idle watchers compare and bow out.
    gen: AtomicU64,
    idle: Duration,
    idle_poll: Duration,
    /// Live idle-watch threads (observable so tests can prove they exit).
    watchers: AtomicU32,
}

pub fn should_unload(last_used: Instant, now: Instant, busy: bool) -> bool {
    idle_expired(last_used, now, busy, IDLE)
}

fn idle_expired(last_used: Instant, now: Instant, busy: bool, idle: Duration) -> bool {
    !busy && now.saturating_duration_since(last_used) > idle
}

/// `ph` = (word index, phonemes, followed-by-whitespace) per token. Returns ids
/// (no pads) and one (word, start, end) span per token that produced ids.
pub fn build_ids(ph: &[(usize, String, bool)], vocab: &HashMap<char, i64>) -> (Vec<i64>, Vec<(usize, usize, usize)>) {
    let mut ids = Vec::new();
    let mut spans = Vec::new();
    let space = vocab.get(&' ').copied();
    for (w, p, ws) in ph {
        if ids.len() >= MAX_IDS {
            break;
        }
        let s = ids.len();
        for c in p.chars() {
            if ids.len() >= MAX_IDS {
                break;
            }
            if let Some(&id) = vocab.get(&c) {
                ids.push(id);
            }
        }
        if ids.len() > s {
            spans.push((*w, s, ids.len()));
        }
        if *ws && ids.len() < MAX_IDS {
            if let Some(sp) = space {
                ids.push(sp);
            }
        }
    }
    (ids, spans)
}

/// Rate actually applied: WSOLA passes audio through untouched outside (1, 4],
/// so word times must use the same effective rate.
fn effective_rate(rate: f32) -> f32 {
    if rate.is_finite() && rate > 1.0001 {
        rate.min(4.0)
    } else {
        1.0
    }
}

fn load_vocab(path: &Path) -> Result<HashMap<char, i64>, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("tokenizer: {e}"))?;
    let v: serde_json::Value = serde_json::from_str(&raw).map_err(|e| format!("tokenizer: {e}"))?;
    let map = v["model"]["vocab"].as_object().ok_or("tokenizer.json: no model.vocab")?;
    let vocab: HashMap<char, i64> = map
        .iter()
        .filter_map(|(k, v)| {
            let mut cs = k.chars();
            let c = cs.next()?;
            if cs.next().is_some() {
                return None;
            }
            Some((c, v.as_i64()?))
        })
        .collect();
    if vocab.is_empty() {
        return Err("tokenizer.json: empty vocab".into());
    }
    Ok(vocab)
}

fn load_voice(path: &Path) -> Result<Vec<f32>, String> {
    let b = std::fs::read(path).map_err(|e| format!("voice {}: {e}", path.display()))?;
    let v: Vec<f32> = b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    if v.len() < STYLE_DIM {
        return Err(format!("voice {}: too short", path.display()));
    }
    Ok(v)
}

fn load_voice_or_default(dir: &Path, voice: &str) -> Result<Vec<f32>, String> {
    load_voice(&dir.join(format!("{voice}.bin"))).or_else(|e| {
        log::warn!("read-aloud: {e}; falling back to af_heart");
        load_voice(&dir.join("af_heart.bin"))
    })
}

fn open_session(path: &Path, gpu: bool) -> Result<Session, String> {
    // CPU arena off: the arena keeps its high-water mark until unload. Measured on the
    // CPU path: steady private 705 -> 455 MB; costs ~0.5 s on a 27-word chunk's first run.
    let cpu = ep::CPU::default().with_arena_allocator(false).build();
    let s = |e: ort::Error<_>| e.to_string();
    let b = Session::builder().map_err(|e| e.to_string())?;
    let mut b = if gpu {
        let b = b.with_execution_providers([ep::DirectML::default().build().error_on_failure(), cpu]).map_err(s)?;
        let b = b.with_memory_pattern(false).map_err(s)?;
        b.with_parallel_execution(false).map_err(s)?
    } else {
        let b = b.with_execution_providers([cpu]).map_err(s)?;
        b.with_intra_threads(num_cpus::get_physical().clamp(1, 8)).map_err(s)?
    };
    b.commit_from_file(path).map_err(|e| e.to_string())
}

impl Engine {
    pub fn new(data_dir: PathBuf) -> Arc<Self> {
        Self::with_idle(data_dir, IDLE, IDLE_POLL)
    }

    fn with_idle(data_dir: PathBuf, idle: Duration, idle_poll: Duration) -> Arc<Self> {
        Arc::new(Self {
            data_dir,
            slot: Mutex::new(Slot::Empty),
            ready: Condvar::new(),
            last_used: Mutex::new(Instant::now()),
            busy: AtomicU32::new(0),
            gen: AtomicU64::new(0),
            idle,
            idle_poll,
            watchers: AtomicU32::new(0),
        })
    }

    pub fn is_loaded(&self) -> bool {
        matches!(*self.slot.lock(), Slot::Ready { .. })
    }

    pub fn device(&self) -> Option<Device> {
        match &*self.slot.lock() {
            Slot::Ready { device, .. } => Some(*device),
            _ => None,
        }
    }

    pub fn touch(&self) {
        *self.last_used.lock() = Instant::now();
    }

    /// Non-blocking: start loading if not loaded. `pref` = "auto" | "gpu" | "cpu".
    /// If already loaded with a different voice, the voice is swapped in place
    /// (`pref` only applies to a fresh load).
    pub fn preload(self: &Arc<Self>, voice: &str, pref: &str) {
        self.touch();
        let mut s = self.slot.lock();
        match &mut *s {
            Slot::Loading => return,
            Slot::Ready { eng, voice: cur, .. } => {
                if cur != voice {
                    *cur = voice.to_string();
                    self.spawn_voice_swap(Arc::clone(eng), voice.to_string());
                }
                return;
            }
            Slot::Empty | Slot::Failed(_) => {}
        }
        *s = Slot::Loading;
        let gen = self.gen.fetch_add(1, Ordering::SeqCst) + 1;
        drop(s);
        let me = Arc::clone(self);
        let (voice, pref) = (voice.to_string(), pref.to_string());
        let spawned = std::thread::Builder::new().name("ra-tts-load".into()).spawn(move || {
            let r = me.load(&voice, &pref);
            let mut s = me.slot.lock();
            if me.gen.load(Ordering::SeqCst) != gen {
                return; // superseded (reset_to_cpu during load): drop this result
            }
            let ok = match r {
                Ok(l) => {
                    let device = l.device;
                    *s = Slot::Ready { eng: Arc::new(Mutex::new(l)), device, voice, gen };
                    true
                }
                Err(e) => {
                    log::warn!("read-aloud: engine load failed: {e}");
                    *s = Slot::Failed(e);
                    false
                }
            };
            me.ready.notify_all();
            drop(s);
            if ok {
                me.spawn_idle_watch(gen);
            }
        });
        if let Err(e) = spawned {
            // never leave Loading behind with nobody to finish it (synth would wait)
            let mut s = self.slot.lock();
            if self.gen.load(Ordering::SeqCst) == gen {
                *s = Slot::Failed(format!("spawn load thread: {e}"));
                self.ready.notify_all();
            }
        }
    }

    /// After a DirectML runtime error: drop the session and reload on CPU
    /// (non-blocking; the next `synth` waits for it).
    pub fn reset_to_cpu(self: &Arc<Self>, voice: &str) {
        *self.slot.lock() = Slot::Empty;
        self.preload(voice, "cpu");
    }

    fn load(&self, voice: &str, pref: &str) -> Result<Loaded, String> {
        let f = ra_model::files(&self.data_dir);
        let vocab = load_vocab(&f.tokenizer)?;
        let voice = load_voice_or_default(&f.voices_dir, voice)?;
        let t = Instant::now();
        // G2P init (lexicon + tagger parse) overlaps the session open
        let g2p_job = std::thread::Builder::new()
            .name("ra-tts-g2p".into())
            .spawn(|| G2P::new(Language::EnglishUS))
            .map_err(|e| format!("spawn g2p: {e}"))?;
        let (session, device) = match pref {
            "cpu" => (open_session(&f.fp32, false)?, Device::Cpu),
            _ => match open_session(&f.q8_dml, true) {
                Ok(s) => (s, Device::Gpu),
                Err(e) if pref != "gpu" => {
                    log::warn!("read-aloud: DirectML failed ({e}); using CPU");
                    (open_session(&f.fp32, false)?, Device::Cpu)
                }
                Err(e) => return Err(format!("dml: {e}")),
            },
        };
        let g2p = g2p_job.join().map_err(|_| "g2p init failed".to_string())?;
        log::info!("read-aloud: engine loaded on {:?} in {:?}", device, t.elapsed());
        Ok(Loaded { session, device, vocab, voice, g2p })
    }

    fn spawn_voice_swap(&self, eng: Arc<Mutex<Loaded>>, voice: String) {
        let dir = ra_model::files(&self.data_dir).voices_dir;
        let _ = std::thread::Builder::new().name("ra-tts-voice".into()).spawn(move || {
            match load_voice_or_default(&dir, &voice) {
                Ok(v) => eng.lock().voice = v,
                Err(e) => log::warn!("read-aloud: voice swap failed: {e}"),
            }
        });
    }

    /// One watcher per load generation; exits after unloading, or as soon as the
    /// engine it watches is gone or replaced.
    fn spawn_idle_watch(self: &Arc<Self>, gen: u64) {
        let me = Arc::clone(self);
        self.watchers.fetch_add(1, Ordering::SeqCst);
        let spawned = std::thread::Builder::new().name("ra-tts-idle".into()).spawn(move || {
            loop {
                std::thread::sleep(me.idle_poll);
                let mut s = me.slot.lock();
                match &*s {
                    Slot::Ready { gen: g, .. } if *g == gen => {}
                    _ => break,
                }
                let last = *me.last_used.lock();
                if idle_expired(last, Instant::now(), me.busy.load(Ordering::SeqCst) > 0, me.idle) {
                    *s = Slot::Empty; // in-flight holders keep their Arc; memory frees when they finish
                    log::info!("read-aloud: engine unloaded after idle");
                    break;
                }
            }
            me.watchers.fetch_sub(1, Ordering::SeqCst);
        });
        if spawned.is_err() {
            self.watchers.fetch_sub(1, Ordering::SeqCst);
            log::warn!("read-aloud: idle watch not started; engine stays loaded");
        }
    }

    /// Waits for the current load (if any), then hands out the engine.
    fn acquire(&self) -> Result<Arc<Mutex<Loaded>>, String> {
        let deadline = Instant::now() + LOAD_WAIT;
        let mut s = self.slot.lock();
        loop {
            match &*s {
                Slot::Ready { eng, .. } => return Ok(Arc::clone(eng)),
                Slot::Failed(e) => return Err(e.clone()),
                Slot::Empty => return Err("engine not loaded".into()),
                Slot::Loading => {
                    if self.ready.wait_until(&mut s, deadline).timed_out() && matches!(*s, Slot::Loading) {
                        return Err("engine load timed out".into());
                    }
                }
            }
        }
    }

    /// Blocks until loaded (or failed). Synthesizes one chunk at 1.0x, then stretches to `rate`.
    /// A DirectML runtime failure comes back as `Err("dml: …")` (caller may `reset_to_cpu`).
    pub fn synth(&self, words: &[&str], rate: f32) -> Result<ChunkAudio, String> {
        self.busy.fetch_add(1, Ordering::SeqCst);
        self.touch();
        let r = self.acquire().and_then(|eng| Self::synth_with(&mut eng.lock(), words, rate));
        self.touch();
        self.busy.fetch_sub(1, Ordering::SeqCst);
        r
    }

    fn synth_with(l: &mut Loaded, words: &[&str], rate: f32) -> Result<ChunkAudio, String> {
        let device = l.device;
        let rate = effective_rate(rate);
        let empty = || ChunkAudio { pcm: Vec::new(), words: Vec::new(), device };
        let text = words.join(" ");
        if text.trim().is_empty() {
            return Ok(empty());
        }
        let (_, tokens) = l.g2p.g2p(&text).map_err(|e| format!("g2p: {e:?}"))?;
        let tok_text: Vec<&str> = tokens.iter().map(|t| t.text.as_str()).collect();
        let owner = ra_timing::align(words, &tok_text);
        // every token with phonemes feeds the model (punctuation shapes prosody);
        // only word-owned ones get spans
        let ph: Vec<(usize, String, bool)> = tokens
            .iter()
            .zip(owner.iter())
            .filter_map(|(t, o)| Some((o.unwrap_or(NO_WORD), t.phonemes.clone()?, !t.whitespace.is_empty())))
            .collect();
        let (ids, spans) = build_ids(&ph, &l.vocab);
        if ids.is_empty() {
            return Ok(empty());
        }
        let rows = l.voice.len() / STYLE_DIM;
        if rows == 0 {
            return Err("voice: no style rows".into());
        }
        let row = ids.len().min(rows - 1);
        let style = l.voice[row * STYLE_DIM..(row + 1) * STYLE_DIM].to_vec();
        let mut input = Vec::with_capacity(ids.len() + 2);
        input.push(0i64);
        input.extend_from_slice(&ids);
        input.push(0);
        let n_in = input.len();
        let tag = |e: String| if device == Device::Gpu { format!("dml: {e}") } else { e };
        let t_ids = Tensor::from_array(([1usize, n_in], input)).map_err(|e| e.to_string())?;
        let t_style = Tensor::from_array(([1usize, STYLE_DIM], style)).map_err(|e| e.to_string())?;
        let t_speed = Tensor::from_array(([1usize], vec![1.0f32])).map_err(|e| e.to_string())?;
        let (wav, dur) = {
            let out = l
                .session
                .run(ort::inputs!["input_ids" => t_ids, "style" => t_style, "speed" => t_speed])
                .map_err(|e| tag(e.to_string()))?;
            let w = out.get("waveform").ok_or("model output: no waveform")?;
            let d = out.get("durations").ok_or("model output: no durations")?;
            let (_, wav) = w.try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
            let (_, dur) = d.try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
            (wav.to_vec(), dur.to_vec())
        };
        if dur.len() != n_in {
            return Err(format!("model output: {} durations for {} ids", dur.len(), n_in));
        }
        // one span per word: merge consecutive token spans of the same word, drop unowned
        let mut merged: Vec<(usize, usize, usize)> = Vec::new();
        for sp in spans.into_iter().filter(|sp| sp.0 != NO_WORD) {
            match merged.last_mut() {
                Some(m) if m.0 == sp.0 => m.2 = sp.2,
                _ => merged.push(sp),
            }
        }
        let words_t = ra_timing::word_times(&dur, &merged, wav.len(), OFFSET_MS, rate);
        let pcm = ra_wsola::stretch(&wav, rate, ra_timing::SR);
        Ok(ChunkAudio { pcm, words: words_t, device })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn vocab() -> HashMap<char, i64> {
        [('h', 1), ('ə', 2), ('l', 3), ('o', 4), (' ', 16)].into_iter().collect()
    }

    #[test]
    fn ids_and_spans() {
        let (ids, spans) = build_ids(&[(0, "həl".into(), true), (1, "lo".into(), false)], &vocab());
        assert_eq!(ids, vec![1, 2, 3, 16, 3, 4]);
        assert_eq!(spans, vec![(0, 0, 3), (1, 4, 6)]);
    }

    #[test]
    fn unknown_phoneme_chars_dropped() {
        let (ids, _) = build_ids(&[(0, "hxz".into(), false)], &vocab());
        assert_eq!(ids, vec![1]);
    }

    #[test]
    fn ids_truncated_to_510() {
        let long: String = std::iter::repeat('h').take(600).collect();
        let (ids, spans) = build_ids(&[(0, long, false)], &vocab());
        assert_eq!(ids.len(), 510);
        assert_eq!(spans[0].2, 510);
    }

    #[test]
    fn unload_after_two_idle_minutes() {
        let t0 = Instant::now();
        assert!(!should_unload(t0, t0 + Duration::from_secs(119), false));
        assert!(should_unload(t0, t0 + Duration::from_secs(121), false));
        assert!(!should_unload(t0, t0 + Duration::from_secs(600), true));
    }

    #[test]
    fn effective_rate_matches_wsola_passthrough() {
        assert_eq!(effective_rate(2.2), 2.2);
        assert_eq!(effective_rate(0.5), 1.0);
        assert_eq!(effective_rate(f32::NAN), 1.0);
        assert_eq!(effective_rate(9.0), 4.0);
    }

    #[test]
    fn synth_without_preload_errors_and_loads_nothing() {
        let e = Engine::new(std::env::temp_dir().join("ra-tts-none"));
        assert!(!e.is_loaded());
        assert_eq!(e.device(), None);
        assert!(e.synth(&["hi"], 1.0).is_err());
    }

    #[test]
    fn failed_load_reports_error_not_hang() {
        let e = Engine::new(std::env::temp_dir().join(format!("ra-tts-missing-{}", std::process::id())));
        e.preload("af_heart", "auto");
        let r = e.synth(&["hi"], 1.0);
        assert!(r.is_err(), "missing model must fail");
        assert!(!e.is_loaded());
    }

    const SENT: [&str; 4] = ["Eight", "times", "a", "year,"];

    fn check(a: &ChunkAudio, rate: f32) {
        assert_eq!(a.words.len(), 4);
        assert!(a.words.windows(2).all(|w| w[0].start <= w[1].start && w[0].end <= w[1].start + 1));
        assert!(a.words.iter().all(|w| w.start <= w.end));
        let last = a.words[3].end as i64;
        let frame = (600.0 / rate).ceil() as i64;
        assert!((last - a.pcm.len() as i64).abs() <= frame, "last end {last} vs pcm {}", a.pcm.len());
    }

    fn write_wav(pcm: &[f32]) -> PathBuf {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target").join("ra_tts_sample.wav");
        let spec = hound::WavSpec { channels: 1, sample_rate: ra_timing::SR, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create(&p, spec).unwrap();
        for s in pcm {
            w.write_sample((s.clamp(-1.0, 1.0) * 32767.0) as i16).unwrap();
        }
        w.finalize().unwrap();
        p
    }

    /// cargo test ra_tts::tests::synth_real -- --ignored --nocapture
    #[test]
    #[ignore]
    fn synth_real() {
        let e = Engine::new(dirs::data_dir().unwrap().join("ALAN Echo"));
        e.preload("af_heart", "auto");
        let t = Instant::now();
        let a = e.synth(&SENT, 2.2).unwrap();
        println!("first chunk {:?} on {:?}, {} samples, words {:?}", t.elapsed(), e.device(), a.pcm.len(), a.words);
        assert_eq!(e.device(), Some(Device::Gpu));
        check(&a, 2.2);
        let t = Instant::now();
        let b = e.synth(&SENT, 2.2).unwrap();
        println!("warm chunk {:?}", t.elapsed());
        check(&b, 2.2);
        let long: Vec<&str> = "The quick brown fox jumps over the lazy dog, and then it runs back home before the rain starts."
            .split(' ')
            .collect();
        let t = Instant::now();
        let c = e.synth(&long, 2.2).unwrap();
        println!("warm 19-word chunk {:?}, {} words timed", t.elapsed(), c.words.len());
        println!("wav: {}", write_wav(&a.pcm).display());
        if std::env::var("RA_HOLD").is_ok() {
            println!("HOLD"); // steady-state RAM sample window for an external poller
            std::thread::sleep(Duration::from_secs(3));
        }
    }

    /// cargo test ra_tts::tests::synth_cpu_real -- --ignored --nocapture
    #[test]
    #[ignore]
    fn synth_cpu_real() {
        let e = Engine::new(dirs::data_dir().unwrap().join("ALAN Echo"));
        e.preload("af_heart", "cpu");
        let t = Instant::now();
        let a = e.synth(&SENT, 2.2).unwrap();
        println!("cpu first chunk {:?} on {:?}, {} samples, words {:?}", t.elapsed(), e.device(), a.pcm.len(), a.words);
        assert_eq!(e.device(), Some(Device::Cpu));
        check(&a, 2.2);
        let t = Instant::now();
        let _ = e.synth(&SENT, 2.2).unwrap();
        println!("cpu warm chunk {:?}", t.elapsed());
        if std::env::var("RA_HOLD").is_ok() {
            println!("HOLD");
            std::thread::sleep(Duration::from_secs(3));
        }
    }

    /// GPU load, then the DML-error recovery path: reset_to_cpu, next synth waits and runs on CPU.
    #[test]
    #[ignore]
    fn reset_to_cpu_real() {
        let e = Engine::new(dirs::data_dir().unwrap().join("ALAN Echo"));
        e.preload("af_heart", "auto");
        assert_eq!(e.synth(&SENT, 2.2).unwrap().device, Device::Gpu);
        e.reset_to_cpu("af_heart");
        let a = e.synth(&SENT, 2.2).unwrap();
        assert_eq!(a.device, Device::Cpu);
        assert_eq!(e.device(), Some(Device::Cpu));
        check(&a, 2.2);
        // voice swap while loaded applies without a reload
        e.preload("am_michael", "auto");
        std::thread::sleep(Duration::from_millis(300));
        let b = e.synth(&SENT, 2.2).unwrap();
        assert_eq!(b.device, Device::Cpu);
        assert_ne!(a.pcm, b.pcm, "voice swap should change audio");
    }

    /// Idle unload for real (short idle window): engine drops and the watcher thread exits.
    #[test]
    #[ignore]
    fn idle_unload_real() {
        let e = Engine::with_idle(dirs::data_dir().unwrap().join("ALAN Echo"), Duration::from_millis(800), Duration::from_millis(100));
        e.preload("af_heart", "cpu");
        e.synth(&SENT, 2.2).unwrap();
        assert!(e.is_loaded());
        assert_eq!(e.watchers.load(Ordering::SeqCst), 1);
        std::thread::sleep(Duration::from_millis(2000));
        assert!(!e.is_loaded(), "should unload after idle");
        assert_eq!(e.watchers.load(Ordering::SeqCst), 0, "idle watcher must exit");
        assert!(e.synth(&SENT, 2.2).is_err(), "unloaded engine needs a new preload");
        e.preload("af_heart", "cpu");
        assert!(e.synth(&SENT, 2.2).is_ok());
    }

    /// Two synths in flight while the engine is still loading, plus repeated preloads: no deadlock.
    #[test]
    #[ignore]
    fn concurrent_synth_real() {
        let e = Engine::new(dirs::data_dir().unwrap().join("ALAN Echo"));
        e.preload("af_heart", "auto");
        let hs: Vec<_> = (0..2)
            .map(|_| {
                let e = Arc::clone(&e);
                std::thread::spawn(move || e.synth(&SENT, 2.2).map(|a| a.words.len()))
            })
            .collect();
        for _ in 0..5 {
            e.preload("af_heart", "auto");
            let _ = (e.is_loaded(), e.device());
        }
        for h in hs {
            assert_eq!(h.join().unwrap(), Ok(4));
        }
    }
}
