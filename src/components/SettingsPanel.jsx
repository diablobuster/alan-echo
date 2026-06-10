import { useState, useEffect } from 'react'
import Icon from './Icons'
import { invoke } from '@tauri-apps/api/core'

export default function SettingsPanel({ open, onClose }) {
  const [settings, setSettings] = useState({
    auto_paste: true,
    sound_enabled: true,
    text_cleanup_level: 'standard',
  })
  const [devices, setDevices] = useState([])
  const [selectedDevice, setSelectedDevice] = useState(null)
  const [whisperReady, setWhisperReady] = useState(false)

  useEffect(() => {
    if (!open) return
    async function load() {
      try {
        const s = await invoke('get_settings')
        setSettings(prev => ({ ...prev, ...s }))
        if (s.microphone_device) setSelectedDevice(s.microphone_device)
        const devs = await invoke('list_audio_devices')
        setDevices(devs || [])
        const ready = await invoke('check_whisper_ready')
        setWhisperReady(ready)
      } catch (e) { console.error('Settings error:', e) }
    }
    load()
  }, [open])

  const updateSetting = async (key, value) => {
    setSettings(prev => ({ ...prev, [key]: value }))
    try { await invoke('set_setting', { key, value }) } catch {}
  }

  if (!open) return null

  return (
    <>
      {/* Backdrop */}
      <div onClick={onClose} style={{
        position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.2)', zIndex: 100,
      }} />

      {/* Panel */}
      <div style={{
        position: 'fixed', top: 0, right: 0, bottom: 0, width: 380,
        background: 'var(--bg-primary)', borderLeft: '1px solid var(--border-primary)',
        zIndex: 101, display: 'flex', flexDirection: 'column',
        animation: 'echo-rise 0.2s ease-out both',
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

          <SettingsRow label="Speech model" hint="Accuracy vs speed tradeoff">
            <Seg
              options={['small', 'medium', 'large-v3']}
              value={settings.whisper_model || 'large-v3'}
              onChange={v => updateSetting('whisper_model', v)}
            />
          </SettingsRow>

          <SettingsRow label="Status" hint="">
            <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
              <span style={{
                width: 6, height: 6, borderRadius: '50%',
                background: whisperReady ? 'var(--accent-green)' : 'var(--accent-red)',
              }} />
              <span className="echo-mono" style={{ fontSize: 11, color: 'var(--text-muted)' }}>
                {whisperReady ? 'Engine ready' : 'Model not found'}
              </span>
            </div>
          </SettingsRow>

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
              onChange={v => updateSetting('text_cleanup_level', v)}
            />
          </SettingsRow>

          {/* Hotkeys section */}
          <div className="echo-eyebrow" style={{ marginTop: 20, marginBottom: 12 }}>Hotkeys</div>

          <HotkeyRow label="Toggle dictation" keys="Ctrl + Shift + Space" />
          <HotkeyRow label="Cancel recording" keys="Ctrl + Shift + Esc" />
          <HotkeyRow label="Show dashboard" keys="Ctrl + Shift + H" />
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

function Seg({ options, value, onChange }) {
  return (
    <div style={{
      display: 'flex', background: 'var(--bg-secondary)',
      borderRadius: 'var(--echo-radius-sm)', padding: 2, gap: 1,
    }}>
      {options.map(opt => (
        <button key={opt} onClick={() => onChange(opt)} style={{
          padding: '3px 10px', fontSize: 10, fontFamily: 'var(--font-mono)',
          border: 'none', borderRadius: 'var(--echo-radius-sm)',
          cursor: 'pointer', fontWeight: opt === value ? 600 : 400,
          background: opt === value ? 'var(--bg-card)' : 'transparent',
          color: opt === value ? 'var(--text-primary)' : 'var(--text-muted)',
          boxShadow: opt === value ? '0 1px 2px rgba(0,0,0,0.06)' : 'none',
        }}>
          {opt}
        </button>
      ))}
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

function HotkeyRow({ label, keys }) {
  return (
    <div style={{
      display: 'flex', alignItems: 'center', justifyContent: 'space-between',
      padding: '6px 0',
    }}>
      <span style={{ fontSize: 12, color: 'var(--text-secondary)' }}>{label}</span>
      <span className="echo-mono" style={{ fontSize: 10, color: 'var(--text-faint)' }}>{keys}</span>
    </div>
  )
}
