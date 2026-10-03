import { useEffect, useRef, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import '../readAloud.css'

export default function ReadAloudOverlay() {
  const [mode, setMode] = useState('pick')
  const [hover, setHover] = useState([])
  const [word, setWord] = useState({ rects: [], style: 'brass' })
  const [shake, setShake] = useState(false)
  const [note, setNote] = useState(null)
  const shakeTimer = useRef(null)
  const noteTimer = useRef(null)

  useEffect(() => {
    document.documentElement.classList.add('ra-window')
    const offs = [
      listen('ra-overlay-mode', e => { setMode(e.payload.mode); setHover([]); setNote(null) }),
      listen('ra-rects', e => {
        if (e.payload.kind === 'hover') setHover(e.payload.rects)
        else setWord({ rects: e.payload.rects, style: e.payload.style })
      }),
      listen('ra-pick-rejected', e => {
        const msg = e.payload?.message
        clearTimeout(noteTimer.current)
        setNote(msg || null)
        if (msg) noteTimer.current = setTimeout(() => setNote(null), 2000)
        clearTimeout(shakeTimer.current)
        setShake(false)
        requestAnimationFrame(() => setShake(true))
        shakeTimer.current = setTimeout(() => setShake(false), 320)
      }),
    ]
    return () => { clearTimeout(shakeTimer.current); clearTimeout(noteTimer.current); offs.forEach(p => p.then(f => f())) }
  }, [])

  const box = (r, cls, i) => (
    <div key={i} className={`ra-box ${cls}`} style={{ left: r.x - 2, top: r.y - 1, width: r.w + 4, height: r.h + 2 }} />
  )

  return (
    <>
      {mode === 'pick' && <div className="ra-dim" />}
      {mode === 'pick' && (
        <div className={`ra-hint${shake ? ' shake' : ''}`} role="status">
          <span>{note || 'Click any word to start reading'}</span>
          <span><kbd>Esc</kbd> cancel</span>
        </div>
      )}
      {mode === 'pick' && hover.map((r, i) => box(r, 'hover', i))}
      {mode === 'read' && word.style !== 'off' && word.rects.map((r, i) => box(r, word.style, i))}
    </>
  )
}
