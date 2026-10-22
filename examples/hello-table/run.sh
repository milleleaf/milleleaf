#!/usr/bin/env bash
# Hello Table, end to end:
#   1. measure the agent's HTML deck into a proposal layer (html2mleaf)
#   2. compose base + proposal + finance review and validate it
#   3. show where the deck table's numbers come from (provenance)
#   4. render the composed deck and report as HTML views
#   5. round-trip: measure the rendered deck again and compare with the proposal
set -euo pipefail
cd "$(dirname "$0")"
MLEAF="cargo run -q --manifest-path ../../Cargo.toml --bin mleaf --"
HTML2MLEAF="node ../../tools/html2mleaf/html2mleaf.mjs"
PROJECT_ROOT=3f1c2a90-0000-4c1e-9a00-000000000001
REPORT=3f1c2a90-0000-4c1e-9a00-000000000020

echo "== 1. HTML deck → proposal layer"
#    deck.proposal.mleaf.json is stored, and the Office fixtures were exported from it. Measure into
#    a temporary file and compare: nodes and text must match, geometry may move a few pixels with
#    the browser and fonts installed. To re-measure for good, run html2mleaf with --out deck.proposal.mleaf.json.
TMP=$(mktemp -d)
$HTML2MLEAF deck.html --out "$TMP/deck.proposal.mleaf.json" --assets "$TMP/assets" --onto base.mleaf.json \
  --attach "$PROJECT_ROOT:board-deck" --author agent --allow-js
node ../../tools/html2mleaf/compare.mjs deck.proposal.mleaf.json "$TMP/deck.proposal.mleaf.json" --warn-geometry
rm -rf "$TMP"

DECK=$(node -e 'console.log(JSON.parse(require("fs").readFileSync("deck.proposal.mleaf.json")).data[1].path)')
TABLE_FRAME=$(node -e 'const d=JSON.parse(require("fs").readFileSync("deck.proposal.mleaf.json")).data;console.log(d.find(n=>n.attributes?.["deck::table"]).path)')

echo; echo "== 2. compose and validate the stack"
$MLEAF layers project.mleaf.json
$MLEAF validate project.mleaf.json

echo; echo "== 3. where do the deck table's numbers come from?"
$MLEAF get project.mleaf.json "$TABLE_FRAME" | node -e '
  const e = JSON.parse(require("fs").readFileSync(0));
  console.log("rows:", JSON.stringify(e.attributes["office::data::table"].rows));
  console.log("office::data::table set by:", JSON.stringify(e.provenance["office::data::table"]));
  console.log("review note:", e.attributes["mleaf::review"].note);'

echo; echo "== 4. render HTML views of the composed model"
$MLEAF render project.mleaf.json "$DECK" --out composed-deck.html
$MLEAF render project.mleaf.json "$REPORT" --out composed-report.html
echo "wrote composed-deck.html and composed-report.html"

echo; echo "== 5. round trip: rendered deck → layer → compare"
TMP=$(mktemp -d)
$HTML2MLEAF composed-deck.html --out "$TMP/roundtrip.mleaf.json" --assets "$TMP/assets" --id roundtrip
node ../../tools/html2mleaf/compare.mjs deck.proposal.mleaf.json "$TMP/roundtrip.mleaf.json"
rm -rf "$TMP"

echo; echo "== 6. Word: export the report as DOCX (via WordCraft)"
$MLEAF docx-export project.mleaf.json "$REPORT" --out report.docx

echo; echo "== 7. Word: bring back edits made in LibreOffice (fixtures/report.libreoffice-edited.docx)"
#    reworded the summary, added a paragraph, and overwrote a number in the linked table
$MLEAF docx-import project.mleaf.json "$REPORT" fixtures/report.libreoffice-edited.docx \
  --out word-edit.mleaf.json --author libreoffice --id edit/report-libreoffice

echo; echo "== 8. Excel: export the finance workbook as XLSX (via GridCraft, recalculated)"
WORKBOOK=3f1c2a90-0000-4c1e-9a00-000000000010
$MLEAF xlsx-export project.mleaf.json "$WORKBOOK" --out finance.xlsx

echo; echo "== 9. Excel: bring back edits made in LibreOffice (fixtures/finance.libreoffice-edited.xlsx)"
#    restated Singapore, inserted a Perth row inside the table, renamed the sheet "Q3 2026"
$MLEAF xlsx-import project.mleaf.json "$WORKBOOK" fixtures/finance.libreoffice-edited.xlsx \
  --out grid-edit.mleaf.json --author libreoffice --id edit/finance-libreoffice

echo; echo "== 10. PowerPoint: export the deck as PPTX (via DeckCraft) and bring back edits made in Impress"
#    (fixtures/deck.libreoffice-edited.pptx: title moved, accent bar deleted, notes rewritten,
#    summary reworded around its live numbers, a bullet extended, a text box added, and a
#    number typed into the linked table)
$MLEAF pptx-export project.mleaf.json "$DECK" --out deck.pptx
$MLEAF pptx-import project.mleaf.json "$DECK" fixtures/deck.libreoffice-edited.pptx \
  --out deck-edit.mleaf.json --author impress --id edit/deck-impress

echo; echo "== 11. everything stacked: project + Word, Excel and PowerPoint edits"
$MLEAF layers with-edits.mleaf.json
$MLEAF validate with-edits.mleaf.json
$MLEAF render with-edits.mleaf.json "$REPORT" --out composed-report-edited.html
$MLEAF render with-edits.mleaf.json "$DECK" --out composed-deck-edited.html
$MLEAF docx-export with-edits.mleaf.json "$REPORT" --out report-edited.docx
$MLEAF xlsx-export with-edits.mleaf.json "$WORKBOOK" --out finance-edited.xlsx
$MLEAF pptx-export with-edits.mleaf.json "$DECK" --out deck-edited.pptx
echo "wrote composed-report-edited.html, composed-deck-edited.html, report-edited.docx,"
echo "finance-edited.xlsx and deck-edited.pptx: every edit in, and the workbook's Perth row, total,"
echo "region count and chart in the report and on the slide"

echo; echo "== 12. the board pack: one PDF from one composed snapshot (WordCraft + GridCraft + DeckCraft → PdfCraft)"
PACK=3f1c2a90-0000-4c1e-9a00-000000000040
$MLEAF pack with-edits.mleaf.json "$PACK" --out board-pack.pdf
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
if command -v soffice >/dev/null || command -v libreoffice >/dev/null || [ -n "${MLEAF_SOFFICE:-}" ] \
   || [ -x /Applications/LibreOffice.app/Contents/MacOS/soffice ]; then
  $MLEAF docx-export with-edits.mleaf.json "$REPORT" --out report-edited.odt
  $MLEAF xlsx-export with-edits.mleaf.json "$WORKBOOK" --out finance-edited.ods
  $MLEAF pptx-export with-edits.mleaf.json "$DECK" --out deck-edited.odp
  TMP=$(mktemp -d)
  $MLEAF docx-import project.mleaf.json "$REPORT" fixtures/report.libreoffice-edited.odt --out "$TMP/w.mleaf.json" --author libreoffice --id edit/report-libreoffice
  $MLEAF xlsx-import project.mleaf.json "$WORKBOOK" fixtures/finance.libreoffice-edited.ods --out "$TMP/g.mleaf.json" --author libreoffice --id edit/finance-libreoffice
  $MLEAF pptx-import project.mleaf.json "$DECK" fixtures/deck.libreoffice-edited.odp --out "$TMP/d.mleaf.json" --author impress --id edit/deck-impress
  node -e '
    const fs = require("fs");
    const d = (f) => JSON.stringify(JSON.parse(fs.readFileSync(f)).data.sort((a, b) => a.path < b.path ? -1 : 1));
    const pairs = [["word-edit.mleaf.json", "w"], ["grid-edit.mleaf.json", "g"], ["deck-edit.mleaf.json", "d"]];
    for (const [ooxml, k] of pairs) {
      if (d(ooxml) !== d(process.argv[1] + "/" + k + ".mleaf.json")) { console.error("ODF edit layer differs from " + ooxml); process.exit(1); }
    }
    console.log("checked: the ODT, ODS and ODP edits give exactly the same edit layers as DOCX, XLSX and PPTX");
  ' "$TMP"
  rm -rf "$TMP"
else
  echo "skipped: LibreOffice isn't installed (set MLEAF_SOFFICE to its soffice)"
fi

echo; echo "== 14. Microsoft Office: the same edits made in Word, Excel and PowerPoint 16 (fixtures/*.msoffice-*)"
#    Word refuses to type over the content-locked linked table, so its edit has no conflict;
#    report.msoffice-unlocked-edited.docx had the lock removed first and comes back as a conflict.
TMP=$(mktemp -d)
$MLEAF docx-import project.mleaf.json "$REPORT" fixtures/report.msoffice-resaved.docx --out "$TMP/wr.mleaf.json" --author word --id edit/report-word
$MLEAF xlsx-import project.mleaf.json "$WORKBOOK" fixtures/finance.msoffice-resaved.xlsx --out "$TMP/gr.mleaf.json" --author excel --id edit/finance-excel
$MLEAF pptx-import project.mleaf.json "$DECK" fixtures/deck.msoffice-resaved.pptx --out "$TMP/dr.mleaf.json" --author powerpoint --id edit/deck-powerpoint
$MLEAF docx-import project.mleaf.json "$REPORT" fixtures/report.msoffice-edited.docx --out "$TMP/w.mleaf.json" --author word --id edit/report-word
$MLEAF docx-import project.mleaf.json "$REPORT" fixtures/report.msoffice-unlocked-edited.docx --out "$TMP/wu.mleaf.json" --author word --id edit/report-word
$MLEAF xlsx-import project.mleaf.json "$WORKBOOK" fixtures/finance.msoffice-edited.xlsx --out "$TMP/g.mleaf.json" --author excel --id edit/finance-excel
$MLEAF pptx-import project.mleaf.json "$DECK" fixtures/deck.msoffice-edited.pptx --out "$TMP/d.mleaf.json" --author powerpoint --id edit/deck-powerpoint
node -e '
  const fs = require("fs");
  const data = (f) => JSON.parse(fs.readFileSync(f)).data.sort((a, b) => a.path < b.path ? -1 : 1);
  const t = (f) => process.argv[1] + "/" + f + ".mleaf.json";
  const fail = (m) => { console.error(m); process.exit(1); };
  for (const k of ["wr", "gr", "dr", "wu"]) if (data(t(k)).length) fail(k + ": expected an empty edit layer");
  for (const [lo, k] of [["word-edit.mleaf.json", "w"], ["grid-edit.mleaf.json", "g"]])
    if (JSON.stringify(data(lo)) !== JSON.stringify(data(t(k)))) fail("Office edit layer differs from " + lo);
  // The deck: same nodes touched; the new text box sits elsewhere and the title rounds differently.
  const paths = (f) => JSON.stringify(data(f).map((n) => n.path));
  if (paths("deck-edit.mleaf.json") !== paths(t("d"))) fail("PowerPoint touched different nodes than Impress");
  console.log("checked: resaves are no change; Word and Excel give the same edit layers as LibreOffice; PowerPoint touches the same nodes as Impress");
' "$TMP"
rm -rf "$TMP"

echo; echo "== 15. Revision store: commit by compare-and-swap, blame, checkpoint, signatures, sealed layers, post-quantum signatures"
TMP=$(mktemp -d)
export MLEAF_KEYS="$TMP/keys"                       # a throwaway key, not yours
$MLEAF init "$TMP/proj" --from project.mleaf.json
$MLEAF key new sam "$TMP/proj" > /dev/null
$MLEAF commit "$TMP/proj" grid-edit.mleaf.json --sign --key sam
#    someone edits the report in Word, exported before the workbook edit: import bases the layer
#    on that earlier head (it matches the file's mleaf.revision), not on the current one …
$MLEAF docx-import "$TMP/proj" "$REPORT" fixtures/report.libreoffice-edited.docx \
  --out "$TMP/word.mleaf.json" --author libreoffice --id edit/report-libreoffice 2> /dev/null
#    … while the deck edit lands first
$MLEAF commit "$TMP/proj" deck-edit.mleaf.json
#    so the Word edit is stale: refused (exit 3), then rebased (it touches different nodes)
if $MLEAF commit "$TMP/proj" "$TMP/word.mleaf.json"; then echo "expected a stale refusal"; exit 1; fi
$MLEAF commit "$TMP/proj" "$TMP/word.mleaf.json" --rebase
echo "-- where does the Q3 total on the slide come from?"
TOTAL_FIELD=$(node -e 'const d=JSON.parse(require("fs").readFileSync("deck.proposal.mleaf.json")).data;
  console.log(d.find(n=>Object.values(n.inherits||{}).includes("3f1c2a90-0000-4c1e-9a00-000000000013")).path)')
$MLEAF blame "$TMP/proj" "$TOTAL_FIELD" office::data::value
$MLEAF checkpoint "$TMP/proj" --keep 3 --author sam
$MLEAF log "$TMP/proj"
$MLEAF verify "$TMP/proj"
#    the three fixes from the 16 October review, on a fresh project
P2="$TMP/proj2"
$MLEAF init "$P2" --from project.mleaf.json > /dev/null
#    init --from copies the deck's picture, and export finds it in the project root
$MLEAF pptx-export "$P2" "$DECK" --out "$TMP/p2.pptx" 2> "$TMP/err"
if grep -q "picture skipped" "$TMP/err"; then cat "$TMP/err"; echo "a picture was lost"; exit 1; fi
echo "checked: a project's deck export keeps its picture"
#    an unedited export from an earlier head imports as no change, so it can't revert newer work
$MLEAF docx-export "$P2" "$REPORT" --out "$TMP/old.docx" 2> /dev/null
$MLEAF commit "$P2" word-edit.mleaf.json > /dev/null
$MLEAF docx-import "$P2" "$REPORT" "$TMP/old.docx" --out "$TMP/old.mleaf.json" 2> /dev/null
node -e 'const l=JSON.parse(require("fs").readFileSync(process.argv[1]));if(l.data.length){console.log("an old unedited export came back with changes");process.exit(1)}' "$TMP/old.mleaf.json"
echo "checked: an unedited export from an earlier head imports as no change"
#    a signature from a key the project doesn't list doesn't count with --require
$MLEAF key new owner "$P2" > /dev/null
$MLEAF key new stranger > /dev/null
for L in $($MLEAF verify "$P2" 2> /dev/null | cut -c1-12); do $MLEAF sign "$P2" "$L" --key owner > /dev/null; done
$MLEAF verify "$P2" --require > /dev/null 2>&1 || { echo "--require refused layers signed by the owner"; exit 1; }
$MLEAF commit "$P2" grid-edit.mleaf.json --sign --key stranger > /dev/null
if $MLEAF verify "$P2" --require > /dev/null 2>&1; then echo "expected --require to refuse an unlisted key"; exit 1; fi
echo "checked: --require refuses a layer signed only by an unlisted key"
#    a sealed layer: only its recipient composes it, its plaintext is never on disk, and an owner
#    destroying its key (crypto-shredding) makes every copy unreadable
$MLEAF key new alice "$P2" > /dev/null
$MLEAF key role "$P2" owner owner > /dev/null
cat > "$TMP/sealed.mleaf.json" <<'JSON'
{"header": {"id": "edit/sealed-note", "mleafVersion": "mleaf_alpha", "dataVersion": "1.0.0", "author": "alice", "timestamp": "2026-10-16"},
 "data": [{"path": "3f1c2a90-0000-4c1e-9a00-000000000023", "attributes": {"test::note": "sealed demo text"}}]}
JSON
$MLEAF commit "$P2" "$TMP/sealed.mleaf.json" --seal-to alice
SEALED=$($MLEAF verify "$P2" 2> /dev/null | grep "sealed for alice" | cut -c1-12)
[ -n "$SEALED" ] || { echo "verify doesn't report the sealed layer"; exit 1; }
if grep -rq "sealed demo text" "$P2/.mleaf"; then echo "a sealed layer's plaintext is on disk"; exit 1; fi
$MLEAF compose "$P2" --out "$TMP/sealed.json" 2> /dev/null
grep -q "sealed demo text" "$TMP/sealed.json" || { echo "the recipient can't open the sealed layer"; exit 1; }
if grep -rq "sealed demo text" "$P2/.mleaf"; then echo "opening a sealed layer left its plaintext on disk"; exit 1; fi
#    mleaf search: the recipient finds the sealed text, now and in the history; its search index stays encrypted
$MLEAF search "$P2" sealed demo 2> /dev/null | head -2
$MLEAF search "$P2" sealed demo --history > /dev/null 2>&1 || { echo "--history doesn't find the sealed text"; exit 1; }
if grep -rq "demo" "$P2/.mleaf/cache/search"; then echo "a sealed layer's search index is plaintext"; exit 1; fi
if MLEAF_KEYS="$TMP/nokeys" $MLEAF search "$P2" sealed demo > /dev/null 2>&1; then echo "search found sealed text without a key"; exit 1; fi
echo "checked: mleaf search finds sealed text for its recipient only, and its index on disk is encrypted"
$MLEAF purge "$P2" "$SEALED" --key owner --key-destroy --reason demo
$MLEAF compose "$P2" --out "$TMP/sealed.json" 2> /dev/null
if grep -q "sealed demo text" "$TMP/sealed.json"; then echo "still readable after its key was destroyed"; exit 1; fi
$MLEAF verify "$P2" 2> /dev/null | grep "key destroyed"
echo "checked: a sealed layer opens for its recipient only, and destroying its key shreds it"
#    post-quantum signatures (plan decision 5, 18 Oct 2026): every signature is Ed25519 + ML-DSA-65, an
#    owner's or an --archive one adds SLH-DSA; --require applies .mleaf/policy.json (default: Ed25519 + ML-DSA-65)
P3="$TMP/proj3"
$MLEAF init "$P3" --from project.mleaf.json > /dev/null
$MLEAF key new pq "$P3" > /dev/null
for L in $($MLEAF verify "$P3" 2> /dev/null | cut -c1-12); do $MLEAF sign "$P3" "$L" --key pq > /dev/null; done
FIRST=$($MLEAF verify "$P3" 2> /dev/null | sed -n 1p | cut -c1-12)
$MLEAF sign "$P3" "$FIRST" --key pq --archive > /dev/null
$MLEAF verify "$P3" --require > "$TMP/v.txt" || { cat "$TMP/v.txt"; echo "--require refused hybrid signatures"; exit 1; }
sed -n 1,2p "$TMP/v.txt"
grep -q "signed by pq (hybrid+slh-dsa)" "$TMP/v.txt" && grep -q "signed by pq (hybrid)" "$TMP/v.txt" \
  || { echo "expected hybrid and hybrid+slh-dsa signatures"; exit 1; }
echo "checked: Ed25519 + ML-DSA-65 on every signature, SLH-DSA with --archive, accepted by --require"
#    a key made before post-quantum keys existed signs Ed25519 only ("classical only"): accepted with a
#    warning until the policy's classicalOnlyUntil (default 2035-01-01) …
$MLEAF key new legacy > /dev/null
node -e 'const fs = require("fs"), f = process.argv[1], k = JSON.parse(fs.readFileSync(f));
  delete k.mldsa65; delete k.slhdsa; fs.writeFileSync(f, JSON.stringify(k, null, 2))' "$MLEAF_KEYS/legacy.key"
$MLEAF key add "$P3" legacy "$(node -p 'JSON.parse(require("fs").readFileSync(process.argv[1])).public' "$MLEAF_KEYS/legacy.key")" > /dev/null
$MLEAF commit "$P3" grid-edit.mleaf.json --sign --key legacy 2> /dev/null
LEGACY=$($MLEAF verify "$P3" 2> /dev/null | grep "signed by legacy" | cut -c1-12)
$MLEAF verify "$P3" --require > "$TMP/v.txt" || { cat "$TMP/v.txt"; echo "--require refused classical only before its date"; exit 1; }
grep "legacy" "$TMP/v.txt"
grep -q "warning: legacy: classical only" "$TMP/v.txt" || { echo "expected a classical-only warning"; exit 1; }
#    … and refused once that date has passed
cat > "$P3/.mleaf/policy.json" <<'JSON'
{"signatures": {"require": ["ed25519", "mldsa65"], "classicalOnlyUntil": "2026-01-01"},
 "algorithms": {"ed25519": {"until": "2035-01-01"}, "mldsa65": {}, "slhdsa-sha2-192s": {}, "xwing": {}}}
JSON
if $MLEAF verify "$P3" --require > "$TMP/v.txt" 2>&1; then echo "expected --require to refuse classical only"; exit 1; fi
grep "REFUSED" "$TMP/v.txt"
#    `mleaf key upgrade` adds the post-quantum keys to the old key file (its Ed25519 key, and so its
#    identity, is unchanged); signed again, the layer is hybrid and accepted
$MLEAF key upgrade legacy "$P3" | sed -n 1p
node -e 'if ((require("fs").statSync(process.argv[1]).mode & 0o777) !== 0o600) { console.error("key file not mode 600"); process.exit(1) }' "$MLEAF_KEYS/legacy.key"
$MLEAF sign "$P3" "$LEGACY" --key legacy > /dev/null
$MLEAF verify "$P3" --require > "$TMP/v.txt" 2>&1 || { cat "$TMP/v.txt"; echo "the upgraded key is still refused"; exit 1; }
echo "checked: an old key is classical only, refused after the policy's date, hybrid after mleaf key upgrade"
#    time-stamps of the signature files from the services the project chose (no default): here an
#    RFC 3161 TSA (openssl ts) and an OpenTimestamps calendar, both local test fakes, never public ones
node ../../tools/local-tsa.mjs "$TMP/tsa" > "$TMP/tsa.url" & TSA_PID=$!
node ../../tools/local-ots-calendar.mjs > "$TMP/cal.url" & CAL_PID=$!
trap 'kill $TSA_PID $CAL_PID 2> /dev/null || true' EXIT
for _ in $(seq 100); do [ -s "$TMP/tsa.url" ] && [ -s "$TMP/cal.url" ] && break; sleep 0.1; done
node -e 'const fs = require("fs"), [f, tsa, cal] = process.argv.slice(1), p = JSON.parse(fs.readFileSync(f));
  p.timestamps = { services: [{ kind: "rfc3161", url: tsa }, { kind: "opentimestamps", calendars: [cal] }] };
  fs.writeFileSync(f, JSON.stringify(p, null, 2)); console.log(JSON.stringify({ timestamps: p.timestamps }))' \
  "$P3/.mleaf/policy.json" "$(head -1 "$TMP/tsa.url")" "$(head -1 "$TMP/cal.url")" | sed 's|127.0.0.1:[0-9]*|127.0.0.1:<port>|g'
$MLEAF timestamp "$P3" "$LEGACY" | sed -e "s|$TMP|\$TMP|" -e 's|127.0.0.1:[0-9]*|127.0.0.1:<port>|g'
ls "$P3"/.mleaf/sigs/*/*.ots > /dev/null || { echo "no OpenTimestamps proof written"; exit 1; }
$MLEAF verify "$P3" > "$TMP/v.txt"
grep "$LEGACY" "$TMP/v.txt" | sed 's|127.0.0.1:[0-9]*|127.0.0.1:<port>|g'
grep "$LEGACY" "$TMP/v.txt" | grep -q "timestamped" || { echo "verify doesn't report the time-stamp"; exit 1; }
grep "$LEGACY" "$TMP/v.txt" | grep -q "OpenTimestamps pending at http://127.0.0.1" || { echo "verify doesn't report the pending proof"; exit 1; }
$MLEAF timestamp "$P3" "$LEGACY" --upgrade | sed 's|127.0.0.1:[0-9]*|127.0.0.1:<port>|g'
kill $CAL_PID
#    the TSA's signature and certificate, checked by openssl (mleaf checks the imprint and time only)
for f in "$P3"/.mleaf/sigs/*/*.tsr; do
  openssl ts -verify -data "${f%.tsr}" -in "$f" -CAfile "$TMP/tsa/ca.pem" > /dev/null 2>&1 || { echo "openssl rejects $f"; exit 1; }
done
kill $TSA_PID
echo "checked: every signature file time-stamped by a local TSA (openssl verifies the tokens) and a local calendar (pending)"
$MLEAF validate "$TMP/proj" | tail -1
#    the store composes exactly like the stack files with the same edits
$MLEAF compose "$TMP/proj" --out "$TMP/a.json" 2> /dev/null
$MLEAF compose with-edits.mleaf.json --out "$TMP/b.json" 2> /dev/null
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
  console.log("checked: the store composes exactly like with-edits.mleaf.json");' "$TMP/a.json" "$TMP/b.json"
rm -rf "$TMP"; unset MLEAF_KEYS

echo; echo "== 16. Formats: every route, what it keeps, and what this machine can do"
$MLEAF formats
TMP=$(mktemp -d)
#    one-way copies of the report (WordCraft's own writers keep no markers, and say so)
$MLEAF docx-export project.mleaf.json "$REPORT" --out "$TMP/report.md" 2>&1 | grep -E "warning|mleaf:" | head -2
#    without LibreOffice: ODT export falls back to WordCraft's writer; ODF imports are refused
#    with the reason, rather than guessing at every paragraph
MLEAF_SOFFICE=none $MLEAF formats | grep -E "^LibreOffice:"
MLEAF_SOFFICE=none $MLEAF docx-export project.mleaf.json "$REPORT" --out "$TMP/report.odt" 2>&1 | grep -E "writing|warning" | head -3
if MLEAF_SOFFICE=none $MLEAF pptx-import project.mleaf.json "$DECK" fixtures/deck.libreoffice-edited.odp --out "$TMP/x.mleaf.json" 2> "$TMP/err"; then
  echo "expected the ODP import to be refused without LibreOffice"; exit 1
fi
cat "$TMP/err"
#    how each import recognised its nodes: Impress drops slide names, so its slides are best guesses
$MLEAF pptx-import project.mleaf.json "$DECK" fixtures/deck.libreoffice-edited.pptx --out "$TMP/d.mleaf.json" 2>&1 | grep -E "matched|best guess"
$MLEAF pptx-import project.mleaf.json "$DECK" fixtures/deck.msoffice-edited.pptx --out "$TMP/d.mleaf.json" 2>&1 | grep -E "matched|best guess"
rm -rf "$TMP"

echo; echo "== 17. No silent loss (M2.5): adopt real files, export them again, and fail on any loss the report doesn't list"
#    tools/inventory counts the original and the re-export from their XML, independently of Milleleaf
TMP=$(mktemp -d)
for f in ../real/*.docx ../real/*.xlsx ../real/*.pptx; do
  MLEAF="${CARGO_TARGET_DIR:-$PWD/../../target}/debug/ox" ../../tools/inventory/roundtrip.sh "$f" "$TMP" > "$TMP/out.txt" 2>&1 || { cat "$TMP/out.txt"; echo "unlisted loss in $f"; exit 1; }
  echo "$(basename "$f"): $(tail -1 "$TMP/out.txt")"
done
rm -rf "$TMP"

echo; echo "== 18. Editor re-saves of the real files import as no change; real edits come back"
#    fixtures in examples/real/fixtures: LibreOffice 24.2 re-saves (made on the box), and Word, Excel and
#    PowerPoint 16 (Mac) re-saves and edits. A Microsoft Office fixture not made yet is said out loud
#    (PENDING) and its part skipped; see examples/real/SOURCES.md
OUT=$(mktemp)
( cd ../.. && cargo test -q -p milleleaf-wordcraft blank_line -- --nocapture && cargo test -q -p milleleaf-deckcraft real_deck -- --nocapture \
    && cargo test -q -p milleleaf-gridcraft real_workbook -- --ignored --nocapture ) > "$OUT" 2>&1 \
  || { cat "$OUT"; echo "real-file re-save test failed"; exit 1; }
# each filter must have run a test (a filter that matches nothing passes with 0)
[ "$(grep -cE 'test result: ok\. [1-9]' "$OUT")" -ge 3 ] || { cat "$OUT"; echo "a real-file test didn't run"; exit 1; }
grep -E 'test result: ok\. [1-9]|^PENDING' "$OUT"; rm -f "$OUT"

echo; echo "== 19. DocLang (M2.7): the composed project for AI tools, checked by the official validator"
#    one-way: content in DocLang elements, the rest as <custom> JSON; .dclx packs the pictures
TMP=$(mktemp -d)
$MLEAF doclang-export with-edits.mleaf.json --out "$TMP/pack.dclg" 2>&1 | grep -v "^mleaf:" || true
$MLEAF doclang-export with-edits.mleaf.json --out "$TMP/pack.dclx"
python3 -c 'import sys,zipfile; z=zipfile.ZipFile(sys.argv[1]); print("dclx:", ", ".join(sorted(z.namelist()))); open(sys.argv[2],"wb").write(z.read("document.xml"))' "$TMP/pack.dclx" "$TMP/document.xml"
if command -v doclang >/dev/null; then
  for f in pack.dclg document.xml; do
    doclang validate "$TMP/$f" > "$TMP/v.txt" 2>&1 || { cat "$TMP/v.txt"; echo "DocLang validation failed: $f"; exit 1; }
    echo "$f: $(grep -E 'VALIDATION' "$TMP/v.txt")"
  done
  # the validator does fail: a table with a short row is refused
  sed 's|<nl/></table>|<fcel/>x<nl/><ecel/><nl/></table>|' "$TMP/pack.dclg" > "$TMP/bad.dclg"
  if doclang validate "$TMP/bad.dclg" > /dev/null 2>&1; then echo "expected the validator to refuse a broken table"; exit 1; fi
  echo "checked: the validator refuses a non-rectangular table"
else
  echo "doclang not installed here: export written, not validated (the CI image has it)"
fi
rm -rf "$TMP"
