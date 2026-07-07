# ALAN Echo — Whole-Product Ship-Readiness Audit

**Date:** 2026-06-20
**Scope:** The ALAN Echo *website + buying funnel* (`stock-analyzer`), the *program* (`alan-echo` Tauri app), and the *cross-repo buy-flow seam* between them.
**Method:** 12 deep auditors across Business/Funnel, Backend/IT, Security, and Optimization perspectives; every CRITICAL/HIGH finding handed to an independent skeptic prompted to refute. 36 agents total; **0 findings refuted, 0 abstentions, 0 failed auditors** (clean verify pass — no rate-limit artifact). Key facts re-verified by hand against live production on 2026-06-20.

---

## EXECUTIVE SUMMARY

### VERDICT: ❌ NO-GO for public launch as-is.

**The funnel is non-functional in production right now: nobody can download Echo.** `https://www.alanglobalintelligence.com/api/echo/download-free` 302-redirects to a **private-repo** GitHub asset that returns **HTTP 404** for every anonymous visitor (verified live 2026-06-20). The paid (key-gated) download short-circuits to the same dead URL. This is a hard stop independent of everything else.

That said, **this is a "fix a focused blocker list, then ship Windows" situation — not a rebuild.** The foundations are genuinely strong: the Windows program is stabilized and tested (6 of 9 prior ship-blockers fixed with regression tests since 2026-06-17), the activation crypto/payment/security perimeter is solid, pricing is correctly single-sourced, and the end-to-end activation contract (key format → activate → Ed25519 token → the app's embedded public key) was **verified to match byte-for-byte**.

There are **5 CRITICAL** and **13 HIGH** verified issues. Two themes dominate: (a) the **release/download/version-integrity plumbing** on the website is broken and self-inconsistent, and (b) **macOS is sold but not deliverable or runnable** while the Windows build is nearly ready.

### Top 5 risks (ship-blockers)
1. **Download is dead in production** — private-repo 404 for free *and* paid users (LIVE-confirmed). `app/api/echo/download-free`, `download/route.ts`.
2. **Three disagreeing version/hash sources of truth** — version API says 1.2.3/`CB772F2D`, served binary is 1.2.1, page shows a third SHA. Breaks the checksum-trust story (the entire mitigation for unsigned binaries) and arms the in-app updater to hard-fail-and-delete on the next release. `version/route.ts`, `download/page.tsx`.
3. **macOS is sold but cannot be delivered or launched** — gated download is `.exe`-only (Mac buyers get no file), the shipped `.app` is unsigned + un-notarized (Gatekeeper hard-block), and Mac is CPU-only while marketed GPU-fast. `download/route.ts:32`, `tauri.conf.json`, `build-macos.yml`.
4. **False claim on the buy page** — the landing FAQ says Echo "activates offline" with "no account / no sign-in"; both are false (online Ed25519 activation + account-gated key retrieval) and re-publish the exact "validated offline" wording counsel flagged. `app/echo/page.tsx:296`.
5. **Buy button dead-ends on a raw JSON error** on any Stripe hiccup or rate-limit trip — conversion loss at the highest-intent moment. `app/api/echo/checkout/route.ts`.

### Top 5 revenue / conversion wins (once unblocked)
1. **Re-enable license-key email** (or render the copyable key on the success page) — delivery is currently fragile and gift purchases strand the recipient.
2. **Add a purchase-conversion analytics event** — today the business can see "Buy" clicks but not purchases; it is blind on the half of the funnel that earns money.
3. **Make the marketing page statically/edge-cached** — `/echo` is force-dynamic per request (root layout `await headers()` + SessionProvider fetch), taxing TTFB/LCP on the paid-acquisition page and billing a function per visitor.
4. **Reduce checkout friction** — buying requires creating a full ALAN account first; reconsider for an ~$89 impulse desktop purchase (or make the value of the account explicit).
5. **Ship a real per-machine deactivation** — the 5-machine cap error tells users to "deactivate a machine from the website," but no such feature exists; capped customers dead-end into support.

---

## SHIP DECISION DETAIL

**Windows v1 can ship after a focused blocker burn-down** (the CRITICALs + top HIGHs below). The program itself is in good shape.

**macOS should NOT be sold in v1.** Treat Mac as a separate launch gated on: Apple Developer ID signing + `notarytool` + staple, the Metal build flag, `.dmg` download wiring + a platform-aware version endpoint, and an Apple-Silicon-only-vs-universal2 decision. Until then, gate Mac out of checkout and remove the "your key will work on both platforms" / Stripe "for Windows" copy contradictions.

---

## AUDIT PLAN (what drove this run)

| # | Auditor | Repo | Perspective |
|---|---------|------|-------------|
| 1 | claims-legal-truth | site | marketing claims vs program reality + legal |
| 2 | pricing-checkout | site | price integrity + Stripe session |
| 3 | payment-license-backend | site | webhook → key mint → delivery → revocation |
| 4 | download-integrity-version | site | download routes + version/hash serving |
| 5 | activation-security | site | activate/validate: crypto, cap, replay, rate-limit |
| 6 | funnel-ux-conversion | site | funnel path, key delivery, instrumentation |
| 7 | web-secrets-headers-admin | site | secrets, admin auth, headers, CSRF, rate-limit |
| 8 | perf-cost-web | site | LCP, caching, cron/API cost |
| 9 | program-blockers-refresh | app | verify 2026-06-17 fixes vs current code |
| 10 | program-macos-parity-signing | app | macOS signing/Metal/Intel/CI |
| 11 | program-release-ci-packaging | app | updater integrity, CI, reproducibility |
| 12 | e2e-buy-flow-contract | both | cross-repo key/activate/version contract |

---

## CRITICAL — fix before any public traffic

### C1 · Download is broken in production (private-repo 404) — `site`
`app/api/echo/download-free/route.ts`, `app/api/echo/download/route.ts:16-17`
`ECHO_DOWNLOAD_URL` is set to a **direct GitHub release-asset URL on a private repo** (`diablobuster/alan-echo-releases`). The gated download route short-circuits to it at line 16-17 *before* the GitHub-API token flow that would mint a working temporary S3 URL, so even valid keys get the dead link. **LIVE 2026-06-20:** `download-free` → 302 → `.../v1.2.1/ALAN.Echo_1.2.1_x64-setup.exe` → **404**; the repo root returns `Not Found` anonymously.
**Impact:** Total funnel failure — every free and paying customer gets a dead download today.
**Fix:** Make the releases repo public, **or** unset `ECHO_DOWNLOAD_URL` so `/api/echo/download` falls through to the `GITHUB_RELEASES_TOKEN` asset-token resolver (and give `download-free` an equivalent authenticated resolver). Verify with an **anonymous** client.

### C2 · Three disagreeing version/hash sources of truth — `site` / cross
`app/api/echo/version/route.ts`, `app/echo/download/page.tsx:9-10`, `app/api/echo/download-free/route.ts`
**LIVE:** version API → `1.2.3` / SHA `CB772F2D…`; download page displays `1.2.3` / SHA `6dbb09f5…` (different); served binary is `v1.2.1`. The page reads `NEXT_PUBLIC_ECHO_INSTALLER_SHA256` while the API/updater read `ECHO_INSTALLER_SHA256` — two independent vars, two values. Nothing validates that the file at `ECHO_DOWNLOAD_URL` matches either hash or the version label, and there is no test/CI guard.
**Impact:** A buyer who runs the on-page `Get-FileHash` gets a mismatch against *both* displayed hashes — destroying the integrity story that is the entire mitigation for shipping unsigned binaries, and training users to ignore mismatches. Arms the updater brick in C2-adjacent finding H4-rel.
**Fix:** Collapse to one source of truth (derive the displayed SHA from the same var the updater serves; drive version+tag+hash from one release manifest). Add a deploy preflight asserting `sha256(file@ECHO_DOWNLOAD_URL) == ECHO_INSTALLER_SHA256 == NEXT_PUBLIC_ECHO_INSTALLER_SHA256` before the env change ships. **Until fixed, do not bump the version env var.**

### C3 · False "activates offline / no account" on the buy page — `site` (legal)
`app/echo/page.tsx:296`
FAQ: *"Neither. Echo is a one-time purchase with no account and no sign-in… the key activates offline…"* — both false. Activation is online-first (`activation.rs` POSTs to `/api/echo/activate`, returns an Ed25519 JWT bound to a machine fingerprint); key retrieval is account-gated (`keys/page.tsx:26-28` redirects to `/login`). This re-publishes the exact "validated offline" wording counsel flagged for the EULA and **contradicts other copy on the same page** (line 130 lists "license activation" as a network call; line 290 says network is required).
**Impact:** Material misrepresentation on the highest-traffic commerce page, aimed at the privacy/air-gapped wedge — false-advertising / refund exposure.
**Fix:** Rewrite the FAQ to match the EULA and reality (one-time online activation + account to retrieve the key; *dictation* is what's fully offline). Copy edit, one entry.

### C4 · macOS is sold but not deliverable or runnable — `app` + `site` / cross
`app/api/echo/download/route.ts:32`, `tauri.conf.json:58-60`, `build-macos.yml:51-54`, `scripts/prepare-resources-macos.sh:36`
Three compounding defects make the Mac SKU non-fulfillable: (1) the gated download selects assets with `name.endsWith(".exe")` only — a Mac buyer with a valid key gets a Windows `.exe` or a 302 bounce; (2) the shipped `.app` is **unsigned and un-notarized** (`TAURI_SIGNING_PRIVATE_KEY:""` is the updater key, not an Apple identity; no `codesign`/`notarytool`/`stapler` step) → Gatekeeper hard-blocks an internet-downloaded quarantined app on Catalina+; (3) Mac is CPU-only (`-DWHISPER_NO_METAL=1`) while the product is marketed GPU-fast.
**Impact:** A Mac customer can pay ~$89 and obtain nothing runnable — total Mac-SKU failure plus refund/chargeback/reputation risk; violates the dual-platform parity mandate.
**Fix (v1):** Gate Mac **out of checkout** and present "Mac — coming soon." **Fix (Mac launch):** Developer ID sign + notarize + staple; drop `-DWHISPER_NO_METAL=1` + add Metal detection; add a `.dmg` branch to the download resolver + platform-aware `/api/echo/version`.

---

## HIGH — fix before public launch

### H1 · Buy button dead-ends on raw JSON error — `site`
`app/api/echo/checkout/route.ts:22-23,37,119,125` vs `CheckoutCta.tsx` / `DualCta.tsx`
The CTAs are plain full-page `<a href="/api/echo/checkout">` navigations, but the route returns JSON on every failure (rate-limit 429, pricing-misconfig 500, Stripe-failure 500). A buyer hitting a transient Stripe blip or 11 clicks/min lands on a bare `{"error":"…"}` page with no nav/retry. The sibling download route already documents the correct redirect-on-error pattern.
**Fix:** Redirect to `/echo?checkout_error=1` and render a friendly retry banner.

### H2 · License delivery is fragile; gift path strands the recipient — `site`
`lib/echo/issue.ts:264-276` (`deliverAndStamp`), `success/page.tsx`, `keys/page.tsx`, `checkout/route.ts:97-104`
License emails were deliberately disabled (`deliverAndStamp` only stamps `emailSentAt`, never sends). The key is shown on `/echo/success` for 48h but the prominent button routes to the login-gated `/echo/keys`; a buyer who closes the tab keeps no passive copy. **Gift purchases** mint the key under the recipient's email with no notification and no account → recipient gets nothing; buyer can't find it under their own email. (Verifiers split this 2 ways and downgraded the duplicate instances to MEDIUM, but the *combined* delivery-reliability gap is a real pre-launch HIGH.)
**Fix:** Re-enable the transactional license email on first issuance (templates still exist), or render the copyable key on the success page (`CopyButton` is imported but unused). Auto-email gift recipients, or remove the gift field.

### H3 · 5-machine cap has a TOCTOU race — `site` (security/legal)
`app/api/echo/activate/route.ts:90-117`
The cap is a non-transactional `count()` then `create()` with no DB-level constraint on distinct-machine count (the only unique is `(keyId, machineHash)`). N concurrent requests with distinct fabricated 64-hex hashes all read `count < 5` and all insert — exceeding the counsel-mandated hard cap. Each win mints a durable ~400-day token. (Rate limit bounds it to ~2× cap per burst, more if Redis is unconfigured.)
**Fix:** Make the cap atomic — `$transaction` + serializable re-check, or a conditional-increment counter column (`updateMany where activeMachines < 5`), or a DB trigger. Don't rely on the unique constraint or rate limiting.

### H4 · GPU/model pack downloads have no integrity check — RCE vector — `app` + `site`
`src-tauri/src/packs.rs:489-547`, `src-tauri/src/main.rs:617-677`, `app/api/echo/download/{gpu,vulkan}/route.ts`
Pack downloads (native CUDA/Vulkan `whisper-server` executables, extracted and launched as the engine) are validated by **size floor only** — no SHA-256 anywhere — while the installer path *does* SHA-verify. A compromised/poisoned origin or TLS-defeating MITM can deliver an arbitrary same-size binary the app extracts and runs. (Currently the pack URLs also 404 on the same private repo, so the feature is broken too.) **Still open from the prior audit.**
**Fix:** Publish + verify a SHA-256 per pack (reuse the `updater.rs:110-122` streaming-hash block) before extract; or disable the GPU pack feature for v1.

### H5 · No purchase/conversion analytics — blind on the revenue funnel — `site`
`app/echo/success/page.tsx`, `DualCta.tsx`, `CheckoutCta.tsx`
Only `echo_checkout_click` + `echo_download_free_click` exist. The success page (server component) fires no conversion event; no `echo_purchase`/goal anywhere. The owner can see Buy clicks but not purchases, click→pay rate, or CTA-location attribution. (Revenue is recoverable from Stripe/DB, so verifier set HIGH not CRITICAL — but the in-product funnel data is permanently lost each day.)
**Fix:** Fire a Plausible purchase goal (with revenue prop) on the success "issued" branch; track `?canceled=1` abandons; track the nav/mobile CTAs too.

### H6 · Updater trusts a server-supplied hash with no app-pinned signing key — `app` (security)
`src-tauri/src/updater.rs:104-123`
The updater compares the download to `resp.sha256` from the *same origin* that serves the file — zero protection if the origin/CDN/DNS/TLS is subverted; an attacker controlling the response ships a malicious installer + matching hash and the app auto-launches it (user privileges). No Authenticode cert either (`certificateThumbprint:null`). This is the team's tracked open CRITICAL ("installer-hash signing weakness"); rated HIGH for ship because it needs active origin compromise.
**Fix:** Sign release artifacts (ed25519-dalek + sha2 are already deps; pin a const pubkey, verify a detached signature before launch), or adopt `tauri-plugin-updater` (note the custom mac DMG path).

### H7 · Windows release installer is not reproducible / CI-built — `app`
`scripts/prepare-resources.ps1:19,26-39`
The shipped Windows payload (`whisper-server.exe`, DLLs) is `Copy-Item`'d from `%APPDATA%\ALAN Echo\models\Release` (a hand-populated dir on one dev box), and CRT DLLs are grabbed from "newest Visual Studio on disk." No Windows build workflow exists (the only `tauri build` in CI is macOS-arm64). The most supply-chain-sensitive artifact has no verifiable provenance and bus-factor-of-one.
**Fix:** Add a `windows-latest` release workflow that builds `whisper.cpp` from a pinned upstream rev, pins the CRT version, runs `tauri build`, and uploads artifacts + `SHA256SUMS`.

### H8 · Refunded/revoked key keeps working offline ~407 days — `site` (business/legal)
`lib/echo/activation.ts:50`, `src-tauri/src/activation.rs:71-77`
Revocation is enforced only at the online endpoints, but tokens carry `exp = now + 400 days` and the app validates offline with a 7-day grace and never re-checks the server once activated. `refund-policy/page.tsx:140` promises the key "is deactivated… when the refund is issued." (The EULA *does* disclose the offline lag, so verifier kept it HIGH, not a hard blocker.)
**Fix:** Shorten `exp` (14-30 days) + re-validate on launch via the existing self-heal path, or align the refund-policy copy to the real window.

### Other HIGH (cross-repo plumbing, fold into C2's fix)
- **H-rel-a · Updater hard-fail-and-delete on next version bump** (`updater.rs:104-123`): once version is bumped while `ECHO_DOWNLOAD_URL`/hash drift, the updater downloads, mismatches, deletes the installer, and surfaces "try the website" — which is itself broken (C1). Armed/latent.
- **H-rel-b · One version+sha for both platforms** (`version/route.ts`): the route ignores `?platform`; a single hash can match at most one of `.dmg`/`.exe`, so the Mac updater is structurally guaranteed to brick. Fix with per-platform version/sha/downloadUrl.

---

## MEDIUM — next sprint
- **Per-machine deactivation feature doesn't exist** though the cap error and EULA reference it; capped customers dead-end (`activate/route.ts:96`). Also: no code path ever sets `EchoActivation.revoked=true`.
- **Checkout-email vs account-email mismatch** hides paid keys on `/echo/keys` (`keys/page.tsx:30-41`); match by `userId` or link to `/echo/recover`.
- **`/echo` rendered dynamically per request** (root layout `await headers()` + SessionProvider fetch) — cost/LCP tax on the top-of-funnel page.
- **CSV transcript export formula-injection** unguarded (`db.rs:227-238`).
- **`version_gt` drops pre-release suffixes** (`updater.rs:162`) — a `-beta`/`-hotfix` tag on the differentiating segment silently suppresses a real (incl. security) update.
- **Error boundary leaks raw `error.message` and dead-ends** (`app/echo/error.tsx`).
- **Rate limiters are per-instance + fail-open** unless Redis is provisioned (`lib/rate-limit.ts`) — verify `UPSTASH_*`/`REDIS_*` in prod.
- **`/api/echo/download` has no rate limit** while making live GitHub API calls — DoS/quota-exhaustion vector (`download/route.ts`).
- **Intel Macs get no build** (aarch64-only CI) vs `minimumSystemVersion 10.15` implying Intel — decide universal2 or state Apple-Silicon-only (Mac-launch item).
- **macOS `osascript` on the dictation hot path** (`paste.rs:142-197`) — latency + permission fragility (Mac-launch item).
- **Installer hash has no single source of truth** (hand-synced across 4 places) — the structural root of C2.
- **Stale version defaults (1.2.1)** across `version`/`download`/`success` (`download/route.ts:22` etc.) — foot-gun if env unset on a deploy.

## LOW — paper cuts
- JSON-LD offer price hardcoded `89.00` (not derived from `ECHO_PRICE_CENTS`); meta copy "$89 once" literal — drift risk on a future price change.
- `/echo/success` renders the key to anyone with the Stripe session id (high-entropy, 48h window — acceptable, consider masking).
- Dead imports (`CheckoutCta`, `CopyButton`) and stale `issue.test.ts` assertions.
- Nav/mobile "Get Echo" CTAs untracked; license key interpolated unencoded into the update URL; transcribe phase uncancelable; system-requirements disk numbers inconsistent (450MB vs +440MB GPU pack); inline-product Stripe fallback when `ECHO_STRIPE_PRODUCT_ID` unset; success "pending" state re-retrieves Stripe every 15s uncapped.

---

## CLEAN AREAS (what passed — do not "fix")
- **Activation crypto contract works end-to-end.** HMAC-keyed keygen + server-only Ed25519 signing key (guarded against client import); the app's hardcoded `PUB_KEY` was derived from the documented private seed and **matches byte-for-byte** — a server-signed token verifies in the app. Key format, activate request/response, and machine-fingerprint definitions all agree across repos.
- **Payment backend security.** Webhook signature verified before trust, fail-closed; idempotent (up-front `WebhookEvent` insert + `@unique stripeSessionId`); refund/dispute revoke the license (and full-refund revokes activations); cron authenticated with constant-time compare; recover endpoint enumeration-safe with atomic resend budget.
- **Pricing single-sourced.** `ECHO_PRICE_CENTS` is the only price; shown == charged, verified end to end. Stripe Managed Payments = merchant-of-record (tax/disputes handled).
- **Web security perimeter.** Admin auth/authz server-side; no client-exposed secrets; strong CSP/HSTS/X-Frame-Options/Permissions-Policy; same-origin CSRF check on mutations (activate correctly exempt + key/rate-limit-gated). The web-secrets auditor found **0 CRITICAL/HIGH**.
- **Program (Windows) stabilization.** 6 of 9 prior ship-blockers fixed *with regression tests* (fingerprint memoization + all-UNKNOWN fallback, regex precompile, verbatim mode, find/replace, resample guard, `verify_token` forgery tests); a real CI gate (`cargo test` on windows+macos) now exists.

---

## ISSUE MATRIX (verified findings, post-skeptic severity)

| Perspective | CRIT | HIGH | MED | LOW |
|-------------|------|------|-----|-----|
| Business / Funnel / Revenue | 1 (C3) | 4 (H1,H2,H5,H8) | 3 | 3 |
| Backend / IT / Release | 3 (C1,C2,C4*) | 4 (H7,H-rel-a,H-rel-b, +Mac dl) | 6 | 4 |
| Cybersecurity | 1 (C4*) | 3 (H3,H4,H6) | 3 | 2 |
| Optimization | 0 | 0 | 2 | 3 |

\* C4 spans IT + security + business; counted once as CRITICAL.
**Totals:** 5 CRITICAL · 13 HIGH · ~14 MEDIUM · ~12 LOW (24 verified survivors + 32 MEDIUM/LOW).

---

## COVERAGE GAPS (never hidden)
- **No agent failures or abstentions** this run — the verify pass was clean (unlike the prior deep-research run; see `feedback_deep-research-rate-limit-artifact`).
- **Production env values are Vercel runtime state**, not in the repo. The download-404 and version/hash drift were re-confirmed by live HTTP on 2026-06-20, but `REDIS_*`/`UPSTASH_*`, `ECHO_STRIPE_PRODUCT_ID`, `ECHO_ACTIVATION_SIGNING_KEY`, and the exact served hash could not all be confirmed from code — **verify these in the Vercel dashboard.**
- **No live purchase was executed** (would charge a card) — the checkout→webhook→key path was audited by code + the verified contract, not by a real transaction. Recommend one real end-to-end test purchase (test-mode or refunded) before public launch.
- **macOS Gatekeeper behavior** was assessed from config + Apple's documented post-10.15 policy, not by running on a live Mac.
- The audit was scoped to the **Echo surface** of `stock-analyzer`, not the entire ALAN platform.
- Legal CRITICALs already tracked (copyright filing due **2026-09-10**; EULA v2 counsel sign-off — the code comment asserts "COUNSEL-APPROVED / PUBLISHED" while memory says hold-for-counsel: **confirm**) are noted, not re-filed.
