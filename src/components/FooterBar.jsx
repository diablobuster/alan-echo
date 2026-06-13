import Icon, { Kbd } from './Icons'
import pkg from '../../package.json'

function HotkeyHint({ accel, label }) {
  if (!accel) return null
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 4, color: 'var(--text-muted)' }}>
      {accel.split(' + ').map(k => <Kbd key={k}>{k}</Kbd>)}
      <span style={{ marginLeft: 2 }}>{label}</span>
    </div>
  )
}

export default function FooterBar({ hotkeys = {} }) {
  return (
    <div style={{
      height: 32, display: 'flex', alignItems: 'center', padding: '0 12px',
      borderTop: '1px solid var(--border-primary)',
      background: 'var(--bg-secondary)',
      gap: 12, fontSize: 10, overflow: 'hidden', flexShrink: 0,
    }}>
      <HotkeyHint accel={hotkeys.toggle === null ? null : (hotkeys.toggle || 'Ctrl + Shift + Space')} label="Dictate" />
      <HotkeyHint accel={hotkeys.cancel} label="Cancel" />

      <div style={{ flex: 1 }} />

      <div style={{ display: 'flex', alignItems: 'center', gap: 4, color: 'var(--text-faint)', whiteSpace: 'nowrap' }}>
        <Icon name="lock" size={10} color="var(--text-faint)" />
        <span>Local &amp; private</span>
      </div>

      <span className="echo-mono" style={{ fontSize: 9, letterSpacing: '0.08em', textTransform: 'uppercase', color: 'var(--text-faint)', whiteSpace: 'nowrap' }}>
        © 2026 ALAN · Echo v{pkg.version}
      </span>
    </div>
  )
}
