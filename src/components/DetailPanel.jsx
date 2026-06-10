import { useState, useEffect } from 'react'
import Icon from './Icons'
import { Btn } from './StatusPanel'
import { formatTimestamp, formatDuration } from './TranscriptCard'

export default function DetailPanel({ transcript, onCopy, onDelete, onSaveEdit }) {
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState('')

  // Leaving a transcript (select/delete) always exits edit mode.
  useEffect(() => {
    setEditing(false)
    setDraft(transcript?.text || '')
  }, [transcript?.id])

  if (!transcript) {
    return (
      <div style={{
        padding: 24, textAlign: 'center', color: 'var(--text-faint)',
        display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center',
        height: '100%', gap: 10,
        border: '1px dashed var(--border-primary)', borderRadius: 'var(--echo-radius)',
      }}>
        <Icon name="waveform" size={26} color="var(--text-faint)" style={{ opacity: 0.6 }} />
        <div>
          <div style={{ fontSize: 12, fontWeight: 500, color: 'var(--text-muted)', marginBottom: 3 }}>
            No transcription selected
          </div>
          <div style={{ fontSize: 11, color: 'var(--text-faint)' }}>
            Pick one from the list to read, copy, or edit it.
          </div>
        </div>
      </div>
    )
  }

  const { id, timestamp, text, duration_seconds, word_count } = transcript

  const startEdit = () => {
    setDraft(text)
    setEditing(true)
  }

  const saveEdit = async () => {
    const trimmed = draft.trim()
    if (!trimmed || trimmed === text) {
      setEditing(false)
      setDraft(text)
      return
    }
    const ok = await onSaveEdit(id, trimmed)
    if (ok) setEditing(false)
  }

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
        {editing ? (
          <>
            <Btn kind="ghost" size="sm" onClick={() => { setEditing(false); setDraft(text) }}>Cancel</Btn>
            <Btn kind="primary" size="sm" onClick={saveEdit}>
              <Icon name="check" size={12} color="#fff" /> Save
            </Btn>
          </>
        ) : (
          <>
            <Btn kind="ghost" size="sm" onClick={startEdit}>Edit</Btn>
            <Btn kind="brass" size="sm" onClick={onCopy}>
              <Icon name="copy" size={12} color="#fff" /> Copy
            </Btn>
            <Btn kind="danger" size="sm" onClick={onDelete}>
              <Icon name="trash" size={12} /> Delete
            </Btn>
          </>
        )}
      </div>

      {/* Full text / editor */}
      {editing ? (
        <textarea
          value={draft}
          onChange={e => setDraft(e.target.value)}
          autoFocus
          style={{
            flex: 1, padding: '14px 16px', margin: 0, resize: 'none',
            fontSize: 13, lineHeight: 1.7, color: 'var(--text-primary)',
            fontFamily: 'var(--font-sans)', background: 'var(--bg-input)',
            border: 'none', outline: 'none',
            borderRadius: '0 0 var(--echo-radius) var(--echo-radius)',
          }}
        />
      ) : (
        <div style={{
          flex: 1, padding: '14px 16px', overflow: 'auto',
          fontSize: 13, lineHeight: 1.7, color: 'var(--text-primary)',
          whiteSpace: 'pre-wrap', wordBreak: 'break-word',
        }}>
          {text}
        </div>
      )}
    </div>
  )
}
