// Checks the staged apps in frames/dist before they deploy: every app loads from this site under its
// own Content Security Policy and makes no request to another origin; GridCraft opens the phone
// keyboard during a double-tap (craft-touch-keyboard.js) and saves through the save picker or, where
// there is none, as a download.
import { chromium } from '@playwright/test'
import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import path from 'node:path'
import { serve } from '../scripts/serve.mjs'

const dist = path.join(import.meta.dirname, 'dist')
const index = JSON.parse(await readFile(path.join(dist, 'apps', 'frame', 'index.json'), 'utf8'))
const { server, origin } = await serve(dist, 0)
// PW_CHANNEL=chrome runs it in an installed Chrome instead of Playwright's own browser.
const browser = await chromium.launch({ headless: true, args: ['--disable-gpu'], ...(process.env.PW_CHANNEL ? { channel: process.env.PW_CHANNEL } : {}) })
try {
  const desktop = await browser.newContext({ viewport: { width: 1280, height: 900 }, acceptDownloads: true })
  const foreign = []
  desktop.on('request', (request) => { const url = request.url(); if (!/^(data|blob):/.test(url) && new URL(url).origin !== origin) foreign.push(url) })
  for (const { app, version } of index) {
    const page = await desktop.newPage()
    const errors = []
    page.on('pageerror', (error) => errors.push(error.message))
    const response = await page.goto(`${origin}/apps/frame/${app}/`)
    assert.match(response.headers()['content-security-policy'] || '', /default-src 'none'/, `${app}: served with its own policy`)
    await page.locator('canvas').first().waitFor({ timeout: 180_000 })
    await page.waitForTimeout(3000)
    assert.deepEqual(errors, [], `${app}: no uncaught page errors`)
    assert.deepEqual(foreign, [], `${app}: no request to another origin`)
    console.log(`PASS ${app} ${version}: loads under its policy, only this site's requests`)
    await page.close()
  }

  // GridCraft saves: positions are in its own layout (cell A1, the Save button in its title bar).
  const grid = await desktop.newPage()
  await grid.goto(`${origin}/apps/frame/gridcraft/`)
  await grid.locator('canvas').first().waitFor({ timeout: 180_000 })
  await grid.waitForTimeout(5000)
  const typeCell = async (text) => { await grid.mouse.click(120, 140); await grid.keyboard.type(text, { delay: 100 }); await grid.keyboard.press('Enter') }
  const pressSave = () => grid.mouse.click(154, 18)
  await grid.evaluate(() => {
    window.__saved = null
    window.showSaveFilePicker = async (options) => ({ name: options?.suggestedName || 'unnamed', async createWritable() {
      const parts = []
      return {
        async write(data) { parts.push(data instanceof Blob ? new Uint8Array(await data.arrayBuffer()) : data instanceof ArrayBuffer ? new Uint8Array(data) : new Uint8Array(data.buffer, data.byteOffset, data.byteLength)) },
        async close() { window.__saved = { head: String.fromCharCode(...(parts[0] || []).slice(0, 2)), size: parts.reduce((n, part) => n + part.length, 0) } },
      }
    } })
  })
  await typeCell('42')
  await pressSave()
  await grid.waitForFunction(() => window.__saved, null, { timeout: 30_000 })
  const picked = await grid.evaluate(() => window.__saved)
  assert(picked.head === 'PK' && picked.size > 1000, `GridCraft writes an XLSX workbook through the save picker: ${JSON.stringify(picked)}`)
  await grid.evaluate(() => { window.showSaveFilePicker = undefined })
  await typeCell('43')
  const download = grid.waitForEvent('download', { timeout: 30_000 })
  await pressSave()
  assert.match((await download).suggestedFilename(), /\.xlsx$/, 'without a save picker GridCraft downloads the workbook')
  assert.deepEqual(foreign, [], 'GridCraft saves without any request to another origin')
  console.log('PASS gridcraft: saves through the save picker, or as a download; nothing leaves this site')
  await desktop.close()

  // Phones and tablets open the keyboard only for focus given during a tap.
  const touch = await browser.newContext({ viewport: { width: 1024, height: 768 }, deviceScaleFactor: 1, hasTouch: true, isMobile: true })
  await touch.addInitScript(() => {
    window.__focusInTap = []
    document.addEventListener('touchend', () => window.__focusInTap.push(document.activeElement.tagName.toLowerCase()), { capture: true, passive: true })
  })
  const tablet = await touch.newPage()
  await tablet.goto(`${origin}/apps/frame/gridcraft/`)
  // /office adds the keyboard script to each app's page (the release's page is served unchanged); do the same here.
  await tablet.addScriptTag({ url: `${origin}/apps/frame/gridcraft/craft-touch-keyboard.js` })
  await tablet.locator('canvas').first().waitFor({ timeout: 180_000 })
  await tablet.waitForFunction(() => !!document.querySelector('input[type=text]'), null, { timeout: 120_000 })
  await tablet.waitForTimeout(5000)
  const active = () => tablet.evaluate(() => document.activeElement.tagName.toLowerCase())
  await tablet.touchscreen.tap(200, 107); await tablet.waitForTimeout(800)
  assert.equal(await active(), 'canvas', 'a single tap leaves the text field alone')
  await tablet.touchscreen.tap(330, 502); await tablet.waitForTimeout(120); await tablet.touchscreen.tap(330, 502); await tablet.waitForTimeout(800)
  assert.equal(await tablet.evaluate(() => window.__focusInTap.at(-1)), 'input', 'a double-tap on a cell focuses the text field during the tap')
  assert.equal(await active(), 'input', 'the cell keeps the text field focused for typing')
  console.log('PASS gridcraft: a double-tap opens the phone keyboard; a single tap does not')
  await touch.close()
} finally {
  await browser.close()
  server.close()
}
