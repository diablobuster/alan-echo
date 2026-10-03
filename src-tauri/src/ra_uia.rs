//! Read Aloud — UI Automation text access.
//!
//! One worker thread ("ra-uia") owns every COM object; callers talk to it over
//! channels with short timeouts, so a hung target app can never block the UI.
//! Behaviour follows Spike A (spec section 5, rules 1-9). Never panics.
#![allow(dead_code)] // wired up by the Read Aloud controller task

use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, Sender};
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
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::HiDpi::{
    SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::GetClassNameW;

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
    hwnd: isize,
}

/// Raw tokens returned per `pick` / `more` call.
const BATCH: usize = 2000;
/// Max ancestors walked from the point element looking for a TextPattern.
const MAX_ANCESTORS: usize = 30;

impl Uia {
    /// Spawn the worker and start warming the UIA connection to `hwnd` now (rule 1).
    /// Non-blocking.
    pub fn start(hwnd: isize) -> Uia {
        let (tx, rx) = mpsc::channel();
        if let Err(e) = std::thread::Builder::new()
            .name("ra-uia".into())
            .spawn(move || worker(hwnd, rx))
        {
            log::warn!("read-aloud: could not spawn UIA worker: {e}");
        }
        Uia { tx, hwnd }
    }

    pub fn source_hwnd(&self) -> isize {
        self.hwnd
    }

    /// Word rects under the point, or `None` (no word, not a text element, or > 50 ms).
    pub fn hover(&self, x: i32, y: i32) -> Option<Vec<Rect>> {
        let (t, r) = mpsc::channel();
        self.tx.send(Req::Hover(x, y, t)).ok()?;
        r.recv_timeout(Duration::from_millis(50)).ok().flatten()
    }

    /// Validate the click (rules 2/3) and return raw tokens from the clicked word
    /// forward (≤ 2000). Token `i` ↔ source index `i` for `rects`/`scroll_to`.
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

    /// Next ≤ 2000 raw tokens (empty at the end). Source indices continue.
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

struct State {
    uia: IUIAutomation,
    chromium: bool,
    /// Word range per source index (rule 7: all from the one TextPattern picked at click time).
    ranges: Vec<IUIAutomationTextRange>,
    /// Last word range handed out; `more` walks forward from here (rule 8).
    cursor: Option<IUIAutomationTextRange>,
    /// Source window's root element (fallback search scope).
    root: Option<IUIAutomationElement>,
    /// Cached TextPattern elements under `root`, for apps whose point element has no
    /// TextPattern ancestor (Win11 Notepad: the point hits a WinUI `InputSiteWindowClass`
    /// pane that is a sibling of the RichEdit document).
    /// Plus when it was (re)built, so a miss refreshes at most once a second (hovering
    /// over non-text must not run a whole-tree FindAll on every mouse move).
    fallback: Option<(Vec<IUIAutomationElement>, std::time::Instant)>,
}

fn worker(hwnd: isize, rx: Receiver<Req>) {
    unsafe {
        // Physical pixels regardless of the process manifest; the LL mouse hook
        // also reports per-monitor-aware coordinates.
        let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let com_ok = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    run(hwnd, rx);
    if com_ok {
        unsafe { CoUninitialize() };
    }
}

fn run(hwnd: isize, rx: Receiver<Req>) {
    let uia: IUIAutomation =
        match unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) } {
            Ok(u) => u,
            Err(e) => {
                log::warn!("read-aloud: UIA init failed: {e}");
                return; // dropping rx makes every call fail fast
            }
        };
    let h = HWND(hwnd as *mut _);
    let chromium = {
        let mut buf = [0u16; 64];
        let n = unsafe { GetClassNameW(h, &mut buf) }.clamp(0, buf.len() as i32) as usize;
        String::from_utf16_lossy(&buf[..n]).starts_with("Chrome_WidgetWin")
    };
    let root = warm_up(&uia, h);
    let mut st = State {
        uia,
        chromium,
        ranges: Vec::new(),
        cursor: None,
        root,
        fallback: None,
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
            // If the caller already timed out (reply undeliverable), roll the read state
            // back so source indices stay aligned with the tokens the caller actually has.
            Req::Pick(x, y, t) => {
                let saved = (std::mem::take(&mut st.ranges), st.cursor.take());
                let res = pick(&mut st, x, y);
                let failed = res.is_err();
                if t.send(res).is_err() || failed {
                    (st.ranges, st.cursor) = saved; // a rejected/lost pick keeps the old read
                }
            }
            Req::More(t) => {
                let (len, cur) = (st.ranges.len(), st.cursor.clone());
                if t.send(read_forward(&mut st, BATCH)).is_err() {
                    st.ranges.truncate(len);
                    st.cursor = cur;
                }
            }
            Req::Rects(src, t) => {
                let v = src
                    .iter()
                    .filter_map(|&i| st.ranges.get(i))
                    .flat_map(rects_of)
                    .collect();
                let _ = t.send(v);
            }
            Req::Scroll(i) => {
                if let Some(r) = st.ranges.get(i) {
                    scroll(&st, r);
                }
            }
        }
    }
}

/// Rule 1: touch the window's tree so Chromium builds its accessibility tree now
/// (first contact costs ~1.8 s), not when the user clicks.
fn warm_up(uia: &IUIAutomation, h: HWND) -> Option<IUIAutomationElement> {
    let t = std::time::Instant::now();
    let root = unsafe { uia.ElementFromHandle(h) }.ok()?;
    unsafe {
        let _ = root.CurrentName();
        if let Ok(c) = text_pattern_condition(uia) {
            let _ = root.FindFirst(TreeScope_Descendants, &c);
        }
    }
    log::info!("read-aloud: UIA warm-up {} ms", t.elapsed().as_millis());
    Some(root)
}

fn text_pattern_condition(uia: &IUIAutomation) -> windows::core::Result<IUIAutomationCondition> {
    unsafe {
        uia.CreatePropertyCondition(UIA_IsTextPatternAvailablePropertyId, &VARIANT::from(true))
    }
}

/// Cap on cached fallback candidates (a browser page can expose many).
const MAX_FALLBACK: i32 = 200;

/// Smallest TextPattern element under the source window whose rect contains the point.
fn fallback_at(st: &mut State, x: i32, y: i32) -> Option<(IUIAutomationTextPattern, Rect)> {
    for refresh in [false, true] {
        let stale = st
            .fallback
            .as_ref()
            .map_or(true, |(_, at)| at.elapsed() > Duration::from_secs(1));
        if refresh && !stale {
            return None;
        }
        if refresh || st.fallback.is_none() {
            let root = st.root.as_ref()?;
            let cond = text_pattern_condition(&st.uia).ok()?;
            let all = unsafe { root.FindAll(TreeScope_Descendants, &cond) }.ok()?;
            let n = unsafe { all.Length() }.unwrap_or(0).min(MAX_FALLBACK);
            let els = (0..n)
                .filter_map(|i| unsafe { all.GetElement(i) }.ok())
                .collect();
            st.fallback = Some((els, std::time::Instant::now()));
        }
        let best = st
            .fallback
            .as_ref()?
            .0
            .iter()
            .filter_map(|e| {
                let r = to_rect(unsafe { e.CurrentBoundingRectangle() }.ok()?);
                contains(std::slice::from_ref(&r), x as f64, y as f64).then_some((e, r))
            })
            .min_by(|a, b| (a.1.w * a.1.h).total_cmp(&(b.1.w * b.1.h)));
        if let Some((e, r)) = best {
            if st.chromium {
                let doc = unsafe { st.uia.ControlViewWalker() }
                    .map(|w| in_document(&w, e))
                    .unwrap_or(false);
                if !doc {
                    return None;
                }
            }
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
/// falls back to a TextPattern element of the source window that contains the point.
fn text_pattern_at(
    st: &mut State,
    x: i32,
    y: i32,
) -> Result<(IUIAutomationTextPattern, Rect), String> {
    let mut el = unsafe { st.uia.ElementFromPoint(POINT { x, y }) }.map_err(|e| e.to_string())?;
    let walker = unsafe { st.uia.ControlViewWalker() }.map_err(|e| e.to_string())?;
    for _ in 0..MAX_ANCESTORS {
        if let Ok(tp) =
            unsafe { el.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) }
        {
            // Rule 3 (browser chrome): Chromium's omnibox also has a TextPattern; only
            // text inside the page's Document counts.
            if st.chromium && !in_document(&walker, &el) {
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
    fallback_at(st, x, y).ok_or_else(|| "no TextPattern under the point".into())
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

fn pick(st: &mut State, x: i32, y: i32) -> Result<Vec<String>, String> {
    // Rule 1: Chromium's first query can come back empty — retry once.
    let (r, _) = match word_at(st, x, y) {
        Ok(v) => v,
        Err(_) => {
            std::thread::sleep(Duration::from_millis(150));
            word_at(st, x, y)?
        }
    };
    // Rule 7: everything for this read comes from this one range's source.
    // (The caller cleared `ranges`/`cursor` and restores them if this pick fails.)
    let first = unsafe { r.GetText(-1) }
        .map(|b| b.to_string())
        .unwrap_or_default();
    st.ranges.push(r.clone());
    st.cursor = Some(r);
    let mut out = Vec::with_capacity(BATCH);
    out.push(first);
    out.extend(read_forward(st, BATCH - 1));
    Ok(out)
}

/// Rule 8: walk forward word by word from the cursor; never re-query visible ranges.
fn read_forward(st: &mut State, n: usize) -> Vec<String> {
    let mut out = Vec::new();
    let Some(cur) = st.cursor.as_ref() else {
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
        st.ranges.push(keep.clone());
        st.cursor = Some(keep);
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
        if st.chromium {
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

    // ---- Manual check (Spike A parity) against Win11 Notepad -----------------------
    // Run: cargo test ra_uia -- --ignored --nocapture
    // Opens its own Notepad window on a temp file, queries it, closes only that window.
    // (The PickHook click-swallow check lives in ra_hook's own manual test.)
    // Refuses to run if any Notepad is already open.

    use std::process::Command;
    use std::time::Instant;
    use windows::Win32::Foundation::{BOOL, LPARAM, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        keybd_event, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VK_MENU,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, PostMessageW,
        SetForegroundWindow, ShowWindow, SW_RESTORE, WM_CLOSE,
    };

    fn notepad_pids() -> Vec<u32> {
        let out = Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq notepad.exe", "/FO", "CSV", "/NH"])
            .output()
            .expect("tasklist");
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| l.split("\",\"").nth(1).and_then(|p| p.parse().ok()))
            .collect()
    }

    fn find_window(title_part: &str) -> Option<HWND> {
        struct Ctx<'a> {
            needle: &'a str,
            found: Option<HWND>,
        }
        unsafe extern "system" fn cb(h: HWND, l: LPARAM) -> BOOL {
            let ctx = &mut *(l.0 as *mut Ctx);
            if IsWindowVisible(h).as_bool() {
                let mut buf = [0u16; 512];
                let n = GetWindowTextW(h, &mut buf).max(0) as usize;
                if String::from_utf16_lossy(&buf[..n]).contains(ctx.needle) {
                    ctx.found = Some(h);
                    return BOOL(0);
                }
            }
            BOOL(1)
        }
        let mut ctx = Ctx {
            needle: title_part,
            found: None,
        };
        unsafe {
            let _ = EnumWindows(Some(cb), LPARAM(&mut ctx as *mut Ctx as isize));
        }
        ctx.found
    }

    /// Closes the Notepad window we opened (and only it) and deletes the temp file.
    struct Cleanup {
        hwnd: Option<HWND>,
        pid: u32,
        file: std::path::PathBuf,
        preexisting: Vec<u32>,
    }
    impl Drop for Cleanup {
        fn drop(&mut self) {
            if let Some(h) = self.hwnd {
                unsafe {
                    let _ = PostMessageW(h, WM_CLOSE, WPARAM(0), LPARAM(0));
                }
                std::thread::sleep(Duration::from_millis(1500));
            }
            if self.pid != 0
                && !self.preexisting.contains(&self.pid)
                && notepad_pids().contains(&self.pid)
            {
                let _ = Command::new("taskkill")
                    .args(["/PID", &self.pid.to_string(), "/F"])
                    .output();
            }
            let _ = std::fs::remove_file(&self.file);
        }
    }

    #[test]
    #[ignore]
    #[allow(clippy::zombie_processes)] // notepad.exe stub hands off; we close by window
    fn uia_notepad_manual() {
        unsafe {
            let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
        let preexisting = notepad_pids();
        assert!(
            preexisting.is_empty(),
            "close Notepad first; this test must not touch your windows"
        );

        let stem = format!("ra_uia_test_{}", std::process::id());
        let file = std::env::temp_dir().join(format!("{stem}.txt"));
        let para = |n: usize| {
            format!("Paragraph{n} alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima.")
        };
        std::fs::write(
            &file,
            format!("{}\r\n\r\n{}\r\n\r\n{}\r\n", para(0), para(1), para(2)),
        )
        .unwrap();
        let mut guard = Cleanup {
            hwnd: None,
            pid: 0,
            file: file.clone(),
            preexisting,
        };

        Command::new("notepad.exe")
            .arg(&file)
            .spawn()
            .expect("launch notepad");
        let t0 = Instant::now();
        let hwnd = loop {
            if let Some(h) = find_window(&stem) {
                break h;
            }
            assert!(
                t0.elapsed() < Duration::from_secs(15),
                "Notepad window not found"
            );
            std::thread::sleep(Duration::from_millis(200));
        };
        let mut pid = 0u32;
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
        }
        guard.hwnd = Some(hwnd);
        guard.pid = pid;
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            keybd_event(VK_MENU.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
            let _ = SetForegroundWindow(hwnd);
            keybd_event(VK_MENU.0 as u8, 0, KEYEVENTF_KEYUP, 0);
        }
        std::thread::sleep(Duration::from_millis(1000));

        // Ground truth from a separate UIA client on this thread (as the spike's probe.py does).
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let uia: IUIAutomation =
            unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }.unwrap();
        let root = unsafe { uia.ElementFromHandle(hwnd) }.unwrap();
        let cond = unsafe {
            uia.CreatePropertyCondition(UIA_IsTextPatternAvailablePropertyId, &VARIANT::from(true))
        }
        .unwrap();
        // Like the spike: of all TextPattern elements, take the one with the longest text.
        let all =
            unsafe { root.FindAll(TreeScope_Descendants, &cond) }.expect("TextPattern elements");
        let (doc_el, tp) = (0..unsafe { all.Length() }.unwrap_or(0))
            .filter_map(|i| unsafe { all.GetElement(i) }.ok())
            .filter_map(|e| {
                let tp: IUIAutomationTextPattern =
                    unsafe { e.GetCurrentPatternAs(UIA_TextPatternId) }.ok()?;
                let n = unsafe { tp.DocumentRange().ok()?.GetText(200_000).ok()?.len() };
                Some((n, e, tp))
            })
            .max_by_key(|(n, _, _)| *n)
            .map(|(_, e, tp)| (e, tp))
            .expect("a TextPattern element with text");
        let el_rect = to_rect(unsafe { doc_el.CurrentBoundingRectangle() }.unwrap());
        let w5 = unsafe {
            let r = tp.DocumentRange().unwrap();
            r.MoveEndpointByRange(
                TextPatternRangeEndpoint_End,
                &r,
                TextPatternRangeEndpoint_Start,
            )
            .unwrap();
            r.Move(TextUnit_Word, 4).unwrap();
            r.ExpandToEnclosingUnit(TextUnit_Word).unwrap();
            r
        };
        let w5_text = unsafe { w5.GetText(-1) }.unwrap().to_string();
        let w5_rects = rects_of(&w5);
        println!("element {el_rect:?}\n5th word {w5_text:?} rects {w5_rects:?}");
        assert!(!w5_rects.is_empty(), "5th word has no rect");
        let cx = (w5_rects[0].x + w5_rects[0].w / 2.0) as i32;
        let cy = (w5_rects[0].y + w5_rects[0].h / 2.0) as i32;
        if let Ok(e) = unsafe { uia.ElementFromPoint(POINT { x: cx, y: cy }) } {
            unsafe {
                println!(
                    "ElementFromPoint({cx},{cy}) -> class {:?} type {:?} name {:?} hwnd {:?}",
                    e.CurrentClassName().map(|s| s.to_string()),
                    e.CurrentControlType().map(|t| t.0),
                    e.CurrentName().map(|s| s.to_string()),
                    e.CurrentNativeWindowHandle()
                );
            }
        }

        // --- the code under test ---
        let t = Instant::now();
        let ra = Uia::start(hwnd.0 as isize);
        assert_eq!(ra.source_hwnd(), hwnd.0 as isize);
        let tokens = ra.pick(cx, cy).expect("pick on the 5th word");
        println!(
            "pick: {} tokens in {} ms (incl. warm-up): {:?}",
            tokens.len(),
            t.elapsed().as_millis(),
            &tokens[..tokens.len().min(16)]
        );
        assert_eq!(
            tokens[0].trim(),
            w5_text.trim(),
            "pick returned the wrong word"
        );
        assert!(
            tokens.len() >= 10,
            "expected >= 10 tokens, got {}",
            tokens.len()
        );
        assert!(
            tokens.iter().any(|t| t.contains("Paragraph2")),
            "walk should reach the 3rd paragraph"
        );

        let r0 = ra.rects(vec![0]);
        println!("rects(0) = {r0:?}");
        assert!(
            !r0.is_empty() && contains(&r0, cx as f64, cy as f64),
            "rects(0) must cover the click"
        );
        let multi = ra.rects(vec![0, 1, 2]);
        assert!(multi.len() >= 3, "3 words -> >= 3 rects, got {multi:?}");
        assert!(ra.rects(vec![999_999]).is_empty());

        let more = ra.more();
        println!("more() -> {} tokens", more.len());
        assert!(more.is_empty(), "short doc: everything came back in pick()");

        // hover: 50 ms budget; allow a few tries in case the first one is cold.
        let mut hov = None;
        for _ in 0..5 {
            hov = ra.hover(cx, cy);
            if hov.is_some() {
                break;
            }
        }
        println!("hover = {hov:?}");
        assert!(
            hov.map(|v| contains(&v, cx as f64, cy as f64))
                .unwrap_or(false),
            "hover must return the word under the point"
        );

        // Rule 2: blank area below the text (inside the element) is rejected.
        let blank = (el_rect.x as i32 + 40, (el_rect.y + el_rect.h) as i32 - 30);
        let res = ra.pick(blank.0, blank.1);
        println!("pick on blank area {blank:?} -> {res:?}");
        assert!(
            res.is_err(),
            "click below the last line must be rejected (rule 2)"
        );
        assert!(ra.hover(blank.0, blank.1).is_none());
        // Rule 3: above the text element (Notepad's tab strip / menu) is rejected.
        let above = (el_rect.x as i32 + 40, el_rect.y as i32 - 40);
        let res = ra.pick(above.0, above.1);
        println!("pick above the element {above:?} -> {res:?}");
        assert!(
            res.is_err(),
            "click outside the text element must be rejected (rule 3)"
        );

        ra.scroll_to(0); // non-Chromium path: must not hang the worker
        assert!(
            !ra.rects(vec![0]).is_empty(),
            "worker still alive after scroll_to; old read kept"
        );
        drop(ra);
    }

    /// Kills only processes whose command line contains our private profile dir.
    struct BrowserCleanup {
        hwnd: Option<HWND>,
        profile: std::path::PathBuf,
        page: std::path::PathBuf,
    }
    impl Drop for BrowserCleanup {
        fn drop(&mut self) {
            if let Some(h) = self.hwnd {
                unsafe {
                    let _ = PostMessageW(h, WM_CLOSE, WPARAM(0), LPARAM(0));
                }
                std::thread::sleep(Duration::from_millis(2000));
            }
            let needle = self.profile.to_string_lossy().replace('\'', "''");
            let ps = format!(
                "Get-CimInstance Win32_Process | Where-Object {{ $_.CommandLine -and $_.CommandLine.Contains('{needle}') }} | ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }}"
            );
            let _ = Command::new("powershell")
                .args(["-NoProfile", "-Command", &ps])
                .output();
            std::thread::sleep(Duration::from_millis(500));
            let _ = std::fs::remove_dir_all(&self.profile);
            let _ = std::fs::remove_file(&self.page);
        }
    }

    /// Manual: Chromium (Edge by default; set RA_BROWSER to chrome.exe's path for Chrome).
    /// Launches a separate browser instance on a private temp profile; never touches the
    /// user's own browser windows. Run: cargo test uia_browser -- --ignored --nocapture
    #[test]
    #[ignore]
    #[allow(clippy::zombie_processes)] // cleaned up by BrowserCleanup
    fn uia_browser_manual() {
        unsafe {
            let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
        let exe = std::env::var("RA_BROWSER").unwrap_or_else(|_| {
            r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe".into()
        });
        let tag = format!("RaUiaBrowser{}", std::process::id());
        let dir = std::env::temp_dir();
        let page = dir.join(format!("{tag}.html"));
        let profile = dir.join(format!("{tag}-profile"));
        let words = [
            "children", "bridges", "stone", "carried", "quiet", "golden", "clouds", "ancient",
            "valleys", "river",
        ];
        let mut body = String::new();
        for p in 0..40 {
            body.push_str(&format!("<p>Paragraph{p:02} begins here."));
            for i in 0..80 {
                body.push(' ');
                body.push_str(words[(p * 7 + i * 3) % words.len()]);
            }
            body.push_str(".</p>\n");
        }
        std::fs::write(
            &page,
            format!("<!doctype html><html><head><meta charset=utf-8><title>{tag}</title><style>body{{font:18px Georgia;max-width:700px;margin:40px auto}}</style></head><body><h1>Read aloud test</h1>\n{body}</body></html>"),
        )
        .unwrap();
        let mut guard = BrowserCleanup {
            hwnd: None,
            profile: profile.clone(),
            page: page.clone(),
        };

        Command::new(&exe)
            .arg(format!("--user-data-dir={}", profile.display()))
            .args([
                "--no-first-run",
                "--no-default-browser-check",
                "--new-window",
                "--window-size=1100,900",
            ])
            .arg(format!(
                "file:///{}",
                page.display().to_string().replace('\\', "/")
            ))
            .spawn()
            .expect("launch browser");
        let t0 = Instant::now();
        let hwnd = loop {
            if let Some(h) = find_window(&tag) {
                break h;
            }
            assert!(
                t0.elapsed() < Duration::from_secs(20),
                "browser window not found"
            );
            std::thread::sleep(Duration::from_millis(250));
        };
        guard.hwnd = Some(hwnd);
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            keybd_event(VK_MENU.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
            let _ = SetForegroundWindow(hwnd);
            keybd_event(VK_MENU.0 as u8, 0, KEYEVENTF_KEYUP, 0);
        }
        std::thread::sleep(Duration::from_millis(1500));

        // Ground truth: rect of "Paragraph03" via a separate UIA client.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let uia: IUIAutomation =
            unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }.unwrap();
        let cond = text_pattern_condition(&uia).unwrap();
        let mut target = None;
        let mut hwnd = hwnd;
        for _ in 0..40 {
            std::thread::sleep(Duration::from_millis(250));
            // Edge may swap its startup window; re-find it by title each time.
            if let Some(h) = find_window(&tag) {
                hwnd = h;
            }
            let Ok(root) = (unsafe { uia.ElementFromHandle(hwnd) }) else {
                continue;
            };
            let Ok(all) = (unsafe { root.FindAll(TreeScope_Descendants, &cond) }) else {
                continue;
            };
            for i in 0..unsafe { all.Length() }.unwrap_or(0) {
                let Ok(e) = (unsafe { all.GetElement(i) }) else {
                    continue;
                };
                let Ok(tp) = (unsafe {
                    e.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
                }) else {
                    continue;
                };
                let Ok(doc) = (unsafe { tp.DocumentRange() }) else {
                    continue;
                };
                if let Ok(r) =
                    unsafe { doc.FindText(&windows::core::BSTR::from("Paragraph03"), false, false) }
                {
                    let rc = rects_of(&r);
                    if !rc.is_empty() {
                        target = Some(rc[0].clone());
                        break;
                    }
                }
            }
            if target.is_some() {
                break;
            }
        }
        let w = target.expect("Paragraph03 on screen");
        let (cx, cy) = ((w.x + w.w / 2.0) as i32, (w.y + w.h / 2.0) as i32);
        println!("Paragraph03 at {w:?}");
        guard.hwnd = Some(hwnd);
        let t = Instant::now();
        let ra = Uia::start(hwnd.0 as isize);

        let tokens = ra.pick(cx, cy).expect("pick in browser");
        println!(
            "pick: {} tokens, {} ms since Uia::start (incl. its warm-up; the test's own UIA client already woke Chromium); first {:?}",
            tokens.len(),
            t.elapsed().as_millis(),
            &tokens[..tokens.len().min(8)]
        );
        assert!(tokens[0].contains("Paragraph03"), "clicked word");
        assert_eq!(tokens.len(), BATCH, "a long page fills a whole batch");
        let t = Instant::now();
        let more = ra.more();
        println!(
            "more(): {} tokens in {} ms; last {:?}",
            more.len(),
            t.elapsed().as_millis(),
            more.last()
        );
        assert!(!more.is_empty());
        assert!(
            ra.more().is_empty() || ra.more().is_empty(),
            "eventually empty at the end"
        );

        let r0 = ra.rects(vec![0]);
        assert!(
            contains(&r0, cx as f64, cy as f64),
            "rects(0) covers the click: {r0:?}"
        );
        let far = 1500;
        let before = ra.rects(vec![far]);
        let t = Instant::now();
        ra.scroll_to(far);
        std::thread::sleep(Duration::from_millis(1000));
        let after = ra.rects(vec![far]);
        println!("token {far} {:?}: rects before scroll {before:?}, after {after:?} (scroll+1s wait {} ms)", tokens[far], t.elapsed().as_millis());
        assert!(
            !after.is_empty(),
            "rule 4: paragraph scroll brings the word on screen"
        );

        // Rule 3: the browser toolbar/tab strip (just below the window's top edge) is rejected.
        let mut wr = windows::Win32::Foundation::RECT::default();
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut wr);
        }
        let res = ra.pick(wr.left + 300, wr.top + 60);
        println!("pick on toolbar -> {res:?}");
        assert!(res.is_err());
        drop(ra);
    }
}
