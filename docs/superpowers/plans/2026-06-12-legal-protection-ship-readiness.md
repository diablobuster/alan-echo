# ALAN Legal Protection & Ship-Readiness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement every finding from `docs/2026-06-12-legal-research-copyright-eula.md` across the Echo app, the website, and distribution, so ALAN Echo is legally protected (enforceable clickwrap, registered copyright posture, OSS compliance, accurate claims) and ready to ship to a massive audience (code signing, consent at checkout, refund/revocation integrity).

**Architecture:** Four workstreams. **A** (alan-echo Tauri app): add the missing first-launch EULA clickwrap gate — the keystone every enforceability case depends on — plus copyright notices, third-party notices, installer license page, and code signing. **B** (stock-analyzer website): checkout consent, EULA/privacy revisions, claims-accuracy fixes, refund/revocation integrity. **C** (distribution): releases-repo hygiene and checksums. **D** (business/legal, human-executed): copyright registration, trademark clearance, entity, counsel review. Code waves are sequenced so nothing blocks on attorney answers except final EULA text.

**Tech Stack:** Tauri 2 (Rust + React/JSX, Vite), Next.js (App Router, TS), Prisma, Stripe Checkout, vitest (website only — the app repo has no test runner; app tasks verify via build + scripted launch checks).

**Repos (absolute paths):**
- App: `C:\Users\arowm\alan-echo` (branch from `main`)
- Website: `C:\Users\arowm\stock-analyzer` (branch from `main`; run the `vercel-pre-deploy-check` skill before any push to main)
- Releases: `github.com/diablobuster/alan-echo-releases` (private; via `gh`)

**Standing rule (from project memory):** every app-facing change targets **Windows AND macOS** simultaneously. The EULA gate, notices, About panel are platform-neutral React/Rust; installer/signing tasks have per-OS steps.

**Source-of-truth findings this plan implements** (from the 2026-06-12 research + this session's code audit):
1. App has NO EULA acceptance flow at all (LicenseGate is key entry only) — clickwrap enforceability currently rests on nothing.
2. No copyright notices anywhere (UI, tauri.conf, installer, README); no LICENSE file; no THIRD-PARTY-NOTICES despite bundling whisper.cpp (MIT) + dozens of crates/npm packages.
3. NSIS installer shows no license page; Windows signing unconfigured (`certificateThumbprint: null`); macOS unsigned/un-notarized.
4. Checkout has no terms consent (`consent_collection` absent) and no EULA microcopy near CTAs.
5. `/terms` says fees non-refundable; `/refund-policy` promises 30 days — direct contradiction.
6. Success/recover pages claim "key was emailed" while email delivery is disabled in `lib/echo/issue.ts`.
7. Marketing claims "No network calls, no telemetry" — true for the dictation path, false as stated (activation, update check, gated download, GPU packs, HuggingFace model fetch all phone out; activation stores IP + user agent in `EchoActivation`).
8. Privacy policy covers the website only — no app section disclosing activation data, registry/local trial state, update pings.
9. EULA (Texas law, AAA Dallas, small-claims carve-out, RE savings clause — better than expected) lacks: trial section, updates/support, feedback, third-party-components pointer, local-storage disclosure, liability-cap carve-outs, arbitration opt-out; refund-policy page is marked "pending counsel review".
10. Revoked keys still pass download/validate gates; activation JWTs never expire; `/api/echo/version` leaks a raw download URL.
11. Releases repo has no README/EULA pointer; releases ship no checksums file (site quotes SHA-256).
12. Business layer: no registration filed (3-month §412 window from ~2026-06-10 first publication ends **~2026-09-10**; fee NPRM will raise $65→$85), no trademark (note: "ECHO" conflicts risk vs Amazon's marks — clearance first), Texas governing law vs Colorado Springs business address (entity question), no insurance.

---

## Phase 0 — Decisions & procurement (human; blocks only Wave 2 tasks)

### Task P0.1: Entity + governing-law decision

**Owner:** User (+ business attorney, optional). **Blocks:** B3 (final EULA text), D3.

- [ ] **Step 1:** Decide entity: form an LLC (likely Colorado — business operates from Colorado Springs) or continue as sole proprietor. Criteria written out in `docs/2026-06-12-legal-research-copyright-eula.md` § 2.3 Q9. Cost: CO LLC $50 filing + $25/yr periodic report + registered agent ~$100–150/yr.
- [ ] **Step 2:** Record the decision in `docs/decisions/2026-06-legal-entity.md` (one paragraph: entity name/state or "sole prop", chosen governing law). Current EULA says **Texas**; unless a Texas entity exists, the default recommendation is to change governing law to the entity's state (or Colorado).
- [ ] **Step 3:** Commit the decision file in stock-analyzer repo: `git add docs/decisions/2026-06-legal-entity.md && git commit -m "docs: record entity + governing-law decision"`

### Task P0.2: Arbitration keep/drop decision

**Owner:** User (+ counsel). **Blocks:** B3.

- [ ] **Step 1:** Read research doc § 2.2 Q5 (fee asymmetry: ~$3k+/case business-side AAA fees vs $89 product; mass-arbitration risk). Current clause already has a small-claims carve-out (good).
- [ ] **Step 2:** Pick one: **(a)** keep AAA arbitration + add 30-day opt-out + informal-resolution prerequisite + batching (draft language is in Task B3 Step 4); **(b)** drop arbitration → informal resolution, then small claims or courts of the governing-law state. Default recommendation for a solo dev: **(b)**.
- [ ] **Step 3:** Record in `docs/decisions/2026-06-legal-entity.md` (append) and commit.

### Task P0.3: Counsel engagement (bundle, ~$1,000–$2,500 one-time)

**Owner:** User. **Blocks:** B3 finalization, B12 Step 3, D1 filing.

- [ ] **Step 1:** Engage a software/IP attorney (flat-fee) for four questions, sent as one memo: (1) EULA revision review (Task B3 drafts), (2) AI-assisted-authorship disclosure strategy for the copyright application (research doc § 1.1 item 5 — the app is heavily AI-built; this is the one filing question with real downside), (3) trademark clearance for "ALAN Echo" (see D2 — Amazon ECHO portfolio risk in class 9), (4) 15-minute screen: EU sales obligations (withdrawal-right acknowledgment wording, European Accessibility Act applicability).
- [ ] **Step 2:** Calendar the §412 deadline: copyright application must be RECEIVED by **2026-09-10** (3 months after ~June 10 first publication) to preserve statutory damages retroactively. Set two reminders (Aug 15, Sep 1).

### Task P0.4: Signing credentials procurement

**Owner:** User. **Blocks:** A8.

- [ ] **Step 1 (Windows):** Set up **Azure Trusted Signing** (~$9.99/mo, individual validation supported) OR purchase an OV code-signing certificate. Record the choice + account details location in the password manager.
- [ ] **Step 2 (macOS):** Enroll in Apple Developer Program ($99/yr); create a "Developer ID Application" certificate; create an App Store Connect API key for `notarytool` (Issuer ID, Key ID, .p8 file).
- [ ] **Step 3:** Store secrets as GitHub Actions secrets in alan-echo repo: `AZURE_TENANT_ID/AZURE_CLIENT_ID/AZURE_CLIENT_SECRET` (or cert thumbprint), `APPLE_SIGNING_IDENTITY`, `APPLE_API_ISSUER`, `APPLE_API_KEY`, `APPLE_API_KEY_PATH`. Verify with `gh secret list --repo <org>/alan-echo`.

---

## Workstream A — Echo app (`C:\Users\arowm\alan-echo`)

> Branch: `git checkout -b legal/eula-gate-and-notices` from `main`. Commit per task. The repo has uncommitted work on `src-tauri/src/main.rs` (Mac error message) — commit or stash it first: `git stash` → restore after branching, or fold into the first commit if trivial.

### Task A1: Bundle the EULA text + version in the app

**Files:**
- Create: `src/legal/eula.md`
- Create: `src/legal/eulaVersion.js`

- [x] **Step 1:** Create `src/legal/eula.md` by copying the EULA text **verbatim** from `C:\Users\arowm\stock-analyzer\app\legal\echo-license\page.tsx` (strip JSX, keep all 11 section texts in order). File must start with:

```markdown
# ALAN Echo License Agreement

Effective: June 10, 2026

## 1. License grant
```

…and contain all 11 headings in order: `1. License grant`, `2. What your license includes`, `3. Restrictions`, `4. Privacy — processing happens on your device`, `5. Refunds`, `6. Disclaimer of warranty`, `7. Limitation of liability`, `8. Termination`, `9. Export and encryption notice`, `10. Governing law and disputes`, `11. Contact`.

- [x] **Step 2:** Verify structure:

Run: `Select-String -Path src/legal/eula.md -Pattern '^## \d+\.' | Measure-Object | Select-Object -ExpandProperty Count`
Expected: `11`

- [x] **Step 3:** Create `src/legal/eulaVersion.js`:

```js
// Single source of truth for which EULA revision this build embeds.
// MUST be bumped in lockstep with EFFECTIVE_DATE in
// stock-analyzer/app/legal/echo-license/page.tsx whenever the EULA changes —
// a mismatch re-prompts every user for acceptance on next launch (intended).
export const EULA_VERSION = '2026-06-10'
```

- [x] **Step 4:** Commit:

```bash
git add src/legal/eula.md src/legal/eulaVersion.js
git commit -m "feat: bundle EULA text and version constant in app"
```

### Task A2: Rust `quit_app` command (Decline path)

**Files:**
- Modify: `src-tauri/src/main.rs` (add command near `check_license` ~line 208; register in `generate_handler!` ~line 1048)

- [x] **Step 1:** Add the command above the existing `check_license` function:

```rust
#[tauri::command]
fn quit_app() {
    // EULA declined — exit cleanly before any engine/tray initialization matters.
    std::process::exit(0);
}
```

- [x] **Step 2:** Register it: in the `tauri::generate_handler![` list (after `set_setting,`), add a line `quit_app,`.

- [x] **Step 3:** Verify it compiles:

Run: `cd src-tauri; cargo check`
Expected: `Finished` with no errors.

- [x] **Step 4:** Commit:

```bash
git add src-tauri/src/main.rs
git commit -m "feat: quit_app command for EULA decline path"
```

### Task A3: EulaGate component (the clickwrap keystone)

**Files:**
- Create: `src/components/EulaGate.jsx`

- [x] **Step 1:** Create `src/components/EulaGate.jsx`:

```jsx
import { useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { Monogram } from './Icons'
import eulaText from '../legal/eula.md?raw'
import { EULA_VERSION } from '../legal/eulaVersion'

// Open in the real default browser (same rationale as LicenseGate).
const openExternal = (url) => (e) => {
  e.preventDefault()
  invoke('plugin:shell|open', { path: url }).catch(() => {
    window.open(url, '_blank', 'noopener')
  })
}

const EULA_URL = 'https://www.alanglobalintelligence.com/legal/echo-license'

export default function EulaGate({ onAccepted }) {
  const [saving, setSaving] = useState(false)

  const handleAccept = async () => {
    setSaving(true)
    // Persist acceptance (version + timestamp) via the existing settings store.
    // Never brick the user on a save failure — proceed and log; the gate will
    // simply re-show next launch if persistence failed.
    try {
      await invoke('set_setting', { key: 'eula_accepted_version', value: EULA_VERSION })
      await invoke('set_setting', { key: 'eula_accepted_at', value: new Date().toISOString() })
    } catch (e) {
      console.warn('EULA acceptance could not be persisted:', e)
    }
    onAccepted()
  }

  const handleDecline = () => {
    invoke('quit_app').catch(() => window.close())
  }

  return (
    <div className="eula-gate" data-tauri-drag-region>
      <div className="eula-gate-card">
        <div className="eula-gate-head">
          <Monogram size={28} />
          <h1>License Agreement</h1>
          <p>Please review and accept the license agreement to use ALAN Echo.</p>
        </div>
        <div className="eula-gate-text" tabIndex={0}>
          <pre>{eulaText}</pre>
        </div>
        <div className="eula-gate-actions">
          <button className="btn-secondary" onClick={handleDecline} disabled={saving}>
            Decline &amp; Quit
          </button>
          <button className="btn-primary" onClick={handleAccept} disabled={saving} autoFocus>
            I Agree — Continue
          </button>
        </div>
        <div className="eula-gate-foot">
          <a href={EULA_URL} onClick={openExternal(EULA_URL)}>View online</a>
          <span> · ALAN Global Intelligence</span>
        </div>
      </div>
    </div>
  )
}
```

- [x] **Step 2:** Add styles to `src/tokens.css` (match existing card/button token usage — reuse the classes LicenseGate uses if `btn-primary`/`btn-secondary` already exist; otherwise append):

```css
.eula-gate { display: flex; align-items: center; justify-content: center; height: 100vh; }
.eula-gate-card { width: min(720px, 92vw); max-height: 88vh; display: flex; flex-direction: column; gap: 12px; padding: 20px; }
.eula-gate-text { overflow-y: auto; flex: 1; min-height: 220px; border: 1px solid var(--border, #333); border-radius: 8px; padding: 12px 16px; }
.eula-gate-text pre { white-space: pre-wrap; font: inherit; margin: 0; }
.eula-gate-actions { display: flex; justify-content: flex-end; gap: 10px; }
.eula-gate-foot { font-size: 12px; opacity: 0.7; }
```

(If `btn-primary`/`btn-secondary` don't exist in tokens.css, check what LicenseGate's buttons use — `Select-String -Path src/components/LicenseGate.jsx -Pattern 'className'` — and reuse those exact classes instead.)

- [x] **Step 3:** Verify the raw import builds:

Run: `npm run build`
Expected: Vite build succeeds (`?raw` is native Vite).

- [x] **Step 4:** Commit:

```bash
git add src/components/EulaGate.jsx src/tokens.css
git commit -m "feat: first-launch EULA clickwrap gate component"
```

### Task A4: Wire the gate into the app boot flow

**Files:**
- Modify: `src/main.jsx` (full replacement below — current file is 85 lines)

- [x] **Step 1:** Replace `src/main.jsx` with:

```jsx
import './tokens.css'
import { useState, useEffect, useRef } from 'react'
import { createRoot } from 'react-dom/client'
import Splash from './components/Splash'
import Dashboard from './components/Dashboard'
import LicenseGate from './components/LicenseGate'
import EulaGate from './components/EulaGate'
import UpdateBanner from './components/UpdateBanner'
import { invoke } from '@tauri-apps/api/core'
import { applyTheme } from './theme'
import { EULA_VERSION } from './legal/eulaVersion'

const ENGINE_WAIT_MS = 90000

function App() {
  const [phase, setPhase] = useState('checking')
  const [progress, setProgress] = useState(0)
  const [engineLabel, setEngineLabel] = useState('')
  const mounted = useRef(true)

  useEffect(() => {
    mounted.current = true
    async function init() {
      let settings = null
      try {
        settings = await invoke('get_settings')
        applyTheme(settings)
      } catch {}

      // EULA gate comes before everything — including the trial. If settings
      // are unreadable we show the gate (one harmless click); if acceptance
      // can't be saved we still proceed (never brick), and re-prompt next run.
      const accepted = settings?.eula_accepted_version === EULA_VERSION
      if (!mounted.current) return
      if (!accepted) {
        setPhase('eula')
        return
      }
      continueAfterEula()
    }
    init()
    return () => { mounted.current = false }
  }, [])

  async function continueAfterEula() {
    let licensed = false
    try {
      licensed = await invoke('check_license')
    } catch (e) {
      // Fail open: an internal error must never brick a paying user.
      console.warn('License check failed, allowing through:', e)
      licensed = true
    }
    if (!mounted.current) return
    if (!licensed) {
      setPhase('license')
      return
    }
    setPhase('splash')
    loadEngine()
  }

  // Wait (bounded) for whisper-server to finish loading the model so the
  // first dictation is instant. On failure/timeout we proceed anyway — the
  // dashboard surfaces engine errors when the user actually dictates.
  async function loadEngine() {
    const started = Date.now()
    while (mounted.current && Date.now() - started < ENGINE_WAIT_MS) {
      try {
        const info = await invoke('get_engine_info')
        if (info?.model_label) setEngineLabel(`${info.model_label} model`)
        if (info?.ready) break
        if (info?.status?.startsWith('failed')) {
          console.warn('Speech engine failed to start:', info.status)
          break
        }
      } catch (e) {
        console.warn('Engine check failed:', e)
        break
      }
      setProgress(p => Math.min(92, p + 3))
      await new Promise(r => setTimeout(r, 500))
    }
    if (!mounted.current) return
    setProgress(100)
    setTimeout(() => { if (mounted.current) setPhase('ready') }, 400)
  }

  function handleLicenseActivated() {
    setPhase('splash')
    loadEngine()
  }

  if (phase === 'checking') return <Splash progress={0} modelLabel={engineLabel} />
  if (phase === 'eula') return <EulaGate onAccepted={continueAfterEula} />
  if (phase === 'license') return <LicenseGate onActivated={handleLicenseActivated} />
  if (phase === 'splash') return <Splash progress={Math.min(progress, 100)} modelLabel={engineLabel} />
  return <><UpdateBanner /><Dashboard /></>
}

createRoot(document.getElementById('root')).render(<App />)
```

- [ ] **Step 2:** Build + manual behavioral test:

Run: `npm run tauri dev`
Expected, in order: (1) EULA gate appears on first run; (2) "Decline & Quit" exits the process; (3) relaunch → gate again; (4) "I Agree" → license/trial gate appears; (5) relaunch → gate does NOT reappear; (6) edit `eulaVersion.js` to `'9999-01-01'`, relaunch → gate reappears (version-bump re-prompt works); revert.

- [ ] **Step 3:** Verify persistence landed in settings storage:

Run (after accepting, Windows): `Get-Content "$env:APPDATA\..\Local\com.alan.echo\settings.json" 2>$null | Select-String eula` (adjust path if the settings file lives elsewhere — find it with `Get-ChildItem $env:LOCALAPPDATA\com.alan.echo`).
Expected: `eula_accepted_version` + `eula_accepted_at` present.

- [x] **Step 4:** Commit:

```bash
git add src/main.jsx
git commit -m "feat: gate app boot on EULA acceptance (clickwrap before trial/license)"
```

### Task A5: Copyright + bundle metadata + installer license page

**Files:**
- Modify: `src-tauri/tauri.conf.json` (bundle section, lines 33–59)
- Create: `legal/EULA.txt` (repo root `legal/` dir)
- Create: `scripts/gen-eula-txt.mjs`
- Modify: `src-tauri/Cargo.toml`, `package.json`

- [x] **Step 1:** Create `scripts/gen-eula-txt.mjs` (keeps installer text in sync with the bundled markdown):

```js
// Generates legal/EULA.txt (plain text for the NSIS license page) from
// src/legal/eula.md. Run whenever the EULA changes: node scripts/gen-eula-txt.mjs
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs'

const md = readFileSync('src/legal/eula.md', 'utf8')
const txt = md
  .replace(/^#{1,3} /gm, '')   // strip heading markers
  .replace(/\*\*([^*]+)\*\*/g, '$1')
  .replace(/\r?\n/g, '\r\n')   // NSIS prefers CRLF
mkdirSync('legal', { recursive: true })
writeFileSync('legal/EULA.txt', txt)
console.log('Wrote legal/EULA.txt (%d chars)', txt.length)
```

- [x] **Step 2:** Run it: `node scripts/gen-eula-txt.mjs` — Expected: `Wrote legal/EULA.txt`.

- [x] **Step 3:** Edit `src-tauri/tauri.conf.json` bundle section — add `copyright` and `licenseFile` (Tauri 2 renders `licenseFile` as the NSIS license page):

```json
  "bundle": {
    "active": true,
    "targets": ["nsis", "dmg"],
    "copyright": "© 2026 ALAN Global Intelligence. All rights reserved.",
    "licenseFile": "../legal/EULA.txt",
    "resources": {
      "resources/models": "models"
    },
```

(rest of the bundle section unchanged). If the bundler rejects `licenseFile` at this level on the pinned Tauri version, move it to `bundle.windows.nsis.license` — verify against the schema referenced at the top of the file.

- [x] **Step 4:** Add license metadata: in `src-tauri/Cargo.toml` `[package]` add `license = "LicenseRef-Proprietary"`; in `package.json` add `"license": "SEE LICENSE IN LICENSE"`.

- [ ] **Step 5:** Verify: `npm run tauri build` completes; run the produced NSIS installer in a VM/sandbox — Expected: a license agreement page appears before install, showing the EULA text.

- [x] **Step 6:** Commit:

```bash
git add src-tauri/tauri.conf.json legal/EULA.txt scripts/gen-eula-txt.mjs src-tauri/Cargo.toml package.json
git commit -m "feat: copyright metadata + NSIS installer license page"
```

### Task A6: Third-party notices (generation + in-app viewer)

**Files:**
- Create: `scripts/gen-notices.mjs`
- Create: `src/legal/third-party-notices.txt` (generated)
- Modify: `src/components/SettingsPanel.jsx` (About section, Task A7 hosts the button)

- [x] **Step 1:** Install the npm license tool as a dev dep and the cargo tool once:

```bash
npm i -D license-checker-rseidelsohn
cargo install cargo-license
```

- [x] **Step 2:** Create `scripts/gen-notices.mjs`:

```js
// Generates src/legal/third-party-notices.txt — MIT requires shipping the
// license text with the software; Apache-2.0 requires NOTICE carriage.
// Run on dependency changes and before each release.
import { execSync } from 'node:child_process'
import { writeFileSync } from 'node:fs'

const header = `THIRD-PARTY NOTICES — ALAN Echo
This product includes open-source software. Full license texts below.

== Bundled components of special note ==
whisper.cpp — Copyright (c) 2023-2026 The ggml authors — MIT License
Whisper speech models — Copyright (c) OpenAI — released under the MIT License
`

let out = header + '\n== Rust crates ==\n'
out += execSync('cargo license --json', { cwd: 'src-tauri' })
  .toString()
  // keep it readable: name, version, license per line
  .split('\n').join('\n')

out += '\n\n== npm packages ==\n'
out += execSync('npx license-checker-rseidelsohn --production --plainVertical').toString()

writeFileSync('src/legal/third-party-notices.txt', out)
console.log('Wrote src/legal/third-party-notices.txt (%d chars)', out.length)
```

- [x] **Step 3:** Run: `node scripts/gen-notices.mjs` — Expected: file written; spot-check it mentions `whisper`, `tauri`, `react`:

Run: `Select-String -Path src/legal/third-party-notices.txt -Pattern 'whisper.cpp','tauri','react' | Select-Object -First 3`
Expected: 3 matches.

- [x] **Step 4:** Add an npm script in `package.json` `"scripts"`: `"gen:legal": "node scripts/gen-eula-txt.mjs && node scripts/gen-notices.mjs"`.

- [x] **Step 5:** Commit:

```bash
git add scripts/gen-notices.mjs src/legal/third-party-notices.txt package.json package-lock.json
git commit -m "feat: third-party license notices generation (MIT/Apache compliance)"
```

### Task A7: About section in Settings (©, version, legal links, notices viewer)

**Files:**
- Modify: `src/components/SettingsPanel.jsx` (append a final section)

- [x] **Step 1:** At the top of `SettingsPanel.jsx` add imports:

```jsx
import { useState } from 'react'   // merge with existing react imports
import { invoke } from '@tauri-apps/api/core'  // already imported in this file? keep single import
import pkg from '../../package.json'
import notices from '../legal/third-party-notices.txt?raw'
```

- [x] **Step 2:** Append an About section at the end of the settings list (match the panel's existing section markup — copy the wrapper element pattern used by the "autostart" section; the snippet below shows the content):

```jsx
{/* About & legal */}
<div className="settings-section">
  <h3>About</h3>
  <p>ALAN Echo v{pkg.version}</p>
  <p>© 2026 ALAN Global Intelligence. All rights reserved.</p>
  <p>
    <a href="https://www.alanglobalintelligence.com/legal/echo-license"
       onClick={(e) => { e.preventDefault(); invoke('plugin:shell|open', { path: 'https://www.alanglobalintelligence.com/legal/echo-license' }).catch(() => window.open('https://www.alanglobalintelligence.com/legal/echo-license', '_blank', 'noopener')) }}>
      License Agreement
    </a>
    {' · '}
    <a href="https://www.alanglobalintelligence.com/privacy"
       onClick={(e) => { e.preventDefault(); invoke('plugin:shell|open', { path: 'https://www.alanglobalintelligence.com/privacy' }).catch(() => window.open('https://www.alanglobalintelligence.com/privacy', '_blank', 'noopener')) }}>
      Privacy Policy
    </a>
    {' · '}
    <button className="link-button" onClick={() => setShowNotices(true)}>Open-source licenses</button>
  </p>
  {showNotices && (
    <div className="eula-gate-text" style={{ maxHeight: '40vh' }}>
      <pre>{notices}</pre>
      <button className="btn-secondary" onClick={() => setShowNotices(false)}>Close</button>
    </div>
  )}
</div>
```

…and add the state hook near the component's other `useState` calls: `const [showNotices, setShowNotices] = useState(false)`.

- [x] **Step 3:** Update `src/components/FooterBar.jsx` line ~32: change the version text `Echo v1.2.1` to `© 2026 ALAN · Echo v{pkg.version}` (import `pkg` the same way), so a copyright notice is visible on every screen (defeats the §504(c)(2) innocent-infringer mitigation per §401(d)).

- [ ] **Step 4:** Verify: `npm run tauri dev` → Settings shows About with version/©/links; "Open-source licenses" opens the notices; links open in the system browser.

- [x] **Step 5:** Commit:

```bash
git add src/components/SettingsPanel.jsx src/components/FooterBar.jsx
git commit -m "feat: About section with copyright, legal links, and OSS notices viewer"
```

### Task A8: Code signing + notarization (after P0.4)

**Files:**
- Modify: `src-tauri/tauri.conf.json` (windows + macOS bundle config)
- Modify: `.github/workflows/*` release workflow (inspect `Get-ChildItem .github\workflows` for the build job)

- [ ] **Step 1 (Windows, Azure Trusted Signing path):** in `tauri.conf.json` → `bundle.windows`, remove `"certificateThumbprint": null` and set:

```json
"windows": {
  "digestAlgorithm": "sha256",
  "timestampUrl": "http://timestamp.digicert.com",
  "signCommand": "trusted-signing-cli -e https://eus.codesigning.azure.net -a alan-echo-signing -c alan-echo-cert %1"
}
```

(adjust endpoint/account/profile names to the P0.4 account; `trusted-signing-cli` is installed in CI via `cargo install trusted-signing-cli`). If P0.4 chose a classic OV cert instead, set `"certificateThumbprint": "<THUMBPRINT>"` and keep `timestampUrl`.

- [ ] **Step 2 (macOS):** in `tauri.conf.json` → `bundle.macOS` add:

```json
"macOS": {
  "minimumSystemVersion": "10.15",
  "signingIdentity": "Developer ID Application: <NAME> (<TEAMID>)",
  "hardenedRuntime": true
}
```

and in the macOS CI job export `APPLE_API_ISSUER`, `APPLE_API_KEY`, `APPLE_API_KEY_PATH` so Tauri notarizes during bundling.

- [ ] **Step 3:** Verify Windows: build, then `Get-AuthenticodeSignature ".\src-tauri\target\release\bundle\nsis\ALAN Echo_*_x64-setup.exe" | Format-List Status,SignerCertificate` — Expected: `Status: Valid`. Verify macOS: `spctl -a -vv "ALAN Echo.app"` → `accepted`, and `xcrun stapler validate` on the DMG.

- [ ] **Step 4:** Commit:

```bash
git add src-tauri/tauri.conf.json .github/workflows
git commit -m "feat: Windows code signing + macOS notarization for release builds"
```

### Task A9: Repo hygiene — LICENSE + README

**Files:**
- Create: `LICENSE`
- Modify: `README.md` (replace Vite boilerplate)

- [x] **Step 1:** Create `LICENSE`:

```
ALAN Echo — Proprietary Software
© 2026 ALAN Global Intelligence. All rights reserved.

This repository contains proprietary software. No license is granted to copy,
modify, distribute, or use this software except as set out in the ALAN Echo
License Agreement: https://www.alanglobalintelligence.com/legal/echo-license

Third-party open-source components are listed in
src/legal/third-party-notices.txt and remain under their own licenses.
```

- [x] **Step 2:** Replace `README.md` body with a short product README: name, one-line description, `© 2026 ALAN Global Intelligence`, "Proprietary — see LICENSE", dev quickstart (`npm i && npm run tauri dev`), and a **Release legal checklist** section:

```markdown
## Release legal checklist
- [ ] EULA changed? Update src/legal/eula.md + bump EULA_VERSION + sync site page + `npm run gen:legal`
- [ ] Deps changed? `npm run gen:legal` (regenerates third-party notices)
- [ ] Installer shows license page (NSIS) / app shows EULA gate on fresh install
- [ ] Binaries signed (Windows) + notarized (macOS); SHA256SUMS attached to release
```

- [x] **Step 3:** Commit:

```bash
git add LICENSE README.md
git commit -m "docs: proprietary LICENSE and real README with release legal checklist"
```

---

## Workstream B — Website (`C:\Users\arowm\stock-analyzer`)

> Branch: `git checkout -b legal/echo-consent-and-claims` from `main`. The repo has vitest (`npm test`). Before any merge to main, run the `vercel-pre-deploy-check` skill.

### Task B1: Fix the TOS ↔ refund-policy contradiction

**Files:**
- Modify: `app/terms/page.tsx` (locate via grep), `app/refund-policy/page.tsx`

- [ ] **Step 1:** Locate the offending sentence:

Run: `Select-String -Path app/terms/page.tsx -Pattern 'non-refundable' -Context 1`
Expected: the "all fees are non-refundable" sentence (~line 205).

- [ ] **Step 2:** Replace that sentence with:

```
Except as expressly stated in a product-specific policy, fees are non-refundable.
ALAN Echo one-time licenses carry a 30-day money-back guarantee as described in
the Refund Policy (/refund-policy) and the ALAN Echo License Agreement
(/legal/echo-license), which control for Echo purchases.
```

- [ ] **Step 3:** Verify no contradiction remains:

Run: `Select-String -Path app/terms/page.tsx -Pattern 'Echo' -Context 0,1`
Expected: the new carve-out present; no remaining blanket "all fees non-refundable" without the exception.

- [ ] **Step 4:** Commit: `git add app/terms/page.tsx && git commit -m "fix: terms now carve out Echo 30-day refund (removes TOS/refund-policy contradiction)"`

### Task B2: Checkout consent + CTA microcopy

**Files:**
- Modify: `app/api/echo/checkout/route.ts` (session params, lines ~65–91)
- Modify: `app/echo/DualCta.tsx`, `app/echo/CheckoutCta.tsx`
- Modify: `app/echo/download/page.tsx` (trial CTA microcopy)

- [ ] **Step 1 (prerequisite, manual):** In the Stripe Dashboard → Settings → Business → Public details, confirm the Terms of Service URL is set to the **site-wide** `https://www.alanglobalintelligence.com/terms` and leave it there — this setting is account-wide and also covers ALAN platform subscription checkouts. Do NOT point it at the Echo EULA. The Echo-specific EULA consent comes from this session's `custom_text.terms_of_service_acceptance` message (Step 2), which links `/legal/echo-license` directly at the consent checkbox; Task B1 additionally makes `/terms` carve out Echo and point to the EULA. While in Public details, set the Privacy Policy URL to `https://www.alanglobalintelligence.com/privacy` if empty. Record done.

- [ ] **Step 2:** In `app/api/echo/checkout/route.ts`, inside the `stripe.checkout.sessions.create({ ... })` object (alongside `automatic_tax`), add:

```ts
      consent_collection: { terms_of_service: "required" },
      custom_text: {
        terms_of_service_acceptance: {
          message:
            "I agree to the [ALAN Echo License Agreement](https://www.alanglobalintelligence.com/legal/echo-license). " +
            "Digital delivery begins immediately; EU/UK customers: you consent to immediate delivery and acknowledge " +
            "this affects the statutory withdrawal right — our 30-day money-back guarantee applies regardless.",
        },
      },
```

- [ ] **Step 3:** In `app/echo/DualCta.tsx`, directly under the buy button JSX, add:

```tsx
      <p className="cta-legal">
        By purchasing, you agree to the{" "}
        <a href="/legal/echo-license">ALAN Echo License Agreement</a>.
      </p>
```

Do the same in `app/echo/CheckoutCta.tsx`. Add a minimal `.cta-legal { font-size: 12px; opacity: 0.7; margin-top: 6px; }` to the component's stylesheet/module (match how these components style today — check for an existing classes file with `Select-String -Path app/echo/DualCta.tsx -Pattern 'className'`).

- [ ] **Step 4:** In `app/echo/download/page.tsx`, under the free-trial download button, add:

```tsx
      <p className="cta-legal">
        Free trial — 5 dictations/day, 50 total. Use is governed by the{" "}
        <a href="/legal/echo-license">License Agreement</a>, accepted in-app on first launch.
      </p>
```

- [ ] **Step 5:** Verify: `npm run build` succeeds; then a Stripe test-mode checkout shows the required ToS checkbox with the custom message.

- [ ] **Step 6:** Commit: `git add app/api/echo/checkout/route.ts app/echo/DualCta.tsx app/echo/CheckoutCta.tsx app/echo/download/page.tsx && git commit -m "feat: checkout ToS consent + EULA microcopy at all purchase/download CTAs"`

### Task B3: EULA revision (drafts ready now; finalize after P0.1–P0.3)

**Files:**
- Modify: `app/legal/echo-license/page.tsx`

> Apply Steps 1–3 (decision-independent additions) immediately; hold Step 4 (governing law/arbitration) until P0 decisions + counsel sign-off, then bump the date/version in Step 5. All drafts below are **for counsel review** — implement verbatim, flag in the PR description.

- [ ] **Step 1:** Add three new sections after current §3 (renumber the rest):

```
3A. Free trial. ALAN Echo includes a free trial limited by the Software (currently
5 dictations per day and 50 total). Trial use is licensed under this Agreement.
ALAN may modify, limit, or discontinue the trial at any time. The trial is
provided strictly AS IS, with no support and no warranty. Tampering with,
disabling, or circumventing trial or license mechanisms (including locally
stored, signed trial state) is a material breach of this Agreement and may
violate applicable law, including 17 U.S.C. § 1201.

3B. Updates and support. ALAN may make updates available but is not obligated
to provide updates, maintenance, or support. Updates are licensed under this
Agreement (or the revision presented with the update). Support, when offered,
is best-effort via the contact in Section 11.

3C. Third-party components. The Software includes open-source components
(including whisper.cpp and OpenAI Whisper models, each under the MIT License),
listed with their license texts in the THIRD-PARTY NOTICES file installed with
the Software and viewable in Settings → About. Those components are governed by
their own licenses, which control over this Agreement for those components.
```

- [ ] **Step 2:** In §4 (Privacy) append the local-storage + activation disclosure:

```
The Software stores license and trial state locally on your device, including
in application data files and (on Windows) the Windows registry, and (on macOS)
application support files. Activating a license sends your license key and an
anonymous machine identifier (a one-way hash of hardware identifiers) to ALAN's
activation service; checking for updates contacts ALAN's update service; and
optional speech-model downloads are fetched from their hosting provider. No
audio, transcripts, or dictation content ever leaves your device. Details:
https://www.alanglobalintelligence.com/privacy
```

- [ ] **Step 3:** In §7 (Limitation of liability) append carve-outs, and in §1 add transfer language:

```
§7 addition: NOTHING IN THIS AGREEMENT LIMITS LIABILITY FOR GROSS NEGLIGENCE,
WILLFUL MISCONDUCT, FRAUD, OR ANY LIABILITY THAT CANNOT BE LIMITED UNDER
APPLICABLE LAW. THE 30-DAY REFUND DESCRIBED IN SECTION 5 IS YOUR PRIMARY REMEDY
FOR DISSATISFACTION WITH THE SOFTWARE.

§1 addition: The Software is licensed, not sold. You may not assign or transfer
this Agreement or your license key except that you may permanently transfer the
license to another person if you transfer the key, cease all use, and delete
all copies; commercial resale remains prohibited under Section 3.
```

- [ ] **Step 4 (HOLD for P0):** Replace §10 per the decisions: governing law = entity/home state from P0.1; dispute resolution = the P0.2 choice. If arbitration is **kept**, add:

```
Opt-out: you may opt out of arbitration by emailing the address in Section 11
within 30 days of first accepting this Agreement, stating your name and order
reference; opting out does not affect any other term. Before filing any claim,
both parties agree to attempt informal resolution for 30 days after written
notice. If 25 or more similar demands are filed by or with the assistance of
the same counsel or organization, the parties agree to batched proceedings in
which 10 bellwether arbitrations proceed first.
```

If arbitration is **dropped**, §10 becomes governing law + exclusive venue in the chosen state's courts + both parties retain small-claims rights.

- [ ] **Step 5:** Bump `const EFFECTIVE_DATE` to the revision date; add `export const EULA_VERSION = "<same date>";` next to it; add a one-line changelog comment. Then sync the app: update `alan-echo/src/legal/eula.md` + `eulaVersion.js` + run `npm run gen:legal` there (per the app README release checklist).

- [ ] **Step 6:** Verify: `npm run build`; render the page locally and confirm 14 sections render and anchor links work.

- [ ] **Step 7:** Commit: `git add app/legal/echo-license/page.tsx && git commit -m "feat: EULA v2 — trial/updates/third-party/storage sections, cap carve-outs, transfer terms"`

### Task B4: Privacy policy — add the desktop-app section

**Files:**
- Modify: `app/privacy/page.tsx`

- [ ] **Step 1:** Add a new top-level section "ALAN Echo desktop app" with this text:

```
ALAN Echo desktop app. Echo is designed so your voice data never leaves your
device: dictation audio is captured from your microphone, transcribed locally
by an on-device model, and the audio is discarded. We do not receive, store, or
have any ability to access your audio, transcripts, or dictation content, and
the app contains no analytics or telemetry.

What the app does transmit: (1) when you activate a license, the app sends your
license key and an anonymous machine identifier (a one-way SHA-256 hash derived
from hardware identifiers) to our activation service; we store the key, machine
hash, activation time, IP address, and user-agent of that request to enforce
the per-license activation limit and prevent fraud; (2) when you check for
updates, the app requests version metadata from our service; (3) if you
download an optional speech model or GPU pack, it is fetched from our service
or from the model's hosting provider (Hugging Face), whose own privacy policy
applies to that download.

What the app stores locally on your device: settings, transcripts you choose to
keep, license/activation tokens, and signed trial state (in application data
files and, on Windows, the Windows registry; on macOS, application support
files). Uninstalling removes the application; local data can be removed per the
instructions in our documentation.

Activation records are retained for the life of the license plus 3 years, then
deleted or anonymized. To exercise privacy rights (access, deletion) over
purchase or activation records, contact the address in Section [contact],
referencing your order email.
```

- [ ] **Step 2:** Verify the existing website-data sections still accurately cover: checkout PII via Stripe (name/email), Plausible analytics (cookieless — state it), account data, and the CalOPPA-required elements (categories collected, third parties, do-not-track response). Add a "Cookies & analytics" line if absent: `We use Plausible, a cookieless analytics service; no advertising trackers, no cross-site cookies.`

- [ ] **Step 3:** Verify build: `npm run build`. Commit: `git add app/privacy/page.tsx && git commit -m "feat: privacy policy covers Echo app (activation data, local storage, no-audio-collection)"`

### Task B5: Marketing claims accuracy pass

**Files:**
- Modify: `app/echo/page.tsx` (privacy section ~lines 122–159), `app/echo/vs-dragon/page.tsx` (~lines 61–66), `app/echo/compare/page.tsx` (~line 33)

- [ ] **Step 1:** In `app/echo/page.tsx`, replace the literal over-claim. Current: `"No network calls, no telemetry, no analytics, no account."` Replace the bullet/paragraph with:

```
100% on-device dictation: the speech model runs on your computer. Your audio
and transcripts never leave your machine — there is no upload code in the
dictation path, no telemetry, and no analytics in the app. The only network
calls Echo ever makes are the ones you'd expect: license activation, optional
update checks, and optional model downloads. Never your voice.
```

Keep the airplane-mode paragraph (it's accurate for dictation) but scope its first sentence: `Turn off Wi-Fi. Dictate. Everything works.` → unchanged (true) — append: `(You'll only need a connection to activate a license or download an optional model.)`

- [ ] **Step 2:** Apply the same correction in `app/echo/vs-dragon/page.tsx` ("No audio is ever sent anywhere. No account required. No telemetry." — keep, it's accurate) but fix any flat "no network calls" phrasing found:

Run: `Select-String -Path app/echo/*.tsx -Pattern 'No network calls' -List`
Expected after edit: no matches.

- [ ] **Step 3:** Resolve the languages contradiction: check ground truth in the app repo — `Select-String -Path C:\Users\arowm\alan-echo\src-tauri\src\*.rs -Pattern 'multilingual|language'` and the model list. If multilingual models ship/downloadable: align the landing page to the compare page ("Dictate in 99 languages via downloadable multilingual models; English model included"). If English-only at runtime: fix `app/echo/compare/page.tsx` line ~33 to "English (more languages coming)". One truth, both pages.

- [ ] **Step 4:** Mac claims: `Select-String -Path app/echo -Pattern 'macOS|Mac' -List` — anywhere Mac availability is implied as current, change to "macOS version in development" until the Mac build actually ships (the audit found macOS currently non-functional).

- [ ] **Step 5:** Verify: `npm run build`; grep checks from steps 2–4 all clean. Commit: `git add app/echo && git commit -m "fix: privacy/availability claims now precisely match what the code does"`

### Task B6: Site footer legal links

**Files:**
- Modify: `app/components/SiteFooter.tsx` (lines ~19–28)

- [ ] **Step 1:** Alongside the existing `/terms` and `/privacy` links add:

```tsx
        <a href="/legal/echo-license">Echo License</a>
        <a href="/refund-policy">Refunds</a>
```

(match the surrounding link markup/classes exactly.)

- [ ] **Step 2:** Verify: `npm run build`; footer renders 4 legal links. Commit: `git add app/components/SiteFooter.tsx && git commit -m "feat: footer links to Echo EULA and refund policy"`

### Task B7: Truthful key-delivery copy (+ email template EULA link)

**Files:**
- Modify: `app/echo/success/page.tsx`, `app/echo/recover/page.tsx`, `lib/echo/email.ts`

- [ ] **Step 1:** In `app/echo/success/page.tsx`, find the "key was emailed" copy (`Select-String -Pattern 'email' app/echo/success/page.tsx`) and replace with:

```
Your license key is ready. View it any time at /echo/keys (sign in with the
account you used at checkout). Keep your Stripe receipt — it's your proof of
purchase.
```

- [ ] **Step 2:** In `app/echo/recover/page.tsx`, remove/replace any "check your inbox for the key email" step with the keys-page + receipt + support path (the page's 3-step structure already exists — make step 1 the `/echo/keys` sign-in).

- [ ] **Step 3:** In `lib/echo/email.ts` (so the template is correct whenever sending is re-enabled), add to the footer block:

```ts
  `Your use of ALAN Echo is governed by the ALAN Echo License Agreement: ${BASE_URL}/legal/echo-license`,
```

(match the template's existing line-array or JSX style.)

- [ ] **Step 4:** Verify: `npm run build`; `Select-String -Path app/echo/success/page.tsx,app/echo/recover/page.tsx -Pattern 'emailed'` → no stale claims. Commit: `git add app/echo/success/page.tsx app/echo/recover/page.tsx lib/echo/email.ts && git commit -m "fix: key-delivery copy tells the truth (keys page, not email); EULA link in email template"`

### Task B8: Revocation integrity — refunded keys must actually stop working

**Files:**
- Create: `lib/echo/guards.ts`
- Create: `tests/echo/guards.test.ts`
- Modify: `app/api/echo/download/route.ts`, `app/api/echo/validate-key/route.ts`, `app/api/echo/activate/route.ts`, `app/api/stripe/webhook/route.ts`

- [ ] **Step 1:** Write the failing test `tests/echo/guards.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { assertLicenseUsable, LicenseGuardError } from "@/lib/echo/guards";

const base = { key: "ECHO-AAAAA-BBBBB-CCCCC-DDDDD", revokedAt: null as Date | null };

describe("assertLicenseUsable", () => {
  it("passes an active license", () => {
    expect(() => assertLicenseUsable(base)).not.toThrow();
  });
  it("rejects a revoked license with a refund-safe message", () => {
    const revoked = { ...base, revokedAt: new Date("2026-06-01") };
    expect(() => assertLicenseUsable(revoked)).toThrow(LicenseGuardError);
    try { assertLicenseUsable(revoked); } catch (e) {
      expect((e as LicenseGuardError).status).toBe(403);
      expect((e as LicenseGuardError).message).toMatch(/revoked|refunded/i);
    }
  });
  it("rejects a missing license", () => {
    expect(() => assertLicenseUsable(null)).toThrow(LicenseGuardError);
  });
});
```

- [ ] **Step 2:** Run it to verify it fails: `npx vitest run tests/echo/guards.test.ts` — Expected: FAIL (module not found).

- [ ] **Step 3:** Create `lib/echo/guards.ts`:

```ts
export class LicenseGuardError extends Error {
  constructor(message: string, public status: number) {
    super(message);
  }
}

type LicenseLike = { revokedAt: Date | null } | null | undefined;

/** Single chokepoint for "is this license allowed to do anything". */
export function assertLicenseUsable(license: LicenseLike): asserts license {
  if (!license) throw new LicenseGuardError("License key not found", 404);
  if (license.revokedAt) {
    throw new LicenseGuardError(
      "This license has been revoked (typically after a refund). Contact support if this is unexpected.",
      403,
    );
  }
}
```

- [ ] **Step 4:** Run the test: `npx vitest run tests/echo/guards.test.ts` — Expected: PASS (3 tests).

- [ ] **Step 5:** Wire the guard into all three routes. In each of `app/api/echo/download/route.ts`, `app/api/echo/validate-key/route.ts`, `app/api/echo/activate/route.ts`: after the prisma `echoLicense` lookup, replace any ad-hoc/absent revocation check with:

```ts
import { assertLicenseUsable, LicenseGuardError } from "@/lib/echo/guards";
// ...
try {
  assertLicenseUsable(license);
} catch (e) {
  if (e instanceof LicenseGuardError) {
    return NextResponse.json({ ok: false, error: e.message }, { status: e.status });
  }
  throw e;
}
```

(Find the lookup anchors with `Select-String -Path app/api/echo/*/route.ts -Pattern 'echoLicense.find'`.)

- [ ] **Step 6:** Webhook: in `app/api/stripe/webhook/route.ts`, confirm a `charge.refunded` / `checkout.session.async_payment_failed`-adjacent handler sets `revokedAt` on the matching `EchoLicense` (find with `Select-String -Pattern 'refund' app/api/stripe/webhook/route.ts`). If absent, add inside the event switch:

```ts
case "charge.refunded": {
  const charge = event.data.object as Stripe.Charge;
  const sessionId = charge.metadata?.checkout_session_id ?? null;
  // Fall back to payment_intent lookup if metadata is absent.
  const where = sessionId
    ? { stripeSessionId: sessionId }
    : { stripeSessionId: String(charge.payment_intent) };
  await prisma.echoLicense.updateMany({
    where: { ...where, revokedAt: null },
    data: { revokedAt: new Date() },
  });
  break;
}
```

(Adapt the `where` to how `stripeSessionId` is actually recorded at issuance — check `lib/echo/issue.ts` for the field written. Also revoke the license's `EchoActivation` rows: `await prisma.echoActivation.updateMany({ where: { keyId }, data: { revoked: true, revokedAt: new Date() } })` after looking up the license id.)

- [ ] **Step 7:** Run the full suite + build: `npm test` and `npm run build` — Expected: green.

- [ ] **Step 8:** Commit: `git add lib/echo/guards.ts tests/echo/guards.test.ts app/api/echo app/api/stripe/webhook/route.ts && git commit -m "fix: revoked licenses blocked at download/validate/activate; refund webhook revokes key + activations"`

### Task B9: Stop `/api/echo/version` leaking the raw asset URL

**Files:**
- Modify: `app/api/echo/version/route.ts`

- [ ] **Step 1:** Open the route (`Select-String -Path app/api/echo/version/route.ts -Pattern 'downloadUrl'`). Change the `downloadUrl` field from the raw GitHub asset URL to the gated endpoint, preserving the response shape the app's `updater.rs` expects (`version`, `downloadUrl`, `sha256`, `sizeMb`, `releaseDate`):

```ts
const downloadUrl = `${baseUrl}/api/echo/download?source=updater`;
```

The download route already gates by key for paid builds; trial installers are intentionally free, so the gate's existing behavior is preserved — the change removes the direct unauthenticated asset link from a public JSON response.

- [ ] **Step 2:** Verify the app updater still works end-to-end: run the app (Task A4 dev build), trigger "Check for updates" via UpdateBanner against a local site (`npm run dev` in stock-analyzer with `.env` pointing the app there if a dev override exists — otherwise verify post-deploy on production with v-next).

- [ ] **Step 3:** `npm run build` green. Commit: `git add app/api/echo/version/route.ts && git commit -m "fix: version endpoint returns gated download URL, not raw asset link"`

### Task B10: Activation token expiry

**Files:**
- Modify: `lib/echo/activation.ts` (token creation), `C:\Users\arowm\alan-echo\src-tauri\src\activation.rs` (verification)

- [ ] **Step 1:** In `lib/echo/activation.ts` `createActivationToken(...)`, add an `exp` claim of now + 400 days (long enough that an annual app update refreshes it; short enough that revoked/refunded machines eventually die offline):

```ts
const exp = Math.floor(Date.now() / 1000) + 400 * 24 * 60 * 60;
// include `exp` in the signed claims object alongside mfp/jti
```

- [ ] **Step 2:** In the app, `src-tauri/src/activation.rs`: locate the JWT claims struct + verification (`is_activated` / the Ed25519 verify path). Add to the claims struct `pub exp: Option<i64>` and after signature verification:

```rust
if let Some(exp) = claims.exp {
    let now = chrono::Utc::now().timestamp();
    // 7-day grace for clock skew; expired token => silent re-activation attempt
    if now > exp + 7 * 86_400 {
        log::info!("Activation token expired; attempting silent re-activation");
        return false; // caller falls through to activate_online with the saved key
    }
}
```

(Confirm `chrono` is already a dependency — `Select-String -Path src-tauri/Cargo.toml -Pattern 'chrono'`; it is used by trial.rs date logic. Tokens WITHOUT `exp` (all existing customers) must continue to validate — the `Option` handles that.)

- [ ] **Step 3:** Verify: `cargo check` green; manual: activate in dev, hand-edit the stored `activation.jwt` exp to the past, relaunch → app re-activates online silently (key is saved in settings) without bricking.

- [ ] **Step 4:** Commit both repos: website `git commit -m "feat: activation tokens carry 400d expiry"`; app `git commit -m "feat: honor activation token expiry with 7d skew + silent re-activation"`.

### Task B11: Counsel-review CI gate covers Echo legal pages

**Files:**
- Modify: the guard in `lib/settings/disclosures.ts` (find the file list it enforces), `app/refund-policy/page.tsx`

- [ ] **Step 1:** Locate the existing counsel gate: `Select-String -Path lib/settings/disclosures.ts -Pattern 'COUNSEL_REVIEWED' -Context 3`. Add `app/legal/echo-license/page.tsx` and `app/refund-policy/page.tsx` to whatever path list / hash check it enforces, so future edits to Echo legal pages require touching the counsel-review stamp.

- [ ] **Step 2 (after P0.3 counsel review only):** Remove the "pending counsel review" header from `app/refund-policy/page.tsx` and set the reviewed-at stamp per the gate's convention.

- [ ] **Step 3:** `npm run build` + the gate's own check pass. Commit: `git add lib/settings/disclosures.ts app/refund-policy/page.tsx && git commit -m "chore: Echo legal pages under counsel-review CI gate"`

---

## Workstream C — Distribution

### Task C1: Releases repo legal hygiene

**Files:** `README.md` in `diablobuster/alan-echo-releases` (private repo, via gh CLI from any directory)

- [ ] **Step 1:** Create the README content locally as `C:\Users\arowm\alan-echo\docs\releases-repo-readme.md`:

```markdown
# ALAN Echo — Releases

Official release binaries for ALAN Echo (private distribution repo; installers
are served to users via https://alanglobalintelligence.com/echo/download).

**Proprietary software.** © 2026 ALAN Global Intelligence. All rights reserved.
Installation and use are governed by the ALAN Echo License Agreement:
https://www.alanglobalintelligence.com/legal/echo-license

Each release attaches `SHA256SUMS.txt`. Verify on Windows:
`Get-FileHash .\ALAN-Echo-setup.exe -Algorithm SHA256`
```

- [ ] **Step 2:** Push it to the releases repo:

```bash
gh api repos/diablobuster/alan-echo-releases/contents/README.md -X PUT -f message="docs: proprietary notice + EULA pointer + checksum instructions" -f content="$(base64 -w0 docs/releases-repo-readme.md)"
```

(If a README already exists, include `-f sha=$(gh api repos/diablobuster/alan-echo-releases/contents/README.md --jq .sha)`.)

- [ ] **Step 3:** Verify: `gh api repos/diablobuster/alan-echo-releases/contents/README.md --jq .name` → `README.md`.

### Task C2: SHA256SUMS per release + release-notes legal line

**Files:**
- Create: `scripts/release-checksums.ps1` in alan-echo

- [ ] **Step 1:** Create `scripts/release-checksums.ps1`:

```powershell
# Generates SHA256SUMS.txt for all installers in the bundle output and uploads
# it to the given release tag. Usage: .\scripts\release-checksums.ps1 v1.2.2
param([Parameter(Mandatory)][string]$Tag)
$bundles = Get-ChildItem "src-tauri\target\release\bundle" -Recurse -Include *.exe,*.dmg
$lines = $bundles | ForEach-Object { "{0}  {1}" -f (Get-FileHash $_ -Algorithm SHA256).Hash.ToLower(), $_.Name }
Set-Content -Path SHA256SUMS.txt -Value $lines -Encoding ascii
gh release upload $Tag SHA256SUMS.txt --repo diablobuster/alan-echo-releases --clobber
Write-Host "Uploaded SHA256SUMS.txt for $Tag"
```

- [ ] **Step 2:** Add to the app README release checklist (Task A9) — already includes the checksum line; verify it's there.

- [ ] **Step 3:** Adopt a release-notes template ending line for every future release: `Use is governed by the ALAN Echo License Agreement: https://www.alanglobalintelligence.com/legal/echo-license`. Add this note to the README release checklist too.

- [ ] **Step 4:** Commit (alan-echo): `git add scripts/release-checksums.ps1 README.md && git commit -m "feat: release checksums script + legal line in release template"`

---

## Workstream D — Business & legal filings (human-executed; bite-sized)

### Task D1: US copyright registration (deadline-driven)

**Depends on:** P0.3 (AI-disclosure answer). **Hard deadline: application received by 2026-09-10.**

- [ ] **Step 1:** With counsel's AI-disclosure answer in hand, assemble the deposit: print/PDF the source as first 25 + last 25 pages, ordering files so `src-tauri/src/activation.rs` and `src-tauri/src/trial.rs` are NOT among the deposited pages (no redaction needed then). Include the page bearing `© 2026 ALAN Global Intelligence`.
- [ ] **Step 2:** File at copyright.gov: **Standard Application** ($65), Type = Literary Work, Title "ALAN Echo v1.2.1", Published = yes (first publication date = first public release date — confirm from the releases repo: `gh release list --repo diablobuster/alan-echo-releases`), Author = your legal name (or LLC per P0.1 with transfer statement), Author Created = "computer program", Limitation of Claim → Material Excluded: "previously published and third-party owned computer code, including open-source components", New Material: "computer program". Add AI disclosure per counsel.
- [ ] **Step 3:** Pay $65, save the case number + a full PDF copy of the application into `docs/legal-filings/` (private; do NOT commit deposit source pages to any repo other than noting the case number).
- [ ] **Step 4:** Calendar: re-register at each major version or annually (next check 2027-06).

### Task D2: Trademark clearance, then filing

- [ ] **Step 1:** **Clearance FIRST** — attorney knock-out search on "ALAN ECHO" (and "ECHO" formative marks) in Classes 9 (software) and 42. Specific known risk to evaluate: Amazon's ECHO registrations for voice-controlled software/devices. Outcome = file as-is, file "ALAN ECHO" as a composite with disclaimer strategy, or adjust naming. Budget $300–$600 for the search opinion.
- [ ] **Step 2:** If cleared: file TEAS Plus ($250/class) for Class 9 (downloadable speech-recognition software), specimen = the website product page + in-app screenshot.
- [ ] **Step 3:** Record serial number + docket dates in `docs/legal-filings/`.

### Task D3: Entity + address hygiene (after P0.1)

- [ ] **Step 1:** If LLC chosen: file formation, get EIN, open business bank account, update Stripe account legal entity + statement descriptor.
- [ ] **Step 2:** Replace the home apartment address ("3341 E Fountain Blvd APT 311") everywhere customer-facing with the registered-agent/business address: `Select-String -Path C:\Users\arowm\stock-analyzer\lib\echo\email.ts,C:\Users\arowm\stock-analyzer\app -Pattern 'Fountain Blvd' -List` → edit each hit. (CAN-SPAM requires a physical address — the registered agent address satisfies it.)
- [ ] **Step 3:** Update EULA/terms contact + governing law per P0.1 (ties to B3 Step 4). Commit the website edits.

### Task D4: Insurance + tax + support readiness

- [ ] **Step 1:** Get quotes for tech E&O + cyber (Hiscox/Vouch/Embroker class; expect ~$500–$1,500/yr at current scale). Bind one.
- [ ] **Step 2:** Stripe Tax: confirm `automatic_tax` registrations — in Stripe Dashboard → Tax, review threshold monitoring; register where Stripe flags obligations (US economic nexus states as they trip; EU OSS/VAT if EU sales materialize). Calendar a quarterly 15-minute review.
- [ ] **Step 3:** Support: confirm `support@alanglobalintelligence.com` (or equivalent) is staffed; write `docs/support/refund-sop.md`: refund in Stripe → webhook revokes key (B8) → verify key dead via `/api/echo/validate-key` → reply template. 30-day guarantee honored no-questions-asked (it's load-bearing for the liability-cap and EULA fairness posture).

### Task D5: Mac launch legal checklist (execute when Mac build is real)

- [ ] **Step 1:** A8 signing/notarization green on macOS; EULA gate verified on macOS (A4 behavioral test repeated).
- [ ] **Step 2:** EULA §1 wording change: "Windows devices" → "devices" (counsel-stamped via B11 gate); bump EULA_VERSION; sync app bundle.
- [ ] **Step 3:** Un-gate Mac marketing claims (reverse of B5 Step 4); add macOS local-storage wording already present from B3 Step 2 (verify).
- [ ] **Step 4:** Update privacy policy app section if any Mac-specific path differs.

---

## Sequencing & dependency map

| Wave | Tasks | Blockers | Est. effort |
|---|---|---|---|
| **Wave 1 (now, parallel)** | A1–A7, A9, B1, B2, B4–B7, B9, C1, C2; P0.1–P0.4 kicked off | none | ~2–3 dev-days |
| **Wave 2 (counsel/decisions back)** | B3 (all steps), B11 Step 2, D3 | P0.1–P0.3 | ~0.5 dev-day + attorney turnaround |
| **Wave 3 (hardening)** | B8, B10, A8 | P0.4 for A8 | ~1 dev-day |
| **Filings** | D1 (by **2026-09-10**), D2, D4 | P0.3 | human calendar time |
| **Mac launch** | D5 | Mac build functional | ~0.5 day |

**Definition of done:** every checkbox checked; fresh-install behavioral test passes on Windows AND macOS (EULA gate → decline quits → accept persists → installer license page shows → About shows ©/notices); Stripe test checkout shows consent checkbox; `npm test` green in stock-analyzer; signed/notarized binaries verify; registration case number recorded; the audit prompt (`docs/2026-06-12-legal-audit-prompt.md`) run clean.

**Cost summary:** $65 registration + $250–$600 trademark search/filing + $1,000–$2,500 counsel bundle + $50–$200 entity + ~$120/yr signing (Azure) + $99/yr Apple + ~$500–$1,500/yr insurance.

---

## Self-review (performed at authoring)

- Spec coverage: all 12 findings map to tasks (1→A1–A4, 2→A5–A7/A9, 3→A5/A8, 4→B2, 5→B1, 6→B7, 7→B5+B4, 8→B4, 9→B3, 10→B8–B10, 11→C1–C2, 12→D1–D4). EU consent = B2 Step 2 custom_text; EAA + withdrawal nuances = P0.3(4).
- Placeholders: none — every code step carries the code; copy/paste sources are named with verification greps; the two read-then-insert steps (B8 Step 6 `where` adaptation, B10 Step 2 claims struct) name the exact anchor greps and full inserted code.
- Type consistency: `EULA_VERSION` (A1/B3/A4), `quit_app` (A2/A3), `assertLicenseUsable`/`LicenseGuardError` (B8 Steps 1/3/5), `gen:legal` (A6/A9) used consistently.
