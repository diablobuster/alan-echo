import { useState, useEffect, useRef, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

export default function UpdateBanner() {
  const [update, setUpdate] = useState(null)
  const [stage, setStage] = useState(null) // null | 'downloading' | 'launching' | 'error'
  const [percent, setPercent] = useState(0)
  const [dismissed, setDismissed] = useState(false)
  // Mirror of `stage` readable from the long-lived recheck closures.
  const stageRef = useRef(null)
  const applyStage = useCallback((s) => { stageRef.current = s; setStage(s) }, [])

  useEffect(() => {
    // Echo lives in the tray for weeks — a single mount-time check meant the
    // banner never appeared until a manual restart. Re-check periodically and
    // whenever the window becomes visible again (the moment a user could
    // actually see the banner). An in-flight or errored update stage stops
    // rechecks so we never clobber active download state.
    const check = () => {
      if (stageRef.current !== null) return
      invoke('check_for_update').then(info => {
        if (info?.available) setUpdate(info)
      }).catch(() => {})
    }
    check()
    const SIX_HOURS = 6 * 60 * 60 * 1000
    const timer = setInterval(check, SIX_HOURS)
    const onVisible = () => { if (document.visibilityState === 'visible') check() }
    document.addEventListener('visibilitychange', onVisible)

    const unlisten = listen('update_progress', (event) => {
      const { stage: s, percent: p, error } = event.payload
      applyStage(s)
      if (p != null) setPercent(p)
      if (error) console.error('[update]', error)
    })
    return () => {
      clearInterval(timer)
      document.removeEventListener('visibilitychange', onVisible)
      unlisten.then(fn => fn())
    }
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
                applyStage('downloading')
                invoke('download_update', { downloadUrl: update.download_url, expectedSha256: update.sha256 || null }).catch(e => {
                  applyStage('error')
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
          <div role="progressbar" aria-valuenow={percent} aria-valuemin={0} aria-valuemax={100} aria-label="Update download progress" style={{
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

      {stage === 'verifying' && (
        <span style={{ fontSize: 11, color: 'var(--echo-accent)' }}>
          Verifying download...
        </span>
      )}

      {stage === 'launching' && (
        <span style={{ fontSize: 11, color: 'var(--echo-accent)' }}>
          Launching installer...
        </span>
      )}

      {stage === 'mac_drag_install' && (
        <span style={{ fontSize: 11, color: 'var(--echo-accent)' }}>
          Drag ALAN Echo to your Applications folder, then relaunch.
        </span>
      )}

      {stage === 'error' && (
        <>
          <span style={{ fontSize: 11, color: 'var(--accent-red, #e53e3e)' }}>
            Update failed — you can retry, or download from the website
          </span>
          <button
            onClick={() => { applyStage(null); setPercent(0) }}
            style={{
              background: 'none', border: '1px solid var(--border-primary)',
              borderRadius: 3, color: 'var(--text-secondary)', cursor: 'pointer',
              fontSize: 11, padding: '3px 10px',
            }}
          >
            Retry
          </button>
          <button
            onClick={() => setDismissed(true)}
            style={{
              background: 'none', border: 'none', color: 'var(--text-faint)',
              cursor: 'pointer', fontSize: 14, padding: '2px 4px',
            }}
            aria-label="Dismiss"
          >
            &times;
          </button>
        </>
      )}
    </div>
  )
}
