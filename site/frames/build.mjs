// Cloudflare's build for the /apps/frame/ Worker: fetch the releases pinned in versions.json (checksums
// checked), stage them as the site does (scripts/stage-releases.mjs) into dist/apps/frame/, and write
// their _headers. Needs only Node and unzip (or Python).
import { execFileSync } from 'node:child_process'
import { mkdir, rm } from 'node:fs/promises'
import path from 'node:path'

const here = import.meta.dirname
const releases = path.join(here, '.releases')
const dist = path.join(here, 'dist')
await rm(dist, { recursive: true, force: true })
await mkdir(releases, { recursive: true })
const run = (script, args, env = {}) => execFileSync(process.execPath, [script, ...args], { stdio: 'inherit', env: { ...process.env, ...env } })
run(path.join(here, 'releases.mjs'), ['fetch', releases])
run(path.join(here, '..', 'scripts', 'stage-releases.mjs'), [], { RELEASES_SRC: releases, RELEASES_OUT: path.join(dist, 'apps', 'frame'), RELEASES_HEADERS: path.join(dist, '_headers') })
