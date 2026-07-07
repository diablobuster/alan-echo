# ALAN Echo — Ship-Blocker Remediation Session Log

**Date:** 2026-06-20
**Scope:** Remediate the blockers from the 2026-06-20 whole-product audit (`docs/auditimprove/2026-06-20/`). Spans both repos: `stock-analyzer` (website/funnel) and `alan-echo` (program).
**Status:** All ship-blockers + key HIGHs fixed on branches and verified (tsc/eslint/cargo test). **Nothing is deployed yet** — see "Required before this takes effect."

---

## 1. What shipped (by batch)

### `stock-analyzer` — branch `fix/echo-ship-blockers` (5 commits)

**Batch 0 — download + version/hash (C1, C2):** `e8577621`
- The live funnel was non-functional: download 302'd to a private-repo asset that 404s for everyone.
- New `lib/echo/download.ts`: shared resolver that keeps the repo PRIVATE by minting a short-lived signed GitHub asset URL (token flow PRIMARY); explicit env URL is fallback-only. Platform-aware (.exe/.dmg). Single release source `ECHO_RELEASE_TAG`; version derived from it.
- `download/route.ts` + `download-free/route.ts` now both use the resolver (free trial stays open; both rate-limited).
- `version/route.ts` branches on `?platform`, returns per-platform `version`+`sha256`+`downloadUrl` (fixes trial dead updater button + mac exe-vs-dmg brick).
- `download/page.tsx` fetches the SHA/version from `/api/echo/version` so the displayed hash can't drift from what the updater verifies.
- `scripts/echo-release-preflight.ts`: asserts served-binary SHA == advertised SHA before an env flip.

**Batch 1 — false claims + checkout dead-end (C3, H1):** `31eba30a`
- Removed the false "no account / activates offline" FAQ (it re-published the counsel-killed "validated offline" claim); now describes one-time online activation + free account for keys + offline dictation.
- Softened the "your key will work on both platforms" Mac promise; dropped misleading "no account" from hero/metadata/privacy FAQ.
- `/api/echo/checkout` redirects to `/echo?checkout_error=...` on every failure instead of a raw JSON 500/429; `/echo` shows an accessible retry banner. Removed dead `CheckoutCta` import.

**Batch 2 — macOS checkout gate (C4):** `4ce200ce`
- `/api/echo/checkout` detects a macOS UA and routes to `/echo?mac_notice=1` (unless `?ack=mac`) — an informed "Windows-only today; continue if buying for Windows" notice. Does NOT block Mac-user-buying-for-Windows sales.

**Batch 3 — cap + delivery + analytics (H3, H2, H5):** `940e68c8`
- `activate/route.ts`: closed the 5-machine-cap TOCTOU race with a per-key `pg_advisory_xact_lock` inside a transaction; reactivates revoked rows; honest cap-error copy.
- `success/page.tsx`: renders the actual key with a Copy button (was login-link-only); fires a deduped `echo_purchase` event (the funnel previously tracked clicks but no purchases).
- New `success/PurchaseTracker.tsx`.

### `alan-echo` — branch `fix/echo-program-hardening` (1 commit)

**Batch 4 (verifiable parts) — pack integrity + correctness (H4 + 3 fixes):** `f7173a9`
- `packs.rs` (H4 RCE): added SHA-256 verification to the GPU/Vulkan pack download (the extracted binary is launched as the engine). Fail-closed when a hash is pinned; `PackKind::expected_sha256()` is `None` today (logs loudly) — **pin the 2 real hashes to fully enforce.**
- `updater.rs` `version_gt`: take the leading numeric run per segment so a `-beta`/`-hotfix` tag can't suppress a real update. +3 regression tests.
- `db.rs`: CSV-export formula-injection guard (prefix `=+-@` cells with `'`).
- `main.rs`: percent-encode the license key in the update download URL.

---

## 2. Verification status

- `stock-analyzer`: `npx tsc --noEmit` clean (exit 0) after every batch; `eslint` 0 errors (only pre-existing `<img>`/unused-symbol warnings).
- `alan-echo`: `cargo test` — **43 passed, 0 failed** (was 19); includes 3 new `version_gt` tests.
- **NOT verified (cannot, in-session):** live production behavior (env-dependent), a real end-to-end test purchase, macOS build (covered by existing CI gate), the GPU-pack download path (server routes still private-404 + hashes unpinned).

---

## 3. Required before this takes effect (USER actions — not code)

The live site is **still broken until deployed + env set.** After reviewing/merging `fix/echo-ship-blockers`:
1. **Vercel env (critical for the download fix):** set `GITHUB_RELEASES_TOKEN` (read access to the private releases repo) and `ECHO_RELEASE_TAG=v1.2.3`. Without the token, the resolver falls back to the broken direct URL. Point/clear `ECHO_DOWNLOAD_URL` (a stale private-asset URL should be unset or a real public/CDN link).
2. Ensure `ECHO_INSTALLER_SHA256` and `NEXT_PUBLIC_ECHO_INSTALLER_VERSION` match v1.2.3; run `npx tsx scripts/echo-release-preflight.ts` (asserts served SHA == advertised) before going live.
3. Deploy, then **verify with an anonymous client**: `/api/echo/download-free` returns the installer (not 404); `/api/echo/version` version == served binary; a real (refundable/test-mode) purchase → key shown on success → activate.
4. Confirm `UPSTASH_*`/`REDIS_*` set in prod (rate limiters degrade to per-instance otherwise).

---

## 4. Follow-ups / known limitations (non-ship-blockers — task #6)

- **H6** updater artifact signing (needs a generated ed25519 release keypair + CI signing secret).
- **H7** Windows release CI (needs a from-source whisper.cpp build + a CI run to validate).
- **Pin the 2 pack SHA-256 hashes** in `PackKind::expected_sha256` (then pack verification is fully enforced).
- Give `/api/echo/download/{gpu,vulkan}` the private-repo token resolver (they still 404 like the installer did).
- Auto-email gift recipients; self-service per-machine deactivation UI (the cap-error copy is honest in the meantime).
- Tracked legal (unchanged): copyright filing due **2026-09-10**; confirm EULA v2 counsel sign-off (the code comment asserts "PUBLISHED").

---

## 5. Guardrails honored
Dual-platform (all `alan-echo` changes are platform-agnostic Rust, no `cfg` paths); no `tauri dev` (resident app); worked on dedicated branches off current heads (parallel-session safety), staged only my files (left the parallel session's untracked docs + lock files alone); no legal text improvised (marketing-copy factual corrections only, flagged for review); no emojis.
