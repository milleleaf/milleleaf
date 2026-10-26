//! Byte-only linked workbook, report and deck sessions.
use super::{Pictures, header, parts};
use milleleaf_core::{Composed, Composer, Flat, Layer, Node, PictureSource};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use wasm_bindgen::prelude::*;

type Result<T> = std::result::Result<T, String>;
#[path = "flow_chart.rs"]
mod chart_svg;
#[path = "flow_png.rs"]
mod png_fallback;
const WB: &str = "3f1c2a90-0000-4c1e-9a00-000000000010";
const REPORT: &str = "flow-report";
const DECK: &str = "flow-deck";
const SOURCE: &str = "flow-source-table";
const RT: &str = "flow-report-table";
const DT: &str = "flow-deck-table";
const DC: &str = "flow-deck-chart";
const RC: &str = "flow-report-chart";
const CHART_SOURCE: &str = "flow-source-chart";
const ODS_NOTE: &str = "Direct ODS import reads cell data and cached formula values. It does not carry formula expressions, workbook styles, charts or Milleleaf names. Existing project chart references are kept when ODS data comes back.";
fn selected_chart_kind(kind: &str) -> Result<&'static str> {
    match kind.to_ascii_lowercase().as_str() {
        "column" | "columnclustered" => Ok("column"),
        "bar" | "barclustered" => Ok("bar"),
        "line" | "linemarkers" => Ok("line"),
        "pie" => Ok("pie"),
        _ => Err(format!(
            "The selected workbook chart uses {kind}. This browser export preserves clustered columns, clustered bars, plain lines and pie charts. Choose Make a chart from this range to intentionally create a new grouped chart, or retain the original chart in the workbook. Stacked and percentage-stacked charts are not silently changed into grouped charts."
        )),
    }
}

fn node(path: &str, attrs: Value, children: &[(&str, &str)]) -> Node {
    Node {
        path: path.into(),
        attributes: attrs.as_object().unwrap().clone(),
        children: children
            .iter()
            .map(|(k, v)| (k.to_string(), Some(v.to_string())))
            .collect(),
        order: Some(children.iter().map(|(k, _)| k.to_string()).collect()),
        ..Node::default()
    }
}
fn link(path: &str, attrs: Value) -> Node {
    let mut n = node(path, attrs, &[]);
    n.inherits.insert("source".into(), Some(SOURCE.into()));
    n.attributes
        .insert("mleaf::link".into(), json!({"source":SOURCE,"mode":"live"}));
    n
}
fn cell(s: &str) -> Option<(usize, usize)> {
    let s = s.trim().replace('$', "").to_uppercase();
    let i = s.find(|c: char| c.is_ascii_digit())?;
    let mut col = 0usize;
    for b in s[..i].bytes() {
        if !b.is_ascii_uppercase() {
            return None;
        }
        col = col.checked_mul(26)?.checked_add((b - b'A' + 1) as usize)?;
    }
    let row = s[i..].parse::<usize>().ok()?;
    if col == 0 || row == 0 || col > 16384 || row > 1048576 {
        None
    } else {
        Some((row - 1, col - 1))
    }
}
fn a1(r: usize, c: usize) -> String {
    let mut c = c + 1;
    let mut letters = String::new();
    while c > 0 {
        letters.insert(0, (b'A' + ((c - 1) % 26) as u8) as char);
        c = (c - 1) / 26;
    }
    format!("{letters}{}", r + 1)
}
fn bounds(s: &str) -> Result<(usize, usize, usize, usize)> {
    let (a, b) = s.split_once(':').unwrap_or((s, s));
    let (r, c) = cell(a).ok_or("Use a cell range such as A1:B4.")?;
    let (rr, cc) = cell(b).ok_or("Use a cell range such as A1:B4.")?;
    if rr < r || cc < c || (rr - r + 1).saturating_mul(cc - c + 1) > 10000 {
        return Err("Select a rectangle of at most 10,000 cells.".into());
    }
    Ok((r, c, rr, cc))
}
fn cells(sh: &Composed) -> BTreeMap<(usize, usize), Value> {
    let mut out = BTreeMap::new();
    for n in &sh.children {
        let Some(range) = n
            .attributes
            .get("grid::range")
            .and_then(|v| v["ref"].as_str())
        else {
            continue;
        };
        let Ok((r, c, _, _)) = bounds(range) else {
            continue;
        };
        if let Some(t) = n.attributes.get("office::data::table") {
            for (i, col) in t["columns"].as_array().into_iter().flatten().enumerate() {
                out.insert((r, c + i), col["name"].clone());
            }
            for (i, row) in t["rows"].as_array().into_iter().flatten().enumerate() {
                for (j, v) in row.as_array().into_iter().flatten().enumerate() {
                    out.insert((r + i + 1, c + j), v.clone());
                }
            }
        } else if let Some(v) = n.attributes.get("office::data::value") {
            out.insert((r, c), v["value"].clone());
        }
    }
    out
}
fn overlap(n: &Node, old: Option<&Composed>, now: Option<&Composed>) -> bool {
    let attrs = n
        .attributes
        .keys()
        .any(|k| old.and_then(|o| o.attributes.get(k)) != now.and_then(|o| o.attributes.get(k)));
    fn child<'a>(owner: Option<&'a Composed>, name: &str) -> Option<&'a Composed> {
        owner.and_then(|o| o.children.iter().find(|c| c.name.as_deref() == Some(name)))
    }
    let children = n.children.iter().any(|(name, target)| {
        let a = child(old, name);
        let b = child(now, name);
        if target.as_deref() != a.map(|v| v.path.as_str()) {
            serde_json::to_value(a).ok() != serde_json::to_value(b).ok()
        } else {
            a.map(|v| &v.path) != b.map(|v| &v.path)
        }
    });
    let order = |owner: Option<&Composed>| {
        owner.map(|o| {
            o.children
                .iter()
                .map(|c| (c.name.clone(), c.path.clone()))
                .collect::<Vec<_>>()
        })
    };
    attrs || children || (n.order.is_some() && order(old) != order(now))
}
fn strip_noops(n: &mut Node, now: Option<&Composed>) {
    n.attributes.retain(|key, value| {
        let current = now.and_then(|v| v.attributes.get(key));
        if value.is_null() {
            current.is_some()
        } else {
            current != Some(value)
        }
    });
    n.children.retain(|name, target| {
        let current = now
            .and_then(|v| v.children.iter().find(|c| c.name.as_deref() == Some(name)))
            .map(|v| v.path.as_str());
        current != target.as_deref()
    });
    if let Some(order) = &n.order {
        let current: Vec<_> = now
            .into_iter()
            .flat_map(|v| v.children.iter())
            .filter_map(|c| c.name.as_ref().map(|name| (name.clone(), c.path.clone())))
            .collect();
        let mut children: BTreeMap<_, _> = current.iter().cloned().collect();
        for (name, path) in &n.children {
            if let Some(path) = path {
                children.insert(name.clone(), path.clone());
            } else {
                children.remove(name);
            }
        }
        let mut proposed = Vec::new();
        for name in order {
            if let Some(path) = children.remove(name) {
                proposed.push((name.clone(), path));
            }
        }
        proposed.extend(children);
        if proposed == current {
            n.order = None;
        }
    }
}

fn proposed_rows(node: &Composed, columns: &[Value]) -> Option<Vec<Vec<Value>>> {
    let rows: Vec<Vec<Value>> = if let Some(rows) = node
        .attributes
        .get("office::data::table")
        .and_then(|t| t["rows"].as_array())
    {
        rows.iter()
            .map(|r| r.as_array().cloned().unwrap_or_default())
            .collect()
    } else {
        node.attributes.get("deck::table")?["snapshot"]
            .as_array()?
            .iter()
            .skip(1)
            .map(|r| {
                r.as_array()
                    .into_iter()
                    .flatten()
                    .map(|c| c["text"].clone())
                    .collect()
            })
            .collect()
    };
    Some(
        rows.into_iter()
            .map(|r| {
                r.into_iter()
                    .enumerate()
                    .map(|(i, v)| {
                        let kind = columns
                            .get(i)
                            .and_then(|c| c["type"].as_str())
                            .unwrap_or("text");
                        if !["number", "currency", "percent"].contains(&kind) {
                            return v;
                        }
                        let Some(text) = v.as_str() else { return v };
                        let cleaned: String = text
                            .chars()
                            .filter(|c| {
                                !matches!(c, '$' | '£' | '€' | '¥' | ',' | ' ' | '%' | '\u{a0}')
                            })
                            .collect();
                        cleaned
                            .parse::<f64>()
                            .ok()
                            .map(|n| json!(if kind == "percent" { n / 100.0 } else { n }))
                            .unwrap_or(v)
                    })
                    .collect()
            })
            .collect(),
    )
}

fn enrich_conflicts(
    conflicts: &mut [Value],
    before: &Composed,
    current: &Composed,
    proposed: Option<&Composed>,
    filename: &str,
) {
    for conflict in conflicts {
        let path = conflict["path"].as_str().unwrap_or("").to_string();
        let place = match path.as_str() {
            RT => "Report table",
            DT => "Slide table",
            RC => "Report chart",
            DC => "Slide chart",
            _ => "Returned content",
        };
        conflict["file"] = json!(filename);
        conflict["place"] = json!(place);
        if let Some(message) = conflict["message"].as_str() {
            conflict["message"] = json!(message.replace('\u{2014}', ". "));
        }
        let Some(was) = milleleaf_core::render::find(std::slice::from_ref(before), &path)
            .and_then(|n| n.attributes.get("office::data::table"))
        else {
            continue;
        };
        let Some(columns) = was["columns"].as_array() else {
            continue;
        };
        let Some(rows) = was["rows"].as_array() else {
            continue;
        };
        let Some(returned) = proposed
            .and_then(|tree| milleleaf_core::render::find(std::slice::from_ref(tree), &path))
            .and_then(|n| proposed_rows(n, columns))
        else {
            continue;
        };
        let now = milleleaf_core::render::find(std::slice::from_ref(current), &path)
            .and_then(|n| n.attributes.get("office::data::table"));
        let mut cells = Vec::new();
        for r in 0..rows.len().max(returned.len()) {
            let old = rows.get(r).and_then(Value::as_array);
            let proposal = returned.get(r);
            for c in 0..old
                .map(Vec::len)
                .unwrap_or(0)
                .max(proposal.map(Vec::len).unwrap_or(0))
            {
                let a = old.and_then(|r| r.get(c)).cloned().unwrap_or(Value::Null);
                let b = proposal
                    .and_then(|r| r.get(c))
                    .cloned()
                    .unwrap_or(Value::Null);
                if milleleaf_core::same(&a, &b) {
                    continue;
                }
                let source = now.map(|t| t["rows"][r][c].clone()).unwrap_or(Value::Null);
                cells.push(json!({"row":r+2,"column":c+1,"column_name":columns.get(c).map(|v|v["name"].clone()),"row_label":old.and_then(|r|r.first()),"before":a,"proposed":b,"source_current":source}));
            }
        }
        if !cells.is_empty() {
            let shown = |v: &Value| {
                v.as_str()
                    .map(String::from)
                    .unwrap_or_else(|| v.to_string())
            };
            let details = cells
                .iter()
                .map(|c| {
                    format!(
                        "{}, {} (row {}, column {}): sent {}, proposed {}, current source {}.",
                        shown(&c["row_label"]),
                        shown(&c["column_name"]),
                        c["row"],
                        c["column"],
                        shown(&c["before"]),
                        shown(&c["proposed"]),
                        shown(&c["source_current"])
                    )
                })
                .collect::<Vec<_>>()
                .join(" ");
            conflict["description"] = json!(details);
            conflict["message"] = json!(format!(
                "Linked content edit needs review. {details} The current workbook source is kept."
            ));
            conflict["cell_changes"] = json!(cells);
        }
    }
}

// WordCraft reads a:blip's raster fallback. The SVG extension on that same drawing is
// authoritative vector content, not an interchangeable image elsewhere in the package.
fn prefer_referenced_svg(bytes: &[u8]) -> Result<Option<Vec<u8>>> {
    use std::io::{Cursor, Read, Write};
    #[derive(Clone)]
    struct Attr<'a> {
        name: &'a str,
        value: &'a str,
        start: usize,
        end: usize,
    }
    struct Tag<'a> {
        name: &'a str,
        attrs: Vec<Attr<'a>>,
        close: bool,
        empty: bool,
    }
    fn tags(xml: &str) -> Option<Vec<Tag<'_>>> {
        let b = xml.as_bytes();
        let mut at = 0;
        let mut out = Vec::new();
        while at < b.len() {
            if b[at] != b'<' {
                at += 1;
                continue;
            }
            if xml[at..].starts_with("<!--") {
                at += xml[at + 4..].find("-->")? + 7;
                continue;
            }
            if xml[at..].starts_with("<?") {
                at += xml[at + 2..].find("?>")? + 4;
                continue;
            }
            if xml[at..].starts_with("<!") {
                return None;
            }
            at += 1;
            let close = b.get(at) == Some(&b'/');
            if close {
                at += 1;
            }
            let start = at;
            while b
                .get(at)
                .is_some_and(|v| !v.is_ascii_whitespace() && !matches!(v, b'/' | b'>'))
            {
                at += 1;
            }
            let name = xml.get(start..at)?;
            if name.is_empty() {
                return None;
            }
            let mut attrs = Vec::new();
            loop {
                while b.get(at).is_some_and(u8::is_ascii_whitespace) {
                    at += 1;
                }
                if matches!(b.get(at), Some(b'>') | Some(b'/')) {
                    break;
                }
                let start = at;
                while b
                    .get(at)
                    .is_some_and(|v| !v.is_ascii_whitespace() && *v != b'=')
                {
                    at += 1;
                }
                let key = xml.get(start..at)?;
                if key.is_empty() {
                    return None;
                }
                while b.get(at).is_some_and(u8::is_ascii_whitespace) {
                    at += 1;
                }
                if b.get(at) != Some(&b'=') {
                    return None;
                }
                at += 1;
                while b.get(at).is_some_and(u8::is_ascii_whitespace) {
                    at += 1;
                }
                let quote = *b.get(at)?;
                if !matches!(quote, b'\'' | b'"') {
                    return None;
                }
                at += 1;
                let start = at;
                while b.get(at).is_some_and(|v| *v != quote) {
                    at += 1;
                }
                if b.get(at) != Some(&quote) {
                    return None;
                }
                attrs.push(Attr {
                    name: key,
                    value: xml.get(start..at)?,
                    start,
                    end: at,
                });
                at += 1;
            }
            let empty = b.get(at) == Some(&b'/');
            if empty {
                at += 1;
            }
            if b.get(at) != Some(&b'>') {
                return None;
            }
            at += 1;
            out.push(Tag {
                name,
                attrs,
                close,
                empty,
            });
            if out.len() > 1_000_000 {
                return None;
            }
        }
        Some(out)
    }
    fn attr<'a>(tag: &'a Tag<'a>, name: &str) -> Option<&'a str> {
        tag.attrs.iter().find(|a| a.name == name).map(|a| a.value)
    }
    fn local(name: &str) -> &str {
        name.rsplit(':').next().unwrap_or(name)
    }
    fn ns<'a>(name: &str, tag: &Tag<'a>, stack: &[&Tag<'a>]) -> Option<&'a str> {
        let key = name
            .split_once(':')
            .map(|(p, _)| format!("xmlns:{p}"))
            .unwrap_or_else(|| "xmlns".into());
        tag.attrs
            .iter()
            .find(|a| a.name == key)
            .map(|a| a.value)
            .or_else(|| {
                stack
                    .iter()
                    .rev()
                    .find_map(|t| t.attrs.iter().find(|a| a.name == key).map(|a| a.value))
            })
    }
    fn embed<'a>(tag: &'a Tag<'a>, stack: &[&Tag<'a>]) -> Option<&'a Attr<'a>> {
        tag.attrs.iter().find(|a| {
            local(a.name) == "embed"
                && a.name.contains(':')
                && ns(a.name, tag, stack)
                    == Some("http://schemas.openxmlformats.org/officeDocument/2006/relationships")
        })
    }
    fn package_children<'a>(nodes: &'a [Tag<'a>], root: &str, uri: &str) -> Vec<&'a Tag<'a>> {
        let mut stack: Vec<&Tag<'a>> = Vec::new();
        let mut found = Vec::new();
        for tag in nodes {
            if tag.close {
                if stack.pop().is_none_or(|open| open.name != tag.name) {
                    return Vec::new();
                }
                continue;
            }
            if stack.len() == 1
                && local(stack[0].name) == root
                && ns(stack[0].name, stack[0], &[]) == Some(uri)
                && ns(tag.name, tag, &stack) == Some(uri)
            {
                found.push(tag);
            }
            if !tag.empty {
                stack.push(tag);
            }
        }
        if stack.is_empty() { found } else { Vec::new() }
    }
    fn part(archive: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        archive.by_name(name).ok()?.read_to_end(&mut out).ok()?;
        Some(out)
    }
    fn target(value: &str) -> Option<String> {
        if value.contains(['\\', ':', '&', '?', '#']) {
            return None;
        }
        let mut path = if value.starts_with('/') {
            Vec::new()
        } else {
            vec!["word"]
        };
        for p in value.split('/') {
            match p {
                "" | "." => {}
                ".." => {
                    path.pop()?;
                }
                _ => path.push(p),
            }
        }
        Some(path.join("/"))
    }
    parts(bytes)?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let Some(xml) = part(&mut archive, "word/document.xml").and_then(|v| String::from_utf8(v).ok())
    else {
        return Ok(None);
    };
    let Some(rels) =
        part(&mut archive, "word/_rels/document.xml.rels").and_then(|v| String::from_utf8(v).ok())
    else {
        return Ok(None);
    };
    let Some(content_types) =
        part(&mut archive, "[Content_Types].xml").and_then(|v| String::from_utf8(v).ok())
    else {
        return Ok(None);
    };
    let (Some(doc_tags), Some(rel_tags), Some(type_tags)) =
        (tags(&xml), tags(&rels), tags(&content_types))
    else {
        return Ok(None);
    };
    let mut images = BTreeMap::<&str, Option<String>>::new();
    for tag in package_children(
        &rel_tags,
        "Relationships",
        "http://schemas.openxmlformats.org/package/2006/relationships",
    ) {
        if tag.close
            || local(tag.name) != "Relationship"
            || attr(tag, "Type")
                != Some("http://schemas.openxmlformats.org/officeDocument/2006/relationships/image")
            || attr(tag, "TargetMode").is_some_and(|v| v != "Internal")
        {
            continue;
        }
        if let (Some(id), Some(path)) = (attr(tag, "Id"), attr(tag, "Target").and_then(target)) {
            if images.contains_key(id) {
                images.insert(id, None);
            } else {
                images.insert(id, Some(path));
            }
        }
    }
    let mut stack: Vec<&Tag<'_>> = Vec::new();
    let mut replacements = BTreeMap::<(usize, usize), Vec<&str>>::new();
    for tag in &doc_tags {
        if tag.close {
            if stack.pop().is_none_or(|open| open.name != tag.name) {
                return Ok(None);
            }
            continue;
        }
        if local(tag.name) == "svgBlip"
            && ns(tag.name, tag, &stack)
                == Some("http://schemas.microsoft.com/office/drawing/2016/SVG/main")
        {
            let blip = stack.iter().enumerate().rev().find(|(i, t)| {
                local(t.name) == "blip"
                    && ns(t.name, t, &stack[..*i])
                        == Some("http://schemas.openxmlformats.org/drawingml/2006/main")
            });
            if let (Some((i, blip)), Some(svg)) = (blip, embed(tag, &stack)) {
                let extension = stack.len() == i + 3
                    && local(stack[i + 1].name) == "extLst"
                    && ns(stack[i + 1].name, stack[i + 1], &stack[..i + 1])
                        == Some("http://schemas.openxmlformats.org/drawingml/2006/main")
                    && local(stack[i + 2].name) == "ext"
                    && ns(stack[i + 2].name, stack[i + 2], &stack[..i + 2])
                        == Some("http://schemas.openxmlformats.org/drawingml/2006/main")
                    && attr(stack[i + 2], "uri") == Some("{96DAC541-7B7A-43D3-8B79-37D633B846F1}");
                if let Some(fallback) = embed(blip, &stack[..i]) {
                    let svg_path = images.get(svg.value).and_then(Option::as_ref);
                    let fallback_path = images.get(fallback.value).and_then(Option::as_ref);
                    if let (true, Some(svg_path), Some(fallback_path)) =
                        (extension, svg_path, fallback_path)
                    {
                        let recognized_types = package_children(
                            &type_tags,
                            "Types",
                            "http://schemas.openxmlformats.org/package/2006/content-types",
                        );
                        let overrides: Vec<_> = recognized_types
                            .iter()
                            .filter(|t| {
                                local(t.name) == "Override"
                                    && attr(t, "PartName") == Some(format!("/{svg_path}").as_str())
                            })
                            .collect();
                        let defaults: Vec<_> = recognized_types
                            .iter()
                            .filter(|t| {
                                local(t.name) == "Default" && attr(t, "Extension") == Some("svg")
                            })
                            .collect();
                        let mime = if overrides.is_empty() {
                            defaults.len() == 1
                                && matches!(
                                    attr(defaults[0], "ContentType"),
                                    Some("image/svg+xml" | "image/svg")
                                )
                        } else {
                            overrides.len() == 1
                                && matches!(
                                    attr(overrides[0], "ContentType"),
                                    Some("image/svg+xml" | "image/svg")
                                )
                        };
                        let vector = part(&mut archive, svg_path)
                            .and_then(|v| String::from_utf8(v).ok())
                            .and_then(|v| {
                                tags(&v).and_then(|ts| {
                                    ts.into_iter().find(|t| !t.close).map(|t| {
                                        local(t.name) == "svg"
                                            && ns(t.name, &t, &[])
                                                == Some("http://www.w3.org/2000/svg")
                                    })
                                })
                            });
                        if svg_path.ends_with(".svg")
                            && mime
                            && vector == Some(true)
                            && part(&mut archive, fallback_path).is_some()
                            && !svg.value.contains(['&', '<', '>', '\'', '"'])
                            && svg.value != fallback.value
                        {
                            replacements
                                .entry((fallback.start, fallback.end))
                                .or_default()
                                .push(svg.value);
                        }
                    }
                }
            }
        }
        if !tag.empty {
            stack.push(tag);
        }
    }
    if !stack.is_empty() {
        return Ok(None);
    }
    let mut normalized = xml.clone();
    let mut changed = false;
    for ((start, end), references) in replacements.into_iter().rev() {
        if references.len() == 1 {
            normalized.replace_range(start..end, references[0]);
            changed = true;
        }
    }
    if !changed {
        return Ok(None);
    }
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = file.name().to_string();
        let options =
            zip::write::SimpleFileOptions::default().compression_method(file.compression());
        if file.is_dir() {
            out.add_directory(name, options)
                .map_err(|e| e.to_string())?;
            continue;
        }
        let mut data = Vec::new();
        file.read_to_end(&mut data).map_err(|e| e.to_string())?;
        out.start_file(&name, options).map_err(|e| e.to_string())?;
        out.write_all(if name == "word/document.xml" {
            normalized.as_bytes()
        } else {
            &data
        })
        .map_err(|e| e.to_string())?;
    }
    Ok(Some(out.finish().map_err(|e| e.to_string())?.into_inner()))
}

#[wasm_bindgen]
#[derive(Default)]
pub struct FlowSession {
    layers: Vec<Layer>,
    root: String,
    pictures: Pictures,
    selection: Option<(String, String, i32)>,
    history: Vec<Value>,
    changes: Vec<Value>,
    conflicts: Vec<Value>,
    warnings: Vec<String>,
    revisions: BTreeMap<String, Composed>,
    key: Option<milleleaf_store::KeyPair>,
    original_workbook: Option<Composed>,
    returned_files: BTreeMap<String, (String, Vec<u8>)>,
    bundled_sample: bool,
}

impl FlowSession {
    fn flat(&self) -> Flat {
        let mut f = Flat::default();
        for l in &self.layers {
            f.apply(l);
        }
        f
    }
    fn compose(&self, path: &str) -> Result<Composed> {
        Composer::new(&self.flat())
            .compose(path, None, &mut vec![])
            .map_err(|e| e.to_string())
    }
    fn push(&mut self, mut l: Layer, app: &str, changes: Vec<Value>, conflicts: Vec<Value>) {
        l.header.id = format!("milleleaf/flow/{}", self.layers.len() + 1);
        if let Some(last) = self.layers.last() {
            l.header.base = Some(milleleaf_store::hash_of(&milleleaf_store::layer_bytes(
                last,
            )));
        }
        self.history.push(json!({"id":l.header.id,"author":l.header.author,"at":l.header.timestamp,"app":app,"changes":changes,"conflicts":conflicts,"layer":l}));
        self.changes = changes;
        self.conflicts = conflicts;
        self.layers.push(l);
    }
    pub fn sample_native(&mut self, author: &str, at: &str) -> Result<Value> {
        let mut f = Flat::default();
        for raw in [
            include_str!("../../../examples/hello-table/base.mleaf.json"),
            include_str!("../../../examples/hello-table/finance-review.mleaf.json"),
        ] {
            let l: Layer = serde_json::from_str(raw).map_err(|e| e.to_string())?;
            f.apply(&l);
        }
        let wb = Composer::new(&f)
            .compose(WB, None, &mut vec![])
            .map_err(|e| e.to_string())?;
        let (w, _) = milleleaf_gridcraft::export_with(&wb, PictureSource::Memory(&Pictures::new()))
            .map_err(|e| e.to_string())?;
        self.open_native(
            &milleleaf_gridcraft::to_xlsx(&w).map_err(|e| e.to_string())?,
            author,
            at,
        )?;
        self.bundled_sample = true;
        if let Some(l) = self.layers.first_mut()
            && let Some(n) = l
                .data
                .iter_mut()
                .find(|n| n.path == "flow-source-file-evidence")
        {
            n.attributes["flow::file"]["name"] = json!("finance.xlsx");
            n.attributes["flow::file"]["label"] = json!("finance.xlsx");
        }
        if let Some(h) = self.history.first_mut() {
            h["file"] = json!("finance.xlsx");
            h["layer"] = json!(self.layers.first());
        }
        self.inspect_native()
    }
    pub fn open_native(&mut self, bytes: &[u8], author: &str, at: &str) -> Result<Value> {
        if author.trim().is_empty() || at.trim().is_empty() {
            return Err("Enter a claimed name and local time.".into());
        }
        let archive = parts(bytes)?;
        let ods = archive.contains_key("mimetype") && !archive.contains_key("xl/workbook.xml");
        let (mut l, pictures, warnings) = if ods {
            let (wb, warnings) = milleleaf_gridcraft::from_ods(bytes).map_err(|e| e.to_string())?;
            let (l, r) =
                milleleaf_gridcraft::adopt(&wb, WB, "Workbook", header("base", author, at))
                    .map_err(|e| e.to_string())?;
            let mut ws = warnings;
            ws.extend(r.warnings);
            ws.push(ODS_NOTE.into());
            (l, r.pictures, ws)
        } else {
            let (l, r) = milleleaf_gridcraft::adopt_for_comparison(
                bytes,
                WB,
                "Workbook",
                header("base", author, at),
            )
            .map_err(|e| e.to_string())?;
            (l, r.pictures, r.warnings)
        };
        *self = Self::default();
        self.root = WB.into();
        self.pictures = pictures;
        self.warnings = warnings;
        let app = milleleaf_formats::saved_by(bytes).unwrap_or_else(|| {
            if ods {
                "ODS file; saving app unknown".into()
            } else {
                "XLSX file; saving app unknown".into()
            }
        });
        let hash = milleleaf_core::integrity_of(bytes);
        let mime = if ods {
            "application/vnd.oasis.opendocument.spreadsheet"
        } else {
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        };
        let label = if ods {
            "Workbook source (.ods)"
        } else {
            "Workbook source (.xlsx)"
        };
        self.returned_files
            .insert(hash.clone(), (mime.into(), bytes.to_vec()));
        l.data.push(node("flow-source-file-evidence",json!({"flow::file":{"name":null,"label":label,"savedBy":app,"hash":hash,"blob":hash,"mime":mime}}),&[]));
        self.push(
            l,
            &app,
            vec![json!({"description":"Opened the workbook as the source."})],
            vec![],
        );
        if let Some(h) = self.history.last_mut() {
            h["file"] = json!(label);
            h["source_hash"] = json!(hash);
            h["source_mime"] = json!(mime);
        }
        let baseline = self.compose(&self.root)?;
        self.original_workbook = Some(baseline.clone());
        if archive.contains_key("xl/workbook.xml") {
            let wb = milleleaf_gridcraft::from_xlsx(bytes).map_err(|e| e.to_string())?;
            if let Some(rev) = milleleaf_gridcraft::exported_revision(&wb) {
                self.revisions.insert(rev, baseline);
            }
        }
        self.inspect_native()
    }
    pub fn inspect_native(&self) -> Result<Value> {
        let wb = self.compose(&self.root)?;
        let mut chart_index = 0usize;
        let sheets=wb.children.iter().map(|sh| {
            let cs=cells(sh); let rr=cs.keys().map(|(r,_)|*r).max().unwrap_or(0); let cc=cs.keys().map(|(_,c)|*c).max().unwrap_or(0);
            let rows:Vec<Vec<Value>>=(0..=rr.min(199)).map(|r|(0..=cc.min(49)).map(|c|cs.get(&(r,c)).cloned().unwrap_or(Value::Null)).collect()).collect();
            let charts:Vec<Value>=sh.attributes["grid::sheet"]["charts"].as_array().into_iter().flatten().map(|c|{
                let i=chart_index;chart_index+=1;
                let kind=c["kind"].as_str().unwrap_or("ColumnClustered");let supported=selected_chart_kind(kind);
                json!({"index":i,"title":c.get("title").cloned().unwrap_or(json!("Workbook chart")),"chart":c,"kind":kind,"supported":supported.is_ok(),"support_note":supported.err().unwrap_or_else(||"Redrawn from the workbook cells in the report and slide styles. The selected kind is kept; report and slide styling are independent.".into())})
            }).collect();
            let full_range=format!("A1:{}",a1(rr,cc));
            let suggested=if self.bundled_sample {sh.children.iter().find(|n|n.attributes.contains_key("office::data::table")).and_then(|n|n.attributes["grid::range"]["ref"].as_str()).unwrap_or(&full_range)}else{&full_range};
            json!({"path":sh.path,"name":sh.attributes["grid::sheet"]["name"],"range":suggested,"full_range":full_range,"rows":rows,"charts":charts,"preview_note":if rr>=200||cc>=50 {"Preview shows the first 200 rows and 50 columns; all cells remain in the workbook."}else{""}})
        }).collect::<Vec<_>>();
        let source = self.history.first();
        Ok(
            json!({"sheets":sheets,"history":self.history,"warnings":self.warnings,"source_app":source.and_then(|h|h.get("app")),"source_hash":source.and_then(|h|h.get("source_hash")),"source_mime":source.and_then(|h|h.get("source_mime"))}),
        )
    }
    fn table(&self) -> Result<Value> {
        let (path, range, _) = self
            .selection
            .as_ref()
            .ok_or("Choose a source range first.")?;
        let sh = self.compose(path)?;
        let cs = cells(&sh);
        let (r, c, rr, cc) = bounds(range)?;
        let cols:Vec<Value>=(c..=cc).map(|j| json!({"name":cs.get(&(r,j)).and_then(Value::as_str).map(String::from).unwrap_or_else(||a1(r,j)),"type":if (r+1..=rr).any(|i|cs.get(&(i,j)).is_some_and(Value::is_number)){"number"}else{"text"}})).collect();
        let rows: Vec<Vec<Value>> = (r + 1..=rr)
            .map(|i| {
                (c..=cc)
                    .map(|j| cs.get(&(i, j)).cloned().unwrap_or(Value::Null))
                    .collect()
            })
            .collect();
        Ok(json!({"columns":cols,"rows":rows}))
    }
    fn chart_spec(&self, table: &Value) -> Value {
        json!({"kind":"column","title":"Workbook figures","category":table["columns"][0]["name"],"values":table["columns"].as_array().into_iter().flatten().filter(|c|c["type"]=="number").map(|c|c["name"].clone()).collect::<Vec<_>>()})
    }
    fn ref_values(&self, reference: &str, fallback: &str) -> Result<Vec<Value>> {
        let (name, range) = reference
            .trim_start_matches('=')
            .rsplit_once('!')
            .unwrap_or((fallback, reference));
        let name = name.trim_matches('\'').replace("''", "'");
        let wb = self.compose(&self.root)?;
        let sh = wb
            .children
            .iter()
            .find(|s| s.attributes["grid::sheet"]["name"].as_str() == Some(&name))
            .ok_or_else(|| format!("Chart refers to unknown sheet {name}."))?;
        let cs = cells(sh);
        let (r, c, rr, cc) = bounds(range)?;
        Ok((r..=rr)
            .flat_map(|i| (c..=cc).map(move |j| (i, j)))
            .map(|at| cs.get(&at).cloned().unwrap_or(Value::Null))
            .collect())
    }
    fn chart_source(&self) -> Result<(Value, Value)> {
        let (_, _, index) = self.selection.as_ref().ok_or("Choose a source first.")?;
        if *index < 0 {
            let t = self.table()?;
            let spec = self.chart_spec(&t);
            return Ok((t, spec));
        }
        let (owner, chart) = self.selected_workbook_chart(*index)?;
        let name = owner.as_str();
        let series = chart["series"]
            .as_array()
            .ok_or("The selected chart has no readable series.")?;
        if series.is_empty() {
            return Err("The selected chart has no series pointing to workbook cells.".into());
        }
        let mut cols = vec![json!({"name":"Category","type":"text"})];
        let mut values = Vec::new();
        let mut cats = Vec::new();
        for (i, s) in series.iter().enumerate() {
            if cats.is_empty()
                && let Some(r) = s["categories"].as_str()
            {
                cats = self.ref_values(r, name)?;
            }
            let reference = s["values"]
                .as_str()
                .ok_or("Chart series does not point to workbook cells.")?;
            values.push(self.ref_values(reference, name)?);
            let label = s["name"].as_str().unwrap_or("");
            let label = if label.contains('!') {
                self.ref_values(label, name)?
                    .first()
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string()
            } else {
                label.trim_matches('"').to_string()
            };
            cols.push(json!({"name":if label.is_empty(){format!("Series {}",i+1)}else{label},"type":"number"}));
        }
        let len = values.iter().map(Vec::len).max().unwrap_or(0);
        if len == 0 || !values.iter().flatten().any(Value::is_number) {
            return Err("The selected chart has no numeric values in its referenced cells.".into());
        }
        let rows: Vec<Vec<Value>> = (0..len)
            .map(|i| {
                let mut row = vec![cats.get(i).cloned().unwrap_or(json!((i + 1).to_string()))];
                row.extend(
                    values
                        .iter()
                        .map(|v| v.get(i).cloned().unwrap_or(Value::Null)),
                );
                row
            })
            .collect();
        let table = json!({"columns":cols,"rows":rows});
        let mut spec = self.chart_spec(&table);
        let original_kind = chart["kind"].as_str().unwrap_or("ColumnClustered");
        spec["kind"] = json!(selected_chart_kind(original_kind)?);
        spec["flowWorkbookKind"] = json!(original_kind);
        spec["flowChartOwner"] = json!(owner);
        spec["title"] = chart["title"].clone();
        chart_svg::validate(&table, &spec)?;
        Ok((table, spec))
    }
    fn selected_workbook_chart(&self, index: i32) -> Result<(String, Value)> {
        if index < 0 {
            return Err("Choose a workbook chart.".into());
        }
        let wb = self.compose(&self.root)?;
        let mut global = 0usize;
        for sh in &wb.children {
            for chart in sh.attributes["grid::sheet"]["charts"]
                .as_array()
                .into_iter()
                .flatten()
            {
                if global == index as usize {
                    return Ok((
                        sh.attributes["grid::sheet"]["name"]
                            .as_str()
                            .unwrap_or("")
                            .into(),
                        chart.clone(),
                    ));
                }
                global += 1;
            }
        }
        Err("Choose an available chart from any workbook sheet.".into())
    }
    pub fn create_native(
        &mut self,
        sheet: &str,
        range: &str,
        chart: i32,
        author: &str,
        at: &str,
    ) -> Result<Value> {
        if author.trim().is_empty() || at.trim().is_empty() {
            return Err("Enter a claimed name and local time.".into());
        }
        let (r, c, rr, cc) = bounds(range)?;
        if rr == r {
            return Err("Choose a header row and at least one data row.".into());
        }
        if chart < 0 && cc == c {
            return Err("Making a chart from the range needs at least two columns.".into());
        }
        let sh = self.compose(sheet)?;
        if !sh.attributes.contains_key("grid::sheet") {
            return Err("Choose a workbook sheet.".into());
        }
        if chart >= 0 {
            self.selected_workbook_chart(chart)?;
        }
        let previous = self.selection.replace((sheet.into(), range.into(), chart));
        let selected = (|| {
            let table = self.table()?;
            if chart < 0
                && !table["rows"].as_array().into_iter().flatten().any(|r| {
                    r.as_array()
                        .into_iter()
                        .flatten()
                        .skip(1)
                        .any(Value::is_number)
                })
            {
                return Err(
                    "Choose a range with a numeric data column after its first column.".into(),
                );
            }
            let (chart_table, spec) = self.chart_source()?;
            Ok((table, chart_table, spec))
        })();
        let (table, chart_table, spec) = match selected {
            Ok(values) => values,
            Err(error) => {
                self.selection = previous;
                return Err(error);
            }
        };
        let l = Layer {
            header: header("proposal", author, at),
            data: vec![
                node(
                    SOURCE,
                    json!({"office::data::table":table,"flow::selection":{"workbook":self.root,"sheet":sheet,"range":range,"chart":chart}}),
                    &[],
                ),
                node(
                    CHART_SOURCE,
                    json!({"office::data::table":chart_table}),
                    &[],
                ),
                node(
                    REPORT,
                    json!({"word::document":{"title":"Workbook report","pageSize":"A4"}}),
                    &[("title", "flow-report-title"), ("table", RT), ("chart", RC)],
                ),
                node(
                    "flow-report-title",
                    json!({"word::paragraph":{"style":"Heading 1","runs":[{"text":"Workbook report"}]}}),
                    &[],
                ),
                link(
                    RT,
                    json!({"word::table":{"style":"Grid Table 4","caption":"Linked workbook figures"}}),
                ),
                node(
                    RC,
                    json!({"word::paragraph":{"runs":[{"text":"Workbook chart"}]},"mleaf::link":{"source":CHART_SOURCE,"mode":"live"},"flow::render":{"source":CHART_SOURCE,"kind":"svg-chart","chartKind":spec["kind"],"generatorVersion":2}}),
                    &[],
                ),
                node(
                    DECK,
                    json!({"deck::deck":{"title":"Workbook deck","width":1280,"height":720}}),
                    &[("slide", "flow-slide")],
                ),
                node(
                    "flow-slide",
                    json!({"deck::slide":{"name":"Workbook figures","background":"#f4f7fb"}}),
                    &[("title", "flow-deck-title"), ("table", DT), ("chart", DC)],
                ),
                node(
                    "flow-deck-title",
                    json!({"deck::frame":{"x":60,"y":32,"w":1100,"h":80},"deck::text":{"paragraphs":[{"align":"left","runs":[{"text":"Workbook figures","font":"Inter","sizePt":30,"color":"#183d4d"}]}]}}),
                    &[],
                ),
                link(
                    DT,
                    json!({"deck::frame":{"x":60,"y":160,"w":550,"h":380},"deck::table":{"style":{"font":"Arial","sizePt":17}}}),
                ),
                {
                    let mut n = link(
                        DC,
                        json!({"deck::frame":{"x":660,"y":160,"w":550,"h":380},"deck::chart":spec}),
                    );
                    n.inherits
                        .insert("source".into(), Some(CHART_SOURCE.into()));
                    n.attributes.insert(
                        "mleaf::link".into(),
                        json!({"source":CHART_SOURCE,"mode":"live"}),
                    );
                    n
                },
            ],
            imports: vec![],
            schemas: Default::default(),
        };
        self.warnings.push("The report chart is a rendered picture linked to workbook cells. The deck chart is a native editable chart. Both refresh from the workbook.".into());
        self.warnings.push("The report picture keeps the selected column, bar, line or pie kind. Every series is included; multiple pie series use separate pies. Column, bar and line charts keep negative values. Report and slide styles are independent of workbook chart formatting.".into());
        self.push(l,"Milleleaf",vec![json!({"description":"Used the workbook range in a report table, a report chart, a slide table and a slide chart."})],vec![]);
        // Creating the outputs establishes their canonical baselines even before a download.
        let report = self.rendered(REPORT)?;
        let deck = self.compose(DECK)?;
        self.revisions
            .insert(milleleaf_wordcraft::revision_of(&report), report);
        self.revisions
            .insert(milleleaf_deckcraft::revision_of(&deck), deck);
        self.snapshot_native()
    }
    fn rendered(&mut self, path: &str) -> Result<Composed> {
        let mut n = self.compose(path)?;
        if path == REPORT {
            let table = self.compose(CHART_SOURCE)?.attributes["office::data::table"].clone();
            let chart = self.compose(DC)?;
            let bytes = chart_svg::svg(&table, &chart.attributes["deck::chart"])?;
            let svg = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
            let intrinsic_height = svg
                .split_once("height=\"")
                .and_then(|(_, v)| v.split('"').next())
                .and_then(|v| v.parse::<f64>().ok())
                .ok_or("The rendered chart has no intrinsic height.")?;
            let image_height = (460.0 * intrinsic_height / 800.0 * 100.0).round() / 100.0;
            let blob = milleleaf_core::integrity_of(&bytes);
            self.pictures
                .insert(blob.clone(), ("image/svg+xml".into(), bytes));
            if let Some(c) = n.children.iter_mut().find(|c| c.path == RC) {
                c.attributes.insert("word::paragraph".into(),json!({"runs":[{"picture":{"blob":blob,"mediaType":"image/svg+xml","w":460,"h":image_height,"alt":"Workbook chart"}}]}));
            }
        }
        Ok(n)
    }
    pub fn export_native(&mut self, kind: &str) -> Result<Vec<u8>> {
        if kind == "layers" {
            return Ok(self.project().into_bytes());
        }
        let path = match kind {
            "xlsx" => self.root.clone(),
            "docx" => REPORT.into(),
            "pptx" => DECK.into(),
            _ => return Err("Choose xlsx, docx or pptx.".into()),
        };
        let n = self.rendered(&path)?;
        let assets = PictureSource::Memory(&self.pictures);
        let (bytes, rev, warnings) = match kind {
            "xlsx" => {
                let (w, r) =
                    milleleaf_gridcraft::export_with(&n, assets).map_err(|e| e.to_string())?;
                (
                    milleleaf_gridcraft::to_xlsx(&w).map_err(|e| e.to_string())?,
                    milleleaf_gridcraft::revision_of(&n),
                    r.warnings,
                )
            }
            "docx" => {
                let (w, r) = milleleaf_wordcraft::export_with(&n, &self.flat().layers, assets)
                    .map_err(|e| e.to_string())?;
                (
                    milleleaf_wordcraft::to_docx(&w).map_err(|e| e.to_string())?,
                    milleleaf_wordcraft::revision_of(&n),
                    r.warnings,
                )
            }
            _ => {
                let (w, r) =
                    milleleaf_deckcraft::export_with(&n, assets).map_err(|e| e.to_string())?;
                (
                    milleleaf_deckcraft::to_pptx(&w).map_err(|e| e.to_string())?,
                    milleleaf_deckcraft::revision_of(&n),
                    r.warnings,
                )
            }
        };
        // Word's own structure for a vector picture: the raster in `a:blip`, the SVG as Word's
        // extension on that same blip. The workbook has no pictures.
        let bytes = match kind {
            "xlsx" => bytes,
            _ => png_fallback::add_png_fallbacks(&bytes)?,
        };
        self.warnings.extend(warnings);
        self.revisions.insert(rev, n);
        Ok(bytes)
    }
    pub fn snapshot_native(&mut self) -> Result<Value> {
        let table = self.table()?;
        let chart_node = self.compose(DC)?;
        let spec = chart_node.attributes["deck::chart"]
            .as_object()
            .ok_or("Create the linked report and deck first.")?;
        let (categories, series) = milleleaf_core::render::chart_data(&chart_node, spec);
        let chart = json!({"categories":categories,"series":series.into_iter().map(|(name,values)|json!({"name":name,"values":values})).collect::<Vec<_>>()});
        Ok(
            json!({"report":{"table":table,"chart":chart},"deck":{"table":table,"chart":chart},"history":self.history,"changes":self.changes,"conflicts":self.conflicts,"warnings":self.warnings}),
        )
    }
    pub fn import_native(
        &mut self,
        bytes: &[u8],
        filename: &str,
        author: &str,
        at: &str,
    ) -> Result<Value> {
        if author.trim().is_empty() || at.trim().is_empty() {
            return Err("Enter a claimed name and local time.".into());
        }
        let archive = parts(bytes)?;
        if archive.contains_key("mimetype") && !archive.contains_key("xl/workbook.xml") {
            let (wb, warnings) = milleleaf_gridcraft::from_ods(bytes).map_err(|e| e.to_string())?;
            let converted = milleleaf_gridcraft::to_xlsx(&wb).map_err(|e| e.to_string())?;
            self.warnings.extend(warnings);
            self.warnings.push(ODS_NOTE.into());
            return self.import_ooxml_native(&converted, filename, author, at, bytes, true);
        }
        self.import_ooxml_native(bytes, filename, author, at, bytes, false)
    }
    fn import_ooxml_native(
        &mut self,
        bytes: &[u8],
        filename: &str,
        author: &str,
        at: &str,
        original: &[u8],
        ods: bool,
    ) -> Result<Value> {
        let kind = milleleaf_formats::inventory::inventory(bytes)?.0;
        let h = header("proposal", author, at);
        let mut referenced_svg = false;
        let (mut layer, report, pictures, before) = match kind {
            "xlsx" => {
                let mut w = milleleaf_gridcraft::from_xlsx(bytes).map_err(|e| e.to_string())?;
                let rev = milleleaf_gridcraft::exported_revision(&w);
                let known = rev.as_ref().and_then(|r| self.revisions.get(r)).cloned();
                if known.is_none() {
                    self.warnings.push("The workbook revision is not recorded in this session. Compared with the original workbook; overlapping project changes become conflicts.".into());
                }
                let b = known
                    .or_else(|| self.original_workbook.clone())
                    .ok_or("Open the source workbook first.")?;
                let (mut l, mut r) = milleleaf_gridcraft::import_for_comparison(
                    &b,
                    &mut w,
                    h,
                    milleleaf_formats::saved_by(bytes).as_deref(),
                )
                .map_err(|e| e.to_string())?;
                milleleaf_gridcraft::keep_charts(&mut l, bytes, &mut r.pictures, &mut r.warnings);
                (
                    l,
                    serde_json::to_value(&r).map_err(|e| e.to_string())?,
                    r.pictures,
                    b,
                )
            }
            "docx" => {
                let normalized = prefer_referenced_svg(bytes)?;
                referenced_svg = normalized.is_some();
                let w = milleleaf_wordcraft::from_docx(normalized.as_deref().unwrap_or(bytes))
                    .map_err(|e| e.to_string())?;
                let rev = milleleaf_wordcraft::exported_revision(&w);
                let b=rev.and_then(|r|self.revisions.get(&r).cloned()).ok_or("This report revision was not exported in this session. Use a report downloaded from this project.")?;
                let (l, r) = milleleaf_wordcraft::import(&b, &w, h).map_err(|e| e.to_string())?;
                (
                    l,
                    serde_json::to_value(&r).map_err(|e| e.to_string())?,
                    r.pictures,
                    b,
                )
            }
            "pptx" => {
                let w = milleleaf_deckcraft::from_pptx(bytes).map_err(|e| e.to_string())?;
                let rev = milleleaf_deckcraft::exported_revision(&w);
                let b=rev.and_then(|r|self.revisions.get(&r).cloned()).ok_or("This deck revision was not exported in this session. Use a deck downloaded from this project.")?;
                let (l, r) = milleleaf_deckcraft::import(&b, &w, h).map_err(|e| e.to_string())?;
                (
                    l,
                    serde_json::to_value(&r).map_err(|e| e.to_string())?,
                    r.pictures,
                    b,
                )
            }
            _ => {
                return Err(
                    "Drop an .xlsx, .docx or .pptx file. Native ODF needs the CLI and LibreOffice."
                        .into(),
                );
            }
        };
        self.pictures.extend(pictures);
        if referenced_svg {
            self.warnings.push("This report retains its explicitly referenced vector drawing alongside a PNG preview. Milleleaf compared the vector drawing and kept the complete returned file.".into());
        }
        self.warnings.extend(
            report["warnings"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(String::from),
        );
        let mut conflicts = report["conflicts"].as_array().cloned().unwrap_or_default();
        if kind == "docx" && layer.data.iter().any(|n| n.path == RC) {
            conflicts.push(json!({"path":RC,"message":"The report chart is linked to workbook cells. The returned picture edit is a conflict; the workbook chart stays current."}));
            layer.data.retain(|n| n.path != RC);
        }
        let current = if kind == "docx" {
            self.rendered(REPORT)?
        } else {
            self.compose(&before.path)?
        };
        if ods {
            for n in &mut layer.data {
                if let Some(gs) = n
                    .attributes
                    .get_mut("grid::sheet")
                    .and_then(Value::as_object_mut)
                    && let Some(charts) =
                        milleleaf_core::render::find(std::slice::from_ref(&current), &n.path)
                            .and_then(|v| v.attributes["grid::sheet"].get("charts"))
                {
                    gs.insert("charts".into(), charts.clone());
                }
            }
        }
        for n in &mut layer.data {
            let now = milleleaf_core::render::find(std::slice::from_ref(&current), &n.path);
            strip_noops(n, now);
        }
        layer.data.retain(|n| {
            !n.attributes.is_empty()
                || !n.children.is_empty()
                || n.order.is_some()
                || !n.inherits.is_empty()
        });
        layer.data.retain(|n| {
            let old=milleleaf_core::render::find(std::slice::from_ref(&before),&n.path);
            let now=milleleaf_core::render::find(std::slice::from_ref(&current),&n.path);
            let overlaps=overlap(n,old,now);
            if overlaps {conflicts.push(json!({"path":n.path,"message":"This place changed since the returned file was exported. Review this proposal before applying it.","proposal":n}));} !overlaps
        });
        let old_table = self.selection.as_ref().map(|_| self.table()).transpose()?;
        let old_chart = self
            .selection
            .as_ref()
            .map(|_| {
                self.compose(CHART_SOURCE)
                    .map(|n| n.attributes["office::data::table"].clone())
            })
            .transpose()?;
        let mut f = self.flat();
        f.apply(&layer);
        let after = Composer::new(&f)
            .compose(&before.path, None, &mut vec![])
            .map_err(|e| e.to_string())?;
        let mut changes = super::describe::describe(&before, &after, &layer);
        if kind == "xlsx" && self.selection.is_some() {
            self.layers.push(layer.clone());
            let table = self.table();
            let chart = self.chart_source();
            self.layers.pop();
            let table = table?;
            let (ct, spec) = chart?;
            if old_chart.as_ref() != Some(&ct) {
                layer
                    .data
                    .push(node(CHART_SOURCE, json!({"office::data::table":ct}), &[]));
                for (file, place) in [
                    ("report.docx", "Report chart"),
                    ("deck.pptx", "Slide chart"),
                ] {
                    changes.push(json!({"file":file,"place":place,"description":format!("{place} now uses the changed chart cells."),"before":old_chart,"after":ct}));
                }
            }
            if self.compose(DC)?.attributes["deck::chart"] != spec {
                layer.data.push(node(DC, json!({"deck::chart":spec}), &[]));
            }
            if old_table.as_ref() != Some(&table) {
                layer
                    .data
                    .push(node(SOURCE, json!({"office::data::table":table}), &[]));
                for (file, place) in [
                    ("report.docx", "Report table"),
                    ("deck.pptx", "Slide table"),
                ] {
                    changes.push(json!({"file":file,"place":place,"description":format!("{place} now uses the changed workbook cells."),"before":old_table,"after":table}));
                }
            }
        }
        for c in &mut changes {
            if c.get("file").is_none() {
                c["file"] = json!(filename);
            }
        }
        let app = milleleaf_formats::saved_by(original).unwrap_or_else(|| {
            if ods {
                "OpenDocument workbook".into()
            } else {
                filename.into()
            }
        });
        let blob = milleleaf_core::integrity_of(original);
        let mime = if ods {
            "application/vnd.oasis.opendocument.spreadsheet"
        } else {
            match kind {
                "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                _ => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            }
        };
        self.returned_files
            .insert(blob.clone(), (mime.into(), original.to_vec()));
        let proposed = if conflicts.is_empty() {
            None
        } else {
            match super::read(bytes, header("returned-file snapshot", author, at))
                .and_then(|adopted| super::tree(&adopted.layer))
            {
                Ok(tree) => Some(tree),
                Err(error) => {
                    self.warnings.push(format!("Proposed cells could not be matched independently: {error}. The original returned file is retained for review."));
                    None
                }
            }
        };
        enrich_conflicts(
            &mut conflicts,
            &before,
            &current,
            proposed.as_ref(),
            filename,
        );
        for c in &mut conflicts {
            c["returned_file"] = json!({"blob":blob,"mime":mime,"name":filename});
            c["evidence_note"] = json!(
                "The original returned file is retained in the downloaded project for review."
            );
        }
        layer.data.push(node("flow-file-evidence",json!({"flow::file":{"name":filename,"savedBy":app,"hash":blob,"blob":blob,"mime":mime,"referencedSvgCompared":referenced_svg},"flow::conflicts":conflicts}),&[]));
        self.push(layer, &app, changes, conflicts);
        if let Some(h) = self.history.last_mut() {
            h["file"] = json!(filename);
        }
        self.snapshot_native()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read, Write};
    const AT: &str = "2026-10-22T12:00:00Z";
    fn ready() -> FlowSession {
        let mut s = FlowSession::default();
        let i = s.sample_native("Sam", AT).unwrap();
        s.create_native(
            i["sheets"][0]["path"].as_str().unwrap(),
            "A1:B4",
            -1,
            "Sam",
            AT,
        )
        .unwrap();
        s
    }
    fn excel_ready() -> FlowSession {
        let mut s = FlowSession::default();
        let inspection = s
            .open_native(
                include_bytes!(
                    "../../../examples/hello-table/fixtures/finance.msoffice-resaved.xlsx"
                ),
                "Excel source",
                AT,
            )
            .unwrap();
        s.create_native(
            inspection["sheets"][0]["path"].as_str().unwrap(),
            "A1:B4",
            -1,
            "Sam",
            AT,
        )
        .unwrap();
        s
    }
    fn rewrite(bytes: &[u8], part: &str, from: &str, to: &str) -> Vec<u8> {
        let mut src = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut dst = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let mut changed = false;
        for i in 0..src.len() {
            let mut f = src.by_index(i).unwrap();
            let name = f.name().to_string();
            let mut b = Vec::new();
            f.read_to_end(&mut b).unwrap();
            if name == part {
                let text = String::from_utf8(b).unwrap();
                assert!(text.contains(from), "{part} lacks {from}");
                b = text.replace(from, to).into_bytes();
                changed = true;
            }
            dst.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            dst.write_all(&b).unwrap();
        }
        assert!(changed);
        dst.finish().unwrap().into_inner()
    }
    fn out() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../_out/flow")
    }
    fn xml_part(bytes: &[u8], name: &str) -> String {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut text = String::new();
        archive
            .by_name(name)
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        text
    }
    fn add_part(bytes: &[u8], name: &str, data: &[u8]) -> Vec<u8> {
        let mut src = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut dst = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..src.len() {
            let mut f = src.by_index(i).unwrap();
            let mut b = Vec::new();
            f.read_to_end(&mut b).unwrap();
            dst.start_file(f.name(), zip::write::SimpleFileOptions::default())
                .unwrap();
            dst.write_all(&b).unwrap();
        }
        dst.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        dst.write_all(data).unwrap();
        dst.finish().unwrap().into_inner()
    }
    fn first_embed(element: &str) -> String {
        element
            .split("r:embed=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .to_string()
    }
    fn svg_element(xml: &str) -> String {
        let start = xml.find("<a:blip ").unwrap();
        let opening_end = start + xml[start..].find('>').unwrap() + 1;
        let end = if xml[start..opening_end].ends_with("/>") {
            opening_end
        } else {
            opening_end + xml[opening_end..].find("</a:blip>").unwrap() + "</a:blip>".len()
        };
        xml[start..end].to_string()
    }
    fn svg_with_png_fallback(bytes: &[u8], reference_svg: bool) -> Vec<u8> {
        let element = svg_element(&xml_part(bytes, "word/document.xml"));
        // A resave may rewrite the raster part; with the extension kept the SVG stays the
        // authoritative image. Without it the changed raster is an ordinary picture edit.
        let resaved = if reference_svg {
            element.replacen(
                &format!("r:embed=\"{}\"", first_embed(&element)),
                "r:embed=\"flowPng\"",
                1,
            )
        } else {
            "<a:blip r:embed=\"flowPng\"></a:blip>".to_string()
        };
        let mut bytes = rewrite(bytes, "word/document.xml", &element, &resaved);
        bytes = rewrite(
            &bytes,
            "word/_rels/document.xml.rels",
            "</Relationships>",
            "<Relationship Id=\"flowPng\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/flow-preview.png\"/></Relationships>",
        );
        // The reader carries image bytes without raster decoding; the original vector is the
        // authoritative image for the referenced case. Unreferenced bytes must never be aliased.
        add_part(
            &bytes,
            "word/media/flow-preview.png",
            &[137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 0],
        )
    }
    #[test]
    fn explicit_svg_reference_is_compared_without_hiding_real_image_edits() {
        let mut s = ready();
        let original = s.export_native("docx").unwrap();
        let resaved = svg_with_png_fallback(&original, true);
        assert!(prefer_referenced_svg(&resaved).unwrap().is_some());
        let result = s
            .import_native(&resaved, "writer-resaved.docx", "Writer", AT)
            .unwrap();
        assert_eq!(result["changes"], json!([]));
        assert_eq!(result["conflicts"], json!([]));
        let lo_alias = rewrite(
            &resaved,
            "[Content_Types].xml",
            "image/svg+xml",
            "image/svg",
        );
        assert!(
            prefer_referenced_svg(&lo_alias).unwrap().is_some(),
            "LibreOffice uses image/svg for the explicitly referenced valid SVG"
        );
        let result = s
            .import_native(&lo_alias, "writer-svg-alias.docx", "Writer", AT)
            .unwrap();
        assert_eq!(result["changes"], json!([]));
        assert_eq!(result["conflicts"], json!([]));
        let unreferenced = svg_with_png_fallback(&original, false);
        assert!(prefer_referenced_svg(&unreferenced).unwrap().is_none());
        let fake = rewrite(
            &resaved,
            "word/document.xml",
            "<a:ext uri=",
            "<fake:ext xmlns:fake=\"urn:unrecognized\" uri=",
        );
        let fake = rewrite(&fake, "word/document.xml", "</a:ext>", "</fake:ext>");
        assert!(
            prefer_referenced_svg(&fake).unwrap().is_none(),
            "an unknown extension cannot make its SVG authoritative"
        );
        let fake = rewrite(
            &resaved,
            "word/_rels/document.xml.rels",
            "http://schemas.openxmlformats.org/package/2006/relationships",
            "urn:unrecognized",
        );
        assert!(
            prefer_referenced_svg(&fake).unwrap().is_none(),
            "unrecognized package relationships must not be used"
        );
        let fake = rewrite(
            &resaved,
            "[Content_Types].xml",
            "http://schemas.openxmlformats.org/package/2006/content-types",
            "urn:unrecognized",
        );
        assert!(
            prefer_referenced_svg(&fake).unwrap().is_none(),
            "unrecognized content types must not be used"
        );
        let result = s
            .import_native(&unreferenced, "changed-raster.docx", "Editor", AT)
            .unwrap();
        assert!(!result["conflicts"].as_array().unwrap().is_empty());
        let mut archive = zip::ZipArchive::new(Cursor::new(&resaved)).unwrap();
        let svg = (0..archive.len())
            .find_map(|i| {
                let f = archive.by_index(i).unwrap();
                (f.name().starts_with("word/media/") && f.name().ends_with(".svg"))
                    .then(|| f.name().to_string())
            })
            .unwrap();
        let changed = rewrite(&resaved, &svg, "#277c88", "#ff0000");
        let result = s
            .import_native(&changed, "changed-vector.docx", "Editor", AT)
            .unwrap();
        assert!(!result["conflicts"].as_array().unwrap().is_empty());
    }
    fn svg_chart(bytes: &[u8]) -> String {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let name = (0..archive.len())
            .find_map(|i| {
                let f = archive.by_index(i).unwrap();
                (f.name().starts_with("word/media/") && f.name().ends_with(".svg"))
                    .then(|| f.name().to_string())
            })
            .expect("report must contain its rendered SVG chart");
        let mut text = String::new();
        archive
            .by_name(&name)
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        text
    }
    fn package(parts: &[(&str, &[u8])]) -> Vec<u8> {
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in parts {
            z.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap().into_inner()
    }
    #[test]
    fn the_report_chart_is_a_png_with_the_svg_as_words_extension() {
        let mut s = ready();
        let docx = s.export_native("docx").unwrap();
        let blip = svg_element(&xml_part(&docx, "word/document.xml"));
        assert!(blip.contains(
            "<asvg:svgBlip xmlns:asvg=\"http://schemas.microsoft.com/office/drawing/2016/SVG/main\""
        ));
        let raster = first_embed(&blip);
        let vector = blip
            .split("<asvg:svgBlip")
            .nth(1)
            .unwrap()
            .split("r:embed=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        assert_ne!(raster, vector);
        let rels = xml_part(&docx, "word/_rels/document.xml.rels");
        let target = |id: &str| {
            rels.split(&format!("Id=\"{id}\""))
                .nth(1)
                .unwrap()
                .split("Target=\"")
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap()
                .to_string()
        };
        let raster_target = target(&raster);
        let vector_target = target(vector);
        assert!(raster_target.ends_with(".png"));
        assert!(vector_target.ends_with(".svg"));
        assert_eq!(
            xml_part(&docx, &format!("word/{vector_target}")),
            svg_chart(&docx)
        );
        let mut archive = zip::ZipArchive::new(Cursor::new(&docx)).unwrap();
        let mut png = Vec::new();
        archive
            .by_name(&format!("word/{raster_target}"))
            .unwrap()
            .read_to_end(&mut png)
            .unwrap();
        assert!(png.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10]));
        assert!(
            xml_part(&docx, "[Content_Types].xml")
                .contains("<Default Extension=\"png\" ContentType=\"image/png\"/>")
        );
        // A second pass leaves the same structure: blips with the extension are left as they are.
        assert_eq!(png_fallback::add_png_fallbacks(&docx).unwrap(), docx);
        // WordCraft's own import reads the file back, the SVG part still authoritative.
        let r = s.import_native(&docx, "report.docx", "Sam", AT).unwrap();
        assert_eq!(r["changes"], json!([]));
        assert_eq!(r["conflicts"], json!([]));
        // The deck's own chart is native and carries no picture; it round trips unchanged.
        let pptx = s.export_native("pptx").unwrap();
        assert_eq!(png_fallback::add_png_fallbacks(&pptx).unwrap(), pptx);
        assert!(xml_part(&pptx, "ppt/charts/chart1.xml").contains("<c:chartSpace"));
        let r = s.import_native(&pptx, "deck.pptx", "Sam", AT).unwrap();
        assert_eq!(r["changes"], json!([]));
        assert_eq!(r["conflicts"], json!([]));
    }
    #[test]
    fn a_deck_svg_picture_carries_its_png_the_same_way() {
        // An adopted deck's vector pictures are written through the same structure.
        let svg = String::from_utf8(
            chart_svg::svg(
                &json!({"columns":[{"name":"Day"},{"name":"First"}],"rows":[["Mon",12]]}),
                &json!({"kind":"column","title":"Figures","category":"Day","values":["First"]}),
            )
            .unwrap(),
        )
        .unwrap();
        let patched = png_fallback::add_png_fallbacks(&package(&[
            (
                "[Content_Types].xml",
                br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="svg" ContentType="image/svg+xml"/></Types>"#,
            ),
            (
                "ppt/slides/slide1.xml",
                br#"<p:sld><p:cSld><p:spTree><p:pic><p:blipFill><a:blip r:embed="rId1"/><a:stretch><a:fillRect/></a:stretch></p:blipFill></p:pic></p:spTree></p:cSld></p:sld>"#,
            ),
            (
                "ppt/slides/_rels/slide1.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/pic.svg"/></Relationships>"#,
            ),
            ("ppt/media/pic.svg", svg.as_bytes()),
        ]))
        .unwrap();
        let xml = xml_part(&patched, "ppt/slides/slide1.xml");
        assert!(xml.contains("r:embed=\"rId2\""));
        assert!(xml.contains(
            "<asvg:svgBlip xmlns:asvg=\"http://schemas.microsoft.com/office/drawing/2016/SVG/main\" r:embed=\"rId1\"/>"
        ));
        assert!(xml_part(&patched, "ppt/slides/_rels/slide1.xml.rels").contains(
            "Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"../media/pic.png\""
        ));
        assert!(
            xml_part(&patched, "[Content_Types].xml")
                .contains("<Default Extension=\"png\" ContentType=\"image/png\"/>")
        );
        let mut archive = zip::ZipArchive::new(Cursor::new(&patched)).unwrap();
        let mut png = Vec::new();
        archive
            .by_name("ppt/media/pic.png")
            .unwrap()
            .read_to_end(&mut png)
            .unwrap();
        assert!(png.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10]));
        let mut svg_part = Vec::new();
        archive
            .by_name("ppt/media/pic.svg")
            .unwrap()
            .read_to_end(&mut svg_part)
            .unwrap();
        assert_eq!(svg_part, svg.as_bytes());
    }
    #[test]
    fn failed_selection_keeps_the_previous_project() {
        let mut s = ready();
        let before = s.project();
        let selected = s.selection.clone();
        // A valid rectangle with no numeric data exercises failure after tentative selection.
        assert!(
            s.create_native(&selected.as_ref().unwrap().0, "C1:D4", -1, "Sam", AT)
                .is_err()
        );
        assert_eq!(s.selection, selected);
        assert_eq!(s.project(), before);
        assert_eq!(s.table().unwrap()["rows"][1][1], 541500);
    }
    #[test]
    fn pinned_chart_kinds_keep_grouping_semantics() {
        assert_eq!(selected_chart_kind("ColumnClustered").unwrap(), "column");
        assert_eq!(selected_chart_kind("BarClustered").unwrap(), "bar");
        assert_eq!(selected_chart_kind("Line").unwrap(), "line");
        assert_eq!(selected_chart_kind("LineMarkers").unwrap(), "line");
        assert_eq!(selected_chart_kind("Pie").unwrap(), "pie");
        for kind in [
            "ColumnStacked",
            "ColumnStacked100",
            "BarStacked",
            "BarStacked100",
            "LineStacked",
        ] {
            let error = selected_chart_kind(kind).unwrap_err();
            assert!(error.contains(kind));
            assert!(error.contains("not silently changed"));
        }
    }
    #[test]
    fn existing_chart_tracks_cells_outside_the_selected_table() {
        // This small fixture was saved by GridCraft and carries real Chart serde fields.
        let mut s = FlowSession::default();
        let inspection = s
            .open_native(
                include_bytes!("../../../examples/real/milleleaf-charts.gridcraft.xlsx"),
                "GridCraft",
                AT,
            )
            .unwrap();
        let sheet = inspection["sheets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["name"] == "Round trips")
            .unwrap();
        assert!(!sheet["charts"].as_array().unwrap().is_empty());
        let initial = s
            .create_native(sheet["path"].as_str().unwrap(), "A2:B4", 0, "Sam", AT)
            .unwrap();
        assert_eq!(
            initial["deck"]["chart"]["series"][0]["values"],
            json!([12.0, 18.0, 15.0, 22.0, 27.0])
        );
        let unchanged = initial["report"]["table"].clone();
        let original = s.export_native("xlsx").unwrap();
        let edited = rewrite(
            &original,
            "xl/worksheets/sheet1.xml",
            "<v>27</v>",
            "<v>30</v>",
        );
        let result = s
            .import_native(&edited, "gridcraft-chart-edited.xlsx", "GridCraft", AT)
            .unwrap();
        assert_eq!(result["report"]["table"], unchanged);
        assert_eq!(result["deck"]["chart"]["series"][0]["values"][4], 30.0);
        let changed = result["changes"].as_array().unwrap();
        for file in ["report.docx", "deck.pptx"] {
            assert!(
                changed.iter().any(|c| c["file"] == file
                    && c["place"].as_str().is_some_and(|v| v.contains("chart"))),
                "{result}"
            );
        }
        assert!(svg_chart(&s.export_native("docx").unwrap()).contains("Fri / Round trips: 30"));
        let deck = s.export_native("pptx").unwrap();
        assert!(xml_part(&deck, "ppt/charts/chart1.xml").contains("<c:v>30</c:v>"));
    }
    #[test]
    fn table_and_chart_can_come_from_different_sheets() {
        let mut s = FlowSession::default();
        let inspection = s
            .open_native(
                include_bytes!("../../../examples/real/milleleaf-charts.gridcraft.xlsx"),
                "GridCraft",
                AT,
            )
            .unwrap();
        let sheets = inspection["sheets"].as_array().unwrap();
        let table_sheet = sheets.iter().find(|v| v["name"] == "Round trips").unwrap();
        let chart_sheet = sheets.iter().find(|v| v["name"] == "Formats").unwrap();
        let indexes: Vec<i64> = sheets
            .iter()
            .flat_map(|sh| sh["charts"].as_array().unwrap())
            .map(|c| c["index"].as_i64().unwrap())
            .collect();
        assert_eq!(indexes, (0..indexes.len() as i64).collect::<Vec<_>>());
        let index = chart_sheet["charts"][0]["index"].as_i64().unwrap() as i32;
        let r = s
            .create_native(
                table_sheet["path"].as_str().unwrap(),
                "A2:A4",
                index,
                "Sam",
                AT,
            )
            .unwrap();
        assert_eq!(r["report"]["table"]["columns"].as_array().unwrap().len(), 1);
        assert_eq!(r["report"]["table"]["rows"][0][0], "Mon");
        assert_eq!(
            r["deck"]["chart"]["series"][0]["values"],
            json!([40.0, 35.0, 25.0])
        );
        assert_eq!(
            s.compose(DC).unwrap().attributes["deck::chart"]["flowChartOwner"],
            "Formats"
        );
        let rendered = s.rendered(REPORT).unwrap();
        let picture = &rendered
            .children
            .iter()
            .find(|v| v.path == RC)
            .unwrap()
            .attributes["word::paragraph"]["runs"][0]["picture"];
        assert_eq!(picture["w"], 460);
        assert_eq!(
            picture["h"], 196.65,
            "single-pie image must keep its 800:342 intrinsic aspect ratio"
        );
        let table = r["report"]["table"].clone();
        let workbook = s.export_native("xlsx").unwrap();
        let edited = rewrite(
            &workbook,
            "xl/worksheets/sheet2.xml",
            "<v>40</v>",
            "<v>45</v>",
        );
        let r = s
            .import_native(&edited, "formats-edited.xlsx", "GridCraft", AT)
            .unwrap();
        assert_eq!(r["report"]["table"], table);
        assert_eq!(r["deck"]["chart"]["series"][0]["values"][0], 45.0);
        let report = s.export_native("docx").unwrap();
        assert!(xml_part(&report, "word/document.xml").contains("Mon"));
        assert!(svg_chart(&report).contains("45"));
        assert!(svg_chart(&report).contains("data-chart-kind=\"pie\""));
        let deck = s.export_native("pptx").unwrap();
        let chart = xml_part(&deck, "ppt/charts/chart1.xml");
        assert!(chart.contains("<c:pieChart"));
        assert!(chart.contains("<c:v>45</c:v>"));

        // Relative references belong to the chart's own sheet, never the table sheet.
        let owner = s.compose(chart_sheet["path"].as_str().unwrap()).unwrap();
        let mut look = owner.attributes["grid::sheet"].clone();
        look["charts"][0]["series"][0]["values"] = json!("$B$2:$B$4");
        look["charts"][0]["series"][0]["categories"] = json!("$A$2:$A$4");
        s.push(
            Layer {
                header: header("proposal", "Chart editor", AT),
                imports: vec![],
                schemas: Default::default(),
                data: vec![node(&owner.path, json!({"grid::sheet":look}), &[])],
            },
            "Project",
            vec![],
            vec![],
        );
        let (data, _) = s.chart_source().unwrap();
        assert_eq!(data["rows"][0][1], 45);
    }
    #[test]
    fn selected_line_report_keeps_line_shape_when_source_data_changes() {
        let mut s = FlowSession::default();
        let i = s
            .open_native(
                include_bytes!("../../../examples/real/milleleaf-charts.gridcraft.xlsx"),
                "GridCraft",
                AT,
            )
            .unwrap();
        let sheet = i["sheets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["name"] == "Round trips")
            .unwrap();
        let line = sheet["charts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| {
                v["kind"]
                    .as_str()
                    .is_some_and(|v| v == "Line" || v == "LineMarkers")
            })
            .unwrap();
        s.create_native(
            sheet["path"].as_str().unwrap(),
            "A2:B4",
            line["index"].as_i64().unwrap() as i32,
            "Sam",
            AT,
        )
        .unwrap();
        let original = s.export_native("xlsx").unwrap();
        let report = s.export_native("docx").unwrap();
        let svg = svg_chart(&report);
        assert!(svg.contains("data-chart-kind=\"line\""));
        assert!(svg.contains("Fri / Round trips: 27"));
        assert!(svg.contains("stroke-width=\"2\""));
        let edited = rewrite(
            &original,
            "xl/worksheets/sheet1.xml",
            "<v>27</v>",
            "<v>31</v>",
        );
        s.import_native(&edited, "line-source-edited.xlsx", "GridCraft", AT)
            .unwrap();
        let svg = svg_chart(&s.export_native("docx").unwrap());
        assert!(svg.contains("data-chart-kind=\"line\""));
        assert!(svg.contains("Fri / Round trips: 31"));
        assert!(!svg.contains("Fri / Round trips: 27"));
    }
    #[test]
    fn formula_totals_recalculate_and_varied_ranges_use_the_selected_cells() {
        let mut s = FlowSession::default();
        let inspection = s.sample_native("Sam", AT).unwrap();
        let path = inspection["sheets"][0]["path"].as_str().unwrap();
        for (range, first) in [("A1:B3", 412000), ("A2:B4", 541500)] {
            let r = s.create_native(path, range, -1, "Sam", AT).unwrap();
            assert_eq!(r["report"]["table"]["rows"][0][1], first);
            assert_eq!(r["report"]["table"]["rows"].as_array().unwrap().len(), 2);
        }
        s.create_native(path, "A1:B5", -1, "Sam", AT).unwrap();
        let bytes = s.export_native("xlsx").unwrap();
        let edited = rewrite(&bytes, "xl/worksheets/sheet1.xml", "541500", "550000");
        let r = s
            .import_native(&edited, "excel-edited.xlsx", "Excel", AT)
            .unwrap();
        assert_eq!(r["report"]["table"]["rows"][3][1], 1259000);
        assert_eq!(r["report"]["table"], r["deck"]["table"]);
    }
    #[test]
    fn ods_reads_directly_with_data_only_limitations() {
        let mut s = FlowSession::default();
        let inspection = s
            .open_native(
                include_bytes!(
                    "../../../examples/hello-table/fixtures/finance.libreoffice-edited.ods"
                ),
                "Calc",
                AT,
            )
            .unwrap();
        assert!(inspection["warnings"].as_array().unwrap().iter().any(|v| {
            v.as_str()
                .is_some_and(|v| v.contains("cached formula values"))
        }));
        let sheet = &inspection["sheets"][0];
        let r = s
            .create_native(sheet["path"].as_str().unwrap(), "A1:B5", -1, "Sam", AT)
            .unwrap();
        assert!(
            r["report"]["table"]["rows"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row[0] == "Perth" && row[1] == 120000)
        );
        for kind in ["docx", "pptx"] {
            assert!(!s.export_native(kind).unwrap().is_empty());
        }
    }
    #[test]
    fn source_history_retains_detected_app_and_original_file_evidence() {
        let bytes =
            include_bytes!("../../../examples/hello-table/fixtures/finance.msoffice-resaved.xlsx");
        let expected_app = milleleaf_formats::saved_by(bytes)
            .expect("real Excel fixture has its application metadata");
        let mut s = FlowSession::default();
        let inspection = s.open_native(bytes, "Sam", AT).unwrap();
        let hash = milleleaf_core::integrity_of(bytes);
        assert_eq!(inspection["source_app"], expected_app);
        assert_eq!(inspection["source_hash"], hash);
        assert_eq!(s.history[0]["app"], expected_app);
        assert_eq!(s.history[0]["source_hash"], hash);
        assert_eq!(s.returned_files[&hash].1.as_slice(), bytes.as_slice());
        let evidence = s.layers[0]
            .data
            .iter()
            .find(|n| n.path == "flow-source-file-evidence")
            .unwrap();
        assert_eq!(evidence.attributes["flow::file"]["blob"], hash);
        let mut sample = FlowSession::default();
        let inspection = sample.sample_native("Sam", AT).unwrap();
        assert_eq!(sample.history[0]["file"], "finance.xlsx");
        assert_eq!(inspection["sheets"][0]["range"], "A1:B4");
        assert_eq!(inspection["sheets"][0]["full_range"], "A1:B5");
        assert_eq!(
            inspection["sheets"][0]["rows"][4][1], 1250500,
            "suggesting the table must retain the workbook total in its preview"
        );
    }
    #[test]
    fn export_mixed_excel_source_for_writer_and_impress() {
        let mut s = excel_ready();
        let dir = out().join("mixed-excel-source");
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(s.table().unwrap()["rows"][1][1], 541500);
        for (kind, name) in [
            ("xlsx", "workbook.xlsx"),
            ("docx", "report.docx"),
            ("pptx", "deck.pptx"),
        ] {
            std::fs::write(dir.join(name), s.export_native(kind).unwrap()).unwrap();
        }
    }
    #[test]
    #[ignore = "Run mixed-check.sh prepare-excel-flow on the box first"]
    fn mixed_excel_source_writer_edits_and_impress_resave() {
        let mut s = excel_ready();
        for kind in ["xlsx", "docx", "pptx"] {
            s.export_native(kind).unwrap();
        }
        let dir = out().join("mixed-excel-source/checked");
        let r = s
            .import_native(
                &std::fs::read(dir.join("writer-edited.docx")).unwrap(),
                "writer-edited.docx",
                "Writer",
                AT,
            )
            .unwrap();
        assert!(
            r["changes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c.to_string().contains("Reviewed in LibreOffice Writer.")),
            "{r}"
        );
        assert!(r["conflicts"].as_array().unwrap().is_empty(), "{r}");
        let r = s
            .import_native(
                &std::fs::read(dir.join("writer-linked-edited.docx")).unwrap(),
                "writer-linked-edited.docx",
                "Writer",
                AT,
            )
            .unwrap();
        assert!(!r["conflicts"].as_array().unwrap().is_empty(), "{r}");
        assert_eq!(r["report"]["table"]["rows"][1][1], 541500);
        let r = s
            .import_native(
                &std::fs::read(dir.join("impress-resaved.pptx")).unwrap(),
                "impress-resaved.pptx",
                "Impress",
                AT,
            )
            .unwrap();
        assert_eq!(r["changes"], json!([]));
        assert_eq!(r["conflicts"], json!([]));
        let report = s.export_native("docx").unwrap();
        assert!(xml_part(&report, "word/document.xml").contains("Reviewed in LibreOffice Writer."));
        let deck = s.export_native("pptx").unwrap();
        assert!(xml_part(&deck, "ppt/charts/chart1.xml").contains("541500"));
    }
    #[test]
    #[ignore = "Run prepare-flow and the confined GridCraft save on the box first"]
    fn mixed_calc_wordcraft_impress_and_gridcraft_routes() {
        let dir = out().join("checked");
        let mut s = ready();
        for kind in ["xlsx", "docx", "pptx"] {
            s.export_native(kind).unwrap();
        }
        let r = s
            .import_native(
                &std::fs::read(dir.join("calc-resaved.xlsx")).unwrap(),
                "calc-resaved.xlsx",
                "Calc",
                AT,
            )
            .unwrap();
        assert_eq!(r["changes"], json!([]));
        assert_eq!(r["conflicts"], json!([]));
        let r = s
            .import_native(
                &std::fs::read(dir.join("calc-edited.xlsx")).unwrap(),
                "calc-edited.xlsx",
                "Calc",
                AT,
            )
            .unwrap();
        assert_eq!(r["report"]["table"]["rows"][1][1], 600000);
        let report = s.export_native("docx").unwrap();
        let library_saved =
            milleleaf_wordcraft::to_docx(&milleleaf_wordcraft::from_docx(&report).unwrap())
                .unwrap();
        let r = s
            .import_native(
                &library_saved,
                "wordcraft-resaved.docx",
                "WordCraft library",
                AT,
            )
            .unwrap();
        assert_eq!(r["changes"], json!([]));
        assert_eq!(r["conflicts"], json!([]));
        let r = s
            .import_native(
                &std::fs::read(dir.join("impress-resaved.pptx")).unwrap(),
                "impress-resaved.pptx",
                "Impress",
                AT,
            )
            .unwrap();
        assert_eq!(r["changes"], json!([]));
        assert_eq!(r["conflicts"], json!([]));
        assert_eq!(r["deck"]["table"]["rows"][1][1], 600000);
        assert!(
            xml_part(&s.export_native("docx").unwrap(), "word/document.xml").contains("600,000")
        );
        assert!(
            xml_part(&s.export_native("pptx").unwrap(), "ppt/charts/chart1.xml").contains("600000")
        );
        let after_calc = out().join("after-calc");
        std::fs::create_dir_all(&after_calc).unwrap();
        std::fs::write(
            after_calc.join("report.docx"),
            s.export_native("docx").unwrap(),
        )
        .unwrap();
        std::fs::write(
            after_calc.join("deck.pptx"),
            s.export_native("pptx").unwrap(),
        )
        .unwrap();

        // A separate mixed project starts from the GridCraft-saved workbook and uses the same outputs.
        let mut s = ready();
        for kind in ["xlsx", "docx", "pptx"] {
            s.export_native(kind).unwrap();
        }
        let r = s
            .import_native(
                &std::fs::read(dir.join("gridcraft-resaved.xlsx")).unwrap(),
                "gridcraft-resaved.xlsx",
                "GridCraft",
                AT,
            )
            .unwrap();
        assert_eq!(r["changes"], json!([]));
        assert_eq!(r["conflicts"], json!([]));
        let r = s
            .import_native(
                &std::fs::read(dir.join("gridcraft-edited.xlsx")).unwrap(),
                "gridcraft-edited.xlsx",
                "GridCraft",
                AT,
            )
            .unwrap();
        assert_eq!(r["report"]["table"]["rows"][1][1], 600000);
        assert_eq!(r["report"]["table"], r["deck"]["table"]);
        assert!(
            xml_part(&s.export_native("docx").unwrap(), "word/document.xml").contains("600,000")
        );
        assert!(
            xml_part(&s.export_native("pptx").unwrap(), "ppt/charts/chart1.xml").contains("600000")
        );
    }
    #[test]
    #[ignore = "Run the mixed Calc route, then actual WordCraft and Impress saves of _out/flow/after-calc first"]
    fn actual_after_calc_wordcraft_and_impress_resaves_return_to_the_same_project() {
        let checked = out().join("after-calc/checked");
        if !checked.join("wordcraft-resaved.docx").exists()
            || !checked.join("impress-resaved.pptx").exists()
        {
            println!(
                "PENDING after-Calc application saves; run prepare-flow-final and flow-final first"
            );
            return;
        }
        let dir = out().join("checked");
        let mut s = ready();
        for kind in ["xlsx", "docx", "pptx"] {
            s.export_native(kind).unwrap();
        }
        for (name, app) in [("calc-resaved.xlsx", "Calc"), ("calc-edited.xlsx", "Calc")] {
            s.import_native(&std::fs::read(dir.join(name)).unwrap(), name, app, AT)
                .unwrap();
        }
        let report = s.export_native("docx").unwrap();
        let saved = milleleaf_wordcraft::to_docx(&milleleaf_wordcraft::from_docx(&report).unwrap())
            .unwrap();
        s.import_native(&saved, "wordcraft-resaved.docx", "WordCraft library", AT)
            .unwrap();
        s.import_native(
            &std::fs::read(dir.join("impress-resaved.pptx")).unwrap(),
            "impress-resaved.pptx",
            "Impress",
            AT,
        )
        .unwrap();
        s.export_native("docx").unwrap();
        s.export_native("pptx").unwrap();
        for (file, app) in [
            ("wordcraft-resaved.docx", "WordCraft CLI"),
            ("impress-resaved.pptx", "Impress"),
        ] {
            let r = s
                .import_native(&std::fs::read(checked.join(file)).unwrap(), file, app, AT)
                .unwrap();
            assert_eq!(r["changes"], json!([]), "{file}: {r}");
            assert_eq!(r["conflicts"], json!([]), "{file}: {r}");
            assert_eq!(r["report"]["table"]["rows"][1][1], 600000);
            assert_eq!(r["deck"]["table"]["rows"][1][1], 600000);
            println!(
                "CHECKED updated Calc source -> {app} {file} -> same project: no change, linked value600000 retained"
            );
        }
    }
    #[test]
    fn optional_signature_verifies_and_detects_a_changed_layer() {
        let mut s = ready();
        assert!(s.key.is_none());
        let proof = s.sign_native().unwrap();
        let signatures: Vec<milleleaf_store::Signature> =
            serde_json::from_value(proof["signatures"].clone()).unwrap();
        let keys: Vec<(String, String)> =
            serde_json::from_value(proof["public_keys"].clone()).unwrap();
        let hash = proof["hash"].as_str().unwrap();
        for sig in &signatures {
            let check = milleleaf_store::check_signature(sig, hash, &keys);
            assert!(check.valid && check.known_as.is_some());
            assert!(!milleleaf_store::check_signature(sig, "changed layer hash", &keys).valid);
        }
        assert!(signatures.len() >= 2);
        assert!(s.key.is_some());
        let project: Value = serde_json::from_str(&s.project()).unwrap();
        assert!(project["selection"].is_array());
        assert!(project["pictures"].is_object());
        assert!(
            s.compose(SOURCE)
                .unwrap()
                .attributes
                .contains_key("flow::selection")
        );
        assert_eq!(
            s.compose(RC).unwrap().attributes["flow::render"]["source"],
            CHART_SOURCE
        );
    }
    #[test]
    fn range_validation_respects_excel_bounds() {
        assert!(bounds("A1:XFD1048576").is_err()); // Selection size cap.
        assert!(cell("XFD1048576").is_some());
        for bad in ["XFE1", "A1048577", "A0", "18446744073709551615"] {
            assert!(cell(bad).is_none(), "{bad}");
        }
        let mut s = FlowSession::default();
        let i = s.sample_native("Sam", AT).unwrap();
        let sh = i["sheets"][0]["path"].as_str().unwrap();
        for bad in ["A1", "A1:B1", "A1:A4"] {
            assert!(s.create_native(sh, bad, -1, "Sam", AT).is_err(), "{bad}");
        }
    }
    #[test]
    fn unknown_returned_revision_is_reported_without_applying() {
        let mut s = ready();
        let bytes = s.export_native("docx").unwrap();
        let head = s.layers.len();
        s.revisions.clear();
        assert!(
            s.import_native(&bytes, "report.docx", "Reviewer", AT)
                .unwrap_err()
                .contains("revision")
        );
        assert_eq!(s.layers.len(), head);
    }
    #[test]
    fn original_workbook_return_cannot_revert_a_newer_cell() {
        let mut s = FlowSession::default();
        s.sample_native("Sam", AT).unwrap();
        let original = s.export_native("xlsx").unwrap();
        // Simulate an arbitrary initial workbook with its revision stamp removed.
        let rev = milleleaf_gridcraft::exported_revision(
            &milleleaf_gridcraft::from_xlsx(&original).unwrap(),
        )
        .unwrap();
        let original = rewrite(
            &original,
            "xl/workbook.xml",
            &rev,
            "unrecorded-initial-revision",
        );
        let mut s = FlowSession::default();
        let i = s.open_native(&original, "Sam", AT).unwrap();
        s.create_native(
            i["sheets"][0]["path"].as_str().unwrap(),
            "A1:B4",
            -1,
            "Sam",
            AT,
        )
        .unwrap();
        let edited = rewrite(&original, "xl/worksheets/sheet1.xml", "541500", "550000");
        let r = s
            .import_native(&edited, "original-edited.xlsx", "Editor", AT)
            .unwrap();
        assert_eq!(r["report"]["table"]["rows"][1][1], 550000);
        let r = s
            .import_native(&original, "original.xlsx", "Editor", AT)
            .unwrap();
        assert_eq!(r["changes"], json!([]));
        assert_eq!(r["report"]["table"]["rows"][1][1], 550000);
        let conflicting = rewrite(&original, "xl/worksheets/sheet1.xml", "541500", "999");
        let r = s
            .import_native(&conflicting, "original-conflicting.xlsx", "Editor", AT)
            .unwrap();
        assert!(!r["conflicts"].as_array().unwrap().is_empty());
        assert_eq!(r["report"]["table"]["rows"][1][1], 550000);
    }
    #[test]
    fn repeated_own_edits_add_history_without_changes_or_conflicts() {
        let mut s = ready();
        let original = s.export_native("xlsx").unwrap();
        let edited = rewrite(&original, "xl/worksheets/sheet1.xml", "541500", "550000");
        s.import_native(&edited, "edited.xlsx", "Editor", AT)
            .unwrap();
        let history = s.history.len();
        let r = s
            .import_native(&edited, "edited.xlsx", "Editor", AT)
            .unwrap();
        assert_eq!(r["changes"], json!([]));
        assert_eq!(r["conflicts"], json!([]));
        assert_eq!(s.history.len(), history + 1);
        let report = s.export_native("docx").unwrap();
        let edited = rewrite(
            &report,
            "word/document.xml",
            "Workbook report",
            "Revised report",
        );
        s.import_native(&edited, "edited.docx", "Editor", AT)
            .unwrap();
        let history = s.history.len();
        let r = s
            .import_native(&edited, "edited.docx", "Editor", AT)
            .unwrap();
        assert_eq!(r["changes"], json!([]));
        assert_eq!(r["conflicts"], json!([]));
        assert_eq!(s.history.len(), history + 1);
        assert!(
            xml_part(&s.export_native("docx").unwrap(), "word/document.xml")
                .contains("Revised report")
        );
    }
    #[test]
    fn recreated_project_accepts_its_canonical_report_without_a_download_click() {
        let mut first = ready();
        let report = first.export_native("docx").unwrap();
        let edited = rewrite(
            &report,
            "word/document.xml",
            "Workbook report",
            "Returned report edit",
        );
        let mut recreated = ready();
        let r = recreated
            .import_native(&edited, "report-edited.docx", "Reviewer", AT)
            .unwrap();
        assert!(
            r["changes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c.to_string().contains("Returned report edit")),
            "{r}"
        );
        assert_eq!(r["conflicts"], json!([]));
        assert!(
            xml_part(
                &recreated.export_native("docx").unwrap(),
                "word/document.xml"
            )
            .contains("Returned report edit")
        );
    }
    #[test]
    fn stale_report_removal_preserves_a_newer_paragraph_and_order() {
        let mut s = ready();
        let original = s.export_native("docx").unwrap();
        let xml = xml_part(&original, "word/document.xml");
        let at = xml.find("mleaf:flow-report-title").unwrap();
        let start = xml[..at].rfind("<w:sdt>").unwrap();
        let end = at + xml[at..].find("</w:sdt>").unwrap() + "</w:sdt>".len();
        let removed = rewrite(&original, "word/document.xml", &xml[start..end], "");
        let mut parent = node(REPORT, json!({}), &[("new", "flow-new-paragraph")]);
        parent.order = Some(vec![
            "title".into(),
            "table".into(),
            "chart".into(),
            "new".into(),
        ]);
        s.push(
            Layer {
                header: header("proposal", "Newer editor", AT),
                imports: vec![],
                schemas: Default::default(),
                data: vec![
                    parent,
                    node(
                        "flow-new-paragraph",
                        json!({"word::paragraph":{"runs":[{"text":"Newer paragraph remains."}]}}),
                        &[],
                    ),
                ],
            },
            "Project",
            vec![],
            vec![],
        );
        let r = s
            .import_native(&removed, "stale-removed.docx", "Older editor", AT)
            .unwrap();
        assert!(!r["conflicts"].as_array().unwrap().is_empty(), "{r}");
        let current = s.compose(REPORT).unwrap();
        assert!(
            current
                .children
                .iter()
                .any(|c| c.path == "flow-new-paragraph")
        );
        assert!(
            xml_part(&s.export_native("docx").unwrap(), "word/document.xml")
                .contains("Newer paragraph remains.")
        );
        // Child replacement and reordered parents are both structural overlaps.
        let mut changed = current.clone();
        changed.children.reverse();
        let order = node(REPORT, json!({}), &[("title", "different-title")]);
        assert!(overlap(&order, Some(&current), Some(&changed)));
        let mut changed = current.clone();
        let title = changed
            .children
            .iter_mut()
            .find(|c| c.name.as_deref() == Some("title"))
            .unwrap();
        title.attributes.insert(
            "word::paragraph".into(),
            json!({"runs":[{"text":"Newer title"}]}),
        );
        let mut replacement = node(REPORT, json!({}), &[("title", "replacement-title")]);
        replacement.order = None;
        assert!(
            overlap(&replacement, Some(&current), Some(&changed)),
            "replacement must preserve newer child content even with unchanged parent order"
        );
    }
    #[test]
    fn ods_return_keeps_original_archive_evidence_and_existing_chart_refs() {
        let mut s = FlowSession::default();
        let i = s
            .open_native(
                include_bytes!("../../../examples/real/milleleaf-charts.gridcraft.xlsx"),
                "GridCraft",
                AT,
            )
            .unwrap();
        let sheet = i["sheets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["name"] == "Round trips")
            .unwrap();
        s.create_native(sheet["path"].as_str().unwrap(), "A2:B7", 0, "Sam", AT)
            .unwrap();
        let original = include_bytes!("../../../examples/real/milleleaf-charts.calc.ods");
        let r = s
            .import_native(original, "calc-data.ods", "Calc", AT)
            .unwrap();
        assert!(!r["deck"]["chart"]["series"].as_array().unwrap().is_empty());
        assert!(r["warnings"].as_array().unwrap().iter().any(|v| {
            v.as_str()
                .is_some_and(|v| v.contains("Existing project chart references"))
        }));
        let hash = milleleaf_core::integrity_of(original);
        let stored = &s.returned_files[&hash];
        assert_eq!(stored.0, "application/vnd.oasis.opendocument.spreadsheet");
        assert_eq!(stored.1, original);
        let last = s.layers.last().unwrap();
        let evidence = last
            .data
            .iter()
            .find(|n| n.path == "flow-file-evidence")
            .unwrap();
        assert_eq!(evidence.attributes["flow::file"]["hash"], hash);
        assert_eq!(evidence.attributes["flow::file"]["name"], "calc-data.ods");
    }
    #[test]
    fn sample_workbook_exports_and_library_roundtrips() {
        let mut s = ready();
        assert_eq!(s.table().unwrap()["rows"][1][1], 541500);
        std::fs::create_dir_all(out()).unwrap();
        for (kind, name) in [
            ("xlsx", "finance.xlsx"),
            ("docx", "report.docx"),
            ("pptx", "deck.pptx"),
        ] {
            let bytes = s.export_native(kind).unwrap();
            assert!(bytes.len() > 500);
            std::fs::write(out().join(name), &bytes).unwrap();
            if kind == "docx" {
                let xml = xml_part(&bytes, "word/document.xml");
                assert!(xml.contains("<w:tbl"));
                assert!(xml.contains("541,500"));
                assert!(xml.contains("<w:drawing"));
                assert!(svg_chart(&bytes).contains("541500"));
                assert!(svg_chart(&bytes).contains("data-chart-kind=\"column\""));
            } else if kind == "pptx" {
                assert!(xml_part(&bytes, "ppt/slides/slide1.xml").contains("541,500"));
                let chart = xml_part(&bytes, "ppt/charts/chart1.xml");
                assert!(chart.contains("541500"));
                assert!(chart.contains("Hong Kong"));
            }
            let r = s.import_native(&bytes, name, "Library", AT).unwrap();
            assert_eq!(r["changes"], json!([]), "{kind}: {r}");
            assert_eq!(r["conflicts"], json!([]), "{kind}: {r}");
        }
    }
    #[test]
    fn changed_cell_updates_both_files_and_linked_overwrite_conflicts() {
        let mut s = ready();
        let x = s.export_native("xlsx").unwrap();
        let d = s.export_native("docx").unwrap();
        let p = s.export_native("pptx").unwrap();
        let x = rewrite(&x, "xl/worksheets/sheet1.xml", "541500", "550000");
        let own = rewrite(
            &d,
            "word/document.xml",
            "Workbook report",
            "Workbook report revised",
        );
        let conflict = rewrite(&d, "word/document.xml", "541,500", "999");
        std::fs::create_dir_all(out()).unwrap();
        for (name, b) in [
            ("finance-edited.xlsx", &x),
            ("report-edited.docx", &own),
            ("report-conflict.docx", &conflict),
        ] {
            std::fs::write(out().join(name), b).unwrap();
        }
        let r = s
            .import_native(&x, "finance-edited.xlsx", "Analyst", AT)
            .unwrap();
        assert_eq!(r["report"]["table"]["rows"][1][1], 550000);
        assert_eq!(r["deck"]["table"]["rows"][1][1], 550000);
        assert_eq!(r["deck"]["chart"]["series"][0]["values"][1], 550000.0);
        let updated_report = s.export_native("docx").unwrap();
        let report_xml = xml_part(&updated_report, "word/document.xml");
        assert!(report_xml.contains("550,000"));
        assert!(!report_xml.contains("541,500"));
        let picture = svg_chart(&updated_report);
        assert!(picture.contains("550000"));
        assert!(!picture.contains("541500"));
        let updated_deck = s.export_native("pptx").unwrap();
        let slide_xml = xml_part(&updated_deck, "ppt/slides/slide1.xml");
        assert!(slide_xml.contains("550,000"));
        assert!(!slide_xml.contains("541,500"));
        let native_chart = xml_part(&updated_deck, "ppt/charts/chart1.xml");
        assert!(native_chart.contains("550000"));
        assert!(!native_chart.contains("541500"));
        let r = s
            .import_native(&conflict, "report-conflict.docx", "Reviewer", AT)
            .unwrap();
        assert!(!r["conflicts"].as_array().unwrap().is_empty());
        assert_eq!(r["report"]["table"]["rows"][1][1], 550000);
        let hash = milleleaf_core::integrity_of(&conflict);
        let project: Value = serde_json::from_str(&s.project()).unwrap();
        let retained: Vec<u8> =
            serde_json::from_value(project["returned_files"][&hash][1].clone()).unwrap();
        assert_eq!(milleleaf_core::integrity_of(&retained), hash);
        assert_eq!(retained, conflict);
        assert!(xml_part(&retained, "word/document.xml").contains("999"));
        assert_eq!(r["conflicts"][0]["returned_file"]["blob"], hash);
        let table_conflict = r["conflicts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["path"] == RT)
            .unwrap();
        assert_eq!(table_conflict["place"], "Report table");
        assert_eq!(table_conflict["file"], "report-conflict.docx");
        let cell = table_conflict["cell_changes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["proposed"].as_f64() == Some(999.0))
            .unwrap();
        assert_eq!(cell["row"], 3);
        assert_eq!(cell["column"], 2);
        assert_eq!(cell["column_name"], "Revenue");
        assert_eq!(cell["row_label"], "Hong Kong");
        assert_eq!(cell["before"].as_f64(), Some(541500.0));
        assert_eq!(cell["source_current"].as_f64(), Some(550000.0));
        let proof = s.sign_native().unwrap();
        let signed: Layer = serde_json::from_value(proof["layer"].clone()).unwrap();
        assert_eq!(
            proof["hash"],
            milleleaf_store::hash_of(&milleleaf_store::layer_bytes(&signed))
        );
        let evidence = signed
            .data
            .iter()
            .find(|n| n.path == "flow-file-evidence")
            .unwrap();
        assert!(
            evidence.attributes["flow::conflicts"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|c| c["cell_changes"].as_array().into_iter().flatten())
                .any(|c| c["proposed"].as_f64() == Some(999.0)),
            "the signed conflict evidence must include the exact proposal"
        );
        let r = s
            .import_native(&own, "report-edited.docx", "Reviewer", AT)
            .unwrap();
        assert!(!r["changes"].as_array().unwrap().is_empty(), "{r}");
        // Returning an old, untouched deck cannot roll back the newer workbook values.
        let r = s.import_native(&p, "deck.pptx", "Reviewer", AT).unwrap();
        assert_eq!(r["changes"], json!([]));
        assert_eq!(r["deck"]["table"]["rows"][1][1], 550000);
    }
    #[test]
    fn slide_table_conflict_has_a_readable_cell_proposal() {
        let mut s = ready();
        let original = s.export_native("pptx").unwrap();
        let edited = rewrite(&original, "ppt/slides/slide1.xml", "541,500", "999");
        let r = s
            .import_native(&edited, "deck-conflict.pptx", "Reviewer", AT)
            .unwrap();
        let conflict = r["conflicts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["path"] == DT)
            .unwrap();
        assert_eq!(conflict["place"], "Slide table");
        let cells = conflict["cell_changes"].as_array().unwrap();
        assert_eq!(cells.len(), 1, "{conflict}");
        assert_eq!(cells[0]["row"], 3);
        assert_eq!(cells[0]["column"], 2);
        assert_eq!(cells[0]["before"].as_f64(), Some(541500.0));
        assert_eq!(cells[0]["proposed"].as_f64(), Some(999.0));
        assert_eq!(cells[0]["source_current"].as_f64(), Some(541500.0));
    }
    #[test]
    #[ignore = "Diagnoses actual Writer image changes using existing checked fixtures; no fixture regeneration"]
    fn diagnose_writer_report_chart_picture() {
        let mut s = ready();
        let before = s.rendered(REPORT).unwrap();
        let bytes = std::fs::read(out().join("checked/writer-resaved.docx")).unwrap();
        let document = milleleaf_wordcraft::from_docx(&bytes).unwrap();
        let (edit, native) =
            milleleaf_wordcraft::import(&before, &document, header("diagnostic", "Reader", AT))
                .unwrap();
        let adopted = super::super::read(&bytes, header("diagnostic", "Reader", AT)).unwrap();
        let returned = super::super::tree(&adopted.layer).unwrap();
        let picture_info = |pictures: &Pictures| {
            pictures
                .iter()
                .map(|(hash, (mime, bytes))| {
                    let png = bytes.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10]);
                    let dimensions = if png && bytes.len() >= 24 {
                        Some((
                            u32::from_be_bytes(bytes[16..20].try_into().unwrap()),
                            u32::from_be_bytes(bytes[20..24].try_into().unwrap()),
                        ))
                    } else {
                        None
                    };
                    json!({"hash":hash,"mime":mime,"bytes":bytes.len(),"png_dimensions":dimensions})
                })
                .collect::<Vec<_>>()
        };
        let result = json!({
            "before":milleleaf_core::render::find(std::slice::from_ref(&before),RC).map(|n|&n.attributes),
            "returned":milleleaf_core::render::find(std::slice::from_ref(&returned),RC).map(|n|&n.attributes),
            "native_patch":edit.data.iter().find(|n|n.path==RC),
            "native_conflicts":native.conflicts,"native_warnings":native.warnings,
            "original_media":picture_info(&s.pictures),"returned_media":picture_info(&adopted.pictures),
            "returned_revision":milleleaf_wordcraft::exported_revision(&document),"current_revision":milleleaf_wordcraft::revision_of(&before)
        });
        std::fs::write(
            out().join("rc-picture-diagnostic.json"),
            serde_json::to_vec_pretty(&result).unwrap(),
        )
        .unwrap();
        println!("RC_PICTURE_DIAGNOSTIC {}", result);
    }
    #[test]
    #[ignore = "Needs real application outputs in _out/flow/checked; run the box application scripts first"]
    fn external_roundtrips() {
        let dir = out().join("checked");
        let mut checked = 0;
        for entry in std::fs::read_dir(&dir).expect("application outputs must exist") {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy();
            if !["xlsx", "docx", "pptx"]
                .contains(&path.extension().and_then(|s| s.to_str()).unwrap_or(""))
            {
                continue;
            }
            let mut s = ready();
            for kind in ["xlsx", "docx", "pptx"] {
                s.export_native(kind).unwrap();
            }
            let result = s
                .import_native(&std::fs::read(&path).unwrap(), &name, "Checked app", AT)
                .unwrap();
            std::fs::write(
                out().join(format!("{name}.json")),
                serde_json::to_vec_pretty(&result).unwrap(),
            )
            .unwrap();
            if name.contains("resaved") {
                assert_eq!(result["changes"], json!([]), "{name}: {result}");
                assert_eq!(result["conflicts"], json!([]), "{name}: {result}");
            } else if name.contains("edited") {
                assert!(
                    !result["changes"].as_array().unwrap().is_empty()
                        || !result["conflicts"].as_array().unwrap().is_empty(),
                    "{name}: {result}"
                );
                if path.extension().unwrap() == "xlsx" {
                    assert_ne!(result["report"]["table"]["rows"][1][1], 541500);
                    assert_eq!(result["report"]["table"], result["deck"]["table"]);
                }
            } else {
                panic!("Unlabelled application fixture: {name}");
            }
            checked += 1;
            println!(
                "CHECKED {name}: {} changes, {} conflicts",
                result["changes"].as_array().unwrap().len(),
                result["conflicts"].as_array().unwrap().len()
            );
        }
        assert!(
            checked >= 6,
            "need resaves and edits for workbook, report and deck"
        );
    }
    /// Microsoft Office 16 (Mac) saves, made by hand, of the Demo B exports, returned in
    /// OFFICE-STEPS.md order: re-saves, the workbook edit, the heading edits, then the linked edit.
    #[test]
    fn microsoft_office_saves_in_office_steps_order() {
        macro_rules! office {
            ($name:literal) => {
                (
                    $name,
                    include_bytes!(concat!("../../../examples/hello-table/fixtures/", $name))
                        .as_slice(),
                )
            };
        }
        let mut s = ready();
        for kind in ["xlsx", "docx", "pptx"] {
            s.export_native(kind).unwrap();
        }
        let mut back =
            |(name, bytes): (&str, &[u8])| s.import_native(bytes, name, "Office", AT).unwrap();
        for f in [
            office!("finance.msoffice-flow-resaved.xlsx"),
            office!("report.msoffice-flow-resaved.docx"),
            office!("deck.msoffice-flow-resaved.pptx"),
        ] {
            let r = back(f);
            assert_eq!(r["changes"], json!([]), "{}: {r}", f.0);
            assert_eq!(r["conflicts"], json!([]), "{}: {r}", f.0);
        }
        let r = back(office!("finance.msoffice-flow-edited.xlsx"));
        assert_eq!(r["report"]["table"]["rows"][1][1], 550000, "{r}");
        assert_eq!(r["report"]["table"], r["deck"]["table"]);
        for (f, heading) in [
            (
                office!("report.msoffice-flow-edited.docx"),
                "Workbook report reviewed",
            ),
            (
                office!("deck.msoffice-flow-edited.pptx"),
                "Workbook figures reviewed",
            ),
        ] {
            let r = back(f);
            assert!(r["changes"].to_string().contains(heading), "{r}");
            assert_eq!(r["conflicts"], json!([]), "{r}");
            assert_eq!(r["report"]["table"]["rows"][1][1], 550000, "{r}");
        }
        let r = back(office!("report.msoffice-flow-linked-edited.docx"));
        let cell = &r["conflicts"][0]["cell_changes"][0];
        assert_eq!(
            (&cell["before"], &cell["proposed"], &cell["source_current"]),
            (&json!(541500), &json!(999), &json!(550000)),
            "{r}"
        );
        assert_eq!(
            r["report"]["table"]["rows"][1][1], 550000,
            "source must not be overwritten"
        );
        let report = s.export_native("docx").unwrap();
        let text = xml_part(&report, "word/document.xml");
        assert!(
            text.contains("Workbook report reviewed"),
            "heading edit lost on export"
        );
        assert!(
            text.contains("550,000") || text.contains("550000"),
            "updated value missing from report"
        );
        assert!(
            xml_part(&s.export_native("pptx").unwrap(), "ppt/charts/chart1.xml").contains("550000")
        );
    }
}

#[wasm_bindgen]
impl FlowSession {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }
    pub fn sample(&mut self, author: &str, at: &str) -> std::result::Result<String, JsError> {
        self.sample_native(author, at)
            .map(|v| v.to_string())
            .map_err(|e| JsError::new(&e))
    }
    pub fn open(
        &mut self,
        bytes: &[u8],
        author: &str,
        at: &str,
    ) -> std::result::Result<String, JsError> {
        self.open_native(bytes, author, at)
            .map(|v| v.to_string())
            .map_err(|e| JsError::new(&e))
    }
    pub fn inspect(&self) -> std::result::Result<String, JsError> {
        self.inspect_native()
            .map(|v| v.to_string())
            .map_err(|e| JsError::new(&e))
    }
    pub fn create(
        &mut self,
        sheet: &str,
        range: &str,
        chart: i32,
        author: &str,
        at: &str,
    ) -> std::result::Result<String, JsError> {
        self.create_native(sheet, range, chart, author, at)
            .map(|v| v.to_string())
            .map_err(|e| JsError::new(&e))
    }
    pub fn import_file(
        &mut self,
        bytes: &[u8],
        filename: &str,
        author: &str,
        at: &str,
        claimed_app: Option<String>,
    ) -> std::result::Result<String, JsError> {
        let mut result = self
            .import_native(bytes, filename, author, at)
            .map_err(|e| JsError::new(&e))?;
        if let Some(app) = claimed_app.filter(|s| !s.trim().is_empty()) {
            if let Some(h) = self.history.last_mut() {
                h["claimed_app"] = json!(app);
            }
            result["history"] = json!(self.history);
        }
        Ok(result.to_string())
    }
    pub fn export_file(&mut self, kind: &str) -> std::result::Result<Vec<u8>, JsError> {
        self.export_native(kind).map_err(|e| JsError::new(&e))
    }
    pub fn project(&self) -> String {
        json!({"version":"milleleaf-flow/1","layers":self.layers,"history":self.history,"selection":self.selection,"pictures":self.pictures,"returned_files":self.returned_files,"privacy":"This project includes the workbook content and picture bytes. Keep it private unless you intend to share those files."}).to_string()
    }
    pub fn sign(&mut self) -> std::result::Result<String, JsError> {
        self.sign_native()
            .map(|v| v.to_string())
            .map_err(|e| JsError::new(&e))
    }
}
impl FlowSession {
    fn sign_native(&mut self) -> Result<Value> {
        if self.key.is_none() {
            self.key = Some(
                milleleaf_store::KeyPair::generate_demo("Session demo key")
                    .map_err(|e| e.to_string())?,
            );
        }
        let layer = self.layers.last().ok_or("Open a workbook first.")?;
        let key = self.key.as_ref().unwrap();
        let hash = milleleaf_store::hash_of(&milleleaf_store::layer_bytes(layer));
        let signatures = milleleaf_store::sign_hash(&hash, key, false, &layer.header.timestamp)
            .map_err(|e| e.to_string())?;
        Ok(
            json!({"version":"milleleaf-flow-proof/1","layer":layer,"hash":hash,"signatures":signatures,"public_keys":[[key.name.clone(),key.public()],[key.name.clone(),key.mldsa_public()]],"trust":"Session demo key. Author and time are claimed; no independent identity or timestamp authority."}),
        )
    }
}
