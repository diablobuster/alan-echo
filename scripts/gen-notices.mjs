// Generates src/legal/third-party-notices.txt — MIT requires shipping the
// license text with the software; Apache-2.0 requires NOTICE carriage.
// Run on dependency changes and before each release.
import { execSync } from 'node:child_process'
import { writeFileSync } from 'node:fs'

const header = `THIRD-PARTY NOTICES — ALAN Echo
This product includes open-source software. Full license texts below.

== Bundled components of special note ==
whisper.cpp — Copyright (c) 2023-2026 The ggml authors — MIT License
Whisper speech models — Copyright (c) OpenAI — released under the MIT License
`

let out = header + '\n== Rust crates ==\n'
out += execSync('cargo license --json', { cwd: 'src-tauri' })
  .toString()
  // keep it readable: name, version, license per line
  .split('\n').join('\n')

out += '\n\n== npm packages ==\n'
out += execSync('npx license-checker-rseidelsohn --production --plainVertical').toString()

writeFileSync('src/legal/third-party-notices.txt', out)
console.log('Wrote src/legal/third-party-notices.txt (%d chars)', out.length)
