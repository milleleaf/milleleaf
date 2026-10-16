"""Counts what an OOXML file (.docx/.xlsx/.pptx) contains, straight from its XML, with no Milleleaf
code: so comparing an original with Milleleaf's re-export shows what was lost, including losses
Milleleaf doesn't know about (silent ones).

    python3 -I ooxml_inventory.py ORIGINAL [REEXPORT] [--json OUT] [--md OUT]

Inputs are untrusted: only zipfile and ElementTree are used (no macros, no external entities, no
code from the file). Run with -I and pass paths as arguments.
"""
import json, re, sys, zipfile
import xml.etree.ElementTree as ET

W = "{http://schemas.openxmlformats.org/wordprocessingml/2006/main}"
S = "{http://schemas.openxmlformats.org/spreadsheetml/2006/main}"
P = "{http://schemas.openxmlformats.org/presentationml/2006/main}"
A = "{http://schemas.openxmlformats.org/drawingml/2006/main}"
MAX_PART = 200 * 1024 * 1024  # refuse zip bombs


def parts(z, pattern):
    return sorted(n for n in z.namelist() if re.fullmatch(pattern, n))


def xml(z, name):
    info = z.getinfo(name)
    if info.file_size > MAX_PART:
        raise ValueError(f"{name} is too large ({info.file_size} bytes)")
    return ET.fromstring(z.read(name))


def count(roots, tag):
    return sum(1 for r in roots for _ in r.iter(tag))


def text_of(roots, tag, sep=""):
    # sep: "" for Word/PowerPoint runs (a word can be split across runs), " " for cells.
    t = sep.join(e.text or "" for r in roots for e in r.iter(tag))
    return re.sub(r"\s+", " ", t).strip()


def bold(run):
    rpr = run.find(W + "rPr")
    b = rpr.find(W + "b") if rpr is not None else None
    return b is not None and b.get(W + "val") not in ("0", "false", "off")


def docx(z):
    body = [xml(z, "word/document.xml")]
    hf = [xml(z, n) for n in parts(z, r"word/(header|footer)\d*\.xml")]
    notes = [xml(z, n) for n in parts(z, r"word/(footnotes|endnotes)\.xml")]
    styles_used = {e.get(W + "val") for r in body for tag in ("pStyle", "rStyle", "tblStyle") for e in r.iter(W + tag)}
    paras = [p for r in body for p in r.iter(W + "p")]
    return {
        "paragraphs (non-empty)": sum(1 for p in paras if "".join(t.text or "" for t in p.iter(W + "t")).strip()),
        "tables": count(body, W + "tbl"),
        "table rows": count(body, W + "tr"),
        "table cells": count(body, W + "tc"),
        "merged cells (gridSpan/vMerge)": count(body, W + "gridSpan") + count(body, W + "vMerge"),
        "images in body (drawings)": count(body, W + "drawing") + count(body, "{urn:schemas-microsoft-com:vml}imagedata"),
        "media files": len(parts(z, r"word/media/.+")),
        "charts": len(parts(z, r"word/charts/chart\d*\.xml")),
        "headers and footers (parts)": len(hf),
        "header/footer text chars": len(text_of(hf, W + "t")),
        # Real notes only: separator / continuation entries are the separator lines themselves.
        "footnotes and endnotes": sum(1 for r in notes for tag in ("footnote", "endnote") for e in r.iter(W + tag) if e.get(W + "type") in (None, "normal")),
        "footnote separator lines": sum(1 for r in notes for e in r.iter(W + "footnote") if e.get(W + "type") == "separator" and e.find(f".//{W}separator") is not None),
        "page setups (size and margins)": count(body, W + "pgSz") + count(body, W + "pgMar"),
        "comments": count([xml(z, n) for n in parts(z, r"word/comments\.xml")], W + "comment"),
        "list paragraphs (numPr)": count(body, W + "numPr"),
        "styles used": len(styles_used - {None}),
        "sections": count(body, W + "sectPr"),
        "hyperlinks": count(body, W + "hyperlink"),
        "fields": count(body, W + "fldSimple") + count(body, W + "instrText"),
        "text boxes": count(body, W + "txbxContent"),
        # Bold text, not bold elements: <w:b> also sits on paragraph marks and as w:val="0", and
        # a re-export may merge adjacent runs, so counting elements isn't a measure of loss.
        "bold text chars": len(re.sub(r"\s+", " ", "".join(t.text or "" for r in body for run in r.iter(W + "r") if bold(run) for t in run.iter(W + "t"))).strip()),
        "body text chars": len(text_of(body, W + "t")),
        "_text": text_of(body, W + "t"),
    }


def xlsx_text(shared, sheets):
    """Text of every cell that shows text, however the writer stored it: a shared string actually
    used by a cell, an inline string, or a formula's text result (t="str", kept in <v>)."""
    ss = ["".join(t.text or "" for t in si.iter(S + "t")) for r in shared for si in r.iter(S + "si")]
    out = []
    for sh in sheets:
        for c in sh.iter(S + "c"):
            t, v = c.get("t"), c.find(S + "v")
            if t == "s" and v is not None and v.text and v.text.isdigit() and int(v.text) < len(ss):
                out.append(ss[int(v.text)])
            elif t == "inlineStr":
                out.append("".join(x.text or "" for x in c.iter(S + "t")))
            elif t == "str" and v is not None:
                out.append(v.text or "")
    return re.sub(r"\s+", " ", " ".join(out)).strip()


def xlsx(z):
    sheets = [xml(z, n) for n in parts(z, r"xl/worksheets/sheet\d+\.xml")]
    styles = [xml(z, n) for n in parts(z, r"xl/styles\.xml")]
    wbx = [xml(z, n) for n in parts(z, r"xl/workbook\.xml")]
    shared = [xml(z, n) for n in parts(z, r"xl/sharedStrings\.xml")]
    cells = [c for s in sheets for c in s.iter(S + "c")]
    return {
        "sheets": len(sheets),
        "cells with a value": sum(1 for c in cells if c.find(S + "v") is not None or c.find(S + "is") is not None),
        "formulas": count(sheets, S + "f"),
        "merged ranges": count(sheets, S + "mergeCell"),
        "custom number formats": count(styles, S + "numFmt"),
        "cell styles (xf)": sum(len(list(x)) for st in styles for x in st.iter(S + "cellXfs")),
        "fonts": sum(len(list(x)) for st in styles for x in st.iter(S + "fonts")),
        "fills": sum(len(list(x)) for st in styles for x in st.iter(S + "fills")),
        "borders": sum(len(list(x)) for st in styles for x in st.iter(S + "borders")),
        "column width settings": count(sheets, S + "col"),
        "custom row heights": sum(1 for s in sheets for r in s.iter(S + "row") if r.get("customHeight") in ("1", "true")),
        "frozen panes": count(sheets, S + "pane"),
        "conditional formats": count(sheets, S + "conditionalFormatting"),
        "data validations": count(sheets, S + "dataValidation"),
        "hyperlinks": count(sheets, S + "hyperlink"),
        # Milleleaf's own identity names (ox_…) would hide the loss of the file's own names.
        "defined names (not Milleleaf's)": sum(1 for w in wbx for d in w.iter(S + "definedName") if not (d.get("name") or "").startswith("ox_")),
        "charts": len(parts(z, r"xl/charts/chart\d*\.xml")),
        "drawings": len(parts(z, r"xl/drawings/drawing\d*\.xml")),
        "media files": len(parts(z, r"xl/media/.+")),
        "comments": sum(count([xml(z, n)], S + "comment") for n in parts(z, r"xl/comments\d*\.xml")),
        "tables (ListObjects)": len(parts(z, r"xl/tables/table\d*\.xml")),
        # Text lives in sharedStrings or inline in the cell (<is>), depending on the writer.
        "cell text chars": len(xlsx_text(shared, sheets)),
        "_text": xlsx_text(shared, sheets),
    }


MC = "{http://schemas.openxmlformats.org/markup-compatibility/2006}"


def fallback_copies(root):
    return {e for fb in root.iter(MC + "Fallback") for e in fb.iter()}


def pptx(z):
    slides = [xml(z, n) for n in parts(z, r"ppt/slides/slide\d+\.xml")]
    notes = [xml(z, n) for n in parts(z, r"ppt/notesSlides/notesSlide\d+\.xml")]
    return {
        "slides": len(slides),
        "shapes (sp)": count(slides, P + "sp"),
        "pictures (pic)": count(slides, P + "pic"),
        "shapes without text": sum(1 for r in slides for sp in r.iter(P + "sp") if not "".join(t.text or "" for t in sp.iter(A + "t")).strip()),
        "groups": count(slides, P + "grpSp"),
        "connectors": count(slides, P + "cxnSp"),
        "tables": count(slides, A + "tbl"),
        "table cells": count(slides, A + "tc"),
        "charts": len(parts(z, r"ppt/charts/chart\d*\.xml")),
        "media files": len(parts(z, r"ppt/media/.+")),
        "notes slides": len(notes),
        "notes text chars": len(text_of(notes, A + "t")),
        "notes slide-number fields": sum(1 for r in notes for f in r.iter(A + "fld") if f.get("type") == "slidenum"),
        "layouts": len(parts(z, r"ppt/slideLayouts/slideLayout\d+\.xml")),
        "masters": len(parts(z, r"ppt/slideMasters/slideMaster\d+\.xml")),
        "animations (timing)": count(slides, P + "timing"),
        # Only transitions that do something (an effect or sound child, or auto-advance), and not
        # the mc:Fallback copy of one in mc:Choice: LibreOffice writes an empty one on every slide.
        "transitions": sum(1 for r in slides for t in r.iter(P + "transition") if t not in fallback_copies(r) and (len(t) or t.get("advTm") is not None)),
        # Sounds: on a transition (p:sndAc/p:stSnd/p:snd) or on an animation (p:audio, p:sndTgt).
        "transition sounds": count(slides, P + "stSnd"),
        "animation sounds": count(slides, P + "audio") + count(slides, P + "sndTgt"),
        "audio or video files": len([n for n in parts(z, r"ppt/media/.+") if re.search(r"\.(wav|mp3|m4a|aac|wma|mid|mp4|mov|m4v|wmv|avi)$", n, re.I)]),
        "hyperlinks": count(slides, A + "hlinkClick"),
        "table text chars": len(text_of([t for r in slides for t in r.iter(A + "tbl")], A + "t")),
        "slide text chars": len(text_of(slides, A + "t")),
        "_text": text_of(slides, A + "t"),
    }


def inventory(path):
    with zipfile.ZipFile(path) as z:
        names = set(z.namelist())
        app = ""
        if "docProps/app.xml" in names:
            a = xml(z, "docProps/app.xml").find("{http://schemas.openxmlformats.org/officeDocument/2006/extended-properties}Application")
            app = a.text if a is not None else ""
        if "word/document.xml" in names:
            kind, inv = "docx", docx(z)
        elif "xl/workbook.xml" in names:
            kind, inv = "xlsx", xlsx(z)
        elif "ppt/presentation.xml" in names:
            kind, inv = "pptx", pptx(z)
        else:
            raise ValueError(f"{path}: not a .docx, .xlsx or .pptx")
    return kind, app, inv


def words(t):
    return re.findall(r"\w+", t.lower())


def compare(orig, new):
    """Rows of (feature, original, re-export, verdict)."""
    rows = []
    for k, v in orig.items():
        if k.startswith("_"):
            continue
        n = new.get(k, 0)
        verdict = "kept" if n == v else ("lost" if n == 0 and v > 0 else ("fewer" if n < v else "more"))
        rows.append((k, v, n, verdict))
    a, b = words(orig.get("_text", "")), words(new.get("_text", ""))
    from collections import Counter
    ca, cb = Counter(a), Counter(b)
    kept = sum(min(ca[w], cb[w]) for w in ca)
    rows.append(("words kept", len(a), kept, f"{100 * kept / len(a):.1f}%" if a else "-"))
    # Emoji, symbols and marks aren't \w: count every other non-space code point too.
    sa = Counter(ch for ch in orig.get("_text", "") if not ch.isspace() and not re.match(r"\w", ch))
    sb = Counter(ch for ch in new.get("_text", "") if not ch.isspace() and not re.match(r"\w", ch))
    n, k = sum(sa.values()), sum(min(sa[c], sb[c]) for c in sa)
    rows.append(("symbols, emoji and marks kept", n, k, f"{100 * k / n:.1f}%" if n else "-"))
    return rows


def main(argv):
    args = [a for a in argv if not a.startswith("--")]
    opt = lambda name: argv[argv.index(name) + 1] if name in argv else None
    orig = args[0]
    k, app, inv = inventory(orig)
    out = {"file": orig, "kind": k, "application": app, "inventory": {x: y for x, y in inv.items() if not x.startswith("_")}}
    lines = [f"# Inventory: {orig} ({k}, made by {app or 'unknown'})", ""]
    if len(args) > 1 and args[1] not in (opt("--json"), opt("--md")):
        k2, app2, inv2 = inventory(args[1])
        rows = compare(inv, inv2)
        out["reexport"] = {"file": args[1], "application": app2, "inventory": {x: y for x, y in inv2.items() if not x.startswith("_")}}
        out["comparison"] = [dict(zip(("feature", "original", "reexport", "verdict"), r)) for r in rows]
        lines += [f"Re-export: {args[1]} (made by {app2 or 'unknown'})", "", "| Feature | Original | Re-export | |", "|---|---|---|---|"]
        lines += [f"| {f} | {a} | {b} | {v} |" for f, a, b, v in rows]
    else:
        lines += ["| Feature | Count |", "|---|---|"] + [f"| {x} | {y} |" for x, y in out["inventory"].items()]
    md = "\n".join(lines) + "\n"
    if opt("--json"):
        open(opt("--json"), "w").write(json.dumps(out, indent=2) + "\n")
    if opt("--md"):
        open(opt("--md"), "w").write(md)
    print(md)


if __name__ == "__main__":
    main(sys.argv[1:])
