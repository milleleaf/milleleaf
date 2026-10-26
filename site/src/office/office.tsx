import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { download, files, saveFile, savedEntry, type SavedFile, type Tool } from './library'
import { CraftEditor, openInEditor, startEditor, type EditorsManifest } from '../editors/editor'
import { fileSize } from '../demos/status'

// The /office desk: four real Craft editors hosted in this page (no iframes), one shared file
// list in the visitor's own browser storage. Element ids are what web checks select by.
const editors: { id: Tool; label: string; format: string }[] = [
  { id: 'wordcraft', label: 'WordCraft', format: 'DOCX' },
  { id: 'gridcraft', label: 'GridCraft', format: 'XLSX' },
  { id: 'pdfcraft', label: 'PdfCraft', format: 'PDF' },
  { id: 'cadcraft', label: 'CADCraft', format: 'DXF' },
]
const getEditor = (id: Tool) => editors.find(editor => editor.id === id)!

const extensionTool = (name: string): Tool => {
  const extension = name.split('.').pop()?.toLowerCase()
  if (extension === 'pdf') return 'pdfcraft'
  if (extension && ['dxf', 'dwg'].includes(extension)) return 'cadcraft'
  if (extension && ['xlsx', 'xlsm', 'csv', 'tsv', 'ods'].includes(extension)) return 'gridcraft'
  return 'wordcraft'
}
// WordCraft and CADCraft report a saveAs flag; GridCraft cannot, PdfCraft can.
const hasSaveAs = (tool: Tool) => tool === 'wordcraft' || tool === 'pdfcraft'

export function OfficeDesk() {
  const [manifest, setManifest] = useState<EditorsManifest>()
  const [manifestError, setManifestError] = useState('')
  const [active, setActive] = useState<Tool>('gridcraft')
  // An editor starts the first time its tab is opened (each is tens of megabytes) and then stays alive.
  const [opened, setOpened] = useState<Set<Tool>>(() => new Set(['gridcraft']))
  useEffect(() => setOpened(previous => previous.has(active) ? previous : new Set(previous).add(active)), [active])
  const [library, setLibrary] = useState<SavedFile[]>([])
  const [notice, setNotice] = useState('')
  const [error, setError] = useState('')
  const [ready, setReady] = useState<Partial<Record<Tool, boolean>>>({})
  const [dirty, setDirty] = useState<Partial<Record<Tool, boolean>>>({})
  const currentFiles = useRef<Partial<Record<Tool, SavedFile>>>({})
  const pending = useRef<Partial<Record<Tool, { name: string; bytes: ArrayBuffer }>>>({})
  const fileInput = useRef<HTMLInputElement>(null)
  const libraryRef = useRef<SavedFile[]>([])
  libraryRef.current = library

  useEffect(() => {
    fetch('/apps/editors.json')
      .then(response => { if (!response.ok) throw new Error(`The editor files are missing (${response.status}).`); return response.json() })
      .then(setManifest)
      .catch(e => setManifestError(String(e.message || e)))
  }, [])

  const refreshLibrary = useCallback(async () => {
    try { setLibrary(await files()); setNotice('') } catch { setError('Browser storage is unavailable. Each editor can still download your work from its own File menu.') }
  }, [])
  useEffect(() => { void refreshLibrary() }, [refreshLibrary])

  // Unsaved-changes guard: WordCraft and CADCraft cannot report dirty state reliably yet, so
  // any session that has saved once (or any active one still loading) asks before leaving.
  useEffect(() => {
    const warn = (event: BeforeUnloadEvent) => {
      if (Object.values(dirty).some(Boolean)) { event.preventDefault(); event.returnValue = '' }
    }
    addEventListener('beforeunload', warn)
    return () => removeEventListener('beforeunload', warn)
  }, [dirty])

  const handleSave = useCallback(async (tool: Tool, detail: { name: string; bytes: ArrayBuffer; saveAs?: boolean }) => {
    const { file, updatesCurrent } = savedEntry(tool, detail, currentFiles.current[tool])
    try {
      await saveFile(file)
      if (updatesCurrent) {
        currentFiles.current[tool] = file
        setDirty(previous => ({ ...previous, [tool]: false }))
      }
      await refreshLibrary()
      setNotice(`Saved ${file.name} on this device. Download a copy to keep it outside your browser.`)
    } catch {
      download(file)
      setNotice(`Browser storage could not save ${file.name}. A backup download has been started; check your Downloads.`)
    }
  }, [refreshLibrary])

  const sendFile = useCallback((tool: Tool, file: { name: string; bytes: ArrayBuffer }) => {
    currentFiles.current[tool] = { id: 'pending', name: file.name, tool, bytes: file.bytes, modified: Date.now() }
    if (!openInEditor(tool, file.name, file.bytes)) {
      pending.current[tool] = { name: file.name, bytes: file.bytes.slice(0) }
      void startEditor(tool)
    }
  }, [])

  const onReady = useCallback((tool: Tool) => () => {
    setReady(previous => ({ ...previous, [tool]: true }))
    setDirty(previous => ({ ...previous, [tool]: false }))
    const waiting = pending.current[tool]
    if (waiting) { delete pending.current[tool]; openInEditor(tool, waiting.name, waiting.bytes) }
  }, [])

  const onFailed = useCallback((tool: Tool) => (message: string) => setError(`${getEditor(tool).label}: ${message}`), [])

  const openFromDesk = useCallback((file: SavedFile) => {
    if (dirty[file.tool] && !confirm(`Open another file in ${getEditor(file.tool).label}? Save your current changes first.`)) return
    setActive(file.tool)
    sendFile(file.tool, file)
  }, [dirty, sendFile])

  const importFile = useCallback(async (file: File) => {
    const tool = extensionTool(file.name)
    try {
      const saved: SavedFile = { id: crypto.randomUUID(), name: file.name, tool, bytes: await file.arrayBuffer(), modified: Date.now() }
      await saveFile(saved)
      await refreshLibrary()
      setActive(tool)
      sendFile(tool, saved)
    } catch (e) { setError(`This file could not be stored: ${String((e as Error).message || e)}`) }
  }, [refreshLibrary, sendFile])

  const deskSize = useMemo(() => library.reduce((total, file) => total + file.bytes.byteLength, 0), [library])
  const activeManifest = manifest?.[active]

  return <div className="office-desk">
    <p className="privacy">Files stay in this browser, in its own storage on this device. No upload. No account.</p>
    {manifestError && <p id="error" role="alert">{manifestError}</p>}
    {!manifestError && <p id="error" role="alert" hidden={!error}>{error}</p>}
    <div className="office-toolbar" role="tablist" aria-label="Editors">
      {editors.map(editor => (
        <button key={editor.id} role="tab" aria-selected={active === editor.id} id={`tab-${editor.id}`}
          className={`office-tab${active === editor.id ? ' active' : ''}`}
          onClick={() => { setError(''); setActive(editor.id) }}>
          {editor.label} <span className="file-tag">{editor.format}</span>
          {dirty[editor.id] && <span className="caption"> · unsaved</span>}
        </button>
      ))}
      <button className="button secondary small" id="office-open-file" onClick={() => fileInput.current?.click()}>Open a file from disk</button>
      <input ref={fileInput} type="file" hidden id="office-file-input"
        accept=".docx,.odt,.rtf,.txt,.md,.xlsx,.xlsm,.ods,.csv,.tsv,.pptx,.odp,.pdf,.dxf,.dwg"
        onChange={event => { const file = event.target.files?.[0]; if (file) void importFile(file); event.target.value = '' }} />
    </div>
    <p id="office-notice" role="status">{notice}</p>
    <section className="office-layout">
      <div className="office-editor-stage">
        <p id="office-editor-name" className="caption">{activeManifest ? `${getEditor(active).label} ${activeManifest.version} · ${getEditor(active).format}` : getEditor(active).label}</p>
        {editors.map(editor => (
          <div key={editor.id} hidden={active !== editor.id}>
            {opened.has(editor.id) && <CraftEditor app={editor.id} label={getEditor(editor.id).label} height={720}
              onSave={detail => { if (detail.bytes instanceof ArrayBuffer) void handleSave(editor.id, detail) }}
              onDirty={value => setDirty(previous => ({ ...previous, [editor.id]: value }))}
              onReady={onReady(editor.id)}
              onFailed={onFailed(editor.id)} />}
          </div>
        ))}
      </div>
      <aside className="office-desk-list" aria-label="Your desk">
        <div className="section-label"><span>Your desk</span><span>{library.length} {library.length === 1 ? 'file' : 'files'}</span></div>
        {library.length === 0
          ? <p className="caption">Nothing on the desk yet. Open a file from disk, or save from any editor. Everything stays in this browser.</p>
          : <ul id="office-files" className="office-files">{library.map(file => (
              <li key={file.id}>
                <button className="office-file-open" onClick={() => openFromDesk(file)}>
                  <strong>{file.name}</strong>
                  <span className="caption">{getEditor(file.tool).label} · {fileSize(file.bytes.byteLength)} · {new Date(file.modified).toLocaleDateString(undefined, { day: 'numeric', month: 'short' })}</span>
                </button>
                <button className="button secondary small" aria-label={`Download ${file.name}`} onClick={() => download(file)}>Download</button>
              </li>
            ))}</ul>}
        <p className="caption">On this device: {fileSize(deskSize)}. Clearing the browser's site data clears the desk. Saves from the editors land here; a workbook's Export to PDF becomes its own PDF entry. Download anything you want to keep outside the browser.</p>
      </aside>
    </section>
  </div>
}
