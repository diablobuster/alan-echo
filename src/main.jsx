import './tokens.css'
import { useState, useEffect } from 'react'
import { createRoot } from 'react-dom/client'
import Splash from './components/Splash'
import Dashboard from './components/Dashboard'
import LicenseGate from './components/LicenseGate'

const invoke = window.__TAURI__?.invoke || (async () => null)

function App() {
  const [phase, setPhase] = useState('checking') // checking → license | splash → ready
  const [progress, setProgress] = useState(0)

  useEffect(() => {
    async function init() {
      // Check license
      let licensed = false
      try {
        licensed = await invoke('check_license')
      } catch {
        licensed = true // Dev mode
      }

      if (!licensed) {
        setPhase('license')
        return
      }

      // Licensed — show splash and load engine
      setPhase('splash')
      loadEngine()
    }
    init()
  }, [])

  async function loadEngine() {
    // Animate progress bar
    const interval = setInterval(() => {
      setProgress(p => {
        if (p >= 92) return 92
        return p + Math.random() * 6
      })
    }, 250)

    // Check if whisper is ready (model might need loading)
    try {
      await invoke('check_whisper_ready')
    } catch {}

    clearInterval(interval)
    setProgress(100)
    setTimeout(() => setPhase('ready'), 500)
  }

  function handleLicenseActivated() {
    setPhase('splash')
    loadEngine()
  }

  if (phase === 'checking') {
    return <Splash progress={0} />
  }

  if (phase === 'license') {
    return <LicenseGate onActivated={handleLicenseActivated} />
  }

  if (phase === 'splash') {
    return <Splash progress={Math.min(progress, 100)} />
  }

  return <Dashboard />
}

createRoot(document.getElementById('root')).render(<App />)
