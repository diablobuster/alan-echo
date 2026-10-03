//! Read Aloud — Kokoro-82M via ONNX Runtime. Loaded on demand (hotkey press or
//! first `synth`), dropped after 2 idle minutes. Nothing is loaded (0 MB) until
//! asked; the idle-watch thread exits once the engine is unloaded or replaced.
//!
//! Device policy (controller ruling, Task 5 review): `pref` "auto" (and anything
//! but "gpu") runs on CPU (fp32): it frees fully on unload and stays < 500 MB.
//! DirectML (q8) is used only for `pref == "gpu"`; if it fails to open or to run,
//! the engine falls back to CPU by itself.
use crate::{
    ra_g2pen::G2pEn,
    ra_model,
    ra_names::{self, Lex, Lts, NameFallback, Names},
    ra_text,
    ra_timing::{self, WordTime},
    ra_wsola,
};
use misaki_rs::{Language, Lexicon, G2P};
use ort::{ep, session::Session, value::Tensor};
use parking_lot::{Condvar, Mutex, MutexGuard};
use std::borrow::Cow;
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
/// Prefix of the error for missing model files (the controller shows a fix-it message).
pub const MISSING: &str = "voice files missing";

/// misaki-rs (espeak-derived dictionaries) writes diphthongs and affricates as two
/// symbols joined by U+200D; Kokoro's vocab has one symbol for each (misaki's own
/// notation). Without this, the vocab filter split them into two separate sounds.
/// All seven targets are in Kokoro's tokenizer.json (A 24, I 25, O 31, W 39, Y 41,
/// ʤ 82, ʧ 133).
const JOINED: [(&str, char); 7] = [
    ("e\u{200D}ɪ", 'A'),
    ("a\u{200D}ɪ", 'I'),
    ("o\u{200D}ʊ", 'O'),
    ("a\u{200D}ʊ", 'W'),
    ("ɔ\u{200D}ɪ", 'Y'),
    ("d\u{200D}ʒ", 'ʤ'),
    ("t\u{200D}ʃ", 'ʧ'),
];

/// Kokoro's phoneme spelling of a misaki-rs phoneme string: joined pairs mapped to
/// their single vocab symbol, any other U+200D dropped.
pub fn kokoro_phonemes(p: &str) -> String {
    if !p.contains('\u{200D}') {
        return p.to_string();
    }
    let mut s = p.to_string();
    for (pair, one) in JOINED {
        if s.contains(pair) {
            s = s.replace(pair, one.encode_utf8(&mut [0; 4]));
        }
    }
    s.retain(|c| c != '\u{200D}');
    s
}

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
    /// Leading chunk words fully synthesized. Words `covered..` were NOT spoken
    /// (510-id model limit); the caller re-queues them.
    pub covered: usize,
}

struct Loaded {
    session: Session,
    device: Device,
    vocab: HashMap<char, i64>,
    voice: Vec<f32>, // rows of 256
    g2p: G2P,
    /// CMUdict + g2p_en, built with the engine and freed with it on idle unload.
    names: Arc<Names>,
}

enum Slot {
    Empty,
    /// `cpu`: the load in flight targets CPU (makes `reset_to_cpu` idempotent).
    Loading { cpu: bool },
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
    /// Last (voice, pref) asked for: used by self-reload and to apply a voice
    /// change requested while a load was in flight. Lock order: slot, then want.
    want: Mutex<(String, String)>,
    idle: Duration,
    idle_poll: Duration,
    /// Live idle-watch threads (observable so tests can prove they exit).
    watchers: AtomicU32,
    /// Loads started (observable so tests can prove reset_to_cpu is idempotent).
    loads: AtomicU32,
}

#[cfg_attr(not(test), allow(dead_code))] // test/diagnostic API
pub fn should_unload(last_used: Instant, now: Instant, busy: bool) -> bool {
    idle_expired(last_used, now, busy, IDLE)
}

fn idle_expired(last_used: Instant, now: Instant, busy: bool, idle: Duration) -> bool {
    !busy && now.saturating_duration_since(last_used) > idle
}

/// Only an explicit "gpu" uses DirectML; "auto" and everything else is CPU.
fn wants_gpu(pref: &str) -> bool {
    pref == "gpu"
}

/// `ph` = (word index or NO_WORD, phonemes, followed-by-whitespace) per token, in
/// order; `n_words` = words in the chunk. Returns ids (no pads), one
/// (word, start, end) span per token that produced ids, and `covered` = leading
/// words fully included. Caps at 510 ids without cutting a word mid-phonemes:
/// on overflow it stops after the last whole word, except that a first word that
/// alone overflows is kept truncated (covered = 1) so callers always progress.
#[cfg_attr(not(test), allow(dead_code))] // the synth path calls build_ids_with
pub fn build_ids(ph: &[(usize, String, bool)], vocab: &HashMap<char, i64>, n_words: usize) -> (Vec<i64>, Vec<(usize, usize, usize)>, usize) {
    build_ids_with(ph, vocab, n_words, true)
}

/// `build_ids`; `map_joined: false` reproduces the old (unmapped) ids for the A/B test.
fn build_ids_with(
    ph: &[(usize, String, bool)],
    vocab: &HashMap<char, i64>,
    n_words: usize,
    map_joined: bool,
) -> (Vec<i64>, Vec<(usize, usize, usize)>, usize) {
    let mut ids: Vec<i64> = Vec::new();
    let mut spans = Vec::new();
    let space = vocab.get(&' ').copied();
    let mut cur: Option<usize> = None; // word being added
    let mut safe = (0usize, 0usize, 0usize); // (ids, spans, covered) after the last whole word
    for (w, p, ws) in ph {
        if *w != NO_WORD && Some(*w) != cur {
            if let Some(c) = cur {
                safe = (ids.len(), spans.len(), c + 1);
            }
            cur = Some(*w);
        }
        let tok: Vec<i64> = if map_joined { kokoro_phonemes(p) } else { p.clone() }
            .chars()
            .filter_map(|c| vocab.get(&c).copied())
            .collect();
        if ids.len() + tok.len() > MAX_IDS {
            if safe.2 == 0 {
                // nothing whole yet: keep the truncated first word to guarantee progress
                let s = ids.len();
                ids.extend(tok.iter().take(MAX_IDS - s));
                if ids.len() > s {
                    spans.push((*w, s, ids.len()));
                }
                return (ids, spans, cur.map_or(1, |c| c + 1));
            }
            ids.truncate(safe.0);
            spans.truncate(safe.1);
            return (ids, spans, safe.2);
        }
        if !tok.is_empty() {
            let s = ids.len();
            ids.extend_from_slice(&tok);
            spans.push((*w, s, ids.len()));
        }
        if *ws && ids.len() < MAX_IDS {
            if let Some(sp) = space {
                ids.push(sp);
            }
        }
    }
    (ids, spans, n_words)
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

/// `Err("voice files missing: …")` when a model file is absent.
fn require(p: &Path) -> Result<(), String> {
    if p.is_file() {
        Ok(())
    } else {
        Err(format!("{MISSING}: {}", p.display()))
    }
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

/// CPU session knobs (the bench sweeps them; production uses `CpuOpts::default()`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct CpuOpts {
    pub intra: usize,
    pub spin: bool,
    pub mem_pattern: bool,
    /// Graph optimisation: 0 = ONNX Runtime's default (all), 1 basic, 2 extended,
    /// 3 layout, 4 all, 9 none. Bench only: production always uses the default.
    #[cfg(test)]
    pub opt: u8,
}

/// Measured (Task 8, ra_tts::bench::tts_shapes, interleaved on a shared machine; every
/// reading chunk is a new input length): spinning off + memory pattern off vs ORT's
/// defaults: CPU per second of speech 5.4 -> 2.8 s, RTF at 2.2x 0.75 -> 0.58, first
/// chunk no slower (1154 -> 1118 ms). Spinning idle threads burn cores between ops;
/// the memory pattern only pays off when an input length repeats, which reading never does.
///
/// Memory pattern trade-off (controller ruling, Task 8 review): with the pattern ON, a
/// chunk whose input length was seen before runs ~300 ms faster (an exact replay of the
/// same 12-word chunk: ~700 vs ~1045 ms), but every NEW length pays extra to plan and
/// record its pattern, which nearly doubled CPU per second of speech. A read never
/// repeats a chunk, and a jump re-chunks from the clicked word, so a jump back almost
/// always gives new lengths too (measured: no slower with it OFF, see `tts_jump_back`).
/// Only re-reading from the exact same start word replays the same shapes, so the
/// pattern stays OFF.
impl Default for CpuOpts {
    fn default() -> Self {
        CpuOpts {
            intra: num_cpus::get_physical().clamp(1, 8),
            spin: false,
            mem_pattern: false,
            #[cfg(test)]
            opt: 0,
        }
    }
}

fn open_session(path: &Path, gpu: bool) -> Result<Session, String> {
    open_session_with(path, gpu, CpuOpts::default())
}

fn open_session_with(path: &Path, gpu: bool, o: CpuOpts) -> Result<Session, String> {
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
        let b = b.with_intra_op_spinning(o.spin).map_err(s)?;
        let b = b.with_memory_pattern(o.mem_pattern).map_err(s)?;
        #[cfg(test)]
        let b = {
            use ort::session::builder::GraphOptimizationLevel as G;
            match o.opt {
                1 => b.with_optimization_level(G::Level1).map_err(s)?,
                2 => b.with_optimization_level(G::Level2).map_err(s)?,
                3 => b.with_optimization_level(G::Level3).map_err(s)?,
                4 => b.with_optimization_level(G::All).map_err(s)?,
                9 => b.with_optimization_level(G::Disable).map_err(s)?,
                _ => b,
            }
        };
        b.with_intra_threads(o.intra).map_err(s)?
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
            want: Mutex::new(("af_heart".into(), "auto".into())),
            idle,
            idle_poll,
            watchers: AtomicU32::new(0),
            loads: AtomicU32::new(0),
        })
    }

    #[cfg_attr(not(test), allow(dead_code))] // test/diagnostic API
    pub fn is_loaded(&self) -> bool {
        matches!(*self.slot.lock(), Slot::Ready { .. })
    }

    pub fn device(&self) -> Option<Device> {
        match &*self.slot.lock() {
            Slot::Ready { device, .. } => Some(*device),
            _ => None,
        }
    }

    /// Restarts the idle countdown. `synth` touches on entry and exit; the
    /// controller should also touch when playback of the last chunk ends.
    pub fn touch(&self) {
        *self.last_used.lock() = Instant::now();
    }

    /// Non-blocking: start loading if not loaded. `pref` = "auto" | "cpu" (CPU) or
    /// "gpu" (DirectML, CPU fallback). Remembers voice/pref for self-reload. A
    /// different voice is swapped in place if loaded, or applied when the load in
    /// flight finishes. `pref` only applies to a fresh load.
    pub fn preload(self: &Arc<Self>, voice: &str, pref: &str) {
        self.touch();
        let mut s = self.slot.lock();
        *self.want.lock() = (voice.to_string(), pref.to_string());
        match &mut *s {
            Slot::Loading { .. } => {} // voice applied by finish_load
            Slot::Ready { eng, voice: cur, .. } => {
                if cur != voice {
                    *cur = voice.to_string();
                    self.spawn_voice_swap(Arc::clone(eng), voice.to_string());
                }
            }
            Slot::Empty | Slot::Failed(_) => self.start_load(&mut s, voice.to_string(), pref.to_string()),
        }
    }

    /// After a DirectML runtime error: switch to CPU in one step (non-blocking; the
    /// next `synth` waits for it). No-op if already on, or loading for, CPU.
    pub fn reset_to_cpu(self: &Arc<Self>, voice: &str) {
        let mut s = self.slot.lock();
        if matches!(*s, Slot::Ready { device: Device::Cpu, .. } | Slot::Loading { cpu: true }) {
            return;
        }
        *self.want.lock() = (voice.to_string(), "cpu".to_string());
        self.start_load(&mut s, voice.to_string(), "cpu".to_string());
    }

    /// Caller holds the slot lock: mark Loading and spawn the loader.
    fn start_load(self: &Arc<Self>, s: &mut MutexGuard<'_, Slot>, voice: String, pref: String) {
        **s = Slot::Loading { cpu: !wants_gpu(&pref) };
        let gen = self.gen.fetch_add(1, Ordering::SeqCst) + 1;
        self.loads.fetch_add(1, Ordering::SeqCst);
        let me = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("ra-tts-load".into())
            .spawn(move || me.finish_load(gen, voice, pref));
        if let Err(e) = spawned {
            // never leave Loading behind with nobody to finish it (synth would wait)
            **s = Slot::Failed(format!("spawn load thread: {e}"));
            self.ready.notify_all();
        }
    }

    fn finish_load(self: &Arc<Self>, gen: u64, voice: String, pref: String) {
        let r = self.load(&voice, &pref);
        let mut s = self.slot.lock();
        if self.gen.load(Ordering::SeqCst) != gen {
            return; // superseded (reset_to_cpu during load): drop this result
        }
        let ok = match r {
            Ok(l) => {
                let device = l.device;
                let eng = Arc::new(Mutex::new(l));
                let wanted = self.want.lock().0.clone();
                if wanted != voice {
                    self.spawn_voice_swap(Arc::clone(&eng), wanted.clone()); // queued during load
                }
                *s = Slot::Ready { eng, device, voice: wanted, gen };
                true
            }
            Err(e) => {
                log::warn!("read-aloud: engine load failed: {e}");
                *s = Slot::Failed(e);
                false
            }
        };
        self.ready.notify_all();
        drop(s);
        if ok {
            self.spawn_idle_watch(gen);
        }
    }

    fn load(&self, voice: &str, pref: &str) -> Result<Loaded, String> {
        let f = ra_model::files(&self.data_dir);
        // the CPU model is the default and the fallback for "gpu": always required
        for p in [&f.fp32, &f.tokenizer, &f.voices_dir.join("af_heart.bin")] {
            require(p)?;
        }
        let vocab = load_vocab(&f.tokenizer)?;
        let voice = load_voice_or_default(&f.voices_dir, voice)?;
        let t = Instant::now();
        // G2P + names init (lexicon, tagger, CMUdict, g2p_en weights) overlaps the session open
        let g2p_job = std::thread::Builder::new().name("ra-tts-g2p".into()).spawn(build_g2p).ok();
        let (session, device) = if wants_gpu(pref) && f.q8_dml.is_file() {
            match open_session(&f.q8_dml, true) {
                Ok(s) => (s, Device::Gpu),
                Err(e) => {
                    log::warn!("read-aloud: DirectML failed ({e}); using CPU");
                    (open_session(&f.fp32, false)?, Device::Cpu)
                }
            }
        } else {
            (open_session(&f.fp32, false)?, Device::Cpu)
        };
        let (g2p, names) = match g2p_job.map(|h| h.join()) {
            Some(Ok(parts)) => parts,
            _ => build_g2p(), // spawn failed (or, debug only, init panicked)
        };
        log::info!("read-aloud: engine loaded on {:?} in {:?}", device, t.elapsed());
        Ok(Loaded { session, device, vocab, voice, g2p, names })
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

    /// Hands out the engine, starting a load (remembered voice/pref) if it is
    /// unloaded, and waiting up to 60 s for a load in flight.
    fn acquire(self: &Arc<Self>) -> Result<Arc<Mutex<Loaded>>, String> {
        let deadline = Instant::now() + LOAD_WAIT;
        let mut s = self.slot.lock();
        loop {
            match &*s {
                Slot::Ready { eng, .. } => return Ok(Arc::clone(eng)),
                Slot::Failed(e) => return Err(e.clone()),
                Slot::Empty => {
                    let (voice, pref) = self.want.lock().clone();
                    self.start_load(&mut s, voice, pref);
                }
                Slot::Loading { .. } => {
                    if self.ready.wait_until(&mut s, deadline).timed_out() && matches!(*s, Slot::Loading { .. }) {
                        return Err("engine load timed out".into());
                    }
                }
            }
        }
    }

    /// Synthesizes one chunk at 1.0x, then stretches to `rate`. Loads the engine
    /// if needed (blocking, up to 60 s). On a DirectML runtime error it switches
    /// to CPU and retries once; an `Err("dml: …")` only escapes if that fails too.
    pub fn synth(self: &Arc<Self>, words: &[&str], rate: f32) -> Result<ChunkAudio, String> {
        self.busy.fetch_add(1, Ordering::SeqCst);
        self.touch();
        let run = || self.acquire().and_then(|eng| Self::synth_with(&mut eng.lock(), words, rate));
        let mut r = run();
        if let Err(e) = &r {
            if e.starts_with("dml:") {
                log::warn!("read-aloud: DirectML runtime error ({e}); switching to CPU");
                let voice = self.want.lock().0.clone();
                self.reset_to_cpu(&voice);
                r = run();
            }
        }
        self.touch();
        self.busy.fetch_sub(1, Ordering::SeqCst);
        r
    }

    fn synth_with(l: &mut Loaded, words: &[&str], rate: f32) -> Result<ChunkAudio, String> {
        Self::synth_inner(l, words, rate, true)
    }

    fn synth_inner(l: &mut Loaded, words: &[&str], rate: f32, map_joined: bool) -> Result<ChunkAudio, String> {
        let device = l.device;
        let rate = effective_rate(rate);
        let empty = || ChunkAudio { pcm: Vec::new(), words: Vec::new(), device, covered: words.len() };
        let fed = g2p_words(&l.g2p, &l.names, words);
        // align against the words as misaki tokenizes them (hand-list overrides stripped)
        let plain: Vec<Cow<str>> = fed.iter().map(|w| plain_word(w)).collect();
        let plain: Vec<&str> = plain.iter().map(|w| w.as_ref()).collect();
        let fed: Vec<&str> = fed.iter().map(|w| w.as_ref()).collect();
        let text = fed.join(" ");
        if text.trim().is_empty() {
            return Ok(empty());
        }
        let (_, tokens) = l.g2p.g2p(&text).map_err(|e| format!("g2p: {e:?}"))?;
        let toks: Vec<(&str, Option<&str>, bool)> =
            tokens.iter().map(|t| (t.text.as_str(), t.phonemes.as_deref(), !t.whitespace.is_empty())).collect();
        let tok_text: Vec<&str> = toks.iter().map(|t| t.0).collect();
        // `plain` keeps the word count and order of `words`, so indices line up
        let owner = ra_timing::align(&plain, &tok_text);
        let ph = token_phonemes(&toks, &owner);
        let (ids, spans, covered) = build_ids_with(&ph, &l.vocab, words.len(), map_joined);
        let covered = covered.min(words.len());
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
        Ok(ChunkAudio { pcm, words: words_t, device, covered })
    }
}

/// misaki G2P whose unknown words go to the names fallback (hand list -> CMUdict
/// -> g2p_en -> spelled letters), plus the names table the caps rule consults.
fn build_g2p() -> (G2P, Arc<Names>) {
    let t = Instant::now();
    let names = Arc::new(Names::load(G2pEn::load().map(|g| Box::new(g) as Box<dyn Lts>)));
    let names_took = t.elapsed();
    let g2p = G2P::with_fallback(Language::EnglishUS, Some(Box::new(NameFallback(Arc::clone(&names)))));
    log::info!("read-aloud: names loaded in {:?}, names + g2p in {:?}", names_took, t.elapsed());
    (g2p, names)
}

/// G2P input for a chunk, one entry per word (same count and order):
/// - a hand-list word (Xi, Nguyen) becomes misaki's `[word](/phonemes/)` override,
///   so the hand list wins even where misaki has its own reading;
/// - ALL-CAPS words are lowercased when misaki or the names dictionary knows them,
///   or they are long enough to be a word (`ra_text::speakable_caps`);
/// - page `[` `]` become `(` `)`, so page text is never read as override syntax
///   (Kokoro has no bracket symbols; parens give the same pause).
/// Hyphen-joined parts are handled one by one. Use `plain_word` for alignment.
fn g2p_words<'a>(g: &G2P, names: &Names, words: &[&'a str]) -> Vec<Cow<'a, str>> {
    let known = |w: &str| lexicon_knows(&g.lexicon, w);
    let names_knows = |w: &str| names.knows(w);
    let lex_knows = |w: &str| lexicon_has(&g.lexicon, w);
    let lex_phonemes = |w: &str| lexicon_phonemes(&g.lexicon, w);
    let lex = Lex { knows: &lex_knows, phonemes: &lex_phonemes };
    let part = |p: &'a str| -> Cow<'a, str> {
        if let Some(o) = hand_override(p, names).or_else(|| rule_override(p, names, &lex)) {
            return Cow::Owned(o);
        }
        unbracket(ra_text::speakable_caps(p, &known, &names_knows))
    };
    words
        .iter()
        .map(|w| {
            if !w.contains('-') {
                return part(w);
            }
            let parts: Vec<Cow<str>> = w.split('-').map(part).collect();
            if parts.iter().all(|p| matches!(p, Cow::Borrowed(_))) {
                Cow::Borrowed(*w)
            } else {
                Cow::Owned(parts.join("-"))
            }
        })
        .collect()
}

/// `[` / `]` -> `(` / `)`.
fn unbracket(s: Cow<'_, str>) -> Cow<'_, str> {
    if s.contains(['[', ']']) {
        Cow::Owned(s.replace('[', "(").replace(']', ")"))
    } else {
        s
    }
}

/// A word (no hyphen) whose letters are a hand-list entry, as misaki's phoneme
/// override with its surrounding punctuation kept (`"Xi's,"` -> `[Xi's](/ʃˈiz/),`).
/// Hand-list phonemes never contain `[ ] ( ) /` (checked at load).
fn hand_override(w: &str, names: &Names) -> Option<String> {
    let start = w.find(char::is_alphanumeric)?;
    let (last, ch) = w.char_indices().rev().find(|(_, c)| c.is_alphanumeric())?;
    let end = last + ch.len_utf8();
    let core = &w[start..end];
    if !core.chars().all(|c| c.is_alphabetic() || c == '\'' || c == '\u{2019}') {
        return None;
    }
    let ph = names.hand(core)?;
    let lead = unbracket(Cow::Borrowed(&w[..start]));
    let trail = unbracket(Cow::Borrowed(&w[end..]));
    Some(format!("{lead}[{core}](/{ph}/){trail}"))
}

/// A word (no hyphen, no digit) that an acronym or compound rule reads, as misaki's
/// phoneme override with its punctuation kept: `SEC's` -> `[SEC's](/ˌɛsˌisˈiz/)`,
/// `TechReview` -> its parts, `elonmusk` -> "elon musk". Plain all-caps words with a
/// vowel (FBI, NASA) stay with misaki and the caps rule; words with digits stay with
/// misaki (and the number pre-pass).
fn rule_override(w: &str, names: &Names, lex: &Lex) -> Option<String> {
    let start = w.find(char::is_alphanumeric)?;
    let (last, ch) = w.char_indices().rev().find(|(_, c)| c.is_alphanumeric())?;
    let end = last + ch.len_utf8();
    let core = &w[start..end];
    if !core.chars().all(|c| c.is_ascii_alphabetic() || c == '\'' || c == '\u{2019}') {
        return None;
    }
    let bare_caps = core.bytes().all(|b| b.is_ascii_uppercase());
    let acronym = if bare_caps && !ra_names::SPELLED.contains(&core) { None } else { names.acronym(core, lex) };
    let ph = acronym.or_else(|| names.compound(core, lex))?;
    if ph.is_empty() || ph.contains(['[', ']', '(', ')', '/']) {
        return None;
    }
    let lead = unbracket(Cow::Borrowed(&w[..start]));
    let trail = unbracket(Cow::Borrowed(&w[end..]));
    Some(format!("{lead}[{core}](/{ph}/){trail}"))
}

/// The misaki gold lexicon has an entry for `w` itself, in any case misaki matches.
/// No stems and no silver (silver adds forms like "deeps" that would split
/// "deepseek" as "deeps eek"); `lexicon_phonemes` still finds both.
fn lexicon_has(lx: &Lexicon, w: &str) -> bool {
    lx.in_gold(w)
}

/// The misaki lexicon's phonemes for `w` (noun reading first), if it has them.
/// Tagged entries may have no noun reading (seek): verb, adjective, adverb next.
fn lexicon_phonemes(lx: &Lexicon, w: &str) -> Option<String> {
    ["NN", "VB", "JJ", "RB"]
        .iter()
        .find_map(|tag| lx.get_word(w, tag, None, None).map(|(p, _)| p).filter(|p| !p.trim().is_empty()))
}

/// A `g2p_words` entry as misaki tokenizes it: every `[text](/ph/)` override
/// replaced by `text`. Page brackets were already turned into parens, so any `[`
/// here is ours. Malformed input is returned as is; never panics.
fn plain_word(w: &str) -> Cow<'_, str> {
    if !w.contains('[') {
        return Cow::Borrowed(w);
    }
    let mut out = String::with_capacity(w.len());
    let mut rest = w;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let Some(mid) = after.find("](/") else { break };
        let Some(close) = after[mid + 3..].find("/)") else { break };
        out.push_str(&rest[..open]);
        out.push_str(&after[..mid]);
        rest = &after[mid + 3 + close + 2..];
    }
    out.push_str(rest);
    Cow::Owned(out)
}

/// Uppercase: the lexicon has that exact spelling (NATO, US). Lowercase: a gold
/// word, a gold stem (+s/es/ies/ed/ing), or a silver word of 4+ letters (short
/// silver entries are mostly acronyms like fbi, cia).
fn lexicon_knows(lx: &Lexicon, w: &str) -> bool {
    if w.chars().any(char::is_uppercase) {
        return lx.golds.contains_key(w) || lx.silvers.contains_key(w);
    }
    if lx.in_gold(w) || (w.chars().count() >= 4 && lx.silvers.contains_key(w)) {
        return true;
    }
    let gold = |s: &str| s.chars().count() >= 2 && lx.in_gold(s);
    let strip = |suf: &str| w.strip_suffix(suf);
    strip("ies").is_some_and(|s| gold(&format!("{s}y")))
        || strip("es").is_some_and(gold)
        || strip("s").is_some_and(gold)
        || strip("ed").is_some_and(|s| gold(s) || gold(&format!("{s}e")))
        || strip("ing").is_some_and(|s| gold(s) || gold(&format!("{s}e")))
}

fn is_hyphen(s: &str) -> bool {
    matches!(s, "-" | "\u{2010}" | "\u{2011}")
}

/// (word or NO_WORD, phonemes, followed-by-whitespace) per G2P token. Every token
/// with phonemes feeds the model (punctuation shapes prosody), except a hyphen
/// inside one word ("mid-year"): misaki gives it a space phoneme, which would
/// add a long pause between the halves.
fn token_phonemes(toks: &[(&str, Option<&str>, bool)], owner: &[Option<usize>]) -> Vec<(usize, String, bool)> {
    let own = |i: Option<usize>| i.and_then(|i| owner.get(i).copied().flatten());
    toks.iter()
        .zip(owner.iter())
        .enumerate()
        .filter(|&(i, ((text, p, _), o))| {
            let inner = o.is_none()
                && is_hyphen(text)
                && p.map_or(true, |p| p.trim().is_empty())
                && own(i.checked_sub(1)).is_some()
                && own(i.checked_sub(1)) == own(Some(i + 1));
            !inner
        })
        .filter_map(|(_, ((_, p, ws), o))| Some((o.unwrap_or(NO_WORD), (*p)?.to_string(), *ws)))
        .collect()
}

#[cfg(test)]
#[path = "ra_tts_bench.rs"]
mod bench;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn joined(ph: &[(usize, String, bool)]) -> String {
        ph.iter().map(|(_, p, ws)| format!("{p}{}", if *ws { " " } else { "" })).collect()
    }

    #[test]
    fn hyphenated_word_has_no_dash_pause() {
        let words = ["a", "mid-year", "plan"];
        let toks = [("a", Some("ɐ"), true), ("mid", Some("mˈɪd"), true), ("-", Some(" "), true), ("year", Some("jˈɪɹ"), true), ("plan", Some("plˈæn"), false)];
        let tt: Vec<&str> = toks.iter().map(|t| t.0).collect();
        let owner = ra_timing::align(&words, &tt);
        assert_eq!(owner, vec![Some(0), Some(1), None, Some(1), Some(2)]);
        let ph = token_phonemes(&toks, &owner);
        let s = joined(&ph);
        assert_eq!(s, "ɐ mˈɪd jˈɪɹ plˈæn");
        assert!(!s.contains("  "), "{s:?}");
        // the word span covers both halves
        let v: HashMap<char, i64> = s.chars().filter(|c| *c != ' ').map(|c| (c, 1)).chain([(' ', 16)]).collect();
        let (_, spans, _) = build_ids(&ph, &v, 3);
        let w1: Vec<_> = spans.iter().filter(|sp| sp.0 == 1).collect();
        assert_eq!(w1.first().map(|sp| sp.1), Some(2));
        assert_eq!(w1.last().map(|sp| sp.2), Some(2 + "mˈɪd jˈɪɹ".chars().count()));
    }

    #[test]
    fn spaced_dash_keeps_its_pause() {
        let words = ["yes", "-", "no"];
        let toks = [("yes", Some("jˈɛs"), true), ("-", Some(" "), true), ("no", Some("nˈO"), false)];
        let tt: Vec<&str> = toks.iter().map(|t| t.0).collect();
        let owner = ra_timing::align(&words, &tt);
        assert_eq!(joined(&token_phonemes(&toks, &owner)), "jˈɛs   nˈO");
        // a dash glued to a different-word neighbour (bad align) also keeps it
        let owner2 = [Some(0), None, Some(2)];
        assert_eq!(joined(&token_phonemes(&toks, &owner2)), "jˈɛs   nˈO");
    }

    #[test]
    fn hyphen_drop_needs_whitespace_only_phonemes() {
        let toks = [("mid", Some("mˈɪd"), false), ("-", Some("x"), false), ("year", Some("jˈɪɹ"), false)];
        let owner = [Some(0), None, Some(0)];
        assert_eq!(joined(&token_phonemes(&toks, &owner)), "mˈɪdxjˈɪɹ");
        // edge positions never panic
        assert_eq!(token_phonemes(&[("-", Some(" "), true)], &[None]).len(), 1);
        assert!(token_phonemes(&[], &[]).is_empty());
    }

    /// The engine's real G2P + names (CMUdict + g2p_en fallback), built once per test run.
    fn test_parts() -> &'static (G2P, Arc<Names>) {
        static G: std::sync::OnceLock<(G2P, Arc<Names>)> = std::sync::OnceLock::new();
        G.get_or_init(build_g2p)
    }

    fn test_g2p() -> &'static G2P {
        &test_parts().0
    }

    fn test_names() -> &'static Names {
        &test_parts().1
    }

    #[test]
    fn names_are_pronounced_not_spelled() {
        let g = test_g2p();
        for (text, word) in [
            ("Pelosi spoke", "Pelosi"),
            ("PELOSI SPOKE", "PELOSI"),
            ("Pelosi's bill", "Pelosi's"),
            ("a Zelensky-led talk", "Zelensky-led"),
            ("Buttigieg won", "Buttigieg"),
        ] {
            let words: Vec<&str> = text.split(' ').collect();
            let fed = g2p_words(g, test_names(), &words);
            let fed: Vec<&str> = fed.iter().map(|c| c.as_ref()).collect();
            let (_, toks) = g.g2p(&fed.join(" ")).unwrap();
            let ph: String = toks.iter().filter_map(|t| t.phonemes.clone()).collect();
            // spelled names have a stress mark per letter; a word has at most ~4
            let marks = ph.matches(['ˈ', 'ˌ']).count();
            assert!(marks <= words.len() + 4, "{text}: {ph}");
            assert!(!ph.contains("  "), "{word}: no multi-space gap: {ph}");
        }
    }

    #[test]
    fn fallback_word_keeps_one_timing_span_per_word() {
        let g = test_g2p();
        let words = ["Senator", "Pelosi", "said", "no"];
        let (_, toks) = g.g2p(&words.join(" ")).unwrap();
        let tt: Vec<&str> = toks.iter().map(|t| t.text.as_str()).collect();
        let owner = ra_timing::align(&words, &tt);
        for w in 0..words.len() {
            assert!(owner.contains(&Some(w)), "word {w} has no token");
        }
    }

    /// Phonemes per word through the real path: g2p_words -> misaki -> align on plain words.
    fn spoken(words: &[&str]) -> Vec<String> {
        phonemes_per_word(test_g2p(), test_names(), words)
    }

    #[test]
    fn hand_list_overrides_misaki() {
        // misaki reads XI as the Roman numeral "eleven"/"Roman" and has its own Nguyen
        assert_eq!(spoken(&["Today,", "Xi", "spoke."])[1], "ʃˈi");
        assert_eq!(spoken(&["Today,", "XI", "spoke."])[1], "ʃˈi");
        assert_eq!(spoken(&["Today,", "Nguyen", "spoke."])[1], "wˈɪn");
        assert_eq!(spoken(&["Today,", "Xi's", "plan."])[1], "ʃˈiz");
        assert_eq!(spoken(&["Today,", "XI'S", "plan."])[1], "ʃˈiz");
        assert_eq!(spoken(&["(Xi),", "spoke."])[0].replace(['(', ')', ','], ""), "ʃˈi");
        assert!(spoken(&["a", "Xi-led", "talk"])[1].starts_with("ʃˈi"), "{:?}", spoken(&["a", "Xi-led", "talk"]));
    }

    #[test]
    fn override_word_keeps_one_span_per_word() {
        let g = test_g2p();
        for words in [
            vec!["Senator", "Xi", "said", "no"],
            vec!["Xi's", "talks", "with", "Nguyen,", "XI", "said."],
            vec!["\u{201C}XI'S", "Xi-led", "plan\u{201D}"],
        ] {
            let fed = g2p_words(g, test_names(), &words);
            let plain: Vec<Cow<str>> = fed.iter().map(|w| plain_word(w)).collect();
            let plain: Vec<&str> = plain.iter().map(|c| c.as_ref()).collect();
            let fed: Vec<&str> = fed.iter().map(|c| c.as_ref()).collect();
            let (_, toks) = g.g2p(&fed.join(" ")).unwrap();
            let tt: Vec<&str> = toks.iter().map(|t| t.text.as_str()).collect();
            let owner = ra_timing::align(&plain, &tt);
            for w in 0..words.len() {
                assert!(owner.contains(&Some(w)), "{words:?}: word {w} has no token ({tt:?})");
            }
        }
    }

    #[test]
    fn words_misaki_gets_right_are_untouched() {
        let words = ["Washington", "Pelosi", "the", "NATO", "FBI", "mid-year", "iPhone", "McDonald's", "said."];
        let fed = g2p_words(test_g2p(), test_names(), &words);
        for (w, f) in words.iter().zip(&fed) {
            assert!(!f.contains('['), "{w} -> {f}");
            assert!(matches!(f, Cow::Borrowed(_)), "{w} -> {f}");
        }
    }

    #[test]
    fn page_brackets_are_never_override_syntax() {
        let g = test_g2p();
        let words = ["[1](2)", "[Xi", "x](/ʒʒʒ/)", "](", "[", "]", "🙂", "東京", "a[b](/zz/)c", "Xi](/x/)", "[[Xi]]", "word"];
        let fed = g2p_words(g, test_names(), &words);
        for (w, f) in words.iter().zip(&fed) {
            if !w.contains("Xi") {
                assert!(!f.contains(['[', ']']), "{w:?} -> {f:?}");
            }
        }
        let fed: Vec<&str> = fed.iter().map(|c| c.as_ref()).collect();
        let (_, toks) = g.g2p(&fed.join(" ")).expect("never errors");
        let ph: String = toks.iter().filter_map(|t| t.phonemes.clone()).collect();
        assert!(!ph.contains("ʒʒ") && !ph.contains("zz"), "page text applied an override: {ph}");
        // every override that does happen is a whole word we produced
        let one = g2p_words(g, test_names(), &["[[Xi]]"]);
        assert_eq!(one[0], "(([Xi](/ʃˈi/)))");
        assert_eq!(plain_word(&one[0]), "((Xi))");
        for w in ["", "[", "](", "[a](/", "[a](/b", "x[Xi](/ʃˈi/", "🙂[", "東京](/"] {
            let _ = plain_word(w);
        }
    }

    #[test]
    fn lexicon_words_never_reach_the_name_fallback() {
        // a lexicon word keeps misaki's own reading: same phonemes with and without the fallback
        let plain = G2P::new(Language::EnglishUS);
        let text = "The president said the report was late";
        let a: Vec<Option<String>> = test_g2p().g2p(text).unwrap().1.into_iter().map(|t| t.phonemes).collect();
        let b: Vec<Option<String>> = plain.g2p(text).unwrap().1.into_iter().map(|t| t.phonemes).collect();
        assert_eq!(a, b);
    }

    fn word_phonemes(text_words: &[&str]) -> Vec<(String, String)> {
        let g = test_g2p();
        let fed = g2p_words(g, test_names(), text_words);
        let fed_refs: Vec<&str> = fed.iter().map(|c| c.as_ref()).collect();
        let (_, toks) = g.g2p(&fed_refs.join(" ")).expect("g2p");
        let tt: Vec<&str> = toks.iter().map(|t| t.text.as_str()).collect();
        let owner = ra_timing::align(&fed_refs, &tt);
        let mut out: Vec<(String, String)> = text_words.iter().map(|w| (w.to_string(), String::new())).collect();
        for (t, o) in toks.iter().zip(owner) {
            if let (Some(o), Some(p)) = (o, &t.phonemes) {
                out[o].1.push_str(p);
            }
        }
        out
    }

    /// Real misaki: "mid-year" comes back split around a hyphen token, and our assembly
    /// (what the model is fed) has no multi-space gap between the halves.
    #[test]
    fn real_g2p_hyphenated_word_has_no_gap() {
        let g = test_g2p();
        let words = ["a", "mid-year", "plan"];
        let fed = g2p_words(g, test_names(), &words);
        let fed: Vec<&str> = fed.iter().map(|c| c.as_ref()).collect();
        let (_, tokens) = g.g2p(&fed.join(" ")).expect("g2p");
        let toks: Vec<(&str, Option<&str>, bool)> =
            tokens.iter().map(|t| (t.text.as_str(), t.phonemes.as_deref(), !t.whitespace.is_empty())).collect();
        let tt: Vec<&str> = toks.iter().map(|t| t.0).collect();
        let owner = ra_timing::align(&fed, &tt);
        let owned_by_mid = owner.iter().filter(|o| **o == Some(1)).count();
        assert!(owned_by_mid >= 2 && tt.iter().any(|t| is_hyphen(t)), "misaki splits mid-year: {toks:?}");
        let s = joined(&token_phonemes(&toks, &owner));
        assert!(!s.contains("  "), "multi-space gap in {s:?} (tokens {toks:?})");
    }

    #[test]
    fn all_caps_sentence_reads_words_not_letters() {
        let s = ["THE", "PRESIDENT", "AND", "THE", "SENATE", "MET", "ON", "TUESDAY", "FOR", "NEW", "TALKS"];
        let lower: Vec<String> = s.iter().map(|w| w.to_lowercase()).collect();
        let lower_refs: Vec<&str> = lower.iter().map(|w| w.as_str()).collect();
        let got = word_phonemes(&s);
        let want = word_phonemes(&lower_refs);
        for ((w, p), (_, q)) in got.iter().zip(want.iter()) {
            assert_eq!(p, q, "{w} spelled differently from its lowercase reading");
        }
    }

    #[test]
    fn all_caps_acronyms_still_spelled() {
        let g = test_g2p();
        let words = ["FBI", "GDP", "CEO", "NATO", "NASA", "US", "IT", "WHO", "THE,", "TALKS."];
        let fed: Vec<String> = g2p_words(g, test_names(), &words).into_iter().map(|c| c.into_owned()).collect();
        assert_eq!(fed, vec!["FBI", "GDP", "CEO", "NATO", "NASA", "US", "IT", "WHO", "the,", "talks."]);
    }

    #[test]
    fn caps_rewrite_is_cheap() {
        let g = test_g2p();
        let words: Vec<&str> = ["THE", "PRESIDENT", "SAID", "FBI", "TALKS", "GDP", "MID-YEAR", "iPhone", "42"].iter().cycle().take(30).copied().collect();
        let t = Instant::now();
        for _ in 0..100 {
            let _ = g2p_words(g, test_names(), &words);
        }
        assert!(t.elapsed() < Duration::from_millis(500), "{:?} for 100 chunks", t.elapsed());
    }

    // ---- phase 3 task 3: acronyms, CamelCase, compounds, fold through the real path ----

    fn say(w: &str) -> String {
        spoken(&["The", w, "said."])[1].clone()
    }

    fn tight(w: &str) -> String {
        kokoro_phonemes(&crate::ra_names::spell_tight(w))
    }

    #[test]
    fn spelled_list_and_suffixed_acronyms_read_letters() {
        for (w, stem, suf) in [
            ("SEC", "SEC", ""),
            ("SEC's", "SEC", "z"),
            ("AWS", "AWS", ""),
            ("GPUs", "GPU", "z"),
            ("ETFs", "ETF", "s"),
            ("HTTPS", "HTTPS", ""),
            ("FTX's", "FTX", "ɪz"),
            ("SVB's", "SVB", "z"),
            ("QQQ", "QQQ", ""),
            ("NVMe", "NVMe", ""),
            ("PCIe", "PCIe", ""),
            ("FYI", "FYI", ""),
            ("IPOs", "IPO", "z"),
            ("CEOs", "CEO", "z"),
        ] {
            assert_eq!(say(w), format!("{}{suf}", tight(stem)), "{w}");
        }
    }

    #[test]
    fn word_acronyms_are_still_words() {
        for w in ["NASA", "NATO", "COVID", "AIDS", "NASA's", "FEMA's", "OPEC"] {
            let p = say(w);
            let marks = p.matches(['ˈ', 'ˌ']).count();
            assert!(marks < 3, "{w} spelled: {p}");
            let fed = g2p_words(test_g2p(), test_names(), &[w]);
            assert!(!fed[0].contains('['), "{w} -> {}", fed[0]);
        }
    }

    #[test]
    fn camel_case_brands_read_their_parts() {
        for (w, has) in [
            ("TechReview", &["tˈɛk", "vjˈu"][..]),
            ("OpenAI", &["ˈOpən", "A", "ˈI"][..]),
            ("JPMorgan", &["ʤˌA", "pˈi", "mˈɔɹɡ"][..]),
            ("PostgreSQL", &["ɛskjˌu", "ˈɛl"][..]),
            ("iHeartMedia", &["ˈI", "hˈɑ", "ɹt", "mˈidiə"][..]),
            ("UnitedHealth", &["ju", "nˈI", "hˈɛlθ"][..]),
            ("DocuSign", &["sˈIn"][..]),
            ("DeepSeek", &["dˈip", "sˈik"][..]),
            ("ChatGPT", &["ʧˈæt", "ʤˌi"][..]),
        ] {
            // length marks (ː) vary between lexicon entries; the sounds are what count
            let p = say(w).replace('ː', "");
            let mut at = 0;
            for h in has {
                let Some(i) = p[at..].find(h) else { panic!("{w}: {h:?} not in {p:?} (after byte {at})") };
                at += i + h.len();
            }
            assert!(!p.contains(' '), "{w}: one word, no gap: {p:?}");
        }
    }

    #[test]
    fn camel_case_known_words_are_not_split() {
        let words = ["McDonald", "iPhone", "YouTube", "McDonald's", "LinkedIn"];
        let fed = g2p_words(test_g2p(), test_names(), &words);
        for (w, f) in words.iter().zip(&fed) {
            assert!(matches!(f, Cow::Borrowed(_)), "{w} -> {f}");
        }
    }

    #[test]
    fn lowercase_compounds_read_as_two_words() {
        let musk = say("musk");
        assert!(say("elonmusk").ends_with(&musk), "{} / {musk}", say("elonmusk"));
        assert!(say("elonmusk").starts_with("ˈil") || say("elonmusk").starts_with("ˈɛl"), "{}", say("elonmusk"));
        assert_eq!(say("techreview"), format!("{}{}", say("tech"), say("review")));
        assert_eq!(say("deepseek"), format!("{}{}", say("deep"), say("seek")));
        // real words (misaki lexicon or CMUdict) are never segmented
        let words = ["together", "carpet", "kingdom", "manage", "nowhere", "pelosi", "therapist"];
        let fed = g2p_words(test_g2p(), test_names(), &words);
        for (w, f) in words.iter().zip(&fed) {
            assert!(matches!(f, Cow::Borrowed(_)), "{w} -> {f}");
        }
    }

    #[test]
    fn microsoft_is_said_not_spelled() {
        // Alex's report: "it doesn't know Microsoft"
        let ms = "mˈIkɹˌOsˈɔft";
        assert_eq!(say("Microsoft"), ms);
        assert_eq!(say("MICROSOFT"), ms);
        assert_eq!(say("Microsoft's"), format!("{ms}s"));
        assert_eq!(spoken(&["MICROSOFT", "SAID"])[0], ms);
    }

    #[test]
    fn accented_names_fold_through_the_real_path() {
        let hand = kokoro_phonemes(&test_names().lookup("erdogan").unwrap());
        assert_eq!(say("Erdoğan"), hand);
        let p = say("Łukasz");
        assert!(p.matches(['ˈ', 'ˌ']).count() < 4, "Łukasz spelled: {p}");
        let p = say("Călin");
        assert!(p.matches(['ˈ', 'ˌ']).count() < 4, "Călin spelled: {p}");
    }

    #[test]
    fn brand_list_entries_win() {
        // spec section 3 rows that are brand-list entries
        assert_eq!(say("EBITDA"), "ˈibɪtdˌɑ");
        assert_eq!(say("PayPal"), "pˈApˌæl");
        assert_eq!(say("SaaS"), "sˈæs");
        assert_eq!(say("GAAP"), "ɡˈæp", "GAAP is said as a word");
        assert_eq!(say("Coinbase's"), "kˈYnbˌAsɪz");
        // brand entries are whole words only: their parts still read normally
        assert_eq!(say("Coin"), spoken(&["The", "coin", "said."])[1]);
    }

    #[test]
    fn rule_overrides_keep_one_span_per_word() {
        let g = test_g2p();
        for words in [
            vec!["The", "SEC's", "TechReview", "said", "elonmusk", "posted."],
            vec!["\u{201C}GPUs,", "OpenAI's", "(AWS)", "JPMorgan-led", "deal\u{201D}"],
        ] {
            let fed = g2p_words(g, test_names(), &words);
            let plain: Vec<Cow<str>> = fed.iter().map(|w| plain_word(w)).collect();
            let plain: Vec<&str> = plain.iter().map(|c| c.as_ref()).collect();
            let fed: Vec<&str> = fed.iter().map(|c| c.as_ref()).collect();
            let (_, toks) = g.g2p(&fed.join(" ")).unwrap();
            let tt: Vec<&str> = toks.iter().map(|t| t.text.as_str()).collect();
            let owner = ra_timing::align(&plain, &tt);
            for w in 0..words.len() {
                assert!(owner.contains(&Some(w)), "{words:?}: word {w} has no token ({tt:?})");
            }
            let ph: String = toks.iter().filter_map(|t| t.phonemes.clone()).collect();
            assert!(!ph.contains("  "), "{ph}");
        }
    }

    /// Brand review sheet (tools/read-aloud/brand_candidates.py drives it):
    /// BRAND_IN = "word<TAB>count" lines, BRAND_OUT = TSV of word, count, the reading
    /// through the real path (hand list, acronym / CamelCase / compound rules, misaki,
    /// NameFallback), and what the GRU alone says for the lowercase word.
    #[test]
    #[ignore]
    fn brand_review_dump() {
        use std::io::Write as _;
        let inp = std::env::var("BRAND_IN").expect("set BRAND_IN");
        let out = std::env::var("BRAND_OUT").expect("set BRAND_OUT");
        let gru = G2pEn::load().expect("g2p_en");
        let text = std::fs::read_to_string(&inp).expect("read BRAND_IN");
        let mut f = std::io::BufWriter::new(std::fs::File::create(&out).expect("create BRAND_OUT"));
        writeln!(f, "word\tcount\tnew_reading\tgru_reading").unwrap();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let (w, n) = line.split_once('\t').unwrap_or((line, ""));
            let new = say(w.trim());
            let low = w.trim().to_lowercase();
            let g = gru.predict(&low).map(|a| kokoro_phonemes(&crate::ra_names::arpa_to_misaki(&a))).unwrap_or_default();
            writeln!(f, "{}\t{}\t{new}\t{g}", w.trim(), n.trim()).unwrap();
        }
        f.flush().unwrap();
    }

    fn vocab() -> HashMap<char, i64> {
        [('h', 1), ('ə', 2), ('l', 3), ('o', 4), (' ', 16)].into_iter().collect()
    }

    #[test]
    fn ids_and_spans() {
        let (ids, spans, covered) = build_ids(&[(0, "həl".into(), true), (1, "lo".into(), false)], &vocab(), 2);
        assert_eq!(ids, vec![1, 2, 3, 16, 3, 4]);
        assert_eq!(spans, vec![(0, 0, 3), (1, 4, 6)]);
        assert_eq!(covered, 2);
    }

    #[test]
    fn unknown_phoneme_chars_dropped() {
        let (ids, _, _) = build_ids(&[(0, "hxz".into(), false)], &vocab(), 1);
        assert_eq!(ids, vec![1]);
    }

    #[test]
    fn ids_truncated_to_510() {
        let long: String = std::iter::repeat('h').take(600).collect();
        let (ids, spans, covered) = build_ids(&[(0, long, false)], &vocab(), 1);
        assert_eq!(ids.len(), 510);
        assert_eq!(spans[0].2, 510);
        assert_eq!(covered, 1, "first word always makes progress, even truncated");
    }

    fn hs(n: usize) -> String {
        "h".repeat(n)
    }

    #[test]
    fn overflowing_word_is_dropped_whole() {
        let (ids, spans, covered) = build_ids(&[(0, hs(300), true), (1, hs(300), false)], &vocab(), 2);
        assert_eq!(ids.len(), 301, "word 0 + its space; word 1 not cut mid-phonemes");
        assert_eq!(spans, vec![(0, 0, 300)]);
        assert_eq!(covered, 1);
    }

    #[test]
    fn overflow_keeps_punctuation_of_covered_words() {
        let ph = [(0, hs(200), false), (NO_WORD, "o".into(), true), (1, hs(200), true), (2, hs(200), false)];
        let (ids, spans, covered) = build_ids(&ph, &vocab(), 3);
        assert_eq!(ids.len(), 200 + 1 + 1 + 200 + 1);
        assert_eq!(spans.last(), Some(&(1, 202, 402)));
        assert_eq!(covered, 2);
    }

    #[test]
    fn covered_counts_trailing_unspeakable_words_when_nothing_overflows() {
        let (_, _, covered) = build_ids(&[(0, "h".into(), true), (1, "o".into(), false)], &vocab(), 4);
        assert_eq!(covered, 4);
    }

    #[test]
    fn joined_diphthongs_map_to_kokoro_symbols() {
        for (src, want) in [
            ("e\u{200D}ɪ", "A"),
            ("a\u{200D}ɪ", "I"),
            ("o\u{200D}ʊ", "O"),
            ("a\u{200D}ʊ", "W"),
            ("ɔ\u{200D}ɪ", "Y"),
            ("d\u{200D}ʒ", "ʤ"),
            ("t\u{200D}ʃ", "ʧ"),
        ] {
            assert_eq!(kokoro_phonemes(src), want, "{src:?}");
        }
        // whole words as misaki-rs spells them
        assert_eq!(kokoro_phonemes("hˈo\u{200D}ʊm"), "hˈOm"); // home
        assert_eq!(kokoro_phonemes("d\u{200D}ʒˈʌd\u{200D}ʒ"), "ʤˈʌʤ"); // judge
        assert_eq!(kokoro_phonemes("t\u{200D}ʃˈɜːt\u{200D}ʃ"), "ʧˈɜːʧ"); // church
        // other joined pairs: the joiner is just dropped
        assert_eq!(kokoro_phonemes("kˈɑː\u{200D}ɹ"), "kˈɑːɹ");
        assert_eq!(kokoro_phonemes("i\u{200D}ə"), "iə");
        assert_eq!(kokoro_phonemes("plain"), "plain");
    }

    #[test]
    fn build_ids_uses_one_id_per_diphthong() {
        let v: HashMap<char, i64> = [('h', 1), ('O', 31), ('m', 3), ('o', 4), ('ʊ', 5)].into_iter().collect();
        let (ids, spans, _) = build_ids(&[(0, "ho\u{200D}ʊm".into(), false)], &v, 1);
        assert_eq!(ids, vec![1, 31, 3]);
        assert_eq!(spans, vec![(0, 0, 3)]);
        let (old, _, _) = build_ids_with(&[(0, "ho\u{200D}ʊm".into(), false)], &v, 1, false);
        assert_eq!(old, vec![1, 4, 5, 3], "the old path split it in two");
    }

    #[test]
    fn missing_model_files_report_the_fix_it_error() {
        let e = Engine::new(std::env::temp_dir().join(format!("ra-tts-missing2-{}", std::process::id())));
        let err = e.synth(&["hi"], 1.0).err().expect("must fail");
        assert!(err.starts_with(MISSING), "{err}");
    }

    #[test]
    fn reset_to_cpu_is_noop_while_cpu_load_in_flight() {
        let e = Engine::new(std::env::temp_dir().join("ra-tts-none"));
        *e.slot.lock() = Slot::Loading { cpu: true };
        e.reset_to_cpu("af_heart");
        e.reset_to_cpu("af_heart");
        assert_eq!(e.loads.load(Ordering::SeqCst), 0);
        assert!(matches!(*e.slot.lock(), Slot::Loading { cpu: true }));
    }

    #[test]
    fn auto_pref_resolves_to_cpu() {
        assert!(!wants_gpu("auto"));
        assert!(!wants_gpu("cpu"));
        assert!(!wants_gpu(""));
        assert!(wants_gpu("gpu"));
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
    fn nothing_loads_until_asked_then_synth_self_loads() {
        let e = Engine::new(std::env::temp_dir().join("ra-tts-none"));
        assert!(!e.is_loaded());
        assert_eq!(e.device(), None);
        assert_eq!(e.loads.load(Ordering::SeqCst), 0);
        // no model here: synth starts a load itself, which fails cleanly (no hang)
        assert!(e.synth(&["hi"], 1.0).is_err());
        assert_eq!(e.loads.load(Ordering::SeqCst), 1);
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
        write_wav_as(pcm, "ra_tts_sample.wav")
    }

    fn write_wav_as(pcm: &[f32], name: &str) -> PathBuf {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target").join(name);
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
        e.preload("af_heart", "gpu");
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
        assert_eq!(a.covered, 4);
        println!("cpu first chunk {:?} on {:?}, {} samples, words {:?}", t.elapsed(), e.device(), a.pcm.len(), a.words);
        assert_eq!(e.device(), Some(Device::Cpu));
        check(&a, 2.2);
        let t = Instant::now();
        let _ = e.synth(&SENT, 2.2).unwrap();
        println!("cpu warm chunk {:?}", t.elapsed());
        // over-long chunk: stops at a whole word, reports covered, rest re-synthesizes
        let text = "the quick brown fox jumps over the lazy dog ".repeat(20);
        let many: Vec<&str> = text.split_whitespace().collect();
        let t = Instant::now();
        let big = e.synth(&many, 2.2).unwrap();
        println!("long chunk: {} words in, covered {}, {} timed, {:?}", many.len(), big.covered, big.words.len(), t.elapsed());
        assert!(big.covered > 0 && big.covered < many.len());
        assert!(big.words.iter().all(|w| w.word < big.covered));
        let rest = e.synth(&many[big.covered..], 2.2).unwrap();
        assert!(rest.covered > 0);
        if std::env::var("RA_HOLD").is_ok() {
            println!("HOLD");
            std::thread::sleep(Duration::from_secs(3));
        }
    }

    /// I-5 A/B: the same passage without and with the U+200D diphthong mapping (CPU, 2.2x).
    /// cargo test ra_tts::tests::joiner_ab_real -- --ignored --nocapture
    #[test]
    #[ignore]
    fn joiner_ab_real() {
        let e = Engine::new(dirs::data_dir().unwrap().join("ALAN Echo"));
        e.preload("af_heart", "cpu");
        let text = "I go home at night. How do you say the time?";
        let words: Vec<&str> = text.split(' ').collect();
        let eng = e.acquire().expect("engine");
        let mut l = eng.lock();
        assert_eq!(l.device, Device::Cpu);
        let (_, toks) = l.g2p.g2p(text).unwrap();
        for t in &toks {
            if let Some(p) = &t.phonemes {
                println!("{:>6} {:?} -> {:?}", t.text, p, kokoro_phonemes(p));
            }
        }
        let before = Engine::synth_inner(&mut l, &words, 2.2, false).unwrap();
        let after = Engine::synth_inner(&mut l, &words, 2.2, true).unwrap();
        println!("before: {} samples, {} words timed", before.pcm.len(), before.words.len());
        println!("after:  {} samples, {} words timed", after.pcm.len(), after.words.len());
        println!("wav before: {}", write_wav_as(&before.pcm, "ra_ab_before.wav").display());
        println!("wav after:  {}", write_wav_as(&after.pcm, "ra_ab_after.wav").display());
        assert_eq!(after.words.len(), words.len());
    }

    /// GPU load, then the DML-error recovery path: reset_to_cpu, next synth waits and runs on CPU.
    #[test]
    #[ignore]
    fn reset_to_cpu_real() {
        let e = Engine::new(dirs::data_dir().unwrap().join("ALAN Echo"));
        e.preload("af_heart", "gpu");
        assert_eq!(e.synth(&SENT, 2.2).unwrap().device, Device::Gpu);
        e.reset_to_cpu("af_heart");
        e.reset_to_cpu("af_heart"); // back-to-back: still one CPU load
        assert_eq!(e.loads.load(Ordering::SeqCst), 2, "1 gpu + 1 cpu load");
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
        e.reset_to_cpu("am_michael"); // already on CPU: no-op
        assert_eq!(e.loads.load(Ordering::SeqCst), 2);
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
        // synth after idle unload reloads by itself (remembered voice/pref)
        let a = e.synth(&SENT, 2.2).unwrap();
        assert_eq!(a.device, Device::Cpu);
        assert_eq!(e.loads.load(Ordering::SeqCst), 2);
    }

    /// Two synths in flight while the engine is still loading, plus repeated preloads: no deadlock.
    #[test]
    #[ignore]
    fn concurrent_synth_real() {
        let e = Engine::new(dirs::data_dir().unwrap().join("ALAN Echo"));
        e.preload("af_heart", "auto");
        e.preload("am_michael", "auto"); // voice change while Loading is queued
        let hs: Vec<_> = (0..2)
            .map(|_| {
                let e = Arc::clone(&e);
                std::thread::spawn(move || e.synth(&SENT, 2.2).map(|a| a.words.len()))
            })
            .collect();
        for _ in 0..5 {
            e.preload("am_michael", "auto");
            let _ = (e.is_loaded(), e.device());
        }
        for h in hs {
            assert_eq!(h.join().unwrap(), Ok(4));
        }
        assert_eq!(e.device(), Some(Device::Cpu), "auto resolves to CPU");
        assert_eq!(e.loads.load(Ordering::SeqCst), 1);
        assert!(matches!(&*e.slot.lock(), Slot::Ready { voice, .. } if voice == "am_michael"));
    }

    /// Phonemes each word of `words` gets (after the caps rule), as fed to the model.
    fn phonemes_per_word(g: &G2P, names: &Names, words: &[&str]) -> Vec<String> {
        let fed = g2p_words(g, names, words);
        let plain: Vec<Cow<str>> = fed.iter().map(|w| plain_word(w)).collect();
        let plain: Vec<&str> = plain.iter().map(|c| c.as_ref()).collect();
        let fed: Vec<&str> = fed.iter().map(|c| c.as_ref()).collect();
        let mut out = vec![String::new(); words.len()];
        let Ok((_, toks)) = g.g2p(&fed.join(" ")) else { return out };
        let tt: Vec<&str> = toks.iter().map(|t| t.text.as_str()).collect();
        let owner = ra_timing::align(&plain, &tt);
        for (t, o) in toks.iter().zip(owner) {
            if let (Some(o), Some(p)) = (o, &t.phonemes) {
                out[o].push_str(&kokoro_phonemes(p));
            }
        }
        out
    }

    fn write_wav_at(pcm: &[f32], p: &Path) {
        let spec = hound::WavSpec { channels: 1, sample_rate: ra_timing::SR, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create(p, spec).unwrap();
        for s in pcm {
            w.write_sample((s.clamp(-1.0, 1.0) * 32767.0) as i16).unwrap();
        }
        w.finalize().unwrap();
    }

    fn html_escape(s: &str) -> String {
        s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
    }

    fn file_url(p: &Path) -> String {
        format!("file:///{}", p.display().to_string().replace('\\', "/").replace(' ', "%20").replace('\'', "%27"))
    }

    /// Names A/B listening board (spec 2.5): every word of tools/read-aloud/names.txt in
    /// "Today, WORD spoke." at 2.2x on the real CPU model, before (plain misaki, empty
    /// names table: the old caps rule) and after (full pipeline). WAVs go to
    /// %TEMP%\echo-names-ab\{before,after}; writes tools/read-aloud/names_ab.html.
    /// cargo test --release names_ab_board -- --ignored --nocapture
    #[test]
    #[ignore]
    fn names_ab_board() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("tools").join("read-aloud");
        let list = std::fs::read_to_string(root.join("names.txt")).unwrap();
        let entries: Vec<&str> = list.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
        assert_eq!(entries.len(), 74, "59 names + 15 controls");
        let out = std::env::temp_dir().join("echo-names-ab");
        for side in ["before", "after"] {
            std::fs::create_dir_all(out.join(side)).unwrap();
        }
        let e = Engine::new(dirs::data_dir().unwrap().join("ALAN Echo"));
        e.preload("af_heart", "cpu");
        let eng = e.acquire().expect("engine (real CPU model in %APPDATA%\\ALAN Echo\\models\\kokoro)");
        let mut l = eng.lock();
        assert_eq!(l.device, Device::Cpu);
        let mut plain_g2p = G2P::new(Language::EnglishUS);
        let mut no_names = Arc::new(Names::empty());
        let mut rows = String::new();
        let mut changed = 0;
        let t = Instant::now();
        for (i, word) in entries.iter().enumerate() {
            let words = ["Today,", *word, "spoke."];
            let safe: String = word.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
            let file = format!("{:02}_{safe}.wav", i + 1);
            let mut cells = Vec::new();
            for side in ["before", "after"] {
                let before = side == "before";
                if before {
                    std::mem::swap(&mut l.g2p, &mut plain_g2p);
                    std::mem::swap(&mut l.names, &mut no_names);
                }
                let ph = phonemes_per_word(&l.g2p, &l.names, &words)[1].clone();
                let a = Engine::synth_inner(&mut l, &words, 2.2, true);
                if before {
                    std::mem::swap(&mut l.g2p, &mut plain_g2p);
                    std::mem::swap(&mut l.names, &mut no_names);
                }
                let a = a.unwrap_or_else(|err| panic!("{side} {word}: {err}"));
                assert_eq!(a.words.len(), 3, "{side} {word}: one timing span per word");
                let p = out.join(side).join(&file);
                write_wav_at(&a.pcm, &p);
                cells.push((p, ph));
            }
            let diff = cells[0].1 != cells[1].1;
            changed += usize::from(diff);
            println!("{:>2} {:<12} {:<28} -> {}", i + 1, word, cells[0].1, cells[1].1);
            rows.push_str(&format!(
                "<tr{}><td>{}</td><td class=w>{}</td><td><audio controls preload=none src=\"{}\"></audio></td><td><audio controls preload=none src=\"{}\"></audio></td><td class=ph>{}<br><b>{}</b></td></tr>\n",
                if diff { " class=chg" } else { "" },
                i + 1,
                html_escape(word),
                file_url(&cells[0].0),
                file_url(&cells[1].0),
                html_escape(&cells[0].1),
                html_escape(&cells[1].1),
            ));
        }
        drop(l);
        let html = format!(
            r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Names A/B Board</title>
<style>
:root {{ --bg:#fafaf8; --fg:#1b1b1b; --muted:#666; --line:#ddd; --chg:#fff6d6; }}
@media (prefers-color-scheme: dark) {{ :root {{ --bg:#16171a; --fg:#e8e8e8; --muted:#9a9a9a; --line:#333; --chg:#3a3320; }} }}
body {{ background:var(--bg); color:var(--fg); font:15px/1.4 system-ui,sans-serif; margin:0; padding:16px; }}
h1 {{ font-size:20px; margin:0 0 4px; }} p {{ color:var(--muted); margin:0 0 12px; }}
table {{ border-collapse:collapse; width:100%; }} td,th {{ border-bottom:1px solid var(--line); padding:6px 8px; text-align:left; vertical-align:middle; }}
tr.chg {{ background:var(--chg); }} .w {{ font-weight:600; white-space:nowrap; }} .ph {{ font-family:Consolas,monospace; font-size:13px; }}
audio {{ height:32px; max-width:220px; }}
</style></head><body>
<h1>Read Aloud names: before vs after</h1>
<p>"Today, WORD spoke." at 2.2x, CPU model, voice af_heart. Before = plain misaki (letters spelled). After = hand list + CMUdict + g2p_en. Highlighted rows changed ({changed} of {n}). Phonemes: before, then <b>after</b>.</p>
<table><thead><tr><th>#</th><th>Word</th><th>Before</th><th>After</th><th>Phonemes</th></tr></thead><tbody>
{rows}</tbody></table></body></html>
"#,
            n = entries.len(),
        );
        let page = root.join("names_ab.html");
        std::fs::write(&page, html).unwrap();
        println!("{changed} of {} words changed; rendered in {:?}", entries.len(), t.elapsed());
        println!("A/B board: {}", page.canonicalize().unwrap_or(page.clone()).display());
    }

    fn private_mb() -> f64 {
        let o = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &format!("(Get-Process -Id {}).PrivateMemorySize64", std::process::id())])
            .output()
            .unwrap();
        String::from_utf8_lossy(&o.stdout).trim().parse::<f64>().unwrap() / 1_048_576.0
    }

    /// Cost of the names pipeline: load time (vs the plain G2P it used to build, and the
    /// ONNX session open it overlaps) and private-RAM delta while loaded.
    /// cargo test --release names_load_cost -- --ignored --nocapture
    #[test]
    #[ignore]
    fn names_load_cost() {
        let t = Instant::now();
        let plain = G2P::new(Language::EnglishUS);
        let plain_ms = t.elapsed();
        drop(plain);
        let m0 = private_mb();
        let t = Instant::now();
        let names = Names::load(G2pEn::load().map(|g| Box::new(g) as Box<dyn Lts>));
        let names_ms = t.elapsed();
        let m1 = private_mb();
        assert!(names.knows("pelosi"));
        drop(names);
        let m2 = private_mb();
        let t = Instant::now();
        let parts = build_g2p();
        let full_ms = t.elapsed();
        drop(parts);
        let f = ra_model::files(&dirs::data_dir().unwrap().join("ALAN Echo"));
        let t = Instant::now();
        let session = open_session(&f.fp32, false);
        let session_ms = t.elapsed();
        drop(session);
        println!("plain G2P::new        {plain_ms:?}");
        println!("Names::load(G2pEn)    {names_ms:?}");
        println!("build_g2p (both)      {full_ms:?}");
        println!("CPU session open      {session_ms:?} (runs in parallel with build_g2p)");
        println!("added to first chunk  {:?}", full_ms.max(session_ms).saturating_sub(plain_ms.max(session_ms)));
        println!("private RAM: before {m0:.1} MB, loaded {m1:.1} MB (+{:.1}), after drop {m2:.1} MB", m1 - m0);
    }
}
