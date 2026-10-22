import assert from 'node:assert/strict'
import { mkdir, writeFile, readFile } from 'node:fs/promises'
import { chromium } from '@playwright/test'
import { serve } from './serve.mjs'

const paths = ['/', '/demo', '/how-it-works', '/data', '/data/round-trip', '/data/worldviews', '/data/datasets', '/method', '/docs', '/docs/getting-started', '/status', '/about']
const headings = ['See the save.Keep the source.', 'Your file.Its next leaf.', 'A file leaves. Its changes come back.', 'Follow the data to its source.', 'What survives the save?', 'Real places. Sources in view.', 'Datasets and their terms.', 'Make the claim the evidence supports.', 'Start with a file you can check.', 'Try it. Inspect what comes back.', 'Built. Checked. Still to come.', 'Useful tools. Evidence in view.']
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
  const evidence = JSON.parse(await readFile('src/data/hero.json', 'utf8'))
  assert.equal(evidence.signed.exitCode, 0)
  assert(evidence.signed.verification.includes('signed by fixture-reviewer'))
  assert(evidence.frames[1].report.changed.length > 0 && evidence.frames[1].report.conflicts.length > 0)
  assert.equal(evidence.layerBytes.includes('site/hero/resaved'), true)
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
      // The only expected missing resource is the separately built demo.
      if (message.type() === 'error' && !message.location().url.endsWith('/demo/app/index.html')) failures.push(message.text())
    })
    page.on('response', response => {
      if (response.status() >= 400 && !response.url().endsWith('/demo/app/index.html')) failures.push(`${response.status()} ${response.url()}`)
    })
    for (const path of paths) {
      assert.equal((await page.goto(origin + path)).status(), 200)
      await page.evaluate(() => document.fonts.ready)
      await page.waitForFunction(() => !!document.querySelector('ml-theme')?.shadowRoot)
      assert.equal(await page.locator('h1').count(), 1)
      assert.equal(await page.locator('h1').textContent(), headings[paths.indexOf(path)], `Hydrated page identity ${path}`)
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, `Overflow ${path} ${width} ${theme}`)
      assert.equal(await page.evaluate(() => getComputedStyle(document.documentElement).scrollBehavior), 'auto')
      assert.equal(await page.evaluate(() => document.documentElement.dataset.theme), theme)
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
  await page.goto(origin + '/demo')
  assert(await page.getByText('Coming with the first release', { exact: true }).isVisible())
  assert.equal(await page.locator('ml-demo iframe').count(), 0)
  await page.route('**/demo/app/index.html', route => route.fulfill({ contentType: 'text/html', body: '<!doctype html><html><body><h1>Demo integration fixture</h1></body></html>' }))
  await page.reload()
  await page.waitForSelector('ml-demo iframe')
  assert(await page.frameLocator('ml-demo iframe').getByText('Demo integration fixture').isVisible())
  assert.equal(await page.locator('.demo-placeholder').isVisible(), false)
  await page.unroute('**/demo/app/index.html')
  await page.route('**/demo/app/index.html', route => route.fulfill({ contentType: 'text/html', body: '<html><head><meta name="milleleaf-site" content="website"></head><body>Fallback site</body></html>' }))
  await page.reload()
  await page.waitForFunction(() => !!document.querySelector('ml-theme')?.shadowRoot)
  assert(await page.getByText('Coming with the first release', { exact: true }).isVisible())
  assert.equal(await page.locator('ml-demo iframe').count(), 0, 'Never recursively embed host fallback')
  await context.close()
  assert.deepEqual(failures, [], 'Browser console, page and asset errors')
  const pkg = JSON.parse(await readFile('package.json', 'utf8'))
  for (const [name, version] of Object.entries({ ...pkg.dependencies, ...pkg.devDependencies })) assert(/^\d+\.\d+\.\d+$/.test(version), `Unpinned ${name}`)
  const result = { routes: paths.length, screenshots: captures.length, failures, checks: ['static HTML without JS', 'internal paths and fragments', 'dataset tables: 14 open, 2 other, seven fields each', 'dataset mobile cards and licence boundaries', 'UNESCO disclaimer and shared pipeline link', 'security contact and security.txt preserved', 'no sideways scroll', 'no browser errors', 'theme persistence', 'keyboard skip link and focus', 'mobile menu navigation', 'revision stepping with arrow keys', 'real browser signature verification and tamper rejection', 'demo absent/present/host-fallback states', 'reduced motion', 'exact package pins'], captures }
  await writeFile('_shots/results.json', JSON.stringify(result, null, 2) + '\n')
  await writeFile('_shots/gallery.html', `<!doctype html><html lang="en"><meta charset="utf-8"><title>Milleleaf release captures</title><style>body{font:16px sans-serif;background:#e6eded;padding:24px}main{display:grid;grid-template-columns:repeat(4,minmax(0,1fr));gap:24px}img{width:100%;height:480px;object-fit:contain;object-position:top;background:#fff}a{color:#174536}figure{margin:0}figcaption{padding:12px 0}@media(max-width:800px){main{grid-template-columns:repeat(2,minmax(0,1fr))}}</style><h1>Milleleaf · final captures</h1><p>12 pages × 2 widths × 2 themes. Full-page images; open each for detail.</p><main>${captures.map(c => `<figure><a href="${c.file}"><img src="${c.file}" alt="${c.path}, ${c.width}px, ${c.theme}" loading="lazy"><figcaption>${c.path} · ${c.width}px · ${c.theme}</figcaption></a></figure>`).join('')}</main></html>`)
  console.log('PASS: 12 static pages, 48 screenshots, interaction and integration checks; zero unexpected browser errors.')
} finally { await browser.close(); await new Promise(done => server.close(done)) }
