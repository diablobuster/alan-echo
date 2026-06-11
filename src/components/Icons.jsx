// ALAN Echo — SVG icon set from Claude Design handoff

const paths = {
  search:   'M7.5 13a5.5 5.5 0 1 0 0-11 5.5 5.5 0 0 0 0 11ZM15 15l-3.6-3.6',
  mic:      'M9 1.8a2.2 2.2 0 0 0-2.2 2.2v4a2.2 2.2 0 0 0 4.4 0V4A2.2 2.2 0 0 0 9 1.8ZM3.8 7.6a5.2 5.2 0 0 0 10.4 0M9 13v3.2M6 16.2h6',
  copy:     'M5.5 5.5V3.2A1.2 1.2 0 0 1 6.7 2h7.1A1.2 1.2 0 0 1 15 3.2v7.1a1.2 1.2 0 0 1-1.2 1.2H11.5M3.2 6.5h7.1A1.2 1.2 0 0 1 11.5 7.7v7.1A1.2 1.2 0 0 1 10.3 16H3.2A1.2 1.2 0 0 1 2 14.8V7.7A1.2 1.2 0 0 1 3.2 6.5Z',
  trash:    'M3 4.5h12M7 4.5V3a1 1 0 0 1 1-1h2a1 1 0 0 1 1 1v1.5M4.4 4.5l.7 10a1 1 0 0 0 1 .9h5.8a1 1 0 0 0 1-.9l.7-10',
  gear:     'M9 11.6a2.6 2.6 0 1 0 0-5.2 2.6 2.6 0 0 0 0 5.2Z M14.7 11a1.1 1.1 0 0 0 .22 1.21l.04.04a1.3 1.3 0 1 1-1.84 1.84l-.04-.04a1.1 1.1 0 0 0-1.86.78v.11a1.3 1.3 0 0 1-2.6 0v-.06a1.1 1.1 0 0 0-1.92-.73l-.04.04A1.3 1.3 0 1 1 2.79 12.4l.04-.04a1.1 1.1 0 0 0-.78-1.86h-.11a1.3 1.3 0 0 1 0-2.6h.06A1.1 1.1 0 0 0 2.73 5.99l-.04-.04A1.3 1.3 0 1 1 4.53 4.1l.04.04a1.1 1.1 0 0 0 1.86-.78V3.3a1.3 1.3 0 0 1 2.6 0v.06a1.1 1.1 0 0 0 1.92.73l.04-.04a1.3 1.3 0 1 1 1.84 1.84l-.04.04a1.1 1.1 0 0 0 .78 1.86h.11a1.3 1.3 0 0 1 0 2.6h-.06a1.1 1.1 0 0 0-1.01.66Z',
  download: 'M9 2.5v8.4M5.6 7.6 9 11l3.4-3.4M3 13.2v1.3a1 1 0 0 0 1 1h10a1 1 0 0 0 1-1v-1.3',
  chevron:  'M5.5 7 9 10.5 12.5 7',
  chevronR: 'M7 4.5 11 9l-4 4.5',
  clock:    'M9 16a7 7 0 1 0 0-14 7 7 0 0 0 0 14ZM9 5v4l2.6 1.6',
  check:    'M3.5 9.5 7 13l7.5-8',
  x:        'M4 4l10 10M14 4 4 14',
  plus:     'M9 3.5v11M3.5 9h11',
  sound:    'M3 7v4h2.5L9 14V4L5.5 7H3ZM11.5 6.6a3.4 3.4 0 0 1 0 4.8M13.4 4.7a6 6 0 0 1 0 8.6',
  waveform: 'M2 9h1.6M5 5.5v7M7.6 3v12M10.2 6v6M12.8 4.5v9M15.4 7v4M18 9h-.6',
  filter:   'M2.5 4h13M5 9h8M7.5 14h3',
  sparkle:  'M9 2.5l1.4 4.1L14.5 8l-4.1 1.4L9 13.5l-1.4-4.1L3.5 8l4.1-1.4L9 2.5Z',
  lock:     'M4.5 8.2V6a4.5 4.5 0 0 1 9 0v2.2M3.7 8.2h10.6a.8.8 0 0 1 .8.8v5.4a.8.8 0 0 1-.8.8H3.7a.8.8 0 0 1-.8-.8V9a.8.8 0 0 1 .8-.8Z',
  cpu:      'M5.5 5.5h7v7h-7zM7.5 2v2M10.5 2v2M7.5 14v2M10.5 14v2M14 7.5h2M14 10.5h2M2 7.5h2M2 10.5h2',
  minimize: 'M4 9h10',
  maximize: 'M4 4h10v10H4z',
  close:    'M5 5l8 8M13 5l-8 8',
}

export default function Icon({ name, size = 16, stroke = 1.6, color = 'currentColor', style = {} }) {
  return (
    <svg width={size} height={size} viewBox="0 0 18 18" fill="none"
      stroke={color} strokeWidth={stroke} strokeLinecap="round" strokeLinejoin="round" style={style}>
      <path d={paths[name] || ''} />
    </svg>
  )
}

import { BRASS, INK, NAVY } from './logoData.js'

const LOGO_MAP = { brass: BRASS, ink: INK, navy: NAVY }

export function Monogram({ size = 26, tone = 'auto' }) {
  const imgStyle = { height: size, width: size * 1.2, objectFit: 'contain', flexShrink: 0 }
  if (tone !== 'auto') {
    const src = LOGO_MAP[tone] || LOGO_MAP.ink
    return <img src={src} alt="ALAN" draggable={false} style={imgStyle} />
  }
  // Theme-aware: ink mark on light surfaces, brass on dark. Both render and
  // tokens.css toggles visibility off :root[data-theme] — theme changes flip
  // the attribute without re-rendering React, so a JS-picked src would go stale.
  return (
    <span className="alan-mono" style={{ display: 'inline-flex', flexShrink: 0 }}>
      <img className="mono-ink" src={INK} alt="ALAN" draggable={false} style={imgStyle} />
      <img className="mono-brass" src={BRASS} alt="" aria-hidden draggable={false} style={imgStyle} />
    </span>
  )
}

export function Kbd({ children }) {
  return (
    <kbd style={{
      fontFamily: 'var(--font-mono)', fontSize: 10, lineHeight: 1,
      padding: '2px 5px', borderRadius: 3,
      background: 'var(--bg-secondary)', border: '1px solid var(--border-primary)',
      color: 'var(--text-muted)',
    }}>{children}</kbd>
  )
}
