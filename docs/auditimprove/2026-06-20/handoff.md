# ALAN Echo — Ship-Readiness Remediation Handoff

**Date:** 2026-06-20 · **Source:** `docs/auditimprove/2026-06-20/audit.md` + `improvement-plan.md`
**Verdict carried in:** ❌ **NO-GO** for public launch — the production download is dead (private-repo 404) and version/hash truth is inconsistent. Windows v1 is shippable after a focused blocker burn-down; **macOS must not be sold in v1.**

> **GUARDRAILS — read first.** If this session runs with `--dangerously-skip-permissions`, no prompt will stop a destructive action. Self-enforce: **never** delete files, drop/alter tables, remove features, force-push, or run destructive commands. Improvements must be additive / organizational / performance / security / revenue-enabling. If a fix appears to require a removal, write the proposed destructive action to a file and stop for human approval.
>
> **These are production payment, legal, and release surfaces.** Do **not** auto-apply changes to checkout, the Stripe webhook, EULA/legal copy, activation, or release/env config — propose, get human review, and re-verify against a live anonymous client.
>
> **Repo-specific:**
> - **Dual-platform always** (`[[dual-platform-dev]]`) — but for v1, Mac is being *gated out of sale*, not shipped; the Mac fixes are a separate track.
> - **A parallel Claude session edits these repos concurrently** (`[[parallel-session-coordination]]`) — re-check `git log`/`git status` before committing; never sweep unrelated working-tree changes (lock files, legal docs) into a commit; never `git reset`/force-push.
> - **Resident Echo app is running on this machine** (`[[resident-echo-app]]`) — never GUI-test via `tauri dev` while it runs; it shares the `com.alan.echo` data dir. Match processes by path, not name.
> - **Legal text is verbatim-from-counsel** (`[[echo-legal-status]]`) — flag legal-copy issues, don't improvise fixes; legal-page edits ride the disclosures.ts CI gate.
> - **No emojis in code/CLI output.**

## Two repos
- **`C:\Users\arowm\stock-analyzer`** — the website + buying funnel + license backend (most blockers).
- **`C:\Users\arowm\alan-echo`** — the Tauri program (currently branch `feat/ultra-audit-stabilization`, v1.2.3; stabilized).

## The blocker list (full detail + file:line in audit.md)
**CRITICAL (before any public traffic):**
- **C1** Download 404 in prod — `site` `app/api/echo/download-free` + `download/route.ts:16-17` (private-repo direct URL).
- **C2** Version/hash 3-way drift — `site` `version/route.ts`, `download/page.tsx:9-10`; single-source it + deploy preflight.
- **C3** False "activates offline / no account" FAQ — `site` `app/echo/page.tsx:296`.
- **C4** macOS sold but undeliverable/unrunnable — `download/route.ts:32` (.exe-only) + `tauri.conf.json`/`build-macos.yml` (unsigned/un-notarized) + Metal-off. **v1: gate Mac out of checkout.**

**HIGH (before public launch):** H1 buy-button JSON dead-end · H2 license-delivery fragility + gift dead-end · H3 5-machine cap TOCTOU race · H4 pack-download no-integrity RCE (`alan-echo` `packs.rs`/`main.rs`) · H5 no purchase analytics · H6 updater no-pinned-key (`alan-echo` `updater.rs`) · H7 Windows installer not CI-built (`alan-echo` `prepare-resources.ps1`) · H8 revoked key works ~407 days.

## Execution order (see improvement-plan.md for batches)
1. **Batch 0** — unblock + single-source download/version/hash; **verify live anonymously**. (`stock-analyzer`)
2. **Batch 1+2** — fix the false FAQ + buy-button errors + Mac copy; **gate Mac out of sale**. → soft-launch Windows.
3. **Batch 3** — delivery email/visible key, purchase analytics, atomic cap, deactivation.
4. **Batch 4** — pack integrity, artifact signing, Windows release CI. (`alan-echo`)
5. **Batch 5** — funnel perf/friction. Then the **Mac launch track**.

## Must verify in the environment (not derivable from code)
- Vercel env: `ECHO_DOWNLOAD_URL`, `ECHO_RELEASE_TAG`, `ECHO_INSTALLER_SHA256`, `NEXT_PUBLIC_ECHO_INSTALLER_SHA256`/`_VERSION`, `ECHO_STRIPE_PRODUCT_ID`, `ECHO_ACTIVATION_SIGNING_KEY`, `UPSTASH_*`/`REDIS_*`.
- One **real end-to-end test purchase** (test-mode or refunded): checkout → webhook → key delivery → download → activate → update.
- **Confirm EULA v2 counsel sign-off** (code says "PUBLISHED"; memory says hold) and the **copyright filing deadline 2026-09-10**.

## Acceptance per change
Failing test/repro first where applicable → minimal fix → for `alan-echo`, `cargo test` green on both targets + dual-platform note → for `stock-analyzer`, re-verify the live anonymous flow → no regression to the resident app's data. Verify before claiming done; if a fix needs a redeploy/reinstall to take effect, say so.
