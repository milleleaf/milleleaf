//! Bringing a workbook Milleleaf didn't make into a project ([`adopt`]): each sheet becomes a
//! `grid::sheet` node and each non-empty cell an `office::data::value` at its `grid::range`, with
//! its `grid::formula`. Number formats become `currency` / `percent` / `number`. Everything else
//! (merged cells, styles, charts, column widths, comments, validation, names) isn't carried yet;
//! the report lists what the file had.

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
    let (mut merges, mut charts) = (0, 0);
    for sh in &wb.sheets {
        report.sheets += 1;
        let sheet_path = stable_id(&format!("{path}|xlsx-sheet|{}", sh.name));
        let sheet_name = format!("sheet-{}", &sheet_path[..8]);
        wb_children.insert(sheet_name.clone(), Some(sheet_path.clone()));
        wb_order.push(sheet_name);
        merges += sh.merges.len();
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
        nodes.push(Node {
            path: sheet_path,
            attributes: Map::from_iter([("grid::sheet".to_string(), json!({ "name": sh.name }))]),
            children: sheet_children,
            order: Some(sheet_order),
            ..Node::default()
        });
        charts += sh.charts.len();
    }
    nodes.insert(
        0,
        Node {
            path: path.to_string(),
            attributes: Map::from_iter([("grid::workbook".to_string(), json!({ "name": name }))]),
            children: wb_children,
            order: Some(wb_order),
            ..Node::default()
        },
    );
    if merges > 0 {
        report.warnings.push(format!("{merges} merged cell range(s) aren't carried yet"));
    }
    if charts > 0 {
        report.warnings.push(format!("{charts} chart(s) aren't carried yet"));
    }
    report.warnings.push("cell styles (fonts, fills, borders, alignment), column widths and row heights aren't carried yet".into());
    let mut header = header;
    if header.ox_version.is_empty() {
        header.ox_version = OX_VERSION.into();
    }
    Ok((Layer { header, imports: vec![], schemas: Default::default(), data: nodes }, report))
}
