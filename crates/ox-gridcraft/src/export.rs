//! Composed `grid::workbook` → GridCraft `Workbook`.

use std::sync::Arc;

use gridcraft_core::{CellRef, RangeRef, Value as GValue};
use gridcraft_formula::quote_sheet;
use gridcraft_model::{Cell, DefinedName, Formula, NumFmt, Sheet, Style, StyleId, Workbook};
use ox_core::Composed;
use serde_json::{Map, Value};

use crate::{GridError, NAME_REVISION, NAME_WORKBOOK, name_for, recalc, revision_of};

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ExportReport {
    pub sheets: usize,
    pub tables: usize,
    pub values: usize,
    pub formulas: usize,
    /// Formulas whose recalculated result differs from the value stored in the model: the model
    /// is stale until the import of this workbook writes the recomputed value back.
    pub stale_values: Vec<(String, f64, f64)>,
    pub warnings: Vec<String>,
    /// Each chart's kept XML, in sheet order, when the chart is unchanged since its XML was kept:
    /// pass it to [`crate::with_kept_charts`] with the written XLSX.
    pub kept_charts: Vec<Option<Vec<u8>>>,
}

fn obj<'a>(n: &'a Composed, key: &str) -> Option<&'a Map<String, Value>> {
    n.attributes.get(key).and_then(Value::as_object)
}

fn s<'a>(m: &'a Map<String, Value>, k: &str) -> Option<&'a str> {
    m.get(k).and_then(Value::as_str)
}

/// Excel number format for an `office::data` column type / value format.
pub(crate) fn num_fmt(kind: &str) -> Option<&'static str> {
    match kind {
        "currency" => Some("\"$\"#,##0"),
        "percent" => Some("0.0%"),
        "number" => Some("#,##0"),
        _ => None,
    }
}

fn to_cell_value(v: &Value) -> GValue {
    match v {
        Value::Number(n) => GValue::number(n.as_f64().unwrap_or(0.0)),
        Value::String(t) => GValue::text(t.as_str()),
        Value::Bool(b) => GValue::Bool(*b),
        Value::Null => GValue::Empty,
        other => GValue::text(other.to_string()),
    }
}

struct Styles {
    header: StyleId,
}

fn style_with(wb: &mut Workbook, fmt: Option<&str>, bold: bool) -> StyleId {
    if fmt.is_none() && !bold {
        return StyleId::DEFAULT;
    }
    let mut st: Style = wb.styles.get(StyleId::DEFAULT).clone();
    if let Some(f) = fmt {
        st.num_fmt = NumFmt(Arc::from(f));
    }
    st.font.bold = bold;
    wb.styles.intern(st)
}

fn name_over(sheet: &str, range: RangeRef, path: &str) -> DefinedName {
    let abs = |c: CellRef| format!("${}${}", gridcraft_core::col_to_letters(c.col), c.row + 1);
    let formula = if range.start == range.end {
        format!("{}!{}", quote_sheet(sheet), abs(range.start))
    } else {
        format!(
            "{}!{}:{}",
            quote_sheet(sheet),
            abs(range.start),
            abs(range.end)
        )
    };
    DefinedName {
        name: name_for(path),
        scope: None,
        formula,
        comment: format!("Milleleaf node {path}"),
        hidden: false,
    }
}

fn string_name(name: &str, value: &str) -> DefinedName {
    DefinedName {
        name: name.to_string(),
        scope: None,
        formula: format!("\"{}\"", value.replace('"', "\"\"")),
        comment: String::new(),
        hidden: false,
    }
}

fn write_table(
    wb: &mut Workbook,
    si: usize,
    sheet: &str,
    node: &Composed,
    at: CellRef,
    styles: &Styles,
    report: &mut ExportReport,
) {
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
    let col_styles: Vec<StyleId> = cols
        .iter()
        .map(|c| style_with(wb, num_fmt(s(c, "type").unwrap_or("text")), false))
        .collect();
    let Some(sh) = wb.sheet_mut(si) else { return };
    for (i, c) in cols.iter().enumerate() {
        let mut cell = Cell::value(GValue::text(s(c, "name").unwrap_or("")));
        cell.style = styles.header;
        sh.set_cell(CellRef::new(at.row, at.col + i as u32), cell);
    }
    for (r, row) in rows.iter().enumerate() {
        for (i, v) in row.iter().enumerate() {
            let mut cell = Cell::value(to_cell_value(v));
            cell.style = col_styles.get(i).copied().unwrap_or(StyleId::DEFAULT);
            sh.set_cell(CellRef::new(at.row + 1 + r as u32, at.col + i as u32), cell);
        }
    }
    let end = CellRef::new(
        at.row + rows.len() as u32,
        at.col + cols.len().max(1) as u32 - 1,
    );
    let range = RangeRef::new(at, end);
    if let Some(declared) = obj(node, "grid::range")
        .and_then(|r| s(r, "ref"))
        .and_then(RangeRef::parse)
        && declared != range
    {
        report.warnings.push(format!(
            "{}: grid::range says {} but the data fills {}; named the data",
            node.path,
            declared.a1(),
            range.a1()
        ));
    }
    wb.names.push(name_over(sheet, range, &node.path));
    report.tables += 1;
}

fn write_value(
    wb: &mut Workbook,
    si: usize,
    sheet: &str,
    node: &Composed,
    at: CellRef,
    report: &mut ExportReport,
) {
    let v = obj(node, "office::data::value")
        .cloned()
        .unwrap_or_default();
    // Text has no number format; a number without one is a plain number.
    let fmt = match v.get("value") {
        Some(Value::String(_)) => None,
        _ => num_fmt(s(&v, "format").unwrap_or("number")),
    };
    let style = style_with(wb, fmt, false);
    let value = to_cell_value(v.get("value").unwrap_or(&Value::Null));
    let formula = obj(node, "grid::formula")
        .and_then(|f| s(f, "text"))
        .map(Formula::new);
    let Some(sh) = wb.sheet_mut(si) else { return };
    let mut cell = Cell::value(value);
    cell.style = style;
    if let Some(f) = formula {
        cell.formula = Some(Arc::new(f));
        report.formulas += 1;
    }
    sh.set_cell(at, cell);
    wb.names
        .push(name_over(sheet, RangeRef::cell(at), &node.path));
    report.values += 1;
}

/// Builds a GridCraft workbook from a composed `grid::workbook` node, recalculated.
/// `assets` is the folder pictures' blobs are read from: the project (or stack) folder.
pub fn export(node: &Composed, assets: &std::path::Path) -> Result<(Workbook, ExportReport), GridError> {
    export_with(node, ox_core::PictureSource::Read(assets))
}

/// [`export`], with pictures from `assets` (a folder, memory, or stand-ins).
pub fn export_with(node: &Composed, assets: ox_core::PictureSource) -> Result<(Workbook, ExportReport), GridError> {
    let gw = obj(node, "grid::workbook")
        .ok_or_else(|| GridError::NotAWorkbook(node.path.clone()))?
        .clone();
    let mut wb = Workbook::new();
    wb.sheets.clear();
    wb.props.title = s(&gw, "name").unwrap_or("").to_string();
    let mut report = ExportReport::default();
    // An adopted workbook's own default style (its Normal font) and cell styles.
    if let Some(n) = gw.get("normalStyle") {
        match serde_json::from_value(n.clone()) {
            Ok(n) => wb.styles = gridcraft_model::StyleTable::with_default(n),
            Err(e) => report.warnings.push(format!("the workbook's default style can't be read ({e}); GridCraft's is used")),
        }
    }
    let cell_styles = crate::look::read_styles(gw.get("styles"), &mut report.warnings);
    let header = style_with(&mut wb, None, true);
    let styles = Styles { header };
    let mut sized: Vec<usize> = Vec::new();
    for sheet_node in &node.children {
        let Some(gs) = obj(sheet_node, "grid::sheet") else {
            report
                .warnings
                .push(format!("{}: not a grid::sheet; skipped", sheet_node.path));
            continue;
        };
        let sheet = s(gs, "name").unwrap_or("Sheet").to_string();
        wb.check_sheet_name(&sheet, None)
            .map_err(GridError::Invalid)?;
        wb.sheets.push(Arc::new(Sheet::new(sheet.clone())));
        let si = wb.sheets.len() - 1;
        wb.names.push(name_over(
            &sheet,
            RangeRef::cell(CellRef::new(0, 0)),
            &sheet_node.path,
        ));
        report.sheets += 1;
        for item in &sheet_node.children {
            let Some(at) = obj(item, "grid::range")
                .and_then(|r| s(r, "ref"))
                .and_then(RangeRef::parse)
                .map(|r| r.start)
            else {
                report
                    .warnings
                    .push(format!("{}: no grid::range; not placed", item.path));
                continue;
            };
            if let Some(other) = obj(item, "grid::range")
                .and_then(|r| s(r, "sheet"))
                .filter(|n| *n != sheet)
            {
                report.warnings.push(format!("{}: grid::range names sheet `{other}` but the node is under `{sheet}`; placed under `{sheet}`", item.path));
            }
            if item.attributes.contains_key("office::data::table") {
                write_table(&mut wb, si, &sheet, item, at, &styles, &mut report);
            } else if item.attributes.contains_key("office::data::value") {
                write_value(&mut wb, si, &sheet, item, at, &mut report);
            } else {
                report.warnings.push(format!(
                    "{}: nothing GridCraft can place yet; skipped",
                    item.path
                ));
            }
        }
        if let Some(sh) = wb.sheet_mut(si) {
            crate::look::apply_look(sh, gs, &sheet_node.path, &mut report.warnings, assets)?;
        }
        // The same charts apply_look placed, in its order.
        let charts = gs.get("charts").and_then(Value::as_array).into_iter().flatten();
        for (i, c) in charts.enumerate() {
            if let Ok(ch) = serde_json::from_value::<gridcraft_model::Chart>(c.clone()) {
                let kept = crate::kept::original(c, &ch, assets, &sheet_node.path, i + 1, &mut report.warnings);
                report.kept_charts.push(kept);
            }
        }
        crate::look::apply_cell_styles(&mut wb, si, gs.get("cellStyles"), &cell_styles, &sheet_node.path, &mut report.warnings);
        // A sheet whose sizes the model gives (an adopted workbook) keeps them: no auto-fit.
        if gs.contains_key("cols") || gs.contains_key("defaults") {
            sized.push(si);
        }
    }
    crate::look::apply_names(&mut wb, gw.get("names"), &mut report.warnings);
    crate::look::apply_workbook_look(&mut wb, &gw, &mut report.warnings);
    if wb.sheets.is_empty() {
        wb.sheets.push(Arc::new(Sheet::new("Sheet1")));
    }
    wb.names.push(string_name(NAME_WORKBOOK, &node.path));
    wb.names
        .push(string_name(NAME_REVISION, &revision_of(node)));

    // Recalculate and compare with what the model stores.
    recalc(&mut wb);
    fit_columns(&mut wb, &sized);
    for sheet_node in &node.children {
        for item in sheet_node
            .children
            .iter()
            .filter(|i| i.attributes.contains_key("grid::formula"))
        {
            let stored = obj(item, "office::data::value")
                .and_then(|v| v.get("value"))
                .and_then(Value::as_f64);
            let computed = crate::import::named_value(&wb, &item.path);
            if let (Some(a), Some(GValue::Number(b))) = (stored, computed)
                && (a - b).abs() > 1e-9
            {
                report.stale_values.push((item.path.clone(), a, b));
            }
        }
    }
    Ok((wb, report))
}

/// Widens columns to their content, roughly as Excel's AutoFit does, so numbers don't print as
/// `#######` in a column too narrow for them.
fn fit_columns(wb: &mut Workbook, skip: &[usize]) {
    for si in (0..wb.sheets.len()).filter(|i| !skip.contains(i)) {
        let Some(sh) = wb.sheet(si) else { continue };
        let Some(used) = sh.used_range() else {
            continue;
        };
        let mut widths: std::collections::BTreeMap<u32, f32> = std::collections::BTreeMap::new();
        for r in used.start.row..=used.end.row {
            for c in used.start.col..=used.end.col {
                let at = CellRef::new(r, c);
                let Some(cell) = sh.cell(at) else { continue };
                let fmt = wb.styles.get(cell.style).num_fmt.0.to_string();
                let kind = if fmt.contains('$') {
                    "currency"
                } else if fmt.contains('%') {
                    "percent"
                } else if fmt.contains("#,##0") {
                    "number"
                } else {
                    "text"
                };
                let text = match &cell.value {
                    GValue::Number(n) => ox_core::render::format_value(&serde_json::json!(n), kind),
                    GValue::Text(t) => t.to_string(),
                    GValue::Bool(b) => b.to_string(),
                    _ => String::new(),
                };
                let bold = wb.styles.get(cell.style).font.bold;
                let px = text.chars().count() as f32 * if bold { 8.0 } else { 7.5 } + 14.0;
                let w = widths.entry(c).or_insert(0.0);
                *w = w.max(px);
            }
        }
        if let Some(sh) = wb.sheet_mut(si) {
            // Columns with a width of their own (from the model) keep it.
            for (c, w) in widths {
                if w > gridcraft_model::DEFAULT_COL_WIDTH && !sh.cols.contains_key(&c) {
                    sh.cols.insert(
                        c,
                        gridcraft_model::LineInfo {
                            size: Some(w),
                            ..Default::default()
                        },
                    );
                }
            }
        }
    }
}
