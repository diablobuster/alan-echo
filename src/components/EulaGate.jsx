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
