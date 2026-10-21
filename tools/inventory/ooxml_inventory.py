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
X14 = "{http://schemas.microsoft.com/office/spreadsheetml/2009/9/main}"
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


def fnv(s):
    """FNV-1a 32-bit of a string's UTF-8 (the same in the Rust counter): compares names as values."""
    h = 0x811C9DC5
    for byte in s.encode():
        h = ((h ^ byte) * 0x01000193) & 0xFFFFFFFF
    return h


def default_style(z):
    """The default paragraph style's font, size and justification (falling back to the document
    defaults), as numbers that can be compared exactly."""
    if "word/styles.xml" not in z.namelist():
        return {}
    st = xml(z, "word/styles.xml")
    dd = st.find(f"{W}docDefaults/{W}rPrDefault/{W}rPr")
    normal = next((e for e in st.iter(W + "style") if e.get(W + "type") == "paragraph" and e.get(W + "default") in ("1", "true")), None)
    def pick(path, attr):
        for root in (normal, dd if path.startswith("rPr") else None):
            if root is None:
                continue
            e = root.find(f"{W}{path.replace('/', '/' + W)}") if root is normal else root.find(W + path.split("/")[-1])
            if e is not None and e.get(W + attr) is not None:
                return e.get(W + attr)
        return None
    # A theme font (asciiTheme="minorHAnsi") is the theme's Latin typeface: what a reader sees.
    tp = parts(z, r"word/theme/theme\d*\.xml")
    A = "{http://schemas.openxmlformats.org/drawingml/2006/main}"
    def theme_font(t):
        if not tp or not t or not t.startswith(("minor", "major")):
            return None
        f = xml(z, tp[0]).find(f".//{A}{t[:5]}Font/{A}latin")
        return f.get("typeface") if f is not None else None
    font = None
    for root in (normal, dd):
        e = None if root is None else (root.find(f"{W}rPr/{W}rFonts") if root is normal else root.find(W + "rFonts"))
        if e is not None and (e.get(W + "ascii") or e.get(W + "asciiTheme")):
            font = e.get(W + "ascii") or theme_font(e.get(W + "asciiTheme"))
            break
    size = pick("rPr/sz", "val")
    jc = pick("pPr/jc", "val")
    return {
        "default style font (value, name hash)": fnv(font) if font else 0,
        "default style size (value, half-points)": int(size) if size else 0,
        "default style justified (value)": 1 if jc in ("both", "distribute") else 0,
    }


def docx(z):
    body = [xml(z, "word/document.xml")]
    hf = [xml(z, n) for n in parts(z, r"word/(header|footer)\d*\.xml")]
    notes = [xml(z, n) for n in parts(z, r"word/(footnotes|endnotes)\.xml")]
    styles_used = {e.get(W + "val") for r in body for tag in ("pStyle", "tblStyle") for e in r.iter(W + tag)}
    char_styles_used = {e.get(W + "val") for r in body for e in r.iter(W + "rStyle")}
    paras = [p for r in body for p in r.iter(W + "p")]
    in_tables = {id(p) for r in body for t in r.iter(W + "tbl") for p in t.iter(W + "p")}
    ppr = [p.find(W + "pPr") for p in paras if id(p) not in in_tables]
    ppr = [x for x in ppr if x is not None]
    cell_ppr = [x for x in (p.find(W + "pPr") for p in paras if id(p) in in_tables) if x is not None]
    formatted = lambda x: (x.find(W + "jc") is not None and x.find(W + "jc").get(W + "val") not in ("left", "start")) or x.find(W + "spacing") is not None or x.find(W + "ind") is not None
    # Values, compared exactly ("(value" in the name): the last section's page size and margins.
    pg = ([None] + [e for r in body for e in r.iter(W + "pgSz")])[-1]
    pm = ([None] + [e for r in body for e in r.iter(W + "pgMar")])[-1]
    val = lambda e, k: int(float(e.get(W + k, 0))) if e is not None else 0
    # Tables inside a table cell (counted on their own; their column widths aren't in the hash).
    nested = {id(t2) for r in body for t in r.iter(W + "tbl") for tc in t.iter(W + "tc") for t2 in tc.iter(W + "tbl")}
    nested_tbls = [t for r in body for t in r.iter(W + "tbl") if id(t) in nested]
    return {
        **default_style(z),
        "table column widths (value, hash)": fnv("|".join(",".join(g.get(W + "w", "") for g in t.findall(f"{W}tblGrid/{W}gridCol")) for r in body for t in r.iter(W + "tbl") if id(t) not in nested)),
        "table cells with shading": sum(1 for r in body for tc in r.iter(W + "tc") for sh in tc.findall(f"{W}tcPr/{W}shd") if sh.get(W + "fill") not in (None, "auto", "FFFFFF", "ffffff")),
        "style definitions": count([xml(z, "word/styles.xml")], W + "style") if "word/styles.xml" in z.namelist() else 0,
        "page width (value, twips)": val(pg, "w"),
        "page height (value, twips)": val(pg, "h"),
        "page margins top/right/bottom/left (value, twips)": sum(val(pm, k) * 10 ** (5 * i) for i, k in enumerate(("top", "right", "bottom", "left"))),
        "paragraphs aligned other than left (outside tables)": sum(1 for x in ppr if x.find(W + "jc") is not None and x.find(W + "jc").get(W + "val") not in ("left", "start")),
        "paragraphs with spacing set (outside tables)": sum(1 for x in ppr if x.find(W + "spacing") is not None),
        "paragraphs with indents set (outside tables)": sum(1 for x in ppr if x.find(W + "ind") is not None),
        "table cell paragraphs with alignment, spacing or indents": sum(1 for x in cell_ppr if formatted(x)),
        "paragraphs (non-empty)": sum(1 for p in paras if "".join(t.text or "" for t in p.iter(W + "t")).strip()),
        "tables": count(body, W + "tbl"),
        "nested tables": len(nested_tbls),
        "rows in nested tables": sum(len(t.findall(W + "tr")) for t in nested_tbls),
        "cells in nested tables": sum(len(tr.findall(W + "tc")) for t in nested_tbls for tr in t.findall(W + "tr")),
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
        "character styles used": len(char_styles_used - {None}),
        # Tracked insertions and deletions (moves too): the marks, not their text.
        "tracked changes (insertions and deletions)": sum(count(body, W + t) for t in ("ins", "del", "moveFrom", "moveTo")),
        "sections": count(body, W + "sectPr"),
        "hyperlinks": count(body, W + "hyperlink"),
        # A complex field is one fldChar begin, however many instrText pieces its instruction is split into.
        "fields": count(body, W + "fldSimple") + sum(1 for r in body for e in r.iter(W + "fldChar") if e.get(W + "fldCharType") == "begin"),
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


def cell_looks(styles, sheets):
    """Per cell, what its style shows: bold, a fill, borders, a number format other than General.
    Counts cells, not style entries: a writer may drop styles no cell uses."""
    if not styles:
        return {}
    st = styles[0]
    lists = {k: next(iter(st.iter(S + k)), None) for k in ("fonts", "fills", "borders", "cellXfs")}
    items = {k: list(v) if v is not None else [] for k, v in lists.items()}
    bold = [f.find(S + "b") is not None and f.find(S + "b").get("val") not in ("0", "false") for f in items["fonts"]]
    def filled(f):
        p = f.find(S + "patternFill")
        return f.find(S + "gradientFill") is not None or (p is not None and p.get("patternType") not in (None, "none", "gray125"))
    fill = [filled(f) for f in items["fills"]]
    border = [any(e.get("style") not in (None, "none") for e in b) for b in items["borders"]]
    xfs = [(int(x.get("fontId", 0)), int(x.get("fillId", 0)), int(x.get("borderId", 0)), int(x.get("numFmtId", 0))) for x in items["cellXfs"]]
    n = {"cells in bold": 0, "cells with a fill": 0, "cells with borders": 0, "cells with a number format": 0}
    at = lambda lst, i: lst[i] if 0 <= i < len(lst) else False
    for sh in sheets:
        for c in sh.iter(S + "c"):
            i = int(c.get("s", 0))
            if not 0 <= i < len(xfs):
                continue
            fo, fi, bo, nf = xfs[i]
            n["cells in bold"] += at(bold, fo)
            n["cells with a fill"] += at(fill, fi)
            n["cells with borders"] += at(border, bo)
            n["cells with a number format"] += nf != 0
    return n


def xlsx(z):
    sheets = [xml(z, n) for n in parts(z, r"xl/worksheets/sheet\d+\.xml")]
    styles = [xml(z, n) for n in parts(z, r"xl/styles\.xml")]
    wbx = [xml(z, n) for n in parts(z, r"xl/workbook\.xml")]
    shared = [xml(z, n) for n in parts(z, r"xl/sharedStrings\.xml")]
    cells = [c for s in sheets for c in s.iter(S + "c")]
    return {
        "sheets": len(sheets),
        "cells with a value": sum(1 for c in cells if c.find(S + "v") is not None or c.find(S + "is") is not None),
        "formulas": sum(1 for r in sheets for f in r.iter(S + "f") if f.get("t") != "dataTable"),
        # A what-if data table's anchor (t="dataTable") holds no formula text.
        "what-if data tables": sum(1 for r in sheets for f in r.iter(S + "f") if f.get("t") == "dataTable"),
        "merged ranges": count(sheets, S + "mergeCell"),
        "custom number formats": count(styles, S + "numFmt"),
        **cell_looks(styles, sheets),
        "cell styles (xf)": sum(len(list(x)) for st in styles for x in st.iter(S + "cellXfs")),
        "fonts": sum(len(list(x)) for st in styles for x in st.iter(S + "fonts")),
        "fills": sum(len(list(x)) for st in styles for x in st.iter(S + "fills")),
        "borders": sum(len(list(x)) for st in styles for x in st.iter(S + "borders")),
        "column width settings": count(sheets, S + "col"),
        "custom row heights": sum(1 for s in sheets for r in s.iter(S + "row") if r.get("customHeight") in ("1", "true")),
        "frozen panes": count(sheets, S + "pane"),
        "autofilters": count(sheets, S + "autoFilter"),
        # Excel 2010 keeps some (cross-sheet lists, newer rule types) in an x14 extension.
        "conditional formats": count(sheets, S + "conditionalFormatting") + count(sheets, X14 + "conditionalFormatting"),
        "data validations": count(sheets, S + "dataValidation") + count(sheets, X14 + "dataValidation"),
        "hyperlinks": count(sheets, S + "hyperlink"),
        # Milleleaf's own identity names (ox_…) would hide the loss of the file's own names.
        "hidden sheets": sum(1 for w in wbx for e in w.iter(S + "sheet") if e.get("state") in ("hidden", "veryHidden")),
        # Protection is on only with sheet="1"; without it the element only holds the settings.
        "protected sheets": sum(1 for r in sheets for e in r.iter(S + "sheetProtection") if e.get("sheet") in ("1", "true")),
        "workbook structure locked": sum(1 for w in wbx for e in w.iter(S + "workbookProtection") if e.get("lockStructure") in ("1", "true")),
        "sheets with gridlines off": sum(1 for sh in sheets for v in sh.iter(S + "sheetView") if v.get("showGridLines") in ("0", "false")),
        # Nor Excel's own record of a sheet's filter (written for every autoFilter, rebuilt from it).
        "defined names (not Milleleaf's)": sum(1 for w in wbx for d in w.iter(S + "definedName") if not (d.get("name") or "").startswith("ox_") and d.get("name") != "_xlnm._FilterDatabase"),
        "defined names into other workbooks": sum(1 for w in wbx for d in w.iter(S + "definedName") if not (d.get("name") or "").startswith("ox_") and re.search(r"\[\d+\]", d.text or "")),
        "charts": len(parts(z, r"xl/charts/chart\d*\.xml")),
        # Charts GridCraft has no kind for: it reads them as nothing.
        "surface charts": sum(1 for n in parts(z, r"xl/charts/chart\d*\.xml") if any(e.tag.split("}")[-1] in ("surfaceChart", "surface3DChart") for e in xml(z, n).iter())),
        "drawings": len(parts(z, r"xl/drawings/drawing\d*\.xml")),
        "media files": len(parts(z, r"xl/media/.+")),
        "external links": len(parts(z, r"xl/externalLinks/externalLink\d*\.xml")),
        "comments": sum(count([xml(z, n)], S + "comment") for n in parts(z, r"xl/comments\d*\.xml")),
        "tables (ListObjects)": len(parts(z, r"xl/tables/table\d*\.xml")),
        # Text lives in sharedStrings or inline in the cell (<is>), depending on the writer.
        "cell text chars": len(xlsx_text(shared, sheets)),
        "_text": xlsx_text(shared, sheets),
    }


MC = "{http://schemas.openxmlformats.org/markup-compatibility/2006}"


def fallback_copies(root):
    return {e for fb in root.iter(MC + "Fallback") for e in fb.iter()}


FILLS = ("solidFill", "gradFill", "pattFill", "blipFill")


def shape_look(sp):
    """(has a fill, has an outline): set on the shape (spPr), else from its style reference."""
    pr, st = sp.find(P + "spPr"), sp.find(P + "style")
    def ref(name):
        r = st.find(A + name) if st is not None else None
        return r is not None and r.get("idx") not in (None, "0")
    direct = [c.tag.split("}")[1] for c in pr] if pr is not None else []
    fill = any(t in FILLS for t in direct) if any(t in FILLS + ("noFill",) for t in direct) else ref("fillRef")
    ln = pr.find(A + "ln") if pr is not None else None
    lfill = [c.tag.split("}")[1] for c in ln] if ln is not None else []
    line = any(t in FILLS for t in lfill) if any(t in FILLS + ("noFill",) for t in lfill) else ref("lnRef")
    return fill, line


def pptx(z):
    slides = [xml(z, n) for n in parts(z, r"ppt/slides/slide\d+\.xml")]
    sz = xml(z, "ppt/presentation.xml").find(P + "sldSz")
    bold_text = re.sub(r"\s+", " ", "".join(t.text or "" for r in slides for run in r.iter(A + "r") if (run.find(A + "rPr") is not None and run.find(A + "rPr").get("b") in ("1", "true")) for t in run.iter(A + "t"))).strip()
    notes = [xml(z, n) for n in parts(z, r"ppt/notesSlides/notesSlide\d+\.xml")]
    return {
        "slide width (value, EMU)": int(sz.get("cx", 0)) if sz is not None else 0,
        "slide height (value, EMU)": int(sz.get("cy", 0)) if sz is not None else 0,
        # In text shapes only: tables and shapes without text are counted on their own.
        "slide paragraphs with an outline level": sum(1 for r in slides for sp in r.iter(P + "sp") if "".join(t.text or "" for t in sp.iter(A + "t")).strip() for pp in sp.iter(A + "pPr") if pp.get("lvl") not in (None, "0")),
        "slide bold text chars": len(bold_text),
        "shapes with a fill": sum(1 for r in slides for sp in r.iter(P + "sp") if shape_look(sp)[0]),
        "shapes with an outline": sum(1 for r in slides for sp in r.iter(P + "sp") if shape_look(sp)[1]),
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
