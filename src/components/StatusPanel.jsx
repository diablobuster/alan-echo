import { StatusDot } from './TitleBar'
import Icon, { Kbd } from './Icons'

function fmtClock(s) {
  const m = Math.floor(s / 60)
  const sec = Math.floor(s % 60)
  return `${m}:${String(sec).padStart(2, '0')}`
}

export default function StatusPanel({ status, elapsed = 0, cap = 300, hotkeys = {}, onToggle, onCancel }) {
  if (status === 'recording') {
    const pct = Math.min((elapsed / cap) * 100, 100)
    return (
      <div style={{
        padding: '14px 16px', background: 'var(--bg-card)',
        border: '1px solid var(--border-primary)', borderRadius: 'var(--echo-radius)',
      }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 10 }}>
          <StatusDot status="recording" />
          <span style={{ fontWeight: 500, color: 'var(--echo-rec)' }}>Recording</span>
          <span className="echo-mono" style={{ fontSize: 20, fontWeight: 500, color: 'var(--echo-rec)', marginLeft: 'auto' }}>
            {fmtClock(elapsed)}
          </span>
        </div>
        {/* Progress bar */}
        <div style={{ height: 3, background: 'var(--bg-tertiary)', borderRadius: 2, overflow: 'hidden', marginBottom: 6 }}>
          <div style={{
            width: `${pct}%`, height: '100%', background: 'var(--echo-rec)',
            borderRadius: 2, transition: 'width 1s linear',
          }} />
        </div>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
          <span className="echo-mono" style={{ fontSize: 10, color: 'var(--text-faint)' }}>
            {fmtClock(cap)} max
          </span>
          <div style={{ display: 'flex', gap: 6 }}>
            <Btn kind="ghost" size="sm" onClick={onCancel}>Cancel</Btn>
            <Btn kind="primary" size="sm" onClick={onToggle} style={{ background: 'var(--echo-rec)' }}>
              <Icon name="x" size={13} color="#fff" /> Stop
            </Btn>
          </div>
        </div>
      </div>
    )
  }

  if (status === 'processing') {
    return (
      <div style={{
        padding: '14px 16px', background: 'var(--bg-card)',
        border: '1px solid var(--border-primary)', borderRadius: 'var(--echo-radius)',
      }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 10 }}>
          <StatusDot status="processing" />
          <span style={{ fontWeight: 500, color: 'var(--accent-yellow)' }}>Transcribing...</span>
          <span className="echo-mono" style={{ fontSize: 10, color: 'var(--text-faint)', marginLeft: 'auto' }}>
            ALAN Speech Engine
          </span>
        </div>
        <div style={{ height: 3, background: 'var(--bg-tertiary)', borderRadius: 2, overflow: 'hidden' }}>
          <div style={{
            width: '30%', height: '100%', background: 'var(--accent-yellow)',
            borderRadius: 2, animation: 'echo-bar 1.4s ease-in-out infinite',
          }} />
        </div>
      </div>
    )
  }

  // Ready state
  return (
    <div style={{
      padding: '14px 16px', background: 'var(--bg-card)',
      border: '1px solid var(--border-primary)', borderRadius: 'var(--echo-radius)',
      display: 'flex', alignItems: 'center', gap: 10,
    }}>
      <StatusDot status="ready" />
      <span style={{ fontWeight: 500, color: 'var(--accent-green)' }}>Ready to dictate</span>
      <div style={{ flex: 1 }} />
      <div style={{ display: 'flex', alignItems: 'center', gap: 4, fontSize: 11, color: 'var(--text-muted)' }}>
        {hotkeys.toggle === null ? (
          <>
            <span style={{ color: 'var(--accent-yellow)' }}>Hotkey unavailable</span>
            <Btn kind="primary" size="sm" onClick={onToggle} style={{ marginLeft: 6 }}>
              <Icon name="mic" size={12} color="#fff" /> Start
            </Btn>
          </>
        ) : (
          <>
            {(hotkeys.toggle || 'Ctrl + Shift + Space').split(' + ').map(k => <Kbd key={k}>{k}</Kbd>)}
            <span style={{ marginLeft: 4 }}>to start</span>
          </>
        )}
      </div>
    </div>
  )
}

function Btn({ kind = 'primary', size = 'md', onClick, children, style = {} }) {
  const base = {
    display: 'inline-flex', alignItems: 'center', gap: 5,
    border: 'none', cursor: 'pointer', fontFamily: 'var(--font-sans)',
    fontWeight: 500, borderRadius: 'var(--echo-radius-sm)',
    padding: size === 'sm' ? '5px 10px' : '7px 14px',
    fontSize: size === 'sm' ? 11 : 12,
  }
  const kinds = {
    primary: { background: 'var(--echo-accent)', color: '#fff' },
    brass:   { background: 'var(--brass)', color: '#fff' },
    ghost:   { background: 'transparent', color: 'var(--text-secondary)', border: '1px solid var(--border-primary)' },
    danger:  { background: 'transparent', color: 'var(--accent-red)', border: '1px solid color-mix(in srgb, var(--accent-red) 30%, transparent)' },
  }
  return <button type="button" onClick={onClick} style={{ ...base, ...kinds[kind], ...style }}>{children}</button>
}

export { Btn }
