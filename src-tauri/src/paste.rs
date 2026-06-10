//! ALAN Echo — auto-paste into the previously focused application.
//!
//! The foreground window is captured when recording starts (before the user
//! interacts with our UI), and after transcription we refocus it and inject
//! Ctrl+V via SendInput. This must live in Rust: the webview cannot focus
//! other applications or synthesize global keystrokes.

#[cfg(target_os = "windows")]
mod win {
    use std::thread;
    use std::time::Duration;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
    use windows::Win32::Security::{
        GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TokenIntegrityLevel,
        TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
        VIRTUAL_KEY, VK_CONTROL, VK_MENU, VK_SHIFT, VK_V,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId, IsWindow, SetForegroundWindow,
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

    pub fn paste_into(hwnd: isize) -> Result<(), String> {
        unsafe {
            let h = HWND(hwnd as *mut core::ffi::c_void);
            if !IsWindow(h).as_bool() {
                return Err("The target window no longer exists".into());
            }
            if target_is_higher_integrity(h) {
                return Err(
                    "Target window is elevated; Windows blocks keystroke injection (UIPI)".into(),
                );
            }

            let _ = SetForegroundWindow(h);
            // Give the window manager a beat to complete the focus switch.
            thread::sleep(Duration::from_millis(120));
            if GetForegroundWindow() != h {
                // The foreground grant may have just expired; retry once.
                let _ = SetForegroundWindow(h);
                thread::sleep(Duration::from_millis(120));
                if GetForegroundWindow() != h {
                    // Never inject Ctrl+V into a window the user didn't choose.
                    return Err("Could not focus the target window".into());
                }
            }

            // The user may still hold Shift from the Ctrl+Shift+Space hotkey;
            // a held Shift would turn our Ctrl+V into Ctrl+Shift+V.
            let inputs = [
                key(VK_SHIFT, true),
                key(VK_MENU, true),
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

#[cfg(not(target_os = "windows"))]
pub fn foreground_window() -> isize {
    0
}

#[cfg(not(target_os = "windows"))]
pub fn paste_into(_hwnd: isize) -> Result<(), String> {
    Err("Auto-paste is only supported on Windows".into())
}
