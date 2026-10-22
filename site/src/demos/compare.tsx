import { useEffect, useRef, useState, type DragEvent, type ReactNode } from 'react'
import { json, useEngine } from './engine'
import { fileSize, Status, tooBig } from './status'

// Demo A: compare two versions of a file, in the browser. Element ids are what web checks select by.
type Dated = { file: File; time: number; source: string }
type Signed = { proof: any; result: any; url: string }

const entries = (value: any): any[] => Array.isArray(value) ? value : value && typeof value === 'object' ? Object.entries(value).map(([feature, count]) => ({ feature, count })) : value == null ? [] : [value]
const valueText = (value: any) => typeof value === 'string' ? value : json(value)
function readable(value: any): string {
  if (value == null) return 'Not present'
  if (typeof value !== 'object') return String(value) || 'Empty line'
  if (Array.isArray(value)) return value.map(readable).join(' · ')
  return Object.entries(value).map(([key, val]) => `${key}: ${readable(val)}`).join(' · ')
}
const verificationMessage = (result: any) => result.valid === true ? 'Valid signature · the signed layer passes verification.' : 'Signature verification failed · inspect the verification details.'

function Detail({ title, value }: { title: ReactNode; value: unknown }) {
  return <details><summary>{title}</summary><pre>{json(value)}</pre></details>
}
function ReportItem({ description, technical }: { description: string; technical: unknown }) {
  return <article className="report-item"><p className="change-description">{description}</p><Detail title="Technical detail" value={technical} /></article>
}
function Empty({ children }: { children: ReactNode }) { return <p className="empty">{children}</p> }

function ContextPanel({ change, side }: { change: any; side: 'before' | 'after' }) {
  const context = change.context?.[side]
  const lines: any[] = context?.media?.length ? [] : context?.lines || (Array.isArray(context) ? context : [context ?? change[side]])
  return <section className={`context-panel ${side}`}>
    <h5>{side === 'before' ? 'Before' : 'After'}</h5>
    {lines.map((line, i) => <p key={i} className={line?.highlight || line?.changed || !context ? 'highlight' : undefined}>
      {typeof line === 'object' && line !== null && 'text' in line ? line.text || (change.context?.kind === 'grid' ? 'Blank cell' : 'Empty line') : readable(line)}
    </p>)}
    {context?.grid && <div className="context-grid"><table><tbody>
      <tr>{['', ...context.grid.columns].map((label, i) => <th key={i}>{typeof label === 'object' ? readable(label) : String(label)}</th>)}</tr>
      {context.grid.rows.map((row: any, r: number) => <tr key={r}><th>{row.label}</th>{row.cells.map((cell: any, c: number) => <td key={c} title={cell.address} className={cell.changed ? 'highlight' : undefined}>{cell.text}</td>)}</tr>)}
    </tbody></table></div>}
    {(context?.media || []).map((media: any, i: number) => <div key={i}>
      <p className="highlight">{`${media.name}${media.width != null ? ` · ${media.width} × ${media.height} px` : ''}${media.bytes != null ? ` · ${media.bytes} bytes` : ''}`}</p>
      {media.thumbnail && <img src={media.thumbnail} alt={media.name || 'Picture'} />}
    </div>)}
  </section>
}
function Change({ change }: { change: any }) {
  return <article className="report-item">
    <h4>{change.locator || change.place}</h4>
    <p className="change-description">{change.description}</p>
    <div className="context-sides"><ContextPanel change={change} side="before" /><ContextPanel change={change} side="after" /></div>
    <Detail title="Developer detail" value={change} />
  </article>
}

function Results({ report, adoption, signed, signing }: { report: any; adoption: any; signed?: Signed; signing: ReactNode }) {
  const changes = entries(report.changes)
  const inventory = entries(report.inventory_changes)
  const losses = report.adapter_losses || {}
  const spacing = changes.filter(c => c.spacing_only)
  const shown = changes.filter(c => !c.spacing_only || spacing.length === changes.length)
  const reductions = inventory.filter(i => i.after < i.before && !i.feature?.includes('value'))
  const warnings = [...entries(adoption.warnings), ...entries(report.warnings)]
  return <section id="results" aria-labelledby="results-title">
    <p className="comparison-summary" id="comparison-summary" role="status" tabIndex={-1}>{report.summary}</p>
    <span className="pill" id="format" hidden>{String(report.format || adoption.format || 'Document')}</span><h2 id="results-title" className="sr-only">Comparison result</h2>
    <section className="report" aria-label="Changes in context" hidden={!changes.length}><div id="changes">
      {shown.map((change, i) => <Change key={i} change={change} />)}
      {spacing.length > 0 && spacing.length < changes.length && <section><h3>{`Empty lines and spacing (${spacing.length})`}</h3>{spacing.map((change, i) => <Change key={i} change={change} />)}</section>}
    </div></section>
    <details className="report" id="inventory-fold"><summary id="inventory-summary">{reductions.length ? `Edited file inventory: ${reductions.map(i => `${i.feature} ${i.before} → ${i.after}`).join('; ')} · possible loss · Review` : `Inventory differences (${inventory.length}) · Review`}</summary>
      <h3 id="losses-title">Changes or removals in the edited file</h3><p>A reduction may be intentional or a possible loss. Equal counts can hide replacements.</p>
      <div id="inventory-changes">
        {inventory.map((item, i) => <ReportItem key={i} description={item.description || `Edited file: ${item.feature} changed from ${item.before} to ${item.after}. ${item.status || ''}`} technical={item} />)}
        {!inventory.length && <Empty>No counted inventory differences between the original and edited files.</Empty>}
        <Detail title={`Archive parts added, removed or rewritten: ${(report.part_changes || []).length} (not automatically a content change or loss)`} value={report.part_changes || []} />
      </div>
    </details>
    <details className="report"><summary id="limitations-summary">{`What Milleleaf cannot carry yet (${entries(losses.original).length} original; ${entries(losses.edited).length} edited) · Review`}</summary>
      <h3 id="adapter-losses-title">What Milleleaf cannot carry yet</h3><p>These limitations belong to Milleleaf, separately for each source file, not the app that saved it.</p>
      <div id="adapter-losses">{([['original', 'Original file'], ['edited', 'Edited file']] as const).map(([side, label]) => {
        const items = entries(losses[side])
        return <div key={side}><h4>{label}</h4>
          {items.map((item, i) => <ReportItem key={i} description={`${item.feature} (${item.count}): ${item.carried === 'partly carried' ? 'Milleleaf carries this only in part' : 'Milleleaf cannot carry this yet'}.${item.note ? ` ${item.note}` : ''}`} technical={{ source: side, ...item }} />)}
          {!items.length && <Empty>{`${label}: no counted adapter limitations.`}</Empty>}
        </div>
      })}</div>
    </details>
    <details className="report"><summary>Reading this result and developer detail</summary>
      <div id="warnings">{warnings.length ? <ul>{warnings.map((w, i) => <li key={i}>{valueText(w)}</li>)}</ul> : <Empty>No additional adapter warnings. The browser edition’s limitations still apply.</Empty>}</div>
      <details><summary>Original inventory</summary><pre id="original-report">{json(adoption)}</pre></details>
      <details><summary>Complete comparison and layer</summary><pre id="complete-report">{json(signed ? signed.result.comparison ?? report : report)}</pre></details>
    </details>
    {signing}
  </section>
}

export function CompareDemo() {
  const [call, engine] = useEngine('/engine/compare-worker.js')
  const [busy, setBusy] = useState(false)
  const [status, setStatus] = useState('Loading the local document engine…')
  const [error, setError] = useState<string>()
  const [pair, setPair] = useState<{ original?: File; edited?: File }>({})
  const [orderNote, setOrderNote] = useState('')
  const [adoption, setAdoption] = useState<any>()
  const [report, setReport] = useState<any>()
  const [run, setRun] = useState(0)
  const [author, setAuthor] = useState('Demo participant')
  const [signed, setSigned] = useState<Signed>()
  const [saved, setSaved] = useState<{ message: string; result?: any }>()
  const [dragging, setDragging] = useState(false)
  const privacy = useRef<HTMLDetailsElement>(null)
  const ready = engine.status === 'ready'

  useEffect(() => {
    if (engine.status === 'ready') setStatus('Ready. Drop or choose both versions to compare automatically.')
    if (engine.status === 'failed') { setError(engine.error); setStatus('The local document engine is unavailable.') }
  }, [engine])
  useEffect(() => { document.body.classList.toggle('busy', busy) }, [busy])
  useEffect(() => {
    // A dropped file outside the drop zone must not navigate away from the page.
    const stop = (event: Event) => event.preventDefault()
    window.addEventListener('dragover', stop); window.addEventListener('drop', stop)
    return () => { window.removeEventListener('dragover', stop); window.removeEventListener('drop', stop) }
  }, [])
  useEffect(() => { if (report) document.getElementById('comparison-summary')?.scrollIntoView({ block: 'start', behavior: 'instant' }) }, [run])

  async function operation(message: string, task: () => Promise<void>) {
    if (busy || !ready) return
    setBusy(true); setError(undefined); setStatus(message)
    try { await task() } catch (e: any) { setError(e?.message || String(e)); setStatus('The operation did not finish. Review the message above and try again.') }
    finally { setBusy(false) }
  }
  function clearSignature() {
    if (signed) URL.revokeObjectURL(signed.url)
    setSigned(undefined)
  }
  async function compare(original: File, edited: File) {
    let buffer = await original.arrayBuffer()
    setAdoption(await call('adopt', [buffer], [buffer]))
    clearSignature()
    buffer = await edited.arrayBuffer()
    setReport(await call('diff', [buffer, 'Unspecified author', new Date().toISOString()], [buffer]))
    setRun(r => r + 1)
    setStatus('Comparison ready. All file operations stay in this browser.')
  }
  function loadFiles(list: FileList | null) {
    const files = [...(list || [])]
    if (!files.length) return
    operation(`Reading and comparing ${files.map(f => `${f.name} (${fileSize(f.size)})`).join(' and ')} in this browser…`, async () => {
      if (files.length > 2) throw new Error('Choose two versions of the same document.')
      if (files.some(f => !/\.(docx|xlsx|pptx)$/i.test(f.name))) throw new Error('Choose DOCX, XLSX or PPTX files.')
      files.forEach(tooBig)
      const chosen = files.length === 2 ? files : pair.original ? [pair.original, files[0]] : files
      const dated: Dated[] = []
      for (const file of chosen) {
        const buffer = await file.arrayBuffer()
        const modified = Date.parse((await call('metadata', [buffer], [buffer])).modified)
        dated.push({ file, time: Number.isFinite(modified) ? modified : file.lastModified, source: Number.isFinite(modified) ? 'document modified time' : 'file modified time' })
      }
      dated.sort((a, b) => a.time - b.time)
      const [original, edited] = [dated[0].file, dated[1]?.file]
      setPair({ original, edited })
      setOrderNote(dated.length === 2 ? dated[0].time === dated[1].time ? 'Modified times match; check the order or Swap.' : `Earlier ${dated[0].source} first. Use Swap to change the order.` : '')
      if (edited) await compare(original, edited)
      else { setReport(undefined); setStatus('Choose the other version to compare automatically.') }
    })
  }
  const swap = () => operation('Swapping versions and comparing locally…', async () => {
    const { original, edited } = pair
    setPair({ original: edited, edited: original })
    setOrderNote('Order chosen with Swap.')
    await compare(edited!, original!)
  })
  const sign = () => operation('Generating the session demo key and signing the layer…', async () => {
    const buffer = await pair.edited!.arrayBuffer()
    const comparison = await call('diff', [buffer, author.trim() || 'Unspecified author', new Date().toISOString()], [buffer])
    const proof = await call('sign')
    const encoded = new TextEncoder().encode(json(proof))
    const result = await call('verify', [encoded.buffer], [encoded.buffer])
    clearSignature()
    setSigned({ proof, result: { ...result, comparison }, url: URL.createObjectURL(new Blob([json(proof)], { type: 'application/json' })) })
    setStatus(result.valid === true ? 'Layer signed and verified with the session demo key. Download the proof to keep it.' : 'The signature did not verify. See the verification details.')
  })
  const verifySaved = (file?: File) => file && operation('Verifying the saved proof locally…', async () => {
    setSaved(undefined)
    const buffer = await file.arrayBuffer()
    const result = await call('verify', [buffer], [buffer])
    setSaved({ message: verificationMessage(result), result })
    setStatus('Saved proof verification complete.')
  })
  const drop = (event: DragEvent) => { event.preventDefault(); setDragging(false); loadFiles(event.dataTransfer.files) }
  const { comparison: _, ...verification } = signed?.result || {}

  const signing = <details className="report signature" id="signing"><summary>Keep a signed record of this change · demo key</summary><div id="signing-content">
    <p>This page generates a <strong>demo key</strong> in this browser session: Ed25519 + ML-DSA-65. The key is not an identity check. The author is a claim, and the time comes from this machine’s clock, not a trusted timestamp service. Reloading the page creates a new session. The proof contains document content and available pictures; keep it as private as the files.</p>
    <label htmlFor="author">Claimed author <input id="author" type="text" value={author} onChange={e => setAuthor(e.target.value)} maxLength={200} autoComplete="off" disabled={!ready || busy} /></label>
    <button id="sign" className="button" onClick={sign} disabled={!ready || busy}>Sign this change with the demo key</button>
    <div id="signature-result" hidden={!signed}>{signed && <>
      <p className={`verification${signed.result.valid !== true ? ' invalid' : ''}`} id="verification" role="status">{verificationMessage(signed.result)}</p>
      <p id="signed-author">{`Claimed author: ${signed.result.author}`}</p>
      <p id="signed-at">{`Recorded at: ${signed.result.at} · local clock`}</p>
      <a className="button secondary" id="download" href={signed.url} download="milleleaf-demo-proof.json">Download signed proof</a>
      <details><summary>Full signature, public keys and signed layer</summary><pre id="proof">{json(signed.proof)}</pre></details>
      <details><summary>Verification details</summary><pre id="verify-report">{json(verification)}</pre></details>
    </>}</div>
  </div></details>

  return <div className="demo-compare">
    <section className="workflow" aria-label="Compare two versions">
      <div className="drop-area">
        <label className={`drop${dragging ? ' dragging' : ''}`} id="files-drop" htmlFor="files" onDragOver={e => { e.preventDefault(); setDragging(true) }} onDragLeave={() => setDragging(false)} onDrop={drop}>
          <strong>Drop both versions here</strong><span className="drop-note">Or choose two DOCX, XLSX or PPTX files. Up to 64 MiB per file. Earlier modified time goes first. Large files take longer.</span>
          <input id="files" type="file" multiple accept=".docx,.xlsx,.pptx" disabled={!ready || busy} onChange={e => loadFiles(e.target.files)} />
        </label>
        <p className="privacy">Your files stay on this device. Nothing is uploaded: the comparison runs in your browser. <a href="#privacy-check" onClick={() => { if (privacy.current) privacy.current.open = true }}>How to check</a></p>
      </div>
      <div className="file-order">
        <p>Original: <span id="original-name">{pair.original?.name || 'Choose a file'}</span>{pair.original && <span className="file-size">{` · ${fileSize(pair.original.size)}`}</span>}</p>
        <p>Edited: <span id="edited-name">{pair.edited?.name || (pair.original ? 'Choose the other version' : 'Choose a file')}</span>{pair.edited && <span className="file-size">{` · ${fileSize(pair.edited.size)}`}</span>}</p>
        <button id="swap" className="button secondary" onClick={swap} disabled={!ready || busy || !pair.original || !pair.edited}>Swap</button>
        <p id="order-note">{orderNote}</p>
      </div>
    </section>
    <Status busy={busy || engine.status === 'loading'}>{status}</Status>
    <p className="error" id="error" role="alert" hidden={!error}>{error}</p>
    {report && adoption ? <Results key={run} report={report} adoption={adoption} signed={signed} signing={signing} /> : <section id="results" hidden />}
    <details className="verify-file report"><summary>Verify a saved signature</summary>
      <p>A downloaded proof can be verified in a fresh browser session. Verification checks the signed layer and signature policy; it does not establish the signer’s real-world identity or independently verify the source files.</p>
      <label className="proof-input" htmlFor="proof-file">Choose a signed proof <input id="proof-file" type="file" accept=".json,application/json" disabled={!ready || busy} onChange={e => { verifySaved(e.target.files?.[0]); e.target.value = '' }} /></label>
      <p id="saved-verification" role="status" aria-live="polite">{saved?.message}</p>
      <details id="saved-verify-details" hidden={!saved?.result}><summary>Full proof verification result</summary><pre id="saved-verify-report">{saved?.result ? json(saved.result) : ''}</pre></details>
    </details>
    <details className="privacy-check report" ref={privacy}><summary id="privacy-check">How to check that your files stay on this device</summary>
      <ol><li>Open your browser’s developer tools and Network panel. Wait for the engine to say “Ready”, then clear the request list. Drop two files, compare, sign or verify a saved proof: no requests appear.</li><li>Or wait for “Ready”, turn the network off in developer tools, and compare two files. It still works.</li><li>Reload with the network on. In Network, select the page request and inspect its response headers for <code>Content-Security-Policy</code>.</li></ol>
      <p>The page loads its scripts, document engine and images from this site before it is ready. Its Content Security Policy blocks fetch, XHR, WebSockets, beacons and form submissions. File operations run locally.</p>
    </details>
    <details className="limits"><summary>What this browser edition covers · local files, no uploads</summary><p>DOCX, XLSX and PPTX, using Milleleaf’s existing format adapters and loss inventory. All apps are treated equally: compare the file, whoever made the software.</p><p>ODF conversion needs a LibreOffice subprocess and is not in this browser build. Native BoringSSL and PDF writers are also omitted because they cannot run in this WebAssembly build. This page compares files; it does not render document pages or export a replacement document.</p><p>Large or complex files can take longer or exceed browser memory. Signature verification proves the integrity of the signed layer, not that the inventory detected every possible change.</p></details>
  </div>
}
