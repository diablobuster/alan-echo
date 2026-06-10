import { useState } from 'react'
import Icon from './Icons'
import { Btn } from './StatusPanel'

const FORMATS = [
  ['txt', 'Plain text (.txt)'],
  ['md', 'Markdown (.md)'],
  ['json', 'JSON (.json)'],
  ['csv', 'CSV (.csv)'],
]

export default function SearchBar({ query, onChange, count, total, onExport }) {
  const [menuOpen, setMenuOpen] = useState(false)

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

      {/* Export with format menu */}
      <div style={{ position: 'relative' }}>
        <Btn kind="ghost" size="sm" onClick={() => setMenuOpen(o => !o)}>
          <Icon name="download" size={13} /> Export <Icon name="chevron" size={11} />
        </Btn>
        {menuOpen && (
          <>
            <div onClick={() => setMenuOpen(false)} style={{ position: 'fixed', inset: 0, zIndex: 90 }} />
            <div style={{
              position: 'absolute', right: 0, top: 'calc(100% + 4px)', zIndex: 91,
              background: 'var(--bg-card)', border: '1px solid var(--border-primary)',
              borderRadius: 'var(--echo-radius)', boxShadow: '0 4px 16px rgba(0,0,0,0.14)',
              padding: 4, minWidth: 150, animation: 'echo-rise 0.14s ease-out both',
            }}>
              {FORMATS.map(([fmt, label]) => (
                <button
                  key={fmt}
                  onClick={() => { setMenuOpen(false); onExport(fmt) }}
                  style={{
                    display: 'block', width: '100%', textAlign: 'left', padding: '6px 10px',
                    fontSize: 11, background: 'none', border: 'none', cursor: 'pointer',
                    borderRadius: 'var(--echo-radius-sm)', color: 'var(--text-primary)',
                    fontFamily: 'var(--font-sans)',
                  }}
                  onMouseEnter={e => e.currentTarget.style.background = 'var(--bg-hover)'}
                  onMouseLeave={e => e.currentTarget.style.background = 'none'}
                >
                  {label}
                </button>
              ))}
            </div>
          </>
        )}
      </div>
    </div>
  )
}
