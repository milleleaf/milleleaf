import assert from 'node:assert/strict'
import { mkdir, writeFile, readFile } from 'node:fs/promises'
import { chromium } from '@playwright/test'
import { serve } from './serve.mjs'

const paths = ['/', '/flow', '/demo', '/office', '/how-it-works', '/data', '/data/round-trip', '/data/worldviews', '/data/datasets', '/method', '/docs', '/docs/getting-started', '/status', '/about', '/privacy', '/keystone', '/standards']
const headings = ['Make it once.Use it in every app.', 'Make it once.Use it in every app.', 'Follow the file.See what changed.', 'Fourteen editors.Nothing to install.', 'One source. Every presentation.', 'Follow the data to its source.', 'What survives the save?', 'Real places. Sources in view.', 'Datasets and their terms.', 'Make the claim the evidence supports.', 'Start with a file you can check.', 'Try it. Inspect what comes back.', 'Built. Checked. Still to come.', 'Useful tools. Evidence in view.', 'We collect nothing about you.', 'Keystone: openBIM ecosystem', 'Standards, with the checks beside them.']
const { server, origin } = await serve()
const browser = await chromium.launch({ headless: true })
const captures = []
const internal = new Set()
const failures = []
await mkdir('_shots', { recursive: true })
try {
  // No runtime server or SPA fallback, and no JavaScript needed for page content.
  const nojs = await browser.newContext({ javaScriptEnabled: false })
  const staticPage = await nojs.newPage()
  for (const path of paths) {
    const response = await staticPage.goto(origin + path)
    assert.equal(response.status(), 200, path)
    assert.equal(await staticPage.locator('h1').count(), 1, path)
    assert.equal(await staticPage.locator('h1').textContent(), headings[paths.indexOf(path)], `Page identity ${path}`)
    assert(await staticPage.locator('h1').isVisible(), path)
    assert(await staticPage.locator('main').innerText(), path)
    for (const href of await staticPage.locator('a[href^="/"]').evaluateAll(nodes => nodes.map(n => n.getAttribute('href')))) internal.add(href)
    console.log(`PASS static HTML, JavaScript disabled: ${path}`)
  }
  assert.equal((await staticPage.goto(origin + '/not-a-page')).status(), 404)
  await staticPage.goto(origin + '/data/datasets')
  const tables = staticPage.locator('.dataset-table')
  assert.equal(await tables.count(), 2)
  assert.equal(await tables.nth(0).locator('tbody tr').count(), 14)
  assert.equal(await tables.nth(1).locator('tbody tr').count(), 2)
  assert.deepEqual(await tables.nth(0).locator('thead th').allTextContents(), ['Dataset', 'What it is', 'Area', 'Updated', 'Licence', 'Source', 'Links'])
  const otherNames = await tables.nth(1).locator('tbody th').allTextContents()
  assert.deepEqual(otherNames, ['world-building-green', 'world-places-nc'])
  assert.equal(await tables.nth(0).locator('tr').filter({ hasText: 'sa-fire' }).locator('[data-label="Licence"]').textContent(), "CC BY 4.0 (one source's 2015 zip notes CC BY 3.0 AU; see card)")
  assert.equal(await staticPage.locator('.section-index, .article-layout aside').count(), 0)
  assert(!(await staticPage.locator('main').innerText()).includes('needs review'))
  assert(await staticPage.getByText('The World Heritage view is not affiliated with or endorsed by UNESCO.', { exact: true }).isVisible())
  assert.equal(await tables.nth(1).getByRole('link', { name: 'world-places-nc GitHub pipeline', exact: true }).getAttribute('href'), 'https://github.com/milleleaf/world-places')
  const security = await staticPage.request.get(origin + '/.well-known/security.txt')
  assert.equal(security.status(), 200)
  assert.equal(await security.text(), await readFile('public/.well-known/security.txt', 'utf8'))
  assert((await security.text()).includes('Contact: mailto:security@milleleaf.com'))
  await staticPage.goto(origin + '/about')
  assert((await staticPage.locator('main').innerText()).includes('security@milleleaf.com'))
  assert.equal(await staticPage.locator('main a[href="mailto:security@milleleaf.com"]').count(), 1, 'Emails in page text are mailto links')
  assert.equal(await staticPage.locator('.footer-contact a[href^="mailto:"]').count(), 2, 'Footer emails are mailto links')
  await staticPage.goto(origin + '/standards')
  assert.equal(await staticPage.locator('.standards-table tbody tr').count(), 13)
  assert(!(await staticPage.locator('main').innerText()).includes('—'))
  const evidence = JSON.parse(await readFile('src/data/hero.json', 'utf8'))
  assert.equal(evidence.signed.exitCode, 0)
  assert(evidence.signed.verification.includes('signed by fixture-reviewer'))
  assert(evidence.frames[1].report.changed.length > 0 && evidence.frames[1].report.conflicts.length > 0)
  assert.equal(evidence.layerBytes.includes('site/hero/resaved'), true)
  await staticPage.goto(origin + '/standards')
  assert.equal(await staticPage.locator('.standards-table tbody tr').count(), 13)
  for (const standard of ['RFC 8785', 'RFC 8032', 'FIPS 204', 'FIPS 205', 'RFC 9180', 'RFC 3161', 'OpenTimestamps', 'DocLang 0.8', 'WCAG 2.2 AA', 'ISO 19650']) assert((await staticPage.locator('main').innerText()).includes(standard))
  for (const path of ['/', '/how-it-works']) {
    await staticPage.goto(origin + path)
    const copy = await staticPage.locator('main').innerText()
    assert(copy.includes('Why not just open it in another app?'))
    assert(copy.includes('Microsoft Office: checked with real Word/Excel/PowerPoint 16 files'))
    assert(copy.includes('LibreOffice: checked with LibreOffice re-saves and edits'))
    assert(copy.includes('ArtCraft apps: files written through WordCraft, GridCraft and DeckCraft'))
    assert(!copy.includes('—'), 'No em dashes')
  }
  await staticPage.goto(origin)
  assert.equal(await staticPage.locator('.alternatives-table tbody tr').count(), 5)
  assert.deepEqual(await staticPage.locator('.alternatives-table tbody th').allTextContents(), ['Copy/paste', 'Paste Link', 'Import/export', 'Plugins', 'Milleleaf'])
  assert.equal(await staticPage.locator('.alternatives-table tbody td small').count(), 20)
  assert((await staticPage.locator('.app-families').innerText()).includes('Being checked in the browser flow'))
  await staticPage.goto(origin + '/how-it-works')
  for (const text of ['Why not copy and paste?', 'Why not Paste Link or linked objects?', 'Why not import and export?', 'Why not a plugin or add-in?', 'What Milleleaf adds', '400 cells', 'not documented', 'signing is optional today']) assert((await staticPage.locator('main').innerText()).includes(text))
  assert(evidence.flow, 'Real mixed-app flow generated')
  for (const snapshot of [evidence.flow.before, evidence.flow.after]) {
    assert.equal(snapshot.checks.length, 3)
    assert.deepEqual(snapshot.deck.chart.categories, snapshot.source.rows.map(r => r[0]))
    assert.deepEqual(snapshot.deck.chart.values, snapshot.source.rows.map(r => r[1]))
  }
  assert.notDeepEqual(evidence.flow.before.source.rows, evidence.flow.after.source.rows)
  await nojs.close()

  for (const href of internal) {
    const url = new URL(href, origin)
    const file = /\.[a-z0-9]+$/i.test(url.pathname)
      ? 'dist/client' + url.pathname
      : 'dist/client' + (url.pathname === '/' ? '' : url.pathname) + '/index.html'
    const html = await readFile(file, 'utf8')
    if (url.hash) assert(html.includes(`id="${url.hash.slice(1)}"`), `Missing anchor ${href}`)
  }

  for (const width of [1280, 400]) for (const theme of ['light', 'dark']) {
    const context = await browser.newContext({ viewport: { width, height: 900 }, colorScheme: theme, reducedMotion: 'reduce' })
    await context.addInitScript(t => localStorage.setItem('milleleaf-theme', t), theme)
    const page = await context.newPage()
    page.on('pageerror', error => failures.push(error.message))
    page.on('console', message => {
      if (message.type() === 'error') failures.push(message.text())
    })
    page.on('response', response => {
      if (response.status() >= 400) failures.push(`${response.status()} ${response.url()}`)
    })
    for (const path of paths) {
      assert.equal((await page.goto(origin + path)).status(), 200)
      await page.evaluate(() => document.fonts.ready)
      await page.waitForFunction(() => !!document.querySelector('ml-theme')?.shadowRoot)
      if (path === '/flow' || path === '/demo') await page.locator('#status').filter({ hasText: /^Ready\./ }).waitFor({ timeout: 60_000 })
      assert.equal(await page.locator('h1').count(), 1)
      assert.equal(await page.locator('h1').textContent(), headings[paths.indexOf(path)], `Hydrated page identity ${path}`)
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, `Overflow ${path} ${width} ${theme}`)
      assert.equal(await page.evaluate(() => getComputedStyle(document.documentElement).scrollBehavior), 'auto')
      assert.equal(await page.evaluate(() => document.documentElement.dataset.theme), theme)
      if (path === '/') {
        if (width === 400) assert((await page.locator('.alternatives-table caption').boundingBox()).width >= width * .75, 'Mobile caption uses the table width')
        if (width === 1280) assert.equal(await page.locator('.alternatives-table thead th').first().evaluate(el => getComputedStyle(el).color), await page.locator('.alternatives-band h2').evaluate(el => getComputedStyle(el).color), 'Table header uses dark ink on mint')
        for (const cell of await page.locator('.alternatives-table td').all()) assert(await cell.locator('small').isVisible(), 'Every comparison cell has visible evidence')
      }
      if (path === '/data/datasets') {
        assert.equal(await page.locator('.dataset-table tbody tr').count(), 16)
        for (const row of await page.locator('.dataset-table tbody tr').all()) {
          assert.equal(await row.locator('th, td').count(), 7)
          assert(await row.locator('[data-label="Licence"]').isVisible())
          const bounds = await row.boundingBox()
          assert(bounds.x >= 0 && bounds.x + bounds.width <= width, 'Dataset row stays within viewport')
        }
        if (width === 400) {
          assert.equal(await page.locator('.dataset-table tbody td[data-label="Updated"]').first().evaluate(el => getComputedStyle(el, '::before').content), '"Updated"')
        }
      }
      const name = `${path === '/' ? 'home' : path.slice(1).replaceAll('/', '-')}-${width}-${theme}.png`
      await page.screenshot({ path: '_shots/' + name, fullPage: true, animations: 'disabled' })
      captures.push({ path, width, theme, file: name })
      console.log(`PASS capture ${name}`)
    }
    await context.close()
  }

  const context = await browser.newContext({ viewport: { width: 400, height: 900 }, colorScheme: 'light' })
  const page = await context.newPage()
  page.on('pageerror', e => failures.push(e.message))
  await page.goto(origin)
  await page.waitForFunction(() => !!document.querySelector('ml-theme')?.shadowRoot)
  await page.keyboard.press('Tab')
  assert.equal(await page.evaluate(() => document.activeElement.textContent), 'Skip to content')
  assert.notEqual(await page.locator('.skip-link').evaluate(el => getComputedStyle(el).outlineStyle), 'none')
  await page.locator('ml-theme').getByRole('button').click()
  const selected = await page.evaluate(() => document.documentElement.dataset.theme)
  await page.reload()
  assert.equal(await page.evaluate(() => document.documentElement.dataset.theme), selected, 'Theme survives reload')
  await page.locator('.mobile-menu summary').click()
  await page.getByRole('navigation', { name: 'Mobile navigation', exact: true }).getByRole('link', { name: 'Data', exact: true }).click()
  assert.equal(new URL(page.url()).pathname, '/data')
  await page.goto(origin)
  const apply = page.getByRole('button', { name: '3. Apply the real Calc edit', exact: true })
  assert.equal(await page.getByRole('table', { name: 'Workbook source revenue', exact: true }).getByText('Perth').count(), 0)
  await apply.click()
  for (const name of ['Workbook source revenue', 'Report linked revenue table', 'Deck linked revenue table']) assert(await page.getByRole('table', { name, exact: true }).getByText('Perth').isVisible())
  assert(await page.getByRole('status').filter({ hasText: 'Both exports follow' }).isVisible())
  await page.getByRole('button', { name: 'Show the original numbers', exact: true }).click()
  assert(await page.locator('.document-sheet ins').count() > 0, 'Real text edits are visible')
  assert(await page.locator('.conflict-cell').count() > 0, 'Real cell conflict is visible')
  await page.locator('ml-revisions').getByRole('button', { name: '3. Re-saved in LibreOffice', exact: true }).click()
  assert.equal(await page.locator('ml-revisions').getByRole('button', { name: '3. Re-saved in LibreOffice', exact: true }).getAttribute('aria-pressed'), 'true')
  await page.locator('ml-revisions').getByRole('button', { name: '3. Re-saved in LibreOffice', exact: true }).press('ArrowRight')
  assert.equal(await page.locator('ml-revisions').getByRole('button', { name: '4. Signed layer', exact: true }).getAttribute('aria-pressed'), 'true')
  await page.locator('ml-verify').getByRole('button').click()
  await page.locator('ml-verify').getByRole('status').filter({ hasText: 'signature match in this browser' }).waitFor()
  await page.evaluate(async () => {
    const data = await (await fetch('/evidence/hero.json')).json()
    document.querySelector('ml-verify').options = { bytes: data.layerBytes + ' ', layer: data.signed.layer, ...data.signed.signature }
  })
  await page.locator('ml-verify').getByRole('button').click()
  await page.locator('ml-verify').getByRole('status').filter({ hasText: 'Layer hash does not match' }).waitFor()
  // The demos are site pages: the React layout plus one demo component each, under a strict policy.
  // /office frames the real editors from their official release pages on this site (same-origin
  // connections only; a frame may only be one of this site's /apps/frame/ editor pages).
  const headers = await readFile('dist/client/_headers', 'utf8')
  // /office budgets memory by the device; the check runs as a 2 GB device.
  await page.addInitScript(() => Object.defineProperty(Navigator.prototype, 'deviceMemory', { get: () => 2, configurable: true }))
  for (const path of ['/flow', '/demo', '/office']) {
    const response = await page.goto(origin + path)
    const csp = response.headers()['content-security-policy']
    assert(csp && headers.includes(`${path}\n  Content-Security-Policy: ${csp}`), `${path}: strict policy from _headers`)
    if (path === '/demo') assert(csp.includes("connect-src 'none'"), '/demo stays offline')
    else assert(csp.includes("connect-src 'self'") && !csp.includes("connect-src 'none'"), `${path} allows same-origin connections only`)
    if (path === '/office') {
      await page.locator('.office-toolbar').waitFor({ timeout: 60_000 })
      await page.locator('iframe[data-app="gridcraft"]').waitFor({ timeout: 60_000 })
      const frames = await page.locator('iframe').evaluateAll((elements) => elements.map((element) => element.src))
      for (const src of frames) assert(new URL(src).origin === origin && new URL(src).pathname.startsWith('/apps/frame/'), `${path}: only this site's editor pages may be framed, found ${src}`)
      await page.frameLocator('iframe[data-app="gridcraft"]').locator('script#craft-touch-keyboard').waitFor({ state: 'attached', timeout: 60_000 })
      console.log(`PASS page ${path}: office desk under the strict policy, site layout, ${frames.length} framed editor(s) from this site, each given the touch keyboard script`)
      // Closing an editor removes its frame (its memory goes back); a framed editor can't report
      // unsaved work, so closing it asks first, and its tab opens it again.
      await page.locator('#office-close').click()
      await page.locator('#office-close-confirm').waitFor()
      await page.locator('#office-close-no').click()
      assert.equal(await page.locator('iframe[data-app="gridcraft"]').count(), 1, '/office: Keep it open keeps the editor')
      await page.locator('#office-close').click()
      await page.locator('#office-close-yes').click()
      await page.locator('#office-reopen').waitFor()
      assert.equal(await page.locator('iframe[data-app="gridcraft"]').count(), 0, '/office: a closed editor has no frame')
      assert.match(await page.locator('#office-notice').textContent(), /GridCraft is closed and its memory is released/)
      await page.locator('#office-reopen').click()
      await page.locator('iframe[data-app="gridcraft"]').waitFor({ timeout: 60_000 })
      console.log(`PASS page ${path}: closing an editor asks first, removes its frame, and its tab reopens it`)
      // An app's button opens it in full window by default; the reader can turn that off, and the
      // choice is remembered in this browser.
      const fullWindow = page.locator('#office-full-window')
      assert(await fullWindow.isChecked(), '/office: apps open in full window by default')
      await page.locator('#tab-gridcraft').click()
      await page.locator('.craft-editor-shell.expanded').waitFor()
      await page.locator('.craft-editor-exit').click()
      await page.locator('.craft-editor-shell.expanded').waitFor({ state: 'detached' })
      await fullWindow.uncheck()
      await page.locator('#tab-gridcraft').click()
      await page.waitForTimeout(500)
      assert.equal(await page.locator('.craft-editor-shell.expanded').count(), 0, '/office: with the option off, an app opens in the page')
      await page.reload()
      await page.locator('#office-full-window').waitFor()
      await page.waitForFunction(() => !document.querySelector('#office-full-window').checked)
      await page.locator('#office-full-window').check()
      console.log(`PASS page ${path}: app buttons open in full window by default; the reader can turn it off and the choice is remembered`)
      // An app hands the page a file another app edits: from GridCraft's frame, a small DXF goes to
      // CADCraft and a small PDF to PdfCraft, whose releases speak the host messages. The target's tab
      // opens with the file, GridCraft hears the hand-off is done, and the target's own save reaches
      // the page as a download.
      await page.locator('iframe[data-app="gridcraft"]').waitFor({ timeout: 60_000 })
      const gridFrame = page.frame({ url: /\/apps\/frame\/gridcraft\// })
      await gridFrame.evaluate(() => {
        window.__replies = []
        addEventListener('message', (event) => { if (String(event.data?.type).startsWith('gridcraft:')) window.__replies.push(event.data) })
      })
      const pdfObjects = ['<< /Type /Catalog /Pages 2 0 R >>', '<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
        '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>',
        '<< /Length 37 >>\nstream\nBT /F1 24 Tf 72 700 Td (Report) Tj ET\nendstream', '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>']
      let pdf = '%PDF-1.4\n'
      const offsets = pdfObjects.map((object, i) => { const at = pdf.length; pdf += `${i + 1} 0 obj\n${object}\nendobj\n`; return at })
      const xref = pdf.length
      pdf += `xref\n0 6\n0000000000 65535 f \n${offsets.map((at) => `${String(at).padStart(10, '0')} 00000 n \n`).join('')}trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`
      const handoffs = [
        { app: 'cadcraft', label: 'CADCraft', name: 'plan.dxf', text: '0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n0\n30\n0\n11\n100\n21\n100\n31\n0\n0\nENDSEC\n0\nEOF\n' },
        { app: 'pdfcraft', label: 'PdfCraft', name: 'report.pdf', text: pdf },
      ]
      for (const { app, label, name, text } of handoffs) {
        await gridFrame.evaluate(({ name, text }) => parent.postMessage({ type: 'gridcraft:handoff', name, bytes: new TextEncoder().encode(text).buffer }, location.origin), { name, text })
        const target = page.locator(`iframe[data-app="${app}"]`)
        await target.waitFor({ timeout: 60_000 })
        assert.equal(new URL(await target.getAttribute('src'), origin).searchParams.get('host'), 'parent', `/office: ${label} is framed with ?host=parent`)
        await page.locator('#office-notice').filter({ hasText: `${name} opened in ${label}.` }).waitFor({ timeout: 120_000 })
        assert.equal(await page.locator(`#tab-${app}`).getAttribute('aria-selected'), 'true', `/office: the hand-off opens the ${label} tab`)
        await gridFrame.waitForFunction((app) => window.__replies.some((reply) => reply.type === 'gridcraft:handoff-done' && reply.app === app), app, { timeout: 30_000 })
        const saved = page.waitForEvent('download', { timeout: 30_000 })
        await target.scrollIntoViewIfNeeded()
        const box = await target.boundingBox()
        await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2)
        await page.keyboard.press('Control+s')
        assert.equal((await saved).suggestedFilename(), name, `/office: a ${label} save downloads ${name}`)
        await page.locator('#office-notice').filter({ hasText: `${label} saved ${name} to your Downloads.` }).waitFor()
        console.log(`PASS page ${path}: ${name} handed off from GridCraft opens in ${label} (?host=parent), GridCraft hears it is done, and ${label}'s save downloads it`)
      }
      // The running apps stay within a memory budget for the device: this page reports 2 GB
      // (navigator.deviceMemory), so the budget is 512 MB. GridCraft, CADCraft and PdfCraft run now (about
      // 379 MB by their measured weights). SolidCraft (340) goes over: the page asks to close GridCraft
      // (used least recently, and it can't report unsaved work); keeping it is respected, then CADCraft
      // and PdfCraft, which report no unsaved work, close by themselves until the apps fit. CADCraft's
      // tab then reopens it with the file it had, plan.dxf.
      assert.equal(await page.evaluate(() => navigator.deviceMemory), 2, '/office: the check runs as a 2 GB device')
      const running = (app) => page.locator(`iframe[data-app="${app}"]`).count()
      await page.locator('#office-full-window').uncheck()
      await page.locator('#tab-solidcraft').click()
      await page.locator('#office-room-confirm').filter({ hasText: 'GridCraft' }).waitFor({ timeout: 60_000 })
      assert.match(await page.locator('#office-room-question').textContent(), /budget of about 512 MB/)
      await page.locator('#office-room-no').click()
      await page.waitForFunction(() => !document.querySelector('iframe[data-app="cadcraft"]') && !document.querySelector('iframe[data-app="pdfcraft"]') || document.querySelector('#office-room-confirm'), null, { timeout: 60_000 })
      while (await page.locator('#office-room-confirm').count()) { await page.locator('#office-room-yes').click(); await page.waitForTimeout(500) }
      assert.equal(await running('gridcraft'), 1, '/office: the kept app still runs')
      assert.equal(await running('solidcraft'), 1, '/office: the app just opened runs')
      assert.equal((await running('cadcraft')) + (await running('pdfcraft')), 0, '/office: the apps that report no unsaved work closed to make room')
      await page.locator('#tab-cadcraft').click()
      await page.locator('#office-notice').filter({ hasText: 'CADCraft reopened with plan.dxf.' }).waitFor({ timeout: 120_000 })
      await page.locator('#office-full-window').check()
      console.log(`PASS page ${path}: the apps stay within the device's memory budget (512 MB on a 2 GB device); the least recently used goes first, by itself when it reports no unsaved work, a kept app stays, and CADCraft reopens with its file`)
      continue
    }
    await page.locator('#status').filter({ hasText: /^Ready\./ }).waitFor({ timeout: 60_000 })
    if (path === '/flow') {
      assert.equal(await page.locator('.demo-switch a').count(), 2, `Demo switcher on ${path}`)
      assert.equal(await page.locator('.demo-switch a[aria-current=page]').getAttribute('href'), path, `Switcher marks ${path}`)
      assert.equal(await page.locator('.site-header nav a[aria-current=page]').first().textContent(), 'Demo', `Menu marks Demo on ${path}`)
    }
    assert.equal(await page.locator('iframe').count(), 0, `No iframe on ${path}`)
    console.log(`PASS demo page ${path}: engine ready under the strict policy, switcher, site layout`)
  }
  // Phones and tablets open their on-screen keyboard only for focus given during a tap. Each framed
  // editor's page loads craft-touch-keyboard.js for that: a double-tap on a GridCraft cell must focus
  // the hidden text field during the tap itself, and a tap that wants no text hands focus back.
  {
    const touch = await browser.newContext({ viewport: { width: 1024, height: 768 }, deviceScaleFactor: 1, hasTouch: true, isMobile: true })
    await touch.addInitScript(() => {
      window.__focusInTap = []
      document.addEventListener('touchend', () => window.__focusInTap.push(document.activeElement.tagName.toLowerCase()), { capture: true, passive: true })
    })
    const grid = await touch.newPage()
    await grid.goto(origin + '/apps/frame/gridcraft/')
  // /office adds the keyboard script to each app's page (the release's page is served unchanged); do the same here.
    await grid.addScriptTag({ url: origin + '/apps/frame/gridcraft/craft-touch-keyboard.js' })
    await grid.locator('canvas').first().waitFor({ timeout: 120_000 })
    await grid.waitForFunction(() => !!document.querySelector('input[type=text]'), null, { timeout: 120_000 })
    await grid.waitForTimeout(5000)
    const active = () => grid.evaluate(() => document.activeElement.tagName.toLowerCase())
    await grid.touchscreen.tap(330, 502); await grid.waitForTimeout(120); await grid.touchscreen.tap(330, 502); await grid.waitForTimeout(800)
    const inTap = await grid.evaluate(() => window.__focusInTap.at(-1))
    assert.equal(inTap, 'input', 'a double-tap on a cell focuses the text field during the tap')
    assert.equal(await active(), 'input', 'the cell keeps the text field focused for typing')
    await grid.keyboard.press('Escape'); await grid.touchscreen.tap(200, 107); await grid.waitForTimeout(800)
    assert.equal(await active(), 'canvas', 'a tap on the ribbon hands focus back to the app')
    console.log('PASS touch keyboard: GridCraft focuses its text field during the tap that starts editing')
    await touch.close()
  }
  // Apps that ship a service worker (EffectCraft, SolidCraft) get it working: the release is served
  // unchanged, so its worker finds every file it lists with the hash it expects, installs, and the app
  // then starts again with the network off.
  for (const app of ['effectcraft', 'solidcraft']) {
    const context = await browser.newContext({ viewport: { width: 1280, height: 800 } })
    const tab = await context.newPage()
    await tab.goto(`${origin}/apps/frame/${app}/`)
    await tab.locator('canvas').first().waitFor({ timeout: 180_000 })
    const state = await tab.evaluate(() => new Promise((resolve) => {
      const done = () => navigator.serviceWorker.getRegistration().then((r) => r?.active?.state === 'activated' && resolve('activated'))
      navigator.serviceWorker.ready.then(done)
      setTimeout(() => navigator.serviceWorker.getRegistration().then((r) => resolve(r?.installing?.state || r?.waiting?.state || r?.active?.state || 'none')), 170_000)
    }))
    assert.equal(state, 'activated', `${app}: its service worker installs and activates`)
    await context.setOffline(true)
    await tab.reload()
    await tab.waitForFunction(() => { const c = document.querySelector('canvas'); return c && c.width > 0 && c.height > 0 }, null, { timeout: 120_000 })
    console.log(`PASS ${app}: its service worker installs from the unchanged release, and the app starts again offline`)
    await context.close()
  }
  // The office desk must appear in the main navigation alongside the demos.
  await page.goto(origin + '/')
  assert.equal(await page.locator('.site-header .desktop-nav a[href="/office"]').count(), 1, 'The desktop menu links the office desk')
  assert.equal(await page.locator('.site-header .mobile-menu a[href="/office"]').count(), 1, 'The mobile menu links the office desk')
  await context.close()
  assert.deepEqual(failures, [], 'Browser console, page and asset errors')
  const pkg = JSON.parse(await readFile('package.json', 'utf8'))
  for (const [name, version] of Object.entries({ ...pkg.dependencies, ...pkg.devDependencies })) assert(/^\d+\.\d+\.\d+$/.test(version), `Unpinned ${name}`)
  const result = { routes: paths.length, screenshots: captures.length, failures, checks: ['static HTML without JS', 'internal paths and fragments', 'dataset tables: 14 open, 2 other, seven fields each', 'dataset mobile cards and licence boundaries', 'UNESCO disclaimer and shared pipeline link', 'security contact and security.txt preserved', 'no sideways scroll', 'no browser errors', 'theme persistence', 'keyboard skip link and focus', 'mobile menu navigation', 'revision stepping with arrow keys', 'real browser signature verification and tamper rejection', 'demo pages: engine ready under the strict policy, switcher, site layout', 'reduced motion', 'exact package pins'], captures }
  await writeFile('_shots/results.json', JSON.stringify(result, null, 2) + '\n')
  await writeFile('_shots/gallery.html', `<!doctype html><html lang="en"><meta charset="utf-8"><title>Milleleaf release captures</title><style>body{font:16px sans-serif;background:#e6eded;padding:24px}main{display:grid;grid-template-columns:repeat(4,minmax(0,1fr));gap:24px}img{width:100%;height:480px;object-fit:contain;object-position:top;background:#fff}a{color:#174536}figure{margin:0}figcaption{padding:12px 0}@media(max-width:800px){main{grid-template-columns:repeat(2,minmax(0,1fr))}}</style><h1>Milleleaf · final captures</h1><p>${paths.length} pages × 2 widths × 2 themes. Full-page images; open each for detail.</p><main>${captures.map(c => `<figure><a href="${c.file}"><img src="${c.file}" alt="${c.path}, ${c.width}px, ${c.theme}" loading="lazy"><figcaption>${c.path} · ${c.width}px · ${c.theme}</figcaption></a></figure>`).join('')}</main></html>`)
  console.log(`PASS: ${paths.length} static pages, ${captures.length} screenshots, interaction and integration checks; zero unexpected browser errors.`)
} finally { await browser.close(); await new Promise(done => server.close(done)) }
