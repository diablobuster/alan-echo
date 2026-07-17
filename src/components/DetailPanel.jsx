import { useState, useEffect } from 'react'
import Icon from './Icons'
import { Btn } from './StatusPanel'
import { formatTimestamp, formatDuration } from './TranscriptCard'

export default function DetailPanel({ transcript, onCopy, onDelete, onSaveEdit }) {
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState('')
  // Two-step in-app delete confirmation — window.confirm() is a silent no-op
  // in the macOS webview, which made Delete do nothing there.
  const [confirmingDelete, setConfirmingDelete] = useState(false)

  // Leaving a transcript (select/delete) always exits edit mode.
  useEffect(() => {
    setEditing(false)
    setDraft(transcript?.text || '')
    setConfirmingDelete(false)
  }, [transcript?.id])

  useEffect(() => {
    if (!confirmingDelete) return
    const t = setTimeout(() => setConfirmingDelete(false), 3000)
    return () => clearTimeout(t)
  }, [confirmingDelete])

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
    try {
      const ok = await onSaveEdit(id, trimmed)
      if (ok) setEditing(false)
    } catch (e) {
      console.error('Save failed:', e)
    }
  }

  return (
    <div style={{
      display: 'flex', flexDirection: 'column', height: '100%',
      background: 'var(--bg-card)', border: '1px solid var(--border-primary)',
      borderRadius: 'var(--echo-radius)',
    }}>
      {/* Metadata */}
      <div style={{
        padding: '10px 14px', borderBottom: '1px solid var(--border-primary)',
        display: 'flex', alignItems: 'center', gap: 6, flexWrap: 'wrap',
        minHeight: 42,
      }}>
        <span className="echo-mono" style={{ fontSize: 11, color: 'var(--text-muted)', whiteSpace: 'nowrap' }}>
          {formatTimestamp(timestamp)}
          {duration_seconds > 0 && ` · ${formatDuration(duration_seconds)}`}
          {word_count > 0 && ` · ${word_count}w`}
        </span>
        <div style={{ flex: 1, minWidth: 8 }} />
        {editing ? (
          <>
            <Btn kind="ghost" size="sm" onClick={() => { setEditing(false); setDraft(text) }}>Cancel</Btn>
            <Btn kind="primary" size="sm" onClick={saveEdit}>
              <Icon name="check" size={12} color="#fff" /> Save
            </Btn>
          </>
        ) : (
          <>
            <Btn kind="ghost" size="sm" onClick={startEdit} aria-label="Edit transcription">Edit</Btn>
            <Btn kind="brass" size="sm" onClick={onCopy}>
              <Icon name="copy" size={12} color="#fff" /> Copy
            </Btn>
            <Btn kind="danger" size="sm" onClick={() => {
              if (!confirmingDelete) {
                setConfirmingDelete(true)
                return
              }
              setConfirmingDelete(false)
              onDelete()
            }}>
              <Icon name="trash" size={12} /> {confirmingDelete ? 'Confirm delete?' : 'Delete'}
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
        <>
          <div style={{
            flex: 1, padding: '14px 16px', overflow: 'auto',
            fontSize: 13, lineHeight: 1.7, color: 'var(--text-primary)',
            whiteSpace: 'pre-wrap', wordBreak: 'break-word',
          }}>
            {text}
          </div>
          <div style={{
            padding: '6px 14px', borderTop: '1px solid var(--border-primary)',
            fontSize: 10.5, color: 'var(--text-faint)',
            display: 'flex', alignItems: 'center', gap: 6,
          }}>
            <Icon name="waveform" size={11} color="var(--text-faint)" />
            Auto-transcribed by an on-device speech model — review before relying on it.
          </div>
        </>
      )}
    </div>
  )
}
