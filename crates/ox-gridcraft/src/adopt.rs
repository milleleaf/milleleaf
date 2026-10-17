//! Bringing a workbook Milleleaf didn't make into a project ([`adopt`]): each sheet becomes a
//! `grid::sheet` node and each non-empty cell an `office::data::value` at its `grid::range`, with
//! its `grid::formula`. Number formats become `currency` / `percent` / `number`. Merged cells,
//! frozen panes, comments, validation, row and column sizes, cell styles and the file's own names
//! floating pictures and charts are carried (`crate::look`).

use std::collections::BTreeMap;

use gridcraft_core::{CellRef, Value as GValue};
use gridcraft_model::Workbook;
use ox_core::model::{Header, Layer, Node, OX_VERSION, stable_id};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::GridError;
use crate::import::json_of;

#[derive(Debug, Default, Serialize)]
pub struct AdoptReport {
    pub sheets: usize,
    pub cells: usize,
    pub formulas: usize,
    /// What the file had that the adopted layer doesn't carry.
    pub warnings: Vec<String>,
    /// The bytes of every picture the layer names, by blob value, with their media type: the
    /// caller stores the ones its project doesn't have yet.
    #[serde(skip)]
    pub pictures: crate::Pictures,
}

fn format_of(fmt: &str) -> Option<&'static str> {
    if fmt.contains('%') {
        Some("percent")
    } else if ['$', '£', '€', '¥'].iter().any(|c| fmt.contains(*c)) {
        Some("currency")
    } else if fmt == "General" || fmt == "@" {
        None
    } else {
        Some("number")
    }
}

pub fn adopt(wb: &Workbook, path: &str, name: &str, header: Header) -> Result<(Layer, AdoptReport), GridError> {
    let mut report = AdoptReport::default();
    let mut nodes = Vec::new();
    let mut wb_children = BTreeMap::new();
    let mut wb_order = Vec::new();
    let (style_table, style_maps) = crate::look::cell_styles(wb);
    for (si, sh) in wb.sheets.iter().enumerate() {
        report.sheets += 1;
        let sheet_path = stable_id(&format!("{path}|xlsx-sheet|{}", sh.name));
        let sheet_name = format!("sheet-{}", &sheet_path[..8]);
        wb_children.insert(sheet_name.clone(), Some(sheet_path.clone()));
        wb_order.push(sheet_name);
        let mut cells: Vec<(CellRef, Value, Option<String>, Option<&str>)> = Vec::new();
        for (at, cell) in sh.cells.iter() {
            let formula = cell.formula.as_ref().map(|f| f.text.clone());
            if matches!(cell.value, GValue::Empty) && formula.is_none() {
                continue;
            }
            let a1 = at.a1();
            let v = json_of(&cell.value, &mut report.warnings, &format!("{}!{a1}", sh.name));
            let fmt = if v.is_number() { format_of(&wb.styles.get(cell.style).num_fmt.0) } else { None };
            cells.push((at, v, formula, fmt));
        }
        cells.sort_by_key(|(at, ..)| (at.row, at.col));
        let mut sheet_children = BTreeMap::new();
        let mut sheet_order = Vec::new();
        for (at, v, formula, fmt) in cells {
            let a1 = at.a1();
            let cell_path = stable_id(&format!("{sheet_path}|cell|{a1}"));
            sheet_children.insert(a1.clone(), Some(cell_path.clone()));
            sheet_order.push(a1.clone());
            let mut value = Map::new();
            value.insert("value".into(), v);
            if let Some(f) = fmt {
                value.insert("format".into(), json!(f));
            }
            let mut attributes = Map::from_iter([
                ("grid::range".to_string(), json!({ "sheet": sh.name, "ref": a1 })),
                ("office::data::value".to_string(), Value::Object(value)),
            ]);
            if let Some(f) = formula {
                attributes.insert("grid::formula".into(), json!({ "text": f }));
                report.formulas += 1;
            }
            report.cells += 1;
            nodes.push(Node { path: cell_path, attributes, ..Node::default() });
        }
        let mut gs = Map::from_iter([("name".to_string(), json!(sh.name))]);
        gs.extend(crate::look::sheet_look(sh, &mut report.pictures));
        if let Some(m) = style_maps.get(si).filter(|m| !m.is_empty()) {
            gs.insert("cellStyles".into(), Value::Object(m.clone()));
        }
        nodes.push(Node {
            path: sheet_path,
            attributes: Map::from_iter([("grid::sheet".to_string(), Value::Object(gs))]),
            children: sheet_children,
            order: Some(sheet_order),
            ..Node::default()
        });
    }
    nodes.insert(
        0,
        Node {
            path: path.to_string(),
            attributes: Map::from_iter([("grid::workbook".to_string(), {
                let mut gw = Map::from_iter([("name".to_string(), json!(name))]);
                let names = crate::look::names(wb);
                if !names.is_empty() {
                    gw.insert("names".into(), Value::Array(names));
                }
                if style_table.as_array().is_some_and(|t| !t.is_empty()) {
                    gw.insert("styles".into(), style_table);
                }
                if let Some(n) = crate::look::normal_style(wb) {
                    gw.insert("normalStyle".into(), n);
                }
                gw.extend(crate::look::workbook_look(wb));
                Value::Object(gw)
            })]),
            children: wb_children,
            order: Some(wb_order),
            ..Node::default()
        },
    );
    let mut header = header;
    if header.ox_version.is_empty() {
        header.ox_version = OX_VERSION.into();
    }
    Ok((Layer { header, imports: vec![], schemas: Default::default(), data: nodes }, report))
}

/// [`adopt`] from the file's bytes, also keeping each chart's own XML (see `crate::kept`), so an
/// unchanged chart exports with its formatting.
pub fn adopt_xlsx(bytes: &[u8], path: &str, name: &str, header: Header) -> Result<(Layer, AdoptReport), GridError> {
    let wb = crate::from_xlsx(bytes)?;
    let (mut layer, mut report) = adopt(&wb, path, name, header)?;
    crate::kept::keep_charts(&mut layer, bytes, &mut report.pictures, &mut report.warnings);
    Ok((layer, report))
}
