//! Read Aloud — Kokoro-82M via ONNX Runtime. Loaded on demand (hotkey press or
//! first `synth`), dropped after 2 idle minutes. Nothing is loaded (0 MB) until
//! asked; the idle-watch thread exits once the engine is unloaded or replaced.
//!
//! Device policy (controller ruling, Task 5 review): `pref` "auto" (and anything
//! but "gpu") runs on CPU (fp32): it frees fully on unload and stays < 500 MB.
//! DirectML (q8) is used only for `pref == "gpu"`; if it fails to open or to run,
//! the engine falls back to CPU by itself.
use crate::{
    ra_model, ra_text,
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
        // G2P init (lexicon + tagger parse) overlaps the session open
        let g2p_job = std::thread::Builder::new().name("ra-tts-g2p".into()).spawn(|| G2P::new(Language::EnglishUS)).ok();
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
        let g2p = match g2p_job.map(|h| h.join()) {
            Some(Ok(g)) => g,
            _ => G2P::new(Language::EnglishUS), // spawn failed (or, debug only, init panicked)
        };
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
        let fed = g2p_words(&l.g2p, words);
        let fed: Vec<&str> = fed.iter().map(|w| w.as_ref()).collect();
        let text = fed.join(" ");
        if text.trim().is_empty() {
            return Ok(empty());
        }
        let (_, tokens) = l.g2p.g2p(&text).map_err(|e| format!("g2p: {e:?}"))?;
        let toks: Vec<(&str, Option<&str>, bool)> =
            tokens.iter().map(|t| (t.text.as_str(), t.phonemes.as_deref(), !t.whitespace.is_empty())).collect();
        let tok_text: Vec<&str> = toks.iter().map(|t| t.0).collect();
        // `fed` keeps the word count and order of `words`, so indices line up
        let owner = ra_timing::align(&fed, &tok_text);
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

/// G2P input for a chunk: ALL-CAPS dictionary words lowercased so misaki reads
/// them as words instead of spelling them (see `ra_text::speakable_caps`).
fn g2p_words<'a>(g: &G2P, words: &[&'a str]) -> Vec<Cow<'a, str>> {
    let known = |w: &str| lexicon_knows(&g.lexicon, w);
    words.iter().map(|w| ra_text::speakable_caps(w, &known)).collect()
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

    fn real_g2p() -> &'static G2P {
        static G: std::sync::OnceLock<G2P> = std::sync::OnceLock::new();
        G.get_or_init(|| G2P::new(Language::EnglishUS))
    }

    fn word_phonemes(text_words: &[&str]) -> Vec<(String, String)> {
        let g = real_g2p();
        let fed = g2p_words(g, text_words);
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
        let g = real_g2p();
        let words = ["a", "mid-year", "plan"];
        let fed = g2p_words(g, &words);
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
        let g = real_g2p();
        let words = ["FBI", "GDP", "CEO", "NATO", "NASA", "US", "IT", "WHO", "THE,", "TALKS."];
        let fed: Vec<String> = g2p_words(g, &words).into_iter().map(|c| c.into_owned()).collect();
        assert_eq!(fed, vec!["FBI", "GDP", "CEO", "NATO", "NASA", "US", "IT", "WHO", "the,", "talks."]);
    }

    #[test]
    fn caps_rewrite_is_cheap() {
        let g = real_g2p();
        let words: Vec<&str> = ["THE", "PRESIDENT", "SAID", "FBI", "TALKS", "GDP", "MID-YEAR", "iPhone", "42"].iter().cycle().take(30).copied().collect();
        let t = Instant::now();
        for _ in 0..100 {
            let _ = g2p_words(g, &words);
        }
        assert!(t.elapsed() < Duration::from_millis(500), "{:?} for 100 chunks", t.elapsed());
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
}
