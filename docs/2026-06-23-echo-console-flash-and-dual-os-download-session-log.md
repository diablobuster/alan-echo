# Session log — Echo console-flash fix + dual-OS free download page

**Date:** 2026-06-23
**Scope:** (1) stop the console window flashing during dictation (alan-echo desktop); (2) turn the site download page into a free Windows **+** Mac chooser with per-OS install steps (stock-analyzer).
**Status:** Shipped. Both fixes committed on dedicated branches off `origin/main` + PRs opened; dual-OS page deployed to production via `vercel --prod` (owner said "fix everything and get this up and running"). Production env was already wired for Mac, so no env change was needed.

---

## 1. What shipped (working tree only)

### A. Console-flash fix — `alan-echo`
The brief black terminal that blinks each time Echo finishes a dictation is the Windows `reg` command being spawned **without** the `CREATE_NO_WINDOW` flag. Every other subprocess in the app (whisper server, `nvidia-smi`, `powershell`) already sets it; the two `reg` calls in `trial.rs` were the only ones that didn't. Path: a dictation finishes → `transcribe` (main.rs:430) → `increment_trial_count` (main.rs:449) → `trial::save` → `write_registry` spawns `reg add`. `read_registry` (startup) had the same gap.

### B. Dual-OS free download page — `stock-analyzer`
`/echo/download` was Windows-only (single free button + SmartScreen/PowerShell copy). It now shows **two side-by-side cards, Windows and macOS**, each a free-trial download with its own version/size/SHA-256 + per-OS first-run instructions. Decision (owner-approved this session): **both platforms get the free trial**, reversing the 2026-06-21 "Mac = paid-only / free = Windows-only" plan. The Mac card carries an honest "new on Mac — not yet Apple-signed" note with the Gatekeeper right-click-Open + Microphone + Accessibility steps. The backend already supported this (`?platform=mac` on `download-free`, `version`, and the key-gated `download`); this was a frontend change only.

---

## 2. Files touched + intent

| File | Change | Intent |
|---|---|---|
| `alan-echo/src-tauri/src/trial.rs` | Added Windows `CommandExt` import + `CREATE_NO_WINDOW` const; applied `.creation_flags(CREATE_NO_WINDOW)` to both `reg` calls (`write_registry`, `read_registry`). | Suppress the console window on every trial-state write (and the startup read). Windows-only (`#[cfg(target_os="windows")]`); macOS untouched. |
| `stock-analyzer/app/echo/download/page.tsx` | Rewrote the free-download section into two cards (Windows + macOS); added a `?platform=mac` version fetch + Mac metadata state; per-OS install steps; the unlocked/key-verified panel now offers both Windows and Mac receipt-linked downloads; moved a one-line "Mac uses your GPU automatically" note into the GPU section. Header/footer/key-verify form preserved. | Let free users pick their OS with correct, honest install guidance for each. |

No backend/API files changed — the platform plumbing already existed.

---

## 3. Commits & PRs

Both changes were branched off `origin/main` (the real active branch — `origin/master` is an abandoned April branch; local `main` was stale). To avoid disturbing the parallel session's loaded working tree, branches were built in isolated worktrees / via git plumbing.

| Repo (GitHub) | Branch | Commit | PR |
|---|---|---|---|
| alan-echo (`diablobuster/alan-echo`) | `fix/echo-no-console-flash` | `a69de8d` | [#4](https://github.com/diablobuster/alan-echo/pull/4) |
| stock-analyzer (`diablobuster/ALAN_post_integration`) | `feat/echo-download-dual-os` | `354ee91e` | [#758](https://github.com/diablobuster/ALAN_post_integration/pull/758) |

**Deploy:** stock-analyzer page deployed to production with `vercel --prod` from a clean worktree of the branch (project `alan_intelligence`, team `alanglobalintelligence`). Prod was already at `origin/main`'s tip (`70bfb864`), so the deploy added exactly the one page file vs. what was live. Production is CLI-deployed (not auto-on-merge), so the PR merge to `main` is for history/sync; the page went live via the CLI deploy.

**Both PRs merged to `main`** (2026-06-24): #4 → `d9b645b` (alan-echo), #758 → `8b2b69ca` (ALAN_post_integration). The failing PR checks were confirmed pre-existing infra issues (alan-echo backend CI dies on missing `resources/models`; ALAN_post `tsc` OOMs on the runner), not regressions from these changes.

**Windows installer build (COMPLETE):** there is **no Windows CI build workflow** (only "Build macOS" + a lint/test "CI"); Windows installers are built locally on this machine (`scripts/prepare-resources.ps1` copies a machine-local `whisper-server.exe` + VS redist DLLs — no in-CI compile path). Built locally from a clean `main` worktree (`d9b645b`, fix present), `tauri build --bundles nsis`, ~5m21s release compile.
- Output (copied out, temp worktree removed): `C:\Users\arowm\Downloads\ALAN-Echo-1.2.3-noflashfix_x64-setup.exe`
- SHA-256: `bfc18b1abe04b2c4e7fc92df2a3809f7dc8fd7f3ab7dfd1574135a76f02c647d` · 135,395,510 bytes · version 1.2.3
- Local installer for the owner to run; **NOT** republished to the live download/release (would need a version bump + re-upload + prod-SHA update). Safe to install over 1.2.3 (updater is version-based: `version_gt("1.2.3","1.2.3")==false`, no revert).

---

## 4. Verification — tested vs deferred

**Tested:**
- `alan-echo/src-tauri`: `cargo check` → **exit 0** (compiles clean, incl. the Windows `cfg` path since the host is Windows).
- `stock-analyzer`: `npx tsc --noEmit` → **exit 0** (whole project); `npx eslint app/echo/download/page.tsx` → **0 errors** (4 warnings, all pre-existing `<img>` LCP notices in the shared header/footer).

- **Live production page** — after `vercel --prod`, `https://www.alanglobalintelligence.com/echo/download` serves both OS cards ("Download for Windows" + "Download for Mac", "new on Mac", "Apple Silicon"); the old Windows-only free section is gone.
- **End-to-end Mac download (live)** — `download-free?platform=mac` 302-redirects to the real `ALAN.Echo_1.2.3_universal.dmg`.

**Deferred (not done this session):**
- **Runtime confirmation of the flash fix** — did not run a built Echo and watch a dictation (the resident tray app is running; building/installing a fresh installer is heavy). The fix is a one-pattern change matching `whisper.rs`/`packs.rs`, compile-verified.
- **Visual/responsive QA** — confirmed the markup is served, but did not eyeball the two-card layout in a browser (mobile stack, copy-button states). Worth a quick look.

---

## 5. Follow-ups / known limitations / decisions for the owner

1. **Branch placement — RESOLVED.** Both changes were branched cleanly off `origin/main` (not committed onto the unrelated branches they were edited on) via isolated worktrees/plumbing, so the parallel session's loaded working tree was never disturbed. **Still open:** the parallel session's `stock-analyzer:feat/echo-mac-funnel-eula` branch goes the **opposite** direction (gates Mac behind purchase). This page supersedes that for `/echo/download` — make sure that branch isn't later merged in a way that reverts this. PRs #4 and #758 should be merged to `main` to keep main in sync with what's deployed.

2. **Prod-env prerequisite — RESOLVED (already in place).** Verified against the live prod API: `version?platform=mac` → v1.2.3 + sha `8f82477…`, and `download-free?platform=mac` 302-redirects to the real `ALAN.Echo_1.2.3_universal.dmg`. `ECHO_RELEASE_TAG` (=v1.2.3 effectively), `ECHO_MAC_*`, and `GITHUB_RELEASES_TOKEN` are all set in Production. No env change was needed; v1.2.3 carries both the `.exe` and `.dmg`.

5. **Desktop fix needs a build to reach users.** The console-flash fix is in the binary, so it only takes effect after a new Windows installer is built and reinstalled — the site deploy doesn't touch it. PR #4 is open; a release build was NOT cut this session (offered to the owner).

3. **Mac is untested on real hardware.** Per `docs/2026-06-21-NEXT-STEPS-mac-launch.md`, the Mac build "has never run on a Mac." Offering it as a free public download means untested software reaches anyone — the honest "new on Mac" note is in, but a real Mac smoke test is still owed.

4. **Out of scope (left as-is):** existing "NVIDIA" GPU-pack copy (conflicts with the no-brand-names preference but predates this change); `<img>` → `next/image` LCP warnings.

---

## 6. Post-merge activity (2026-06-24)

**PRs merged to `main`:** #4 → `d9b645b` (alan-echo), #758 → `8b2b69ca` (ALAN_post_integration). Failing PR checks confirmed pre-existing infra (alan-echo backend CI dies on missing `resources/models`; ALAN_post `tsc` OOMs on the runner), not regressions.

**Windows installer built + installed on this machine (owner request "run the installer for me"):**
- Built locally from a clean `main` worktree (`d9b645b`), `tauri build --bundles nsis`, ~5m21s. No Windows CI build workflow exists (only "Build macOS" + lint/test "CI") — Windows installers are local-only here (`scripts/prepare-resources.ps1` copies a machine-local `whisper-server.exe` + VS redist DLLs).
- Output: `C:\Users\arowm\Downloads\ALAN-Echo-1.2.3-noflashfix_x64-setup.exe`, SHA-256 `bfc18b1abe04b2c4e7fc92df2a3809f7dc8fd7f3ab7dfd1574135a76f02c647d`.
- **Installed it:** stopped the resident Echo (PID 82228) + its CUDA `whisper-server.exe` by path, ran the NSIS installer silently (`/S`, exit 0), confirmed the binary at `%LOCALAPPDATA%\ALAN Echo\alan-echo.exe` was replaced (Jun 17 build `5A7BEBDA…` → Jun 23 build `47459E65…`), relaunched Echo (now PID 92228). Local file = no Mark-of-the-Web → no SmartScreen; per-user install → no UAC.
- Deferred: did not trigger a live dictation to visually confirm the flash is gone (needs audio/hotkey). Fix is in the installed binary by provenance + hash.
- NOT republished to the public download/release (would be a version bump + re-upload + prod-SHA update).

**Brendan onboarding email (Mac):** drafted and saved to `C:\Users\arowm\Downloads\brendan-echo-setup-email.md` — download → Gatekeeper install → free-license redemption via `?ack=mac` checkout + a 100%-off promo code. Placeholders: `[Your name]`, `[PROMO CODE]`.

**Giveaway promo codes — could not retrieve programmatically.** 10 single-use 100%-off codes exist (allowlisted in prod `ECHO_GIVEAWAY_PROMO_IDS`, created by `scripts/echo-giveaway.ts --codes 10`), but they're live-mode only: prod `STRIPE_SECRET_KEY` is "Sensitive" (pulls empty), local Stripe CLI is test-mode only, and the creation run was never logged. → Owner must read an unused code (`times_redeemed=0`) from the live Stripe Dashboard (coupon "ALAN Echo giveaway (single-use)"), or provide the live key for me to query. (Pulled prod env to a scratch temp file to check; deleted it, verified no secret files left behind.)

**Mac checkout gate — RESOLVED + deployed (2026-06-24).** PR [#760](https://github.com/diablobuster/ALAN_post_integration/pull/760) (`fix/echo-mac-sell-unblock`, commit `83c612c1`), built in an isolated worktree off `origin/main` (node_modules + .vercel junctioned, then unlinked before teardown). Three files:
- `app/api/echo/checkout/route.ts` — removed the Mac→`/echo?mac_notice=1` redirect; product description → "for Windows and Mac".
- `lib/echo/email.ts` — license-email download CTA repointed from the Windows-default `/api/echo/download?key=…` to `/echo/download` (dual-OS page); masthead + install steps now cover Windows **and** Mac (HTML + text).
- `app/echo/page.tsx` — removed the dead "Echo is Windows-only right now" banner (+ its `mac_notice` param); requirements row "Mac: Coming soon" → "macOS: Supported — Apple Silicon & Intel"; refreshed the post-purchase + "Does Echo work on Mac?" FAQs.
- Verified: `tsc` + ESLint clean; deployed `vercel --prod`; live checks confirm Mac buyer now lands on `/signup → Stripe` (was `mac_notice`), and the stale copy is gone.

**Still owner-only (cannot fix in code):** the Mac build has never run on real Mac hardware — a launch/dictate/paste/activate smoke test is owed before charging Mac users with full confidence. Windows funnel is fully live; Mac funnel is now mechanically unblocked end-to-end.

**"Download free" CTA → download page — RESOLVED + deployed.** PR [#761](https://github.com/diablobuster/ALAN_post_integration/pull/761) (`00be1ae3`). `app/echo/DualCta.tsx` (the hero/pricing/closing CTA) linked straight to `/api/echo/download-free` (Windows `.exe` by default), so Mac visitors clicking "Download free" got the Windows installer. Repointed to `/echo/download` (OS chooser). Verified live: `/echo` now has zero direct `download-free` links; the button routes to the dual-OS page.

**Branch sync:** PRs #760 (`3e4a29cc`) and #761 merged to `main`, so `main` now matches what's deployed (earlier CLI `vercel --prod` deploys had put prod ahead of main).

## Promo / giveaway (2026-06-24)
Found the 8 unused 100%-off Echo giveaway codes via the (rolled-after-use) live Stripe key; assigned `V0YWVM0Y` to Brendan. Email saved to `Downloads/brendan-echo-setup-email.md`; full code inventory at `OneDrive/Desktop/ALAN Intelligence/ALAN Echo - Free License Codes.md`. Owner rolled the live `sk_live_…` key afterward (it had been pasted in chat).
