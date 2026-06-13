import './tokens.css'
import { useState, useEffect, useRef } from 'react'
import { createRoot } from 'react-dom/client'
import Splash from './components/Splash'
import Dashboard from './components/Dashboard'
import LicenseGate from './components/LicenseGate'
import EulaGate from './components/EulaGate'
import UpdateBanner from './components/UpdateBanner'
import { invoke } from '@tauri-apps/api/core'
import { applyTheme } from './theme'
import { EULA_VERSION } from './legal/eulaVersion'

const ENGINE_WAIT_MS = 90000

function App() {
  const [phase, setPhase] = useState('checking')
  const [progress, setProgress] = useState(0)
  const [engineLabel, setEngineLabel] = useState('')
  const mounted = useRef(true)

  useEffect(() => {
    mounted.current = true
    async function init() {
      let settings = null
      try {
        settings = await invoke('get_settings')
        applyTheme(settings)
      } catch {}

      // EULA gate comes before everything — including the trial. If settings
      // are unreadable we show the gate (one harmless click); if acceptance
      // can't be saved we still proceed (never brick), and re-prompt next run.
      const accepted = settings?.eula_accepted_version === EULA_VERSION
      if (!mounted.current) return
      if (!accepted) {
        setPhase('eula')
        return
      }
      continueAfterEula()
    }
    init()
    return () => { mounted.current = false }
  }, [])

  async function continueAfterEula() {
    let licensed = false
    try {
      licensed = await invoke('check_license')
    } catch (e) {
      // Fail open: an internal error must never brick a paying user.
      console.warn('License check failed, allowing through:', e)
      licensed = true
    }
    if (!mounted.current) return
    if (!licensed) {
      setPhase('license')
      return
    }
    setPhase('splash')
    loadEngine()
  }

  // Wait (bounded) for whisper-server to finish loading the model so the
  // first dictation is instant. On failure/timeout we proceed anyway — the
  // dashboard surfaces engine errors when the user actually dictates.
  async function loadEngine() {
    const started = Date.now()
    while (mounted.current && Date.now() - started < ENGINE_WAIT_MS) {
      try {
        const info = await invoke('get_engine_info')
        if (info?.model_label) setEngineLabel(`${info.model_label} model`)
        if (info?.ready) break
        if (info?.status?.startsWith('failed')) {
          console.warn('Speech engine failed to start:', info.status)
          break
        }
      } catch (e) {
        console.warn('Engine check failed:', e)
        break
      }
      setProgress(p => Math.min(92, p + 3))
      await new Promise(r => setTimeout(r, 500))
    }
    if (!mounted.current) return
    setProgress(100)
    setTimeout(() => { if (mounted.current) setPhase('ready') }, 400)
  }

  function handleLicenseActivated() {
    setPhase('splash')
    loadEngine()
  }

  if (phase === 'checking') return <Splash progress={0} modelLabel={engineLabel} />
  if (phase === 'eula') return <EulaGate onAccepted={continueAfterEula} />
  if (phase === 'license') return <LicenseGate onActivated={handleLicenseActivated} />
  if (phase === 'splash') return <Splash progress={Math.min(progress, 100)} modelLabel={engineLabel} />
  return <><UpdateBanner /><Dashboard /></>
}

createRoot(document.getElementById('root')).render(<App />)
