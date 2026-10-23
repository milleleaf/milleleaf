import { useEffect, useState, type ChangeEvent, type DragEvent, type ReactNode } from 'react'
import { json, saveFile, useEngine } from './engine'
import { fileSize, Status, tooBig } from './status'

// Demo B: one workbook source feeds a report and a deck; edits come back as layers.
// Element ids are what web checks select by.
const text = (value: any): string => value == null ? '' : typeof value === 'object' ? value.text ?? value.value ?? value.name ?? JSON.stringify(value) : String(value)
const number = new Intl.NumberFormat('en-US', { maximumSignificantDigits: 21 })
const show = (value: any) => typeof value === 'number' && Number.isFinite(value) ? number.format(value) : text(value)
const now = () => new Date().toISOString()

// A workbook can raise hundreds of cell notes (for example #DIV/0! results); group repeats by message.
function Notes({ warnings, className }: { warnings: any[]; className?: string }) {
  if (!warnings.length) return null
  const groups = new Map<string, string[]>()
  const single: string[] = []
  for (const w of warnings) {
    const message = String(w?.message ?? w)
    const cell = message.match(/^(.+?![A-Z]+[0-9]+): (.+)$/)
    if (cell) groups.set(cell[2], [...(groups.get(cell[2]) || []), cell[1]])
    else single.push(message)
  }
  const lines = [...single, ...[...groups].map(([message, cells]) => cells.length === 1 ? `${cells[0]}: ${message}` : `${cells.length} cells: ${message}, for example ${cells.slice(0, 3).join(', ')}.`)]
  return <details className={`notes ${className || ''}`}><summary>{`Workbook notes (${warnings.length})`}</summary>{lines.map((line, i) => <p key={i} className="caption">{line}</p>)}</details>
}
// Suggest a table to start from: the longest run of label cells with numbers beside them (plus a header
// row, at most 30 rows), since a chart needs labels and values and a whole sheet is usually too big.
const letters = (n: number) => { let s = ''; for (n++; n; n = Math.floor((n - 1) / 26)) s = String.fromCharCode(65 + (n - 1) % 26) + s; return s }
const isLabel = (x: any) => typeof x === 'string' && x.trim() !== ''
function best(rows: any[][] = []): { range: string; length: number } | undefined {
  let found: { range: string; length: number } | undefined
  for (let r = 0; r < rows.length; r++) for (let c = 0; c + 1 < rows[r].length; c++) {
    if (!isLabel(rows[r][c]) || typeof rows[r][c + 1] !== 'number') continue
    if (r > 0 && isLabel(rows[r - 1][c]) && typeof rows[r - 1][c + 1] === 'number') continue // inside a run already counted
    let end = r
    while (end + 1 < rows.length && end - r < 29 && isLabel(rows[end + 1][c]) && typeof rows[end + 1][c + 1] === 'number') end++
    const length = end - r + 1
    if (length < 2 || (found && found.length >= length)) continue
    // A header row names the values; its label-column cell is often blank.
    const start = r > 0 && isLabel(rows[r - 1][c + 1]) && (isLabel(rows[r - 1][c]) || rows[r - 1][c] == null || rows[r - 1][c] === '') ? r - 1 : r
    found = { range: `${letters(c)}${start + 1}:${letters(c + 1)}${end + 1}`, length }
  }
  return found
}
const suggest = (rows?: any[][]) => best(rows)?.range
// A table the workbook defines (its range differs from the whole sheet) wins over the suggestion.
const startRange = (sheet: any) => sheet && (sheet.range !== sheet.full_range ? sheet.range : suggest(sheet.rows) || sheet.range)

function Table({ value }: { value: any }) {
  const rows: any[] = value?.rows || value || []
  return <table>
    {value?.columns?.length > 0 && <thead><tr>{value.columns.map((c: any, i: number) => <th key={i} scope="col">{text(c)}</th>)}</tr></thead>}
    <tbody>{rows.map((row, r) => <tr key={r}>{(Array.isArray(row) ? row : Object.values(row)).map((cell, c) => <td key={c}>{show(cell)}</td>)}</tr>)}</tbody>
  </table>
}

function Preview({ kind, value }: { kind: 'report' | 'deck'; value?: any }) {
  if (!value) return kind === 'report'
    ? <div className="report-placeholder"><p className="mini-eyebrow">Quarterly review</p><h3>The numbers,<br />in context.</h3><p>Your linked table and chart will appear here.</p><div className="placeholder-lines" /></div>
    : <div className="deck-placeholder"><p className="mini-eyebrow">The same story</p><h3>One set of figures.<br />A different view.</h3><div className="bars"><i /><i /><i /><i /></div></div>
  const series: any[] = value.chart?.series || [], categories: any[] = value.chart?.categories || []
  const maximum = Math.max(1, ...series.flatMap(s => s.values).map((v: any) => Math.abs(Number(v) || 0)))
  return <div className={`${kind}-placeholder live-preview`}>
    <p className="mini-eyebrow">{kind === 'report' ? 'Workbook report' : 'The same figures, on a slide'}</p>
    <h3>{kind === 'report' ? 'Quarterly review' : 'The quarter in view'}</h3>
    <div className="table-scroll"><Table value={value.table} /></div>
    <div className="live-chart">{series.map((item, s) => <div key={s}>
      <p className="chart-label">{item.name || 'Values'}</p>
      {item.values.map((raw: any, i: number) => {
        const n = Number(raw)
        return Number.isFinite(n) && <div className="chart-row" key={i}><span>{text(categories[i] ?? i + 1)}</span><progress max={maximum} value={Math.abs(n)} aria-label={`${text(categories[i])}: ${n}`} /><span>{show(n)}</span></div>
      })}
    </div>)}</div>
    <p className="caption">Linked content preview. Download to see the full document or slide.</p>
  </div>
}

// A returned workbook edit carries whole tables before and after; show the cells that differ.
const rowsOf = (t: any): any[][] => (Array.isArray(t?.rows) ? t.rows : []).map((r: any) => Array.isArray(r) ? r : Object.values(r ?? {}))
function cellDiff(before: any, after: any): string[] {
  const a = rowsOf(before), b = rowsOf(after), columns: any[] = after?.columns || before?.columns || [], out: string[] = []
  for (let r = 0; r < Math.max(a.length, b.length); r++) for (let c = 0; c < Math.max(a[r]?.length || 0, b[r]?.length || 0); c++) {
    const was = a[r]?.[c], now = b[r]?.[c]
    if (was === now) continue
    const row = c === 0 ? `Row ${r + 1}` : text(b[r]?.[0] ?? a[r]?.[0]) || `Row ${r + 1}`
    out.push(`${[row, text(columns[c]?.name ?? columns[c]) || `Column ${c + 1}`].join(' · ')}: ${was == null ? '(empty)' : show(was)} → ${now == null ? '(empty)' : show(now)}`)
  }
  return out
}
const scalar = (v: any) => v == null || typeof v !== 'object'

function ChangeCard({ change, conflict = false }: { change: any; conflict?: boolean }) {
  if (typeof change === 'string') return <article className={`change-item${conflict ? ' conflict' : ''}`}><p>{change}</p></article>
  return <article className={`change-item${conflict ? ' conflict' : ''}`}>
    <h4>{[change.file, change.place || change.path].filter(Boolean).join(' · ') || (conflict ? 'Linked-value conflict' : 'Project content')}</h4>
    {/* A cell edit's description is often just the raw values; the formatted pair below replaces it. */}
    {(change.description || change.message || '') !== `${text(change.before)} → ${text(change.after)}` && <p>{change.description || change.message || change.summary || change.kind || 'Content changed'}</p>}
    {conflict && Array.isArray(change.cell_changes) && change.cell_changes.map((cell: any, i: number) => {
      const label = [cell.row_label || (cell.row != null ? `Row ${cell.row}` : ''), cell.column_name || (cell.column != null ? `Column ${cell.column}` : '')].filter(Boolean).join(' · ')
      return <div key={i}>{label && <strong>{label}</strong>}<p>{`Sent value ${show(cell.before)} → proposed value ${show(cell.proposed)}. Current source: ${show(cell.source_current)}.`}</p></div>
    })}
    {('before' in change || 'after' in change) && (scalar(change.before) && scalar(change.after)
      ? <p>{`${show(change.before)} → ${show(change.after)}`}</p>
      : <CellList cells={cellDiff(change.before, change.after)} />)}
    {conflict && <p>The source value stays current. The typed-over value is recorded for review.</p>}
    <details><summary>Technical evidence</summary><pre>{json(change)}</pre></details>
  </article>
}

function CellList({ cells }: { cells: string[] }) {
  if (!cells.length) return null
  return <ul className="cell-changes">{cells.slice(0, 12).map((c, i) => <li key={i}>{c}</li>)}{cells.length > 12 && <li>{`and ${cells.length - 12} more cells`}</li>}</ul>
}

function Layer({ layer }: { layer: any }) {
  const changes: any[] = Array.isArray(layer.changes) ? layer.changes : []
  const conflicts: any[] = Array.isArray(layer.conflicts) ? layer.conflicts : []
  const count = Array.isArray(layer.changes) ? changes.length : layer.changes ?? 0
  const conflictCount = Array.isArray(layer.conflicts) ? conflicts.length : layer.conflicts ?? 0
  const summary = changes.length === 1 && !conflicts.length ? text(changes[0].description || changes[0].message || changes[0].summary || changes[0]) : `${count} changes · ${conflictCount} conflicts`
  return <li><div>
    <strong>{`${layer.author || 'Unnamed participant'} · ${layer.claimed_app || layer.app || 'Milleleaf'}`}</strong>
    {layer.claimed_app && <p className="caption">{`App label supplied by the participant. File metadata: ${layer.app || 'unspecified'}`}</p>}
    <p>{layer.summary || summary}</p>
    <p className="caption">{[layer.file, layer.id || layer.layer?.header?.id, layer.at || layer.timestamp].filter(Boolean).join(' · ')}</p>
    <details className="history-changes"><summary>What this layer changed</summary>
      {changes.map((c, i) => <ChangeCard key={`c${i}`} change={c} />)}
      {conflicts.map((c, i) => <ChangeCard key={`k${i}`} change={c} conflict />)}
      {!changes.length && !conflicts.length && <p className="muted">No content changes or linked-value conflicts in this saved file.</p>}
    </details>
  </div></li>
}

export function FlowDemo() {
  const [call, engine] = useEngine('/engine/flow-worker.js')
  const [busy, setBusy] = useState(false)
  const [status, setStatus] = useState('Loading the local document engine…')
  const [error, setError] = useState<string>()
  const [info, setInfo] = useState<any>()
  const [name, setName] = useState('Your file or the bundled Hello Table example.')
  const [sheetPath, setSheetPath] = useState('')
  const [range, setRange] = useState('')
  const [chart, setChart] = useState('-1')
  const [snapshot, setSnapshot] = useState<any>()
  const [imported, setImported] = useState(false)
  const [author, setAuthor] = useState('You')
  const [app, setApp] = useState('')
  const [signature, setSignature] = useState('')
  const [dragging, setDragging] = useState(false)
  const ready = engine.status === 'ready'

  // The project lives only in this tab: ask before a reload, a link or closing the tab discards it.
  useEffect(() => {
    if (!snapshot) return
    const warn = (event: BeforeUnloadEvent) => { event.preventDefault(); event.returnValue = '' }
    addEventListener('beforeunload', warn)
    return () => removeEventListener('beforeunload', warn)
  }, [snapshot])

  useEffect(() => {
    if (engine.status === 'ready') setStatus('Ready. Choose your workbook or try the finance example.')
    if (engine.status === 'failed') { setError(engine.error); setStatus('The local document engine is unavailable.') }
  }, [engine])

  const sheets: any[] = info?.sheets || []
  const sheet = sheets.find(s => s.path === sheetPath)
  const charts = sheets.flatMap(s => (s.charts || []).map((c: any) => ({ ...c, sheetName: s.name })))
  const chartNote = charts.find(c => String(c.index) === chart)?.support_note || ''
  const who = () => author.trim() || 'Unnamed participant'

  async function operation(message: string, task: () => Promise<void>) {
    if (busy || !ready) return
    setBusy(true); setError(undefined); setStatus(message)
    try { await task() } catch (e: any) { setError(e?.message || String(e)); setStatus('Review the message, then try again.') }
    finally { setBusy(false) }
  }
  function chooseSheet(path: string, from = info) {
    setSheetPath(path)
    setRange(startRange(from?.sheets?.find((s: any) => s.path === path)) || 'A1:B5')
  }
  function opened(next: any, label: string) {
    setSnapshot(undefined); setImported(false); setSignature('')
    setInfo(next); setName(label); setChart('-1')
    // A workbook-defined table first; otherwise the sheet with the longest label-and-number run.
    const sheets: any[] = next.sheets || []
    const pick = sheets.find(s => s.range !== s.full_range) || sheets.reduce((top, s) => (best(s.rows)?.length || 0) > (best(top?.rows)?.length || 0) ? s : top, sheets[0])
    chooseSheet(pick?.path || '', next)
    setStatus('Workbook ready. Choose your sheet, range and chart.')
  }
  function received(next: any, wasImported: boolean) {
    setSnapshot(next); setImported(wasImported); setSignature('')
    setStatus(wasImported ? 'Saved as a new layer. The current report and deck are ready to download.' : 'Ready. Your report and deck share one source, each in its own style.')
  }
  // Opening another workbook discards the project, which lives only in this tab: ask first.
  const replaceOk = () => !snapshot || confirm('Open another workbook? This discards the current project: its report, deck and layers. Download them first to keep them.')
  const picked = (event: ChangeEvent<HTMLInputElement>, task: (file: File) => Promise<void>, message: string) => {
    const input = event.target, file = input.files?.[0]
    if (!file) return
    operation(`${message} ${file.name} (${fileSize(file.size)})…`, async () => { try { await task(file) } finally { input.value = '' } })
  }
  async function openWorkbook(file: File) {
    if (!/\.(xlsx|ods)$/i.test(file.name)) throw new Error('Choose an XLSX or ODS workbook.')
    tooBig(file)
    opened(await call('open', [await file.arrayBuffer(), who(), now()]), `${file.name} · ${fileSize(file.size)}`)
  }
  async function importRevised(file?: File) {
    if (!file) return
    if (!snapshot) throw new Error('Create the linked report and deck before bringing an edited file back.')
    if (!/\.(xlsx|ods|docx|pptx)$/i.test(file.name)) throw new Error('Choose an XLSX, ODS, DOCX or PPTX file.')
    tooBig(file)
    received(await call('import_file', [await file.arrayBuffer(), file.name, who(), now(), app.trim() || undefined]), true)
  }
  const create = () => operation('Building the report and deck from one source…', async () => {
    if (!sheet) throw new Error('Open a workbook and choose a sheet first.')
    if (!/^[A-Z]+[1-9]\d*:[A-Z]+[1-9]\d*$/i.test(range.trim())) throw new Error('Enter a cell range such as A1:B5.')
    received(await call('create', [sheetPath, range.trim().toUpperCase(), Number(chart), who(), now()]), false)
  })
  const download = (kind: 'report' | 'deck' | 'workbook') => operation('Preparing your local download…', async () => {
    const ext = { report: 'docx', deck: 'pptx', workbook: 'xlsx' }[kind]
    saveFile(await call('export_file', [ext]), `milleleaf-${kind}.${ext}`)
    setStatus(`Ready. The ${kind} download was prepared in this browser.`)
  })
  const sign = () => operation('Signing the current layer locally…', async () => {
    saveFile(json(await call('sign')), 'milleleaf-layer-proof.json', 'application/json')
    setSignature('Signed with an in-memory demo key. The proof contains document content. The claimed name and local clock do not prove identity or time.')
    setStatus('Ready. Demo proof prepared locally.')
  })
  const project = () => operation('Preparing the project layers…', async () => {
    saveFile(json(await call('project')), 'milleleaf-project.json', 'application/json')
    setStatus('Ready. Project layers prepared locally.')
  })
  const drop = (event: DragEvent) => { event.preventDefault(); setDragging(false); const file = event.dataTransfer.files[0]; operation(file ? `Reading the dropped file locally: ${file.name} (${fileSize(file.size)})…` : 'Reading the dropped file locally…', () => importRevised(file)) }

  const noWorkbook = busy || !sheets.length, noProject = busy || !snapshot
  const changes: any[] = snapshot?.changes || [], conflicts: any[] = snapshot?.conflicts || []
  const warnings: any[] = snapshot?.warnings || info?.warnings || []
  let source: ReactNode = <p className="caption">The selected cells appear after you create the linked files.</p>
  if (snapshot?.report?.table) source = <Table value={snapshot.report.table} />
  else if (sheet?.rows?.length) source = <Table value={sheet.rows} />

  return <div className="demo-flow">
    <p className="privacy">Files stay in this browser. No upload. No account.</p>
    <Status busy={busy || engine.status === 'loading'}>{status}</Status><p id="error" role="alert" hidden={!error}>{error}</p>
    <section className="workbench" aria-label="Content flow">
      <article className="source panel">
        <div className="section-label"><span>1. The source</span><span className="file-tag">.xlsx</span></div>
        <h2>Start with a workbook</h2><p>Made in Excel, Calc or GridCraft. Every app is a place to work.</p>
        <div className="actions">
          <button id="sample" className="button" disabled={!ready || busy} onClick={() => replaceOk() && operation('Opening the bundled workbook…', async () => opened(await call('sample', [who(), now()]), 'Hello Table · finance.xlsx'))}>Try the finance workbook</button>
          <label className="button secondary file-button">Choose a workbook<input id="workbook-file" type="file" accept=".xlsx,.ods" disabled={!ready || busy} onChange={e => { if (replaceOk()) picked(e, openWorkbook, 'Reading your workbook locally:'); else e.target.value = '' }} /></label>
        </div>
        <p className="caption">ODS data import reads values, text and merges. Formula cells use saved cached values; ODS styles and charts are not carried. Keep your original workbook. XLSX carries the richer workbook model. Up to 64 MiB per file.</p>
        <p id="workbook-name" className="caption">{name}</p>
        <div id="source-warnings" aria-live="polite"><Notes warnings={info?.warnings || []} /></div>
        <div id="selection" hidden={!info}>
          <div className="fields">
            <label>Sheet<select id="sheet" value={sheetPath} disabled={noWorkbook} onChange={e => chooseSheet(e.target.value)}>{sheets.map(s => <option key={s.path} value={s.path} data-chart-count={(s.charts || []).length}>{s.name}</option>)}</select></label>
            <label>Table range<input id="range" placeholder="A1:B5" spellCheck={false} value={range} disabled={noWorkbook} onChange={e => setRange(e.target.value)} /></label>
            <label className="wide">Chart<select id="chart" aria-describedby="chart-note" value={chart} disabled={noWorkbook} onChange={e => setChart(e.target.value)}>
              <option value="-1">Make a chart from this range</option>
              {charts.map(c => <option key={c.index} value={String(c.index)}>{[c.sheetName, c.title || c.name || `Chart ${c.index + 1}`, c.kind, c.supported === false ? 'unsupported' : ''].filter(Boolean).join(' · ')}</option>)}
            </select></label>
          </div>
          <p id="chart-note" className="caption" aria-live="polite" hidden={!chartNote}>{chartNote}</p>
          <div id="source-table" className="table-scroll">{source}{!snapshot && sheet?.preview_note && <p className="caption">{sheet.preview_note}</p>}</div>
          <button id="create" className="button primary" disabled={noWorkbook} onClick={create}>Use it in a report and a deck</button>
        </div>
      </article>
      <div className="connector" aria-hidden="true"><span>One<br />source</span>→</div>
      <div className="destinations">
        <article className="report panel"><div className="section-label"><span>2. The report</span><span className="file-tag">.docx</span></div>
          <div className="preview-heading"><h2>A document's own style</h2><button id="download-report" className="button secondary small" disabled={noProject} onClick={() => download('report')}>Download report</button></div>
          <Preview kind="report" value={snapshot?.report} /></article>
        <article className="deck panel"><div className="section-label"><span>3. The deck</span><span className="file-tag">.pptx</span></div>
          <div className="preview-heading"><h2>A slide's own style</h2><button id="download-deck" className="button secondary small" disabled={noProject} onClick={() => download('deck')}>Download deck</button></div>
          <Preview kind="deck" value={snapshot?.deck} /></article>
      </div>
    </section>
    <p className="mix-note">Any app in, any app out. Today: Microsoft Office, LibreOffice and the ArtCraft apps, through DOCX, XLSX, PPTX, ODT, ODS and ODP. A Calc workbook can feed a Word report and a DeckCraft deck. These Office-format files open in Microsoft Office, LibreOffice and the ArtCraft apps. <a href="#checked">See the evidence and limits.</a></p>
    <section id="project" hidden={!snapshot}>
      <div className="section-heading"><div><h2>Change it anywhere. Bring it back.</h2></div>
        <p id="keep-note" className="caption">This project lives only in this browser tab. Leaving or reloading the page discards it; download the project layers to keep them.</p>
        <button id="download-project" className="button secondary" disabled={noProject} onClick={project}>Download project layers</button>
        <button id="download-workbook" className="button secondary" disabled={noProject} onClick={() => download('workbook')}>Download current workbook</button></div>
      <div className="review-layout">
        <article className="panel bring-back">
          <div className="fields"><label>Claimed name<input id="author" value={author} maxLength={120} onChange={e => setAuthor(e.target.value)} /></label><label>App used<input id="app" placeholder="e.g. Calc, Word, DeckCraft" maxLength={120} value={app} onChange={e => setApp(e.target.value)} /></label></div>
          <label id="drop-zone" className={`drop-zone${dragging ? ' dragging' : ''}`} onDragOver={e => { e.preventDefault(); setDragging(true) }} onDragLeave={() => setDragging(false)} onDrop={drop}>
            <strong>Drop a changed workbook,<br />report or deck here</strong><span>or choose a .xlsx, .ods, .docx or .pptx file, up to 64 MiB</span>
            <input id="edited-file" type="file" accept=".xlsx,.ods,.docx,.pptx" disabled={noProject} onChange={e => picked(e, importRevised, 'Reading the saved file and recording its layer:')} />
          </label>
          <p className="caption">A workbook edit updates linked places in both files. A report or deck edit becomes a reviewable layer. Typing over a linked number raises a conflict.</p>
        </article>
        <article className="panel"><div className="section-label">The latest layer</div>
          <h3 id="change-summary">{imported ? `${changes.length} ${changes.length === 1 ? 'changed place' : 'changed places'} · ${conflicts.length} ${conflicts.length === 1 ? 'conflict' : 'conflicts'}` : snapshot ? 'One source, two linked files' : 'Ready for your next edit'}</h3>
          <div id="changes">{changes.map((c, i) => <ChangeCard key={i} change={c} />)}
            {!changes.length && <p className="muted">{conflicts.length ? 'No independent changes applied. Review the conflict below.' : imported ? 'No content changes in this saved file.' : 'The report and deck use the same workbook cells.'}</p>}</div>
          <div id="conflicts">{conflicts.map((c, i) => <ChangeCard key={i} change={c} conflict />)}</div>
          <div id="warnings"><Notes warnings={warnings} /></div>
          <button id="sign" className="button secondary" disabled={noProject} onClick={sign}>Sign this layer with a demo key</button>
          <p id="signature" className="caption">{signature}</p>
        </article>
      </div>
      <section className="history"><div className="section-heading"><h2>One project, layer by layer.</h2><span className="caption">Names are claims, not verified identities.</span></div>
        <ol id="history">{(snapshot?.history || []).map((layer: any, i: number) => <Layer key={i} layer={layer} />)}</ol></section>
      <details className="technical"><summary>Project evidence</summary><pre id="snapshot">{snapshot ? json(snapshot) : ''}</pre></details>
    </section>
    <section id="checked" className="checked"><h2>A mix of apps.<br />A precise record.</h2>
      <div id="app-evidence" className="evidence-grid">
        <article><h3>Microsoft Office</h3><p>A real Excel 16 saved workbook fixture was checked with Writer report edits and an Impress deck save in one project. Fresh Excel, Word and PowerPoint 16 saves of these outputs were checked too: re-saves returned no changes, a workbook edit updated both files, heading edits returned as changes, and typing over a linked value raised a conflict.</p></article>
        <article><h3>LibreOffice</h3><p>Fresh LibreOffice 24.2.7.2 checks: Calc, Writer and Impress re-saves returned no changes. Calc edits updated both outputs; Writer returned a paragraph edit and a linked-value conflict. Mixed routes passed.</p></article>
        <article><h3>ArtCraft apps</h3><p>The exports use the GridCraft, WordCraft and DeckCraft libraries. Pinned CLI checks passed: GridCraft workbook re-save and cell edit, WordCraft report re-save and paragraph edit, DeckCraft deck re-save and title edit.</p></article>
      </div>
      <details><summary>Native OpenDocument files and the command line</summary><p>This browser flow writes .xlsx, .docx and .pptx. It also reads .ods data directly through GridCraft, with the limits stated beside the workbook picker. Full ODF conversion and native .odt and .odp files use LibreOffice as a separate process through the command-line tool, which does not run inside a browser.</p></details>
    </section>
  </div>
}
