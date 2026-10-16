#!/usr/bin/env bash
# Hello Table, end to end:
#   1. measure the agent's HTML deck into a proposal layer (html2ox)
#   2. compose base + proposal + finance review and validate it
#   3. show where the deck table's numbers come from (provenance)
#   4. render the composed deck and report as HTML views
#   5. round-trip: measure the rendered deck again and compare with the proposal
set -euo pipefail
cd "$(dirname "$0")"
OX="cargo run -q --manifest-path ../../Cargo.toml --bin ox --"
HTML2OX="node ../../tools/html2ox/html2ox.mjs"
PROJECT_ROOT=3f1c2a90-0000-4c1e-9a00-000000000001
REPORT=3f1c2a90-0000-4c1e-9a00-000000000020

echo "== 1. HTML deck → proposal layer"
#    deck.proposal.ox.json is stored, and the Office fixtures were exported from it. Measure into
#    a temporary file and compare: nodes and text must match, geometry may move a few pixels with
#    the browser and fonts installed. To re-measure for good, run html2ox with --out deck.proposal.ox.json.
TMP=$(mktemp -d)
$HTML2OX deck.html --out "$TMP/deck.proposal.ox.json" --assets "$TMP/assets" --onto base.ox.json \
  --attach "$PROJECT_ROOT:board-deck" --author agent --allow-js
node ../../tools/html2ox/compare.mjs deck.proposal.ox.json "$TMP/deck.proposal.ox.json" --warn-geometry
rm -rf "$TMP"

DECK=$(node -e 'console.log(JSON.parse(require("fs").readFileSync("deck.proposal.ox.json")).data[1].path)')
TABLE_FRAME=$(node -e 'const d=JSON.parse(require("fs").readFileSync("deck.proposal.ox.json")).data;console.log(d.find(n=>n.attributes?.["deck::table"]).path)')

echo; echo "== 2. compose and validate the stack"
$OX layers project.ox.json
$OX validate project.ox.json

echo; echo "== 3. where do the deck table's numbers come from?"
$OX get project.ox.json "$TABLE_FRAME" | node -e '
  const e = JSON.parse(require("fs").readFileSync(0));
  console.log("rows:", JSON.stringify(e.attributes["office::data::table"].rows));
  console.log("office::data::table set by:", JSON.stringify(e.provenance["office::data::table"]));
  console.log("review note:", e.attributes["ox::review"].note);'

echo; echo "== 4. render HTML views of the composed model"
$OX render project.ox.json "$DECK" --out composed-deck.html
$OX render project.ox.json "$REPORT" --out composed-report.html
echo "wrote composed-deck.html and composed-report.html"

echo; echo "== 5. round trip: rendered deck → layer → compare"
TMP=$(mktemp -d)
$HTML2OX composed-deck.html --out "$TMP/roundtrip.ox.json" --assets "$TMP/assets" --id roundtrip
node ../../tools/html2ox/compare.mjs deck.proposal.ox.json "$TMP/roundtrip.ox.json"
rm -rf "$TMP"

echo; echo "== 6. Word: export the report as DOCX (via WordCraft)"
$OX docx-export project.ox.json "$REPORT" --out report.docx

echo; echo "== 7. Word: bring back edits made in LibreOffice (fixtures/report.libreoffice-edited.docx)"
#    reworded the summary, added a paragraph, and overwrote a number in the linked table
$OX docx-import project.ox.json "$REPORT" fixtures/report.libreoffice-edited.docx \
  --out word-edit.ox.json --author libreoffice --id edit/report-libreoffice

echo; echo "== 8. Excel: export the finance workbook as XLSX (via GridCraft, recalculated)"
WORKBOOK=3f1c2a90-0000-4c1e-9a00-000000000010
$OX xlsx-export project.ox.json "$WORKBOOK" --out finance.xlsx

echo; echo "== 9. Excel: bring back edits made in LibreOffice (fixtures/finance.libreoffice-edited.xlsx)"
#    restated Singapore, inserted a Perth row inside the table, renamed the sheet "Q3 2026"
$OX xlsx-import project.ox.json "$WORKBOOK" fixtures/finance.libreoffice-edited.xlsx \
  --out grid-edit.ox.json --author libreoffice --id edit/finance-libreoffice

echo; echo "== 10. PowerPoint: export the deck as PPTX (via DeckCraft) and bring back edits made in Impress"
#    (fixtures/deck.libreoffice-edited.pptx: title moved, accent bar deleted, notes rewritten,
#    summary reworded around its live numbers, a bullet extended, a text box added, and a
#    number typed into the linked table)
$OX pptx-export project.ox.json "$DECK" --out deck.pptx
$OX pptx-import project.ox.json "$DECK" fixtures/deck.libreoffice-edited.pptx \
  --out deck-edit.ox.json --author impress --id edit/deck-impress

echo; echo "== 11. everything stacked: project + Word, Excel and PowerPoint edits"
$OX layers with-edits.ox.json
$OX validate with-edits.ox.json
$OX render with-edits.ox.json "$REPORT" --out composed-report-edited.html
$OX render with-edits.ox.json "$DECK" --out composed-deck-edited.html
$OX docx-export with-edits.ox.json "$REPORT" --out report-edited.docx
$OX xlsx-export with-edits.ox.json "$WORKBOOK" --out finance-edited.xlsx
$OX pptx-export with-edits.ox.json "$DECK" --out deck-edited.pptx
echo "wrote composed-report-edited.html, composed-deck-edited.html, report-edited.docx,"
echo "finance-edited.xlsx and deck-edited.pptx: every edit in, and the workbook's Perth row, total,"
echo "region count and chart in the report and on the slide"

echo; echo "== 12. the board pack: one PDF from one composed snapshot (WordCraft + GridCraft + DeckCraft → PdfCraft)"
PACK=3f1c2a90-0000-4c1e-9a00-000000000040
$OX pack with-edits.ox.json "$PACK" --out board-pack.pdf
if command -v pdftotext >/dev/null; then
  # The same figures on the report (p1), workbook (p2) and slide (p4) pages.
  for p in 1 2 4; do
    pdftotext -f "$p" -l "$p" board-pack.pdf - | grep -q '1,378,750' || { echo "page $p lacks the total"; exit 1; }
  done
  for p in 1 4; do
    pdftotext -f "$p" -l "$p" board-pack.pdf - | grep -q '4 regions' || { echo "page $p lacks the region count"; exit 1; }
  done
  pdftotext -f 4 -l 4 board-pack.pdf - | grep -q 'Page 4 of 6' || { echo "page 4 lacks its footer"; exit 1; }
  echo "checked: \$1,378,750 on the report, workbook and slide pages; \"4 regions\" in the report and on the slide;"
  echo "         running footer with continuous page numbers"
fi

echo; echo "== 13. OpenDocument: the same round trips as ODT, ODS and ODP (through LibreOffice, if installed)"
if command -v soffice >/dev/null || command -v libreoffice >/dev/null || [ -n "${OX_SOFFICE:-}" ] \
   || [ -x /Applications/LibreOffice.app/Contents/MacOS/soffice ]; then
  $OX docx-export with-edits.ox.json "$REPORT" --out report-edited.odt
  $OX xlsx-export with-edits.ox.json "$WORKBOOK" --out finance-edited.ods
  $OX pptx-export with-edits.ox.json "$DECK" --out deck-edited.odp
  TMP=$(mktemp -d)
  $OX docx-import project.ox.json "$REPORT" fixtures/report.libreoffice-edited.odt --out "$TMP/w.ox.json" --author libreoffice --id edit/report-libreoffice
  $OX xlsx-import project.ox.json "$WORKBOOK" fixtures/finance.libreoffice-edited.ods --out "$TMP/g.ox.json" --author libreoffice --id edit/finance-libreoffice
  $OX pptx-import project.ox.json "$DECK" fixtures/deck.libreoffice-edited.odp --out "$TMP/d.ox.json" --author impress --id edit/deck-impress
  node -e '
    const fs = require("fs");
    const d = (f) => JSON.stringify(JSON.parse(fs.readFileSync(f)).data.sort((a, b) => a.path < b.path ? -1 : 1));
    const pairs = [["word-edit.ox.json", "w"], ["grid-edit.ox.json", "g"], ["deck-edit.ox.json", "d"]];
    for (const [ooxml, k] of pairs) {
      if (d(ooxml) !== d(process.argv[1] + "/" + k + ".ox.json")) { console.error("ODF edit layer differs from " + ooxml); process.exit(1); }
    }
    console.log("checked: the ODT, ODS and ODP edits give exactly the same edit layers as DOCX, XLSX and PPTX");
  ' "$TMP"
  rm -rf "$TMP"
else
  echo "skipped: LibreOffice isn't installed (set OX_SOFFICE to its soffice)"
fi

echo; echo "== 14. Microsoft Office: the same edits made in Word, Excel and PowerPoint 16 (fixtures/*.msoffice-*)"
#    Word refuses to type over the content-locked linked table, so its edit has no conflict;
#    report.msoffice-unlocked-edited.docx had the lock removed first and comes back as a conflict.
TMP=$(mktemp -d)
$OX docx-import project.ox.json "$REPORT" fixtures/report.msoffice-resaved.docx --out "$TMP/wr.ox.json" --author word --id edit/report-word
$OX xlsx-import project.ox.json "$WORKBOOK" fixtures/finance.msoffice-resaved.xlsx --out "$TMP/gr.ox.json" --author excel --id edit/finance-excel
$OX pptx-import project.ox.json "$DECK" fixtures/deck.msoffice-resaved.pptx --out "$TMP/dr.ox.json" --author powerpoint --id edit/deck-powerpoint
$OX docx-import project.ox.json "$REPORT" fixtures/report.msoffice-edited.docx --out "$TMP/w.ox.json" --author word --id edit/report-word
$OX docx-import project.ox.json "$REPORT" fixtures/report.msoffice-unlocked-edited.docx --out "$TMP/wu.ox.json" --author word --id edit/report-word
$OX xlsx-import project.ox.json "$WORKBOOK" fixtures/finance.msoffice-edited.xlsx --out "$TMP/g.ox.json" --author excel --id edit/finance-excel
$OX pptx-import project.ox.json "$DECK" fixtures/deck.msoffice-edited.pptx --out "$TMP/d.ox.json" --author powerpoint --id edit/deck-powerpoint
node -e '
  const fs = require("fs");
  const data = (f) => JSON.parse(fs.readFileSync(f)).data.sort((a, b) => a.path < b.path ? -1 : 1);
  const t = (f) => process.argv[1] + "/" + f + ".ox.json";
  const fail = (m) => { console.error(m); process.exit(1); };
  for (const k of ["wr", "gr", "dr", "wu"]) if (data(t(k)).length) fail(k + ": expected an empty edit layer");
  for (const [lo, k] of [["word-edit.ox.json", "w"], ["grid-edit.ox.json", "g"]])
    if (JSON.stringify(data(lo)) !== JSON.stringify(data(t(k)))) fail("Office edit layer differs from " + lo);
  // The deck: same nodes touched; the new text box sits elsewhere and the title rounds differently.
  const paths = (f) => JSON.stringify(data(f).map((n) => n.path));
  if (paths("deck-edit.ox.json") !== paths(t("d"))) fail("PowerPoint touched different nodes than Impress");
  console.log("checked: resaves are no change; Word and Excel give the same edit layers as LibreOffice; PowerPoint touches the same nodes as Impress");
' "$TMP"
rm -rf "$TMP"

echo; echo "== 15. Revision store: commit by compare-and-swap, blame, checkpoint, signatures"
TMP=$(mktemp -d)
export OX_KEYS="$TMP/keys"                       # a throwaway key, not yours
$OX init "$TMP/proj" --from project.ox.json
$OX key new sam "$TMP/proj" > /dev/null
$OX commit "$TMP/proj" grid-edit.ox.json --sign --key sam
#    someone edits the report in Word, exported before the workbook edit: import bases the layer
#    on that earlier head (it matches the file's ox.revision), not on the current one …
$OX docx-import "$TMP/proj" "$REPORT" fixtures/report.libreoffice-edited.docx \
  --out "$TMP/word.ox.json" --author libreoffice --id edit/report-libreoffice 2> /dev/null
#    … while the deck edit lands first
$OX commit "$TMP/proj" deck-edit.ox.json
#    so the Word edit is stale: refused (exit 3), then rebased (it touches different nodes)
if $OX commit "$TMP/proj" "$TMP/word.ox.json"; then echo "expected a stale refusal"; exit 1; fi
$OX commit "$TMP/proj" "$TMP/word.ox.json" --rebase
echo "-- where does the Q3 total on the slide come from?"
TOTAL_FIELD=$(node -e 'const d=JSON.parse(require("fs").readFileSync("deck.proposal.ox.json")).data;
  console.log(d.find(n=>Object.values(n.inherits||{}).includes("3f1c2a90-0000-4c1e-9a00-000000000013")).path)')
$OX blame "$TMP/proj" "$TOTAL_FIELD" office::data::value
$OX checkpoint "$TMP/proj" --keep 3 --author sam
$OX log "$TMP/proj"
$OX verify "$TMP/proj"
#    the three fixes from the 16 October review, on a fresh project
P2="$TMP/proj2"
$OX init "$P2" --from project.ox.json > /dev/null
#    init --from copies the deck's picture, and export finds it in the project root
$OX pptx-export "$P2" "$DECK" --out "$TMP/p2.pptx" 2> "$TMP/err"
if grep -q "picture skipped" "$TMP/err"; then cat "$TMP/err"; echo "a picture was lost"; exit 1; fi
echo "checked: a project's deck export keeps its picture"
#    an unedited export from an earlier head imports as no change, so it can't revert newer work
$OX docx-export "$P2" "$REPORT" --out "$TMP/old.docx" 2> /dev/null
$OX commit "$P2" word-edit.ox.json > /dev/null
$OX docx-import "$P2" "$REPORT" "$TMP/old.docx" --out "$TMP/old.ox.json" 2> /dev/null
node -e 'const l=JSON.parse(require("fs").readFileSync(process.argv[1]));if(l.data.length){console.log("an old unedited export came back with changes");process.exit(1)}' "$TMP/old.ox.json"
echo "checked: an unedited export from an earlier head imports as no change"
#    a signature from a key the project doesn't list doesn't count with --require
$OX key new owner "$P2" > /dev/null
$OX key new stranger > /dev/null
for L in $($OX verify "$P2" 2> /dev/null | cut -c1-12); do $OX sign "$P2" "$L" --key owner > /dev/null; done
$OX verify "$P2" --require > /dev/null 2>&1 || { echo "--require refused layers signed by the owner"; exit 1; }
$OX commit "$P2" grid-edit.ox.json --sign --key stranger > /dev/null
if $OX verify "$P2" --require > /dev/null 2>&1; then echo "expected --require to refuse an unlisted key"; exit 1; fi
echo "checked: --require refuses a layer signed only by an unlisted key"
$OX validate "$TMP/proj" | tail -1
#    the store composes exactly like the stack files with the same edits
$OX compose "$TMP/proj" --out "$TMP/a.json" 2> /dev/null
$OX compose with-edits.ox.json --out "$TMP/b.json" 2> /dev/null
# Object key order carries no meaning (the store keeps layers in RFC 8785 form, keys sorted); arrays keep order.
# The stack files name pictures by path, the store by the hash of their bytes: compare as hashes.
node -e 'const fs = require("fs"), crypto = require("crypto");
  const types = { svg: "image/svg+xml", jpg: "image/jpeg", jpeg: "image/jpeg", gif: "image/gif", webp: "image/webp" };
  const asDigest = (v) => typeof v.blob === "string" && !v.blob.startsWith("sha256-")
    ? { ...v, blob: "sha256-" + crypto.createHash("sha256").update(fs.readFileSync(v.blob)).digest("hex"),
        mediaType: v.mediaType || types[v.blob.split(".").pop().toLowerCase()] || "image/png" } : v;
  const sorted = (k, v) => v && typeof v === "object" && !Array.isArray(v) ? Object.fromEntries(Object.entries(asDigest(v)).sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0)) : v;
  const r = (f) => JSON.stringify(JSON.parse(require("fs").readFileSync(f)).roots, sorted);
  if (r(process.argv[1]) !== r(process.argv[2])) { console.error("store and stack files compose differently"); process.exit(1); }
  console.log("checked: the store composes exactly like with-edits.ox.json");' "$TMP/a.json" "$TMP/b.json"
rm -rf "$TMP"; unset OX_KEYS

echo; echo "== 16. Formats: every route, what it keeps, and what this machine can do"
$OX formats
TMP=$(mktemp -d)
#    one-way copies of the report (WordCraft's own writers keep no markers, and say so)
$OX docx-export project.ox.json "$REPORT" --out "$TMP/report.md" 2>&1 | grep -E "warning|ox:" | head -2
#    without LibreOffice: ODT export falls back to WordCraft's writer; ODF imports are refused
#    with the reason, rather than guessing at every paragraph
OX_SOFFICE=none $OX formats | grep -E "^LibreOffice:"
OX_SOFFICE=none $OX docx-export project.ox.json "$REPORT" --out "$TMP/report.odt" 2>&1 | grep -E "writing|warning" | head -3
if OX_SOFFICE=none $OX pptx-import project.ox.json "$DECK" fixtures/deck.libreoffice-edited.odp --out "$TMP/x.ox.json" 2> "$TMP/err"; then
  echo "expected the ODP import to be refused without LibreOffice"; exit 1
fi
cat "$TMP/err"
#    how each import recognised its nodes: Impress drops slide names, so its slides are best guesses
$OX pptx-import project.ox.json "$DECK" fixtures/deck.libreoffice-edited.pptx --out "$TMP/d.ox.json" 2>&1 | grep -E "matched|best guess"
$OX pptx-import project.ox.json "$DECK" fixtures/deck.msoffice-edited.pptx --out "$TMP/d.ox.json" 2>&1 | grep -E "matched|best guess"
rm -rf "$TMP"
