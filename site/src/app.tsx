import { useEffect } from 'react'
import { StandardsTable } from './standards'
import { AlternativesTable } from './alternatives'
import { pages } from './content'
import { DatasetTables } from './dataset-tables'
import { Comparison, ContentFlow } from './hero'
import { CompareDemo } from './demos/compare'
import { FlowDemo } from './demos/flow'
import { OfficeDesk } from './office/office'

const nav = [['Demo', '/flow'], ['Office', '/office'], ['How it works', '/how-it-works'], ['Data', '/data'], ['Docs', '/docs'], ['Keystone', '/keystone'], ['GitHub', 'https://github.com/milleleaf']]
const slug = (s: string) => s.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '')

function Logo() {
  return <a className="brand" href="/" aria-label="Milleleaf home"><img src="/brand/milleleaf-mark.svg" alt="" width="40" height="40" /><span>Milleleaf</span></a>
}

function Header({ path }: { path: string }) {
  const links = nav.map(([label, href]) => <a key={href} href={href} aria-current={path === href || (href === '/data' && path.startsWith('/data/')) || (href === '/flow' && path === '/demo') ? 'page' : undefined}>{label}</a>)
  return <header className="site-header wrap"><Logo /><nav className="desktop-nav" aria-label="Main navigation">{links}</nav><div className="header-tools"><ml-theme /></div><details className="mobile-menu"><summary>Menu</summary><nav aria-label="Mobile navigation">{links}<a href="/method">Method</a><a href="/about">About</a></nav></details></header>
}

function Footer() {
  return <footer className="site-footer"><div className="wrap footer-main"><Logo /><p>One source.<br />A leaf for every change.</p><nav aria-label="Footer"><a href="/how-it-works">How it works</a><a href="/standards">Standards</a><a href="/status">Status</a><a href="/method">Method</a><a href="/keystone">Keystone</a><a href="/about">About</a><a href="/privacy">Privacy</a><a href="/about#open-work-individual-terms">Licence</a><a href="/.well-known/security.txt">Security</a></nav><div className="footer-contact"><a href="mailto:hello@milleleaf.com">hello@milleleaf.com</a><a href="mailto:security@milleleaf.com">security@milleleaf.com</a></div></div><div className="wrap footer-bottom"><span>Office tool: MIT OR Apache-2.0. <a href="https://github.com/milleleaf/milleleaf">Source on GitHub</a>.</span><span>Data has its own terms. <a href="/data/datasets">Read the register.</a></span></div></footer>
}

// The two browser demos: the site renders their page; each demo is one component.
const demos = {
  '/flow': { label: 'Make it once: linked content', title: <>Make it once.<br />Use it in every app.</>, lead: 'A table and chart, shared by a report and a deck. Change the source and both stay current, each in its own style. Bring an edit back from any app and see what changed.', demo: <FlowDemo /> },
  '/demo': { label: 'Follow the file: compare versions', title: <>Follow the file.<br />See what changed.</>, lead: 'Choose two versions. See what happened to your file, wherever it was edited.', demo: <CompareDemo /> },
} as const
type DemoPath = keyof typeof demos

function DemoPage({ path }: { path: DemoPath }) {
  const page = demos[path]
  return <main id="main" className="wrap content-page page-demo">
    <nav className="demo-switch" aria-label="Demos">{(Object.keys(demos) as DemoPath[]).map(p => <a key={p} href={p} aria-current={p === path ? 'page' : undefined}>{demos[p].label}</a>)}</nav>
    <header className="page-heading"><span className="page-topic">Demo</span><h1>{page.title}</h1><p className="lead">{page.lead}</p></header>
    {page.demo}
  </main>
}

function Home() {
  return <main id="main">
    <section className="product-hero wrap"><div className="hero-heading"><h1>Make it once.<br />Use it in every app.</h1><div><p>One table or chart, in the workbook, report, deck and PDF. Each keeps its own style. When the numbers change, every use follows. Edits come back for review.</p><a href="/flow" className="button">Try it in your browser <span aria-hidden="true">↗</span></a><a href="/office" className="button secondary" style={{ marginLeft: 12 }}>Open the office desk <span aria-hidden="true">↗</span></a></div></div><ContentFlow /></section>
    <section className="story-band alternatives-band"><div className="wrap"><div className="story-layout"><h2>Why not just open it in another app?</h2><div><p>Or copy/paste, import/export, or a plugin? Each does a useful job. Here is where each stops.</p><p>“Reported” means vendor or community statements in our research, not a test by Milleleaf. Paste and conversion behaviour describe a copy, not a linked workflow.</p></div></div><AlternativesTable /><p className="alternatives-limit">Milleleaf needs no app add-in. Refresh uses CLI export today; one-click refresh inside apps, review approvals and shared stores are still missing or planned. Signing is optional today.</p><a href="/how-it-works#why-not-just-open-it-in-another-app">Read each answer and its evidence →</a></div></section>
    <AppFamilies />
    <section className="wrap open-work"><h2>Open the work.</h2><div className="open-tool"><span>The office tool</span><h3>Built to be inspected.</h3><p>The CLI and libraries work locally, and the demos run in your browser. The source is public.</p><a href="https://github.com/milleleaf/milleleaf">Source on GitHub ↗</a></div><div className="open-data sheet-surface"><span>The data</span><h3>Real places.<br />Individual terms.</h3><a href="https://worldviews-libre.pages.dev">WorldViews Libre ↗</a><a href="https://worldviews.pages.dev">WorldViews ↗</a><a href="/data/datasets">Dataset register →</a></div><div className="open-method"><span>The method</span><p>One standard for human and AI work. Make the claim the evidence supports.</p><a href="/method">AI Output to Value →</a></div><div className="open-method"><span>The proof</span><p>Keystone, an openBIM ecosystem, won 1st place in That Open’s #TOP Founding Members Hackathon.</p><a href="/keystone">Keystone →</a></div></section>
    <section className="wrap release-note"><h2>Working fixtures.<br />Still in development.</h2><div><p>Layers, signed history and selected round trips are built. Review approvals, shared stores and the browser workspace are planned.</p><a className="button secondary" href="/status">Read the status →</a></div></section>
  </main>
}

function AppChecks() {
  return <ul className="app-checks"><li>Microsoft Office: checked with real Word/Excel/PowerPoint 16 files</li><li>LibreOffice: checked with LibreOffice re-saves and edits</li><li>ArtCraft apps: files written through WordCraft, GridCraft and DeckCraft</li></ul>
}

function AppFamilies() {
  return <section className="wrap app-families" aria-label="Any app in, any app out"><h2>Any app in. Any app out.</h2><p>Today: Microsoft Office, LibreOffice and the ArtCraft apps, through DOCX, XLSX, PPTX, ODT, ODS and ODP.</p><p>One project mixes apps from Microsoft Office, LibreOffice and the ArtCraft apps at the same time. Every app is a place where the content is made or shown.</p><div>{[
    ['A Calc workbook', 'feeds a Word report and a DeckCraft deck', 'Checked: the example above, with the real CLI'],
    ['An Excel workbook', 'feeds a LibreOffice Writer report and a PowerPoint deck', 'Being checked in the browser flow'],
    ['An edit made in WordCraft', 'comes back while the deck was last saved by Impress', 'Being checked in the browser flow'],
  ].map(([from, to, status]) => <article key={from}><h3>{from}</h3><p>{to}</p><p className="dataset-note">{status}</p></article>)}</div><AppChecks /><a href="/standards">Formats, standards and checked limits →</a></section>
}

function DataSheets() {
  return <div className="data-sheets"><article className="map-sheet libre-sheet"><span>Open licences</span><h2>WorldViews<br />Libre</h2><p>Real places, from openly licensed data.</p><a href="https://worldviews-libre.pages.dev">Explore the map ↗</a></article><article className="map-sheet world-sheet"><span>Extra sources, individual terms</span><h2>WorldViews</h2><p>The same map, with additional credited data.</p><a href="https://worldviews.pages.dev">Explore the map ↗</a></article></div>
}

function MethodLadder() {
  const rungs = [['Access', 'You have the tool.'], ['Output', 'Something was produced.'], ['Deliverable', 'The artifact was checked.'], ['Operating capability', 'It works repeatedly.'], ['Outcome', 'Someone sees a measured change.'], ['Value', 'That change has evidenced worth.']]
  return <ol className="evidence-ladder" aria-label="Evidence rungs">{rungs.map(([name, meaning]) => <li key={name}><strong>{name}</strong><span>{meaning}</span></li>)}</ol>
}

// Email addresses in page text become mailto links.
function Mail({ text }: { text: string }) {
  return <>{text.split(/([\w.+-]+@milleleaf\.com)/).map((part, i) => i % 2 ? <a key={i} href={`mailto:${part}`}>{part}</a> : part)}</>
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
      {path === '/standards' && <StandardsTable />}
      {path === '/how-it-works' && <><ContentFlow /><AppChecks /></>}
      {(path === '/data/round-trip') && <Comparison />}
      {(path === '/docs' || path === '/docs/getting-started') && <div className="command-sheet"><span>Inspect the available routes</span><code>mleaf formats</code><p>CLI command. Public installation instructions are coming with the release.</p></div>}
      <div className="page-leaves">{page.sections.map(section => <section id={slug(section.title)} key={section.title} className="page-leaf"><h2>{section.title}</h2><div>{section.body.map(p => <p key={p}><Mail text={p} /></p>)}{section.figure && <a className="page-figure" href={section.figure.href}><picture><source media="(prefers-reduced-motion: reduce)" srcSet={section.figure.still} /><img src={section.figure.gif} alt={section.figure.alt} width={960} height={540} loading="lazy" /></picture></a>}{section.links && <div className="article-links">{section.links.map(l => <a href={l.href} key={l.href}>{l.label} <span aria-hidden="true">{l.href.startsWith('https:') ? '↗' : '→'}</span></a>)}</div>}</div></section>)}</div>
    </>}
  </main>
}

function OfficePage() {
  return <main id="main" className="wrap content-page page-office">
    <header className="page-heading"><span className="page-topic">Office</span><h1>Thirteen editors.<br />Nothing to install.</h1><p className="lead">Real documents, spreadsheets, slides, PDFs, drawings, photos, vector art, layouts, interface designs, video, motion graphics and audio, running in your browser on your device. Nothing is uploaded: the page may only talk to this site.</p></header>
    <OfficeDesk />
  </main>
}

export function App({ path }: { path: string }) {
  useEffect(() => { void import('./elements').then(m => m.registerElements()) }, [])
  return <><a className="skip-link" href="#main">Skip to content</a><Header path={path} />{path === '/' ? <Home /> : path === '/office' ? <OfficePage /> : path in demos ? <DemoPage path={path as DemoPath} /> : <Content path={path} />}<Footer /></>
}
