//! File-to-file comparison keeps names already in a file, then uses the normal importer.

use crate::{
    AdoptReport, GridError, ImportReport, NAME_PREFIX, NAME_WORKBOOK, name_for, split_ref,
};
use gridcraft_core::CellRef;
use gridcraft_model::{DefinedName, Workbook};
use milleleaf_core::{Composed, Header, Layer, Node};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

/// The native importer's chart comparison, including theme/RGB and anchor tolerances.
pub fn charts_equivalent(
    a: &serde_json::Value,
    b: &serde_json::Value,
    theme: &serde_json::Value,
) -> bool {
    let theme = serde_json::from_value(theme.clone()).unwrap_or_default();
    crate::look::same_charts(a, b, &theme, false)
}

/// The workbook identity carried by an exported file, when present.
pub fn exported_path(wb: &Workbook) -> Option<String> {
    wb.names
        .iter()
        .find(|n| n.name == NAME_WORKBOOK)?
        .formula
        .trim()
        .trim_start_matches('=')
        .strip_prefix('"')?
        .strip_suffix('"')
        .map(|s| s.replace("\"\"", "\""))
}

fn path_of(name: &str) -> String {
    let body = name.strip_prefix(NAME_PREFIX).unwrap_or(name);
    // UUIDs are reversible; other names can still be used verbatim with name_for.
    if body.len() == 36 && [8, 13, 18, 23].iter().all(|i| body.as_bytes()[*i] == b'_') {
        body.replace('_', "-")
    } else {
        body.into()
    }
}

/// Adopt file values, retaining existing Milleleaf sheet/range names for later imports.
/// File bytes do not contain the source stack's live-link provenance.
pub fn adopt_for_comparison(
    bytes: &[u8],
    path: &str,
    title: &str,
    header: Header,
) -> Result<(Layer, AdoptReport), GridError> {
    let wb = crate::from_xlsx(bytes)?;
    let (mut layer, report) = crate::adopt_xlsx(bytes, path, title, header)?;
    if exported_path(&wb).is_none() {
        return Ok((layer, report));
    }
    let mut remap = BTreeMap::new();
    let mut replacements = Vec::new();
    let mut remove = BTreeSet::new();
    let cells: BTreeMap<_, _> = layer
        .data
        .iter()
        .filter_map(|n| {
            let r = n.attributes.get("grid::range")?;
            Some((
                (
                    r["sheet"].as_str()?.to_string(),
                    r["ref"].as_str()?.to_string(),
                ),
                n.attributes.clone(),
            ))
        })
        .collect();
    for sheet in layer
        .data
        .iter_mut()
        .filter(|n| n.attributes.contains_key("grid::sheet"))
    {
        let name = sheet.attributes["grid::sheet"]["name"]
            .as_str()
            .unwrap_or("");
        let Some(sh) = wb.sheets.iter().find(|s| s.name == name) else {
            continue;
        };
        let names: Vec<_> = wb
            .names
            .iter()
            .filter(|n| n.scope.is_none() && n.name.starts_with(NAME_PREFIX))
            .filter_map(|n| {
                let (s, r) = split_ref(&n.formula)?;
                (s == name).then_some((n, r))
            })
            .collect();
        // Our writer names a sheet's A1 before naming the ranges placed on that sheet.
        let Some((sheet_name, _)) = names
            .iter()
            .find(|(_, r)| r.start == CellRef::new(0, 0) && r.start == r.end)
        else {
            continue;
        };
        let sheet_path = path_of(&sheet_name.name);
        remap.insert(sheet.path.clone(), sheet_path);
        let old_children = sheet.children.clone();
        let mut named = Vec::new();
        let mut covered = BTreeSet::new();
        for (n, range) in names.iter().filter(|(n, _)| n.name != sheet_name.name) {
            let p = path_of(&n.name);
            let mut attrs = serde_json::Map::new();
            attrs.insert(
                "grid::range".into(),
                json!({"sheet": name, "ref": range.a1()}),
            );
            if range.start != range.end {
                let columns: Vec<_> = (range.start.col..=range.end.col).map(|c| {
                    let v = crate::import::json_of(&sh.value(CellRef::new(range.start.row,c)), &mut Vec::new(), &p);
                    let kind = if range.start.row < range.end.row {
                        let cell = sh.cell(CellRef::new(range.start.row+1,c));
                        cell.filter(|c| c.value.is_number()).map(|c| wb.styles.get(c.style).num_fmt.0.to_string())
                    } else { None };
                    let ty = kind.map(|f| if f.contains('$') { "currency" } else if f.contains('%') { "percent" } else { "number" }).unwrap_or("text");
                    json!({"name": v.as_str().map(String::from).unwrap_or_else(|| v.to_string()), "type": ty})
                }).collect();
                let rows: Vec<Vec<_>> = ((range.start.row + 1)..=range.end.row)
                    .map(|r| {
                        (range.start.col..=range.end.col)
                            .map(|c| {
                                crate::import::json_of(
                                    &sh.value(CellRef::new(r, c)),
                                    &mut Vec::new(),
                                    &p,
                                )
                            })
                            .collect()
                    })
                    .collect();
                attrs.insert(
                    "office::data::table".into(),
                    json!({"columns": columns, "rows": rows}),
                );
            } else {
                if let Some(original) = cells.get(&(name.to_string(), range.a1())) {
                    attrs.extend(
                        original
                            .iter()
                            .filter(|(k, _)| k.as_str() != "grid::range")
                            .map(|(k, v)| (k.clone(), v.clone())),
                    );
                } else {
                    let cell = sh.cell(range.start).cloned().unwrap_or_default();
                    attrs.insert(
                        "office::data::value".into(),
                        json!({"value": crate::import::json_of(&cell.value, &mut Vec::new(), &p)}),
                    );
                    if let Some(f) = cell.formula {
                        attrs.insert("grid::formula".into(), json!({"text": f.text}));
                    }
                }
            }
            for r in range.start.row..=range.end.row {
                for c in range.start.col..=range.end.col {
                    covered.insert(CellRef::new(r, c).a1());
                }
            }
            named.push((range.start.a1(), p.clone()));
            replacements.push(Node {
                path: p,
                attributes: attrs,
                ..Node::default()
            });
        }
        for (cell, p) in &old_children {
            if covered.contains(cell)
                && let Some(p) = p {
                    remove.insert(p.clone());
                }
        }
        sheet.children.retain(|cell, _| !covered.contains(cell));
        for (cell, p) in named {
            sheet.children.insert(format!("range-{cell}"), Some(p));
        }
        sheet.order = Some(sheet.children.keys().cloned().collect());
    }
    layer.data.retain(|n| !remove.contains(&n.path));
    layer.data.extend(replacements);
    for n in &mut layer.data {
        if let Some(p) = remap.get(&n.path) {
            n.path = p.clone();
        }
        for p in n.children.values_mut().flatten() {
            if let Some(new) = remap.get(p) {
                *p = new.clone();
            }
        }
    }
    Ok((layer, report))
}

fn add_name(wb: &mut Workbook, path: &str, sheet: &str, range: &str) {
    let name = name_for(path);
    wb.names.push(DefinedName {
        name,
        formula: format!("'{}'!{}", sheet.replace('\'', "''"), range),
        scope: None,
        comment: String::new(),
        hidden: false,
    });
}

/// Use the normal import comparison; missing file identities are explicit coordinate guesses.
pub fn import_for_comparison(
    node: &Composed,
    wb: &mut Workbook,
    header: Header,
    saved_by: Option<&str>,
) -> Result<(Layer, ImportReport), GridError> {
    if let Some(found) = exported_path(wb) {
        if found != node.path {
            return Err(GridError::WrongWorkbook {
                expected: node.path.clone(),
                found,
            });
        }
    } else {
        wb.names.push(DefinedName {
            name: NAME_WORKBOOK.into(),
            formula: format!("\"{}\"", node.path),
            scope: None,
            comment: String::new(),
            hidden: false,
        });
    }
    let mut guessed = Vec::new();
    let mut present: BTreeSet<_> = wb.names.iter().map(|n| n.name.clone()).collect();
    for (si, sheet) in node.children.iter().enumerate() {
        let old = sheet.attributes["grid::sheet"]["name"]
            .as_str()
            .unwrap_or("");
        let now = wb
            .sheets
            .iter()
            .find(|s| s.name == old)
            .or_else(|| wb.sheets.get(si))
            .map(|s| s.name.clone());
        let Some(now) = now else { continue };
        if present.insert(name_for(&sheet.path)) {
            add_name(wb, &sheet.path, &now, "A1");
            guessed.push(sheet.path.clone());
        }
        for item in &sheet.children {
            if present.insert(name_for(&item.path))
                && let Some(r) = item.attributes["grid::range"]["ref"].as_str() {
                    add_name(wb, &item.path, &now, r);
                    guessed.push(item.path.clone());
                }
        }
    }
    let revision = crate::revision_of(node);
    if let Some(n) = wb.names.iter_mut().find(|n| n.name == crate::NAME_REVISION) {
        n.formula = format!("\"{revision}\"");
    } else {
        wb.names.push(DefinedName {
            name: crate::NAME_REVISION.into(),
            formula: format!("\"{revision}\""),
            scope: None,
            comment: String::new(),
            hidden: false,
        });
    }
    let (mut layer, mut report) = crate::import_saved_by(node, wb, header.clone(), saved_by, None)?;
    // The store importer follows named ranges. An unmarked file also needs newly occupied
    // cells: adopt their values with the same reader, and append them to the comparison layer.
    {
        let (adopted, _) = crate::adopt(wb, &node.path, "Document", header)?;
        let adopted_nodes: BTreeMap<_, _> =
            adopted.data.iter().map(|n| (n.path.as_str(), n)).collect();
        let locations: BTreeMap<_, _> = wb
            .names
            .iter()
            .filter_map(|n| Some((n.name.clone(), split_ref(&n.formula)?)))
            .collect();
        for sheet in &node.children {
            let location = locations.get(&name_for(&sheet.path));
            let Some((name, _)) = location else { continue };
            let Some(new_sheet) = adopted.data.iter().find(|n| {
                n.attributes
                    .get("grid::sheet")
                    .is_some_and(|s| s["name"].as_str() == Some(name.as_str()))
            }) else {
                continue;
            };
            let mut covered = BTreeSet::new();
            for (s, r) in sheet
                .children
                .iter()
                .filter_map(|c| locations.get(&name_for(&c.path)))
            {
                if s != name {
                    continue;
                }
                for row in r.start.row..=r.end.row {
                    for col in r.start.col..=r.end.col {
                        covered.insert((row, col));
                    }
                }
            }
            let mut parent = Node {
                path: sheet.path.clone(),
                ..Node::default()
            };
            let mut order: Vec<_> = sheet
                .children
                .iter()
                .filter_map(|n| n.name.clone())
                .collect();
            for child in new_sheet.children.values().flatten() {
                let Some(value) = adopted_nodes.get(child.as_str()) else {
                    continue;
                };
                let Some(reference) = value.attributes["grid::range"]["ref"].as_str() else {
                    continue;
                };
                let Some(at) = CellRef::parse(reference) else {
                    continue;
                };
                if covered.contains(&(at.row, at.col)) {
                    continue;
                }
                let mut cell = (*value).clone();
                cell.path = milleleaf_core::model::stable_id(&format!("{}|cell|{reference}", sheet.path));
                let child_name = format!("cell-{reference}");
                parent
                    .children
                    .insert(child_name.clone(), Some(cell.path.clone()));
                order.push(child_name);
                report.changed.push(cell.path.clone());
                guessed.push(cell.path.clone());
                layer.data.push(cell);
            }
            if !parent.children.is_empty() {
                parent.order = Some(order);
                layer.data.push(parent);
            }
        }
        // Entire new sheets retain all their occupied cells, rather than only a style change.
        for new_sheet in adopted
            .data
            .iter()
            .filter(|n| n.attributes.contains_key("grid::sheet"))
        {
            let name = new_sheet.attributes["grid::sheet"]["name"]
                .as_str()
                .unwrap_or("");
            let matched = node.children.iter().any(|s| {
                locations
                    .get(&name_for(&s.path))
                    .is_some_and(|(n, _)| n == name)
            });
            if matched {
                continue;
            }
            let child_name = format!("sheet-{}", &new_sheet.path[..8]);
            let mut root = Node {
                path: node.path.clone(),
                ..Node::default()
            };
            root.children
                .insert(child_name.clone(), Some(new_sheet.path.clone()));
            let mut order: Vec<_> = node
                .children
                .iter()
                .filter_map(|n| n.name.clone())
                .collect();
            order.push(child_name);
            root.order = Some(order);
            layer.data.push(root);
            layer.data.push(new_sheet.clone());
            for p in new_sheet.children.values().flatten() {
                if let Some(n) = adopted_nodes.get(p.as_str()) {
                    layer.data.push((*n).clone());
                }
            }
            report.changed.push(new_sheet.path.clone());
            guessed.push(new_sheet.path.clone());
        }
    }
    for path in &guessed {
        report.matches.retain(|m| &m.path != path);
        report.matches.push(milleleaf_core::Match::best_guess(
            path,
            "same sheet and cell address; file identity was absent",
        ));
    }
    milleleaf_core::matching::flag_best_guesses(&mut layer, &report.matches);
    Ok((layer, report))
}
