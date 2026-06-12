import { useState } from 'react'
import { Monogram } from './Icons'
import Icon from './Icons'
import { invoke } from '@tauri-apps/api/core'

// Open external pages in the user's real default browser — a plain
// target=_blank would open a chromeless WebView2 popup with no address bar,
// which is exactly where a buyer about to type card details bails out.
const openExternal = (url) => (e) => {
  e.preventDefault()
  invoke('plugin:shell|open', { path: url }).catch(() => {
    // Shell plugin unavailable — fall back to the popup rather than a dead link.
    window.open(url, '_blank', 'noopener')
  })
}

// Echo keys never contain these glyphs (CHARSET drops ambiguous ones) — a
// hand-typed key holding them is a transcription slip, not a bad key.
const CONFUSABLES = /[01ILO]/

export default function LicenseGate({ onActivated }) {
  const [key, setKey] = useState('')
  const [error, setError] = useState('')
  const [hint, setHint] = useState('')
  const [loading, setLoading] = useState(false)
  const [success, setSuccess] = useState(false)
  const [persistWarning, setPersistWarning] = useState(false)

  const handleActivate = async () => {
    setError('')
    setHint('')
    setLoading(true)
    try {
      const result = await invoke('validate_license', { key })
      if (result.valid) {
        if (result.activated) {
          setSuccess(true)
          setLoading(false)
          if (result.persisted === false) {
            setPersistWarning(true)
            setTimeout(() => onActivated(), 3500)
          } else {
            setTimeout(() => onActivated(), 800)
          }
          return
        }
        // Key format valid but online activation failed (offline)
        setError(result.message || 'Connect to the internet to activate')
        setHint('Your key is saved — connect to the internet and try again.')
      } else {
        setError(result.message || 'Invalid license key')
        const body = key.startsWith('ECHO-') ? key.slice(5) : key
        if (CONFUSABLES.test(body)) {
          setHint('Tip: Echo keys never contain 0, 1, I, L, or O — check those characters.')
        }
      }
    } catch (e) {
      console.error('License validation error:', e)
      setError('Could not validate the key — please restart the app and try again')
    }
    setLoading(false)
  }

  const handleKeyInput = (e) => {
    // Re-chunk whatever was typed/pasted into ECHO-XXXXX-XXXXX-XXXXX-XXXXX
    const raw = e.target.value.toUpperCase().replace(/[^A-Z0-9]/g, '')
    let val
    if ('ECHO'.startsWith(raw)) {
      // Still typing (or deleting back through) the prefix — leave it alone.
      val = raw
    } else {
      // Locate the key anywhere in the pasted text ("Key: ECHO-…" etc.).
      // Prefer a full ECHO + 20-charset-character match (the LAST one, so
      // prose containing "ALAN Echo" before the key can't shadow it); fall
      // back to chunking after the last bare "ECHO" for partial pastes.
      const full = [...raw.matchAll(/ECHO([A-HJ-NP-Z2-9]{20})/g)].pop()
      if (full) {
        const groups = full[1].match(/.{5}/g) || []
        val = ['ECHO', ...groups].join('-')
      } else {
        const idx = raw.lastIndexOf('ECHO')
        const body = idx !== -1 ? raw.slice(idx + 4) : raw
        const groups = (body.match(/.{1,5}/g) || []).slice(0, 4)
        val = ['ECHO', ...groups].join('-')
      }
    }
    setKey(val)
    setError('')
    setHint('')
  }

  const handleKeyDown = (e) => {
    if (e.key === 'Enter' && key.length >= 24) handleActivate()
  }

  return (
    <div style={{
      height: '100vh', display: 'flex', flexDirection: 'column',
      alignItems: 'center', justifyContent: 'center',
      background: 'var(--bg-primary)', gap: 0,
    }}>
      <Monogram size={48} tone="brass" />

      <div style={{ marginTop: 16, textAlign: 'center' }}>
        <div className="echo-serif" style={{ fontSize: 24, color: 'var(--text-primary)' }}>
          ALAN Echo
        </div>
        <div className="echo-mono" style={{ fontSize: 10, letterSpacing: '0.14em', textTransform: 'uppercase', color: 'var(--text-muted)', marginTop: 4 }}>
          Activate your license
        </div>
      </div>

      <div style={{ width: 340, marginTop: 32 }}>
        {/* Key input */}
        <div style={{ position: 'relative' }}>
          <input
            type="text"
            placeholder="ECHO-XXXXX-XXXXX-XXXXX-XXXXX"
            value={key}
            onChange={handleKeyInput}
            onKeyDown={handleKeyDown}
            style={{
              width: '100%', padding: '10px 14px',
              background: 'var(--bg-card)', border: `1px solid ${error ? 'var(--accent-red)' : 'var(--border-primary)'}`,
              borderRadius: 'var(--echo-radius)', fontSize: 13,
              fontFamily: 'var(--font-mono)', letterSpacing: '0.05em',
              color: 'var(--text-primary)', outline: 'none',
              textAlign: 'center',
            }}
            autoFocus
          />
        </div>

        {/* Error */}
        {error && (
          <div style={{
            marginTop: 8, fontSize: 11, color: 'var(--accent-red)',
            textAlign: 'center', display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 4,
          }}>
            <Icon name="x" size={12} color="var(--accent-red)" />
            {error}
          </div>
        )}
        {hint && (
          <div style={{ marginTop: 6, fontSize: 11, color: 'var(--text-muted)', textAlign: 'center' }}>
            {hint}
          </div>
        )}

        {/* Success */}
        {success && (
          <div style={{
            marginTop: 8, fontSize: 11, color: 'var(--accent-green)',
            textAlign: 'center', display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 4,
          }}>
            <Icon name="check" size={12} color="var(--accent-green)" />
            License activated — launching...
          </div>
        )}
        {persistWarning && (
          <div style={{
            marginTop: 6, fontSize: 11, color: 'var(--accent-yellow)',
            textAlign: 'center', lineHeight: 1.5,
          }}>
            Heads up: the key couldn&apos;t be saved to disk (is the drive full,
            or antivirus blocking AppData?). It works for this session, but you
            may need to enter it again next launch.
          </div>
        )}

        {/* Activate button */}
        <button
          onClick={handleActivate}
          disabled={key.length < 24 || loading || success}
          style={{
            width: '100%', marginTop: 12, padding: '10px 0',
            background: key.length >= 24 && !loading ? 'var(--brass)' : 'var(--bg-tertiary)',
            color: key.length >= 24 && !loading ? '#fff' : 'var(--text-faint)',
            border: 'none', borderRadius: 'var(--echo-radius)',
            fontSize: 13, fontWeight: 600, cursor: key.length >= 24 ? 'pointer' : 'default',
            fontFamily: 'var(--font-sans)',
            transition: 'background 0.15s, color 0.15s',
          }}
        >
          {loading ? 'Validating...' : success ? 'Activated' : 'Activate License'}
        </button>

        {/* Try for free */}
        <button
          onClick={onActivated}
          disabled={loading || success}
          style={{
            width: '100%', marginTop: 10, padding: '9px 0',
            background: 'none',
            color: 'var(--text-secondary)',
            border: '1px solid var(--border-primary)',
            borderRadius: 'var(--echo-radius)',
            fontSize: 12, fontWeight: 500, cursor: 'pointer',
            fontFamily: 'var(--font-sans)',
            transition: 'border-color 0.15s',
          }}
        >
          Try for free &mdash; 5/day, 50 total
        </button>

        {/* Buy + recover links */}
        <div style={{ marginTop: 16, textAlign: 'center' }}>
          <span style={{ fontSize: 11, color: 'var(--text-faint)' }}>
            Don't have a key?{' '}
            <a
              href="https://alanglobalintelligence.com/echo"
              onClick={openExternal('https://alanglobalintelligence.com/echo')}
              style={{ color: 'var(--echo-accent)', textDecoration: 'none' }}
            >
              Buy ALAN Echo
            </a>
          </span>
        </div>
        <div style={{ marginTop: 6, textAlign: 'center' }}>
          <span style={{ fontSize: 11, color: 'var(--text-faint)' }}>
            Lost your key?{' '}
            <a
              href="https://alanglobalintelligence.com/echo/keys"
              onClick={openExternal('https://alanglobalintelligence.com/echo/keys')}
              style={{ color: 'var(--echo-accent)', textDecoration: 'none' }}
            >
              Sign in to view your keys
            </a>
          </span>
        </div>
      </div>

      {/* Footer */}
      <div style={{ position: 'absolute', bottom: 24, textAlign: 'center' }}>
        <span className="echo-mono" style={{ fontSize: 9, letterSpacing: '0.1em', textTransform: 'uppercase', color: 'var(--text-faint)' }}>
          ALAN Global Intelligence &middot; Echo v1.2.1
        </span>
      </div>
    </div>
  )
}
