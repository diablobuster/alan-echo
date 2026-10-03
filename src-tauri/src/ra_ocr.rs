//! Read Aloud — OCR source for windows with no readable text (scans, images, canvas).
//!
//! Windows' built-in OCR (`Windows.Media.Ocr`) over the visible client area of one
//! window (spec §3.4–3.5). Runs only on the "ra-uia" worker thread (MTA, per-monitor
//! DPI aware, so everything is physical screen px). Never panics: every GDI object is
//! released by a Drop guard on all paths, and the captured pixels are dropped right
//! after OCR. No background work: staleness is checked only when asked, at most every
//! 500 ms.

use std::cell::Cell;
use std::time::{Duration, Instant};

use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, ClientToScreen, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject,
    GetDC, GetDIBits, ReleaseDC, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetSystemMetrics, IsIconic, IsWindow, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

use crate::ra_uia::Rect;

/// `pick` found no words in the window.
pub const NO_TEXT_OCR: &str = "OCR found no words";
const FIRST_BATCH: usize = 200;
const MORE_BATCH: usize = 400;
/// Staleness (and a stale hover cache's re-OCR) is checked at most this often.
const STALE_EVERY: Duration = Duration::from_millis(500);
/// Tiles overlap by this much so a word cut by one tile edge is whole in the next.
const OVERLAP: u32 = 64;
/// `thumb_sum` samples a THUMB x THUMB grid.
const THUMB: u32 = 64;
/// Width of the fingerprint column. Every GDI screen read costs ~7 ms whatever its
/// size, so the fingerprint is ONE BitBlt: a full-height column through the text.
const STRIP_W: u32 = 32;

/// OCR words in reading order: lines as given, words left->right in each line.
pub(crate) struct OcrWord {
    pub text: String,
    pub rect: Rect,
    pub line: usize,
}

/// `OcrRead::mark`.
pub struct OcrMark {
    next: usize,
    at_end: bool,
}

/// A read over one OCR snapshot of a window. Source index `i` is word `start + i`.
pub struct OcrRead {
    hwnd: isize,
    words: Vec<OcrWord>,
    toks: Vec<String>,
    start: usize,
    next: usize,
    at_end: bool,
    /// (client rect in screen px, thumbnail checksum) when the OCR ran.
    orig: (Rect, u64),
    /// Fingerprint column (x offset into the capture region, width).
    strip: (u32, u32),
    made: Instant,
    /// Last staleness answer and when it was computed.
    memo: Cell<Option<(Instant, bool)>>,
}

impl OcrRead {
    /// OCR the window's visible client area, then ≤ 200 tokens from the word at or
    /// nearest the point. `Err(NO_TEXT_OCR)` when no words are found.
    pub fn pick(hwnd: isize, x: i32, y: i32) -> Result<(OcrRead, Vec<String>), String> {
        let t0 = Instant::now();
        let mut r = Self::build(hwnd)?;
        let Some(s) = start_index(&r.words, x as f64, y as f64) else {
            log::info!("read-aloud: OCR found no words ({} ms)", t0.elapsed().as_millis());
            return Err(NO_TEXT_OCR.into());
        };
        r.start = s;
        r.next = s;
        let first = r.take(FIRST_BATCH);
        log::info!(
            "read-aloud: OCR pick {} words, start {s}, {} ms",
            r.words.len(),
            t0.elapsed().as_millis()
        );
        Ok((r, first))
    }

    /// The rest of the OCR'd words (≤ 400 per call), then `Ok(empty)`.
    pub fn more(&mut self) -> Result<Vec<String>, String> {
        let out = self.take(MORE_BATCH);
        if out.is_empty() {
            self.at_end = true;
        }
        Ok(out)
    }

    /// The stored boxes of source indices `src`; `Some(empty)` when the window moved
    /// or its content changed since the OCR; `None` for an unknown index.
    pub fn rects(&self, src: &[usize]) -> Option<Vec<Rect>> {
        if self.stale() {
            return Some(Vec::new());
        }
        src.iter()
            .map(|&i| {
                self.start
                    .checked_add(i)
                    .and_then(|k| self.words.get(k))
                    .map(|w| w.rect.clone())
            })
            .collect()
    }

    /// OCR reads only the visible area; there is nothing to scroll.
    pub fn scroll_to(&self, _idx: usize) {}

    pub fn hwnd(&self) -> isize {
        self.hwnd
    }

    /// True once `more` returned empty (the controller then says it read to the end
    /// of what is on screen).
    pub fn at_end(&self) -> bool {
        self.at_end
    }

    /// Where `more` is; `rollback` returns there (a batch the caller never got).
    pub fn mark(&self) -> OcrMark {
        OcrMark { next: self.next, at_end: self.at_end }
    }

    pub fn rollback(&mut self, m: OcrMark) {
        self.next = m.next;
        self.at_end = m.at_end;
    }

    /// A read over ready-made tokens (no OCR), for unit tests.
    #[cfg(test)]
    pub(crate) fn for_test(hwnd: isize, toks: &[&str]) -> OcrRead {
        let mut r = OcrRead::failed(hwnd);
        r.toks = toks.iter().map(|t| t.to_string()).collect();
        r
    }

    fn take(&mut self, n: usize) -> Vec<String> {
        let a = self.next.min(self.toks.len());
        let b = a.saturating_add(n).min(self.toks.len());
        self.next = b;
        self.toks[a..b].to_vec()
    }

    /// OCR the whole visible client area (may find no words: `words` is then empty).
    fn build(hwnd: isize) -> Result<OcrRead, String> {
        let h = HWND(hwnd as *mut _);
        let win = client_rect(h).ok_or("window is gone or minimised")?;
        let reg = clip_to_screen(&win).ok_or("window is not on screen")?;
        let engine = OcrEngine::TryCreateFromUserProfileLanguages()
            .map_err(|_| "Windows OCR unavailable".to_string())?;
        let max = OcrEngine::MaxImageDimension().unwrap_or(2600);
        let bgra = grab(&reg).ok_or("screen capture failed")?;
        let mut words = Vec::new();
        let mut line_tile = Vec::new();
        let tl = tiles(reg.w, reg.h, max);
        for (ti, &t) in tl.iter().enumerate() {
            let lines = if tl.len() == 1 {
                recognize(&engine, &bgra, reg.w, reg.h)
            } else {
                recognize(&engine, &crop(&bgra, reg.w, t), t.2, t.3)
            };
            let lines = match lines {
                Ok(l) => l,
                Err(e) => {
                    // keep an empty read (cached like "no words") so hover does not
                    // re-OCR on every mouse move
                    log::warn!("read-aloud: OCR failed: {e}");
                    words.clear();
                    line_tile.clear();
                    break;
                }
            };
            for line in lines {
                let id = line_tile.len();
                line_tile.push(ti);
                for (text, b) in line {
                    words.push(OcrWord {
                        text,
                        rect: Rect {
                            x: b.x + t.0 as f64 + reg.x as f64,
                            y: b.y + t.1 as f64 + reg.y as f64,
                            w: b.w,
                            h: b.h,
                        },
                        line: id,
                    });
                }
            }
        }
        // the fingerprint comes from the OCR'd pixels themselves (no extra screen read)
        let strip = strip_of(&words, reg.x as f64, reg.w);
        let col = crop(&bgra, reg.w, (strip.0, 0, strip.1, reg.h));
        let thumb = thumb_sum(&col, strip.1, reg.h);
        drop(col);
        drop(bgra); // the pixels are not kept past OCR
        let mut words = dedupe_tiles(words);
        if tl.len() > 1 {
            words = join_tile_lines(words, &line_tile);
        }
        let toks = tokens(&words);
        Ok(OcrRead {
            hwnd,
            words,
            toks,
            start: 0,
            next: 0,
            at_end: false,
            orig: (win, thumb),
            strip,
            made: Instant::now(),
            memo: Cell::new(None),
        })
    }

    /// The window moved, resized, closed, or its pixels changed since the OCR.
    /// The rect is checked first (free); only then is the fingerprint column re-read
    /// (one BitBlt). At most every 500 ms.
    fn stale(&self) -> bool {
        if let Some((at, v)) = self.memo.get() {
            if at.elapsed() < STALE_EVERY {
                return v;
            }
        }
        let v = match client_rect(HWND(self.hwnd as *mut _)) {
            Some(win) if win == self.orig.0 => match clip_to_screen(&win).and_then(|r| self.strip_sum(&r)) {
                Some(sum) => is_stale(self.orig.clone(), (win, sum)),
                None => {
                    log::warn!("read-aloud: OCR fingerprint capture failed; hiding boxes");
                    true
                }
            },
            _ => true,
        };
        self.memo.set(Some((Instant::now(), v)));
        v
    }

    /// `thumb_sum` of the fingerprint column, read from the screen with one BitBlt.
    fn strip_sum(&self, reg: &Region) -> Option<u64> {
        let (x0, sw) = self.strip;
        if sw == 0 || x0.checked_add(sw)? > reg.w {
            return None;
        }
        let col = Region { x: reg.x + x0 as i32, y: reg.y, w: sw, h: reg.h };
        capture(&col, &[(reg.y, reg.h)]).map(|b| thumb_sum(&b, sw, reg.h))
    }
}

/// The fingerprint column (x offset into the region, width): STRIP_W wide, centred
/// on the median word centre so a scroll moves text through it; the region centre
/// when there are no words. Clamped inside the region.
pub(crate) fn strip_of(words: &[OcrWord], reg_x: f64, reg_w: u32) -> (u32, u32) {
    let sw = STRIP_W.min(reg_w);
    let mut cx: Vec<f64> = words.iter().map(|w| w.rect.x + w.rect.w / 2.0 - reg_x).collect();
    cx.sort_by(|a, b| a.total_cmp(b));
    let mid = cx.get(cx.len() / 2).copied().unwrap_or(reg_w as f64 / 2.0);
    let x0 = (mid - sw as f64 / 2.0).round().clamp(0.0, (reg_w - sw) as f64);
    (x0 as u32, sw)
}

/// What hover does with the cache.
#[derive(Debug, PartialEq)]
pub(crate) enum HoverPlan {
    /// The cached OCR is current: look the point up in it.
    Use,
    /// Stale, but re-OCR'd less than 500 ms ago: no box for now.
    NoBox,
    /// Another window, no cache, or stale and old enough: OCR again.
    Rebuild,
}

/// Hover's cache decision. `stale` is only asked when the cache is for this window.
pub(crate) fn hover_plan(same_window: bool, stale: impl FnOnce() -> bool, age: Duration) -> HoverPlan {
    if !same_window {
        HoverPlan::Rebuild
    } else if !stale() {
        HoverPlan::Use
    } else if age < STALE_EVERY {
        HoverPlan::NoBox
    } else {
        HoverPlan::Rebuild
    }
}

impl OcrRead {
    /// A read with no words that is always stale (its rect is empty), so a failed
    /// OCR is cached and retried at most every 500 ms instead of on every move.
    fn failed(hwnd: isize) -> OcrRead {
        OcrRead {
            hwnd,
            words: Vec::new(),
            toks: Vec::new(),
            start: 0,
            next: 0,
            at_end: false,
            orig: (Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 }, 0),
            strip: (0, 0),
            made: Instant::now(),
            memo: Cell::new(None),
        }
    }
}

/// Hover in pick mode: the box of the OCR word under the point. OCRs once per window;
/// a stale cache is re-OCR'd at most every 500 ms (until then: no box). A failed OCR
/// is cached too (as an empty read: `None` now, then no box until it is retried).
pub fn hover_cached(cache: &mut Option<OcrRead>, hwnd: isize, x: i32, y: i32) -> Option<Vec<Rect>> {
    let plan = match cache.as_ref() {
        Some(c) => hover_plan(c.hwnd == hwnd, || c.stale(), c.made.elapsed()),
        None => HoverPlan::Rebuild,
    };
    match plan {
        HoverPlan::NoBox => return Some(Vec::new()),
        HoverPlan::Use => {}
        HoverPlan::Rebuild => {
            *cache = None;
            *cache = Some(match OcrRead::build(hwnd) {
                Ok(r) => r,
                Err(e) => {
                    log::info!("read-aloud: OCR hover: {e}");
                    *cache = Some(OcrRead::failed(hwnd));
                    return None;
                }
            });
        }
    }
    let c = cache.as_ref()?;
    Some(
        c.words
            .iter()
            .find(|w| inside(&w.rect, x as f64, y as f64))
            .map(|w| vec![w.rect.clone()])
            .unwrap_or_default(),
    )
}

// ---------------------------------------------------------------- pure logic

fn inside(r: &Rect, x: f64, y: f64) -> bool {
    x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h
}

/// Distance from `v` to the span [a, a + len] (0 inside).
fn span_dist(v: f64, a: f64, len: f64) -> f64 {
    if v < a {
        a - v
    } else if v > a + len {
        v - (a + len)
    } else {
        0.0
    }
}

/// The word containing the point, else the nearest word on the closest line, else None.
pub(crate) fn start_index(words: &[OcrWord], x: f64, y: f64) -> Option<usize> {
    if let Some(i) = words.iter().position(|w| inside(&w.rect, x, y)) {
        return Some(i);
    }
    let line = words
        .iter()
        .min_by(|a, b| span_dist(y, a.rect.y, a.rect.h).total_cmp(&span_dist(y, b.rect.y, b.rect.h)))?
        .line;
    words
        .iter()
        .enumerate()
        .filter(|(_, w)| w.line == line)
        .min_by(|(_, a), (_, b)| span_dist(x, a.rect.x, a.rect.w).total_cmp(&span_dist(x, b.rect.x, b.rect.w)))
        .map(|(i, _)| i)
}

/// (start, len) spans of one axis: each ≤ max, neighbours overlapping by 64 px.
fn spans(len: u32, max: u32) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    if len == 0 {
        return out;
    }
    let max = max.max(1);
    let step = if max > 2 * OVERLAP { max - OVERLAP } else { (max / 2).max(1) };
    let mut s = 0u32;
    loop {
        let e = s.saturating_add(max).min(len);
        out.push((s, e - s));
        if e >= len {
            return out;
        }
        s += step;
    }
}

/// Tiles (x, y, w, h) covering a w x h image, row-major, each side ≤ max, 64 px overlap.
pub(crate) fn tiles(w: u32, h: u32, max: u32) -> Vec<(u32, u32, u32, u32)> {
    let xs = spans(w, max);
    spans(h, max)
        .into_iter()
        .flat_map(|(y, th)| xs.iter().map(move |&(x, tw)| (x, y, tw, th)))
        .collect()
}

/// FNV-1a over the B, G, R bytes of a 64x64 grid of sample pixels. Never panics
/// (missing pixels hash as a marker).
pub(crate) fn thumb_sum(bgra: &[u8], w: u32, h: u32) -> u64 {
    let mut sum: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |b: u8| {
        sum ^= b as u64;
        sum = sum.wrapping_mul(0x0000_0100_0000_01b3);
    };
    if w == 0 || h == 0 {
        return sum;
    }
    for gy in 0..THUMB as u64 {
        let py = gy * h as u64 / THUMB as u64;
        for gx in 0..THUMB as u64 {
            let px = gx * w as u64 / THUMB as u64;
            let i = ((py * w as u64 + px) * 4) as usize;
            match bgra.get(i..i + 3) {
                Some(p) => p.iter().for_each(|&b| eat(b)),
                None => eat(0xA5),
            }
        }
    }
    sum
}

/// Intersection area over the smaller area (0 for an empty rect).
fn overlap_frac(a: &Rect, b: &Rect) -> f64 {
    let iw = (a.x + a.w).min(b.x + b.w) - a.x.max(b.x);
    let ih = (a.y + a.h).min(b.y + b.h) - a.y.max(b.y);
    let small = (a.w * a.h).min(b.w * b.h);
    if iw <= 0.0 || ih <= 0.0 || small <= 0.0 {
        return 0.0;
    }
    iw * ih / small
}

/// Drop words duplicated in a tile overlap: same text, rects overlapping > 50%.
pub(crate) fn dedupe_tiles(words: Vec<OcrWord>) -> Vec<OcrWord> {
    let mut out: Vec<OcrWord> = Vec::with_capacity(words.len());
    for w in words {
        if !out.iter().any(|k| k.text == w.text && overlap_frac(&k.rect, &w.rect) > 0.5) {
            out.push(w);
        }
    }
    out
}

/// Stale if the window rect moved or resized, or the checksum differs.
pub(crate) fn is_stale(orig: (Rect, u64), now: (Rect, u64)) -> bool {
    orig != now
}

fn union(a: &Rect, b: &Rect) -> Rect {
    let (x, y) = (a.x.min(b.x), a.y.min(b.y));
    Rect {
        x,
        y,
        w: (a.x + a.w).max(b.x + b.w) - x,
        h: (a.y + a.h).max(b.y + b.h) - y,
    }
}

/// Words grouped into consecutive same-`line` runs: (range, bounding box).
fn line_runs(words: &[OcrWord]) -> Vec<(std::ops::Range<usize>, Rect)> {
    let mut out: Vec<(std::ops::Range<usize>, Rect)> = Vec::new();
    for (i, w) in words.iter().enumerate() {
        match out.last_mut() {
            Some((r, b)) if words[r.start].line == w.line => {
                r.end = i + 1;
                *b = union(b, &w.rect);
            }
            _ => out.push((i..i + 1, w.rect.clone())),
        }
    }
    out
}

/// A new paragraph between two consecutive lines: a jump back up (next column) or a
/// vertical gap of about a line height.
fn para_gap(a: &Rect, b: &Rect) -> bool {
    b.y < a.y || b.y - (a.y + a.h) > 0.9 * a.h.max(b.h)
}

/// Raw tokens shaped like UIA's for `ra_text::normalize`, one per word: `"word "`;
/// a paragraph's last word ends in `"\n"`; a soft line wrap stays `"word "`, except a
/// line-end `"hyphen-"` keeps no space so normalize joins it with the next word.
pub(crate) fn tokens(words: &[OcrWord]) -> Vec<String> {
    let mut out = Vec::with_capacity(words.len());
    let runs = line_runs(words);
    for (k, (range, b)) in runs.iter().enumerate() {
        for i in range.clone() {
            let t = words[i].text.trim();
            let tok = if i + 1 < range.end {
                format!("{t} ")
            } else if runs.get(k + 1).map_or(true, |(_, nb)| para_gap(b, nb)) {
                format!("{t}\n")
            } else if t.len() > 1 && t.ends_with('-') {
                t.to_string()
            } else {
                format!("{t} ")
            };
            out.push(tok);
        }
    }
    out
}

/// Multi-tile only: join a line cut by a vertical tile edge with its continuation in
/// the next tile (vertical overlap > 50%, starting right of it, close by), keep tile
/// order otherwise, and renumber lines in reading order. `line_tile[line]` = tile.
pub(crate) fn join_tile_lines(words: Vec<OcrWord>, line_tile: &[usize]) -> Vec<OcrWord> {
    struct L {
        tile: usize,
        words: Vec<OcrWord>,
        b: Rect,
    }
    let mut lines: Vec<L> = Vec::new();
    for w in words {
        let tile = line_tile.get(w.line).copied().unwrap_or(usize::MAX);
        match lines.last_mut() {
            Some(l) if l.words.last().map(|p| p.line) == Some(w.line) => {
                l.b = union(&l.b, &w.rect);
                l.words.push(w);
            }
            _ => lines.push(L { tile, b: w.rect.clone(), words: vec![w] }),
        }
    }
    let mut out: Vec<L> = Vec::new();
    for l in lines {
        let joins = |k: &L| {
            let ov = (k.b.y + k.b.h).min(l.b.y + l.b.h) - k.b.y.max(l.b.y);
            let tall = k.b.h.max(l.b.h);
            k.tile != l.tile
                && ov > 0.5 * k.b.h.min(l.b.h)
                && l.b.x >= k.b.x + k.b.w / 2.0
                && l.b.x - (k.b.x + k.b.w) <= 3.0 * tall
        };
        match out.iter().rposition(joins) {
            Some(t) => {
                let k = &mut out[t];
                k.b = union(&k.b, &l.b);
                k.tile = l.tile;
                k.words.extend(l.words);
                k.words.sort_by(|a, b| a.rect.x.total_cmp(&b.rect.x));
            }
            None => out.push(l),
        }
    }
    out.into_iter()
        .enumerate()
        .flat_map(|(n, l)| {
            l.words.into_iter().map(move |mut w| {
                w.line = n;
                w
            })
        })
        .collect()
}

// ---------------------------------------------------------------- capture

/// A capture region in screen px, clipped to the virtual screen.
struct Region {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
}

/// The client area in screen px (`None` if the window is gone, minimised or empty).
fn client_rect(h: HWND) -> Option<Rect> {
    unsafe {
        if !IsWindow(h).as_bool() || IsIconic(h).as_bool() {
            return None;
        }
        let mut rc = RECT::default();
        GetClientRect(h, &mut rc).ok()?;
        let mut p = POINT { x: 0, y: 0 };
        if !ClientToScreen(h, &mut p).as_bool() {
            return None;
        }
        let (w, ht) = (rc.right - rc.left, rc.bottom - rc.top);
        if w <= 0 || ht <= 0 {
            return None;
        }
        Some(Rect { x: p.x as f64, y: p.y as f64, w: w as f64, h: ht as f64 })
    }
}

fn clip_to_screen(r: &Rect) -> Option<Region> {
    let (vx, vy, vw, vh) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    let x0 = (r.x as i64).max(vx as i64);
    let y0 = (r.y as i64).max(vy as i64);
    let x1 = ((r.x + r.w) as i64).min(vx as i64 + vw as i64);
    let y1 = ((r.y + r.h) as i64).min(vy as i64 + vh as i64);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(Region { x: x0 as i32, y: y0 as i32, w: (x1 - x0) as u32, h: (y1 - y0) as u32 })
}

struct ScreenDc(HDC);
impl Drop for ScreenDc {
    fn drop(&mut self) {
        unsafe {
            ReleaseDC(HWND::default(), self.0);
        }
    }
}
struct MemDc(HDC);
impl Drop for MemDc {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteDC(self.0);
        }
    }
}
struct Bitmap(HBITMAP);
impl Drop for Bitmap {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.0);
        }
    }
}
/// Restores the DC's previous object (so the bitmap can be read and deleted).
struct Selected(HDC, HGDIOBJ);
impl Drop for Selected {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.0, self.1);
        }
    }
}

/// Screen rows of the region, stacked top-down as BGRA (alpha forced to 255): each
/// band is (screen y, height), all `reg.w` wide. One band of the whole region is the
/// OCR capture; a one-band column is the staleness fingerprint.
fn capture(reg: &Region, bands: &[(i32, u32)]) -> Option<Vec<u8>> {
    let total: u32 = bands.iter().try_fold(0u32, |a, b| a.checked_add(b.1))?;
    if reg.w == 0 || total == 0 || reg.w > i32::MAX as u32 || total > i32::MAX as u32 {
        return None;
    }
    let (ow, oh) = (reg.w as i32, total as i32);
    unsafe {
        let screen = ScreenDc(GetDC(HWND::default()));
        if screen.0.is_invalid() {
            return None;
        }
        let mem = MemDc(CreateCompatibleDC(screen.0));
        if mem.0.is_invalid() {
            return None;
        }
        let bmp = Bitmap(CreateCompatibleBitmap(screen.0, ow, oh));
        if bmp.0.is_invalid() {
            return None;
        }
        {
            let old = SelectObject(mem.0, bmp.0);
            if old.is_invalid() {
                return None;
            }
            let _sel = Selected(mem.0, old);
            let mut dy = 0i32;
            for &(sy, bh) in bands {
                BitBlt(mem.0, 0, dy, ow, bh as i32, screen.0, reg.x, sy, SRCCOPY).ok()?;
                dy += bh as i32;
            }
        } // deselected: GetDIBits needs the bitmap out of any DC
        let mut bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: ow,
                biHeight: -oh, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let len = (reg.w as usize).checked_mul(total as usize)?.checked_mul(4)?;
        let mut buf = vec![0u8; len];
        let got = GetDIBits(mem.0, bmp.0, 0, total, Some(buf.as_mut_ptr().cast()), &mut bi, DIB_RGB_COLORS);
        if got != oh {
            return None;
        }
        buf.chunks_exact_mut(4).for_each(|p| p[3] = 255);
        Some(buf)
    }
}

/// The whole region (for OCR).
fn grab(reg: &Region) -> Option<Vec<u8>> {
    capture(reg, &[(reg.y, reg.h)])
}

/// Copy one tile out of a w-wide BGRA image.
fn crop(bgra: &[u8], w: u32, (x, y, tw, th): (u32, u32, u32, u32)) -> Vec<u8> {
    let mut out = Vec::with_capacity(tw as usize * th as usize * 4);
    for row in y..y + th {
        let a = (row as usize * w as usize + x as usize) * 4;
        if let Some(s) = bgra.get(a..a + tw as usize * 4) {
            out.extend_from_slice(s);
        }
    }
    out
}

/// One OCR pass over a BGRA image: lines of (word text, tile-local box).
fn recognize(engine: &OcrEngine, bgra: &[u8], w: u32, h: u32) -> windows::core::Result<Vec<Vec<(String, Rect)>>> {
    let bmp = {
        let dw = DataWriter::new()?;
        dw.WriteBytes(bgra)?;
        let buf = dw.DetachBuffer()?;
        SoftwareBitmap::CreateCopyFromBuffer(&buf, BitmapPixelFormat::Bgra8, w as i32, h as i32)?
    };
    let res = engine.RecognizeAsync(&bmp)?.get()?;
    let lines = res.Lines()?;
    let mut out = Vec::new();
    for i in 0..lines.Size()? {
        let words = lines.GetAt(i)?.Words()?;
        let mut line = Vec::new();
        for j in 0..words.Size()? {
            let wd = words.GetAt(j)?;
            let b = wd.BoundingRect()?;
            line.push((
                wd.Text()?.to_string(),
                Rect { x: b.X as f64, y: b.Y as f64, w: b.Width as f64, h: b.Height as f64 },
            ));
        }
        if !line.is_empty() {
            out.push(line);
        }
    }
    let _ = bmp.Close();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }
    fn wd(text: &str, line: usize, rect: Rect) -> OcrWord {
        OcrWord { text: text.into(), rect, line }
    }
    /// line 0: The(10..40) cat(50..80) sat(90..120) at y 10..30; line 1: on(10..30) mats(40..80) at y 40..60
    fn sample() -> Vec<OcrWord> {
        vec![
            wd("The", 0, r(10.0, 10.0, 30.0, 20.0)),
            wd("cat", 0, r(50.0, 10.0, 30.0, 20.0)),
            wd("sat", 0, r(90.0, 10.0, 30.0, 20.0)),
            wd("on", 1, r(10.0, 40.0, 20.0, 20.0)),
            wd("mats", 1, r(40.0, 40.0, 40.0, 20.0)),
        ]
    }

    #[test]
    fn start_index_containing_word() {
        assert_eq!(start_index(&sample(), 60.0, 20.0), Some(1));
        assert_eq!(start_index(&sample(), 45.0, 50.0), Some(4));
    }

    #[test]
    fn start_index_between_words_picks_nearest_on_line() {
        assert_eq!(start_index(&sample(), 84.0, 20.0), Some(1)); // 4 px from cat, 6 from sat
        assert_eq!(start_index(&sample(), 87.0, 20.0), Some(2));
        assert_eq!(start_index(&sample(), 500.0, 12.0), Some(2)); // far right of line 0
    }

    #[test]
    fn start_index_below_all_lines_uses_last_line() {
        assert_eq!(start_index(&sample(), 75.0, 200.0), Some(4));
        assert_eq!(start_index(&sample(), 0.0, 200.0), Some(3));
        assert_eq!(start_index(&sample(), 0.0, 0.0), Some(0)); // above: first line
    }

    #[test]
    fn start_index_empty_is_none() {
        assert_eq!(start_index(&[], 1.0, 1.0), None);
    }

    fn assert_covers(w: u32, h: u32, max: u32) -> Vec<(u32, u32, u32, u32)> {
        let t = tiles(w, h, max);
        let mut seen = vec![false; (w * h) as usize];
        for &(x, y, tw, th) in &t {
            assert!(tw <= max && th <= max && tw > 0 && th > 0, "tile {:?}", (x, y, tw, th));
            assert!(x + tw <= w && y + th <= h, "tile inside the image");
            for yy in y..y + th {
                for xx in x..x + tw {
                    seen[(yy * w + xx) as usize] = true;
                }
            }
        }
        assert!(!t.is_empty() && seen.iter().all(|&s| s), "every pixel covered");
        t
    }

    #[test]
    fn tiles_4k_cover_with_correct_offsets() {
        let t = assert_covers(3840, 2160, 2600);
        // 64 px overlap: the second tile starts at 2600 - 64
        assert_eq!(t, vec![(0, 0, 2600, 2160), (2536, 0, 1304, 2160)]);
    }

    #[test]
    fn tiles_large_both_axes_overlap_64() {
        let t = assert_covers(6000, 5300, 2600);
        let xs: Vec<u32> = t.iter().filter(|r| r.1 == 0).map(|r| r.0).collect();
        assert_eq!(xs, vec![0, 2536, 5072]);
        let ys: Vec<u32> = t.iter().filter(|r| r.0 == 0).map(|r| r.1).collect();
        assert_eq!(ys, vec![0, 2536, 5072]);
        // row-major order: all of row 0 first
        assert_eq!(t[1], (2536, 0, 2600, 2600));
    }

    #[test]
    fn tiles_small_is_one_tile() {
        assert_eq!(tiles(800, 600, 2600), vec![(0, 0, 800, 600)]);
        assert!(tiles(0, 600, 2600).is_empty());
        assert!(tiles(800, 0, 2600).is_empty());
        assert_covers(300, 200, 10); // a tiny max still terminates and covers
    }

    fn pattern(w: u32, h: u32) -> Vec<u8> {
        (0..w * h * 4).map(|i| (i * 7 % 251) as u8).collect()
    }

    #[test]
    fn thumb_sum_stable_and_sensitive() {
        let (w, h) = (200, 100);
        let a = pattern(w, h);
        assert_eq!(thumb_sum(&a, w, h), thumb_sum(&a.clone(), w, h));
        // (0,0) is always sampled
        let mut b = a.clone();
        b[0] ^= 0xFF;
        assert_ne!(thumb_sum(&a, w, h), thumb_sum(&b, w, h));
        // a sampled pixel in the middle
        let (sx, sy) = (32 * w / 64, 32 * h / 64);
        let mut c = a.clone();
        c[((sy * w + sx) * 4 + 1) as usize] ^= 0x10;
        assert_ne!(thumb_sum(&a, w, h), thumb_sum(&c, w, h));
        // x = 1 is between samples (0, 3, 6, ...) at w = 200: unchanged
        let mut d = a.clone();
        d[((w + 1) * 4) as usize] ^= 0xFF;
        assert_eq!(thumb_sum(&a, w, h), thumb_sum(&d, w, h));
    }

    #[test]
    fn thumb_sum_zero_size_and_short_buffer_do_not_panic() {
        let _ = thumb_sum(&[], 0, 0);
        let _ = thumb_sum(&[], 10, 10);
        let _ = thumb_sum(&[1, 2, 3], 100, 100);
        let _ = thumb_sum(&pattern(10, 10), 10, 0);
    }

    #[test]
    fn dedupe_drops_overlap_duplicates_only() {
        let words = vec![
            wd("word", 0, r(2550.0, 10.0, 40.0, 20.0)),
            wd("other", 0, r(2600.0, 10.0, 50.0, 20.0)),
            wd("word", 1, r(2552.0, 11.0, 40.0, 20.0)), // same word from the next tile
            wd("word", 2, r(100.0, 10.0, 40.0, 20.0)),  // same text, far away
            wd("wurd", 3, r(2551.0, 10.0, 40.0, 20.0)), // different text, overlapping
        ];
        let out = dedupe_tiles(words);
        let got: Vec<(&str, f64)> = out.iter().map(|w| (w.text.as_str(), w.rect.x)).collect();
        assert_eq!(got, vec![("word", 2550.0), ("other", 2600.0), ("word", 100.0), ("wurd", 2551.0)]);
    }

    #[test]
    fn dedupe_keeps_touching_same_words() {
        // "the the" side by side, overlapping a little (< 50%)
        let words = vec![wd("the", 0, r(0.0, 0.0, 30.0, 20.0)), wd("the", 0, r(25.0, 0.0, 30.0, 20.0))];
        assert_eq!(dedupe_tiles(words).len(), 2);
    }

    #[test]
    fn stale_when_moved_resized_or_changed() {
        let a = r(100.0, 100.0, 800.0, 600.0);
        assert!(!is_stale((a.clone(), 42), (a.clone(), 42)));
        assert!(is_stale((a.clone(), 42), (r(110.0, 100.0, 800.0, 600.0), 42)), "moved");
        assert!(is_stale((a.clone(), 42), (r(100.0, 100.0, 820.0, 600.0), 42)), "resized");
        assert!(is_stale((a.clone(), 42), (a.clone(), 43)), "content scrolled");
    }

    #[test]
    fn tokens_mirror_uia_convention() {
        let mut words = sample();
        // a paragraph gap, then a line ending in a hyphenated split
        words.push(wd("Next", 2, r(10.0, 100.0, 40.0, 20.0)));
        words.push(wd("hyphen-", 2, r(60.0, 100.0, 70.0, 20.0)));
        words.push(wd("ated", 3, r(10.0, 125.0, 40.0, 20.0)));
        words.push(wd("end.", 3, r(60.0, 125.0, 40.0, 20.0)));
        let t = tokens(&words);
        assert_eq!(t, vec!["The ", "cat ", "sat ", "on ", "mats\n", "Next ", "hyphen-", "ated ", "end.\n"]);
        let n = crate::ra_text::normalize(&t);
        let texts: Vec<&str> = n.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(texts, vec!["The", "cat", "sat", "on", "mats", "Next", "hyphen-ated", "end."]);
        assert!(n[5].para_break_before, "gap starts a paragraph");
        assert!(!n[3].para_break_before, "a soft line wrap is not a paragraph");
        assert_eq!(n[6].src, vec![6, 7], "one token per OCR word");
    }

    #[test]
    fn hover_plan_decisions() {
        let young = Duration::from_millis(100);
        let old = Duration::from_millis(600);
        assert_eq!(hover_plan(false, || panic!("not asked"), young), HoverPlan::Rebuild);
        assert_eq!(hover_plan(true, || false, old), HoverPlan::Use);
        assert_eq!(hover_plan(true, || true, young), HoverPlan::NoBox);
        assert_eq!(hover_plan(true, || true, old), HoverPlan::Rebuild);
    }

    #[test]
    fn failed_read_is_cached_and_throttled() {
        // a failed OCR is kept (hover returns early without re-OCR) until 500 ms pass
        let f = OcrRead::failed(12345);
        assert!(f.words.is_empty() && f.toks.is_empty());
        assert_eq!(hover_plan(f.hwnd == 12345, || true, f.made.elapsed()), HoverPlan::NoBox);
        assert_eq!(hover_plan(true, || true, Duration::from_millis(501)), HoverPlan::Rebuild);
    }

    #[test]
    fn strip_follows_the_text_and_stays_inside() {
        // words centred around x = 1000 (screen), region starts at 100
        let words = vec![
            wd("a", 0, r(900.0, 0.0, 40.0, 20.0)),
            wd("b", 0, r(980.0, 0.0, 40.0, 20.0)),
            wd("c", 0, r(1060.0, 0.0, 40.0, 20.0)),
        ];
        assert_eq!(strip_of(&words, 100.0, 1920), (900 - 16, 32));
        assert_eq!(strip_of(&[], 0.0, 1920), (960 - 16, 32), "no words: centre");
        let edge = vec![wd("z", 0, r(1910.0, 0.0, 10.0, 10.0))];
        assert_eq!(strip_of(&edge, 0.0, 1920), (1888, 32), "clamped right");
        assert_eq!(strip_of(&words, 5000.0, 1920).0, 0, "clamped left");
        assert_eq!(strip_of(&[], 0.0, 10), (0, 10), "narrow region");
    }

    #[test]
    fn a_rolled_back_batch_is_read_again_and_the_end_is_undone() {
        let s = |v: &[&str]| v.iter().map(|t| t.to_string()).collect::<Vec<_>>();
        let mut rd = OcrRead::for_test(9, &["a", "b", "c"]);
        let m = rd.mark();
        assert_eq!(rd.more(), Ok(s(&["a", "b", "c"])));
        rd.rollback(m);
        assert_eq!(rd.more(), Ok(s(&["a", "b", "c"])), "the undelivered batch comes again");
        let m = rd.mark();
        assert_eq!(rd.more(), Ok(vec![]));
        assert!(rd.at_end(), "an empty more() is the end of the screen");
        rd.rollback(m);
        assert!(!rd.at_end(), "an undelivered end is not the end");
        assert_eq!(rd.more(), Ok(vec![]));
        assert!(rd.at_end());
    }

    #[test]
    fn tokens_empty() {
        assert!(tokens(&[]).is_empty());
    }

    #[test]
    fn tile_line_fragments_are_joined_in_reading_order() {
        // tile 0 has lines 0 and 1, tile 1 (to the right) has their continuations 2 and 3
        let words = vec![
            wd("The", 0, r(2400.0, 10.0, 40.0, 20.0)),
            wd("quick", 0, r(2450.0, 10.0, 60.0, 20.0)),
            wd("lazy", 1, r(2400.0, 40.0, 50.0, 20.0)),
            wd("brown", 2, r(2520.0, 11.0, 60.0, 20.0)),
            wd("fox", 2, r(2590.0, 11.0, 40.0, 20.0)),
            wd("dog", 3, r(2460.0, 41.0, 40.0, 20.0)),
        ];
        let out = join_tile_lines(words, &[0, 0, 1, 1]);
        let got: Vec<(&str, usize)> = out.iter().map(|w| (w.text.as_str(), w.line)).collect();
        assert_eq!(got, vec![("The", 0), ("quick", 0), ("brown", 0), ("fox", 0), ("lazy", 1), ("dog", 1)]);
    }

    #[test]
    fn tile_lines_in_one_tile_are_not_joined() {
        let words = vec![wd("left", 0, r(0.0, 10.0, 40.0, 20.0)), wd("right", 1, r(60.0, 10.0, 40.0, 20.0))];
        let out = join_tile_lines(words, &[0, 0]);
        let lines: Vec<usize> = out.iter().map(|w| w.line).collect();
        assert_eq!(lines, vec![0, 1]);
    }

    // ------------------------------------------------------------ manual

    const LIVE_LINES: [&str; 3] = [
        "The quick brown fox jumps over the lazy dog.",
        "Pack my box with five dozen liquor jugs today.",
        "Reading aloud from pixels works fine.",
    ];
    const LIVE_FONT_PX: i32 = 56;
    const LIVE_LINE_PX: i32 = 90;
    const LIVE_MARGIN: i32 = 60;
    /// Vertical paint offset of the test text ("scroll" it to make the read stale).
    static LIVE_SCROLL: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

    unsafe extern "system" fn live_wndproc(
        h: windows::Win32::Foundation::HWND,
        msg: u32,
        wp: windows::Win32::Foundation::WPARAM,
        lp: windows::Win32::Foundation::LPARAM,
    ) -> windows::Win32::Foundation::LRESULT {
        use windows::core::w;
        use windows::Win32::Graphics::Gdi::*;
        use windows::Win32::UI::WindowsAndMessaging::*;
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(h, &mut ps);
                let mut rc = windows::Win32::Foundation::RECT::default();
                let _ = GetClientRect(h, &mut rc);
                FillRect(dc, &rc, HBRUSH(GetStockObject(WHITE_BRUSH).0));
                let font = CreateFontW(
                    -LIVE_FONT_PX, 0, 0, 0, 400, 0, 0, 0, 0, 0, 0, 5, 0, w!("Segoe UI"),
                );
                let old = SelectObject(dc, font);
                SetBkMode(dc, TRANSPARENT);
                for (i, line) in LIVE_LINES.iter().enumerate() {
                    let mut t: Vec<u16> = line.encode_utf16().collect();
                    let top = LIVE_MARGIN
                        + i as i32 * LIVE_LINE_PX
                        + LIVE_SCROLL.load(std::sync::atomic::Ordering::Relaxed);
                    let mut r = windows::Win32::Foundation::RECT {
                        left: LIVE_MARGIN,
                        top,
                        right: rc.right,
                        bottom: top + LIVE_LINE_PX,
                    };
                    DrawTextW(dc, &mut t, &mut r, DT_LEFT | DT_TOP | DT_NOPREFIX | DT_SINGLELINE);
                }
                SelectObject(dc, old);
                let _ = DeleteObject(font);
                let _ = EndPaint(h, &ps);
                windows::Win32::Foundation::LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                windows::Win32::Foundation::LRESULT(0)
            }
            _ => DefWindowProcW(h, msg, wp, lp),
        }
    }

    /// Width in px of `s` in the test font.
    fn live_text_width(s: &str) -> i32 {
        use windows::core::w;
        use windows::Win32::Graphics::Gdi::*;
        unsafe {
            let dc = CreateCompatibleDC(HDC::default());
            let font = CreateFontW(-LIVE_FONT_PX, 0, 0, 0, 400, 0, 0, 0, 0, 0, 0, 5, 0, w!("Segoe UI"));
            let old = SelectObject(dc, font);
            let t: Vec<u16> = s.encode_utf16().collect();
            let mut sz = windows::Win32::Foundation::SIZE::default();
            let _ = GetTextExtentPoint32W(dc, &t, &mut sz);
            SelectObject(dc, old);
            let _ = DeleteObject(font);
            let _ = DeleteDC(dc);
            sz.cx
        }
    }

    /// Fraction of `expected` found in `got`, in order (greedy subsequence match).
    fn in_order_ratio(expected: &[&str], got: &[String]) -> f64 {
        let norm = |s: &str| s.trim().trim_end_matches(['.', ',']).to_lowercase();
        let mut gi = 0;
        let mut hits = 0;
        for e in expected {
            if let Some(k) = got[gi..].iter().position(|g| norm(g) == norm(e)) {
                hits += 1;
                gi += k + 1;
            }
        }
        hits as f64 / expected.len().max(1) as f64
    }

    /// Manual: cargo test ocr_live -- --ignored --nocapture
    /// Creates its own topmost (no-activate) window of known text, OCRs it, closes it.
    /// Never touches any other window.
    #[test]
    #[ignore]
    fn ocr_live() {
        use std::sync::mpsc;
        use windows::core::w;
        use windows::Win32::Foundation::HINSTANCE;
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
        use windows::Win32::UI::HiDpi::{
            SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };
        use windows::Win32::UI::WindowsAndMessaging::*;

        unsafe {
            let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        // a 4K-sized window, clipped to the virtual screen (the capture is clipped too)
        let (sw, sh) = unsafe { (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN)) };
        let (vx, vy) = unsafe { (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN)) };
        let (wd, ht) = (3840.min(sw), 2160.min(sh));
        let (htx, hrx) = mpsc::channel::<isize>();
        let win = std::thread::spawn(move || unsafe {
            let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let wc = WNDCLASSW {
                lpfnWndProc: Some(live_wndproc),
                lpszClassName: w!("RaOcrLiveTestWnd"),
                ..Default::default()
            };
            RegisterClassW(&wc);
            let h = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("RaOcrLiveTestWnd"),
                w!("ra_ocr live test"),
                WS_POPUP | WS_VISIBLE,
                vx,
                vy,
                wd,
                ht,
                None,
                None,
                HINSTANCE::default(),
                None,
            )
            .expect("test window");
            let _ = htx.send(h.0 as isize);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        });
        let h = hrx.recv().unwrap();
        std::thread::sleep(Duration::from_millis(700)); // painted and composed

        // click in the middle of "brown" (line 0, after "The quick ")
        let x = vx + LIVE_MARGIN + live_text_width("The quick ") + live_text_width("brown") / 2;
        let y = vy + LIVE_MARGIN + LIVE_FONT_PX / 2 + 8;
        let t0 = Instant::now();
        let picked = OcrRead::pick(h, x, y);
        let pick_ms = t0.elapsed().as_millis();
        let (mut read, first) = match picked {
            Ok(v) => v,
            Err(e) => {
                close_live(h, win);
                panic!("pick failed: {e}");
            }
        };
        let mut all = first.clone();
        loop {
            let m = read.more().unwrap();
            if m.is_empty() {
                break;
            }
            all.extend(m);
        }
        let first_rect = read.rects(&[0]);
        // the warm-cache staleness recheck (memo expired): fingerprint + compare
        let mut recheck = Vec::new();
        for _ in 0..5 {
            std::thread::sleep(Duration::from_millis(520));
            let t = Instant::now();
            let r = read.rects(&[0]);
            recheck.push(t.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(r.map(|v| v.len()), Some(1), "unchanged window is not stale");
        }
        let mut cache = None;
        let t1 = Instant::now();
        let hov = hover_cached(&mut cache, h, x, y);
        let hover_ms = t1.elapsed().as_millis();
        let t2 = Instant::now();
        let hov2 = hover_cached(&mut cache, h, x, y);
        let hover2_ms = t2.elapsed().as_millis();
        // "scroll" the content by 30 px: the read must go stale (no boxes)
        LIVE_SCROLL.store(30, std::sync::atomic::Ordering::Relaxed);
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::InvalidateRect(
                windows::Win32::Foundation::HWND(h as *mut _),
                None,
                true,
            );
        }
        std::thread::sleep(Duration::from_millis(600)); // repainted, past the memo
        let after_scroll = read.rects(&[0]);
        close_live(h, win);
        std::thread::sleep(Duration::from_millis(600)); // past the staleness memo
        let after_close = read.rects(&[0]);

        println!("ocr_live: window {wd}x{ht}, pick {pick_ms} ms, click ({x},{y})");
        println!("ocr_live: tokens {all:?}");
        println!("ocr_live: first rect {first_rect:?}; hover {hover_ms} ms then {hover2_ms} ms (cached)");
        println!("ocr_live: staleness recheck ms {recheck:.2?}");
        println!("ocr_live: {} paragraphs", all.iter().filter(|t| t.ends_with('\n')).count());
        let expected: Vec<&str> = LIVE_LINES
            .iter()
            .flat_map(|l| l.split(' '))
            .skip(2) // start at "brown"
            .collect();
        let ratio = in_order_ratio(&expected, &all);
        println!("ocr_live: {:.0}% of expected words in order", ratio * 100.0);
        assert!(ratio >= 0.8, "≥ 80% of the expected words, in order");
        assert_eq!(all.first().map(|s| s.trim().to_lowercase()), Some("brown".into()), "starts at the clicked word");
        assert!(read.at_end(), "more() reached the end");
        let r = first_rect.expect("rects answered").into_iter().next().expect("a box");
        assert!(crate::ra_uia::contains(&[r], x as f64, y as f64), "box of the clicked word holds the click");
        assert_eq!(hov.map(|v| v.len()), Some(1), "hover finds the word");
        assert_eq!(hov2.map(|v| v.len()), Some(1), "cached hover finds the word");
        assert_eq!(after_scroll, Some(Vec::new()), "scrolled content -> stale -> no boxes");
        assert_eq!(after_close, Some(Vec::new()), "closed window -> stale -> no boxes");
        // one GDI screen read costs ~7 ms whatever its size (DWM readback); 25 ms is
        // half the hover timeout and leaves room for a loaded machine
        assert!(recheck.iter().all(|&ms| ms < 25.0), "staleness recheck well under the hover timeout");
    }

    fn close_live(h: isize, win: std::thread::JoinHandle<()>) {
        use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
        unsafe {
            let _ = PostMessageW(HWND(h as *mut _), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        let _ = win.join();
    }
}
