# Session Log — Comprehensive Legal Audit of ALAN Echo (2026-06-13)

**Lead model:** Opus 4.8 (1M), ultracode. **Mission:** maximally lawsuit-safe / IP-protected / globally-compliant ALAN Echo + commerce surface — research-first, then remediate. **Legal information, not advice.**
**Status:** Research + synthesis + hand-back COMPLETE. Remediation staged & gated on owner confirmation (nothing shipped — see §3/§5).

---

## 1. What shipped this session
- **17-domain deep-research workflow** (`wf_39b1d071-2e2`): 35 agents, 2.44M tokens, 830 tool calls, ~22 min. One research agent per legal domain (8–25 authoritative sources each) → per-domain adversarial skeptic → completeness/blind-spot critic. Produced **116 findings (6 CRITICAL / 54 HIGH / 28 MED / 28 NOTE)**.
- **Full audit package** under `C:\Users\arowm\alan_rd\legal-audit-2026-06\`: 17 `research/<domain>.md` files + `_blind-spot-sweep.md`; `audit-report.md` (master ledger + verdict); `counsel-memo.md` (lawyer-only questions, §412 deadline at top); `user-action-list.md` (sequenced owner actions); `remediation-log.md` (edit plan + deferrals); `_live-state-verification.md` and `_findings-ledger-raw.md` (evidence).
- **No code/legal-text changes** were made in either repo (alan-echo or ALAN_post_integration). Remediation is prepared and gated on owner confirmation because the safe edits are outward-facing (live site) and the top item (testimonials) turns on a fact only the owner knows.

## 2. Files touched (intent)
| File / artifact | Location | Intent | Committed |
|---|---|---|---|
| 17× `research/<domain>.md` + `_blind-spot-sweep.md` | `alan_rd\legal-audit-2026-06\research\` | Cited per-domain research (not version-controlled) | N/A |
| `audit-report.md`, `counsel-memo.md`, `user-action-list.md`, `remediation-log.md`, `_live-state-verification.md`, `_findings-ledger-raw.md` | `alan_rd\legal-audit-2026-06\` | Synthesis + hand-back | N/A |
| `2026-06-13-legal-audit-session-log.md` | `alan-echo\docs\` | This log | No (untracked) |

## 3. Commits / PRs (final state)
**Merged to production (ALAN_post_integration, base `main`):**
- **#734** `legal/audit-marketing-claims` (381e3341) — removed owner-confirmed-fabricated "Joe Anderson" testimonial + scoped two false "nothing leaves the device" absolutes (FTC-1/FTC-2). **MERGED + verified live** (Joe Anderson gone; "dictation is 100% on-device" present). "Becky Romano" (surname match) deliberately left for owner disclose-or-remove.
- **#735** `feat/echo-managed-payments` (289bccd9) — enabled **Stripe Managed Payments (merchant of record)** on the Echo `mode:payment` checkout: `managed_payments[enabled]=true`; removed `payment_method_types` + `automatic_tax`; kept `consent_collection`/`custom_text`. Solves CR-5 (seller-of-record tax) + chargeback risk going forward. **MERGED**; local `npm run build` passed; deploy verified **Ready**; checkout route returns clean **307→/signup** (healthy). One earlier deploy died on a **transient build OOM (SIGKILL)** — retry succeeded (build is near Vercel's memory ceiling).

**Merged (alan-echo, base `main`; ships on next release build):**
- **#2** `legal/audit-app-safe-fixes` (8664b16) — EulaGate clickwrap-assent line (Berman, eula F1) + DetailPanel point-of-use "verify before relying" transcript notice (product-liability F7 / ai-law F5). UI-only, **no EULA_VERSION bump**. **MERGED.**

**Open as DRAFT (counsel-gated, NOT merged):**
- **#736 (draft)** `legal/audit-privacy-accuracy` (31eae89c) — privacy §7 CalOPPA DNT disclosure (footprint verified: Plausible cookieless only, no GA/GTM/ad trackers, no homepage cookies) + §10 "anonymous"→"pseudonymous" relabel; `disclosures.ts` review-block note satisfies the counsel-review CI gate **without** bumping the re-accept version. Held for counsel sign-off on DNT wording.

**Tax/MoR (owner-driven this session):** owner enabled Stripe Managed Payments live; categorized ALAN Echo as **Downloadable Software – non-recreational – personal use** (the 3 ALAN Intelligence tiers stay SaaS-business), chose **Prebuilt checkout**. Confirmed via Stripe docs it covers US sales tax + EU VAT. Business is at a **loss** (~$130 rev vs ~$300 costs) → no income/SE tax; loss offsets other income. Wrote `tax-calendar.md` + `tax-guide-indepth.md` to ALAN_RD (both locations).

**NOT auto-executed (need human judgment):**
- **Contrast tokens** — the audit's `#9c968a` (2.70:1) is in `print.css`; the screen tokens (`#797368` ~4.4:1, `#8a9099`) are a site-wide visual/**designer** decision, not a mechanical fix. Documented, not changed.
- **EULA v2** — all clauses spec'd in `counsel-memo.md §3`; **counsel** drafting via PR #731 (needs rebase to main). Not drafted solo to avoid shipping unreviewed legal text.
- **ALAN Intelligence subscription MoR** — **HELD** pending the owner's Echo test-purchase confirmation (don't replicate the MoR pattern across both checkouts unverified). Subscription checkout = `app/api/stripe/checkout/route.ts`.

Full audit + tax docs copied to the canonical **ALAN_RD library** (`C:\Users\arowm\OneDrive\Desktop\ALAN Intelligence\ALAN_RD\legal-audit-2026-06\`).

## 4. Verification status
**Live-verified (vs `origin/main` + production):** entity-name fix IS merged + live (sole-prop; corrects the handoff's stale "still LLC" worry); two false EULA claims ("any number of machines", "validated offline") confirmed live in app EULA + website; governing law = Texas vs CO domicile; `/privacy` missing DNT; privacy Echo data-flow disclosures accurate; releases serve v1.2.3 (source tag missing); PR #731 OPEN.
**Adversarially verified:** every CRITICAL/HIGH re-checked by an independent skeptic — materially corrected ~12 prior beliefs (real TM blocker is Alan AI Inc. not Amazon; Texas imports the non-waivable DTPA; EAA microenterprise exemption applies; Heckman cert denied 10/2025; Colorado Springs local tax nexus is live now; Win11 Smart App Control hard-blocks unsigned; BIPA forum is Cook County; Unicolors limits the AI-disclosure downside).
**Deferred/not done:** all remediation edits (gated); cookie-footprint check for the DNT (b)(6) sentence; the diligence gaps with no domain owner (Whisper-weight licenses, patent FTO, code chain-of-title).

## 5. The 6 CRITICALs (full detail in audit-report.md)
1. Fabricated/insider testimonials → FTC Fake Reviews Rule ($51,744/violation) — **remove today**.
2. Sole-proprietor personal liability total & live now → form CO LLC immediately.
3. Mass-arbitration tail ~$283k, personal (no entity).
4. Live "ALAN" Reg. 6180597 (Alan AI) §2(d) bar — stage around its 2026-10-20 §8 cliff.
5. Stripe ≠ marketplace facilitator → seller-of-record owes EU/UK VAT + US/CO-Springs tax now.
6. §412 copyright registration deadline **2026-09-10** (first-publication trigger) + AI-authorship disclosure.

## 6. Follow-ups / known limitations
- **Owner decisions gate remediation:** testimonial authenticity (#1), identity reconciliation (blocking), entity, governing law, arbitration, tax structure. See `user-action-list.md`.
- **HARD deadline:** copyright by **2026-09-10**; needs the AI-authorship counsel answer first.
- **Diligence gaps with no domain owner:** verify Whisper-weight + accel-pack licenses; patent FTO; code chain-of-title; foreign markets (Brazil/Japan/Korea/India/Switzerland) and Alan SA (EU) unanalyzed.
- **Security:** rotate the Stripe secret key exposed in a prior session.
- **Tax (CR-5) — owner leaning toward Stripe Managed Payments** (merchant-of-record, 3.5% add-on): confirmed via Stripe docs it covers US sales tax + EU VAT (registers/collects/files/remits, "no action required"), which resolves CR-5 going forward + the chargeback risk. Caveats logged: it's a checkout code change (`managed_payments` param in `app/api/echo/checkout/route.ts`), Stripe becomes seller-of-record (minor EULA/refund-wording ripple), and it does NOT fix back-tax on past sales. Business is operating at a loss (~$130 rev vs ~$300 costs) → no income/SE tax (loss can offset other income); VAT/sales-tax still applies regardless of profit. Not selling UK currently. Next offered: prep the Managed Payments checkout change.
- **EULA changes batched:** the two live false claims + all v2 terms go through counsel-held PR #731 with a single `EULA_VERSION` bump (avoid spurious re-acceptance) — never piecemeal.
- **PHASE 3 discipline reminders:** website PRs `--base main` + edit `lib/settings/disclosures.ts` when touching a legal page (CI gate) + `vercel-pre-deploy-check`; app off `main`, dual-platform, preserve fail-open boot; never GUI-test against the user's resident Echo.
