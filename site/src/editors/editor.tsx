import { useCallback, useEffect, useRef, useState, type RefObject } from 'react'

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
  /** Each new value opens the editor in full window (0 or unset: inline). */
  expandRequest?: number
  /** Fired on APP:save (detail: name, bytes, and saveAs for PdfCraft and WordCraft). */
  onSave?: (detail: { name: string; bytes: ArrayBuffer; saveAs?: boolean }) => void
  /** Fired on APP:dirty (detail: dirty). */
  onDirty?: (dirty: boolean) => void
  /** Fired on APP:ready and again if the same canvas ever re-announces itself. */
  onReady?: () => void
  /** Fired on APP:failed or when the glue/wasm fails to load. */
  onFailed?: (message: string) => void
  /** Fired on APP:handoff: the app passes the page a file another app edits. */
  onHandoff?: (detail: Handoff) => void
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

// A file an app hands to the page because another app edits it (the apps' `handoff` message).
export type Handoff = { name: string; file?: Blob; bytes?: ArrayBuffer; size?: number }

// Framed editors that speak the host messages: their frames, and those that said they are ready.
const frames = new Map<string, HTMLIFrameElement>()
// A framed app's WebAssembly memory, kept when its start-up script announces the module
// (TrunkApplicationStarted; apps that start another way aren't seen), and its size then.
const wasmMemory = new Map<string, { memory: WebAssembly.Memory; startMiB: number }>()
// How far a framed app's WebAssembly memory has grown since it started, in MiB: what its documents
// add to its measured weight. 0 when the app isn't seen.
export function wasmGrowthMiB(app: string): number {
  const seen = wasmMemory.get(app)
  return seen ? Math.max(0, seen.memory.buffer.byteLength / 1048576 - seen.startMiB) : 0
}
const framesReady = new Set<string>()

// Send one host message to an app, whichever way it runs: an event on its canvas in this page, or a
// postMessage to its frame on this page's own origin.
export function sendToApp(app: string, type: string, fields: Record<string, unknown>) {
  const canvas = canvases.get(app)
  if (canvas?.isConnected) { canvas.dispatchEvent(new CustomEvent(`${app}:${type}`, { detail: fields })); return true }
  const frame = frames.get(app)
  if (frame?.contentWindow) { frame.contentWindow.postMessage({ type: `${app}:${type}`, ...fields }, location.origin); return true }
  return false
}

// Open a file in a framed editor once it has said it is ready (its `ready` host message).
export async function openFramedWhenReady(app: string, name: string, bytes: ArrayBuffer) {
  for (let waited = 0; !framesReady.has(app); waited += 100) {
    if (waited > 90_000) throw new Error(`The ${app} editor did not get ready for a file.`)
    await new Promise(done => setTimeout(done, 100))
  }
  if (!sendToApp(app, 'open', { name, bytes: bytes.slice(0) })) throw new Error(`The ${app} editor could not open ${name}.`)
}

// Expanded, an editor fills the browser window edge to edge; a small floating button or Escape
// twice in quick succession leaves. One Escape still belongs to the app, which uses it itself.
// The listener goes on the page and, for a framed editor, on the frame's own window, since keys
// typed into the frame never reach the page.
function useLeaveOnDoubleEscape(active: boolean, leave: () => void, frame?: RefObject<HTMLIFrameElement | null>) {
  useEffect(() => {
    if (!active) return
    let last = 0
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || event.repeat) return
      const now = Date.now()
      if (now - last < 500) { last = 0; leave() } else last = now
    }
    const targets = [window, frame?.current?.contentWindow].filter((target): target is Window => !!target)
    for (const target of targets) target.addEventListener('keydown', onKey, true)
    return () => { for (const target of targets) target.removeEventListener('keydown', onKey, true) }
  }, [active, leave, frame])
}

function ExpandControls({ label, expanded, setExpanded }: { label: string; expanded: boolean; setExpanded: (next: boolean) => void }) {
  return expanded
    ? <button type="button" className="craft-editor-exit" aria-pressed="true" onClick={() => setExpanded(false)} title="Exit full window (Esc twice)">Exit full window</button>
    : <div className="craft-editor-bar"><span /><button type="button" className="button secondary small" aria-pressed="false" onClick={() => setExpanded(true)}>Expand</button></div>
}

// An editor that runs from its own official release page in a same-origin frame. It looks and
// behaves like CraftEditor (Expand, loading and failure states); it is ready once the app has
// drawn its canvas. It does not take files from the page or report saves: its own File menu
// opens files and downloads what it saves.
export function FramedEditor({ app, label, src, height, expandRequest, hostBridge, onSave, onDirty, onHandoff }: {
  app: string; label: string; src: string; height: number | string; expandRequest?: number
  /** The release speaks the host messages: framed with ?host=parent, its saves come to the page. */
  hostBridge?: boolean
  onSave?: (detail: { name: string; bytes: ArrayBuffer }) => void
  onDirty?: (dirty: boolean) => void
  onHandoff?: (detail: Handoff) => void
}) {
  const frame = useRef<HTMLIFrameElement>(null)
  const callbacks = useRef({ onSave, onDirty, onHandoff })
  callbacks.current = { onSave, onDirty, onHandoff }
  const frameSrc = hostBridge ? `${src}${src.includes('?') ? '&' : '?'}host=parent` : src

  const [state, setState] = useState<'loading' | 'ready' | 'failed'>('loading')
  const [expanded, setExpanded] = useState(false)

  const [slow, setSlow] = useState(false)
  const [attempt, setAttempt] = useState(0)

  // The app's host messages come from its frame. Any framed app may hand a file off; only one framed
  // with ?host=parent sends ready, save and dirty, and is told the page takes hand-offs.
  useEffect(() => {
    const element = frame.current
    if (!element) return
    frames.set(app, element)
    framesReady.delete(app)
    const onMessage = (event: MessageEvent) => {
      if (event.source !== element.contentWindow || event.origin !== location.origin) return
      const data = event.data
      if (!data || typeof data.type !== 'string' || !data.type.startsWith(`${app}:`)) return
      const kind = data.type.slice(app.length + 1)
      if (kind === 'handoff') callbacks.current.onHandoff?.(data)
      if (!hostBridge) return
      if (kind === 'ready') { framesReady.add(app); sendToApp(app, 'host', { version: '1', handoff: true }) }
      if (kind === 'save' && data.bytes instanceof ArrayBuffer) callbacks.current.onSave?.({ name: String(data.name || 'drawing'), bytes: data.bytes })
      if (kind === 'dirty') callbacks.current.onDirty?.(data.dirty === true)
    }
    addEventListener('message', onMessage)
    return () => { removeEventListener('message', onMessage); framesReady.delete(app); wasmMemory.delete(app); if (frames.get(app) === element) frames.delete(app) }
  }, [app, hostBridge, attempt])

  // Ready once the app has started: its canvas has a size and its own loading notice is gone. An
  // HTML load alone is not a start (the WebAssembly can still fail). An uncaught error or rejected
  // promise in the frame before that, or no start within a minute, is a failure with a Reload.
  // After 20 s the loading overlay already offers a reload.
  const [failure, setFailure] = useState('')
  useEffect(() => {
    setState('loading'); setSlow(false); setFailure('')
    const element = frame.current
    if (!element) return
    let stopped = false, watched: Window | null = null
    const fail = (message: string) => { if (stopped) return; stopped = true; setFailure(message); setState('failed') }
    const onError = (event: ErrorEvent) => fail(event.message || 'The editor stopped while starting.')
    const onRejection = (event: PromiseRejectionEvent) => fail(String(event.reason?.message || event.reason || 'The editor stopped while starting.'))
    const poll = () => {
      if (stopped) return
      try {
        const win = element.contentWindow, doc = element.contentDocument
        if (win && doc && doc.URL !== 'about:blank' && win !== watched) {
          watched?.removeEventListener('error', onError); watched?.removeEventListener('unhandledrejection', onRejection)
          watched = win; win.addEventListener('error', onError); win.addEventListener('unhandledrejection', onRejection)
          wasmMemory.delete(app)
          win.addEventListener('TrunkApplicationStarted', (event: Event) => {
            const memory = (event as CustomEvent<{ wasm?: { memory?: WebAssembly.Memory } }>).detail?.wasm?.memory
            if (memory) wasmMemory.set(app, { memory, startMiB: memory.buffer.byteLength / 1048576 })
          })
        }
        // Phones and tablets open their keyboard for the app through craft-touch-keyboard.js, staged
        // beside the release. It goes in from here, so the release's own page stays unchanged (its
        // service worker checks that page against the release).
        if (doc && doc.URL !== 'about:blank' && (doc.head || doc.body) && !doc.getElementById('craft-touch-keyboard')) {
          const script = doc.createElement('script')
          script.id = 'craft-touch-keyboard'
          script.src = new URL('craft-touch-keyboard.js', doc.URL).href
          ;(doc.head || doc.body)!.appendChild(script)
        }
        const canvas = doc?.querySelector('canvas'), notice = doc?.querySelector(`#${app}_loading`)
        if (canvas && canvas.width > 0 && canvas.height > 0 && !(notice && (notice as HTMLElement).offsetParent)) { stopped = true; setState('ready'); return }
      } catch { /* Not reachable yet. */ }
      setTimeout(poll, 250)
    }
    poll()
    const slowTimer = setTimeout(() => setSlow(true), 20_000)
    const failTimer = setTimeout(() => fail(`${label} did not start within a minute.`), 60_000)
    return () => { stopped = true; clearTimeout(slowTimer); clearTimeout(failTimer); watched?.removeEventListener('error', onError); watched?.removeEventListener('unhandledrejection', onRejection) }
  }, [src, attempt, app, label])

  useEffect(() => {
    if (!expanded) return
    const root = document.documentElement
    root.classList.add('craft-editor-open')
    frame.current?.focus()
    return () => root.classList.remove('craft-editor-open')
  }, [expanded])
  const collapse = useCallback(() => setExpanded(false), [])
  useLeaveOnDoubleEscape(expanded, collapse, frame)
  useEffect(() => { if (expandRequest) setExpanded(true) }, [expandRequest])

  return <div className={`craft-editor-shell${expanded ? ' expanded' : ''}`} role={expanded ? 'dialog' : undefined} aria-label={expanded ? `${label}, full window` : undefined}>
    <ExpandControls label={label} expanded={expanded} setExpanded={setExpanded} />
    <div className="craft-editor" style={expanded ? undefined : { height: typeof height === 'number' ? `${height}px` : height }} aria-label={`${label} editor`}>
      <iframe key={attempt} ref={frame} src={frameSrc} title={`${label} editor`} data-app={app} />
      {state === 'loading' && <div className="craft-editor-state" role="status"><p><strong>Opening {label}…</strong></p><p className="caption">The first visit takes a moment. The editor runs on this device.</p>
        {slow && <button type="button" className="button secondary small craft-editor-retry" onClick={() => setAttempt(n => n + 1)}>Reload {label}</button>}</div>}
      {state === 'failed' && <div className="craft-editor-state" role="alert"><p><strong>{label} could not start.</strong></p><p className="caption">{failure || 'The editor is unavailable in this browser.'}</p>
        <button type="button" className="button secondary small craft-editor-retry" onClick={() => setAttempt(n => n + 1)}>Reload {label}</button></div>}
    </div>
  </div>
}

export function CraftEditor({ app, label, height, expandRequest, onSave, onDirty, onReady, onFailed, onHandoff }: CraftEditorProps) {
  const box = useRef<HTMLDivElement>(null)
  const [state, setState] = useState<'loading' | 'ready' | 'failed'>('loading')
  const [error, setError] = useState('')
  const [expanded, setExpanded] = useState(false)
  const callbacks = useRef({ onSave, onDirty, onReady, onFailed, onHandoff })
  callbacks.current = { onSave, onDirty, onReady, onFailed, onHandoff }

  useEffect(() => {
    const canvas = getCanvas(app)
    const element = box.current
    if (!element) return

    // The listeners exist before any load of this app: they are attached to the registry
    // canvas the moment a component for it mounts, ahead of the glue import.
    const onSaveEvent = (event: Event) => callbacks.current.onSave?.((event as CustomEvent).detail)
    const onDirtyEvent = (event: Event) => callbacks.current.onDirty?.((event as CustomEvent).detail?.dirty === true)
    // Once ready, the app is told this page takes hand-offs.
    const onReadyEvent = () => { markReady(app); setState('ready'); sendToApp(app, 'host', { version: '1', handoff: true }); callbacks.current.onReady?.() }
    const onHandoffEvent = (event: Event) => callbacks.current.onHandoff?.((event as CustomEvent).detail)
    const onFailedEvent = (event: Event) => {
      const message = String((event as CustomEvent).detail?.error || `${label} could not start.`)
      readiness.set(app, false)
      setState('failed'); setError(message); callbacks.current.onFailed?.(message)
    }
    canvas.addEventListener(`${app}:save`, onSaveEvent)
    canvas.addEventListener(`${app}:dirty`, onDirtyEvent)
    canvas.addEventListener(`${app}:ready`, onReadyEvent)
    canvas.addEventListener(`${app}:failed`, onFailedEvent)
    canvas.addEventListener(`${app}:handoff`, onHandoffEvent)

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
        [`${app}:ready`, onReadyEvent], [`${app}:failed`, onFailedEvent], [`${app}:handoff`, onHandoffEvent],
      ] as const) canvas.removeEventListener(name, handler)
      // Park the canvas off-screen instead of destroying it: the app inside it stays alive.
      if (canvas.parentNode === element) element.removeChild(canvas)
    }
  }, [app, label])

  // Expanded, the editor fills the browser window (see useLeaveOnDoubleEscape for leaving it).
  useEffect(() => {
    if (!expanded) return
    const root = document.documentElement
    root.classList.add('craft-editor-open')
    box.current?.querySelector('canvas')?.focus()
    return () => root.classList.remove('craft-editor-open')
  }, [expanded])
  const collapse = useCallback(() => setExpanded(false), [])
  useLeaveOnDoubleEscape(expanded, collapse)
  useEffect(() => { if (expandRequest) setExpanded(true) }, [expandRequest])

  return <div className={`craft-editor-shell${expanded ? ' expanded' : ''}`} role={expanded ? 'dialog' : undefined} aria-label={expanded ? `${label}, full window` : undefined}>
    <ExpandControls label={label} expanded={expanded} setExpanded={setExpanded} />
    <div className="craft-editor" style={expanded ? undefined : { height: typeof height === 'number' ? `${height}px` : height }} ref={box} aria-label={`${label} editor`}>
      {state === 'loading' && <div className="craft-editor-state" role="status"><p><strong>Opening {label.toLowerCase()}…</strong></p><p className="caption">The first visit takes a moment. The editor runs on this device.</p></div>}
      {state === 'failed' && <div className="craft-editor-state" role="alert"><p><strong>{label} could not load.</strong></p><p className="caption">{error || 'The editor is unavailable in this browser.'}</p><p className="caption">The {label.toLowerCase()} preview below stays available.</p></div>}
    </div>
  </div>
}
