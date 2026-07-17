import Icon, { Monogram } from './Icons'

const STATUS = {
  ready:      { label: 'Ready',          color: 'var(--accent-green)', pulse: false },
  recording:  { label: 'Recording',      color: 'var(--echo-rec)',     pulse: true },
  processing: { label: 'Transcribing...', color: 'var(--accent-yellow)', pulse: false, blink: true },
}

export function StatusDot({ status = 'ready', size = 7 }) {
  const s = STATUS[status] || STATUS.ready
  return (
    <span style={{
      width: size, height: size, borderRadius: '50%',
      background: s.color, display: 'inline-block', flexShrink: 0,
      animation: s.pulse ? 'echo-pulse 1.8s ease-in-out infinite' : s.blink ? 'echo-blink 1.2s ease-in-out infinite' : 'none',
      '--pc': `${s.color}73`,
    }} />
  )
}

function fmtClock(s) {
  const m = Math.floor(s / 60)
  const sec = Math.floor(s % 60)
  return `${m}:${String(sec).padStart(2, '0')}`
}

export default function TitleBar({ status = 'ready', elapsed = 0, onSettings, onMinimize, onMaximize, onClose }) {
  const s = STATUS[status] || STATUS.ready
  return (
    <div data-tauri-drag-region style={{
      height: 42, display: 'flex', alignItems: 'center', padding: '0 12px',
      background: 'linear-gradient(180deg, var(--bg-secondary) 0%, var(--bg-primary) 100%)',
      borderBottom: '1px solid var(--border-primary)',
      userSelect: 'none', WebkitAppRegion: 'drag',
      borderRadius: '7px 7px 0 0', flexShrink: 0,
    }}>
      {/* Wordmark */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <Monogram size={20} />
        <span style={{ fontSize: 13, fontWeight: 500, color: 'var(--text-primary)', letterSpacing: '-0.01em' }}>
          ALAN
        </span>
        <span className="echo-mono" style={{ fontSize: 10, letterSpacing: '0.12em', textTransform: 'uppercase', color: 'var(--text-faint)' }}>
          Global Intelligence
        </span>
        <span style={{ color: 'var(--border-secondary)', margin: '0 4px' }}>|</span>
        <span className="echo-serif" style={{ fontSize: 14, color: 'var(--text-secondary)' }}>Echo</span>
      </div>

      {/* Status pill */}
      <div style={{
        display: 'flex', alignItems: 'center', gap: 6,
        marginLeft: 16, padding: '3px 10px 3px 8px',
        background: 'var(--bg-card)', border: '1px solid var(--border-primary)',
        borderRadius: 20, fontSize: 11, color: s.color,
      }}>
        <StatusDot status={status} size={6} />
        <span style={{ fontWeight: 500 }}>{s.label}</span>
        {status === 'recording' && (
          <span className="echo-mono" style={{ fontWeight: 500, fontVariantNumeric: 'tabular-nums' }}>
            {fmtClock(elapsed)}
          </span>
        )}
      </div>

      <div style={{ flex: 1 }} />

      {/* Settings */}
      <button onClick={onSettings} aria-label="Settings" style={{
        background: 'none', border: 'none', cursor: 'pointer', padding: 6,
        color: 'var(--text-muted)', borderRadius: 4, WebkitAppRegion: 'no-drag',
      }}
        onMouseEnter={e => e.currentTarget.style.background = 'var(--bg-hover)'}
        onMouseLeave={e => e.currentTarget.style.background = 'none'}
      >
        <Icon name="gear" size={15} />
      </button>

      {/* Window controls */}
      <div style={{ display: 'flex', marginLeft: 8, gap: 2, WebkitAppRegion: 'no-drag' }}>
        <WinBtn icon="minimize" onClick={onMinimize} label="Minimize" />
        <WinBtn icon="maximize" onClick={onMaximize} label="Maximize" />
        <WinBtn icon="close" onClick={onClose} danger label="Close" />
      </div>
    </div>
  )
}

function WinBtn({ icon, onClick, danger, label }) {
  return (
    <button onClick={onClick} aria-label={label} style={{
      width: 32, height: 28, display: 'flex', alignItems: 'center', justifyContent: 'center',
      background: 'none', border: 'none', cursor: 'pointer', borderRadius: 3,
      color: danger ? 'var(--text-muted)' : 'var(--text-muted)',
    }}
      onMouseEnter={e => {
        e.currentTarget.style.background = danger ? 'var(--accent-red)' : 'var(--bg-hover)'
        if (danger) e.currentTarget.style.color = '#fff'
      }}
      onMouseLeave={e => {
        e.currentTarget.style.background = 'none'
        e.currentTarget.style.color = 'var(--text-muted)'
      }}
    >
      <Icon name={icon} size={14} stroke={1.4} />
    </button>
  )
}
