import './tokens.css'
import { useState, useEffect, useRef } from 'react'
import { createRoot } from 'react-dom/client'
import Splash from './components/Splash'
import Dashboard from './components/Dashboard'
import LicenseGate from './components/LicenseGate'
import { invoke } from '@tauri-apps/api/core'

function App() {
  const [phase, setPhase] = useState('checking')
  const [progress, setProgress] = useState(0)
  const mounted = useRef(true)

  useEffect(() => {
    mounted.current = true
    async function init() {
      let licensed = false
      try {
        licensed = await invoke('check_license')
      } catch (e) {
        console.warn('License check failed, assuming dev mode:', e)
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
    init()
    return () => { mounted.current = false }
  }, [])

  async function loadEngine() {
    const interval = setInterval(() => {
      if (!mounted.current) { clearInterval(interval); return }
      setProgress(p => p >= 92 ? 92 : p + Math.random() * 6)
    }, 250)

    try {
      const ready = await invoke('check_whisper_ready')
      if (!ready) console.warn('Whisper engine not ready — transcription may fail')
    } catch (e) {
      console.warn('Whisper check failed:', e)
    }

    clearInterval(interval)
    if (!mounted.current) return
    setProgress(100)
    setTimeout(() => { if (mounted.current) setPhase('ready') }, 500)
  }

  function handleLicenseActivated() {
    setPhase('splash')
    loadEngine()
  }

  if (phase === 'checking') return <Splash progress={0} />
  if (phase === 'license') return <LicenseGate onActivated={handleLicenseActivated} />
  if (phase === 'splash') return <Splash progress={Math.min(progress, 100)} />
  return <Dashboard />
}

createRoot(document.getElementById('root')).render(<App />)
