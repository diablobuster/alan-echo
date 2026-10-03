//! Read Aloud — UI Automation text access.
//!
//! One worker thread ("ra-uia") owns every COM object; callers talk to it over
//! channels with short timeouts, so a hung target app can never block the UI.
//! Behaviour follows Spike A (spec section 5, rules 1-9). Never panics.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

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

/// A rectangle in physical screen pixels.
#[derive(Clone, Debug, serde::Serialize, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// True when (x, y) lies inside any of the rects (edges inclusive).
pub fn contains(rects: &[Rect], x: f64, y: f64) -> bool {
    rects
        .iter()
        .any(|r| x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h)
}

enum Req {
    Hover(i32, i32, Sender<Option<Vec<Rect>>>),
    Pick(i32, i32, Sender<Result<Vec<String>, String>>),
    More(Sender<Vec<String>>),
    Rects(Vec<usize>, Sender<Vec<Rect>>),
    Scroll(usize),
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
const NO_TEXT: &str = "no TextPattern under the point";

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

    /// Word rects under the point, or `None` (no word, not a text element, our own
    /// window, or > 50 ms).
    pub fn hover(&self, x: i32, y: i32) -> Option<Vec<Rect>> {
        let (t, r) = mpsc::channel();
        self.tx.send(Req::Hover(x, y, t)).ok()?;
        r.recv_timeout(Duration::from_millis(50)).ok().flatten()
    }

    /// Validate the click (rules 2/3) on whatever window is under the point and return
    /// raw tokens from the clicked word forward (≤ 200). Token `i` ↔ source index `i`
    /// for `rects`/`scroll_to`. A failed pick keeps the previous read.
    pub fn pick(&self, x: i32, y: i32) -> Result<Vec<String>, String> {
        let (t, r) = mpsc::channel();
        self.tx
            .send(Req::Pick(x, y, t))
            .map_err(|_| "text reader is not running".to_string())?;
        match r.recv_timeout(Duration::from_secs(5)) {
            Ok(res) => res,
            Err(mpsc::RecvTimeoutError::Timeout) => Err("text lookup timed out".into()),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err("text reader stopped".into()),
        }
    }

    /// Next ≤ 400 raw tokens; source indices continue. Only an EMPTY result means the
    /// end of the document (a short batch does not).
    pub fn more(&self) -> Vec<String> {
        let (t, r) = mpsc::channel();
        if self.tx.send(Req::More(t)).is_err() {
            return Vec::new();
        }
        r.recv_timeout(Duration::from_secs(5)).unwrap_or_default()
    }

    /// Fresh screen rects for the given source indices (rule 9: never cached).
    pub fn rects(&self, src: Vec<usize>) -> Vec<Rect> {
        let (t, r) = mpsc::channel();
        if self.tx.send(Req::Rects(src, t)).is_err() {
            return Vec::new();
        }
        r.recv_timeout(Duration::from_millis(100))
            .unwrap_or_default()
    }

    /// Scroll the source so token `src_idx` is visible (rule 4). Fire-and-forget.
    pub fn scroll_to(&self, src_idx: usize) {
        let _ = self.tx.send(Req::Scroll(src_idx));
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

/// The current read: everything from the one TextPattern picked at click time (rule 7).
#[derive(Default)]
struct Read {
    hwnd: isize,
    chromium: bool,
    /// Word range per source index.
    ranges: Vec<IUIAutomationTextRange>,
    /// Last word range handed out; `more` walks forward from here (rule 8).
    cursor: Option<IUIAutomationTextRange>,
}

struct State {
    uia: IUIAutomation,
    own_pid: u32,
    target: Target,
    read: Read,
    source: Arc<AtomicIsize>,
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
        read: Read {
            hwnd,
            ..Default::default()
        },
        source,
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
                if pending.iter().any(|r| matches!(r, Req::Hover(..))) {
                    continue;
                }
                let _ = t.send(word_at(&mut st, x, y).ok().map(|(_, r)| r));
            }
            // If the caller already timed out (reply undeliverable), roll the read back so
            // source indices stay aligned with the tokens the caller actually has.
            Req::Pick(x, y, t) => {
                let saved = std::mem::take(&mut st.read);
                let res = pick(&mut st, x, y);
                let failed = res.is_err();
                if t.send(res).is_err() || failed {
                    st.read = saved; // a rejected/lost pick keeps the old read
                }
                st.source.store(st.read.hwnd, Ordering::Release);
            }
            Req::More(t) => {
                let (len, cur) = (st.read.ranges.len(), st.read.cursor.clone());
                if t.send(read_forward(&mut st.read, MORE_BATCH)).is_err() {
                    st.read.ranges.truncate(len);
                    st.read.cursor = cur;
                }
            }
            Req::Rects(src, t) => {
                let v = src
                    .iter()
                    .filter_map(|&i| st.read.ranges.get(i))
                    .flat_map(rects_of)
                    .collect();
                let _ = t.send(v);
            }
            Req::Scroll(i) => {
                if let Some(r) = st.read.ranges.get(i) {
                    scroll(&st, r);
                }
            }
        }
    }
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
    let root = root_at(x, y).ok_or("no window under the point")?;
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(root, Some(&mut pid)) };
    if pid == st.own_pid {
        return Err("point is over our own window".into());
    }
    let h = root.0 as isize;
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
    let rects = rects_of(&r);
    // Rule 2: UIA snaps to the nearest word; accept only a click on the word itself.
    if !contains(&rects, x as f64, y as f64) {
        return Err("click is not on a word".into());
    }
    Ok((r, rects))
}

/// Starts a new read in `st.read` (the caller saved and cleared the old one).
fn pick(st: &mut State, x: i32, y: i32) -> Result<Vec<String>, String> {
    // Rule 1: Chromium's first query can come back empty — retry once, but only for
    // "nothing there"; rule 2/3 rejections are final.
    let (r, _) = match word_at(st, x, y) {
        Ok(v) => v,
        Err(e) if e == NO_TEXT => {
            std::thread::sleep(Duration::from_millis(150));
            word_at(st, x, y)?
        }
        Err(e) => return Err(e),
    };
    // Rule 7: everything for this read comes from this one range's source.
    let first = unsafe { r.GetText(-1) }
        .map(|b| b.to_string())
        .unwrap_or_default();
    st.read = Read {
        hwnd: st.target.hwnd,
        chromium: st.target.chromium,
        ranges: vec![r.clone()],
        cursor: Some(r),
    };
    let mut out = Vec::with_capacity(FIRST_BATCH);
    out.push(first);
    out.extend(read_forward(&mut st.read, FIRST_BATCH - 1));
    Ok(out)
}

/// Rule 8: walk forward word by word from the cursor; never re-query visible ranges.
fn read_forward(read: &mut Read, n: usize) -> Vec<String> {
    let mut out = Vec::new();
    let Some(cur) = read.cursor.as_ref() else {
        return out;
    };
    let Ok(r) = (unsafe { cur.Clone() }) else {
        return out;
    };
    for _ in 0..n {
        let moved = unsafe { r.Move(TextUnit_Word, 1) }.unwrap_or(0);
        if moved == 0 {
            break;
        }
        if unsafe { r.ExpandToEnclosingUnit(TextUnit_Word) }.is_err() {
            break;
        }
        let Ok(keep) = (unsafe { r.Clone() }) else {
            break;
        };
        let text = unsafe { keep.GetText(-1) }
            .map(|b| b.to_string())
            .unwrap_or_default();
        read.ranges.push(keep.clone());
        read.cursor = Some(keep);
        out.push(text);
    }
    out
}

/// Bounding rects of a range (empty when off-screen). Never cached (rule 9).
fn rects_of(r: &IUIAutomationTextRange) -> Vec<Rect> {
    let Ok(sa) = (unsafe { r.GetBoundingRectangles() }) else {
        return Vec::new();
    };
    if sa.is_null() {
        return Vec::new();
    }
    let nums = unsafe { read_f64_array(sa) };
    unsafe {
        let _ = SafeArrayDestroy(sa);
    }
    nums.chunks_exact(4)
        .filter(|c| c[2] > 0.0 && c[3] > 0.0)
        .map(|c| Rect {
            x: c[0],
            y: c[1],
            w: c[2],
            h: c[3],
        })
        .collect()
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

fn scroll(st: &State, r: &IUIAutomationTextRange) {
    unsafe {
        if st.read.chromium {
            // Rule 4: Chromium ignores word-range ScrollIntoView. Measured on Edge 2026-10-02:
            // the paragraph *range* ScrollIntoView was also a no-op, while ScrollItemPattern on
            // the word's enclosing element (the <p>) scrolled it into view. Use that first and
            // keep the paragraph range as the fallback.
            if scroll_item(st, r) {
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

/// ScrollItemPattern on the range's enclosing element or its nearest ancestor that has one.
fn scroll_item(st: &State, r: &IUIAutomationTextRange) -> bool {
    let Ok(walker) = (unsafe { st.uia.ControlViewWalker() }) else {
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
}

#[cfg(test)]
#[path = "ra_uia_manual_tests.rs"]
mod manual_tests;
