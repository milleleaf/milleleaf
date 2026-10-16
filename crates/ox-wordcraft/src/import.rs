//! Edited WordCraft `Document` → minimal OX edit layer.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use ox_core::Composed;
use ox_core::model::{Header, Layer, Node, OX_VERSION, stable_id};
use ox_core::render::format_value;
use serde::Serialize;
use serde_json::{Map, Value, json};
use wordcraft_doc::para::OBJ;
use wordcraft_doc::props::Underline;
use wordcraft_doc::{Block, CharProps, Document, InlineObject, Paragraph, Table, TextColor};

use crate::{CAPTION_SUFFIX, PROP_DOCUMENT, PROP_REVISION, TAG_PREFIX, WordError, revision_of};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Conflict {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ImportReport {
    /// Existing nodes whose own attributes changed.
    pub changed: Vec<String>,
    /// New nodes (paragraphs or tables typed into the file).
    pub added: Vec<String>,
    /// Child names removed from the document.
    pub removed: Vec<String>,
    /// Edits to live-linked content, which are not written back.
    pub conflicts: Vec<Conflict>,
    pub warnings: Vec<String>,
    /// The stack changed since this file was exported.
    pub stale: bool,
    /// How each paragraph and table in the file was recognised (all by content-control tag, so
    /// all certain).
    pub matches: Vec<ox_core::Match>,
}

impl ImportReport {
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.added.is_empty() && self.removed.is_empty()
    }
}

fn obj<'a>(n: &'a Composed, key: &str) -> Option<&'a Map<String, Value>> {
    n.attributes.get(key).and_then(Value::as_object)
}

/// The composed revision a file was exported at (`ox.revision`), if it carries one.
pub fn exported_revision(edited: &Document) -> Option<String> {
    custom_prop(edited, PROP_REVISION).map(String::from)
}

fn custom_prop<'a>(doc: &'a Document, name: &str) -> Option<&'a str> {
    doc.custom_props
        .iter()
        .find(|p| p.name == name)
        .map(|p| p.value.as_str())
}

fn hex(c: &TextColor) -> Option<String> {
    match c {
        TextColor::Rgb(rgb) => Some(format!("#{:02x}{:02x}{:02x}", rgb.0, rgb.1, rgb.2)),
        TextColor::Auto => None,
    }
}

/// Direct character formatting as run keys (the subset OX runs carry).
fn run_format(cp: &CharProps) -> Map<String, Value> {
    let mut m = Map::new();
    if cp.bold == Some(true) {
        m.insert("bold".into(), json!(true));
    }
    if cp.italic == Some(true) {
        m.insert("italic".into(), json!(true));
    }
    if cp.underline.is_some_and(|u| u != Underline::None) {
        m.insert("underline".into(), json!(true));
    }
    if let Some(c) = cp.color.as_ref().and_then(hex) {
        m.insert("color".into(), json!(c));
    }
    if let Some(f) = &cp.font {
        m.insert("font".into(), json!(f));
    }
    if let Some(s) = cp.size {
        m.insert("sizePt".into(), json!(s));
    }
    m
}

/// Canonical form for comparing runs: text runs merged when formatting matches, colours in
/// lower case, field runs reduced to their name.
fn canonical(runs: &[Value]) -> Vec<Value> {
    let mut out: Vec<Map<String, Value>> = Vec::new();
    for r in runs {
        let Some(r) = r.as_object() else { continue };
        if r.contains_key("fieldPath") {
            out.push(r.clone());
            continue;
        }
        if let Some(f) = r.get("field") {
            out.push(Map::from_iter([("field".to_string(), f.clone())]));
            continue;
        }
        let text = r.get("text").and_then(Value::as_str).unwrap_or("");
        if text.is_empty() {
            continue;
        }
        let mut fmt = r.clone();
        fmt.remove("text");
        if let Some(Value::String(c)) = fmt.get_mut("color") {
            *c = c.to_lowercase();
        }
        for k in ["bold", "italic", "underline"] {
            if fmt.get(k) == Some(&json!(false)) {
                fmt.remove(k);
            }
        }
        if let Some(last) = out.last_mut() {
            let mut last_fmt = last.clone();
            last_fmt.remove("text");
            if !last.contains_key("field") && last_fmt == fmt {
                let joined = format!(
                    "{}{}",
                    last.get("text").and_then(Value::as_str).unwrap_or(""),
                    text
                );
                last.insert("text".into(), json!(joined));
                continue;
            }
        }
        let mut m = fmt;
        m.insert("text".into(), json!(text));
        out.push(m);
    }
    out.into_iter().map(Value::Object).collect()
}

/// A second block with the same tag (pasted copy): the original's match becomes a best guess, and
/// the copy is imported as new content.
fn duplicate(report: &mut ImportReport, path: &str, what: &str) {
    let already = report
        .matches
        .iter()
        .any(|m| m.path == path && m.confidence == ox_core::matching::Confidence::BestGuess);
    if !already {
        report.warnings.push(format!(
            "{path}: this {what}'s content control tag appears more than once (pasted copy?): the first is taken as the original, the rest as new; best guess"
        ));
        report.matches.push(ox_core::Match::best_guess(path, format!("{what} tag appears more than once")));
    }
}

/// The tag of the block-level `ox:` control a paragraph starts with.
/// The `ox:` tags of the controls opened in a paragraph, in order.
fn ox_tags(p: &Paragraph) -> impl Iterator<Item = &str> {
    p.objects.iter().filter_map(|o| match o {
        InlineObject::ControlStart { control } => control.tag.strip_prefix(TAG_PREFIX),
        _ => None,
    })
}

/// The node a paragraph stands for: the first `ox:` control in it that isn't a linked field.
///
/// Word keeps our block-level controls around the paragraph; LibreOffice (24.2) turns them into
/// inline controls inside it. Either way the tag names the node, so the tag's target decides,
/// not the control's shape.
fn para_tag(p: &Paragraph, is_field: &dyn Fn(&str) -> bool) -> Option<String> {
    ox_tags(p).find(|t| !is_field(t)).map(str::to_string)
}

/// The node a table stands for: a control around it, or (after LibreOffice moved it) a control
/// at the start of its first cell.
fn table_tag(t: &Table, is_table: &dyn Fn(&str) -> bool) -> Option<String> {
    if let Some(tag) = t
        .controls
        .open
        .iter()
        .find_map(|c| c.tag.strip_prefix(TAG_PREFIX))
    {
        return Some(tag.to_string());
    }
    let first = t.rows.first()?.cells.first()?.blocks.first()?.as_para()?;
    ox_tags(first).find(|tag| is_table(tag)).map(str::to_string)
}

/// What a paragraph says: runs (with `ox:` inline controls as field runs, by field node path),
/// and the text each field shows.
struct Extracted {
    runs: Vec<Value>,
    /// (field node path, shown text), in order.
    fields: Vec<(String, String)>,
    text: String,
    dropped_objects: usize,
}

fn extract(p: &Paragraph, is_field: &dyn Fn(&str) -> bool) -> Extracted {
    let mut runs: Vec<Value> = Vec::new();
    let mut fields = Vec::new();
    let mut text = String::new();
    let mut dropped_objects = 0;
    // Open inline controls: Some(path) for an ox field, None for anything else.
    let mut open: Vec<Option<String>> = Vec::new();
    let mut field_text = String::new();
    for (off, ch) in p.text.char_indices() {
        let in_field = open.iter().any(Option::is_some);
        if ch == OBJ {
            match p.object_at(off) {
                Some(InlineObject::ControlStart { control }) => {
                    match control.tag.strip_prefix(TAG_PREFIX) {
                        // A linked field: its shown text is replaced by a field run.
                        Some(path) if !in_field && is_field(path) => {
                            runs.push(json!({ "fieldPath": path }));
                            open.push(Some(path.to_string()));
                            field_text.clear();
                        }
                        // Anything else (the paragraph's own identity control, a form field) is
                        // transparent: its content is ordinary text.
                        _ => open.push(None),
                    }
                }
                Some(InlineObject::ControlEnd) => {
                    if let Some(Some(path)) = open.pop() {
                        fields.push((path, field_text.trim().to_string()));
                    }
                }
                Some(o) if o.is_marker() => {}
                Some(_) => dropped_objects += 1,
                None => {}
            }
            continue;
        }
        if in_field {
            field_text.push(ch);
            continue;
        }
        text.push(ch);
        let mut fmt = run_format(p.props_of_char(off));
        fmt.insert("text".into(), json!(ch.to_string()));
        runs.push(Value::Object(fmt));
    }
    Extracted {
        runs: canonical(&runs),
        fields,
        text,
        dropped_objects,
    }
}

fn cell_text(c: &wordcraft_doc::Cell) -> String {
    c.blocks
        .iter()
        .filter_map(|b| b.as_para())
        .map(|p| {
            p.plain_text()
                .chars()
                .filter(|c| *c != OBJ)
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn table_grid(t: &Table) -> Vec<Vec<String>> {
    t.rows
        .iter()
        .map(|r| r.cells.iter().map(cell_text).collect())
        .collect()
}

/// The grid a composed `office::data::table` exports as: header names, then formatted rows.
fn expected_grid(node: &Composed) -> (Vec<Map<String, Value>>, Vec<Vec<String>>) {
    let data = obj(node, "office::data::table")
        .cloned()
        .unwrap_or_default();
    let cols: Vec<Map<String, Value>> = data
        .get("columns")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|c| c.as_object().cloned()).collect())
        .unwrap_or_default();
    let mut grid = vec![
        cols.iter()
            .map(|c| {
                c.get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string()
            })
            .collect::<Vec<_>>(),
    ];
    for row in data
        .get("rows")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let cells = row.as_array().cloned().unwrap_or_default();
        grid.push(
            (0..cols.len())
                .map(|i| {
                    format_value(
                        cells.get(i).unwrap_or(&Value::Null),
                        cols[i]
                            .get("type")
                            .and_then(Value::as_str)
                            .unwrap_or("text"),
                    )
                })
                .collect(),
        );
    }
    (cols, grid)
}

/// Parses a shown cell back to a value of the column's type.
fn parse_cell(text: &str, kind: &str) -> Option<Value> {
    if kind == "text" || kind == "date" {
        return Some(json!(text));
    }
    let cleaned: String = text
        .chars()
        .filter(|c| !matches!(c, '$' | ',' | ' ' | '%' | '\u{a0}'))
        .collect();
    let n: f64 = cleaned.parse().ok()?;
    let n = if kind == "percent" { n / 100.0 } else { n };
    Some(if n.fract() == 0.0 && n.abs() < 1e15 {
        json!(n as i64)
    } else {
        json!(n)
    })
}

fn guess_type(cells: &[&str]) -> &'static str {
    if cells.is_empty() || cells.iter().any(|c| parse_cell(c, "number").is_none()) {
        "text"
    } else if cells.iter().any(|c| c.contains('$')) {
        "currency"
    } else if cells.iter().all(|c| c.ends_with('%')) {
        "percent"
    } else {
        "number"
    }
}

fn is_linked(n: &Composed, key: &str) -> bool {
    n.provenance.get(key).is_some_and(|p| !p.via.is_empty())
}

/// Compares `edited` with the composed document `node` it was exported from and returns the
/// edit layer that would turn one into the other.
pub fn import(
    node: &Composed,
    edited: &Document,
    header: Header,
) -> Result<(Layer, ImportReport), WordError> {
    let found = custom_prop(edited, PROP_DOCUMENT).ok_or(WordError::NotExported)?;
    if found != node.path {
        return Err(WordError::WrongDocument {
            expected: node.path.clone(),
            found: found.to_string(),
        });
    }
    import_into(node, edited, header, false)
}

/// Brings a document Milleleaf didn't make into a project: it is imported into an empty
/// `word::document` node at `path`, so every paragraph and table becomes new content.
/// What the importer can't carry is in the report's warnings.
pub fn adopt(
    edited: &Document,
    path: &str,
    title: &str,
    header: Header,
) -> Result<(Layer, ImportReport), WordError> {
    let empty = Composed {
        path: path.to_string(),
        name: None,
        attributes: Map::from_iter([("word::document".into(), json!({ "title": title }))]),
        provenance: Default::default(),
        children: vec![],
    };
    let (mut layer, report) = import_into(&empty, edited, header, true)?;
    match layer.data.iter_mut().find(|n| n.path == path) {
        Some(n) => {
            n.attributes.insert("word::document".into(), json!({ "title": title }));
        }
        None => layer.data.insert(
            0,
            Node {
                path: path.to_string(),
                attributes: Map::from_iter([("word::document".into(), json!({ "title": title }))]),
                ..Node::default()
            },
        ),
    }
    Ok((layer, report))
}

fn import_into(
    node: &Composed,
    edited: &Document,
    header: Header,
    adopting: bool,
) -> Result<(Layer, ImportReport), WordError> {
    let mut report = ImportReport::default();
    if !adopting && custom_prop(edited, PROP_REVISION) != Some(revision_of(node).as_str()) {
        report.stale = true;
        report.warnings.push("the document changed in the stack since this file was exported; review the edit layer before stacking it".into());
    }

    let by_path: HashMap<&str, &Composed> =
        node.children.iter().map(|c| (c.path.as_str(), c)).collect();
    let mut used_names: BTreeSet<String> = node
        .children
        .iter()
        .filter_map(|c| c.name.clone())
        .collect();
    // Field nodes anywhere in the document: path → (owner paragraph, name, the field node).
    let mut fields: HashMap<&str, (&str, &str, &Composed)> = HashMap::new();
    for c in &node.children {
        for f in &c.children {
            if obj(f, "ox::link").is_some() {
                fields.insert(
                    f.path.as_str(),
                    (c.path.as_str(), f.name.as_deref().unwrap_or(""), f),
                );
            }
        }
    }

    let field_paths: BTreeSet<String> = fields.keys().map(|k| k.to_string()).collect();
    let is_field = |t: &str| field_paths.contains(t);
    let is_table = |t: &str| {
        by_path
            .get(t)
            .is_some_and(|c| c.attributes.contains_key("office::data::table"))
    };

    let mut nodes: Vec<Node> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut captions_seen: BTreeSet<String> = BTreeSet::new();
    let mut new_order: Vec<String> = Vec::new();
    let mut new_children: BTreeMap<String, Option<String>> = BTreeMap::new();
    let fresh_name = |prefix: &str, path: &str, used: &mut BTreeSet<String>| {
        let base = format!("{prefix}-{}", &path[..8.min(path.len())]);
        let mut name = base.clone();
        let mut i = 2;
        while used.contains(&name) {
            name = format!("{base}-{i}");
            i += 1;
        }
        used.insert(name.clone());
        name
    };

    for (index, block) in edited.body.iter().enumerate() {
        match &**block {
            Block::Para(p) => {
                let tag = para_tag(p, &is_field);
                let ex = extract(p, &is_field);
                if ex.dropped_objects > 0 {
                    report.warnings.push(format!(
                        "paragraph {}: {} inline object(s) (pictures, shapes…) are not carried yet",
                        index + 1,
                        ex.dropped_objects
                    ));
                }
                // Captions belong to their table node.
                if let Some(table_path) =
                    tag.as_deref().and_then(|t| t.strip_suffix(CAPTION_SUFFIX))
                {
                    if let Some(t) = by_path.get(table_path) {
                        captions_seen.insert(table_path.to_string());
                        let wt = obj(t, "word::table").cloned().unwrap_or_default();
                        let caption = ex.text.trim().to_string();
                        if wt.get("caption").and_then(Value::as_str) != Some(caption.as_str()) {
                            let mut wt2 = wt.clone();
                            wt2.insert("caption".into(), json!(caption));
                            nodes.push(Node {
                                path: table_path.to_string(),
                                attributes: Map::from_iter([(
                                    "word::table".into(),
                                    Value::Object(wt2),
                                )]),
                                ..Node::default()
                            });
                            report.changed.push(table_path.to_string());
                        }
                        continue;
                    }
                    // A caption for a table this document doesn't have: keep it as a paragraph
                    // rather than dropping its text.
                }
                let style = p
                    .props
                    .style
                    .as_deref()
                    .map(|id| {
                        edited
                            .styles
                            .get(id)
                            .map(|s| s.name.clone())
                            .unwrap_or_else(|| id.to_string())
                    })
                    .unwrap_or_else(|| "Normal".into());
                let existing = tag
                    .as_deref()
                    .and_then(|t| by_path.get(t).copied())
                    .filter(|c| c.attributes.contains_key("word::paragraph"));
                // A pasted copy carries its original's tag: the first is the original, a copy is new.
                let existing = match existing {
                    Some(c) if seen.contains(&c.path) => {
                        duplicate(&mut report, &c.path, "paragraph");
                        None
                    }
                    e => e,
                };
                if existing.is_none()
                    && tag.is_none()
                    && ex.text.trim().is_empty()
                    && ex.fields.is_empty()
                {
                    continue; // an empty paragraph typed between blocks
                }
                let para_path = match existing {
                    Some(c) => c.path.clone(),
                    None => stable_id(&format!("{}|word-edit|{}|{}", node.path, index, ex.text)),
                };
                // Resolve field runs to child names, adding a child when a field was copied here.
                let mut children: BTreeMap<String, Option<String>> = BTreeMap::new();
                let mut extra_nodes = Vec::new();
                let mut field_iter = ex.fields.iter();
                let mut runs = Vec::new();
                for r in &ex.runs {
                    let Some(fp) = r.get("fieldPath").and_then(Value::as_str) else {
                        runs.push(r.clone());
                        continue;
                    };
                    let shown = field_iter
                        .next()
                        .map(|(_, t)| t.clone())
                        .unwrap_or_default();
                    let Some((owner, name, fnode)) = fields.get(fp).copied() else {
                        report.warnings.push(format!(
                            "{para_path}: field `{fp}` is unknown; kept its text"
                        ));
                        runs.push(json!({ "text": shown }));
                        continue;
                    };
                    let expected = existing
                        .or(by_path.get(owner).copied())
                        .map(|o| ox_core::render::field_text(o, name))
                        .unwrap_or_default();
                    if shown != expected {
                        report.conflicts.push(Conflict { path: fp.to_string(), message: format!("linked field shows `{shown}`, source says `{expected}`; edit the source instead") });
                    }
                    if owner == para_path {
                        runs.push(json!({ "field": name }));
                    } else {
                        // Copied into another paragraph: a new field node inheriting the same source.
                        let new_path = stable_id(&format!("{para_path}|field|{fp}"));
                        let link = obj(fnode, "ox::link").cloned().unwrap_or_default();
                        let source = link
                            .get("source")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        let new_name = format!("field-{}", &new_path[..8]);
                        children.insert(new_name.clone(), Some(new_path.clone()));
                        extra_nodes.push(Node {
                            path: new_path,
                            inherits: BTreeMap::from([("source".to_string(), Some(source))]),
                            attributes: Map::from_iter([("ox::link".into(), Value::Object(link))]),
                            ..Node::default()
                        });
                        runs.push(json!({ "field": new_name }));
                    }
                }
                let new_para = json!({ "style": style, "runs": runs });
                match existing {
                    Some(c) => {
                        seen.insert(c.path.clone());
                        report
                            .matches
                            .push(ox_core::Match::certain(&c.path, "content control tag"));
                        new_order.push(c.name.clone().unwrap_or_default());
                        let old = obj(c, "word::paragraph").cloned().unwrap_or_default();
                        let old_runs: Vec<Value> = old
                            .get("runs")
                            .and_then(Value::as_array)
                            .cloned()
                            .unwrap_or_default();
                        let same_runs = canonical(&old_runs)
                            == canonical(
                                new_para["runs"]
                                    .as_array()
                                    .map(Vec::as_slice)
                                    .unwrap_or(&[]),
                            );
                        let same_style =
                            old.get("style").and_then(Value::as_str).unwrap_or("Normal") == style;
                        if !(same_runs && same_style) || !children.is_empty() {
                            // Keep the original field runs' formatting where the field is unchanged.
                            let mut merged = new_para.clone();
                            if let Some(rs) = merged["runs"].as_array_mut() {
                                for r in rs.iter_mut() {
                                    if let Some(f) = r.get("field").and_then(Value::as_str)
                                        && let Some(orig) = old_runs.iter().find(|o| {
                                            o.get("field").and_then(Value::as_str) == Some(f)
                                        })
                                    {
                                        *r = orig.clone();
                                    }
                                }
                            }
                            nodes.push(Node {
                                path: c.path.clone(),
                                children,
                                attributes: Map::from_iter([("word::paragraph".into(), merged)]),
                                ..Node::default()
                            });
                            report.changed.push(c.path.clone());
                        }
                    }
                    None => {
                        let name = fresh_name("p", &para_path, &mut used_names);
                        new_children.insert(name.clone(), Some(para_path.clone()));
                        new_order.push(name);
                        nodes.push(Node {
                            path: para_path.clone(),
                            children,
                            attributes: Map::from_iter([("word::paragraph".into(), new_para)]),
                            ..Node::default()
                        });
                        report.added.push(para_path);
                    }
                }
                nodes.extend(extra_nodes);
            }
            Block::Table(t) => {
                let grid = table_grid(t);
                let existing = table_tag(t, &is_table)
                    .and_then(|tag| by_path.get(tag.as_str()).copied())
                    .filter(|c| c.attributes.contains_key("office::data::table"));
                let existing = match existing {
                    Some(c) if seen.contains(&c.path) => {
                        duplicate(&mut report, &c.path, "table");
                        None
                    }
                    e => e,
                };
                match existing {
                    Some(c) => {
                        seen.insert(c.path.clone());
                        let by = if t
                            .controls
                            .open
                            .iter()
                            .any(|c| c.tag.starts_with(TAG_PREFIX))
                        {
                            "content control tag"
                        } else {
                            "content control tag in its first cell"
                        };
                        report.matches.push(ox_core::Match::certain(&c.path, by));
                        new_order.push(c.name.clone().unwrap_or_default());
                        let (cols, expected) = expected_grid(c);
                        if grid != expected {
                            if is_linked(c, "office::data::table") {
                                report.conflicts.push(Conflict {
                                    path: c.path.clone(),
                                    message: "linked table was edited in the document; the source wins — edit it in the workbook, or detach the link".into(),
                                });
                            } else {
                                let mut new_cols = cols.clone();
                                for (i, col) in new_cols.iter_mut().enumerate() {
                                    if let Some(h) = grid.first().and_then(|r| r.get(i)) {
                                        col.insert("name".into(), json!(h));
                                    }
                                }
                                let rows: Vec<Value> = grid
                                    .iter()
                                    .skip(1)
                                    .map(|r| {
                                        Value::Array(
                                            r.iter()
                                                .enumerate()
                                                .map(|(i, cell)| {
                                                    let kind = new_cols
                                                        .get(i)
                                                        .and_then(|c| c.get("type"))
                                                        .and_then(Value::as_str)
                                                        .unwrap_or("text");
                                                    parse_cell(cell, kind)
                                                        .unwrap_or_else(|| json!(cell))
                                                })
                                                .collect(),
                                        )
                                    })
                                    .collect();
                                let data = json!({ "columns": new_cols, "rows": rows });
                                nodes.push(Node {
                                    path: c.path.clone(),
                                    attributes: Map::from_iter([(
                                        "office::data::table".into(),
                                        data,
                                    )]),
                                    ..Node::default()
                                });
                                report.changed.push(c.path.clone());
                            }
                        }
                    }
                    None => {
                        let path = stable_id(&format!(
                            "{}|word-edit-table|{}|{:?}",
                            node.path,
                            index,
                            grid.first()
                        ));
                        let header = grid.first().cloned().unwrap_or_default();
                        if !t.rows.first().is_some_and(|r| r.props.header) {
                            report.warnings.push(format!(
                                "table starting `{}`: its first row becomes the column names, which export as a bold header row",
                                header.join(" | ").chars().take(40).collect::<String>()
                            ));
                        }
                        let body: Vec<&Vec<String>> = grid.iter().skip(1).collect();
                        let cols: Vec<Value> = header
                            .iter()
                            .enumerate()
                            .map(|(i, h)| {
                                let cells: Vec<&str> = body
                                    .iter()
                                    .filter_map(|r| r.get(i).map(String::as_str))
                                    .collect();
                                json!({ "name": h, "type": guess_type(&cells) })
                            })
                            .collect();
                        let rows: Vec<Value> = body
                            .iter()
                            .map(|r| {
                                Value::Array(
                                    r.iter()
                                        .enumerate()
                                        .map(|(i, cell)| {
                                            let kind = cols
                                                .get(i)
                                                .and_then(|c| c.get("type"))
                                                .and_then(Value::as_str)
                                                .unwrap_or("text");
                                            parse_cell(cell, kind).unwrap_or_else(|| json!(cell))
                                        })
                                        .collect(),
                                )
                            })
                            .collect();
                        let name = fresh_name("table", &path, &mut used_names);
                        new_children.insert(name.clone(), Some(path.clone()));
                        new_order.push(name);
                        nodes.push(Node {
                            path: path.clone(),
                            attributes: Map::from_iter([
                                (
                                    "office::data::table".into(),
                                    json!({ "columns": cols, "rows": rows }),
                                ),
                                ("word::table".into(), json!({})),
                            ]),
                            ..Node::default()
                        });
                        report.added.push(path);
                    }
                }
            }
        }
    }

    // Captions deleted from tables that are still there.
    for c in &node.children {
        if seen.contains(&c.path)
            && !captions_seen.contains(&c.path)
            && let Some(wt) = obj(c, "word::table").filter(|wt| wt.contains_key("caption"))
        {
            let mut wt2 = wt.clone();
            wt2.remove("caption");
            nodes.push(Node {
                path: c.path.clone(),
                attributes: Map::from_iter([("word::table".into(), Value::Object(wt2))]),
                ..Node::default()
            });
            report.changed.push(c.path.clone());
        }
    }
    // Removed children, and the new order.
    for c in &node.children {
        if !seen.contains(&c.path) {
            let name = c.name.clone().unwrap_or_default();
            new_children.insert(name.clone(), None);
            report.removed.push(name);
        }
    }
    let old_order: Vec<String> = node
        .children
        .iter()
        .filter_map(|c| c.name.clone())
        .collect();
    if !new_children.is_empty() || new_order != old_order {
        nodes.insert(
            0,
            Node {
                path: node.path.clone(),
                children: new_children,
                order: Some(new_order),
                ..Node::default()
            },
        );
    }
    // Merge statements about the same path (a paragraph changed twice, a caption and a table…).
    let mut merged: Vec<Node> = Vec::new();
    for n in nodes {
        match merged.iter_mut().find(|m| m.path == n.path) {
            Some(m) => {
                m.children.extend(n.children);
                m.inherits.extend(n.inherits);
                m.attributes.extend(n.attributes);
                if n.order.is_some() {
                    m.order = n.order;
                }
            }
            None => merged.push(n),
        }
    }
    let header = Header {
        ox_version: OX_VERSION.into(),
        kind: Some(header.kind.unwrap_or_else(|| "edit".into())),
        ..header
    };
    Ok((
        Layer {
            header,
            imports: vec![],
            schemas: BTreeMap::new(),
            data: merged,
        },
        report,
    ))
}
