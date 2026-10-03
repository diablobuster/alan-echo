//! Read Aloud — session state machine, session pipeline and controller thread.
//!
//! Three layers, top to bottom:
//! 1. `step`: the pure state machine (Idle → Picking → Warming → Reading ↔ Paused).
//! 2. `Session`: chunks, the synthesis queue, the in-order audio push and the word
//!    timeline. No Tauri, no UIA: unit-tested, and driven headlessly by the smoke test.
//! 3. `Controller`: one thread ("ra-ctl") that owns every non-Send piece (UIA, mouse
//!    hook, player, windows) and receives `Msg`s. The hotkey / commands / media keys
//!    only post a message (debug-journal rule: the hotkey path never blocks).
//!
//! Nothing runs at rest: the controller starts on the first hotkey press and exits
//! once the session is Idle again with its windows gone; the synth worker, UIA
//! worker, hook and player all live only for a session.

use crate::{
    ra_hook::PickHook,
    ra_player::Player,
    ra_text::{self, Word},
    ra_tts::{ChunkAudio, Device, Engine},
    ra_uia::{Rect, Uia},
};
use parking_lot::Mutex;
use std::collections::{BTreeMap, VecDeque};
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase {
    Idle,
    Picking { reading: bool },
    Warming,
    Reading,
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ev {
    Hotkey,
    Click,
    ClickRejected,
    FirstAudio,
    Esc,
    PlayPause,
    Stop,
    End,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    EnterPick,
    ExitPick,
    StartRead,
    ShowWarming,
    ShowReading,
    ShowPaused,
    Teardown,
    Toast(&'static str),
    BumpGen,
}

pub fn step(p: &Phase, ev: Ev) -> (Phase, Vec<Act>) {
    use Act::*;
    use Phase::*;
    match (p, ev) {
        (Idle, Ev::Error | Ev::Stop) => (Idle, vec![]),
        (_, Ev::Error | Ev::Stop) => (Idle, vec![Teardown]),
        (Idle, Ev::Hotkey) => (Picking { reading: false }, vec![EnterPick]),
        (Reading | Paused | Warming, Ev::Hotkey) => (Picking { reading: true }, vec![EnterPick]),
        (Picking { .. }, Ev::Hotkey) => (*p, vec![]),
        (Picking { .. }, Ev::Click) => (Warming, vec![ExitPick, BumpGen, StartRead, ShowWarming]),
        (Picking { .. }, Ev::ClickRejected) => (*p, vec![]),
        (Picking { reading: false }, Ev::Esc) => (Idle, vec![ExitPick, Teardown]),
        // reading carries on (playback never stopped during pick); the pill shows it again
        (Picking { reading: true }, Ev::Esc) => (Reading, vec![ExitPick, ShowReading]),
        (Warming, Ev::FirstAudio) => (Reading, vec![ShowReading]),
        (Reading, Ev::PlayPause) => (Paused, vec![ShowPaused]),
        (Paused, Ev::PlayPause) => (Reading, vec![ShowReading]),
        (Reading | Paused | Warming, Ev::End) => (Idle, vec![Teardown]),
        _ => (*p, vec![]),
    }
}

// ── Session pipeline ─────────────────────────────────────────────────

/// Word limits per chunk (addendum ruling: first 12, then 30 so CPU synthesis per
/// chunk stays short and the queue never starves at 2.2–3.5x).
const FIRST_MAX: usize = 12;
const CHUNK_MAX: usize = 30;
/// Chunks synthesized ahead of the playhead (queued + in flight + pushed-unplayed).
const AHEAD: usize = 2;
/// Ask UIA for the next batch when fewer unsynthesized words than this remain.
const REFILL_AT: usize = 150;
/// Max samples per `Player::push` (bounds the audio-callback lock hold time).
const PUSH_PIECE: usize = 4800;

/// One chunk to synthesize. `gen` lets the synth worker skip jobs made stale by a jump.
pub struct Job {
    pub gen: u64,
    pub idx: usize,
    pub words: Vec<String>,
    pub rate: f32,
}

pub struct Session {
    pub words: Vec<Word>,
    /// Raw UIA tokens received so far (source index offset for the next batch).
    raw_len: usize,
    /// UIA said there is no more text (an empty `more()`).
    doc_ended: bool,
    pub chunks: Vec<Range<usize>>,
    /// Next chunk to submit for synthesis.
    next_chunk: usize,
    /// Next chunk to push to the player (chunks are pushed strictly in order).
    next_push: usize,
    inflight: usize,
    /// Finished chunks waiting for an earlier one.
    pending: BTreeMap<usize, ChunkAudio>,
    /// (start sample since the last player clear, global word index), ascending.
    timeline: Vec<(u64, usize)>,
    /// End sample of each pushed chunk (since the last clear).
    chunk_ends: Vec<u64>,
    pushed: u64,
    gen: Arc<AtomicU64>,
}

impl Session {
    pub fn new(gen: Arc<AtomicU64>) -> Self {
        Session {
            words: Vec::new(),
            raw_len: 0,
            doc_ended: true,
            chunks: Vec::new(),
            next_chunk: 0,
            next_push: 0,
            inflight: 0,
            pending: BTreeMap::new(),
            timeline: Vec::new(),
            chunk_ends: Vec::new(),
            pushed: 0,
            gen,
        }
    }

    pub fn gen(&self) -> u64 {
        self.gen.load(Ordering::SeqCst)
    }

    /// New read from freshly picked words. The caller clears the player.
    pub fn start(&mut self, words: Vec<Word>, raw_len: usize, doc_ended: bool) {
        self.chunks = ra_text::chunk(&words, FIRST_MAX, CHUNK_MAX);
        self.words = words;
        self.raw_len = raw_len;
        self.doc_ended = doc_ended;
        self.seek(0);
    }

    pub fn clear(&mut self) {
        self.seek(0);
        self.words.clear();
        self.chunks.clear();
        self.raw_len = 0;
        self.doc_ended = true;
    }

    /// Restart synthesis + playback bookkeeping at chunk `idx` under a new generation:
    /// queued jobs and late results of the old one are dropped. The caller clears the player.
    pub fn seek(&mut self, idx: usize) {
        self.gen.fetch_add(1, Ordering::SeqCst);
        self.next_chunk = idx;
        self.next_push = idx;
        self.inflight = 0;
        self.pending.clear();
        self.timeline.clear();
        self.chunk_ends.clear();
        self.pushed = 0;
    }

    fn unplayed(&self, pos: u64) -> usize {
        self.chunk_ends.iter().rev().take_while(|&&e| e > pos).count()
    }

    fn job(&self, idx: usize, rate: f32) -> Option<Job> {
        let r = self.chunks.get(idx)?.clone();
        let words = self.words.get(r)?.iter().map(|w| w.text.clone()).collect();
        Some(Job { gen: self.gen(), idx, words, rate })
    }

    /// Jobs to submit now so that at most `AHEAD` chunks are queued past the playhead.
    pub fn jobs(&mut self, pos: u64, rate: f32) -> Vec<Job> {
        let mut out = Vec::new();
        while self.next_chunk < self.chunks.len() && self.inflight + self.pending.len() + self.unplayed(pos) < AHEAD {
            let Some(j) = self.job(self.next_chunk, rate) else { break };
            out.push(j);
            self.next_chunk += 1;
            self.inflight += 1;
        }
        out
    }

    /// Resubmit chunk `idx` (after a recoverable engine error).
    pub fn retry(&mut self, idx: usize, rate: f32) -> Option<Job> {
        let j = self.job(idx, rate)?;
        self.inflight += 1;
        Some(j)
    }

    /// True when the next UIA batch should be fetched.
    pub fn wants_more(&self) -> bool {
        let unsubmitted_from = self.chunks.get(self.next_chunk).map_or(self.words.len(), |c| c.start);
        !self.doc_ended && self.words.len().saturating_sub(unsubmitted_from) < REFILL_AT
    }

    /// Append the next UIA batch. Source indices continue after the tokens received so
    /// far; unsubmitted chunks are re-chunked so a sentence cut at the batch edge joins up.
    /// Only an empty batch ends the document.
    pub fn append_more(&mut self, raw: &[String]) {
        if raw.is_empty() {
            self.doc_ended = true;
            return;
        }
        let mut more = ra_text::normalize(raw);
        for w in &mut more {
            for s in &mut w.src {
                *s += self.raw_len;
            }
        }
        self.raw_len += raw.len();
        let tail = self.chunks.get(self.next_chunk).map_or(self.words.len(), |c| c.start);
        self.words.extend(more);
        self.chunks.truncate(self.next_chunk);
        let rest = ra_text::chunk(&self.words[tail..], CHUNK_MAX, CHUNK_MAX);
        self.chunks.extend(rest.into_iter().map(|r| r.start + tail..r.end + tail));
    }

    /// A failed job. Returns false when it is stale (ignore it).
    pub fn on_error(&mut self, g: u64) -> bool {
        if g != self.gen() {
            return false;
        }
        self.inflight = self.inflight.saturating_sub(1);
        true
    }

    /// One finished job. Returns the PCM to push now (in chunk order) and how many
    /// chunks were consumed; nothing for a stale result or one still waiting for an
    /// earlier chunk. A chunk the engine only partly covered (510-id limit) is split
    /// and its unspoken tail queued right after it.
    pub fn on_chunk(&mut self, g: u64, idx: usize, a: ChunkAudio) -> (Vec<Vec<f32>>, usize) {
        if g != self.gen() || idx < self.next_push || idx >= self.chunks.len() {
            return (Vec::new(), 0);
        }
        self.inflight = self.inflight.saturating_sub(1);
        self.pending.insert(idx, a);
        let (mut out, mut n) = (Vec::new(), 0);
        while let Some(a) = self.pending.remove(&self.next_push) {
            let idx = self.next_push;
            let r = self.chunks[idx].clone();
            let covered = if a.covered == 0 { r.len() } else { a.covered.min(r.len()) };
            for w in a.words.iter().filter(|w| w.word < covered) {
                self.timeline.push((self.pushed + w.start, r.start + w.word));
            }
            self.pushed += a.pcm.len() as u64;
            self.chunk_ends.push(self.pushed);
            out.push(a.pcm);
            self.next_push += 1;
            n += 1;
            if covered < r.len() {
                self.chunks[idx] = r.start..r.start + covered;
                self.chunks.insert(idx + 1, r.start + covered..r.end);
                // jobs already queued past `idx` now point at shifted chunks: drop them
                self.gen.fetch_add(1, Ordering::SeqCst);
                self.next_chunk = idx + 1;
                self.inflight = 0;
                self.pending.clear();
                break;
            }
        }
        (out, n)
    }

    /// Global index of the word being spoken at player position `pos`.
    pub fn word_at(&self, pos: u64) -> Option<usize> {
        let k = self.timeline.partition_point(|&(s, _)| s <= pos);
        (k > 0).then(|| self.timeline[k - 1].1)
    }

    pub fn chunk_of(&self, wi: usize) -> Option<usize> {
        let k = self.chunks.partition_point(|r| r.end <= wi);
        self.chunks.get(k).filter(|r| r.contains(&wi)).map(|_| k)
    }

    /// The chunk a skip is relative to: the one being spoken, else the last pushed.
    pub fn current_chunk(&self, cur_word: Option<usize>) -> usize {
        cur_word.and_then(|w| self.chunk_of(w)).unwrap_or(self.next_push.saturating_sub(1))
    }

    /// Everything synthesized, pushed and played, and the document has no more text.
    pub fn finished(&self, pos: u64) -> bool {
        self.doc_ended
            && self.next_chunk >= self.chunks.len()
            && self.next_push >= self.chunks.len()
            && self.inflight == 0
            && self.pending.is_empty()
            && pos >= self.pushed
    }
}

/// The synth worker: runs jobs in order on its own thread, skipping any made stale by
/// a jump, and answers with `Msg::Chunk`. Ends when the job sender is dropped.
pub fn spawn_synth(engine: Arc<Engine>, gen: Arc<AtomicU64>, out: mpsc::Sender<Msg>) -> Option<mpsc::Sender<Job>> {
    let (tx, rx) = mpsc::channel::<Job>();
    std::thread::Builder::new()
        .name("ra-synth".into())
        .spawn(move || {
            for job in rx {
                if job.gen != gen.load(Ordering::SeqCst) {
                    continue;
                }
                let words: Vec<&str> = job.words.iter().map(String::as_str).collect();
                let r = engine.synth(&words, job.rate);
                if out.send(Msg::Chunk(job.gen, job.idx, r)).is_err() {
                    break;
                }
            }
        })
        .map_err(|e| log::warn!("read-aloud: could not spawn synth worker: {e}"))
        .ok()?;
    Some(tx)
}

// ── Controller ───────────────────────────────────────────────────────

pub enum Msg {
    Hotkey,
    Hover(i32, i32),
    Click(i32, i32),
    Esc,
    PlayPause,
    Skip(i32),
    Speed(f32),
    Stop,
    Chunk(u64, usize, Result<ChunkAudio, String>),
}

const HOTKEY: &str = "CmdOrCtrl+Shift+E";
const ESC_KEYS: &[&str] = &["Escape"];
const MEDIA_KEYS: &[&str] = &["MediaPlayPause", "MediaTrackNext", "MediaTrackPrevious"];
const TICK: Duration = Duration::from_millis(33);
const HOVER_MIN_MS: u64 = 33;
/// Word rects are re-read this often even when the word did not change (the page may
/// have scrolled; rects are never cached).
const RECTS_REFRESH: Duration = Duration::from_millis(250);
/// The pill re-sends its state this often while reading (an event emitted before the
/// pill's page finished loading is lost).
const STATE_RESEND: Duration = Duration::from_millis(1000);
/// How long an error stays on the pill after the session ended.
const ERROR_LINGER: Duration = Duration::from_secs(4);

pub fn clamp_speed(s: f32) -> f32 {
    if !s.is_finite() {
        return 2.2;
    }
    (s.clamp(1.0, 3.5) * 10.0).round() / 10.0
}

pub struct ReadAloud {
    slot: Arc<Mutex<Option<mpsc::Sender<Msg>>>>,
    engine: Arc<Engine>,
}

impl ReadAloud {
    pub fn new(data_dir: std::path::PathBuf) -> Self {
        Self { slot: Arc::new(Mutex::new(None)), engine: Engine::new(data_dir) }
    }

    /// Post a message, starting the controller thread if it is not running.
    /// Never blocks beyond a thread spawn (safe on the hotkey path).
    pub fn send(&self, app: &AppHandle, m: Msg) {
        let mut g = self.slot.lock();
        if g.is_none() {
            let (tx, rx) = mpsc::channel();
            let ctl = Controller::new(app.clone(), Arc::clone(&self.engine), tx.clone(), Arc::clone(&self.slot));
            match std::thread::Builder::new().name("ra-ctl".into()).spawn(move || ctl.run(rx)) {
                Ok(_) => *g = Some(tx),
                Err(e) => {
                    log::warn!("read-aloud: could not start controller: {e}");
                    return;
                }
            }
        }
        if let Some(tx) = g.as_ref() {
            let _ = tx.send(m);
        }
    }

    /// Post a message only if a session controller is running (pill buttons, media
    /// keys, Esc, settings changes). Returns whether it was delivered.
    pub fn post(&self, m: Msg) -> bool {
        self.slot.lock().as_ref().is_some_and(|tx| tx.send(m).is_ok())
    }
}

/// Registers Ctrl+Shift+E. Call on the main thread (the global-shortcut plugin's
/// hotkey window lives there). The handler only posts a message.
pub fn register_hotkey(app: &AppHandle) -> bool {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
    if app.global_shortcut().is_registered(HOTKEY) {
        return true; // enabled twice: keep the existing registration
    }
    app.global_shortcut()
        .on_shortcut(HOTKEY, |app, _s, ev| {
            if ev.state == ShortcutState::Pressed {
                if let Some(st) = app.try_state::<Arc<crate::AppState>>() {
                    st.read_aloud.send(app, Msg::Hotkey);
                }
            }
        })
        .map_err(|e| log::warn!("read-aloud hotkey: {e}"))
        .is_ok()
}

pub fn unregister_hotkey(app: &AppHandle) {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let _ = app.global_shortcut().unregister(HOTKEY);
}

/// The hotkey label for the `hotkeys` JSON.
pub fn hotkey_label() -> &'static str {
    "Ctrl + Shift + E"
}

/// (Un)register session keys on the main thread (non-blocking post).
fn set_keys(app: &AppHandle, keys: &'static [&'static str], on: bool) {
    let h = app.clone();
    let posted = app.run_on_main_thread(move || {
        use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
        let gs = h.global_shortcut();
        for &k in keys {
            if on {
                let r = gs.on_shortcut(k, move |app, _s, ev| {
                    if ev.state != ShortcutState::Pressed {
                        return;
                    }
                    let m = match k {
                        "Escape" => Msg::Esc,
                        "MediaPlayPause" => Msg::PlayPause,
                        "MediaTrackNext" => Msg::Skip(1),
                        _ => Msg::Skip(-1),
                    };
                    if let Some(st) = app.try_state::<Arc<crate::AppState>>() {
                        st.read_aloud.post(m);
                    }
                });
                if let Err(e) = r {
                    log::warn!("read-aloud: could not register {k}: {e}");
                }
            } else {
                let _ = gs.unregister(k);
            }
        }
    });
    if let Err(e) = posted {
        log::warn!("read-aloud: key (un)registration not posted: {e}");
    }
}

/// A monitor in physical pixels plus its scale factor.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Mon {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    scale: f64,
}

mod win {
    use super::Mon;
    use windows::Win32::Foundation::{HWND, POINT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST,
        MONITOR_DEFAULTTONULL,
    };
    use windows::Win32::UI::HiDpi::{
        GetDpiForMonitor, SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, MDT_EFFECTIVE_DPI,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetAncestor, GetCursorPos, GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindow, GA_ROOT,
    };

    fn hwnd(h: isize) -> HWND {
        HWND(h as *mut _)
    }

    pub fn per_monitor_dpi() {
        unsafe {
            let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
    }

    fn mon(h: HMONITOR) -> Option<Mon> {
        if h.is_invalid() {
            return None;
        }
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !unsafe { GetMonitorInfoW(h, &mut mi) }.as_bool() {
            return None;
        }
        let (mut dx, mut dy) = (96u32, 96u32);
        let scale = match unsafe { GetDpiForMonitor(h, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) } {
            Ok(()) if dx > 0 => dx as f64 / 96.0,
            _ => 1.0,
        };
        let r = mi.rcMonitor;
        Some(Mon { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top, scale })
    }

    pub fn monitor_of_window(h: isize) -> Option<Mon> {
        if h == 0 {
            return None;
        }
        mon(unsafe { MonitorFromWindow(hwnd(h), MONITOR_DEFAULTTONEAREST) })
    }

    pub fn monitor_of_point(x: i32, y: i32) -> Option<Mon> {
        mon(unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST) })
    }

    pub fn point_on_screen(x: i32, y: i32) -> bool {
        !unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONULL) }.is_invalid()
    }

    pub fn cursor() -> Option<(i32, i32)> {
        let mut p = POINT::default();
        unsafe { GetCursorPos(&mut p) }.ok().map(|_| (p.x, p.y))
    }

    pub fn is_own(h: isize) -> bool {
        if h == 0 {
            return false;
        }
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd(h), Some(&mut pid)) };
        pid == std::process::id()
    }

    /// The source window is on screen and the one the user is looking at:
    /// it exists, is not minimized, and is (the root of) the foreground window.
    pub fn source_visible(src: isize) -> bool {
        if src == 0 {
            return false;
        }
        unsafe {
            let s = hwnd(src);
            if !IsWindow(s).as_bool() || IsIconic(s).as_bool() {
                return false;
            }
            let fg = GetForegroundWindow();
            !fg.is_invalid() && GetAncestor(fg, GA_ROOT) == s
        }
    }
}

struct Controller {
    app: AppHandle,
    engine: Arc<Engine>,
    tx: mpsc::Sender<Msg>,
    slot: Arc<Mutex<Option<mpsc::Sender<Msg>>>>,
    phase: Phase,
    s: Session,
    uia: Option<Uia>,
    hook: Option<PickHook>,
    /// A hook dropped by ExitPick; released after the click's acts ran (its Drop may
    /// wait up to 500 ms for the swallowed mouse-up).
    hook_to_drop: Option<PickHook>,
    player: Option<Player>,
    synth: Option<mpsc::Sender<Job>>,
    /// Words of an accepted click, consumed by StartRead.
    picked: Option<(Vec<Word>, usize)>,
    cur_word: Option<usize>,
    speed: f32,
    style: String,
    voice: String,
    device_pref: String,
    /// Fallback source (hotkey-time foreground) until a pick succeeds.
    source_hwnd: isize,
    overlay: Option<Mon>,
    used_cpu_retry: bool,
    device_lost: bool,
    /// Pill status line ("Paused" etc. are rendered by the pill itself).
    message: Option<String>,
    shown: &'static str,
    esc_on: bool,
    media_on: bool,
    last_hover: Vec<Rect>,
    last_word: Option<Vec<Rect>>,
    rects_at: Instant,
    rects_retry: bool,
    state_at: Instant,
    /// The pill shows an error after the session ended; close it at this time.
    linger_until: Option<Instant>,
    keep_pill: bool,
    /// Bumped by every `feed`; an act that fed a nested event stops the outer act list.
    epoch: u64,
}

impl Controller {
    fn new(app: AppHandle, engine: Arc<Engine>, tx: mpsc::Sender<Msg>, slot: Arc<Mutex<Option<mpsc::Sender<Msg>>>>) -> Self {
        let now = Instant::now();
        Controller {
            app,
            engine,
            tx,
            slot,
            phase: Phase::Idle,
            s: Session::new(Arc::new(AtomicU64::new(0))),
            uia: None,
            hook: None,
            hook_to_drop: None,
            player: None,
            synth: None,
            picked: None,
            cur_word: None,
            speed: 2.2,
            style: "brass".into(),
            voice: "af_heart".into(),
            device_pref: "auto".into(),
            source_hwnd: 0,
            overlay: None,
            used_cpu_retry: false,
            device_lost: false,
            message: None,
            shown: "warming",
            esc_on: false,
            media_on: false,
            last_hover: Vec::new(),
            last_word: None,
            rects_at: now,
            rects_retry: false,
            state_at: now,
            linger_until: None,
            keep_pill: false,
            epoch: 0,
        }
    }

    /// Audio is (or may be) playing: drive the timeline / queue.
    fn ticking(&self) -> bool {
        matches!(self.phase, Phase::Reading | Phase::Picking { reading: true })
    }

    fn run(mut self, rx: mpsc::Receiver<Msg>) {
        win::per_monitor_dpi();
        let mut backlog: VecDeque<Msg> = VecDeque::new();
        let mut next_tick = Instant::now();
        loop {
            let msg = match backlog.pop_front() {
                Some(m) => Some(m),
                None => {
                    let deadline = if self.ticking() { Some(next_tick) } else { self.linger_until };
                    match deadline {
                        None => match rx.recv() {
                            Ok(m) => Some(m),
                            Err(_) => break,
                        },
                        Some(d) => match rx.recv_timeout(d.saturating_duration_since(Instant::now())) {
                            Ok(m) => Some(m),
                            Err(mpsc::RecvTimeoutError::Timeout) => None,
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        },
                    }
                }
            };
            if let Some(mut m) = msg {
                // coalesce queued hovers: only the newest position matters
                if matches!(m, Msg::Hover(..)) {
                    while let Ok(n) = rx.try_recv() {
                        if matches!(n, Msg::Hover(..)) {
                            m = n;
                        } else {
                            backlog.push_back(n);
                            break;
                        }
                    }
                }
                self.handle(m);
            }
            let now = Instant::now();
            if self.ticking() && now >= next_tick {
                self.tick();
                next_tick = now + TICK;
            }
            if self.linger_until.is_some_and(|t| now >= t) {
                self.linger_until = None;
                self.destroy("ra-pill");
            }
            // Back to rest: exit (zero threads) unless something is still queued.
            if self.phase == Phase::Idle && self.linger_until.is_none() && backlog.is_empty() {
                let mut g = self.slot.lock();
                match rx.try_recv() {
                    Ok(m) => backlog.push_back(m),
                    Err(_) => {
                        *g = None;
                        break;
                    }
                }
            }
        }
        log::info!("read-aloud: controller stopped");
    }

    fn handle(&mut self, m: Msg) {
        match m {
            Msg::Hotkey => {
                if self.phase == Phase::Idle {
                    self.close_lingering_pill();
                }
                self.feed(Ev::Hotkey);
            }
            Msg::Hover(x, y) => {
                if matches!(self.phase, Phase::Picking { .. }) {
                    self.on_hover(x, y);
                }
            }
            Msg::Click(x, y) => {
                if matches!(self.phase, Phase::Picking { .. }) {
                    self.on_click(x, y);
                }
            }
            Msg::Esc => self.feed(Ev::Esc),
            Msg::PlayPause => {
                if self.phase == Phase::Paused && self.device_lost && !self.recover_device() {
                    return;
                }
                self.feed(Ev::PlayPause);
            }
            Msg::Skip(dir) => {
                if matches!(self.phase, Phase::Reading | Phase::Paused | Phase::Warming) {
                    self.skip(dir);
                }
            }
            Msg::Speed(s) => {
                self.speed = clamp_speed(s);
                if self.phase != Phase::Idle {
                    self.emit_state();
                }
            }
            Msg::Stop => {
                if self.phase == Phase::Idle {
                    self.close_lingering_pill();
                } else {
                    self.feed(Ev::Stop);
                }
            }
            Msg::Chunk(g, idx, r) => self.on_chunk(g, idx, r),
        }
    }

    fn feed(&mut self, ev: Ev) {
        let (p, acts) = step(&self.phase, ev);
        self.run_acts(p, acts);
    }

    /// Error path: toast, then the state machine's Error (Teardown).
    fn fail(&mut self, msg: &'static str) {
        log::warn!("read-aloud: {msg}");
        let (p, mut acts) = step(&self.phase, Ev::Error);
        acts.insert(0, Act::Toast(msg));
        self.run_acts(p, acts);
    }

    fn run_acts(&mut self, p: Phase, acts: Vec<Act>) {
        self.phase = p;
        self.epoch += 1;
        let mine = self.epoch;
        for a in acts {
            if self.epoch != mine {
                break; // a nested event (e.g. a failure) already moved the session on
            }
            self.act(a);
        }
        self.hook_to_drop = None;
        if self.epoch == mine {
            self.keep_pill = false; // only meaningful for a Toast followed by Teardown
        }
    }

    fn act(&mut self, a: Act) {
        match a {
            Act::EnterPick => self.enter_pick(),
            Act::ExitPick => {
                self.hook_to_drop = self.hook.take();
                self.set_esc(false);
                self.emit_overlay("ra-overlay-mode", serde_json::json!({ "mode": "read" }));
                self.emit_hover(Vec::new(), true);
            }
            Act::BumpGen => {
                if let Some(p) = &self.player {
                    p.clear();
                }
                self.s.seek(0);
                self.cur_word = None;
                self.last_word = None;
            }
            Act::StartRead => self.start_read(),
            Act::ShowWarming => self.show("warming"),
            Act::ShowReading => self.show("reading"),
            Act::ShowPaused => self.show("paused"),
            Act::Teardown => self.teardown(),
            Act::Toast(msg) => self.toast(msg),
        }
    }

    fn read_settings(&mut self) {
        let Some(st) = self.app.try_state::<Arc<crate::AppState>>() else { return };
        let s = st.settings.lock();
        self.speed = clamp_speed(s.get("read_aloud_speed").and_then(|v| v.as_f64()).unwrap_or(2.2) as f32);
        self.voice = s.get_str("read_aloud_voice").unwrap_or_else(|| "af_heart".into());
        self.device_pref = s.get_str("read_aloud_device").unwrap_or_else(|| "auto".into());
        self.style = match s.get_str("read_aloud_highlight").as_deref() {
            Some("mono") => "mono".into(),
            Some("off") => "off".into(),
            _ => "brass".into(),
        };
    }

    fn enter_pick(&mut self) {
        self.close_lingering_pill();
        self.read_settings();
        let fg = crate::paste::foreground_window();
        if fg != 0 && !win::is_own(fg) {
            self.source_hwnd = fg;
        }
        if self.uia.is_none() {
            // warm-up of the foreground window starts now, on the UIA worker
            self.uia = Some(Uia::start(self.source_hwnd));
        }
        self.engine.preload(&self.voice, &self.device_pref); // engine load starts now
        let mon = win::monitor_of_window(self.source_hwnd)
            .or_else(|| win::cursor().and_then(|(x, y)| win::monitor_of_point(x, y)));
        let Some(mon) = mon else { return self.fail("No display found for Read Aloud") };
        if !self.show_overlay(mon) {
            return self.fail("Couldn't open the Read Aloud overlay");
        }
        self.emit_overlay("ra-overlay-mode", serde_json::json!({ "mode": "pick" }));
        self.emit_hover(Vec::new(), true);
        self.set_esc(true);
        let (tm, tc) = (self.tx.clone(), self.tx.clone());
        let (t0, last) = (Instant::now(), Arc::new(AtomicU64::new(0)));
        let hook = PickHook::install(
            move |x, y| {
                // throttle inside the hook proc: at most one hover per 33 ms
                let now = t0.elapsed().as_millis() as u64 + HOVER_MIN_MS;
                if now.saturating_sub(last.load(Ordering::Relaxed)) >= HOVER_MIN_MS {
                    last.store(now, Ordering::Relaxed);
                    let _ = tm.send(Msg::Hover(x, y));
                }
            },
            move |x, y| {
                let _ = tc.send(Msg::Click(x, y));
            },
        );
        if !hook.is_active() {
            return self.fail("Couldn't capture the mouse for Read Aloud");
        }
        self.hook = Some(hook);
    }

    fn on_hover(&mut self, x: i32, y: i32) {
        let Some(u) = &self.uia else { return };
        let rects = u.hover(x, y).unwrap_or_default();
        if !rects.is_empty() {
            if let Some(m) = win::monitor_of_point(x, y) {
                if Some(m) != self.overlay {
                    self.show_overlay(m);
                }
            }
        }
        let css = self.to_css(&rects);
        self.emit_hover(css, false);
    }

    fn on_click(&mut self, x: i32, y: i32) {
        let res = match &self.uia {
            Some(u) => u.pick(x, y),
            None => Err("text reader is not running".into()),
        };
        match res {
            Ok(raw) => {
                let words = ra_text::normalize(&raw);
                if words.is_empty() {
                    log::info!("read-aloud: pick had no speakable words");
                    return self.reject();
                }
                self.picked = Some((words, raw.len()));
                self.feed(Ev::Click);
            }
            Err(e) => {
                log::info!("read-aloud: pick rejected: {e}");
                self.reject();
            }
        }
    }

    fn reject(&mut self) {
        self.emit_overlay("ra-pick-rejected", serde_json::json!({}));
        self.feed(Ev::ClickRejected);
    }

    fn start_read(&mut self) {
        let Some((words, raw_len)) = self.picked.take() else { return self.fail("Nothing readable here") };
        self.read_settings();
        if self.player.is_none() {
            match Player::start() {
                Ok(p) => self.player = Some(p),
                Err(e) => {
                    log::warn!("read-aloud: player: {e}");
                    return self.fail("No audio output device");
                }
            }
        }
        if self.synth.is_none() {
            self.synth = spawn_synth(Arc::clone(&self.engine), Arc::clone(&self.s.gen), self.tx.clone());
            if self.synth.is_none() {
                return self.fail("Voice engine failed");
            }
        }
        self.used_cpu_retry = false;
        self.device_lost = false;
        self.message = None;
        self.set_media(true);
        if let Some(u) = &self.uia {
            self.source_hwnd = u.source_hwnd();
        }
        if let Some(m) = win::monitor_of_window(self.source_hwnd) {
            if Some(m) != self.overlay {
                self.show_overlay(m);
            }
        }
        if let Some(p) = &self.player {
            p.clear();
        }
        self.s.start(words, raw_len, false);
        self.cur_word = None;
        self.last_word = None;
        self.ensure_pill();
        self.pump();
    }

    fn submit(&mut self, jobs: Vec<Job>) {
        for j in jobs {
            let sent = self.synth.as_ref().is_some_and(|tx| tx.send(j).is_ok());
            if !sent {
                return self.fail("Voice engine failed");
            }
        }
    }

    /// Keep the synthesis queue AHEAD chunks past the playhead; fetch more text when low.
    fn pump(&mut self) {
        let Some(pos) = self.player.as_ref().map(|p| p.position()) else { return };
        let jobs = self.s.jobs(pos, self.speed);
        self.submit(jobs);
        if self.s.wants_more() {
            let more = self.uia.as_ref().map(|u| u.more()).unwrap_or_default();
            self.s.append_more(&more);
            let jobs = self.s.jobs(pos, self.speed);
            self.submit(jobs);
        }
    }

    fn on_chunk(&mut self, g: u64, idx: usize, r: Result<ChunkAudio, String>) {
        if self.phase == Phase::Idle {
            return;
        }
        match r {
            Err(e) => {
                if !self.s.on_error(g) {
                    return;
                }
                log::warn!("read-aloud: synth chunk {idx}: {e}");
                if e.starts_with("dml") && !self.used_cpu_retry {
                    self.used_cpu_retry = true;
                    self.engine.reset_to_cpu(&self.voice);
                    self.message = Some("Voice engine hiccup — using CPU".into());
                    self.emit_state();
                    let job = self.s.retry(idx, self.speed);
                    self.submit(job.into_iter().collect());
                } else {
                    self.fail("Voice engine failed");
                }
            }
            Ok(a) => {
                let (pcm, n) = self.s.on_chunk(g, idx, a);
                if let Some(p) = &self.player {
                    for buf in &pcm {
                        for piece in buf.chunks(PUSH_PIECE) {
                            p.push(piece);
                        }
                    }
                }
                if n > 0 && self.phase == Phase::Warming {
                    self.feed(Ev::FirstAudio);
                }
                if self.phase != Phase::Idle {
                    self.pump();
                }
            }
        }
    }

    fn tick(&mut self) {
        let Some((pos, failed)) = self.player.as_ref().map(|p| (p.position(), p.failed())) else { return };
        if failed && !self.device_lost {
            self.device_lost = true;
            self.message = Some("Audio device changed — press play".into());
            if self.phase == Phase::Reading {
                self.feed(Ev::PlayPause);
            }
            return;
        }
        self.pump();
        if self.phase != Phase::Reading {
            return; // picking while reading: audio + queue only, no highlight
        }
        if let Some(wi) = self.s.word_at(pos) {
            let changed = self.cur_word != Some(wi);
            if changed {
                self.cur_word = Some(wi);
                self.emit_sentence(wi);
            }
            self.update_word_rects(changed);
        }
        if self.s.finished(pos) {
            return self.feed(Ev::End);
        }
        if self.state_at.elapsed() >= STATE_RESEND {
            self.emit_state();
        }
    }

    fn update_word_rects(&mut self, changed: bool) {
        if !changed && !self.rects_retry && self.rects_at.elapsed() < RECTS_REFRESH {
            return;
        }
        self.rects_at = Instant::now();
        let retrying = self.rects_retry && !changed;
        self.rects_retry = false;
        let Some(src) = self.cur_word.and_then(|w| self.s.words.get(w)).map(|w| w.src.clone()) else { return };
        let Some(u) = &self.uia else { return };
        let source = u.source_hwnd();
        let mut css = Vec::new();
        if self.style != "off" && win::source_visible(source) {
            let rects = u.rects(src.clone());
            if rects.is_empty() {
                if changed {
                    // off screen: scroll it into view, look again next tick
                    if let Some(&first) = src.first() {
                        u.scroll_to(first);
                    }
                    self.rects_retry = true;
                } else if retrying {
                    log::debug!("read-aloud: word still has no rects after scrolling");
                }
            } else {
                let (cx, cy) = (rects[0].x + rects[0].w / 2.0, rects[0].y + rects[0].h / 2.0);
                if let Some(m) = win::monitor_of_point(cx as i32, cy as i32) {
                    if Some(m) != self.overlay {
                        self.show_overlay(m);
                    }
                }
                css = self.to_css(&rects);
            }
        }
        if self.last_word.as_ref() != Some(&css) {
            self.emit_overlay("ra-rects", serde_json::json!({ "kind": "word", "rects": css, "style": self.style }));
            self.last_word = Some(css);
        }
    }

    fn skip(&mut self, dir: i32) {
        if self.s.chunks.is_empty() {
            return;
        }
        let cur = self.s.current_chunk(self.cur_word);
        let last = self.s.chunks.len() - 1;
        let target = if dir < 0 {
            cur.saturating_sub(1)
        } else if cur < last {
            cur + 1
        } else {
            return; // already in the last loaded chunk
        };
        if let Some(p) = &self.player {
            p.clear();
        }
        self.s.seek(target);
        self.cur_word = None;
        self.pump();
    }

    /// The audio device went away: start a player on the new default device and
    /// restart from the chunk that was playing.
    fn recover_device(&mut self) -> bool {
        self.player = None;
        match Player::start() {
            Ok(p) => {
                p.set_paused(true);
                self.player = Some(p);
                self.device_lost = false;
                self.message = None;
                let target = self.s.current_chunk(self.cur_word);
                self.s.seek(target);
                self.cur_word = None;
                self.pump();
                true
            }
            Err(e) => {
                log::warn!("read-aloud: player restart: {e}");
                self.fail("No audio output device");
                false
            }
        }
    }

    fn show(&mut self, state: &'static str) {
        if let Some(p) = &self.player {
            p.set_paused(self.phase == Phase::Paused);
        }
        self.shown = state;
        self.emit_state();
    }

    fn teardown(&mut self) {
        self.hook = None;
        self.hook_to_drop = None;
        self.set_esc(false);
        self.set_media(false);
        self.synth = None; // the worker ends after its current job
        self.player = None;
        self.uia = None;
        self.s.clear();
        self.picked = None;
        self.cur_word = None;
        self.last_word = None;
        self.last_hover.clear();
        self.message = None;
        self.device_lost = false;
        self.destroy("ra-overlay");
        self.overlay = None;
        if std::mem::take(&mut self.keep_pill) {
            self.linger_until = Some(Instant::now() + ERROR_LINGER);
        } else {
            self.destroy("ra-pill");
        }
        self.engine.touch(); // the 2-minute idle unload counts from here
    }

    fn toast(&mut self, msg: &'static str) {
        log::info!("read-aloud: {msg}");
        if self.app.get_webview_window("ra-pill").is_some() {
            self.keep_pill = true;
            self.emit_pill(
                "ra-state",
                serde_json::json!({ "state": "error", "speed": self.speed, "device": null, "message": msg }),
            );
        } else {
            use tauri_plugin_notification::NotificationExt;
            let _ = self.app.notification().builder().title("Read Aloud").body(msg).show();
        }
    }

    fn close_lingering_pill(&mut self) {
        if self.linger_until.take().is_some() {
            self.destroy("ra-pill");
        }
    }

    fn set_esc(&mut self, on: bool) {
        if self.esc_on != on {
            self.esc_on = on;
            set_keys(&self.app, ESC_KEYS, on);
        }
    }

    fn set_media(&mut self, on: bool) {
        if self.media_on != on {
            self.media_on = on;
            set_keys(&self.app, MEDIA_KEYS, on);
        }
    }

    // ── windows ──

    fn destroy(&self, label: &str) {
        if let Some(w) = self.app.get_webview_window(label) {
            // destroy, not close: close goes through CloseRequested
            let _ = w.destroy();
        }
    }

    /// Create (once) and place the click-through overlay over `mon`.
    fn show_overlay(&mut self, mon: Mon) -> bool {
        let w = match self.app.get_webview_window("ra-overlay") {
            Some(w) => w,
            None => {
                let built = WebviewWindowBuilder::new(&self.app, "ra-overlay", WebviewUrl::App("index.html".into()))
                    .title("Read Aloud overlay")
                    .transparent(true)
                    .decorations(false)
                    .always_on_top(true)
                    .skip_taskbar(true)
                    .focused(false)
                    .focusable(false)
                    .shadow(false)
                    .resizable(false)
                    .visible(false)
                    .build();
                match built {
                    Ok(w) => w,
                    Err(e) => {
                        log::warn!("read-aloud: overlay window: {e}");
                        return false;
                    }
                }
            }
        };
        // always click-through: the low-level hook captures pick clicks
        let _ = w.set_ignore_cursor_events(true);
        let _ = w.set_position(tauri::PhysicalPosition::new(mon.x, mon.y));
        let _ = w.set_size(tauri::PhysicalSize::new(mon.w.max(1) as u32, mon.h.max(1) as u32));
        let _ = w.show();
        if self.overlay != Some(mon) {
            self.overlay = Some(mon);
            // rects were relative to the old monitor
            self.last_hover.clear();
            self.last_word = None;
        }
        true
    }

    fn ensure_pill(&mut self) {
        if self.app.get_webview_window("ra-pill").is_some() {
            return;
        }
        let saved = self.app.try_state::<Arc<crate::AppState>>().and_then(|st| {
            let v = st.settings.lock().get("read_aloud_pill_pos").cloned()?;
            Some((v.get(0)?.as_f64()?, v.get(1)?.as_f64()?))
        });
        // physical px (the pill saves its onMoved position); must still be on a screen
        let pos = saved
            .map(|(x, y)| (x as i32, y as i32))
            .filter(|&(x, y)| win::point_on_screen(x + 40, y + 20))
            .or_else(|| {
                let m = self.overlay?;
                Some((m.x + (40.0 * m.scale) as i32, m.y + (40.0 * m.scale) as i32))
            });
        let built = WebviewWindowBuilder::new(&self.app, "ra-pill", WebviewUrl::App("index.html".into()))
            .title("Now Reading")
            .transparent(true)
            .decorations(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .focused(false)
            .focusable(false)
            .resizable(false)
            .shadow(false)
            .inner_size(540.0, 132.0)
            .visible(false)
            .build();
        match built {
            Ok(w) => {
                if let Some((x, y)) = pos {
                    let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
                }
                let _ = w.show();
            }
            Err(e) => log::warn!("read-aloud: pill window: {e}"), // reading still works
        }
    }

    // ── events ──

    fn emit_overlay(&self, event: &str, payload: serde_json::Value) {
        let _ = self.app.emit_to("ra-overlay", event, payload);
    }

    fn emit_pill(&self, event: &str, payload: serde_json::Value) {
        let _ = self.app.emit_to("ra-pill", event, payload);
    }

    /// Hover rects, only when they changed (`force` for mode switches).
    fn emit_hover(&mut self, css: Vec<Rect>, force: bool) {
        if force || css != self.last_hover {
            self.emit_overlay("ra-rects", serde_json::json!({ "kind": "hover", "rects": css, "style": self.style }));
            self.last_hover = css;
        }
    }

    fn emit_state(&mut self) {
        self.state_at = Instant::now();
        let device = self.engine.device().map(|d| match d {
            Device::Gpu => "gpu",
            Device::Cpu => "cpu",
        });
        self.emit_pill(
            "ra-state",
            serde_json::json!({ "state": self.shown, "speed": self.speed, "device": device, "message": self.message }),
        );
    }

    fn emit_sentence(&self, wi: usize) {
        let Some(ci) = self.s.chunk_of(wi) else { return };
        let r = self.s.chunks[ci].clone();
        let words: Vec<&str> = self.s.words[r.clone()].iter().map(|w| w.text.as_str()).collect();
        let progress = if self.s.words.is_empty() { 0.0 } else { (wi as f64 / self.s.words.len() as f64).min(1.0) };
        self.emit_pill("ra-sentence", serde_json::json!({ "words": words, "current": wi - r.start, "progress": progress }));
    }

    /// Physical screen rects → CSS px relative to the overlay's monitor.
    fn to_css(&self, rects: &[Rect]) -> Vec<Rect> {
        let Some(m) = self.overlay else { return Vec::new() };
        rects
            .iter()
            .map(|r| Rect {
                x: (r.x - m.x as f64) / m.scale,
                y: (r.y - m.y as f64) / m.scale,
                w: r.w / m.scale,
                h: r.h / m.scale,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Phase::*;

    #[test]
    fn hotkey_from_idle_enters_pick() {
        let (p, a) = step(&Idle, Ev::Hotkey);
        assert_eq!(p, Picking { reading: false });
        assert!(a.contains(&Act::EnterPick));
    }

    #[test]
    fn picking_click_while_loading_goes_to_warming() {
        let (p, a) = step(&Picking { reading: false }, Ev::Click);
        assert_eq!(p, Warming);
        assert!(a.contains(&Act::StartRead) && a.contains(&Act::ShowWarming) && a.contains(&Act::ExitPick));
        let (p2, a2) = step(&p, Ev::FirstAudio);
        assert_eq!(p2, Reading);
        assert!(a2.contains(&Act::ShowReading));
    }

    #[test]
    fn rejected_click_stays_in_pick() {
        let (p, a) = step(&Picking { reading: false }, Ev::ClickRejected);
        assert_eq!(p, Picking { reading: false });
        assert!(a.is_empty());
    }

    #[test]
    fn esc_in_pick_returns_to_previous() {
        assert_eq!(step(&Picking { reading: false }, Ev::Esc).0, Idle);
        assert_eq!(step(&Picking { reading: true }, Ev::Esc).0, Reading);
    }

    #[test]
    fn jump_bumps_generation() {
        let (p, _) = step(&Reading, Ev::Hotkey);
        assert_eq!(p, Picking { reading: true });
        let (_, a) = step(&p, Ev::Click);
        assert!(a.contains(&Act::BumpGen));
    }

    #[test]
    fn play_pause_toggles() {
        assert_eq!(step(&Reading, Ev::PlayPause).0, Paused);
        assert_eq!(step(&Paused, Ev::PlayPause).0, Reading);
    }

    #[test]
    fn every_error_ends_idle() {
        for p in [Picking { reading: false }, Picking { reading: true }, Warming, Reading, Paused] {
            let (n, a) = step(&p, Ev::Error);
            assert_eq!(n, Idle, "{p:?}");
            assert!(a.contains(&Act::Teardown));
            let (n, a) = step(&p, Ev::Stop);
            assert_eq!(n, Idle);
            assert!(a.contains(&Act::Teardown));
        }
        assert_eq!(step(&Reading, Ev::End).0, Idle);
    }

    #[test]
    fn idle_ignores_controls() {
        for ev in [Ev::PlayPause, Ev::Esc, Ev::Click, Ev::End, Ev::FirstAudio] {
            assert_eq!(step(&Idle, ev).0, Idle);
        }
    }

    #[test]
    fn idle_stop_and_error_do_nothing() {
        assert!(step(&Idle, Ev::Stop).1.is_empty());
        assert!(step(&Idle, Ev::Error).1.is_empty());
    }

    #[test]
    fn esc_while_reading_resumes_and_shows_reading() {
        let (p, a) = step(&Picking { reading: true }, Ev::Esc);
        assert_eq!(p, Reading);
        assert_eq!(a, vec![Act::ExitPick, Act::ShowReading]);
        let (_, a) = step(&Picking { reading: false }, Ev::Esc);
        assert_eq!(a, vec![Act::ExitPick, Act::Teardown]);
    }

    #[test]
    fn hotkey_in_pick_is_ignored() {
        assert_eq!(step(&Picking { reading: true }, Ev::Hotkey), (Picking { reading: true }, vec![]));
    }
}

#[cfg(test)]
mod session_tests {
    use super::*;
    use crate::ra_timing::WordTime;

    const PER_WORD: u64 = 1000;

    /// `n` words, a sentence end every `every` words.
    fn words(n: usize, every: usize) -> Vec<Word> {
        (0..n)
            .map(|i| Word {
                text: if (i + 1) % every == 0 { format!("w{i}.") } else { format!("w{i}") },
                src: vec![i],
                para_break_before: false,
            })
            .collect()
    }

    fn audio(n_words: usize, covered: usize) -> ChunkAudio {
        ChunkAudio {
            pcm: vec![0.0; covered * PER_WORD as usize],
            words: (0..covered)
                .map(|w| WordTime { word: w, start: w as u64 * PER_WORD, end: (w as u64 + 1) * PER_WORD })
                .collect(),
            device: Device::Cpu,
            covered: covered.min(n_words),
        }
    }

    fn session(n: usize, every: usize) -> Session {
        let mut s = Session::new(Arc::new(AtomicU64::new(0)));
        s.start(words(n, every), n, true);
        s
    }

    #[test]
    fn chunks_use_first_12_then_30() {
        let s = session(100, 100);
        assert_eq!(s.chunks[0], 0..12);
        assert!(s.chunks[1..].iter().all(|r| r.len() <= 30));
        assert_eq!(s.chunks.last().map(|r| r.end), Some(100));
    }

    #[test]
    fn keeps_two_chunks_ahead() {
        let mut s = session(40, 5); // 8 sentences of 5
        let j = s.jobs(0, 2.2);
        assert_eq!(j.iter().map(|j| j.idx).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(j[0].words.len(), 5);
        assert!(s.jobs(0, 2.2).is_empty(), "2 in flight already");
        let g = s.gen();
        let (pcm, n) = s.on_chunk(g, 0, audio(5, 5));
        assert_eq!((pcm.len(), n), (1, 1));
        // chunk 0 pushed but unplayed + chunk 1 in flight = 2: nothing new
        assert!(s.jobs(0, 2.2).is_empty());
        // playhead passes chunk 0's end: one more
        assert_eq!(s.jobs(5 * PER_WORD, 2.2).iter().map(|j| j.idx).collect::<Vec<_>>(), vec![2]);
    }

    #[test]
    fn pushes_strictly_in_order() {
        let mut s = session(20, 5);
        s.jobs(0, 2.2);
        let g = s.gen();
        let (pcm, n) = s.on_chunk(g, 1, audio(5, 5));
        assert!(pcm.is_empty() && n == 0, "chunk 1 must wait for chunk 0");
        let (pcm, n) = s.on_chunk(g, 0, audio(5, 5));
        assert_eq!((pcm.len(), n), (2, 2));
        // timeline is global word indices, monotonic across chunks
        assert_eq!(s.word_at(0), Some(0));
        assert_eq!(s.word_at(5 * PER_WORD), Some(5));
        assert_eq!(s.word_at(10 * PER_WORD - 1), Some(9));
    }

    #[test]
    fn stale_results_after_a_jump_are_dropped() {
        let mut s = session(40, 5);
        s.jobs(0, 2.2);
        let old = s.gen();
        s.seek(4);
        assert_ne!(s.gen(), old);
        assert_eq!(s.on_chunk(old, 0, audio(5, 5)).1, 0);
        assert!(!s.on_error(old));
        let j = s.jobs(0, 2.2);
        assert_eq!(j[0].idx, 4);
        assert_eq!(j[0].gen, s.gen());
        let g = s.gen();
        s.on_chunk(g, 4, audio(5, 5));
        assert_eq!(s.word_at(0), Some(20), "global index after the jump");
    }

    #[test]
    fn partly_covered_chunk_requeues_its_tail_next() {
        let mut s = session(40, 40); // chunks 0..12, 12..40
        s.jobs(0, 2.2);
        let g = s.gen();
        let (_, n) = s.on_chunk(g, 0, audio(12, 7));
        assert_eq!(n, 1);
        assert_eq!(s.chunks[0], 0..7);
        assert_eq!(s.chunks[1], 7..12);
        assert_eq!(s.chunks[2], 12..40);
        assert_ne!(s.gen(), g, "jobs queued past the split are stale");
        assert_eq!(s.on_chunk(g, 1, audio(28, 28)).1, 0, "old chunk-1 result is dropped");
        let j = s.jobs(0, 2.2);
        assert_eq!(j[0].idx, 1);
        assert_eq!(j[0].words, vec!["w7", "w8", "w9", "w10", "w11"]);
        let g = s.gen();
        s.on_chunk(g, 1, audio(5, 5));
        assert_eq!(s.word_at(7 * PER_WORD), Some(7));
    }

    #[test]
    fn finished_only_after_everything_played() {
        let mut s = session(10, 5);
        let g = s.gen();
        for j in s.jobs(0, 2.2) {
            s.on_chunk(g, j.idx, audio(5, 5));
        }
        assert!(!s.finished(9 * PER_WORD));
        assert!(s.finished(10 * PER_WORD));
    }

    #[test]
    fn more_text_appends_with_offset_and_rechunks_tail() {
        let mut s = Session::new(Arc::new(AtomicU64::new(0)));
        let raw: Vec<String> = (0..20).map(|i| format!("a{i} ")).collect();
        s.start(ra_text::normalize(&raw), raw.len(), false);
        assert!(s.wants_more());
        s.jobs(0, 2.2); // submits 0..12 and 12..20
        let more: Vec<String> = (0..10).map(|i| format!("b{i} ")).collect();
        s.append_more(&more);
        assert_eq!(s.words.len(), 30);
        assert_eq!(s.words[20].src, vec![20], "source indices continue");
        assert_eq!(s.chunks[2], 20..30);
        assert!(!s.finished(u64::MAX));
        s.append_more(&[]);
        assert!(!s.wants_more(), "an empty batch ends the document");
    }

    #[test]
    fn chunk_lookup_and_skip_anchor() {
        let s = session(40, 5);
        assert_eq!(s.chunk_of(0), Some(0));
        assert_eq!(s.chunk_of(12), Some(2));
        assert_eq!(s.chunk_of(40), None);
        assert_eq!(s.current_chunk(Some(12)), 2);
        assert_eq!(s.current_chunk(None), 0);
    }

    #[test]
    fn speed_clamps_and_rounds() {
        assert_eq!(clamp_speed(0.2), 1.0);
        assert_eq!(clamp_speed(9.0), 3.5);
        assert_eq!(clamp_speed(2.24), 2.2);
        assert_eq!(clamp_speed(f32::NAN), 2.2);
    }

    /// Headless end-to-end: raw tokens → normalize → chunk → Kokoro (CPU) → real output
    /// device (muted unless RA_SMOKE_GAIN is set) → timeline. No windows, no UIA, no input.
    /// cargo test read_aloud::session_tests::pipeline_smoke_real -- --ignored --nocapture
    #[test]
    #[ignore = "manual: needs the Kokoro model and an audio output device"]
    fn pipeline_smoke_real() {
        let text = "Eight times a year, twelve people sit around a long table in Washington and decide \
            what your money costs. They do not vote on your rent, your car loan, or your credit card, \
            but every one of those prices moves within days of their meeting. Most people never watch \
            it happen. The room is quiet, the statement is short, and the market reacts in seconds.";
        let raw: Vec<String> = text.split_inclusive(' ').map(String::from).collect();
        let gain: f32 = std::env::var("RA_SMOKE_GAIN").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0);
        let rate = 2.2;
        let engine = Engine::new(dirs::data_dir().expect("data dir").join("ALAN Echo"));
        let t0 = Instant::now();
        engine.preload("af_heart", "cpu");
        let gen = Arc::new(AtomicU64::new(0));
        let mut s = Session::new(Arc::clone(&gen));
        s.start(ra_text::normalize(&raw), raw.len(), true);
        let n_words = s.words.len();
        let (tx, rx) = mpsc::channel::<Msg>();
        let synth = spawn_synth(Arc::clone(&engine), Arc::clone(&gen), tx).expect("synth worker");
        let player = Player::start().expect("player");
        let (mut first_audio, mut seen, mut audio_samples) = (None, Vec::<usize>::new(), 0u64);
        let deadline = t0 + Duration::from_secs(120);
        while !s.finished(player.position()) {
            assert!(Instant::now() < deadline, "timed out; saw words {:?}", seen.last());
            for j in s.jobs(player.position(), rate) {
                synth.send(j).expect("submit");
            }
            if let Ok(Msg::Chunk(g, idx, r)) = rx.recv_timeout(TICK) {
                let (pcm, n) = s.on_chunk(g, idx, r.expect("synth"));
                for buf in pcm {
                    audio_samples += buf.len() as u64;
                    let buf: Vec<f32> = buf.iter().map(|v| v * gain).collect();
                    for piece in buf.chunks(PUSH_PIECE) {
                        player.push(piece);
                    }
                }
                if n > 0 && first_audio.is_none() {
                    first_audio = Some(t0.elapsed());
                }
            }
            if let Some(w) = s.word_at(player.position()) {
                if seen.last() != Some(&w) {
                    seen.push(w);
                }
            }
            assert!(!player.failed(), "output device failed");
        }
        let total = t0.elapsed();
        let audio_secs = audio_samples as f64 / crate::ra_timing::SR as f64;
        println!(
            "words {n_words}, chunks {}, first audio {:?}, total {:?}, audio {:.2}s at {rate}x, words seen {} (first {:?}, last {:?})",
            s.chunks.len(),
            first_audio,
            total,
            audio_secs,
            seen.len(),
            seen.first(),
            seen.last()
        );
        assert!(seen.windows(2).all(|w| w[0] < w[1]), "word index must advance monotonically: {seen:?}");
        assert_eq!(seen.last(), Some(&(n_words - 1)), "timeline must reach the last word");
        assert!(seen.len() * 10 >= n_words * 8, "most words highlighted ({} of {n_words})", seen.len());
        let fa = first_audio.expect("audio").as_secs_f64();
        assert!(total.as_secs_f64() <= fa + audio_secs + 3.0, "playback stalled: total {total:?}");
    }
}
