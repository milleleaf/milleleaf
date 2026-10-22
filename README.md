# Milleleaf

One office workspace over the ArtCraft
[WordCraft](https://github.com/storytold/wordcraft), [GridCraft](https://github.com/storytold/gridcraft),
[DeckCraft](https://github.com/storytold/deckcraft) and [PdfCraft](https://github.com/storytold/pdfcraft):
four apps, four modules, **one model**.

People switch between documents, spreadsheets, slides and PDFs all day, whichever suite they
use. Milleleaf keeps the numbers, tables and text they share as one set of objects, so a
correction in the workbook shows up in the report and the deck, and every export comes from
the same version.

## Ideas

- **Data-model separation, single source of truth, single-model continuity**: a project is a
  stack of JSON layers composed into one truth; each app reads and writes its own namespace on
  shared nodes; every value knows which layer set it.

  Milleleaf's layered model follows the composition ideas of Pixar's Universal Scene Description
  (OpenUSD), as adopted by buildingSMART's IFC5. Its export for AI tools follows the DocLang 0.8
  specification (doclang.ai, Apache-2.0) and is checked with the DocLang validator. The schema
  and code are written independently; no USD, IFC5 or DocLang code or schema text is included.
- **HTML in and out.** Agents and HTML-deck skills write HTML well. A restricted profile lets
  their decks come in as reviewable *proposal layers*, and lets the model render back out to
  HTML that re-imports onto the same nodes.
- **Office formats are exports**, made from a composed snapshot (planned: DOCX, XLSX, PPTX,
  ODF, PDF through the Craft crates).

## What's here (alpha)

| Path | What |
|---|---|
| `schema/ox.tsp` | TypeSpec for the file format; compiles to JSON Schema with `@typespec/compiler` and `@typespec/json-schema` 1.17.0 |
| `schema/office.mleaf.json` | Attribute schemas: `office::`, `grid::`, `word::`, `deck::`, `mleaf::` |
| `crates/milleleaf-core` | Rust: load stacks (imports, integrity), flatten, compose with provenance, validate, render HTML |
| `crates/milleleaf-cli` | `mleaf layers / validate / compose / get / render / pack`, `docx-`, `xlsx-`, `pptx-` `export` / `import`, and the store: `init / commit / head / log / blame / checkpoint / verify / sign / key` |
| `crates/milleleaf-formats` | Format registry: every route in and out, what it keeps, what it needs, and the best one on this machine |
| `crates/milleleaf-store` | Revision store in a project's `.mleaf/` folder |
| `crates/milleleaf-wordcraft` | WordCraft adapter: composed document → DOCX with node identity in content controls; edited DOCX → minimal edit layer |
| `crates/milleleaf-gridcraft` | GridCraft adapter: composed workbook → recalculated XLSX with node identity in defined names; edited XLSX → edit layer recalculated by GridCraft's calc engine |
| `crates/milleleaf-pack` | One composed snapshot → each part's PDF through WordCraft, GridCraft and DeckCraft → one bookmarked PDF assembled with PdfCraft, with an "About this pack" page |
| `crates/milleleaf-odf` | ODT, ODS and ODP for every export and import, converted through LibreOffice, which keeps the identity markers the Craft apps' ODF code drops |
| `crates/milleleaf-deckcraft` | DeckCraft adapter: composed deck → PPTX with native tables and charts drawn from the data; edited PPTX → edit layer |
| `tools/html2mleaf` | Node + Playwright: measure an HTML deck into a proposal layer + loss report; `compare.mjs` for round-trip checks |
| `examples/hello-table` | Workbook → report → agent HTML deck → finance correction → Word, Excel and PowerPoint edits made in LibreOffice, end to end |

## Try it

Needs Rust 1.95+ (PdfCraft's minimum; the first build fetches the Craft crates from GitHub) and
Node 20+. Optional: `pdftotext` (poppler), which `run.sh` uses to check the pack, and
LibreOffice, for OpenDocument files.

```sh
cargo test
cd tools/html2mleaf && npm install && npx playwright install chromium && cd ../..
examples/hello-table/run.sh
```

`run.sh` measures `deck.html` again and checks it against the stored `deck.proposal.mleaf.json` (the same nodes and text; geometry that moves with the installed browser and fonts is only a warning), stacks that proposal with the base workbook +
report and a finance review that restates Hong Kong revenue, shows that the slide table's rows
come from the finance layer *via* the workbook table, renders `composed-deck.html` and
`composed-report.html` (both now say $541,500 and $1,250,500), then re-imports the rendered
deck and checks nothing changed. It then exports the report as `report.docx` through
WordCraft and imports a copy edited in LibreOffice: the reworded summary and a new paragraph come
back as `word-edit.mleaf.json`, while the number typed over the linked table is reported as a
conflict, because the workbook owns it. Next it exports the workbook as `finance.xlsx` through
GridCraft and imports a copy edited in LibreOffice (Singapore restated, a Perth row inserted,
the sheet renamed) as `grid-edit.mleaf.json`, recalculated by GridCraft, and the deck as
`deck.pptx` through DeckCraft, with a copy edited in Impress coming back as `deck-edit.mleaf.json`.
Finally `with-edits.mleaf.json` stacks all three: the report, the workbook and the deck are
re-exported with every edit, and the slide's table, its chart and "across 4 regions" all show
the Perth row the workbook gained. The last step publishes `board-pack.pdf`: report, workbook
and deck from that one snapshot, bookmarked, page-numbered, with a page listing every layer
the numbers came from. With LibreOffice installed it also repeats the round trips as ODT, ODS
and ODP and checks they give the same edit layers, and checks that the same edits made in
Microsoft Word, Excel and PowerPoint 16 give the same edit layers too. Last, it puts the project
into a revision store: commits the edits (one signed), has a stale Word edit refused and then
rebased, blames the Q3 total on the slide back through the finance review to the workbook edit,
checkpoints, verifies, and checks the store composes exactly like the stack files. Step 16
prints `mleaf formats`, makes one-way copies (Markdown; ODT without LibreOffice), shows an ODP
import refused without LibreOffice, and shows Impress's slide matches as best guesses.

## Next Done: the Microsoft Office proof (M0), the revision store (M1), and honest
matching with format routing (M2). Next: review rules and agent conflict resolution (M3),
shared-folder collaboration, iroh peer-to-peer sync, and a browser workspace.
