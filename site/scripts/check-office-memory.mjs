// Memory on /office: start every editor one after another, then close them one by one, and record
// the browser's resident memory after each step (renderer, GPU and all processes, from /proc, so
// WebAssembly memory and graphics buffers count too). It reports numbers; it does not pass or fail on
// them yet. The last check: closing every editor must give most of the growth back.
// Linux only (reads /proc). Run after `npm run build`: node scripts/check-office-memory.mjs [out.json]
import assert from 'node:assert/strict'
import { readFile, writeFile } from 'node:fs/promises'
import { chromium } from '@playwright/test'
import { serve } from './serve.mjs'

const out = process.argv[2]
const { server, origin } = await serve()
const browser = await chromium.launch({ headless: true })
const cdp = await browser.newBrowserCDPSession()

// Resident memory per process type, in MiB, from the browser's own process list.
async function memory() {
  const { processInfo } = await cdp.send('SystemInfo.getProcessInfo')
  const mib = { renderer: 0, gpu: 0, total: 0 }
  for (const { id, type } of processInfo) {
    const status = await readFile(`/proc/${id}/status`, 'utf8').catch(() => '')
    const kib = Number(/VmRSS:\s+(\d+)/.exec(status)?.[1] ?? 0)
    const value = kib / 1024
    if (type === 'renderer') mib.renderer += value
    if (type === 'GPU' || type === 'gpu') mib.gpu += value
    mib.total += value
  }
  for (const key of Object.keys(mib)) mib[key] = Math.round(mib[key])
  return mib
}

const page = await browser.newPage({ viewport: { width: 1280, height: 900 } })
page.setDefaultTimeout(180_000)
await page.goto(origin + '/office')
await page.locator('.office-toolbar').waitFor()
// Apps stay in the page, so the Close button stays reachable between steps.
await page.locator('#office-full-window').uncheck()
const tabs = await page.locator('.office-tab').evaluateAll(elements => elements.map(element => element.id.replace(/^tab-/, '')))
const settle = () => page.waitForTimeout(4000)
// A framed editor is running once its canvas has a size.
const started = app => page.frameLocator(`iframe[data-app="${app}"]`).locator('canvas').first()
  .evaluate(canvas => new Promise(resolve => { const check = () => canvas.width > 0 && canvas.height > 0 ? resolve(true) : setTimeout(check, 250); check() }))

const steps = [{ step: 'desk loaded (GridCraft)', ...(await settle(), await memory()) }]
for (const app of tabs) {
  await page.locator(`#tab-${app}`).click()
  await started(app)
  await settle()
  steps.push({ step: `+ ${app}`, ...(await memory()) })
}
const peak = steps.at(-1)
for (const app of tabs) {
  await page.locator(`#tab-${app}`).click()
  if (await page.locator('#office-close').count() === 0) continue
  await page.locator('#office-close').click()
  if (await page.locator('#office-close-yes').count()) await page.locator('#office-close-yes').click()
  await page.locator('#office-reopen').waitFor()
  await page.evaluate(() => new Promise(resolve => setTimeout(resolve, 0)))
  await settle()
  steps.push({ step: `- ${app}`, ...(await memory()) })
}
const base = steps[0], end = steps.at(-1)

console.log('step'.padEnd(30), 'renderer MiB'.padStart(13), 'GPU MiB'.padStart(9), 'total MiB'.padStart(10))
for (const s of steps) console.log(s.step.padEnd(30), String(s.renderer).padStart(13), String(s.gpu).padStart(9), String(s.total).padStart(10))
if (out) await writeFile(out, JSON.stringify({ origin, editors: tabs, steps }, null, 2))

const growth = peak.total - base.total, kept = end.total - base.total
console.log(`all ${tabs.length} editors open: +${growth} MiB; after closing them all: +${kept} MiB kept`)
assert(growth <= 0 || kept <= growth / 2, `closing every editor should give most of the memory back (+${growth} MiB at the peak, +${kept} MiB kept)`)
console.log(`PASS office memory: ${tabs.length} editors measured; closing gives back ${growth > 0 ? Math.round(100 * (growth - kept) / growth) : 100}% of the growth`)
await browser.close()
server.close()
