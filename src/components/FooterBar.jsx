import Icon, { Kbd } from './Icons'

export default function FooterBar() {
  return (
    <div style={{
      height: 36, display: 'flex', alignItems: 'center', padding: '0 16px',
      borderTop: '1px solid var(--border-primary)',
      background: 'var(--bg-secondary)',
      gap: 16, fontSize: 10,
    }}>
      {/* Hotkey hints */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 4, color: 'var(--text-muted)' }}>
        <Kbd>Ctrl</Kbd><Kbd>&#8679;</Kbd><Kbd>Space</Kbd>
        <span style={{ marginLeft: 2 }}>Dictate</span>
      </div>
      <div style={{ display: 'flex', alignItems: 'center', gap: 4, color: 'var(--text-muted)' }}>
        <Kbd>Esc</Kbd>
        <span style={{ marginLeft: 2 }}>Cancel</span>
      </div>

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
