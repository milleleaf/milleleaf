import { useEffect, useRef, useState } from 'react'
import evidence from './data/hero.json'

type Segment = { kind: string; text: string }
type Block = { kind: string; text?: string; rows?: string[][] }
type Frame = { label: string; app: string; sha256: string; blocks: Block[]; paragraphDiffs: Segment[][]; rewrites: { part: string }[]; report: { changed?: string[]; conflicts?: { path: string; message: string }[]; losses?: { feature: string; count: number }[] } }
type HeroData = { fixture: string; frames: Frame[]; layerBytes: string; signed: { signer: string; at: string; layer: string; verification: string; exitCode: number; signature: { key: string; signature: string } } }
export const hero = evidence as unknown as HeroData

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
      <div className="verified-seal" aria-hidden="true">✓</div><div><strong>Layer verified by ox</strong><p>{hero.signed.signer} <span>·</span> <time dateTime={hero.signed.at}>{hero.signed.at.replace('T', ' ').replace('Z', ' UTC')}</time></p><code className="layer-hash">{hero.signed.layer}</code><p className="signature-scope">Fixture test key. Hybrid signatures checked at build time. Linked overrides stay flagged, not applied. Signing time is not a trusted time-stamp.</p></div><ml-verify ref={verifier} />
    </aside>
  </section>
}
