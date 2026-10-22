import { useEffect, useRef, useState } from 'react'
import { pages } from './content'
import { DatasetTables } from './dataset-tables'
import { Comparison } from './hero'

const nav = [['Demo', '/demo'], ['How it works', '/how-it-works'], ['Data', '/data'], ['Docs', '/docs'], ['GitHub', 'https://github.com/milleleaf']]
const slug = (s: string) => s.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '')

function Logo() {
  return <a className="brand" href="/" aria-label="Milleleaf home"><img src="/brand/milleleaf-mark.svg" alt="" width="40" height="40" /><span>Milleleaf</span></a>
}

function Header({ path }: { path: string }) {
  const links = nav.map(([label, href]) => <a key={href} href={href} aria-current={path === href || (href === '/data' && path.startsWith('/data/')) ? 'page' : undefined}>{label}</a>)
  return <header className="site-header wrap"><Logo /><nav className="desktop-nav" aria-label="Main navigation">{links}</nav><div className="header-tools"><ml-theme /></div><details className="mobile-menu"><summary>Menu</summary><nav aria-label="Mobile navigation">{links}<a href="/method">Method</a><a href="/about">About</a></nav></details></header>
}

function Footer() {
  return <footer className="site-footer"><div className="wrap footer-main"><Logo /><p>One source.<br />A leaf for every change.</p><nav aria-label="Footer"><a href="/how-it-works">How it works</a><a href="/status">Status</a><a href="/method">Method</a><a href="/about">About</a><a href="/about#open-work-individual-terms">Licence</a><a href="/.well-known/security.txt">Security</a></nav><div className="footer-contact"><span>hello@milleleaf.com</span><span>security@milleleaf.com</span></div></div><div className="wrap footer-bottom"><span>Office tool: MIT OR Apache-2.0. Public release in preparation.</span><span>Data has its own terms. <a href="/data/datasets">Read the register.</a></span></div></footer>
}

function DemoHost() {
  const ref = useRef<HTMLElement>(null)
  const [ready, setReady] = useState(false)
  useEffect(() => {
    const element = ref.current!
    let active = true
    const onReady = () => setReady(true)
    element.addEventListener('demo-ready', onReady)
    void customElements.whenDefined('ml-demo').then(() => {
      if (active) (element as HTMLElement & { options: unknown }).options = { src: '/demo/app/index.html', title: 'Milleleaf in-browser demo' }
    })
    return () => { active = false; element.removeEventListener('demo-ready', onReady) }
  }, [])
  return <><div className="demo-placeholder" hidden={ready}><span className="availability">Coming with the first release</span><p>Live file upload is being prepared. For now, inspect this real fixture result.</p><Comparison /></div><ml-demo ref={ref} /></>
}

function Home() {
  return <main id="main">
    <section className="product-hero wrap"><div className="hero-heading"><h1>See the save.<br />Keep the source.</h1><div><p>A real report, edited in LibreOffice.<br />Text changes, a cell conflict, and a signed layer.</p><a href="/demo" className="button">Try the demo <span aria-hidden="true">↗</span></a></div></div><Comparison /></section>
    <section className="story-band"><div className="wrap story-layout"><h2>The edit is a leaf.<br />The source stays yours.</h2><div><p>The summary changed. A linked number was overwritten in the file. Milleleaf imports the text as a layer and flags the number as a conflict.</p><p>This is evidence for this fixture, not every file. Microsoft Office, LibreOffice, the Craft apps and your software all have a place here.</p><a href="/how-it-works">Follow the file through Milleleaf →</a></div></div></section>
    <section className="wrap open-work"><h2>Open the work.</h2><div className="open-tool"><span>The office tool</span><h3>Built to be inspected.</h3><p>The CLI and libraries work locally. The public repository and browser demo are in preparation.</p><a href="https://github.com/milleleaf">Public GitHub organisation ↗</a></div><div className="open-data sheet-surface"><span>The data</span><h3>Real places.<br />Individual terms.</h3><a href="https://worldviews-libre.pages.dev">WorldViews Libre ↗</a><a href="https://worldviews.pages.dev">WorldViews ↗</a><a href="/data/datasets">Dataset register →</a></div><div className="open-method"><span>The method</span><p>One standard for human and AI work. Make the claim the evidence supports.</p><a href="/method">AI Output to Value →</a></div></section>
    <section className="wrap release-note"><h2>Working fixtures.<br />Still in development.</h2><div><p>Layers, signed history and selected round trips are built. Review approvals, shared stores and the browser workspace are planned.</p><a className="button secondary" href="/status">Read the status →</a></div></section>
  </main>
}

function DataSheets() {
  return <div className="data-sheets"><article className="map-sheet libre-sheet"><span>Open licences</span><h2>WorldViews<br />Libre</h2><p>Real places, from openly licensed data.</p><a href="https://worldviews-libre.pages.dev">Explore the map ↗</a></article><article className="map-sheet world-sheet"><span>Extra sources, individual terms</span><h2>WorldViews</h2><p>The same map, with additional credited data.</p><a href="https://worldviews.pages.dev">Explore the map ↗</a></article></div>
}

function MethodLadder() {
  const rungs = [['Access', 'You have the tool.'], ['Output', 'Something was produced.'], ['Deliverable', 'The artifact was checked.'], ['Operating capability', 'It works repeatedly.'], ['Outcome', 'Someone sees a measured change.'], ['Value', 'That change has evidenced worth.']]
  return <ol className="evidence-ladder" aria-label="Evidence rungs">{rungs.map(([name, meaning]) => <li key={name}><strong>{name}</strong><span>{meaning}</span></li>)}</ol>
}

function Content({ path }: { path: string }) {
  const page = pages.find(p => p.path === path)!
  const parent = path.startsWith('/data/') ? ['Data', '/data'] : path.startsWith('/docs/') ? ['Docs', '/docs'] : null
  const isData = path.startsWith('/data')
  return <main id="main" className={`wrap content-page page-${path.slice(1).replaceAll('/', '-')} ${isData ? 'data-page' : ''}`}>
    {parent && <nav className="breadcrumbs" aria-label="Breadcrumb"><a href="/">Home</a><span aria-hidden="true">/</span><a href={parent[1]}>{parent[0]}</a><span aria-hidden="true">/</span><span aria-current="page">{page.kicker}</span></nav>}
    <header className="page-heading"><span className="page-topic">{page.kicker}</span><h1>{page.title}</h1><p className="lead">{page.intro}</p></header>
    {path === '/data/datasets' ? <DatasetTables /> : <>
      {(path === '/data' || path === '/data/worldviews') && <DataSheets />}
      {path === '/method' && <MethodLadder />}
      {(path === '/how-it-works' || path === '/data/round-trip') && <Comparison />}
      {(path === '/docs' || path === '/docs/getting-started') && <div className="command-sheet"><span>Inspect the available routes</span><code>ox formats</code><p>CLI command. Public installation instructions are coming with the release.</p></div>}
      <div className="page-leaves">{page.sections.map(section => <section id={slug(section.title)} key={section.title} className="page-leaf"><h2>{section.title}</h2><div>{section.body.map(p => <p key={p}>{p}</p>)}{section.links && <div className="article-links">{section.links.map(l => <a href={l.href} key={l.href}>{l.label} <span aria-hidden="true">{l.href.startsWith('https:') ? '↗' : '→'}</span></a>)}</div>}</div></section>)}</div>
    </>}
  </main>
}

export function App({ path }: { path: string }) {
  useEffect(() => { void import('./elements').then(m => m.registerElements()) }, [])
  return <><a className="skip-link" href="#main">Skip to content</a><Header path={path} />{path === '/' ? <Home /> : path === '/demo' ? <main id="main" className="wrap content-page page-demo"><header className="page-heading"><span className="page-topic">Demo</span><h1>Your file.<br />Its next leaf.</h1><p className="lead">See an actual fixture result now. Bring your own file when the live demo arrives.</p></header><DemoHost /></main> : <Content path={path} />}<Footer /></>
}
