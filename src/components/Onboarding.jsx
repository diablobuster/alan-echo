import { useState, useEffect, useRef } from 'react'
import { invoke } from '@tauri-apps/api/core'
import Icon, { Kbd, Monogram } from './Icons'
import { Btn } from './StatusPanel'
import { MicTest } from './SettingsPanel'

/**
 * First-run wizard: pick a microphone, do a real test dictation, learn the
 * hotkeys. Shown once; completion is persisted via the onboarding_complete
 * setting by the Dashboard.
 */
export default function Onboarding({ hotkeys = {}, onDone }) {
  const [step, setStep] = useState(0)

  const steps = [
    { title: 'Choose your microphone', body: <StepMic /> },
    { title: 'Try a test dictation', body: <StepTest /> },
    { title: 'Your hotkeys', body: <StepHotkeys hotkeys={hotkeys} /> },
  ]
  const last = step === steps.length - 1

  return (
    <div style={{
      position: 'fixed', inset: 0, zIndex: 300,
      background: 'color-mix(in srgb, var(--bg-primary) 80%, transparent)',
      backdropFilter: 'blur(3px)',
      display: 'flex', alignItems: 'center', justifyContent: 'center',
      animation: 'echo-fade-in 0.25s ease-out both',
    }}>
      <div style={{
        width: 440, maxHeight: '85vh', overflow: 'auto',
        background: 'var(--bg-card)', border: '1px solid var(--border-primary)',
        borderRadius: 'var(--echo-radius)', boxShadow: '0 12px 40px rgba(0,0,0,0.18)',
        padding: '22px 24px', animation: 'echo-rise 0.25s ease-out both',
      }}>
        {/* Header */}
        <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 4 }}>
          <Monogram size={22} tone="brass" />
          <span className="echo-serif" style={{ fontSize: 17 }}>Welcome to ALAN Echo</span>
          <div style={{ flex: 1 }} />
          <button onClick={onDone} style={{
            background: 'none', border: 'none', cursor: 'pointer', fontSize: 10,
            color: 'var(--text-faint)', fontFamily: 'var(--font-mono)', letterSpacing: '0.06em',
            textTransform: 'uppercase', padding: 4,
          }}>
            Skip
          </button>
        </div>

        {/* Step indicator */}
        <div style={{ display: 'flex', gap: 5, margin: '12px 0 16px' }}>
          {steps.map((_, i) => (
            <div key={i} style={{
              height: 3, flex: 1, borderRadius: 2,
              background: i <= step ? 'var(--brass)' : 'var(--bg-tertiary)',
              transition: 'background 0.2s',
            }} />
          ))}
        </div>

        <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 10 }}>
          {step + 1}. {steps[step].title}
        </div>

        {steps[step].body}

        {/* Nav */}
        <div style={{ display: 'flex', justifyContent: 'space-between', marginTop: 18 }}>
          {step > 0
            ? <Btn kind="ghost" size="sm" onClick={() => setStep(s => s - 1)}>Back</Btn>
            : <span />}
          {last
            ? <Btn kind="brass" size="sm" onClick={onDone}><Icon name="check" size={12} color="#fff" /> Start dictating</Btn>
            : <Btn kind="primary" size="sm" onClick={() => setStep(s => s + 1)}>Next <Icon name="chevronR" size={11} color="#fff" /></Btn>}
        </div>
      </div>
    </div>
  )
}

function StepMic() {
  const [devices, setDevices] = useState([])
  const [selected, setSelected] = useState('')

  useEffect(() => {
    invoke('list_audio_devices').then(d => setDevices(d || [])).catch(() => {})
    invoke('get_settings').then(s => { if (s?.microphone_device) setSelected(s.microphone_device) }).catch(() => {})
  }, [])

  const choose = async (name) => {
    setSelected(name)
    try { await invoke('set_setting', { key: 'microphone_device', value: name || null }) } catch {}
  }

  return (
    <div>
      <p style={{ fontSize: 12, color: 'var(--text-muted)', margin: '0 0 10px', lineHeight: 1.5 }}>
        ALAN Echo listens through this device. The system default works for most setups.
      </p>
      <select
        value={selected}
        onChange={e => choose(e.target.value)}
        style={{
          width: '100%', background: 'var(--bg-input)', border: '1px solid var(--border-primary)',
          borderRadius: 'var(--echo-radius-sm)', padding: '7px 10px',
          fontSize: 12, fontFamily: 'var(--font-sans)', color: 'var(--text-primary)',
        }}
      >
        <option value="">System default</option>
        {devices.map(d => (
          <option key={d.index} value={d.name}>{d.name} {d.is_default ? '(default)' : ''}</option>
        ))}
      </select>
      <div style={{ marginTop: 6 }}>
        <MicTest />
      </div>
    </div>
  )
}

function StepTest() {
  const [state, setState] = useState('idle') // idle | recording | processing | done | error
  const [result, setResult] = useState('')
  const stateRef = useRef(state)
  stateRef.current = state

  // Discard an in-flight recording if the user clicks Next/Back/Skip mid-test —
  // an abandoned recording would otherwise block dictation until restart.
  useEffect(() => () => {
    if (stateRef.current === 'recording') invoke('cancel_recording').catch(() => {})
  }, [])

  const start = async () => {
    setResult('')
    try {
      if (await invoke('is_recording')) {
        setResult('Another recording is in progress — stop it first.')
        setState('error')
        return
      }
      await invoke('start_recording')
      setState('recording')
    } catch (e) {
      setResult(String(e))
      setState('error')
    }
  }

  const stop = async () => {
    setState('processing')
    try {
      const rec = await invoke('stop_recording')
      if (!rec?.has_speech || !rec?.wav_path) {
        if (rec?.wav_path) invoke('discard_recording', { wavPath: rec.wav_path }).catch(() => {})
        setResult('No speech detected — speak a little louder and try again.')
        setState('error')
        return
      }
      const r = await invoke('transcribe', { wavPath: rec.wav_path })
      if (r?.empty) {
        setResult('Nothing came through — try a full sentence.')
        setState('error')
      } else {
        setResult(r.text)
        setState('done')
      }
    } catch (e) {
      setResult(String(e))
      setState('error')
    }
  }

  return (
    <div>
      <p style={{ fontSize: 12, color: 'var(--text-muted)', margin: '0 0 12px', lineHeight: 1.5 }}>
        Press Record, say a sentence — for example <em>"This is my first ALAN Echo dictation"</em> — then press Stop.
      </p>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        {state === 'recording' ? (
          <Btn size="sm" onClick={stop} style={{ background: 'var(--echo-rec)' }}>
            <Icon name="x" size={12} color="#fff" /> Stop
          </Btn>
        ) : (
          <Btn size="sm" onClick={start} style={{ background: 'var(--accent-green)' }}>
            <Icon name="mic" size={12} color="#fff" /> {state === 'done' ? 'Record again' : 'Record'}
          </Btn>
        )}
        {state === 'recording' && (
          <span className="echo-mono" style={{ fontSize: 11, color: 'var(--echo-rec)' }}>Listening... speak now</span>
        )}
        {state === 'processing' && (
          <span className="echo-mono" style={{ fontSize: 11, color: 'var(--accent-yellow)' }}>Transcribing...</span>
        )}
      </div>
      {state === 'done' && (
        <div style={{
          marginTop: 12, padding: '10px 12px', fontSize: 12, lineHeight: 1.6,
          background: 'color-mix(in srgb, var(--accent-green) 7%, var(--bg-card))',
          border: '1px solid color-mix(in srgb, var(--accent-green) 25%, transparent)',
          borderRadius: 'var(--echo-radius-sm)', color: 'var(--text-primary)',
        }}>
          <Icon name="check" size={12} color="var(--accent-green)" style={{ marginRight: 5, verticalAlign: '-2px' }} />
          {result}
        </div>
      )}
      {state === 'error' && (
        <div style={{ marginTop: 12, fontSize: 11, color: 'var(--accent-red)' }}>{result}</div>
      )}
    </div>
  )
}

function StepHotkeys({ hotkeys }) {
  // null = registration failed (vs undefined = not yet loaded)
  const rows = [
    { keys: hotkeys.toggle === null ? null : (hotkeys.toggle || 'Ctrl + Shift + Space'), what: 'Start / stop dictation from any app' },
    { keys: hotkeys.cancel, what: 'Cancel a recording (discard it)' },
    { keys: hotkeys.show === null ? null : (hotkeys.show || 'Ctrl + Shift + H'), what: 'Bring up this dashboard' },
  ].filter(r => r.keys)

  return (
    <div>
      <p style={{ fontSize: 12, color: 'var(--text-muted)', margin: '0 0 12px', lineHeight: 1.5 }}>
        Echo works everywhere: put your cursor where you want text, hit the hotkey, speak, hit it
        again — your words are typed for you. The window can stay closed; Echo lives in the tray.
      </p>
      {rows.map(r => (
        <div key={r.what} style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '7px 0' }}>
          <span style={{ display: 'flex', gap: 3, minWidth: 170 }}>
            {r.keys.split(' + ').map(k => <Kbd key={k}>{k}</Kbd>)}
          </span>
          <span style={{ fontSize: 12, color: 'var(--text-secondary)' }}>{r.what}</span>
        </div>
      ))}
      {hotkeys.toggle === null && (
        <div style={{ fontSize: 11, color: 'var(--accent-yellow)', marginTop: 8 }}>
          The dictation hotkey could not be registered — another app may own it.
          Use the tray menu's Start Dictation instead.
        </div>
      )}
    </div>
  )
}
