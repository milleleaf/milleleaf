// The Craft apps' official web releases for the /apps/frame/ Worker.
//   node releases.mjs latest        writes the newest release of each app into versions.json
//   node releases.mjs fetch <dir>   downloads the versions in versions.json into <dir>/APP/APP-web-VERSION/,
//                                   each zip checked against its release's SHA256SUMS.txt
import { createHash } from 'node:crypto'
import { execFileSync } from 'node:child_process'
import { mkdir, readFile, rm, writeFile } from 'node:fs/promises'
import path from 'node:path'

const here = import.meta.dirname
const versionsFile = path.join(here, 'versions.json')
const versions = JSON.parse(await readFile(versionsFile, 'utf8'))
const [mode, dir] = process.argv.slice(2)
// The GitHub owner of each app's repository: the storytold org unless named here.
const owners = { solidcraft: 'tangency-labs' }
const repo = (app) => `${owners[app] ?? 'storytold'}/${app}`
// Releases known not to start here, kept off until a newer one comes out; each with its issue.
const skip = {
  photocraft: { '0.17.0': 'storytold/photocraft#10017: app/photocraft-web.js imports ./snippets/…/inline0.js, which the zip ships only at the top level' },
}

async function get(url, init) {
  const response = await fetch(url, init)
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`)
  return response
}

if (mode === 'latest') {
  const headers = { Accept: 'application/vnd.github+json', ...(process.env.GITHUB_TOKEN ? { Authorization: `Bearer ${process.env.GITHUB_TOKEN}` } : {}) }
  const next = {}
  for (const app of Object.keys(versions)) {
    const release = await (await get(`https://api.github.com/repos/${repo(app)}/releases/latest`, { headers })).json()
    const version = release.tag_name.replace(/^v/, '')
    if (skip[app]?.[version]) {
      console.log(`${app}: ${version} skipped (${skip[app][version]}), keeping ${versions[app]}`)
      next[app] = versions[app]
      continue
    }
    if (!release.assets.some((asset) => asset.name === `${app}-web-${version}.zip`)) {
      console.log(`${app}: ${version} has no web build, keeping ${versions[app]}`)
      next[app] = versions[app]
      continue
    }
    next[app] = version
    console.log(`${app}: ${versions[app]}${version === versions[app] ? '' : ` -> ${version}`}`)
  }
  await writeFile(versionsFile, JSON.stringify(next, null, 2) + '\n')
} else if (mode === 'fetch' && dir) {
  for (const [app, version] of Object.entries(versions)) {
    const base = `https://github.com/${repo(app)}/releases/download/v${version}`
    const name = `${app}-web-${version}.zip`
    const sums = await (await get(`${base}/SHA256SUMS.txt`)).text()
    const expected = sums.split('\n').map((line) => line.trim().split(/\s+/)).find(([, file]) => file === name)?.[0]
    if (!expected) throw new Error(`${app}: ${name} is not listed in SHA256SUMS.txt`)
    const zip = Buffer.from(await (await get(`${base}/${name}`)).arrayBuffer())
    const actual = createHash('sha256').update(zip).digest('hex')
    if (actual !== expected) throw new Error(`${app}: ${name} sha256 ${actual}, release says ${expected}`)
    const target = path.join(dir, app)
    await rm(target, { recursive: true, force: true })
    await mkdir(target, { recursive: true })
    const file = path.join(target, name)
    await writeFile(file, zip)
    try { execFileSync('unzip', ['-q', file, '-d', target]) } catch { execFileSync('python3', ['-I', '-m', 'zipfile', '-e', file, target]) }
    await rm(file)
    console.log(`PASS ${app} ${version}: sha256 matches the release`)
  }
} else {
  console.error('Usage: node releases.mjs latest | fetch <dir>')
  process.exit(2)
}
