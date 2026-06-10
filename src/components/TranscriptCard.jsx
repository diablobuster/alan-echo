function formatTimestamp(iso) {
  if (!iso) return ''
  const d = new Date(iso)
  const now = new Date()
  const isToday = d.toDateString() === now.toDateString()
  const yesterday = new Date(now); yesterday.setDate(yesterday.getDate() - 1)
  const isYesterday = d.toDateString() === yesterday.toDateString()

  const time = d.toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit', hour12: true })
  if (isToday) return `Today, ${time}`
  if (isYesterday) return `Yesterday, ${time}`
  return d.toLocaleDateString('en-US', { month: 'short', day: 'numeric' }) + `, ${time}`
}

function formatDuration(sec) {
  if (!sec || sec <= 0) return ''
  if (sec < 60) return `${Math.round(sec)}s`
  return `${Math.floor(sec / 60)}m ${Math.round(sec % 60)}s`
}

export default function TranscriptCard({ transcript, selected, isNew, onClick }) {
  const { timestamp, text, duration_seconds, word_count } = transcript
  return (
    <button onClick={onClick} style={{
      display: 'block', width: '100%', textAlign: 'left',
      background: selected ? 'var(--bg-secondary)' : 'var(--bg-card)',
      border: '1px solid ' + (selected ? 'var(--echo-accent)' : 'var(--border-primary)'),
      borderLeft: selected ? '3px solid var(--echo-accent)' : '1px solid var(--border-primary)',
      borderRadius: 'var(--echo-radius)',
      padding: 'var(--echo-cardpy) 14px',
      cursor: 'pointer',
      fontFamily: 'var(--font-sans)',
      transition: 'background 0.15s, border-color 0.15s',
      animation: isNew ? 'echo-rise 0.26s ease-out both, echo-flash 1.4s ease-out both' : 'none',
    }}
      onMouseEnter={e => { if (!selected) e.currentTarget.style.background = 'var(--bg-hover)' }}
      onMouseLeave={e => { if (!selected) e.currentTarget.style.background = 'var(--bg-card)' }}
    >
      {/* Header row */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 4 }}>
        <span style={{ fontSize: 11, color: 'var(--text-muted)', fontWeight: 500 }}>
          {formatTimestamp(timestamp)}
        </span>
        {isNew && (
          <span style={{
            fontSize: 9, fontWeight: 600, textTransform: 'uppercase', letterSpacing: '0.08em',
            padding: '1px 5px', borderRadius: 3,
            background: 'color-mix(in srgb, var(--echo-accent) 12%, transparent)',
            color: 'var(--echo-accent)',
          }}>new</span>
        )}
        <span style={{ flex: 1 }} />
        <span className="echo-mono" style={{ fontSize: 10, color: 'var(--text-faint)' }}>
          {formatDuration(duration_seconds)}{duration_seconds && word_count ? ' · ' : ''}{word_count ? `${word_count}w` : ''}
        </span>
      </div>
      {/* Preview */}
      <div style={{
        fontSize: 12, lineHeight: 1.5, color: 'var(--text-secondary)',
        display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical',
        overflow: 'hidden', textOverflow: 'ellipsis',
      }}>
        {text}
      </div>
    </button>
  )
}

export { formatTimestamp, formatDuration }
