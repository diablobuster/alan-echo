import { useState, useEffect, useRef } from 'react'
import Icon from './Icons'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { applyTheme } from '../theme'
import pkg from '../../package.json'
import notices from '../legal/third-party-notices.txt?raw'

// Must contain aggressive-only tokens ("gonna", "in order to") so the preview
// visibly differs between standard and aggressive — pinned by the
// levels_visibly_differ_on_settings_sample test in text_cleanup.rs.
const CLEANUP_SAMPLE = "um so basically we're gonna need to uh move the the meeting to friday in order to hit the api deadline you know"
const MODEL_LABELS = { base: 'Basic', small: 'Standard', medium: 'Enhanced', 'large-v3': 'Ultra' }
const LABEL_TO_MODEL = { Basic: 'base', Standard: 'small', Enhanced: 'medium', Ultra: 'large-v3' }

export default function SettingsPanel({ open, onClose, hotkeys = {} }) {
  const [settings, setSettings] = useState({
    auto_paste: true,
    sound_enabled: true,
    text_cleanup_level: 'standard',
  })
  const [devices, setDevices] = useState([])
  const [selectedDevice, setSelectedDevice] = useState(null)
  const [engine, setEngine] = useState(null)
  const [preview, setPreview] = useState(null)
  const [modelError, setModelError] = useState(null)
  const [models, setModels] = useState([])
  const [autostart, setAutostart] = useState(false)
  const [hasMultilingualModel, setHasMultilingualModel] = useState(true)
  const [modelDownload, setModelDownload] = useState(null) // null | {stage, percent, ...}
  const [showNotices, setShowNotices] = useState(false)
  const pollRef = useRef(null)

  const LANGUAGES = [
    { value: 'en', label: 'English' },
    { value: 'es', label: 'Spanish', beta: true },
    { value: 'fr', label: 'French', beta: true },
    { value: 'de', label: 'German', beta: true },
    { value: 'pt', label: 'Portuguese', beta: true },
    { value: 'ja', label: 'Japanese', beta: true },
    { value: 'zh', label: 'Chinese', beta: true },
    { value: 'ko', label: 'Korean', beta: true },
    { value: 'it', label: 'Italian', beta: true },
    { value: 'nl', label: 'Dutch', beta: true },
    { value: 'auto', label: 'Auto-detect', beta: true },
  ]

  const loadEngineInfo = async () => {
    try {
      const info = await invoke('get_engine_info')
      setEngine(info)
      return info
    } catch { return null }
  }

  // clearInterval+setInterval run in one synchronous block, and a stale
  // interval clears only its own id — concurrent model switches can't orphan
  // a poll that then runs forever.
  const startEnginePoll = () => {
    clearInterval(pollRef.current)
    const id = setInterval(async () => {
      const info = await loadEngineInfo()
      if (info?.ready || info?.status?.startsWith('failed')) clearInterval(id)
    }, 800)
    pollRef.current = id
  }

  useEffect(() => {
    if (!open) return
    async function load() {
      try {
        const s = await invoke('get_settings')
        setSettings(prev => ({ ...prev, ...s }))
        if (s.microphone_device) setSelectedDevice(s.microphone_device)
        const devs = await invoke('list_audio_devices')
        setDevices(devs || [])
        try { setModels(await invoke('list_models') || []) } catch {}
        try { setAutostart(await invoke('get_autostart')) } catch {}
        try { setHasMultilingualModel(await invoke('has_multilingual_model')) } catch {}
        const info = await loadEngineInfo()
        // Resume polling if the panel reopens while a model is still loading.
        if (info && !info.ready && !info.status?.startsWith('failed')) startEnginePoll()
        const cleaned = await invoke('clean_text', { text: CLEANUP_SAMPLE })
        setPreview(cleaned)
      } catch (e) { console.error('Settings error:', e) }
    }
    load()
    const unlisten = listen('model_download_progress', (event) => {
      const d = event.payload
      setModelDownload(d)
      if (d.stage === 'done') {
        // The done event carries no model identity — re-ask the backend
        // instead of assuming the multilingual model just landed (an
        // English-only download used to falsely flip this flag).
        invoke('has_multilingual_model').then(v => setHasMultilingualModel(!!v)).catch(() => {})
        setModelDownload(null)
        invoke('list_models').then(m => setModels(m || [])).catch(() => {})
        const lang = pendingLangRef.current
        if (lang) {
          // Commit the language ONLY now that its model is on disk.
          pendingLangRef.current = null
          updateSetting('language', lang).then(() => { loadEngineInfo(); startEnginePoll() })
          setModelError(null)
        } else {
          setModelError('Model downloaded! Select it again to activate.')
        }
      }
      if (d.stage === 'error') {
        setModelError(d.error || 'Download failed')
        setModelDownload(null)
        pendingLangRef.current = null // language stays unchanged on failure
      }
    })
    return () => { clearInterval(pollRef.current); unlisten.then(fn => fn()) }
  }, [open])

  // Language chosen while its model download is still in flight — committed
  // only when the download reports done, so a failed download can't leave the
  // app configured for a language no installed model can transcribe.
  const pendingLangRef = useRef(null)
  // What the shared model_download_progress stream is currently carrying —
  // 'multilingual' (language flow) or 'model' (speech-model picker).
  const downloadKindRef = useRef('multilingual')

  const updateSetting = async (key, value) => {
    const prev = settings[key]
    setSettings(p => ({ ...p, [key]: value }))
    try {
      await invoke('set_setting', { key, value })
    } catch (e) {
      // Roll back — an optimistic toggle must not show a state the backend
      // refused (e.g. auto-paste "off" while the backend keeps pasting).
      setSettings(p => ({ ...p, [key]: prev }))
      console.error('Save failed:', e)
      setModelError(String(e))
    }
  }

  const changeModel = async (label) => {
    const name = LABEL_TO_MODEL[label] || 'base'
    setModelError(null)
    const isAvailable = models.find(m => m.label === label)?.available !== false
    if (!isAvailable) {
      const sizes = { Basic: '~148 MB', Standard: '~488 MB', Enhanced: '~1.5 GB', Ultra: '~3.1 GB' }
      downloadKindRef.current = 'model'
      setModelError(`Downloading ${label} model (${sizes[label] || ''})...`)
      try {
        await invoke('download_model', { name })
      } catch (e) {
        setModelError(String(e))
        return
      }
      return
    }
    try {
      await invoke('set_setting', { key: 'whisper_model', value: name })
      setSettings(prev => ({ ...prev, whisper_model: name }))
    } catch (e) {
      setModelError(String(e))
      return
    }
    loadEngineInfo()
    startEnginePoll()
  }

  const changeCleanup = async (level) => {
    await updateSetting('text_cleanup_level', level)
    try {
      const cleaned = await invoke('clean_text', { text: CLEANUP_SAMPLE })
      setPreview(cleaned)
    } catch {}
  }

  const changeTheme = async (label) => {
    const theme = label === 'Dark' ? 'dark' : 'light'
    applyTheme({ theme })
    await updateSetting('theme', theme)
  }

  if (!open) return null

  const activeModelLabel = engine?.model_label
    || MODEL_LABELS[settings.whisper_model]
    || 'Basic'

  // Grey out models that aren't on disk (retail installs bundle base.en only).
  // Until list_models loads, disable nothing — never block the active model.
  const disabledModels = models.filter(m => !m.available).map(m => m.label)

  return (
    <>
      {/* Backdrop */}
      <div onClick={onClose} style={{
        position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.2)', zIndex: 100,
        animation: 'echo-fade-in 0.2s ease-out both',
      }} />

      {/* Panel */}
      <div style={{
        position: 'fixed', top: 0, right: 0, bottom: 0, width: 'min(380px, 85vw)',
        background: 'var(--bg-primary)', borderLeft: '1px solid var(--border-primary)',
        zIndex: 101, display: 'flex', flexDirection: 'column',
        animation: 'echo-slide-in 0.22s ease-out both',
      }}>
        {/* Header */}
        <div style={{
          padding: '14px 16px', display: 'flex', alignItems: 'center',
          borderBottom: '1px solid var(--border-primary)',
        }}>
          <span style={{ fontSize: 14, fontWeight: 600 }}>Settings</span>
          <div style={{ flex: 1 }} />
          <button onClick={onClose} aria-label="Close settings" style={{
            background: 'none', border: 'none', cursor: 'pointer', padding: 4,
            color: 'var(--text-muted)', borderRadius: 4,
          }}>
            <Icon name="x" size={16} />
          </button>
        </div>

        {/* Content */}
        <div style={{ flex: 1, overflow: 'auto', padding: 16 }}>
          {/* Engine section */}
          <div className="echo-eyebrow" style={{ marginBottom: 12 }}>Engine</div>

          <SettingsRow label="Speech model" hint="Higher quality = more accurate, slower">
            <Seg
              label="Speech model"
              options={['Basic', 'Standard', 'Enhanced', 'Ultra']}
              value={activeModelLabel}
              onChange={changeModel}
              disabled={disabledModels}
            />
          </SettingsRow>
          {modelError && (
            <div style={{ fontSize: 10, color: 'var(--accent-yellow)', marginBottom: 4 }}>{modelError}</div>
          )}

          <EngineStatus engine={engine} />

          <SettingsRow label="Language" hint="Which language you'll dictate in">
            <select
              value={settings.language || 'en'}
              onChange={async e => {
                const lang = e.target.value
                if (lang !== 'en' && !hasMultilingualModel) {
                  // The invoke returns as soon as the download THREAD starts —
                  // hold the language in pendingLangRef and commit it from the
                  // 'done' event, never before the model is actually on disk.
                  setModelError(null)
                  downloadKindRef.current = 'multilingual'
                  pendingLangRef.current = lang
                  setModelDownload({ stage: 'downloading', percent: 0 })
                  try {
                    await invoke('download_multilingual_model')
                  } catch (err) {
                    setModelError(String(err))
                    setModelDownload(null)
                    pendingLangRef.current = null
                  }
                  return
                }
                setModelError(null)
                await updateSetting('language', lang)
                loadEngineInfo()
                startEnginePoll()
              }}
              style={{
                background: 'var(--bg-card)', border: '1px solid var(--border-primary)',
                borderRadius: 'var(--echo-radius-sm)', padding: '4px 8px',
                fontSize: 11, fontFamily: 'var(--font-sans)', color: 'var(--text-primary)',
                width: '100%',
              }}
            >
              {LANGUAGES.map(l => (
                <option key={l.value} value={l.value}>
                  {l.label}{l.beta ? ' (beta)' : ''}
                </option>
              ))}
            </select>
          </SettingsRow>
          {modelDownload && modelDownload.stage === 'downloading' && (
            <div style={{
              margin: '4px 0 8px', padding: '8px 10px',
              background: 'var(--bg-card)', border: '1px solid var(--border-primary)',
              borderRadius: 'var(--echo-radius-sm)', fontSize: 11,
            }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 6 }}>
                <span style={{ color: 'var(--echo-accent)', fontWeight: 600 }}>
                  {downloadKindRef.current === 'model' ? 'Downloading speech model' : 'Downloading language support'}
                </span>
                <span style={{ color: 'var(--text-faint)', fontVariantNumeric: 'tabular-nums' }}>
                  {modelDownload.downloaded_mb || 0}{modelDownload.total_mb ? ` / ${modelDownload.total_mb} MB` : ' MB'}
                </span>
              </div>
              <div style={{ width: '100%', height: 4, background: 'var(--bg-tertiary)', borderRadius: 2, overflow: 'hidden' }}>
                <div style={{
                  width: `${modelDownload.percent || 0}%`, height: '100%',
                  background: 'var(--echo-accent)', borderRadius: 2, transition: 'width 0.3s',
                }} />
              </div>
            </div>
          )}

          <GpuTestRow savedResult={settings.gpu_test} />

          <GpuPackRow onEngineRestart={() => { loadEngineInfo(); startEnginePoll() }} />

          {/* Microphone */}
          <div className="echo-eyebrow" style={{ marginTop: 20, marginBottom: 12 }}>Microphone</div>
          {devices.length === 0 && (
            <div style={{
              padding: '8px 10px', marginBottom: 8, fontSize: 11, lineHeight: 1.5,
              background: 'color-mix(in srgb, var(--accent-yellow) 8%, var(--bg-card))',
              border: '1px solid color-mix(in srgb, var(--accent-yellow) 30%, transparent)',
              borderRadius: 'var(--echo-radius-sm)', color: 'var(--text-primary)',
            }}>
              No microphone detected. Make sure a microphone is connected and that your system allows this app to access it (check Privacy &amp; Security &rarr; Microphone in your system settings).
            </div>
          )}
          <SettingsRow label="Input device">
            <select
              value={selectedDevice || ''}
              onChange={e => {
                setSelectedDevice(e.target.value)
                updateSetting('microphone_device', e.target.value || null)
              }}
              style={{
                background: 'var(--bg-card)', border: '1px solid var(--border-primary)',
                borderRadius: 'var(--echo-radius-sm)', padding: '4px 8px',
                fontSize: 11, fontFamily: 'var(--font-sans)', color: 'var(--text-primary)',
                width: '100%',
              }}
            >
              <option value="">System default</option>
              {devices.map(d => (
                <option key={d.index} value={d.name}>
                  {d.name} {d.is_default ? '(default)' : ''}
                </option>
              ))}
            </select>
          </SettingsRow>

          <MicTest />

          {/* Behavior section */}
          <div className="echo-eyebrow" style={{ marginTop: 20, marginBottom: 12 }}>Behavior</div>

          <SettingsRow label="Auto-paste" hint="Automatically paste transcription into focused app">
            <Toggle label="Auto-paste" checked={settings.auto_paste !== false} onChange={v => updateSetting('auto_paste', v)} />
          </SettingsRow>

          <SettingsRow label="Launch at startup" hint="Start ALAN Echo when you log in">
            <Toggle label="Launch at startup" checked={autostart} onChange={async v => {
              setAutostart(v)
              try { await invoke('set_autostart', { enabled: v }) } catch (e) { console.error('Autostart error:', e); setAutostart(!v) }
            }} />
          </SettingsRow>

          <SettingsRow label="Sound feedback" hint="Play beeps when recording starts and stops">
            <Toggle label="Sound feedback" checked={settings.sound_enabled !== false} onChange={v => updateSetting('sound_enabled', v)} />
          </SettingsRow>

          <SettingsRow label="Text cleanup" hint="How aggressively to clean up transcriptions">
            <Seg
              label="Text cleanup level"
              options={['verbatim', 'light', 'standard', 'aggressive']}
              value={settings.text_cleanup_level || 'standard'}
              onChange={changeCleanup}
            />
          </SettingsRow>

          {preview != null && (
            <div style={{
              margin: '4px 0 8px', padding: '8px 10px',
              background: 'var(--bg-card)', border: '1px solid var(--border-primary)',
              borderRadius: 'var(--echo-radius-sm)', fontSize: 11, lineHeight: 1.5,
            }}>
              <div style={{ color: 'var(--text-faint)', textDecoration: 'line-through' }}>
                "{CLEANUP_SAMPLE}"
              </div>
              <div style={{ color: 'var(--text-secondary)', marginTop: 4, display: 'flex', gap: 5, alignItems: 'baseline' }}>
                <Icon name="sparkle" size={11} color="var(--brass)" style={{ flexShrink: 0, alignSelf: 'center' }} />
                <span>"{preview}"</span>
              </div>
            </div>
          )}

          {/* Appearance */}
          <div className="echo-eyebrow" style={{ marginTop: 20, marginBottom: 12 }}>Appearance</div>
          <SettingsRow label="Theme">
            <Seg
              label="Theme"
              options={['Light', 'Dark']}
              value={settings.theme === 'dark' ? 'Dark' : 'Light'}
              onChange={changeTheme}
            />
          </SettingsRow>

          {/* Hotkeys section */}
          <div className="echo-eyebrow" style={{ marginTop: 20, marginBottom: 12 }}>Hotkeys</div>

          <HotkeyRow
            label="Toggle dictation"
            keys={hotkeys.toggle === null ? 'Unavailable' : (hotkeys.toggle || 'Ctrl + Shift + Space')}
            warn={hotkeys.toggle === null}
          />
          <HotkeyRow label="Cancel recording" keys={hotkeys.cancel || 'Unavailable'} warn={!hotkeys.cancel} />
          <HotkeyRow label="Show dashboard" keys={hotkeys.show === null ? 'Unavailable' : (hotkeys.show || 'Ctrl + Shift + H')} warn={hotkeys.show === null} />
          {hotkeys.toggle === null && (
            <div style={{ fontSize: 10, color: 'var(--accent-yellow)', marginTop: 4 }}>
              Dictation hotkey could not be registered — another app may be using it. Use the tray menu's Start Dictation instead.
            </div>
          )}
          {!hotkeys.cancel && (
            <div style={{ fontSize: 10, color: 'var(--accent-yellow)', marginTop: 4 }}>
              Cancel hotkey could not be registered — another app may be using it. Press Esc in the dashboard instead.
            </div>
          )}

          {/* About & legal */}
          <div className="echo-eyebrow" style={{ marginTop: 20, marginBottom: 12 }}>About</div>

          <div style={{ fontSize: 11, color: 'var(--text-secondary)', lineHeight: 1.7 }}>
            <div>ALAN Echo v{pkg.version}</div>
            <div>© 2026 ALAN Global Intelligence. All rights reserved.</div>
            <div style={{ marginTop: 6 }}>
              {[
                { label: 'License Agreement', url: 'https://www.alanglobalintelligence.com/legal/echo-license' },
                { label: 'Privacy Policy', url: 'https://www.alanglobalintelligence.com/privacy' },
              ].map((link, i) => (
                <span key={link.url}>
                  {i > 0 && <span style={{ color: 'var(--text-faint)' }}> · </span>}
                  <a
                    href={link.url}
                    onClick={e => {
                      e.preventDefault()
                      invoke('plugin:shell|open', { path: link.url }).catch(() => {
                        window.open(link.url, '_blank', 'noopener')
                      })
                    }}
                    style={{ color: 'var(--echo-accent)', textDecoration: 'none' }}
                  >
                    {link.label}
                  </a>
                </span>
              ))}
              <span style={{ color: 'var(--text-faint)' }}> · </span>
              <button
                onClick={() => setShowNotices(v => !v)}
                style={{
                  background: 'none', border: 'none', padding: 0, cursor: 'pointer',
                  fontSize: 11, fontFamily: 'inherit', color: 'var(--echo-accent)',
                }}
              >
                Open-source licenses
              </button>
            </div>
          </div>

          {showNotices && (
            <div style={{
              marginTop: 8, maxHeight: '40vh', overflowY: 'auto',
              background: 'var(--bg-card)', border: '1px solid var(--border-primary)',
              borderRadius: 'var(--echo-radius-sm)', padding: '8px 10px',
            }}>
              <pre style={{ whiteSpace: 'pre-wrap', fontSize: 9, lineHeight: 1.5, margin: 0, color: 'var(--text-secondary)' }}>
                {notices}
              </pre>
              <button
                onClick={() => setShowNotices(false)}
                style={{
                  marginTop: 8, padding: '5px 10px', background: 'none',
                  border: '1px solid var(--border-primary)', borderRadius: 'var(--echo-radius-sm)',
                  fontSize: 10, cursor: 'pointer', color: 'var(--text-secondary)', fontFamily: 'inherit',
                }}
              >
                Close
              </button>
            </div>
          )}
        </div>

        {/* ALAN Platform links */}
        <div style={{
          padding: '12px 16px', borderTop: '1px solid var(--border-primary)',
        }}>
          <div className="echo-eyebrow" style={{ marginBottom: 8 }}>ALAN Platform</div>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
            {[
              { label: 'Research Terminal', path: '/research' },
              { label: 'Global Dashboard', path: '/dashboard' },
              { label: 'Support', path: '/contact' },
            ].map(link => (
              <a
                key={link.path}
                href={`https://alanglobalintelligence.com${link.path}`}
                onClick={e => {
                  e.preventDefault()
                  invoke('plugin:shell|open', { path: `https://alanglobalintelligence.com${link.path}` }).catch(() => {
                    window.open(`https://alanglobalintelligence.com${link.path}`, '_blank', 'noopener')
                  })
                }}
                style={{ fontSize: 11, color: 'var(--text-secondary)', textDecoration: 'none', padding: '3px 0' }}
              >
                {link.label} <span style={{ color: 'var(--text-faint)', fontSize: 9 }}>&rarr;</span>
              </a>
            ))}
          </div>
          <div style={{ marginTop: 10, textAlign: 'center' }}>
            <span className="echo-mono" style={{ fontSize: 9, letterSpacing: '0.1em', textTransform: 'uppercase', color: 'var(--text-faint)' }}>
              Part of ALAN Global Intelligence &middot; v{pkg.version}
            </span>
          </div>
        </div>
      </div>
    </>
  )
}

function EngineStatus({ engine }) {
  const ready = engine?.ready
  const loading = engine?.status === 'loading model'
  const color = ready ? 'var(--accent-green)' : loading ? 'var(--accent-yellow)' : 'var(--accent-red)'
  const statusText = ready
    ? `Engine ready — ${engine?.model_label || ''} model`.trim()
    : loading ? 'Loading model...'
    : engine?.status ? `Engine ${engine.status}` : 'Checking engine...'
  // engine_kind reports the binary actually running — a present GPU with the
  // CPU engine active means the acceleration pack isn't installed yet.
  // Capability language only — never underlying tech/brand names (no GPU
  // vendor strings, no acceleration-framework names).
  const computeText = engine
    ? (engine.engine_kind === 'cuda'
        ? `GPU acceleration active${engine.vram_mb ? ` · ${Math.round(engine.vram_mb / 1024)} GB graphics memory` : ''}`
        : engine.engine_kind === 'vulkan'
        ? `GPU acceleration active (beta)`
        : engine.cuda
        ? `CPU engine · ${engine.cpu_cores} cores (GPU acceleration available)`
        : `CPU only · ${engine.cpu_cores} cores`)
    : ''

  return (
    <>
      <SettingsRow label="Status" hint="">
        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
          <span style={{
            width: 6, height: 6, borderRadius: '50%', background: color,
            animation: loading ? 'echo-blink 1.2s ease-in-out infinite' : 'none',
          }} />
          <span className="echo-mono" style={{ fontSize: 11, color: 'var(--text-muted)' }}>
            {statusText}
          </span>
        </div>
      </SettingsRow>
      {computeText && (
        <SettingsRow label="Compute" hint="">
          <div style={{ display: 'flex', alignItems: 'center', gap: 6, minWidth: 0 }}>
            <Icon name="cpu" size={12} color="var(--text-muted)" style={{ flexShrink: 0 }} />
            <span className="echo-mono" style={{ fontSize: 10, color: 'var(--text-muted)', wordBreak: 'break-word' }}>
              {computeText}
            </span>
          </div>
        </SettingsRow>
      )}
    </>
  )
}

/** Plain-language consequences of a GPU test result. Capability language
 *  only — hardware vendor/product names never reach the UI. */
function gpuVerdictText(r) {
  const vram = r.vram_mb ? ` (${Math.round(r.vram_mb / 1024)} GB graphics memory)` : ''
  if (r.verdict === 'cuda_ready') {
    return `Dedicated graphics card${vram} — GPU acceleration is installed and active. Short dictations transcribe in well under a second.`
  }
  if (r.verdict === 'cuda_available') {
    return `A compatible dedicated graphics card${vram} was found — but Echo is using the CPU engine right now. Enable GPU acceleration below to transcribe several times faster.`
  }
  const other = (r.display_gpus || []).filter(n => !/nvidia/i.test(n))
  if (r.verdict === 'vulkan_ready') {
    const active = r.engine_kind === 'vulkan'
    return `Your graphics card — the GPU acceleration pack (beta) is installed${active ? ' and active. Short dictations transcribe in well under a second' : ''}. If anything ever looks wrong, visit alanglobalintelligence.com/echo for support.`
  }
  if (r.verdict === 'vulkan_available') {
    return `We see your graphics card — enable the beta GPU acceleration pack below to transcribe several times faster than on CPU.`
  }
  // cpu_only: acknowledge what we DID find so owners of unrecognized cards
  // get a real answer.
  if (other.length > 0) {
    return `We see your graphics hardware, but Echo can't accelerate on it yet. Echo uses its CPU engine: expect about a second for short dictations on a modern ${r.cpu_cores}-core CPU.`
  }
  return `No dedicated GPU found. Echo uses its CPU engine: expect about a second for short dictations on a modern ${r.cpu_cores}-core CPU; very long clips take proportionally longer.`
}

/** Explicit "do I have a GPU?" test — runs a fresh probe, persists the
 *  result (settings.gpu_test), and states the consequences in plain words. */
function GpuTestRow({ savedResult }) {
  const [result, setResult] = useState(savedResult || null)
  const [testing, setTesting] = useState(false)
  const [error, setError] = useState(null)

  useEffect(() => { if (savedResult) setResult(savedResult) }, [savedResult])

  const runTest = async () => {
    setTesting(true)
    setError(null)
    try {
      setResult(await invoke('test_gpu'))
    } catch (e) {
      setError(String(e))
    }
    setTesting(false)
  }

  const testedDate = result?.tested_at
    ? new Date(result.tested_at).toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' })
    : null

  return (
    <div style={{ padding: '8px 0' }}>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12 }}>
        <div>
          <div style={{ fontSize: 12, fontWeight: 500, color: 'var(--text-primary)' }}>Test my GPU</div>
          <div style={{ fontSize: 10, color: 'var(--text-faint)', marginTop: 1 }}>
            {testedDate ? `Last tested ${testedDate}` : 'Check what your hardware means for speed'}
          </div>
        </div>
        <button type="button" onClick={runTest} disabled={testing} style={{
          padding: '4px 14px', fontSize: 11,
          background: 'var(--bg-card)', color: 'var(--text-secondary)',
          border: '1px solid var(--border-primary)', borderRadius: 'var(--echo-radius-sm)',
          cursor: testing ? 'default' : 'pointer', fontFamily: 'var(--font-sans)', flexShrink: 0,
        }}>{testing ? 'Testing…' : result ? 'Re-test' : 'Run test'}</button>
      </div>
      {result && (
        <div style={{
          marginTop: 6, padding: '8px 10px',
          background: 'var(--bg-card)', border: '1px solid var(--border-primary)',
          borderRadius: 'var(--echo-radius-sm)', fontSize: 11, lineHeight: 1.55,
          color: 'var(--text-secondary)', display: 'flex', gap: 6, alignItems: 'flex-start',
        }}>
          <Icon
            name={result.verdict === 'cpu_only' ? 'cpu' : 'check'}
            size={12}
            color={result.verdict.endsWith('_ready') ? 'var(--accent-green)' : result.verdict.endsWith('_available') ? 'var(--brass)' : 'var(--text-muted)'}
            style={{ flexShrink: 0, marginTop: 2 }}
          />
          <span>{gpuVerdictText(result)}</span>
        </div>
      )}
      {error && (
        <div className="echo-mono" style={{ fontSize: 10, color: 'var(--accent-red)', marginTop: 4 }}>{error}</div>
      )}
    </div>
  )
}

/** One-click GPU acceleration. The backend decides what to offer: the CUDA
 *  pack on NVIDIA machines, the beta Vulkan pack on AMD/Intel machines,
 *  nothing otherwise. Renders progress while the pack streams in; hands off
 *  to the engine poll once the accelerated build restarts. */
function GpuPackRow({ onEngineRestart }) {
  const [status, setStatus] = useState(null) // { gpu_name, installed, engine_kind, offer, offer_gpu, beta, progress }
  const [progress, setProgress] = useState(null)
  const [error, setError] = useState(null)
  const doneRef = useRef(false)

  const refresh = async () => {
    try {
      const s = await invoke('get_gpu_pack_status')
      setStatus(s)
      if (s?.progress?.state && s.progress.state !== 'idle') setProgress(s.progress)
      return s
    } catch { return null }
  }

  useEffect(() => {
    refresh()
    // Hold the promise, not the resolved fn — if the panel closes before
    // listen() resolves, an `if (unsub) unsub()` cleanup runs while unsub is
    // still null and leaks the listener for the window's lifetime.
    let cancelled = false
    const unlisten = listen('gpu-pack-progress', (event) => {
      if (cancelled) return
      const p = event.payload
      setProgress(p)
      if (p.state === 'failed') {
        setError(p.error || 'Download failed — try again')
        refresh() // a rollback may have disabled the pack — re-read reality
      }
      if (p.state === 'done' && !doneRef.current) {
        doneRef.current = true
        refresh()
        onEngineRestart()
      }
    })
    return () => { cancelled = true; unlisten.then(fn => fn()) }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const start = async () => {
    setError(null)
    doneRef.current = false
    try {
      await invoke('download_gpu_pack', { kind: status.offer })
      setProgress({ state: 'downloading', downloaded_mb: 0, total_mb: null })
    } catch (e) {
      setError(String(e))
    }
  }

  // Nothing to offer on this hardware.
  if (!status?.offer) return null

  const beta = !!status.beta
  // Never render the vendor/product name of the card — capability terms only.
  const gpuLabel = 'graphics card'
  const sizeText = status.offer === 'cuda' ? '~440 MB' : '~18 MB'
  const busy = progress && ['downloading', 'extracting', 'restarting'].includes(progress.state)
  const active = status.installed && !busy

  if (active && !progress) {
    // Pack on disk and no install in flight — the Compute row already says
    // which engine is active; stay quiet.
    return null
  }

  return (
    <div style={{ padding: '8px 0' }}>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12 }}>
        <div>
          <div style={{ fontSize: 12, fontWeight: 500, color: 'var(--text-primary)' }}>
            GPU acceleration{beta ? ' (beta)' : ''}
          </div>
          <div style={{ fontSize: 10, color: 'var(--text-faint)', marginTop: 1 }}>
            {busy
              ? progress.state === 'downloading'
                ? `Downloading… ${progress.downloaded_mb || 0}${progress.total_mb ? ` / ${progress.total_mb}` : ''} MB`
                : progress.state === 'extracting'
                ? 'Installing…'
                : 'Restarting the speech engine…'
              : progress?.state === 'done'
              ? `Active — transcribing on your ${gpuLabel}`
              : `Free one-time download (${sizeText}) — uses your ${gpuLabel}`}
          </div>
          {beta && !busy && progress?.state !== 'done' && (
            <div style={{ fontSize: 10, color: 'var(--text-faint)', marginTop: 2 }}>
              Beta — works on most AMD and Intel GPUs. If the driver can't run it, Echo
              switches back to the CPU engine automatically. Problems? support@alanglobalintelligence.com
            </div>
          )}
        </div>
        {!busy && progress?.state !== 'done' && (
          <button type="button" onClick={start} style={{
            padding: '4px 14px', fontSize: 11, background: 'var(--accent-green)', color: '#fff',
            border: 'none', borderRadius: 'var(--echo-radius-sm)', cursor: 'pointer',
            fontFamily: 'var(--font-sans)', fontWeight: 500, flexShrink: 0,
          }}>Enable</button>
        )}
        {progress?.state === 'done' && (
          <Icon name="check" size={14} color="var(--accent-green)" />
        )}
      </div>
      {busy && progress.state === 'downloading' && (
        <div style={{
          marginTop: 6, height: 4, borderRadius: 2, background: 'var(--bg-tertiary)', overflow: 'hidden',
        }}>
          <div style={{
            height: '100%', background: 'var(--accent-green)', transition: 'width 0.4s',
            width: progress.total_mb ? `${Math.min(100, Math.round((progress.downloaded_mb / progress.total_mb) * 100))}%` : '30%',
          }} />
        </div>
      )}
      {error && (
        <div className="echo-mono" style={{ fontSize: 10, color: 'var(--accent-red)', marginTop: 4 }}>{error}</div>
      )}
    </div>
  )
}

function SettingsRow({ label, hint, children }) {
  return (
    <div style={{
      display: 'flex', alignItems: 'center', justifyContent: 'space-between',
      padding: '8px 0', gap: 10, minWidth: 0,
    }}>
      <div style={{ minWidth: 0, flexShrink: 1 }}>
        <div style={{ fontSize: 12, fontWeight: 500, color: 'var(--text-primary)' }}>{label}</div>
        {hint && <div style={{ fontSize: 10, color: 'var(--text-faint)', marginTop: 1 }}>{hint}</div>}
      </div>
      <div style={{ flexShrink: 0, minWidth: 0 }}>{children}</div>
    </div>
  )
}

function Seg({ options, value, onChange, disabled = [], label }) {
  return (
    <div role="radiogroup" aria-label={label} style={{
      display: 'flex', background: 'var(--bg-secondary)',
      borderRadius: 'var(--echo-radius-sm)', padding: 2, gap: 1,
    }}>
      {options.map(opt => {
        const needsDownload = disabled.includes(opt) && opt !== value
        return (
          <button
            key={opt}
            role="radio"
            aria-checked={opt === value}
            onClick={() => onChange(opt)}
            title={needsDownload ? 'Click to download this model' : undefined}
            style={{
              padding: '3px 10px', fontSize: 10, fontFamily: 'var(--font-mono)',
              border: 'none', borderRadius: 'var(--echo-radius-sm)',
              cursor: 'pointer',
              fontWeight: opt === value ? 600 : 400,
              background: opt === value ? 'var(--bg-card)' : 'transparent',
              color: opt === value ? 'var(--text-primary)' : needsDownload ? 'var(--text-faint)' : 'var(--text-muted)',
              boxShadow: opt === value ? '0 1px 2px rgba(0,0,0,0.06)' : 'none',
              textDecoration: needsDownload ? 'underline dotted' : 'none',
            }}
          >
            {opt}
          </button>
        )
      })}
    </div>
  )
}

function Toggle({ checked, onChange, label }) {
  return (
    <button
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={() => onChange(!checked)}
      style={{
        width: 36, height: 20, borderRadius: 10, padding: 2, border: 'none', cursor: 'pointer',
        background: checked ? 'var(--accent-green)' : 'var(--bg-tertiary)',
        transition: 'background 0.15s',
        display: 'flex', alignItems: 'center',
      }}
    >
      <div style={{
        width: 16, height: 16, borderRadius: '50%', background: '#fff',
        transition: 'transform 0.15s',
        transform: checked ? 'translateX(16px)' : 'translateX(0)',
        boxShadow: '0 1px 2px rgba(0,0,0,0.15)',
      }} />
    </button>
  )
}

function HotkeyRow({ label, keys, warn }) {
  return (
    <div style={{
      display: 'flex', alignItems: 'center', justifyContent: 'space-between',
      padding: '6px 0',
    }}>
      <span style={{ fontSize: 12, color: 'var(--text-secondary)' }}>{label}</span>
      <span className="echo-mono" style={{ fontSize: 10, color: warn ? 'var(--accent-yellow)' : 'var(--text-faint)' }}>{keys}</span>
    </div>
  )
}

export function MicTest() {
  const [state, setState] = useState('idle') // idle | recording | playing
  const [audioUrl, setAudioUrl] = useState(null)
  const [error, setError] = useState(null)
  const [engineReady, setEngineReady] = useState(null) // null = checking, true/false
  const audioRef = useRef(null)
  const stateRef = useRef(state)
  stateRef.current = state

  // Check engine readiness on mount — the mic test itself doesn't need the
  // engine, but recording requires the backend to be fully initialised and
  // the license to be validated. Poll briefly so we know when to enable.
  useEffect(() => {
    let cancelled = false
    async function check() {
      for (let i = 0; i < 60; i++) {
        if (cancelled) return
        try {
          const info = await invoke('get_engine_info')
          if (info?.ready) { setEngineReady(true); return }
          if (info?.status?.startsWith('failed')) { setEngineReady(true); return } // let recording attempt surface its own error
        } catch {}
        await new Promise(r => setTimeout(r, 500))
      }
      // Timed out — enable anyway so the user isn't stuck
      if (!cancelled) setEngineReady(true)
    }
    invoke('get_engine_info').then(info => {
      if (info?.ready || info?.status?.startsWith('failed')) setEngineReady(true)
      else check()
    }).catch(() => setEngineReady(true))
    return () => { cancelled = true }
  }, [])

  // Discard an in-flight recording if the panel unmounts mid-test — an
  // abandoned recording would otherwise block dictation until restart.
  useEffect(() => () => {
    if (stateRef.current === 'recording') invoke('cancel_recording').catch(() => {})
  }, [])

  const startTest = async () => {
    setError(null)
    try {
      if (await invoke('is_recording')) {
        setError('Dictation in progress — stop it before testing the microphone.')
        return
      }
      await invoke('start_recording')
      setState('recording')
    } catch (e) {
      setError(String(e))
      console.error('Mic test failed:', e)
    }
  }

  const stopTest = async () => {
    try {
      const result = await invoke('stop_recording')
      if (result?.wav_path) {
        const b64 = await invoke('read_wav_base64', { wavPath: result.wav_path })
        const url = `data:audio/wav;base64,${b64}`
        setAudioUrl(url)
        setState('playing')
        setTimeout(() => {
          if (audioRef.current) {
            audioRef.current.play().catch((e) => {
              console.error('Audio playback failed:', e)
              setError('Playback failed — your mic is working, but the browser could not play the audio back.')
            })
          }
        }, 100)
      } else {
        setError('No audio was captured — check that your microphone is connected.')
        setState('idle')
      }
    } catch (e) {
      console.error('Stop failed:', e)
      setError('Recording failed — ' + String(e))
      setState('idle')
    }
  }

  const reset = () => {
    setAudioUrl(null)
    setError(null)
    setState('idle')
  }

  const warming = engineReady === null || engineReady === false

  return (
    <div style={{ padding: '8px 0' }}>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 6 }}>
        <div>
          <div style={{ fontSize: 12, fontWeight: 500, color: 'var(--text-primary)' }}>Test microphone</div>
          <div style={{ fontSize: 10, color: 'var(--text-faint)', marginTop: 1 }}>
            {warming ? 'Warming up the speech engine...' : 'Record and play back to verify'}
          </div>
        </div>
        {state === 'idle' && (
          <button type="button" onClick={startTest} disabled={warming} style={{
            padding: '4px 14px', fontSize: 11,
            background: warming ? 'var(--bg-tertiary)' : 'var(--accent-green)',
            color: warming ? 'var(--text-faint)' : '#fff',
            border: 'none', borderRadius: 'var(--echo-radius-sm)',
            cursor: warming ? 'default' : 'pointer',
            fontFamily: 'var(--font-sans)', fontWeight: 500,
          }}>{warming ? 'Warming up...' : 'Record'}</button>
        )}
        {state === 'recording' && (
          <button type="button" onClick={stopTest} style={{
            padding: '4px 14px', fontSize: 11, background: 'var(--accent-red)', color: '#fff',
            border: 'none', borderRadius: 'var(--echo-radius-sm)', cursor: 'pointer', fontFamily: 'var(--font-sans)', fontWeight: 500,
            animation: 'echo-pulse 1.8s ease-in-out infinite',
          }}>Stop</button>
        )}
        {state === 'playing' && (
          <button type="button" onClick={reset} style={{
            padding: '4px 14px', fontSize: 11, background: 'var(--bg-card)', color: 'var(--text-secondary)',
            border: '1px solid var(--border-primary)', borderRadius: 'var(--echo-radius-sm)', cursor: 'pointer', fontFamily: 'var(--font-sans)',
          }}>Reset</button>
        )}
      </div>
      {state === 'recording' && (
        <div className="echo-mono" style={{ fontSize: 10, color: 'var(--accent-red)' }}>
          Recording... speak now
        </div>
      )}
      {error && (
        <div className="echo-mono" style={{ fontSize: 10, color: 'var(--accent-red)' }}>{error}</div>
      )}
      {audioUrl && (
        <audio ref={audioRef} src={audioUrl} controls onEnded={reset} style={{
          width: '100%', height: 28, marginTop: 4,
        }} />
      )}
    </div>
  )
}
