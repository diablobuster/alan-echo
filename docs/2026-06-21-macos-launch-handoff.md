# ALAN Echo — macOS Launch Handoff

**Date:** 2026-06-21
**Goal:** Ship a fully working macOS build at parity with Windows — a paying customer can **download → launch (no Gatekeeper block) → activate → dictate (Metal-fast) → auto-update**.
**Status going in:** Windows funnel is **live + verified** (serves v1.2.3, advertised hash == served-file hash). macOS is **intentionally gated out of checkout** until this handoff is done.

> **Read these first:**
> - `docs/2026-06-17-slice8-macos-parity-spec.md` — the osascript hot-path + Metal detection spec (already written; this handoff references it, doesn't duplicate).
> - `docs/auditimprove/2026-06-20/audit.md` — C4 + the macOS parity/e2e findings with file:line.
> - `docs/2026-06-21-echo-ship-out-deploy-session-log.md` — the Windows ship-out, including two hard-won lessons that apply directly here (see Guardrails).

---

## 0. What's already done (don't redo)
- **Site download is platform-aware** (`stock-analyzer/lib/echo/download.ts`): the resolver selects `.dmg` for `?platform=mac` via the private-repo signed-URL token flow (repo stays private). `app/api/echo/version` branches on `?platform` and returns per-platform `version`/`sha256`/`downloadUrl`. Mac env vars are already wired: `ECHO_MAC_DOWNLOAD_URL`, `ECHO_MAC_INSTALLER_SHA256`, `ECHO_MAC_INSTALLER_VERSION`, `ECHO_MAC_INSTALLER_MB`, `ECHO_MAC_RELEASE_DATE` (all currently unset → mac returns null sha = fail-safe "no update").
- **App updater already handles macOS** (`src-tauri/src/updater.rs`): platform select (line ~9), saves `.dmg` vs `.exe` (~63-66), mac-gated install (~130/152). It calls `/api/echo/version?platform=mac`.
- **Activation contract holds cross-platform** (audit-verified): the app's embedded Ed25519 `PUB_KEY` matches the server signing key; machine fingerprint is computed on mac too. No work needed.
- **`scripts/echo-release-preflight.ts --mac`** exists — asserts the served `.dmg` hash == advertised (use it in the runbook).
- Marketing already softened: the `/echo` FAQ says "we intend your license to cover the Mac build when it's available."

## 1. The blocking work (in dependency order)

### A. Apple Developer Program enrollment — **start now (lead time)**
Identity verification can take days. Enroll ($99/yr), then create a **"Developer ID Application"** certificate (distribution *outside* the App Store) and note the **Team ID**. Export the cert + private key as a `.p12`.

### B. Code signing + notarization in CI — **the hard Gatekeeper blocker (C4)**
Today: `tauri.conf.json:58-59` macOS block has only `minimumSystemVersion: 10.15` (no signing); `build-macos.yml:54` sets `TAURI_SIGNING_PRIVATE_KEY:""` (that's the *updater* key, not an Apple identity) and has **no** codesign/notarytool/stapler step. An unsigned + un-notarized internet-downloaded `.app` is **hard-blocked** by Gatekeeper ("damaged / cannot verify developer") — a paying Mac buyer can't launch it.

**Do:**
1. `tauri.conf.json` → `bundle.macOS`: add `"entitlements": "entitlements.plist"` and (optional) `"signingIdentity"`/`"providerShortName"` (or drive identity via the `APPLE_SIGNING_IDENTITY` env). Tauri 2 enables hardened runtime automatically when signing.
2. Create `src-tauri/entitlements.plist` with the mic entitlement (the app records audio → required under hardened runtime):
   ```xml
   <key>com.apple.security.device.audio-input</key><true/>
   ```
   If the bundled `whisper-server` sidecar fails to launch under hardened runtime, also add `com.apple.security.cs.allow-unsigned-executable-memory` / `disable-library-validation` (test minimal set first).
3. Add **`NSMicrophoneUsageDescription`** (mic permission prompt string) to the macOS Info.plist (Tauri 2: via a bundled `Info.plist` or the config) — without it macOS silently denies mic access.
4. `build-macos.yml` — set the Tauri signing/notarization env from CI secrets and let the bundler sign + notarize + staple:
   - `APPLE_CERTIFICATE` (base64 of the `.p12`), `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY` (`"Developer ID Application: <Name> (<TEAMID>)"`).
   - Notarization: either `APPLE_API_ISSUER` + `APPLE_API_KEY` + `APPLE_API_KEY_PATH` (App Store Connect API key — preferred for CI) **or** `APPLE_ID` + `APPLE_PASSWORD` (app-specific password) + `APPLE_TEAM_ID`.
   - Tauri runs notarization + stapling during `tauri build` when these are present and signing is configured.
5. Verify in CI (and locally) on the artifact: `spctl -a -vvv --type install <app-or-dmg>` and `xcrun stapler validate <dmg>` → must pass.

### C. Architecture decision — universal2 vs Apple-Silicon-only
`build-macos.yml` builds `aarch64-apple-darwin` only, while `minimumSystemVersion 10.15` (Catalina) implies Intel support → an Intel Mac buyer gets a non-runnable ARM binary. **Decide explicitly:**
- **Option 1 (recommended if Intel matters): universal2.** Build `--target universal-apple-darwin`. Critically, the bundled `whisper-server` must ALSO be universal — `scripts/prepare-resources-macos.sh` must build it with `-DCMAKE_OSX_ARCHITECTURES="arm64;x86_64"` (or `lipo` two builds). Keep `minimumSystemVersion 10.15`.
- **Option 2 (simpler): Apple-Silicon-only.** Keep `aarch64`, raise `minimumSystemVersion` to `11.0`, and state "Apple Silicon only" on the download + pricing pages (so no Intel buyer is misled).

### D. Metal GPU — parity + the "GPU-fast" claim
Today Mac is CPU-only: `scripts/prepare-resources-macos.sh:36` passes `-DWHISPER_NO_METAL=1`; `whisper.rs:601` `detect_nvidia_gpu` returns `(None,None)` on non-Windows; `binary_kind` (`whisper.rs:557`) has no `metal` arm → `engine_kind` is always `cpu` on Mac. **Per the slice8 spec:**
1. Remove `-DWHISPER_NO_METAL=1` (Metal links a framework present on every Mac since 2014).
2. Add Apple-Silicon/Metal detection in `whisper.rs` and a `"metal"` `binary_kind`/`engine_kind` arm; fix any "No dedicated GPU found" copy on Mac.
3. **Must validate on real Apple Silicon** (transcription should drop to ~1-2s on the GPU path).

### E. osascript hot-path replacement — perf parity (per slice8 spec)
`paste.rs:142-197` shells out to `osascript` twice per dictation (frontmost-app capture + paste), adding ~100-400ms each + fragility. Replace with native APIs: `NSWorkspace.frontmostApplication` (capture) + `CGEventPost` (Cmd+V) — in-process, no subprocess. Lower priority than B/C/D but do it before marketing "snappy on Mac."

### F. Site — ungate checkout + wire the `.dmg` (mostly config now)
1. **Ungate macOS checkout:** `stock-analyzer/app/api/echo/checkout/route.ts` currently redirects macOS UAs to `/echo?mac_notice=1` (the `isMacBrowser` block). Remove/relax it once Mac is fulfillable. Also remove the `mac_notice` banner in `app/echo/page.tsx` (or repurpose it to "Mac now available").
2. **Download page Mac variant:** `app/echo/download/page.tsx` is Windows-only copy ("Windows 10/11", `Get-FileHash`, SmartScreen). Add a Mac branch: the `.dmg`, Gatekeeper "open on first launch" note, and `shasum -a 256` verify instructions.
3. **Set the mac env** (Production + Preview): `ECHO_MAC_INSTALLER_VERSION`, `ECHO_MAC_INSTALLER_SHA256` (the **served-asset** hash — see runbook), `ECHO_MAC_INSTALLER_MB`, `ECHO_MAC_RELEASE_DATE`. The resolver finds the `.dmg` in the release via the token (no `ECHO_MAC_DOWNLOAD_URL` needed unless you host it elsewhere).

### G. EULA / legal — widen the grant (counsel)
EULA §1 (`app/legal/echo-license/page.tsx:104-109`) grants use on **"Windows devices"** only. Widen to platform-neutral ("devices") so the contract matches the cross-platform sale. **Counsel verbatim — flag, don't improvise.** Update the FAQ "we intend…" line to present-tense once Mac ships. Legal-page edits must ride the disclosures.ts CI gate.

## 2. Release + env runbook (mac) — mirrors the Windows flow, with the lessons baked in
1. Tag-trigger `build-macos.yml` (now signing+notarizing) → produces a **signed + notarized + stapled** `.dmg` (and `.app`).
2. Upload the `.dmg` + `SHA256SUMS` to `diablobuster/alan-echo-releases` (private; the token resolver serves it).
3. **Get the hash by downloading the actual released `.dmg` and hashing it** (`shasum -a 256`) — NOT a local build, NOT the SHA256SUMS file blindly (Windows lesson: those drifted). Set `ECHO_MAC_INSTALLER_SHA256` to that value.
4. Set the `ECHO_MAC_*` env (prod). Run `npx tsx scripts/echo-release-preflight.ts --mac` → must print "served binary matches advertised SHA".
5. **Redeploy production** (env is snapshotted at deploy-CREATION, so a deploy created *after* the env change is required — the live deploy won't pick up new env on its own).
6. Ungate checkout (F.1).
7. **Verify live, anonymously:** `/api/echo/version?platform=mac` returns the mac version+sha; download the live `.dmg` and confirm its `shasum` == advertised.

## 3. Acceptance / test matrix (on real hardware — Apple Silicon, + Intel if universal2)
- [ ] `.dmg` downloads from the live site (200, signed URL).
- [ ] Double-click opens **without** Gatekeeper block; `spctl`/`stapler validate` pass.
- [ ] Mic permission prompt appears; dictation produces text.
- [ ] `engine_kind` reports **metal**; transcription is GPU-fast (~1-2s).
- [ ] Buy flow (checkout ungated) → success page shows key → download `.dmg` → paste key → activate (Ed25519 token verifies) → dictate.
- [ ] In-app update: `version?platform=mac` → `.dmg` download → hash matches `ECHO_MAC_INSTALLER_SHA256` → installs.
- [ ] Intel path (if universal2): all of the above on an Intel Mac.

## 4. Guardrails & lessons (non-negotiable)
- **Hash the actually-served file, never a recorded SHA256SUMS.** On Windows, `SHA256SUMS-123.txt`/the handoff listed a stale build's hash (`74569fc6`) while the real served asset was `cb772f2d`; setting the wrong one created an updater-brick mismatch caught only by downloading + hashing the live file. Do the gold-standard download-and-hash check before declaring done.
- **Vercel env is snapshotted at deploy-creation, not build-time** — always trigger a *fresh* deploy after any env change, then verify live.
- **Resident-app rule:** the user runs Windows Echo daily on the dev machine; do not `tauri dev`. Test the Mac build on **real Mac hardware** (CI builds; verify on a device), never by poking the resident app.
- **Dual-platform mandate** still applies to any shared code you touch.
- **Apple enrollment has lead time** — do step A first; B-G can proceed in parallel but B can't be *verified* until the identity exists.
- **Legal text is counsel verbatim** (G); legal-page edits ride the B11 disclosures.ts CI gate.
- **Parallel session** edits these repos concurrently — work on a branch, verify git state, never deploy from the shared (often dirty) working tree; redeploy from the latest `main` git source.

## 5. Suggested sequencing
A (enroll, async) → in parallel: C (arch decision), D (Metal), E (osascript), F.3 site copy. Then B (signing/notarize CI) once the identity exists → release runbook (§2) → ungate checkout (F.1) → verify on hardware (§3) → flip live. G (EULA) before taking Mac money.

---
**Bottom line:** the *plumbing* (site download/version, app updater, activation) is already cross-platform and done. The macOS launch is now four real pieces of work — **Apple signing+notarization, the arch decision, Metal, and ungating the funnel** — plus the EULA grant. None are research; all are scoped above with file:line.
