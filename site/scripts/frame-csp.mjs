// The _headers rule for one framed editor (an official release page staged under /apps/frame/APP/).
// The page starts the app with an inline module script (allowed by hash), may run workers, play media
// from blob: URLs and read blob: files, and connects only back to this site. One rule per app, never a
// catch-all: the host joins two matching rules' policies with a comma, and the browser then enforces both.
import { createHash } from 'node:crypto'

export function frameRule(app, html) {
  const inline = [...html.matchAll(/<script(?![^>]*\bsrc=)[^>]*>([\s\S]*?)<\/script>/g)].map(m => m[1])
  const hashes = inline.map(code => `'sha256-${createHash('sha256').update(code).digest('base64')}'`).join(' ')
  return `/apps/frame/${app}/*\n  Content-Security-Policy: default-src 'none'; script-src 'self' 'wasm-unsafe-eval' ${hashes}; style-src 'self' 'unsafe-inline'; connect-src 'self' blob: data:; img-src 'self' blob: data:; media-src 'self' blob:; font-src 'self' data:; worker-src 'self' blob:; object-src 'none'; base-uri 'self'; form-action 'none'; frame-ancestors 'self'\n  X-Content-Type-Options: nosniff\n`
}
