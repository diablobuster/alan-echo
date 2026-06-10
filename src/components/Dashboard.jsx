import { useState, useEffect, useCallback, useRef } from 'react'
import TitleBar from './TitleBar'
import QuickStats from './QuickStats'
import StatusPanel from './StatusPanel'
import SearchBar from './SearchBar'
import TranscriptCard from './TranscriptCard'
import DetailPanel from './DetailPanel'
import FooterBar from './FooterBar'
import Icon from './Icons'

const invoke = window.__TAURI__?.invoke || (async () => ({}))

// Sample data for development (removed when Tauri backend is connected)
const SAMPLE_DATA = [
  { id: 1, timestamp: new Date().toISOString(), text: "The quarterly report shows a significant increase in revenue across all segments. The technology division saw a 23% year-over-year growth while the consulting arm maintained steady margins.", duration_seconds: 45, word_count: 32 },
  { id: 2, timestamp: new Date(Date.now() - 3600000).toISOString(), text: "Please schedule the meeting for next Thursday at 3 PM with the engineering team. We need to discuss the API migration timeline and the deployment strategy for Q3.", duration_seconds: 23, word_count: 28 },
  { id: 3, timestamp: new Date(Date.now() - 86400000).toISOString(), text: "I've been reviewing the architecture document and I think we should reconsider the approach to caching. The current Redis implementation has some edge cases around session invalidation that could cause issues at scale.", duration_seconds: 67, word_count: 38 },
  { id: 4, timestamp: new Date(Date.now() - 172800000).toISOString(), text: "Looking at the NVIDIA earnings call transcript, they're guiding for 28 billion next quarter which is above consensus. The data center segment is the real story here with 47% sequential growth.", duration_seconds: 34, word_count: 32 },
  { id: 5, timestamp: new Date(Date.now() - 259200000).toISOString(), text: "The macro environment suggests we should be cautious on duration exposure. The yield curve is steepening again and the Fed's dot plot indicates two more hikes this cycle.", duration_seconds: 28, word_count: 29 },
]

export default function Dashboard() {
  const [transcripts, setTranscripts] = useState(SAMPLE_DATA)
  const [stats, setStats] = useState({ total: 5, words: 159, duration: 197 })
  const [selectedId, setSelectedId] = useState(null)
  const [flashId, setFlashId] = useState(null)
  const [query, setQuery] = useState('')
  const [status, setStatus] = useState('ready')
  const [elapsed, setElapsed] = useState(0)
  const [showSettings, setShowSettings] = useState(false)
  const [toast, setToast] = useState(null)
  const timerRef = useRef(null)

  // Load transcripts from backend
  useEffect(() => {
    async function load() {
      try {
        const result = await invoke('get_transcripts', { page: 0, pageSize: 50 })
        if (result.transcripts?.length) {
          setTranscripts(result.transcripts)
        }
        const s = await invoke('get_stats')
        if (s.total !== undefined) setStats(s)
      } catch {
        // Dev mode — use sample data
      }
    }
    load()
  }, [])

  // Recording timer
  useEffect(() => {
    if (status === 'recording') {
      timerRef.current = setInterval(() => setElapsed(e => e + 1), 1000)
    } else {
      clearInterval(timerRef.current)
      setElapsed(0)
    }
    return () => clearInterval(timerRef.current)
  }, [status])

  // Search filter
  const filtered = query
    ? transcripts.filter(t => t.text.toLowerCase().includes(query.toLowerCase()))
    : transcripts

  const selected = transcripts.find(t => t.id === selectedId)

  const fireToast = useCallback((msg) => {
    setToast(msg)
    setTimeout(() => setToast(null), 1800)
  }, [])

  const handleCopy = useCallback(() => {
    if (selected) {
      navigator.clipboard.writeText(selected.text).catch(() => {})
      fireToast('Copied to clipboard')
    }
  }, [selected, fireToast])

  const handleDelete = useCallback(async () => {
    if (!selected) return
    try {
      await invoke('delete_transcript', { id: selected.id })
    } catch { /* dev mode */ }
    setTranscripts(prev => prev.filter(t => t.id !== selected.id))
    setSelectedId(null)
    fireToast('Transcript deleted')
  }, [selected, fireToast])

  const handleExport = useCallback(() => {
    fireToast('Export coming soon')
  }, [fireToast])

  // Toggle dictation
  const handleToggle = useCallback(() => {
    if (status === 'ready') {
      setStatus('recording')
    } else if (status === 'recording') {
      setStatus('processing')
      setTimeout(() => {
        setStatus('ready')
        // In production, this would be the actual transcription result
      }, 2000)
    }
  }, [status])

  const handleCancel = useCallback(() => {
    if (status === 'recording') setStatus('ready')
  }, [status])

  // Window controls (Tauri)
  const handleMinimize = () => window.__TAURI__?.window?.getCurrentWindow?.()?.minimize?.()
  const handleMaximize = () => window.__TAURI__?.window?.getCurrentWindow?.()?.toggleMaximize?.()
  const handleClose = () => window.__TAURI__?.window?.getCurrentWindow?.()?.hide?.()

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100vh', background: 'var(--bg-primary)' }}>
      <TitleBar
        status={status}
        onSettings={() => setShowSettings(!showSettings)}
        onMinimize={handleMinimize}
        onMaximize={handleMaximize}
        onClose={handleClose}
      />

      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', padding: '12px 16px', gap: 12, overflow: 'hidden' }}>
        {/* Quick Stats */}
        <QuickStats total={stats.total} words={stats.words} duration={stats.duration} />

        {/* Status Panel */}
        <StatusPanel status={status} elapsed={elapsed} onToggle={handleToggle} onCancel={handleCancel} />

        {/* Search */}
        <SearchBar
          query={query} onChange={setQuery}
          count={query ? filtered.length : undefined}
          total={transcripts.length}
          onExport={handleExport}
        />

        {/* Split: List + Detail */}
        <div style={{ flex: 1, display: 'flex', gap: 12, overflow: 'hidden', minHeight: 0 }}>
          {/* Transcript list */}
          <div style={{
            flex: 1.05, display: 'flex', flexDirection: 'column', gap: 'var(--echo-list-gap)',
            overflow: 'auto', paddingRight: 4,
          }}>
            {filtered.length === 0 ? (
              <EmptyState query={query} />
            ) : (
              filtered.map(t => (
                <TranscriptCard
                  key={t.id}
                  transcript={t}
                  selected={t.id === selectedId}
                  isNew={t.id === flashId}
                  onClick={() => setSelectedId(t.id)}
                />
              ))
            )}
          </div>

          {/* Detail panel */}
          <div style={{ flex: 0.95, minWidth: 0 }}>
            <DetailPanel
              transcript={selected}
              onCopy={handleCopy}
              onDelete={handleDelete}
            />
          </div>
        </div>
      </div>

      <FooterBar />

      {/* Toast */}
      {toast && (
        <div style={{
          position: 'fixed', bottom: 52, left: '50%', transform: 'translateX(-50%)',
          background: 'var(--text-primary)', color: 'var(--bg-primary)',
          padding: '6px 16px', borderRadius: 20, fontSize: 12, fontWeight: 500,
          display: 'flex', alignItems: 'center', gap: 6,
          animation: 'echo-rise 0.2s ease-out both',
          boxShadow: '0 4px 12px rgba(0,0,0,0.15)',
        }}>
          <Icon name="check" size={13} color="var(--accent-green)" />
          {toast}
        </div>
      )}
    </div>
  )
}

function EmptyState({ query }) {
  if (query) {
    return (
      <div style={{ padding: 40, textAlign: 'center' }}>
        <div style={{ fontSize: 13, color: 'var(--text-muted)', marginBottom: 4 }}>
          No transcriptions match "<strong>{query}</strong>"
        </div>
        <div style={{ fontSize: 12, color: 'var(--text-faint)' }}>
          Try a different search term.
        </div>
      </div>
    )
  }
  return (
    <div style={{ padding: 40, textAlign: 'center' }}>
      <Icon name="mic" size={32} color="var(--text-faint)" style={{ opacity: 0.5, marginBottom: 12 }} />
      <div style={{ fontSize: 14, fontWeight: 500, color: 'var(--text-secondary)', marginBottom: 6 }}>
        Nothing captured yet
      </div>
      <div style={{ fontSize: 12, color: 'var(--text-muted)' }}>
        Press <strong>Ctrl+Shift+Space</strong> to start your first dictation.
      </div>
    </div>
  )
}
