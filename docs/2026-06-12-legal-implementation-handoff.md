# HANDOFF: Implement the Legal Protection & Ship-Readiness Plan

**Date:** 2026-06-12
**Plan:** `C:\Users\arowm\alan-echo\docs\superpowers\plans\2026-06-12-legal-protection-ship-readiness.md`
**Research basis:** `C:\Users\arowm\alan-echo\docs\2026-06-12-legal-research-copyright-eula.md`
**Priority:** Wave 1 is shippable immediately; D1 has a hard deadline (2026-09-10).

---

## PROMPT (paste into a new session, working directory `C:\Users\arowm\alan-echo`)

```
YOU ARE THE LEGAL-IMPLEMENTATION TERMINAL.

## Mission

Execute the implementation plan at
docs/superpowers/plans/2026-06-12-legal-protection-ship-readiness.md
task-by-task. The plan is complete and self-contained — every task has exact
files, code, verification commands, and commit messages. Do not redesign it;
implement it. Where the plan says "verify against the actual file" (two marked
read-then-insert steps in B8/B10), read the anchor it names first.

Use the superpowers:executing-plans skill (or subagent-driven-development if
dispatching task subagents). Track progress by checking the plan's checkboxes
as you complete steps — edit the plan file itself so progress survives the
session.

## Repos

- App (Tauri 2, Rust + React/JSX): C:\Users\arowm\alan-echo — branch
  legal/eula-gate-and-notices off main. NOTE: main.rs may have uncommitted
  work — stash/commit it first per the plan's Workstream A preamble.
- Website (Next.js + vitest): C:\Users\arowm\stock-analyzer — branch
  legal/echo-consent-and-claims off main.
- Releases repo: diablobuster/alan-echo-releases (private) — via gh CLI only.

## Execution order

1. Workstream A tasks A1→A7, A9 (skip A8 unless signing secrets exist — check
   `gh secret list --repo <org>/alan-echo`).
2. Workstream B tasks B1, B2, B4, B5, B6, B7, B9 (B2 Step 1 is a Stripe
   Dashboard action — if you don't have dashboard access, implement the code
   and list the dashboard step in your final report as USER ACTION).
3. Workstream B tasks B8, B10 (hardening — has the only cross-repo step).
4. Workstream C tasks C1, C2.
5. STOP before B3 Step 4, B11 Step 2, and all of Workstream D unless
   docs/decisions/2026-06-legal-entity.md exists in stock-analyzer (the Phase 0
   decision record). B3 Steps 1–3 and 5–7 (decision-independent EULA additions)
   MAY be implemented now but the PR must be labeled "HOLD FOR COUNSEL — do not
   merge until P0.3 sign-off".

## Hard rules

- Every app-facing change targets Windows AND macOS simultaneously (project
  memory rule). The EULA gate, About panel, and notices are platform-neutral;
  verify nothing you write is Windows-only without a Mac equivalent.
- Never weaken the existing fail-open posture: a EULA/licensing error must
  never brick a paying user (see main.jsx comments — preserve them).
- Legal TEXT changes (EULA sections, privacy policy, terms carve-out) must be
  implemented VERBATIM from the plan — they are drafts for counsel; do not
  paraphrase, "improve", or expand them.
- stock-analyzer merges to main: run the vercel-pre-deploy-check skill first.
  Do not push to main without it.
- Do not commit secrets, the Stripe keys, signing credentials, or any deposit
  source-code PDFs to any repo.
- Commit per task with the plan's commit messages. Open one PR per workstream
  branch with a checklist of completed task IDs in the description.

## Verification protocol (Definition of Done for this session)

- alan-echo: `npm run build` green; `cargo check` green in src-tauri;
  `npm run tauri dev` fresh-profile behavioral test passes: EULA gate shows →
  Decline quits → relaunch shows gate → Accept persists → license/trial gate
  next → relaunch skips EULA gate → version-bump re-prompts (then revert).
- stock-analyzer: `npm test` green (including the new tests/echo/guards.test.ts),
  `npm run build` green, claim-greps from B5 return clean.
- Releases repo: README visible via
  `gh api repos/diablobuster/alan-echo-releases/contents/README.md --jq .name`.
- Produce a session log at docs/YYYY-MM-DD-legal-implementation-session-log.md
  (alan-echo repo) listing: tasks completed (by ID), tasks blocked + why,
  USER ACTIONS required (Stripe dashboard ToS URL, P0 decisions, signing
  procurement, counsel engagement, D1 filing by 2026-09-10), and verification
  evidence (command outputs).

## What you must NOT do

- Do not file anything with the Copyright Office, USPTO, or any state — those
  are human tasks (Workstream D).
- Do not change pricing, refund duration, or the substance of any legal
  position beyond the plan's verbatim drafts.
- Do not enable email sending (B7 fixes copy only; re-enabling delivery is a
  separate product decision).
- Do not merge the B3 EULA PR — counsel reviews it first.

When done, run the audit prompt at
docs/2026-06-12-legal-audit-prompt.md against your own work and append its
findings ledger to the session log.
```

---

## Notes for the human

- Wave 1 needs no decisions and no money — it can run tonight.
- Your four parallel human tasks while it runs: (1) Stripe Dashboard → Settings → Business → set Terms URL to the EULA page; (2) Phase 0 decisions P0.1/P0.2 written into `stock-analyzer/docs/decisions/2026-06-legal-entity.md`; (3) engage counsel per P0.3 (the four-question memo); (4) start signing procurement per P0.4.
- The copyright registration deadline (**2026-09-10**) is the only date that can't slip without losing rights (statutory damages retroactive to first publication).
