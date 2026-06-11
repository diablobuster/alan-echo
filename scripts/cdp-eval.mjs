// Evaluate a JS expression inside the running app's WebView2 via CDP.
// Launch the app with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222
// then: node scripts/cdp-eval.mjs 9222 "window.__TAURI_INTERNALS__.invoke('get_engine_info')"
// Tauri commands return promises — they are awaited and printed as JSON.

const [, , port = '9222', expr] = process.argv
if (!expr) {
  console.error('usage: node cdp-eval.mjs <port> <expression>')
  process.exit(2)
}

const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json()
const page = targets.find(t => t.type === 'page')
if (!page) {
  console.error('no page target found; targets: ' + JSON.stringify(targets.map(t => t.type)))
  process.exit(1)
}

const ws = new WebSocket(page.webSocketDebuggerUrl)
await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej })

const reply = await new Promise((res, rej) => {
  const timer = setTimeout(() => rej(new Error('CDP eval timed out')), 30_000)
  ws.onmessage = e => {
    const m = JSON.parse(e.data)
    if (m.id === 1) { clearTimeout(timer); res(m) }
  }
  ws.send(JSON.stringify({
    id: 1,
    method: 'Runtime.evaluate',
    params: { expression: expr, awaitPromise: true, returnByValue: true },
  }))
})

ws.close()
if (reply.result?.exceptionDetails) {
  console.error(JSON.stringify(reply.result.exceptionDetails, null, 2))
  process.exit(1)
}
console.log(JSON.stringify(reply.result?.result?.value ?? reply.result, null, 2))
