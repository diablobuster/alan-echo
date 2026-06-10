import './tokens.css'
import { useState, useEffect } from 'react'
import Splash from './components/Splash'
import Dashboard from './components/Dashboard'

const { invoke } = window.__TAURI__ || { invoke: async () => ({}) }

export default function App() {
  const [loading, setLoading] = useState(true)
  const [licensed, setLicensed] = useState(null) // null = checking, true/false = known
  const [progress, setProgress] = useState(0)

  useEffect(() => {
    // Simulate model loading progress
    const interval = setInterval(() => {
      setProgress(p => {
        if (p >= 95) { clearInterval(interval); return 95 }
        return p + Math.random() * 8
      })
    }, 200)

    // Check license and initialize
    async function init() {
      try {
        const isLicensed = await invoke('check_license')
        setLicensed(isLicensed)
      } catch {
        setLicensed(true) // Dev mode — skip license check
      }
      setProgress(100)
      setTimeout(() => setLoading(false), 600)
    }
    init()

    return () => clearInterval(interval)
  }, [])

  if (loading) {
    return <Splash progress={Math.min(progress, 100)} />
  }

  return <Dashboard />
}

// Mount
import { createRoot } from 'react-dom/client'
createRoot(document.getElementById('root')).render(<App />)
