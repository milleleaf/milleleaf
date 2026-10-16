//! Edited WordCraft `Document` → minimal OX edit layer.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use ox_core::Composed;
use ox_core::model::{Header, Layer, Node, OX_VERSION, stable_id};
use ox_core::render::format_value;
use serde::Serialize;
use serde_json::{Map, Value, json};
use wordcraft_doc::para::OBJ;
use wordcraft_doc::props::{Underline, VMerge};
use wordcraft_doc::section::HeaderSet;
use wordcraft_doc::styles::{PartFormat, PartSet};
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
        if r.contains_key("fieldPath") || r.contains_key("docField") {
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

/// A list's id in the model: the name Milleleaf gave its definition on export, else `list-<num>`.
fn list_id(doc: &Document, num: u32) -> String {
    doc.numbering
        .abstract_of(num)
        .and_then(|a| a.name.clone())
        .filter(|n| n.starts_with("list-"))
        .unwrap_or_else(|| format!("list-{num}"))
}

/// A list's definition as data: its levels (WordCraft's `Level`), with the list's own overrides.
fn list_def(doc: &Document, num: u32) -> Option<Value> {
    let a = doc.numbering.abstract_of(num)?;
    let n = doc.numbering.num(num)?;
    let mut levels = a.levels.clone();
    for (l, def) in &n.level_overrides {
        if let Some(x) = levels.get_mut(*l as usize) {
            *x = def.clone();
        }
    }
    for (l, start) in &n.start_overrides {
        if let Some(x) = levels.get_mut(*l as usize) {
            x.start = *start;
        }
    }
    Some(json!({ "levels": serde_json::to_value(levels).ok()? }))
}

/// A paragraph's direct formatting (alignment, spacing, indents, tabs, borders, keep with next…)
/// as data: WordCraft's paragraph properties, without what the model holds elsewhere (style,
/// list) and without tracked changes. `None` when it has none.
/// Settings that only repeat what the paragraph's style already gives are left out: editors write
/// them out on save (Word puts Heading 1's spacing on every heading), and they aren't an edit.
fn para_format(doc: &Document, p: &Paragraph) -> Option<Value> {
    let mut v = serde_json::to_value(&p.props).ok()?;
    let o = v.as_object_mut()?;
    for k in ["style", "numbering", "numChange", "fmtChange"] {
        o.remove(k);
    }
    let from_style = style_para(doc, p);
    o.retain(|k, v| from_style.get(k) != Some(v));
    (!o.is_empty()).then_some(v)
}

/// What a paragraph's style gives it: the document defaults, then each style it is based on.
fn style_para(doc: &Document, p: &Paragraph) -> Map<String, Value> {
    let mut m: Map<String, Value> = serde_json::to_value(&doc.styles.default_para)
        .ok()
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    let id = p.props.style.clone().or_else(|| doc.styles.find("Normal").map(|s| s.id.clone()));
    for st in id.as_deref().map(|id| doc.styles.chain(id)).unwrap_or_default() {
        if let Ok(Value::Object(sp)) = serde_json::to_value(&st.para) {
            m.extend(sp);
        }
    }
    m
}

/// The section's page setup as data (WordCraft's section properties: size, orientation,
/// margins, columns, page numbering, borders…), without the headers and footers, which the
/// model holds as `headers` / `footers` / `titlePage`.
fn page_setup(doc: &Document) -> Option<Value> {
    let mut v = serde_json::to_value(&doc.last_section).ok()?;
    let o = v.as_object_mut()?;
    for k in ["headers", "footers", "titlePage", "fmtChange"] {
        o.remove(k);
    }
    Some(v)
}

/// The names of the styles the document's paragraphs use, and Normal.
fn used_style_names(doc: &Document) -> BTreeSet<String> {
    let mut out: BTreeSet<String> = BTreeSet::from(["Normal".to_string()]);
    let parts = doc.parts.values().flat_map(|p| p.blocks.iter());
    for b in doc.body.iter().chain(parts) {
        if let Some(id) = b.as_para().and_then(|p| p.props.style.as_deref()) {
            out.insert(doc.styles.get(id).map(|s| s.name.clone()).unwrap_or_else(|| id.to_string()));
        }
    }
    out
}

/// How each style named in `names` looks, as a reader sees it: its effective font,
/// size, emphasis, colour, alignment, spacing and indents (document defaults, then the styles it
/// is based on). Editors rewrite style definitions on save (LibreOffice moves the defaults into
/// Normal, adds complex-script fonts) without changing this.
fn used_styles(doc: &Document, names: &BTreeSet<String>) -> BTreeMap<String, Value> {
    const SEEN: [&str; 15] = [
        "align", "spaceBefore", "spaceAfter", "lineSpacing", "indentLeft", "indentRight", "indentFirst",
        "font", "size", "bold", "italic", "underline", "color", "caps", "strike",
    ];
    let ids: Vec<String> = names.iter().filter_map(|n| doc.styles.find(n).map(|s| s.id.clone())).collect();
    let obj = |v: Option<Value>| v.and_then(|v| v.as_object().cloned()).unwrap_or_default();
    let mut out = BTreeMap::new();
    for id in &ids {
        let mut m = obj(serde_json::to_value(&doc.styles.default_para).ok());
        m.extend(obj(serde_json::to_value(&doc.styles.default_chr).ok()));
        for st in doc.styles.chain(id) {
            m.extend(obj(serde_json::to_value(&st.para).ok()));
            m.extend(obj(serde_json::to_value(&st.chr).ok()));
        }
        m.retain(|k, v| {
            SEEN.contains(&k.as_str())
                && !matches!((k.as_str(), &*v), ("align", Value::String(a)) if a == "left")
                && !matches!((k.as_str(), &*v), ("color", Value::String(c)) if c == "Auto")
                && *v != Value::Bool(false)
                && v.as_f64() != Some(0.0)
        });
        out.insert(doc.styles.get(id).map(|s| s.name.clone()).unwrap_or_else(|| id.clone()), Value::Object(m));
    }
    out
}

/// A paragraph's named style ("Normal" when it has none).
fn style_name(doc: &Document, p: &Paragraph) -> String {
    p.props
        .style
        .as_deref()
        .map(|id| doc.styles.get(id).map(|s| s.name.clone()).unwrap_or_else(|| id.to_string()))
        .unwrap_or_else(|| "Normal".into())
}

/// A header's or footer's paragraphs as `{style, runs}` (what isn't carried is counted in
/// `dropped`), by which page they're for: `default`, `first`, `even`.
fn header_set(doc: &Document, set: &HeaderSet, dropped: &mut usize) -> Map<String, Value> {
    let mut out = Map::new();
    for (which, id) in [("default", set.default), ("first", set.first), ("even", set.even)] {
        let Some(part) = id.and_then(|id| doc.parts.get(&id)) else { continue };
        let mut paras = Vec::new();
        for b in &part.blocks {
            match b.as_para() {
                Some(p) => {
                    let ex = extract(p, &|_| false);
                    *dropped += ex.dropped_objects;
                    let mut wp = json!({ "style": style_name(doc, p), "runs": ex.runs });
                    if let Some(f) = para_format(doc, p) {
                        wp["format"] = f;
                    }
                    paras.push(wp);
                }
                None => *dropped += 1,
            }
        }
        out.insert(which.into(), Value::Array(paras));
    }
    out
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
                // A Word field (PAGE, DATE…): its code, with the result it showed.
                Some(InlineObject::Field { instr, result, .. }) if !in_field => {
                    let mut fmt = run_format(p.props_of_char(off));
                    fmt.insert("docField".into(), json!(instr.trim()));
                    fmt.insert("text".into(), json!(result));
                    text.push_str(result);
                    runs.push(Value::Object(fmt));
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

/// The table's text on its grid: a cell spanning columns fills its first column and leaves the
/// rest empty, so every row has a cell for every column.
fn table_grid(t: &Table) -> Vec<Vec<String>> {
    t.rows
        .iter()
        .map(|r| {
            r.cells
                .iter()
                .flat_map(|c| {
                    std::iter::once(cell_text(c)).chain((1..c.props.span.max(1)).map(|_| String::new()))
                })
                .collect()
        })
        .collect()
}

/// How a table looks beyond its data: merged cells, and `header: false` when its first row isn't
/// a header row (its text becomes the column names either way).
fn table_look(t: &Table) -> Map<String, Value> {
    let mut wt = Map::new();
    if !t.rows.first().is_some_and(|r| r.props.header) {
        wt.insert("header".into(), json!(false));
    }
    let merges = table_merges(t);
    if !merges.is_empty() {
        wt.insert("merges".into(), json!(merges));
    }
    wt
}

/// How a table is drawn: its column widths (`grid`, points), its own properties (`props`:
/// borders, alignment, cell margins, shading… WordCraft's form) and each cell's (`cells`, by row
/// and cell: the cell's properties, its first paragraph's formatting and character format).
fn table_style_look(doc: &Document, t: &Table) -> Map<String, Value> {
    let strip = |v: Option<Value>, keys: &[&str]| -> Option<Value> {
        let mut v = v?;
        let o = v.as_object_mut()?;
        for k in keys {
            o.remove(*k);
        }
        o.retain(|_, x| !x.is_null());
        (!o.is_empty()).then_some(v)
    };
    let mut m = Map::new();
    m.insert("grid".into(), json!(t.grid));
    if let Some(p) = strip(serde_json::to_value(&t.props).ok(), &["style", "caption", "description", "fmtChange"]) {
        m.insert("props".into(), p);
    }
    let cells: Vec<Value> = t
        .rows
        .iter()
        .map(|r| {
            Value::Array(
                r.cells
                    .iter()
                    .map(|c| {
                        let mut cm = Map::new();
                        if let Some(p) = strip(serde_json::to_value(&c.props).ok(), &["span", "vmerge", "width", "widthPct", "fmtChange"]) {
                            cm.insert("props".into(), p);
                        }
                        if let Some(p) = c.blocks.iter().find_map(|b| b.as_para()) {
                            if let Some(f) = para_format(doc, p) {
                                cm.insert("para".into(), f);
                            }
                            if !p.text.is_empty() {
                                if let Some(ch) = strip(serde_json::to_value(p.props_of_char(0)).ok(), &["fmtChange", "change"]) {
                                    cm.insert("chr".into(), ch);
                                }
                            }
                        }
                        Value::Object(cm)
                    })
                    .collect(),
            )
        })
        .collect();
    m.insert("cells".into(), Value::Array(cells));
    m
}

/// What a reader sees of a table: column widths (to the half point), its alignment and indent,
/// and per cell the effective shading, emphasis, colour, size and alignment (its table style's
/// parts, then the cell's own formatting). Editors move table-style formatting into the cells on
/// save (LibreOffice does) without changing this.
fn table_seen(doc: &Document, t: &Table) -> Value {
    let style = t.props.style.as_deref().and_then(|id| doc.styles.table_style(id)).unwrap_or_default();
    let half = |x: f32| (x * 2.0).round() as i64;
    let nrows = t.rows.len();
    let ncols = t.grid.len();
    let mut rows = Vec::new();
    for (ri, row) in t.rows.iter().enumerate() {
        let mut cells = Vec::new();
        let mut g = 0usize;
        for c in &row.cells {
            let span = c.props.span.max(1) as usize;
            let mut pf = PartFormat::default();
            for part in PartSet::of_cell(&t.props.look, 1, (ri, nrows), (g, span, ncols)).iter() {
                pf.overlay(&style.parts.part(part));
            }
            let p = c.blocks.iter().find_map(|b| b.as_para());
            let ch = p.filter(|p| !p.text.is_empty()).map(|p| p.props_of_char(0).clone()).unwrap_or_default();
            // Document defaults, the paragraph's style, the table style, its parts, the run.
            let mut chr = doc.styles.default_chr.clone();
            let pstyle = p.and_then(|p| p.props.style.clone()).or_else(|| doc.styles.find("Normal").map(|s| s.id.clone()));
            for st in pstyle.as_deref().map(|id| doc.styles.chain(id)).unwrap_or_default() {
                chr.overlay(&st.chr);
            }
            chr.overlay(&style.chr);
            chr.overlay(&pf.chr);
            chr.overlay(&ch);
            let align = p.and_then(|p| p.props.align).or(pf.align);
            cells.push(json!([
                c.props.shading.or(pf.fill).map(|r| r.hex()),
                chr.bold == Some(true),
                chr.italic == Some(true),
                serde_json::to_value(chr.color.as_ref().filter(|c| !matches!(c, TextColor::Auto))).ok(),
                chr.size.map(half),
                serde_json::to_value(align.filter(|a| *a != wordcraft_doc::Align::Left)).ok(),
                serde_json::to_value(c.props.valign.or(pf.v_align)).ok(),
            ]));
            g += span;
        }
        rows.push(Value::Array(cells));
    }
    json!({
        "grid": t.grid.iter().map(|w| half(*w)).collect::<Vec<_>>(),
        "align": serde_json::to_value(t.props.align.filter(|a| *a != wordcraft_doc::Align::Left)).ok(),
        "indent": t.props.indent.map(half).filter(|i| *i != 0),
        "rows": rows,
    })
}

/// Whether two `table_seen` values look the same, over the rows both have: a linked table can
/// have gained or lost rows since the file was exported (the data changed, not the look).
fn same_look(ours: Option<&Value>, now: &Value) -> bool {
    let Some(ours) = ours else { return false };
    let row_cells = |v: &Value| v["rows"].as_array().cloned().unwrap_or_default();
    let (a, b) = (row_cells(ours), row_cells(now));
    ["grid", "align", "indent"].iter().all(|k| ours[k] == now[k]) && a.iter().zip(&b).all(|(x, y)| x == y)
}

/// Merged cells on the grid: `{row, col, rows, cols}`, row 0 being the first (header) row.
fn table_merges(t: &Table) -> Vec<Value> {
    // (row, grid column) → (vmerge, span) for every cell.
    let mut at: Vec<Vec<(VMerge, u32)>> = Vec::new();
    for r in &t.rows {
        let mut row = Vec::new();
        for c in &r.cells {
            row.push((c.props.vmerge, c.props.span.max(1)));
            row.extend((1..c.props.span.max(1)).map(|_| (VMerge::None, 0)));
        }
        at.push(row);
    }
    let mut out = Vec::new();
    for (ri, row) in at.iter().enumerate() {
        for (ci, &(vm, span)) in row.iter().enumerate() {
            if span == 0 || vm == VMerge::Continue {
                continue;
            }
            let rows = if vm == VMerge::Restart {
                1 + at[ri + 1..]
                    .iter()
                    .take_while(|r| r.get(ci).is_some_and(|c| c.0 == VMerge::Continue))
                    .count()
            } else {
                1
            };
            if rows > 1 || span > 1 {
                out.push(json!({ "row": ri, "col": ci, "rows": rows, "cols": span }));
            }
        }
    }
    out
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
            let wd = n.attributes.entry("word::document").or_insert_with(|| json!({}));
            wd["title"] = json!(title);
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
    // What Milleleaf's own export writes for this node, read back the same way: page setup,
    // styles and tables' looks are carried when the file's differ from it.
    let ours_doc = crate::export(node, &[])
        .ok()
        .and_then(|(d, _)| crate::from_docx(&crate::to_docx(&d).ok()?).ok());
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

    let ours_tables: HashMap<String, Value> = ours_doc
        .iter()
        .flat_map(|d| d.body.iter().map(move |b| (d, b)))
        .filter_map(|(d, b)| match &**b {
            Block::Table(t) => Some((table_tag(t, &is_table)?, table_seen(d, t))),
            _ => None,
        })
        .collect();
    let mut nodes: Vec<Node> = Vec::new();
    // Lists the paragraphs use: list id → numbering instance in the edited file.
    let mut lists_used: BTreeMap<String, u32> = BTreeMap::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut captions_seen: BTreeSet<String> = BTreeSet::new();
    let mut new_order: Vec<String> = Vec::new();
    // Where the last matched block sits among the original's children.
    let child_idx: HashMap<&str, usize> = node.children.iter().enumerate().map(|(i, c)| (c.path.as_str(), i)).collect();
    let mut last_seen: Option<usize> = None;
    let tags_in_file: BTreeSet<String> = edited
        .body
        .iter()
        .filter_map(|b| match &**b {
            Block::Para(p) => para_tag(p, &is_field),
            _ => None,
        })
        .collect();
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
                let style = style_name(edited, p);
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
                // An untagged empty paragraph is a blank line someone typed, and comes back, unless
                // it's one Word must have: after a table that ends the body, the only paragraph of
                // an empty body, or one holding a section break (sections are reported as not
                // carried).
                let required = index + 1 == edited.body.len()
                    && (index == 0 || matches!(&*edited.body[index - 1], Block::Table(_)))
                    || p.section.is_some();
                let blank = existing.is_none() && tag.is_none() && ex.text.trim().is_empty() && ex.fields.is_empty();
                if !adopting && blank && required {
                    continue;
                }
                // Editors drop the tag from an empty paragraph (LibreOffice does): a blank line where
                // the original had an empty paragraph, right after the last block matched, is that one.
                let by_position = !adopting && blank;
                let existing = if by_position {
                    node.children
                        .get(last_seen.map_or(0, |i| i + 1))
                        .filter(|c| !seen.contains(&c.path) && !tags_in_file.contains(c.path.as_str()))
                        .filter(|c| {
                            obj(c, "word::paragraph").is_some_and(|w| {
                                w.get("runs").and_then(Value::as_array).is_none_or(|r| {
                                    r.iter().all(|r| r.get("text").and_then(Value::as_str).is_some_and(|t| t.trim().is_empty()))
                                })
                            })
                        })
                } else {
                    existing
                };
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
                let mut new_para = json!({ "style": style, "runs": runs });
                if let Some(f) = para_format(edited, p) {
                    new_para["format"] = f;
                }
                if let Some(n) = p.props.numbering.filter(|n| n.num != 0) {
                    let id = list_id(edited, n.num);
                    lists_used.entry(id.clone()).or_insert(n.num);
                    new_para["list"] = json!({ "id": id, "level": n.level });
                }
                match existing {
                    Some(c) => {
                        seen.insert(c.path.clone());
                        last_seen = child_idx.get(c.path.as_str()).copied();
                        report.matches.push(if by_position {
                            ox_core::Match::best_guess(&c.path, "an empty paragraph in the same place")
                        } else {
                            ox_core::Match::certain(&c.path, "content control tag")
                        });
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
                            old.get("style").and_then(Value::as_str).unwrap_or("Normal") == style
                                && old.get("list") == new_para.get("list")
                                && old.get("format") == new_para.get("format");
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
                        last_seen = child_idx.get(c.path.as_str()).copied();
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
                        // Merges or the header row changed in Word.
                        let old = obj(c, "word::table").cloned().unwrap_or_default();
                        let mut wt = old.clone();
                        wt.remove("header");
                        wt.remove("merges");
                        wt.extend(table_look(t));
                        // The table's look changed in Word when what a reader sees differs from
                        // what Milleleaf would export: then the file's look comes back whole.
                        if !same_look(ours_tables.get(&c.path), &table_seen(edited, t)) {
                            for (k, v) in table_style_look(edited, t) {
                                wt.insert(k, v);
                            }
                        }
                        if wt != old {
                            nodes.push(Node {
                                path: c.path.clone(),
                                attributes: Map::from_iter([("word::table".into(), Value::Object(wt))]),
                                ..Node::default()
                            });
                            report.changed.push(c.path.clone());
                        }
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
                        let mut wt = table_look(t);
                        wt.extend(table_style_look(edited, t));
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
                                ("word::table".into(), Value::Object(wt)),
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
    // Definitions of lists the document doesn't have yet (a list made in Word, or adopted).
    let wd = obj(node, "word::document").cloned().unwrap_or_default();
    let known = wd.get("lists").and_then(Value::as_object).cloned().unwrap_or_default();
    let new_lists: Map<String, Value> = lists_used
        .iter()
        .filter(|(id, _)| !known.contains_key(*id))
        .filter_map(|(id, num)| Some((id.clone(), list_def(edited, *num)?)))
        .collect();
    let mut wd2 = wd.clone();
    if !new_lists.is_empty() {
        let mut all = known.clone();
        all.extend(new_lists);
        wd2.insert("lists".into(), Value::Object(all));
    }
    let ours = ours_doc.as_ref().and_then(page_setup);
    // The style sheet (WordCraft's form), when a style the document uses is defined differently:
    // an adopted file's own Normal (its font, size, alignment), or a style changed in Word.
    if let Some(od) = &ours_doc {
        let ids = used_style_names(edited);
        if used_styles(edited, &ids) != used_styles(od, &ids) {
            if let Ok(v) = serde_json::to_value(&edited.styles) {
                wd2.insert("styles".into(), v);
            }
        }
    }
    // Compared on what a reader sees (size, orientation, margins, columns): editors rewrite the
    // rest (grid, header distance) on every save.
    let seen = |v: &Option<Value>| {
        v.as_ref().map(|v| {
            let n = |k: &str| (v.get(k).and_then(Value::as_f64).unwrap_or(0.0) * 2.0).round() as i64;
            let cols = v.pointer("/columns/count").and_then(Value::as_u64);
            (n("pageW"), n("pageH"), v.get("landscape") == Some(&json!(true)), [n("marginTop"), n("marginBottom"), n("marginLeft"), n("marginRight")], cols)
        })
    };
    let page = page_setup(edited);
    if seen(&page) != seen(&ours) {
        if let Some(p) = page {
            wd2.insert("page".into(), p);
        }
    }
    // Headers and footers, and which pages they're for.
    let sect = &edited.last_section;
    let mut dropped = 0;
    for (key, set) in [("headers", &sect.headers), ("footers", &sect.footers)] {
        let v = header_set(edited, set, &mut dropped);
        if v.is_empty() {
            wd2.remove(key);
        } else {
            wd2.insert(key.into(), Value::Object(v));
        }
    }
    for (key, on) in [("titlePage", sect.title_page), ("evenOddHeaders", edited.settings.even_odd_headers)] {
        if on {
            wd2.insert(key.into(), json!(true));
        } else {
            wd2.remove(key);
        }
    }
    // The document's own footnote and endnote separator lines (WordCraft's form, as data): a
    // template that removes the line would otherwise get it back.
    let mut seps = Map::new();
    for (k, ns) in [("footnote", &edited.footnote_separators), ("endnote", &edited.endnote_separators)] {
        if !ns.is_empty() {
            if let Ok(v) = serde_json::to_value(ns) {
                seps.insert(k.into(), v);
            }
        }
    }
    if seps.is_empty() {
        wd2.remove("noteSeparators");
    } else {
        wd2.insert("noteSeparators".into(), Value::Object(seps));
    }
    if dropped > 0 {
        report.warnings.push(format!(
            "headers and footers: {dropped} picture(s), shape(s) or table(s) are not carried yet"
        ));
    }
    if wd2 != wd {
        if !adopting {
            report.changed.push(node.path.clone());
        }
        nodes.push(Node {
            path: node.path.clone(),
            attributes: Map::from_iter([("word::document".into(), Value::Object(wd2))]),
            ..Node::default()
        });
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
