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
//! - `print`: `<GridCraft PrintSettings>` (page setup, print area, print titles, the sheet's
//!   printed header and footer), when not the default
//!
//! GridCraft's own types are kept as data (its serde form) where they carry many options.

use gridcraft_core::{CellRef, RangeRef};
use gridcraft_model::{Comment, DefinedName, LineInfo, PrintSettings, Sheet, Validation, Workbook};
use serde_json::{Map, Value, json};

use crate::NAME_PREFIX;

fn col_letters(c: u32) -> String {
    CellRef::new(0, c).a1().trim_end_matches(|ch: char| ch.is_ascii_digit()).to_string()
}

fn line(l: &LineInfo) -> Option<Value> {
    let l = LineInfo { style: None, ..*l };
    serde_json::to_value(l).ok()
}

pub(crate) const KEYS: [&str; 8] = ["merges", "freeze", "comments", "validations", "rows", "cols", "defaults", "print"];

/// The sheet's look as `grid::sheet` attributes (only the ones it has).
pub(crate) fn sheet_look(sh: &Sheet) -> Map<String, Value> {
    let mut m = Map::new();
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
    if !sh.validations.is_empty() {
        if let Ok(v) = serde_json::to_value(&sh.validations) {
            m.insert("validations".into(), v);
        }
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
    if (sh.default_col_width, sh.default_row_height) != (blank.default_col_width, blank.default_row_height) {
        m.insert("defaults".into(), json!({ "colWidth": sh.default_col_width, "rowHeight": sh.default_row_height }));
    }
    if sh.print != PrintSettings::default() {
        if let Ok(v) = serde_json::to_value(&sh.print) {
            m.insert("print".into(), v);
        }
    }
    m
}

/// Applies `grid::sheet`'s look attributes to a sheet; what can't be read is a warning.
pub(crate) fn apply_look(sh: &mut Sheet, gs: &Map<String, Value>, at: &str, warnings: &mut Vec<String>) {
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
}

/// The workbook's own defined names (not Milleleaf's `ox_` ones): `{name, formula, scope?,
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
