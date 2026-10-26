// Stage the four Craft web editors into site/public/apps/ for page host mode (/office, /flow).
// Input: EDITORS_SRC points at a directory holding one folder per app (gridcraft, pdfcraft,
// wordcraft, cadcraft), each a complete static site built from the branch pinned in
// site/editors.lock.json (index.html, the hashed APP-web-<hash>.js glue, the
// APP-web-<hash>_bg.wasm, licence files, HOSTING/README and _headers).
// The script copies each folder to site/public/apps/APP/, gzips the wasm at level 9 so no file
// breaks the host's 25 MiB per-file limit (the page decompresses it with DecompressionStream),
// removes the raw .wasm, and writes site/public/apps/editors.json with the version, commit,
// glue and wasm file names and the canvas id read from each app's index.html.
// Usage: EDITORS_SRC=/path/to/staged-editors node scripts/stage-editors.mjs
import { gzipSync } from 'node:zlib'
import { createHash } from 'node:crypto'
import { cp, mkdir, readFile, readdir, rm, stat, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('../', import.meta.url))
const MAX_BYTES = 25 * 1024 * 1024

const lock = JSON.parse(await readFile(path.join(root, 'editors.lock.json'), 'utf8'))
// Every app pinned in the lock is staged.
const APPS = Object.keys(lock)
const sourceDir = process.env.EDITORS_SRC
if (!sourceDir) {
  console.error('Set EDITORS_SRC to the directory of staged editor folders (one folder per app).')
  process.exit(1)
}

const editors = {}
for (const app of APPS) {
  const staged = path.join(sourceDir, app)
  let entries
  try { entries = await readdir(staged) } catch {
    console.error(`FAIL ${app}: no staged folder at ${staged}`)
    process.exit(1)
  }

  // trunk builds name the glue APP-web-<hash>.js; plain wasm-bindgen builds (FilmCraft) APP_web.js.
  const glue = entries.find(name => new RegExp(`^${app}(-web-[0-9a-f]+|_web)\\.js$`).test(name))
  const wasm = entries.find(name => new RegExp(`^${app}(-web-[0-9a-f]+|_web)_bg\\.wasm$`).test(name))
  if (!glue || !wasm) {
    console.error(`FAIL ${app}: missing ${app}-web-<hash>.js glue or ${app}-web-<hash>_bg.wasm in ${staged}`)
    process.exit(1)
  }

  const html = await readFile(path.join(staged, 'index.html'), 'utf8')
  const canvasMatch = html.match(new RegExp(`<canvas[^>]*id=["']([^"']+)["'][^>]*>`))
  if (!canvasMatch) {
    console.error(`FAIL ${app}: no <canvas> id found in its index.html`)
    process.exit(1)
  }
  const canvasId = canvasMatch[1]
  // The staged index.html is the standalone app's own page and does not carry data-host: the
  // hosting page adds it to the canvas it renders. The lookup of that attribute is compiled into
  // the wasm, so a build without page host mode has no "data-host" string in it.
  if (!(await readFile(path.join(staged, wasm))).includes('data-host')) {
    console.error(`FAIL ${app}: the staged build has no page host mode (no data-host lookup in its wasm)`)
    process.exit(1)
  }

  const destination = path.join(root, 'public/apps', app)
  await rm(destination, { recursive: true, force: true })
  await cp(staged, destination, { recursive: true })

  // Gzip at level 9 for the host's per-file limit; the page decompresses with DecompressionStream.
  const wasmBytes = await readFile(path.join(destination, wasm))
  const gzipped = gzipSync(wasmBytes, { level: 9 })
  if (gzipped.byteLength > MAX_BYTES) {
    console.error(`FAIL ${app}: ${app}-web-*.wasm.gz is ${(gzipped.byteLength / 1048576).toFixed(1)} MiB, over the 25 MiB per-file limit`)
    await rm(path.join(destination, wasm))
    process.exit(1)
  }
  await writeFile(path.join(destination, `${wasm}.gz`), gzipped)
  await rm(path.join(destination, wasm))
  console.log(`PASS ${app}: wasm gzipped ${(wasmBytes.byteLength / 1048576).toFixed(1)} → ${(gzipped.byteLength / 1048576).toFixed(1)} MiB`)

  // No staged file (including licence, README, HOSTING and _headers) may break the limit either.
  for (const name of await readdir(destination)) {
    const info = await stat(path.join(destination, name))
    if (info.isFile() && info.size > MAX_BYTES) {
      console.error(`FAIL ${app}: ${name} is ${(info.size / 1048576).toFixed(1)} MiB, over the 25 MiB per-file limit`)
      process.exit(1)
    }
  }

  editors[app] = {
    version: lock[app].version,
    commit: lock[app].commit,
    pull: lock[app].pull,
    repo: lock[app].repo,
    glue,
    wasm: `${wasm}.gz`,
    canvasId,
    // A glue that exports start(canvasId) needs it called after init (the app doesn't start itself).
    start: /export function start\(/.test(await readFile(path.join(staged, glue), 'utf8')),
  }
}

// Sanity-check every app against the lock so a stale build is caught, not silently staged.
for (const app of APPS) {
  const html = await readFile(path.join(root, 'public/apps', app, 'index.html'), 'utf8')
  const version = html.match(new RegExp(`${app}:ready[^\\d]*([0-9.]+)`, 'i'))?.[1]
  if (version && version !== editors[app].version) {
    console.error(`FAIL ${app}: staged build reports version ${version}, editors.lock.json pins ${editors[app].version}`)
    process.exit(1)
  }
}

await writeFile(path.join(root, 'public/apps/editors.json'), `${JSON.stringify(editors, null, 2)}\n`)
console.log(`PASS editors.json written for ${APPS.join(', ')}`)
