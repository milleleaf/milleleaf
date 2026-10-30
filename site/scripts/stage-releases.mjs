// Stages the Craft apps' official web releases for the site, each to run in a frame from its own
// index.html: site/public/apps/frame/APP/.
//
// Usage: RELEASES_SRC=<folder holding APP/APP-web-VERSION/> node scripts/stage-releases.mjs
// RELEASES_OUT stages into another folder (frames/ uses it); RELEASES_HEADERS also writes the apps'
// _headers rules to that file.
//
// The host rejects any file over 25 MiB, and most apps' wasm is larger. So every wasm over 1 MiB
// is gzipped at level 9 and, if still too large, cut into 20 MiB parts; the plain wasm is not
// staged. wasm-parts.json lists each one's parts and SHA-256: the server (frames/worker.mjs, and
// scripts/serve.mjs for the checks) answers the wasm's own URL with them joined as gzip. Every
// other file is the release's own, unchanged, so the apps' service workers find the files they list.
//
// Each app's folder also gets scripts/craft-touch-keyboard.js, which /office adds to the app's page
// so phones and tablets open their on-screen keyboard when the app wants text (see that file).

import { createHash } from 'node:crypto'
import { gzipSync } from 'node:zlib'
import { readFileSync } from 'node:fs'
import { mkdir, readdir, readFile, rm, stat, writeFile, cp } from 'node:fs/promises'
import path from 'node:path'
import { frameRule } from './frame-csp.mjs'

const MAX_BYTES = 25 * 1024 * 1024
const PART_BYTES = 20 * 1024 * 1024
const MIN_COMPRESS = 1024 * 1024

const src = process.env.RELEASES_SRC
if (!src) {
  console.error('Set RELEASES_SRC to the folder holding APP/APP-web-VERSION/')
  process.exit(2)
}
const site = path.resolve(import.meta.dirname, '..')
const out = process.env.RELEASES_OUT ? path.resolve(process.env.RELEASES_OUT) : path.join(site, 'public', 'apps', 'frame')

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
const wasmParts = {}
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
    const wasm = await readFile(file)
    const gzipped = gzipSync(wasm, { level: 9 })
    const name = path.basename(file)
    // Parts are named <wasm>.gz.part-a, -b…, a name no release uses (LightCraft ships its own .gz and
    // .br beside its files, which stay as they are); a name the release has is refused.
    const parts = []
    for (let offset = 0, n = 0; offset < gzipped.byteLength; offset += PART_BYTES, n++) {
      const part = `${name}.gz.part-${String.fromCharCode(97 + n)}`
      const target = path.join(path.dirname(file), part)
      if (await stat(target).then(() => true, () => false)) {
        console.error(`FAIL ${app}: ${part} is already in the release`)
        failed = true
        continue
      }
      await writeFile(target, gzipped.subarray(offset, offset + PART_BYTES))
      parts.push(part)
    }
    await rm(file)
    map[name] = parts
    const dir = path.relative(out, path.dirname(file)).split(path.sep).join('/')
    wasmParts[`/apps/frame/${dir}/${name}`] = { parts, sha256: createHash('sha256').update(wasm).digest('hex') }
  }
  await cp(path.join(site, 'scripts', 'craft-touch-keyboard.js'), path.join(destination, 'craft-touch-keyboard.js'))
  // Every file of the release is staged as it is, but for the wasm kept as parts (checked by hash).
  for (const original of await files(path.join(appDir, release))) {
    const relative = path.relative(path.join(appDir, release), original)
    const staged = path.join(destination, relative)
    const kept = map[path.basename(original)] && wasmParts[`/apps/frame/${app}/${relative.split(path.sep).join('/')}`]
    const same = kept
      ? createHash('sha256').update(await readFile(original)).digest('hex') === kept.sha256
      : await readFile(staged).then((bytes) => bytes.equals(readFileSync(original))).catch(() => false)
    if (!same) { console.error(`FAIL ${app}: ${relative} is not staged as the release has it`); failed = true }
  }

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
await writeFile(path.join(out, 'wasm-parts.json'), JSON.stringify(wasmParts, null, 2) + '\n')
if (process.env.RELEASES_HEADERS) {
  const rules = []
  for (const { app } of index) rules.push(frameRule(app, await readFile(path.join(out, app, 'index.html'), 'utf8')))
  await writeFile(process.env.RELEASES_HEADERS, rules.join(''))
}
if (failed) process.exit(1)
