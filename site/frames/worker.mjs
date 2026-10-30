// The /apps/frame/ Worker. Every file of an app's release is a static asset served as it is, except
// a wasm over the host's 25 MiB per-file limit: that one is staged as gzip parts
// (scripts/stage-releases.mjs lists them in wasm-parts.json), and this answers the wasm's own URL
// with the parts joined and labelled gzip, so the browser gets the release's own bytes from the
// release's own path. The page, its loader and its service worker see the release unchanged.
let manifest

export default {
  async fetch(request, env) {
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
        if (request.headers.get('If-None-Match') === headers.ETag) return new Response(null, { status: 304, headers })
        const parts = await Promise.all(entry.parts.map((part) => env.ASSETS.fetch(new URL(part, url))))
        const failed = parts.find((r) => !r.ok)
        if (failed) return new Response(`${failed.url}: ${failed.status}`, { status: 502 })
        if (request.method === 'HEAD') return new Response(null, { headers })
        const { readable, writable } = new TransformStream()
        ;(async () => {
          for (const part of parts) await part.body.pipeTo(writable, { preventClose: true })
          await writable.close()
        })().catch((error) => writable.abort(error))
        return new Response(readable, { headers, encodeBody: 'manual' })
      }
    }
    return env.ASSETS.fetch(request)
  },
}
