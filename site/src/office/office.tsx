import { useCallback, useEffect, useMemo, useState } from 'react'
import { download, framedBridge, ownerOf, type Tool } from './library'
import { CraftEditor, FramedEditor, openFramedWhenReady, openWhenReady, sendToApp, type EditorsManifest, type Handoff } from '../editors/editor'

// Tall enough to work in, short enough that the page around the editor stays reachable with the
// wheel (an editor takes the wheel while the pointer is over it). Expand fills the window.
const editorHeight = 'min(720px, calc(100vh - 160px))'

// /office: the thirteen Craft editors, each used on its own. Each runs from its official release page
// in a same-origin frame, so the site's styles never reach the app; a page-host build is used only for
// an editor with no staged release.
// Element ids are what web checks select by. An editor shows only when its build is staged.
const allEditors: { id: Tool; label: string; kind: string; group: string }[] = [
  { id: 'wordcraft', group: 'Office', label: 'WordCraft', kind: 'Documents' },
  { id: 'gridcraft', group: 'Office', label: 'GridCraft', kind: 'Spreadsheets' },
  { id: 'deckcraft', group: 'Office', label: 'DeckCraft', kind: 'Presentations' },
  { id: 'pdfcraft', group: 'Office', label: 'PdfCraft', kind: 'PDFs' },
  { id: 'cadcraft', group: 'Drawing and design', label: 'CADCraft', kind: 'CAD drawings' },
  { id: 'photocraft', group: 'Photo', label: 'PhotoCraft', kind: 'Photo editing' },
  { id: 'lightcraft', group: 'Photo', label: 'LightCraft', kind: 'Photo develop' },
  { id: 'vectorcraft', group: 'Drawing and design', label: 'VectorCraft', kind: 'Vector art' },
  { id: 'designcraft', group: 'Drawing and design', label: 'DesignCraft', kind: 'Page layout' },
  { id: 'figcraft', group: 'Drawing and design', label: 'FigCraft', kind: 'Interface design' },
  { id: 'filmcraft', group: 'Video and sound', label: 'FilmCraft', kind: 'Video editing' },
  { id: 'effectcraft', group: 'Video and sound', label: 'EffectCraft', kind: 'Motion graphics' },
  { id: 'soundcraft', group: 'Video and sound', label: 'SoundCraft', kind: 'Audio' },
]
const getEditor = (id: Tool) => allEditors.find(editor => editor.id === id)!

export function OfficeDesk() {
  const [manifest, setManifest] = useState<EditorsManifest>()
  const [manifestError, setManifestError] = useState('')
  // Editors that run from their own official release page, in a frame (public/apps/frame/).
  const [framed, setFramed] = useState<Partial<Record<Tool, { version: string; path: string }>>>({})
  // Only the editors whose builds are staged get a tab; an official release wins over a page-host build.
  const editors = useMemo(() => allEditors.filter(editor => manifest?.[editor.id] || framed[editor.id]), [manifest, framed])
  const [active, setActive] = useState<Tool>('gridcraft')
  // An editor starts when its tab is opened (each is tens of megabytes, more with a document) and
  // stays alive until it is closed: closing removes its frame, which gives its memory back.
  const [opened, setOpened] = useState<Set<Tool>>(() => new Set(['gridcraft']))
  const open = (tool: Tool) => setOpened(previous => previous.has(tool) ? previous : new Set(previous).add(tool))
  // The editor waiting for the reader to confirm closing it.
  const [closing, setClosing] = useState<Tool>()
  const [notice, setNotice] = useState('')
  const [error, setError] = useState('')
  const [dirty, setDirty] = useState<Partial<Record<Tool, boolean>>>({})
  // Opening an app from its button fills the window unless the reader turned that off; the choice
  // is remembered in this browser. Each press counts up, so pressing it again expands it again.
  const [fullWindow, setFullWindow] = useState(true)
  useEffect(() => { try { if (localStorage.getItem('office-full-window') === 'off') setFullWindow(false) } catch { /* No storage: keep the default. */ } }, [])
  const chooseFullWindow = (on: boolean) => { setFullWindow(on); try { localStorage.setItem('office-full-window', on ? 'on' : 'off') } catch { /* Not remembered. */ } }
  const [expandRequests, setExpandRequests] = useState<Partial<Record<Tool, number>>>({})

  // Both lists arrive before any editor mounts, so an editor never starts as one kind and is then
  // replaced by the other.
  const [routed, setRouted] = useState(false)
  useEffect(() => {
    Promise.allSettled([
      fetch('/apps/editors.json')
        .then(response => { if (!response.ok) throw new Error(`The editor files are missing (${response.status}).`); return response.json() })
        .then(setManifest)
        .catch(e => setManifestError(String(e.message || e))),
      fetch('/apps/frame/index.json')
        .then(response => response.ok ? response.json() : [])
        .then((list: { app: Tool; version: string; path: string }[]) => setFramed(Object.fromEntries(list.map(({ app, version, path }) => [app, { version, path }]))))
        .catch(() => setFramed({})),
    ]).then(() => setRouted(true))
  }, [])

  // Unsaved-changes guard for the editors that report it.
  useEffect(() => {
    const warn = (event: BeforeUnloadEvent) => {
      if (Object.values(dirty).some(Boolean)) { event.preventDefault(); event.returnValue = '' }
    }
    addEventListener('beforeunload', warn)
    return () => removeEventListener('beforeunload', warn)
  }, [dirty])

  // A save from a page editor downloads, as the framed editors' saves do. The unsaved mark follows
  // the editor's own dirty reports only: an exported copy (a PDF, an image) leaves edits unsaved.
  const handleSave = useCallback((tool: Tool, detail: { name: string; bytes: ArrayBuffer }) => {
    download(detail)
    setNotice(`${getEditor(tool).label} saved ${detail.name} to your Downloads.`)
  }, [])

  const close = (tool: Tool) => {
    setOpened(previous => { const next = new Set(previous); next.delete(tool); return next })
    setDirty(previous => ({ ...previous, [tool]: false }))
    setClosing(undefined)
    setNotice(`${getEditor(tool).label} is closed and its memory is released. Open its tab to start it again.`)
  }
  // A framed editor doesn't report unsaved changes, so closing it always asks; a page editor asks
  // only when it reports them.
  const askClose = (tool: Tool) => framed[tool] || dirty[tool] ? setClosing(tool) : close(tool)

  const onFailed = useCallback((tool: Tool) => (message: string) => setError(`${getEditor(tool).label}: ${message}`), [])

  // An app hands over a file another app edits: the page picks that app by the file's type, opens
  // its tab, gives it the file and tells the sender. An editor here that can't be handed a file
  // gets its tab opened and the file in Downloads, to open with its own File menu.
  const handoff = useCallback(async (from: Tool, detail: Handoff) => {
    const name = String(detail?.name || 'document')
    const target = ownerOf(name)
    const reply = (type: string, fields: Record<string, unknown>) => sendToApp(from, type, fields)
    if (!target || target === from || !editors.some(editor => editor.id === target)) {
      reply('failed', { error: `No other editor here opens ${name}.` })
      return
    }
    const label = getEditor(target).label
    setError(''); setActive(target); open(target)
    try {
      const bytes = detail.bytes instanceof ArrayBuffer ? detail.bytes : await detail.file!.arrayBuffer()
      const takesFiles = framed[target] ? framedBridge.has(target) : !!manifest?.[target]
      if (!takesFiles) {
        download({ name, bytes })
        setNotice(`${name} is for ${label}: it is in your Downloads; open it there with ${label}'s File menu.`)
        reply('failed', { error: `${label} here can't be handed a file; it was saved to Downloads.` })
        return
      }
      if (framed[target]) await openFramedWhenReady(target, name, bytes)
      else await openWhenReady(target, name, bytes)
      setNotice(`${name} opened in ${label}.`)
      reply('handoff-done', { name, app: target })
    } catch (e) {
      const message = String((e as Error)?.message || e)
      setError(`${label}: ${message}`)
      reply('failed', { error: message })
    }
  }, [editors, framed, manifest])
  const activeManifest = framed[active] ?? manifest?.[active]

  return <div className="office-desk">
    {manifestError && <p id="error" role="alert">{manifestError}</p>}
    {!manifestError && <p id="error" role="alert" hidden={!error}>{error}</p>}
    <div className="office-toolbar">
      <div className="office-groups" role="tablist" aria-label="Editors">
        {[...new Set(editors.map(editor => editor.group))].map(group => (
          <div key={group} className="office-group" role="presentation">
            <span className="office-group-label">{group}</span>
            {editors.filter(editor => editor.group === group).map(editor => (
              <button key={editor.id} role="tab" aria-selected={active === editor.id} id={`tab-${editor.id}`}
                className={`office-tab${active === editor.id ? ' active' : ''}`}
                onClick={() => { setError(''); setActive(editor.id); open(editor.id); if (fullWindow) setExpandRequests(previous => ({ ...previous, [editor.id]: (previous[editor.id] || 0) + 1 })) }}>
                <span className="office-tab-name">{editor.label}</span> <span className="office-tab-kind">{editor.kind}</span>
                {dirty[editor.id] ? <span className="caption"> · unsaved</span> : opened.has(editor.id) && <span className="caption"> · open</span>}
              </button>
            ))}
          </div>
        ))}
      </div>
      <label className="office-full-window"><input type="checkbox" id="office-full-window" checked={fullWindow} onChange={e => chooseFullWindow(e.target.checked)} /> Open apps in full window</label>
    </div>
    <p id="office-notice" role="status">{notice}</p>
    <section className="office-layout">
      <div className="office-editor-stage">
        <div className="office-editor-head">
          <p id="office-editor-name" className="caption">{activeManifest ? `${getEditor(active).label} ${activeManifest.version} · ${getEditor(active).kind}` : getEditor(active).label}</p>
          {opened.has(active) && !closing && <button type="button" id="office-close" className="button secondary" onClick={() => askClose(active)}>Close {getEditor(active).label}</button>}
        </div>
        {closing && <div id="office-close-confirm" role="alertdialog" aria-labelledby="office-close-question" className="office-close-confirm">
          <p id="office-close-question">Close {getEditor(closing).label}? Anything you haven't saved in it will be lost.</p>
          <button type="button" id="office-close-yes" className="button" onClick={() => close(closing)}>Close</button>
          <button type="button" id="office-close-no" className="button secondary" onClick={() => setClosing(undefined)}>Keep it open</button>
        </div>}
        {routed && editors.map(editor => (
          <div key={editor.id} hidden={active !== editor.id}>
            {active === editor.id && !opened.has(editor.id) && <p className="office-closed">{editor.label} is closed. <button type="button" id="office-reopen" className="button" onClick={() => open(editor.id)}>Open {editor.label}</button></p>}
            {opened.has(editor.id) && framed[editor.id] &&
              <FramedEditor app={editor.id} label={getEditor(editor.id).label} src={framed[editor.id]!.path} height={editorHeight} expandRequest={expandRequests[editor.id]}
                hostBridge={framedBridge.has(editor.id)}
                onSave={detail => handleSave(editor.id, detail)}
                onDirty={value => setDirty(previous => ({ ...previous, [editor.id]: value }))}
                onHandoff={detail => void handoff(editor.id, detail)} />}
            {opened.has(editor.id) && !framed[editor.id] && manifest?.[editor.id] && <CraftEditor app={editor.id} label={getEditor(editor.id).label} height={editorHeight} expandRequest={expandRequests[editor.id]}
              onSave={detail => { if (detail.bytes instanceof ArrayBuffer) handleSave(editor.id, detail) }}
              onDirty={value => setDirty(previous => ({ ...previous, [editor.id]: value }))}
              onFailed={onFailed(editor.id)}
              onHandoff={detail => void handoff(editor.id, detail)} />}
          </div>
        ))}
        <p className="caption office-frame-note">Each editor opens files from this device with its own File menu and saves back to this device: your browser asks where, or puts the file in your Downloads.</p>
      </div>
    </section>
  </div>
}
