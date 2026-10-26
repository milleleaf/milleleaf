// Stages the Craft apps' official web releases for the site, each to run in a frame from its own
// index.html: site/public/apps/frame/APP/.
//
// Usage: RELEASES_SRC=<folder holding APP/APP-web-VERSION/> node scripts/stage-releases.mjs
//
// The host rejects any file over 25 MiB, and most apps' wasm is larger. So every wasm over 1 MiB
// is gzipped at level 9 and, if still too large, cut into 20 MiB parts; the plain wasm is not
// staged. A small script, run first in each app's page, answers the app's own fetch of that wasm
// from the gzip (its parts joined) through DecompressionStream, so the release's loader is
// untouched. The wasm's preload link goes, since the file it names is no longer there.

import { gzipSync } from 'node:zlib'
import { mkdir, readdir, readFile, rm, stat, writeFile, cp } from 'node:fs/promises'
import path from 'node:path'

const MAX_BYTES = 25 * 1024 * 1024
const PART_BYTES = 20 * 1024 * 1024
const MIN_COMPRESS = 1024 * 1024

const src = process.env.RELEASES_SRC
if (!src) {
  console.error('Set RELEASES_SRC to the folder holding APP/APP-web-VERSION/')
  process.exit(2)
}
const site = path.resolve(import.meta.dirname, '..')
const out = path.join(site, 'public', 'apps', 'frame')

// Runs before the app's modules; MAP is filled in per app.
const shim = `// Answers this app's fetch of its wasm from the gzipped copy (or its parts) staged beside it.
(() => {
  const map = MAP
  const original = window.fetch.bind(window)
  window.fetch = (input, init) => {
    const url = new URL(input instanceof Request ? input.url : String(input), location.href)
    const parts = url.origin === location.origin && map[url.pathname.split('/').pop()]
    if (!parts) return original(input, init)
    return (async () => {
      const responses = await Promise.all(parts.map((part) => original(new URL(part, url))))
      const failed = responses.find((r) => !r.ok)
      if (failed) throw new Error(failed.url + ': ' + failed.status)
      const joined = new Blob(await Promise.all(responses.map((r) => r.arrayBuffer())))
      // A server that labels the file Content-Encoding: gzip has the browser unzip it already;
      // decompress only what still starts with the gzip signature.
      const head = new Uint8Array(await joined.slice(0, 2).arrayBuffer())
      const body = head[0] === 0x1f && head[1] === 0x8b ? joined.stream().pipeThrough(new DecompressionStream('gzip')) : joined.stream()
      return new Response(body, { headers: { 'Content-Type': 'application/wasm' } })
    })()
  }
})()
`

async function files(dir) {
  const found = []
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name)
    if (entry.isDirectory()) found.push(...(await files(full)))
    else found.push(full)
  }
  return found
}

const index = []
let failed = false
for (const app of (await readdir(src)).sort()) {
  const appDir = path.join(src, app)
  if (!(await stat(appDir)).isDirectory()) continue
  const release = (await readdir(appDir)).find((name) => name.startsWith(`${app}-web-`))
  if (!release) continue
  const destination = path.join(out, app)
  await rm(destination, { recursive: true, force: true })
  await mkdir(path.dirname(destination), { recursive: true })
  await cp(path.join(appDir, release), destination, { recursive: true })

  const map = {}
  for (const file of await files(destination)) {
    if (!file.endsWith('.wasm') || (await stat(file)).size <= MIN_COMPRESS) continue
    const gzipped = gzipSync(await readFile(file), { level: 9 })
    const name = path.basename(file)
    const parts = []
    if (gzipped.byteLength <= MAX_BYTES) {
      await writeFile(`${file}.gz`, gzipped)
      parts.push(`${name}.gz`)
    } else {
      for (let offset = 0, n = 0; offset < gzipped.byteLength; offset += PART_BYTES, n++) {
        const part = `${name}.gz.part-${String.fromCharCode(97 + n)}`
        await writeFile(path.join(path.dirname(file), part), gzipped.subarray(offset, offset + PART_BYTES))
        parts.push(part)
      }
    }
    await rm(file)
    map[name] = parts
  }

  const page = path.join(destination, 'index.html')
  let html = await readFile(page, 'utf8')
  for (const name of Object.keys(map)) {
    html = html.replace(new RegExp(`<link[^>]*href="[^"]*${name.replace(/[.]/g, '\\.')}"[^>]*>\\s*`, 'g'), '')
  }
  if (Object.keys(map).length) {
    await writeFile(path.join(destination, 'craft-wasm-gz.js'), shim.replace('MAP', JSON.stringify(map)))
    html = html.replace(/<head>/i, '<head><script src="./craft-wasm-gz.js"></script>')
  }
  await writeFile(page, html)

  for (const file of await files(destination)) {
    const size = (await stat(file)).size
    if (size > MAX_BYTES) {
      console.error(`FAIL ${app}: ${path.relative(destination, file)} is ${(size / 1048576).toFixed(1)} MiB, over the 25 MiB per-file limit`)
      failed = true
    }
  }
  const version = release.slice(`${app}-web-`.length)
  index.push({ app, version, path: `/apps/frame/${app}/` })
  console.log(`PASS ${app} ${version}: ${Object.entries(map).map(([name, parts]) => `${name} → ${parts.length} gzip file(s)`).join(', ') || 'no wasm over 1 MiB'}`)
}
await writeFile(path.join(out, 'index.json'), JSON.stringify(index, null, 2) + '\n')
if (failed) process.exit(1)
