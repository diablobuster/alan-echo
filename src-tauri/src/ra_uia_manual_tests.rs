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
