//! What an OOXML file contains, counted from its XML, and which of those things Milleleaf's adapters
//! carry. `ox adopt` and the `*-import` commands report every counted feature an adapter doesn't
//! carry, so no loss is silent.
//!
//! The feature names are the same as `tools/inventory/ooxml_inventory.py`, an independent Python
//! count of the original and the re-export. `tools/inventory/roundtrip.sh` checks that every loss
//! the Python count sees is in this report: a wrong `carried` entry here fails there.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use quick_xml::events::{BytesStart, Event};
use quick_xml::name::ResolveResult;
use quick_xml::NsReader;
use serde::Serialize;

const W: &[u8] = b"http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const S: &[u8] = b"http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const P: &[u8] = b"http://schemas.openxmlformats.org/presentationml/2006/main";
const A: &[u8] = b"http://schemas.openxmlformats.org/drawingml/2006/main";
const V: &[u8] = b"urn:schemas-microsoft-com:vml";
const X14: &[u8] = b"http://schemas.microsoft.com/office/spreadsheetml/2009/9/main";
const MC: &[u8] = b"http://schemas.openxmlformats.org/markup-compatibility/2006";
/// Refuse zip bombs (the same limit as the Python tool).
const MAX_PART: u64 = 200 * 1024 * 1024;

pub type Counts = BTreeMap<&'static str, u64>;

/// One feature the file has that the adapter doesn't carry, or carries only in part.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Loss {
    pub feature: &'static str,
    pub count: u64,
    /// `"not carried"` or `"partly carried"`.
    pub carried: &'static str,
    pub note: &'static str,
}

/// `docx`, `xlsx` or `pptx`, and the counts. Errors only on a file that isn't one of the three.
pub fn inventory(bytes: &[u8]) -> Result<(&'static str, Counts), String> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("not a zip: {e}"))?;
    let names: Vec<String> = z.file_names().map(String::from).collect();
    let mut part = |n: &str| -> Result<Vec<u8>, String> {
        let f = z.by_name(n).map_err(|e| format!("{n}: {e}"))?;
        if f.size() > MAX_PART {
            return Err(format!("{n} is too large ({} bytes)", f.size()));
        }
        let mut b = Vec::new();
        f.take(MAX_PART).read_to_end(&mut b).map_err(|e| format!("{n}: {e}"))?;
        Ok(b)
    };
    let matching = |re: fn(&str) -> bool| -> Vec<String> {
        let mut v: Vec<String> = names.iter().filter(|n| re(n)).cloned().collect();
        v.sort();
        v
    };
    let has = |n: &str| names.iter().any(|x| x == n);
    if has("word/document.xml") {
        let body = part("word/document.xml")?;
        let hf: Vec<Vec<u8>> = matching(|n| numbered(n, "word/header") || numbered(n, "word/footer"))
            .iter()
            .map(|n| part(n))
            .collect::<Result<_, _>>()?;
        let notes: Vec<Vec<u8>> = matching(|n| n == "word/footnotes.xml" || n == "word/endnotes.xml")
            .iter()
            .map(|n| part(n))
            .collect::<Result<_, _>>()?;
        let comments: Vec<Vec<u8>> =
            matching(|n| n == "word/comments.xml").iter().map(|n| part(n)).collect::<Result<_, _>>()?;
        let styles = if has("word/styles.xml") { Some(part("word/styles.xml")?) } else { None };
        let theme = match matching(|n| numbered(n, "word/theme/theme")).first() {
            Some(n) => Some(part(n)?),
            None => None,
        };
        let mut c = docx(&body, &hf, &notes, &comments, &names);
        if let Some(st) = styles {
            default_style(&st, theme.as_deref(), &mut c);
        }
        Ok(("docx", c))
    } else if has("xl/workbook.xml") {
        let sheets: Vec<Vec<u8>> = matching(|n| numbered_req(n, "xl/worksheets/sheet"))
            .iter()
            .map(|n| part(n))
            .collect::<Result<_, _>>()?;
        let styles = if has("xl/styles.xml") { part("xl/styles.xml")? } else { Vec::new() };
        let wb = part("xl/workbook.xml")?;
        let comments: Vec<Vec<u8>> = matching(|n| numbered(n, "xl/comments"))
            .iter()
            .map(|n| part(n))
            .collect::<Result<_, _>>()?;
        // Charts GridCraft has no kind for (surface charts): it reads them as nothing.
        let mut surface = 0u64;
        for n in matching(|n| numbered(n, "xl/charts/chart")) {
            let mut found = false;
            walk(&part(&n)?, |ev| {
                if let Ev::Open(_, b"surfaceChart" | b"surface3DChart", _) = ev {
                    found = true;
                }
            });
            surface += u64::from(found);
        }
        let mut c = xlsx(&sheets, &styles, &wb, &comments, &names);
        c.insert("surface charts", surface);
        Ok(("xlsx", c))
    } else if has("ppt/presentation.xml") {
        let slides: Vec<Vec<u8>> = matching(|n| numbered_req(n, "ppt/slides/slide"))
            .iter()
            .map(|n| part(n))
            .collect::<Result<_, _>>()?;
        let notes: Vec<Vec<u8>> = matching(|n| numbered_req(n, "ppt/notesSlides/notesSlide"))
            .iter()
            .map(|n| part(n))
            .collect::<Result<_, _>>()?;
        let mut c = pptx(&slides, &notes, &names);
        let (mut w, mut h) = (0, 0);
        walk(&part("ppt/presentation.xml")?, |ev| {
            if let Ev::Open(P, b"sldSz", e) = ev {
                let n = |k: &[u8]| attr(e, k).and_then(|v| v.parse().ok()).unwrap_or(0);
                (w, h) = (n(b"cx"), n(b"cy"));
            }
        });
        c.insert("slide width (value, EMU)", w);
        c.insert("slide height (value, EMU)", h);
        Ok(("pptx", c))
    } else {
        Err("not a .docx, .xlsx or .pptx".into())
    }
}

/// `prefix` + optional digits + `.xml` (Python `prefix\d*\.xml`).
/// A formula that refers to another workbook: `[1]Sheet!A1`, `'[2]My sheet'!B2`.
pub fn external_ref(f: &str) -> bool {
    f.split('[').skip(1).any(|r| r.split(']').next().is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())))
}

fn numbered(n: &str, prefix: &str) -> bool {
    n.strip_prefix(prefix)
        .and_then(|r| r.strip_suffix(".xml"))
        .is_some_and(|d| d.bytes().all(|b| b.is_ascii_digit()))
}

/// `prefix` + one or more digits + `.xml` (Python `prefix\d+\.xml`).
fn numbered_req(n: &str, prefix: &str) -> bool {
    numbered(n, prefix) && !n.ends_with(&format!("{prefix}.xml"))
}

fn count_parts(names: &[String], f: impl Fn(&str) -> bool) -> u64 {
    names.iter().filter(|n| f(n)).count() as u64
}

/// A walk event: an element opening (empty ones are followed by `Close`), closing, or text.
enum Ev<'e> {
    Open(&'e [u8], &'e [u8], &'e BytesStart<'e>),
    Close(&'e [u8], &'e [u8]),
    Text(&'e str),
}

/// Walk one part, calling `f` with the namespace URI and local name of every element and the text.
fn walk(xml: &[u8], mut f: impl FnMut(Ev)) {
    let mut r = NsReader::from_reader(xml);
    let mut buf = Vec::new();
    loop {
        let Ok((res, ev)) = r.read_resolved_event_into(&mut buf) else { break };
        let ns: Vec<u8> = match res {
            ResolveResult::Bound(n) => n.as_ref().to_vec(),
            _ => Vec::new(),
        };
        match ev {
            Event::Start(e) => f(Ev::Open(&ns, e.local_name().as_ref(), &e)),
            Event::Empty(e) => {
                f(Ev::Open(&ns, e.local_name().as_ref(), &e));
                f(Ev::Close(&ns, e.local_name().as_ref()));
            }
            Event::End(e) => f(Ev::Close(&ns, e.local_name().as_ref())),
            Event::Text(t) => {
                if let Ok(s) = t.decode() {
                    f(Ev::Text(&s));
                }
            }
            Event::CData(t) => {
                if let Ok(s) = t.decode() {
                    f(Ev::Text(&s));
                }
            }
            Event::GeneralRef(g) => {
                let name = g.decode().unwrap_or_default();
                let ch = match name.as_ref() {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    n => n
                        .strip_prefix("#x")
                        .and_then(|h| u32::from_str_radix(h, 16).ok())
                        .or_else(|| n.strip_prefix('#').and_then(|d| d.parse().ok()))
                        .and_then(char::from_u32),
                };
                if let Some(c) = ch {
                    f(Ev::Text(c.encode_utf8(&mut [0; 4])));
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
}

#[allow(deprecated)] // unescape_value: XML 1.0 entity rules are what these files use
fn attr(e: &BytesStart, local: &[u8]) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.local_name().as_ref() == local)
        .and_then(|a| a.unescape_value().ok().map(|v| v.into_owned()))
}

/// Characters after collapsing whitespace runs to one space and trimming (Python's `text_of`).
fn norm_len(s: &str) -> u64 {
    s.split_whitespace().map(|w| w.chars().count() as u64 + 1).sum::<u64>().saturating_sub(1)
}

/// How many `tag` elements in namespace `ns`.
fn count_tag(xml: &[u8], ns: &[u8], tag: &[u8]) -> u64 {
    let mut n = 0;
    walk(xml, |ev| {
        if let Ev::Open(x, l, _) = ev {
            if x == ns && l == tag {
                n += 1;
            }
        }
    });
    n
}

/// The text inside `tag` elements in namespace `ns`.
fn text_in(xml: &[u8], ns: &[u8], tag: &[u8]) -> String {
    let (mut inside, mut out) = (0usize, String::new());
    walk(xml, |ev| match ev {
        Ev::Open(x, l, _) if x == ns && l == tag => inside += 1,
        Ev::Close(x, l) if x == ns && l == tag => inside = inside.saturating_sub(1),
        Ev::Text(t) if inside > 0 => out.push_str(t),
        _ => {}
    });
    out
}

/// FNV-1a 32-bit of a string's UTF-8 (the same in the Python counter): compares names as values.
fn fnv(s: &str) -> u64 {
    s.bytes().fold(0x811C_9DC5u32, |h, b| (h ^ b as u32).wrapping_mul(0x0100_0193)) as u64
}

/// The default paragraph style's font, size and justification (font and size falling back to
/// the document defaults), and how many styles the document defines.
fn default_style(st: &[u8], theme: Option<&[u8]>, c: &mut Counts) {
    // A font given as the theme's (asciiTheme="minorHAnsi") is the theme's Latin typeface: what a
    // reader sees, and what an editor writing the name out writes.
    let (mut minor, mut major, mut in_minor, mut in_major) = (None, None, false, false);
    walk(theme.unwrap_or_default(), |ev| match ev {
        Ev::Open(n, l, e) if n == A => match l {
            b"minorFont" => in_minor = true,
            b"majorFont" => in_major = true,
            b"latin" if in_minor && minor.is_none() => minor = attr(e, b"typeface"),
            b"latin" if in_major && major.is_none() => major = attr(e, b"typeface"),
            _ => {}
        },
        Ev::Close(n, l) if n == A => match l {
            b"minorFont" => in_minor = false,
            b"majorFont" => in_major = false,
            _ => {}
        },
        _ => {}
    });
    let font_of = |e: &BytesStart| {
        attr(e, b"ascii").or_else(|| match attr(e, b"asciiTheme")?.as_str() {
            t if t.starts_with("minor") => minor.clone(),
            t if t.starts_with("major") => major.clone(),
            _ => None,
        })
    };
    // Where we are: in docDefaults' run properties, or in the default paragraph style's rPr / pPr.
    let (mut in_dd, mut in_normal, mut in_rpr, mut in_ppr) = (false, false, false, false);
    let (mut n_font, mut n_size, mut n_jc, mut d_font, mut d_size) = (None, None, None, None, None);
    let mut styles = 0u64;
    walk(st, |ev| match ev {
        Ev::Open(n, l, e) if n == W => match l {
            b"rPrDefault" => in_dd = true,
            b"style" => {
                styles += 1;
                in_normal = attr(e, b"type").as_deref() == Some("paragraph")
                    && matches!(attr(e, b"default").as_deref(), Some("1" | "true"));
            }
            b"rPr" => in_rpr = true,
            b"pPr" => in_ppr = true,
            b"rFonts" if in_rpr && in_normal => n_font = font_of(e),
            b"rFonts" if in_dd => d_font = font_of(e),
            b"sz" if in_rpr && in_normal => n_size = attr(e, b"val"),
            b"sz" if in_dd => d_size = attr(e, b"val"),
            b"jc" if in_ppr && in_normal => n_jc = attr(e, b"val"),
            _ => {}
        },
        Ev::Close(n, l) if n == W => match l {
            b"rPrDefault" => in_dd = false,
            b"style" => in_normal = false,
            b"rPr" => in_rpr = false,
            b"pPr" => in_ppr = false,
            _ => {}
        },
        _ => {}
    });
    let font = n_font.or(d_font);
    c.insert("default style font (value, name hash)", font.as_deref().map(fnv).unwrap_or(0));
    c.insert("default style size (value, half-points)", n_size.or(d_size).and_then(|v| v.parse().ok()).unwrap_or(0));
    c.insert("default style justified (value)", u64::from(matches!(n_jc.as_deref(), Some("both" | "distribute"))));
    c.insert("style definitions", styles);
}

fn docx(body: &[u8], hf: &[Vec<u8>], notes: &[Vec<u8>], comments: &[Vec<u8>], names: &[String]) -> Counts {
    let mut c = Counts::new();
    // Body: one pass, tracking open paragraphs (non-empty if any w:t inside has text), the styles
    // used, and bold text (w:t inside a run whose w:rPr has w:b that isn't off).
    let count = [
        "tbl", "tr", "tc", "gridSpan", "vMerge", "drawing", "pgSz", "pgMar", "numPr", "sectPr", "hyperlink",
        "fldSimple", "txbxContent",
    ];
    let mut tags: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut paras: Vec<bool> = Vec::new();
    let mut nonempty = 0u64;
    let (mut styles, mut char_styles) = (std::collections::BTreeSet::new(), std::collections::BTreeSet::new());
    // Bold counts only a w:b in the run's own w:rPr (not in w:rPrChange, a tracked change).
    let (mut in_t, mut run_bold, mut depth) = (0usize, false, 0usize);
    let mut runs: Vec<usize> = Vec::new();
    let (mut text, mut bold_text) = (String::new(), String::new());
    // Paragraph properties: a child of w:p's own w:pPr (depth p+2) is the paragraph's.
    let mut para_at: Vec<usize> = Vec::new();
    let (mut aligned, mut spaced, mut indented) = (0u64, 0u64, 0u64);
    // Inside tables: (table depth, is the open cell paragraph formatted yet), and the count.
    let (mut tables, mut cell_formatted, mut cell_paras) = (0usize, false, 0u64);
    let (mut page, mut margins) = ([0u64; 2], [0u64; 4]);
    // Table column widths (every table's gridCol w, joined) and shaded cells (w:tc/w:tcPr/w:shd).
    let (mut grids, mut grid_seen, mut shaded) = (String::new(), false, 0u64);
    let mut tc_at: Vec<usize> = Vec::new();
    let num = |e: &BytesStart, k: &[u8]| attr(e, k).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0) as u64;
    walk(body, |ev| match ev {
        Ev::Open(n, l, e) if n == W => {
            depth += 1;
            if para_at.last().map(|d| d + 2) == Some(depth) {
                let f = match l {
                    b"jc" => !matches!(attr(e, b"val").as_deref(), Some("left" | "start")),
                    b"spacing" | b"ind" => true,
                    _ => false,
                };
                if tables > 0 {
                    cell_formatted |= f;
                } else {
                    match l {
                        b"jc" if f => aligned += 1,
                        b"spacing" => spaced += 1,
                        b"ind" => indented += 1,
                        _ => {}
                    }
                }
            }
            if l == b"tbl" {
                tables += 1;
                if tables > 1 {
                    *tags.entry("nested tables").or_default() += 1;
                }
            }
            if tables > 1 && l == b"tr" {
                *tags.entry("rows in nested tables").or_default() += 1;
            }
            if tables > 1 && l == b"tc" {
                *tags.entry("cells in nested tables").or_default() += 1;
            }
            if l == b"tc" {
                tc_at.push(depth);
            }
            if l == b"hyperlink" && !tc_at.is_empty() {
                *tags.entry("hyperlinks in table cells").or_default() += 1;
            }
            if l == b"shd"
                && tc_at.last().map(|d| d + 2) == Some(depth)
                && !matches!(attr(e, b"fill").as_deref(), None | Some("auto" | "FFFFFF" | "ffffff"))
            {
                shaded += 1;
            }
            // A nested table's column widths go with it (counted as a nested table).
            if l == b"tblGrid" && tables == 1 {
                if grid_seen {
                    grids.push('|');
                }
                grid_seen = true;
            }
            if l == b"gridCol" && tables == 1 {
                if !grids.is_empty() && !grids.ends_with('|') {
                    grids.push(',');
                }
                grids.push_str(&attr(e, b"w").unwrap_or_default());
            }
            // A complex field is one fldChar begin, however many instrText pieces Word split
            // its instruction into.
            if l == b"fldChar" && attr(e, b"fldCharType").as_deref() == Some("begin") {
                *tags.entry("fldChar begin").or_default() += 1;
            }
            match l {
                b"pgSz" => page = [num(e, b"w"), num(e, b"h")],
                b"pgMar" => margins = [num(e, b"top"), num(e, b"right"), num(e, b"bottom"), num(e, b"left")],
                _ => {}
            }
            let l = std::str::from_utf8(l).unwrap_or("");
            if let Some(k) = count.iter().find(|k| **k == l) {
                *tags.entry(k).or_default() += 1;
            }
            match l {
                "p" => {
                    paras.push(false);
                    para_at.push(depth);
                }
                "t" => in_t += 1,
                "r" => {
                    runs.push(depth);
                    run_bold = false;
                }
                "b" if runs.last().map(|d| d + 2) == Some(depth) => run_bold = !matches!(attr(e, b"val").as_deref(), Some("0" | "false" | "off")),
                "pStyle" | "tblStyle" => {
                    if let Some(v) = attr(e, b"val") {
                        styles.insert(v);
                    }
                }
                "rStyle" => {
                    if let Some(v) = attr(e, b"val") {
                        char_styles.insert(v);
                    }
                }
                // Tracked insertions and deletions (moves too): the marks, not their text.
                "ins" | "del" | "moveFrom" | "moveTo" => *tags.entry("tracked changes").or_default() += 1,
                _ => {}
            }
        }
        Ev::Open(n, l, _) => {
            depth += 1;
            if n == V && l == b"imagedata" {
                *tags.entry("drawing").or_default() += 1;
            }
        }
        Ev::Close(n, l) => {
            depth -= 1;
            if n != W {
                return;
            }
            match l {
            b"tbl" => tables -= 1,
            b"tc" => {
                tc_at.pop();
            }
            b"p" => {
                para_at.pop();
                if tables > 0 && std::mem::take(&mut cell_formatted) {
                    cell_paras += 1;
                }
                if paras.pop() == Some(true) {
                    nonempty += 1;
                }
            }
            b"t" => in_t = in_t.saturating_sub(1),
            b"r" => {
                runs.pop();
                run_bold = false;
            }
            _ => {}
            }
        }
        Ev::Text(t) if in_t > 0 => {
            text.push_str(t);
            if !t.trim().is_empty() {
                paras.iter_mut().for_each(|p| *p = true);
            }
            if run_bold && !runs.is_empty() {
                bold_text.push_str(t);
            }
        }
        _ => {}
    });
    let g = |k: &str| tags.get(k).copied().unwrap_or(0);
    let hf_text: String = hf.iter().map(|x| text_in(x, W, b"t")).collect();
    let (mut real_notes, mut separators) = (0u64, 0u64);
    for x in notes {
        // A note is real when its w:type is absent or "normal"; a separator line is a footnote of
        // type "separator" that holds w:separator.
        let mut stack: Vec<(bool, bool)> = Vec::new();
        walk(x, |ev| match ev {
            Ev::Open(n, l, e) if n == W && (l == b"footnote" || l == b"endnote") => {
                let ty = attr(e, b"type");
                if matches!(ty.as_deref(), None | Some("normal")) {
                    real_notes += 1;
                }
                stack.push((l == b"footnote" && ty.as_deref() == Some("separator"), false));
            }
            Ev::Open(n, b"separator", _) if n == W => {
                if let Some(top) = stack.last_mut() {
                    top.1 = true;
                }
            }
            Ev::Close(n, l) if n == W && (l == b"footnote" || l == b"endnote") => {
                if stack.pop() == Some((true, true)) {
                    separators += 1;
                }
            }
            _ => {}
        });
    }
    c.insert("table column widths (value, hash)", if grid_seen { fnv(&grids) } else { fnv("") });
    c.insert("table cells with shading", shaded);
    c.insert("page width (value, twips)", page[0]);
    c.insert("page height (value, twips)", page[1]);
    c.insert(
        "page margins top/right/bottom/left (value, twips)",
        margins.iter().enumerate().map(|(i, m)| m * 10u64.pow(5 * i as u32)).sum(),
    );
    c.insert("paragraphs aligned other than left (outside tables)", aligned);
    c.insert("paragraphs with spacing set (outside tables)", spaced);
    c.insert("paragraphs with indents set (outside tables)", indented);
    c.insert("table cell paragraphs with alignment, spacing or indents", cell_paras);
    c.insert("paragraphs (non-empty)", nonempty);
    c.insert("tables", g("tbl"));
    c.insert("table rows", g("tr"));
    c.insert("table cells", g("tc"));
    c.insert("merged cells (gridSpan/vMerge)", g("gridSpan") + g("vMerge"));
    c.insert("images in body (drawings)", g("drawing"));
    c.insert("media files", count_parts(names, |n| n.starts_with("word/media/") && n.len() > 11));
    c.insert("charts", count_parts(names, |n| numbered(n, "word/charts/chart")));
    c.insert("headers and footers (parts)", hf.len() as u64);
    c.insert("header/footer text chars", norm_len(&hf_text));
    c.insert("footnotes and endnotes", real_notes);
    c.insert("footnote separator lines", separators);
    c.insert("page setups (size and margins)", g("pgSz") + g("pgMar"));
    c.insert("comments", comments.iter().map(|x| count_tag(x, W, b"comment")).sum());
    c.insert("list paragraphs (numPr)", g("numPr"));
    c.insert("styles used", styles.len() as u64);
    c.insert("character styles used", char_styles.len() as u64);
    c.insert("tracked changes (insertions and deletions)", g("tracked changes"));
    c.insert("sections", g("sectPr"));
    c.insert("nested tables", g("nested tables"));
    c.insert("rows in nested tables", g("rows in nested tables"));
    c.insert("cells in nested tables", g("cells in nested tables"));
    c.insert("hyperlinks", g("hyperlink"));
    c.insert("hyperlinks in table cells", g("hyperlinks in table cells"));
    c.insert("fields", g("fldSimple") + g("fldChar begin"));
    c.insert("text boxes", g("txbxContent"));
    c.insert("bold text chars", norm_len(&bold_text));
    c.insert("body text chars", norm_len(&text));
    c
}

/// Per cell style (`cellXfs` index): (bold, filled, bordered, a number format other than General).
fn xf_looks(styles: &[u8]) -> Vec<[bool; 4]> {
    let (mut bold, mut fill, mut border) = (Vec::new(), Vec::new(), Vec::new());
    let mut xfs: Vec<(usize, usize, usize, bool)> = Vec::new();
    // Which list we're in and the depth of its items.
    let (mut list, mut depth): (&[u8], usize) = (b"", 0);
    let mut list_depth = 0usize;
    walk(styles, |ev| match ev {
        Ev::Open(n, l, e) => {
            depth += 1;
            if n != S {
                return;
            }
            match l {
                b"fonts" | b"fills" | b"borders" | b"cellXfs" if list.is_empty() => {
                    list = match l {
                        b"fonts" => b"fonts",
                        b"fills" => b"fills",
                        b"borders" => b"borders",
                        _ => b"cellXfs",
                    };
                    list_depth = depth;
                }
                _ if list.is_empty() => {}
                _ if depth == list_depth + 1 => match list {
                    b"fonts" => bold.push(false),
                    b"fills" => fill.push(false),
                    b"borders" => border.push(false),
                    _ => {
                        let id = |k: &[u8]| attr(e, k).and_then(|v| v.parse().ok()).unwrap_or(0usize);
                        xfs.push((id(b"fontId"), id(b"fillId"), id(b"borderId"), id(b"numFmtId") != 0));
                    }
                },
                b"b" if list == b"fonts" && depth == list_depth + 2 => {
                    if let Some(x) = bold.last_mut() {
                        *x = !matches!(attr(e, b"val").as_deref(), Some("0" | "false"));
                    }
                }
                b"gradientFill" if list == b"fills" => {
                    if let Some(x) = fill.last_mut() {
                        *x = true;
                    }
                }
                b"patternFill" if list == b"fills" => {
                    if let Some(x) = fill.last_mut() {
                        *x = !matches!(attr(e, b"patternType").as_deref(), None | Some("none" | "gray125"));
                    }
                }
                _ if list == b"borders" && depth == list_depth + 2 => {
                    if let Some(x) = border.last_mut() {
                        *x |= !matches!(attr(e, b"style").as_deref(), None | Some("none"));
                    }
                }
                _ => {}
            }
        }
        Ev::Close(..) => {
            if depth == list_depth && !list.is_empty() {
                list = b"";
            }
            depth -= 1;
        }
        _ => {}
    });
    let at = |v: &[bool], i: usize| v.get(i).copied().unwrap_or(false);
    xfs.iter().map(|&(fo, fi, bo, nf)| [at(&bold, fo), at(&fill, fi), at(&border, bo), nf]).collect()
}

fn xlsx(sheets: &[Vec<u8>], styles: &[u8], wb: &[u8], comments: &[Vec<u8>], names: &[String]) -> Counts {
    let mut t: BTreeMap<&'static str, u64> = BTreeMap::new();
    let looks = xf_looks(styles);
    const LOOKS: [&str; 4] = ["cells in bold", "cells with a fill", "cells with borders", "cells with a number format"];
    for k in LOOKS {
        t.insert(k, 0);
    }
    for s in sheets {
        let (mut in_c, mut has_value) = (false, false);
        walk(s, |ev| match ev {
            // Excel 2010 keeps some (cross-sheet lists, newer rule types) in an x14 extension.
            Ev::Open(X14, b"conditionalFormatting", _) => *t.entry("conditional formats").or_default() += 1,
            Ev::Open(X14, b"dataValidation", _) => *t.entry("data validations").or_default() += 1,
            Ev::Open(n, l, e) if n == S => {
                let k = match l {
                    b"c" => {
                        in_c = true;
                        has_value = false;
                        let i = attr(e, b"s").and_then(|v| v.parse().ok()).unwrap_or(0usize);
                        if let Some(look) = looks.get(i) {
                            for (k, on) in LOOKS.iter().zip(look) {
                                *t.entry(k).or_default() += u64::from(*on);
                            }
                        }
                        return;
                    }
                    b"v" | b"is" => {
                        has_value |= in_c;
                        return;
                    }
                    // A what-if data table's anchor (`t="dataTable"`) holds no formula text.
                    b"f" if attr(e, b"t").as_deref() == Some("dataTable") => "what-if data tables",
                    b"f" => "formulas",
                    b"mergeCell" => "merged ranges",
                    b"col" => "column width settings",
                    b"row" if matches!(attr(e, b"customHeight").as_deref(), Some("1" | "true")) => "custom row heights",
                    b"pane" => "frozen panes",
                    b"conditionalFormatting" => "conditional formats",
                    b"dataValidation" => "data validations",
                    b"hyperlink" => "hyperlinks",
                    // Protection is on only with sheet="1"; without it the element only holds
                    // the settings protection would use (Excel writes those too).
                    b"autoFilter" => "autofilters",
                    b"sheetProtection" if matches!(attr(e, b"sheet").as_deref(), Some("1" | "true")) => "protected sheets",
                    b"sheetView" if matches!(attr(e, b"showGridLines").as_deref(), Some("0" | "false")) => "sheets with gridlines off",
                    _ => return,
                };
                *t.entry(k).or_default() += 1;
            }
            Ev::Close(n, b"c") if n == S => {
                if has_value {
                    *t.entry("cells with a value").or_default() += 1;
                }
                in_c = false;
            }
            _ => {}
        });
    }
    // Styles: numFmt elements, and the direct children of cellXfs, fonts, fills and borders.
    let (mut depth, mut list): (usize, Option<(&'static str, usize)>) = (0, None);
    walk(styles, |ev| match ev {
        Ev::Open(n, l, _) => {
            depth += 1;
            if n != S {
                return;
            }
            if let Some((k, d)) = list {
                if depth == d + 1 {
                    *t.entry(k).or_default() += 1;
                }
                return;
            }
            let k = match l {
                b"numFmt" => {
                    *t.entry("custom number formats").or_default() += 1;
                    return;
                }
                b"cellXfs" => "cell styles (xf)",
                b"fonts" => "fonts",
                b"fills" => "fills",
                b"borders" => "borders",
                _ => return,
            };
            list = Some((k, depth));
        }
        Ev::Close(..) => {
            if list.is_some_and(|(_, d)| d == depth) {
                list = None;
            }
            depth = depth.saturating_sub(1);
        }
        _ => {}
    });
    let (mut own_names, mut hidden, mut locked) = (0u64, 0u64, 0u64);
    // Names that point into another workbook (`[1]Sheet!A1`), through an external link.
    let (mut external_names, mut in_name) = (0u64, false);
    walk(wb, |ev| match ev {
        // Not Milleleaf's (`ox_…`), nor Excel's own record of a sheet's filter (`_xlnm._FilterDatabase`,
        // which Excel writes for every autoFilter and rebuilds from it; counted as "autofilters").
        Ev::Open(S, b"definedName", e) if !attr(e, b"name").is_some_and(|n| n.starts_with("ox_") || n == "_xlnm._FilterDatabase") => {
            own_names += 1;
            in_name = true;
        }
        Ev::Close(S, b"definedName") => in_name = false,
        Ev::Text(t) if in_name && external_ref(t) => external_names += 1,
        Ev::Open(S, b"sheet", e) if matches!(attr(e, b"state").as_deref(), Some("hidden" | "veryHidden")) => hidden += 1,
        Ev::Open(S, b"workbookProtection", e) if matches!(attr(e, b"lockStructure").as_deref(), Some("1" | "true")) => locked += 1,
        _ => {}
    });
    let g = |k: &str| t.get(k).copied().unwrap_or(0);
    let mut c = Counts::new();
    c.insert("sheets", sheets.len() as u64);
    for k in [
        "cells in bold", "cells with a fill", "cells with borders", "cells with a number format",
        "cells with a value", "formulas", "merged ranges", "custom number formats", "cell styles (xf)", "fonts",
        "fills", "borders", "column width settings", "custom row heights", "frozen panes", "conditional formats",
        "data validations", "hyperlinks", "what-if data tables", "autofilters",
    ] {
        c.insert(k, g(k));
    }
    c.insert("defined names (not Milleleaf's)", own_names);
    c.insert("defined names into other workbooks", external_names);
    c.insert("hidden sheets", hidden);
    c.insert("workbook structure locked", locked);
    c.insert("protected sheets", g("protected sheets"));
    c.insert("sheets with gridlines off", g("sheets with gridlines off"));
    c.insert("charts", count_parts(names, |n| numbered(n, "xl/charts/chart")));
    c.insert("drawings", count_parts(names, |n| numbered(n, "xl/drawings/drawing")));
    c.insert("media files", count_parts(names, |n| n.starts_with("xl/media/") && n.len() > 9));
    c.insert("external links", count_parts(names, |n| numbered(n, "xl/externalLinks/externalLink")));
    c.insert("comments", comments.iter().map(|x| count_tag(x, S, b"comment")).sum());
    c.insert("tables (ListObjects)", count_parts(names, |n| numbered(n, "xl/tables/table")));
    c
}

/// A shape's fill and outline as the counters see them: set on the shape (`p:spPr`), else from its
/// style's `fillRef` / `lnRef` (index 0 is none).
#[derive(Default)]
struct SpLook {
    depth: usize,
    fill: Option<bool>,
    line: Option<bool>,
    fill_ref: bool,
    line_ref: bool,
    in_ln: bool,
}

impl SpLook {
    const FILLS: [&[u8]; 4] = [b"solidFill", b"gradFill", b"pattFill", b"blipFill"];
    /// An element opening at `depth` (the shape is at `self.depth`).
    fn see(&mut self, n: &[u8], l: &[u8], e: &BytesStart, depth: usize) {
        let filled = Self::FILLS.contains(&l);
        match (n, depth - self.depth) {
            // p:sp > p:style > a:fillRef / a:lnRef
            (A, 2) if l == b"fillRef" || l == b"lnRef" => {
                let on = !matches!(attr(e, b"idx").as_deref(), None | Some("0"));
                if l == b"fillRef" {
                    self.fill_ref = on
                } else {
                    self.line_ref = on
                }
            }
            // p:sp > p:spPr > fill, and p:sp > p:spPr > a:ln > fill
            (A, 2) if filled || l == b"noFill" => self.fill = Some(filled),
            (A, 2) => self.in_ln = l == b"ln",
            (A, 3) if self.in_ln && (filled || l == b"noFill") => self.line = Some(filled),
            _ => {}
        }
    }
    fn shown(&self) -> (bool, bool) {
        (self.fill.unwrap_or(self.fill_ref), self.line.unwrap_or(self.line_ref))
    }
}

fn pptx(slides: &[Vec<u8>], notes: &[Vec<u8>], names: &[String]) -> Counts {
    let mut t: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut table_text = String::new();
    let mut bold_text = String::new();
    for s in slides {
        let (mut in_tbl, mut in_t) = (0usize, 0usize);
        // Runs: is the open a:r bold (its own a:rPr b="1")?
        let mut run_bold: Option<bool> = None;
        // Open shapes: (has text yet, is Milleleaf's own). An import carries Milleleaf's own.
        // and how many of its paragraphs have an outline level (counted when it has text).
        let mut sps: Vec<(bool, bool, u64)> = Vec::new();
        // Their fill and outline: set in spPr (Some(shown)), and the style's references.
        let mut looks: Vec<SpLook> = Vec::new();
        // A transition counts when it does something (an effect or sound child, or auto-advance)
        // and isn't the mc:Fallback copy of one already counted in mc:Choice.
        let (mut fallback, mut transition): (usize, Option<(usize, bool)>) = (0, None);
        let mut depth = 0usize;
        walk(s, |ev| match ev {
            Ev::Open(n, l, e) => {
                depth += 1;
                if let Some(lk) = looks.last_mut() {
                    lk.see(n, l, e, depth);
                }
                if let Some((d, does)) = transition.as_mut() {
                    *does |= depth == *d + 1;
                }
                let k = match (n, l) {
                    (MC, b"Fallback") => {
                        fallback += 1;
                        return;
                    }
                    (P, b"transition") => {
                        transition = Some((depth, fallback == 0 && attr(e, b"advTm").is_some()));
                        return;
                    }
                    (P, b"sp") => {
                        sps.push((false, false, 0));
                        looks.push(SpLook { depth, ..SpLook::default() });
                        "shapes (sp)"
                    }
                    (P, b"cNvPr") => {
                        if let Some(top) = sps.last_mut() {
                            top.1 = attr(e, b"name").is_some_and(|n| n.starts_with("ox:"));
                        }
                        return;
                    }
                    (P, b"pic") => "pictures (pic)",
                    (P, b"grpSp") => "groups",
                    (P, b"cxnSp") => "connectors",
                    (A, b"tbl") => {
                        in_tbl += 1;
                        "tables"
                    }
                    (A, b"tc") => "table cells",
                    (P, b"timing") => "animations (timing)",
                    (P, b"stSnd") => "transition sounds",
                    (P, b"audio") | (P, b"sndTgt") => "animation sounds",
                    (A, b"hlinkClick") => "hyperlinks",
                    (A, b"t") => {
                        in_t += 1;
                        return;
                    }
                    (A, b"r") => {
                        run_bold = Some(false);
                        return;
                    }
                    (A, b"rPr") => {
                        if let Some(b) = run_bold.as_mut() {
                            *b = matches!(attr(e, b"b").as_deref(), Some("1" | "true"));
                        }
                        return;
                    }
                    (A, b"pPr") if !matches!(attr(e, b"lvl").as_deref(), None | Some("0")) => {
                        if let Some(top) = sps.last_mut() {
                            top.2 += 1;
                        }
                        return;
                    }
                    _ => return,
                };
                *t.entry(k).or_default() += 1;
            }
            Ev::Close(n, l) => {
                depth -= 1;
                match (n, l) {
                    (A, b"tbl") => in_tbl = in_tbl.saturating_sub(1),
                    (A, b"t") => in_t = in_t.saturating_sub(1),
                    (A, b"r") => run_bold = None,
                    (MC, b"Fallback") => fallback -= 1,
                    (P, b"transition") => {
                        if transition.take().is_some_and(|(_, does)| does) && fallback == 0 {
                            *t.entry("transitions").or_default() += 1;
                        }
                    }
                    (P, b"sp") => {
                        if let Some(lk) = looks.pop() {
                            let (f, l) = lk.shown();
                            *t.entry("shapes with a fill").or_default() += u64::from(f);
                            *t.entry("shapes with an outline").or_default() += u64::from(l);
                        }
                        match sps.pop() {
                            Some((false, ours, _)) => {
                                *t.entry("shapes without text").or_default() += 1;
                                if ours {
                                    *t.entry("_milleleaf shapes without text").or_default() += 1;
                                }
                            }
                            Some((true, _, levels)) => *t.entry("slide paragraphs with an outline level").or_default() += levels,
                            None => {}
                        }
                    }
                    _ => {}
                }
            }
            Ev::Text(x) if in_t > 0 => {
                if !x.trim().is_empty() {
                    sps.iter_mut().for_each(|s| s.0 = true);
                }
                if run_bold == Some(true) {
                    bold_text.push_str(x);
                }
                if in_tbl > 0 {
                    table_text.push_str(x);
                }
            }
            _ => {}
        });
    }
    // A notes page counts when it holds text besides its slide number: a page with nothing typed
    // on it (PowerPoint adds one with only the number) holds no notes.
    let (mut slide_numbers, mut with_text) = (0u64, 0u64);
    for x in notes {
        let (mut in_num, mut in_t, mut text) = (0usize, 0usize, false);
        walk(x, |ev| match ev {
            Ev::Open(A, b"fld", e) if attr(e, b"type").as_deref() == Some("slidenum") => {
                slide_numbers += 1;
                in_num += 1;
            }
            Ev::Open(A, b"fld", _) if in_num > 0 => in_num += 1,
            Ev::Close(A, b"fld") => in_num = in_num.saturating_sub(1),
            Ev::Open(A, b"t", _) => in_t += 1,
            Ev::Close(A, b"t") => in_t = in_t.saturating_sub(1),
            Ev::Text(t) if in_t > 0 && in_num == 0 && !t.trim().is_empty() => text = true,
            _ => {}
        });
        with_text += u64::from(text);
    }
    let g = |k: &str| t.get(k).copied().unwrap_or(0);
    let media = |n: &str| n.starts_with("ppt/media/") && n.len() > 10;
    let mut c = Counts::new();
    c.insert("slides", slides.len() as u64);
    for k in ["shapes (sp)", "shapes without text", "pictures (pic)", "groups", "connectors", "tables", "table cells"] {
        c.insert(k, g(k));
    }
    c.insert("charts", count_parts(names, |n| numbered(n, "ppt/charts/chart")));
    c.insert("media files", count_parts(names, media));
    c.insert("_milleleaf shapes without text", g("_milleleaf shapes without text"));
    c.insert("notes slides", with_text);
    c.insert("notes slide-number fields", slide_numbers);
    c.insert("layouts", count_parts(names, |n| numbered_req(n, "ppt/slideLayouts/slideLayout")));
    c.insert("masters", count_parts(names, |n| numbered_req(n, "ppt/slideMasters/slideMaster")));
    for k in ["animations (timing)", "transitions", "transition sounds", "animation sounds"] {
        c.insert(k, g(k));
    }
    let av = [".wav", ".mp3", ".m4a", ".aac", ".wma", ".mid", ".mp4", ".mov", ".m4v", ".wmv", ".avi"];
    c.insert(
        "audio or video files",
        count_parts(names, |n| media(n) && av.iter().any(|e| n.to_ascii_lowercase().ends_with(e))),
    );
    c.insert("hyperlinks", g("hyperlinks"));
    c.insert("slide paragraphs with an outline level", g("slide paragraphs with an outline level"));
    c.insert("shapes with a fill", g("shapes with a fill"));
    c.insert("shapes with an outline", g("shapes with an outline"));
    c.insert("slide bold text chars", norm_len(&bold_text));
    // Table text, counted on its own (the shapes' text doesn't include it).
    c.insert("table text chars", norm_len(&table_text));
    c
}

fn carried(kind: &str) -> &'static [(&'static str, Option<&'static str>)] {
    match kind {
        "docx" => &[
            ("paragraphs (non-empty)", None),
            ("tables", None),
            ("table rows", None),
            ("table cells", None),
            ("nested tables", Some("their text is kept, as paragraphs of the cell they sit in; their rows, columns and formatting aren't")),
            ("page setups (size and margins)", None),
            ("sections", None),
            ("body text chars", None),
            // The export applies its own named styles; direct formatting is kept on runs.
            ("styles used", None),
            ("character styles used", Some("their formatting is kept on each run; the style names aren't")),
            ("tracked changes (insertions and deletions)", Some("they come in accepted: inserted text is kept, deleted text dropped; the marks (who, when) aren't kept")),
            ("bold text chars", None),
            ("list paragraphs (numPr)", None),
            ("page width (value, twips)", None),
            ("default style font (value, name hash)", None),
            ("default style size (value, half-points)", None),
            ("default style justified (value)", None),
            ("style definitions", None),
            ("table column widths (value, hash)", None),
            ("table cells with shading", None),
            ("table cell paragraphs with alignment, spacing or indents", None),
            ("page height (value, twips)", None),
            ("page margins top/right/bottom/left (value, twips)", None),
            ("paragraphs aligned other than left (outside tables)", None),
            ("paragraphs with spacing set (outside tables)", None),
            ("paragraphs with indents set (outside tables)", None),
            ("merged cells (gridSpan/vMerge)", None),
            ("fields", None),
            ("headers and footers (parts)", None),
            ("header/footer text chars", None),
            ("footnote separator lines", None),
            ("media files", Some("pictures in the text, headers and footers are kept; embedded objects and other media aren't")),
            ("hyperlinks", Some("web and mail links on text are kept (not in table cells, counted on their own); a link to a bookmark keeps its target, but bookmarks aren't carried")),
        ],
        "xlsx" => &[
            ("sheets", None),
            ("cells with a value", None),
            ("formulas", None),
            ("what-if data tables", Some("their cells' values are kept; the TABLE() that recalculates them isn't (GridCraft has no data tables)")),
            ("column width settings", Some("widths are kept; entries that only give columns a style are not (styles aren't carried yet)")),
            ("custom row heights", None),
            ("merged ranges", None),
            ("frozen panes", None),
            ("autofilters", None),
            ("comments", None),
            ("data validations", None),
            ("defined names (not Milleleaf's)", None),
            // Styles: the ones cells use are carried (the "cells in bold"… rows), so these lists
            // can get shorter.
            ("custom number formats", Some("the formats cells use are kept; unused ones are dropped")),
            ("cell styles (xf)", Some("the styles cells use are kept; unused ones are dropped")),
            ("fonts", Some("the fonts cells use are kept; unused ones are dropped")),
            ("fills", Some("the fills cells use are kept; unused ones are dropped")),
            ("borders", Some("the borders cells use are kept; unused ones are dropped")),
            ("cells in bold", None),
            ("hidden sheets", None),
            ("protected sheets", None),
            ("workbook structure locked", None),
            ("sheets with gridlines off", None),
            ("cells with a fill", None),
            ("cells with borders", None),
            ("cells with a number format", None),
            ("media files", Some("floating pictures are kept; pictures inside cells and other media aren't")),
            ("drawings", Some("floating pictures and charts are kept; shapes aren't")),
            ("charts", Some("a chart keeps its own XML, so all its formatting, while unchanged, when an editor changed it, or when its title, data cells or legend position changed; a change to its kind, series count, colours, labels, gridlines, axis titles or trendlines makes GridCraft redraw it, keeping kind, series and their cells, title, legend, axis titles, gridlines, data labels on or off, trendlines and series colours, while fonts, per-point colours, axis scales, manual layout, line dashes and label positions become its defaults (the export names each one); a chart with shapes on it or an embedded workbook is always redrawn; Excel's chart-style gallery parts aren't kept")),
        ],
        "pptx" => &[
            ("slides", None),
            ("slide width (value, EMU)", None),
            ("slide height (value, EMU)", None),
            ("slide paragraphs with an outline level", None),
            ("slide bold text chars", None),
            ("shapes with a fill", None),
            ("shapes with an outline", None),
            // Shapes without text are the exception, listed on their own.
            ("shapes (sp)", None),
            ("notes slides", None),
            ("pictures (pic)", Some("position, picture, alt text and crop are kept; transparency, corrections and effects aren't")),
            ("media files", Some("the slides' pictures are kept; the masters' and layouts' own pictures, sound and video aren't")),
            ("layouts", Some("only the layouts the slides use")),
            ("masters", Some("only the masters the slides use")),
            ("tables", Some("cells' text, fills, borders, margins and merges are kept; the table style is written out into the cells")),
            ("table cells", None),
            ("table text chars", None),
            ("connectors", Some("kept with their line, arrowheads and the shapes they join; shadows and other effects aren't")),
            ("hyperlinks", Some("web and mail links on text are kept; links to slides, other actions, tooltips and links on whole shapes aren't")),
        ],
        _ => &[],
    }
}

/// Every counted feature the adapter for `kind` doesn't carry, or carries only in part.
/// `baseline`: for an import, the counts of what Milleleaf itself exports for the node; only
/// what the file holds beyond that (what someone added) is reported. `None` for adopt.
pub fn losses(kind: &str, counts: &Counts, baseline: Option<&Counts>) -> Vec<Loss> {
    let table = carried(kind);
    let mut counts = counts.clone();
    if let Some(base) = baseline {
        // Milleleaf's own objects (named `ox:…`) are matched and carried on import.
        let ours = counts.get("_milleleaf shapes without text").copied().unwrap_or(0);
        if let Some(n) = counts.get_mut("shapes without text") {
            *n -= ours.min(*n);
        }
        for (k, n) in counts.iter_mut() {
            if *k != "shapes without text" {
                *n = n.saturating_sub(base.get(k).copied().unwrap_or(0));
            }
        }
    }
    counts
        .iter()
        .filter(|(k, n)| **n > 0 && !k.starts_with('_'))
        .filter_map(|(k, n)| match table.iter().find(|(f, _)| f == k) {
            Some((_, None)) => None,
            Some((_, Some(note))) => Some(Loss { feature: k, count: *n, carried: "partly carried", note }),
            None => Some(Loss { feature: k, count: *n, carried: "not carried", note: "" }),
        })
        .collect()
}
