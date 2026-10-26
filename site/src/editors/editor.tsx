import { useEffect, useRef, useState } from 'react'

// One page-host editor component for the four Craft apps. Each app runs in the page's own
// document: the page renders the app's <canvas> (with data-host="page"), listens for the
// app's CustomEvents on that canvas, loads the wasm-bindgen glue and decompresses the
// gzipped wasm. The contract is identical for every app (APP = gridcraft, pdfcraft,
// wordcraft or cadcraft); PdfCraft's canvas id is "pdfcraft", the others "APP_canvas".

export type EditorManifest = {
  version: string
  commit: string
  pull: string
  repo: string
  glue: string
  wasm: string
  canvasId: string
  /** The glue exports start(canvasId), called after init. */
  start?: boolean
}

export type EditorsManifest = { [app: string]: EditorManifest }

export type CraftEditorProps = {
  app: string
  label: string
  height: number | string
  /** Fired on APP:save (detail: name, bytes, and saveAs for PdfCraft and WordCraft). */
  onSave?: (detail: { name: string; bytes: ArrayBuffer; saveAs?: boolean }) => void
  /** Fired on APP:dirty (detail: dirty). */
  onDirty?: (dirty: boolean) => void
  /** Fired on APP:ready and again if the same canvas ever re-announces itself. */
  onReady?: () => void
  /** Fired on APP:failed or when the glue/wasm fails to load. */
  onFailed?: (message: string) => void
}

// An app binds to one canvas forever and cannot start twice in a document, so every app gets
// exactly one canvas element per document, created lazily and kept in a module-level registry.
// Mounting moves that same element into the component's box; unmounting parks it off-screen.
// Navigation and re-renders can never orphan it or start a second instance.
const canvases = new Map<string, HTMLCanvasElement>()
const started = new Set<string>()

// Shared per app across the document: the resolved manifest and the load promise, so two
// components for the same app never fetch the glue twice or call init twice.
const manifests = new Map<string, EditorManifest>()
const loaders = new Map<string, Promise<'ready' | 'failed'>>()

async function fetchManifest(): Promise<EditorsManifest> {
  const response = await fetch('/apps/editors.json')
  if (!response.ok) throw new Error(`The editor list is unavailable (${response.status}).`)
  return response.json()
}

async function loadApp(app: string): Promise<'ready' | 'failed'> {
  let promise = loaders.get(app)
  if (promise) return promise
  promise = (async () => {
    const canvas = getCanvas(app)
    if (!manifests.has(app)) {
      try { manifests.set(app, (await fetchManifest())[app]) } catch (error) { return startFailed(app, error) }
    }
    const manifest = manifests.get(app)
    if (!manifest) return startFailed(app, new Error(`The editor list has no entry for ${app}.`))
    if (!started.has(app)) {
      started.add(app)
      try {
        // Import the glue as a script module, then hand init a Response that streams the
        // gzipped wasm through DecompressionStream as application/wasm.
        // The glue's default export is wasm-bindgen's init function.
        const glue = await import(/* webpackIgnore: true */ `/apps/${app}/${manifest.glue}`)
        const init = glue.default
        const wasmUrl = `/apps/${app}/${manifest.wasm}`
        await init({
          module_or_path: fetch(wasmUrl).then(async response => {
            if (!response.ok) throw new Error(`The ${app} app files are unavailable (${response.status}).`)
            // A server that labels the file Content-Encoding: gzip (a dev server does) has the
            // browser unzip it already; decompress only what still starts with the gzip signature.
            const file = await response.blob()
            const head = new Uint8Array(await file.slice(0, 2).arrayBuffer())
            const body = head[0] === 0x1f && head[1] === 0x8b ? file.stream().pipeThrough(new DecompressionStream('gzip')) : file.stream()
            return new Response(body, { headers: { 'Content-Type': 'application/wasm' } })
          }),
        })
        if (manifest.start) await glue.start(canvas.id)
      } catch (error) { return startFailed(app, error) }
    }
    return await listenForReady(app, canvas)
  })()
  loaders.set(app, promise)
  return promise
}

// ready arrives while init() is still running (the app announces itself during startup), so the
// listener must exist before the glue loads. It does, as long as every code path attaches it
// before first use: getCanvas() wires it at canvas creation time.
function listenForReady(app: string, canvas: HTMLCanvasElement): Promise<'ready' | 'failed'> {
  return new Promise(resolve => {
    if (readySeen(app)) return resolve('ready')
    const onReady = () => { cleanup(); resolve('ready') }
    const onFailed = (event: Event) => {
      const message = (event as CustomEvent).detail?.error || `${app} could not start.`
      startFailed(app, new Error(String(message)))
      cleanup()
      resolve('failed')
    }
    const cleanup = () => {
      canvas.removeEventListener(`${app}:ready`, onReady)
      canvas.removeEventListener(`${app}:failed`, onFailed)
    }
    canvas.addEventListener(`${app}:ready`, onReady, { once: true })
    canvas.addEventListener(`${app}:failed`, onFailed, { once: true })
    // A safety net only: the app always answers ready or failed. Never restarts the app.
    const timer = setTimeout(() => { cleanup(); resolve(readySeen(app) ? 'ready' : 'failed') }, 120_000)
    if (readySeen(app)) { clearTimeout(timer); cleanup(); resolve('ready') }
  })
}

const readiness = new Map<string, boolean>()
function readySeen(app: string) { return readiness.get(app) === true }
function markReady(app: string) { readiness.set(app, true) }

function startFailed(app: string, error: unknown) {
  readiness.set(app, false)
  console.error(`${app} failed to start:`, error)
  return 'failed' as const
}

function getCanvas(app: string): HTMLCanvasElement {
  let canvas = canvases.get(app)
  if (!canvas) {
    canvas = document.createElement('canvas')
    canvas.id = app === 'pdfcraft' ? 'pdfcraft' : `${app}_canvas`
    canvas.dataset.host = 'page'
    // Record ready/failed from the start: an app may announce itself before any component mounts.
    canvas.addEventListener(`${app}:ready`, () => markReady(app))
    canvas.addEventListener(`${app}:failed`, () => readiness.set(app, false))
    canvases.set(app, canvas)
  }
  return canvas
}

export function getEditorCanvas(app: string): HTMLCanvasElement | undefined { return canvases.get(app) }
export function isEditorReady(app: string): boolean { return readySeen(app) }
export async function startEditor(app: string): Promise<'ready' | 'failed'> { return loadApp(app) }

// Open a file once the app's component has put its canvas in the page and the app is ready.
// Use this right after asking for an editor to appear: the app can't start before its canvas is mounted.
export async function openWhenReady(app: string, name: string, bytes: ArrayBuffer) {
  for (let waited = 0; !canvases.get(app)?.isConnected; waited += 50) {
    if (waited > 10_000) throw new Error(`The ${app} editor did not appear on the page.`)
    await new Promise(done => setTimeout(done, 50))
  }
  if (await loadApp(app) !== 'ready') throw new Error(`The ${app} editor could not start.`)
  if (!openInEditor(app, name, bytes)) throw new Error(`The ${app} editor could not open ${name}.`)
}

// Open a file in an app that already reported ready. Call after startEditor resolves 'ready'.
export function openInEditor(app: string, name: string, bytes: ArrayBuffer) {
  const canvas = canvases.get(app)
  if (!canvas || !readySeen(app)) return false
  canvas.dispatchEvent(new CustomEvent(`${app}:open`, { detail: { name, bytes: bytes.slice(0) } }))
  return true
}

// An editor that runs from its own official release page in a same-origin frame. It looks and
// behaves like CraftEditor (Expand, loading and failure states); it is ready once the app has
// drawn its canvas. It does not take files from the page or report saves: its own File menu
// opens files and downloads what it saves.
export function FramedEditor({ app, label, src, height }: { app: string; label: string; src: string; height: number | string }) {
  const frame = useRef<HTMLIFrameElement>(null)
  const [state, setState] = useState<'loading' | 'ready' | 'failed'>('loading')
  const [expanded, setExpanded] = useState(false)

  const [slow, setSlow] = useState(false)
  const [attempt, setAttempt] = useState(0)

  // Ready once the frame's page has loaded (the app shows its own start screen from then on) or
  // has drawn a canvas, whichever comes first. After 20 s the overlay offers a reload.
  useEffect(() => {
    setState('loading'); setSlow(false)
    const element = frame.current
    if (!element) return
    const ready = () => setState('ready')
    element.addEventListener('load', ready)
    let stopped = false
    const poll = () => {
      if (stopped) return
      let drawn = false
      try { const doc = element.contentDocument; drawn = !!doc && doc.URL !== 'about:blank' && (doc.readyState === 'complete' || !!doc.querySelector('canvas')) } catch { /* Not reachable yet. */ }
      if (drawn) { ready(); return }
      setTimeout(poll, 250)
    }
    poll()
    const timer = setTimeout(() => setSlow(true), 20_000)
    return () => { stopped = true; clearTimeout(timer); element.removeEventListener('load', ready) }
  }, [src, attempt])

  useEffect(() => {
    if (!expanded) return
    const root = document.documentElement
    root.classList.add('craft-editor-open')
    frame.current?.focus()
    return () => root.classList.remove('craft-editor-open')
  }, [expanded])

  return <div className={`craft-editor-shell${expanded ? ' expanded' : ''}`} role={expanded ? 'dialog' : undefined} aria-label={expanded ? `${label}, full window` : undefined}>
    <div className="craft-editor-bar">
      <span>{expanded ? label : ''}</span>
      <button type="button" className="button secondary small" aria-pressed={expanded} onClick={() => setExpanded(value => !value)}>
        {expanded ? 'Collapse' : 'Expand'}
      </button>
    </div>
    <div className="craft-editor" style={expanded ? undefined : { height: typeof height === 'number' ? `${height}px` : height }} aria-label={`${label} editor`}>
      <iframe key={attempt} ref={frame} src={src} title={`${label} editor`} data-app={app} />
      {state === 'loading' && <div className="craft-editor-state" role="status"><p><strong>Opening {label}…</strong></p><p className="caption">The first visit takes a moment. The editor runs on this device.</p>
        {slow && <button type="button" className="button secondary small craft-editor-retry" onClick={() => setAttempt(n => n + 1)}>Reload {label}</button>}</div>}
      {state === 'failed' && <div className="craft-editor-state" role="alert"><p><strong>{label} could not load.</strong></p><p className="caption">The editor is unavailable in this browser.</p></div>}
    </div>
  </div>
}

export function CraftEditor({ app, label, height, onSave, onDirty, onReady, onFailed }: CraftEditorProps) {
  const box = useRef<HTMLDivElement>(null)
  const [state, setState] = useState<'loading' | 'ready' | 'failed'>('loading')
  const [error, setError] = useState('')
  const [expanded, setExpanded] = useState(false)
  const callbacks = useRef({ onSave, onDirty, onReady, onFailed })
  callbacks.current = { onSave, onDirty, onReady, onFailed }

  useEffect(() => {
    const canvas = getCanvas(app)
    const element = box.current
    if (!element) return

    // The listeners exist before any load of this app: they are attached to the registry
    // canvas the moment a component for it mounts, ahead of the glue import.
    const onSaveEvent = (event: Event) => callbacks.current.onSave?.((event as CustomEvent).detail)
    const onDirtyEvent = (event: Event) => callbacks.current.onDirty?.((event as CustomEvent).detail?.dirty === true)
    const onReadyEvent = () => { markReady(app); setState('ready'); callbacks.current.onReady?.() }
    const onFailedEvent = (event: Event) => {
      const message = String((event as CustomEvent).detail?.error || `${label} could not start.`)
      readiness.set(app, false)
      setState('failed'); setError(message); callbacks.current.onFailed?.(message)
    }
    canvas.addEventListener(`${app}:save`, onSaveEvent)
    canvas.addEventListener(`${app}:dirty`, onDirtyEvent)
    canvas.addEventListener(`${app}:ready`, onReadyEvent)
    canvas.addEventListener(`${app}:failed`, onFailedEvent)

    // Move the one registry canvas into this box. If it already sits in another box, take it out.
    if (canvas.parentNode && canvas.parentNode !== element) canvas.parentNode.removeChild(canvas)
    element.appendChild(canvas)
    if (readySeen(app)) setState('ready')

    void loadApp(app).then(result => {
      if (result === 'ready' && !readySeen(app)) { markReady(app); setState('ready'); callbacks.current.onReady?.() }
      if (result === 'failed' && !readiness.get(app)) { setState('failed'); setError(`${label} could not be loaded. The app files may be missing.`) }
    })

    return () => {
      for (const [name, handler] of [
        [`${app}:save`, onSaveEvent], [`${app}:dirty`, onDirtyEvent],
        [`${app}:ready`, onReadyEvent], [`${app}:failed`, onFailedEvent],
      ] as const) canvas.removeEventListener(name, handler)
      // Park the canvas off-screen instead of destroying it: the app inside it stays alive.
      if (canvas.parentNode === element) element.removeChild(canvas)
    }
  }, [app, label])

  // Expanded, the editor fills the browser window. Only the button collapses it: the apps use Escape themselves.
  useEffect(() => {
    if (!expanded) return
    const root = document.documentElement
    root.classList.add('craft-editor-open')
    box.current?.querySelector('canvas')?.focus()
    return () => root.classList.remove('craft-editor-open')
  }, [expanded])

  return <div className={`craft-editor-shell${expanded ? ' expanded' : ''}`} role={expanded ? 'dialog' : undefined} aria-label={expanded ? `${label}, full window` : undefined}>
    <div className="craft-editor-bar">
      <span>{expanded ? label : ''}</span>
      <button type="button" className="button secondary small" aria-pressed={expanded} onClick={() => setExpanded(value => !value)}>
        {expanded ? 'Collapse' : 'Expand'}
      </button>
    </div>
    <div className="craft-editor" style={expanded ? undefined : { height: typeof height === 'number' ? `${height}px` : height }} ref={box} aria-label={`${label} editor`}>
      {state === 'loading' && <div className="craft-editor-state" role="status"><p><strong>Opening {label.toLowerCase()}…</strong></p><p className="caption">The first visit takes a moment. The editor runs on this device.</p></div>}
      {state === 'failed' && <div className="craft-editor-state" role="alert"><p><strong>{label} could not load.</strong></p><p className="caption">{error || 'The editor is unavailable in this browser.'}</p><p className="caption">The {label.toLowerCase()} preview below stays available.</p></div>}
    </div>
  </div>
}
