// The /apps/frame/ Worker. Every file of an app's release is a static asset served as it is, except
// a wasm over the host's 25 MiB per-file limit: that one is staged as gzip parts
// (scripts/stage-releases.mjs lists them in wasm-parts.json), and this answers the wasm's own URL
// with the parts joined and labelled gzip, so the browser gets the release's own bytes from the
// release's own path. The page, its loader and its service worker see the release unchanged.
let manifest

export default {
  async fetch(request, env, ctx) {
    const url = new URL(request.url)
    if (url.pathname.endsWith('.wasm') && (request.method === 'GET' || request.method === 'HEAD')) {
      manifest ??= await env.ASSETS.fetch(new URL('/apps/frame/wasm-parts.json', url)).then((r) => (r.ok ? r.json() : {}))
      const entry = manifest[url.pathname]
      if (entry) {
        const headers = {
          'Content-Type': 'application/wasm',
          'Content-Encoding': 'gzip',
          'Cache-Control': 'public, max-age=0, must-revalidate',
          ETag: `"${entry.sha256}"`,
          'X-Content-Type-Options': 'nosniff',
        }
        // Cloudflare's edge weakens the tag it passes on (W/"…"), and browsers send that form back:
        // compare without the W/ (weak comparison, RFC 9110 §13.1.2), in a list or alone.
        const tags = (request.headers.get('If-None-Match') || '').split(',').map((tag) => tag.trim().replace(/^W\//, ''))
        if (tags.includes(headers.ETag) || tags.includes('*')) return new Response(null, { status: 304, headers })
        const parts = await Promise.all(entry.parts.map((part) => env.ASSETS.fetch(new URL(part, url))))
        const failed = parts.find((r) => !r.ok)
        if (failed) return new Response(`${failed.url}: ${failed.status}`, { status: 502 })
        // The bytes move inside the runtime, never through JavaScript: a body copied by script costs CPU
        // time for every chunk, and production cut such responses off after about 0.6 MB.
        if (request.method === 'HEAD') return new Response(null, { headers })
        if (parts.length === 1) return new Response(parts[0].body, { headers, encodeBody: 'manual' })
        // Several parts: joined in a native stream, of the parts' total length when the host gives it.
        const sizes = parts.map((part) => Number(part.headers.get('Content-Length')))
        const { readable, writable } = sizes.every((size) => size > 0) ? new FixedLengthStream(sizes.reduce((a, b) => a + b, 0)) : new IdentityTransformStream()
        ctx.waitUntil((async () => {
          for (const part of parts) await part.body.pipeTo(writable, { preventClose: true })
          await writable.close()
        })().catch((error) => writable.abort(error)))
        return new Response(readable, { headers, encodeBody: 'manual' })
      }
    }
    return env.ASSETS.fetch(request)
  },
}
