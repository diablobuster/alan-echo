import { useEffect, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import { invoke } from '@tauri-apps/api/core'
import '../readAloud.css'

const Icon = {
  back: <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M19 20 9 12l10-8v16z"/><path d="M5 19V5"/></svg>,
  fwd: <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="m5 4 10 8-10 8V4z"/><path d="M19 5v14"/></svg>,
  pause: <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="5" width="4" height="14" rx="1"/><rect x="14" y="5" width="4" height="14" rx="1"/></svg>,
  play: <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor"><path d="M7 4.5v15l13-7.5z"/></svg>,
  x: <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>,
}

export default function NowReadingPill() {
  const [st, setSt] = useState({ state: 'warming', speed: 2.2, device: null, message: null })
  const [sent, setSent] = useState({ words: [], current: 0, progress: 0 })
  const [mono, setMono] = useState(false)

  useEffect(() => {
    document.documentElement.classList.add('ra-window')
    invoke('ra_get_state').then(s => setMono(s?.highlight === 'mono')).catch(() => {})
    const offs = [
      listen('ra-state', e => setSt(e.payload)),
      listen('ra-sentence', e => setSent(e.payload)),
    ]
    return () => offs.forEach(p => p.then(f => f()))
  }, [])

  const drag = async () => {
    const { getCurrentWindow } = await import('@tauri-apps/api/window')
    const w = getCurrentWindow()
    await w.startDragging()
    const p = await w.outerPosition()
    invoke('ra_save_pill_pos', { x: p.x, y: p.y }).catch(() => {})
  }
  const speed = d => invoke('ra_set_speed', { speed: Math.round((st.speed + d) * 10) / 10 })

  if (st.state === 'warming') {
    return (
      <div className="ra-pill"><div className="ra-row">
        <div className="ra-grip" onMouseDown={drag} aria-hidden="true">⋮⋮</div>
        <div className="ra-spin" role="status" aria-label="Starting voice" />
        <div style={{ flex: 1, marginLeft: 8 }}>Starting voice…</div>
        <button className="ra-btn stop" aria-label="Stop reading" onClick={() => invoke('ra_stop')}>{Icon.x}</button>
      </div></div>
    )
  }

  if (st.state === 'error') {
    return (
      <div className="ra-pill"><div className="ra-row" role="alert">
        <div className="ra-grip" onMouseDown={drag} aria-hidden="true">⋮⋮</div>
        <div className="ra-err">{st.message || 'Something went wrong'}</div>
        <button className="ra-btn stop" aria-label="Dismiss" onClick={() => invoke('ra_stop')}>{Icon.x}</button>
      </div></div>
    )
  }

  const paused = st.state === 'paused'
  return (
    <div className="ra-pill">
      <div className="ra-row">
        <div className="ra-grip" onMouseDown={drag} aria-hidden="true">⋮⋮</div>
        <button className="ra-btn" aria-label="Back one sentence" onClick={() => invoke('ra_skip', { dir: -1 })}>{Icon.back}</button>
        <button className={`ra-btn primary${paused ? ' paused' : ''}`} aria-label={paused ? 'Play' : 'Pause'} onClick={() => invoke('ra_toggle')}>{paused ? Icon.play : Icon.pause}</button>
        <button className="ra-btn" aria-label="Forward one sentence" onClick={() => invoke('ra_skip', { dir: 1 })}>{Icon.fwd}</button>
        {paused && <span className="ra-status">Paused</span>}
        {st.message && <span className="ra-status">{st.message}</span>}
        <div style={{ flex: 1 }} />
        <div className="ra-speed">
          <button aria-label="Slower" onClick={() => speed(-0.1)}>−</button>
          <span>{st.speed.toFixed(1)}×</span>
          <button aria-label="Faster" onClick={() => speed(0.1)}>+</button>
        </div>
        <button className="ra-btn stop" aria-label="Stop reading" onClick={() => invoke('ra_stop')}>{Icon.x}</button>
      </div>
      <div className="ra-sentence">
        {sent.words.map((w, i) => (
          <span key={i} className={i === sent.current ? `cur${mono ? ' mono' : ''}` : i < sent.current ? 'done' : ''}>{w}{' '}</span>
        ))}
      </div>
      <div className="ra-progress"><div style={{ width: `${Math.round(sent.progress * 100)}%` }} /></div>
    </div>
  )
}
