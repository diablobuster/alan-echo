# ALAN Echo — macOS CI Build Validation (session log)

**Date:** 2026-06-21
**Branch:** `feat/macos-launch-scaffold` (pushed to origin)
**Commits:** `f6a935c` (CI fix) — follows the scaffold in `2026-06-21-macos-launch-scaffold-session-log.md`
**CI runs:** `27916919377` (failed, diagnostic) → `27917303491` (**success**)

## 1. What happened
Pushed the macOS branch and triggered `build-macos.yml` on the `macos-14` (Apple Silicon) runner — the first time this code touched a real Mac.

- **Run 1 (`27916919377`) — failed at 10m42s, but only at the final codesign step.** Everything before it succeeded: `npm ci`, the universal+Metal whisper-server build (`Prepare macOS resources`), and crucially the full Rust compile + universal bundling. The error was `failed codesign application: failed to run command security import: failed to import keychain certificate`.
  - **Root cause (a CI bug, not a code bug):** the Apple secrets were mapped to **job-level `env:`**, so an *unset* secret became an **empty-but-present** `APPLE_CERTIFICATE` in `tauri build`'s environment. Tauri's bundler treats that as "import this cert" and runs `security import` on empty data → fail. The "graceful unsigned fallback" leaked the empty vars.
- **Fix (`f6a935c`):** pass `APPLE_*` only to the steps that need them (never job-level); gate on a `HAS_APPLE_CERT` boolean (`${{ secrets.APPLE_CERTIFICATE != '' }}`); split into mutually-exclusive **unsigned** / **signed** build steps so the unsigned path never sees an `APPLE_*` var.
- **Run 2 (`27917303491`) — SUCCESS (~8.5 min).** Produced + uploaded an **unsigned universal `.dmg` (140 MB)** and the `.app`. Signing/verify steps correctly skipped (no identity).

## 2. What this validates (on real macOS, first time)
- ✅ The **Rust compiles on macOS** — including the native `paste.rs` (objc2-app-kit + CoreGraphics) that had never been compiled anywhere, `detect_apple_gpu`, and the resource-resolution fix.
- ✅ The **universal arm64+x86_64** build works; the **Metal whisper-server** builds (full-Xcode `metal` preflight passed).
- ✅ It **bundles into a `.dmg`** and uploads. The ~140 MB compressed size is consistent with the bundled base model + universal binaries (sanity-check the model is inside when testing on hardware).
- ✅ The **graceful-unsigned CI path** is now correct, and the **signed path** is wired (untestable until the Apple identity exists).

## 3. Verification status
- **Tested:** macOS compile + universal + Metal build + bundle + artifact upload (CI, green).
- **Deferred (need a real device / Apple identity):** signing + notarization (no cert yet); Metal transcription *speed*; the native Cmd+V paste + Accessibility prompt; mic permission; that the bundled app actually locates + runs the model/whisper-server; universal binary on a real Intel Mac. The unsigned `.dmg` can be run locally on a Mac via right-click → **Open** for an eyeball test (it is NOT a customer-downloadable artifact — Gatekeeper blocks unsigned internet downloads).

## 4. Release-readiness verdict: **NO-GO for full release**
The build proves the engineering is sound; release is blocked entirely by **non-code gates**:
1. 🔴 **Apple Developer enrollment** — the wall. Unsigned = Gatekeeper hard-block = no customer download. ($99/yr, identity verification takes days.) Critical path.
2. 🔴 **On-device validation** — nothing exercised on real Apple hardware (Metal speed, paste, mic, activation).
3. 🔴 **Site funnel not live** — `ECHO_MAC_*` env unset, checkout still gated to Windows, no `.dmg` on the release host.
4. 🔴 **EULA §1 grant** still "Windows devices" (counsel).

**Unblock order:** enroll (A) → add `APPLE_*` secrets → re-run this exact build (auto-signs/notarizes/staples) → validate the signed `.dmg` on a real Mac → wire the site funnel + EULA → flip live.
