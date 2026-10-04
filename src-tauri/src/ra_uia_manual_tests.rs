//! Manual (#[ignore]) checks for ra_uia against real apps. They open their own windows
//! (Notepad / a browser on a throwaway profile), never touch the user's, and clean up.
//! Run: cargo test ra_uia -- --ignored --nocapture --test-threads=1
//! The Notepad tests refuse to run if any Notepad is already open. (The PickHook
//! click-swallow check lives in ra_hook's own manual test.)

use super::*;
use std::process::Command;
use std::time::Instant;
use windows::Win32::Foundation::{BOOL, LPARAM, RECT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowRect, GetWindowTextW, IsWindow, IsWindowVisible, MoveWindow,
    PostMessageW, SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WM_CLOSE,
};

/// Every Notepad temp file of these tests starts with this (also used to find our tabs).
const NP_PREFIX: &str = "ra_uia_t";

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

fn new_uia() -> IUIAutomation {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).unwrap()
    }
}

/// Win11 Notepad restores its previous session: closing a window only saves its tabs for
/// next time. So first close OUR tabs (by name, via UIA: select the tab, invoke its close
/// button), then WM_CLOSE the window. The user's own restored tabs stay as they were.
fn close_our_tabs(hwnd: HWND) {
    let uia = new_uia();
    unsafe {
        let Ok(root) = uia.ElementFromHandle(hwnd) else {
            return;
        };
        let by_type = |t: UIA_CONTROLTYPE_ID| {
            uia.CreatePropertyCondition(UIA_ControlTypePropertyId, &VARIANT::from(t.0))
        };
        let (Ok(tab_c), Ok(btn_c)) = (
            by_type(UIA_TabItemControlTypeId),
            by_type(UIA_ButtonControlTypeId),
        ) else {
            return;
        };
        let named = |e: &IUIAutomationElement, s: &str| {
            e.CurrentName()
                .map(|n| n.to_string().contains(s))
                .unwrap_or(false)
        };
        for _ in 0..20 {
            let Ok(tabs) = root.FindAll(TreeScope_Descendants, &tab_c) else {
                return;
            };
            let Some(tab) = (0..tabs.Length().unwrap_or(0))
                .filter_map(|i| tabs.GetElement(i).ok())
                .find(|t| named(t, NP_PREFIX))
            else {
                return;
            };
            if let Ok(sel) = tab.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(
                UIA_SelectionItemPatternId,
            ) {
                let _ = sel.Select();
            }
            std::thread::sleep(Duration::from_millis(400));
            let Ok(btns) = tab.FindAll(TreeScope_Descendants, &btn_c) else {
                return;
            };
            let inv = (0..btns.Length().unwrap_or(0))
                .filter_map(|i| btns.GetElement(i).ok())
                .find(|b| named(b, "Close"))
                .and_then(|b| {
                    b.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)
                        .ok()
                });
            let Some(inv) = inv else { return };
            let _ = inv.Invoke();
            std::thread::sleep(Duration::from_millis(400));
        }
    }
}

/// Closes our Notepad tabs + windows (only them), then deletes the temp files.
struct Cleanup {
    hwnds: Vec<HWND>,
    files: Vec<std::path::PathBuf>,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        let mut pids = Vec::new();
        for &h in &self.hwnds {
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
            pids.push(pid);
            close_our_tabs(h);
            unsafe {
                let _ = PostMessageW(h, WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        if !self.hwnds.is_empty() {
            std::thread::sleep(Duration::from_millis(1500));
        }
        // A "save changes?" prompt means stray keystrokes landed in one of OUR windows
        // (its untitled tab); discard them so nothing of ours is left in Notepad's session.
        for &h in &self.hwnds {
            if unsafe { IsWindow(h) }.as_bool() && invoke_named(h, "Don't save") {
                std::thread::sleep(Duration::from_millis(1000));
            }
        }
        // No Notepad existed before the test (checked), so a straggler is ours.
        let alive = notepad_pids();
        for pid in pids {
            if pid != 0 && alive.contains(&pid) {
                let _ = Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/F"])
                    .output();
            }
        }
        for f in &self.files {
            let _ = std::fs::remove_file(f);
        }
    }
}

fn require_no_notepad() {
    assert!(
        notepad_pids().is_empty(),
        "close Notepad first; this test must not touch your windows"
    );
}

/// Launch Notepad on `file` and wait for a window whose title contains `stem`.
#[allow(clippy::zombie_processes)] // the notepad.exe stub hands off; we close by window
fn open_in_notepad(file: &std::path::Path, stem: &str) -> HWND {
    Command::new("notepad.exe")
        .arg(file)
        .spawn()
        .expect("launch notepad");
    let t0 = Instant::now();
    loop {
        if let Some(h) = find_window(stem) {
            return h;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(15),
            "Notepad window not found"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Put our window on top WITHOUT activating it or sending any keys (the user may be
/// typing elsewhere; never steal focus or inject keystrokes). The window is closed at
/// the end, so its topmost flag goes with it.
fn raise(hwnd: HWND) {
    // A freshly launched (WinUI) Notepad can re-order itself while starting up, so
    // apply twice with a short gap.
    for _ in 0..2 {
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Invoke a named control in our own window via UIA (menu items, dialog buttons).
fn invoke_named(hwnd: HWND, name: &str) -> bool {
    let uia = new_uia();
    unsafe {
        let Ok(root) = uia.ElementFromHandle(hwnd) else {
            return false;
        };
        let Ok(c) = uia.CreatePropertyCondition(
            UIA_NamePropertyId,
            &VARIANT::from(windows::core::BSTR::from(name)),
        ) else {
            return false;
        };
        let Ok(e) = root.FindFirst(TreeScope_Descendants, &c) else {
            return false;
        };
        if let Ok(ec) =
            e.GetCurrentPatternAs::<IUIAutomationExpandCollapsePattern>(UIA_ExpandCollapsePatternId)
        {
            return ec.Expand().is_ok();
        }
        e.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)
            .and_then(|i| i.Invoke())
            .is_ok()
    }
}

/// Of all TextPattern elements in `hwnd`, the one with the longest text (as the spike did).
fn main_text(uia: &IUIAutomation, hwnd: HWND) -> (IUIAutomationElement, IUIAutomationTextPattern) {
    let root = unsafe { uia.ElementFromHandle(hwnd) }.unwrap();
    let cond = text_pattern_condition(uia).unwrap();
    let all = unsafe { root.FindAll(TreeScope_Descendants, &cond) }.expect("TextPattern elements");
    (0..unsafe { all.Length() }.unwrap_or(0))
        .filter_map(|i| unsafe { all.GetElement(i) }.ok())
        .filter_map(|e| {
            let tp: IUIAutomationTextPattern =
                unsafe { e.GetCurrentPatternAs(UIA_TextPatternId) }.ok()?;
            let n = unsafe { tp.DocumentRange().ok()?.GetText(200_000).ok()?.len() };
            Some((n, e, tp))
        })
        .max_by_key(|(n, _, _)| *n)
        .map(|(_, e, tp)| (e, tp))
        .expect("a TextPattern element with text")
}

/// Text and rects of word `n` (0-based) of a TextPattern's document.
fn nth_word(tp: &IUIAutomationTextPattern, n: i32) -> (String, Vec<Rect>) {
    unsafe {
        let r = tp.DocumentRange().unwrap();
        r.MoveEndpointByRange(
            TextPatternRangeEndpoint_End,
            &r,
            TextPatternRangeEndpoint_Start,
        )
        .unwrap();
        r.Move(TextUnit_Word, n).unwrap();
        r.ExpandToEnclosingUnit(TextUnit_Word).unwrap();
        (r.GetText(-1).unwrap().to_string(), rects_of(&r).unwrap_or_default())
    }
}

fn center(r: &Rect) -> (i32, i32) {
    ((r.x + r.w / 2.0) as i32, (r.y + r.h / 2.0) as i32)
}

fn window_rect(h: HWND) -> Rect {
    let mut wr = RECT::default();
    unsafe {
        let _ = GetWindowRect(h, &mut wr);
    }
    to_rect(wr)
}

fn hover_retry(ra: &Uia, x: i32, y: i32) -> Option<Vec<Rect>> {
    // 50 ms budget; the first call may be cold (None = no answer in time).
    (0..5).find_map(|_| ra.hover(x, y).filter(|v| !v.is_empty()))
}

#[test]
#[ignore]
fn uia_notepad_manual() {
    unsafe {
        let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    require_no_notepad();
    let stem = format!("{NP_PREFIX}est_{}", std::process::id());
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
        hwnds: vec![],
        files: vec![file.clone()],
    };
    let hwnd = open_in_notepad(&file, &stem);
    guard.hwnds.push(hwnd);
    raise(hwnd);
    std::thread::sleep(Duration::from_millis(1000));

    // Ground truth from a separate UIA client on this thread (as the spike's probe.py does).
    let uia = new_uia();
    let (doc_el, tp) = main_text(&uia, hwnd);
    let el_rect = to_rect(unsafe { doc_el.CurrentBoundingRectangle() }.unwrap());
    let (w5_text, w5_rects) = nth_word(&tp, 4);
    println!("element {el_rect:?}\n5th word {w5_text:?} rects {w5_rects:?}");
    assert!(!w5_rects.is_empty(), "5th word has no rect");
    let (cx, cy) = center(&w5_rects[0]);

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
    assert_eq!(ra.source_hwnd(), hwnd.0 as isize);

    let r0 = ra.rects(vec![0]).expect("rects(0) answered");
    println!("rects(0) = {r0:?}");
    assert!(
        !r0.is_empty() && contains(&r0, cx as f64, cy as f64),
        "rects(0) must cover the click"
    );
    let multi = ra.rects(vec![0, 1, 2]).expect("rects answered");
    assert!(multi.len() >= 3, "3 words -> >= 3 rects, got {multi:?}");
    assert_eq!(ra.rects(vec![999_999]), None, "unknown index: no answer, not 'off screen'");
    // I-4: a burst of rects requests is coalesced; only the newest is answered, and an
    // answered request is never an empty "off screen" result for an on-screen word.
    let burst: Vec<_> = std::thread::scope(|sc| {
        let hs: Vec<_> = (0..8).map(|_| sc.spawn(|| ra.rects(vec![0]))).collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    println!("rects burst: {:?}", burst.iter().map(|r| r.as_ref().map(|v| v.len())).collect::<Vec<_>>());
    assert!(burst.iter().flatten().all(|v| !v.is_empty()), "no spurious empty rects");

    let more = ra.more().expect("more()");
    println!("more() -> {} tokens", more.len());
    assert!(more.is_empty(), "short doc: everything came back in pick()");

    let hov = hover_retry(&ra, cx, cy);
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
    assert!(ra.hover(blank.0, blank.1).is_none_or(|v| v.is_empty()));
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
        !ra.rects(vec![0]).unwrap_or_default().is_empty(),
        "worker still alive after scroll_to; old read kept"
    );
    assert_eq!(
        ra.source_hwnd(),
        hwnd.0 as isize,
        "failed picks keep the source"
    );
    drop(ra);
}

/// Two overlapping Notepad windows: the window UNDER THE POINT is read; text of the
/// lower window hidden under the top one is never read.
#[test]
#[ignore]
fn uia_two_notepads_manual() {
    unsafe {
        let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    require_no_notepad();
    let pid = std::process::id();
    let (stem_a, stem_b) = (
        format!("{NP_PREFIX}woA_{pid}"),
        format!("{NP_PREFIX}woB_{pid}"),
    );
    let dir = std::env::temp_dir();
    let (file_a, file_b) = (
        dir.join(format!("{stem_a}.txt")),
        dir.join(format!("{stem_b}.txt")),
    );
    let lines: String = (0..60)
        .map(|i| format!("Alpha{i:02} covered lower window text text text text text text text text text.\r\n"))
        .collect();
    std::fs::write(&file_a, lines).unwrap();
    std::fs::write(&file_b, "Bravo one two three four five.\r\n").unwrap();
    let mut guard = Cleanup {
        hwnds: vec![],
        files: vec![file_a.clone(), file_b.clone()],
    };

    // Window A; then File > New window (invoked via UIA, no keystrokes) opens a 2nd
    // Notepad window, and B opens as a tab there (Notepad sends files to its most
    // recently opened window).
    let wa = open_in_notepad(&file_a, &stem_a);
    guard.hwnds.push(wa);
    std::thread::sleep(Duration::from_millis(800));
    assert!(invoke_named(wa, "File"), "expand Notepad's File menu");
    std::thread::sleep(Duration::from_millis(600));
    assert!(invoke_named(wa, "New window"), "File > New window");
    let t0 = Instant::now();
    let untitled = loop {
        if let Some(h) = find_window("Untitled - Notepad") {
            break h;
        }
        assert!(t0.elapsed() < Duration::from_secs(10), "new Notepad window");
        std::thread::sleep(Duration::from_millis(200));
    };
    guard.hwnds.insert(0, untitled);
    let wb = open_in_notepad(&file_b, &stem_b);
    assert_eq!(wb, untitled, "B should open as a tab in the new window");
    unsafe {
        let _ = MoveWindow(wa, 100, 100, 1000, 750, true);
        let _ = MoveWindow(wb, 450, 300, 900, 650, true);
    }
    raise(wa);
    raise(wb); // B above A
    std::thread::sleep(Duration::from_millis(1000));
    let (ra_rect, rb_rect) = (window_rect(wa), window_rect(wb));
    println!("A {ra_rect:?}\nB {rb_rect:?} (on top)");

    let uia = new_uia();
    let (_, tp_a) = main_text(&uia, wa);
    let (b_el, tp_b) = main_text(&uia, wb);

    let ra = Uia::start(wa.0 as isize); // hotkey-time foreground could be either window
    assert_eq!(ra.source_hwnd(), wa.0 as isize);

    // 1. A word of B that lies over A: reads B (the top window), source becomes B.
    let (b_word, b_rects) = nth_word(&tp_b, 1);
    let (bx, by) = center(&b_rects[0]);
    assert!(
        contains(std::slice::from_ref(&ra_rect), bx as f64, by as f64),
        "test setup: B's word must lie over window A"
    );
    let tokens = ra.pick(bx, by).expect("pick B's word");
    println!(
        "pick B word {b_word:?} at ({bx},{by}) -> {:?}",
        &tokens[..tokens.len().min(6)]
    );
    assert_eq!(tokens[0].trim(), b_word.trim());
    assert!(
        tokens.iter().all(|t| !t.contains("Alpha")),
        "must not read A"
    );
    assert_eq!(
        ra.source_hwnd(),
        wb.0 as isize,
        "source = window under the click"
    );

    // 2. B's blank area where A's text sits underneath: rejected, A's hidden text not read.
    let b_text = to_rect(unsafe { b_el.CurrentBoundingRectangle() }.unwrap());
    let covered = (0..40)
        .map(|k| {
            (
                b_text.x as i32 + 30,
                (b_text.y + b_text.h) as i32 - 20 - k * 4,
            )
        })
        .find(|&(x, y)| unsafe {
            let Ok(r) = tp_a.RangeFromPoint(POINT { x, y }) else {
                return false;
            };
            let _ = r.ExpandToEnclosingUnit(TextUnit_Word);
            let rc = rects_of(&r).unwrap_or_default();
            contains(&rc, x as f64, y as f64)
                && contains(std::slice::from_ref(&ra_rect), x as f64, y as f64)
        })
        .expect("test setup: a point where A's text lies under B's blank area");
    let res = ra.pick(covered.0, covered.1);
    println!("pick over covered A text at {covered:?} -> {res:?}");
    assert!(
        res.is_err(),
        "text hidden under the top window must not be read"
    );
    assert!(ra.hover(covered.0, covered.1).is_none_or(|v| v.is_empty()));
    assert_eq!(
        ra.source_hwnd(),
        wb.0 as isize,
        "failed pick keeps the source"
    );

    // 3. A visible word of A (outside B): reads A, source becomes A.
    let (a_word, a_rects) = nth_word(&tp_a, 0);
    let (ax, ay) = center(&a_rects[0]);
    assert!(!contains(
        std::slice::from_ref(&rb_rect),
        ax as f64,
        ay as f64
    ));
    let tokens = ra.pick(ax, ay).expect("pick A's visible word");
    println!(
        "pick A word {a_word:?} at ({ax},{ay}) -> {:?}",
        &tokens[..tokens.len().min(6)]
    );
    assert_eq!(tokens[0].trim(), a_word.trim());
    assert_eq!(ra.source_hwnd(), wa.0 as isize);
    assert!(
        hover_retry(&ra, bx, by).is_some(),
        "hover follows the window under the point"
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
    let exe = std::env::var("RA_BROWSER")
        .unwrap_or_else(|_| r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe".into());
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
    raise(hwnd);
    std::thread::sleep(Duration::from_millis(1500));

    // Ground truth: rect of "Paragraph03" via a separate UIA client.
    let uia = new_uia();
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
            let Ok(tp) =
                (unsafe { e.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) })
            else {
                continue;
            };
            let Ok(doc) = (unsafe { tp.DocumentRange() }) else {
                continue;
            };
            if let Ok(r) =
                unsafe { doc.FindText(&windows::core::BSTR::from("Paragraph03"), false, false) }
            {
                let rc = rects_of(&r).unwrap_or_default();
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
    let (cx, cy) = center(&w);
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
    assert_eq!(
        tokens.len(),
        FIRST_BATCH,
        "a long page fills the first batch"
    );
    assert_eq!(ra.source_hwnd(), hwnd.0 as isize);
    // Only an EMPTY more() means the end; loop (bounded) until it converges.
    let mut all = tokens.clone();
    let mut calls = 0;
    loop {
        let t = Instant::now();
        let more = ra.more().expect("more()");
        calls += 1;
        println!(
            "more() #{calls}: {} tokens in {} ms",
            more.len(),
            t.elapsed().as_millis()
        );
        if more.is_empty() {
            break;
        }
        assert!(more.len() <= MORE_BATCH);
        all.extend(more);
        assert!(calls < 50, "more() never reached the end");
    }
    println!("total {} tokens; last {:?}", all.len(), all.last());
    assert!(all.len() > 3000, "the whole page was read");
    assert!(ra.more().expect("more()").is_empty(), "stays empty at the end");

    let r0 = ra.rects(vec![0]).unwrap_or_default();
    assert!(
        contains(&r0, cx as f64, cy as f64),
        "rects(0) covers the click: {r0:?}"
    );
    let far = 1500;
    let before = ra.rects(vec![far]);
    assert_eq!(before.as_deref().map(<[Rect]>::is_empty), Some(true), "far word is off screen: Some(empty)");
    let t = Instant::now();
    ra.scroll_to(far);
    std::thread::sleep(Duration::from_millis(1000));
    let after = ra.rects(vec![far]).unwrap_or_default();
    println!(
        "token {far} {:?}: rects before scroll {before:?}, after {after:?} (scroll+1s wait {} ms)",
        all[far],
        t.elapsed().as_millis()
    );
    assert!(
        !after.is_empty(),
        "rule 4: scroll brings the word on screen"
    );

    // Rule 3: the browser toolbar (just below the window's top edge) is rejected.
    let wr = window_rect(hwnd);
    let res = ra.pick(wr.x as i32 + 300, wr.y as i32 + 60);
    println!("pick on toolbar -> {res:?}");
    assert!(res.is_err());
    drop(ra);
}

/// Task 8 bench: worker latency of hover (on words, on blank text area, on Notepad's
/// own chrome) and of rects, on our own Notepad window. Run in release:
/// cargo test --release uia_notepad_bench -- --ignored --nocapture --test-threads=1
#[test]
#[ignore]
fn uia_notepad_bench() {
    use crate::ra_perf::{ms, pct, thread_cpu};
    unsafe {
        let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    if !notepad_pids().is_empty() {
        println!("skipped: a Notepad is open (this bench never touches your windows)");
        return;
    }
    let stem = format!("{NP_PREFIX}bench_{}", std::process::id());
    let file = std::env::temp_dir().join(format!("{stem}.txt"));
    let lines: String = (0..30)
        .map(|i| format!("Line{i:02} alpha bravo charlie delta echo foxtrot golf hotel india juliet.\r\n"))
        .collect();
    std::fs::write(&file, lines).unwrap();
    let mut guard = Cleanup { hwnds: vec![], files: vec![file.clone()] };
    let hwnd = open_in_notepad(&file, &stem);
    guard.hwnds.push(hwnd);
    unsafe {
        let _ = MoveWindow(hwnd, 80, 60, 1100, 800, true);
    }
    raise(hwnd);
    std::thread::sleep(Duration::from_millis(1000));
    let uia = new_uia();
    let (doc_el, tp) = main_text(&uia, hwnd);
    let el = to_rect(unsafe { doc_el.CurrentBoundingRectangle() }.unwrap());
    // word centres spread over the visible lines (every 7th word)
    let pts: Vec<(i32, i32)> = (0..40)
        .filter_map(|k| nth_word(&tp, k * 7).1.first().map(center))
        .filter(|&(x, y)| contains(std::slice::from_ref(&el), x as f64, y as f64))
        .collect();
    assert!(pts.len() >= 20, "visible words: {}", pts.len());
    let ra = Uia::start(hwnd.0 as isize);
    let wait = Duration::from_secs(2);
    let _ = ra.pick(pts[0].0, pts[0].1).expect("pick");
    let time = |f: &mut dyn FnMut() -> bool| {
        let t = Instant::now();
        let ok = f();
        (ms(t.elapsed()), ok)
    };
    let c0 = thread_cpu();
    let mut word = Vec::new();
    let mut hits = 0;
    for _ in 0..3 {
        for &(x, y) in &pts {
            let (d, ok) = time(&mut || ra.hover_wait(x, y, wait).is_some_and(|v| contains(&v, x as f64, y as f64)));
            word.push(d);
            hits += usize::from(ok);
        }
    }
    let blank_pts: Vec<(i32, i32)> = (0..20).map(|k| (el.x as i32 + 40 + k * 30, (el.y + el.h) as i32 - 25)).collect();
    let mut blank = Vec::new();
    for &(x, y) in blank_pts.iter().cycle().take(60) {
        blank.push(time(&mut || ra.hover_wait(x, y, wait).is_some()).0);
    }
    // Notepad's own chrome (menu / tab strip) above the text element
    let chrome_pts: Vec<(i32, i32)> = (0..20).map(|k| (el.x as i32 + 40 + k * 30, el.y as i32 - 40)).collect();
    let mut chrome = Vec::new();
    for &(x, y) in chrome_pts.iter().cycle().take(60) {
        chrome.push(time(&mut || ra.hover_wait(x, y, wait).is_some()).0);
    }
    let mut rects = Vec::new();
    for i in 0..120usize {
        rects.push(time(&mut || ra.rects_wait(vec![i % 60], wait).is_some()).0);
    }
    let caller_cpu = ms(thread_cpu() - c0);
    println!("hover on words:   {} ({hits}/{} hit the word)", pct(&word), word.len());
    println!("hover on blank:   {}", pct(&blank));
    println!("hover on chrome:  {}", pct(&chrome));
    println!("rects (1 word):   {}", pct(&rects));
    println!("caller thread CPU {caller_cpu:.1} ms");
    drop(ra);
}

/// Where hover time goes on Notepad's own chrome (menu / tab strip): breakdown of the
/// steps `hover` runs for a point with no text. Same rules as `uia_notepad_bench`.
/// cargo test --release uia_chrome_breakdown -- --ignored --nocapture --test-threads=1
#[test]
#[ignore]
fn uia_chrome_breakdown() {
    use crate::ra_perf::{ms, pct};
    unsafe {
        let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    if !notepad_pids().is_empty() {
        println!("skipped: a Notepad is open (this bench never touches your windows)");
        return;
    }
    let stem = format!("{NP_PREFIX}chrome_{}", std::process::id());
    let file = std::env::temp_dir().join(format!("{stem}.txt"));
    std::fs::write(&file, "Alpha bravo charlie delta echo.\r\n").unwrap();
    let mut guard = Cleanup { hwnds: vec![], files: vec![file.clone()] };
    let hwnd = open_in_notepad(&file, &stem);
    guard.hwnds.push(hwnd);
    unsafe {
        let _ = MoveWindow(hwnd, 80, 60, 1100, 800, true);
    }
    raise(hwnd);
    std::thread::sleep(Duration::from_millis(1000));
    let uia = new_uia();
    let (doc_el, _) = main_text(&uia, hwnd);
    let el = to_rect(unsafe { doc_el.CurrentBoundingRectangle() }.unwrap());
    let mut st = State {
        uia: uia.clone(),
        own_pid: unsafe { GetCurrentProcessId() },
        target: make_target(&uia, hwnd.0 as isize),
        read: Read::default(),
        staged: None,
        source: Arc::new(AtomicIsize::new(hwnd.0 as isize)),
        ocr_hover: None,
        acro_hover: None,
    };
    let pts: Vec<(i32, i32)> = (0..20).map(|k| (el.x as i32 + 40 + k * 30, el.y as i32 - 40)).collect();
    let (mut tp, mut fb, mut ok, mut all) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for &(x, y) in pts.iter().cycle().take(100) {
        let t = Instant::now();
        let r = text_pattern_at(&mut st, x, y).map(|_| ());
        tp.push(ms(t.elapsed()));
        let t = Instant::now();
        let _ = fallback_at(&mut st, x, y);
        fb.push(ms(t.elapsed()));
        let t = Instant::now();
        let _ = ocr_ok_at(&st, x, y);
        ok.push(ms(t.elapsed()));
        let t = Instant::now();
        let _ = hover(&mut st, x, y);
        all.push(ms(t.elapsed()));
        assert!(r.is_err(), "chrome point has no text pattern");
    }
    println!("text_pattern_at (incl. fallback): {}", pct(&tp));
    println!("fallback_at alone:                {}", pct(&fb));
    println!("ocr_ok_at:                        {}", pct(&ok));
    println!("hover total:                      {}", pct(&all));
}

/// Round 3 manual: follow mode on a page that moves by itself
/// (tools/read-aloud/follow-test/reflow.html: smooth scrolling, a 120 px block inserted
/// mid-view every 5 s, a growing ad slot, lazy images, a sticky header with a live
/// clock). Runs the real UIA worker and the real `ra_follow::Guard` for 20 s hands-off,
/// reading at 12 words/s so Echo's own (smooth) scrolls happen too, and asserts follow
/// stays on. Lead scrolling (fix round 1) keeps the word on screen; RA_LEAD=0 measures
/// without it, RA_WPS sets the reading rate. Then a keyboard-like PageDown (the page's
/// own button runs window.scrollBy; invoked through UIA, no input sent) must turn
/// follow off.
/// Edge on a throwaway profile (RA_BROWSER = chrome.exe's path for Chrome); never
/// touches the user's windows; sends no input.
/// Run: cargo test follow_reflow_manual -- --ignored --nocapture --test-threads=1
#[test]
#[ignore]
#[allow(clippy::zombie_processes)] // cleaned up by BrowserCleanup
fn follow_reflow_manual() {
    use crate::ra_follow::{anchor_of, Decision, Guard};
    use crate::ra_hook::{MouseEv, WatchHook};
    unsafe {
        let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let exe = std::env::var("RA_BROWSER")
        .unwrap_or_else(|_| r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe".into());
    let tag = format!("RaFollowReflow{}", std::process::id());
    let dir = std::env::temp_dir();
    let page = dir.join(format!("{tag}.html"));
    let profile = dir.join(format!("{tag}-profile"));
    let lab = concat!(env!("CARGO_MANIFEST_DIR"), "/../tools/read-aloud/follow-test/reflow.html");
    let html = std::fs::read_to_string(lab).expect("lab page");
    let html = html.replace("<title>Echo follow reflow test</title>", &format!("<title>{tag}</title>"));
    std::fs::write(&page, html).unwrap();
    let mut guard = BrowserCleanup { hwnd: None, profile: profile.clone(), page: page.clone() };
    Command::new(&exe)
        .arg(format!("--user-data-dir={}", profile.display()))
        .args(["--no-first-run", "--no-default-browser-check", "--new-window", "--window-size=1100,900"])
        .arg(format!(
            "file:///{}{}",
            page.display().to_string().replace('\\', "/"),
            if std::env::var("RA_STATIC").is_ok() { "?static" } else { "" }
        ))
        .spawn()
        .expect("launch browser");
    let t0 = Instant::now();
    let mut hwnd = loop {
        if let Some(h) = find_window(&tag) {
            break h;
        }
        assert!(t0.elapsed() < Duration::from_secs(20), "browser window not found");
        std::thread::sleep(Duration::from_millis(250));
    };
    guard.hwnd = Some(hwnd);
    raise(hwnd);
    std::thread::sleep(Duration::from_millis(1500));

    // Ground truth: an early paragraph on screen, via a separate UIA client.
    let uia = new_uia();
    let cond = text_pattern_condition(&uia).unwrap();
    let mut target = None;
    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(250));
        if let Some(h) = find_window(&tag) {
            hwnd = h;
        }
        let Ok(root) = (unsafe { uia.ElementFromHandle(hwnd) }) else { continue };
        let Ok(all) = (unsafe { root.FindAll(TreeScope_Descendants, &cond) }) else { continue };
        for i in 0..unsafe { all.Length() }.unwrap_or(0) {
            let Ok(e) = (unsafe { all.GetElement(i) }) else { continue };
            let Ok(tp) = (unsafe { e.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) }) else {
                continue;
            };
            let Ok(doc) = (unsafe { tp.DocumentRange() }) else { continue };
            // the page shifts itself, so take the first of these that is on screen
            for needle in ["Paragraph02", "Paragraph01", "Paragraph03", "Paragraph00"] {
                if let Ok(r) = unsafe { doc.FindText(&windows::core::BSTR::from(needle), false, false) } {
                    let rc = rects_of(&r).unwrap_or_default();
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
        if target.is_some() {
            break;
        }
    }
    if target.is_none() {
        let mut buf = [0u16; 512];
        let n = unsafe { GetWindowTextW(hwnd, &mut buf) }.max(0) as usize;
        let root = unsafe { uia.ElementFromHandle(hwnd) }.ok();
        let found = root.and_then(|r| unsafe { r.FindAll(TreeScope_Descendants, &cond) }.ok());
        println!(
            "setup: window {:?} title {:?}, {} TextPattern elements",
            hwnd,
            String::from_utf16_lossy(&buf[..n]),
            found.as_ref().map_or(-1, |a| unsafe { a.Length() }.unwrap_or(-1))
        );
        if let Some(all) = found {
            for i in 0..unsafe { all.Length() }.unwrap_or(0) {
                let e = unsafe { all.GetElement(i) }.unwrap();
                let tp = unsafe { e.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) }.unwrap();
                let doc = unsafe { tp.DocumentRange() }.unwrap();
                let txt = unsafe { doc.GetText(300) }.map(|b| b.to_string()).unwrap_or_default();
                let f = unsafe { doc.FindText(&windows::core::BSTR::from("Paragraph02"), false, false) };
                println!(
                    "  element {i}: {:?}... find={} rects={:?} wr={:?}",
                    &txt.chars().take(80).collect::<String>(),
                    f.is_ok(),
                    f.ok().and_then(|r| rects_of(&r)),
                    window_rect(hwnd)
                );
            }
        }
    }
    let w = target.expect("an early paragraph on screen");
    guard.hwnd = Some(hwnd);
    let (cx, cy) = center(&w);
    let ra = Uia::start(hwnd.0 as isize);
    let mut raw = ra.pick(cx, cy).expect("pick in browser");
    for _ in 0..3 {
        match ra.more() {
            Ok(m) if !m.is_empty() => raw.extend(m),
            _ => break,
        }
    }
    let words = crate::ra_text::normalize(&raw);
    println!("picked {:?}; {} words", &raw[..3], words.len());

    // Wheel evidence exactly as the controller gets it (observe-only hook): the run is
    // hands-off, so a wheel over the test window would be a real (unexpected) user scroll.
    let (htx, hrx) = mpsc::channel::<MouseEv>();
    let hook = WatchHook::install(move |ev| {
        let _ = htx.send(ev);
    });
    let root_at = |x: i32, y: i32| unsafe {
        let w = WindowFromPoint(POINT { x, y });
        if w.is_invalid() {
            0
        } else {
            GetAncestor(w, GA_ROOT).0 as isize
        }
    };
    let win_of = |h: HWND| {
        let r = window_rect(h);
        (r.x as i32, r.y as i32, r.w as i32, r.h as i32)
    };
    let (atx, arx) = mpsc::channel::<ScrollRan>();
    let mut g = Guard::default();
    let mut follow = true;
    let mut offs: Vec<String> = Vec::new();
    let mut look_ms: Vec<f64> = Vec::new();
    // RA_LEAD=0: the round-3 behaviour (scroll only once the word is off screen, by
    // scroll-into-view) for the before / after on-screen measurement
    let lead_on = std::env::var("RA_LEAD").map_or(true, |v| v != "0");
    // RA_WPS: words per second (6.6 = Echo at 2.2x, 8.5 = 3x; the gapless bench article)
    let wps: f64 = std::env::var("RA_WPS").ok().and_then(|v| v.parse().ok()).unwrap_or(6.6);
    // stats: looks, timeouts, visible, scroll known, single empties, own scrolls,
    // reflows ignored, own-scroll moves ignored, lead scrolls
    // The controller's update_word_rects, step for step (no Tauri here).
    let t_start = Instant::now();
    let step = |g: &mut Guard, follow: &mut bool, wi: usize, stats: &mut [u32; 9], offs: &mut Vec<String>, look_ms: &mut Vec<f64>| -> bool {
        let prev_src = g.anchor_word().filter(|&p| p != wi).map(|p| words[p].src.clone());
        let t = Instant::now();
        let Some(look) = ra.look(words[wi].src.clone(), prev_src) else {
            stats[1] += 1;
            return false;
        };
        look_ms.push(t.elapsed().as_secs_f64() * 1000.0);
        stats[0] += 1;
        stats[2] += !look.rects.is_empty() as u32;
        stats[3] += look.scroll.is_some() as u32;
        let w = win_of(hwnd);
        let obs = anchor_of(wi, &look.rects, w);
        let prev_again = match (g.anchor_word(), &look.prev) {
            (Some(p), Some(r)) if p != wi => Some(anchor_of(p, r, w)),
            _ => None,
        };
        if std::env::var("RA_TRACE").is_ok() {
            println!(
                "  {:>6} ms w{wi} rect {:?} scroll {:?}",
                t.duration_since(t_start).as_millis(),
                look.rects.first().map(|r| (r.y as i32, r.h as i32)),
                look.scroll.map(|s| (s.y as i32, s.top as i32, s.extent as i32))
            );
        }
        let first = words[wi].src[0];
        let ack = |atx: &mpsc::Sender<ScrollRan>| {
            let tx = atx.clone();
            move |r| {
                let _ = tx.send(r);
            }
        };
        let mut retry = false;
        match g.look(Instant::now(), *follow, obs, prev_again, look.scroll) {
            Decision::Off(r) => {
                println!("{r}");
                offs.push(r.to_string());
                *follow = false;
            }
            Decision::Ignored(i) => {
                if std::env::var("RA_TRACE").is_ok() {
                    println!("  {i}");
                }
                stats[if i.why == "reflow" { 6 } else { 7 }] += 1;
            }
            Decision::Wait => {
                stats[4] += 1;
                return true;
            }
            Decision::Scroll => {
                stats[5] += 1;
                let prev_y = look.prev.as_ref().and_then(|r| r.first()).map(|r| r.y);
                let lead = look
                    .scroll
                    .filter(|_| lead_on)
                    .and_then(|sp| g.lead(Instant::now(), *follow, None, prev_y, (sp.top, sp.extent)));
                match lead {
                    Some(dy) => ra.scroll_by_acked(first, dy, ack(&atx)),
                    None => ra.scroll_to_acked(first, ack(&atx)),
                }
                g.own_queued(Instant::now());
                retry = true;
            }
            Decision::Nothing => {}
        }
        if let (true, Some(r), Some(sp)) = (lead_on, look.rects.first(), look.scroll) {
            if let Some(dy) = g.lead(Instant::now(), *follow, Some((r.y, r.h)), None, (sp.top, sp.extent)) {
                stats[8] += 1;
                if std::env::var("RA_TRACE").is_ok() {
                    println!("  lead {dy:.0}");
                }
                ra.scroll_by_acked(first, dy, ack(&atx));
                g.own_queued(Instant::now());
                retry = true;
            }
        }
        retry
    };
    let mut stats = [0u32; 9];
    let mut wheels = 0;
    let mut queued_ms: Vec<u64> = Vec::new();
    let run = Duration::from_secs(20);
    let t0 = Instant::now();
    let (mut last_word, mut last_look, mut retry) = (usize::MAX, t0, false);
    // The read goes on past the hands-off part; at 20 s the page's own "Page down" button
    // (window.scrollBy, like the PageDown key; invoked through UIA in our own throwaway
    // browser, no input sent) must turn follow off within 3 s.
    let page_down = {
        let cond = unsafe {
            uia.CreatePropertyCondition(UIA_NamePropertyId, &VARIANT::from(windows::core::BSTR::from("Page down")))
        }
        .unwrap();
        unsafe { uia.ElementFromHandle(hwnd).unwrap().FindFirst(TreeScope_Descendants, &cond) }
            .ok()
            .and_then(|e| unsafe { e.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId) }.ok())
    };
    let mut pressed: Option<(Instant, usize)> = None;
    let mut press_y: Option<f64> = None;
    let mut presses = 1u32;
    let mut hands_off_offs = None;
    let mut visible_hands_off = (0, 0);
    while t0.elapsed() < run + Duration::from_secs(5) {
        for ev in hrx.try_iter() {
            if let MouseEv::Wheel { x, y } = ev {
                if root_at(x, y) == hwnd.0 as isize {
                    g.wheel(Instant::now());
                    wheels += 1;
                }
            }
        }
        for r in arx.try_iter() {
            queued_ms.push(r.queued_ms);
            g.own_ran(Instant::now(), r.before, r.after, r.expected);
        }
        if t0.elapsed() >= run && pressed.is_none() {
            hands_off_offs = Some(offs.clone());
            visible_hands_off = (stats[2], stats[0]);
            if let Some(p) = &page_down {
                unsafe { p.Invoke() }.expect("press the page's Page down button");
                println!("pressed Page down (window.scrollBy) with follow {}", if follow { "on" } else { "off" });
            }
            pressed = Some((Instant::now(), offs.len()));
            press_y = ra.look(words[last_word].src.clone(), None).and_then(|l| l.rects.first().map(|r| r.y));
        }
        if pressed.is_some_and(|(_, n)| offs.len() > n) {
            break;
        }
        // the page's own ad / anchoring can swallow a scrollBy (measured: the offset moved,
        // the words did not): press again when the spoken word has not moved in 1 s
        if let (Some((at, _)), Some(p), true) = (pressed, &page_down, presses < 3) {
            if at.elapsed() >= Duration::from_secs(1) * presses {
                let y = ra.look(words[last_word].src.clone(), None).and_then(|l| l.rects.first().map(|r| r.y));
                let still = matches!((press_y, y), (Some(a), Some(b)) if (a - b).abs() < 100.0);
                if still {
                    unsafe { p.Invoke() }.expect("press Page down again");
                    println!("Page down #{presses} did not move the words; pressed again");
                }
                presses += 1;
            }
        }
        let wi = ((t0.elapsed().as_secs_f64() * wps) as usize).min(words.len() - 1);
        if wi != last_word || retry || last_look.elapsed() >= Duration::from_millis(250) {
            last_word = wi;
            last_look = Instant::now();
            retry = step(&mut g, &mut follow, wi, &mut stats, &mut offs, &mut look_ms);
        }
        std::thread::sleep(Duration::from_millis(33));
    }
    let [_, timeouts, _, with_scroll, waits, scrolls, reflows, owns, leads] = stats;
    let (visible, looks) = visible_hands_off;
    let mut lm = look_ms.clone();
    lm.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p = |q: f64| lm.get(((lm.len() as f64 - 1.0) * q) as usize).copied().unwrap_or(0.0);
    let hands_off_offs = hands_off_offs.unwrap_or_default();
    println!(
        "20 s hands-off at {wps} words/s, lead {}: word on screen {visible}/{looks} = {:.1}%; timeouts {timeouts}, \
         scroll offset known {with_scroll}, single empties {waits}, off-screen scrolls {scrolls}, lead scrolls {leads}, \
         reflows ignored {reflows}, own-scroll moves ignored {owns}, wheels over the window {wheels}, \
         follow-offs {}; look p50 {:.1} ms p95 {:.1} ms; own scroll queue ms {queued_ms:?}",
        if lead_on { "on" } else { "off" },
        visible as f64 * 100.0 / looks.max(1) as f64,
        hands_off_offs.len(),
        p(0.5),
        p(0.95)
    );
    drop(hook);
    drop(ra);
    assert!(hands_off_offs.is_empty(), "follow turned off with no user scroll: {hands_off_offs:?}");
    assert!(with_scroll > 0, "the page's scroll offset was readable");
    assert!(page_down.is_some(), "the page's Page down button");
    if !lead_on {
        return; // the round-3 behaviour, for the on-screen measurement only
    }
    let after = pressed.map_or(&offs[..0], |(_, n)| &offs[n..]);
    println!("after Page down: {after:?}");
    assert!(
        after.iter().any(|o| ["scroll_offset", "beyond_own_scroll", "against_own_scroll"]
            .iter()
            .any(|e| o.contains(&format!("evidence={e} ")))),
        "a PageDown (window.scrollBy) turns follow off: {after:?}"
    );
}
