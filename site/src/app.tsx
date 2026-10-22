import { useEffect, useRef, useState } from 'react'
import { pages } from './content'

const github = 'https://github.com/milleleaf'
const nav = [['Demo', '/demo'], ['How it works', '/how-it-works'], ['Data', '/data'], ['Docs', '/docs'], ['GitHub', github]]
const slug = (s: string) => s.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '')

function Logo() {
  return <a className="brand" href="/" aria-label="Milleleaf home"><img src="/brand/milleleaf-mark.svg" alt="" width="36" height="36" /><span>Milleleaf</span></a>
}

function Header({ path }: { path: string }) {
  const links = nav.map(([label, href]) => <a key={href} href={href} aria-current={path === href ? 'page' : undefined}>{label}</a>)
  return <header className="site-header wrap"><Logo /><nav className="desktop-nav" aria-label="Main navigation">{links}</nav><div className="header-tools"><ml-theme /><a className="button small" href="/demo">Try the demo <span aria-hidden="true">↗</span></a></div><details className="mobile-menu"><summary>Menu</summary><nav aria-label="Mobile navigation">{links}<a href="/method">Method</a><a href="/about">About</a></nav></details></header>
}

function Footer() {
  return <footer className="site-footer"><div className="wrap footer-grid"><div><Logo /><p>Open tools and data.<br />Evidence before claims.</p><span className="mono">Made to be inspected.</span></div><div><h2>Product</h2><a href="/demo">Demo</a><a href="/how-it-works">How it works</a><a href="/status">Status</a><span>Apps · planned</span><span>Blog · planned</span></div><div><h2>Open</h2><a href={github}>GitHub</a><a href="https://huggingface.co/milleleaf">Hugging Face</a><a href="https://www.kaggle.com/helenkwok">Kaggle</a><a href="/data/round-trip">Round-trip dataset · planned</a><a href="https://worldviews-libre.pages.dev">WorldViews Libre</a><a href="https://worldviews.pages.dev">WorldViews</a></div><div><h2>Milleleaf</h2><a href="/method">Method</a><a href="/about">About</a><a href="/about#open-work-with-clear-boundaries">Licence</a><a href="/about#contact-and-security">Security</a><p className="contact">hello@milleleaf.com</p><p className="contact">security@milleleaf.com</p></div></div><div className="wrap footer-bottom"><span>Office tool: MIT OR Apache-2.0 · public release in preparation</span><span>Dataset licences vary. Read each card.</span></div></footer>
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
      if (active) (element as HTMLElement & { options: { src: string; title: string } }).options = { src: '/demo/app/index.html', title: 'Milleleaf in-browser demo' }
    })
    return () => { active = false; element.removeEventListener('demo-ready', onReady) }
  }, [])
  return <><div className="demo-placeholder" hidden={ready}><span className="status-label">Coming with the first release</span><h2>A file out.<br />A record back.</h2><p>The in-browser demo is being prepared. It will let you inspect a file’s round trip without installing the CLI.</p><ol className="demo-steps"><li><span>01</span> Open an example</li><li><span>02</span> Edit in your app</li><li><span>03</span> See the changes and losses</li></ol><a className="text-link" href="/status">Read what works today <span aria-hidden="true">→</span></a></div><ml-demo ref={ref} /><noscript><p>The demo needs JavaScript. Its availability message and the rest of this site work without it.</p></noscript></>
}

function Home() {
  return <main id="main"><section className="wrap hero"><div className="hero-copy"><p className="eyebrow"><span className="dot" /> Milleleaf / office tool</p><h1>Make content once.<br /><span>See what<br className="desktop-break" /> comes back.</span></h1><p className="lead">A figure in a report, a workbook and a deck. Keep one source, each file’s own style, and a record of the changes.</p><div className="actions"><a className="button" href="/demo">Try the demo <span aria-hidden="true">↗</span></a><a className="text-link" href="/how-it-works">How it works <span aria-hidden="true">→</span></a></div><p className="caption">In development. Browser demo coming with the first release.</p></div><div className="hero-demo"><div className="panel-heading"><span className="mono">01 / THE ROUND TRIP</span><span className="panel-dot" /></div><div className="file-path"><span>Native file</span><span aria-hidden="true">→</span><span>Your app</span><span aria-hidden="true">→</span><span>Review</span></div><DemoHost /></div></section>
    <section className="app-strip"><div className="wrap"><span className="mono">YOUR APP HAS A PLACE HERE</span><p>Microsoft Office <span>·</span> LibreOffice <span>·</span> the Craft apps <span>·</span> your own software</p><a href="/status">See tested routes →</a></div></section>
    <section className="wrap section split"><div><p className="eyebrow">02 / One source, many presentations</p><h2>Same figure.<br />Your own style.</h2><p>A presentation inherits the data and carries its own formatting. An edit comes back as a layer you can inspect, rather than silently replacing newer work.</p><a className="text-link" href="/how-it-works">Follow a change through the project →</a></div><div className="illustration"><p className="mono">ILLUSTRATION / NOT A FILE COMPATIBILITY TEST</p><ml-layers /><noscript><p>Workbook, report and deck: each presents the same source figure, 42,800, in its own style.</p></noscript></div></section>
    <section className="section proof-section"><div className="wrap"><div className="section-heading"><div><p className="eyebrow">03 / Evidence, with boundaries</p><h2>What survived the save?</h2></div><a className="text-link" href="/data/round-trip">The dataset plan →</a></div><p className="measure">Project fixtures have been through Microsoft Office 16 for Mac and LibreOffice. These are checked deliverables for specific files, not a guarantee for every document.</p><div className="evidence-table"><table><caption>Selected findings from the project’s round-trip fixtures</caption><thead><tr><th scope="col">Content</th><th scope="col">Checked result</th><th scope="col">Boundary</th></tr></thead><tbody><tr><th scope="row">Linked tables</th><td><span className="tag">Changes traced</span></td><td>Example report, workbook and deck</td></tr><tr><th scope="row">Workbook charts</th><td><span className="tag">XML retained</span></td><td>Unchanged charts and selected edits; other edits warn</td></tr><tr><th scope="row">Adopted Word & deck charts</th><td><span className="tag warning">Loss reported</span></td><td>Preservation still planned</td></tr></tbody></table></div><p className="caption">The public files and per-file comparison dataset are planned. <a href="/status">Read the current limits.</a></p></div></section>
    <section className="wrap section"><p className="eyebrow">04 / A record you can inspect</p><div className="feature-grid">{[
      ['01', 'Losses in view', 'Reports name the features an adapter cannot keep. The fixture inventory checks for unlisted losses.'],
      ['02', 'Signed history', 'Changes have authors and hashes. Optional signatures check integrity against a key.'],
      ['03', 'Your storage', 'Start in a local folder. Shared-folder collaboration and peer-to-peer sync are planned.'],
    ].map(([n, title, body]) => <article key={n}><span className="feature-number">{n}</span><h2>{title}</h2><p>{body}</p><a href="/how-it-works">How it works <span aria-hidden="true">↗</span></a></article>)}</div></section>
    <section className="open-section"><div className="wrap section"><p className="eyebrow">05 / More from Milleleaf</p><h2>Open to inspection.</h2><div className="open-grid"><article><h3>The tool</h3><p>Free and open source is the plan. The office repository is being prepared for public release.</p><a href={github}>Public GitHub organisation ↗</a></article><article><h3>The data</h3><p>Explore real places. Follow the sources, pipelines and individual licence terms.</p><a href="/data">Data and dataset cards →</a><a href="https://worldviews-libre.pages.dev">WorldViews Libre ↗</a><a href="https://worldviews.pages.dev">WorldViews ↗</a></article><article><h3>The method</h3><p>AI Output to Value: one standard for human and AI work, judged by process and evidence.</p><a href="/method">Read the method →</a></article></div></div></section>
    <section className="wrap section status-strip"><div><p className="eyebrow">In development / 22 October 2026</p><h2>Working examples.<br />A wider workflow ahead.</h2></div><div><p>Layers, revision history, selected round trips and PDF packs are built. Review approvals, shared storage and the browser workspace are still planned.</p><a className="button secondary" href="/status">Read the status <span aria-hidden="true">→</span></a></div></section>
  </main>
}

function DemoPage() {
  return <main id="main" className="wrap content-page"><p className="eyebrow">Milleleaf / demo</p><h1>A round trip,<br />in your browser.</h1><p className="lead measure">Open a file, edit it in the app that suits you, then inspect what changed and what could not be kept.</p><div className="demo-page-host"><DemoHost /></div><div className="note"><h2>What to expect</h2><p>The demo is being built separately. This page will load it here when it is available. Today, the CLI and libraries have checked example projects; the browser demo is not released.</p><a className="text-link" href="/how-it-works">Learn about layers and loss reports →</a></div></main>
}

function Content({ path }: { path: string }) {
  const page = pages.find(p => p.path === path)!
  const parent = path.startsWith('/data/') ? ['Data', '/data'] : path.startsWith('/docs/') ? ['Docs', '/docs'] : null
  return <main id="main" className="wrap content-page">{parent && <nav className="breadcrumbs" aria-label="Breadcrumb"><a href="/">Home</a><span aria-hidden="true">/</span><a href={parent[1]}>{parent[0]}</a><span aria-hidden="true">/</span><span aria-current="page">{page.kicker.replace(' · planned', '')}</span></nav>}<div className="page-heading"><p className="eyebrow">{page.kicker}</p><h1>{page.title}</h1><p className="lead">{page.intro}</p></div><div className="article-layout"><aside><nav aria-label="On this page"><span className="mono">ON THIS PAGE</span>{page.sections.map((s, i) => <a href={'#' + slug(s.title)} key={s.title}><span>{String(i + 1).padStart(2, '0')}</span>{s.title}</a>)}</nav></aside><article className="article-body">{page.sections.map((s, i) => <section id={slug(s.title)} key={s.title}><span className="section-index mono">{String(i + 1).padStart(2, '0')}</span><h2>{s.title}</h2>{s.body.map(p => <p key={p}>{p}</p>)}{s.links && <div className="article-links">{s.links.map(l => <a href={l.href} key={l.href}>{l.label} <span aria-hidden="true">{l.href.startsWith('https:') ? '↗' : '→'}</span></a>)}</div>}</section>)}</article></div></main>
}

export function App({ path }: { path: string }) {
  useEffect(() => { void import('./elements').then(m => m.registerElements()) }, [])
  return <><a className="skip-link" href="#main">Skip to content</a><Header path={path} />{path === '/' ? <Home /> : path === '/demo' ? <DemoPage /> : <Content path={path} />}<Footer /></>
}
