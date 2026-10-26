// After `npm run build`: write _headers with a strict Content Security Policy for the demo pages,
// the office desk and the editor apps.
// The pages may run only this site's scripts (plus their own prerendered inline scripts, by hash)
// and the WebAssembly engine. /demo stays fully offline: dropped files cannot leave the browser
// (connect-src 'none'). /flow and /office host the Craft editors in the page, and the editors
// fetch their gzipped WebAssembly from this site, so those two pages allow connect-src 'self'
// only — still no third-party origin of any kind. /apps/* serves the editors' own static files:
// they run scripts from this site and compile wasm, and may only connect back to this site.
// Cloudflare reads _headers; scripts/serve.mjs applies it locally for the checks.
import { createHash } from 'node:crypto'
import { readFile, writeFile } from 'node:fs/promises'

const out = 'dist/client'
const rules = []
for (const page of ['demo', 'flow', 'office']) {
  const html = await readFile(`${out}/${page}/index.html`, 'utf8')
  const inline = [...html.matchAll(/<script(?![^>]*\bsrc=)[^>]*>([\s\S]*?)<\/script>/g)].map(m => m[1])
  const hashes = inline.map(code => `'sha256-${createHash('sha256').update(code).digest('base64')}'`)
  // The editors load their glue as a script module and fetch their wasm from /apps/<app>/,
  // so /flow and /office allow same-origin connections; /demo keeps every connection off.
  const connectSrc = page === 'demo' ? "connect-src 'none'" : "connect-src 'self'"
  const policy = [
    "default-src 'none'",
    `script-src 'self' 'wasm-unsafe-eval' ${hashes.join(' ')}`.trim(),
    // Inline styles cannot send data: images and fonts are limited to this site.
    "style-src 'self' 'unsafe-inline'",
    "font-src 'self'",
    "img-src 'self' blob: data:",
    connectSrc,
    "worker-src 'self'",
    "manifest-src 'self'",
    "object-src 'none'",
    "base-uri 'none'",
    "form-action 'none'",
    "frame-ancestors 'none'",
  ].join('; ')
  for (const path of [`/${page}`, `/${page}/`]) rules.push(`${path}\n  Content-Security-Policy: ${policy}\n  Referrer-Policy: no-referrer\n  X-Content-Type-Options: nosniff\n`)
  console.log(`CSP for /${page}: ${hashes.length} inline scripts allowed by hash, ${connectSrc}`)
}
// The editor apps are static files on this site: they run their own scripts, compile WebAssembly
// and fetch nothing but this site's own files (their wasm comes through the hosting page).
rules.push(`/apps/*\n  Content-Security-Policy: default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'unsafe-inline'; connect-src 'self'; img-src 'self' blob: data:; font-src 'self'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'self'\n  X-Content-Type-Options: nosniff\n`)
// Workers take their policy from their own response, not the page's: the engine scripts get the same limits.
rules.push(`/engine/*\n  Content-Security-Policy: default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; connect-src 'none'; worker-src 'none'\n  X-Content-Type-Options: nosniff\n`)
await writeFile(`${out}/_headers`, rules.join(''))
