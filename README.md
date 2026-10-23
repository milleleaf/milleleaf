# Milleleaf

**Make it once. Use it in every app.** One table or chart, in the workbook, report, deck and PDF. Each keeps its own style. When the numbers change, every use follows. Edits come back for review.

Website, demos and status: [milleleaf.com](https://milleleaf.com).

## Any app in. Any app out.

Today: Microsoft Office, LibreOffice and the ArtCraft apps, through DOCX, XLSX, PPTX, ODT, ODS and ODP.

Microsoft Office, LibreOffice, the ArtCraft apps ([WordCraft](https://github.com/storytold/wordcraft), [GridCraft](https://github.com/storytold/gridcraft), [DeckCraft](https://github.com/storytold/deckcraft), [PdfCraft](https://github.com/storytold/pdfcraft)) and your own software are equals. One project can mix them: a Calc workbook can feed a Word report and a DeckCraft deck. Every app is a place where the content is made or shown.

Milleleaf works on the files themselves, through the places the file standards allow (content-control tags, defined names, shape names, custom properties). Nothing is installed into Word, Excel, PowerPoint, LibreOffice or the ArtCraft apps, and the files stay ordinary office files.

- **One layered source.** A project is a stack of layers composed into one model. Each app reads and writes its own part of shared nodes, and every value knows which layer set it.
- **Edits come back for review.** An edited file imports as a new layer. Typing over a linked value becomes a conflict instead of overwriting the source. The import report says which matches are certain and which are best guesses, and lists what the format could not carry.
- **History you can check.** Layers and pictures are stored by content hash, with history, blame and checkpoints. Signatures (optional, hybrid post-quantum) and time-stamps verify a layer against a key; they don't establish that its content is correct.
- **HTML in and out.** An agent's HTML deck, written to a restricted profile (`deck/0`), comes in as a reviewable proposal layer, and the model renders back to HTML that re-imports onto the same nodes.

## Status: in development

Layers, signed history and selected round trips are built and checked on example projects and real-file fixtures: Word, Excel and PowerPoint 16 for Mac, LibreOffice (ODF through LibreOffice), and the ArtCraft writers. That is not production readiness. Refresh uses the CLI today; there is no one-click refresh inside the apps. Review approvals, shared folders, peer-to-peer sync and a browser workspace are planned. Known limits and what is checked: [milleleaf.com/status](https://milleleaf.com/status).

The layered model follows the composition ideas of Pixar's Universal Scene Description (OpenUSD), as adopted by buildingSMART's IFC5. The export for AI tools follows the DocLang 0.8 specification (doclang.ai, Apache-2.0) and is checked with the DocLang validator. The schema and code are written independently; no USD, IFC5 or DocLang code or schema text is included.

## What's here

| Path | What |
|---|---|
| `schema/mleaf.tsp` | TypeSpec for the layer format; compiles to JSON Schema with `@typespec/compiler` and `@typespec/json-schema` 1.17.0 |
| `schema/office.mleaf.json` | Attribute schemas: `office::`, `grid::`, `word::`, `deck::`, `mleaf::` |
| `crates/milleleaf-core` | Load stacks, flatten, compose with provenance, validate, render HTML, DocLang export |
| `crates/milleleaf-cli` | The `mleaf` command: layers, validate, compose, render, pack, Office export and import, and the revision store |
| `crates/milleleaf-formats` | Every route in and out of each format, what it keeps, and the best one on this machine (`mleaf formats`) |
| `crates/milleleaf-store` | The revision store in a project's `.mleaf/` folder: commits, blame, checkpoints, signatures, sealed layers, search |
| `crates/milleleaf-wordcraft` | Composed document → DOCX through WordCraft, with node identity in content controls; edited DOCX → edit layer |
| `crates/milleleaf-gridcraft` | Composed workbook → recalculated XLSX through GridCraft, with node identity in defined names; edited XLSX → edit layer |
| `crates/milleleaf-deckcraft` | Composed deck → PPTX through DeckCraft, with native tables and charts drawn from the data; edited PPTX → edit layer |
| `crates/milleleaf-pack` | One snapshot → one bookmarked PDF (PdfCraft) of report, workbook and deck, with a page listing every layer the numbers came from |
| `crates/milleleaf-odf` | ODT, ODS and ODP, converted through LibreOffice, which keeps the identity markers |
| `crates/milleleaf-seal` | Sealed (encrypted) layers: HPKE with X-Wing, natively through BoringSSL and in pure Rust for the browser |
| `crates/milleleaf-headers` | Keeps the Bitcoin header file that `mleaf verify --online` checks time-stamps against |
| `crates/milleleaf-web` | The engine behind the website's demos, built for the browser |
| `tools/html2mleaf` | Node + Playwright: measure an HTML deck into a proposal layer and a loss report |
| `tools/inventory` | An independent count of what an Office file contains, to check that every loss is reported |
| `tools/local-tsa.mjs`, `tools/local-ots-calendar.mjs` | A local time-stamp authority and OpenTimestamps calendar, for tests without a network |
| `examples/hello-table` | Workbook → report → agent HTML deck → finance correction → edits in Word, Excel, PowerPoint and LibreOffice, end to end |
| `examples/real` | Real-world Office files (CC BY 4.0) and our own, with their sources in `SOURCES.md` |
| `site/` | The website, milleleaf.com |

## Try it

The demos on [milleleaf.com](https://milleleaf.com) run in your browser with nothing to install.

### Quickstart (about 5 minutes)

1. Download the archive for your system from [Releases](https://github.com/milleleaf/milleleaf/releases): Linux (x86_64), macOS (Apple silicon) or Windows (x86_64), and unpack it. It holds the `mleaf` command and the Hello Table example. To check that it was built by this repository's release workflow: `gh attestation verify <archive> --repo milleleaf/milleleaf`. Or build from source (below) and use `target/release/mleaf`.
2. Export the example's report and workbook (on Windows, use `..\..\mleaf.exe`):

```sh
cd examples/hello-table
../../mleaf validate project.mleaf.json
../../mleaf docx-export project.mleaf.json 3f1c2a90-0000-4c1e-9a00-000000000020 --out report.docx
../../mleaf xlsx-export project.mleaf.json 3f1c2a90-0000-4c1e-9a00-000000000010 --out finance.xlsx
```

`validate` composes the project's layers and reports 0 errors. `report.docx` has a table and two fields linked to the workbook; `finance.xlsx` is that workbook.

3. Open `report.docx` in Word, LibreOffice Writer or WordCraft. Reword a sentence, type a different number into the linked table, and save. Then bring it back:

```sh
../../mleaf docx-import project.mleaf.json 3f1c2a90-0000-4c1e-9a00-000000000020 report.docx --out my-edit.mleaf.json --author sam --id edit/sam
```

The reworded text comes back as a change in a new layer, `my-edit.mleaf.json`. The number typed over the linked table comes back as a conflict, because the workbook is its source. No editor at hand? Import `fixtures/report.libreoffice-edited.docx` instead, which has those edits.

4. `../../mleaf formats` lists the routes in and out of each format on your machine. The ODF routes need LibreOffice.

### Build from source

It needs Rust 1.95+ (the first build fetches the ArtCraft crates from GitHub), Node 20+, and cmake, a C/C++ compiler and libclang, which build BoringSSL for the sealing crate (on Debian or Ubuntu: `apt install build-essential cmake clang libclang-dev`; on macOS: Xcode command-line tools and `brew install cmake`). Optional: `pdftotext` (poppler), which `run.sh` uses to check the pack, and LibreOffice, for OpenDocument files.

```sh
cargo test
cd tools/html2mleaf && npm install && npx playwright install chromium && cd ../..
examples/hello-table/run.sh
```

`run.sh` walks the whole loop on a small example: an agent's HTML deck becomes a proposal layer, a finance review restates one number, and the report, workbook and deck all follow. They are exported through WordCraft, GridCraft and DeckCraft; copies edited in LibreOffice (and, from the stored fixtures, in Word, Excel and PowerPoint 16) come back as edit layers, and a number typed over a linked table comes back as a conflict. It then publishes one PDF board pack from a single snapshot, puts the project into a revision store (signed commits, a stale edit refused and rebased, blame, checkpoints, verification), round-trips real-world files with every loss listed, and exports the project for AI tools as DocLang, checked by the official validator. `mleaf formats` lists the routes available on your machine and the tools missing.

## Licence

Code: MIT OR Apache-2.0, at your option (`LICENSE-MIT`, `LICENSE-APACHE`). Third-party material and its licences: `NOTICE` and `ATTRIBUTION.md`. The Milleleaf name and logo are not open source; their terms are in `brand/LICENSE-brand.txt`. What Milleleaf collects (nothing): `DATA-POLICY.md`. Questions and contributions: hello@milleleaf.com. Security reports: security@milleleaf.com.
