export default function QuickStats({ total = 0, words = 0, duration = 0 }) {
  const minutes = (duration / 60).toFixed(1)
  return (
    <div style={{
      display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: 12,
    }}>
      <StatCell label="Transcriptions" value={total} />
      <StatCell label="Words" value={words.toLocaleString()} />
      <StatCell label="Minutes" value={minutes} />
    </div>
  )
}

function StatCell({ label, value }) {
  return (
    <div style={{
      padding: '10px 14px',
      background: 'var(--bg-card)', border: '1px solid var(--border-primary)',
      borderRadius: 'var(--echo-radius)',
    }}>
      <div className="echo-serif" style={{ fontSize: 22, color: 'var(--text-primary)', lineHeight: 1 }}>
        {value}
      </div>
      <div className="echo-mono" style={{ fontSize: 10, letterSpacing: '0.12em', textTransform: 'uppercase', color: 'var(--text-faint)', marginTop: 4 }}>
        {label}
      </div>
    </div>
  )
}
