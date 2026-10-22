// After `npm run build`, on the box: publish the browser demos as site pages at /demo/ and /flow/
// (no iframe). Each keeps its own scripts and engine; the site's prerendered header and footer and its
// stylesheet are put in. Build the demos first with `bash web/flow/build.sh` (it builds web/demo too).
import { cp, readFile, writeFile, access } from 'node:fs/promises'

const out = 'dist/client'
const site = await readFile(`${out}/index.html`, 'utf8')
const take = (re, what) => { const m = site.match(re); if (!m) throw new Error(`Site ${what} not found`); return m[0] }
// The theme switch needs the site's own script, which the demo pages don't load.
const header = take(/<header class="site-header[\s\S]*?<\/header>/, 'header')
  .replace(/<div class="header-tools">[\s\S]*?<\/div>/, '')
  .replace(/ aria-current="page"/g, '')
const footer = take(/<footer class="site-footer[\s\S]*?<\/footer>/, 'footer')

const apps = {
  demo: ['index.html', 'demo.css', 'demo.js', 'worker.js', 'README.md', 'pkg'],
  flow: ['index.html', 'flow.css', 'flow.js', 'worker.js', 'README.md', 'pkg'],
}
for (const [name, files] of Object.entries(apps)) {
  await access(`../web/${name}/pkg/ox_web_bg.wasm`).catch(() => { throw new Error(`web/${name}/pkg is not built: run bash web/flow/build.sh first`) })
  for (const file of files) await cp(`../web/${name}/${file}`, `${out}/${name}/${file}`, { recursive: true })
  const page = `${out}/${name}/index.html`
  let html = await readFile(page, 'utf8')
  const before = html
  html = html
    .replace(/<link rel="stylesheet"/, '<link rel="stylesheet" href="/styles.css"><link rel="stylesheet"')
    .replace(/<header class="masthead">[\s\S]*?<\/header>/, header.replace(/(href="\/demo\/?")/, name === 'demo' ? '$1 aria-current="page"' : '$1'))
    .replace(/<footer[ >][\s\S]*?<\/footer>/, footer)
  // Demo B has no dark design yet; pin the site's light theme so the site's dark colours don't reach it.
  if (name === 'flow') html = html.replace('<html lang="en">', '<html lang="en" data-theme="light">')
  if (html === before || !html.includes('site-header') || !html.includes('site-footer')) throw new Error(`${page}: header or footer not replaced`)
  await writeFile(page, html)
}
// Demo A's header rules, scoped to its folder (Cloudflare Pages reads _headers; other hosts use the meta CSP).
const rules = (await readFile('../web/demo/_headers', 'utf8')).replace(/^\/\*$/m, '/demo/*')
if (!rules.includes('/demo/*')) throw new Error('web/demo/_headers changed shape')
await writeFile(`${out}/_headers`, rules)
console.log('Demos published at /demo/ and /flow/ with the site header and footer; _headers written')
