//! Read Aloud — low-level mouse hook, alive only during pick mode.
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
    MSG, MSLLHOOKSTRUCT, PM_NOREMOVE, WH_MOUSE_LL, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_QUIT, WM_USER,
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
        let (tx, rx) = std::sync::mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("ra-hook".into())
            .spawn(move || unsafe {
                let mut msg = MSG::default();
                // Make sure this thread has a message queue before anyone posts WM_QUIT.
                let _ = PeekMessageW(&mut msg, None, WM_USER, WM_USER, PM_NOREMOVE);
                let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(hook_proc), HINSTANCE::default(), 0);
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
        let (thread_id, active, thread) = match thread {
            Ok(t) => {
                let (id, ok) = rx.recv().unwrap_or((0, false));
                (id, ok, Some(t))
            }
            Err(e) => {
                log::warn!("read-aloud: could not spawn mouse hook thread: {e}");
                (0, false, None)
            }
        };
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
        if self.thread_id != 0 {
            unsafe {
                let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }
        if let Some(t) = self.thread.take() {
            if !on_hook_thread {
                let _ = t.join();
            }
        }
        *slot(&MOVE).lock() = None;
        *slot(&CLICK).lock() = None;
        SWALLOW_UP.store(false, Ordering::Release);
        INSTALLED.store(false, Ordering::Release);
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
}
