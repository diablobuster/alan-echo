# ALAN Echo — NEXT STEPS (resume here)

**Last worked:** 2026-06-21. **Owner decision:** ship Mac **unsigned** (no Apple Developer signing) — buyers self-install with guided Gatekeeper steps; **free trial is Windows-only**; **Mac download is post-purchase**.

This doc = the single "start here" to resume. Detailed logs: `docs/2026-06-21-*-session-log.md` (four of them).

---

## ✅ DO THESE IN ORDER

### 1. Smoke-test the Mac APP (the real unknown — has never run on a Mac) — needs a Mac
1. On a Mac, open: `https://www.alanglobalintelligence.com/api/echo/download-free?platform=mac`
   → downloads `ALAN.Echo_1.2.3_universal.dmg` (the exact build buyers get). *(This URL serves it free right now because the Windows-only gate isn't deployed yet; after go-live it becomes the key-gated post-purchase download.)*
2. Open `.dmg` → drag to Applications → **right-click → Open** → "Open Anyway" if prompted → **Allow** Microphone → enable **Accessibility** (System Settings → Privacy & Security → Accessibility).
3. Confirm: launches, dictation transcribes, auto-paste works, feels GPU-fast (Metal), license-key activation works.
   - ⚠️ This build does NOT include the new **multilingual** feature (separate branch) — see step 5.
   - If bugs appear, the assistant fixes them on `feat/macos-launch-scaffold`, re-runs the Mac CI build, re-uploads the `.dmg`.

### 2. Smoke-test the site funnel UI (preview) — sign into Vercel first
- Open: `https://alanintelligence-jmd3pqgwh-alanglobalintelligence.vercel.app/echo/download`
- On a Mac you should see: the **"paid download" notice (no free button)**; verifying a license key reveals the **Mac download + six-step install guide**. The EULA page (`/legal/echo-license`) now reads **"devices"** (not "Windows devices").
- (Preview is behind Vercel login — your account. A real Stripe purchase only works in production.)

### 3. GO LIVE (when 1 & 2 look good) — production deploy of the funnel
- Tell the assistant **"go live"** (it will deploy + verify), **or** do it manually:
  ```
  cd stock-analyzer && git checkout feat/echo-mac-funnel-eula && vercel --prod --yes
  ```
  (Production `ECHO_MAC_*` env is already set, so the page shows the Mac SHA and the updater works.)
- ⚠️ Note: the funnel commit also edits `lib/settings/disclosures.ts` (a legal audit comment). If you merge to `main` via PR, the **legal counsel-review CI gate** may flag it — either deploy directly with `vercel --prod` (bypasses the gate) or drop that one comment. echo-license itself is NOT a gated page.
- After deploy, verify on a Mac: `/echo/download` shows the paid-Mac flow; buy (or $0 promo) → key → `.dmg` downloads.

### 4. Merge the app branches to `main` + cut a release
- `feat/macos-launch-scaffold` (macOS launch + in-app EULA "devices") → main.
- `feat/multilingual-dictation` (multilingual via large-v3-turbo) → main.
- Both touch `src/components/SettingsPanel.jsx` in different spots — land in sequence, expect a trivial/no conflict.
- Cut a new tagged release so Windows + Mac ship these features.

### 5. (Optional) Combined Mac build incl. multilingual
- The hosted Mac `.dmg` has Metal + native paste + resource fix, but NOT multilingual. To test/ship multilingual on Mac: merge `feat/multilingual-dictation` into `feat/macos-launch-scaffold`, re-run the Mac CI build (`gh workflow run "Build macOS" --ref <branch>`), re-upload the new `.dmg` to the v1.2.3 release. Same for a new Windows build with multilingual.

---

## 📍 CURRENT STATE (what's where)

**Branches (all pushed):**
| Repo | Branch | Contains |
|---|---|---|
| alan-echo | `feat/macos-launch-scaffold` | macOS signing CI, universal2, Metal, native paste, resource-resolution fix, in-app EULA "devices" |
| alan-echo | `feat/multilingual-dictation` | Multilingual dictation (large-v3-turbo), no model names in UI |
| stock-analyzer | `feat/echo-mac-funnel-eula` | Mac sales funnel (ungate checkout, free=Windows, post-purchase Mac download + install guide) + site EULA "devices" |

**Live / hosted:**
- **Mac `.dmg` hosted + downloadable now**: `diablobuster/alan-echo-releases` release **v1.2.3**, asset `ALAN.Echo_1.2.3_universal.dmg`. SHA-256 `8f82477c29226988abd2637c9b81f1d9f5382b827aacd61eb4502dc9f1902fe1` (served == advertised). Verified: prod resolver 302s to the signed asset.
- **Mac CI build**: GitHub Actions "Build macOS" run `27916919377`→fix→`27917303491` (GREEN, unsigned universal). Proves the code (incl. native `paste.rs`) compiles + the universal Metal whisper-server builds on real macOS.
- **Vercel** (project `alan_intelligence`, team `alanglobalintelligence`): **Production** `ECHO_MAC_INSTALLER_SHA256/VERSION/MB/RELEASE_DATE` set (inert until prod deploy). **Preview** env branch-scoped to `feat/echo-mac-funnel-eula` (incl. `ECHO_RELEASE_TAG=v1.2.3`). Preview deploy: the URL in step 2.
- **Live production site is UNCHANGED** (funnel still on a branch). The only prod change so far: env vars (no effect until deploy) + the `.dmg` added to the release.

**Decisions locked:** No Apple signing (unsigned + guided install). EULA grant widened to platform-neutral "devices" — **USER-DIRECTED, NOT counsel-reviewed**; in-app `EULA_VERSION` deliberately NOT bumped (favorable widening, preserves the batched LLC re-prompt).

---

## 🔭 REMAINING WORK (beyond go-live)

- **EULA → counsel** on the next round (the "devices" widening + the still-pending v2/LLC re-acceptance batch).
- **Apple signing** is OFF by choice. If you ever reconsider: enroll → add `APPLE_*` secrets → re-run "Build macOS" (already wired to sign+notarize+staple) → removes the install friction + enables Gatekeeper-clean downloads + Mac auto-update.
- **Mac auto-update** wiring (in-app updater → key-gated download) — small follow-up; no live Mac updates yet.
- **Site copy**: marketing still says "9 beta languages" — update when multilingual ships.
- **Windows**: re-confirm fully GO (the 2026-06-20 audit was NO-GO; the download now resolves + ship-audit fixes landed).
- **Cosmetic**: quality segment shows "Ultra" when multilingual is active; pin `WHISPER_CPP_REF` before the next release build.
- Open from memory: copyright filing by 2026-09-10.

---

## 🔑 QUICK REFERENCE
- Mac `.dmg` (live download): `https://www.alanglobalintelligence.com/api/echo/download-free?platform=mac`
- Preview (Vercel login): `https://alanintelligence-jmd3pqgwh-alanglobalintelligence.vercel.app`
- Mac `.dmg` SHA-256: `8f82477c29226988abd2637c9b81f1d9f5382b827aacd61eb4502dc9f1902fe1`
- Release host: `diablobuster/alan-echo-releases` @ `v1.2.3`
- Mac CI: `gh run view 27917303491 -R diablobuster/alan-echo`
- **Resume with the assistant:** "read docs/2026-06-21-NEXT-STEPS-mac-launch.md and continue."
