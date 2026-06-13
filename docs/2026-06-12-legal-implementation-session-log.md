# Legal Protection & Ship-Readiness — Implementation Session Log

**Date:** 2026-06-12
**Plan:** `docs/superpowers/plans/2026-06-12-legal-protection-ship-readiness.md`
**Branches:**
- alan-echo: `legal/eula-gate-and-notices` (off main)
- stock-analyzer: `legal/echo-consent-and-claims` (off main); B3 on child branch `legal/echo-eula-v2-HOLD` (HOLD FOR COUNSEL)
- releases repo: README updated via GitHub API (no branch)

---

## Tasks completed (by ID)

| ID | Status | Notes |
|---|---|---|
| A1 | DONE | `src/legal/eula.md` (11 sections verified) + `eulaVersion.js` (2026-06-10). Preamble ("By installing or using…you agree") included — it is the agreement-formation sentence the clickwrap shows. |
| A2 | DONE | `quit_app` command + handler registration; `cargo check` green. |
| A3 | DONE | `EulaGate.jsx` + scoped `.eula-gate` styles in tokens.css (btn classes did not exist; styled to match LicenseGate's brass/bordered idiom). Platform-neutral. |
| A4 | DONE (code) | `main.jsx` boot gate: EULA → license/trial → splash. Fail-open comments preserved. **Behavioral click-through test = USER ACTION** (GUI). |
| A5 | DONE | `copyright` + `licenseFile` in tauri.conf.json; `legal/EULA.txt` generator; Cargo.toml `license = "LicenseRef-Proprietary"`; package.json license. Full `npx tauri build` green; generated NSIS script contains `MUI_PAGE_LICENSE` and the staged `license_file` is the EULA. **Running the installer in a VM = USER ACTION.** |
| A6 | DONE | `gen-notices.mjs` (cargo-license + license-checker-rseidelsohn); 219 KB notices file mentions whisper.cpp/tauri/react; `gen:legal` npm script. |
| A7 | DONE (code) | About section in SettingsPanel (version, ©, EULA/privacy links via system browser, OSS-notices viewer); FooterBar shows `© 2026 ALAN · Echo v{pkg.version}`. **Visual check in `npm run tauri dev` = USER ACTION.** |
| A8 | SKIPPED | `gh secret list --repo diablobuster/alan-echo` returned no signing secrets → blocked on P0.4 procurement. |
| A9 | DONE | Proprietary LICENSE; real README with release legal checklist (incl. checksums + release-notes legal line). |
| B1 | DONE | Terms §8.4: blanket non-refundable sentence replaced with the plan's verbatim carve-out (Echo 30-day guarantee controls). |
| B2 | DONE (code) | `consent_collection.terms_of_service: required` + verbatim `custom_text` EU/UK message in checkout; `.cta-legal` microcopy under DualCta, CheckoutCta, and the trial download button. **Stripe Dashboard confirmation = USER ACTION (deploy-blocking, see below).** |
| B3 (1–3, 5–7) | DONE on HOLD branch | See `legal/echo-eula-v2-HOLD`. Step 4 (governing law/arbitration) NOT touched — blocked on P0.1/P0.2. PR labeled HOLD FOR COUNSEL. |
| B4 | DONE | Privacy §10 rewritten with the plan's verbatim 4-paragraph Echo text (replaced the false "activation involves no server communication" claim); Plausible cookieless line added to §7; kept the accurate website-purchase paragraph. |
| B5 | DONE | Landing privacy paragraph replaced verbatim; airplane-mode scope sentence appended; FAQ "no network calls"/"activates offline" claims fixed; languages aligned to ground truth (English + 9 beta via multilingual download) on landing + compare; "Mac coming soon" → "macOS version in development"; `internet` cell scoped to "Not required for dictation". All claim-greps clean. |
| B6 | DONE | Footer: Echo License + Refunds links added (4 legal links total). |
| B7 | DONE | success/recover pages were already truthful (prior session) — verified by grep. Email template: EULA-link footer added (verbatim); false "activates offline"/"works with Wi-Fi off" claims fixed (template-only; sending stays disabled). |
| B8 | DONE | TDD: `tests/echo/guards.test.ts` (fail → pass, 3 tests) → `lib/echo/guards.ts` → wired into download (kept redirect UX), validate-key (kept `{valid}` shape + surfaced refund-safe message in the unlock form), activate (guard status codes). Webhook `charge.refunded` already revoked the license (prior session); added EchoActivation revocation on full refund. Dispute open/close lifecycle already present. |
| B9 | ALREADY SATISFIED | Route returns no `downloadUrl` at all (prior hardening). Verified `updater.rs` Option-compatibility + gated key-URL injection in `main.rs`. Implementing the plan's key-less URL would regress (302→HTML fails the updater's SHA-256 check). No change. |
| B10 | DONE | 400-day `exp` claim website-side; app honors `exp` with 7-day grace in `verify_token`; `check_license` spawns background silent re-activation with the saved key (never blocks/bricks — `is_licensed()` short-circuit preserved). `cargo check` green. **Hand-edit-exp relaunch test = USER ACTION.** Note: these edits were committed by the parallel session (df26835 app / 1cee63eb website) which swept the working tree mid-task; content verified identical to what was written. |
| B11 Step 1 | DONE | `app/refund-policy/page.tsx` added to the counsel-review CI workflow (echo-license was already covered by `app/legal/**`); disclosures.ts annotated with an explicit DRAFTS-PENDING-COUNSEL note (the gate's documented waiver path) — this also satisfies the gate for this PR's B1/B4 edits. Step 2 blocked on P0.3. |
| C1 | DONE | Releases-repo README updated via `gh api` (merged plan content + existing links; sha-guarded PUT). Verified: `…/contents/README.md --jq .name` → `README.md`. |
| C2 | DONE | `scripts/release-checksums.ps1`; README checklist already carries checksum + release-notes-legal lines (A9). |

## Tasks blocked / not executed

- **A8** — no signing credentials (P0.4). Config snippets are in the plan, ready once secrets exist.
- **B3 Step 4** — governing law / arbitration replacement: blocked on P0.1 + P0.2 (no `docs/decisions/2026-06-legal-entity.md` exists).
- **B11 Step 2** — refund-policy "pending counsel review" header removal: blocked on P0.3 sign-off.
- **Workstream D (D1–D5)** — human filings (copyright, trademark, entity, insurance, Mac launch). **D1 hard deadline: application received by 2026-09-10.**

## USER ACTIONS required

1. **Stripe Dashboard (deploy-blocking for B2):** Settings → Business → Public details — confirm the Terms of Service URL is the site-wide `/terms` (do NOT point it at the Echo EULA; it is account-wide) and set the Privacy Policy URL if empty. `consent_collection.terms_of_service: "required"` **fails checkout-session creation if the account ToS URL is empty** — verify before merging the B2 branch. Then run one test-mode checkout to see the consent checkbox + custom message.
2. **Phase 0 decisions:** P0.1 (entity/governing law) + P0.2 (arbitration keep/drop) → write `stock-analyzer/docs/decisions/2026-06-legal-entity.md`. Unblocks B3 Step 4, D3.
3. **Counsel engagement (P0.3):** four-question memo (EULA v2 review — point counsel at the HOLD PR; AI-authorship disclosure for the copyright filing; "ALAN Echo" trademark clearance vs Amazon ECHO; EU sales screen). Unblocks B3 finalization, B11 Step 2, D1 filing strategy.
4. **Signing procurement (P0.4):** Azure Trusted Signing (or OV cert) + Apple Developer Program; store secrets as GitHub Actions secrets. Unblocks A8.
5. **D1 copyright filing:** by **2026-09-10** (calendar Aug 15 + Sep 1 reminders).
6. **Manual app verification (GUI):** fresh-profile `npm run tauri dev` behavioral test — EULA gate → Decline quits → relaunch shows gate → Accept persists (`eula_accepted_version` in settings.json) → license/trial gate → relaunch skips → version-bump re-prompts (then revert). Repeat on macOS per the dual-platform rule. Also: run the built NSIS installer in a VM to see the license page; B10 hand-edit-exp relaunch test.
7. **Pre-existing test-env failures (not from this session):** 113 failing vitest files on BOTH main and the legal branch (verified identical sets; zero new failures). Largest cause: `ECHO_HMAC_SECRET` exists only in `.env.prod.local`, so `lib/echo/issue.test.ts`/`license.test.ts` throw at env lookup; others are DB/env-dependent. Decide: load test env vars in vitest config or CI, or accept local-red. (Also: vitest was collecting duplicated suites from `.claude/worktrees/**` scratch copies — excluded in vitest.config.ts this session, which removed ~213 phantom failures.)
8. **CalOPPA gap flagged for counsel:** the privacy policy has no do-not-track-response disclosure; the plan contained no verbatim draft for it, so none was added (per the no-improvising rule on legal text).

## Verification evidence

- alan-echo `npm run build` (vite): ✓ green (multiple runs; final bundle includes EULA + notices).
- alan-echo `cargo check`: ✓ `Finished dev profile` (after A2 and after B10).
- alan-echo `npx tauri build`: ✓ `Finished 1 bundle at: …\nsis\ALAN Echo_1.2.1_x64-setup.exe`; `installer.nsi` contains `MUI_PAGE_LICENSE "${LICENSE}"`; staged `license_file` starts with "ALAN Echo License Agreement / Effective: June 10, 2026".
- eula.md structure: `Select-String '^## \d+\.'` count = **11** ✓.
- Notices spot-check: whisper.cpp (line 5), tauri, react present ✓.
- stock-analyzer `npx vitest run tests/echo/guards.test.ts`: ✗ fail (module not found) before → ✓ **3 passed** after.
- Full-suite regression attribution: failing-file list on main vs branch — **identical (113/113), 0 new, 0 fixed** (env-dependent failures pre-exist; see USER ACTION 7).
- B5 claim greps: `no network calls` → CLEAN; `english-only` → CLEAN; `mac coming` → CLEAN.
- B7 grep: `emailed` in success/recover → CLEAN; `activates offline|Wi-Fi off` in email.ts → CLEAN.
- C1: `gh api repos/diablobuster/alan-echo-releases/contents/README.md --jq .name` → `README.md` ✓.
- stock-analyzer `next build`: see addendum below (run via `prisma generate && next build --webpack` to avoid `migrate deploy` against the configured database).

## Commits

**alan-echo `legal/eula-gate-and-notices`** (14 commits off main):
`2cac08b` pre-existing platform fixes folded in · `fbef9d8` legal docs/plan · `3b57799` A1 EULA bundle · `753e0c0` A2 quit_app · `62aa9c7` A3 EulaGate · `e62b3ce` A4 boot gate · `d5e042e` A5 copyright+installer · `922c03f` A6 notices · `0b90f1d` A7 About · `4eed388` A9 LICENSE/README · `b55b35d`+`df26835` parallel-session commits (debug journal; B10 app-side swept from this session's working tree) · `9b00b98` C2 checksums · `38667fb` plan progress.

**stock-analyzer `legal/echo-consent-and-claims`** (9 commits off main):
`77a1e3be` B1 terms carve-out · `4fd8ade1` B2 checkout consent+microcopy · `19c7926d` B4 privacy Echo section · `559008d0` B5 claims accuracy · `3ad37a02` B6 footer links · `ec739815` B7 email template truth · `d40b5e67` B8 guards+webhook · `1cee63eb` B10 website-side (parallel-session commit of this session's edit) · `507de9be` B11.1 CI gate.

**Releases repo:** README commit `518965a` via gh API.

**stock-analyzer `legal/echo-eula-v2-HOLD`** (2 commits off the consent branch): `58aa9af9` B3 EULA v2 (steps 1–3, 5–7 verbatim; §10 untouched) · `c33ac9db` merge of hero-claim fixes. Plus on the consent branch post-audit: `93a24435` hero-claims fix ("Nothing leaves" / "0 Network calls" scoped to dictation truth).

## Pull requests

- **alan-echo PR #1** — Workstreams A + C: https://github.com/diablobuster/alan-echo/pull/1
- **stock-analyzer PR #730** — Workstream B Wave 1 + hardening: https://github.com/diablobuster/ALAN_post_integration/pull/730 (⚠️ deploy gates: Stripe ToS-URL confirmation + vercel-pre-deploy-check before merge)
- **stock-analyzer PR #731** — **HOLD FOR COUNSEL** EULA v2: https://github.com/diablobuster/ALAN_post_integration/pull/731 (do not merge until P0.3; carries 3 counsel flags incl. the "validated offline" sentence and the 5-machine-cap wording conflict)

## Final verification (post-B3)

- stock-analyzer `npx prisma generate && npx next build --webpack`: ✓ exit 0 on the consent branch, and ✓ exit 0 again on the HOLD branch (superset of both).
- v1.2.1 release: `SHA256SUMS.txt` computed from the **actual released asset** and attached (closing an audit CRITICAL). Asset hash: `6dbb09f5…9e76`.
- ⚠️ **Open CRITICAL:** the site env `ECHO_INSTALLER_SHA256` (`576c16d0…2c44` in .env.prod.local) does NOT match the actual v1.2.1 asset — users following the verify-the-hash instructions today get a mismatch. USER ACTION: check the live Vercel value, update to `6dbb09f5…` or investigate why the asset and the recorded hash diverged.

## Audit (per handoff: audit prompt run against this session's work)

Full report: `docs/2026-06-12-legal-completeness-audit.md` — **Verdict: SHIP-READY WITH USER ACTIONS; open CRITICALs: 2** (installer-hash reconciliation above; copyright registration deadline 2026-09-10). Method: 3 parallel read-only evidence agents + primary re-verification of every FAIL. Two audit findings were closed in-session (SHA256SUMS upload; hero absolute claims fixed in `93a24435`); one agent CRITICAL was **refuted** on re-check (CAN-SPAM address IS present in the email template — it's the home-apartment/D3 issue, not a missing-address issue). Open HIGHs are counsel-text items flagged on PR #731 (live EULA "validated offline" falsehood; "any number of machines" vs 5-machine cap) plus the missing CalOPPA DNT sentence (no verbatim draft existed; not improvised).

## Session notes / deviations

- **Parallel-session interleaving:** a second Claude session was active in both working trees during this run. It committed this session's uncommitted B10 edits (app `df26835`, website `1cee63eb`) and the debug journal (`b55b35d`) under its own messages, and fixed the handoff/plan B2 Step 1 text (Stripe ToS URL correction — reviewed and adopted). Content verified; no work lost.
- **Stash to restore on main (stock-analyzer):** `stash@{1}` "pre-legal-session WIP: settings sign-out UI, testimonial copy, /account redirect, activate CSRF exemption (restore on main)" — pre-existing uncommitted work moved aside so legal commits stayed clean. Restore with `git checkout main && git stash pop` (note: the stashed middleware hunk adds `/api/echo/activate` to CSRF-exempt prefixes — that looks load-bearing for the app's activation POSTs; consider landing it properly).
- alan-echo pre-existing working-tree changes (platform-neutral messages, www URLs) were folded into the branch's first commit, as the plan preamble allowed.
- B4: the old privacy §10 contained a `nvidia-smi` GPU-detection disclosure that the plan's verbatim replacement does not carry — flag to counsel if they want it restored.
- B5: two additional false claims found and fixed beyond the plan's named lines (FAQ "makes no network calls", FAQ/email "activates offline") — same finding-#7 class, scoped minimally.
- `npm run tauri` script did not exist; added `"tauri": "tauri"` so the plan's/README's documented commands work.
