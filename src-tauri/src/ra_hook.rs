//! Read Aloud — low-level mouse hooks.
//!
//! `PickHook` lives only during pick mode; `WatchHook` (observe-only) lives only while a
//! read session is active (see the bottom of this file).
//!
//! Moves pass through to the target app; the left click (down + its up) is
//! swallowed so the app never sees it — except over our own windows (the Now Reading
//! pill), where the click passes through so its buttons keep working. Callbacks run inside the hook proc, which
//! Windows drops if it takes ~300 ms+, so they must only send to a channel.
//! Only one `PickHook` may be alive at a time (callbacks live in statics); a second
//! `install` while one is alive returns an inactive no-op hook.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetAncestor, GetMessageW, GetWindowThreadProcessId, PeekMessageW,
    PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, WindowFromPoint, GA_ROOT, HHOOK,
    HOOKPROC, MSG, MSLLHOOKSTRUCT, PM_NOREMOVE, WH_MOUSE_LL, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_QUIT, WM_USER,
};

type Cb = Box<dyn Fn(i32, i32) + Send>;

static MOVE: OnceLock<Mutex<Option<Cb>>> = OnceLock::new();
static CLICK: OnceLock<Mutex<Option<Cb>>> = OnceLock::new();
/// Set when we swallowed a left-down, so we also swallow its matching up.
static SWALLOW_UP: AtomicBool = AtomicBool::new(false);
/// True while a `PickHook` owns the statics above.
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Test hook: when false, clicks over our own windows are swallowed like any other
/// (the manual test's target window lives in the test process).
static PASS_OWN: AtomicBool = AtomicBool::new(true);

/// The top-level window under the point belongs to this process (the click-through
/// overlay is skipped by hit-testing, so this is the pill or another Echo window).
fn over_own_window(x: i32, y: i32) -> bool {
    unsafe {
        let w = WindowFromPoint(POINT { x, y });
        if w.is_invalid() {
            return false;
        }
        let root = GetAncestor(w, GA_ROOT);
        let mut pid = 0u32;
        GetWindowThreadProcessId(if root.is_invalid() { w } else { root }, Some(&mut pid));
        pid == std::process::id()
    }
}

/// How long `drop` waits for the up of a swallowed click (so it is swallowed too).
const UP_WAIT: Duration = Duration::from_millis(500);

fn slot(s: &'static OnceLock<Mutex<Option<Cb>>>) -> &'static Mutex<Option<Cb>> {
    s.get_or_init(|| Mutex::new(None))
}

/// Live WH_MOUSE_LL hook. Drop to uninstall it and end its message loop.
pub struct PickHook {
    /// This instance owns the statics (false for the no-op hook of a rejected install).
    owner: bool,
    /// The hook was actually installed.
    active: bool,
    thread_id: u32,
    thread: Option<std::thread::JoinHandle<()>>,
}

unsafe extern "system" fn hook_proc(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code >= 0 && l.0 != 0 {
        let info = &*(l.0 as *const MSLLHOOKSTRUCT);
        let (x, y) = (info.pt.x, info.pt.y);
        match w.0 as u32 {
            WM_MOUSEMOVE => {
                if let Some(cb) = slot(&MOVE).lock().as_ref() {
                    cb(x, y);
                }
            }
            WM_LBUTTONDOWN if PASS_OWN.load(Ordering::Relaxed) && over_own_window(x, y) => {}
            WM_LBUTTONDOWN => {
                if let Some(cb) = slot(&CLICK).lock().as_ref() {
                    cb(x, y);
                }
                SWALLOW_UP.store(true, Ordering::Release);
                return LRESULT(1);
            }
            WM_LBUTTONUP if SWALLOW_UP.swap(false, Ordering::AcqRel) => return LRESULT(1),
            _ => {}
        }
    }
    CallNextHookEx(HHOOK::default(), code, w, l)
}

impl PickHook {
    pub fn install(
        on_move: impl Fn(i32, i32) + Send + 'static,
        on_click: impl Fn(i32, i32) + Send + 'static,
    ) -> PickHook {
        if INSTALLED.swap(true, Ordering::AcqRel) {
            log::warn!("read-aloud: pick hook already installed; ignoring second install");
            return PickHook {
                owner: false,
                active: false,
                thread_id: 0,
                thread: None,
            };
        }
        *slot(&MOVE).lock() = Some(Box::new(on_move));
        *slot(&CLICK).lock() = Some(Box::new(on_click));
        SWALLOW_UP.store(false, Ordering::Release);
        let (thread_id, active, thread) = spawn_hook_thread("ra-hook", Some(hook_proc));
        PickHook {
            owner: true,
            active,
            thread_id,
            thread,
        }
    }

    /// True when the WH_MOUSE_LL hook is really installed (false if Windows refused it,
    /// or this was a second `install` while another hook was alive).
    pub fn is_active(&self) -> bool {
        self.active
    }
}

/// Run a WH_MOUSE_LL hook with `proc` on its own thread (with a message loop, which LL
/// hooks need). Returns (thread id, hook installed, thread). The loop ends on WM_QUIT.
fn spawn_hook_thread(
    name: &str,
    proc: HOOKPROC,
) -> (u32, bool, Option<std::thread::JoinHandle<()>>) {
    let (tx, rx) = std::sync::mpsc::channel();
    let thread = std::thread::Builder::new()
        .name(name.into())
        .spawn(move || unsafe {
            let mut msg = MSG::default();
            // Make sure this thread has a message queue before anyone posts WM_QUIT.
            let _ = PeekMessageW(&mut msg, None, WM_USER, WM_USER, PM_NOREMOVE);
            let hook = SetWindowsHookExW(WH_MOUSE_LL, proc, HINSTANCE::default(), 0);
            let ok = hook.is_ok();
            if let Err(e) = &hook {
                log::warn!("read-aloud: mouse hook install failed: {e}");
            }
            let _ = tx.send((GetCurrentThreadId(), ok));
            let Ok(h) = hook else { return };
            // GetMessageW returns 0 on WM_QUIT and -1 on error; stop on both.
            while GetMessageW(&mut msg, None, 0, 0).0 > 0 {}
            let _ = UnhookWindowsHookEx(h);
        });
    match thread {
        Ok(t) => {
            let (id, ok) = rx.recv().unwrap_or((0, false));
            (id, ok, Some(t))
        }
        Err(e) => {
            log::warn!("read-aloud: could not spawn mouse hook thread: {e}");
            (0, false, None)
        }
    }
}

/// End a hook thread started by `spawn_hook_thread` (never joins from the hook thread itself).
fn stop_hook_thread(thread_id: u32, thread: Option<std::thread::JoinHandle<()>>) {
    let on_hook_thread = thread_id != 0 && unsafe { GetCurrentThreadId() } == thread_id;
    if thread_id != 0 {
        unsafe {
            let _ = PostThreadMessageW(thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
    if let Some(t) = thread {
        if !on_hook_thread {
            let _ = t.join();
        }
    }
}

impl Drop for PickHook {
    fn drop(&mut self) {
        if !self.owner {
            return;
        }
        let on_hook_thread =
            self.thread_id != 0 && unsafe { GetCurrentThreadId() } == self.thread_id;
        // A swallowed left-down whose up has not arrived yet: keep the hook alive briefly
        // so the up is swallowed too (otherwise the app sees an up without a down).
        // Never block the hook thread itself.
        if self.active && !on_hook_thread {
            let t = Instant::now();
            while SWALLOW_UP.load(Ordering::Acquire) && t.elapsed() < UP_WAIT {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        stop_hook_thread(self.thread_id, self.thread.take());
        *slot(&MOVE).lock() = None;
        *slot(&CLICK).lock() = None;
        SWALLOW_UP.store(false, Ordering::Release);
        INSTALLED.store(false, Ordering::Release);
    }
}

// ── Watch hook (observe-only, alive only while a read session is active) ──────────

/// A mouse event seen by the watch hook. `t` = the event's time stamp in ms
/// (`MSLLHOOKSTRUCT::time`, wraps); coordinates are physical screen pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MouseEv {
    Down { x: i32, y: i32, t: u32 },
    Up { x: i32, y: i32, t: u32 },
    /// Vertical or horizontal wheel.
    Wheel { x: i32, y: i32 },
}

type WatchCb = Box<dyn Fn(MouseEv) + Send>;

static WATCH: OnceLock<Mutex<Option<WatchCb>>> = OnceLock::new();
static WATCH_INSTALLED: AtomicBool = AtomicBool::new(false);

fn watch_slot() -> &'static Mutex<Option<WatchCb>> {
    WATCH.get_or_init(|| Mutex::new(None))
}

unsafe extern "system" fn watch_proc(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code >= 0 && l.0 != 0 {
        let info = &*(l.0 as *const MSLLHOOKSTRUCT);
        let (x, y, t) = (info.pt.x, info.pt.y, info.time);
        let ev = match w.0 as u32 {
            WM_LBUTTONDOWN => Some(MouseEv::Down { x, y, t }),
            WM_LBUTTONUP => Some(MouseEv::Up { x, y, t }),
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => Some(MouseEv::Wheel { x, y }),
            _ => None, // moves are not reported (no traffic while the mouse just moves)
        };
        if let Some(ev) = ev {
            if let Some(cb) = watch_slot().lock().as_ref() {
                cb(ev);
            }
        }
    }
    // never swallow: every event reaches the app as usual
    CallNextHookEx(HHOOK::default(), code, w, l)
}

/// Observe-only WH_MOUSE_LL hook: reports left down/up and wheel events, swallows
/// nothing. The callback runs inside the hook proc, so it must only send to a channel.
/// Only one may be alive at a time (a second `install` returns an inactive no-op).
/// Drop to uninstall it.
pub struct WatchHook {
    owner: bool,
    active: bool,
    thread_id: u32,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl WatchHook {
    pub fn install(on_event: impl Fn(MouseEv) + Send + 'static) -> WatchHook {
        if WATCH_INSTALLED.swap(true, Ordering::AcqRel) {
            log::warn!("read-aloud: watch hook already installed; ignoring second install");
            return WatchHook { owner: false, active: false, thread_id: 0, thread: None };
        }
        *watch_slot().lock() = Some(Box::new(on_event));
        let (thread_id, active, thread) = spawn_hook_thread("ra-watch", Some(watch_proc));
        WatchHook { owner: true, active, thread_id, thread }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }
}

impl Drop for WatchHook {
    fn drop(&mut self) {
        if !self.owner {
            return;
        }
        stop_hook_thread(self.thread_id, self.thread.take());
        *watch_slot().lock() = None;
        WATCH_INSTALLED.store(false, Ordering::Release);
    }
}

/// A press longer than this is not a click (a long press / selection hold).
pub const CLICK_MAX_MS: u32 = 500;

/// Down → up is a click, not a drag: it moved at most `slop` px on each axis and took
/// at most `CLICK_MAX_MS`. Times are hook time stamps (ms, wrapping).
pub fn is_click(down: (i32, i32, u32), up: (i32, i32, u32), slop: i32) -> bool {
    let (dx, dy) = ((up.0 - down.0).abs(), (up.1 - down.1).abs());
    dx <= slop && dy <= slop && up.2.wrapping_sub(down.2) <= CLICK_MAX_MS
}

/// The second click of a double-click: it went down within `dbl_ms` of the previous
/// click's up and within `slop` px of it (that word is already being jumped to).
pub fn is_double_click(prev_up: Option<(i32, i32, u32)>, down: (i32, i32, u32), dbl_ms: u32, slop: i32) -> bool {
    prev_up.is_some_and(|p| {
        (down.0 - p.0).abs() <= slop && (down.1 - p.1).abs() <= slop && down.2.wrapping_sub(p.2) <= dbl_ms
    })
}

#[cfg(test)]
mod click_tests {
    use super::*;

    #[test]
    fn a_short_still_press_is_a_click() {
        assert!(is_click((100, 100, 1000), (100, 100, 1080), 6));
        assert!(is_click((100, 100, 1000), (106, 94, 1080), 6), "small jitter is fine");
    }

    #[test]
    fn a_drag_is_not_a_click() {
        assert!(!is_click((100, 100, 1000), (107, 100, 1080), 6), "moved sideways");
        assert!(!is_click((100, 100, 1000), (100, 140, 1080), 6), "dragged down (selection / scrollbar)");
    }

    #[test]
    fn a_long_press_is_not_a_click() {
        assert!(is_click((5, 5, 0), (5, 5, CLICK_MAX_MS), 6));
        assert!(!is_click((5, 5, 0), (5, 5, CLICK_MAX_MS + 1), 6));
    }

    #[test]
    fn click_timing_survives_the_tick_count_wrapping() {
        assert!(is_click((5, 5, u32::MAX - 20), (5, 5, 40), 6));
        assert!(!is_click((5, 5, u32::MAX - 20), (5, 5, 1000), 6));
    }

    #[test]
    fn second_click_of_a_double_click_is_recognised() {
        let up = Some((200, 50, 5000));
        assert!(is_double_click(up, (201, 51, 5200), 500, 6));
        assert!(!is_double_click(up, (201, 51, 5600), 500, 6), "too late: a new click");
        assert!(!is_double_click(up, (260, 51, 5200), 500, 6), "elsewhere: a new click");
        assert!(!is_double_click(None, (201, 51, 5200), 500, 6));
    }
}

#[cfg(test)]
mod tests {
    // Manual check: cargo test ra_hook -- --ignored --nocapture
    // Shows a small topmost window of our own, moves the cursor onto it and injects clicks
    // (SendInput). Any click that leaks lands only on that window.
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc;
    use std::time::Duration;
    use windows::core::w;
    use windows::Win32::Foundation::{HWND, POINT};
    use windows::Win32::UI::HiDpi::{
        SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
        MOUSEEVENTF_MOVE, MOUSEINPUT, MOUSE_EVENT_FLAGS,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos,
        PostMessageW, RegisterClassW, SetCursorPos, SetLayeredWindowAttributes, ShowWindow,
        TranslateMessage, HMENU, LWA_ALPHA, SW_SHOWNOACTIVATE, WM_CLOSE, WNDCLASSW, WS_EX_LAYERED,
        WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP, WS_VISIBLE,
    };
    use windows::Win32::Foundation::COLORREF;

    static DOWNS: AtomicUsize = AtomicUsize::new(0);
    static UPS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "system" fn wndproc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        match m {
            WM_LBUTTONDOWN => {
                DOWNS.fetch_add(1, Ordering::SeqCst);
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                UPS.fetch_add(1, Ordering::SeqCst);
                LRESULT(0)
            }
            WM_CLOSE => {
                let _ = DestroyWindow(h);
                windows::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(h, m, w, l),
        }
    }

    fn mouse(flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32) -> INPUT {
        INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    fn click_at(x: i32, y: i32) {
        let sz = std::mem::size_of::<INPUT>() as i32;
        unsafe {
            let _ = SetCursorPos(x, y);
            SendInput(&[mouse(MOUSEEVENTF_LEFTDOWN, 0, 0)], sz);
            std::thread::sleep(Duration::from_millis(50));
            SendInput(&[mouse(MOUSEEVENTF_LEFTUP, 0, 0)], sz);
        }
        std::thread::sleep(Duration::from_millis(300));
    }

    fn counts() -> (usize, usize) {
        (DOWNS.load(Ordering::SeqCst), UPS.load(Ordering::SeqCst))
    }

    #[test]
    #[ignore]
    fn pick_hook_manual() {
        unsafe {
            let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
        let (x, y, wd, ht) = (200, 200, 240, 120);
        let (htx, hrx) = mpsc::channel::<(isize, isize)>();
        let win = std::thread::spawn(move || unsafe {
            let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                lpszClassName: w!("RaHookTestWnd"),
                ..Default::default()
            };
            RegisterClassW(&wc);
            // WS_EX_NOACTIVATE: the test never takes focus from the user's window
            let h = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("RaHookTestWnd"),
                w!("ra_hook test"),
                WS_POPUP | WS_VISIBLE,
                x,
                y,
                wd,
                ht,
                HWND::default(),
                HMENU::default(),
                HINSTANCE::default(),
                None,
            )
            .expect("test window");
            // A stand-in for the Read Aloud overlay on top of it: Tauri's
            // set_ignore_cursor_events(true) = WS_EX_LAYERED | WS_EX_TRANSPARENT.
            let ov = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TRANSPARENT,
                w!("RaHookTestWnd"),
                w!("ra_hook overlay"),
                WS_POPUP,
                x - 20,
                y - 20,
                wd + 40,
                ht + 40,
                HWND::default(),
                HMENU::default(),
                HINSTANCE::default(),
                None,
            )
            .expect("overlay window");
            let _ = SetLayeredWindowAttributes(ov, COLORREF(0), 128, LWA_ALPHA);
            let _ = ShowWindow(ov, SW_SHOWNOACTIVATE);
            let _ = htx.send((h.0 as isize, ov.0 as isize));
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, HWND::default(), 0, 0).0 > 0 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        });
        let (h, ov) = hrx.recv().unwrap();
        let hwnd = HWND(h as *mut _);
        std::thread::sleep(Duration::from_millis(300));
        let (cx, cy) = (x + wd / 2, y + ht / 2);
        // I-1 / m5 premise: hit-testing skips the click-through overlay, so
        // WindowFromPoint finds the window under it.
        let under = unsafe { GetAncestor(WindowFromPoint(POINT { x: cx, y: cy }), GA_ROOT) }.0 as isize;
        println!("WindowFromPoint under the click-through overlay: {under:#x} (window {h:#x}, overlay {ov:#x})");
        let mut saved = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut saved);
        }

        // Control: injected clicks reach the window without the hook.
        click_at(cx, cy);
        println!("control click -> downs/ups {:?}", counts());

        // m5: clicks over our own windows (the pill) pass through and are not reported.
        let (otx, orx) = mpsc::channel();
        let own = PickHook::install(|_, _| {}, move |x, y| {
            let _ = otx.send((x, y));
        });
        click_at(cx, cy);
        let own_click = orx.recv_timeout(Duration::from_millis(300)).ok();
        let own_counts = counts();
        drop(own);
        println!("own window, hooked: reported {own_click:?}, downs/ups {own_counts:?}");
        let base = own_counts;

        // The rest checks swallowing, using this process's window as the stand-in app.
        PASS_OWN.store(false, Ordering::Relaxed);
        let (mtx, mrx) = mpsc::channel();
        let (ctx, crx) = mpsc::channel();
        let hook = PickHook::install(
            move |x, y| {
                let _ = mtx.send((x, y));
            },
            move |x, y| {
                let _ = ctx.send((x, y));
            },
        );
        unsafe {
            let sz = std::mem::size_of::<INPUT>() as i32;
            SendInput(
                &[
                    mouse(MOUSEEVENTF_MOVE, 1, 0),
                    mouse(MOUSEEVENTF_MOVE, -1, 0),
                ],
                sz,
            );
        }
        let mv = mrx.recv_timeout(Duration::from_secs(2));
        click_at(cx, cy);
        let click = crx.recv_timeout(Duration::from_secs(2));
        let during = counts();
        println!("hooked: move {mv:?} click {click:?} downs/ups {during:?}");
        let active = hook.is_active();
        // A second install while one is alive is a logged no-op.
        let second = PickHook::install(|_, _| {}, |_, _| {});
        let second_active = second.is_active();
        drop(second); // must not uninstall the first one

        // Task 8 pattern: drop the hook right after on_click, before the up arrives.
        // Drop must wait for the up and swallow it.
        let sz = std::mem::size_of::<INPUT>() as i32;
        unsafe {
            SendInput(&[mouse(MOUSEEVENTF_LEFTDOWN, 0, 0)], sz);
        }
        let click2 = crx.recv_timeout(Duration::from_secs(2));
        let dropper = std::thread::spawn(move || {
            let t = std::time::Instant::now();
            drop(hook);
            t.elapsed()
        });
        std::thread::sleep(Duration::from_millis(120));
        unsafe {
            SendInput(&[mouse(MOUSEEVENTF_LEFTUP, 0, 0)], sz);
        }
        let drop_took = dropper.join().unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let early_drop = counts();
        println!(
            "second install active={second_active}; early drop: click {click2:?}, drop waited {drop_took:?}, downs/ups {early_drop:?}"
        );

        click_at(cx, cy);
        let after = counts();
        println!("after drop: downs/ups {after:?}");
        // Re-install works after a drop (guard released).
        let again = PickHook::install(|_, _| {}, |_, _| {});
        let again_active = again.is_active();
        drop(again);

        unsafe {
            let _ = SetCursorPos(saved.x, saved.y);
            let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        let _ = win.join();

        PASS_OWN.store(true, Ordering::Relaxed);
        assert_eq!(under, h, "WindowFromPoint must skip the WS_EX_LAYERED|WS_EX_TRANSPARENT overlay");
        assert_eq!(base, (2, 2), "control click and the own-window click reach the window");
        assert_eq!(own_click, None, "a click over our own window is not a pick");
        assert!(mv.is_ok(), "hook reports moves");
        let click = click.expect("hook reports the click");
        assert!(
            (click.0 - cx).abs() <= 1 && (click.1 - cy).abs() <= 1,
            "click at cursor in physical px"
        );
        assert_eq!(during, base, "hooked click (down AND up) must be swallowed");
        assert!(active, "hook installed");
        assert!(!second_active, "second install is a no-op");
        assert!(
            click2.is_ok(),
            "first hook still alive after the no-op drop"
        );
        assert_eq!(early_drop, base, "up after an early drop is swallowed too");
        assert!(
            drop_took < Duration::from_millis(500),
            "drop waited only for the up"
        );
        assert!(again_active, "can install again after drop");
        assert_eq!(
            after,
            (base.0 + 1, base.1 + 1),
            "after drop, clicks reach the app again"
        );
    }

    /// Manual: cargo test ra_hook::tests::watch_hook_manual -- --ignored --nocapture
    /// The watch hook reports a click on our own test window and does NOT swallow it.
    #[test]
    #[ignore]
    fn watch_hook_manual() {
        unsafe {
            let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
        let (x, y, wd, ht) = (260, 260, 200, 100);
        let (htx, hrx) = mpsc::channel::<isize>();
        let win = std::thread::spawn(move || unsafe {
            let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                lpszClassName: w!("RaWatchTestWnd"),
                ..Default::default()
            };
            RegisterClassW(&wc);
            let h = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("RaWatchTestWnd"),
                w!("ra_hook watch test"),
                WS_POPUP | WS_VISIBLE,
                x,
                y,
                wd,
                ht,
                HWND::default(),
                HMENU::default(),
                HINSTANCE::default(),
                None,
            )
            .expect("test window");
            let _ = htx.send(h.0 as isize);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, HWND::default(), 0, 0).0 > 0 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        });
        let h = hrx.recv().unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let mut saved = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut saved);
        }
        let (cx, cy) = (x + wd / 2, y + ht / 2);
        let before = counts();
        let (tx, rx) = mpsc::channel();
        let hook = WatchHook::install(move |ev| {
            let _ = tx.send(ev);
        });
        let active = hook.is_active();
        let second = WatchHook::install(|_| {});
        let second_active = second.is_active();
        drop(second);
        click_at(cx, cy);
        let evs: Vec<MouseEv> = rx.try_iter().collect();
        let during = counts();
        drop(hook);
        let again = WatchHook::install(|_| {});
        let again_active = again.is_active();
        drop(again);
        unsafe {
            let _ = SetCursorPos(saved.x, saved.y);
            let _ = PostMessageW(HWND(h as *mut _), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        let _ = win.join();
        println!("watch: events {evs:?}, downs/ups {before:?} -> {during:?}");
        assert!(active, "watch hook installed");
        assert!(!second_active, "second install is a no-op");
        assert!(again_active, "can install again after drop");
        assert_eq!(during, (before.0 + 1, before.1 + 1), "the click reaches the window (not swallowed)");
        let down = evs.iter().any(|e| matches!(e, MouseEv::Down { x, y, .. } if (x - cx).abs() <= 1 && (y - cy).abs() <= 1));
        let up = evs.iter().any(|e| matches!(e, MouseEv::Up { .. }));
        assert!(down && up, "down and up reported");
    }
}
