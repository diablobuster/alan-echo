import Icon from './Icons'
import { Btn } from './StatusPanel'

export default function SearchBar({ query, onChange, count, total, onExport, onSettings }) {
  return (
    <div style={{
      display: 'flex', alignItems: 'center', gap: 8,
    }}>
      {/* Search input */}
      <div style={{
        flex: 1, display: 'flex', alignItems: 'center', gap: 8,
        background: 'var(--bg-input)', border: '1px solid var(--border-primary)',
        borderRadius: 'var(--echo-radius-sm)', padding: '6px 10px',
      }}>
        <Icon name="search" size={14} color="var(--text-faint)" />
        <input
          type="text"
          placeholder="Search transcriptions..."
          value={query}
          onChange={e => onChange(e.target.value)}
          style={{
            flex: 1, border: 'none', outline: 'none', background: 'none',
            fontFamily: 'var(--font-sans)', fontSize: 12, color: 'var(--text-primary)',
          }}
        />
        {query && count !== undefined && (
          <span className="echo-mono" style={{ fontSize: 10, color: 'var(--text-faint)', whiteSpace: 'nowrap' }}>
            {count} / {total}
          </span>
        )}
      </div>

      {/* Export */}
      <Btn kind="ghost" size="sm" onClick={onExport}>
        <Icon name="download" size={13} /> Export
      </Btn>
    </div>
  )
}
