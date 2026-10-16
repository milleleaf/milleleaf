//! Edited GridCraft `Workbook` → recalculated OX edit layer.

use std::collections::BTreeMap;

use gridcraft_core::{CellRef, RangeRef, Value as GValue};
use gridcraft_model::{Formula, Workbook};
use ox_core::Composed;
use ox_core::model::{Header, Layer, Node, OX_VERSION};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::{GridError, NAME_REVISION, NAME_WORKBOOK, name_for, recalc, revision_of, split_ref};

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ImportReport {
    /// Nodes whose data, value or formula changed.
    pub changed: Vec<String>,
    /// Nodes whose range moved or resized (rows inserted, cells cut and pasted…).
    pub moved: Vec<String>,
    /// Sheets renamed.
    pub renamed: Vec<String>,
    /// Nodes whose `ox_` name is gone from the file (range deleted); their data is left alone.
    pub missing: Vec<String>,
    /// Formula results recomputed by GridCraft that differ from what the file had cached.
    pub recomputed: Vec<String>,
    /// Edits to data the workbook takes from elsewhere (a live link), not written back.
    pub conflicts: Vec<String>,
    pub warnings: Vec<String>,
    /// The stack changed since this file was exported.
    pub stale: bool,
    /// How each sheet and range was recognised (by its `ox_` defined name: certain).
    pub matches: Vec<ox_core::Match>,
}

impl ImportReport {
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.moved.is_empty() && self.renamed.is_empty()
    }
}

fn obj<'a>(n: &'a Composed, key: &str) -> Option<&'a Map<String, Value>> {
    n.attributes.get(key).and_then(Value::as_object)
}

/// The composed revision a workbook was exported at (`ox_revision`), if it carries one.
pub fn exported_revision(wb: &Workbook) -> Option<String> {
    string_name(wb, NAME_REVISION)
}

fn string_name(wb: &Workbook, name: &str) -> Option<String> {
    let f = wb
        .names
        .iter()
        .find(|n| n.name.eq_ignore_ascii_case(name))?
        .formula
        .trim()
        .trim_start_matches('=')
        .to_string();
    f.strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .map(|x| x.replace("\"\"", "\""))
}

/// Where a node's `ox_` name points: (sheet index, sheet name, range).
fn locate(wb: &Workbook, path: &str) -> Option<(usize, String, RangeRef)> {
    let want = name_for(path);
    // Copying a sheet copies its names as sheet-scoped ones; the workbook-scoped name is the original.
    let mut found = wb.names.iter().filter(|n| n.name.eq_ignore_ascii_case(&want));
    let first = found.next()?;
    let dn = std::iter::once(first)
        .chain(found)
        .find(|n| n.scope.is_none())
        .unwrap_or(first);
    let (sheet, range) = split_ref(&dn.formula)?;
    let si = wb.sheet_index(&sheet)?;
    Some((si, sheet, range))
}

/// The value at the start of a node's named range.
/// How many defined names carry a node's identity (more than one after a sheet is copied).
fn name_count(wb: &Workbook, path: &str) -> usize {
    let want = name_for(path);
    wb.names.iter().filter(|n| n.name.eq_ignore_ascii_case(&want)).count()
}

/// Certain if the name is unique, else a best guess (with a warning).
fn name_match(wb: &Workbook, path: &str, warnings: &mut Vec<String>) -> ox_core::Match {
    let name = name_for(path);
    match name_count(wb, path) {
        n if n > 1 => {
            warnings.push(format!("{path}: defined name {name} appears {n} times (a copied sheet?): the workbook-level one is taken as the original; best guess"));
            ox_core::Match::best_guess(path, format!("defined name {name}, appears {n} times"))
        }
        _ => ox_core::Match::certain(path, format!("defined name {name}")),
    }
}

pub(crate) fn named_value(wb: &Workbook, path: &str) -> Option<GValue> {
    let (si, _, r) = locate(wb, path)?;
    Some(wb.sheet(si)?.value(r.start))
}

pub(crate) fn json_of(v: &GValue, warnings: &mut Vec<String>, at: &str) -> Value {
    match v {
        GValue::Empty => Value::Null,
        GValue::Number(n) => {
            if n.fract() == 0.0 && n.abs() < 1e15 {
                json!(*n as i64)
            } else {
                json!(n)
            }
        }
        GValue::Text(t) => json!(t.as_ref()),
        GValue::Bool(b) => json!(b),
        GValue::Error(e) => {
            warnings.push(format!("{at}: error value {e:?}"));
            json!(format!("{e:?}"))
        }
        GValue::Array(_) => {
            warnings.push(format!("{at}: array value; kept as text"));
            json!("{array}")
        }
    }
}

/// Number keeps numbers, text keeps text; a column of numbers is `number` unless it was typed.
fn guess_type(cells: &[&Value]) -> &'static str {
    if !cells.is_empty() && cells.iter().all(|c| c.is_number() || c.is_null()) {
        "number"
    } else {
        "text"
    }
}

fn normalize_formula(text: &str) -> String {
    Formula::new(text).text
}

fn is_linked(n: &Composed, key: &str) -> bool {
    n.provenance.get(key).is_some_and(|p| !p.via.is_empty())
}

/// Recalculates `wb` with GridCraft, compares it with the composed workbook `node` it was
/// exported from, and returns the edit layer.
pub fn import(
    node: &Composed,
    wb: &mut Workbook,
    header: Header,
) -> Result<(Layer, ImportReport), GridError> {
    let found = string_name(wb, NAME_WORKBOOK).ok_or(GridError::NotExported)?;
    if found != node.path {
        return Err(GridError::WrongWorkbook {
            expected: node.path.clone(),
            found,
        });
    }
    let mut report = ImportReport::default();
    if string_name(wb, NAME_REVISION).as_deref() != Some(revision_of(node).as_str()) {
        report.stale = true;
        report.warnings.push("the workbook changed in the stack since this file was exported; review the edit layer before stacking it".into());
    }

    // Values the file cached, then GridCraft's own results.
    let mut cached: BTreeMap<String, GValue> = BTreeMap::new();
    for sheet_node in &node.children {
        for item in &sheet_node.children {
            if let Some(v) = named_value(wb, &item.path) {
                cached.insert(item.path.clone(), v);
            }
        }
    }
    recalc(wb);

    let mut nodes: Vec<Node> = Vec::new();
    let attr_node = |path: &str, key: &str, v: Value| Node {
        path: path.to_string(),
        attributes: Map::from_iter([(key.to_string(), v)]),
        ..Node::default()
    };

    for sheet_node in &node.children {
        let old_sheet = obj(sheet_node, "grid::sheet")
            .and_then(|g| g.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let sheet_now = locate(wb, &sheet_node.path).map(|(_, s, _)| s);
        if sheet_now.is_some() {
            let m = name_match(wb, &sheet_node.path, &mut report.warnings);
            report.matches.push(m);
        }
        if let Some(now) = &sheet_now
            && *now != old_sheet
        {
            let mut gs = obj(sheet_node, "grid::sheet").cloned().unwrap_or_default();
            gs.insert("name".into(), json!(now));
            nodes.push(attr_node(
                &sheet_node.path,
                "grid::sheet",
                Value::Object(gs),
            ));
            report.renamed.push(format!("{old_sheet} → {now}"));
        }
        for item in &sheet_node.children {
            let Some((si, sheet, range)) = locate(wb, &item.path) else {
                if item.attributes.contains_key("grid::range") {
                    report.missing.push(item.path.clone());
                }
                continue;
            };
            let Some(sh) = wb.sheet(si) else { continue };
            let m = name_match(wb, &item.path, &mut report.warnings);
            report.matches.push(m);
            // Range moved or resized?
            let old_range = obj(item, "grid::range").cloned().unwrap_or_default();
            let new_ref = range.a1();
            if old_range.get("ref").and_then(Value::as_str) != Some(new_ref.as_str())
                || old_range.get("sheet").and_then(Value::as_str) != Some(sheet.as_str())
            {
                nodes.push(attr_node(
                    &item.path,
                    "grid::range",
                    json!({ "sheet": sheet, "ref": new_ref }),
                ));
                report.moved.push(item.path.clone());
            }

            if let Some(data) = obj(item, "office::data::table") {
                let old_cols: Vec<Map<String, Value>> = data
                    .get("columns")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(|c| c.as_object().cloned()).collect())
                    .unwrap_or_default();
                let width = (range.end.col - range.start.col + 1) as usize;
                let mut grid: Vec<Vec<Value>> = Vec::new();
                for r in range.start.row..=range.end.row {
                    let row: Vec<Value> = (0..width)
                        .map(|i| {
                            json_of(
                                &sh.value(CellRef::new(r, range.start.col + i as u32)),
                                &mut report.warnings,
                                &item.path,
                            )
                        })
                        .collect();
                    grid.push(row);
                }
                // Rows emptied out at the bottom are deleted rows, not data.
                while grid.len() > 1 && grid.last().is_some_and(|r| r.iter().all(Value::is_null)) {
                    grid.pop();
                }
                let header_row = grid.first().cloned().unwrap_or_default();
                let body: Vec<Vec<Value>> = grid.into_iter().skip(1).collect();
                let cols: Vec<Value> = header_row
                    .iter()
                    .enumerate()
                    .map(|(i, h)| {
                        let name = h
                            .as_str()
                            .map(String::from)
                            .unwrap_or_else(|| h.to_string());
                        let kind = old_cols
                            .get(i)
                            .and_then(|c| c.get("type"))
                            .and_then(Value::as_str)
                            .map(String::from)
                            .unwrap_or_else(|| {
                                let cells: Vec<&Value> =
                                    body.iter().filter_map(|r| r.get(i)).collect();
                                guess_type(&cells).to_string()
                            });
                        let mut c = old_cols.get(i).cloned().unwrap_or_default();
                        c.insert("name".into(), json!(name));
                        c.insert("type".into(), json!(kind));
                        Value::Object(c)
                    })
                    .collect();
                let mut new_data = data.clone();
                new_data.insert("columns".into(), Value::Array(cols));
                new_data.insert(
                    "rows".into(),
                    Value::Array(body.into_iter().map(Value::Array).collect()),
                );
                if Value::Object(new_data.clone()) != Value::Object(data.clone()) {
                    if is_linked(item, "office::data::table") {
                        report.conflicts.push(item.path.clone());
                    } else {
                        nodes.push(attr_node(
                            &item.path,
                            "office::data::table",
                            Value::Object(new_data),
                        ));
                        report.changed.push(item.path.clone());
                    }
                }
            } else if let Some(v) = obj(item, "office::data::value") {
                let cell = sh.cell(range.start).cloned().unwrap_or_default();
                let new_value = json_of(&cell.value, &mut report.warnings, &item.path);
                if let Some(old) = cached.get(&item.path)
                    && *old != cell.value
                {
                    report.recomputed.push(item.path.clone());
                }
                if v.get("value") != Some(&new_value) {
                    let mut nv = v.clone();
                    nv.insert("value".into(), new_value);
                    nodes.push(attr_node(
                        &item.path,
                        "office::data::value",
                        Value::Object(nv),
                    ));
                    report.changed.push(item.path.clone());
                }
                let old_f = obj(item, "grid::formula")
                    .and_then(|f| f.get("text"))
                    .and_then(Value::as_str)
                    .map(normalize_formula);
                let new_f = cell.formula.as_ref().map(|f| f.text.clone());
                if old_f != new_f {
                    let v = match &new_f {
                        Some(t) => json!({ "text": format!("={t}") }),
                        None => Value::Null,
                    };
                    nodes.push(attr_node(&item.path, "grid::formula", v));
                    if !report.changed.contains(&item.path) {
                        report.changed.push(item.path.clone());
                    }
                }
            }
        }
    }

    // One statement per node.
    let mut merged: Vec<Node> = Vec::new();
    for n in nodes {
        match merged.iter_mut().find(|m| m.path == n.path) {
            Some(m) => m.attributes.extend(n.attributes),
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
