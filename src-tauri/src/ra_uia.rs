//! Read Aloud — UI Automation text access.
//!
//! One worker thread ("ra-uia") owns every COM object; callers talk to it over
//! channels with short timeouts, so a hung target app can never block the UI.
//! Behaviour follows Spike A (spec section 5, rules 1-9). Never panics.
//!
//! The worker reads from one of three sources (phase 2 spec 3.2), chosen per pick:
//! UIA (TextPattern), Adobe Acrobat's own DOM (`ra_acro`), or Windows OCR of the
//! visible window (`ra_ocr`) for windows with no readable text.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::core::VARIANT;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    SAFEARRAY,
};
use windows::Win32::System::Ole::{
    SafeArrayDestroy, SafeArrayGetElement, SafeArrayGetLBound, SafeArrayGetUBound,
};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::HiDpi::{
    SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetClassNameW, GetWindowThreadProcessId, WindowFromPoint, GA_ROOT,
};

use crate::ra_acro::{self, AcroRead};
use crate::ra_ocr::{self, OcrRead};

/// A rectangle in physical screen pixels.
#[derive(Clone, Debug, serde::Serialize, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// The page's scroll offset in physical px (x, y) of the nearest scrollable ancestor of
/// the reading element (UIA ScrollPattern), so a page that grows (lazy content) does not
/// look like a scroll the way the bare percent would.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollPos {
    pub x: f64,
    pub y: f64,
    /// The raw vertical ScrollPattern percent (for the log; -1 = not scrollable).
    pub pct: f64,
    /// The scroller's viewport on screen: top (physical px) and height (px).
    pub top: f64,
    pub extent: f64,
    /// The vertical view size (percent of the content that is visible).
    pub view_pct: f64,
}

/// The vertical ScrollPattern percent that puts the offset at `offset` px: the inverse
/// of `scroll_offset`, clamped to 0..=100. `None` when the page cannot scroll.
pub fn scroll_percent_for(offset: f64, view: f64, extent: f64) -> Option<f64> {
    if !(view > 0.0 && view < 100.0 && extent > 0.0 && offset.is_finite()) {
        return None;
    }
    let range = (100.0 / view - 1.0) * extent;
    Some((offset / range * 100.0).clamp(0.0, 100.0))
}

/// The largest scroll offset (px) of a page with this view size and viewport extent.
pub fn max_offset(view: f64, extent: f64) -> f64 {
    if view > 0.0 && view < 100.0 && extent > 0.0 {
        (100.0 / view - 1.0) * extent
    } else {
        0.0
    }
}

/// Scroll offset in px from a ScrollPattern percent (0-100, or -1 = not scrollable), the
/// view size (percent of the content that is visible) and the viewport extent in px.
/// offset = pct/100 * (content - viewport), and content = viewport * 100 / view.
pub fn scroll_offset(pct: f64, view: f64, extent: f64) -> f64 {
    if !(pct >= 0.0 && view > 0.0 && view < 100.0 && extent > 0.0) {
        return 0.0;
    }
    pct / 100.0 * (100.0 / view - 1.0) * extent
}

/// One look at the reading (round 3 follow mode): the spoken word's rects, the
/// previously anchored word's rects measured again (when asked; `None` = no answer),
/// and the page's scroll offset (`None` = not known for this read).
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub rects: Vec<Rect>,
    pub prev: Option<Vec<Rect>>,
    pub scroll: Option<ScrollPos>,
}

/// Echo's own scroll, measured on the worker when it actually ran: the scroll offset
/// right before and after the call, and how long the request waited in the queue.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollRan {
    pub before: Option<ScrollPos>,
    pub after: Option<ScrollPos>,
    pub queued_ms: u64,
    /// The vertical offset change (px) the scroll asked for (`Uia::scroll_by_acked`);
    /// `None` for a scroll-into-view, whose size is not known.
    pub expected: Option<f64>,
}

/// Called on the UIA worker right after an own scroll ran (must only send to a channel).
pub type ScrollAck = Box<dyn FnOnce(ScrollRan) + Send>;

/// True when (x, y) lies inside any of the rects (edges inclusive).
pub fn contains(rects: &[Rect], x: f64, y: f64) -> bool {
    rects
        .iter()
        .any(|r| x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h)
}

enum Req {
    Hover(i32, i32, Sender<Vec<Rect>>),
    Pick(i32, i32, Sender<Result<Vec<String>, String>>),
    /// A plain click while reading: `Pick`, but only on ordinary text, and no retry.
    /// Non-committing: the read it finds is only staged under the id until a
    /// matching `CommitClick` (the controller accepted it); the current read stays.
    ClickPick(u64, i32, i32, Sender<Result<Vec<String>, String>>),
    CommitClick(u64),
    More(Sender<Result<Vec<String>, String>>),
    Rects(Vec<usize>, Sender<Option<Vec<Rect>>>),
    /// The spoken word, optionally the previous word, and the scroll offset (`Look`).
    Look(Vec<usize>, Option<Vec<usize>>, Sender<Option<Look>>),
    /// Scroll token `i` into view; the ack (with the time it was queued) runs after it.
    Scroll(usize, Option<(Instant, ScrollAck)>),
    /// Scroll the page by `dy` px (word `i` finds the scroller); acked like `Scroll`.
    ScrollBy(usize, f64, Instant, ScrollAck),
    /// Is the current read an OCR read that reached the end of what is on screen?
    AtEnd(Sender<bool>),
}

/// Handle to the UIA worker thread. Dropping it ends the worker.
pub struct Uia {
    tx: Sender<Req>,
    /// Top-level window of the current read (the warmed window until a pick succeeds).
    source: Arc<AtomicIsize>,
}

/// Raw tokens returned by `pick` (small, so speech can start fast; ~0.7 ms/word in Chromium).
const FIRST_BATCH: usize = 200;
/// Raw tokens per `more` call (keeps the worker free for `rects`/`hover` between batches).
const MORE_BATCH: usize = 400;
/// Max ancestors walked from the point element looking for a TextPattern.
const MAX_ANCESTORS: usize = 30;
/// Error text for "nothing with a TextPattern here" — the only pick error that is retried.
pub const NO_TEXT: &str = "no TextPattern under the point";
/// Error text for a plain click on a link, button, field, etc. (not a jump).
pub const NOT_TEXT_CLICK: &str = "click is on a control, not text";

/// Control types a plain click may land on (or sit inside) and still be a jump. Any
/// other type on the way from the clicked element up to its Document (a link, button,
/// edit field, check box, menu, tab, scroll bar, ...) means the click is for the app.
const TEXT_CONTROL_TYPES: &[i32] = &[
    50020, // Text
    50030, // Document
    50026, // Group (paragraphs, sections)
    50025, // Custom
    50033, // Pane
    50008, // List
    50007, // ListItem (a bullet point)
    50036, // Table
    50028, // DataGrid
    50029, // DataItem (table cell)
    50034, // Header (table header)
    50035, // HeaderItem
];
/// Document control type: the walk up from the clicked element stops here.
const DOCUMENT_TYPE: i32 = 50030;

/// App chrome: a point on (or just inside) one of these is for the app, so a UIA
/// `NO_TEXT` there is final and OCR is not tried (a click on Notepad's menu bar must not
/// read the menu). Content with no text (an image, a canvas pane) is OCR'd.
const APP_CHROME_TYPES: &[i32] = &[
    50000, // Button
    50002, // CheckBox
    50003, // ComboBox
    50004, // Edit
    50005, // Hyperlink
    50009, // Menu
    50010, // MenuBar
    50011, // MenuItem
    50012, // ProgressBar
    50013, // RadioButton
    50014, // ScrollBar
    50015, // Slider
    50016, // Spinner
    50017, // StatusBar
    50018, // Tab
    50019, // TabItem
    50021, // ToolBar
    50022, // ToolTip
    50023, // Tree
    50024, // TreeItem
    50027, // Thumb
    50031, // SplitButton
    50037, // TitleBar
    50040, // AppBar
];
/// How far up from the point element the chrome check looks: chrome sits right at the
/// point (Text in a MenuItem in a Menu), while an image's pane may live deep inside a
/// Tab's page, which must not count.
const CHROME_DEPTH: usize = 3;

/// May OCR read at a point whose UIA lookup found no text? `chain` = control types of
/// the element under the point and its nearest ancestors. Pure, so it is unit-tested.
pub(crate) fn ocr_ok_on(chain: &[i32]) -> bool {
    !chain.iter().any(|t| APP_CHROME_TYPES.contains(t))
}

/// A text source of the worker.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Src {
    Uia,
    Acro,
    Ocr,
}

/// Which sources a pick tries. `first_err` = `None` before anything ran (the whole
/// order), or the error of the first source (Acro on Acrobat, else UIA): the sources
/// still worth trying after it. OCR follows only "nothing readable here" (UIA
/// `NO_TEXT`; Acrobat: no words, or no DOM at all), never a timeout or a rejection
/// (rules 2/3, a control click). A Chromium window (Edge, Chrome) never falls through
/// to OCR: its tree may just not be ready yet (cold window), and an OCR read there has
/// no scroll and no follow; UIA's retry, then "Nothing readable here", is the answer.
/// Pure, so it is unit-tested.
pub(crate) fn choose_source(is_acrobat: bool, chromium: bool, first_err: Option<&str>) -> Vec<Src> {
    match (is_acrobat, first_err) {
        (true, None) => vec![Src::Acro, Src::Ocr],
        (false, None) if chromium => vec![Src::Uia],
        (false, None) => vec![Src::Uia, Src::Ocr],
        (true, Some(e)) if e == ra_acro::NO_TEXT_ACRO || e.starts_with(ra_acro::NO_DOM) => {
            vec![Src::Ocr]
        }
        (false, Some(e)) if e == NO_TEXT && !chromium => vec![Src::Ocr],
        _ => Vec::new(),
    }
}

/// Decide whether a plain click is on ordinary text. `chain` = control type ids of
/// the element under the point, then its ancestors, up to and including the first
/// Document (or as far as was walked). Pure, so it is unit-tested.
pub fn click_is_on_text(chain: &[i32]) -> bool {
    if chain.is_empty() {
        return false;
    }
    let upto = chain.iter().position(|&t| t == DOCUMENT_TYPE).map_or(chain.len(), |i| i + 1);
    chain[..upto].iter().all(|t| TEXT_CONTROL_TYPES.contains(t))
}

impl Uia {
    /// Spawn the worker and start warming the UIA connection to `hwnd` (the foreground
    /// window at hotkey time) now (rule 1). Non-blocking.
    pub fn start(hwnd: isize) -> Uia {
        let (tx, rx) = mpsc::channel();
        let source = Arc::new(AtomicIsize::new(hwnd));
        let src = source.clone();
        if let Err(e) = std::thread::Builder::new()
            .name("ra-uia".into())
            .spawn(move || worker(hwnd, src, rx))
        {
            log::warn!("read-aloud: could not spawn UIA worker: {e}");
        }
        Uia { tx, source }
    }

    /// Top-level window being read: the one under the last successful `pick`
    /// (before any pick: the window passed to `start`).
    pub fn source_hwnd(&self) -> isize {
        self.source.load(Ordering::Acquire)
    }

    /// Word rects under the point; empty when there is no word there (not a text
    /// element, our own window). `None` = no answer: the lookup took > 50 ms or was
    /// superseded by a newer hover (the caller keeps what it shows).
    pub fn hover(&self, x: i32, y: i32) -> Option<Vec<Rect>> {
        let (t, r) = mpsc::channel();
        self.tx.send(Req::Hover(x, y, t)).ok()?;
        r.recv_timeout(Duration::from_millis(50)).ok()
    }

    /// `hover` with a longer wait, so a bench sees the worker's real latency.
    #[cfg(test)]
    pub fn hover_wait(&self, x: i32, y: i32, wait: Duration) -> Option<Vec<Rect>> {
        let (t, r) = mpsc::channel();
        self.tx.send(Req::Hover(x, y, t)).ok()?;
        r.recv_timeout(wait).ok()
    }

    /// `rects` with a longer wait (bench only).
    #[cfg(test)]
    pub fn rects_wait(&self, src: Vec<usize>, wait: Duration) -> Option<Vec<Rect>> {
        let (t, r) = mpsc::channel();
        self.tx.send(Req::Rects(src, t)).ok()?;
        r.recv_timeout(wait).ok().flatten()
    }

    /// Validate the click (rules 2/3) on whatever window is under the point and return
    /// raw tokens from the clicked word forward (≤ 200; ≤ 60 from Acrobat). Token `i` ↔
    /// source index `i` for `rects`/`scroll_to`. The source (UIA, Acrobat, OCR) is chosen
    /// per window (`choose_source`). A failed pick keeps the previous read.
    pub fn pick(&self, x: i32, y: i32) -> Result<Vec<String>, String> {
        let (t, r) = mpsc::channel();
        self.tx
            .send(Req::Pick(x, y, t))
            .map_err(|_| "text reader is not running".to_string())?;
        Self::wait_pick(r)
    }

    /// A Send handle for click lookups, so the controller can run them on another
    /// thread (a hung target app then stalls only that thread).
    pub fn click_fetcher(&self) -> ClickFetcher {
        ClickFetcher { tx: self.tx.clone() }
    }

    fn wait_pick(r: Receiver<Result<Vec<String>, String>>) -> Result<Vec<String>, String> {
        match r.recv_timeout(Duration::from_secs(5)) {
            Ok(res) => res,
            Err(mpsc::RecvTimeoutError::Timeout) => Err("text lookup timed out".into()),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err("text reader stopped".into()),
        }
    }

    /// Next ≤ 400 raw tokens; source indices continue. Only `Ok(empty)` means the end
    /// of the document (a short batch does not); `Err` = timeout / UIA failure, retry later.
    #[cfg_attr(not(test), allow(dead_code))] // the controller uses more_fetcher()
    pub fn more(&self) -> Result<Vec<String>, String> {
        self.more_fetcher().more()
    }

    /// A Send handle for `more()`, so a caller can fetch from another thread (a hung
    /// target app then stalls only that thread).
    pub fn more_fetcher(&self) -> MoreFetcher {
        MoreFetcher { tx: self.tx.clone() }
    }

    /// Fresh screen rects for the given source indices (rule 9: never cached).
    /// `Some(empty)` = the word is off screen; `None` = no answer (timed out after
    /// 100 ms, superseded by a newer request, or a UIA error): not "off screen".
    pub fn rects(&self, src: Vec<usize>) -> Option<Vec<Rect>> {
        let (t, r) = mpsc::channel();
        self.tx.send(Req::Rects(src, t)).ok()?;
        r.recv_timeout(Duration::from_millis(100)).ok().flatten()
    }

    /// One look for follow mode (see `Look`): same contract and timeout as `rects` for
    /// the spoken word; `prev` is measured in the same round trip.
    pub fn look(&self, src: Vec<usize>, prev: Option<Vec<usize>>) -> Option<Look> {
        let (t, r) = mpsc::channel();
        self.tx.send(Req::Look(src, prev, t)).ok()?;
        r.recv_timeout(Duration::from_millis(100)).ok().flatten()
    }

    /// Scroll the source so token `src_idx` is visible (rule 4). Fire-and-forget.
    #[cfg_attr(not(test), allow(dead_code))] // the controller uses scroll_to_acked()
    pub fn scroll_to(&self, src_idx: usize) {
        let _ = self.tx.send(Req::Scroll(src_idx, None));
    }

    /// Scroll the page by `dy` px (positive = down) through the scroller of word
    /// `src_idx`'s element (`SetScrollPercent`), then `ack` with the delta asked for.
    pub fn scroll_by_acked(&self, src_idx: usize, dy: f64, ack: impl FnOnce(ScrollRan) + Send + 'static) {
        let _ = self.tx.send(Req::ScrollBy(src_idx, dy, Instant::now(), Box::new(ack)));
    }

    /// `scroll_to`, then `ack` on the worker once the scroll actually ran (it may wait
    /// behind a long `more()`), with the scroll offset before and after it.
    pub fn scroll_to_acked(&self, src_idx: usize, ack: impl FnOnce(ScrollRan) + Send + 'static) {
        let _ = self.tx.send(Req::Scroll(src_idx, Some((Instant::now(), Box::new(ack)))));
    }

    /// Switch the worker to the read staged by click lookup `id` (the controller
    /// accepted it). Requests sent after this one see the new read. Fire-and-forget.
    pub fn commit_click(&self, id: u64) {
        let _ = self.tx.send(Req::CommitClick(id));
    }

    /// True only when the current read is an OCR read and `more` already said "end":
    /// it read everything that was on screen (not the end of the document). False on
    /// no answer within 50 ms.
    pub fn at_end_of_screen(&self) -> bool {
        let (t, r) = mpsc::channel();
        if self.tx.send(Req::AtEnd(t)).is_err() {
            return false;
        }
        r.recv_timeout(Duration::from_millis(50)).unwrap_or(false)
    }
}

/// The staged click read, if it belongs to `id`. Anything staged is consumed either
/// way (a mismatched one is stale). Pure, so it is unit-tested.
fn take_staged<T>(staged: &mut Option<(u64, T)>, id: u64) -> Option<T> {
    staged.take().filter(|(k, _)| *k == id).map(|(_, v)| v)
}

/// See `Uia::click_fetcher`.
pub struct ClickFetcher {
    tx: Sender<Req>,
}

impl ClickFetcher {
    /// Like `Uia::pick`, for a plain (not swallowed) click while reading: rejected with
    /// `NOT_TEXT_CLICK` when it is on a link, button, field, menu, etc.; no retry.
    /// Does NOT switch the worker to the clicked text: that happens only on
    /// `Uia::commit_click(id)`, so a result the controller drops changes nothing.
    pub fn pick(&self, id: u64, x: i32, y: i32) -> Result<Vec<String>, String> {
        let (t, r) = mpsc::channel();
        self.tx
            .send(Req::ClickPick(id, x, y, t))
            .map_err(|_| "text reader is not running".to_string())?;
        Uia::wait_pick(r)
    }
}

/// See `Uia::more_fetcher`.
pub struct MoreFetcher {
    tx: Sender<Req>,
}

impl MoreFetcher {
    /// Same contract as `Uia::more`.
    pub fn more(&self) -> Result<Vec<String>, String> {
        let (t, r) = mpsc::channel();
        self.tx
            .send(Req::More(t))
            .map_err(|_| "text reader is not running".to_string())?;
        match r.recv_timeout(Duration::from_secs(5)) {
            Ok(res) => res,
            Err(mpsc::RecvTimeoutError::Timeout) => Err("text read timed out".into()),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err("text reader stopped".into()),
        }
    }
}

/// The window currently probed by hover/pick: the top-level window under the point.
struct Target {
    hwnd: isize,
    root: Option<IUIAutomationElement>,
    chromium: bool,
    /// Cached TextPattern elements of `root` (non-Chromium only), for apps whose point
    /// element has no TextPattern ancestor (Win11 Notepad: the point hits a WinUI
    /// `InputSiteWindowClass` pane that is a sibling of the RichEdit document), plus
    /// when it was built, so a miss refreshes at most once a second.
    fallback: Option<(Vec<IUIAutomationElement>, std::time::Instant)>,
}

/// A UIA read: everything from the one TextPattern picked at click time (rule 7).
#[derive(Default)]
struct UiaRead {
    hwnd: isize,
    chromium: bool,
    /// Word range per source index.
    ranges: Vec<IUIAutomationTextRange>,
    /// Last word range handed out; `more` walks forward from here (rule 8).
    cursor: Option<IUIAutomationTextRange>,
    /// The nearest scrollable ancestor of the reading element (ScrollPattern) and the
    /// cache request that reads its offset in one round trip; looked up once per read.
    scroller: Option<(IUIAutomationElement, IUIAutomationCacheRequest)>,
    scroller_looked: bool,
    /// When the scroller was last (re)resolved: a failed query re-resolves it at most
    /// once a second (Chrome rebuilds its tree on single-page apps).
    scroller_at: Option<Instant>,
}

impl UiaRead {
    /// The page's scroll offset; the scroller is found from word `at` on first use.
    fn scroll_pos(&mut self, uia: &IUIAutomation, at: Option<usize>) -> Option<ScrollPos> {
        let stale = self.scroller_at.is_none_or(|t| t.elapsed() >= Duration::from_secs(1));
        if !self.scroller_looked || (self.scroller.is_none() && stale) {
            let range = self.ranges.get(at?)?;
            let first = !self.scroller_looked;
            self.scroller_looked = true;
            self.scroller_at = Some(Instant::now());
            self.scroller = find_scroller(uia, range);
            if first {
                log::info!(
                    "read-aloud: scroll offset {}",
                    if self.scroller.is_some() { "from ScrollPattern" } else { "unknown (word-left-screen rule)" }
                );
            }
        }
        let (el, cr) = self.scroller.as_ref()?;
        let p = scroll_pos_of(el, cr);
        if p.is_none() {
            // the element went away (tree rebuilt): look it up again (rate limited)
            log::debug!("read-aloud: scroll offset query failed; re-resolving the scroller");
            self.scroller = None;
        }
        p
    }

    /// Scroll the page by `dy` px through the scroller's ScrollPattern (vertical only;
    /// the horizontal axis is left alone). Returns (before, after, the delta asked for
    /// after clamping to the page), or `None` without a scroller.
    fn scroll_by(&mut self, uia: &IUIAutomation, at: Option<usize>, dy: f64) -> Option<(ScrollPos, Option<ScrollPos>, f64)> {
        let before = self.scroll_pos(uia, at)?;
        let target = (before.y + dy).clamp(0.0, max_offset(before.view_pct, before.extent));
        let pct = scroll_percent_for(target, before.view_pct, before.extent)?;
        let (el, _) = self.scroller.as_ref()?;
        unsafe {
            let sp = el.GetCurrentPatternAs::<IUIAutomationScrollPattern>(UIA_ScrollPatternId).ok()?;
            sp.SetScrollPercent(UIA_ScrollPatternNoScroll, pct).ok()?;
        }
        let after = self.scroll_pos(uia, at);
        Some((before, after, target - before.y))
    }
}

/// The current read, from whichever source the pick chose. Moved as a whole (click
/// staging, rollback of a failed pick).
enum Read {
    Uia(UiaRead),
    Acro(AcroRead),
    Ocr(OcrRead),
}

impl Default for Read {
    fn default() -> Self {
        Read::Uia(UiaRead::default())
    }
}

/// Where a read's `more` was, so a batch the caller never got can be undone.
enum Mark {
    Uia(usize, Option<IUIAutomationTextRange>),
    Acro(ra_acro::AcroMark),
    Ocr(ra_ocr::OcrMark),
}

impl Read {
    /// Top-level window being read.
    fn hwnd(&self) -> isize {
        match self {
            Read::Uia(r) => r.hwnd,
            Read::Acro(r) => r.hwnd(),
            Read::Ocr(r) => r.hwnd(),
        }
    }

    /// Next ≤ 400 tokens (same contract as `Uia::more`).
    fn more(&mut self) -> Result<Vec<String>, String> {
        match self {
            Read::Uia(r) => read_forward(r, MORE_BATCH),
            Read::Acro(r) => r.more(),
            Read::Ocr(r) => r.more(),
        }
    }

    fn mark(&self) -> Mark {
        match self {
            Read::Uia(r) => Mark::Uia(r.ranges.len(), r.cursor.clone()),
            Read::Acro(r) => Mark::Acro(r.mark()),
            Read::Ocr(r) => Mark::Ocr(r.mark()),
        }
    }

    fn rollback(&mut self, m: Mark) {
        match (self, m) {
            (Read::Uia(r), Mark::Uia(len, cur)) => {
                r.ranges.truncate(len);
                r.cursor = cur;
            }
            (Read::Acro(r), Mark::Acro(m)) => r.rollback(m),
            (Read::Ocr(r), Mark::Ocr(m)) => r.rollback(m),
            _ => {} // a mark of another read: nothing of it to undo
        }
    }

    /// Same contract as `Uia::rects`.
    fn rects(&self, src: &[usize]) -> Option<Vec<Rect>> {
        match self {
            Read::Uia(r) => word_rects(r, src),
            Read::Acro(r) => r.rects(src),
            Read::Ocr(r) => r.rects(src),
        }
    }

    /// Same contract as `Uia::look`.
    fn look(&mut self, uia: &IUIAutomation, src: &[usize], prev: Option<&[usize]>) -> Option<Look> {
        let rects = self.rects(src)?;
        let prev = prev.and_then(|p| self.rects(p));
        let scroll = self.scroll_pos(uia, src.first().copied());
        Some(Look { rects, prev, scroll })
    }

    fn scroll_pos(&mut self, uia: &IUIAutomation, at: Option<usize>) -> Option<ScrollPos> {
        match self {
            Read::Uia(r) => r.scroll_pos(uia, at),
            Read::Acro(_) | Read::Ocr(_) => None,
        }
    }

    fn scroll_to(&self, uia: &IUIAutomation, i: usize) {
        match self {
            Read::Uia(r) => {
                if let Some(range) = r.ranges.get(i) {
                    scroll(uia, r, range);
                }
            }
            Read::Acro(r) => r.scroll_to(i),
            Read::Ocr(r) => r.scroll_to(i),
        }
    }

    /// An OCR read that already returned its last batch.
    fn at_end_of_screen(&self) -> bool {
        matches!(self, Read::Ocr(r) if r.at_end())
    }
}

/// Answer a `More`: the next batch, rolled back when it failed or the caller is gone
/// (it timed out), so source indices stay aligned with the tokens the caller has.
fn serve_more(read: &mut Read, send: impl FnOnce(Result<Vec<String>, String>) -> bool) {
    let m = read.mark();
    let res = read.more();
    let failed = res.is_err();
    if !send(res) || failed {
        read.rollback(m);
    }
}

struct State {
    uia: IUIAutomation,
    own_pid: u32,
    target: Target,
    read: Read,
    /// A click lookup's read, waiting for the controller's `CommitClick`.
    staged: Option<(u64, Read)>,
    source: Arc<AtomicIsize>,
    /// Pick-mode hover caches: the OCR of the hovered window, and the DOM root of the
    /// hovered Acrobat page view. Dropped when a pick succeeds and when reading needs
    /// the highlight (`Rects`); the OCR one also when the hover moves to another window.
    ocr_hover: Option<OcrRead>,
    acro_hover: Option<ra_acro::HoverView>,
}

impl State {
    fn drop_hover_caches(&mut self) {
        self.ocr_hover = None;
        self.acro_hover = None;
    }
}

fn worker(hwnd: isize, source: Arc<AtomicIsize>, rx: Receiver<Req>) {
    unsafe {
        // Physical pixels regardless of the process manifest; the LL mouse hook
        // also reports per-monitor-aware coordinates.
        let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let com_ok = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    run(hwnd, source, rx);
    if com_ok {
        unsafe { CoUninitialize() };
    }
}

fn is_chromium(h: HWND) -> bool {
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(h, &mut buf) }.clamp(0, buf.len() as i32) as usize;
    String::from_utf16_lossy(&buf[..n]).starts_with("Chrome_WidgetWin")
}

fn make_target(uia: &IUIAutomation, hwnd: isize) -> Target {
    let h = HWND(hwnd as *mut _);
    Target {
        hwnd,
        root: unsafe { uia.ElementFromHandle(h) }.ok(),
        chromium: is_chromium(h),
        fallback: None,
    }
}

fn run(hwnd: isize, source: Arc<AtomicIsize>, rx: Receiver<Req>) {
    let uia: IUIAutomation =
        match unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) } {
            Ok(u) => u,
            Err(e) => {
                log::warn!("read-aloud: UIA init failed: {e}");
                return; // dropping rx makes every call fail fast
            }
        };
    let target = make_target(&uia, hwnd);
    warm_up(&uia, &target);
    let mut st = State {
        uia,
        own_pid: unsafe { GetCurrentProcessId() },
        target,
        read: Read::Uia(UiaRead {
            hwnd,
            ..Default::default()
        }),
        staged: None,
        source,
        ocr_hover: None,
        acro_hover: None,
    };

    let mut pending: VecDeque<Req> = VecDeque::new();
    loop {
        let req = match pending.pop_front() {
            Some(r) => r,
            None => match rx.recv() {
                Ok(r) => r,
                Err(_) => break,
            },
        };
        while let Ok(r) = rx.try_recv() {
            pending.push_back(r);
        }
        match req {
            Req::Hover(x, y, t) => {
                // Coalesce: a newer hover is queued, so this one is stale. Dropping `t`
                // makes the (already timed-out) caller see `None`.
                if superseded(&pending, |r| matches!(r, Req::Hover(..))) {
                    continue;
                }
                let _ = t.send(hover(&mut st, x, y));
            }
            Req::Pick(x, y, t) => {
                st.staged = None;
                pick_req(&mut st, x, y, t);
            }
            Req::ClickPick(id, x, y, t) => {
                // pick into st.read, then put the current read back: the new one is staged
                let ok = click_on_text(&mut st, x, y);
                let saved = std::mem::take(&mut st.read);
                let res = ok.and_then(|()| pick(&mut st, x, y, false));
                let found = std::mem::replace(&mut st.read, saved);
                st.staged = None;
                if res.is_ok() {
                    if t.send(res).is_ok() {
                        st.staged = Some((id, found));
                    }
                } else {
                    let _ = t.send(res);
                }
            }
            Req::CommitClick(id) => {
                if let Some(r) = take_staged(&mut st.staged, id) {
                    st.read = r;
                    st.source.store(st.read.hwnd(), Ordering::Release);
                }
            }
            Req::More(t) => serve_more(&mut st.read, |res| t.send(res).is_ok()),
            Req::Rects(src, t) => {
                // reading (the highlight): pick mode's hover caches are not needed now
                st.drop_hover_caches();
                // Coalesce like hovers: only the newest request is answered.
                if superseded(&pending, |r| matches!(r, Req::Rects(..) | Req::Look(..))) {
                    let _ = t.send(None);
                    continue;
                }
                let _ = t.send(st.read.rects(&src));
            }
            Req::Look(src, prev, t) => {
                st.drop_hover_caches();
                if superseded(&pending, |r| matches!(r, Req::Rects(..) | Req::Look(..))) {
                    let _ = t.send(None);
                    continue;
                }
                let _ = t.send(st.read.look(&st.uia, &src, prev.as_deref()));
            }
            Req::Scroll(i, ack) => {
                let before = if ack.is_some() { st.read.scroll_pos(&st.uia, Some(i)) } else { None };
                st.read.scroll_to(&st.uia, i);
                if let Some((queued, f)) = ack {
                    let after = st.read.scroll_pos(&st.uia, Some(i));
                    f(ScrollRan { before, after, queued_ms: queued.elapsed().as_millis() as u64, expected: None });
                }
            }
            Req::ScrollBy(i, dy, queued, f) => {
                let queued_ms = queued.elapsed().as_millis() as u64;
                let r = match &mut st.read {
                    Read::Uia(r) => r.scroll_by(&st.uia, Some(i), dy),
                    Read::Acro(_) | Read::Ocr(_) => None,
                };
                f(match r {
                    Some((before, after, expected)) => ScrollRan { before: Some(before), after, queued_ms, expected: Some(expected) },
                    None => ScrollRan { before: None, after: None, queued_ms, expected: None },
                });
            }
            Req::AtEnd(t) => {
                let _ = t.send(st.read.at_end_of_screen());
            }
        }
    }
}

/// Run a pick (a plain click when `click`). If the caller already timed out (reply
/// undeliverable), roll the read back so source indices stay aligned with the tokens
/// the caller actually has; a rejected pick keeps the old read too.
fn pick_req(st: &mut State, x: i32, y: i32, t: Sender<Result<Vec<String>, String>>) {
    let saved = std::mem::take(&mut st.read);
    let res = pick(st, x, y, true);
    let failed = res.is_err();
    if t.send(res).is_err() || failed {
        st.read = saved;
    } else {
        st.drop_hover_caches(); // pick mode is over
    }
    st.source.store(st.read.hwnd(), Ordering::Release);
}

/// Err(NOT_TEXT_CLICK) unless the element under the point (and every ancestor up to
/// its Document) is plain text: a click on a link, button or field is for the app.
/// Acrobat pages and the window being read by OCR have no control tree to ask: a
/// click there is always a jump.
fn click_on_text(st: &mut State, x: i32, y: i32) -> Result<(), String> {
    let root = own_root(st, x, y)?;
    if acrobat_at(x, y) || matches!(&st.read, Read::Ocr(r) if r.hwnd() == root) {
        return Ok(());
    }
    retarget(st, x, y)?;
    if click_is_on_text(&control_chain(st, x, y, MAX_ANCESTORS, true)?) {
        Ok(())
    } else {
        Err(NOT_TEXT_CLICK.into())
    }
}

/// Control types of the element under the point and up to `depth - 1` ancestors
/// (stopping after the first Document when `to_document`).
fn control_chain(st: &State, x: i32, y: i32, depth: usize, to_document: bool) -> Result<Vec<i32>, String> {
    let el = unsafe { st.uia.ElementFromPoint(POINT { x, y }) }.map_err(|e| e.to_string())?;
    let walker = unsafe { st.uia.ControlViewWalker() }.map_err(|e| e.to_string())?;
    let mut chain = Vec::new();
    let mut cur = Some(el);
    for _ in 0..depth {
        let Some(e) = cur else { break };
        let t = unsafe { e.CurrentControlType() }.map(|t| t.0).unwrap_or(0);
        chain.push(t);
        if to_document && t == DOCUMENT_TYPE {
            break;
        }
        cur = unsafe { walker.GetParentElement(&e) }.ok();
    }
    Ok(chain)
}

/// After a UIA `NO_TEXT`: is the point on content (OCR it) rather than app chrome?
fn ocr_ok_at(st: &State, x: i32, y: i32) -> bool {
    control_chain(st, x, y, CHROME_DEPTH, false).map_or(true, |c| ocr_ok_on(&c))
}

/// The window under the point is an Acrobat page view.
fn acrobat_at(x: i32, y: i32) -> bool {
    let w = unsafe { WindowFromPoint(POINT { x, y }) };
    !w.is_invalid() && ra_acro::is_acrobat_page(w.0 as isize)
}

/// Top-level window under the point, unless it is one of ours or there is none.
fn own_root(st: &State, x: i32, y: i32) -> Result<isize, String> {
    let root = root_at(x, y).ok_or("no window under the point")?;
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(root, Some(&mut pid)) };
    if pid == st.own_pid {
        return Err("point is over our own window".into());
    }
    Ok(root.0 as isize)
}

/// Hover in pick mode, through the same source switch as `pick`. Always an answer:
/// empty = no word box here (the overlay then clears any box from another window).
fn hover(st: &mut State, x: i32, y: i32) -> Vec<Rect> {
    let Ok(root) = own_root(st, x, y) else {
        return Vec::new();
    };
    // the OCR cache belongs to one window
    if st.ocr_hover.as_ref().is_some_and(|c| c.hwnd() != root) {
        st.ocr_hover = None;
    }
    if acrobat_at(x, y) {
        return match ra_acro::hover_cached(&mut st.acro_hover, x, y) {
            ra_acro::Hover::Rects(v) => v,
            ra_acro::Hover::NoAnswer => Vec::new(),
            // Acrobat withholds its DOM: the page is still readable by OCR
            ra_acro::Hover::NoDom => ra_ocr::hover_cached(&mut st.ocr_hover, root, x, y).unwrap_or_default(),
        };
    }
    match word_at(st, x, y) {
        Ok((_, r)) => r,
        // Chromium: no OCR (see `choose_source`), so no OCR hover box either
        Err(e) if e == NO_TEXT && !st.target.chromium && ocr_ok_at(st, x, y) => {
            ra_ocr::hover_cached(&mut st.ocr_hover, root, x, y).unwrap_or_default()
        }
        Err(_) => Vec::new(),
    }
}

/// A newer request of the same kind is queued, so this one is stale.
fn superseded(pending: &VecDeque<Req>, same_kind: impl Fn(&Req) -> bool) -> bool {
    pending.iter().any(same_kind)
}

/// Rects of the words at source indices `src`; `None` if a range is unknown or UIA
/// errors (so the caller never mistakes a failure for "off screen").
fn word_rects(read: &UiaRead, src: &[usize]) -> Option<Vec<Rect>> {
    let mut out = Vec::new();
    for &i in src {
        out.extend(rects_of(read.ranges.get(i)?)?);
    }
    Some(out)
}

/// Rule 1: touch the window's tree so Chromium builds its accessibility tree now
/// (first contact costs ~1.8 s), not when the user clicks.
fn warm_up(uia: &IUIAutomation, t: &Target) {
    let start = std::time::Instant::now();
    let Some(root) = t.root.as_ref() else { return };
    unsafe {
        let _ = root.CurrentName();
        if let Ok(c) = text_pattern_condition(uia) {
            let _ = root.FindFirst(TreeScope_Descendants, &c);
        }
    }
    log::info!("read-aloud: UIA warm-up {} ms", start.elapsed().as_millis());
}

fn text_pattern_condition(uia: &IUIAutomation) -> windows::core::Result<IUIAutomationCondition> {
    unsafe {
        uia.CreatePropertyCondition(UIA_IsTextPatternAvailablePropertyId, &VARIANT::from(true))
    }
}

/// Point the probe at the top-level window under (x, y). Rejects our own windows
/// (overlay, panels) and empty desktop space. Re-targets when the window changed.
fn retarget(st: &mut State, x: i32, y: i32) -> Result<(), String> {
    let h = own_root(st, x, y)?;
    if h != st.target.hwnd {
        st.target = make_target(&st.uia, h);
    }
    Ok(())
}

fn root_at(x: i32, y: i32) -> Option<HWND> {
    let w = unsafe { WindowFromPoint(POINT { x, y }) };
    if w.is_invalid() {
        return None;
    }
    let root = unsafe { GetAncestor(w, GA_ROOT) };
    (!root.is_invalid()).then_some(root)
}

/// Cap on cached fallback candidates.
const MAX_FALLBACK: i32 = 200;

/// Smallest TextPattern element of the target window whose rect contains the point.
/// Only for non-Chromium targets, and only while that window is the one actually under
/// the point (so text hidden under a covering window is never read).
fn fallback_at(st: &mut State, x: i32, y: i32) -> Option<(IUIAutomationTextPattern, Rect)> {
    if st.target.chromium || root_at(x, y).map(|h| h.0 as isize) != Some(st.target.hwnd) {
        return None;
    }
    for refresh in [false, true] {
        let stale = st
            .target
            .fallback
            .as_ref()
            .is_none_or(|(_, at)| at.elapsed() > Duration::from_secs(1));
        if refresh && !stale {
            return None;
        }
        if refresh || st.target.fallback.is_none() {
            let root = st.target.root.as_ref()?;
            let cond = text_pattern_condition(&st.uia).ok()?;
            let all = unsafe { root.FindAll(TreeScope_Descendants, &cond) }.ok()?;
            let n = unsafe { all.Length() }.unwrap_or(0).min(MAX_FALLBACK);
            let els = (0..n)
                .filter_map(|i| unsafe { all.GetElement(i) }.ok())
                .collect();
            st.target.fallback = Some((els, std::time::Instant::now()));
        }
        let best = st
            .target
            .fallback
            .as_ref()?
            .0
            .iter()
            .filter_map(|e| {
                // Hidden tabs (Notepad keeps one editor per tab, all with the same
                // rect) report IsOffscreen; only the visible one counts.
                if unsafe { e.CurrentIsOffscreen() }
                    .map(|b| b.as_bool())
                    .unwrap_or(false)
                {
                    return None;
                }
                let r = to_rect(unsafe { e.CurrentBoundingRectangle() }.ok()?);
                contains(std::slice::from_ref(&r), x as f64, y as f64).then_some((e, r))
            })
            .min_by(|a, b| (a.1.w * a.1.h).total_cmp(&(b.1.w * b.1.h)));
        if let Some((e, r)) = best {
            if let Ok(tp) =
                unsafe { e.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) }
            {
                return Some((tp, r));
            }
        }
    }
    None
}

fn to_rect(b: windows::Win32::Foundation::RECT) -> Rect {
    Rect {
        x: b.left as f64,
        y: b.top as f64,
        w: (b.right - b.left) as f64,
        h: (b.bottom - b.top) as f64,
    }
}

/// Nearest element at/above the point that has a TextPattern, plus its bounding rect;
/// falls back to a TextPattern element of the target window that contains the point.
fn text_pattern_at(
    st: &mut State,
    x: i32,
    y: i32,
) -> Result<(IUIAutomationTextPattern, Rect), String> {
    retarget(st, x, y)?;
    let mut el = unsafe { st.uia.ElementFromPoint(POINT { x, y }) }.map_err(|e| e.to_string())?;
    let walker = unsafe { st.uia.ControlViewWalker() }.map_err(|e| e.to_string())?;
    for _ in 0..MAX_ANCESTORS {
        if let Ok(tp) =
            unsafe { el.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) }
        {
            // Rule 3 (browser chrome): Chromium's omnibox also has a TextPattern; only
            // text inside the page's Document counts.
            if st.target.chromium && !in_document(&walker, &el) {
                return Err("browser UI, not page text".into());
            }
            let b = unsafe { el.CurrentBoundingRectangle() }.map_err(|e| e.to_string())?;
            return Ok((tp, to_rect(b)));
        }
        match unsafe { walker.GetParentElement(&el) } {
            Ok(p) => el = p,
            Err(_) => break,
        }
    }
    fallback_at(st, x, y).ok_or_else(|| NO_TEXT.into())
}

/// True when `el` is, or sits inside, a Document control (web page content).
fn in_document(walker: &IUIAutomationTreeWalker, el: &IUIAutomationElement) -> bool {
    let mut cur = el.clone();
    for _ in 0..MAX_ANCESTORS {
        if unsafe { cur.CurrentControlType() }
            .map(|t| t == UIA_DocumentControlTypeId)
            .unwrap_or(false)
        {
            return true;
        }
        match unsafe { walker.GetParentElement(&cur) } {
            Ok(p) => cur = p,
            Err(_) => return false,
        }
    }
    false
}

/// The validated word range under the point and its rects.
fn word_at(st: &mut State, x: i32, y: i32) -> Result<(IUIAutomationTextRange, Vec<Rect>), String> {
    let (tp, elem) = text_pattern_at(st, x, y)?;
    // Rule 3: ignore points outside the text element (browser chrome).
    if !contains(std::slice::from_ref(&elem), x as f64, y as f64) {
        return Err("outside the text element".into());
    }
    let r = unsafe { tp.RangeFromPoint(POINT { x, y }) }.map_err(|e| e.to_string())?;
    unsafe { r.ExpandToEnclosingUnit(TextUnit_Word) }.map_err(|e| e.to_string())?;
    let rects = rects_of(&r).unwrap_or_default();
    // Rule 2: UIA snaps to the nearest word; accept only a click on the word itself.
    if !contains(&rects, x as f64, y as f64) {
        return Err("click is not on a word".into());
    }
    Ok((r, rects))
}

/// Starts a new read in `st.read` (the caller saved and cleared the old one), from the
/// source `choose_source` picks for the window under the point (spec 3.2).
fn pick(st: &mut State, x: i32, y: i32, retry: bool) -> Result<Vec<String>, String> {
    let root = own_root(st, x, y)?;
    let acro = acrobat_at(x, y);
    let chromium = !acro && is_chromium(HWND(root as *mut _));
    let t0 = std::time::Instant::now();
    let mut err = String::new();
    let mut last = None;
    for (i, src) in choose_source(acro, chromium, None).into_iter().enumerate() {
        last = Some(src);
        if i > 0 {
            // fall through only after "nothing readable" (not a timeout or a rejection),
            // and not onto app chrome (menus, tabs, buttons) of a UIA window
            if !choose_source(acro, chromium, Some(&err)).contains(&src) || (!acro && !ocr_ok_at(st, x, y)) {
                return Err(err);
            }
            log::info!("read-aloud: {err}; trying {src:?}");
        }
        let res = match src {
            Src::Uia => pick_uia(st, x, y, retry),
            Src::Acro => AcroRead::pick(x, y).map(|(r, t)| (Read::Acro(r), t)),
            Src::Ocr => OcrRead::pick(root, x, y).map(|(r, t)| (Read::Ocr(r), t)),
        };
        match res {
            Ok((read, toks)) => {
                log::info!(
                    "read-aloud: pick from {src:?}: {} tokens in {} ms",
                    toks.len(),
                    t0.elapsed().as_millis()
                );
                st.read = read;
                return Ok(toks);
            }
            Err(e) => err = e,
        }
    }
    if last != Some(Src::Ocr) {
        // Chromium: UIA was the only source; its error stands (NO_TEXT = nothing readable)
        return Err(err);
    }
    // OCR (the last source) failed: whatever it hit, nothing is readable here
    log::info!("read-aloud: OCR: {err}");
    Err(NO_TEXT.into())
}

/// The UIA source: the word under the point and up to 199 after it.
fn pick_uia(st: &mut State, x: i32, y: i32, retry: bool) -> Result<(Read, Vec<String>), String> {
    // Rule 1: Chromium's first query can come back empty — retry once, but only for
    // "nothing there"; rule 2/3 rejections are final.
    let (r, _) = match word_at(st, x, y) {
        Ok(v) => v,
        Err(e) if e == NO_TEXT && retry => {
            std::thread::sleep(Duration::from_millis(150));
            word_at(st, x, y)?
        }
        Err(e) => return Err(e),
    };
    // Rule 7: everything for this read comes from this one range's source.
    let first = unsafe { r.GetText(-1) }
        .map(|b| b.to_string())
        .unwrap_or_default();
    let mut read = UiaRead {
        hwnd: st.target.hwnd,
        chromium: st.target.chromium,
        ranges: vec![r.clone()],
        cursor: Some(r),
        ..Default::default()
    };
    let mut out = Vec::with_capacity(FIRST_BATCH);
    out.push(first);
    // a failed forward read keeps the clicked word; more() continues from the cursor
    out.extend(read_forward(&mut read, FIRST_BATCH - 1).unwrap_or_default());
    Ok((Read::Uia(read), out))
}

/// Rule 8: walk forward word by word from the cursor; never re-query visible ranges.
/// `Ok(empty)` only at the true end of the text. A UIA error before any token is read
/// is `Err` (retryable); after some tokens it ends the batch early (`Ok(partial)`).
fn read_forward(read: &mut UiaRead, n: usize) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let Some(cur) = read.cursor.as_ref() else {
        return Err("no current read".into());
    };
    let r = unsafe { cur.Clone() }.map_err(|e| format!("clone range: {e}"))?;
    for _ in 0..n {
        let step = unsafe { r.Move(TextUnit_Word, 1) }
            .map_err(|e| format!("move: {e}"))
            .and_then(|moved| {
                if moved == 0 {
                    return Ok(None); // true end of the text
                }
                unsafe { r.ExpandToEnclosingUnit(TextUnit_Word) }.map_err(|e| format!("expand: {e}"))?;
                unsafe { r.Clone() }.map(Some).map_err(|e| format!("clone word: {e}"))
            });
        let keep = match step {
            Ok(Some(k)) => k,
            Ok(None) => break,
            Err(e) if out.is_empty() => return Err(e),
            Err(_) => break,
        };
        let text = unsafe { keep.GetText(-1) }
            .map(|b| b.to_string())
            .unwrap_or_default();
        read.ranges.push(keep.clone());
        read.cursor = Some(keep);
        out.push(text);
    }
    Ok(out)
}

/// Bounding rects of a range (empty when off-screen; `None` on a UIA error).
/// Never cached (rule 9).
fn rects_of(r: &IUIAutomationTextRange) -> Option<Vec<Rect>> {
    let sa = unsafe { r.GetBoundingRectangles() }.ok()?;
    if sa.is_null() {
        return Some(Vec::new());
    }
    let nums = unsafe { read_f64_array(sa) };
    unsafe {
        let _ = SafeArrayDestroy(sa);
    }
    Some(
        nums.chunks_exact(4)
            .filter(|c| c[2] > 0.0 && c[3] > 0.0)
            .map(|c| Rect {
                x: c[0],
                y: c[1],
                w: c[2],
                h: c[3],
            })
            .collect(),
    )
}

/// Read a 1-D SAFEARRAY of VT_R8. Caller still owns (and destroys) `sa`.
unsafe fn read_f64_array(sa: *mut SAFEARRAY) -> Vec<f64> {
    let mut nums = Vec::new();
    let (Ok(lo), Ok(hi)) = (SafeArrayGetLBound(sa, 1), SafeArrayGetUBound(sa, 1)) else {
        return nums;
    };
    let mut i = lo;
    while i <= hi {
        let mut d = 0f64;
        if SafeArrayGetElement(sa, &i, &mut d as *mut f64 as *mut _).is_ok() {
            nums.push(d);
        }
        i += 1;
    }
    nums
}

fn scroll(uia: &IUIAutomation, read: &UiaRead, r: &IUIAutomationTextRange) {
    unsafe {
        if read.chromium {
            // Rule 4: Chromium ignores word-range ScrollIntoView. Measured on Edge 2026-10-02:
            // the paragraph *range* ScrollIntoView was also a no-op, while ScrollItemPattern on
            // the word's enclosing element (the <p>) scrolled it into view. Use that first and
            // keep the paragraph range as the fallback.
            if scroll_item(uia, r) {
                return;
            }
            if let Ok(p) = r.Clone() {
                if p.ExpandToEnclosingUnit(TextUnit_Paragraph).is_ok() {
                    let _ = p.ScrollIntoView(true);
                }
            }
        } else {
            let _ = r.ScrollIntoView(true);
        }
    }
}

/// One element on the walk up from the reading element (`choose_scroller`).
#[derive(Clone, Copy, Debug)]
struct Ancestor {
    document: bool,
    pattern: bool,
    vertical: bool,
}

/// Which ancestor's offset to read: the nearest that scrolls vertically (a horizontal
/// carousel or code block near the word is not the page), else the Document itself.
/// The walk stops at the Document (never browser UI). Pure.
fn choose_scroller(chain: &[Ancestor]) -> Option<usize> {
    for (k, a) in chain.iter().enumerate() {
        if a.pattern && a.vertical {
            return Some(k);
        }
        if a.document {
            return a.pattern.then_some(k);
        }
    }
    None
}

/// The scroller of the range's element (`choose_scroller`), with a cache request for
/// its offset properties.
fn find_scroller(
    uia: &IUIAutomation,
    r: &IUIAutomationTextRange,
) -> Option<(IUIAutomationElement, IUIAutomationCacheRequest)> {
    unsafe {
        let walker = uia.RawViewWalker().ok()?;
        let mut el = r.GetEnclosingElement().ok();
        let mut chain = Vec::new();
        let mut elems = Vec::new();
        for _ in 0..MAX_ANCESTORS {
            let Some(e) = el else { break };
            let sp = e.GetCurrentPatternAs::<IUIAutomationScrollPattern>(UIA_ScrollPatternId).ok();
            let a = Ancestor {
                document: e.CurrentControlType().is_ok_and(|t| t == UIA_DocumentControlTypeId),
                pattern: sp.is_some(),
                vertical: sp.as_ref().is_some_and(|p| p.CurrentVerticallyScrollable().is_ok_and(|b| b.as_bool())),
            };
            chain.push(a);
            elems.push(e.clone());
            if (a.pattern && a.vertical) || a.document {
                break;
            }
            el = walker.GetParentElement(&e).ok();
        }
        let e = elems.get(choose_scroller(&chain)?)?.clone();
        let cr = uia.CreateCacheRequest().ok()?;
        for id in [
            UIA_ScrollVerticalScrollPercentPropertyId,
            UIA_ScrollVerticalViewSizePropertyId,
            UIA_ScrollHorizontalScrollPercentPropertyId,
            UIA_ScrollHorizontalViewSizePropertyId,
            UIA_BoundingRectanglePropertyId,
        ] {
            cr.AddProperty(id).ok()?;
        }
        Some((e, cr))
    }
}

/// The scroller's offset in px, read in one cross-process round trip.
fn scroll_pos_of(el: &IUIAutomationElement, cr: &IUIAutomationCacheRequest) -> Option<ScrollPos> {
    unsafe {
        let e = el.BuildUpdatedCache(cr).ok()?;
        let get = |id| e.GetCachedPropertyValue(id).ok().and_then(|v| f64::try_from(&v).ok());
        let vp = get(UIA_ScrollVerticalScrollPercentPropertyId)?;
        let vv = get(UIA_ScrollVerticalViewSizePropertyId)?;
        let hp = get(UIA_ScrollHorizontalScrollPercentPropertyId)?;
        let hv = get(UIA_ScrollHorizontalViewSizePropertyId)?;
        let b = e.CachedBoundingRectangle().ok()?;
        Some(ScrollPos {
            x: scroll_offset(hp, hv, (b.right - b.left) as f64),
            y: scroll_offset(vp, vv, (b.bottom - b.top) as f64),
            pct: vp,
            top: b.top as f64,
            extent: (b.bottom - b.top) as f64,
            view_pct: vv,
        })
    }
}

/// ScrollItemPattern on the range's enclosing element or its nearest ancestor that has one.
fn scroll_item(uia: &IUIAutomation, r: &IUIAutomationTextRange) -> bool {
    let Ok(walker) = (unsafe { uia.ControlViewWalker() }) else {
        return false;
    };
    let mut el = unsafe { r.GetEnclosingElement() }.ok();
    for _ in 0..6 {
        let Some(e) = el else { return false };
        if unsafe { e.CurrentControlType() }
            .map(|t| t == UIA_DocumentControlTypeId)
            .unwrap_or(false)
        {
            return false; // reached the page itself: scrolling it "into view" does nothing useful
        }
        if let Ok(si) = unsafe {
            e.GetCurrentPatternAs::<IUIAutomationScrollItemPattern>(UIA_ScrollItemPatternId)
        } {
            return unsafe { si.ScrollIntoView() }.is_ok();
        }
        el = unsafe { walker.GetParentElement(&e) }.ok();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scroller_is_the_nearest_vertical_one_or_the_document() {
        // (is document, has ScrollPattern, vertically scrollable) from the word upward
        let v = |d, p, vs| Ancestor { document: d, pattern: p, vertical: vs };
        // a horizontal-only carousel near the word is skipped
        assert_eq!(choose_scroller(&[v(false, true, false), v(false, true, true), v(true, true, true)]), Some(1));
        // nothing vertical below the page: the Document itself
        assert_eq!(choose_scroller(&[v(false, true, false), v(true, true, false)]), Some(1));
        assert_eq!(choose_scroller(&[v(false, false, false), v(true, true, true)]), Some(1));
        // the walk never goes past the Document (browser UI)
        assert_eq!(choose_scroller(&[v(true, false, false), v(false, true, true)]), None);
        assert_eq!(choose_scroller(&[]), None);
        // no Document at all (a native app): the nearest vertical one
        assert_eq!(choose_scroller(&[v(false, true, true)]), Some(0));
    }

    #[test]
    fn scroll_percent_for_is_the_inverse_of_scroll_offset() {
        // 4000 px page, 800 px viewport (view 20%): 1600 px is 50%
        assert!((scroll_percent_for(1600.0, 20.0, 800.0).unwrap() - 50.0).abs() < 1e-9);
        let pct = scroll_percent_for(1234.5, 13.0, 777.0).unwrap();
        assert!((scroll_offset(pct, 13.0, 777.0) - 1234.5).abs() < 1e-6);
        assert_eq!(scroll_percent_for(-50.0, 20.0, 800.0), Some(0.0), "clamped at the top");
        assert_eq!(scroll_percent_for(1e9, 20.0, 800.0), Some(100.0), "clamped at the end");
        assert_eq!(scroll_percent_for(10.0, 100.0, 800.0), None, "page fits: nothing to scroll");
        assert!((max_offset(20.0, 800.0) - 3200.0).abs() < 1e-9);
    }

    #[test]
    fn scroll_offset_is_pixels_not_percent() {
        // 4000 px page, 800 px viewport (view 20%): 50% = 1600 px down
        assert!((scroll_offset(50.0, 20.0, 800.0) - 1600.0).abs() < 1e-9);
        // the page grows to 8000 px (lazy content) while the offset stays 1600 px:
        // the percent drops to 22.2..., the offset in px does not change
        let pct = 1600.0 / (8000.0 - 800.0) * 100.0;
        assert!((scroll_offset(pct, 10.0, 800.0) - 1600.0).abs() < 1e-6);
        assert_eq!(scroll_offset(-1.0, 100.0, 800.0), 0.0, "not scrollable");
        assert_eq!(scroll_offset(0.0, 20.0, 800.0), 0.0);
        assert_eq!(scroll_offset(f64::NAN, 20.0, 800.0), 0.0);
    }

    #[test]
    fn contains_checks_any_rect() {
        let r = vec![
            Rect {
                x: 10.0,
                y: 10.0,
                w: 20.0,
                h: 10.0,
            },
            Rect {
                x: 0.0,
                y: 30.0,
                w: 5.0,
                h: 5.0,
            },
        ];
        assert!(contains(&r, 15.0, 15.0));
        assert!(contains(&r, 2.0, 32.0));
        assert!(!contains(&r, 15.0, 25.0));
        assert!(!contains(&[], 0.0, 0.0));
    }

    #[test]
    fn newer_request_of_the_same_kind_supersedes() {
        let (t, _r) = mpsc::channel();
        let (h, _r2) = mpsc::channel();
        let q: VecDeque<Req> = [Req::Hover(1, 1, h), Req::Scroll(3, None)].into_iter().collect();
        assert!(superseded(&q, |r| matches!(r, Req::Hover(..))));
        assert!(!superseded(&q, |r| matches!(r, Req::Rects(..))));
        let q: VecDeque<Req> = [Req::Rects(vec![0], t)].into_iter().collect();
        assert!(superseded(&q, |r| matches!(r, Req::Rects(..))));
    }

    #[test]
    fn click_read_commits_only_for_its_own_id() {
        let mut staged = Some((7u64, "clicked"));
        assert_eq!(take_staged(&mut staged, 7), Some("clicked"));
        assert_eq!(take_staged(&mut staged, 7), None, "committed once");
        let mut staged = Some((7u64, "clicked"));
        assert_eq!(take_staged(&mut staged, 8), None, "another click's commit");
        assert!(staged.is_none(), "a stale staged read is dropped");
        let mut none: Option<(u64, &str)> = None;
        assert_eq!(take_staged(&mut none, 1), None, "nothing staged (dropped result)");
    }

    #[test]
    fn plain_text_clicks_are_jumps() {
        // Text in a paragraph group in the page document
        assert!(click_is_on_text(&[50020, 50026, 50030]));
        // a bullet point, a table cell
        assert!(click_is_on_text(&[50020, 50007, 50008, 50030]));
        assert!(click_is_on_text(&[50020, 50029, 50036, 50030]));
        // only the part up to the first Document counts (browser frame above it)
        assert!(click_is_on_text(&[50020, 50030, 50033, 50032]));
    }

    #[test]
    fn clicks_on_controls_are_not_jumps() {
        for ctl in [
            50005, // Hyperlink
            50000, // Button
            50004, // Edit
            50002, // CheckBox
            50013, // RadioButton
            50003, // ComboBox
            50011, // MenuItem
            50009, // Menu
            50019, // TabItem
            50014, // ScrollBar
            50027, // Thumb
            50015, // Slider
            50031, // SplitButton
            50006, // Image
        ] {
            assert!(!click_is_on_text(&[ctl, 50026, 50030]), "{ctl} itself");
            // link text: a Text element inside a Hyperlink
            assert!(!click_is_on_text(&[50020, ctl, 50030]), "text inside {ctl}");
        }
    }

    #[test]
    fn nothing_under_the_point_is_not_a_jump() {
        assert!(!click_is_on_text(&[]));
        assert!(!click_is_on_text(&[0]), "unknown control type");
    }

    #[test]
    fn rects_of_an_unknown_word_is_no_answer_not_off_screen() {
        let read = Read::default();
        assert!(matches!(read, Read::Uia(_)), "the default read is an empty UIA read");
        assert_eq!(read.rects(&[0]), None);
        assert_eq!(read.rects(&[]), Some(vec![]));
        assert_eq!(read.hwnd(), 0);
        assert!(!read.at_end_of_screen());
    }

    #[test]
    fn acrobat_tries_its_dom_then_ocr() {
        assert_eq!(choose_source(true, false, None), vec![Src::Acro, Src::Ocr]);
    }

    #[test]
    fn other_windows_try_uia_then_ocr() {
        assert_eq!(choose_source(false, false, None), vec![Src::Uia, Src::Ocr]);
    }

    #[test]
    fn only_uia_no_text_falls_through_to_ocr() {
        assert_eq!(choose_source(false, false, Some(NO_TEXT)), vec![Src::Ocr]);
        for e in [
            "text lookup timed out",
            NOT_TEXT_CLICK,
            "click is not on a word",
            "browser UI, not page text",
            "point is over our own window",
        ] {
            assert_eq!(choose_source(false, false, Some(e)), vec![], "{e}");
        }
    }

    #[test]
    fn acrobat_falls_through_to_ocr_only_without_a_dom_or_words() {
        assert_eq!(choose_source(true, false, Some(ra_acro::NO_TEXT_ACRO)), vec![Src::Ocr]);
        assert_eq!(
            choose_source(true, false, Some("Acrobat DOM unavailable (QueryService)")),
            vec![Src::Ocr],
            "Acrobat withholds its DOM: OCR still reads the visible page"
        );
        assert_eq!(choose_source(true, false, Some(ra_acro::ACRO_TIMEOUT)), vec![], "a timeout is final");
    }

    #[test]
    fn chromium_never_falls_through_to_ocr() {
        // a cold Edge/Chrome tree is retried by pick_uia, then "Nothing readable here":
        // an OCR read there would have no scroll and no follow
        assert_eq!(choose_source(false, true, None), vec![Src::Uia]);
        assert_eq!(choose_source(false, true, Some(NO_TEXT)), vec![]);
        assert_eq!(choose_source(false, true, Some("browser UI, not page text")), vec![]);
    }

    #[test]
    fn ocr_is_not_tried_on_app_chrome() {
        // an image (Photos), a canvas pane, the desktop's icon list: content
        assert!(ocr_ok_on(&[50006, 50033, 50032]));
        assert!(ocr_ok_on(&[50025, 50033, 50032]));
        assert!(ocr_ok_on(&[]), "UIA could not say: let OCR look");
        // menus, tabs, toolbars, buttons, the title bar: the click is for the app
        for ctl in [50011, 50010, 50019, 50018, 50021, 50000, 50037, 50014, 50004] {
            assert!(!ocr_ok_on(&[ctl, 50033, 50032]), "{ctl} itself");
            assert!(!ocr_ok_on(&[50020, ctl, 50032]), "inside {ctl}");
        }
    }

    fn toks(v: &[&str]) -> Vec<String> {
        v.iter().map(|t| t.to_string()).collect()
    }

    #[test]
    fn an_ocr_read_delegates_and_reaches_the_end_of_the_screen() {
        let mut read = Read::Ocr(ra_ocr::OcrRead::for_test(77, &["a", "b"]));
        assert_eq!(read.hwnd(), 77);
        assert!(!read.at_end_of_screen());
        assert_eq!(read.more(), Ok(toks(&["a", "b"])));
        assert!(!read.at_end_of_screen(), "a full batch is not the end");
        assert_eq!(read.more(), Ok(vec![]));
        assert!(read.at_end_of_screen(), "OCR read to the end of what is on screen");
        // UIA reads never show the end-of-screen notice
        assert!(!Read::default().at_end_of_screen());
    }

    /// `serve_more` with a caller that did (or did not) get the reply.
    fn more_req(read: &mut Read, delivered: bool) -> Option<Result<Vec<String>, String>> {
        let mut got = None;
        serve_more(read, |r| {
            if delivered {
                got = Some(r);
            }
            delivered
        });
        got
    }

    #[test]
    fn an_undelivered_batch_is_rolled_back() {
        let mut read = Read::Ocr(ra_ocr::OcrRead::for_test(77, &["a", "b"]));
        assert_eq!(more_req(&mut read, false), None, "reply undeliverable: rolled back");
        assert_eq!(more_req(&mut read, true), Some(Ok(toks(&["a", "b"]))));
        assert_eq!(more_req(&mut read, false), None);
        assert!(!read.at_end_of_screen(), "an end nobody saw is undone");
        assert_eq!(more_req(&mut read, true), Some(Ok(vec![])));
        assert!(read.at_end_of_screen());
    }
}

#[cfg(test)]
#[path = "ra_uia_manual_tests.rs"]
mod manual_tests;
