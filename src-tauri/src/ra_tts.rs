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
    ra_homograph, ra_model,
    ra_names::{self, Lex, Lts, NameFallback, Names},
    ra_norm, ra_text,
    ra_timing::{self, WordTime},
    ra_wsola,
};
use misaki_rs::{Language, Lexicon, MToken, G2P};
use ort::{ep, session::RunOptions, session::Session, value::Tensor};
use parking_lot::{Condvar, Mutex, MutexGuard};
use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;
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
    /// The synth run in flight. A jump terminates a run whose generation went stale
    /// (`cancel_stale`), so the worker is free for the new position at once instead of
    /// finishing the old chunk. Keyed by controller generation too (round 3): the
    /// engine outlives controllers, whose job generations restart at 0.
    run: Mutex<Option<Run>>,
    /// Controller generations handed out (`controller_gen`).
    ctl_seq: AtomicU64,
}

/// The synth run in flight (see `Engine::run`).
pub struct Run {
    /// The controller generation whose worker runs it.
    pub ctl: u64,
    /// Its job generation (that controller's `Session::gen`).
    pub gen: u64,
    pub opts: Arc<RunOptions>,
    /// Terminated by `cancel_stale`.
    pub done: bool,
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

/// Round 2 gapless: the reading speed split into the model's own speed and the
/// WSOLA stretch, `(model, stretch)` with `model * stretch == rate`. Up to 2.2x the
/// model speaks at 1.0 and WSOLA does it all (as before). Faster than that, the model
/// speaks faster too, up to `MODEL_SPEED_MAX` (1.25), and WSOLA does the rest. A
/// documented exception to the phase-1 rule "never Kokoro speed > 1.0" (phase-3 spec,
/// round-2 note): on 100 random sentences Whisper's word error rate at 3.0x / 3.5x
/// was 9.6% / 15.3% with WSOLA alone, 8.4% / 11.4% with the model at up to 1.25, and
/// 17.5% / 28.7% with it at up to 1.6 (round 2's first try). The faster model also
/// cuts the CPU synth time a 3-3.5x read needs.
fn split_rate(rate: f32) -> (f32, f32) {
    let rate = effective_rate(rate);
    let model = (rate / WSOLA_MAX).clamp(1.0, model_speed_max());
    (model, rate / model)
}

/// `MODEL_SPEED_MAX`; tests and the clarity bench can override it
/// (`set_model_speed_max`, 1.0 = pure WSOLA as before round 2).
#[cfg(not(test))]
fn model_speed_max() -> f32 {
    MODEL_SPEED_MAX
}
#[cfg(test)]
static MODEL_MAX_OVERRIDE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
#[cfg(test)]
fn model_speed_max() -> f32 {
    match MODEL_MAX_OVERRIDE.load(Ordering::Relaxed) {
        0 => MODEL_SPEED_MAX,
        b => f32::from_bits(b),
    }
}
#[cfg(test)]
pub(crate) fn set_model_speed_max(v: f32) {
    MODEL_MAX_OVERRIDE.store(v.to_bits(), Ordering::Relaxed);
}
const WSOLA_MAX: f32 = 2.2;

/// The error of a synth run stopped by `Engine::cancel_stale`.
pub const CANCELLED: &str = "cancelled: stale job";

/// Silence kept before a chunk's first sound, at 1x (divided by the reading speed),
/// but never less than `KEEP_LEAD_MIN_MS` whatever the speed: a quiet first sound
/// ("h", "f", "s") fades in below the threshold and must not be clipped.
const KEEP_LEAD_MS: f32 = 40.0;
const KEEP_LEAD_MIN_MS: f32 = 30.0;
/// Silence kept after a chunk's last sound, at 1x (divided by the speed), by how the
/// chunk ends (`Tail`): a sentence end keeps the longest pause, a comma (a ramp cut at
/// a clause) a comma-length one, a cut mid-phrase a short one. Round 2 trimmed every
/// chunk to 100 ms, so a ramp cut at a comma paused less than a comma inside a chunk.
const KEEP_TRAIL_SENTENCE_MS: f32 = 200.0;
const KEEP_TRAIL_COMMA_MS: f32 = 150.0;
const KEEP_TRAIL_PHRASE_MS: f32 = 60.0;
/// A 10 ms window is silence when its peak is under this fraction of the chunk's
/// peak (-50 dB re peak), so a quiet onset of a loud chunk still counts as sound.
const SILENCE_RE_PEAK: f32 = 0.003_162;

/// How a chunk ends, for the pause kept after it (`trim_silence`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tail {
    Sentence,
    Comma,
    Phrase,
}

/// The `Tail` of a chunk from its last source word.
fn tail_of(last: Option<&str>) -> Tail {
    let w = last.unwrap_or("").trim_end_matches(['"', '\'', ')', ']', '\u{201D}', '\u{2019}']);
    if w.ends_with(['.', '!', '?']) || w.is_empty() {
        Tail::Sentence
    } else if w.ends_with([',', ';', ':', '\u{2014}', '\u{2013}']) {
        Tail::Comma
    } else {
        Tail::Phrase
    }
}

/// Trims a chunk's leading and trailing silence to the kept lengths above (at 1x,
/// divided by `rate`; the lead never under 30 ms) and shifts the word times to match.
/// Silence is judged relative to the chunk's own peak. Never trims inside the speech,
/// never adds silence; an all-silent chunk is left as it is. Word times stay one per
/// word, clamped to the new length.
fn trim_silence(pcm: Vec<f32>, words: &mut [WordTime], rate: f32, tail: Tail) -> Vec<f32> {
    let sr = ra_timing::SR as f32;
    let win = (sr * 0.010) as usize;
    let n = pcm.len() / win;
    let peak = pcm.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    if peak <= 0.0 {
        return pcm;
    }
    let thr = peak * SILENCE_RE_PEAK;
    let loud = |k: usize| pcm[k * win..(k + 1) * win].iter().any(|x| x.abs() > thr);
    let Some(first) = (0..n).find(|&k| loud(k)) else { return pcm };
    let last = (0..n).rev().find(|&k| loud(k)).unwrap_or(first);
    let rate = rate.max(1.0);
    let lead_ms = (KEEP_LEAD_MS / rate).max(KEEP_LEAD_MIN_MS);
    let trail_ms = match tail {
        Tail::Sentence => KEEP_TRAIL_SENTENCE_MS,
        Tail::Comma => KEEP_TRAIL_COMMA_MS,
        Tail::Phrase => KEEP_TRAIL_PHRASE_MS,
    } / rate;
    let keep_lead = (lead_ms / 1000.0 * sr) as usize;
    let keep_trail = (trail_ms / 1000.0 * sr) as usize;
    let start = (first * win).saturating_sub(keep_lead);
    let end = ((last + 1) * win + keep_trail).min(pcm.len());
    if start == 0 && end == pcm.len() {
        return pcm;
    }
    let (s, len) = (start as u64, (end - start) as u64);
    for w in words.iter_mut() {
        w.start = w.start.saturating_sub(s).min(len);
        w.end = w.end.saturating_sub(s).min(len).max(w.start);
    }
    pcm[start..end].to_vec()
}
const MODEL_SPEED_MAX: f32 = 1.25;

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
            run: Mutex::new(None),
            ctl_seq: AtomicU64::new(0),
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

    /// Synthesizes one chunk (the model at 1.0x, or faster above 2.2x: `split_rate`), then stretches to `rate`. Loads the engine
    /// if needed (blocking, up to 60 s). On a DirectML runtime error it switches
    /// to CPU and retries once; an `Err("dml: …")` only escapes if that fails too.
    #[cfg_attr(not(test), allow(dead_code))] // tests, benches, the glitch hunt
    pub fn synth(self: &Arc<Self>, words: &[&str], rate: f32) -> Result<ChunkAudio, String> {
        self.synth_opts(words, rate, None)
    }

    /// A new controller's generation (each controller's job generations restart at 0).
    pub fn controller_gen(&self) -> u64 {
        self.ctl_seq.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Register a run of controller `ctl`'s job generation `gen` in the slot. A worker
    /// of an older controller never overwrites a newer controller's run (`None`: its
    /// run is simply not cancellable).
    pub fn begin_run(&self, ctl: u64, gen: u64) -> Option<Arc<RunOptions>> {
        let mut slot = self.run.lock();
        if slot.as_ref().is_some_and(|r| r.ctl > ctl) {
            return None;
        }
        let opts = Arc::new(RunOptions::new().ok()?);
        *slot = Some(Run { ctl, gen, opts: Arc::clone(&opts), done: false });
        Some(opts)
    }

    /// The run with these options finished: free the slot if it is still ours (never
    /// someone else's) and say whether it was cancelled.
    pub fn end_run(&self, opts: Option<&Arc<RunOptions>>) -> bool {
        let Some(o) = opts else { return false };
        let mut slot = self.run.lock();
        if slot.as_ref().is_some_and(|r| Arc::ptr_eq(&r.opts, o)) {
            return slot.take().is_some_and(|r| r.done);
        }
        false
    }

    /// Our run (these options) was terminated by `cancel_stale`.
    fn run_cancelled(&self, opts: Option<&RunOptions>) -> bool {
        let Some(o) = opts else { return false };
        self.run.lock().as_ref().is_some_and(|r| std::ptr::eq(Arc::as_ptr(&r.opts), o) && r.done)
    }

    /// `synth` for a queued job of controller `ctl`'s generation `gen`: `cancel_stale`
    /// can stop it mid-run (it then returns `Err(CANCELLED)`, which the controller
    /// ignores as stale).
    pub fn synth_job(self: &Arc<Self>, words: &[&str], rate: f32, ctl: u64, gen: u64) -> Result<ChunkAudio, String> {
        let opts = self.begin_run(ctl, gen);
        let r = self.synth_opts(words, rate, opts.as_deref());
        if self.end_run(opts.as_ref()) { Err(CANCELLED.into()) } else { r }
    }

    /// Terminates the synth run in flight if it is stale for controller `ctl` at job
    /// generation `gen`: another generation (a jump), or an older controller's run.
    /// True when a run was stopped. Never blocks on the model.
    pub fn cancel_stale(&self, ctl: u64, gen: u64) -> bool {
        let mut run = self.run.lock();
        match run.as_mut() {
            Some(r) if (r.ctl != ctl || r.gen != gen) && r.ctl <= ctl && !r.done => {
                r.done = r.opts.terminate().is_ok();
                r.done
            }
            _ => false,
        }
    }

    fn synth_opts(self: &Arc<Self>, words: &[&str], rate: f32, opts: Option<&RunOptions>) -> Result<ChunkAudio, String> {
        self.busy.fetch_add(1, Ordering::SeqCst);
        self.touch();
        let run = || self.acquire().and_then(|eng| Self::synth_inner_with(&mut eng.lock(), words, rate, true, opts));
        let mut r = run();
        let cancelled = self.run_cancelled(opts);
        if let (Err(e), false) = (&r, cancelled) {
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

    #[cfg_attr(not(test), allow(dead_code))] // tests, benches, the glitch hunt
    fn synth_inner(l: &mut Loaded, words: &[&str], rate: f32, map_joined: bool) -> Result<ChunkAudio, String> {
        Self::synth_inner_with(l, words, rate, map_joined, None)
    }

    fn synth_inner_with(
        l: &mut Loaded,
        words: &[&str],
        rate: f32,
        map_joined: bool,
        opts: Option<&RunOptions>,
    ) -> Result<ChunkAudio, String> {
        let device = l.device;
        // the model's own speed above 2.2x, WSOLA for the rest (`split_rate`)
        let (model_speed, rate) = split_rate(rate);
        let empty = || empty_audio(device, words.len());
        let p = prepare_chunk(&l.g2p, &l.names, words)?;
        let (ids, spans, covered) = build_ids_with(&p.ph, &l.vocab, words.len(), map_joined);
        let covered = cover_spans(covered, &p.inner).min(words.len());
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
        let t_speed = Tensor::from_array(([1usize], vec![model_speed])).map_err(|e| e.to_string())?;
        let (wav, dur) = {
            let inputs = ort::inputs!["input_ids" => t_ids, "style" => t_style, "speed" => t_speed];
            let out = match opts {
                Some(o) => l.session.run_with_options(inputs, o),
                None => l.session.run(inputs),
            }
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
        let mut words_t = chunk_word_times(&dur, spans, wav.len(), rate, covered);
        let pcm = ra_wsola::stretch(&wav, rate, ra_timing::SR);
        let pcm = trim_silence(pcm, &mut words_t, model_speed * rate, tail_of(words.get(covered.saturating_sub(1)).copied()));
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

/// No audio for a chunk (nothing speakable, e.g. only emoji): every word still gets
/// one zero-length timing entry, so the highlight passes over it.
fn empty_audio(device: Device, n_words: usize) -> ChunkAudio {
    let words = (0..n_words).map(|word| WordTime { word, start: 0, end: 0 }).collect();
    ChunkAudio { pcm: Vec::new(), words, device, covered: n_words }
}

/// One chunk through G2P, as the model is fed it.
struct Prepared {
    /// `g2p_words` output, one entry per word ("" for a word inside a span).
    #[cfg_attr(not(test), allow(dead_code))] // read by the glitch hunt
    fed: Vec<String>,
    /// misaki's tokens (homographs applied) and the word each belongs to.
    #[cfg_attr(not(test), allow(dead_code))] // read by the glitch hunt
    tokens: Vec<MToken>,
    #[cfg_attr(not(test), allow(dead_code))] // read by the glitch hunt
    owner: Vec<Option<usize>>,
    /// (word or NO_WORD, phonemes, followed-by-whitespace) per model token.
    ph: Vec<(usize, String, bool)>,
    /// The words inside multi-word spans (`$5 billion` -> the "billion" index). They
    /// have no audio: their span is said on its first word.
    inner: Vec<Range<usize>>,
    /// Every applied span: (start, end, spoken text).
    #[cfg_attr(not(test), allow(dead_code))] // read by the glitch hunt
    spoken: Vec<(usize, usize, String)>,
}

/// Runs a chunk through the pre-pass, misaki, alignment and the homograph table.
/// `synth_inner` and the glitch hunt both use it, so the hunt measures what is spoken.
fn prepare_chunk(g: &G2P, names: &Names, words: &[&str]) -> Result<Prepared, String> {
    let fed = g2p_chunk(g, names, words, 0);
    // align against the words as misaki tokenizes them (overrides stripped)
    let plain: Vec<Cow<str>> = fed.words.iter().map(|w| plain_word(w)).collect();
    let plain: Vec<&str> = plain.iter().map(|w| w.as_ref()).collect();
    let text = fed.words.iter().map(|w| w.as_ref()).collect::<Vec<&str>>().join(" ");
    let mut tokens = if text.trim().is_empty() {
        Vec::new()
    } else {
        run_g2p(g, &text)?
    };
    let tok_text: Vec<&str> = tokens.iter().map(|t| t.text.as_str()).collect();
    // `plain` keeps the word count and order of `words`, so indices line up
    let owner = ra_timing::align(&plain, &tok_text);
    // a span or hand-list override already says the word: no homograph swap there
    let overridden: Vec<bool> =
        owner.iter().map(|o| o.and_then(|w| fed.words.get(w)).is_some_and(|f| f.contains("](/"))).collect();
    apply_homographs(&mut tokens, &overridden);
    let toks: Vec<(&str, Option<&str>, bool)> =
        tokens.iter().map(|t| (t.text.as_str(), t.phonemes.as_deref(), !t.whitespace.is_empty())).collect();
    let ph = token_phonemes(&toks, &owner);
    drop(toks);
    Ok(Prepared { fed: fed.words.into_iter().map(Cow::into_owned).collect(), tokens, owner, ph, inner: fed.inner, spoken: fed.spoken })
}

fn run_g2p(g: &G2P, text: &str) -> Result<Vec<MToken>, String> {
    g.g2p(text).map(|(_, t)| t).map_err(|e| format!("g2p: {e:?}"))
}

/// Homographs (spec §2): after misaki has tagged the chunk, a table word gets the
/// reading its tag and one word of context on each side pick (`I have read it`).
/// Tokens marked in `skip` (their word is an override) keep their phonemes.
fn apply_homographs(toks: &mut [MToken], skip: &[bool]) {
    for i in 0..toks.len() {
        if skip.get(i).copied().unwrap_or(false) {
            continue;
        }
        let pick = {
            let Some(t) = toks.get(i) else { break };
            // table words are 3-8 ASCII letters: skip everything else without allocating
            if !(3..=8).contains(&t.text.len()) || !t.text.bytes().all(|b| b.is_ascii_alphabetic()) {
                continue;
            }
            let prev = i.checked_sub(1).and_then(|j| toks.get(j)).map(|p| p.text.as_str());
            let next = toks.get(i + 1).map(|n| n.text.as_str());
            ra_homograph::reading(&t.text.to_ascii_lowercase(), &t.tag, prev, next)
        };
        if let (Some(r), Some(t)) = (pick, toks.get_mut(i)) {
            t.phonemes = Some(r.to_string());
        }
    }
}

/// `covered` (leading words fully synthesized) never stops inside a span: the
/// span's first word speaks all of it, so its inner words are covered with it.
fn cover_spans(covered: usize, inner: &[Range<usize>]) -> usize {
    inner.iter().fold(covered, |c, r| if c >= r.start && c < r.end { r.end } else { c })
}

/// Per-word times from the model's durations: token spans merged per word, then
/// one entry for every word `0..covered`, in order. A word with no audio of its
/// own (inside a span, or a lone dash) gets a zero-length entry where the word
/// before it ends, so the highlight passes over it and never moves backwards.
fn chunk_word_times(dur: &[f32], spans: Vec<(usize, usize, usize)>, n_samples: usize, rate: f32, covered: usize) -> Vec<WordTime> {
    // one span per word: merge consecutive token spans of the same word, drop unowned
    let mut merged: Vec<(usize, usize, usize)> = Vec::new();
    for sp in spans.into_iter().filter(|sp| sp.0 != NO_WORD) {
        match merged.last_mut() {
            Some(m) if m.0 == sp.0 => m.2 = sp.2,
            _ => merged.push(sp),
        }
    }
    let timed = ra_timing::word_times(dur, &merged, n_samples, OFFSET_MS, rate);
    let mut out = Vec::with_capacity(covered.max(timed.len()));
    let mut it = timed.into_iter().peekable();
    let mut last_end: Option<u64> = None;
    for w in 0..covered {
        let mut found = false;
        while let Some(e) = it.next_if(|e| e.word <= w) {
            found |= e.word == w;
            last_end = Some(e.end);
            out.push(e);
        }
        if !found {
            let at = last_end.or_else(|| it.peek().map(|e| e.start)).unwrap_or(0);
            out.push(WordTime { word: w, start: at, end: at });
        }
    }
    out.extend(it);
    out
}

/// `g2p_chunk` at depth 0, words only (tests and the glitch hunt).
#[cfg(test)]
fn g2p_words<'a>(g: &G2P, names: &Names, words: &[&'a str]) -> Vec<Cow<'a, str>> {
    g2p_chunk(g, names, words, 0).words
}

struct Fed<'a> {
    words: Vec<Cow<'a, str>>,
    inner: Vec<Range<usize>>,
    spoken: Vec<(usize, usize, String)>,
}

/// G2P input for a chunk, one entry per word (same count and order):
/// - **pre-pass** (`ra_norm`, spec §2): curly apostrophes and Unicode hyphens are
///   fixed first; then every span of words that normalises (`$3.2B`, `$5 billion`,
///   `U.S.`, `1990,`) is said on its first word as `[key](/phonemes/)`, where the
///   phonemes are the span's spoken text through this same G2P (depth 1: no
///   pre-pass again) and `key` is that word's letters and digits (one plain misaki
///   token, so alignment stays one token group per word). The span's trailing
///   punctuation stays outside, so its pause or sentence period survives. The other
///   words of the span are fed as "" (no tokens: alignment skips them and timing
///   gives them zero length); the simpler choice over empty overrides, which
///   misaki would still tokenize.
/// - a hand-list word (Xi, Nguyen) becomes misaki's `[word](/phonemes/)` override,
///   so the hand list wins even where misaki has its own reading;
/// - ALL-CAPS words are lowercased when misaki or the names dictionary knows them,
///   or they are long enough to be a word (`ra_text::speakable_caps`);
/// - page `[` `]` become `(` `)`, so page text is never read as override syntax
///   (Kokoro has no bracket symbols; parens give the same pause).
/// Hyphen-joined parts are handled one by one. Use `plain_word` for alignment.
///
/// Context limit: only the chunk's words are seen, so its last word is read as a
/// sentence end. `ra_text::chunk` cuts at sentence ends and clause marks (and never
/// after a title such as `Sen.`); only a hard length cut mid-sentence can add a
/// period after an initialism or leave a title unexpanded at the chunk's end.
///
/// Returns the span ranges too. `depth` 0 runs the pre-pass; depth 1 (a span's
/// spoken text) does not, so phonemising never recurses further.
fn g2p_chunk<'a>(g: &G2P, names: &Names, words: &[&'a str], depth: u8) -> Fed<'a> {
    let prepared: Vec<Cow<'a, str>> = words.iter().map(|w| ra_norm::prepare(w)).collect();
    let mut out: Vec<Option<Cow<'a, str>>> = vec![None; words.len()];
    let mut inner = Vec::new();
    let mut spoken = Vec::new();
    if depth == 0 {
        let refs: Vec<&str> = prepared.iter().map(|c| c.as_ref()).collect();
        let spans = ra_norm::spans(&refs);
        // one misaki call for every span of the chunk (see `spans_phonemes`)
        let edges: Vec<(&str, &str, &str)> = spans.iter().map(|sp| split_edges(&sp.spoken)).collect();
        let cores: Vec<&str> = edges.iter().map(|e| e.1).collect();
        let phs = spans_phonemes(g, names, &cores);
        for ((sp, (lead, _, trail)), ph) in spans.iter().zip(&edges).zip(phs) {
            let (Some(first), Some(ph)) = (refs.get(sp.start), ph) else {
                log::debug!("ra_tts: no phonemes for span {:?}; old path", sp.spoken);
                continue;
            };
            let Some(slots) = out.get_mut(sp.start..sp.end) else { continue };
            let o = span_override(first, lead, &ph, trail);
            for (k, slot) in slots.iter_mut().enumerate() {
                *slot = Some(if k == 0 { Cow::Owned(o.clone()) } else { Cow::Borrowed("") });
            }
            if sp.end - sp.start > 1 {
                inner.push(sp.start + 1..sp.end);
            }
            spoken.push((sp.start, sp.end, sp.spoken.clone()));
        }
    }
    let head = headline_flags(&prepared);
    let words = out
        .into_iter()
        .zip(prepared)
        .zip(head)
        .map(|((o, p), h)| match (o, p) {
            (Some(o), _) => o,
            (None, Cow::Borrowed(b)) => word_path(g, names, b, h),
            (None, Cow::Owned(s)) => Cow::Owned(word_path(g, names, &s, h).into_owned()),
        })
        .collect();
    Fed { words, inner, spoken }
}

/// A word whose letters are all capitals (2+ letters, optional `'s` / plural `s`).
fn is_caps_word(w: &str) -> bool {
    let core = w.trim_matches(|c: char| !c.is_alphanumeric());
    let core = core.strip_suffix("'s").or_else(|| core.strip_suffix("\u{2019}s")).unwrap_or(core);
    let core = core.strip_suffix('s').unwrap_or(core);
    core.len() >= 2 && core.bytes().all(|b| b.is_ascii_uppercase())
}

/// How capitalised a word's sentence is (see `headline_flags`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CapsMode {
    /// Normal text: a 2-3 letter caps token is an acronym.
    Mixed,
    /// A headline (80%+ caps): dictionary words read as words.
    Headline,
    /// Every 2+ letter word is caps ("SHE SAT DOWN"): also the ambiguous acronyms
    /// (`ra_names::AMBIGUOUS_ACRONYMS`: SAT, ERA, AD) read as words.
    AllCaps,
}

/// Per word: headline mode. A sentence of the chunk is a headline when at least 80%
/// of its alphabetic words (2+ letters) are ALL-CAPS and there are at least 3 of
/// them ("FED CUTS RATES AGAIN"); all of them makes it `AllCaps`. "took the SAT,
/// ACT and AP" (3 of 6), "the ER, ICU and VA" (3 of 5) and "DO NOT" (2) are mixed.
fn headline_flags(words: &[Cow<str>]) -> Vec<CapsMode> {
    let mut out = vec![CapsMode::Mixed; words.len()];
    let mut start = 0;
    let letters = |w: &str| w.chars().filter(|c| c.is_alphabetic()).count();
    for i in 0..words.len() {
        let w = words[i].trim_end_matches(|c: char| matches!(c, '"' | '\'' | ')' | ']' | '\u{201D}' | '\u{2019}'));
        if i + 1 == words.len() || w.ends_with(['.', '!', '?']) {
            let sent = &words[start..=i];
            let alpha = sent.iter().filter(|w| letters(w) >= 2).count();
            let caps = sent.iter().filter(|w| is_caps_word(w)).count();
            let mode = if caps < 3 || caps * 10 < alpha * 8 {
                CapsMode::Mixed
            } else if caps >= alpha {
                CapsMode::AllCaps
            } else {
                CapsMode::Headline
            };
            for o in out.iter_mut().skip(start).take(i + 1 - start) {
                *o = mode;
            }
            start = i + 1;
        }
    }
    out
}

/// Alex's acronym rule for a 2-5 letter ALL-CAPS token (optional `'s` / plural `s`):
/// - a `ra_names::PRONOUNCED` acronym is a word (NASA, NATO, SPACs): misaki's caps
///   entry, else the lowercase word;
/// - an acronym list entry (`SPELLED`, `KEEP_CAPS`, `ACRONYM_WORDS`, a CMUdict
///   letter-spelled entry) is spelled;
/// - in mixed-case text a 2-3 letter token is spelled (SAT, ACT, ISS, IRAs) unless
///   it is an `EMPHASIS` word ("do NOT", "a BIG deal", "the BUS"); a 4-5 letter
///   token that is a common dictionary word reads as that word (HUGE, SORRY, NEWS);
/// - in a headline (`headline`) a dictionary word is a word (THE NEW TAX LAW).
/// "Dictionary word": a misaki gold word (an inflected one at 4+ letters, HITS) or,
/// at 4+ letters, a CMUdict word not read letter by letter. Anything else is
/// spelled, so a 2-5 letter caps token never reaches the GRU. Roman numerals (II,
/// XIV) and other lengths are left to the other rules. `None`: not decided here.
fn caps_rule(w: &str, names: &Names, lex: &Lex, known: &dyn Fn(&str) -> bool, mode: CapsMode) -> Option<String> {
    let headline = mode != CapsMode::Mixed;
    let start = w.find(char::is_alphanumeric)?;
    let (last, ch) = w.char_indices().rev().find(|(_, c)| c.is_alphanumeric())?;
    let end = last + ch.len_utf8();
    let core = &w[start..end];
    let poss = ["'s", "\u{2019}s", "'S", "\u{2019}S"].iter().find_map(|x| core.strip_suffix(x));
    let plural = || core.strip_suffix('s').filter(|s| s.len() >= 2 && s.bytes().all(|b| b.is_ascii_uppercase()));
    let (stem, suf) = match poss.or_else(plural) {
        Some(s) => (s, true),
        None => (core, false),
    };
    if !(2..=5).contains(&stem.len()) || !stem.bytes().all(|b| b.is_ascii_uppercase()) {
        return None;
    }
    if stem.bytes().all(|b| matches!(b, b'I' | b'V' | b'X')) {
        return None; // Roman numerals: the text pass and misaki
    }
    let lead = unbracket(Cow::Borrowed(&w[..start]));
    let trail = unbracket(Cow::Borrowed(&w[end..]));
    if ra_names::PRONOUNCED.contains(&stem) {
        if !suf && known(stem) {
            return None; // misaki has the caps word (NASA, NATO)
        }
        return Some(format!("{lead}{}{trail}", core.to_lowercase()));
    }
    let low = stem.to_ascii_lowercase();
    // an all-caps plural (IRAS, CEOS) is checked by its stem too
    let bare = stem.strip_suffix('S').filter(|b| b.len() >= 2);
    let on_list = |x: &str| {
        ra_names::SPELLED.contains(&x)
            || ra_text::KEEP_CAPS.contains(&x)
            || ra_names::ACRONYM_WORDS.contains(&x)
            || (mode != CapsMode::AllCaps && ra_names::AMBIGUOUS_ACRONYMS.contains(&x))
    };
    let listed = on_list(stem) || bare.is_some_and(on_list) || names.letter_spelled(&low);
    let word = if listed {
        false
    } else if ra_names::EMPHASIS.contains(&stem) && !suf {
        true
    } else if !headline && stem.len() <= 3 {
        false
    } else {
        // a dictionary word, read as a word (not misaki's letter-spelled entries)
        let as_word = |p: String| p.matches(['ˈ', 'ˌ']).count() < stem.len();
        // exact gold entry, or an inflected 4+ letter word (HITS); "iss" is not "is" + s
        let gold = ((lex.knows)(&low) || (stem.len() >= 4 && known(&low))) && (lex.phonemes)(&low).map_or(true, as_word);
        gold || (stem.len() >= 4 && names.knows(&low))
    };
    if word {
        return Some(format!("{lead}{}{trail}", core.to_lowercase()));
    }
    let p = ra_names::spell_tight(stem);
    let p = if suf { format!("{p}{}", ra_names::possessive(&p)) } else { p };
    Some(format!("{lead}[{core}](/{p}/){trail}"))
}

/// The per-word path outside spans: hand override, Alex's caps acronym rule
/// (`caps_rule`, `headline` = the word sits in ALL-CAPS text), acronym/CamelCase
/// rule (`rule_override`), caps rule, page brackets.
fn word_path<'b>(g: &G2P, names: &Names, w: &'b str, headline: CapsMode) -> Cow<'b, str> {
    let known = |x: &str| lexicon_knows(&g.lexicon, x);
    let names_knows = |x: &str| names.knows(x);
    let lex_knows = |x: &str| lexicon_has(&g.lexicon, x);
    let lex_phonemes = |x: &str| lexicon_phonemes(&g.lexicon, x);
    let lex = Lex { knows: &lex_knows, phonemes: &lex_phonemes };
    let part = |p: &'b str| -> Cow<'b, str> {
        // hand list, then the acronym / CamelCase / compound rules, then the caps rule
        let caps = || caps_rule(p, names, &lex, &known, headline);
        if let Some(o) = hand_override(p, names).or_else(caps).or_else(|| rule_override(p, names, &lex)) {
            return Cow::Owned(o);
        }
        unbracket(ra_text::speakable_caps(p, &known, &names_knows))
    };
    if !w.contains('-') {
        return part(w);
    }
    let parts: Vec<Cow<str>> = w.split('-').map(part).collect();
    if parts.iter().all(|p| matches!(p, Cow::Borrowed(_))) {
        Cow::Borrowed(w)
    } else {
        Cow::Owned(parts.join("-"))
    }
}

/// `spoken` split into (leading punctuation, words, trailing punctuation).
fn split_edges(s: &str) -> (&str, &str, &str) {
    let start = s.find(char::is_alphanumeric).unwrap_or(s.len());
    let end = s.char_indices().rev().find(|(_, c)| c.is_alphanumeric()).map_or(start, |(i, c)| i + c.len_utf8());
    (s.get(..start).unwrap_or(""), s.get(start..end).unwrap_or(""), s.get(end..).unwrap_or(""))
}

/// The override that says a span on its first word: `{lead}[key](/P/){trail}`.
/// `key` is the word's letters and digits (one plain misaki token; "x" for a
/// symbol-only word such as `&`), so alignment sees one token group per word.
fn span_override(first: &str, lead: &str, ph: &str, trail: &str) -> String {
    let key: String = first.chars().filter(|c| c.is_alphanumeric()).collect();
    let key = if key.is_empty() { "x" } else { key.as_str() };
    let lead = unbracket(Cow::Borrowed(lead));
    let trail = unbracket(Cow::Borrowed(trail));
    format!("{lead}[{key}](/{ph}/){trail}")
}

/// Phonemes for each span's spoken words (`cores`), through the same G2P (hand list,
/// caps rule, misaki, names fallback) at depth 1, in ONE misaki call for the chunk:
/// misaki compiles a regex on every call (~0.1 ms), so a call per span broke the
/// 1 ms budget. Tokens are mapped back to their span by aligning on the words; a
/// punctuation token between two words of one span ("fourteenth, twenty") stays
/// with it. Pause-only tokens are dropped, runs of spaces collapse, and override
/// syntax characters never appear. `None` for a span with nothing speakable.
fn spans_phonemes(g: &G2P, names: &Names, cores: &[&str]) -> Vec<Option<String>> {
    let mut words: Vec<&str> = Vec::new();
    let mut span_of: Vec<usize> = Vec::new();
    for (k, c) in cores.iter().enumerate() {
        for w in c.split_whitespace() {
            words.push(w);
            span_of.push(k);
        }
    }
    let mut out = vec![String::new(); cores.len()];
    if words.is_empty() {
        return vec![None; cores.len()];
    }
    let fed = g2p_chunk(g, names, &words, 1).words;
    let plain: Vec<Cow<str>> = fed.iter().map(|w| plain_word(w)).collect();
    let plain: Vec<&str> = plain.iter().map(|w| w.as_ref()).collect();
    let text = fed.iter().map(|w| w.as_ref()).collect::<Vec<&str>>().join(" ");
    let Ok(toks) = run_g2p(g, &text) else { return vec![None; cores.len()] };
    let tt: Vec<&str> = toks.iter().map(|t| t.text.as_str()).collect();
    let owner = ra_timing::align(&plain, &tt);
    let span_at: Vec<Option<usize>> = owner.iter().map(|o| o.and_then(|w| span_of.get(w).copied())).collect();
    for (i, t) in toks.iter().enumerate() {
        let k = span_at.get(i).copied().flatten().or_else(|| {
            let before = span_at.get(..i)?.iter().rev().find_map(|x| *x);
            let after = span_at.get(i + 1..)?.iter().find_map(|x| *x);
            before.filter(|b| Some(*b) == after)
        });
        let (Some(o), Some(p)) = (k.and_then(|k| out.get_mut(k)), t.phonemes.as_deref().map(str::trim)) else {
            continue;
        };
        if p.is_empty() {
            continue;
        }
        if !o.is_empty() {
            o.push(' ');
        }
        o.extend(p.chars().filter(|c| !matches!(c, '[' | ']' | '(' | ')' | '/')));
    }
    out.into_iter()
        .map(|o| {
            let o = collapse_spaces(o.trim()).into_owned();
            (!o.trim().is_empty()).then_some(o)
        })
        .collect()
}

/// Runs of spaces become one space (misaki writes number words as "twenty   five").
fn collapse_spaces(p: &str) -> Cow<'_, str> {
    if !p.contains("  ") {
        return Cow::Borrowed(p);
    }
    let mut out = String::with_capacity(p.len());
    for c in p.chars() {
        if !(c == ' ' && out.ends_with(' ')) {
            out.push(c);
        }
    }
    Cow::Owned(out)
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
/// with phonemes feeds the model (punctuation shapes prosody), except:
/// - a hyphen inside one word ("mid-year"): misaki gives it a space phoneme, which
///   would add a long pause between the halves;
/// - a token with empty phonemes: the subtokens an override blanked (`[32B](/…/)`
///   -> "32" speaks, "B" is ""), whose whitespace alone would add a pause.
/// Runs of spaces inside one token's phonemes collapse to one ("twenty   five").
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
        .filter_map(|(_, ((_, p, ws), o))| {
            let p = (*p).filter(|p| !p.is_empty())?;
            Some((o.unwrap_or(NO_WORD), collapse_spaces(p).into_owned(), *ws))
        })
        .collect()
}

#[cfg(test)]
#[path = "ra_tts_bench.rs"]
mod bench;

#[cfg(test)]
#[path = "ra_glitch_hunt.rs"]
mod glitch_hunt;

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

    // ---------------------------------------------------------------- phase 3 pre-pass

    /// Pause guard: subtokens an override blanked ("$3.2B" -> "3.2" gets the phonemes,
    /// "B" comes back empty) add no whitespace of their own.
    #[test]
    fn empty_subtokens_of_one_word_add_no_pause() {
        let toks = [("cost", Some("kˈɔst"), true), ("32", Some("θɹˈi"), true), ("B", Some(""), true), ("today", Some("tədˈA"), false)];
        let owner = [Some(0), Some(1), Some(1), Some(2)];
        assert_eq!(joined(&token_phonemes(&toks, &owner)), "kˈɔst θɹˈi tədˈA");
        // an unowned blank ("." inside "U.S") is collapsed the same way
        let toks = [("U", Some("jˈu ˈɛs"), true), (".", Some(""), true), ("S", Some(""), true), (".", Some("."), true)];
        let owner = [Some(0), None, Some(0), None];
        assert_eq!(joined(&token_phonemes(&toks, &owner)), "jˈu ˈɛs . ");
    }

    #[test]
    fn runs_of_spaces_inside_one_token_collapse() {
        let toks = [("2025", Some("twˈɛnti   twˈɛnti  fˈIv"), true), ("-", Some(" "), true), ("x", Some("ˈɛks"), false)];
        let owner = [Some(0), None, Some(2)];
        let s = joined(&token_phonemes(&toks, &owner));
        assert!(s.starts_with("twˈɛnti twˈɛnti fˈIv "), "{s:?}");
        // a lone pause token keeps its single space
        assert_eq!(token_phonemes(&toks, &owner)[1].1, " ");
    }

    /// The chunk as synth feeds it: (word, phonemes, ws) per model token.
    fn chunk_ph(words: &[&str]) -> Vec<(usize, String, bool)> {
        prepare_chunk(test_g2p(), test_names(), words).expect("g2p").ph
    }

    fn word_ph(ph: &[(usize, String, bool)], w: usize) -> String {
        ph.iter().filter(|p| p.0 == w).map(|p| kokoro_phonemes(&p.1)).collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn money_span_has_no_gap_before_the_next_word() {
        let ph = chunk_ph(&["it", "cost", "$3.2B", "today"]);
        let s = joined(&ph);
        assert!(!s.contains("  "), "multi-space gap: {s:?}");
        let money = word_ph(&ph, 2);
        assert!(money.contains("bˈɪliən") && money.contains("dˈɑːlɚz"), "{money:?}");
        assert!(money.contains("pˈYnt"), "{money:?}");
    }

    #[test]
    fn multi_word_span_speaks_once_on_its_first_word() {
        let words = ["It", "cost", "$5", "billion", "in", "World", "War", "II."];
        let p = prepare_chunk(test_g2p(), test_names(), &words).expect("g2p");
        assert_eq!(p.inner, vec![3..4], "\"billion\" is inside the $5 span");
        let money = word_ph(&p.ph, 2);
        assert_eq!(money.matches("bˈɪliən").count(), 1, "{money:?}");
        assert!(money.contains("dˈɑːlɚz"), "{money:?}");
        assert!(word_ph(&p.ph, 3).is_empty(), "billion has no audio of its own");
        assert!(word_ph(&p.ph, 7).contains("tˈu"), "II -> two: {:?}", word_ph(&p.ph, 7));
        assert_eq!(p.ph.last().map(|t| t.1.as_str()), Some("."), "sentence period kept");
    }

    // Review Focus 3: one timing entry per word, zero length inside a span, never backwards.
    #[test]
    fn span_words_get_zero_length_entries_at_the_span_end() {
        let words = ["It", "cost", "$5", "billion", "in", "World", "War", "II."];
        let p = prepare_chunk(test_g2p(), test_names(), &words).expect("g2p");
        let s = joined(&p.ph);
        let v: HashMap<char, i64> = s.chars().filter(|c| *c != ' ').map(|c| (c, 1)).chain([(' ', 16)]).collect();
        let (ids, spans, covered) = build_ids(&p.ph, &v, words.len());
        let covered = cover_spans(covered, &p.inner);
        // fake duration model: 2 frames per id (plus the two pads)
        let dur = vec![2.0f32; ids.len() + 2];
        let n = dur.len() * 2 * 600;
        let t = chunk_word_times(&dur, spans, n, 1.0, covered);
        assert_eq!(t.len(), 8, "{t:?}");
        assert_eq!(t.iter().map(|w| w.word).collect::<Vec<_>>(), (0..8).collect::<Vec<_>>());
        let billion = &t[3];
        assert_eq!(billion.start, billion.end, "billion is zero length: {t:?}");
        assert_eq!(billion.start, t[2].end, "at the end of the $5 span");
        assert!(t[2].end > t[2].start, "$5 itself is spoken");
        assert!(t[7].end > t[7].start, "II is its own (one-word) span and is spoken");
        assert!(t.windows(2).all(|w| w[0].start <= w[1].start && w[0].end <= w[1].end && w[0].end <= w[1].start), "{t:?}");
    }

    #[test]
    fn covered_never_ends_inside_a_span() {
        // inner 3..5: word 2 speaks the span
        assert_eq!(cover_spans(2, &[3..5]), 2);
        assert_eq!(cover_spans(3, &[3..5]), 5);
        assert_eq!(cover_spans(4, &[3..5]), 5);
        assert_eq!(cover_spans(4, &[1..2, 3..6]), 6);
        assert_eq!(cover_spans(6, &[3..6]), 6);
        assert_eq!(cover_spans(2, &[]), 2);
    }

    #[test]
    fn every_word_gets_one_timing_entry() {
        // words with no audio (a lone dash) still get a zero-length entry
        let dur = vec![2.0f32; 8];
        let t = chunk_word_times(&dur, vec![(0, 0, 2), (2, 3, 6)], 8 * 2 * 600, 1.0, 4);
        assert_eq!(t.iter().map(|w| w.word).collect::<Vec<_>>(), vec![0, 1, 2, 3]);
        assert_eq!((t[1].start, t[1].end), (t[0].end, t[0].end));
        assert_eq!((t[3].start, t[3].end), (t[2].end, t[2].end));
        assert!(chunk_word_times(&dur, vec![], 100, 1.0, 0).is_empty());
    }

    /// A source word spoken as "" (junk block, footnote marker) goes through the real
    /// pre-pass, g2p, id build and timing and still gets exactly one timing entry.
    #[test]
    fn empty_source_word_keeps_one_zero_length_timing_entry() {
        let words = ["It", "works.[1]", "", "Advertisement", "", "", "Then", "this."];
        let fed = ["It", "works.", "", "", "", "", "Then", "this."];
        let p = prepare_chunk(test_g2p(), test_names(), &fed).expect("prepare");
        assert_eq!(p.fed.len(), words.len(), "one fed string per word");
        for w in [2, 3, 4, 5] {
            assert!(word_ph(&p.ph, w).trim().is_empty(), "word {w} must be silent");
        }
        assert!(!word_ph(&p.ph, 1).trim().is_empty() && !word_ph(&p.ph, 6).trim().is_empty());
        let vocab: HashMap<char, i64> = p.ph.iter().flat_map(|x| x.1.chars()).map(|c| (c, 5)).chain([(' ', 16)]).collect();
        let (ids, spans, covered) = build_ids(&p.ph, &vocab, words.len());
        assert!(!ids.is_empty());
        assert_eq!(covered, words.len());
        assert!(spans.iter().all(|sp| ![2, 3, 4, 5].contains(&sp.0)), "empty words own no ids");
        let dur = vec![2.0f32; ids.len()];
        let t = chunk_word_times(&dur, spans, ids.len() * 2 * 600, 1.0, covered);
        assert_eq!(t.iter().map(|w| w.word).collect::<Vec<_>>(), (0..words.len()).collect::<Vec<_>>());
        for w in [2usize, 3, 4, 5] {
            assert_eq!(t[w].start, t[w].end, "word {w} zero length");
            assert_eq!(t[w].start, t[1].end, "word {w} sits where the word before ends");
        }
        assert!(t.windows(2).all(|x| x[0].end <= x[1].start.max(x[0].end) && x[0].start <= x[1].start));
        // leading and all-empty chunks also keep one entry per word
        let p0 = prepare_chunk(test_g2p(), test_names(), &["", "", "Hi."]).expect("prepare");
        let (ids0, sp0, cov0) = build_ids(&p0.ph, &vocab, 3);
        let t0 = chunk_word_times(&vec![2.0; ids0.len()], sp0, ids0.len().max(1) * 1200, 1.0, cov0);
        assert_eq!(t0.iter().map(|w| w.word).collect::<Vec<_>>(), vec![0, 1, 2]);
        let pe = prepare_chunk(test_g2p(), test_names(), &["", ""]).expect("prepare");
        assert!(pe.ph.is_empty());
    }

    #[test]
    fn homographs_never_replace_override_phonemes() {
        let (_, mut toks) = test_g2p().g2p("I have read it").unwrap();
        let before: Vec<Option<String>> = toks.iter().map(|t| t.phonemes.clone()).collect();
        apply_homographs(&mut toks, &[true, true, true, true]);
        assert_eq!(toks.iter().map(|t| t.phonemes.clone()).collect::<Vec<_>>(), before, "overridden tokens kept");
        apply_homographs(&mut toks, &[false, false, false, false]);
        assert_eq!(toks[2].phonemes.as_deref(), Some("ɹˈɛd"));
    }

    #[test]
    fn unspeakable_chunk_still_times_every_word() {
        let a = empty_audio(Device::Cpu, 3);
        assert_eq!(a.covered, 3);
        assert_eq!(a.words.iter().map(|w| (w.word, w.start, w.end)).collect::<Vec<_>>(), vec![(0, 0, 0), (1, 0, 0), (2, 0, 0)]);
        // reachable: emoji only give no model ids
        let p = prepare_chunk(test_g2p(), test_names(), &["😀", "🙂"]).expect("g2p");
        let v: HashMap<char, i64> = [('a', 1), (' ', 16)].into_iter().collect();
        assert!(build_ids(&p.ph, &v, 2).0.iter().all(|&i| i == 16) || build_ids(&p.ph, &v, 2).0.is_empty());
    }

    #[test]
    fn homograph_is_applied() {
        let ph = chunk_ph(&["I", "have", "read", "it"]);
        assert_eq!(word_ph(&ph, 2), "ɹˈɛd");
        let ph = chunk_ph(&["I", "will", "read", "it"]);
        assert_eq!(word_ph(&ph, 2), "ɹˈiːd");
    }

    // Review Focus 4: a sentence-final initialism keeps the sentence's own period.
    #[test]
    fn initialism_at_chunk_end_keeps_the_final_period() {
        let p = prepare_chunk(test_g2p(), test_names(), &["moved", "to", "the", "U.S."]).expect("g2p");
        assert_eq!(p.ph.last().map(|t| (t.0, t.1.as_str())), Some((NO_WORD, ".")), "{:?}", p.ph);
        let us = word_ph(&p.ph, 3);
        assert!(us.contains("jˈu") && us.contains("ˈɛs"), "U S: {us:?}");
        assert!(!joined(&p.ph).contains("  "), "{:?}", joined(&p.ph));
    }

    #[test]
    fn lone_symbols_are_never_silent() {
        let words = ["pay", "$", "now", ".", "and", "§230", "&", "5%"];
        let p = prepare_chunk(test_g2p(), test_names(), &words).expect("g2p");
        for w in [1, 5, 6, 7] {
            assert!(!word_ph(&p.ph, w).trim().is_empty(), "{} silent: {:?}", words[w], p.ph);
        }
    }

    /// Chunk boundaries: g2p_words sees the chunk only, so the chunk's last word is
    /// read as a sentence end (ra_text::chunk cuts at sentence ends, clause marks or,
    /// rarely, a hard limit). A hard cut mid-sentence adds a period after an
    /// initialism and leaves a title unexpanded. Documented limitation.
    #[test]
    fn chunk_end_is_read_as_a_sentence_end() {
        let fed = g2p_words(test_g2p(), test_names(), &["the", "U.S."]);
        assert!(plain_word(&fed[1]).ends_with('.'), "{fed:?}");
        let fed = g2p_words(test_g2p(), test_names(), &["met", "Sen."]);
        assert_eq!(fed[1], "Sen.", "no next word: the title stays as written");
    }

    /// Spec §3 acronym and CamelCase rows end to end (Task 3's rules through the
    /// Task 5 pre-pass path).
    #[test]
    fn acronyms_and_camel_case_end_to_end() {
        let words = ["The", "SEC's", "AWS", "GPUs", "and", "TechReview", "rose."];
        let ph = chunk_ph(&words);
        let marks = |w: usize| word_ph(&ph, w).matches(['ˈ', 'ˌ']).count();
        assert!(marks(1) >= 3, "SEC's spelled: {}", word_ph(&ph, 1));
        assert!(marks(2) >= 3, "AWS spelled: {}", word_ph(&ph, 2));
        assert!(marks(3) >= 3, "GPUs spelled: {}", word_ph(&ph, 3));
        assert!(marks(5) <= 3, "TechReview as two words, not letters: {}", word_ph(&ph, 5));
    }

    /// Both phases together: a span (`$3.2B`), a curly apostrophe fixed by `prepare`,
    /// a lexicon brand, a CamelCase brand and suffixed acronyms in one chunk.
    #[test]
    fn spans_and_acronyms_together() {
        let words = ["Microsoft\u{2019}s", "$3.2B", "deal", "with", "OpenAI", "and", "the", "SEC's", "GPUs", "in", "2025"];
        let p = prepare_chunk(test_g2p(), test_names(), &words).expect("g2p");
        // one timing entry per word, in order
        let s = joined(&p.ph);
        let v: HashMap<char, i64> = s.chars().filter(|c| *c != ' ').map(|c| (c, 1)).chain([(' ', 16)]).collect();
        let (ids, spans, covered) = build_ids(&p.ph, &v, words.len());
        let covered = cover_spans(covered, &p.inner);
        let dur = vec![2.0f32; ids.len() + 2];
        let t = chunk_word_times(&dur, spans, dur.len() * 2 * 600, 1.0, covered);
        assert_eq!(t.iter().map(|w| w.word).collect::<Vec<_>>(), (0..words.len()).collect::<Vec<_>>(), "{t:?}");
        for (w, word) in words.iter().enumerate() {
            assert!(!word_ph(&p.ph, w).trim().is_empty(), "{word} silent: {:?}", p.ph);
        }
        // said, not spelled
        assert_eq!(word_ph(&p.ph, 0), format!("{}s", say("Microsoft")), "Microsoft's");
        assert_eq!(word_ph(&p.ph, 4), say("OpenAI"), "OpenAI");
        // letters spelled
        assert_eq!(word_ph(&p.ph, 7), format!("{}z", tight("SEC")), "SEC's");
        assert_eq!(word_ph(&p.ph, 8), format!("{}z", tight("GPU")), "GPUs");
        assert!(word_ph(&p.ph, 1).contains("bˈɪl"), "$3.2B: {}", word_ph(&p.ph, 1));
    }

    /// The real CPU model on a chunk with a multi-word span: one entry per word,
    /// "billion" zero length at the $5 span's end, never backwards.
    /// cargo test --release span_synth_real -- --ignored --nocapture
    #[test]
    #[ignore]
    fn span_synth_real() {
        let e = Engine::new(dirs::data_dir().unwrap().join("ALAN Echo"));
        e.preload("af_heart", "cpu");
        let words = ["It", "cost", "$5", "billion", "in", "World", "War", "II."];
        let a = e.synth(&words, 2.2).unwrap();
        println!("{:?}", a.words);
        assert_eq!(a.covered, 8);
        assert_eq!(a.words.iter().map(|w| w.word).collect::<Vec<_>>(), (0..8).collect::<Vec<_>>());
        assert_eq!(a.words[3].start, a.words[3].end);
        assert_eq!(a.words[3].start, a.words[2].end);
        assert!(a.words.windows(2).all(|w| w[0].start <= w[1].start && w[0].end <= w[1].start));
        println!("wav: {}", write_wav_as(&a.pcm, "ra_span_sample.wav").display());
    }

    /// Pre-pass budget (spec §5): spans + phonemising the spoken text adds <= 1 ms per
    /// 30-word chunk. cargo test --release prepass_cost -- --ignored --nocapture
    #[test]
    #[ignore]
    fn prepass_cost_per_chunk() {
        let text = "On March 3, 2025 the U.S. Senate passed a $3.2B bill, up 8.6% from 2024, \
                    after Sen. Warren spoke at 7:30pm in Washington about World War II and the 1990s. \
                    Shares of Microsoft rose 2.5% to $480 while about 1,500 workers in St. Louis \
                    read the 21st report on the company’s 5 km site at No. 10 Main St. today.";
        let words: Vec<&str> = text.split_whitespace().collect();
        let chunks: Vec<&[&str]> = words.chunks(30).collect();
        let (g, names) = (test_g2p(), test_names());
        let n = 300;
        let time = |depth: u8| {
            let t = Instant::now();
            for _ in 0..n {
                for c in &chunks {
                    std::hint::black_box(g2p_chunk(g, names, c, depth));
                }
            }
            t.elapsed().as_secs_f64() * 1000.0 / (n * chunks.len()) as f64
        };
        let _ = (time(0), time(1)); // warm
        let with = time(0);
        let without = time(1);
        let spans_only = {
            let t = Instant::now();
            for _ in 0..n {
                for c in &chunks {
                    std::hint::black_box(crate::ra_norm::spans(c));
                }
            }
            t.elapsed().as_secs_f64() * 1000.0 / (n * chunks.len()) as f64
        };
        println!(
            "g2p_words per 30-word chunk: {with:.3} ms with pre-pass, {without:.3} ms without; \
             pre-pass adds {:.3} ms (spans alone {spans_only:.4} ms)",
            with - without
        );
        assert!(with - without <= 1.0, "pre-pass adds {:.3} ms per chunk", with - without);
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
        let words = ["Washington", "Pelosi", "the", "NATO", "mid-year", "iPhone", "McDonald's", "said."];
        let fed = g2p_words(test_g2p(), test_names(), &words);
        for (w, f) in words.iter().zip(&fed) {
            assert!(!f.contains('['), "{w} -> {f}");
            assert!(matches!(f, Cow::Borrowed(_)), "{w} -> {f}");
        }
        // a caps acronym in mixed text is spelled explicitly (final review, Alex's rule)
        let fed = g2p_words(test_g2p(), test_names(), &["the", "FBI", "said."]);
        assert_eq!(fed[1], format!("[FBI](/{}/)", crate::ra_names::spell_tight("FBI")));
    }

    #[test]
    fn page_brackets_are_never_override_syntax() {
        let g = test_g2p();
        let words = ["[1](2)", "[Xi", "x](/ʒʒʒ/)", "](", "[", "]", "🙂", "東京", "a[b](/zz/)c", "Xi](/x/)", "[[Xi]]", "word"];
        let fed = g2p_words(g, test_names(), &words);
        for (w, f) in words.iter().zip(&fed) {
            // any bracket left is an override we wrote: stripping ours leaves none
            if !w.contains("Xi") {
                assert!(!plain_word(f).contains(['[', ']']), "{w:?} -> {f:?}");
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
        // a headline (2+ other caps words): acronyms spelled explicitly, NATO/NASA words
        let sp = |w: &str| format!("[{w}](/{}/)", crate::ra_names::spell_tight(w));
        let want = [sp("FBI"), sp("GDP"), sp("CEO"), "NATO".into(), "NASA".into(), sp("US"), sp("IT"), sp("WHO"), "the,".into(), "talks.".into()];
        assert_eq!(fed, want);
    }

    fn caps_rewrite_words() -> Vec<&'static str> {
        ["THE", "PRESIDENT", "SAID", "FBI", "TALKS", "GDP", "MID-YEAR", "iPhone", "42"].iter().cycle().take(30).copied().collect()
    }

    /// Non-timing check of the caps rewrite on the same 30-word chunk the perf test times.
    #[test]
    fn caps_rewrite_correct() {
        let g = test_g2p();
        let words = caps_rewrite_words();
        let fed: Vec<String> = g2p_words(g, test_names(), &words).into_iter().map(|c| c.into_owned()).collect();
        assert_eq!(fed.len(), words.len());
        let sp = |w: &str| format!("[{w}](/{}/)", crate::ra_names::spell_tight(w));
        for (w, f) in words.iter().zip(&fed) {
            match *w {
                "FBI" | "GDP" => assert_eq!(*f, sp(w), "{w} spelled out"),
                "THE" | "SAID" | "TALKS" | "PRESIDENT" => assert_eq!(*f, w.to_lowercase(), "{w} folded"),
                _ => {}
            }
        }
    }

    /// Caps-rewrite cost per 30-word chunk (release only; wall-clock is meaningless in a
    /// loaded debug run). Budget matches the pre-pass: <= 1 ms per chunk.
    /// cargo test --release caps_rewrite -- --ignored --nocapture
    #[test]
    #[ignore]
    fn caps_rewrite_is_cheap() {
        let g = test_g2p();
        let words = caps_rewrite_words();
        let n = 1000;
        let _ = g2p_words(g, test_names(), &words); // warm
        let t = Instant::now();
        for _ in 0..n {
            std::hint::black_box(g2p_words(g, test_names(), std::hint::black_box(&words)));
        }
        let per = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
        println!("caps rewrite: {per:.4} ms per 30-word chunk");
        assert!(per <= 1.0, "{per} ms per chunk");
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

    // ---- final review: acronym spelling (Alex's board feedback) ----

    /// A caps token in mixed-case text, through the real pipeline.
    fn in_text(w: &str) -> String {
        spoken(&["we", "saw", w, "today."])[2].clone()
    }

    fn spelled_with_suffix(w: &str) -> String {
        let (stem, suf) = match w.strip_suffix("'s") {
            Some(s) => (s, true),
            None => match w.strip_suffix('s') {
                Some(s) if s.chars().all(|c| c.is_ascii_uppercase()) && s.len() >= 2 => (s, true),
                _ => (w, false),
            },
        };
        let p = crate::ra_names::spell_tight(stem);
        let p = if suf { format!("{p}{}", crate::ra_names::possessive(&p)) } else { p };
        kokoro_phonemes(&p)
    }

    /// Alex's SPELLED table plus ~150 common acronyms (government, finance, tech,
    /// medicine, sports, media, schools): letter by letter in mixed-case text.
    const SPELL_IN_TEXT: &[&str] = &[
        // Alex's list
        "ISS", "IRS", "ICU", "IPO", "EPA", "FDA", "FCC", "GOP", "DOJ", "NIH", "NSA", "CIA", "ATM", "SEC", "ETF", "CEO",
        "CFO", "GDP", "CPI", "NFL", "NBA", "MLB", "UFC", "IBM", "HP", "AMD", "TSMC", "BBC", "CNN", "NPR", "ESPN", "MIT",
        "UCLA", "NYU", "LLC", "IOU", "ISP", "VPN", "USB", "HDMI",
        // government
        "FBI", "FTC", "DOD", "DHS", "DOE", "HHS", "USDA", "CDC", "FAA", "TSA", "ATF", "DEA", "NTSB", "NLRB", "SBA", "SSA",
        "VA", "GAO", "CBO", "OMB", "USPS", "CBP", "DNC", "RNC", "UAE", "USA", "NGO", "UN", "EU", "UK", "US", "IT", "WHO",
        "IMF", "WTO", "ECB", "BOJ", "PBOC", "UNSC", "IAEA", "OECD", "MI", "KGB", "SNB",
        // finance
        "COO", "CTO", "PPI", "PCE", "FOMC", "ROI", "EPS", "IRA", "IRAs", "LLP", "APR", "APY", "FDIC", "NYSE", "ESG",
        "EBIT", "YTD", "YOY", "ARR", "MRR", "CAGR", "KPI", "ETFs", "IPOs", "CEOs", "SVB", "FTX", "DJIA", "HSBC", "UBS",
        "JPM", "CFA", "CPA", "SPX", "QQQ",
        // tech
        "CPU", "GPU", "SSD", "HDD", "LCD", "DNS", "URL", "HTML", "CSS", "API", "SDK", "IDE", "AWS", "GCP", "PDF", "SQL",
        "XML", "NFT", "PC", "TV", "AI", "LLM", "GPT", "IoT", "HTTP", "HTTPS", "SSH", "VR", "AR", "OS", "UI", "UX",
        "CRM", "ERP", "DVD", "CD", "LTE", "NFC", "RFID", "IP", "TCP", "UDP", "USBs", "GPUs", "IEC",
        "ISO", "NXP", "HTC", "LG", "BYD", "BMW", "GM", "DHL", "BP",
        // medicine
        "ER", "MRI", "CT", "EKG", "ECG", "DNA", "RNA", "HIV", "ADHD", "PTSD", "OCD", "BMI", "CPR", "EMT", "ENT", "MD",
        "RN", "NHS", "ICUs", "STD", "UTI", "COPD", "ALS",
        // sports
        "NHL", "MLS", "NCAA", "WWE", "PGA", "LPGA", "ATP", "WTA", "NBA's", "MVP",
        // media and schools
        "PBS", "CBS", "NBC", "ABC", "MSNBC", "HBO", "AP", "NYT", "WSJ", "USC", "LSU", "UCSD", "NYU's",
        // everyday
        "FAQ", "DIY", "ETA", "TBD", "RSVP", "OMG", "FYI", "PS", "TBH", "BRB", "DM", "SUV", "RV", "ID",
    ];

    /// Acronyms said as words (`ra_names::PRONOUNCED`).
    const WORD_IN_TEXT: &[&str] = &[
        "NASA", "NATO", "OPEC", "UNESCO", "UNICEF", "FEMA", "AIDS", "COVID", "SCOTUS", "POTUS", "FLOTUS", "NAFTA",
        "ASEAN", "GIF", "JPEG", "LASER", "RADAR", "SONAR", "SWAT", "ASAP", "NIMBY", "YOLO", "FOMO", "CAPTCHA", "WASP",
        "SARS", "MERS", "ISIS", "DARPA", "NOAA", "OSHA", "HUD", "GAAP", "ICE", "DOGE", "SPAC", "SPACs", "NASA's",
        "CERN", "FIFA", "UEFA", "IKEA", "REIT", "JSON", "AWOL", "RAM", "LAN", "SIM", "MRSA", "HIPAA", "ASCII", "GUI",
        "SCUBA", "TASER", "FICO", "META", "EDGAR", "POSIX", "NAND", "MIPS", "NIST", "CHIP", "PRISM", "ATLAS", "ENSO",
        "XBOX", "BIOS", "ARPA", "PIN", "ARM", "BERT", "GNOME", "BLOOM", "FRED", "SAKS", "VESA", "SUSE", "LEO", "MIRI", "MEMS", "FOSS", "PEP", "MAC",
    ];

    #[test]
    fn acronyms_in_mixed_text_are_spelled_unless_pronounced() {
        let mut bad = Vec::new();
        for w in SPELL_IN_TEXT {
            let got = in_text(w);
            if got != spelled_with_suffix(w) {
                bad.push(format!("SPELL {w}: {got} (want {})", spelled_with_suffix(w)));
            }
        }
        for w in WORD_IN_TEXT {
            let got = in_text(w);
            if got == spelled_with_suffix(w) || got.trim().is_empty() {
                bad.push(format!("WORD {w}: {got}"));
            }
        }
        assert!(bad.is_empty(), "{} wrong:\n{}", bad.len(), bad.join("\n"));
        // hand-list fixes for pronounced acronyms the G2P got wrong
        assert_eq!(in_text("JSON"), "ʤˈAsən");
        assert_eq!(in_text("REIT"), "ɹˈit");
        assert_eq!(in_text("MRSA"), "mˈɜɹsə");
        // pronounced acronyms never reach the GRU: hand entries
        assert_eq!(in_text("ENSO"), "ˈɛnsO");
        assert_eq!(in_text("VESA"), "vˈisə");
        assert_eq!(in_text("SUSE"), "sˈuzə");
        // EFTA and PAHO are words, also inside a slash pair
        assert_eq!(in_text("EFTA"), "ˈɛftə");
        assert!(!in_text("PAHO").contains('ˌ'), "{}", in_text("PAHO"));
    }

    /// Every 2-5 letter ALL-CAPS token of the frozen glitch corpus, read in a
    /// lowercase sentence: spelled, unless it is a pronounced acronym, an emphasis
    /// word, a Roman numeral or a hand-list name.
    #[test]
    fn corpus_caps_tokens_follow_the_rule() {
        const CORPUS: &str = "US CEO FTX AI NATO OPEC NASA USB EU AWS IPO SEC AMD WHO UK TSMC SVB NYSE LLM API IMF IBM UN \
            GPU LHC GDP PC GPT ETF QE FDIC HP CPU SSD UTC DNA RNA ENSO CCP BJP IDF FBI TV VR IEEE CERN EVA BBC OS NSA FTC \
            LLC UPS FOMC ACA COO CFO ARM PCI CDMA NT RF NIST EBS PEP ONNX AD HIV AIDS ASML EMI OEM HTC DVD AMA VPN PBS DJIA \
            GNU NFL FTT PT SQL SAP DRAM HD APU NXP GB LCD BERT IDE XP GNOME KDE SSP MCS EPS SLA CIA CRI HTML ABC HDMI PS XBOX \
            ISS COTS BLOOM FCC TNT HLS SLS BMI ER ECJ ECB ISAF MIT ROC SARS PDT CVS AAPL NVDA AMZN META TSLA JPM XOM LLY AVGO \
            NFLX KO INTC QCOM PLTR CRM ORCL QQQ VOO TLT GLD ETH RAM LED OCD DRM PRISM BFR PVC RIVA GTC CAC RTX LPO BC EEA LP \
            GP SME EFTA NIS FRED QT CCI ECI SPDR INX SPX CFA CCAF ECDSA TTI BHE BYD CF USPS ASR XML MGM OAIC UFC RT MRC AMC USA \
            GSCI SAKS CDO PNC HPS GIP CCS MSME RJH GSE DPO EDGAR USD SNB EF DFS RVI SFV DEF VMLY IAL VESA ATIC SIMD SSE FPGA \
            TDP QTL UMB LTE JASM FFC FFN MEMS METI NAND ABB WCP ATT RIM NLP UNK NSO MDCT LPC AOSP FOSS ADT AOT LTS MIPS CNET \
            OSR SE CE ELKS LMI DVB POSIX LAN WPA MAC PHY BLE SIG CRC ARQ EDR AVDTP HDP CSA BLER NAS URLLC SK KT LG NR ITU IAM \
            ISO RJE CVE APN EDF CLI ASD CSI SUSE JDK JIT IEC EPR PD DAC TB UI PDIF CRS IAG NGO LEO MCP ABKCO GGUF MCDR WMAP GTO \
            ICRP FGS MIRI PCR UNSC UTR ORF MHC SPS GJ MJ GLP ROS AMPK CHIP APOE NMDA QCD ATLAS CMS CP SOI SST EEAS EEC ESCB NCB \
            NPT ICHR CAA NHS CFP ICC WCK ANERA PRC FC JK OSESP RTS IHR TCM ICT PAHO AMRO FDMD LNG ZWO BMW GM BP BHP DHL LVMH \
            IKEA MSFT GOOGL UNH MA WMT SPY IWM BTC JD IRS FDA CDC EPA NIH DOJ DHS DOD FAA NLRB CPI PCE ROI PPI SPAC REIT CTO \
            WTO BOJ PBOC ML SDK JSON CSS HTTP URL HTTPS PDF OLED DAO NFT DEI ESG HR PR FAQ FYI ASAP DIY ETA TBD RSVP AWOL NCAA \
            NBA MLB MLS NHL FIFA UEFA CNN NPR CBS NBC MSNBC ESPN HBO AARP ACLU NAACP NRA MRI ICU CT ADHD PTSD AOC MB EST PM AM \
            UA JFK FTSE EV MMLU XL ISBN DOI NY OMG LOL BRB TBH OK";
        let n = test_names();
        let roman = |w: &str| w.chars().all(|c| matches!(c, 'I' | 'V' | 'X'));
        let mut bad = Vec::new();
        for w in CORPUS.split_whitespace() {
            // round 1: 4-5 letter caps that are dictionary words (names, CNET) read as words
            const DICT: &[&str] = &["RIVA", "CNET", "ELKS", "AMRO"];
            let word = crate::ra_names::PRONOUNCED.contains(&w) || crate::ra_names::EMPHASIS.contains(&w) || DICT.contains(&w);
            if word || roman(w) || n.hand(w).is_some() {
                continue;
            }
            let got = in_text(w);
            if got != spelled_with_suffix(w) {
                bad.push(format!("{w}: {got}"));
            }
        }
        assert!(bad.is_empty(), "{} not spelled:\n{}", bad.len(), bad.join("\n"));
    }

    #[test]
    fn headline_caps_read_words_and_spell_the_rest() {
        let s = spoken(&["THE", "NEW", "LAW", "HITS", "BUS", "FIRMS"]);
        let l = spoken(&["the", "new", "law", "hits", "bus", "firms"]);
        assert_eq!(s, l, "dictionary words in a headline read as words");
        let s = spoken(&["ISS", "CREW", "RETURNS", "HOME"]);
        assert_eq!(s[0], spelled_with_suffix("ISS"), "{s:?}");
        assert_eq!(s[1], spoken(&["the", "crew"])[1]);
        // never a 2-4 letter caps guess: unknown short caps are spelled in a headline
        let s = spoken(&["FBI", "AND", "CIA", "PROBE", "XQZT"]);
        assert_eq!(s[0], spelled_with_suffix("FBI"));
        assert_eq!(s[2], spelled_with_suffix("CIA"));
        assert_eq!(s[4], spelled_with_suffix("XQZT"));
        // KEEP_CAPS words stay spelled in a headline too
        let s = spoken(&["US", "AND", "UK", "SIGN", "DEAL"]);
        assert_eq!(s[0], spelled_with_suffix("US"));
        assert_eq!(s[2], spelled_with_suffix("UK"));
        // pronounced acronyms stay words
        let s = spoken(&["NASA", "AND", "NATO", "MEET"]);
        assert_ne!(s[0], spelled_with_suffix("NASA"));
        assert_ne!(s[2], spelled_with_suffix("NATO"));
    }

    /// Round 1: headline mode needs 60%+ caps words and 3+ of them; in mixed text a
    /// 2-3 letter caps token is spelled (unless EMPHASIS / PRONOUNCED) and a 4-5
    /// letter one that is a common dictionary word reads as that word.
    #[test]
    fn round1_caps_context_table() {
        // (sentence, tokens that must be spelled, tokens that must read as words)
        let cases: &[(&str, &[&str], &[&str])] = &[
            // dense acronym sentences: never a headline, never "sat" / "act" / "era"
            ("we took the SAT, ACT and AP.", &["SAT", "ACT", "AP"], &[]),
            ("took the SAT, ACT and AP", &["SAT", "ACT", "AP"], &[]),
            ("the ER, ICU and VA", &["ER", "ICU", "VA"], &[]),
            ("ETA and ERA", &["ETA", "ERA"], &[]),
            ("IRAs, ISS's, CEO and CFO", &["IRAs", "ISS's", "CEO", "CFO"], &[]),
            ("an AD for the MA program", &["AD", "MA"], &[]),
            // emphasis caps in normal text read as words
            ("this is NOT okay", &[], &["NOT"]),
            ("it was HUGE", &[], &["HUGE"]),
            ("a BIG deal", &[], &["BIG"]),
            ("I'm SORRY", &[], &["SORRY"]),
            ("the NEWS today", &[], &["NEWS"]),
            ("follow the MONEY", &[], &["MONEY"]),
            ("we are DONE", &[], &["DONE"]),
            // real headlines: dictionary words are words, acronyms spelled
            ("BREAKING: SENATE PASSES NEW TAX LAW", &[], &["BREAKING:", "SENATE", "PASSES", "NEW", "TAX", "LAW"]),
            ("FDA APPROVES NEW DRUG", &["FDA"], &["APPROVES", "NEW", "DRUG"]),
        ];
        let mut bad = Vec::new();
        for (text, spell, word) in cases {
            let words: Vec<&str> = text.split(' ').collect();
            let lower: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
            let lower: Vec<&str> = lower.iter().map(|w| w.as_str()).collect();
            let got = spoken(&words);
            let low = spoken(&lower);
            for (i, w) in words.iter().enumerate() {
                let core = w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'');
                let core = core.trim_end_matches('\'');
                if spell.contains(&core) && got[i] != spelled_with_suffix(core) {
                    bad.push(format!("{text}: {w} should be spelled, got {}", got[i]));
                }
                if word.contains(w) && got[i] != low[i] {
                    bad.push(format!("{text}: {w} should read as a word, got {} (want {})", got[i], low[i]));
                }
            }
        }
        // emphasis caps Alex heard spelled
        for w in [
            "OKAY", "SORRY", "FROM", "HARD", "SURE", "HATE", "DONE", "FAKE", "EVEN", "BACK", "NEXT", "LATE", "CRAZY", "AWFUL",
            "MONEY", "NEWS", "BUS", "HUGS",
        ] {
            let low = w.to_lowercase();
            let (got, want) = (in_text(w), spoken(&["we", "saw", &low, "today."])[2].clone());
            if got != want {
                bad.push(format!("emphasis {w}: {got} (want {want})"));
            }
        }
        assert!(bad.is_empty(), "{} wrong:\n{}", bad.len(), bad.join("\n"));
    }

    /// Round 2: headline mode needs 80%+ caps words and 3+ of them; 100% caps is
    /// its own mode (the ambiguous acronyms SAT / ERA / AD read as words only there).
    #[test]
    fn round2_headline_threshold_is_80_percent() {
        let c = |ws: &[&str]| headline_flags(&ws.iter().map(|w| Cow::Borrowed(*w)).collect::<Vec<_>>());
        // 3 of 5 (60%) is mixed text now
        assert!(c(&["THE", "NEW", "LAW", "passed", "today."]).iter().all(|m| *m == CapsMode::Mixed));
        assert!(c(&["the", "ER,", "ICU", "and", "VA"]).iter().all(|m| *m == CapsMode::Mixed));
        // 4 of 5 (80%) is a headline, not all caps
        assert!(c(&["FED", "CUTS", "RATES", "AGAIN", "today."]).iter().all(|m| *m == CapsMode::Headline));
        // 2 caps words is never a headline
        assert!(c(&["FED", "CUTS"]).iter().all(|m| *m == CapsMode::Mixed));
        // every word caps
        assert!(c(&["SHE", "SAT", "DOWN."]).iter().all(|m| *m == CapsMode::AllCaps));
        // per sentence
        let m = c(&["SHE", "SAT", "DOWN.", "Then", "she", "left."]);
        assert_eq!(m, vec![CapsMode::AllCaps, CapsMode::AllCaps, CapsMode::AllCaps, CapsMode::Mixed, CapsMode::Mixed, CapsMode::Mixed]);
    }

    #[test]
    fn round2_ambiguous_acronyms_are_words_only_in_all_caps() {
        let low = |ws: &[&str]| spoken(&ws.iter().map(|w| w.to_lowercase()).collect::<Vec<_>>().iter().map(|s| s.as_str()).collect::<Vec<_>>());
        for ws in [&["SHE", "SAT", "DOWN"][..], &["THE", "ERA", "OF", "AI", "ENDS"][..], &["THE", "AD", "WAS", "BANNED"][..]] {
            let got = spoken(ws);
            let want = low(ws);
            let i = ws.iter().position(|w| ["SAT", "ERA", "AD"].contains(w)).unwrap();
            assert_eq!(got[i], want[i], "{ws:?}");
        }
        // mixed text and an 80% headline: spelled
        assert_eq!(spoken(&["took", "the", "SAT"])[2], spelled_with_suffix("SAT"));
        assert_eq!(spoken(&["the", "ERA", "rose"])[1], spelled_with_suffix("ERA"));
        assert_eq!(spoken(&["SHE", "SAT", "THE", "EXAM", "today."])[1], spelled_with_suffix("SAT"));
        // the always-spelled list stays spelled even in all caps
        for (ws, i) in [(&["THE", "ER", "IS", "FULL"][..], 1), (&["ETA", "IS", "NOON"][..], 0), (&["AP", "CALLS", "RACE"][..], 0), (&["MY", "IRA", "GREW"][..], 1)] {
            assert_eq!(spoken(ws)[i], spelled_with_suffix(ws[i]), "{ws:?}");
        }
    }

    #[test]
    fn emphasis_caps_in_text_are_words() {
        for w in ["NOT", "NEVER", "ALL", "ONLY", "VERY", "MUST", "FREE", "NO", "YES", "THE"] {
            let got = spoken(&["you", "do", w, "know"])[2].clone();
            let low = w.to_lowercase();
            let want = spoken(&["you", "do", &low, "know"])[2].clone();
            assert_eq!(got, want, "{w}");
        }
        // one other caps word nearby is still mixed text: DO NOT stays words
        let s = spoken(&["please", "DO", "NOT", "touch"]);
        assert_eq!(s[1..3], spoken(&["please", "do", "not", "touch"])[1..3]);
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

    /// Round 2 gapless: a chunk's lead-in and final pause are trimmed to a short gap
    /// scaled by speed; word times shift with it, one per word.
    #[test]
    fn chunk_silence_is_trimmed_to_a_short_gap() {
        let sr = ra_timing::SR as usize;
        let tone = |n: usize, a: f32| (0..n).map(|i| if i % 2 == 0 { a } else { -a }).collect::<Vec<f32>>();
        let mut pcm = vec![0.0f32; sr / 2]; // 500 ms lead-in
        pcm.extend(tone(sr, 0.2)); // 1 s of sound
        pcm.extend(vec![0.0f32; sr / 2]); // 500 ms final pause
        let mut words = vec![
            WordTime { word: 0, start: 0, end: sr as u64 },
            WordTime { word: 1, start: sr as u64, end: (2 * sr) as u64 },
        ];
        let out = trim_silence(pcm.clone(), &mut words, 2.0, Tail::Sentence);
        let lead = (KEEP_LEAD_MIN_MS / 1000.0 * sr as f32) as usize; // 40/2 < 30: the floor
        let trail = (KEEP_TRAIL_SENTENCE_MS / 1000.0 * sr as f32 / 2.0) as usize;
        assert_eq!(out.len(), lead + sr + trail);
        assert!(out[..lead].iter().all(|x| *x == 0.0) && out[lead] != 0.0, "sound is never cut");
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].start, 0);
        assert_eq!(words[1].start, (sr / 2 + lead) as u64, "shifted by the trimmed lead-in");
        assert!(words.iter().all(|w| w.end <= out.len() as u64 && w.start <= w.end));
        // faster speed keeps less trailing silence, never less than 30 ms lead
        let mut w2 = Vec::new();
        let fast = trim_silence(pcm.clone(), &mut w2, 3.5, Tail::Sentence);
        assert!(fast.len() < out.len());
        assert!(fast[..lead].iter().all(|x| *x == 0.0) && fast[lead] != 0.0);
        // all silence, or no silence: unchanged
        let mut w3 = Vec::new();
        assert_eq!(trim_silence(vec![0.0; 4800], &mut w3, 2.2, Tail::Sentence).len(), 4800);
        assert_eq!(trim_silence(tone(4800, 0.2), &mut w3, 2.2, Tail::Sentence).len(), 4800);
        assert!(trim_silence(Vec::new(), &mut w3, 2.2, Tail::Sentence).is_empty());
    }

    /// Review fix: a quiet "h"-like onset (-45 dBFS) before a loud vowel survives the
    /// trim; silence is judged against the chunk's own peak.
    #[test]
    fn quiet_onset_before_a_loud_vowel_survives_the_trim() {
        let sr = ra_timing::SR as usize;
        let q = 10f32.powf(-45.0 / 20.0); // -45 dBFS
        let mut pcm = vec![0.0f32; sr / 2];
        let onset_at = pcm.len();
        // 80 ms breathy onset: noise-like, -45 dBFS peak
        pcm.extend((0..sr * 80 / 1000).map(|i| if (i * 7919) % 3 == 0 { q } else { -q * 0.5 }));
        pcm.extend((0..sr / 2).map(|i| if i % 2 == 0 { 0.9 } else { -0.9 })); // loud vowel
        pcm.extend(vec![0.0f32; sr / 2]);
        for rate in [1.0f32, 2.2, 3.5] {
            let mut w = Vec::new();
            let out = trim_silence(pcm.clone(), &mut w, rate, Tail::Sentence);
            let cut = pcm.len() - out.len();
            let lead_cut = pcm[..].iter().position(|x| *x != 0.0).unwrap(); // = onset_at
            assert_eq!(lead_cut, onset_at);
            // the kept audio starts before the onset, with at least 30 ms of lead
            let first_sound = out.iter().position(|x| *x != 0.0).unwrap();
            assert!(first_sound >= sr * 30 / 1000 - 1, "{rate}x: lead {first_sound}");
            let removed_lead = onset_at - first_sound;
            assert!(removed_lead <= onset_at, "{rate}x");
            assert!(out.iter().filter(|x| x.abs() > 0.0 && x.abs() < 0.01).count() >= sr * 80 / 1000, "{rate}x: onset clipped (cut {cut})");
        }
    }

    #[test]
    fn ramp_cut_at_a_comma_keeps_a_comma_pause() {
        assert_eq!(tail_of(Some("year,")), Tail::Comma);
        assert_eq!(tail_of(Some("said;")), Tail::Comma);
        assert_eq!(tail_of(Some("ended.")), Tail::Sentence);
        assert_eq!(tail_of(Some("ended.\"")), Tail::Sentence);
        assert_eq!(tail_of(Some("developers")), Tail::Phrase);
        assert_eq!(tail_of(None), Tail::Sentence);
        // a sentence end keeps at least the comma pause, a comma more than a phrase cut
        let sr = ra_timing::SR as usize;
        let mut pcm: Vec<f32> = (0..sr / 2).map(|i| if i % 2 == 0 { 0.5 } else { -0.5 }).collect();
        pcm.extend(vec![0.0f32; sr]);
        let len = |t| trim_silence(pcm.clone(), &mut [], 2.2, t).len();
        assert!(len(Tail::Sentence) >= len(Tail::Comma));
        assert!(len(Tail::Comma) > len(Tail::Phrase));
        // ~150 ms at 1x, divided by speed
        let comma = len(Tail::Comma) - sr / 2;
        let want = (KEEP_TRAIL_COMMA_MS / 2.2 / 1000.0 * sr as f32) as usize;
        assert!(comma.abs_diff(want) <= 1, "{comma} vs {want}");
    }

    /// Review fix: nothing to cancel without a run in flight; a run of the current
    /// generation is never cancelled.
    #[test]
    fn cancel_stale_only_stops_an_older_generation() {
        let e = Engine::new(std::env::temp_dir().join("ra-tts-cancel"));
        let ctl = e.controller_gen();
        assert!(!e.cancel_stale(ctl, 3), "no run in flight");
        let o = e.begin_run(ctl, 3).expect("registered");
        assert!(!e.cancel_stale(ctl, 3), "same generation keeps running");
        assert!(e.cancel_stale(ctl, 4), "a jump stops it");
        assert!(!e.cancel_stale(ctl, 5), "once only");
        assert!(e.end_run(Some(&o)), "the run learns it was cancelled");
        assert!(e.run.lock().is_none(), "and frees the slot");
    }

    /// Round 3: the run slot is shared by every controller (the engine outlives them),
    /// and each controller's job generations start at 0 again. An old controller's
    /// worker, still finishing its job, must neither overwrite nor clear the slot of a
    /// newer controller's run.
    #[test]
    fn an_old_controllers_worker_cannot_touch_the_new_run_slot() {
        let e = Engine::new(std::env::temp_dir().join("ra-tts-slot"));
        let old = e.controller_gen();
        let new = e.controller_gen();
        assert!(new > old);
        let old_run = e.begin_run(old, 1).expect("old run registered");
        // the new controller starts: its run replaces the old (stale) one
        let new_run = e.begin_run(new, 1).expect("new run registered");
        // the old worker finishing must not clear the new run's slot
        assert!(!e.end_run(Some(&old_run)), "the old run was not cancelled");
        assert!(e.run.lock().as_ref().is_some_and(|r| r.ctl == new && Arc::ptr_eq(&r.opts, &new_run)));
        // the old worker's next job must not overwrite it (same job generation number)
        assert!(e.begin_run(old, 1).is_none(), "an older controller cannot take the slot");
        assert!(e.run.lock().as_ref().is_some_and(|r| r.ctl == new));
        // the new controller's same-numbered generation is not stale for it
        assert!(!e.cancel_stale(new, 1));
        // a run of an older controller is always stale for a newer one
        let e2 = Engine::new(std::env::temp_dir().join("ra-tts-slot2"));
        let (a, b) = (e2.controller_gen(), e2.controller_gen());
        let _r = e2.begin_run(a, 7).unwrap();
        assert!(e2.cancel_stale(b, 7), "same number, older controller: stale");
    }

    /// Real CPU model: a long chunk stopped mid-run returns quickly with `CANCELLED`.
    /// cargo test --release cancel_real -- --ignored --nocapture
    #[test]
    #[ignore]
    fn cancel_real() {
        let e = Engine::new(dirs::data_dir().unwrap().join("ALAN Echo"));
        e.preload("af_heart", "cpu");
        e.synth(&["Warm", "up."], 1.0).unwrap();
        let text = "Eight times a year twelve people sit around a long table in Washington and decide what your money costs and none of them vote on your rent your car loan or your credit card";
        let words: Vec<&str> = text.split(' ').collect();
        let t = Instant::now();
        e.synth(&words, 1.0).unwrap();
        let full = t.elapsed();
        let e2 = Arc::clone(&e);
        let ctl = e.controller_gen();
        let w2: Vec<String> = words.iter().map(|s| s.to_string()).collect();
        let h = std::thread::spawn(move || {
            let w: Vec<&str> = w2.iter().map(String::as_str).collect();
            let t = Instant::now();
            (e2.synth_job(&w, 1.0, ctl, 1), t.elapsed())
        });
        std::thread::sleep(Duration::from_millis(80));
        let stopped = e.cancel_stale(ctl, 2);
        let (r, took) = h.join().unwrap();
        println!("full {full:?}; cancelled run took {took:?}; stopped {stopped}; err {:?}", r.as_ref().err());
        assert!(stopped && r.is_err_and(|x| x == CANCELLED));
        assert!(took < full, "{took:?} vs {full:?}");
        // the engine still works after a cancel
        assert!(e.synth(&["Still", "works."], 1.0).is_ok());
    }

    #[test]
    fn rates_above_2_2_use_the_model_speed_for_the_rest() {
        let close = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4;
        assert!(close(split_rate(1.0), (1.0, 1.0)));
        assert!(close(split_rate(1.5), (1.0, 1.5)));
        assert!(close(split_rate(2.2), (1.0, 2.2)), "Alex's usual speed is unchanged");
        assert!(close(split_rate(2.6), (2.6 / 2.2, 2.2)));
        // review fix: the model never above 1.25 (clarity measured by WER)
        assert!(close(split_rate(3.0), (1.25, 2.4)));
        assert!(close(split_rate(3.5), (1.25, 2.8)));
        assert!(close(split_rate(4.0), (1.25, 3.2)));
        assert!(close(split_rate(f32::NAN), (1.0, 1.0)));
        for r in [1.0f32, 1.7, 2.2, 2.8, 3.5, 4.0] {
            let (m, w) = split_rate(r);
            assert!((m * w - effective_rate(r)).abs() < 1e-4, "{r}");
        }
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
