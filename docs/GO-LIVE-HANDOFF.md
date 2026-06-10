# ALAN Echo — Go-Live Handoff (next session)

> **Your mission:** take ALAN Echo from "code-complete on the dev machine" to "a stranger can pay (or redeem a free key) at alanglobalintelligence.com/echo, download an installer, and be dictating within five minutes — legally, reliably, and on a machine that has never seen a dev tool."
>
> The app itself is done and audited (see `.claude/debug-journal.md` and `whispr-local/docs/2026-06-10-alan-echo-ship-ready-session-log.md`). What's left is **packaging, distribution, commerce, delivery, legal, and the clean-machine proof**. Nothing below has been built yet.

---

## 0. Read these first
- `C:\Users\arowm\alan-echo` — the Tauri 2 app (Rust + React). Code-complete, `cargo test` 9/9, builds clean.
- `C:\Users\arowm\alan-echo\.claude\debug-journal.md` — every bug fixed and why.
- `C:\Users\arowm\stock-analyzer` — **this is the website** that serves alanglobalintelligence.com. Next.js 16 App Router on Vercel. Already has: Stripe (`lib/stripe.ts`, tiers FREE/PRO/ADVISOR/INTELLIGENCE/ENTERPRISE), NextAuth v4, Prisma + Postgres (Supabase), Resend email (`lib/email/`, from `noreply@alanglobalintelligence.com`), design tokens (`lib/brand/colors.ts`, `app/globals.css`, `app/theme.css`), legal pages (`/terms`, `/privacy`, `/refund-policy`, `/legal/disclosures`). **There is no `/echo` route yet — blank slate.**
- The app already links to `https://alanglobalintelligence.com/echo` from `LicenseGate.jsx` (the "Buy ALAN Echo" link) — that page must exist by launch.

---

## 1. DECISIONS TO MAKE BEFORE BUILDING (surface these to the human, don't guess)

These are business/architecture forks that change everything downstream. Get answers first.

1. **Pricing & model.** One-time purchase or subscription? Price point? (Echo is a local desktop app with no server costs → one-time is natural, e.g. $39–$79. Subscription is harder to justify for offline software.) This decides the Stripe product type (one-time `mode: payment` vs `mode: subscription`).

2. **Merchant of Record vs. direct Stripe.** Selling downloadable software **worldwide** creates VAT/sales-tax obligations in dozens of jurisdictions. Two paths:
   - **Direct Stripe + Stripe Tax** — you keep more margin but you are the merchant and are liable for registering/remitting tax. The site already uses Stripe.
   - **Merchant of Record (Paddle, Lemon Squeezy, FastSpring)** — they become the seller of record, handle all global tax/VAT, fraud, chargebacks, and can host checkout + license delivery + the download. Higher fee (~5%+) but removes the entire tax/legal burden and often includes license-key issuance and email delivery out of the box.
   - **Recommendation to put to the human:** for a first paid desktop product, a Merchant of Record (Lemon Squeezy or Paddle) is dramatically less work and risk — it can replace most of sections 3, 4, 5, and 7 below. If they insist on direct Stripe, build sections 3–7 in `stock-analyzer`.

3. **License binding policy.** Keys are currently **machine-bound on first activation** and **offline** (no server, can't be revoked or reset remotely). That means: a buyer who changes laptops, reinstalls Windows, or swaps a motherboard **cannot reactivate** — a guaranteed support ticket. Choose one:
   - (a) **Drop hardware binding** — validate key format/HMAC only. Simplest, zero support burden, but a key can be shared. *Recommended for v1.*
   - (b) **Allow N machines** (e.g. 3) — needs a server to count activations → defeats "offline."
   - (c) **Online activation server** — most control, most work.
   The code supports (a) trivially (the binding check already returns `true` when not yet bound; just stop persisting/enforcing the bind). This is a one-line-ish change in `license.rs` + `main.rs`.

4. **Model/binary delivery.** This is the **biggest technical blocker** (see §2). The whisper models are 1.5 GB (medium) / 3.1 GB (large) and the CUDA DLLs are ~700 MB+ (`cublasLt64_12.dll` alone is 473 MB). You **cannot** ship all that in a normal installer. Pick:
   - (a) **Bundle a small quantized CPU model** (`ggml-base.bin` ~150 MB or quantized small ~180 MB) so the installer is ~200–300 MB and works offline immediately on any machine; offer GPU acceleration + larger models as an **in-app optional download**. *Recommended.*
   - (b) **Stub installer that downloads** the model + binaries on first run from your CDN. Smaller download, but first-run needs internet and a download-progress UI that doesn't exist yet.
   Either way you must decide what hardware the *default* experience targets. Most buyers won't have an RTX 4060.

5. **Code-signing certificate.** Unsigned `.exe` triggers SmartScreen "Windows protected your PC" — a conversion killer and a trust problem for a paid product. Decide: buy an **OV** cert (cheaper, ~$200–400/yr, reputation builds over weeks) or **EV** cert (instant SmartScreen reputation, ~$300–700/yr, may need a hardware token / cloud HSM like Azure Trusted Signing). This is a purchasing lead-time item — start it early.

---

## 2. PACKAGING & DISTRIBUTION (the app side)

**Acceptance:** a signed installer exists, hosted at a stable URL, that installs and runs on a clean Windows machine with no dev tools and no pre-existing `%APPDATA%\ALAN Echo`.

- [ ] **Solve model/binary bundling per Decision 4.** Today the engine reads binaries/models from `%APPDATA%\ALAN Echo\models\...`, which only exists because this dev machine populated it manually. `tauri.conf.json` `bundle.resources` is empty. You must either bundle chosen binaries+model as Tauri `resources` (and update `whisper.rs` search paths to include the install dir / resource dir) or build the first-run downloader. **The current `npx tauri build` produces an installer that will NOT work on any other machine.** Verify the resource-dir path resolution in `whisper.rs::find_server_binary` / `resolve_model` against where Tauri actually unpacks resources.
- [ ] **Bundle the right whisper build for the target.** CPU build is in `models\Release\`, CUDA in `models\cuda_release\Release\`. For a general-audience installer, bundle the **CPU build + its DLLs** (and gate GPU on optional download). Confirm the CPU `whisper-server.exe` + `ggml-*.dll` + `SDL2.dll` set is complete and runs without CUDA DLLs present.
- [ ] **`npx tauri build`** → NSIS installer at `src-tauri/target/release/bundle/nsis/`. Installer is already `installMode: currentUser` (no admin/UAC — good).
- [ ] **Code-sign** the `.exe` and the installer (Decision 5). Set `tauri.conf.json` → `bundle.windows.certificateThumbprint` (or use `signCommand` for cloud signing). Verify the signature with `signtool verify /pa`.
- [ ] **Host the installer** at a stable, versioned URL. Options: Vercel Blob, Cloudflare R2, AWS S3, or GitHub Releases. Must support large files and resumable download. Record the URL — the website/email link to it.
- [ ] **Auto-update (optional v1, recommended soon):** wire Tauri Updater (`tauri-plugin-updater`) + a signed `latest.json` manifest so you can push fixes without users re-downloading manually. Skipping for v1 is acceptable; if so, document the manual-update story.
- [ ] **Versioning:** bump `tauri.conf.json` / `Cargo.toml` / `package.json` to a real release version and tag the git commit.

---

## 3. LICENSING INFRASTRUCTURE (server-side keygen + delivery)

The HMAC algorithm is **proven identical** between Rust and Python (unit-tested vectors). You'll reimplement it once in **Node/TypeScript** so the website can mint keys. Exact spec:

```
SECRET  = "ALAN_ECHO_v1_GLOBAL_INTELLIGENCE_2026"   (utf-8 bytes)
CHARSET = "ABCDEFGHJKMNPQRSTUVWXYZ23456789"          (31 chars)
payload = "XXXXX-XXXXX-XXXXX"  (3 groups of 5 random CHARSET chars)
check5  = for the first 5 bytes b of HMAC-SHA256(SECRET, payload): CHARSET[b % 31]
key     = "ECHO-" + payload + "-" + check5     →  ECHO-XXXXX-XXXXX-XXXXX-XXXXX
```
Verified test vector: `compute_check("AAAAA-BBBBB-CCCCC") === "FEMJB"`. **Write a unit test asserting that vector** in the website repo so the JS keygen can never silently drift from the Rust validator.

- [ ] **Node keygen module** in `stock-analyzer` (`lib/echo/license.ts`) producing valid keys, with the parity test above.
- [ ] **Persist issued keys** in Prisma (new model: key, email, stripe session id, issued-at, source = `purchase|giveaway`, redeemed-at). Even with offline validation you want a record for support ("what's my key", refunds, abuse).
- [ ] **If you keep machine binding (Decision 3b/c):** build the activation endpoint. **If you drop it (3a, recommended):** also remove `license_binding` enforcement from the app so reinstalls/hardware changes don't lock users out. Decide and make app + server consistent.

---

## 4. WEBSITE: /echo SALES PAGE + CHECKOUT (in stock-analyzer)

**Acceptance:** a visitor can read what Echo is, click buy, pay, and immediately receive a key + download link.

- [ ] **`/echo` landing/sales page** matching the ALAN editorial design system (use `lib/brand/colors.ts`, the `_primitives` components, Instrument Serif). Copy: the privacy story ("100% local, nothing leaves your device" — **this is the strongest selling point**, and it's now literally true since Google Fonts was removed), speed (sub-second after warm-up on GPU; works on CPU too), auto-paste-anywhere, hotkey workflow. Screenshots/GIF of the dashboard + a dictation demo. System requirements (Windows 10/11; optional NVIDIA GPU for max speed).
- [ ] **Stripe product/price** for Echo (Decision 1). Add `STRIPE_ECHO_PRICE_ID` env var; map it in `lib/stripe.ts`. Use `mode: payment` for one-time.
- [ ] **Checkout** — Stripe Checkout Session (collect email; enable Stripe Tax if going direct per Decision 2). A "Buy" CTA on `/echo` → `/api/echo/checkout`.
- [ ] **Pricing/legal copy** on the page: price, what you get, refund terms link, EULA link.

> If you chose a **Merchant of Record** (Decision 2), most of §4/§5/§7 collapses into "configure the product in Lemon Squeezy/Paddle, paste their checkout/buy-button on `/echo`, let them issue & email the key and host the download." Build the custom Stripe pipeline only if going direct.

---

## 5. DELIVERY PIPELINE (purchase → key → download → email)

**Acceptance:** completing checkout results in (a) a key on screen, (b) a receipt email containing the key + download link + install steps, within seconds, reliably.

- [ ] **Stripe webhook** (`/api/echo/webhook`, handle `checkout.session.completed`): generate key (§3), store it, send email. Verify the Stripe signature; make it idempotent (Stripe retries).
- [ ] **Receipt/license email** via Resend (reuse `lib/email/`): key, download URL, step-by-step install + activation instructions, support email, link to help page. This email **is** the product delivery — it must be reliable and not land in spam (check SPF/DKIM/DMARC on the domain).
- [ ] **Success page** (`/echo/success`) showing the key + download button (don't rely on email alone).
- [ ] **"Resend my key" / lookup page** (`/echo/recover`) — enter purchase email → re-send the key. Reduces support load massively.
- [ ] **Download endpoint or direct link** to the hosted installer (§2). If gated, gate lightly.

---

## 6. FREE GIVEAWAY MECHANISM

**Goal:** hand out a limited number of free copies through the *same* trusted pipeline so free users get a real key + real delivery, and you can track them.

- **Recommended approach — 100%-off Stripe coupon / promo code:** create a Stripe coupon (e.g. `ECHO-FRIENDS`, 100% off, max redemptions = N, optional expiry). The buyer "checks out" for $0, the **same `checkout.session.completed` webhook fires**, and they get a key + download exactly like a paying customer. Zero new code, full tracking, identical UX. Mark `source = giveaway` from the coupon metadata.
- **Alternative — pre-minted batch + redemption page:** generate N keys offline (a small CLI using the §3 keygen), store them as `source = giveaway, redeemed = false`, and build `/echo/redeem` where a person enters one to get the download. More moving parts; only do this if you want physical/printed codes or distribution outside checkout.
- [ ] Decide count and audience (beta testers, influencers per the launch plan, press).
- [ ] If keys are machine-bound, free reviewers on multiple machines will hit the binding wall — another reason to prefer Decision 3a.

---

## 7. LEGAL & BUSINESS

Don't ship a paid product without these. (A Merchant of Record covers the tax/seller pieces but **not** your EULA/privacy.)

- [ ] **EULA / Software License Agreement** — it's licensed software, not a service. Cover: license grant (personal/commercial?), restrictions (no resale/reverse-engineering), no-warranty/limitation-of-liability, termination, governing law. Add `/echo/eula` or `/legal/echo-license`. *(The `legalzoom:review-contract` skill can review a draft.)*
- [ ] **Privacy policy** — extend the existing `/privacy`. Two distinct surfaces: **the app** (100% local; explicitly state it does NOT transmit audio/transcripts; note it runs `nvidia-smi` locally for GPU detection and bundles fonts locally — no network calls) and **the website/checkout** (collects email + payment via Stripe; what's stored). The "nothing leaves your device" marketing claim must exactly match the privacy policy and the actual behavior — verify no telemetry was ever added.
- [ ] **Refund policy** — extend `/refund-policy` for a downloadable one-time product (digital goods; state your stance, e.g. 14-day refund if key unused, or no-refund-after-download — your call, but be explicit and lawful for the jurisdictions you sell to; EU has digital-goods withdrawal rules).
- [ ] **Terms of sale** — price, delivery method (instant digital), what's included, support scope.
- [ ] **Tax/VAT** — resolved by Decision 2 (Stripe Tax registration vs MoR). Do not skip; selling into the EU/UK without VAT handling is a real liability.
- [ ] **Business entity & banking** — confirm the selling entity, Stripe account ownership, and that payouts land correctly. Trademark/brand: confirm "ALAN" usage is cleared.
- [ ] **Export/encryption note** — the app uses HMAC/SHA-256 (standard crypto); generally exempt but worth a one-line record.

---

## 8. THE CLEAN-MACHINE END-TO-END TEST (the single most important gate)

**Everything to date was verified on the dev machine, where models, binaries, GPU drivers, and `%APPDATA%` were already set up.** None of that proves a customer's experience. Before launch, on a **clean Windows VM or a second PC with no dev tools, no CUDA, no pre-existing `%APPDATA%\ALAN Echo`**, walk the entire chain and fix whatever breaks:

1. Land on `/echo` → buy (use a Stripe test card, then one real/coupon run) → receive key + download email.
2. Download the installer → observe the SmartScreen behavior (this is why §2 signing matters) → install (should be no-admin).
3. First launch → onboarding wizard → mic selection + **real test dictation** (this exercises the bundled CPU model with no GPU present — the make-or-break moment for Decision 4).
4. Enter the license key → activation succeeds.
5. Global hotkey from a third-party app (Notepad, browser, Word) → speak → text is **auto-pasted** into that app.
6. Close to tray, quit from tray (confirm no orphaned `whisper-server.exe` left in Task Manager), relaunch.
7. Test the giveaway path the same way (100%-off coupon).
8. Test "resend my key" and a refund.

Record results. **This test will surface the real go-live bugs** — bundling path resolution, missing DLLs, CPU-only performance, SmartScreen, email deliverability.

---

## 9. SUPPORT & OPS (lightweight, but have it)

- [ ] **Support email** (e.g. `echo@alanglobalintelligence.com` or reuse support) monitored.
- [ ] **Help/FAQ page** — install steps, "key won't activate," "how to change mic," "is it really private," "GPU vs CPU speed," system requirements, "I got a new computer" (answer depends on Decision 3).
- [ ] **Changelog** page for future updates (the app already brands "v1.0").
- [ ] **Funnel analytics** on `/echo` (the site has analytics patterns already) — but **keep the app telemetry-free** to protect the privacy claim. If you ever want crash reporting, make it explicit opt-in and disclose it.

---

## 10. SUGGESTED ORDER OF OPERATIONS

1. Get the four+ **decisions in §1** answered by the human (especially MoR-vs-Stripe and model-bundling — they gate the most work).
2. **§2 model bundling + a working installer on a clean machine** (prove the app installs and dictates before building any commerce around it). Start the **code-signing cert purchase** in parallel — it has lead time.
3. **§3/§4/§5 commerce + delivery** (or MoR configuration).
4. **§7 legal** in parallel with commerce.
5. **§6 giveaway** (trivial once §5 exists).
6. **§8 full clean-machine E2E**, fix everything it finds.
7. Then hand to launch marketing (see `launch-strategy`: the ORB framework, Product Hunt prep, the privacy angle is the hook).

---

## Known landmines (from the build session — save yourself the debugging)
- The current `tauri build` installer **does not bundle binaries/models** → runs only on machines where `%APPDATA%\ALAN Echo\models` was hand-populated. This is §2's whole point.
- whisper models + CUDA DLLs are multi-GB → cannot live in a normal installer (Decision 4).
- Unsigned exe → SmartScreen scare screen (Decision 5).
- Offline machine-bound keys → no remote reset; hardware changes lock users out (Decision 3).
- "Standard" model option errors on machines lacking `ggml-small.bin`; the app correctly surfaces it, but for retail either bundle that file or hide the option.
- Privacy marketing claim must stay literally true — no telemetry, no network calls from the app. Audit before you advertise it.
- Email delivery is the actual product delivery — verify SPF/DKIM/DMARC so license emails don't go to spam.
