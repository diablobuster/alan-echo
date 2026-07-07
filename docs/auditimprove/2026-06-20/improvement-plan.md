# ALAN Echo — Improvement Plan (sequenced, batched to minimize deploys)

Derived from `audit.md` (2026-06-20). Ordered by **path-to-GO for a Windows v1 launch**, then Mac, then hardening. Each item: effort · ROI · revenue tag. Most fixes live in **`stock-analyzer`** (the site); program items are in **`alan-echo`**.

> **Do not auto-apply on production payment/legal/release surfaces.** This plan is for human-reviewed execution. Re-verify each fix against a live, anonymous client.

---

## BATCH 0 — Unblock the funnel (must ship first; 1 deploy) · `stock-analyzer`
*Without this, the product cannot be downloaded or trusted. Highest ROI possible: 0 → functional.*

1. **Fix the download** (C1). Make `diablobuster/alan-echo-releases` public **or** unset `ECHO_DOWNLOAD_URL` so `/api/echo/download` uses the `GITHUB_RELEASES_TOKEN` resolver; give `/api/echo/download-free` an authenticated resolver too. *Effort: S · ROI: critical · Revenue: enabling.*
2. **Single-source version + tag + hash** (C2, H-rel-a/b, MED installer-hash). Drive `version`, `ECHO_RELEASE_TAG`/`ECHO_DOWNLOAD_URL`, and the displayed + updater SHA from **one** release manifest (ideally the uploaded `SHA256SUMS.txt`). Make `/api/echo/version` branch on `?platform` and emit `downloadUrl`. *Effort: M · ROI: critical · Revenue: enabling.*
3. **Add a deploy preflight** asserting `sha256(file@ECHO_DOWNLOAD_URL) == ECHO_INSTALLER_SHA256 == NEXT_PUBLIC_ECHO_INSTALLER_SHA256` for the served tag, per platform, before the env flip ships. *Effort: S · ROI: high · Revenue: protective.*
4. **Verify live** (anonymous): download (free + key-gated, both platforms once Mac is gated), `version` per platform, `Get-FileHash` matches. *Effort: S.*

## BATCH 1 — Truth + checkout integrity (1 deploy) · `stock-analyzer`
5. **Rewrite the "activates offline / no account" FAQ** (C3) to match the EULA + reality. *Effort: S · ROI: high (legal) · Revenue: protective.*
6. **Redirect-on-error for the buy route** (H1) — mirror the download route's pattern; friendly retry banner on `/echo`. *Effort: S · ROI: high · Revenue: direct.*
7. **Reconcile Mac copy** (part of C4): remove "your key will work on both platforms" and the Stripe product description "for Windows"; keep "Mac — coming soon." *Effort: S · ROI: high (legal).*

## BATCH 2 — Gate Mac out of sale (1 deploy) · `stock-analyzer`
8. **Gate macOS out of checkout** (C4) until the Mac launch — don't sell a SKU you can't fulfill. Detect platform on the buy path and route Mac users to a "notify me" instead of Stripe. *Effort: S-M · ROI: critical (refund prevention) · Revenue: protective.*

## BATCH 3 — Delivery + measurement + cap (1-2 deploys) · `stock-analyzer`
9. **Re-enable license email or render the copyable key on success** (H2); auto-email gift recipients or remove the gift field. *Effort: S-M · ROI: high · Revenue: direct (refund/support reduction).*
10. **Add the purchase-conversion event** (H5) + track abandons + nav/mobile CTAs. *Effort: S · ROI: high · Revenue: measurement.*
11. **Make the 5-machine cap atomic** (H3) — transaction or conditional-increment counter. *Effort: M · ROI: high (revenue leak + counsel control) · Revenue: protective.*
12. **Ship per-machine deactivation** (MED) or fix the misleading cap error copy + give support an admin action. *Effort: M · ROI: med · Revenue: protective.*

## BATCH 4 — Program/release hardening · `alan-echo`
13. **Add SHA-256 verification to pack/model downloads** (H4) — reuse `updater.rs:110-122`; or disable the GPU pack for v1. *Effort: S-M · ROI: high (RCE) · Revenue: protective.*
14. **Sign update artifacts / pin a key** (H6) — ed25519 detached signature against a const pubkey, verified before launch. *Effort: M · ROI: high (security) · Revenue: protective.*
15. **Add a `windows-latest` release workflow** (H7) — build whisper.cpp from pinned upstream, pin CRT, `tauri build`, upload artifacts + `SHA256SUMS`. *Effort: M · ROI: high (provenance/bus-factor) · Revenue: protective.*
16. **Quick program correctness:** CSV formula-injection guard (`db.rs`), `version_gt` pre-release handling (`updater.rs`), percent-encode the key in the update URL. *Effort: S each.*

## BATCH 5 — Funnel optimization (post-launch, revenue upside) · `stock-analyzer`
17. **Statically/edge-cache `/echo`** (decouple `await headers()`/SessionProvider from the marketing tree) — TTFB/LCP + infra cost. *Effort: M · ROI: med-high · Revenue: conversion.*
18. **Reduce checkout friction** — reconsider forced account creation, or make the account's value explicit on the CTA. *Effort: M · ROI: med · Revenue: direct.* (Validate with an A/B test.)
19. **Match keys by `userId`/recover link** on `/echo/keys` (MED email-mismatch); add rate limit to `/api/echo/download`; harden error boundary; confirm Redis provisioned. *Effort: S each.*

## MAC LAUNCH (separate track, not v1) · `alan-echo` + `stock-analyzer`
- Developer ID sign + `notarytool` + `stapler` in CI; add audio-input entitlement under hardened runtime.
- Drop `-DWHISPER_NO_METAL=1`; add Metal/Apple-Silicon detection + a `'metal'` engine arm.
- `.dmg` branch in the download resolver + per-platform `version` (already in Batch 0 #2).
- Decide universal2 vs Apple-Silicon-only (raise `minimumSystemVersion` to 11.0 if AS-only); replace the `osascript` hot path with native APIs.

## TRACKED / TIME-BOXED (not code, do not improvise)
- **Copyright application received by 2026-09-10** (§412 window).
- **Confirm EULA v2 counsel sign-off** — code comment says "COUNSEL-APPROVED / PUBLISHED"; memory says hold-for-counsel.
- **Windows code signing** (SmartScreen) — deliberate, checksum-mitigated; schedule when budget allows.

---

### Suggested deploy sequencing
`Batch 0` (verify live) → `Batch 1` + `Batch 2` together → **soft launch Windows** → `Batch 3` → `Batch 4` → flip to broad/public → `Batch 5` → Mac track.
