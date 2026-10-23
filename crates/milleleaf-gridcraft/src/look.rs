//! How a sheet looks beyond its values, as `grid::sheet` attributes, and the workbook's own
//! defined names as `grid::workbook.names`:
//!
//! - `merges`: `["A1:C1", …]`
//! - `freeze`: `{rows, cols}` frozen at the top and left
//! - `comments`: `{"B4": <GridCraft Comment>}`
//! - `validations`: `[<GridCraft Validation>]` (drop-downs and input rules)
//! - `rows`: `{"6": <LineInfo>}` (1-based row numbers) and `cols`: `{"B": <LineInfo>}`: sizes,
//!   hidden lines, outline levels. Rows only when their height is custom.
//! - `defaults`: `{colWidth, rowHeight}`, the sheet's default sizes, when not GridCraft's
//! - `view`: what differs from a plain visible sheet: `hidden` (`"hidden"` / `"veryHidden"`),
//!   `gridlines`, `headings`, `zeros` (false when off), `zoom`, `rightToLeft`, `tabColor`
//! - `protection`: `<GridCraft SheetProtection>` (a password's hash, never the password)
//! - `autoFilter`: `<GridCraft AutoFilter>`, the sheet's filter buttons and any criteria
//! - `print`: `<GridCraft PrintSettings>` (page setup, print area, print titles, the sheet's
//!   printed header and footer), when not the default
//! - `pictures`: `[{blob, mediaType, alt?, at: "B2", dx, dy, w, h, mode?}]`, floating pictures
//!   (offsets and size in points; `mode` GridCraft's `AnchorMode` when not move-and-size)
//! - `charts`: `[<GridCraft Chart>]`; their series point at cells (`'Sheet 1'!$B$2:$B$9`), so a
//!   chart follows its data. An adopted chart also has `xml: {blob, was}`, its own XML (`crate::kept`)
//!
//! GridCraft's own types are kept as data (its serde form) where they carry many options.

use gridcraft_core::{CellRef, RangeRef};
use gridcraft_model::{Anchor, AnchorMode, Chart, BorderLine, BorderStyle, Borders, Color, Comment, DefinedName, Image, LineInfo, PrintSettings, Sheet, Style, StyleId, Validation, Visibility, Workbook};
use serde_json::{Map, Value, json};

use crate::NAME_PREFIX;

fn col_letters(c: u32) -> String {
    CellRef::new(0, c).a1().trim_end_matches(|ch: char| ch.is_ascii_digit()).to_string()
}

fn line(l: &LineInfo) -> Option<Value> {
    let l = LineInfo { style: None, ..*l };
    serde_json::to_value(l).ok()
}

pub(crate) const KEYS: [&str; 13] =
    ["merges", "freeze", "comments", "validations", "rows", "cols", "defaults", "print", "view", "protection", "autoFilter", "pictures", "charts"];

/// Pictures by blob value: (media type, bytes).
pub type Pictures = std::collections::BTreeMap<String, (String, Vec<u8>)>;

/// The sheet's look as `grid::sheet` attributes (only the ones it has); its pictures' bytes go
/// in `pics`.
pub(crate) fn sheet_look(sh: &Sheet, pics: &mut Pictures) -> Map<String, Value> {
    let mut m = Map::new();
    let r2 = |x: f32| ((x as f64) * 100.0).round() / 100.0;
    let pictures: Vec<Value> = sh
        .images
        .iter()
        .map(|im| {
            let blob = milleleaf_core::blob_of(&im.data);
            let mut p = json!({
                "blob": blob, "mediaType": im.mime, "at": im.anchor.cell.a1(),
                "dx": r2(im.anchor.dx), "dy": r2(im.anchor.dy), "w": r2(im.anchor.width), "h": r2(im.anchor.height),
            });
            if !im.alt.is_empty() {
                p["alt"] = json!(im.alt);
            }
            if im.anchor.mode != AnchorMode::default() {
                p["mode"] = serde_json::to_value(im.anchor.mode).unwrap_or_default();
            }
            if !im.data.starts_with(milleleaf_core::STAND_IN) {
                pics.insert(blob, (im.mime.clone(), im.data.clone()));
            }
            p
        })
        .collect();
    if !pictures.is_empty() {
        m.insert("pictures".into(), Value::Array(pictures));
    }
    if !sh.charts.is_empty() {
        m.insert("charts".into(), json!(sh.charts));
    }
    if !sh.merges.is_empty() {
        m.insert("merges".into(), json!(sh.merges.iter().map(|r| r.a1()).collect::<Vec<_>>()));
    }
    if let Some((rows, cols)) = sh.freeze.filter(|f| *f != (0, 0)) {
        m.insert("freeze".into(), json!({ "rows": rows, "cols": cols }));
    }
    let comments: Map<String, Value> = sh
        .comments
        .iter()
        .filter_map(|(at, c)| Some((at.a1(), serde_json::to_value(c).ok()?)))
        .collect();
    if !comments.is_empty() {
        m.insert("comments".into(), Value::Object(comments));
    }
    if !sh.validations.is_empty()
        && let Ok(v) = serde_json::to_value(&sh.validations) {
            m.insert("validations".into(), v);
        }
    let rows: Map<String, Value> = sh
        .rows
        .iter()
        .filter(|(_, l)| l.custom || l.hidden || l.outline > 0)
        .filter_map(|(r, l)| Some(((r + 1).to_string(), line(l)?)))
        .collect();
    if !rows.is_empty() {
        m.insert("rows".into(), Value::Object(rows));
    }
    let cols: Map<String, Value> = sh
        .cols
        .iter()
        .filter(|(_, l)| l.size.is_some() || l.hidden || l.outline > 0)
        .filter_map(|(c, l)| Some((col_letters(*c), line(l)?)))
        .collect();
    if !cols.is_empty() {
        m.insert("cols".into(), Value::Object(cols));
    }
    let blank = Sheet::new("");
    let mut view = Map::new();
    match sh.visibility {
        Visibility::Hidden => {
            view.insert("hidden".into(), json!("hidden"));
        }
        Visibility::VeryHidden => {
            view.insert("hidden".into(), json!("veryHidden"));
        }
        Visibility::Visible => {}
    }
    for (k, on, was) in [
        ("gridlines", sh.show_gridlines, blank.show_gridlines),
        ("headings", sh.show_headings, blank.show_headings),
        ("zeros", sh.show_zeros, blank.show_zeros),
        ("rightToLeft", sh.right_to_left, blank.right_to_left),
    ] {
        if on != was {
            view.insert(k.into(), json!(on));
        }
    }
    if sh.zoom != blank.zoom {
        view.insert("zoom".into(), json!(sh.zoom));
    }
    if let Some(c) = sh.tab_color.as_ref().and_then(|c| serde_json::to_value(c).ok()) {
        view.insert("tabColor".into(), c);
    }
    if !view.is_empty() {
        m.insert("view".into(), Value::Object(view));
    }
    if let Some(p) = sh.protection.as_ref().and_then(|p| serde_json::to_value(p).ok()) {
        m.insert("protection".into(), p);
    }
    if let Some(f) = sh.autofilter.as_ref().and_then(|f| serde_json::to_value(f).ok()) {
        m.insert("autoFilter".into(), f);
    }
    if (sh.default_col_width, sh.default_row_height) != (blank.default_col_width, blank.default_row_height) {
        m.insert("defaults".into(), json!({ "colWidth": sh.default_col_width, "rowHeight": sh.default_row_height }));
    }
    if sh.print != PrintSettings::default()
        && let Ok(v) = serde_json::to_value(&sh.print) {
            m.insert("print".into(), v);
        }
    m
}

/// Applies `grid::sheet`'s look attributes to a sheet; what can't be read is a warning. Pictures'
/// bytes come from `assets` (see `milleleaf_core::PictureSource`); a missing one is an error, since a
/// workbook with a hole in it is worse than none.
pub(crate) fn apply_look(sh: &mut Sheet, gs: &Map<String, Value>, at: &str, warnings: &mut Vec<String>, assets: milleleaf_core::PictureSource) -> Result<(), crate::GridError> {
    for (i, p) in gs.get("pictures").and_then(Value::as_array).into_iter().flatten().enumerate() {
        let blob = p.get("blob").and_then(Value::as_str).unwrap_or("");
        let data = milleleaf_core::picture_bytes(assets, blob).map_err(|e| crate::GridError::MissingAsset(format!("{at}: can't read {blob}: {e}")))?;
        let n = |k: &str| p.get(k).and_then(Value::as_f64).unwrap_or(0.0) as f32;
        let Some(cell) = p.get("at").and_then(Value::as_str).and_then(CellRef::parse) else {
            warnings.push(format!("{at}: picture {} has no cell to sit at; skipped", i + 1));
            continue;
        };
        let mode = p.get("mode").and_then(|m| serde_json::from_value(m.clone()).ok()).unwrap_or_default();
        sh.images.push(Image {
            id: i as u32 + 1,
            anchor: Anchor { cell, dx: n("dx"), dy: n("dy"), width: n("w"), height: n("h"), mode },
            mime: p.get("mediaType").and_then(Value::as_str).map(String::from).unwrap_or_else(|| milleleaf_core::media_type_for(blob).into()),
            alt: p.get("alt").and_then(Value::as_str).unwrap_or("").to_string(),
            data,
        });
    }
    for (i, c) in gs.get("charts").and_then(Value::as_array).into_iter().flatten().enumerate() {
        match serde_json::from_value::<Chart>(c.clone()) {
            Ok(c) => sh.charts.push(c),
            Err(e) => warnings.push(format!("{at}: chart {} can't be read ({e}); skipped", i + 1)),
        }
    }
    let mut bad = |what: &str, v: &dyn std::fmt::Display| warnings.push(format!("{at}: {what} `{v}` can't be read; skipped"));
    for r in gs.get("merges").and_then(Value::as_array).into_iter().flatten() {
        match r.as_str().and_then(RangeRef::parse) {
            Some(r) => sh.merges.push(r),
            None => bad("merged range", r),
        }
    }
    if let Some(f) = gs.get("freeze") {
        let g = |k: &str| f.get(k).and_then(Value::as_u64).unwrap_or(0) as u32;
        sh.freeze = Some((g("rows"), g("cols")));
    }
    for (a1, c) in gs.get("comments").and_then(Value::as_object).into_iter().flatten() {
        match (CellRef::parse(a1), serde_json::from_value::<Comment>(c.clone())) {
            (Some(at), Ok(c)) => {
                sh.comments.insert(at, c);
            }
            _ => bad("comment at", a1),
        }
    }
    if let Some(v) = gs.get("validations") {
        match serde_json::from_value::<Vec<Validation>>(v.clone()) {
            Ok(v) => sh.validations.extend(v),
            Err(e) => bad("validations", &e),
        }
    }
    for (r, l) in gs.get("rows").and_then(Value::as_object).into_iter().flatten() {
        match (r.parse::<u32>().ok().filter(|r| *r > 0), serde_json::from_value::<LineInfo>(l.clone())) {
            (Some(r), Ok(l)) => {
                sh.rows.insert(r - 1, l);
            }
            _ => bad("row", r),
        }
    }
    if let Some(v) = gs.get("view").and_then(Value::as_object) {
        sh.visibility = match v.get("hidden").and_then(Value::as_str) {
            Some("veryHidden") => Visibility::VeryHidden,
            Some(_) => Visibility::Hidden,
            None => Visibility::Visible,
        };
        let flag = |k: &str| v.get(k).and_then(Value::as_bool);
        if let Some(b) = flag("gridlines") {
            sh.show_gridlines = b;
        }
        if let Some(b) = flag("headings") {
            sh.show_headings = b;
        }
        if let Some(b) = flag("zeros") {
            sh.show_zeros = b;
        }
        if let Some(b) = flag("rightToLeft") {
            sh.right_to_left = b;
        }
        if let Some(z) = v.get("zoom").and_then(Value::as_u64) {
            sh.zoom = z as u16;
        }
        sh.tab_color = v.get("tabColor").and_then(|c| serde_json::from_value(c.clone()).ok());
    }
    if let Some(p) = gs.get("protection") {
        match serde_json::from_value(p.clone()) {
            Ok(p) => sh.protection = Some(p),
            Err(e) => bad("sheet protection", &e),
        }
    }
    if let Some(f) = gs.get("autoFilter") {
        match serde_json::from_value(f.clone()) {
            Ok(f) => sh.autofilter = Some(f),
            Err(e) => bad("filter", &e),
        }
    }
    if let Some(d) = gs.get("defaults") {
        if let Some(w) = d.get("colWidth").and_then(Value::as_f64) {
            sh.default_col_width = w as f32;
        }
        if let Some(h) = d.get("rowHeight").and_then(Value::as_f64) {
            sh.default_row_height = h as f32;
        }
    }
    if let Some(p) = gs.get("print") {
        match serde_json::from_value::<PrintSettings>(p.clone()) {
            Ok(p) => sh.print = p,
            Err(e) => bad("print settings", &e),
        }
    }
    for (c, l) in gs.get("cols").and_then(Value::as_object).into_iter().flatten() {
        match (CellRef::parse(&format!("{c}1")), serde_json::from_value::<LineInfo>(l.clone())) {
            (Some(at), Ok(l)) => {
                sh.cols.insert(at.col, l);
            }
            _ => bad("column", c),
        }
    }
    Ok(())
}

/// The workbook's own defined names (not Milleleaf's `mleaf_` ones): `{name, formula, scope?,
/// comment?, hidden?}`, `scope` being a sheet name.
pub(crate) fn names(wb: &Workbook) -> Vec<Value> {
    wb.names
        .iter()
        .filter(|n| !n.name.starts_with(NAME_PREFIX))
        .map(|n| {
            let mut m = Map::new();
            m.insert("name".into(), json!(n.name));
            m.insert("formula".into(), json!(n.formula));
            if let Some(s) = n.scope.and_then(|i| wb.sheets.get(i)) {
                m.insert("scope".into(), json!(s.name));
            }
            if !n.comment.is_empty() {
                m.insert("comment".into(), json!(n.comment));
            }
            if n.hidden {
                m.insert("hidden".into(), json!(true));
            }
            Value::Object(m)
        })
        .collect()
}

/// Adds `grid::workbook.names` to a workbook whose sheets are in place.
pub(crate) fn apply_names(wb: &mut Workbook, names: Option<&Value>, warnings: &mut Vec<String>) {
    for n in names.and_then(Value::as_array).into_iter().flatten() {
        let s = |k: &str| n.get(k).and_then(Value::as_str);
        let (Some(name), Some(formula)) = (s("name"), s("formula")) else {
            warnings.push(format!("defined name `{n}` can't be read; skipped"));
            continue;
        };
        // A name into another workbook needs the external link, which isn't carried: written
        // without it, Excel calls the file damaged and removes the name.
        if external_ref(formula) {
            warnings.push(format!("defined name {name} points into another workbook (external links aren't carried yet); left out"));
            continue;
        }
        let scope = match s("scope") {
            Some(sheet) => match wb.sheets.iter().position(|x| x.name == sheet) {
                Some(i) => Some(i),
                None => {
                    warnings.push(format!("defined name {name}: sheet `{sheet}` isn't in the workbook; skipped"));
                    continue;
                }
            },
            None => None,
        };
        wb.names.push(DefinedName {
            name: name.to_string(),
            scope,
            formula: formula.to_string(),
            comment: s("comment").unwrap_or("").to_string(),
            hidden: n.get("hidden").and_then(Value::as_bool) == Some(true),
        });
    }
}

/// The cell styles of a workbook as data: the styles cells use (GridCraft's form) and, per sheet,
/// which cells use each, as `{"<index>": ["A1:D1", "F5", …]}` runs along rows. Cells in the
/// default style aren't listed; the default style itself is `normal`.
pub(crate) fn cell_styles(wb: &Workbook) -> (Value, Vec<Map<String, Value>>) {
    let mut table: Vec<Value> = Vec::new();
    let mut index: std::collections::HashMap<StyleId, usize> = std::collections::HashMap::new();
    let mut maps = Vec::new();
    for sh in &wb.sheets {
        let mut cells: Vec<(CellRef, StyleId)> = sh
            .cells
            .iter()
            .filter(|(_, c)| c.style != StyleId::DEFAULT)
            .map(|(at, c)| (at, c.style))
            .collect();
        cells.sort_by_key(|(at, _)| (at.row, at.col));
        let mut by: std::collections::BTreeMap<usize, Vec<Value>> = std::collections::BTreeMap::new();
        let mut i = 0;
        while i < cells.len() {
            let (start, sid) = cells[i];
            let mut end = start;
            let mut j = i + 1;
            while j < cells.len() && cells[j].0.row == start.row && cells[j].0.col == end.col + 1 && cells[j].1 == sid {
                end = cells[j].0;
                j += 1;
            }
            let idx = *index.entry(sid).or_insert_with(|| {
                table.push(serde_json::to_value(wb.styles.get(sid)).unwrap_or_default());
                table.len() - 1
            });
            let a1 = if start == end { start.a1() } else { RangeRef::new(start, end).a1() };
            by.entry(idx).or_default().push(json!(a1));
            i = j;
        }
        maps.push(by.into_iter().map(|(k, v)| (k.to_string(), Value::Array(v))).collect());
    }
    (Value::Array(table), maps)
}

/// The workbook's default (Normal) cell style, when it isn't GridCraft's.
pub(crate) fn normal_style(wb: &Workbook) -> Option<Value> {
    let normal = wb.styles.get(StyleId::DEFAULT);
    (*normal != *Workbook::new().styles.get(StyleId::DEFAULT)).then(|| serde_json::to_value(normal).ok()).flatten()
}

/// The styles of `grid::workbook.styles`, read once for every sheet.
pub(crate) fn read_styles(v: Option<&Value>, warnings: &mut Vec<String>) -> Vec<Option<Style>> {
    v.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(i, s)| match serde_json::from_value(s.clone()) {
            Ok(s) => Some(s),
            Err(e) => {
                warnings.push(format!("cell style {i} can't be read ({e}); its cells keep Milleleaf's"));
                None
            }
        })
        .collect()
}

/// Applies a sheet's `cellStyles` map: each listed cell takes its style (cells with no value yet
/// are created, so a filled or bordered empty cell keeps its look).
///
/// When the workbook carries its own styles (`styles` not empty) the map is the whole story: a
/// cell it doesn't list is in the default style, not in the one Milleleaf would give its value.
pub(crate) fn apply_cell_styles(wb: &mut Workbook, si: usize, map: Option<&Value>, styles: &[Option<Style>], at: &str, warnings: &mut Vec<String>) {
    if !styles.is_empty()
        && let Some(sh) = wb.sheet_mut(si) {
            let styled: Vec<CellRef> = sh.cells.iter().filter(|(_, c)| c.style != StyleId::DEFAULT).map(|(at, _)| at).collect();
            for c in styled {
                let mut cell = sh.cell(c).cloned().unwrap_or_default();
                cell.style = StyleId::DEFAULT;
                sh.set_cell(c, cell);
            }
        }
    for (idx, ranges) in map.and_then(Value::as_object).into_iter().flatten() {
        let Some(style) = idx.parse::<usize>().ok().and_then(|i| styles.get(i)).and_then(Option::clone) else {
            warnings.push(format!("{at}: cell style `{idx}` isn't in the workbook's styles; skipped"));
            continue;
        };
        let sid = wb.styles.intern(style);
        let Some(sh) = wb.sheet_mut(si) else { return };
        for r in ranges.as_array().into_iter().flatten() {
            let Some(r) = r.as_str().and_then(|r| RangeRef::parse(r).or_else(|| CellRef::parse(r).map(RangeRef::cell))) else {
                warnings.push(format!("{at}: styled range `{r}` can't be read; skipped"));
                continue;
            };
            for row in r.start.row..=r.end.row {
                for col in r.start.col..=r.end.col {
                    let c = CellRef::new(row, col);
                    let mut cell = sh.cell(c).cloned().unwrap_or_default();
                    cell.style = sid;
                    sh.set_cell(c, cell);
                }
            }
        }
    }
}

/// What each styled cell of a sheet looks like (its style as data), for comparing two workbooks.
pub(crate) fn cell_looks(wb: &Workbook, si: usize) -> std::collections::BTreeMap<String, Value> {
    let Some(sh) = wb.sheets.get(si) else { return Default::default() };
    let normal = wb.styles.get(StyleId::DEFAULT);
    // Cells a merge covers aren't seen (LibreOffice gives them the first cell's style).
    let covered = |at: CellRef| sh.merges.iter().any(|m| m.contains(at) && m.start != at);
    // What a cell with no style of its own looks like, the same way: a cell whose style differs
    // from the normal one only in what nobody sees (LibreOffice writes such cells, with a style of
    // its own, over a whole column) looks unstyled.
    let unstyled = unstyled_look(wb);
    sh.cells
        .iter()
        .filter(|(at, c)| wb.styles.get(c.style) != normal && !covered(*at))
        .filter_map(|(at, c)| {
            let mut st = wb.styles.get(c.style).clone();
            // Colours as seen (a theme colour and its RGB are the same colour).
            let seen = |c: &mut Color| {
                if let Some([r, g, b]) = c.resolve(&wb.theme) {
                    *c = Color::rgb(r, g, b);
                }
            };
            seen(&mut st.font.color);
            seen(&mut st.fill.fg);
            seen(&mut st.fill.bg);
            // A shared edge is drawn by either cell: LibreOffice copies a neighbour's border
            // into an empty side.
            // A merged cell's sides are drawn from its whole range: either the first cell or
            // the cells along that edge may carry them.
            if let Some(m) = sh.merges.iter().find(|m| m.start == at) {
                let edge = |cells: Vec<CellRef>, pick: fn(&Borders) -> &BorderLine| {
                    cells.into_iter().filter_map(|c| sh.cell(c)).map(|n| *pick(&wb.styles.get(n.style).borders)).find(|l| l.style != BorderStyle::None)
                };
                let rows = || m.start.row..=m.end.row;
                let cols = || m.start.col..=m.end.col;
                let b = &mut st.borders;
                for (line, n, inside) in [
                    (&mut b.top, edge(cols().map(|c| CellRef::new(m.start.row, c)).collect(), |b| &b.top), false),
                    (&mut b.bottom, edge(cols().map(|c| CellRef::new(m.end.row, c)).collect(), |b| &b.bottom), m.end.row > m.start.row),
                    (&mut b.left, edge(rows().map(|r| CellRef::new(r, m.start.col)).collect(), |b| &b.left), false),
                    (&mut b.right, edge(rows().map(|r| CellRef::new(r, m.end.col)).collect(), |b| &b.right), m.end.col > m.start.col),
                ] {
                    // The first cell's own bottom or right is inside the merge when it spans more
                    // rows or columns: the edge cells' line is the one seen.
                    if inside {
                        *line = n.unwrap_or_default();
                    } else if line.style == BorderStyle::None
                        && let Some(n) = n
                    {
                        *line = n;
                    }
                }
            }
            let (end_r, end_c) = sh.merges.iter().find(|m| m.start == at).map_or((at.row, at.col), |m| (m.end.row, m.end.col));
            let side = |r: i64, c: i64, pick: fn(&Borders) -> &BorderLine| {
                if r < 0 || c < 0 {
                    return None;
                }
                sh.cell(CellRef::new(r as u32, c as u32)).map(|n| *pick(&wb.styles.get(n.style).borders))
            };
            let (r0, c0, r1, c1) = (at.row as i64, at.col as i64, end_r as i64, end_c as i64);
            let b = &mut st.borders;
            // A shared edge shows the line of the cell below or to the right when it has one
            // (what LibreOffice keeps when the two cells disagree), else the other cell's.
            let (above, below, before, after) = (
                side(r0 - 1, c0, |b| &b.bottom),
                side(r1 + 1, c0, |b| &b.top),
                side(r0, c0 - 1, |b| &b.right),
                side(r0, c1 + 1, |b| &b.left),
            );
            for (line, n, theirs_wins) in [(&mut b.top, above, false), (&mut b.left, before, false), (&mut b.bottom, below, true), (&mut b.right, after, true)] {
                if let Some(n) = n
                    && n.style != BorderStyle::None
                    && (theirs_wins || line.style == BorderStyle::None)
                {
                    *line = n;
                }
            }
            for l in [&mut b.top, &mut b.bottom, &mut b.left, &mut b.right, &mut b.diag_down, &mut b.diag_up] {
                seen(&mut l.color);
            }
            let mut v = finish_look(serde_json::to_value(&st).unwrap_or_default());
            // An empty cell shows no number, so its number format isn't seen.
            if c.value.is_empty() && c.formula.is_none() {
                v["num_fmt"] = unstyled["num_fmt"].clone();
            }
            (v != unstyled).then(|| (at.a1(), v))
        })
        .collect()
}

/// What a cell with no style of its own looks like, as [`cell_looks`] writes looks.
pub(crate) fn unstyled_look(wb: &Workbook) -> Value {
    let mut st = wb.styles.get(StyleId::DEFAULT).clone();
    for c in [&mut st.font.color, &mut st.fill.fg, &mut st.fill.bg] {
        if let Some([r, g, b]) = c.resolve(&wb.theme) {
            *c = Color::rgb(r, g, b);
        }
    }
    finish_look(serde_json::to_value(&st).unwrap_or_default())
}

/// The last of [`cell_looks`]'s normalising: what two spellings of one look share.
fn finish_look(mut v: Value) -> Value {
    // A solid fill shows only its foreground colour.
    if let Some(f) = v.get_mut("fill").and_then(Value::as_object_mut)
        && f.get("pattern").and_then(Value::as_str) == Some("Solid")
    {
        f.remove("bg");
    }
    // `"$"#,##0` and `\$#,##0` are the same format: editors pick either spelling.
    if let Some(Value::String(f)) = v.get_mut("num_fmt") {
        f.retain(|ch| ch != '"' && ch != '\\');
    }
    v
}

/// The workbook's structure lock (sheets can't be added, moved or unhidden) and which sheet is
/// active, as `grid::workbook` attributes.
pub(crate) fn workbook_look(wb: &Workbook) -> Map<String, Value> {
    let mut m = Map::new();
    if wb.protected_structure {
        let mut p = Map::from_iter([("structure".to_string(), json!(true))]);
        if let Some(h) = wb.structure_password.as_ref().and_then(|h| serde_json::to_value(h).ok()) {
            p.insert("password".into(), h);
        }
        m.insert("protection".into(), Value::Object(p));
    }
    if wb.active_sheet != 0 {
        m.insert("activeSheet".into(), json!(wb.active_sheet));
    }
    // The workbook's theme (its colours and fonts), when not GridCraft's own: themed fills, fonts
    // and chart colours mean those colours.
    if wb.theme != gridcraft_model::Theme::default() {
        m.insert("theme".into(), json!(wb.theme));
    }
    m
}

pub(crate) fn apply_workbook_look(wb: &mut Workbook, gw: &Map<String, Value>, warnings: &mut Vec<String>) {
    if let Some(p) = gw.get("protection") {
        wb.protected_structure = p.get("structure").and_then(Value::as_bool) == Some(true);
        if let Some(h) = p.get("password") {
            match serde_json::from_value(h.clone()) {
                Ok(h) => wb.structure_password = Some(h),
                Err(e) => warnings.push(format!("the workbook's protection password hash can't be read ({e}); protected without one")),
            }
        }
    }
    if let Some(a) = gw.get("activeSheet").and_then(Value::as_u64) {
        wb.active_sheet = (a as usize).min(wb.sheets.len().saturating_sub(1));
    }
    if let Some(t) = gw.get("theme") {
        match serde_json::from_value(t.clone()) {
            Ok(t) => wb.theme = t,
            Err(e) => warnings.push(format!("the workbook's theme can't be read ({e}); GridCraft's is used")),
        }
    }
}

/// Puts Milleleaf's own value back in `now` (the look of a sheet an editor saved) wherever it
/// differs from `was` (Milleleaf's export, read back) only by a rewrite editors make on every
/// save, so that only real edits differ. Seen with Excel 16 (Mac): automatic row heights dropped,
/// trailing spaces trimmed from headers and footers. With LibreOffice 24.2: rows at the default height
/// written as custom rows and heights moved by a pixel; hidden columns' widths dropped; comment
/// authors replaced ("Unknown Author"); header/footer codes re-escaped (`_x005F_`), font codes
/// dropped and colours lower-cased; the scale rewritten when the sheet fits to pages; margins
/// rounded; a header or footer distance reset to 13 mm (tdf#117229) when there is no header or
/// footer; default sizes moved by rounding.
/// When `libreoffice` saved the file, also: rows whose height was automatic, which it re-fits
/// and saves as custom heights. Outline `collapsed` markers and a list
/// validation's unused second value are never compared.
pub(crate) fn settle(now: &mut Map<String, Value>, was: &Map<String, Value>, libreoffice: bool) {
    let blank = Sheet::new("");
    let close = |a: Option<f64>, b: Option<f64>, tol: f64| match (a, b) {
        (Some(a), Some(b)) => (a - b).abs() <= tol,
        (a, b) => a == b,
    };
    let num = |m: &Map<String, Value>, k: &str, key: &str, fallback: f64| {
        m.get(k).and_then(|d| d.get(key)).and_then(Value::as_f64).unwrap_or(fallback)
    };
    let (dw, dh) = (blank.default_col_width as f64, blank.default_row_height as f64);
    let defaults = |m: &Map<String, Value>| (num(m, "defaults", "colWidth", dw), num(m, "defaults", "rowHeight", dh));
    let (now_d, was_d) = (defaults(now), defaults(was));
    // Pictures: the same pictures in the same cells, placed and sized within a point. A picture's
    // width is worked out from the columns it spans, and every editor writes its own column widths
    // (Excel 2.5% and LibreOffice 4% narrower on the real workbook, anchors unchanged): width and
    // left offset compare within 5% (a resize keeps the aspect ratio, so it still shows in the
    // height).
    if let (Some(Value::Array(n)), Some(Value::Array(w))) = (now.get("pictures"), was.get("pictures")) {
        let f = |p: &Value, k: &str| p.get(k).and_then(Value::as_f64);
        let same = n.len() == w.len()
            && n.iter().zip(w).all(|(a, b)| {
                ["blob", "at", "alt", "mode"].iter().all(|k| a.get(*k) == b.get(*k))
                    && ["dy", "h"].iter().all(|k| close(f(a, k), f(b, k), 1.0))
                    && ["dx", "w"].iter().all(|k| {
                        close(f(a, k), f(b, k), (f(b, k).unwrap_or(0.0).abs() * 0.05).max(1.0))
                    })
            });
        if same {
            now.insert("pictures".into(), Value::Array(w.clone()));
        }
    }
    // Merges in another order; a list's items with the spaces after commas removed.
    if let (Some(Value::Array(n)), Some(Value::Array(w))) = (now.get("merges"), was.get("merges")) {
        let sorted = |a: &Vec<Value>| {
            let mut v: Vec<String> = a.iter().map(|x| x.to_string()).collect();
            v.sort();
            v
        };
        if sorted(n) == sorted(w) {
            now.insert("merges".into(), Value::Array(w.clone()));
        }
    }
    if let (Some(n), Some(w)) = (now.get("validations"), was.get("validations")) {
        // A list's second value means nothing (LibreOffice writes "0").
        let squeeze = |v: &Value| {
            let mut v = v.clone();
            for x in v.as_array_mut().into_iter().flatten() {
                if x.get("kind").and_then(Value::as_str) == Some("List") {
                    x["f2"] = Value::Null;
                }
                // The cells a rule covers, in any order.
                if let Some(r) = x.get_mut("ranges").and_then(Value::as_array_mut) {
                    r.sort_by_key(|r| r.to_string());
                }
            }
            v.to_string().replace(", ", ",")
        };
        if squeeze(n) == squeeze(w) {
            now.insert("validations".into(), w.clone());
        }
    }
    // Default sizes: Excel and LibreOffice write their own default column width (60–62 px for
    // GridCraft's 64), so only a change of more than 5 px (a user's Standard Width moves it by whole
    // characters, about 7 px each) is an edit.
    // LibreOffice works the default row height out from the default font (12.8 pt where Excel has
    // 15) and rewrites it on every save; its users can't set it, so it's never an edit.
    if close(Some(now_d.0), Some(was_d.0), 5.0) && (libreoffice || close(Some(now_d.1), Some(was_d.1), 1.0)) {
        match was.get("defaults") {
            Some(v) => now.insert("defaults".into(), v.clone()),
            None => now.remove("defaults"),
        };
    }

    // Rows and columns: the same if they look the same (a size within a pixel, hidden ones'
    // sizes not compared).
    for (k, default) in [("rows", was_d.1), ("cols", was_d.0)] {
        let empty = Map::new();
        let w = was.get(k).and_then(Value::as_object).unwrap_or(&empty);
        let mut n = now.get(k).and_then(Value::as_object).cloned().unwrap_or_default();
        let keys: Vec<String> = n.keys().chain(w.keys()).cloned().collect();
        let seen = |l: Option<&Value>| {
            let g = |f: &str| l.and_then(|l| l.get(f)).cloned().unwrap_or(Value::Null);
            let hidden = g("hidden").as_bool() == Some(true);
            let size = g("size").as_f64().unwrap_or(default);
            // `collapsed` only marks a group's summary line; the group's hidden lines show it.
            (hidden, g("outline").as_u64().unwrap_or(0), (), (!hidden).then_some(size))
        };
        for key in keys {
            let (a, b) = (seen(n.get(&key)), seen(w.get(&key)));
            // A height the application worked out (not custom) isn't an edit: Excel drops it,
            // LibreOffice re-fits it and saves it as custom.
            let custom = |l: Option<&Value>| l.and_then(|l| l.get("custom")).and_then(Value::as_bool) == Some(true);
            let auto = k == "rows" && !custom(w.get(&key)) && (libreoffice || !custom(n.get(&key)));
            if (a.0, a.1, a.2) == (b.0, b.1, b.2) && (auto || close(a.3, b.3, 1.0)) {
                match w.get(&key) {
                    Some(v) => n.insert(key, v.clone()),
                    None => n.remove(&key),
                };
            }
        }
        if n.is_empty() {
            now.remove(k);
        } else {
            now.insert(k.into(), Value::Object(n));
        }
    }

    // Comments: an author replaced while the comment itself is the same.
    if let (Some(Value::Object(n)), Some(Value::Object(w))) = (now.get_mut("comments"), was.get("comments")) {
        for (at, c) in n.iter_mut() {
            if let (Some(c), Some(wc)) = (c.as_object_mut(), w.get(at).and_then(Value::as_object)) {
                let mut same = c.clone();
                if let Some(a) = wc.get("author") {
                    same.insert("author".into(), a.clone());
                }
                if same == *wc {
                    *c = same;
                }
            }
        }
    }

    // Page setup.
    // A page setup at the default isn't in a look: compare with the default then.
    let default_print = || serde_json::to_value(PrintSettings::default()).unwrap_or_default();
    let w_print = was.get("print").cloned().unwrap_or_else(default_print);
    if let (Value::Object(n), Value::Object(w)) = (now.get("print").cloned().unwrap_or_else(default_print), &w_print) {
        let mut n = n;
        let hf = |s: &str| {
            // `_x005F_` escapes an underscore; `&"font,style"` picks a font; `&K` colours are hex.
            let s = s.replace("_x005F_", "_");
            let mut out = String::new();
            let mut it = s.chars().peekable();
            while let Some(ch) = it.next() {
                if ch == '&' && it.peek() == Some(&'"') {
                    it.next();
                    for c in it.by_ref() {
                        if c == '"' {
                            break;
                        }
                    }
                    continue;
                }
                out.push(ch);
            }
            out.trim_end().to_lowercase()
        };
        let texts = ["header", "footer", "first_header", "first_footer", "even_header", "even_footer"];
        for t in texts {
            if let (Some(a), Some(b)) = (n.get(t).and_then(Value::as_str), w.get(t).and_then(Value::as_str))
                && a != b && hf(a) == hf(b) {
                    n.insert(t.into(), json!(b));
                }
        }
        if n.get("fit_to").is_some_and(|f| !f.is_null()) && n.get("fit_to") == w.get("fit_to") {
            n.insert("scale".into(), w.get("scale").cloned().unwrap_or(Value::Null));
        }
        let empty_hf = |part: &str| {
            texts.iter().filter(|t| t.ends_with(part)).all(|t| n.get(*t).and_then(Value::as_str).is_none_or(str::is_empty))
        };
        let (no_header, no_footer) = (empty_hf("header"), empty_hf("footer"));
        if let (Some(Value::Array(nm)), Some(Value::Array(wm))) = (n.get("margins").cloned(), w.get("margins")) {
            let mut nm = nm;
            for (i, (a, b)) in nm.iter_mut().zip(wm).enumerate() {
                let (x, y) = (a.as_f64(), b.as_f64());
                let lo_default = (i == 4 && no_header || i == 5 && no_footer) && close(x, Some(1300.0 / 2540.0), 0.001);
                if close(x, y, 0.001) || lo_default {
                    *a = b.clone();
                }
            }
            n.insert("margins".into(), Value::Array(nm));
        }
        if Value::Object(n.clone()) == w_print && !was.contains_key("print") {
            now.remove("print");
        } else {
            now.insert("print".into(), Value::Object(n));
        }
    }
}

/// Charts' sizes in the rows and columns a [`settle`]d look keeps (`now`, the file's sheet `file`
/// read through it). A chart in a file sits between two cells, and its size is worked out from
/// the sizes of the rows and columns between them: when an editor rewrote those sizes and that
/// isn't an edit (a row height Excel worked out, its own default width), the chart's size isn't
/// one either. Seen on the real workbook's sheet "Heat map article": Excel 16's plain re-save
/// gave rows 7 onwards the height it works out itself (`ht="16"`, not custom; Milleleaf's export
/// has none, so 15) and left both charts' `xdr:from` and `xdr:to` cells as they were, which read
/// as 546.33 → 574.33 px. A size an edit changed (a custom height, a new default width) still
/// moves the chart's size, as the file shows it. Charts placed by position alone aren't touched,
/// and nor are LibreOffice's: it keeps a chart's size in its own rows and re-anchors it to them.
pub(crate) fn settle_chart_sizes(now: &mut Map<String, Value>, file: &Sheet) {
    let mut kept = Sheet::new("");
    let sizes: Map<String, Value> = ["rows", "cols", "defaults"].iter().filter_map(|k| Some((k.to_string(), now.get(*k)?.clone()))).collect();
    if apply_look(&mut kept, &sizes, "", &mut Vec::new(), milleleaf_core::PictureSource::StandIn(None)).is_err() {
        return;
    }
    let Some(Value::Array(charts)) = now.get_mut("charts") else { return };
    for c in charts {
        let Ok(mut ch) = serde_json::from_value::<Chart>(c.clone()) else { continue };
        let a = &mut ch.anchor;
        if a.mode == AnchorMode::Absolute {
            continue;
        }
        // Where the chart ends, in the file's own rows and columns.
        let (x1, y1) = (file.col_left(a.cell.col) + a.dx as f64, file.row_top(a.cell.row) + a.dy as f64);
        let (x2, y2) = (x1 + a.width as f64, y1 + a.height as f64);
        let (tc, tr) = (file.col_at(x2), file.row_at(y2));
        let (tdx, tdy) = (x2 - file.col_left(tc), y2 - file.row_top(tr));
        a.width = (kept.col_left(tc) + tdx - kept.col_left(a.cell.col) - a.dx as f64).max(0.0) as f32;
        a.height = (kept.row_top(tr) + tdy - kept.row_top(a.cell.row) - a.dy as f64).max(0.0) as f32;
        if let Ok(v) = serde_json::to_value(&ch) {
            *c = v;
        }
    }
}

/// A formula that refers to another workbook: `[1]Sheet!A1`, `'[2]My sheet'!B2`.
pub(crate) fn external_ref(f: &str) -> bool {
    f.split('[').skip(1).any(|r| r.split(']').next().is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())))
}

/// Whether a sheet's charts read back from an editor are the ones Milleleaf wrote. What isn't an
/// edit (seen on the real workbook's nine charts):
/// - GridCraft writes theme colours as RGB, and LibreOffice spells out an automatic one;
/// - GridCraft drops the sheet before `#REF!`; LibreOffice drops a series that only reads `#REF!`
///   and quotes a literal series name;
/// - every editor places a chart by its own row heights and column widths: Excel moves the size by a
///   few points, LibreOffice re-anchors it up to two rows away and stores it as move-only.
///
/// `theme` is the editor's workbook's.
pub(crate) fn same_charts(now: &Value, was: &Value, theme: &gridcraft_model::Theme, libreoffice: bool) -> bool {
    let (Ok(now), Ok(was)) = (serde_json::from_value::<Vec<Chart>>(now.clone()), serde_json::from_value::<Vec<Chart>>(was.clone())) else {
        return false;
    };
    let refs = |s: &str| if let Some(i) = s.find("!#REF!") { s[i + 1..].to_string() } else { s.to_string() };
    let rgb = |c: &Option<Color>| c.as_ref().and_then(|c| c.resolve(theme));
    let close = |a: f32, b: f32| (a - b).abs() <= (b.abs() * 0.05).max(5.0);
    let series = |c: &Chart| -> Vec<gridcraft_model::Series> {
        c.series.iter().filter(|s| !(libreoffice && refs(&s.values) == "#REF!")).cloned().collect()
    };
    now.len() == was.len()
        && now.iter().zip(&was).all(|(n, w)| {
            let (na, wa) = (&n.anchor, &w.anchor);
            let placed = na.cell.col == wa.cell.col
                && (na.cell.row == wa.cell.row || libreoffice && na.cell.row.abs_diff(wa.cell.row) <= 2)
                && (na.mode == wa.mode || libreoffice)
                && close(na.width, wa.width)
                && close(na.height, wa.height);
            let (ns, ws) = (series(n), series(w));
            // LibreOffice (24.2) gives a series without categories the chart's (another series'),
            // and writes theme colours as RGB with their shade applied, which GridCraft reads
            // without: series colours aren't compared in its files.
            let shared_cats = |c: &Option<String>| libreoffice && ws.iter().any(|s| s.categories.as_deref().map(refs) == c.as_deref().map(refs));
            let same_series = ns.len() == ws.len()
                && ns.iter().zip(&ws).all(|(a, b)| {
                    let name = |s: &gridcraft_model::Series| s.name.as_deref().map(|n| refs(n.trim_matches('"')));
                    name(a) == name(b)
                        && (a.categories.as_deref().map(refs) == b.categories.as_deref().map(refs) || b.categories.is_none() && shared_cats(&a.categories))
                        && refs(&a.values) == refs(&b.values)
                        && (b.color.is_none() || libreoffice || rgb(&a.color) == rgb(&b.color))
                        && (a.secondary, a.kind, a.smooth, &a.trendlines, &a.bubble_sizes) == (b.secondary, b.kind, b.smooth, &b.trendlines, &b.bubble_sizes)
                });
            fn rest(c: &Chart) -> impl PartialEq + '_ {
                (c.kind, c.legend, c.data_labels, c.gridlines, &c.x_title, &c.y_title, c.by_rows, &c.pivot)
            }
            // A chart without a title shows an automatic one (with one series, its name);
            // LibreOffice writes it out.
            let titled = n.title == w.title || libreoffice && w.title.is_none() && (n.title.as_deref() == Some("Chart Title") || ns.len() == 1);
            placed && same_series && titled && rest(n) == rest(w)
        })
}
