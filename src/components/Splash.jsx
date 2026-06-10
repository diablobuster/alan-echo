import { Monogram } from './Icons'

export default function Splash({ progress = 64 }) {
  return (
    <div style={{
      height: '100vh', display: 'flex', flexDirection: 'column',
      alignItems: 'center', justifyContent: 'center',
      background: 'var(--bg-primary)', gap: 0,
    }}>
      <Monogram size={64} tone="brass" />

      <div style={{ marginTop: 20, textAlign: 'center' }}>
        <div className="echo-serif" style={{ fontSize: 28, color: 'var(--text-primary)', letterSpacing: '-0.02em' }}>
          ALAN
        </div>
        <div className="echo-mono" style={{ fontSize: 10, letterSpacing: '0.16em', textTransform: 'uppercase', color: 'var(--text-muted)', marginTop: 2 }}>
          Global Intelligence
        </div>
      </div>

      {/* Gold divider + Echo wordmark */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 12, margin: '16px 0' }}>
        <div style={{ width: 40, height: 1, background: 'var(--brass)', opacity: 0.5 }} />
        <span className="echo-serif" style={{ fontSize: 18, color: 'var(--brass)', letterSpacing: '0.05em' }}>
          Echo
        </span>
        <div style={{ width: 40, height: 1, background: 'var(--brass)', opacity: 0.5 }} />
      </div>

      <div style={{ fontSize: 12, color: 'var(--text-faint)', fontStyle: 'italic', marginBottom: 24 }}>
        Your voice, captured.
      </div>

      {/* Progress bar */}
      <div style={{
        width: 220, height: 3, background: 'var(--bg-tertiary)',
        borderRadius: 2, overflow: 'hidden',
      }}>
        <div style={{
          width: `${progress}%`, height: '100%',
          background: 'var(--brass)',
          borderRadius: 2,
          transition: 'width 0.3s ease-out',
        }} />
      </div>

      <div className="echo-mono" style={{ fontSize: 11, color: 'var(--text-muted)', marginTop: 10 }}>
        Initializing speech engine...
      </div>
      <div className="echo-mono" style={{ fontSize: 10, color: 'var(--text-faint)', marginTop: 2 }}>
        large-v3
      </div>

      {/* Footer */}
      <div style={{ display: 'flex', gap: 16, marginTop: 32 }}>
        {['Local', 'Private', 'Fast'].map((label, i) => (
          <span key={label} className="echo-mono" style={{ fontSize: 10, letterSpacing: '0.12em', textTransform: 'uppercase', color: 'var(--text-faint)' }}>
            {i > 0 && <span style={{ marginRight: 16, opacity: 0.4 }}>&middot;</span>}
            {label}
          </span>
        ))}
      </div>
    </div>
  )
}
