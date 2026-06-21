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

#[cfg(target_os = "macos")]
mod mac {
    //! Native auto-paste for macOS.
    //!
    //! Replaces the previous two-osascript-subprocesses-per-dictation path with
    //! in-process Cocoa/CoreGraphics calls: NSWorkspace captures the frontmost
    //! app at record start, NSRunningApplication refocuses it, and CGEventPost
    //! synthesizes Cmd+V. Like the osascript path, injecting keystrokes into
    //! another app requires Accessibility access.
    //!
    //! VALIDATION (must compile + run-test on real Mac hardware): the objc2
    //! crate surface and pinned versions below were authored from a non-Mac
    //! host — a Mac CI build is the first real compile check, and Accessibility
    //! behavior must be verified on a device.
    use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};
    use std::thread;
    use std::time::Duration;

    /// kVK_ANSI_V — the virtual key code for the 'v' key.
    const KEY_V: u16 = 9;

    // AXIsProcessTrusted() reports whether this process holds Accessibility
    // access. CGEventPost into another app silently no-ops without it, so we
    // check up front to surface the same actionable error the osascript path
    // returned (CGEventPost itself reports no permission error).
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> u8;
    }

    fn accessibility_trusted() -> bool {
        unsafe { AXIsProcessTrusted() != 0 }
    }

    /// Capture the PID of the frontmost application (the one receiving
    /// keystrokes) at record start, so we can refocus it before pasting.
    pub fn foreground_window() -> isize {
        unsafe {
            let ws = NSWorkspace::sharedWorkspace();
            match ws.frontmostApplication() {
                Some(app) => app.processIdentifier() as isize,
                None => 0,
            }
        }
    }

    /// Re-focus the app captured at record start, then synthesize Cmd+V.
    /// Requires Accessibility access.
    pub fn paste_into(pid: isize) -> Result<(), String> {
        if pid <= 0 {
            return Err("No target application captured".into());
        }

        if !accessibility_trusted() {
            return Err(
                "Auto-paste requires Accessibility access. \
                 Open System Settings \u{2192} Privacy & Security \u{2192} Accessibility, \
                 and enable ALAN Echo."
                    .into(),
            );
        }

        // Refocus the captured app. Best-effort: if it has since quit, fall
        // through and paste into whatever is frontmost rather than failing.
        unsafe {
            if let Some(app) =
                NSRunningApplication::runningApplicationWithProcessIdentifier(pid as i32)
            {
                app.activateWithOptions(NSApplicationActivationOptions::NSApplicationActivateAllWindows);
                // Let the window server complete the focus switch.
                thread::sleep(Duration::from_millis(120));
            }
        }

        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .map_err(|_| "Could not create a keyboard event source".to_string())?;

        // Neutralize a physically-held Shift from the Cmd+Shift+Space hotkey
        // (mirrors the Windows path, which releases Shift before pasting). With
        // the HID source merging real modifier state, a still-held Shift could
        // otherwise turn the synthetic Cmd+V into Cmd+Shift+V ("Paste and Match
        // Style"). Verify on hardware (see the macOS parity spec §8d).
        const KEY_SHIFT: u16 = 56; // kVK_Shift
        if let Ok(shift_up) = CGEvent::new_keyboard_event(source.clone(), KEY_SHIFT, false) {
            shift_up.post(CGEventTapLocation::HID);
        }

        // Down + up for 'v', each carrying ONLY the Command flag. Setting the
        // flag explicitly keeps a physically-held Shift (from the
        // Cmd+Shift+Space hotkey) from turning this into Cmd+Shift+V
        // ("Paste and Match Style").
        let down = CGEvent::new_keyboard_event(source.clone(), KEY_V, true)
            .map_err(|_| "Could not synthesize the paste keystroke".to_string())?;
        down.set_flags(CGEventFlags::CGEventFlagCommand);
        down.post(CGEventTapLocation::HID);

        let up = CGEvent::new_keyboard_event(source, KEY_V, false)
            .map_err(|_| "Could not synthesize the paste keystroke".to_string())?;
        up.set_flags(CGEventFlags::CGEventFlagCommand);
        up.post(CGEventTapLocation::HID);

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
