import { useState, useEffect, useRef } from 'react'
import Icon from './Icons'
import { invoke } from '@tauri-apps/api/core'
import { applyTheme } from '../theme'

const CLEANUP_SAMPLE = "um so basically i think we should you know move the the meeting to friday"
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
  const pollRef = useRef(null)

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
        const info = await loadEngineInfo()
        // Resume polling if the panel reopens while a model is still loading.
        if (info && !info.ready && !info.status?.startsWith('failed')) startEnginePoll()
        const cleaned = await invoke('clean_text', { text: CLEANUP_SAMPLE })
        setPreview(cleaned)
      } catch (e) { console.error('Settings error:', e) }
    }
    load()
    return () => clearInterval(pollRef.current)
  }, [open])

  const updateSetting = async (key, value) => {
    setSettings(prev => ({ ...prev, [key]: value }))
    try { await invoke('set_setting', { key, value }) } catch (e) { console.error('Save failed:', e) }
  }

  const changeModel = async (label) => {
    const name = LABEL_TO_MODEL[label] || 'base'
    setModelError(null)
    try {
      // Direct invoke (not updateSetting) — the backend rejects models that
      // aren't installed, and that error belongs in the UI.
      await invoke('set_setting', { key: 'whisper_model', value: name })
      setSettings(prev => ({ ...prev, whisper_model: name }))
    } catch (e) {
      setModelError(String(e))
      return
    }
    // The backend restarts whisper-server with the new model — poll until done.
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
        position: 'fixed', top: 0, right: 0, bottom: 0, width: 380,
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
          <button onClick={onClose} style={{
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

          {/* Microphone */}
          <div className="echo-eyebrow" style={{ marginTop: 20, marginBottom: 12 }}>Microphone</div>
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
            <Toggle checked={settings.auto_paste !== false} onChange={v => updateSetting('auto_paste', v)} />
          </SettingsRow>

          <SettingsRow label="Sound feedback" hint="Play beeps when recording starts and stops">
            <Toggle checked={settings.sound_enabled !== false} onChange={v => updateSetting('sound_enabled', v)} />
          </SettingsRow>

          <SettingsRow label="Text cleanup" hint="How aggressively to clean up transcriptions">
            <Seg
              options={['light', 'standard', 'aggressive']}
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
          <HotkeyRow label="Show dashboard" keys={hotkeys.show || 'Ctrl + Shift + H'} />
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
        </div>

        {/* Footer */}
        <div style={{
          padding: '12px 16px', borderTop: '1px solid var(--border-primary)',
          textAlign: 'center',
        }}>
          <span className="echo-mono" style={{ fontSize: 9, letterSpacing: '0.1em', textTransform: 'uppercase', color: 'var(--text-faint)' }}>
            Part of ALAN Global Intelligence &middot; v1.0
          </span>
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
  const computeText = engine
    ? (engine.cuda
        ? `GPU: ${engine.gpu_name}${engine.vram_mb ? ` (${Math.round(engine.vram_mb / 1024)} GB)` : ''} · CUDA`
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
          <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
            <Icon name="cpu" size={12} color="var(--text-muted)" />
            <span className="echo-mono" style={{ fontSize: 11, color: 'var(--text-muted)' }}>
              {computeText}
            </span>
          </div>
        </SettingsRow>
      )}
    </>
  )
}

function SettingsRow({ label, hint, children }) {
  return (
    <div style={{
      display: 'flex', alignItems: 'center', justifyContent: 'space-between',
      padding: '8px 0', gap: 12,
    }}>
      <div>
        <div style={{ fontSize: 12, fontWeight: 500, color: 'var(--text-primary)' }}>{label}</div>
        {hint && <div style={{ fontSize: 10, color: 'var(--text-faint)', marginTop: 1 }}>{hint}</div>}
      </div>
      <div style={{ flexShrink: 0 }}>{children}</div>
    </div>
  )
}

function Seg({ options, value, onChange, disabled = [] }) {
  return (
    <div style={{
      display: 'flex', background: 'var(--bg-secondary)',
      borderRadius: 'var(--echo-radius-sm)', padding: 2, gap: 1,
    }}>
      {options.map(opt => {
        const isDisabled = disabled.includes(opt) && opt !== value
        return (
          <button
            key={opt}
            onClick={() => { if (!isDisabled) onChange(opt) }}
            title={isDisabled ? 'Not installed' : undefined}
            style={{
              padding: '3px 10px', fontSize: 10, fontFamily: 'var(--font-mono)',
              border: 'none', borderRadius: 'var(--echo-radius-sm)',
              cursor: isDisabled ? 'not-allowed' : 'pointer',
              fontWeight: opt === value ? 600 : 400,
              background: opt === value ? 'var(--bg-card)' : 'transparent',
              color: opt === value ? 'var(--text-primary)' : 'var(--text-muted)',
              opacity: isDisabled ? 0.35 : 1,
              boxShadow: opt === value ? '0 1px 2px rgba(0,0,0,0.06)' : 'none',
            }}
          >
            {opt}
          </button>
        )
      })}
    </div>
  )
}

function Toggle({ checked, onChange }) {
  return (
    <button onClick={() => onChange(!checked)} style={{
      width: 36, height: 20, borderRadius: 10, padding: 2, border: 'none', cursor: 'pointer',
      background: checked ? 'var(--accent-green)' : 'var(--bg-tertiary)',
      transition: 'background 0.15s',
      display: 'flex', alignItems: 'center',
    }}>
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
  const audioRef = useRef(null)
  const stateRef = useRef(state)
  stateRef.current = state

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
        setTimeout(() => { if (audioRef.current) audioRef.current.play().catch(() => {}) }, 100)
      } else {
        setState('idle')
      }
    } catch (e) {
      console.error('Stop failed:', e)
      setState('idle')
    }
  }

  const reset = () => {
    setAudioUrl(null)
    setState('idle')
  }

  return (
    <div style={{ padding: '8px 0' }}>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 6 }}>
        <div>
          <div style={{ fontSize: 12, fontWeight: 500, color: 'var(--text-primary)' }}>Test microphone</div>
          <div style={{ fontSize: 10, color: 'var(--text-faint)', marginTop: 1 }}>Record and play back to verify</div>
        </div>
        {state === 'idle' && (
          <button type="button" onClick={startTest} style={{
            padding: '4px 14px', fontSize: 11, background: 'var(--accent-green)', color: '#fff',
            border: 'none', borderRadius: 'var(--echo-radius-sm)', cursor: 'pointer', fontFamily: 'var(--font-sans)', fontWeight: 500,
          }}>Record</button>
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
