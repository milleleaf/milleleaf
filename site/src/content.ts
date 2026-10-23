export type ContentPage = {
  path: string;
  title: string;
  kicker: string;
  intro: string;
  sections: { title: string; body: string[]; links?: { label: string; href: string }[] }[];
};

export const pages: ContentPage[] = [
  { path: '/standards', title: 'Standards, with the checks beside them.', kicker: 'Standards', intro: 'What Milleleaf reads, writes and uses. Each claim stops at the check named here. No certification or blanket conformance is claimed.', sections: [] },
  {
    path: '/how-it-works', title: 'One source. Every presentation.', kicker: 'How it works',
    intro: 'Make content in the app that suits it. Use it across reports, workbooks, decks and PDFs, in each file’s own style. Edits come back for review.',
    sections: [
      { title: 'One figure, several presentations', body: ['A figure can appear in a workbook, report and deck. Each presentation follows the data and keeps its own style.', 'Microsoft Office, LibreOffice and the ArtCraft apps use the same layered source. The fixture above shows a real Calc workbook edit flowing through the WordCraft and DeckCraft writers.'], links: [{ label: 'Current support', href: '/status' }] },
      { title: 'Why not just open it in another app?', body: ['Or copy/paste, import/export, or a plugin? The answers below separate plain copy behaviour, findings in our tests and reported vendor or community statements. Reported does not mean tested by Milleleaf.'], links: [{ label: 'Files, versions and sources', href: 'https://github.com/milleleaf/milleleaf/blob/main/examples/real/SOURCES.md' }] },
      { title: 'Why not copy and paste?', body: ['Paste behaviour: you get a copy. When the source numbers change, that copy stays as it was. Formatting is converted into what the destination accepts. The paste itself provides no continuing source history, and edits in the copy do not flow back. This describes a plain paste, not Paste Link.'] },
      { title: 'Why not Paste Link or linked objects?', body: ['Reported: Microsoft Office OLE, LibreOffice DDE/OLE and Google linked charts refresh linked content within their own suites. Our sources describe one-way links, not edits returning from the copy.', 'Reported for file-based Office and LibreOffice links: absolute paths can break when files move or are renamed. This is not a claim about Google file paths. Google reports that updating a linked object overwrites edits made in the copy, and tables over 400 cells paste unlinked.', 'These are vendor and community statements recorded in the competitor research, not comparison tests we ran. Microsoft also offers destination styles; we have not tested a general claim that Paste Link breaks formatting.'] },
      { title: 'Why not import and export?', body: ['Conversion behaviour: Save As and converters make a one-time conversion. Conversion alone leaves no continuing link to the source, so it supplies no route for edits to come back.', 'Found in our tests: Word re-measured auto-fit tables 2 and 6 in paper.docx. Excel resized charts after a Standard Width change in lca-model.xlsx. LibreOffice saved three charts in that workbook without their data; Milleleaf retained their source versions and listed it.', 'Each app writes its own representation on save. Rewrites are not automatically losses. These findings cover the named Office 16 and LibreOffice 24.2 files, not every converter or file.'], links: [{ label: 'File sources and fixture versions', href: 'https://github.com/milleleaf/milleleaf/blob/main/examples/real/SOURCES.md' }] },
      { title: 'Why not a plugin or add-in?', body: ['Reported: think-cell, Macabacus and UpSlide refresh content well inside Microsoft Office. They require app installation and per-seat licensing, often paid. The 18 October research records Macabacus at about $200–360 per user per year, think-cell at roughly €189–$327 from third-party estimates, and UpSlide by quote. These are reported prices, not current quotes or tests.', 'Reported scope: the sources describe Microsoft Office support, not LibreOffice, the ArtCraft apps or your own software. Edits flowing back and cross-app loss reporting are not documented in those sources. That is not proof that they cannot exist.', 'Deployment constraint: managed computers may not permit installing an add-in. Check the organisation’s policy. This is a possible restriction, not a finding from our file tests.'] },
      { title: 'What Milleleaf adds', body: ['Built approach: Milleleaf works on the files themselves through places the file standards allow. Nothing is installed into Word, Excel, PowerPoint, LibreOffice or the ArtCraft apps. Your own software can open the same standard files. A mix of apps uses one source; the format and adapter still set what is carried.', 'Found in our tests: the real Calc edit feeds a Word report file through WordCraft and a DeckCraft deck. Imported edits become changes to review. Typing over a linked value becomes a conflict instead of overwriting the source. Inventoried losses are listed on the checked files.', 'Found in our tests: the fixture change has a signed layer, and its hash and signature can be checked above. Layers record every imported change; signing is optional today. A signature identifies the signing key, not a person’s identity or approval.', 'Current limits from the plan: refresh uses CLI export, with no one-click refresh inside apps today. Review approvals and shared stores are planned. The mixed-route statuses above remain the boundary of what has been checked.'], links: [{ label: 'Standards and allowed identity markers', href: '/standards' }, { label: 'Found in our tests: flow and signed-layer evidence', href: '/evidence/hero.json' }, { label: 'Built and planned work', href: '/status' }] },
      { title: 'Review the change where it happened', body: ['Stable IDs connect an exported file to its source revision. Edits return as a new layer; changes to linked values become conflicts.', 'When an app removes an ID, matching may be a best guess. The import report says so. It also lists unsupported features. Fixture checks fail on an unreported inventoried loss.'], links: [{ label: 'Round-trip dataset plan', href: '/data/round-trip' }] },
      { title: 'Check the layer and its assets', body: ['Layers and pictures are stored by content hash. Optional detached signatures verify signed layers and their stored assets against a key. They do not establish that the content is correct.', 'Hybrid post-quantum signatures and optional time-stamps are built. Projects choose their time-stamp services. RFC 3161 checking verifies the imprint and time, but does not validate the authority’s signature in Rust.'] },
      { title: 'Standard places for identity', body: ['Identity markers use content-control tags, defined names, shape names and custom properties, all allowed by OOXML. The files stay ordinary office files and open normally. Standards do not guarantee that every app preserves a marker; checks measure survival per app.'], links: [{ label: 'Standards and their checks', href: '/standards' }] },
      { title: 'Start in a local folder', body: ['The CLI and libraries work locally today. Shared-folder collaboration, peer-to-peer sync, a browser workspace and the wider review workflow are planned. Several machines sharing a store are not yet proven.'], links: [{ label: 'Getting started', href: '/docs/getting-started' }, { label: 'Standards and checks', href: '/standards' }] },
    ],
  },
  {
    path: '/data', title: 'Follow the data to its source.', kicker: 'Data',
    intro: 'Explore WorldViews, inspect the dataset cards, or follow the planned office-file round-trip dataset.',
    sections: [
      { title: 'Two maps, different terms', body: ['WorldViews Libre uses openly licensed data. WorldViews adds credited data under other terms. Each dataset keeps its own licence.'], links: [{ label: 'About WorldViews', href: '/data/worldviews' }, { label: 'Explore WorldViews Libre', href: 'https://worldviews-libre.pages.dev' }, { label: 'Explore WorldViews', href: 'https://worldviews.pages.dev' }] },
      { title: 'Cards, pipelines and mirrors', body: ['Hugging Face is the main home for the data, GitHub holds the pipelines and Kaggle the mirrors. The tables separate open licences from other terms.'], links: [{ label: 'Datasets and their terms', href: '/data/datasets' }] },
      { title: 'What survives another save?', body: ['Planned: source files, app re-saves and reports of what each app kept, changed or dropped. The public round-trip dataset is not available yet.'], links: [{ label: 'Round-trip dataset plan', href: '/data/round-trip' }] },
    ],
  },
  {
    path: '/data/round-trip', title: 'What survives the save?', kicker: 'Planned dataset',
    intro: 'A planned public record of what office apps keep, change and lose as a file moves between them.',
    sections: [
      { title: 'The file beside the finding', body: ['The dataset will pair source files, app re-saves and per-file loss reports. The public dataset and swapped examples are still release work; there is no download yet.'] },
      { title: 'Evidence so far', body: ['The repo has round-trip fixtures for Word, Excel and PowerPoint 16 for Mac and LibreOffice, plus inventory checks on real reports, workbooks and decks.', 'These checks cover those files and app versions. A public comparison needs the files and reports alongside it.'], links: [{ label: 'Current support and limits', href: '/status' }] },
      { title: 'Contribute a re-save', body: ['The aim is to include more apps, including your own software. Use files you may publish. Contact hello@milleleaf.com to discuss a contribution.'], links: [{ label: 'How loss reporting works', href: '/how-it-works' }] },
    ],
  },
  {
    path: '/data/worldviews', title: 'Real places. Sources in view.', kicker: 'WorldViews',
    intro: 'Two interactive maps backed by public pipelines and dataset cards.',
    sections: [
      { title: 'WorldViews Libre', body: ['Only openly licensed data. Each card names its licence and attribution requirements.'], links: [{ label: 'Explore WorldViews Libre', href: 'https://worldviews-libre.pages.dev' }] },
      { title: 'WorldViews', body: ['The same map with extra credited data under other terms. Those datasets are listed separately from the open licences.', 'The World Heritage view is not affiliated with or endorsed by UNESCO.'], links: [{ label: 'Explore WorldViews', href: 'https://worldviews.pages.dev' }, { label: 'World Heritage view', href: 'https://worldviews.pages.dev?view=heritage' }] },
      { title: 'Inspect before reusing', body: ['Hugging Face holds the data, GitHub the pipelines and Kaggle the mirrors. Check each dataset’s card for its terms.'], links: [{ label: 'Datasets and their terms', href: '/data/datasets' }, { label: 'Hugging Face', href: 'https://huggingface.co/milleleaf' }, { label: 'GitHub', href: 'https://github.com/milleleaf' }, { label: 'Kaggle', href: 'https://www.kaggle.com/helenkwok' }] },
    ],
  },
  {
    path: '/data/datasets', title: 'Datasets and their terms.', kicker: 'Datasets',
    intro: 'Hugging Face is the main home for the data; GitHub holds the pipelines and Kaggle the mirrors. Every dataset has its own terms.',
    sections: [],
  },
  {
    path: '/method', title: 'Make the claim the evidence supports.', kicker: 'AI Output to Value',
    intro: 'One standard for human and AI work: judge the process and the evidence.',
    sections: [
      { title: 'Six rungs, six different claims', body: ['Access means having a tool. Output means producing something. A checked artifact can establish a Deliverable.', 'Operating capability needs repeated operation. Outcome needs a measured change for someone. Value needs evidence of what that change is worth.'] },
      { title: 'The claim on this site', body: ['A fixture supports a claim about that file and those app versions. A passing check supports the property it checks.', 'Milleleaf has checked projects and real-file fixtures. It has no measured user Outcome or Value to claim. Planned features stay labelled planned.'] },
      { title: 'Read the method', body: ['AI Output to Value keeps its own name and public home. It guides the tool, the pipelines and this site.'], links: [{ label: 'AI Output to Value', href: 'https://milleleaf.github.io' }, { label: 'Product status', href: '/status' }] },
    ],
  },
  {
    path: '/docs', title: 'Start with a file you can check.', kicker: 'Documentation',
    intro: 'Milleleaf is a local-first CLI and library project. Try the browser demos, or build it from source.',
    sections: [
      { title: 'Try the example', body: ['The browser demos need no install. The repository has a small example project, examples/hello-table, that runs the whole loop. There is no packaged release yet: build from source.'], links: [{ label: 'Getting started', href: '/docs/getting-started' }, { label: 'Demo', href: '/demo' }, { label: 'Source on GitHub', href: 'https://github.com/milleleaf/milleleaf' }] },
      { title: 'Understand the file’s journey', body: ['A project holds the layered source, assets and history. Export makes native files; import brings edits back. Reports describe matching and unsupported features.'], links: [{ label: 'How it works', href: '/how-it-works' }, { label: 'Support and limits', href: '/status' }] },
      { title: 'Reference to come', body: ['A CLI reference and project-format documentation are planned. Until then, the repository README and schema/mleaf.tsp describe them.', 'Questions: hello@milleleaf.com. Security reports: security@milleleaf.com.'] },
    ],
  },
  {
    path: '/docs/getting-started', title: 'Try it. Inspect what comes back.', kicker: 'Getting started',
    intro: 'Try the browser demos first. To run the CLI, build it from source: the repository README lists what it needs.',
    sections: [
      { title: 'Begin with a small project', body: ['The example project, examples/hello-table, puts the same figures in a report, workbook and deck. Its run.sh walks the whole loop. With your own files, use files you may share.'], links: [{ label: 'Demo', href: '/demo' }, { label: 'Source on GitHub', href: 'https://github.com/milleleaf/milleleaf' }] },
      { title: 'Edit in your app', body: ['Checked routes include Word, Excel and PowerPoint 16 for Mac and LibreOffice through the ArtCraft adapters.', 'The mleaf formats command reports available routes and missing tools.'] },
      { title: 'Inspect the imported layer', body: ['Read conflicts, best-guess matches and listed losses before accepting a result. Keep the original file: checks cover specific fixtures, and some features remain unsupported.'], links: [{ label: 'Support limits', href: '/status' }, { label: 'Layers and reports', href: '/how-it-works' }] },
    ],
  },
  {
    path: '/status', title: 'Built. Checked. Still to come.', kicker: '23 October 2026',
    intro: 'In development. These results cover example projects and real-file fixtures, not production readiness or measured user outcomes.',
    sections: [
      { title: 'Built: source and history', body: ['Layers, provenance, derived values, example charts and PDF packs. The revision store provides history, blame, checkpoints and content-addressed assets.', 'Optional hybrid post-quantum signatures, RFC 3161 and OpenTimestamps support are built. Projects choose their time-stamp services.'] },
      { title: 'Checked: native-file round trips', body: ['Example reports, workbooks and decks have passed through Word, Excel and PowerPoint 16 for Mac and LibreOffice. ODF uses LibreOffice. Import reports separate certain matches from best guesses.', 'Inventory checks fail on an unlisted inventoried loss. Pictures, deck tables, connectors and selected links are carried. Unchanged workbook charts retain their XML; selected edits are patched, while other chart edits warn and use the writer. DocLang example exports pass its official validator.'] },
      { title: 'Known limits', body: ['Lost and reported: deck and Word charts, pictures inside cells, OLE objects, conditional formats and rich text inside workbook cells. Workbook charts with shapes or embedded workbooks, and chart-style gallery parts, also have limits.', 'Transition, animation and sound fixtures are incomplete. Fuller ODF loss checks are planned. Signatures establish integrity against a key, not content accuracy. RFC 3161 checking verifies imprint and time, but does not validate the authority’s signature in Rust.', 'Local operation is built. Several machines sharing a store and a production review workflow are not proven. The round-trip dataset is not released yet.'] },
      { title: 'Planned', body: ['Review rules and approvals; archival PDF validation; shared folders and peer-to-peer sync; more ArtCraft app links; charts across reports and decks; PDF and scan import; a browser workspace.', 'PDF extraction remains a spike. PDF/A and other conformance claims need the relevant validator to pass; none is claimed here.'], links: [{ label: 'How we make claims', href: '/method' }, { label: 'Demo', href: '/demo' }] },
    ],
  },
  {
    path: '/privacy', title: 'We collect nothing about you.', kicker: 'Privacy',
    intro: 'No accounts, no telemetry, no crash reports, no analytics and no update check. Your files stay where you keep them.',
    sections: [
      { title: 'The tool and libraries', body: ['Your files stay on your machine or wherever your project lives. Milleleaf needs no server of its own and sends your files nowhere. LibreOffice and the ArtCraft apps run on your machine as local programs.', 'The network is used only when you ask for it, and only to services you choose: a time-stamp authority or OpenTimestamps calendar you name (only a hash is sent), and public Bitcoin block headers for online verification. Each reveals your IP address to the service you contact. There is no default time-stamp service.'], links: [{ label: 'The full data policy', href: 'https://github.com/milleleaf/milleleaf/blob/main/DATA-POLICY.md' }] },
      { title: 'This website', body: ['The site is static files served by Cloudflare, which handles each request (your IP address and browser details) to deliver the page. Cloudflare request logging and Web Analytics are off. Cloudflare still shows us aggregate counts (how many requests, their status codes and which of our addresses), not who made them. The site loads no third-party scripts, fonts or trackers.', 'If you pick a light or dark theme, your browser remembers it (localStorage, milleleaf-theme). It never leaves your browser.'] },
      { title: 'The demos', body: ['Files you drop are read and processed on your own device by the document engine, running as WebAssembly. The demo pages block every network connection after the page loads, so a file cannot be sent anywhere. Automated checks load a demo, go offline and run every operation, with zero network requests after loading.'], links: [{ label: 'Demo', href: '/demo' }] },
      { title: 'Questions and changes', body: ['Questions about privacy: hello@milleleaf.com.', 'Any change to this policy is made in the repository, so its history is public.'] },
    ],
  },
  {
    path: '/about', title: 'Useful tools. Evidence in view.', kicker: 'About Milleleaf',
    intro: 'An office tool, public data pipelines, WorldViews and the AI Output to Value method.',
    sections: [
      { title: 'Content can move', body: ['Milleleaf is being built to move content between apps with reviewable changes and a report of what could not be kept.', 'Microsoft Office, LibreOffice, the ArtCraft apps and your own software are equals. The broader review workflow remains planned.'], links: [{ label: 'How it works', href: '/how-it-works' }, { label: 'Status', href: '/status' }] },
      { title: 'Open work, individual terms', body: ['Public pipelines, dataset cards and both WorldViews maps are available now. AI Output to Value keeps its own public home.', 'The office tool is free and open source under MIT OR Apache-2.0. Dataset licences are separate.'], links: [{ label: 'Source on GitHub', href: 'https://github.com/milleleaf/milleleaf' }, { label: 'Data and WorldViews', href: '/data' }, { label: 'Method', href: '/method' }] },
      { title: 'Contact', body: ['Questions and contributions: hello@milleleaf.com. Security reports: security@milleleaf.com.', 'For security reports, name the component and give reproduction steps without passwords, private files or other people’s data.'] },
    ],
  },
];
