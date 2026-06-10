import Icon, { Kbd } from './Icons'

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
      height: 36, display: 'flex', alignItems: 'center', padding: '0 16px',
      borderTop: '1px solid var(--border-primary)',
      background: 'var(--bg-secondary)',
      gap: 16, fontSize: 10,
    }}>
      {/* Hotkey hints — "Shift" spelled out; ⇧ doesn't render everywhere.
          toggle === null means registration failed: advertise nothing. */}
      <HotkeyHint accel={hotkeys.toggle === null ? null : (hotkeys.toggle || 'Ctrl + Shift + Space')} label="Dictate" />
      <HotkeyHint accel={hotkeys.cancel} label="Cancel" />

      <div style={{ flex: 1 }} />

      {/* Security badge */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 4, color: 'var(--text-faint)' }}>
        <Icon name="lock" size={11} color="var(--text-faint)" />
        <span>Local & private — nothing leaves this device</span>
      </div>

      <span style={{ color: 'var(--border-secondary)' }}>|</span>

      <span className="echo-mono" style={{ fontSize: 9, letterSpacing: '0.1em', textTransform: 'uppercase', color: 'var(--text-faint)' }}>
        ALAN Global Intelligence &middot; Echo v1.0
      </span>
    </div>
  )
}
