import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

export default function UpdateBanner() {
  const [update, setUpdate] = useState(null)
  const [stage, setStage] = useState(null) // null | 'downloading' | 'launching' | 'error'
  const [percent, setPercent] = useState(0)
  const [dismissed, setDismissed] = useState(false)

  useEffect(() => {
    invoke('check_for_update').then(info => {
      if (info?.available) setUpdate(info)
    }).catch(() => {})

    const unlisten = listen('update_progress', (event) => {
      const { stage: s, percent: p, error } = event.payload
      setStage(s)
      if (p != null) setPercent(p)
      if (error) console.error('[update]', error)
    })
    return () => { unlisten.then(fn => fn()) }
  }, [])

  if (!update || dismissed) return null

  return (
    <div style={{
      padding: '8px 14px',
      background: 'color-mix(in srgb, var(--echo-accent) 10%, transparent)',
      borderBottom: '1px solid color-mix(in srgb, var(--echo-accent) 30%, transparent)',
      display: 'flex',
      alignItems: 'center',
      gap: 10,
      fontSize: 12,
    }}>
      <span style={{ color: 'var(--echo-accent)', fontWeight: 600 }}>
        Update available
      </span>
      <span style={{ color: 'var(--text-secondary)' }}>
        v{update.latest_version}
        {update.release_date ? ` · ${update.release_date}` : ''}
        {update.size_mb ? ` · ${update.size_mb} MB` : ''}
      </span>
      <span style={{ flex: 1 }} />

      {!stage && (
        <>
          <button
            onClick={() => {
              if (update.download_url) {
                setStage('downloading')
                invoke('download_update', { downloadUrl: update.download_url }).catch(e => {
                  setStage('error')
                  console.error('[update]', e)
                })
              }
            }}
            style={{
              background: 'var(--echo-accent)',
              color: '#fff',
              border: 'none',
              borderRadius: 3,
              padding: '4px 12px',
              fontSize: 11,
              fontWeight: 600,
              cursor: 'pointer',
            }}
          >
            Download &amp; install
          </button>
          <button
            onClick={() => setDismissed(true)}
            style={{
              background: 'none',
              border: 'none',
              color: 'var(--text-faint)',
              cursor: 'pointer',
              fontSize: 14,
              padding: '2px 4px',
            }}
            aria-label="Dismiss"
          >
            &times;
          </button>
        </>
      )}

      {stage === 'downloading' && (
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <div style={{
            width: 80, height: 4, background: 'var(--border-primary)', borderRadius: 2, overflow: 'hidden',
          }}>
            <div style={{
              width: `${percent}%`, height: '100%', background: 'var(--echo-accent)',
              borderRadius: 2, transition: 'width 0.3s',
            }} />
          </div>
          <span style={{ fontSize: 10, color: 'var(--text-muted)', fontVariantNumeric: 'tabular-nums' }}>
            {percent}%
          </span>
        </div>
      )}

      {stage === 'launching' && (
        <span style={{ fontSize: 11, color: 'var(--echo-accent)' }}>
          Launching installer...
        </span>
      )}

      {stage === 'error' && (
        <span style={{ fontSize: 11, color: 'var(--accent-red, #e53e3e)' }}>
          Update failed — try downloading from the website
        </span>
      )}
    </div>
  )
}
