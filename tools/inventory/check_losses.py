"""Fails when a round trip loses something Milleleaf didn't report (M2.5: no silent loss).

    python3 -I check_losses.py INVENTORY.json ADOPT-REPORT.json

INVENTORY.json is ooxml_inventory.py's comparison of the original and the re-export. ADOPT-REPORT.json
is `ox adopt --report`: its `inventory` (Milleleaf's own count of the original) and `losses`.
Two checks:
1. Milleleaf counted the original the same way as this independent tool (else its loss list can't
   be trusted).
2. every feature the re-export has fewer of is in the loss list. Text rows (words, symbols, `text
   chars`) may fall only as far as the text inside features listed as lost.
"""
import json, sys

inv = json.load(open(sys.argv[1]))
rep = json.load(open(sys.argv[2]))
ours = rep.get("inventory", {})
listed = {l["feature"]: l for l in rep.get("losses", [])}
problems = []

for k, v in inv["inventory"].items():
    if k in ours and ours[k] != v:
        problems.append(f"count differs: {k}: Milleleaf {ours[k]}, inventory {v}")

# A fall in one row is covered by a listed feature that takes that much with it: (feature, how much
# of the row each one can take). Tables take their text; a slide-number field its digits.
covered_by = {
    "slide text chars": ("table text chars", 1),
    "header/footer text chars": ("headers and footers (parts)", 10**9),
    "shapes (sp)": ("shapes without text", 1),
    # A shape without text that had a fill or an outline takes it with it.
    "shapes with a fill": ("shapes without text", 1),
    "shapes with an outline": ("shapes without text", 1),
    "notes text chars": ("notes slide-number fields", 4),
    "defined names (not Milleleaf's)": ("defined names into other workbooks", 1),
    # A nested table's text stays in its cell; the table, its rows and cells go.
    "tables": ("nested tables", 1),
    "table rows": ("rows in nested tables", 1),
    "table cells": ("cells in nested tables", 1),
}
text_loss_listed = any(k.endswith("text chars") or k in ("headers and footers (parts)", "tables") for k in listed)

for row in inv.get("comparison", []):
    f, a, b, verdict = row["feature"], row["original"], row["reexport"], row["verdict"]
    if f in ("words kept", "symbols, emoji and marks kept"):
        if b < a and not text_loss_listed:
            problems.append(f"unlisted loss: {f}: {b} of {a}")
        continue
    if "(value" in f:
        if a != b and f not in listed:
            problems.append(f"unlisted change: {f}: {a} → {b}")
        continue
    if verdict not in ("lost", "fewer") or f in listed:
        continue
    by, each = covered_by.get(f, (None, 0))
    if by in listed and a - b <= listed[by]["count"] * each + 2:
        continue
    problems.append(f"unlisted loss: {f}: {a} → {b}")

for p in problems:
    print(p)
print(f"{len(problems)} problem(s); {len(listed)} losses listed")
sys.exit(1 if problems else 0)
