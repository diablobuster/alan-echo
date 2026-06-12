// Generates legal/EULA.txt (plain text for the NSIS license page) from
// src/legal/eula.md. Run whenever the EULA changes: node scripts/gen-eula-txt.mjs
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs'

const md = readFileSync('src/legal/eula.md', 'utf8')
const txt = md
  .replace(/^#{1,3} /gm, '')   // strip heading markers
  .replace(/\*\*([^*]+)\*\*/g, '$1')
  .replace(/\r?\n/g, '\r\n')   // NSIS prefers CRLF
mkdirSync('legal', { recursive: true })
writeFileSync('legal/EULA.txt', txt)
console.log('Wrote legal/EULA.txt (%d chars)', txt.length)
