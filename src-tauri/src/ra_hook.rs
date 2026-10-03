//! Read Aloud — low-level mouse hook, alive only during pick mode.
//!
//! Moves pass through to the target app; the left click (down + its up) is
//! swallowed so the app never sees it. Callbacks run inside the hook proc, which
//! Windows drops if it takes ~300 ms+, so they must only send to a channel.
//! Only one `PickHook` may be alive at a time (callbacks live in statics).
#![allow(dead_code)] // wired up by the Read Aloud controller task

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use parking_lot::Mutex;
use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, PeekMessageW, PostThreadMessageW, SetWindowsHookExW,
    UnhookWindowsHookEx, HHOOK, MSG, MSLLHOOKSTRUCT, PM_NOREMOVE, WH_MOUSE_LL, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_QUIT, WM_USER,
};

type Cb = Box<dyn Fn(i32, i32) + Send>;

static MOVE: OnceLock<Mutex<Option<Cb>>> = OnceLock::new();
static CLICK: OnceLock<Mutex<Option<Cb>>> = OnceLock::new();
/// Set when we swallowed a left-down, so we also swallow its matching up.
static SWALLOW_UP: AtomicBool = AtomicBool::new(false);

fn slot(s: &'static OnceLock<Mutex<Option<Cb>>>) -> &'static Mutex<Option<Cb>> {
    s.get_or_init(|| Mutex::new(None))
}

/// Live WH_MOUSE_LL hook. Drop to uninstall it and end its message loop.
pub struct PickHook {
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
            WM_LBUTTONDOWN => {
                if let Some(cb) = slot(&CLICK).lock().as_ref() {
                    cb(x, y);
                }
                SWALLOW_UP.store(true, Ordering::Relaxed);
                return LRESULT(1);
            }
            WM_LBUTTONUP if SWALLOW_UP.swap(false, Ordering::Relaxed) => return LRESULT(1),
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
        *slot(&MOVE).lock() = Some(Box::new(on_move));
        *slot(&CLICK).lock() = Some(Box::new(on_click));
        SWALLOW_UP.store(false, Ordering::Relaxed);
        let (tx, rx) = std::sync::mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("ra-hook".into())
            .spawn(move || unsafe {
                let mut msg = MSG::default();
                // Make sure this thread has a message queue before anyone posts WM_QUIT.
                let _ = PeekMessageW(&mut msg, None, WM_USER, WM_USER, PM_NOREMOVE);
                let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(hook_proc), HINSTANCE::default(), 0);
                if let Err(e) = &hook {
                    log::warn!("read-aloud: mouse hook install failed: {e}");
                }
                let _ = tx.send(GetCurrentThreadId());
                // GetMessageW returns 0 on WM_QUIT and -1 on error; stop on both.
                while GetMessageW(&mut msg, None, 0, 0).0 > 0 {}
                if let Ok(h) = hook {
                    let _ = UnhookWindowsHookEx(h);
                }
            });
        let (thread_id, thread) = match thread {
            Ok(t) => (rx.recv().unwrap_or(0), Some(t)),
            Err(e) => {
                log::warn!("read-aloud: could not spawn mouse hook thread: {e}");
                (0, None)
            }
        };
        PickHook { thread_id, thread }
    }
}

impl Drop for PickHook {
    fn drop(&mut self) {
        if self.thread_id != 0 {
            unsafe {
                let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        *slot(&MOVE).lock() = None;
        *slot(&CLICK).lock() = None;
        SWALLOW_UP.store(false, Ordering::Relaxed);
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
        PostMessageW, RegisterClassW, SetCursorPos, TranslateMessage, HMENU, WM_CLOSE, WNDCLASSW,
        WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
    };

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
        let (htx, hrx) = mpsc::channel::<isize>();
        let win = std::thread::spawn(move || unsafe {
            let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                lpszClassName: w!("RaHookTestWnd"),
                ..Default::default()
            };
            RegisterClassW(&wc);
            let h = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
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
            let _ = htx.send(h.0 as isize);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, HWND::default(), 0, 0).0 > 0 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        });
        let hwnd = HWND(hrx.recv().unwrap() as *mut _);
        std::thread::sleep(Duration::from_millis(300));
        let (cx, cy) = (x + wd / 2, y + ht / 2);
        let mut saved = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut saved);
        }

        // Control: injected clicks reach the window without the hook.
        click_at(cx, cy);
        let base = counts();
        println!("control click -> downs/ups {base:?}");

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
        drop(hook);

        click_at(cx, cy);
        let after = counts();
        println!("after drop: downs/ups {after:?}");

        unsafe {
            let _ = SetCursorPos(saved.x, saved.y);
            let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        let _ = win.join();

        assert_eq!(base, (1, 1), "injected click should reach the test window");
        assert!(mv.is_ok(), "hook reports moves");
        let click = click.expect("hook reports the click");
        assert!(
            (click.0 - cx).abs() <= 1 && (click.1 - cy).abs() <= 1,
            "click at cursor in physical px"
        );
        assert_eq!(during, base, "hooked click (down AND up) must be swallowed");
        assert_eq!(
            after,
            (base.0 + 1, base.1 + 1),
            "after drop, clicks reach the app again"
        );
    }
}
