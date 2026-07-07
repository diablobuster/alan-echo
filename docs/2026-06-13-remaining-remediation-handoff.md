# HANDOFF — Finish the 3 Remaining ALAN Echo Remediation Items

**Date:** 2026-06-13 · **For:** the next session (any capable model)
**Goal in one line:** execute the three remaining legal-audit remediation items — **(A) EULA v2 draft for counsel, (B) accessibility contrast fix, (C) Managed Payments on the ALAN Intelligence subscription checkout** — as reviewable PRs, following the existing discipline, and verify.

> This builds on the completed 2026-06 legal audit. Read PART 0 first; don't re-derive what's already done.

---

## PART 0 — Read first (context that's already established)

1. `C:\Users\arowm\alan_rd\legal-audit-2026-06\audit-report.md` — the master audit (116 findings, 6 CRITICAL). Also at `C:\Users\arowm\OneDrive\Desktop\ALAN Intelligence\ALAN_RD\legal-audit-2026-06\`.
2. `…\counsel-memo.md` — **§3 is the full spec for Task A** (every EULA v2 clause to draft).
3. `…\remediation-log.md` — the edit plan (Tier 1–4).
4. `alan-echo/docs/2026-06-13-legal-audit-session-log.md` — what already shipped this session.

### What's already DONE (do not redo)
- **MERGED to production** (repo `diablobuster/ALAN_post_integration`, local `C:\Users\arowm\stock-analyzer`, branch `main`):
  - **#734** — removed a fabricated testimonial + scoped false "nothing leaves the device" claims. Verified live.
  - **#735** — **Stripe Managed Payments (merchant of record)** enabled on the **Echo** checkout (`app/api/echo/checkout/route.ts`). This is the template for Task C. Verified: build passed, deploy Ready, route returns 307→/signup.
- **MERGED** (repo `diablobuster/alan-echo`, local `C:\Users\arowm\alan-echo`, branch `main`): **#2** — EULA clickwrap-assent line + transcript "verify before relying" notice.
- **Open DRAFT (counsel-gated, do not merge):** **#736** — privacy DNT + "pseudonymous" relabel.

### The one OWNER action gating Task C
The owner must do a **test-mode Echo purchase** to confirm Managed Payments works end-to-end (Stripe page shows "Sold through Link", test card `4242 4242 4242 4242` completes, **a license key is issued**). **Do NOT start Task C until the owner confirms that test passed** — otherwise a bad MoR pattern breaks both checkouts.

### Repos / slugs / discipline (applies to all tasks)
- **Website:** `diablobuster/ALAN_post_integration` · local `C:\Users\arowm\stock-analyzer` · deploy branch **`main`**. `gh pr create` **MUST** use `--base main` (default is `staging`). Editing any **legal page** (`app/privacy`, `app/terms`, `app/legal/*`, `app/refund-policy`) requires also touching `lib/settings/disclosures.ts` in the same diff (counsel-review CI gate). Run the `vercel-pre-deploy-check` skill before merge.
- **App:** `diablobuster/alan-echo` · local `C:\Users\arowm\alan-echo` · branch off `main`. App does NOT auto-deploy — changes ship on a manual release build. Never GUI-test against the user's resident Echo.
- **Build caveat:** the website build is near Vercel's memory ceiling — deploys occasionally die on a **transient OOM (SIGKILL after "Compiled")**. If a deploy errors, check `npx vercel inspect <url> --logs`; if it's SIGKILL, just re-trigger (push again or redeploy) — it usually succeeds on retry.
- **Never auto-merge counsel-gated legal text.** Open as `--draft`.
- **Commit messages:** use a temp file + `git commit -F <file>` (PowerShell here-strings don't bind cleanly to native `git -m`). End with `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.

---

## TASK A — Draft EULA v2 into PR #731 (HOLD FOR COUNSEL)

**What it is:** the app's End-User License Agreement needs ~11 substantive changes. A lawyer writes the final wording; your job is to draft a clean first version *for counsel to edit*, in the existing hold-for-counsel PR. **Nothing here ships live until counsel approves and the version is bumped.**

**The PR:** `#731` in `diablobuster/ALAN_post_integration` — title "HOLD FOR COUNSEL — EULA v2", head branch `legal/echo-eula-v2-HOLD`, base `legal/echo-consent-and-claims`. **It needs rebasing onto `main`** first (its base branch is already merged). Check its current contents before adding — some clauses may already be drafted.

**The clauses to draft — full spec is in `counsel-memo.md §3` (Q3.1–Q3.11):**
- Q3.1 liability-cap carve-outs (gross negligence / willful / fraud / personal injury) + failure-of-essential-purpose independence clause.
- Q3.2 consumer non-derogation / **savings clause** (EU Software Directive, EU/UK consumer law, UK CRA 2015, Australian Consumer Law, Quebec) — drafted to survive either Texas or Colorado governing law.
- Q3.3 narrow the reverse-engineering clause to preserve EU Software Directive Arts. 5–6.
- Q3.4 **assignment clause** (licensor may assign; licensee may not) for the LLC transition.
- Q3.5 **machine-count fix** — EULA §2 currently says "any number of machines" but the server caps at **5**. (This is also live-false on the website `echo-license` page; counsel decides narrow-to-5 vs raise-the-cap.)
- Q3.6 output-accuracy / not-for-high-stakes disclaimer (note Magnuson-Moss trap: don't give a written warranty).
- Q3.7 export / sanctioned-party representation + accurate crypto description (EULA §9 currently names only HMAC; the app also uses Ed25519 + SHA-256).
- Q3.8 recording-consent / acceptable-use clause + biometric representation ("no voiceprints / no speaker-ID / audio deleted").
- Q3.9 **"validated offline" fix** — EULA §4 says keys are "validated offline" but activation is **online-first** (this is also live-false on the website).
- Q3.10 confirm the EULA states the user owns their transcript output.
- Q3.11 jurisdiction-variant texts (a single global "AS IS" is an offence under Australian law).

**Files that hold the EULA (must stay in lockstep):**
- App: `alan-echo/src/legal/eula.md` (source) → regenerate `alan-echo/legal/EULA.txt` via `node alan-echo/scripts/gen-eula-txt.mjs` after any change. `alan-echo/src/legal/eulaVersion.js` holds `EULA_VERSION` (currently `2026-06-10`).
- Website: `ALAN_post_integration/app/legal/echo-license/page.tsx` (the same EULA, public copy) with an `EFFECTIVE_DATE` that **must match** `EULA_VERSION`.

**Discipline:** Bumping `EULA_VERSION` forces every user to re-accept on next launch — so **batch all EULA changes into ONE version bump**, and time it with the LLC-formation re-acceptance. In the draft PR, keep the version UNCHANGED and mark everything **DRAFT PENDING COUNSEL** (mirror the pattern in `lib/settings/disclosures.ts` review block). Do NOT touch governing-law / arbitration clauses — those are P0 owner/counsel *decisions*, not edits.

**Deliverable:** PR #731 (rebased to main) containing the drafted clause language, clearly labeled draft-for-counsel, with a checklist mapping each clause to its counsel-memo question.

---

## TASK B — Accessibility contrast fix (reviewable PR)

**What it is:** some faint gray text on the website fails WCAG AA contrast — an accessibility + serial-plaintiff-lawsuit risk (audit finding **accessibility F-7**; California Unruh Act = $4,000/violation is the real exposure). This is a **visual change**, so present it for the owner/designer to eyeball; do NOT silently restyle the whole site.

**Where the token lives** (`C:\Users\arowm\stock-analyzer\app\`):
- `globals.css:97` — `--text-faint: #8a9099;` (cool gray; comment claims "accessibility minimums" — verify, likely FAILS ~3.5:1 on white)
- `globals.css:213` — `--text-faint: #797368;` ("hints, captions" on warm paper; ~4.4:1 — marginal, just under 4.5)
- `globals.css:606` (dark theme) `rgba(255,255,255,0.6)` and `:612` (light theme) `rgba(0,0,0,0.55)` — check both.
- `print.css:96` — `#9c968a` (2.70:1 — this is the value the audit cited, but it's **print-only** → low legal priority).
- `theme.css:1001` `#fde047` — a special high-contrast/yellow theme; **leave it**.

**What to do:**
1. For each screen token, compute the actual contrast ratio against the background it's used on (don't assume white — the marketing pages use warm paper `~#f7f5f0`; the app uses card backgrounds).
2. Darken each failing token to **≥4.5:1 for normal text** (WCAG 1.4.3) and ensure any UI-component/border/focus use hits **≥3:1** (1.4.11). Alternatively, restrict `--text-faint` to non-text decoration only.
3. Fix in the CSS source (NOT via an accessibility-overlay widget — overlays are themselves a litigation target).
4. Open ONE reviewable PR `--base main` with before/after contrast numbers in the body so the owner can approve the look. Don't merge without their sign-off (it changes appearance).

**Pair with (optional):** publish an honest accessibility statement (audit finding F-8) — but that wording is counsel-gated; just note it.

---

## TASK C — Managed Payments on the ALAN Intelligence subscription checkout

**GATE: only start after the owner confirms the Echo test purchase works** (see PART 0).

**What it is:** the owner enrolled all 4 products in Stripe Managed Payments, but only the **Echo** checkout was wired up (#735). The **ALAN Intelligence** tiers (Pro / Advisor / Intelligence) use a *different*, subscription-mode checkout that still needs the same change.

**The file:** `C:\Users\arowm\stock-analyzer\app\api\stripe\checkout\route.ts`. It has **two** `stripe.checkout.sessions.create({...})` calls (around lines **82** and **207**), both `mode: "subscription"`. Read the whole file first — it may have trial logic, customer creation, etc.

**The change (mirror #735's pattern — see `app/api/echo/checkout/route.ts` as the reference):**
1. Add `managed_payments: { enabled: true }` to each Checkout Session create call. Because the installed SDK is `stripe@^21` whose types may predate the param, add it via a cast: build the params as `Stripe.Checkout.SessionCreateParams`, then `stripe.checkout.sessions.create({ ...params, managed_payments: { enabled: true } } as Stripe.Checkout.SessionCreateParams)`. Add `import type Stripe from "stripe";` if not present.
2. **Remove the Managed-Payments-incompatible params** if present (Stripe's required-removal list): `automatic_tax`, `tax_id_collection`, `payment_method_types`, `payment_method_collection`, `payment_method_configuration`, `payment_method_options`, `saved_payment_method_options`, `customer_update[name]`, `customer_update[address]`, `shipping_address_collection`, `shipping_options`, and these `subscription_data` fields: `default_tax_rates`, `application_fee_percent`, `on_behalf_of`, `transfer_data`, `invoice_settings`. (The Echo route had `payment_method_types` + `automatic_tax` to remove — subscription routes often also have `automatic_tax` and `tax_id_collection`.)
3. The global Stripe `apiVersion` is already `2026-03-25.dahlia` (`lib/stripe.ts:16`), which satisfies the ≥`2025-03-31.basil` requirement — **no client change needed**.
4. Keep any ToS/consent params that are NOT on the removal list.

**Verify (before merge):** local `npm --prefix C:\Users\arowm\stock-analyzer run build` passes; then test a **subscription** checkout in Stripe test mode ("Sold through Link", tax by address, test card, and **the subscription + tier entitlement still provisions** via the webhook). Watch the Vercel deploy (retry on transient OOM). Open `--base main`; the owner merges after testing.

---

## PART 3 — Suggested order & a note on scope
1. **Task C** first *if* the owner has confirmed the Echo test purchase (highest value: finishes the tax fix across all products). Otherwise do A/B and wait.
2. **Task B** next (self-contained, quick once contrast is computed).
3. **Task A** last (largest; counsel-gated draft, no live impact).

Keep updating `remediation-log.md` and the session log. Save any new research to **both** `C:\Users\arowm\alan_rd\legal-audit-2026-06\` and the OneDrive ALAN_RD mirror.

## PART 4 — Quick reference
- Website: `C:\Users\arowm\stock-analyzer` → `diablobuster/ALAN_post_integration`, base `main`.
- App: `C:\Users\arowm\alan-echo` → `diablobuster/alan-echo`, base `main`.
- Echo checkout (DONE, reference): `app/api/echo/checkout/route.ts`. Intelligence checkout (TODO): `app/api/stripe/checkout/route.ts`.
- Stripe client: `lib/stripe.ts` (apiVersion `2026-03-25.dahlia`). Legal CI gate: `lib/settings/disclosures.ts`.
- EULA: app `src/legal/eula.md` + `legal/EULA.txt` (regen `scripts/gen-eula-txt.mjs`) + `src/legal/eulaVersion.js`; website `app/legal/echo-license/page.tsx`.
- Contrast tokens: `app/globals.css` (lines 97, 213, 606, 612), `app/print.css:96`.
- Open PRs: #731 (EULA v2, hold), #736 (privacy, draft). Merged: #734, #735, alan-echo #2.
- Deploy status: `cd C:\Users\arowm\stock-analyzer; npx vercel ls`. Build gate: `vercel-pre-deploy-check` skill.
- Owner to-dos still outstanding: Echo test purchase; rotate the exposed Stripe secret key; form the CO LLC; copyright filing by **2026-09-10**; trademark clearance (Alan AI Inc. §8 cliff 2026-10-20); see `user-action-list.md`.
