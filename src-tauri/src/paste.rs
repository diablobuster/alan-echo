//! ALAN Echo — auto-paste into the application the user dictated into.
//!
//! The foreground window is captured when recording starts; after
//! transcription we inject Ctrl+V — but ONLY if the user is still in that
//! app. This module never calls SetForegroundWindow: focus-stealing was the
//! root of the "my window minimized / flashed / lost focus when transcription
//! finished" class of bugs. Windows routinely denies SetForegroundWindow to
//! background processes anyway (foreground-lock), which made the old path
//! fail with "Could not focus the target window" even when the user was
//! sitting in the target. If the user has moved to a different app, the
//! transcript stays on the clipboard and the UI says so — we never yank
//! windows around.
//!
//! Injection also waits for the user's physical modifier keys to clear
//! instead of blind-injecting Shift/Alt key-ups. A stray Alt-up while the
//! user is mid-keystroke (e.g. Alt+Tabbing during transcription) can throw
//! the target app into menu mode — another "my window did something weird"
//! source. Polling GetAsyncKeyState until Ctrl/Shift/Alt/Win are genuinely
//! released guarantees the target receives a clean Ctrl+V chord.

#[cfg(target_os = "windows")]
mod win {
    use std::thread;
    use std::time::{Duration, Instant};
    use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
    use windows::Win32::Security::{
        GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TokenIntegrityLevel,
        TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
        KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN,
        VK_SHIFT, VK_V,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId, IsWindow,
    };

    pub fn foreground_window() -> isize {
        unsafe { GetForegroundWindow().0 as isize }
    }

    fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    /// Last sub-authority of the token's integrity SID (0x2000 medium, 0x3000 high…).
    unsafe fn process_integrity(process: HANDLE) -> Option<u32> {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
        let mut buf = [0u8; 128];
        let mut needed = 0u32;
        let res = GetTokenInformation(
            token,
            TokenIntegrityLevel,
            Some(buf.as_mut_ptr() as *mut _),
            buf.len() as u32,
            &mut needed,
        );
        let _ = CloseHandle(token);
        res.ok()?;
        let label = &*(buf.as_ptr() as *const TOKEN_MANDATORY_LABEL);
        let sid = label.Label.Sid;
        let count = *GetSidSubAuthorityCount(sid) as u32;
        Some(*GetSidSubAuthority(sid, count - 1))
    }

    /// UIPI silently swallows SendInput aimed at higher-integrity (elevated)
    /// windows — SendInput still reports success. Detect that case up front so
    /// the caller can fall back to leaving the text on the clipboard.
    unsafe fn target_is_higher_integrity(h: HWND) -> bool {
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, Some(&mut pid));
        if pid == 0 {
            return false;
        }
        let Ok(proc_handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return true; // can't even query it — assume elevated (safe direction)
        };
        let target = process_integrity(proc_handle);
        let _ = CloseHandle(proc_handle);
        let own = process_integrity(GetCurrentProcess());
        match (target, own) {
            (Some(t), Some(o)) => t > o,
            (None, _) => true, // unreadable token — assume elevated
            _ => false,
        }
    }

    fn any_modifier_down() -> bool {
        [VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN]
            .iter()
            .any(|&vk| unsafe { (GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000) != 0 })
    }

    /// Wait for the user's physical Ctrl/Shift/Alt/Win to clear so the
    /// injected chord is exactly Ctrl+V. With a GPU engine, transcription can
    /// finish while the hotkey fingers are still on the keys.
    fn wait_for_modifier_release(timeout: Duration) -> bool {
        let start = Instant::now();
        while any_modifier_down() {
            if start.elapsed() > timeout {
                return false;
            }
            thread::sleep(Duration::from_millis(15));
        }
        true
    }

    pub fn paste_into(hwnd: isize) -> Result<(), String> {
        unsafe {
            let h = HWND(hwnd as *mut core::ffi::c_void);
            if !IsWindow(h).as_bool() {
                return Err("The target window no longer exists".into());
            }

            let fg = GetForegroundWindow();
            let same_window = fg == h;
            // Multi-window apps (browsers, editors) may present a different
            // top-level HWND for the same app the user never left; pasting
            // into the focused field of the same process is what they expect.
            let same_app = !same_window && !fg.0.is_null() && {
                let mut target_pid = 0u32;
                GetWindowThreadProcessId(h, Some(&mut target_pid));
                let mut fg_pid = 0u32;
                GetWindowThreadProcessId(fg, Some(&mut fg_pid));
                target_pid != 0 && target_pid == fg_pid
            };
            if !same_window && !same_app {
                return Err("Focus moved to a different app after dictation".into());
            }

            // Input lands in the *foreground* window, so that's the one that
            // must pass the UIPI (elevation) check.
            if target_is_higher_integrity(fg) {
                return Err(
                    "Target window is elevated; Windows blocks keystroke injection (UIPI)".into(),
                );
            }

            if !wait_for_modifier_release(Duration::from_millis(2000)) {
                return Err("Modifier keys were still held down".into());
            }

            let inputs = [
                key(VK_CONTROL, false),
                key(VK_V, false),
                key(VK_V, true),
                key(VK_CONTROL, true),
            ];
            let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
            if sent != inputs.len() as u32 {
                return Err("Keystroke injection was blocked".into());
            }
        }
        Ok(())
    }
}

#[cfg(target_os = "windows")]
pub use win::{foreground_window, paste_into};

#[cfg(target_os = "macos")]
mod mac {
    use std::process::Command;

    /// Capture the PID of the frontmost application (the one receiving keystrokes).
    pub fn foreground_window() -> isize {
        let output = Command::new("osascript")
            .args([
                "-e",
                "tell application \"System Events\" to unix id of first process whose frontmost is true",
            ])
            .output()
            .ok();
        match output {
            Some(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
                .trim()
                .parse::<isize>()
                .unwrap_or(0),
            _ => 0,
        }
    }

    /// Simulate Cmd+V into the app that was frontmost when recording started —
    /// but only if it is STILL frontmost. Same no-focus-stealing policy as
    /// Windows: if the user moved on, the transcript stays on the clipboard.
    /// Requires Accessibility access.
    pub fn paste_into(pid: isize) -> Result<(), String> {
        if pid <= 0 {
            return Err("No target application captured".into());
        }
        if foreground_window() != pid {
            return Err("Focus moved to a different app after dictation".into());
        }

        // Mitigation for a still-held Shift from the dictation hotkey (which
        // would turn Cmd+V into "Paste and Match Style" in some apps): a short
        // settle delay before the keystroke. osascript has no clean way to
        // poll physical modifier state — the real GetAsyncKeyState-equivalent
        // wait lands with the native Mac work
        // (docs/2026-06-17-slice8-macos-parity-spec.md §8d).
        let script = "delay 0.35\ntell application \"System Events\" to keystroke \"v\" using command down";

        let result = Command::new("osascript")
            .args(["-e", script])
            .output()
            .map_err(|e| format!("Auto-paste failed: {}", e))?;

        if !result.status.success() {
            let stderr = String::from_utf8_lossy(&result.stderr);
            if stderr.contains("not allowed assistive access")
                || stderr.contains("osascript is not allowed")
            {
                return Err(
                    "Auto-paste requires Accessibility access. \
                     Open System Settings \u{2192} Privacy & Security \u{2192} Accessibility, \
                     and enable ALAN Echo."
                        .into(),
                );
            }
            return Err(format!("Auto-paste failed: {}", stderr.trim()));
        }

        Ok(())
    }
}

#[cfg(target_os = "macos")]
pub use mac::{foreground_window, paste_into};

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn foreground_window() -> isize {
    0
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn paste_into(_hwnd: isize) -> Result<(), String> {
    Err("Auto-paste is not supported on this platform".into())
}
