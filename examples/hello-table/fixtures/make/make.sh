#!/usr/bin/env bash
# Re-make the test files a program makes, and the inputs for the Microsoft Office saves. Run on Linux
# with LibreOffice 24.2.7.2, Node and a Rust toolchain (the project's CI container), from the repo root:
#   bash examples/hello-table/fixtures/make/make.sh _out/fixtures
# OUT/fixtures  -> examples/hello-table/fixtures (LibreOffice re-saves and edits)
# OUT/real      -> examples/real/fixtures (LibreOffice re-saves) and examples/real (DeckCraft's export)
# OUT/office    -> the packet for Microsoft Office (exports, the unlocked report, the real re-exports)
# finance.openpyxl-edited.xlsx is made by openpyxl_edit.py, from OUT/office/finance.xlsx.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../../../.." && pwd)
OUT=$(realpath -m "${1:?usage: make.sh OUTDIR}")
rm -rf "$OUT"; mkdir -p "$OUT"/{office,fixtures,real,lo,rt}
cargo build -q --manifest-path "$ROOT/Cargo.toml" --bin mleaf
export MLEAF="${CARGO_TARGET_DIR:-$ROOT/target}/debug/mleaf"
PROFILE="-env:UserInstallation=file://$OUT/lo/profile"
lo() { soffice "$PROFILE" --headless --norestore "$@" > /dev/null 2>&1; }

# Exports of the current project
cd "$ROOT/examples/hello-table"
DECK=$(node -e 'console.log(JSON.parse(require("fs").readFileSync("deck.proposal.mleaf.json")).data[1].path)')
"$MLEAF" docx-export project.mleaf.json 3f1c2a90-0000-4c1e-9a00-000000000020 --out "$OUT/office/report.docx"
"$MLEAF" xlsx-export project.mleaf.json 3f1c2a90-0000-4c1e-9a00-000000000010 --out "$OUT/office/finance.xlsx"
"$MLEAF" pptx-export project.mleaf.json "$DECK" --out "$OUT/office/deck.pptx"
# Word enforces content locks: a copy with them removed, for the overwrite that comes back as a conflict
python3 -I - "$OUT/office/report.docx" "$OUT/office/report-unlocked.docx" <<'PY'
import re, sys, zipfile
src, dst = zipfile.ZipFile(sys.argv[1]), sys.argv[2]
with zipfile.ZipFile(dst, "w") as out:
    for i in src.infolist():
        b = src.read(i.filename)
        if i.filename == "word/document.xml":
            b, n = re.subn(rb"<w:lock [^>]*/>", b"", b)
            assert n, "no locks found"
        out.writestr(i, b, compress_type=i.compress_type)
PY

# LibreOffice re-saves (no edits)
resave() {  # file format filter target
  mkdir -p "$OUT/lo/$4" && lo --convert-to "$2:$3" --outdir "$OUT/lo/$4" "$1"
  mv "$OUT/lo/$4/$(basename "${1%.*}").$2" "$4"
}
resave "$OUT/office/report.docx" docx "MS Word 2007 XML" "$OUT/fixtures/report.libreoffice-resaved.docx"
resave "$OUT/office/finance.xlsx" xlsx "Calc MS Excel 2007 XML" "$OUT/fixtures/finance.libreoffice-resaved.xlsx"
resave "$OUT/office/deck.pptx" pptx "Impress MS PowerPoint 2007 XML" "$OUT/fixtures/deck.libreoffice-resaved.pptx"

# LibreOffice edits, by the Basic macro Edits.xba
lo --terminate_after_init || true
B="$OUT/lo/profile/user/basic/Standard"
mkdir -p "$B" && cp "$ROOT/examples/hello-table/fixtures/make/Edits.xba" "$B/Edits.xba"
cat > "$B/script.xlb" <<'X'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE library:library PUBLIC "-//OpenOffice.org//DTD OfficeDocument 1.0//EN" "library.dtd">
<library:library xmlns:library="http://openoffice.org/2000/library" library:name="Standard" library:readonly="false" library:passwordprotected="false">
 <library:element library:name="Edits"/>
</library:library>
X
FX_IN="$OUT/office" FX_OUT="$OUT/fixtures" timeout 180 soffice "$PROFILE" --headless --invisible --norestore "macro:///Standard.Edits.Main" || true
for f in report.libreoffice-edited.{docx,odt} finance.libreoffice-edited.{xlsx,ods} deck.libreoffice-edited.{pptx,odp}; do
  [ -s "$OUT/fixtures/$f" ] || { echo "the macro didn't write $f" >&2; cat "$OUT/fixtures/macro.log" >&2 2> /dev/null; exit 1; }
done

# Real files: Milleleaf's export of each (roundtrip.sh), then LibreOffice re-saves of those
cd "$ROOT/examples/real"
for f in paper.docx lca-model.xlsx milleleaf-deck.impress.pptx; do
  "$ROOT/tools/inventory/roundtrip.sh" "$f" "$OUT/rt" > "$OUT/rt/${f%.*}.roundtrip.txt"
done
cp "$OUT/rt/paper.reexport.docx" "$OUT/rt/lca-model.reexport.xlsx" "$OUT/office/"
cp "$OUT/rt/milleleaf-deck.impress.reexport.pptx" "$OUT/office/milleleaf-deck.reexport.pptx"
cp "$OUT/rt/milleleaf-deck.impress.reexport.pptx" "$OUT/real/milleleaf-deck.deckcraft.pptx"
resave "$OUT/rt/paper.reexport.docx" docx "MS Word 2007 XML" "$OUT/real/paper.libreoffice-resaved.docx"
resave "$OUT/rt/lca-model.reexport.xlsx" xlsx "Calc MS Excel 2007 XML" "$OUT/real/lca-model.libreoffice-resaved.xlsx"
resave "$OUT/rt/milleleaf-deck.impress.reexport.pptx" pptx "Impress MS PowerPoint 2007 XML" "$OUT/real/milleleaf-deck.libreoffice-resaved.pptx"

cd "$OUT" && ls -l office fixtures real && sha256sum office/* > office/SHA256SUMS
