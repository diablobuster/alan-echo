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
import Onboarding from './Onboarding'
import Icon from './Icons'
import { applyTheme } from '../theme'

const MAX_RECORDING_SECONDS = 300

export default function Dashboard() {
  const [transcripts, setTranscripts] = useState([])
  const [stats, setStats] = useState({ total: 0, words: 0, duration: 0 })
  const [selectedId, setSelectedId] = useState(null)
  const [flashId, setFlashId] = useState(null)
  const [query, setQuery] = useState('')
  const [status, setStatus] = useState('ready')
  const [elapsed, setElapsed] = useState(0)
  const [showSettings, setShowSettings] = useState(false)
  const [showOnboarding, setShowOnboarding] = useState(false)
  const [toast, setToast] = useState(null)
  const [error, setError] = useState(null)
  const [hotkeys, setHotkeys] = useState({})
  const [trial, setTrial] = useState(null)
  const [hasMore, setHasMore] = useState(false)
  const [loadingMore, setLoadingMore] = useState(false)
  const pageRef = useRef(0)
  const timerRef = useRef(null)
  const statusRef = useRef(status)
  statusRef.current = status
  const settingsRef = useRef({})

  // ── Load data ──────────────────────────────────────────────────────
  const PAGE_SIZE = 100

  const loadTranscripts = useCallback(async () => {
    try {
      pageRef.current = 0
      const result = await invoke('get_transcripts', { page: 0, pageSize: PAGE_SIZE })
      if (result?.transcripts) {
        setTranscripts(result.transcripts)
        setHasMore(result.transcripts.length >= PAGE_SIZE)
      }
    } catch (e) { console.error('Failed to load transcripts:', e) }
  }, [])

  const loadMoreTranscripts = useCallback(async () => {
    if (loadingMore) return
    setLoadingMore(true)
    try {
      const nextPage = pageRef.current + 1
      const result = await invoke('get_transcripts', { page: nextPage, pageSize: PAGE_SIZE })
      if (result?.transcripts?.length) {
        pageRef.current = nextPage
        setTranscripts(prev => [...prev, ...result.transcripts])
        setHasMore(result.transcripts.length >= PAGE_SIZE)
      } else {
        setHasMore(false)
      }
    } catch (e) { console.error('Failed to load more:', e) }
    setLoadingMore(false)
  }, [loadingMore])

  const loadStats = useCallback(async () => {
    try {
      const s = await invoke('get_stats')
      if (s?.total !== undefined) setStats(s)
    } catch (e) { console.error('Failed to load stats:', e) }
  }, [])

  const loadSettings = useCallback(async () => {
    try {
      const s = await invoke('get_settings')
      settingsRef.current = s || {}
      applyTheme(s)
      if (s?.onboarding_complete !== true) setShowOnboarding(true)
    } catch (e) { console.error('Failed to load settings:', e) }
  }, [])

  useEffect(() => {
    loadTranscripts()
    loadStats()
    loadSettings()
    invoke('get_hotkey_info').then(h => setHotkeys(h || {})).catch(() => {})
    invoke('get_trial_status').then(t => setTrial(t)).catch(() => {})
  }, [loadTranscripts, loadStats, loadSettings])

  // ── Audio beeps ─────────────────────────────────────────────────────
  const audioCtx = useRef(null)
  const playBeep = useCallback((type) => {
    if (settingsRef.current.sound_enabled === false) return
    try {
      if (!audioCtx.current) audioCtx.current = new AudioContext()
      const ctx = audioCtx.current
      const osc = ctx.createOscillator()
      const gain = ctx.createGain()
      osc.connect(gain)
      gain.connect(ctx.destination)
      gain.gain.value = 0.05
      if (type === 'start') {
        osc.frequency.value = 800
        osc.start(); osc.stop(ctx.currentTime + 0.12)
      } else {
        osc.frequency.value = 600
        osc.start(); osc.stop(ctx.currentTime + 0.08)
        const osc2 = ctx.createOscillator()
        const gain2 = ctx.createGain()
        osc2.connect(gain2); gain2.connect(ctx.destination)
        gain2.gain.value = 0.05; osc2.frequency.value = 400
        osc2.start(ctx.currentTime + 0.1); osc2.stop(ctx.currentTime + 0.18)
      }
    } catch {}
  }, [])

  // ── Tauri event listeners (hotkeys + tray from backend) ────────────
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

  // Enforce the recording cap — the progress bar promises 5:00 max.
  useEffect(() => {
    if (status === 'recording' && elapsed >= MAX_RECORDING_SECONDS) {
      handleToggleRef.current()
    }
  }, [status, elapsed])

  // ── Dictation state machine ────────────────────────────────────────
  // statusRef is also written synchronously (not just at render) and an
  // inflight guard serializes toggles — otherwise rapid hotkey presses
  // double-invoke start/stop and desync the UI from the recorder.
  const inflightRef = useRef(false)
  const applyStatus = useCallback((next) => {
    statusRef.current = next
    setStatus(next)
  }, [])

  const discardWav = (wavPath) => {
    if (wavPath) invoke('discard_recording', { wavPath }).catch(() => {})
  }

  const handleToggle = useCallback(async () => {
    if (inflightRef.current) return
    inflightRef.current = true
    try {
      const s = statusRef.current
      if (s === 'ready') {
        setError(null)
        try {
          await invoke('start_recording')
          playBeep('start')
          applyStatus('recording')
        } catch (e) {
          setError('Microphone error — ' + (e || 'check your audio device'))
          console.error('Recording error:', e)
        }
      } else if (s === 'recording') {
        playBeep('stop')
        applyStatus('processing')
        try {
          const recording = await invoke('stop_recording')
          if (recording?.duration_seconds != null && recording.duration_seconds < 0.5) {
            discardWav(recording?.wav_path)
            setError('Recording too short — try a full sentence')
            applyStatus('ready')
            return
          }
          if (!recording?.has_speech) {
            discardWav(recording?.wav_path)
            setError('No speech detected — try again')
            applyStatus('ready')
            return
          }
          if (!recording?.wav_path) {
            setError('Recording failed')
            applyStatus('ready')
            return
          }
          const result = await invoke('transcribe', { wavPath: recording.wav_path })
          if (result?.empty) {
            setError('Nothing worth keeping was heard — try again')
          } else if (result?.id) {
            await loadTranscripts()
            await loadStats()
            setFlashId(result.id)
            setSelectedId(result.id)
            setTimeout(() => setFlashId(null), 1500)
            fireToast(result.pasted ? 'Pasted into your app' : 'Copied to clipboard')
          }
          invoke('get_trial_status').then(t => setTrial(t)).catch(() => {})
        } catch (e) {
          setError('Transcription failed — ' + (e || 'unknown error'))
          console.error('Transcription error:', e)
        }
        applyStatus('ready')
      }
    } finally {
      inflightRef.current = false
    }
  }, [loadTranscripts, loadStats, applyStatus])

  const handleCancel = useCallback(async () => {
    if (inflightRef.current) return
    if (statusRef.current === 'recording') {
      try { await invoke('cancel_recording') } catch {}
      applyStatus('ready')
    }
  }, [applyStatus])

  // Keep refs updated
  handleToggleRef.current = handleToggle
  handleCancelRef.current = handleCancel

  // Escape cancels a recording while the dashboard itself is focused.
  useEffect(() => {
    const onKey = (e) => {
      if (e.key === 'Escape' && statusRef.current === 'recording') {
        handleCancelRef.current()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

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

  const handleSaveEdit = useCallback(async (id, text) => {
    try {
      await invoke('update_transcript', { id, text })
      await loadTranscripts()
      await loadStats()
      fireToast('Transcript updated')
      return true
    } catch (e) {
      console.error('Update failed:', e)
      fireToast('Update failed')
      return false
    }
  }, [loadTranscripts, loadStats, fireToast])

  const handleExport = useCallback(async (format) => {
    const meta = {
      txt: { name: 'Plain text', ext: 'txt' },
      md: { name: 'Markdown', ext: 'md' },
      json: { name: 'JSON', ext: 'json' },
      csv: { name: 'CSV', ext: 'csv' },
    }[format] || { name: 'File', ext: 'txt' }
    try {
      const path = await invoke('plugin:dialog|save', {
        options: {
          title: 'Export transcripts',
          defaultPath: `alan-echo-transcripts.${meta.ext}`,
          filters: [{ name: meta.name, extensions: [meta.ext] }],
        },
      })
      if (!path) return
      await invoke('export_transcripts', { path, format })
      fireToast('Exported ' + String(path).split(/[\\/]/).pop())
    } catch (e) {
      console.error('Export failed:', e)
      fireToast('Export failed')
    }
  }, [fireToast])

  // ── Window controls ────────────────────────────────────────────────
  const handleMinimize = async () => { const { getCurrentWindow } = await import('@tauri-apps/api/window'); getCurrentWindow().minimize() }
  const handleMaximize = async () => { const { getCurrentWindow } = await import('@tauri-apps/api/window'); getCurrentWindow().toggleMaximize() }
  // close() routes through Rust's CloseRequested handler → hides to tray.
  const handleClose = async () => { const { getCurrentWindow } = await import('@tauri-apps/api/window'); getCurrentWindow().close() }

  // ── Clear error after timeout ──────────────────────────────────────
  useEffect(() => {
    if (error) { const t = setTimeout(() => setError(null), 6000); return () => clearTimeout(t) }
  }, [error])

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100vh', background: 'var(--bg-primary)' }}>
      <TitleBar status={status} elapsed={elapsed} onSettings={() => setShowSettings(!showSettings)} onMinimize={handleMinimize} onMaximize={handleMaximize} onClose={handleClose} />

      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', padding: '12px 16px', gap: 12, overflow: 'hidden' }}>
        {trial && !trial.licensed && (
          <TrialBanner used={trial.used} limit={trial.limit} remaining={trial.remaining} />
        )}
        <QuickStats total={stats.total} words={stats.words} duration={stats.duration} />

        <StatusPanel status={status} elapsed={elapsed} cap={MAX_RECORDING_SECONDS} hotkeys={hotkeys} onToggle={handleToggle} onCancel={handleCancel} />

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
          <div style={{ flex: 1, minWidth: 200, display: 'flex', flexDirection: 'column', gap: 'var(--echo-list-gap)', overflow: 'auto', paddingRight: 4 }}>
            {displayList.length === 0 ? <EmptyState query={query} hotkeys={hotkeys} /> : displayList.map(t => (
              <TranscriptCard key={t.id} transcript={t} selected={t.id === selectedId} isNew={t.id === flashId} onClick={() => setSelectedId(t.id)} />
            ))}
            {hasMore && !query && (
              <button
                onClick={loadMoreTranscripts}
                disabled={loadingMore}
                style={{
                  padding: '8px 0', fontSize: 11, fontFamily: 'var(--font-mono)',
                  color: 'var(--text-muted)', background: 'none',
                  border: '1px solid var(--border-primary)', borderRadius: 'var(--echo-radius-sm)',
                  cursor: loadingMore ? 'default' : 'pointer', marginTop: 4,
                }}
              >
                {loadingMore ? 'Loading...' : 'Load more'}
              </button>
            )}
          </div>
          <div style={{ flex: 1, minWidth: 200 }}>
            <DetailPanel transcript={selected} onCopy={handleCopy} onDelete={handleDelete} onSaveEdit={handleSaveEdit} />
          </div>
        </div>
      </div>

      <FooterBar hotkeys={hotkeys} />
      <SettingsPanel open={showSettings} onClose={() => { setShowSettings(false); loadSettings() }} hotkeys={hotkeys} />

      {showOnboarding && (
        <Onboarding
          hotkeys={hotkeys}
          onDone={async () => {
            setShowOnboarding(false)
            try { await invoke('set_setting', { key: 'onboarding_complete', value: true }) } catch {}
            try { await invoke('set_autostart', { enabled: true }) } catch {}
            loadTranscripts()
            loadStats()
          }}
        />
      )}

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

function TrialBanner({ used, limit, remaining }) {
  const atLimit = remaining <= 0
  return (
    <div style={{
      padding: '7px 12px', fontSize: 11, display: 'flex', alignItems: 'center', gap: 8,
      background: atLimit
        ? 'color-mix(in srgb, var(--accent-red) 8%, var(--bg-card))'
        : 'color-mix(in srgb, var(--brass) 8%, var(--bg-card))',
      border: `1px solid color-mix(in srgb, ${atLimit ? 'var(--accent-red)' : 'var(--brass)'} 25%, transparent)`,
      borderRadius: 'var(--echo-radius-sm)',
    }}>
      <span style={{ color: atLimit ? 'var(--accent-red)' : 'var(--brass)', fontWeight: 600 }}>
        {atLimit ? 'Trial limit reached' : `Trial · ${remaining} of ${limit} remaining today`}
      </span>
      <span style={{ flex: 1 }} />
      <a
        href="https://alanglobalintelligence.com/echo"
        onClick={e => {
          e.preventDefault()
          invoke('plugin:shell|open', { path: 'https://alanglobalintelligence.com/echo' }).catch(() => {
            window.open('https://alanglobalintelligence.com/echo', '_blank', 'noopener')
          })
        }}
        style={{ color: 'var(--echo-accent)', fontSize: 11, textDecoration: 'none', fontWeight: 600 }}
      >
        {atLimit ? 'Get unlimited' : 'Upgrade'}
      </a>
    </div>
  )
}

function EmptyState({ query, hotkeys }) {
  if (query) {
    return (
      <div style={{ padding: 40, textAlign: 'center' }}>
        <div style={{ fontSize: 13, color: 'var(--text-muted)', marginBottom: 4 }}>No transcriptions match "<strong>{query}</strong>"</div>
        <div style={{ fontSize: 12, color: 'var(--text-faint)' }}>Try a different search term.</div>
      </div>
    )
  }
  // hotkeys.toggle === null means registration FAILED (vs undefined = loading)
  const toggleDead = hotkeys?.toggle === null
  return (
    <div style={{ padding: 40, textAlign: 'center' }}>
      <Icon name="mic" size={32} color="var(--text-faint)" style={{ opacity: 0.5, marginBottom: 12 }} />
      <div style={{ fontSize: 14, fontWeight: 500, color: 'var(--text-secondary)', marginBottom: 6 }}>Nothing captured yet</div>
      <div style={{ fontSize: 12, color: 'var(--text-muted)' }}>
        {toggleDead ? (
          <>The dictation hotkey is unavailable — another app owns it.<br />
          Start dictation from the tray menu or the panel above.</>
        ) : (
          <>Press <strong>{hotkeys?.toggle || 'Ctrl + Shift + Space'}</strong> to start your first dictation.
          <br />Your transcriptions will appear here automatically.</>
        )}
      </div>
    </div>
  )
}
