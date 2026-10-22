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
    /// The bytes of every picture the file shows, by blob value, with their media type: the
    /// caller stores the ones its project doesn't have yet.
    #[serde(skip)]
    pub pictures: crate::Pictures,
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

/// The composed workbook with an import's edits applied to its sheets and ranges (attribute by
/// attribute; `null` removes one): what the next export would be built from.
fn with_edits(node: &Composed, edits: &[Node]) -> Composed {
    let mut n = node.clone();
    let apply = |c: &mut Composed| {
        for e in edits.iter().filter(|e| e.path == c.path) {
            for (k, v) in &e.attributes {
                if v.is_null() {
                    c.attributes.remove(k);
                } else {
                    c.attributes.insert(k.clone(), v.clone());
                }
            }
        }
    };
    for sheet in &mut n.children {
        apply(sheet);
        for item in &mut sheet.children {
            apply(item);
        }
    }
    n
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

/// For comparing: LibreOffice writes `TRUE()`/`FALSE()` for `TRUE`/`FALSE`.
fn normalize_formula(text: &str) -> String {
    // ponytail: also rewrites the words inside a string literal; only the comparison sees it.
    Formula::new(text).text.replace("TRUE()", "TRUE").replace("FALSE()", "FALSE")
}

/// Equal, with numbers equal to 12 significant digits: recalculating (GridCraft, LibreOffice)
/// moves the last digits of a double.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) => x == y || (x - y).abs() <= 1e-12 * x.abs().max(y.abs()),
            _ => x == y,
        },
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(a, b)| same(a, b)),
        (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w))),
        _ => a == b,
    }
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
    import_saved_by(node, wb, header, None, None)
}

/// [`import`], knowing which application saved the file (`docProps/app.xml`): LibreOffice's own
/// rewrites that look like edits (its fitted row heights, its default sizes) aren't taken as edits.
/// `assets`: the folder a stack's pictures are read from when they're still named by a path (a
/// stack file's, not stored by hash), so they're compared by the hash of that file.
pub fn import_saved_by(
    node: &Composed,
    wb: &mut Workbook,
    header: Header,
    saved_by: Option<&str>,
    assets: Option<&std::path::Path>,
) -> Result<(Layer, ImportReport), GridError> {
    let libreoffice = saved_by.is_some_and(|a| a.starts_with("LibreOffice"));
    let found = string_name(wb, NAME_WORKBOOK).ok_or(GridError::NotExported)?;
    if found != node.path {
        return Err(GridError::WrongWorkbook {
            expected: node.path.clone(),
            found,
        });
    }
    let mut report = ImportReport::default();
    for b in ox_core::unreadable_pictures(node, assets) {
        report.warnings.push(format!("picture `{b}` can't be read, so it's compared by its path and may come back as changed"));
    }
    if string_name(wb, NAME_REVISION).as_deref() != Some(revision_of(node).as_str()) {
        report.stale = true;
        report.warnings.push("the workbook changed in the stack since this file was exported; review the edit layer before stacking it".into());
    }

    // What Milleleaf's own export writes for this node, read back the same way: a sheet's look
    // or a defined name that differs from it was changed in Excel (column widths Milleleaf fitted
    // itself don't come back as edits).
    let base = crate::export::export_with(node, ox_core::PictureSource::StandIn(assets))
        .ok()
        .and_then(|(b, _)| crate::to_xlsx(&b).ok())
        .and_then(|x| crate::from_xlsx(&x).ok());

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
        let located = locate(wb, &sheet_node.path);
        let sheet_now = located.as_ref().map(|(_, s, _)| s.clone());
        if sheet_now.is_some() {
            let m = name_match(wb, &sheet_node.path, &mut report.warnings);
            report.matches.push(m);
        }
        let old_gs = obj(sheet_node, "grid::sheet").cloned().unwrap_or_default();
        let mut gs = old_gs.clone();
        if let Some(now) = &sheet_now
            && *now != old_sheet
        {
            gs.insert("name".into(), json!(now));
            report.renamed.push(format!("{old_sheet} → {now}"));
        }
        if let (Some((si, ..)), Some(base)) = (&located, &base) {
            let now = wb.sheet(*si).map(|s| crate::look::sheet_look(s, &mut report.pictures)).unwrap_or_default();
            let was = base
                .sheets
                .iter()
                .find(|s| s.name == old_sheet)
                .map(|s| crate::look::sheet_look(s, &mut crate::Pictures::new()))
                .unwrap_or_default();
            let mut now = now;
            crate::look::settle(&mut now, &was, libreoffice);
            if let Some(file) = wb.sheet(*si) {
                crate::look::settle_chart_sizes(&mut now, file);
            }
            if let (Some(n), Some(w)) = (now.get("charts"), was.get("charts"))
                && crate::look::same_charts(n, w, &wb.theme, libreoffice)
            {
                now.insert("charts".into(), w.clone());
            } else if let (Some(Value::Array(n)), Some(Value::Array(w)), Some(Value::Array(old))) = (now.get_mut("charts"), was.get("charts"), old_gs.get("charts"))
                && n.len() == w.len()
                && n.len() == old.len()
            {
                // Some charts changed: the unchanged ones stay as the model has them (with their own
                // XML); the changed ones come back as the editor saved them.
                for i in 0..n.len() {
                    let mut placed_as_was = n[i].clone();
                    placed_as_was["anchor"] = w[i]["anchor"].clone();
                    if crate::look::same_charts(&json!([n[i]]), &json!([w[i]]), &wb.theme, libreoffice) {
                        n[i] = old[i].clone();
                    } else if crate::look::same_charts(&json!([placed_as_was]), &json!([w[i]]), &wb.theme, libreoffice) {
                        // Only moved or resized: the chart stays as the model has it (its own XML
                        // too), in its new place. Otherwise the editor's way of writing the same
                        // chart came back as edits: on the real workbook's sheet Graphs, a new
                        // Standard Width widened four combo charts (396 → 446 px) and Excel 16's
                        // XML for them read as theme colours turned RGB and series kinds spelled
                        // out, which its plain re-save, read the same way, doesn't count as edits.
                        let anchor = n[i]["anchor"].clone();
                        n[i] = old[i].clone();
                        n[i]["anchor"] = anchor;
                    } else if libreoffice && n[i]["series"].as_array().is_some_and(|ss| ss.iter().any(|s| s["values"] == json!(""))) {
                        // LibreOffice lost the chart's data (a series with no values): its loss,
                        // not an edit; the chart stays as the model has it.
                        report.warnings.push(format!("{old_sheet}: LibreOffice saved chart {} without its data; the chart is kept as it was", i + 1));
                        n[i] = old[i].clone();
                    }
                }
            }
            let mut looked = false;
            for k in crate::look::KEYS {
                if k == "cols" {
                    // A width set or changed in the editor comes back; one that's gone doesn't
                    // (LibreOffice drops the widths Milleleaf fitted, and the next export fits
                    // them again).
                    let empty = Map::new();
                    let was_c = was.get(k).and_then(Value::as_object).unwrap_or(&empty);
                    let mut cols = gs.get(k).and_then(Value::as_object).cloned().unwrap_or_default();
                    for (c, l) in now.get(k).and_then(Value::as_object).into_iter().flatten() {
                        if was_c.get(c) != Some(l) {
                            cols.insert(c.clone(), l.clone());
                            looked = true;
                        }
                    }
                    if !cols.is_empty() {
                        gs.insert(k.into(), Value::Object(cols));
                    }
                } else if now.get(k) != was.get(k) {
                    looked = true;
                    match now.get(k) {
                        Some(v) => gs.insert(k.into(), v.clone()),
                        None => gs.remove(k),
                    };
                }
            }
            // (A look the editor only rewrote, kept as the model has it, isn't a change.)
            if looked && gs != old_gs {
                report.changed.push(sheet_node.path.clone());
            }
        }
        if gs != old_gs {
            nodes.push(attr_node(&sheet_node.path, "grid::sheet", Value::Object(gs)));
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
                if !same(&Value::Object(new_data.clone()), &Value::Object(data.clone())) {
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
                if !v.get("value").is_some_and(|old| same(old, &new_value)) {
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
                if old_f != new_f.as_deref().map(normalize_formula) {
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

    // Cell styles changed in Excel (bold, fills, borders, number formats…): compared, cell by
    // cell, with what Milleleaf would export for the edited data (so a row added in Excel, which
    // Milleleaf formats itself, isn't a style edit). The workbook's styles and every sheet's map
    // then come back from the file.
    // With no edits, that is the export already read back above (one recalculation fewer).
    let expected = if nodes.is_empty() {
        base.clone()
    } else {
        crate::export::export_with(&with_edits(node, &nodes), ox_core::PictureSource::StandIn(assets))
            .ok()
            .and_then(|(b, _)| crate::to_xlsx(&b).ok())
            .and_then(|x| crate::from_xlsx(&x).ok())
    };
    if let Some(exp) = &expected {
        let changed = node.children.iter().any(|sn| {
            let Some((si, sheet, _)) = locate(wb, &sn.path) else { return false };
            // LibreOffice (24.2) writes rotated text's General alignment as Left: the same to a reader.
            let lo_rotated = |mut m: std::collections::BTreeMap<String, Value>| {
                if libreoffice {
                    for v in m.values_mut() {
                        if let Some(a) = v.get_mut("align").and_then(Value::as_object_mut)
                            && a.get("rotation").and_then(Value::as_i64).is_some_and(|r| r != 0)
                            && a.get("h").and_then(Value::as_str) == Some("Left")
                        {
                            a.insert("h".into(), json!("General"));
                        }
                    }
                }
                m
            };
            let mut now = lo_rotated(crate::look::cell_looks(wb, si));
            let was = lo_rotated(exp.sheets.iter().position(|s| s.name == sheet).map(|i| crate::look::cell_looks(exp, i)).unwrap_or_default());
            // LibreOffice gives a formula cell in General format the number format of the cells
            // it reads (its automatic format for formula results) when it loads the file, and
            // saves that: not the user's.
            if libreoffice {
                let unstyled = crate::look::unstyled_look(exp);
                let general = unstyled["num_fmt"].clone();
                now.retain(|k, v| {
                    let formula = CellRef::parse(k).and_then(|at| wb.sheets[si].cell(at)).is_some_and(|c| c.formula.is_some());
                    let was_general = was.get(k).is_none_or(|w| w["num_fmt"] == general);
                    if formula && was_general {
                        v["num_fmt"] = general.clone();
                    }
                    was.contains_key(k) || *v != unstyled
                });
            }
            now != was
        });
        if changed {
            let (table, maps) = crate::look::cell_styles(wb);
            let mut gw = obj(node, "grid::workbook").cloned().unwrap_or_default();
            gw.insert("styles".into(), table);
            match crate::look::normal_style(wb) {
                Some(n) => gw.insert("normalStyle".into(), n),
                None => gw.remove("normalStyle"),
            };
            nodes.push(attr_node(&node.path, "grid::workbook", Value::Object(gw)));
            for sn in &node.children {
                let Some((si, ..)) = locate(wb, &sn.path) else { continue };
                let mut gs = nodes
                    .iter()
                    .find(|n| n.path == sn.path)
                    .and_then(|n| n.attributes.get("grid::sheet"))
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_else(|| obj(sn, "grid::sheet").cloned().unwrap_or_default());
                gs.insert("cellStyles".into(), Value::Object(maps.get(si).cloned().unwrap_or_default()));
                nodes.push(attr_node(&sn.path, "grid::sheet", Value::Object(gs)));
                report.changed.push(sn.path.clone());
            }
        }
    }

    // The workbook's own defined names and its structure lock, changed in Excel. (Which sheet is
    // active changes with every save: adopt carries it, imports don't.)
    if let Some(base) = &base {
        let lock = |w: &Workbook| crate::look::workbook_look(w).remove("protection");
        let (mut now, was) = (crate::look::names(wb), crate::look::names(base));
        let (mut lock_now, lock_was) = (lock(wb), lock(base));
        if libreoffice {
            // LibreOffice drops names that point into another workbook (Milleleaf doesn't carry
            // external links, so the reference it writes dangles) and a structure lock whose
            // password hash it can't keep: neither is the user's edit.
            let external = |n: &Value| n.get("formula").and_then(Value::as_str).is_some_and(crate::look::external_ref);
            for n in &was {
                if external(n) && !now.iter().any(|m| m.get("name") == n.get("name") && m.get("scope") == n.get("scope")) {
                    now.push(n.clone());
                }
            }
            if lock_now.is_none() && lock_was.is_some() {
                report.warnings.push("LibreOffice dropped the workbook's structure lock; it is kept".into());
                lock_now = lock_was.clone();
            }
        }
        let key = |v: &[Value]| {
            let mut k: Vec<String> = v.iter().map(Value::to_string).collect();
            k.sort();
            k
        };
        if key(&now) != key(&was) || lock_now != lock_was {
            // On top of a workbook statement already made above (its styles).
            let mut gw = nodes
                .iter()
                .find(|n| n.path == node.path)
                .and_then(|n| n.attributes.get("grid::workbook"))
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_else(|| obj(node, "grid::workbook").cloned().unwrap_or_default());
            if now.is_empty() {
                gw.remove("names");
            } else {
                gw.insert("names".into(), Value::Array(now));
            }
            match lock_now {
                Some(l) => gw.insert("protection".into(), l),
                None => gw.remove("protection"),
            };
            nodes.push(attr_node(&node.path, "grid::workbook", Value::Object(gw)));
            report.changed.push(node.path.clone());
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
