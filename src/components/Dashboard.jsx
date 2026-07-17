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
  const [pending, setPending] = useState(0)
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

  // ── Recording timer ────────────────────────────────────────────────
  useEffect(() => {
    if (status === 'recording') {
      const start = Date.now()
      timerRef.current = setInterval(() => {
        setElapsed(Math.floor((Date.now() - start) / 1000))
      }, 1000)
    } else if (status === 'ready') {
      clearInterval(timerRef.current)
      setElapsed(0)
    } else {
      clearInterval(timerRef.current)
    }
    return () => clearInterval(timerRef.current)
  }, [status])

  // ── Dictation (mirrors the Rust state machine) ─────────────────────
  // The hotkey, recorder, beeps, 5:00 cap, transcription, and paste all run
  // in Rust — reliable even when this webview is throttled or suspended in
  // the tray. The UI only mirrors backend state from 'dictation' events, so
  // the buttons and the hotkey can never disagree about what's happening.
  const applyStatus = useCallback((next) => {
    statusRef.current = next
    setStatus(next)
  }, [])

  const fireToast = useCallback((msg) => {
    setToast(msg)
    setTimeout(() => setToast(null), 1800)
  }, [])

  const handleToggle = useCallback(() => {
    // Fire-and-forget: Rust serializes/debounces presses and reports back
    // via 'dictation' events.
    invoke('toggle_dictation').catch(() => {})
  }, [])

  const handleCancel = useCallback(() => {
    invoke('cancel_dictation').catch(() => {})
  }, [])

  useEffect(() => {
    let cancelled = false
    // Sync on mount in case the webview (re)loaded mid-recording.
    invoke('get_dictation_state').then(s => {
      if (cancelled || !s) return
      applyStatus(s.recording ? 'recording' : 'ready')
      setPending(s.pending || 0)
    }).catch(() => {})
    const unsub = listen('dictation', (e) => {
      if (cancelled) return
      const p = e?.payload || {}
      switch (p.type) {
        case 'recording-started':
          setError(null)
          applyStatus('recording')
          break
        case 'recording-stopped':
          applyStatus('ready')
          break
        case 'pending':
          setPending(p.count ?? 0)
          break
        case 'transcript':
          if (p.empty) {
            setError('Nothing worth keeping was heard — try again')
          } else if (p.id) {
            loadTranscripts()
            loadStats()
            setFlashId(p.id)
            setSelectedId(p.id)
            setTimeout(() => setFlashId(null), 1500)
            fireToast(p.pasted ? 'Pasted into your app' : 'Copied to clipboard')
          }
          invoke('get_trial_status').then(t => setTrial(t)).catch(() => {})
          break
        case 'error':
          setError(p.message || 'Something went wrong')
          break
        default:
          break
      }
    })
    return () => { cancelled = true; unsub.then(fn => fn()) }
  }, [applyStatus, loadTranscripts, loadStats, fireToast])

  // Escape cancels a recording while the dashboard itself is focused.
  useEffect(() => {
    const onKey = (e) => {
      if (e.key === 'Escape' && statusRef.current === 'recording') {
        invoke('cancel_dictation').catch(() => {})
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

  // Recorder truth is `status` (ready|recording); `pending` counts background
  // transcriptions. The badge merges them for DISPLAY only — the timer, the cap
  // effect, and the toggle decision all key on raw `status`, never this.
  const panelStatus = status === 'recording' ? 'recording' : (pending > 0 ? 'processing' : 'ready')

  // ── Actions ────────────────────────────────────────────────────────
  // Backend re-paste-last hotkey: the paste already happened in Rust, this is
  // just user feedback.
  useEffect(() => {
    let cancelled = false
    const unsub = listen('paste-last', (e) => {
      if (cancelled) return
      fireToast(e?.payload?.pasted ? 'Re-pasted last transcript' : 'Copied last transcript')
    })
    return () => { cancelled = true; unsub.then(fn => fn()) }
  }, [fireToast])

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
      <TitleBar status={panelStatus} elapsed={elapsed} onSettings={() => setShowSettings(!showSettings)} onMinimize={handleMinimize} onMaximize={handleMaximize} onClose={handleClose} />

      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', padding: '12px 16px', gap: 12, overflow: 'hidden' }}>
        {trial && !trial.licensed && (
          <TrialBanner used={trial.used} limit={trial.limit} remaining={trial.remaining} lifetime_used={trial.lifetime_used} lifetime_limit={trial.lifetime_limit} lifetime_remaining={trial.lifetime_remaining} />
        )}
        <QuickStats total={stats.total} words={stats.words} duration={stats.duration} />

        <StatusPanel status={panelStatus} elapsed={elapsed} cap={MAX_RECORDING_SECONDS} hotkeys={hotkeys} onToggle={handleToggle} onCancel={handleCancel} />

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

function TrialBanner({ used, limit, remaining, lifetime_used, lifetime_limit, lifetime_remaining }) {
  const expired = lifetime_remaining != null && lifetime_remaining <= 0
  const atLimit = expired || remaining <= 0
  const lifetimeNote = lifetime_used != null ? ` (${lifetime_used} of ${lifetime_limit} total)` : ''
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
        {expired ? 'Trial ended' : atLimit ? 'Daily limit reached' : `Trial · ${remaining} of ${limit} remaining today`}{lifetimeNote}
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
        {atLimit ? 'Get unlimited — $89' : 'Upgrade'}
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
