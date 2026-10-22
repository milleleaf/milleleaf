export type ContentPage = {
  path: string;
  title: string;
  kicker: string;
  intro: string;
  sections: { title: string; body: string[]; links?: { label: string; href: string }[] }[];
};

export const pages: ContentPage[] = [
  {
    path: '/how-it-works', title: 'One source. Changes you can review.', kicker: 'How Milleleaf works',
    intro: 'Make content wherever it is easiest. Milleleaf keeps a layered source, writes native files and brings edits back with a report.',
    sections: [
      { title: 'Content and presentation have different jobs', body: ['A figure can appear in a workbook, a report and a deck. Each presentation inherits the data and carries its own style. The example project demonstrates this with linked tables and a chart.', 'Microsoft Office, LibreOffice, the Craft apps and your own software are places to make and show content. No app has to be the centre. Support depends on the file format and adapter; the Status page names the current limits.'] },
      { title: 'Edits come back as layers', body: ['A layer records one author’s changes. Stable IDs connect exported content to the project. Import turns edits into a new layer, based on the revision the file came from.', 'If an edit changes a linked value, it becomes a conflict to review. Where an editor removes an ID, matching can be a best guess; the import report flags it.'] },
      { title: 'See what a round trip could not keep', body: ['Import and export reports list features the adapter does not carry. Checks on the real-file fixtures fail when an inventoried loss is missing from the report.', 'This is evidence for the checked files and features. It is not a promise that every feature in every file is preserved. Deck and Word charts, for example, are currently lost and reported.'], links: [{ label: 'Read the current limits', href: '/status' }, { label: 'The planned round-trip dataset', href: '/data/round-trip' }] },
      { title: 'A history with signatures', body: ['Layers are stored by hash, with provenance and optional detached signatures. Pictures are stored by their content hash too. Verification checks signed layers and their stored assets.', 'Hybrid post-quantum layer signatures and optional time-stamps are built. Projects choose their own time-stamp services; none is enabled by default. RFC 3161 checking verifies the imprint and time, but does not validate the authority’s signature in Rust.', 'A valid layer signature establishes integrity against a key; it does not establish that a figure is correct. The wider review workflow remains planned.'] },
      { title: 'Any storage, starting with a local folder', body: ['The CLI and libraries work with a local project folder today. You do not need to move your files to a new cloud.', 'Shared-folder collaboration, peer-to-peer sync and a browser workspace are planned. Several machines editing a shared store are not yet a proven operating capability.'], links: [{ label: 'Getting started', href: '/docs/getting-started' }] },
    ],
  },
  {
    path: '/data', title: 'Data you can inspect.', kicker: 'Milleleaf Data',
    intro: 'Explore real places with WorldViews, find public dataset cards, and follow the planned round-trip dataset for office files.',
    sections: [
      { title: 'What an app keeps and loses', body: ['The round-trip dataset is planned. It will pair files with re-saves and reports showing what each app kept, changed or dropped. The public dataset is not available yet.'], links: [{ label: 'Round-trip dataset plan', href: '/data/round-trip' }] },
      { title: 'Two WorldViews maps', body: ['WorldViews Libre uses only openly licensed data. WorldViews adds sources without an open licence, republished with credit. The separation is about licence hygiene; the maps keep open and non-open data apart.'], links: [{ label: 'About WorldViews', href: '/data/worldviews' }, { label: 'Open WorldViews Libre', href: 'https://worldviews-libre.pages.dev' }, { label: 'Open WorldViews', href: 'https://worldviews.pages.dev' }] },
      { title: 'The dataset card is the source', body: ['Milleleaf publishes data on Hugging Face, pipeline code on GitHub, and mirrors and history on Kaggle. Each dataset has its own terms. Public access alone does not make a dataset open.'], links: [{ label: 'Open datasets and licence boundaries', href: '/data/datasets' }, { label: 'Hugging Face', href: 'https://huggingface.co/milleleaf' }, { label: 'GitHub pipelines', href: 'https://github.com/milleleaf' }, { label: 'Kaggle mirrors', href: 'https://www.kaggle.com/helenkwok' }] },
    ],
  },
  {
    path: '/data/round-trip', title: 'What survives the save?', kicker: 'Round-trip dataset · planned',
    intro: 'A planned public record of what office apps keep, change and lose when the same file passes through them.',
    sections: [
      { title: 'Files alongside the findings', body: ['The intended dataset pairs source files, app re-saves and per-file loss reports. It will let you inspect a result against the file that produced it.', 'The public dataset and swapped example files are still release work. There is no download to offer yet.'] },
      { title: 'What the project has checked so far', body: ['The project has round-trip fixtures for Word, Excel and PowerPoint 16 for Mac and LibreOffice, plus inventory checks for real reports, workbooks and decks.', 'Those checks establish deliverables for particular files. They do not establish compatibility with every file or every app version. A public comparison needs its files and reports alongside it.'], links: [{ label: 'What works today', href: '/status' }] },
      { title: 'Contribute a re-save', body: ['When the dataset is published, the aim is to include re-saves from more apps, including your own software. Use files you are allowed to publish.', 'For now, contact hello@milleleaf.com to discuss a contribution.'], links: [{ label: 'How loss reporting works', href: '/how-it-works' }] },
    ],
  },
  {
    path: '/data/worldviews', title: 'Real places. Sources in view.', kicker: 'WorldViews',
    intro: 'Two interactive world maps, rebuilt by Milleleaf’s public data pipelines, with a clear boundary between open and non-open sources.',
    sections: [
      { title: 'WorldViews Libre', body: ['An interactive world map built only from openly licensed data. The public pipelines rebuild its sources; the dataset cards carry the specific licence and attribution requirements.'], links: [{ label: 'Explore WorldViews Libre', href: 'https://worldviews-libre.pages.dev' }] },
      { title: 'WorldViews', body: ['The same map plus sources without an open licence, republished with credit. These sources are kept separate from the openly licensed data.', 'WorldViews is not affiliated with or endorsed by UNESCO.'], links: [{ label: 'Explore WorldViews', href: 'https://worldviews.pages.dev' }, { label: 'World Heritage view', href: 'https://worldviews.pages.dev?view=heritage' }] },
      { title: 'Follow the sources', body: ['Data lives on Hugging Face, code on GitHub, and mirrors and history on Kaggle. Check the individual card before reusing a dataset.'], links: [{ label: 'Dataset cards', href: 'https://huggingface.co/milleleaf' }, { label: 'Pipeline code', href: 'https://github.com/milleleaf' }, { label: 'Kaggle mirrors', href: 'https://www.kaggle.com/helenkwok' }] },
    ],
  },
  {
    path: '/data/datasets', title: 'Open data, with its terms attached.', kicker: 'Datasets',
    intro: 'Start with each dataset’s own card. It records the sources and terms you need to understand before reuse.',
    sections: [
      { title: 'Find the data and the pipeline', body: ['Milleleaf’s public datasets cover world, Hong Kong and South Australian sources. Hugging Face is the canonical home for the data; GitHub holds the pipelines. Kaggle carries mirrors and history under titles beginning “Milleleaf:”.', 'A collection does not have one blanket licence. Follow each card for its exact licence, source credits and conditions.'], links: [{ label: 'Browse dataset cards on Hugging Face', href: 'https://huggingface.co/milleleaf' }, { label: 'Browse public pipelines on GitHub', href: 'https://github.com/milleleaf' }, { label: 'Browse Kaggle mirrors', href: 'https://www.kaggle.com/helenkwok' }] },
      { title: 'Hong Kong: transport and flights', body: ['hk-transit: hk-government-open-data.', 'hk-roads: data-gov-hk-terms-and-conditions; road centreline data also carries CSDI terms.', 'hk-airport-flights: data-gov-hk-terms-of-use.'], links: [{ label: 'hk-transit card', href: 'https://huggingface.co/datasets/milleleaf/hk-transit' }, { label: 'hk-roads card', href: 'https://huggingface.co/datasets/milleleaf/hk-roads' }, { label: 'hk-airport-flights card', href: 'https://huggingface.co/datasets/milleleaf/hk-airport-flights' }] },
      { title: 'Hong Kong: buildings and property', body: ['hk-buildings: hk-csdi-terms-of-use.', 'hk-property: DATA.GOV.HK terms; the area shapes carry CSDI terms. Read the card’s source-specific conditions.'], links: [{ label: 'hk-buildings card', href: 'https://huggingface.co/datasets/milleleaf/hk-buildings' }, { label: 'hk-property card', href: 'https://huggingface.co/datasets/milleleaf/hk-property' }] },
      { title: 'World: buildings and places', body: ['world-buildings: ODbL-1.0 for Overture regions. Official Hong Kong regions retain CSDI terms. Check the licence for the region you use.', 'world-places: terms vary by source, including Taiwan Open Government Data License, SPC terms, ÅSUB CC BY 4.0, INSEE Licence Ouverte 2.0 and Jersey OGL-J v1.', 'world-gazetteer: CC0-1.0.'], links: [{ label: 'world-buildings card', href: 'https://huggingface.co/datasets/milleleaf/world-buildings' }, { label: 'world-places card', href: 'https://huggingface.co/datasets/milleleaf/world-places' }, { label: 'world-gazetteer card', href: 'https://huggingface.co/datasets/milleleaf/world-gazetteer' }] },
      { title: 'World: airports, flights and heritage', body: ['world-airports: public domain, from OurAirports.', 'world-flights: ODbL-1.0.', 'world-heritage: CC0-1.0 for sites; ODbL-1.0 for areas. These are separate files with separate terms.', 'The World Heritage view is not affiliated with or endorsed by UNESCO.'], links: [{ label: 'world-airports card', href: 'https://huggingface.co/datasets/milleleaf/world-airports' }, { label: 'world-flights card', href: 'https://huggingface.co/datasets/milleleaf/world-flights' }, { label: 'world-heritage card', href: 'https://huggingface.co/datasets/milleleaf/world-heritage' }] },
      { title: 'South Australia: transit and planning', body: ['sa-transit: CC BY 4.0.', 'sa-planning: the card metadata says CC BY 4.0. Its note for the 2015 ZIP identifies CC BY 3.0 Australia. Check the terms attached to the particular source file.'], links: [{ label: 'sa-transit card', href: 'https://huggingface.co/datasets/milleleaf/sa-transit' }, { label: 'sa-planning card', href: 'https://huggingface.co/datasets/milleleaf/sa-planning' }] },
      { title: 'Non-open sources: separate terms', body: ['world-building-green: world-bank-noncommercial-attribution. Its noncommercial terms are not an open licence.', 'world-places-nc: per-source-no-open-licence. Sources are republished with credit; public access does not make them open.'], links: [{ label: 'world-building-green card', href: 'https://huggingface.co/datasets/milleleaf/world-building-green' }, { label: 'world-places-nc card', href: 'https://huggingface.co/datasets/milleleaf/world-places-nc' }] },
      { title: 'sa-fire: classification needs review', body: ['The current sa-fire card metadata says CC BY 4.0, with a note identifying CC BY 3.0 Australia for the 2015 ZIP. The site plan places sa-fire outside the open collection.', 'That difference needs review. This site keeps sa-fire separate and does not classify it as open while the source terms and collection boundary are reconciled.'], links: [{ label: 'Read the current sa-fire card', href: 'https://huggingface.co/datasets/milleleaf/sa-fire' }] },
      { title: 'Explore before downloading', body: ['WorldViews Libre shows the openly licensed collection. WorldViews adds credited, non-open sources while keeping the collections separate.', 'The World Heritage view is not affiliated with or endorsed by UNESCO.'], links: [{ label: 'WorldViews Libre', href: 'https://worldviews-libre.pages.dev' }, { label: 'WorldViews', href: 'https://worldviews.pages.dev' }] },
    ],
  },
  {
    path: '/method', title: 'Claims need evidence.', kicker: 'AI Output to Value',
    intro: 'One standard for human and AI work: judge the process and the evidence, and make only the claim that the evidence supports.',
    sections: [
      { title: 'From access to value', body: ['The method distinguishes six rungs: Access, Output, Deliverable, Operating capability, Outcome and Value.', 'Having a tool is Access. Producing something is Output. A checked artifact can establish a Deliverable. Repeated operation needs evidence of an Operating capability. An Outcome needs a measured change for someone; Value needs evidence of what that change is worth.'] },
      { title: 'What that means here', body: ['A round-trip fixture is evidence about that file, through those app versions. A passing check is evidence for the property it checks. Neither establishes that every user’s files will work.', 'Milleleaf has checked example projects and real-file fixtures. It has no measured user Outcome or Value to claim. Planned features are labelled as planned.'] },
      { title: 'Inspect the method', body: ['AI Output to Value keeps its own name and public home. The same approach guides the office tool, the data pipelines and the claims on this site.'], links: [{ label: 'Read AI Output to Value', href: 'https://milleleaf.github.io' }, { label: 'Read the product status', href: '/status' }] },
    ],
  },
  {
    path: '/docs', title: 'Start with a file you can check.', kicker: 'Documentation',
    intro: 'Milleleaf is a local-first CLI and library project today. Public installation instructions will accompany its first release.',
    sections: [
      { title: 'Getting started', body: ['The first release will begin with the browser demo and a small example project. The office tool’s repository is not public yet; there is no public install command to run today.'], links: [{ label: 'Getting started', href: '/docs/getting-started' }, { label: 'Demo', href: '/demo' }] },
      { title: 'Understand the model', body: ['A project contains a layered source, its assets and revision history. Export makes native files; import brings edits back as changes you can inspect. Reports describe matching and features the adapter cannot keep.'], links: [{ label: 'How it works', href: '/how-it-works' }, { label: 'Current support and limits', href: '/status' }] },
      { title: 'Documentation to come', body: ['The public release will include installation steps, CLI reference and project-format documentation. These are not published here yet.', 'For questions, contact hello@milleleaf.com; for security reports, security@milleleaf.com.'] },
    ],
  },
  {
    path: '/docs/getting-started', title: 'Try it. Inspect what comes back.', kicker: 'Getting started',
    intro: 'The browser demo is coming with the first release. The CLI and its public installation instructions will follow the release of the office repository.',
    sections: [
      { title: '1. Begin with the example', body: ['The intended first step is a small project whose figures appear in a report, workbook and deck. Start with a copy of a file you can inspect and are allowed to share.', 'The public example and download are still being prepared.'], links: [{ label: 'Demo availability', href: '/demo' }] },
      { title: '2. Edit in the app that suits you', body: ['The tested routes include Microsoft Word, Excel and PowerPoint 16 for Mac and LibreOffice, through the Craft adapters. Available routes depend on the format and tools installed.', 'Milleleaf’s ox formats command reports which routes are available and why others are unavailable. The CLI is not publicly downloadable yet.'] },
      { title: '3. Read the changes and the losses', body: ['Import creates an edit layer and a report. Check conflicts, best-guess matches and listed losses before accepting the result.', 'Keep the original file. The present checks cover specific examples and fixtures, and some features remain unsupported.'], links: [{ label: 'Read the support limits', href: '/status' }, { label: 'How layers and reports work', href: '/how-it-works' }] },
    ],
  },
  {
    path: '/status', title: 'Built, checked, still to come.', kicker: 'Status · 22 October 2026',
    intro: 'Milleleaf is in development. The evidence below covers example projects and real-file fixtures; it does not establish production readiness or user outcomes.',
    sections: [
      { title: 'Built: layers and native-file round trips', body: ['The layered model, provenance, derived values, example charts and PDF packs are built. A revision store provides history, blame, checkpoints, optional hybrid post-quantum signatures and content-addressed assets. Optional RFC 3161 and OpenTimestamps support is built; projects choose their services.', 'Example reports, workbooks and decks have been round-tripped through Microsoft Word, Excel and PowerPoint 16 for Mac and LibreOffice. ODF has a route through LibreOffice. Import reports distinguish certain matches from best guesses.'] },
      { title: 'Built: reporting and selected preservation', body: ['Inventory checks compare real reports, workbooks and decks with their re-exports and fail on an unlisted inventoried loss. Pictures, deck tables, connectors and selected links are carried.', 'Workbook chart XML is kept for unchanged charts and selected edits. Some chart edits fall back to the writer and produce a warning. DocLang export is built and checked with the official validator for its example outputs.'] },
      { title: 'Known limits', body: ['Deck and Word charts, pictures inside cells, OLE objects, conditional formats and rich text inside workbook cells are still lost and reported. Workbook charts with shapes or embedded workbooks, and chart-style gallery parts, have limits too.', 'Transition, animation and sound fixtures are not complete. The fuller ODF loss-checking work is planned. A signature proves integrity against a key, not the accuracy of the content. RFC 3161 verification checks the imprint and time, but does not validate the authority’s signature in Rust.', 'Local-folder operation is built. Several machines sharing a store, and a production review workflow, are not yet proven. The office repository, browser demo and round-trip dataset are not publicly released.'] },
      { title: 'Planned', body: ['Review rules and approvals; archival PDF validation; shared-folder collaboration and peer-to-peer sync; links to more Craft apps; charts across reports and decks; PDF and scan import; and a browser workspace.', 'PDF extraction has been explored, but a spike is not a released import route. PDF/A and other conformance claims require the relevant validator to pass; none is claimed here.'], links: [{ label: 'How we make claims', href: '/method' }, { label: 'Demo availability', href: '/demo' }] },
    ],
  },
  {
    path: '/about', title: 'Useful tools. Evidence in view.', kicker: 'About Milleleaf',
    intro: 'Milleleaf brings together an office tool, public data pipelines, WorldViews and the AI Output to Value method.',
    sections: [
      { title: 'Make content where it belongs', body: ['The office tool is called Milleleaf. It is being built to let content move between apps with reviewable changes and a report of what could not be kept.', 'Microsoft Office, LibreOffice, the Craft apps and your own software are equals in that approach. The current release work starts with the routes already checked, while the broader workflow remains planned.'], links: [{ label: 'How Milleleaf works', href: '/how-it-works' }, { label: 'Current status', href: '/status' }] },
      { title: 'Open work, with clear boundaries', body: ['Public data pipelines and dataset cards are available now. WorldViews and WorldViews Libre keep their own names, as does AI Output to Value.', 'The office tool is intended to be free and open source under MIT OR Apache-2.0. Its repository is still private while the public release is prepared. Dataset licences are separate and must be checked on each card.'], links: [{ label: 'Public GitHub organisation', href: 'https://github.com/milleleaf' }, { label: 'Data and WorldViews', href: '/data' }, { label: 'AI Output to Value', href: '/method' }] },
      { title: 'Contact and security', body: ['For questions and contributions: hello@milleleaf.com. For security reports: security@milleleaf.com.', 'For a security report, describe the affected component and a way to reproduce the issue without including passwords, private files or other people’s data.'] },
    ],
  },
];
