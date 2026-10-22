// After `npm run build`: write _headers with a strict Content Security Policy for the two demo pages.
// The pages may run only this site's scripts (plus their own prerendered inline scripts, by hash) and the
// WebAssembly engine, and may not connect anywhere: dropped files cannot leave the browser.
// Cloudflare reads _headers; scripts/serve.mjs applies it locally for the checks.
import { createHash } from 'node:crypto'
import { readFile, writeFile } from 'node:fs/promises'

const out = 'dist/client'
const rules = []
for (const page of ['demo', 'flow']) {
  const html = await readFile(`${out}/${page}/index.html`, 'utf8')
  const inline = [...html.matchAll(/<script(?![^>]*\bsrc=)[^>]*>([\s\S]*?)<\/script>/g)].map(m => m[1])
  const hashes = inline.map(code => `'sha256-${createHash('sha256').update(code).digest('base64')}'`)
  const policy = [
    "default-src 'none'",
    `script-src 'self' 'wasm-unsafe-eval' ${hashes.join(' ')}`.trim(),
    // Inline styles cannot send data: images and fonts are limited to this site, and connect-src is none.
    "style-src 'self' 'unsafe-inline'",
    "font-src 'self'",
    "img-src 'self' blob: data:",
    "connect-src 'none'",
    "worker-src 'self'",
    "manifest-src 'self'",
    "object-src 'none'",
    "base-uri 'none'",
    "form-action 'none'",
    "frame-ancestors 'none'",
  ].join('; ')
  for (const path of [`/${page}`, `/${page}/`]) rules.push(`${path}\n  Content-Security-Policy: ${policy}\n  Referrer-Policy: no-referrer\n  X-Content-Type-Options: nosniff\n`)
  console.log(`CSP for /${page}: ${hashes.length} inline scripts allowed by hash`)
}
// Workers take their policy from their own response, not the page's: the engine scripts get the same limits.
rules.push(`/engine/*\n  Content-Security-Policy: default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; connect-src 'none'; worker-src 'none'\n  X-Content-Type-Options: nosniff\n`)
await writeFile(`${out}/_headers`, rules.join(''))
