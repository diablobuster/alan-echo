# ALAN Legal Completeness Audit — 2026-06-12

**Verdict: SHIP-READY WITH USER ACTIONS — 2 open CRITICALs, both human-action items (installer-hash reconciliation; copyright filing by 2026-09-10).**

Audited: alan-echo `legal/eula-gate-and-notices`, stock-analyzer `legal/echo-consent-and-claims` + `legal/echo-eula-v2-HOLD`, releases repo, email templates. Method: three parallel read-only evidence agents (lenses 1–8, 10) + primary verification of every FAIL by the coordinating session (one agent FAIL was refuted on re-check — see ledger). GUI/behavioral items are USER-VERIFY (no display interaction available headless).

## Findings ledger

| Lens | Item | Status | Evidence | Severity | Fix |
|---|---|---|---|---|---|
| 1 | EULA gate before trial/license; Decline quits; acceptance persisted (version+timestamp) | PASS (static) | main.jsx phase gate; EulaGate.jsx `set_setting('eula_accepted_version'…)`; `quit_app` | — | Behavioral click-through = USER-VERIFY |
| 1 | App EULA_VERSION ↔ site EFFECTIVE_DATE match | PASS | `2026-06-10` ↔ "June 10, 2026" | — | — |
| 1 | NSIS installer license page | PASS | installer.nsi `MUI_PAGE_LICENSE "${LICENSE}"`; staged license_file = EULA; EULA.txt fresh vs eula.md | — | Run installer in VM = USER-VERIFY |
| 1 | Checkout consent + custom_text; CTA microcopy at hero/pricing/download CTAs | PASS | checkout route consent_collection + EU/UK message; `cta-legal` in DualCta, CheckoutCta, download page | — | — |
| 1 | EchoClient nav buy button lacks microcopy | PARTIAL | EchoClient.tsx:28 nav CTA, no adjacent cta-legal | MED | Optional: nav CTAs don't carry microcopy by design; Stripe consent covers formation. Follow-up if counsel wants it. |
| 1 | Gate bypass: hotkey path | PASS | `start_recording` calls `require_license()` Rust-side before capture | — | — |
| 1 | Gate bypass: tray dictate is frontend-routed | PARTIAL | Tray emits `dictate-toggle` → frontend; during EULA phase Dashboard isn't mounted (no handler) and any Rust recording path still passes require_license. EULA acceptance itself is enforced frontend-only (by design — clickwrap is a formation flow, license/trial limits are Rust-enforced). | MED→NOTE | Documented design; no change. |
| 2 | EULA: 11 sections, licensed-not-sold, EU reverse-engineering savings, jurisdictional savings | PASS | eula.md / live page | — | — |
| 2 | Live EULA §4 "License keys are validated offline" is FALSE (activation is online) | FAIL | activation.rs ACTIVATE_URL POST; auto-called on key entry | HIGH | HOLD PR #731 appends the truthful §4 disclosure; the offline sentence itself flagged to counsel for deletion (cannot rewrite legal text unilaterally) |
| 2 | EULA §2 "any number of machines" vs server 5-machine cap | FAIL | MAX_MACHINES_PER_KEY=5 in activate route | HIGH | Counsel flag in HOLD PR #731 — converge wording & cap |
| 2 | Missing v2 sections (trial/updates/3P/storage/carve-outs/transfer) | PARTIAL | Implemented verbatim on HOLD PR #731, pending counsel | — | Merge after P0.3 |
| 2 | "Pending counsel review" text public | NOTE | refund-policy header; echo-license code comment | — | B11 Step 2 after P0.3 |
| 3 | © notices everywhere (app footer, About, tauri.conf, LICENSE, README, site footer, email) | PASS | grep evidence, all hits | — | — |
| 3 | Proprietary LICENSE + product README; no stray OSS LICENSE | PASS | LICENSE first lines; Glob | — | — |
| 3 | Copyright registration not filed | OPEN | docs/legal-filings/ absent (expected) | **CRITICAL (deadline)** | D1 by **2026-09-10** (~90 days). Human filing. |
| 4 | Third-party notices: exists, bundled, whisper.cpp + OpenAI MIT, in-app viewer | PASS | notices file + ?raw import in SettingsPanel | — | — |
| 4 | GPL/AGPL/SSPL scan | PASS* | No pure copyleft; r-efi is `Apache-2.0 OR LGPL-2.1+ OR MIT` (permissive option elected) | NOTE | — |
| 5 | Outbound-call inventory: activation, version, gated download, GPU/Vulkan packs, HuggingFace models, localhost whisper | PASS | 5 remote endpoints enumerated, all disclosed in privacy §10 + EULA v2 §4 (HOLD) | — | — |
| 5 | Hero "Nothing leaves your machine" + "0 Network calls" stat | FAIL→FIXED | page.tsx:70/:83 — fixed in `93a24435` ("Your voice never leaves your machine"; "Network calls while dictating") | CRITICAL→closed | — |
| 5 | Other absolutes (airplane-mode, FAQ, vs-dragon, testimonials) | PASS | Scoped to dictation truth in B5 commits; vs-dragon claims audio-scoped (true) | — | "No cloud, no account, no subscription" lede: app needs no account, purchase requires one — NOTE for copy review |
| 5 | Privacy §10 ↔ prisma schema PII parity (key, machineHash, ip, userAgent, retention) | PASS | All four fields disclosed with purpose + retention (license life + 3y) | — | — |
| 5 | CalOPPA do-not-track response | FAIL | No DNT statement in /privacy | HIGH | Counsel to supply a verbatim DNT sentence (none drafted in plan; not improvised per rules) |
| 5 | BIPA: no speaker-ID/embeddings; audio discarded post-transcription | PASS | grep clean; `remove_file(&wav_path)` after transcribe; whisper bound to 127.0.0.1 | — | — |
| 6 | Refund integrity: full refund revokes license + activations; partial doesn't; guards reject at all 3 routes | PASS | webhook handler + guards.ts wiring; 3/3 tests green | — | — |
| 6 | Dispute lifecycle (revoke on open, restore on win) | PASS | charge.dispute.created/closed handlers | — | — |
| 6 | 30-day consistency across terms/refund-policy/EULA/checkout/email | PASS | grep consistent | — | — |
| 6 | Price $89 single source (ECHO_PRICE_CENTS) | PASS | derived everywhere | — | — |
| 6 | CAN-SPAM physical address in email | PASS (agent FAIL refuted) | Address present at email.ts:166 (HTML) + :228 (text) — verified directly; agent missed it | NOTE | It's the home apartment → replace per D3 after entity decision |
| 6 | EU immediate-delivery withdrawal acknowledgment at checkout | PASS | custom_text message | — | — |
| 7 | Token exp (400d) + 7d grace + silent re-activation | PASS | activation.ts / activation.rs / main.rs | — | Hand-edit-exp relaunch test = USER-VERIFY |
| 7 | Secrets: none in app source; Ed25519 public key only; no sk_live/sk_test/whsec_ in repos | PASS | scans clean | — | — |
| 7 | Rate limiting on echo endpoints + credentials login | PASS | limiter ids quoted; auth-login limiter present (fixed since 2026-06-11 audit) | — | — |
| 7 | version endpoint: no raw asset URL | PASS | response shape has no downloadUrl | — | — |
| 7 | SHA256SUMS.txt on latest release | FAIL→FIXED | Was absent; **uploaded this session** — hash computed from the actual v1.2.1 asset (`6dbb09f5…`) | CRITICAL→closed | — |
| 7 | Site-quoted hash ≠ release asset hash | **FAIL (OPEN)** | `ECHO_INSTALLER_SHA256` env = `576c16d0…`; actual v1.2.1 asset = `6dbb09f54aeb9c0c6a9fc16aa2673186ed83e7539fdd6f011848ae5273eb9e76` | **CRITICAL** | USER: check the live Vercel env value; if it matches 576c…, the site is telling users to verify against the wrong hash — update env to 6dbb09f5… (or explain/replace the asset if the mismatch is unexpected — supply-chain check) |
| 7 | Code signing / notarization | OPEN | No signing secrets (P0.4); SmartScreen warning honestly documented on site | HIGH | A8 after P0.4 |
| 8 | Terms/privacy/disclaimer/disclosures dates + counsel stamps; Echo carve-out present | PASS | §8.4 carve-out; counsel-review gate now covers refund-policy too | — | — |
| 8 | Account deletion: EchoLicense survives account soft-delete | USER-VERIFY | api/account/delete has no echoLicense handling — consistent with "retention life of license + 3y" but confirm intent | MED | Document the intended behavior |
| 9 | Entity/governing-law/address hygiene | OPEN | Texas law vs no Texas entity; home address in email | — | P0.1/D3 (human) |
| 10 | Cross-platform parity: gate/About/notices platform-neutral; dmg + macOS bundle config present; privacy discloses macOS storage | PASS | component APIs platform-agnostic | — | EULA §1 "Windows devices" = D5 item (counsel-stamped at Mac GA) |

## USER-VERIFY queue
1. **Vercel env `ECHO_INSTALLER_SHA256`** — compare to `6dbb09f5…` (actual asset). Update or investigate. (CRITICAL)
2. **Stripe Dashboard** — Public details ToS URL = `/terms`, Privacy URL set; one test-mode checkout shows the consent checkbox. (Deploy gate for PR #730)
3. **GUI behavioral tests** — EULA gate flow (fresh profile, both platforms), About panel, NSIS installer license page in VM, B10 expired-token relaunch.
4. **Account-deletion ↔ license retention intent** — document.
5. **P0.1–P0.4 + D1 (2026-09-10) + D2–D4** — human filings/decisions per plan.

## Blind-spot sweep
(a) **Surfaces not in scope inventory:** stock-analyzer's GitHub remote is `diablobuster/ALAN_post_integration` (PRs live there); HuggingFace as a third-party download host (now disclosed); local `.env.prod.local` holds production secrets on this workstation (note: outside repo, but on-disk).
(b) **Least-certain absolute claim:** "the app contains no analytics or telemetry" (privacy §10) — supported by the outbound-call inventory (only the 5 enumerated endpoints; no telemetry lib in Cargo.toml), but any future dependency could silently break it; re-run the inventory each release.
(c) **Adversaries:** consumer lawyer → biggest residual exposure is the live EULA's "validated offline" falsehood until PR #731 merges (counsel-gated); pirate → offline HMAC `is_licensed()` accepts a revoked key forever on an already-activated machine (accepted constraint, partially mitigated by 400d token exp; legal trail for keygen response blocked only by the unfiled registration — D1); regulator → privacy story now matches code; DNT gap is the open regulator item.
(d) **World changes since research:** none — research and audit are same-day (2026-06-12). Re-research trigger stands at +6 months or on the copyright-fee NPRM finalizing.
(e) Non-empty answers above are already ledger rows (hash mismatch, DNT, EULA falsehoods, D1).

**Open CRITICAL count: 2** (installer-hash reconciliation; copyright registration deadline). Both require human action; neither blocks merging the code PRs.
