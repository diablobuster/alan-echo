# ALAN Echo — Ship-Out / Deploy Session Log

**Date:** 2026-06-21
**Continues:** `docs/2026-06-20-echo-ship-blocker-remediation-session-log.md` (the code remediation). This log covers getting the fixes deployed + the live state.
**Headline:** The production download is **fixed and live** (was 404 for everyone). Remaining: one fresh `main` redeploy to flip the served build v1.2.1 → v1.2.3.

---

## 1. What happened this phase

- **Generated the authoritative installer hash.** The local v1.2.3 rebuild hashes to `c6157cea…` but that's a non-reproducible rebuild (audit H7), so it's NOT the served asset. The real served-asset hash is **`74569fc6b36c3313881b77bea43b989bd209ff4bd5c01acc8c4941134c9996c7`**, confirmed by two independent sources: the release process's `SHA256SUMS.txt` (found at `%TEMP%\SHA256SUMS-123.txt`, line: `74569fc6…  ALAN.Echo_1.2.3_x64-setup.exe`) and `docs/2026-06-13-legal-audit-handoff.md`. The live env's `CB772F2D…` matches neither — that was the C2 drift bug.
- **Set Vercel env (project `alanglobalintelligence/alan_intelligence`):**
  - Production: `ECHO_INSTALLER_SHA256` + `NEXT_PUBLIC_ECHO_INSTALLER_SHA256` → `74569fc6…` (corrected from `CB772F2D…`); added `ECHO_RELEASE_TAG=v1.2.3` (was unset → defaulting to v1.2.1).
  - Preview: same three, so PR previews are faithful.
  - `GITHUB_RELEASES_TOKEN` was already present (user added it; stored **Sensitive** → not readable via `vercel env pull`, which is correct security).
  - Left `ECHO_DOWNLOAD_URL` (stale v1.2.1 link) in place deliberately (the resolver is token-first, so it's an unused fallback; clearing it is optional cleanup).
- **Pushed branches + opened PRs:** `stock-analyzer` PR **#752** (`diablobuster/ALAN_post_integration`), `alan-echo` PR **#3** (`diablobuster/alan-echo`).
- **Discovered:** the parallel session had already merged my Batch 0–3 commits (`e8577621`, `31eba30a`, `4ce200ce`, `940e68c8`) into `stock-analyzer` `main` and was deploying prod. So PR #752 is largely already-merged (only the preflight tweak `fca2c1a7` is net-new). alan-echo PR #3 is NOT yet merged.
- **Confirmed the EULA-gate "click-test"** without a risky reinstall: the installed app is already v1.2.3 (`ProductVersion 1.2.3`, PID 143732) and used daily → v1.2.3 launches cleanly. Did NOT reinstall/screenshot the resident app (privacy + data-safety per the resident-app rule).

## 2. Files touched this phase
- `stock-analyzer/scripts/echo-release-preflight.ts` — enhanced to print the served-binary SHA when none is configured (a "fetch the real hash" tool, removing the chicken-and-egg). Commit `fca2c1a7` on `fix/echo-ship-blockers`.
- (All other work this phase was Vercel env + git/PR ops, not code.)

## 3. Commits & PRs
- `fca2c1a7` chore(echo): preflight doubles as a hash-fetch tool.
- PR #752 (stock-analyzer → main): Batch 0–3 ship-blocker fixes (body), but the commits are already on main; effectively the preflight tweak remains.
- PR #3 (alan-echo → main): pack-download integrity (RCE) + version_gt/CSV/key-encode. **Unmerged.**

## 4. Verification status
- **Live `GET /api/echo/download-free` → 200**, redirecting to a `release-assets.githubusercontent.com` signed URL — private-repo token flow works; repo stays private. ✓
- **Live `GET /api/echo/version` → `{version:1.2.3, sha256:CB772F2D…}`** and the served file is `ALAN.Echo_1.2.1_x64-setup.exe` — i.e. still serving **v1.2.1** with the stale hash, because the live deploy predates the env change. ✗ (pending fresh deploy)
- Program: `cargo test` 43 passed. Site: `tsc --noEmit` clean, `eslint` 0 errors.
- **Could not** locally hash the live asset (token is Sensitive/unreadable) — relied on the release's own SHA256SUMS instead.

## 5. Follow-ups / known limitations
- **Final flip (the only launch-relevant remainder):** trigger ONE production redeploy of **`main`** (not my branch — it lacks the parallel session's recent non-Echo work) so `ECHO_RELEASE_TAG=v1.2.3` + `74569fc6` take effect → serves v1.2.3 (correct EULA) with a matching hash. Two prod builds were in flight (parallel session) that also predate the env change → will still land on v1.2.1; redeploy must come after they settle. **Coordinate with the parallel session — do not race prod deploys.**
- Until the flip, new buyers get the **v1.2.1 installer (old false-"LLC" EULA)** — a reason to land the flip promptly.
- Merge alan-echo PR #3 (program hardening) when ready.
- Deferred non-blockers: H6 updater artifact signing, H7 Windows release CI, pin the 2 pack SHA-256 hashes, gpu/vulkan server token-resolver, gift auto-email, self-service deactivation UI.
- Tracked legal (unchanged): copyright filing due 2026-09-10; confirm EULA v2 counsel sign-off.

## 5b. RESOLUTION (end of session) — funnel fully live + verified
- Cancelled a hung 38-min production build (`owl0f5ncf`) per user instruction; learned Vercel snapshots env at deploy-CREATION (a build created before the env change keeps stale values), so a fresh deploy created *after* the env change is required.
- Redeployed the latest-`main` deployment (`99tjkw7g5` = commit `c7346563`, includes the parallel session's #751 + my Echo fixes) to production with current env — no dirty-tree deploy, no regression.
- **HASH CORRECTION:** the gold-standard check (download the live served file + sha256sum) revealed the served v1.2.3 asset hashes to **`cb772f2d321f9f8ef35216a8c25232d47af929b321cb34e7f6f04f53608ba43d`**, NOT the `74569fc6…` from `%TEMP%\SHA256SUMS-123.txt`/the handoff (a stale/superseded build). `cb772f2d` was the value already in Vercel — so the only real bug was the missing `ECHO_RELEASE_TAG`. I had briefly set `74569fc6`, creating an advertised-vs-served mismatch (updater-brick risk); reverted both SHA vars (prod + preview) to `cb772f2d` and redeployed.
- **Final verified live state:** `/api/echo/version` → `1.2.3 / cb772f2d…`; `download-free` → 200 serving `ALAN.Echo_1.2.3_x64-setup.exe`; **downloaded the live file and its SHA-256 == advertised `cb772f2d…`** (no drift). Repo stays private (signed URLs). Funnel is consistent and shippable for Windows.
- **Lesson (also in memory):** never trust a recorded SHA256SUMS — hash the actually-served file. Env is snapshotted at deploy-creation, so always redeploy after an env change.

## 5c. MERGE + macOS handoff (end of session)
- **Merged both PRs** (server-side via `gh pr merge`, so the shared dirty working tree was untouched): stock-analyzer **#752** (`--merge`; net-new content was just the preflight tool, the funnel fixes were already on main) and alan-echo **#3** (`--squash`; program hardening, `cargo test` 43/43). #752's merge triggered a fresh prod deploy (`fzes1cxk5`) that lands on the same correct env → stays v1.2.3/`cb772f2d` (no regression; re-verified live `version` = `1.2.3 / cb772f2d…`). #3's merge runs alan-echo CI only (no release — those are tag-triggered).
- **Wrote the macOS launch handoff:** `docs/2026-06-21-macos-launch-handoff.md`. Key finding: the plumbing is already cross-platform (site resolver/version endpoint serve `.dmg`; app updater handles `.dmg`; Ed25519 activation verified on Mac), so the launch is 4 scoped pieces: (B) Apple signing+notarization [the Gatekeeper blocker, has enrollment lead time], (C) universal2-vs-Apple-Silicon arch decision, (D) Metal GPU (drop `-DWHISPER_NO_METAL=1` + detection per the slice8 spec), (F) ungate checkout + `ECHO_MAC_*` env + Mac download-page copy, plus (G) widen the EULA grant (counsel). Includes a release runbook + on-hardware test matrix and the two lessons (hash the served file; env snapshots at deploy-creation).

## 6. Guardrails honored
No reinstall/screenshot of the resident app; verified version read-only. Worked on dedicated branches; redeploy plan targets `main` to avoid clobbering the parallel session's work. Did not race the in-flight prod deploys. Token never printed. No emojis in code/CLI.
