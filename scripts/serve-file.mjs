// Serve one file on a local port for pack-install testing:
//   node scripts/serve-file.mjs <path> [port]
// Pairs with the debug-only ECHO_*_PACK_URL override in packs.rs.

import { createServer } from 'node:http'
import { readFileSync } from 'node:fs'

const [, , path, port = '8990'] = process.argv
if (!path) {
  console.error('usage: node serve-file.mjs <path> [port]')
  process.exit(2)
}
const file = readFileSync(path)
createServer((req, res) => {
  res.writeHead(200, { 'Content-Type': 'application/zip', 'Content-Length': file.length })
  res.end(file)
}).listen(Number(port), '127.0.0.1', () => console.log(`serving ${path} on ${port}`))
