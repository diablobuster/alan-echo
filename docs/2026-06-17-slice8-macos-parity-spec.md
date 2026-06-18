# Slice 8 — macOS parity (osascript hot-path + Metal): implementation spec

**Status:** SPEC ONLY — deferred from the 2026-06-17 stabilization session.
**Why deferred:** every change here requires a macOS toolchain / Apple Silicon /
Metal to compile and validate. The session that produced Slices 1–7 ran on
Windows with no way to compile `cfg(target_os = "macos")` code, run Metal, or
observe the beep latency on a Mac. Per the dual-platform mandate and "verify
before claiming done," committing blind Objective-C FFI or a Metal-enabled build
risks shipping a **broken mac build that Windows CI cannot catch**. This spec is
written so a macOS session can execute and verify it immediately — and the new
`ci.yml` `macos-14` job now compiles `cfg(macos)` code on every PR, so a mistake
here surfaces at review time.

All line numbers verified against the tree at this commit; re-check before edit.

---

## 8a — Replace the osascript spawn on the hotkey→record→beep path

**Problem:** `paste.rs` `mac::foreground_window()` (lines 142–157) shells out to
`osascript` to get the frontmost app's PID. It is called from `start_recording`
*before* the start beep, so the subprocess spawn stalls the first beep — the
exact macOS sibling of the memoized-fingerprint bug (commit 489e2fc).

**Fix:** get the frontmost PID via `NSWorkspace` (no subprocess). Keep
`paste_into` on osascript — it runs after transcription, off the beep path, and
re-uses the same PID (`NSRunningApplication.processIdentifier` == unix PID, so
`paste_into`'s `unix id is {pid}` lookup is unchanged).

Add the dep (mac-only) in `src-tauri/Cargo.toml`:
```toml
[target.'cfg(target_os = "macos")'.dependencies]
objc2 = "0.5"
objc2-app-kit = { version = "0.2", features = ["NSWorkspace", "NSRunningApplication"] }
```
(Confirm the current objc2/objc2-app-kit versions and the exact feature names at
implementation time — these crates rev quickly.)

Replace `mac::foreground_window`:
```rust
/// Capture the PID of the frontmost application (the one receiving keystrokes).
/// Uses NSWorkspace rather than osascript so it never spawns a subprocess on the
/// hotkey→record→beep hot path.
pub fn foreground_window() -> isize {
    use objc2_app_kit::NSWorkspace;
    unsafe {
        NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .map(|app| app.processIdentifier() as isize)
            .unwrap_or(0)
    }
}
```
`Command` may become unused in the `mac` module after this — keep it (still used
by `paste_into`).

**Validate:** builds on `macos-14` CI; on a real Mac, the start beep is
instant after the hotkey (compare against `osascript` baseline); auto-paste
still lands in the correct app.

---

## 8b — Enable Metal (GPU on Apple Silicon)

**Problem:** `scripts/prepare-resources-macos.sh` (≈ lines 31–37) compiles
whisper-server with `-DWHISPER_NO_METAL=1` and even comments "Remove this flag in
a future release to enable Metal." So a ~$89 Mac buyer gets a CPU experience next
to Metal-fast MacWhisper/superwhisper — breaking the "GPU-fast, cross-platform"
headline.

**Fix:** drop `-DWHISPER_NO_METAL=1` from the cmake invocation. Metal links a
framework present on every Mac since 2014. Verify the build still succeeds and
the server actually offloads to the GPU (whisper-server logs Metal init).

**Coupling:** 8b alone produces a Metal binary the app still *reports* as CPU.
Ship it together with 8c so the engine label / GPU verdict are correct.

---

## 8c — Teach the app it has a Metal GPU

1. **`whisper.rs`** — add a `"metal"` arm to `binary_kind()` / `engine_kind`
   (mirror the CUDA/CPU arms) and an Apple-Silicon probe. Today `detect_nvidia_gpu`
   returns `None` on macOS (≈ lines 600–605); add a mac probe that reports the
   Apple GPU (e.g. via `sysctl machdep.cpu.brand_string` / `hw.optional.arm64`,
   or a static Apple-Silicon assumption gated on `cfg(target_arch = "aarch64")`).
2. **`SettingsPanel.jsx`** — fix `gpuVerdictText` (≈ line 553): "No dedicated GPU
   found" is wrong on Apple Silicon. When the Metal probe is positive, show the
   Apple GPU as the active accelerator.

**Validate on real Apple Silicon:** engine info reports Metal; the GPU panel
shows the Apple GPU; a transcription is GPU-accelerated (latency drop vs CPU);
Intel-Mac path (if still supported — see below) still reports CPU and works.

---

## 8d — Release held Shift on the macOS paste path (surfaced by re-paste-last)

**Problem:** `paste.rs` `win::paste_into` explicitly synthesizes Shift-up (and
Alt-up) before sending Ctrl+V (≈ line 118) precisely because "the user may still
hold Shift from the Ctrl+Shift+Space hotkey." The macOS `mac::paste_into` (≈
lines 166–171) has **no equivalent** — it runs `keystroke "v" using command
down` after only `delay 0.15`. For the new **re-paste-last** hotkey
(`CmdOrCtrl+Shift+V`) this matters: it fires within ~150 ms of the keypress, so
the user is likely still physically holding Shift, and the synthesized Cmd+V can
combine into **Cmd+Shift+V** ("Paste and Match Style" / a different shortcut in
many apps). Dictation tolerates it only because seconds of Whisper latency elapse
first; the synchronous paste-last does not.

**Fix:** before the `keystroke "v" using command down`, release Shift (and
Option) explicitly — e.g. post a Core Graphics `key up` for the Shift modifier /
clear the synthesized event's flags — mirroring the Windows VK_SHIFT key-up.
Intermittent and macOS-only, so it ships with this spec rather than blind from
Windows. (Found by the 2026-06-17 self-review; low severity.)

**Current state:** the re-paste-last hotkey is registered **Windows-only**
(`register_paste_last_hotkey` is gated behind `cfg!(target_os = "windows")` in
`main.rs` setup) so this buggy behavior does NOT ship to macOS users. Once this
modifier-release is implemented and validated, **remove that cfg gate** to enable
re-paste-last on macOS.

---

## Out of scope but adjacent (decide separately)

- **Intel-Mac build:** CI targets `aarch64` only. Decide Apple-Silicon-only
  (and say so in the GPU probe + marketing) or add the `x86_64` leg. The Metal
  probe must not claim a GPU on an Intel Mac without Metal-capable hardware.
- **macOS signing/notarization:** unsigned `.app` is likely Gatekeeper-*blocked*
  (handoff §7). Separate release-hardening task.

---

## Acceptance

- [ ] `cargo test` green on `macos-14` (CI) and Windows (no regression).
- [ ] Start beep is instant on a real Mac after the hotkey.
- [ ] whisper-server initializes Metal; a transcription is GPU-accelerated.
- [ ] Engine info + GPU panel report Metal/Apple GPU correctly.
- [ ] Auto-paste still targets the correct app.
- [ ] Dual-platform note in each commit; Windows behavior unchanged.
