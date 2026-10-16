#!/usr/bin/env bash
# Adopt a real .docx/.xlsx/.pptx, export it again through Milleleaf, and compare the two with the
# independent inventory: what Milleleaf says it didn't carry vs what actually went missing.
#   tools/inventory/roundtrip.sh FILE OUTDIR
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OX="${OX:-$ROOT/target/debug/ox}"
f="$1"; out="$2"; mkdir -p "$out"
name="$(basename "${f%.*}")"; ext="${f##*.}"
"$OX" adopt "$f" --out "$out/$name.adopt.ox.json" --report "$out/$name.adopt-report.json" 2> "$out/$name.not-carried.txt"
root=$(python3 -I -c 'import json,sys;print(json.load(open(sys.argv[1]))["root"])' "$out/$name.adopt-report.json")
cat > "$out/$name.stack.ox.json" <<JSON
{ "header": { "id": "roundtrip/$name", "oxVersion": "ox_alpha", "dataVersion": "1.0.0", "author": "roundtrip", "timestamp": "$(date +%F)", "kind": "stack" },
  "imports": [ { "uri": "$ROOT/schema/office.ox.json" }, { "uri": "$name.adopt.ox.json" } ], "schemas": {}, "data": [] }
JSON
"$OX" validate "$out/$name.stack.ox.json" > "$out/$name.validate.txt" 2>&1 || echo "validate: problems (see $name.validate.txt)"
case "$ext" in
  docx) "$OX" docx-export "$out/$name.stack.ox.json" "$root" --out "$out/$name.reexport.docx" 2> "$out/$name.export.txt" ;;
  xlsx) "$OX" xlsx-export "$out/$name.stack.ox.json" "$root" --out "$out/$name.reexport.xlsx" 2> "$out/$name.export.txt" ;;
  pptx) "$OX" pptx-export "$out/$name.stack.ox.json" "$root" --out "$out/$name.reexport.pptx" 2> "$out/$name.export.txt" ;;
esac
python3 -I "$ROOT/tools/inventory/ooxml_inventory.py" "$f" "$out/$name.reexport.$ext" --json "$out/$name.inventory.json" --md "$out/$name.inventory.md" > /dev/null
echo "== $name.$ext: $(tail -1 "$out/$name.validate.txt")"
sed -n '1,200p' "$out/$name.inventory.md" | grep -E "\| (lost|fewer|more) \||words kept"
echo "-- Milleleaf said it didn't carry:"; sed 's/^/   /' "$out/$name.not-carried.txt"
echo "-- every loss reported?"
python3 -I "$ROOT/tools/inventory/check_losses.py" "$out/$name.inventory.json" "$out/$name.adopt-report.json"
