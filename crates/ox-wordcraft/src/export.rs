//! Composed `word::document` → WordCraft `Document`.

use std::collections::HashMap;
use std::sync::Arc;

use ox_core::Composed;
use ox_core::render::{field_text, format_value};
use serde_json::{Map, Value};
use wordcraft_doc::numbering::Level;
use wordcraft_doc::para::OBJ;
use wordcraft_doc::props::{CellProps, NumRef, ParaProps, TableProps, VMerge};
use wordcraft_doc::section::SectionProps;
use wordcraft_doc::styles::StyleSheet;
use wordcraft_doc::props::Underline;
use wordcraft_doc::{
    Align, Block, Cell, CharProps, ContentControl, ControlKind, ControlLock, CustomProp, Document,
    InlineObject, Paragraph, PartKind, Rgb, Table, TextColor, para_block,
};

use crate::{
    CAPTION_SUFFIX, PROP_DOCUMENT, PROP_LAYERS, PROP_REVISION, TAG_PREFIX, WordError, revision_of,
};

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ExportReport {
    /// Child nodes with nothing WordCraft can show yet, or styles the document lacks.
    pub warnings: Vec<String>,
    pub paragraphs: usize,
    pub tables: usize,
    pub fields: usize,
}

/// Page size in points for a `word::document.pageSize`.
pub fn page_size(name: &str) -> Option<(f32, f32)> {
    match name {
        "A4" => Some((595.3, 841.9)),
        "A3" => Some((841.9, 1190.6)),
        "Letter" => Some((612.0, 792.0)),
        _ => None,
    }
}

fn obj<'a>(n: &'a Composed, key: &str) -> Option<&'a Map<String, Value>> {
    n.attributes.get(key).and_then(Value::as_object)
}

fn err(e: impl std::fmt::Display) -> WordError {
    WordError::Docx(e.to_string())
}

/// A block-level control around a whole paragraph or table, tagged with a node path.
fn tag_control(tag: String, title: &str, lock: ControlLock) -> ContentControl {
    ContentControl {
        tag,
        title: title.to_string(),
        lock,
        block: true,
        kind: ControlKind::RichText,
        placeholder_text: String::new(),
        ..ContentControl::default()
    }
}

fn push_text(p: &mut Paragraph, s: &str, props: &CharProps) -> Result<(), WordError> {
    // Never let stray object-replacement characters into the text: they mean "object here".
    let clean: String = s.chars().filter(|c| *c != OBJ).collect();
    if clean.is_empty() {
        return Ok(());
    }
    let off = p.len();
    p.insert_text(off, &clean, props).map(|_| ()).map_err(err)
}

fn push_obj(p: &mut Paragraph, o: InlineObject) -> Result<(), WordError> {
    let off = p.len();
    p.insert_object(off, o, &CharProps::default()).map_err(err)
}

fn parse_hex(c: &str) -> Option<Rgb> {
    let h = c.strip_prefix('#')?;
    if h.len() < 6 {
        return None;
    }
    let v = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    Some(Rgb(v(0)?, v(2)?, v(4)?))
}

/// `office`/`word` run formatting → WordCraft character properties (direct formatting only).
pub(crate) fn char_props(r: &Map<String, Value>) -> CharProps {
    let mut cp = CharProps::default();
    if r.get("bold").and_then(Value::as_bool) == Some(true) {
        cp.bold = Some(true);
    }
    if r.get("italic").and_then(Value::as_bool) == Some(true) {
        cp.italic = Some(true);
    }
    if r.get("underline").and_then(Value::as_bool) == Some(true) {
        cp.underline = Some(Underline::Single);
    }
    if let Some(c) = r.get("color").and_then(Value::as_str).and_then(parse_hex) {
        cp.color = Some(TextColor::Rgb(c));
    }
    if let Some(f) = r.get("font").and_then(Value::as_str) {
        cp.font = Some(f.to_string());
    }
    if let Some(s) = r.get("sizePt").and_then(Value::as_f64) {
        cp.size = Some(s as f32);
    }
    cp
}

/// The style id for a style display name ("Heading 1" → "Heading1"), if the document has it.
/// A family name such as "Grid Table 4" picks its first variant ("Grid Table 4 – Blue").
fn style_id(doc: &Document, name: &str) -> Option<String> {
    doc.styles
        .find(name)
        .or_else(|| {
            doc.styles
                .styles
                .iter()
                .find(|s| s.name.starts_with(&format!("{name} ")))
        })
        .map(|s| s.id.clone())
}

/// Where pictures' bytes come from, and the document media they become.
struct Pics<'a> {
    assets: ox_core::PictureSource<'a>,
    media: std::collections::BTreeMap<String, Arc<Vec<u8>>>,
}

impl Pics<'_> {
    /// The media key for a picture's blob, its bytes read once.
    fn key(&mut self, blob: &str, media_type: &str, at: &str) -> Result<String, WordError> {
        // One media part per blob value; the name only has to be unique in the file.
        let key = format!("image-{}.{}", &ox_core::integrity_of(blob.as_bytes())[7..19], ox_core::ext_for(media_type));
        if !self.media.contains_key(&key) {
            let bytes = ox_core::picture_bytes(self.assets, blob).map_err(|e| WordError::MissingAsset(format!("{at}: can't read {blob}: {e}")))?;
            self.media.insert(key.clone(), Arc::new(bytes));
        }
        Ok(key)
    }
}

fn paragraph_block(
    doc: &Document,
    node: &Composed,
    lists: &HashMap<String, u32>,
    pics: &mut Pics,
    report: &mut ExportReport,
) -> Result<Paragraph, WordError> {
    let wp = obj(node, "word::paragraph").cloned().unwrap_or_default();
    let p = para_from(doc, &wp, Some(node), &node.path, lists, pics, report)?;
    report.paragraphs += 1;
    Ok(p)
}

/// A paragraph from `{style, list, runs}`. With `node` (a body paragraph) it carries the node's
/// identity control and its linked fields; without (a header or footer) it is plain content.
fn para_from(
    doc: &Document,
    wp: &Map<String, Value>,
    node: Option<&Composed>,
    at: &str,
    lists: &HashMap<String, u32>,
    pics: &mut Pics,
    report: &mut ExportReport,
) -> Result<Paragraph, WordError> {
    let style_name = wp.get("style").and_then(Value::as_str).unwrap_or("Normal");
    let mut p = Paragraph::new();
    match style_id(doc, style_name) {
        Some(id) => p = p.styled(&id),
        None => report.warnings.push(format!(
            "{at}: style `{style_name}` not in the document; used Normal"
        )),
    }
    if let Some(f) = wp.get("format") {
        // Direct formatting (WordCraft's paragraph properties); the style and list set above win.
        match serde_json::from_value::<ParaProps>(f.clone()) {
            Ok(fp) => p.props = ParaProps { style: p.props.style.take(), numbering: p.props.numbering, ..fp },
            Err(e) => report.warnings.push(format!("{at}: paragraph formatting can't be read ({e}); not applied")),
        }
    }
    if let Some(l) = wp.get("list") {
        let id = l.get("id").and_then(Value::as_str).unwrap_or("");
        let level = l.get("level").and_then(Value::as_u64).unwrap_or(0).min(8) as u8;
        match lists.get(id) {
            Some(&num) => p.props.numbering = Some(NumRef { num, level }),
            None => report.warnings.push(format!(
                "{at}: list `{id}` isn't defined in the document; exported without numbering"
            )),
        }
    }
    if let Some(node) = node {
        push_obj(
            &mut p,
            InlineObject::ControlStart {
                control: Box::new(tag_control(
                    format!("{TAG_PREFIX}{}", node.path),
                    "",
                    ControlLock::Unlocked,
                )),
            },
        )?;
    }
    for run in wp
        .get("runs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(r) = run.as_object() else { continue };
        if let Some(pic) = r.get("picture").and_then(Value::as_object) {
            let blob = pic.get("blob").and_then(Value::as_str).unwrap_or("");
            let media_type = pic.get("mediaType").and_then(Value::as_str).unwrap_or_else(|| ox_core::media_type_for(blob));
            let n = |k: &str| pic.get(k).and_then(Value::as_f64).unwrap_or(0.0) as f32;
            let mut crop = [0.0f32; 4];
            for (i, c) in pic.get("crop").and_then(Value::as_array).into_iter().flatten().take(4).enumerate() {
                crop[i] = c.as_f64().unwrap_or(0.0) as f32;
            }
            let float = match pic.get("float") {
                Some(f) => serde_json::from_value(f.clone()).unwrap_or_else(|e| {
                    report.warnings.push(format!("{at}: a picture's placement can't be read ({e}); placed inline"));
                    Default::default()
                }),
                None => Default::default(),
            };
            let image = InlineObject::Image {
                media: pics.key(blob, media_type, at)?,
                w: n("w"),
                h: n("h"),
                alt: pic.get("alt").and_then(Value::as_str).unwrap_or("").to_string(),
                float,
                crop,
                ole: None,
            };
            push_obj(&mut p, image)?;
            continue;
        }
        let props = char_props(r);
        let text = r.get("text").and_then(Value::as_str).unwrap_or("");
        if let Some(instr) = r.get("docField").and_then(Value::as_str) {
            // A Word field: Word updates its result (the page number…) when it lays the page out.
            let f = InlineObject::Field {
                instr: format!(" {instr} "),
                result: text.to_string(),
                locked: false,
                dirty: false,
                code: false,
                form: String::new(),
            };
            let off = p.len();
            p.insert_object(off, f, &props).map_err(err)?;
        } else if let (Some(field), Some(node)) = (r.get("field").and_then(Value::as_str), node) {
            let child = node
                .children
                .iter()
                .find(|c| c.name.as_deref() == Some(field));
            let source = child
                .and_then(|c| obj(c, "ox::link"))
                .and_then(|l| l.get("source"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let path = child.map(|c| c.path.as_str()).unwrap_or("");
            let mut control = tag_control(
                format!("{TAG_PREFIX}{path}"),
                &format!("Linked: {source}"),
                ControlLock::ContentLocked,
            );
            control.block = false;
            control.kind = ControlKind::Text { multi_line: false };
            push_obj(
                &mut p,
                InlineObject::ControlStart {
                    control: Box::new(control),
                },
            )?;
            push_text(&mut p, &field_text(node, field), &props)?;
            push_obj(&mut p, InlineObject::ControlEnd)?;
            report.fields += 1;
        } else {
            push_text(&mut p, text, &props)?;
        }
    }
    if node.is_some() {
        push_obj(&mut p, InlineObject::ControlEnd)?;
    }
    Ok(p)
}

/// Applies `word::table`'s `grid`, `props` and `cells` (see the importer's `table_style_look`) to a
/// table whose merges are done: cells are addressed as they stand, row by row.
fn table_style_look(t: &mut Table, wt: &Map<String, Value>, path: &str, report: &mut ExportReport) {
    let mut bad = |what: &str, e: &dyn std::fmt::Display| report.warnings.push(format!("{path}: {what} can't be read ({e}); not applied"));
    if let Some(g) = wt.get("grid") {
        match serde_json::from_value::<Vec<f32>>(g.clone()) {
            Ok(g) if g.len() == t.grid.len() => {
                t.grid = g;
                for row in &mut t.rows {
                    let mut col = 0usize;
                    for c in &mut row.cells {
                        let span = c.props.span.max(1) as usize;
                        c.props.width = Some(t.grid[col.min(t.grid.len())..(col + span).min(t.grid.len())].iter().sum());
                        col += span;
                    }
                }
            }
            Ok(g) => bad("column widths", &format!("{} widths for {} columns", g.len(), t.grid.len())),
            Err(e) => bad("column widths", &e),
        }
    }
    if let Some(p) = wt.get("props") {
        match serde_json::from_value::<TableProps>(p.clone()) {
            Ok(p) => t.props = TableProps { style: t.props.style.take(), ..p },
            Err(e) => bad("table properties", &e),
        }
    }
    let Some(rows) = wt.get("cells").and_then(Value::as_array) else { return };
    for (row, looks) in t.rows.iter_mut().zip(rows) {
        for (c, look) in row.cells.iter_mut().zip(looks.as_array().into_iter().flatten()) {
            if let Some(p) = look.get("props") {
                match serde_json::from_value::<CellProps>(p.clone()) {
                    Ok(p) => c.props = CellProps { span: c.props.span, vmerge: c.props.vmerge, width: c.props.width, ..p },
                    Err(e) => bad("cell properties", &e),
                }
            }
            let para = look.get("para").and_then(|v| serde_json::from_value::<ParaProps>(v.clone()).ok());
            let chr = look.get("chr").and_then(|v| serde_json::from_value::<CharProps>(v.clone()).ok());
            for b in &mut c.blocks {
                let Block::Para(p) = Arc::make_mut(b) else { continue };
                let props = match &para {
                    Some(f) => ParaProps { style: p.props.style.take(), numbering: p.props.numbering, ..f.clone() },
                    None => p.props.clone(),
                };
                if let Some(ch) = &chr {
                    let mut q = Paragraph::with_text(&p.plain_text(), ch.clone());
                    q.props = props;
                    *p = q;
                } else {
                    p.props = props;
                }
            }
        }
    }
}

/// Applies `word::table.merges` (`{row, col, rows, cols}` on the grid) to a table with one cell
/// per grid column: the first cell spans, the cells it covers go (across) or continue (down).
fn merge_cells(t: &mut Table, merges: Option<&Value>, path: &str, report: &mut ExportReport) {
    let mut drop: Vec<Vec<usize>> = vec![Vec::new(); t.rows.len()];
    for m in merges.and_then(Value::as_array).into_iter().flatten() {
        let g = |k: &str| m.get(k).and_then(Value::as_u64).unwrap_or(1) as usize;
        let (r0, c0, rows, cols) = (g("row"), g("col"), g("rows").max(1), g("cols").max(1));
        if r0 + rows > t.rows.len() || t.rows[r0..r0 + rows].iter().any(|r| c0 + cols > r.cells.len()) {
            report.warnings.push(format!("{path}: merged cells at row {r0}, column {c0} are outside the table; not merged"));
            continue;
        }
        for r in r0..r0 + rows {
            let row = &mut t.rows[r].cells;
            let width: Option<f32> = row[c0..c0 + cols].iter().map(|c| c.props.width).sum();
            let first = &mut row[c0].props;
            first.span = cols as u32;
            first.width = width;
            first.vmerge = match (rows > 1, r == r0) {
                (false, _) => VMerge::None,
                (true, true) => VMerge::Restart,
                (true, false) => VMerge::Continue,
            };
            drop[r].extend(c0 + 1..c0 + cols);
        }
    }
    for (row, d) in t.rows.iter_mut().zip(drop) {
        let mut i = 0;
        row.cells.retain(|_| {
            i += 1;
            !d.contains(&(i - 1))
        });
    }
}

fn table_block(
    doc: &Document,
    node: &Composed,
    text_width: f32,
    report: &mut ExportReport,
) -> Result<Vec<Block>, WordError> {
    let data = obj(node, "office::data::table")
        .cloned()
        .unwrap_or_default();
    let cols: Vec<Map<String, Value>> = data
        .get("columns")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|c| c.as_object().cloned()).collect())
        .unwrap_or_default();
    let rows: Vec<Vec<Value>> = data
        .get("rows")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|r| r.as_array().cloned().unwrap_or_default())
                .collect()
        })
        .unwrap_or_default();
    let ncols = cols.len().max(1);
    let mut t = Table::new(rows.len() + 1, ncols, text_width);
    let wt = obj(node, "word::table").cloned().unwrap_or_default();
    if let Some(style) = wt.get("style").and_then(Value::as_str) {
        match style_id(doc, style) {
            _ if style.is_empty() => t.props.style = None,
            Some(id) => t.props.style = Some(id),
            None => report.warnings.push(format!(
                "{}: table style `{style}` not in the document; used Table Grid",
                node.path
            )),
        }
    }
    let numeric = |i: usize| {
        matches!(
            cols.get(i)
                .and_then(|c| c.get("type"))
                .and_then(Value::as_str),
            Some("currency" | "number" | "percent")
        )
    };
    let cell = |text: &str, bold: bool, right: bool| {
        let props = CharProps {
            bold: bold.then_some(true),
            ..CharProps::default()
        };
        let mut p = Paragraph::with_text(text, props);
        if right {
            p.props.align = Some(Align::Right);
        }
        let mut c = Cell::with_text("");
        c.blocks = vec![para_block(p)];
        c
    };
    // The first row holds the column names; it is a (bold, repeating) header row unless the
    // table says otherwise.
    let is_header = wt.get("header").and_then(Value::as_bool) != Some(false);
    if let Some(header) = t.rows.first_mut() {
        header.props.header = is_header;
        for (i, c) in header.cells.iter_mut().enumerate() {
            let name = cols
                .get(i)
                .and_then(|c| c.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let w = c.props.width;
            *c = cell(name, is_header, numeric(i));
            c.props.width = w;
        }
    }
    for (r, row) in rows.iter().enumerate() {
        let Some(trow) = t.rows.get_mut(r + 1) else {
            continue;
        };
        for (i, c) in trow.cells.iter_mut().enumerate() {
            let fmt = cols
                .get(i)
                .and_then(|c| c.get("type"))
                .and_then(Value::as_str)
                .unwrap_or("text");
            let text = format_value(row.get(i).unwrap_or(&Value::Null), fmt);
            let w = c.props.width;
            *c = cell(&text, false, numeric(i));
            c.props.width = w;
        }
    }
    merge_cells(&mut t, wt.get("merges"), &node.path, report);
    table_style_look(&mut t, &wt, &node.path, report);
    let linked = obj(node, "ox::link")
        .and_then(|l| l.get("source"))
        .and_then(Value::as_str);
    let (title, lock) = match linked {
        Some(src) => (format!("Linked: {src}"), ControlLock::ContentLocked),
        None => (String::new(), ControlLock::Unlocked),
    };
    t.controls.open.push(tag_control(
        format!("{TAG_PREFIX}{}", node.path),
        &title,
        lock,
    ));
    t.controls.close = 1;
    let mut out = vec![Block::Table(t)];
    if let Some(caption) = wt.get("caption").and_then(Value::as_str) {
        let mut p = Paragraph::new();
        if let Some(id) = style_id(doc, "Caption") {
            p = p.styled(&id);
        }
        push_obj(
            &mut p,
            InlineObject::ControlStart {
                control: Box::new(tag_control(
                    format!("{TAG_PREFIX}{}{CAPTION_SUFFIX}", node.path),
                    "",
                    ControlLock::Unlocked,
                )),
            },
        )?;
        push_text(&mut p, caption, &CharProps::default())?;
        push_obj(&mut p, InlineObject::ControlEnd)?;
        out.push(Block::Para(p));
    }
    report.tables += 1;
    Ok(out)
}

/// Builds a WordCraft document from a composed `word::document` node.
///
/// `layers` are the ids of the stack's layers (bottom first), recorded in the file so an import
/// can say what the export was based on.
/// `assets` is the folder pictures' blobs are read from: the project (or stack) folder.
pub fn export(node: &Composed, layers: &[String], assets: &std::path::Path) -> Result<(Document, ExportReport), WordError> {
    export_with(node, layers, ox_core::PictureSource::Read(assets))
}

/// The export, or the importer's comparison export (stand-ins for pictures).
pub(crate) fn export_with(node: &Composed, layers: &[String], assets: ox_core::PictureSource) -> Result<(Document, ExportReport), WordError> {
    let mut pics = Pics { assets, media: Default::default() };
    let wd = obj(node, "word::document")
        .ok_or_else(|| WordError::NotADocument(node.path.clone()))?
        .clone();
    let mut doc = Document::new();
    let mut report = ExportReport::default();
    doc.body.clear();
    if let Some((w, h)) = wd
        .get("pageSize")
        .and_then(Value::as_str)
        .and_then(page_size)
    {
        doc.last_section.page_w = w;
        doc.last_section.page_h = h;
    }
    if let Some(st) = wd.get("styles") {
        // The document's own style sheet (an adopted file's, or one edited in Word).
        match serde_json::from_value::<StyleSheet>(st.clone()) {
            Ok(st) => doc.styles = st,
            Err(e) => report.warnings.push(format!("styles can't be read ({e}); Milleleaf's own are used")),
        }
    }
    if let Some(page) = wd.get("page") {
        // The page setup (WordCraft's section properties); pageSize above is the simple form.
        match serde_json::from_value::<SectionProps>(page.clone()) {
            Ok(sp) => doc.last_section = sp,
            Err(e) => report.warnings.push(format!("page setup can't be read ({e}); the default is used")),
        }
    }
    if let Some(title) = wd.get("title").and_then(Value::as_str) {
        doc.core.title = title.to_string();
    }
    let text_width =
        doc.last_section.page_w - doc.last_section.margin_left - doc.last_section.margin_right;
    // Lists: each definition becomes a numbering instance, named by its id so an import can
    // recognise it.
    let mut lists: HashMap<String, u32> = HashMap::new();
    for (id, def) in wd.get("lists").and_then(Value::as_object).into_iter().flatten() {
        let levels: Vec<Level> = def
            .get("levels")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        match doc.numbering.try_add_levels(Some(id.clone()), levels) {
            Ok(num) if def.get("levels").is_some() => {
                lists.insert(id.clone(), num);
            }
            _ => report.warnings.push(format!("list `{id}`: its definition can't be read; its paragraphs export without numbering")),
        }
    }
    // Headers and footers, by which pages they're for.
    for (key, kind) in [("headers", PartKind::Header), ("footers", PartKind::Footer)] {
        let Some(set) = wd.get(key).and_then(Value::as_object) else { continue };
        for (which, paras) in set {
            let mut blocks = Vec::new();
            for wp in paras.as_array().into_iter().flatten().filter_map(Value::as_object) {
                blocks.push(para_block(para_from(&doc, wp, None, &format!("{key}.{which}"), &lists, &mut pics, &mut report)?));
            }
            let id = doc.add_part(kind, blocks);
            let hs = match kind {
                PartKind::Header => &mut doc.last_section.headers,
                _ => &mut doc.last_section.footers,
            };
            match which.as_str() {
                "default" => hs.default = Some(id),
                "first" => hs.first = Some(id),
                "even" => hs.even = Some(id),
                other => report.warnings.push(format!("{key}: `{other}` isn't a page kind (default, first, even); skipped")),
            }
        }
    }
    let seps = wd.get("noteSeparators");
    for (k, target) in [("footnote", &mut doc.footnote_separators), ("endnote", &mut doc.endnote_separators)] {
        if let Some(v) = seps.and_then(|s| s.get(k)) {
            match serde_json::from_value(v.clone()) {
                Ok(ns) => *target = ns,
                Err(e) => report.warnings.push(format!("{k} separators can't be read ({e}); the default line is used")),
            }
        }
    }
    doc.last_section.title_page = wd.get("titlePage").and_then(Value::as_bool) == Some(true);
    doc.settings.even_odd_headers = wd.get("evenOddHeaders").and_then(Value::as_bool) == Some(true);
    for child in &node.children {
        if child.attributes.contains_key("word::paragraph") {
            let p = paragraph_block(&doc, child, &lists, &mut pics, &mut report)?;
            doc.body.push(para_block(p));
        } else if child.attributes.contains_key("office::data::table") {
            for b in table_block(&doc, child, text_width, &mut report)? {
                doc.body.push(Arc::new(b));
            }
        } else {
            report.warnings.push(format!(
                "{} ({}): no Word representation yet; skipped",
                child.path,
                child.name.as_deref().unwrap_or("")
            ));
        }
    }
    if doc.body.is_empty() {
        doc.body.push(para_block(Paragraph::new()));
    }
    let prop = |name: &str, value: String| CustomProp {
        name: name.to_string(),
        kind: "lpwstr".into(),
        value,
    };
    doc.custom_props.retain(|p| !p.name.starts_with("ox."));
    doc.custom_props
        .push(prop(PROP_DOCUMENT, node.path.clone()));
    doc.custom_props
        .push(prop(PROP_REVISION, revision_of(node)));
    doc.custom_props.push(prop(PROP_LAYERS, layers.join(" ")));
    doc.balance_controls();
    doc.media.extend(pics.media);
    Ok((doc, report))
}
