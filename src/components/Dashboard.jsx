import { useState, useEffect, useCallback, useRef } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import TitleBar from './TitleBar'
import QuickStats from './QuickStats'
import StatusPanel from './StatusPanel'
import SearchBar from './SearchBar'
import TranscriptCard from './TranscriptCard'
import DetailPanel from './DetailPanel'
import FooterBar from './FooterBar'
import SettingsPanel from './SettingsPanel'
import Icon from './Icons'

export default function Dashboard() {
  const [transcripts, setTranscripts] = useState([])
  const [stats, setStats] = useState({ total: 0, words: 0, duration: 0 })
  const [selectedId, setSelectedId] = useState(null)
  const [flashId, setFlashId] = useState(null)
  const [query, setQuery] = useState('')
  const [status, setStatus] = useState('ready')
  const [elapsed, setElapsed] = useState(0)
  const [showSettings, setShowSettings] = useState(false)
  const [toast, setToast] = useState(null)
  const [error, setError] = useState(null)
  const timerRef = useRef(null)
  const statusRef = useRef(status)
  statusRef.current = status

  // ── Load data ──────────────────────────────────────────────────────
  const loadTranscripts = useCallback(async () => {
    try {
      const result = await invoke('get_transcripts', { page: 0, pageSize: 100 })
      if (result?.transcripts) setTranscripts(result.transcripts)
    } catch (e) { console.error('Failed to load transcripts:', e) }
  }, [])

  const loadStats = useCallback(async () => {
    try {
      const s = await invoke('get_stats')
      if (s?.total !== undefined) setStats(s)
    } catch (e) { console.error('Failed to load stats:', e) }
  }, [])

  useEffect(() => {
    loadTranscripts()
    loadStats()
  }, [loadTranscripts, loadStats])

  // ── Tauri event listeners (hotkeys from backend) ───────────────────
  useEffect(() => {
    let cancelled = false
    const unsub1 = listen('dictate-toggle', () => { if (!cancelled) handleToggleRef.current() })
    const unsub2 = listen('dictate-cancel', () => { if (!cancelled) handleCancelRef.current() })
    return () => {
      cancelled = true
      unsub1.then(fn => fn())
      unsub2.then(fn => fn())
    }
  }, [])

  // Use refs for handlers so event listeners always call latest version
  const handleToggleRef = useRef(null)
  const handleCancelRef = useRef(null)

  // ── Recording timer ────────────────────────────────────────────────
  useEffect(() => {
    if (status === 'recording') {
      const start = Date.now()
      timerRef.current = setInterval(() => {
        setElapsed(Math.floor((Date.now() - start) / 1000))
      }, 250)
    } else if (status === 'ready') {
      clearInterval(timerRef.current)
      setElapsed(0)
    } else {
      clearInterval(timerRef.current)
    }
    return () => clearInterval(timerRef.current)
  }, [status])

  // ── Dictation state machine ────────────────────────────────────────
  const handleToggle = useCallback(async () => {
    const s = statusRef.current
    if (s === 'ready') {
      setError(null)
      try {
        await invoke('start_recording')
        setStatus('recording')
      } catch (e) {
        setError('Microphone error — ' + (e || 'check your audio device'))
        console.error('Recording error:', e)
      }
    } else if (s === 'recording') {
      setStatus('processing')
      try {
        const recording = await invoke('stop_recording')
        if (!recording?.has_speech) {
          setError('No speech detected — try again')
          setStatus('ready')
          return
        }
        if (!recording?.wav_path) {
          setError('Recording failed')
          setStatus('ready')
          return
        }
        const result = await invoke('transcribe', { wavPath: recording.wav_path })
        if (result?.empty) {
          setError('Transcription returned empty — audio may be too short')
        } else if (result?.id) {
          await loadTranscripts()
          await loadStats()
          setFlashId(result.id)
          setSelectedId(result.id)
          setTimeout(() => setFlashId(null), 1500)
          // Auto-paste via clipboard
          try { await navigator.clipboard.writeText(result.text) } catch {}
        }
      } catch (e) {
        setError('Transcription failed — ' + (e || 'unknown error'))
        console.error('Transcription error:', e)
      }
      setStatus('ready')
    }
  }, [loadTranscripts, loadStats])

  const handleCancel = useCallback(async () => {
    if (statusRef.current === 'recording') {
      try { await invoke('stop_recording') } catch {}
      setStatus('ready')
    }
  }, [])

  // Keep refs updated
  handleToggleRef.current = handleToggle
  handleCancelRef.current = handleCancel

  // ── Search ─────────────────────────────────────────────────────────
  const [searchResults, setSearchResults] = useState(null)
  useEffect(() => {
    if (!query) { setSearchResults(null); return }
    const timeout = setTimeout(async () => {
      try {
        const results = await invoke('search_transcripts', { query })
        setSearchResults(results)
      } catch {
        setSearchResults(transcripts.filter(t => t.text.toLowerCase().includes(query.toLowerCase())))
      }
    }, 200)
    return () => clearTimeout(timeout)
  }, [query, transcripts])

  const displayList = searchResults ?? transcripts
  const selected = transcripts.find(t => t.id === selectedId) || displayList.find(t => t.id === selectedId)

  // ── Actions ────────────────────────────────────────────────────────
  const fireToast = useCallback((msg) => {
    setToast(msg)
    setTimeout(() => setToast(null), 1800)
  }, [])

  const handleCopy = useCallback(async () => {
    if (!selected) return
    try { await navigator.clipboard.writeText(selected.text) } catch {}
    fireToast('Copied to clipboard')
  }, [selected, fireToast])

  const handleDelete = useCallback(async () => {
    if (!selected) return
    try { await invoke('delete_transcript', { id: selected.id }) } catch (e) { console.error('Delete failed:', e) }
    setTranscripts(prev => prev.filter(t => t.id !== selected.id))
    setSelectedId(null)
    await loadStats()
    fireToast('Transcript deleted')
  }, [selected, fireToast, loadStats])

  const handleExport = useCallback(() => {
    fireToast('Export: coming soon')
  }, [fireToast])

  // ── Window controls ────────────────────────────────────────────────
  const handleMinimize = async () => { const { getCurrentWindow } = await import('@tauri-apps/api/window'); getCurrentWindow().minimize() }
  const handleMaximize = async () => { const { getCurrentWindow } = await import('@tauri-apps/api/window'); getCurrentWindow().toggleMaximize() }
  const handleClose = async () => { const { getCurrentWindow } = await import('@tauri-apps/api/window'); getCurrentWindow().hide() }

  // ── Clear error after timeout ──────────────────────────────────────
  useEffect(() => {
    if (error) { const t = setTimeout(() => setError(null), 6000); return () => clearTimeout(t) }
  }, [error])

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100vh', background: 'var(--bg-primary)' }}>
      <TitleBar status={status} onSettings={() => setShowSettings(!showSettings)} onMinimize={handleMinimize} onMaximize={handleMaximize} onClose={handleClose} />

      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', padding: '12px 16px', gap: 12, overflow: 'hidden' }}>
        <QuickStats total={stats.total} words={stats.words} duration={stats.duration} />

        <StatusPanel status={status} elapsed={elapsed} onToggle={handleToggle} onCancel={handleCancel} />

        {error && (
          <div style={{
            padding: '8px 12px', background: 'color-mix(in srgb, var(--accent-red) 8%, var(--bg-card))',
            border: '1px solid color-mix(in srgb, var(--accent-red) 20%, transparent)',
            borderRadius: 'var(--echo-radius)', fontSize: 12, color: 'var(--accent-red)',
            display: 'flex', alignItems: 'center', gap: 6,
          }}>
            <Icon name="x" size={13} color="var(--accent-red)" /> {error}
          </div>
        )}

        <SearchBar query={query} onChange={setQuery} count={query ? displayList.length : undefined} total={transcripts.length} onExport={handleExport} />

        <div style={{ flex: 1, display: 'flex', gap: 12, overflow: 'hidden', minHeight: 0 }}>
          <div style={{ flex: 1.05, display: 'flex', flexDirection: 'column', gap: 'var(--echo-list-gap)', overflow: 'auto', paddingRight: 4 }}>
            {displayList.length === 0 ? <EmptyState query={query} /> : displayList.map(t => (
              <TranscriptCard key={t.id} transcript={t} selected={t.id === selectedId} isNew={t.id === flashId} onClick={() => setSelectedId(t.id)} />
            ))}
          </div>
          <div style={{ flex: 0.95, minWidth: 0 }}>
            <DetailPanel transcript={selected} onCopy={handleCopy} onDelete={handleDelete} />
          </div>
        </div>
      </div>

      <FooterBar />
      <SettingsPanel open={showSettings} onClose={() => setShowSettings(false)} />

      {toast && (
        <div style={{
          position: 'fixed', bottom: 52, left: '50%', transform: 'translateX(-50%)',
          background: 'var(--text-primary)', color: 'var(--bg-primary)',
          padding: '6px 16px', borderRadius: 20, fontSize: 12, fontWeight: 500,
          display: 'flex', alignItems: 'center', gap: 6,
          animation: 'echo-rise 0.2s ease-out both', boxShadow: '0 4px 12px rgba(0,0,0,0.15)', zIndex: 200,
        }}>
          <Icon name="check" size={13} color="var(--accent-green)" /> {toast}
        </div>
      )}
    </div>
  )
}

function EmptyState({ query }) {
  if (query) {
    return (
      <div style={{ padding: 40, textAlign: 'center' }}>
        <div style={{ fontSize: 13, color: 'var(--text-muted)', marginBottom: 4 }}>No transcriptions match "<strong>{query}</strong>"</div>
        <div style={{ fontSize: 12, color: 'var(--text-faint)' }}>Try a different search term.</div>
      </div>
    )
  }
  return (
    <div style={{ padding: 40, textAlign: 'center' }}>
      <Icon name="mic" size={32} color="var(--text-faint)" style={{ opacity: 0.5, marginBottom: 12 }} />
      <div style={{ fontSize: 14, fontWeight: 500, color: 'var(--text-secondary)', marginBottom: 6 }}>Nothing captured yet</div>
      <div style={{ fontSize: 12, color: 'var(--text-muted)' }}>
        Press <strong>Ctrl + Shift + Space</strong> to start your first dictation.
        <br />Your transcriptions will appear here automatically.
      </div>
    </div>
  )
}
