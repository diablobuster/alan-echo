import Icon from './Icons'
import { Btn } from './StatusPanel'
import { formatTimestamp, formatDuration } from './TranscriptCard'

export default function DetailPanel({ transcript, onCopy, onDelete }) {
  if (!transcript) {
    return (
      <div style={{
        padding: 24, textAlign: 'center', color: 'var(--text-faint)',
        display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center',
        height: '100%', gap: 8,
      }}>
        <Icon name="chevron" size={20} color="var(--text-faint)" style={{ opacity: 0.5 }} />
        <span style={{ fontSize: 12 }}>Select a transcription to view</span>
      </div>
    )
  }

  const { timestamp, text, duration_seconds, word_count } = transcript
  return (
    <div style={{
      display: 'flex', flexDirection: 'column', height: '100%',
      background: 'var(--bg-card)', border: '1px solid var(--border-primary)',
      borderRadius: 'var(--echo-radius)',
    }}>
      {/* Metadata */}
      <div style={{
        padding: '10px 16px', borderBottom: '1px solid var(--border-primary)',
        display: 'flex', alignItems: 'center', gap: 8,
      }}>
        <span className="echo-mono" style={{ fontSize: 11, color: 'var(--text-muted)' }}>
          {formatTimestamp(timestamp)}
        </span>
        {duration_seconds > 0 && (
          <span className="echo-mono" style={{ fontSize: 10, color: 'var(--text-faint)' }}>
            &middot; {formatDuration(duration_seconds)}
          </span>
        )}
        {word_count > 0 && (
          <span className="echo-mono" style={{ fontSize: 10, color: 'var(--text-faint)' }}>
            &middot; {word_count} words
          </span>
        )}
        <div style={{ flex: 1 }} />
        <Btn kind="brass" size="sm" onClick={onCopy}>
          <Icon name="copy" size={12} color="#fff" /> Copy
        </Btn>
        <Btn kind="danger" size="sm" onClick={onDelete}>
          <Icon name="trash" size={12} /> Delete
        </Btn>
      </div>

      {/* Full text */}
      <div style={{
        flex: 1, padding: '14px 16px', overflow: 'auto',
        fontSize: 13, lineHeight: 1.7, color: 'var(--text-primary)',
        whiteSpace: 'pre-wrap', wordBreak: 'break-word',
      }}>
        {text}
      </div>
    </div>
  )
}
