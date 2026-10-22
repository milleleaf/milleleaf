import { useEffect, useRef, useState } from 'react'
import evidence from './data/hero.json'

type Segment = { kind: string; text: string }
type Block = { kind: string; text?: string; rows?: string[][] }
type Frame = { label: string; app: string; sha256: string; blocks: Block[]; paragraphDiffs: Segment[][]; rewrites: { part: string }[]; report: { changed?: string[]; conflicts?: { path: string; message: string }[]; losses?: { feature: string; count: number }[] } }
type Snapshot = { source: { columns: { name: string }[]; rows: [string, number][] }; report: { blocks: Block[]; sha256: string }; deck: { rows: string[][]; chart: { categories: string[]; values: number[] }; sha256: string }; checks: string[] }
type HeroData = { fixture: string; flow?: { route: string; fixture: string; before: Snapshot; after: Snapshot; scope: string }; frames: Frame[]; layerBytes: string; signed: { signer: string; at: string; layer: string; verification: string; exitCode: number; signature: { key: string; signature: string } } }
export const hero = evidence as unknown as HeroData

function FlowTable({ rows, label }: { rows: string[][]; label: string }) {
  return <div className="document-table-wrap"><table className="document-table" aria-label={label}><tbody>{rows.map((row, r) => <tr key={r}>{row.map((cell, c) => r === 0 ? <th scope="col" key={c}>{cell}</th> : <td key={c}>{cell}</td>)}</tr>)}</tbody></table></div>
}

function FlowChart({ chart }: { chart: Snapshot['deck']['chart'] }) {
  const max = Math.max(...chart.values)
  return <figure className="flow-chart"><figcaption>Revenue by region (AUD)</figcaption><ul>{chart.categories.map((category, i) => <li key={category}><span>{category}</span><span className="flow-bar-track"><span className="flow-bar" style={{ width: `${chart.values[i] / max * 100}%` }} /></span><strong>${chart.values[i].toLocaleString('en-AU')}</strong></li>)}</ul></figure>
}

export function ContentFlow() {
  const [updated, setUpdated] = useState(false)
  const flow = hero.flow
  if (!flow) return <Comparison />
  const snapshot = updated ? flow.after : flow.before
  const sourceRows = [snapshot.source.columns.map(c => c.name), ...snapshot.source.rows.map(([region, value]) => [region, `$${value.toLocaleString('en-AU')}`])]
  const reportRows = snapshot.report.blocks.find(b => b.kind === 'table')?.rows || []
  const summary = snapshot.report.blocks.filter(b => b.kind === 'paragraph').slice(1).map(b => b.text).join(' ')
  return <section className="content-flow" aria-label="One source across a mix of apps">
    <div className="flow-heading"><div><p className="eyebrow">1. One table and chart</p><h2>The workbook is the source.</h2><p>2. The report and deck use it in their own style.</p></div><button className="flow-update" type="button" aria-pressed={updated} onClick={() => setUpdated(!updated)}>{updated ? 'Show the original numbers' : '3. Apply the real Calc edit'}<span aria-hidden="true"> ↻</span></button></div>
    <p className="flow-route">Calc workbook → Word report file via WordCraft + DeckCraft deck</p>
    <div className="flow-state" role="status">{updated ? 'Calc edit applied. Perth is added and Singapore changes. Both exports follow.' : 'Original workbook. The report table and deck chart share these numbers.'}</div>
    <div className="flow-presentations">
      <article className="flow-source document-sheet"><header><span>Workbook source</span><span>{updated ? 'Calc edit' : 'GridCraft source'}</span></header><h3>Q3 revenue</h3><FlowTable rows={sourceRows} label="Workbook source revenue"/></article>
      <article className="flow-report document-sheet"><header><span>Word report file</span><span>WordCraft</span></header><h3>Q3 results</h3><p>{summary}</p><FlowTable rows={reportRows} label="Report linked revenue table"/></article>
      <article className="flow-deck document-sheet"><header><span>Deck</span><span>DeckCraft</span></header><h3>Revenue by region</h3><FlowTable rows={snapshot.deck.rows} label="Deck linked revenue table"/><FlowChart chart={snapshot.deck.chart}/></article>
    </div>
    <p className="flow-scope">Real hello-table exports, shown as content previews. The workbook supplies the table; the linked chart is in the deck. The report carries the table. Refresh today uses CLI export. <a href="/evidence/hero.json">Inspect the data and checks ↗</a></p>
    <div className="flow-return"><p className="eyebrow">4. An edit comes back</p><h2>A wording edit is reviewable. A typed-over link is a conflict.</h2><p>The real LibreOffice report edit below changes the text and a linked value. The source value is retained.</p></div>
    <Comparison />
  </section>
}

function DocumentSheet({ frame, original = false }: { frame: Frame; original?: boolean }) {
  let paragraph = -1
  const oldTable = hero.frames[0].blocks.find(b => b.kind === 'table')?.rows || []
  return <article className={`document-sheet ${original ? 'original-sheet' : 'current-sheet'}`} aria-label={original ? 'Original document' : 'Compared document'}>
    <header><span>{original ? 'Original' : frame.label}</span><span>{frame.app}</span></header>
    {frame.blocks.map((block, index) => {
      if (block.kind === 'paragraph') {
        paragraph += 1
        const parts = frame.paragraphDiffs[paragraph]
        return index === 0 ? <h3 key={index}>{block.text}</h3> : <p key={index} className="document-paragraph">{original ? block.text : parts?.map((part, n) => part.kind === 'added' ? <ins key={n}>{part.text}</ins> : part.kind === 'removed' ? <del key={n}>{part.text}</del> : <span key={n}>{part.text}</span>)}</p>
      }
      return <div key={index} className="document-table-wrap"><table className="document-table" aria-label="Revenue by region (AUD)"><tbody>{block.rows?.map((row, r) => <tr key={r}>{row.map((cell, c) => r === 0 ? <th scope="col" key={c}>{cell}</th> : <td key={c} className={!original && oldTable[r]?.[c] !== cell ? 'conflict-cell' : undefined}>{!original && oldTable[r]?.[c] !== cell ? <><del>{oldTable[r]?.[c]}</del> <strong>{cell}</strong><span className="cell-note">Linked value changed in file. Source retained on import.</span></> : cell}</td>)}</tr>)}</tbody></table></div>
    })}
    {!original && frame.rewrites.some(r => r.part === 'word/styles.xml') && <p className="rewrite-note"><span aria-hidden="true">↺</span> Style definitions rewritten <code>styles.xml</code></p>}
    {!original && <p className="loss-note">{frame.report.losses === undefined ? 'Source export. No import report for this step.' : frame.report.losses.length ? frame.report.losses.map(l => `${l.count} ${l.feature} not carried`).join('; ') : 'No additional inventoried loss reported.'}</p>}
  </article>
}

export function Comparison() {
  const [selected, setSelected] = useState(1)
  const revisions = useRef<HTMLElement>(null)
  const verifier = useRef<HTMLElement>(null)
  useEffect(() => {
    const element = revisions.current!
    let active = true
    const choose = (event: Event) => setSelected((event as CustomEvent<{ index: number }>).detail.index)
    element.addEventListener('revision-select', choose)
    void customElements.whenDefined('ml-revisions').then(() => {
      if (active) (element as HTMLElement & { options: unknown }).options = { labels: ['Original', 'Edited in LibreOffice', 'Re-saved in LibreOffice', 'Signed layer'], selected: 1 }
    })
    void customElements.whenDefined('ml-verify').then(() => {
      if (active && verifier.current) (verifier.current as HTMLElement & { options: unknown }).options = { bytes: hero.layerBytes, ...hero.signed.signature, layer: hero.signed.layer }
    })
    return () => { active = false; element.removeEventListener('revision-select', choose) }
  }, [])
  const frame = hero.frames[Math.min(selected, 2)]
  return <section className="comparison-stack" aria-label="Real report round trip">
    <div className="comparison-toolbar"><div><strong>Q3 Board Report</strong><span>Real fixture comparison</span></div><a href="/evidence/hero.json">Inspect the JSON ↗</a></div>
    <ml-revisions ref={revisions} />
    <noscript><p className="no-script-note">Showing the original and LibreOffice edit. Revision controls and browser verification need JavaScript.</p></noscript>
    <div className="comparison-legend"><span><i className="added-key" /> Text changed</span><span><i className="conflict-key" /> Linked value conflict</span><span>Select and inspect the actual content below.</span></div>
    <div className="document-pair"><DocumentSheet frame={hero.frames[0]} original /><DocumentSheet frame={frame} /></div>
    <aside className={`signature-receipt ${selected === 3 ? 'selected-receipt' : ''}`} aria-label="Signed import layer">
      <div className="verified-seal" aria-hidden="true">✓</div><div><strong>Layer verified by mleaf</strong><p>{hero.signed.signer} <span>·</span> <time dateTime={hero.signed.at}>{hero.signed.at.replace('T', ' ').replace('Z', ' UTC')}</time></p><code className="layer-hash">{hero.signed.layer}</code><p className="signature-scope">Fixture test key. Hybrid signatures checked at build time. Linked overrides stay flagged, not applied. Signing time is not a trusted time-stamp.</p></div><ml-verify ref={verifier} />
    </aside>
  </section>
}
