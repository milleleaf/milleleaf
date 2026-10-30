import { createServer } from 'node:http'
import { readFile, stat } from 'node:fs/promises'
import { resolve, extname, sep } from 'node:path'
import { pathToFileURL } from 'node:url'

// Rules from _headers (exact paths and trailing-* prefixes), as Cloudflare applies them.
async function headerRules(root) {
  const rules = new Map()
  let current
  const text = await readFile(resolve(root, '_headers'), 'utf8').catch(() => '')
  for (const line of text.split('\n')) {
    if (!line.trim()) continue
    if (!/^\s/.test(line)) rules.set(line.trim(), current = {})
    else if (current) { const i = line.indexOf(':'); current[line.slice(0, i).trim()] = line.slice(i + 1).trim() }
  }
  return rules
}

// Deliberately no SPA fallback: every page must have its own generated HTML.
// onRequest lets a check record what reached the server (privacy checks count them).
export async function serve(directory = 'dist/client', port = 0, onRequest) {
  const root = resolve(directory)
  const rules = await headerRules(root)
  // Large app wasm staged as gzip parts, answered at its own URL as frames/worker.mjs does.
  const wasmParts = JSON.parse(await readFile(resolve(root, 'apps/frame/wasm-parts.json'), 'utf8').catch(() => '{}'))
  const types = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.woff2': 'font/woff2', '.ttf': 'font/ttf', '.png': 'image/png', '.wasm': 'application/wasm', '.json': 'application/json' }
  const server = createServer(async (req, res) => {
    onRequest?.({ method: req.method, url: req.url })
    const pathname = decodeURIComponent(new URL(req.url, 'http://localhost').pathname)
    for (const [pattern, values] of rules) {
      if (pattern === pathname || (pattern.endsWith('*') && pathname.startsWith(pattern.slice(0, -1)))) for (const [name, value] of Object.entries(values)) res.setHeader(name, value)
    }
    if (!['GET', 'HEAD'].includes(req.method)) { res.writeHead(405).end(); return }
    const joined = wasmParts[pathname]
    if (joined) {
      try {
        const parts = await Promise.all(joined.parts.map(part => readFile(resolve(root, '.' + pathname, '..', part))))
        res.writeHead(200, { 'Content-Type': 'application/wasm', 'Content-Encoding': 'gzip', ETag: `"${joined.sha256}"` })
        res.end(req.method === 'HEAD' ? undefined : Buffer.concat(parts))
      } catch { res.writeHead(502, { 'Content-Type': 'text/plain' }); res.end('Missing wasm parts') }
      return
    }
    try {
      let file = resolve(root, '.' + pathname)
      if (file !== root && !file.startsWith(root + sep)) throw new Error('Outside root')
      if ((await stat(file)).isDirectory()) file = resolve(file, 'index.html')
      const data = await readFile(file)
      res.writeHead(200, { 'Content-Type': types[extname(file)] || 'application/octet-stream' })
      res.end(req.method === 'HEAD' ? undefined : data)
    } catch { res.writeHead(404, { 'Content-Type': 'text/plain' }); res.end('Not found') }
  })
  await new Promise(done => server.listen(port, '127.0.0.1', done))
  return { server, origin: `http://127.0.0.1:${server.address().port}` }
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const { origin } = await serve(process.argv[2], Number(process.argv[3] || 4173))
  console.log(origin)
}
