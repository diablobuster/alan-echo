# AUDIT PROMPT: ALAN Legal Completeness — No Blind Spots

**Date authored:** 2026-06-12
**Use:** paste into a fresh session AFTER the implementation plan has been executed (and re-run before any major launch: Mac GA, pricing change, new product, EU push). It audits everything ALAN-related — Echo, the website/platform, distribution, communications, and business filings — against the full legal posture, demanding evidence for every "pass".
**Companions:** research = `docs/2026-06-12-legal-research-copyright-eula.md`; plan = `docs/superpowers/plans/2026-06-12-legal-protection-ship-readiness.md` (both in alan-echo).

---

## PROMPT (paste verbatim; working directory `C:\Users\arowm\alan-echo`)

```
YOU ARE THE LEGAL COMPLETENESS AUDITOR for ALAN Global Intelligence.

Your job is adversarial: assume the implementation has blind spots and hunt
them. You are auditing on behalf of the owner, against three imagined
adversaries: (1) a plaintiff's consumer-protection lawyer reading every public
claim, (2) a software pirate probing the license system, (3) a regulator (FTC /
state AG / EU DPA) reading the privacy story. For every checklist item below,
record a PASS only with EVIDENCE (a command output, a file:line quote, a
screenshot description, or a dashboard fact). "It should be fine" = FAIL.

## Scope inventory (audit ALL of it — list anything you find that is NOT here)

Repos/properties:
- C:\Users\arowm\alan-echo (Tauri desktop app, Windows + macOS)
- C:\Users\arowm\stock-analyzer (alanglobalintelligence.com — Echo commerce
  pages AND the ALAN finance/research platform)
- github.com/diablobuster/alan-echo-releases (private releases repo, via gh)
- Stripe account (checkout, tax, refunds, business profile)
- Transactional email templates (stock-analyzer lib/echo/email.ts and any
  other senders found via grep for nodemailer/sendmail usage)
- Any other distribution or social surface you discover (search the repos for
  outbound URLs, store listings, social handles) — inventory first, then audit.

First action: build the inventory. Run broad discovery greps (domains, URLs,
email senders, third-party services, data stores) and list every surface a
customer, pirate, or regulator could touch. Anything outside the list above is
a finding by itself (severity: note).

## Method

Work lens by lens. Within each lens: check every item, attach evidence,
classify CONFIRMED-PASS / FAIL / PARTIAL / NOT-VERIFIABLE-LOCALLY (e.g., needs
Stripe dashboard or a filed-document number — list those as USER-VERIFY with
exact instructions). Then run the adversarial pass for that lens: "what would
my adversary attack here that the checklist missed?" Add anything you find.

### Lens 1 — Contract formation (clickwrap integrity)
- [ ] Fresh-install behavioral test (npm run tauri dev with a clean profile):
      EULA gate renders BEFORE any trial/license use; Decline exits; Accept
      persists {eula_accepted_version, eula_accepted_at}; version bump
      re-prompts. Evidence: the settings file contents after acceptance.
- [ ] The accepted-version constant in the app matches the live EULA's
      effective date on the site (grep both; they must be identical strings).
- [ ] NSIS installer displays the license page (build or inspect bundle config
      + legal/EULA.txt freshness vs src/legal/eula.md — diff them).
- [ ] Checkout: Stripe session code contains consent_collection terms_of_service
      "required" + custom_text; CTA microcopy links the EULA on /echo,
      /echo/download (trial), and any other buy entry point (grep for
      checkout links sitewide — pricing page too).
- [ ] EULA is reachable pre-purchase: site footer link, echo page link,
      checkout consent link. Evidence: grep hits + rendered nav.
- [ ] Acceptance evidence trail: app persists version+timestamp; site records
      Stripe consent (verify Checkout session stores consent — USER-VERIFY in
      dashboard if needed).
- Adversarial: can ANY path use the software without ever passing the gate?
  (Portable exe? Old installer version still served anywhere? Update path that
  skips first-run? Trial via direct binary from the version endpoint?)

### Lens 2 — EULA substance
- [ ] All sections present: grant (licensed-not-sold + transfer restriction +
      use restrictions — the Vernor triad), trial, updates/support, third-party
      components pointer, local-storage/activation disclosure, restrictions,
      refunds, warranty disclaimer w/ jurisdictional savings, liability cap
      WITH carve-outs (gross negligence/willful/fraud), termination, export,
      governing law matching the actual entity/state, dispute clause matching
      the recorded P0.2 decision (with opt-out + small-claims if arbitration),
      EU reverse-engineering savings language, contact with a real address.
- [ ] No contradictions: EULA vs /terms vs /refund-policy vs marketing claims
      vs Stripe receipt text. Grep each money/refund/warranty assertion and
      cross-check all five surfaces.
- [ ] Counsel status: is the counsel-review CI gate covering echo-license +
      refund-policy? Is any "pending counsel review" text still public?
- Adversarial: read the EULA as a hostile EU consumer-protection lawyer —
  flag any term void under Directive 2009/24/EC Art. 8, any blanket
  disclaimer without savings language, any "we can change anything anytime"
  clause.

### Lens 3 — IP protection state
- [ ] Copyright: registration case number recorded in docs/legal-filings/
      (USER-VERIFY the filing receipt). Deadline check: was the application
      received within 3 months of first publication (compare first release
      date via gh release list)? If not yet filed and today < 2026-09-10:
      severity CRITICAL with days remaining.
- [ ] © notices: app FooterBar, About panel, installer metadata
      (tauri.conf copyright field), website footer, releases README, email
      templates. Grep "© 2026" across all; list misses.
- [ ] Trademark: clearance opinion + filing status recorded (USER-VERIFY).
      If "ALAN Echo" is in use with no clearance docketed: severity HIGH
      (Amazon ECHO Class 9 conflict risk).
- [ ] Repo hygiene: LICENSE (proprietary) + real README in alan-echo; no
      open-source LICENSE file in the releases repo or anywhere implying an
      OSS grant over the product.
- Adversarial: if someone forked/copied the app today, what would enforcement
  actually rest on? Walk the ladder (DMCA→CCB→court) and confirm each
  prerequisite exists NOW, not in a plan.

### Lens 4 — Open-source compliance
- [ ] THIRD-PARTY notices file exists, regenerated against CURRENT lockfiles
      (re-run the generator; diff — stale = FAIL), includes whisper.cpp and
      Whisper model MIT entries, ships in the bundle, viewable in-app.
- [ ] Apache-2.0 deps: any with NOTICE files? Verify carriage (spot-check 3).
- [ ] No GPL/AGPL/SSPL dependency anywhere in either lockfile
      (license-checker + cargo-license scans; evidence: the scan summary).
- [ ] whisper.cpp usage consistent with MIT (attribution present; no claim of
      ownership over it in EULA/registration materials).

### Lens 5 — Privacy truth (claims vs code vs policy)
- [ ] Build the app's ACTUAL outbound-call inventory from source (grep reqwest/
      http in src-tauri): every endpoint listed. Compare against (a) marketing
      claims on /echo, /echo/compare, /echo/vs-dragon, (b) the privacy
      policy's app section, (c) the EULA §4 disclosure. All four must agree.
      Any "no network calls" absolutism remaining = FAIL with file:line.
- [ ] Airplane-mode test claim is literally true for dictation (verify the
      dictation path makes zero calls — code inspection of the record→
      transcribe→paste path).
- [ ] Server-side data inventory: prisma schema Echo models — every PII field
      (email, ip, userAgent, machineHash) appears in the privacy policy with
      purpose + retention. Site analytics (Plausible) disclosed as cookieless.
- [ ] CalOPPA elements present in /privacy (categories, third parties, DNT
      response, effective date). GDPR basics for EU buyers (controller
      identity, rights, contact).
- [ ] BIPA posture: confirm the app computes no speaker-identification
      embeddings and retains no voice biometrics; confirm the one-sentence
      local-processing disclosure exists in EULA+privacy.
- [ ] Registry/local trial-state storage disclosed (EULA §4 + privacy).
- Adversarial: an FTC deception analysis — list every absolute claim
  ("never", "no", "100%", "zero") on any public surface and prove each one
  literally true or get it scoped.

### Lens 6 — Commerce & consumer protection
- [ ] Refund integrity end-to-end: Stripe refund → webhook sets revokedAt →
      validate-key/download/activate all reject (run the vitest suite +
      trace the webhook handler; evidence: test output + code path).
- [ ] 30-day guarantee consistent everywhere (terms carve-out present;
      refund-policy live; checkout custom_text mentions it).
- [ ] Key delivery truth: success/recover pages match reality (no "emailed"
      claims while sending is disabled); /echo/keys works logged-in.
- [ ] Stripe Tax: automatic_tax in code; registrations reviewed (USER-VERIFY
      dashboard; record date of last review).
- [ ] EU digital-goods: immediate-delivery withdrawal acknowledgment present
      at checkout; EAA applicability noted/screened (USER-VERIFY counsel
      answer recorded).
- [ ] Price/claims accuracy: $89 consistent across site, Stripe, EULA cap;
      machine limit (5) consistent across activate route, EULA, marketing.
- [ ] CAN-SPAM: transactional emails carry a physical address (and it's the
      business address post-D3, not the home apartment).
- Adversarial: file a mental chargeback — does every customer-facing promise
  have a matching system behavior? List promise→mechanism pairs.

### Lens 7 — Anti-piracy & security-legal interlocks
- [ ] Code signing: Windows binaries Authenticode-valid; macOS signed +
      notarized + stapled (Get-AuthenticodeSignature / spctl evidence, or
      USER-VERIFY on latest release artifacts).
- [ ] SHA256SUMS attached to the latest release; site download page hash
      instructions match.
- [ ] Activation tokens carry exp; expired→silent re-activation works; revoked
      keys die offline within the exp horizon (code inspection).
- [ ] No secrets in shipped binaries: scan for the HMAC checksum secret and
      any Stripe/API keys in the bundle (strings scan of the built exe for
      known env-var values — evidence: scan ran + result). Ed25519 PUBLIC key
      in app is correct-by-design; PRIVATE key only server-side (grep env
      usage; confirm not in repo history via gitleaks-style grep).
- [ ] /api/echo/version no longer returns a raw unauthenticated asset URL.
- [ ] Rate limiting present on activation + auth endpoints (grep; the
      2026-06-11 audit flagged NextAuth credentials login unthrottled).
- [ ] DMCA readiness: takedown contact info + the GitHub AUP-route playbook
      reachable in docs (research doc §2.5 Q16) — this is the operational
      anti-keygen path.
- Adversarial: think like the keygen author — what's the weakest link today,
  and is the legal trail (registration + §1201-protected TPMs + EULA) ready
  to use against them THIS WEEK?

### Lens 8 — The ALAN finance platform (the rest of the site)
The Echo work must not leave the platform side stale:
- [ ] /terms, /privacy, /legal/disclaimer, /legal/disclosures: dates current,
      counsel-review stamps present, no contradiction with new Echo pages.
- [ ] Investment-content posture: disclaimers on research/analysis surfaces
      ("informational, not investment advice") present on the routes that
      render recommendations/signals (spot-check 5 routes incl. /research,
      /trade, /global); no performance claims without basis; testimonial/
      results claims (if any) FTC-compliant.
- [ ] Subscription terms (the platform side) don't accidentally apply to or
      contradict Echo's one-time license (cross-references checked).
- [ ] Account/data rights: deletion path exists for platform accounts holding
      Echo licenses (what happens to license records on account deletion —
      documented?).

### Lens 9 — Business & records hygiene
- [ ] Entity decision recorded; governing law in EULA/terms matches it;
      Stripe legal entity matches; customer-facing address is not a home
      apartment (grep "Fountain Blvd" — must be gone post-D3).
- [ ] docs/legal-filings/ holds: registration case number, trademark docket,
      counsel memo, insurance binder, decision records. (USER-VERIFY contents;
      confirm the folder isn't committed to a public repo.)
- [ ] Insurance bound (E&O/cyber) or consciously declined in writing.
- [ ] Calendar artifacts exist: §412 deadline (until filed), annual
      re-registration, quarterly Stripe Tax review, EULA-version sync check.
- [ ] Acceptance-records retention: EULA acceptance logs (app settings) and
      Stripe consent records — note where each lives and that nothing purges
      them.

### Lens 10 — Cross-platform parity (standing memory rule)
- [ ] Every Lens 1–7 item that touches the app verified for BOTH Windows and
      macOS paths (gate, storage disclosure wording, signing, notices,
      uninstall guidance). Anything Windows-only without a Mac twin = FAIL.

## Output format

Produce `docs/YYYY-MM-DD-legal-completeness-audit.md` (alan-echo repo):
1. **Verdict line:** SHIP-READY / SHIP-READY WITH USER ACTIONS / NOT READY.
2. **Findings ledger** — one row per item: Lens | Item | Status | Evidence |
   Severity (CRITICAL/HIGH/MED/NOTE) | Exact fix (file + change).
3. **USER-VERIFY queue** — dashboard/filing items a human must confirm, with
   exact instructions.
4. **Blind-spot sweep** (mandatory final section): answer each —
   (a) What surface did I discover that was NOT in the scope inventory?
   (b) Which absolute public claim am I least certain is literally true?
   (c) What would each of the three adversaries attack first, and is it closed?
   (d) What changed in the world since 2026-06-12 that the research doc
       wouldn't know? (fee NPRM finalized? new case law? — flag for re-research
       if >6 months have passed.)
   (e) If the answer to any of these is non-empty, convert it to ledger rows.
5. Keep total severity-CRITICAL count on the verdict line.

Be exhaustive. A clean audit here is the artifact that says "completely
protected and ready to ship at scale" — do not award it cheaply.
```

---

## Re-run triggers (for the human)

Run this audit: after the implementation plan completes; before Mac GA (with Lens 10 emphasized); before any paid-marketing push at scale; after any EULA/pricing/privacy change; every 6 months regardless (case law and Copyright Office fees move — the 2026 NPRM will eventually change the numbers baked into the research).
