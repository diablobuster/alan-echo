# ALAN Echo — macOS Sales Funnel + EULA (session log)

**Date:** 2026-06-21
**Decision (owner):** ship Mac **unsigned** (no Apple signing). Mac is a **paid** platform — **free trial is Windows-only**, the **Mac download is post-purchase**, with granular self-install (Gatekeeper) instructions. EULA grant widened to cover Mac (user-directed, **not** counsel-reviewed).

## 1. What shipped (on branches — NOT yet deployed)

**stock-analyzer** — branch `feat/echo-mac-funnel-eula` (commit `68c8e8ff`, pushed; tsc --noEmit = 0 errors):
- `app/api/echo/checkout/route.ts` — removed the `isMacBrowser` gate (Mac users can buy); product description → "Windows and macOS".
- `app/api/echo/download-free/route.ts` — **free trial is Windows-only**: a free `?platform=mac` request bounces to `/echo/download` (Mac is paid/post-purchase).
- `app/echo/download/page.tsx` — detects macOS client-side. Mac visitors get **no free download** (a "paid download" notice) and, once they verify a license key, a **Mac `.dmg` download** (`/api/echo/download?key=…&platform=mac`) plus a **six-step install guide** (open the `.dmg` → drag to Applications → right-click → Open / "Open Anyway" → allow Microphone → enable Accessibility → paste key), with a `shasum -a 256` verify tip.
- `app/echo/page.tsx` — Mac notice banner + FAQ ("Does Echo work on Mac?", "Is there a free trial?") now say Mac is available with purchase, trial is Windows-only.
- `app/legal/echo-license/page.tsx` + `lib/settings/disclosures.ts` — §1 grant "Windows devices" → platform-neutral **"devices"**; description → "Windows and macOS"; EFFECTIVE_DATE → June 21, 2026. Recorded in the disclosures.ts audit trail as **USER-DIRECTED, NOT counsel-reviewed**. (echo-license is not one of the four counsel-gated pages, so no CI-gate constant bump was required.)

**alan-echo** — branch `feat/macos-launch-scaffold` (commit `6b0d4bc`, pushed):
- `src/legal/eula.md` + `legal/EULA.txt` — §1 grant "Windows devices" → "devices". **`EULA_VERSION` deliberately NOT bumped** — a grant *widening* is favorable (no re-acceptance needed) and it preserves the batched LLC-formation re-prompt the codebase plans.

## 2. The Mac `.dmg` is hosted and downloadable NOW
- Took the unsigned universal `.dmg` from CI run `27917303491`, renamed to `ALAN.Echo_1.2.3_universal.dmg` (140.9 MB), uploaded it to the **v1.2.3** release on `diablobuster/alan-echo-releases` (alongside the Windows `.exe`; the resolver picks `.dmg` vs `.exe` by extension, so Windows is unaffected).
- **sha256 (served == advertised):** `8f82477c29226988abd2637c9b81f1d9f5382b827aacd61eb4502dc9f1902fe1`
- **Verified live:** `GET https://www.alanglobalintelligence.com/api/echo/download-free?platform=mac` → **302 → signed GitHub asset URL** for the `.dmg`. The production token resolver serves the Mac build today.

## 3. Verification status
- **Tested:** site TypeScript (0 errors); the live resolver serves the Mac `.dmg` (302 → signed URL); the `.dmg` hash computed from the exact uploaded file.
- **NOT tested (needs deploy / hardware):** the new download-page UI live (it's on a branch); the actual Mac *install + run* on real hardware (the `.dmg` is the unsigned CI build — `paste.rs`/Metal/activation are still device-unvalidated per the earlier macOS logs); a real purchase→download round-trip.

## 4. Go-live handoff (remaining steps — deploy is owner/coordination)
1. **Merge + deploy the site branch.** Merge `feat/echo-mac-funnel-eula` → `main` (coordinate with the parallel session on `fix/echo-ship-blockers`) and let Vercel deploy. Until this deploys, the live site still shows the Windows-only page.
2. **(Recommended) set the Mac env in Vercel (Production), then redeploy** so the download page shows the Mac SHA and the in-app Mac updater works (download itself already works without these):
   - `ECHO_MAC_INSTALLER_SHA256` = `8f82477c29226988abd2637c9b81f1d9f5382b827aacd61eb4502dc9f1902fe1`
   - `ECHO_MAC_INSTALLER_VERSION` = `1.2.3`
   - `ECHO_MAC_INSTALLER_MB` = `141`
   - `ECHO_MAC_RELEASE_DATE` = `Jun 21, 2026`
3. **Verify live after deploy:** on a Mac, `/echo/download` shows the paid-Mac notice (no free download); buy → verify key → Mac `.dmg` downloads + the six-step guide renders; `shasum -a 256` of the download == the sha above.
4. **Merge the alan-echo EULA branch** so the next app release ships the Mac-inclusive in-app EULA.

## 5. Known caveats
- **Unsigned install friction is by design** (owner chose no Apple signing): Mac buyers do the one-time right-click→Open / "Open Anyway". The six-step guide + FAQ set that expectation.
- **Temporary pre-deploy exposure:** because the dmg is hosted but the Windows-only gate isn't deployed yet, the *raw* `download-free?platform=mac` URL currently serves the dmg free. No UI links to it, so it's unexposed; the deploy closes it.
- **Mac auto-update** path (`version?platform=mac` → `download-free?platform=mac`) will, post-deploy, redirect to the download page; wiring the in-app Mac updater to the key-gated route is a follow-up (no live Mac app updates yet).
- **EULA change is not counsel-reviewed** (owner-accepted); fold into the next counsel round.
