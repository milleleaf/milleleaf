import { createServer } from 'node:http'
import { readFile, stat } from 'node:fs/promises'
import { resolve, extname, sep } from 'node:path'
import { pathToFileURL } from 'node:url'

// Deliberately no SPA fallback: every page must have its own generated HTML.
export async function serve(directory = 'dist/client', port = 0) {
  const root = resolve(directory)
  const types = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.woff2': 'font/woff2', '.png': 'image/png', '.wasm': 'application/wasm', '.json': 'application/json' }
  const server = createServer(async (req, res) => {
    try {
      const pathname = decodeURIComponent(new URL(req.url, 'http://localhost').pathname)
      let file = resolve(root, '.' + pathname)
      if (file !== root && !file.startsWith(root + sep)) throw new Error('Outside root')
      if ((await stat(file)).isDirectory()) {
        // The demo pages (add-demos.mjs) use relative paths, so they need the folder slash, as static hosts add it.
        if (['/demo', '/flow'].includes(pathname)) { res.writeHead(301, { Location: pathname + '/' }); res.end(); return }
        file = resolve(file, 'index.html')
      }
      const data = await readFile(file)
      res.writeHead(200, { 'Content-Type': types[extname(file)] || 'application/octet-stream' })
      res.end(data)
    } catch { res.writeHead(404, { 'Content-Type': 'text/plain' }); res.end('Not found') }
  })
  await new Promise(done => server.listen(port, '127.0.0.1', done))
  return { server, origin: `http://127.0.0.1:${server.address().port}` }
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const { origin } = await serve(process.argv[2], Number(process.argv[3] || 4173))
  console.log(origin)
}
