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
        Ok(("docx", docx(&body, &hf, &notes, &comments, &names)))
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
        Ok(("xlsx", xlsx(&sheets, &styles, &wb, &comments, &names)))
    } else if has("ppt/presentation.xml") {
        let slides: Vec<Vec<u8>> = matching(|n| numbered_req(n, "ppt/slides/slide"))
            .iter()
            .map(|n| part(n))
            .collect::<Result<_, _>>()?;
        let notes: Vec<Vec<u8>> = matching(|n| numbered_req(n, "ppt/notesSlides/notesSlide"))
            .iter()
            .map(|n| part(n))
            .collect::<Result<_, _>>()?;
        Ok(("pptx", pptx(&slides, &notes, &names)))
    } else {
        Err("not a .docx, .xlsx or .pptx".into())
    }
}

/// `prefix` + optional digits + `.xml` (Python `prefix\d*\.xml`).
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

fn docx(body: &[u8], hf: &[Vec<u8>], notes: &[Vec<u8>], comments: &[Vec<u8>], names: &[String]) -> Counts {
    let mut c = Counts::new();
    // Body: one pass, tracking open paragraphs (non-empty if any w:t inside has text), the styles
    // used, and bold text (w:t inside a run whose w:rPr has w:b that isn't off).
    let count = [
        "tbl", "tr", "tc", "gridSpan", "vMerge", "drawing", "pgSz", "pgMar", "numPr", "sectPr", "hyperlink",
        "fldSimple", "instrText", "txbxContent",
    ];
    let mut tags: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut paras: Vec<bool> = Vec::new();
    let mut nonempty = 0u64;
    let mut styles = std::collections::BTreeSet::new();
    // Bold counts only a w:b in the run's own w:rPr (not in w:rPrChange, a tracked change).
    let (mut in_t, mut run_bold, mut depth) = (0usize, false, 0usize);
    let mut runs: Vec<usize> = Vec::new();
    let (mut text, mut bold_text) = (String::new(), String::new());
    walk(body, |ev| match ev {
        Ev::Open(n, l, e) if n == W => {
            depth += 1;
            let l = std::str::from_utf8(l).unwrap_or("");
            if let Some(k) = count.iter().find(|k| **k == l) {
                *tags.entry(k).or_default() += 1;
            }
            match l {
                "p" => paras.push(false),
                "t" => in_t += 1,
                "r" => {
                    runs.push(depth);
                    run_bold = false;
                }
                "b" if runs.last().map(|d| d + 2) == Some(depth) => run_bold = !matches!(attr(e, b"val").as_deref(), Some("0" | "false" | "off")),
                "pStyle" | "rStyle" | "tblStyle" => {
                    if let Some(v) = attr(e, b"val") {
                        styles.insert(v);
                    }
                }
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
            b"p" => {
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
    c.insert("sections", g("sectPr"));
    c.insert("hyperlinks", g("hyperlink"));
    c.insert("fields", g("fldSimple") + g("instrText"));
    c.insert("text boxes", g("txbxContent"));
    c.insert("bold text chars", norm_len(&bold_text));
    c.insert("body text chars", norm_len(&text));
    c
}

fn xlsx(sheets: &[Vec<u8>], styles: &[u8], wb: &[u8], comments: &[Vec<u8>], names: &[String]) -> Counts {
    let mut t: BTreeMap<&'static str, u64> = BTreeMap::new();
    for s in sheets {
        let (mut in_c, mut has_value) = (false, false);
        walk(s, |ev| match ev {
            Ev::Open(n, l, e) if n == S => {
                let k = match l {
                    b"c" => {
                        in_c = true;
                        has_value = false;
                        return;
                    }
                    b"v" | b"is" => {
                        has_value |= in_c;
                        return;
                    }
                    b"f" => "formulas",
                    b"mergeCell" => "merged ranges",
                    b"col" => "column width settings",
                    b"row" if matches!(attr(e, b"customHeight").as_deref(), Some("1" | "true")) => "custom row heights",
                    b"pane" => "frozen panes",
                    b"conditionalFormatting" => "conditional formats",
                    b"dataValidation" => "data validations",
                    b"hyperlink" => "hyperlinks",
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
    let mut own_names = 0u64;
    walk(wb, |ev| {
        if let Ev::Open(n, b"definedName", e) = ev {
            if n == S && !attr(e, b"name").unwrap_or_default().starts_with("ox_") {
                own_names += 1;
            }
        }
    });
    let g = |k: &str| t.get(k).copied().unwrap_or(0);
    let mut c = Counts::new();
    c.insert("sheets", sheets.len() as u64);
    for k in [
        "cells with a value", "formulas", "merged ranges", "custom number formats", "cell styles (xf)", "fonts",
        "fills", "borders", "column width settings", "custom row heights", "frozen panes", "conditional formats",
        "data validations", "hyperlinks",
    ] {
        c.insert(k, g(k));
    }
    c.insert("defined names (not Milleleaf's)", own_names);
    c.insert("charts", count_parts(names, |n| numbered(n, "xl/charts/chart")));
    c.insert("drawings", count_parts(names, |n| numbered(n, "xl/drawings/drawing")));
    c.insert("media files", count_parts(names, |n| n.starts_with("xl/media/") && n.len() > 9));
    c.insert("comments", comments.iter().map(|x| count_tag(x, S, b"comment")).sum());
    c.insert("tables (ListObjects)", count_parts(names, |n| numbered(n, "xl/tables/table")));
    c
}

fn pptx(slides: &[Vec<u8>], notes: &[Vec<u8>], names: &[String]) -> Counts {
    let mut t: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut table_text = String::new();
    for s in slides {
        let (mut in_tbl, mut in_t) = (0usize, 0usize);
        // Open shapes: (has text yet, is Milleleaf's own). An import carries Milleleaf's own.
        let mut sps: Vec<(bool, bool)> = Vec::new();
        // A transition counts when it does something (an effect or sound child, or auto-advance)
        // and isn't the mc:Fallback copy of one already counted in mc:Choice.
        let (mut fallback, mut transition): (usize, Option<(usize, bool)>) = (0, None);
        let mut depth = 0usize;
        walk(s, |ev| match ev {
            Ev::Open(n, l, e) => {
                depth += 1;
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
                        sps.push((false, false));
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
                    _ => return,
                };
                *t.entry(k).or_default() += 1;
            }
            Ev::Close(n, l) => {
                depth -= 1;
                match (n, l) {
                    (A, b"tbl") => in_tbl = in_tbl.saturating_sub(1),
                    (A, b"t") => in_t = in_t.saturating_sub(1),
                    (MC, b"Fallback") => fallback -= 1,
                    (P, b"transition") => {
                        if transition.take().is_some_and(|(_, does)| does) && fallback == 0 {
                            *t.entry("transitions").or_default() += 1;
                        }
                    }
                    (P, b"sp") => {
                        if let Some((false, ours)) = sps.pop() {
                            *t.entry("shapes without text").or_default() += 1;
                            if ours {
                                *t.entry("_milleleaf shapes without text").or_default() += 1;
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ev::Text(x) if in_t > 0 => {
                if !x.trim().is_empty() {
                    sps.iter_mut().for_each(|s| s.0 = true);
                }
                if in_tbl > 0 {
                    table_text.push_str(x);
                }
            }
            _ => {}
        });
    }
    let mut slide_numbers = 0u64;
    for x in notes {
        walk(x, |ev| {
            if let Ev::Open(A, b"fld", e) = ev {
                if attr(e, b"type").as_deref() == Some("slidenum") {
                    slide_numbers += 1;
                }
            }
        });
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
    c.insert("notes slides", count_parts(names, |n| numbered_req(n, "ppt/notesSlides/notesSlide")));
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
    // The deck adapter doesn't carry tables, so their text goes with them.
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
            ("page setups (size and margins)", None),
            ("sections", None),
            ("body text chars", None),
            // The export applies its own named styles; direct formatting is kept on runs.
            ("styles used", None),
            ("bold text chars", None),
            ("list paragraphs (numPr)", None),
            ("merged cells (gridSpan/vMerge)", None),
            ("fields", None),
            ("headers and footers (parts)", None),
            ("header/footer text chars", None),
            ("footnote separator lines", None),
        ],
        "xlsx" => &[
            ("sheets", None),
            ("cells with a value", None),
            ("formulas", None),
            ("column width settings", Some("widths are re-fitted to the content, not kept")),
            ("custom number formats", Some("number, currency and percent are kept; the exact format codes are not")),
            ("cell styles (xf)", Some("only the styles Milleleaf's own formats need")),
            ("fonts", Some("only the styles Milleleaf's own formats need")),
            ("fills", Some("only the styles Milleleaf's own formats need")),
            ("borders", Some("only the styles Milleleaf's own formats need")),
        ],
        "pptx" => &[
            ("slides", None),
            // Shapes without text are the exception, listed on their own.
            ("shapes (sp)", None),
            ("notes slides", None),
            ("layouts", Some("only the layouts the slides use")),
            ("masters", Some("only the masters the slides use")),
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
