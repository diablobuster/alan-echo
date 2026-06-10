import { useState } from 'react'
import { Monogram } from './Icons'
import Icon from './Icons'

const invoke = window.__TAURI__?.invoke || (async () => ({ valid: true, message: 'Dev mode' }))

export default function LicenseGate({ onActivated }) {
  const [key, setKey] = useState('')
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)
  const [success, setSuccess] = useState(false)

  const handleActivate = async () => {
    setError('')
    setLoading(true)
    try {
      const result = await invoke('validate_license', { key })
      if (result.valid) {
        setSuccess(true)
        setTimeout(() => onActivated(), 800)
      } else {
        setError(result.message || 'Invalid license key')
      }
    } catch (e) {
      // Dev mode fallback
      setSuccess(true)
      setTimeout(() => onActivated(), 400)
    }
    setLoading(false)
  }

  const handleKeyInput = (e) => {
    let val = e.target.value.toUpperCase().replace(/[^A-Z0-9-]/g, '')
    // Auto-format: add dashes after ECHO and every 5 chars
    if (val.startsWith('ECHO') && val.length === 4) val += '-'
    setKey(val)
    setError('')
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
            maxLength={29}
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

        {/* Buy link */}
        <div style={{ marginTop: 16, textAlign: 'center' }}>
          <span style={{ fontSize: 11, color: 'var(--text-faint)' }}>
            Don't have a key?{' '}
            <a href="https://alan.com/echo" target="_blank" rel="noopener" style={{
              color: 'var(--echo-accent)', textDecoration: 'none',
            }}>
              Buy ALAN Echo
            </a>
          </span>
        </div>
      </div>

      {/* Footer */}
      <div style={{ position: 'absolute', bottom: 24, textAlign: 'center' }}>
        <span className="echo-mono" style={{ fontSize: 9, letterSpacing: '0.1em', textTransform: 'uppercase', color: 'var(--text-faint)' }}>
          ALAN Global Intelligence &middot; Echo v1.0
        </span>
      </div>
    </div>
  )
}
