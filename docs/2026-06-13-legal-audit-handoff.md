# HANDOFF: Comprehensive, Research-Heavy Legal Audit of ALAN Echo

**Date:** 2026-06-13
**For:** the next session (use a high-capability model; this is broad + adversarial)
**Goal in one line:** make ALAN Echo (and its commerce/legal surface) maximally safe from lawsuits, airtight on IP protection, and compliant across every body of law that touches a globally-sold, voice-capturing, AI-powered desktop app — by running deep multi-perspective subagent research, saving it all to the ALAN_RD folder, then editing whatever needs editing.

> This is **legal information, not legal advice.** The audit produces research + drafts + flags. Substantive new legal terms are drafted **for counsel**, never shipped as if a lawyer reviewed them.

---

## PART 0 — Read these first (don't re-derive what's known)

Read, in order, before doing anything:
1. `alan-echo/docs/2026-06-12-legal-research-copyright-eula.md` — the original two-report deep research (US software copyright registration + EULA law), per-claim provenance tags.
2. `alan-echo/docs/superpowers/plans/2026-06-12-legal-protection-ship-readiness.md` — the implementation plan (4 workstreams A–D).
3. `alan-echo/docs/2026-06-12-legal-audit-prompt.md` — a 10-lens verification audit prompt. **This new audit BUILDS ON and SUPERSEDES it** — go deeper, research-first, and remediate.
4. `alan-echo/docs/2026-06-12-legal-completeness-audit.md` — the implementing terminal's self-audit (verdict: SHIP-READY WITH USER ACTIONS).
5. `alan-echo/docs/2026-06-12-legal-research-session-log.md` — the full execution log (Addenda 1–4): everything done this session.
6. `stock-analyzer/docs/2026-06-11-website-comprehensive-audit.md` and `…echo-comprehensive-audit.md` — prior product audits (pre-legal-work; some items now fixed).
7. Auto-memory (loaded each session): `project_echo-legal-status`, `project_resident-echo-app`, `project_parallel-session-coordination`, `feedback_deep-research-rate-limit-artifact`, `feedback_dual-platform-dev`.

---

## PART 1 — The product (facts the audit rests on)

- **ALAN Echo**: $89 one-time-purchase desktop voice-to-text app. **Windows shipping; macOS in development** (the Mac build was historically non-functional — verify current status).
- **Stack**: Tauri 2 — Rust backend (`alan-echo/src-tauri/`), React/JSX frontend (`alan-echo/src/`), NSIS installer.
- **Speech**: bundled **whisper.cpp** (MIT, Georgi Gerganov) + **OpenAI Whisper models** (MIT). All dictation is **100% local** — audio never leaves the device; no telemetry; no analytics in the app.
- **The app's only outbound calls** (verify against `src-tauri/` source — grep `reqwest`/`http`): (1) license **activation** → `https://www.alanglobalintelligence.com/api/echo/activate` (sends license key + a one-way SHA-256 machine-fingerprint hash; server stores key, machine hash, activation time, **IP, user-agent**); (2) **update check** → `/api/echo/version`; (3) optional **model/GPU-pack downloads** → site + **huggingface.co**. The dictation path itself makes zero network calls.
- **Licensing**: HMAC-checksummed key format `ECHO-XXXXX-XXXXX-XXXXX-CCCCC`; offline activation via **Ed25519-signed JWT** (now with 400-day `exp`); up to **5 machines/key**; signed local **trial** state (5/day, 50 lifetime) stored in app-data files AND the **Windows registry**.
- **Commerce**: sold via **Stripe** on **alanglobalintelligence.com** (Next.js app, repo `stock-analyzer`, deploy branch `main`). Keys delivered via `/echo/keys` (login) + `/echo/recover`; email delivery currently disabled. Refunds revoke keys + activations.
- **Distribution**: installers via **private** GitHub Releases repo `diablobuster/alan-echo-releases`, served through `/api/echo/download`.
- **Business**: **sole proprietorship** (Stripe business type = "individual"; legal name on Stripe = "Richard Romano"). **No LLC yet** — user plans to form one (~1 week from 2026-06-13). Operating name "ALAN Global Intelligence." Address on file is a **home apartment** in Colorado Springs, CO. The website ALSO hosts a separate ALAN finance/research platform (out of scope except where its legal pages are shared).

---

## PART 2 — Everything that was done (so the audit doesn't re-flag fixed items)

### 2a. This session (research + planning + execution)
- Produced the research doc, the implementation plan, the implementation handoff, and the 10-lens audit prompt (Part 0 items 1–4).
- Fixed Vercel `ECHO_INSTALLER_SHA256` (was a stale `576c16d0…`; now the verified v1.2.1 `6dbb09f5…`) and **discovered + created** the missing `NEXT_PUBLIC_ECHO_INSTALLER_SHA256` (the download page had shown "pending" forever).
- Reviewed + merged the implementation PRs (below); deployed the website; built + published app releases **v1.2.2** then **v1.2.3**.
- Verified both audit CRITICALs live (installer-hash consistency; Stripe consent not blocking checkout).
- **Found + fixed a live legal inconsistency**: the EULA/terms/privacy/disclaimer/disclosures all named a **non-existent "ALAN Global Intelligence LLC"** (terms even claimed "a corporation organized under the laws of the State of Texas" — triple-false). Corrected sitewide to "ALAN Global Intelligence, a sole proprietorship" (PR #733, merged + **live**) and in the app EULA (v1.2.3).

### 2b. The other (implementing) terminal — executed the plan
- **alan-echo PR #1 (MERGED):** first-launch **EULA clickwrap gate** (Decline quits, Accept persists version+timestamp, version-bump re-prompts); copyright notices (FooterBar, Settings→About, tauri.conf `copyright`, installer); **third-party OSS notices** (generated `src/legal/third-party-notices.txt` + in-app viewer); **NSIS installer license page** (`legal/EULA.txt`, confirmed working via screenshot); proprietary `LICENSE` + real `README` with release checklist; **activation token `exp`** + silent re-activation + atomic write + license-key salvage on corrupt settings; whisper language allowlist; WAV cleanup on failed transcription; accessibility roles; Mac-neutral copy; `scripts/release-checksums.ps1`.
- **stock-analyzer PR #730 (MERGED + deployed):** Stripe **`consent_collection` ToS required** + `custom_text` EULA/EU-withdrawal microcopy; **"by purchasing you agree"** links at every CTA; **terms↔refund contradiction fixed**; **privacy policy Echo app section** (activation data, local storage, no audio collection) + Plausible-cookieless disclosure; **marketing claims scoped to dictation truth** (removed the false absolute "no network calls / no telemetry"); **revocation guards** (`lib/echo/guards.ts` + tests) on download/validate/activate + refund webhook revokes activations; **version endpoint no longer leaks the raw asset URL**; footer Echo-License + Refunds links; counsel-review CI gate extended to echo-license + refund-policy.
- **stock-analyzer PR #731 (OPEN — HOLD FOR COUNSEL):** EULA v2 drafts — §3A trial terms, §3B updates/support, §3C third-party components, §4 storage/activation disclosure, §7 liability-cap carve-outs (gross negligence/willful/fraud), §1 licensed-not-sold + transfer. Base branch `legal/echo-consent-and-claims` (now merged to main → **needs rebase to main**). Three flagged wording problems for counsel: an inaccurate "validates offline" claim, "any number of machines" vs the 5-machine cap, "Windows devices" vs the Mac plan, and a missing CalOPPA DNT sentence.
- **Releases repo:** proprietary README (EULA pointer + checksum instructions); SHA256SUMS on releases.

### 2c. Current shipped state vs. pending
**Live now:** all of 2a + 2b-merged items; the website legal pages are accurate (sole-prop, consistent refund, truthful claims/privacy); the app has the EULA gate + notices + signing-less but checksummed releases.
**Pending (the audit should track, not re-discover):**
- EULA-gate **click-test** by the user → gates the **serve-v1.2.3 env flip** (`ECHO_RELEASE_TAG=v1.2.3`, both SHA vars → `74569fc6b36c3313881b77bea43b989bd209ff4bd5c01acc8c4941134c9996c7`, version/size/date display vars, redeploy). Until then the site serves **v1.2.1** (which still has the old "LLC" EULA in its installer — another reason to land the flip soon).
- **PR #731** EULA v2 (counsel-held; rebase to main).
- **P0.1** entity decision + **governing law** (EULA §10 still says **Texas**; business is in **Colorado** — mismatch to resolve).
- **P0.2** arbitration keep/drop. **P0.3** counsel engagement. **P0.4** code-signing credentials (app still **unsigned** → SmartScreen warnings).
- **D1 copyright registration — HARD DEADLINE 2026-09-10** (§412 window; needs the AI-authorship-disclosure counsel answer first).
- **D2** trademark clearance ("ALAN ECHO" vs Amazon's ECHO marks, Class 9). **D3** entity + replace the **home address** (still in Stripe profile + email templates). **D4** insurance (E&O/cyber) + sales-tax/VAT review.
- **Rotate the Stripe secret key** (a live key was pasted into chat this session).
- **v1.2.2** release contains the old "LLC" EULA, is unserved + private — delete or ignore.

---

## PART 3 — THE MISSION (paste/execute in the next session)

```
YOU ARE THE ALAN ECHO LEGAL AUDIT & REMEDIATION LEAD.

Mission: produce the most thorough, multi-perspective legal audit possible of
ALAN Echo and its commerce/legal surface, then FIX what's fixable. Optimize for
"we will not get sued, our IP is protected, and we're compliant in every market
we sell to." Be creative and adversarial — assume a plaintiff's lawyer, a
competitor, a regulator (FTC/state AG/EU DPA), and a patent/IP troll are each
looking for a way in.

First: read every file in PART 0 of this handoff and the auto-memory notes, so
you build on prior work instead of repeating it. Confirm current git/PR/deploy
state with live commands (don't trust this doc's snapshot — verify).

## Output location (REQUIRED)
Save ALL research and audit artifacts under:
  C:\Users\arowm\alan_rd\legal-audit-2026-06\
- research\<domain>.md          ← one file per research domain (raw findings + citations)
- audit-report.md               ← the master synthesized audit (findings ledger)
- remediation-log.md            ← what you edited, why, and what you deferred to counsel
- counsel-memo.md               ← the consolidated list of questions only a lawyer can answer
Do NOT commit alan_rd to any code repo (research lives outside version control).

## Method: subagent fan-out, then synthesize, then remediate

PHASE 1 — RESEARCH (parallelize aggressively).
Spawn one research subagent per domain below (use the Agent tool in parallel,
and/or the deep-research Workflow). Each subagent must: search 8–15 AUTHORITATIVE
sources (statutes, regulations, court opinions, .gov guidance, bar/firm
publications — not SEO blogspam), cross-check load-bearing claims across ≥2
sources, flag disagreements/unsettled law, and WRITE its findings to
research\<domain>.md with citations (source name + URL) and a confidence tag per
claim. Tell each subagent the PART 1 product facts so its analysis is concrete.

CAVEAT (learned 2026-06-12): the deep-research Workflow miscounts rate-limited
verifier abstentions as "refuted." If you use it and see mass refutations, read
the failures[] array — treat claims as UNVERIFIED, not refuted, and salvage them
from the agent-*.jsonl transcripts. Parallel Agent calls avoid this entirely.

Research domains (cover all; add any you think of):
1.  COPYRIGHT — registration mechanics + the AI-generated-code authorship
    question (USCO Jan-2025 report; what to claim/disclose for an AI-assisted
    app); deposit strategy that excludes secret source; derivative-work/new-
    version cadence; the §412 2026-09-10 deadline; whisper.cpp + Whisper-weights
    treatment.
2.  TRADEMARK — clearance for "ALAN ECHO" and "ALAN" in Class 9 (software) and
    42; the crowded "ECHO" field incl. Amazon's marks; likelihood-of-confusion
    analysis; whether to file/Reword/coexist; brand-protection roadmap.
3.  EULA ENFORCEABILITY (US) — clickwrap formation (the app's first-launch gate);
    sole-prop as contracting party vs the planned LLC; governing-law choice
    (Texas clause vs Colorado domicile — reasonable-relationship doctrine);
    liability-cap carve-outs; unconscionability; reverse-engineering clause
    (Bowers/Davidson) limits.
4.  EULA & CONSUMER LAW (EU/UK/CA/AU) — since they sell globally via Stripe: EU
    Software Directive 2009/24 (decompilation savings), Unfair Contract Terms
    Directive, Consumer Rights Directive withdrawal right for digital goods,
    UK CRA 2015, Canada consumer protection, Australia ACL non-excludable
    guarantees. Which mandatory protections override the EULA's choice of law?
5.  ARBITRATION — FAA enforceability + economics at $89 (per-case AAA/JAMS fees
    the BUSINESS pays); mass-arbitration risk; 30-day opt-out + small-claims
    carve-out best practice; is mandatory arbitration even advisable for a solo
    vendor — recommend keep-with-fixes vs drop.
6.  PRIVACY — US — CalOPPA (website), CCPA/CPRA thresholds (does a sub-$25M
    sole prop qualify?), the activation data (IP/UA/machine-hash) retention +
    disclosure accuracy, COPPA (any minor users?), state privacy laws (VA/CO/CT
    etc.). Verify the live privacy policy matches the app's ACTUAL data flows.
7.  PRIVACY — GDPR/global — EU/UK buyers make the website a controller; lawful
    basis, DPA with Stripe, SCCs, data-subject rights, the no-audio-collection
    story. The app as (non-)controller of audio.
8.  BIOMETRIC & RECORDING LAW — Illinois BIPA (does local speech-to-text create
    a "voiceprint"? the Apple/Siri BIPA line); Texas CUBI; Washington; state
    two-party-consent wiretap/recording laws (CIPA §632 et al.) — exposure to
    the DEVELOPER when the USER records. One-sentence disclosures needed?
9.  FTC / DECEPTION / UDAP — every absolute public claim ("100% on-device,"
    "private by architecture," "nothing leaves your machine," "no telemetry,"
    "30-day money-back") tested for literal truth + substantiation; dark-pattern
    review of checkout/trial; endorsement/testimonial rules if any reviews are
    shown; "AI" claims accuracy.
10. ANTI-PIRACY / DMCA / CFAA — §1201 anti-circumvention posture for the license
    /trial TPMs; DMCA takedown + §512(h) readiness; CFAA post-Van Buren; the
    GitHub AUP route for keygens; realistic enforcement ladder.
11. EXPORT / SANCTIONS / ENCRYPTION — EAR treatment of the app's crypto
    (Ed25519 auth-only — likely exempt, confirm); OFAC sanctions (selling via
    Stripe to embargoed countries — does Stripe geo-block? what must the EULA
    say?); the export clause adequacy.
12. ACCESSIBILITY — ADA (website + app) Title III web-accessibility exposure;
    EU **European Accessibility Act** (in force June 2025 — applies to e-commerce
    + many software products sold to EU consumers): does Echo/its checkout fall
    in scope, and what's required? WCAG 2.1/2.2 AA gaps.
13. AI-SPECIFIC LAW — EU AI Act classification of a local speech-to-text app
    (likely minimal/limited-risk → transparency duties?); USCO AI authorship for
    the app's own copyright; training-data/IP risk inherited from Whisper;
    output-accuracy disclaimers.
14. TAX / PAYMENTS / EMAIL — US sales-tax economic nexus per state + Stripe Tax
    config; EU VAT/OSS for digital goods; CAN-SPAM (physical address — currently
    a home apartment); Stripe ToS/Restricted-Business compliance; refund/charge-
    back posture.
15. ENTITY / LIABILITY / INSURANCE — sole-prop personal-liability exposure;
    LLC formation timing + governing-law implications; piercing risks; E&O/cyber
    insurance norms + cost; the home-address exposure (DMCA notices, disputes).
16. PRODUCT-LIABILITY / WARRANTY — Magnuson-Moss (no written warranty given);
    UCC 2-316/2-719 disclaimer + failure-of-essential-purpose; could a bad
    transcription cause downstream harm claims? "AS IS" adequacy.
17. DISTRIBUTION-CHANNEL TERMS — GitHub ToS for serving release binaries;
    future Microsoft Store / Mac App Store policy fit; code-signing/notarization
    legal-trust implications; SmartScreen liability framing.

PHASE 2 — SYNTHESIZE.
After the research files land, write audit-report.md: a findings ledger with
columns [Domain | Finding | Severity CRITICAL/HIGH/MED/NOTE | Evidence (file:line
or source) | Status: already-fixed / needs-edit / needs-counsel / needs-user-
action | Exact remedy]. Cross-reference PART 2 of the handoff so you mark
already-shipped items as such. Rank by litigation/financial risk. Then a
"blind-spot sweep": what domain/claim/market did we NOT cover, and what would
each adversary attack first?

PHASE 3 — REMEDIATE (edit what's safely fixable).
For every finding marked needs-edit where the fix is factual/clear (not novel
legal drafting), MAKE the change, following the disciplined flow:
- App (alan-echo): branch off main; dual-platform (Windows+macOS); never break
  the fail-open boot posture; if you touch the EULA text, regenerate legal/EULA.txt
  (`node scripts/gen-eula-txt.mjs`) and decide consciously on EULA_VERSION (bump =
  forces re-acceptance — batch with the LLC-formation re-accept if possible).
- Website (stock-analyzer): branch off main; **`gh pr create` defaults base to
  `staging` — ALWAYS `--base main`**; editing any legal page requires touching
  `lib/settings/disclosures.ts` in the same diff (counsel-review CI gate); run the
  vercel-pre-deploy-check skill before merge; CI carries ~113 pre-existing
  failures (verify no NEW ones; local `npm run build` is the real gate); verify
  live after deploy.
- Legal TEXT that is NEW substantive terms (not a factual correction) → DRAFT it
  into PR #731 or a new HOLD-FOR-COUNSEL PR and add it to counsel-memo.md. Do
  NOT ship novel legal language as if reviewed.
Log every edit in remediation-log.md.

PHASE 4 — HAND BACK.
Produce: (a) audit-report.md verdict line (SHIP-READY / WITH ACTIONS / NOT
READY + CRITICAL count); (b) counsel-memo.md (the consolidated lawyer questions,
with the §412 2026-09-10 deadline at top); (c) a prioritized user-action list
(decisions, filings, signups, the Stripe-key rotation, the EULA-gate click-test
+ serve-v1.2.3 flip); (d) a session log under alan-echo/docs/ per the repo
convention.

## Guardrails
- Legal information, not advice. Flag where only a licensed attorney can decide.
- NEVER GUI-test the app via `tauri dev` while the user's resident Echo runs
  (shared com.alan.echo data dir) — test the installer; match processes by PATH;
  never screenshot the user's app window (private transcripts).
- Don't touch governing-law/arbitration clauses as "edits" — those are P0
  decisions; surface recommendations instead.
- A live Stripe secret key was exposed earlier — treat all secrets as sensitive;
  never echo key values; remind the user to rotate.
- Verify before claiming done: live curls for web, installer inspection for app.
```

---

## PART 4 — Quick-reference: repos, branches, commands

- **App:** `C:\Users\arowm\alan-echo` (Tauri; deploy = build+publish to releases repo). Branch off `main`.
- **Website:** `C:\Users\arowm\stock-analyzer` (Next.js; deploy branch `main`; `gh pr create` → **always `--base main`**). Run `vercel-pre-deploy-check` before merge.
- **Releases:** `diablobuster/alan-echo-releases` (private; `gh` only). Latest = v1.2.3 (`74569fc6…`).
- **Live verify:** `curl -s https://www.alanglobalintelligence.com/<path>` (legal pages, checkout, download).
- **Vercel envs:** `cd stock-analyzer && npx vercel env ls production` (and `env pull` to read values).
- **Research output:** `C:\Users\arowm\alan_rd\legal-audit-2026-06\`.

---

## PART 5 — The single highest-stakes clock
**US copyright registration must be RECEIVED by 2026-09-10** to preserve statutory damages back to first publication (§412). It needs the AI-authorship-disclosure answer from counsel first. Everything else can slip; this date cannot. Put it at the top of counsel-memo.md and the user-action list.
