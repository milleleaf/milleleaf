//! Readable descriptions of the native import layer; raw evidence stays with each item.
use std::collections::{BTreeMap, BTreeSet};

use ox_core::{Composed, Layer};
use serde_json::{Value, json};

#[derive(Clone, Default)]
struct Context {
    place: String,
    parent: String,
    sheet: String,
    range: String,
    slide: usize,
}

type Index<'a> = BTreeMap<String, (&'a Composed, Context)>;

fn index<'a>(root: &'a Composed) -> Index<'a> {
    fn walk<'a>(
        node: &'a Composed,
        mut ctx: Context,
        counts: &mut [usize; 3],
        out: &mut Index<'a>,
    ) {
        if let Some(sheet) = node.attributes.get("grid::sheet") {
            ctx.sheet = sheet["name"].as_str().unwrap_or("Untitled").into();
            ctx.place = format!("Sheet {}", ctx.sheet);
        }
        if let Some(range) = node.attributes.get("grid::range") {
            ctx.range = range["ref"].as_str().unwrap_or("").into();
            if ctx.sheet.is_empty() {
                ctx.sheet = range["sheet"].as_str().unwrap_or("Untitled").into();
            }
            ctx.place = format!(
                "Sheet {}, {} {}",
                ctx.sheet,
                if ctx.range.contains(':') {
                    "range"
                } else {
                    "cell"
                },
                ctx.range
            );
        } else if node.attributes.contains_key("word::paragraph") {
            counts[0] += 1;
            ctx.place = format!("Paragraph {}", counts[0]);
        } else if node.attributes.contains_key("word::table") {
            counts[1] += 1;
            ctx.place = format!("Table {}", counts[1]);
        } else if node.attributes.contains_key("deck::slide") {
            counts[2] += 1;
            ctx.slide = counts[2];
            ctx.place = format!("Slide {}", ctx.slide);
        }
        if ctx.place.is_empty() {
            ctx.place = "Document".into();
        }
        out.insert(node.path.clone(), (node, ctx.clone()));
        let mut shape = 0;
        for child in &node.children {
            let mut child_ctx = ctx.clone();
            child_ctx.parent = node.path.clone();
            if child.attributes.contains_key("deck::frame") {
                shape += 1;
                child_ctx.place = format!("Slide {}, object {}", ctx.slide, shape);
            }
            walk(child, child_ctx, counts, out);
        }
    }
    let mut out = BTreeMap::new();
    walk(root, Context::default(), &mut [0; 3], &mut out);
    out
}

fn text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => "blank".into(),
        _ => value.to_string(),
    }
}

fn runs(owner: &Composed, value: &Value) -> String {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|run| {
            if let Some(field) = run["field"].as_str() {
                ox_core::render::field_text(owner, field)
            } else {
                run["text"].as_str().unwrap_or("").to_string()
            }
        })
        .collect()
}

fn content(owner: &Composed, attr: &str, value: &Value) -> String {
    if attr == "word::paragraph" {
        runs(owner, &value["runs"])
    } else if attr == "deck::text" {
        value["paragraphs"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| runs(owner, &p["runs"]))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        String::new()
    }
}

fn push(
    out: &mut Vec<Value>,
    place: &str,
    description: String,
    before: &Value,
    after: &Value,
    technical: &Value,
) {
    out.push(json!({"place":place, "description":description,
        "kind":if before.is_null() {"added"} else if after.is_null() {"removed"} else {"changed"},
        "before":before, "after":after, "confidence":"Native import; identity follows file markers where present, otherwise a best guess by position or cell address", "technical":technical}));
}

fn changed(a: &Value, b: &Value) -> bool {
    !ox_core::same(a, b)
}
fn words(key: &str) -> String {
    match key {
        "colWidth" => "default column width".into(),
        "rowHeight" => "default row height".into(),
        "marginLeft" | "leftMargin" | "left" => "left margin".into(),
        "marginRight" | "rightMargin" | "right" => "right margin".into(),
        "marginTop" | "topMargin" | "top" => "top margin".into(),
        "marginBottom" | "bottomMargin" | "bottom" => "bottom margin".into(),
        _ => {
            let mut s = String::new();
            for c in key.chars() {
                if c.is_uppercase() && !s.is_empty() {
                    s.push(' ');
                }
                s.push(c.to_ascii_lowercase());
            }
            s
        }
    }
}

fn measurement(value: &Value) -> String {
    value
        .as_f64()
        .map(|n| {
            format!("{n:.3}")
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_string()
        })
        .unwrap_or_else(|| text(value))
}

fn settings(a: &Value, b: &Value, out: &mut Vec<String>) {
    if !changed(a, b) {
        return;
    }
    let keys: BTreeSet<_> = a
        .as_object()
        .into_iter()
        .flatten()
        .map(|(k, _)| k)
        .chain(b.as_object().into_iter().flatten().map(|(k, _)| k))
        .collect();
    for key in keys {
        let was = &a[key];
        let now = &b[key];
        if !changed(was, now) {
            continue;
        }
        if key == "margins" && (was.is_array() || now.is_array()) {
            for (i, side) in ["left", "right", "top", "bottom", "header", "footer"]
                .iter()
                .enumerate()
            {
                if changed(&was[i], &now[i]) {
                    out.push(format!(
                        "{side} margin: {} → {} in",
                        measurement(&was[i]),
                        measurement(&now[i])
                    ));
                }
            }
        } else if was.is_object() || now.is_object() {
            settings(was, now, out);
        } else if ["colWidth", "rowHeight"].contains(&key.as_str()) {
            out.push(format!(
                "{}: {} → {} px",
                words(key),
                measurement(was),
                measurement(now)
            ));
        } else {
            out.push(format!("{}: {} → {}", words(key), text(was), text(now)));
        }
    }
}

fn column(mut n: usize) -> String {
    let mut out = String::new();
    loop {
        out.insert(0, (b'A' + (n % 26) as u8) as char);
        if n < 26 {
            return out;
        }
        n = n / 26 - 1;
    }
}
fn origin(range: &str) -> (usize, usize) {
    let start = range.split(':').next().unwrap_or("A1");
    let mut col = 0;
    let mut row = String::new();
    for c in start.chars() {
        if c.is_ascii_alphabetic() {
            col = col * 26 + (c.to_ascii_uppercase() as usize - 'A' as usize + 1);
        } else if c.is_ascii_digit() {
            row.push(c);
        }
    }
    (col.saturating_sub(1), row.parse::<usize>().unwrap_or(1))
}
fn cell_place(ctx: &Context, row: usize, col: usize) -> String {
    if ctx.sheet.is_empty() {
        format!("{}, row {}, column {}", ctx.place, row + 1, col + 1)
    } else {
        let (c, r) = origin(&ctx.range);
        format!("Sheet {}, cell {}{}", ctx.sheet, column(c + col), r + row)
    }
}

fn table_changes(ctx: &Context, a: &Value, b: &Value, technical: &Value, out: &mut Vec<Value>) {
    let empty = Vec::new();
    let old = a["rows"].as_array().unwrap_or(&empty);
    let new = b["rows"].as_array().unwrap_or(&empty);
    let old_cols = a["columns"].as_array().unwrap_or(&empty);
    let new_cols = b["columns"].as_array().unwrap_or(&empty);
    for c in 0..old_cols.len().max(new_cols.len()) {
        let was = old_cols.get(c).unwrap_or(&Value::Null);
        let now = new_cols.get(c).unwrap_or(&Value::Null);
        if changed(was, now) {
            push(
                out,
                &cell_place(ctx, 0, c),
                format!(
                    "Column heading or type changed: {} → {}",
                    text(&was["name"]),
                    text(&now["name"])
                ),
                was,
                now,
                technical,
            );
        }
    }
    // Unique first-column labels let an inserted row retain its neighbours' identities.
    let labels = |rows: &[Value]| -> Option<BTreeMap<String, usize>> {
        let mut out = BTreeMap::new();
        for (i, row) in rows.iter().enumerate() {
            let label = row[0].as_str()?;
            if out.insert(label.into(), i).is_some() {
                return None;
            }
        }
        Some(out)
    };
    let keyed = old.len() != new.len();
    let matching = if keyed {
        labels(old).zip(labels(new))
    } else {
        None
    };
    if let Some((old_labels, new_labels)) = matching {
        for (r, row) in old.iter().enumerate() {
            let label = row[0].as_str().unwrap_or("");
            if !new_labels.contains_key(label) {
                push(
                    out,
                    &cell_place(ctx, r + 1, 0),
                    format!(
                        "Row removed: {}",
                        row.as_array()
                            .unwrap_or(&empty)
                            .iter()
                            .map(text)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    row,
                    &Value::Null,
                    technical,
                );
            }
        }
        for (r, row) in new.iter().enumerate() {
            let label = row[0].as_str().unwrap_or("");
            if let Some(was) = old_labels.get(label) {
                row_changes(ctx, r + 1, &old[*was], row, technical, out);
            } else {
                push(
                    out,
                    &cell_place(ctx, r + 1, 0),
                    format!(
                        "Row added: {}",
                        row.as_array()
                            .unwrap_or(&empty)
                            .iter()
                            .map(text)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    &Value::Null,
                    row,
                    technical,
                );
            }
        }
    } else {
        for r in 0..old.len().max(new.len()) {
            row_changes(
                ctx,
                r + 1,
                old.get(r).unwrap_or(&Value::Null),
                new.get(r).unwrap_or(&Value::Null),
                technical,
                out,
            );
        }
    }
    let mut rest_a = a.clone();
    let mut rest_b = b.clone();
    for value in [&mut rest_a, &mut rest_b] {
        if let Some(map) = value.as_object_mut() {
            map.remove("rows");
            map.remove("columns");
        }
    }
    if changed(&rest_a, &rest_b) {
        push(
            out,
            &ctx.place,
            "Table formatting or properties changed".into(),
            &rest_a,
            &rest_b,
            technical,
        );
    }
}
fn row_changes(
    ctx: &Context,
    row: usize,
    a: &Value,
    b: &Value,
    technical: &Value,
    out: &mut Vec<Value>,
) {
    let empty = Vec::new();
    let old = a.as_array().unwrap_or(&empty);
    let new = b.as_array().unwrap_or(&empty);
    for c in 0..old.len().max(new.len()) {
        let was = old.get(c).unwrap_or(&Value::Null);
        let now = new.get(c).unwrap_or(&Value::Null);
        if changed(was, now) {
            push(
                out,
                &cell_place(ctx, row, c),
                format!("{} → {}", text(was), text(now)),
                was,
                now,
                technical,
            );
        }
    }
}

fn deck_table_changes(
    ctx: &Context,
    a: &Value,
    b: &Value,
    technical: &Value,
    out: &mut Vec<Value>,
) {
    let empty = Vec::new();
    let old = a["snapshot"].as_array().unwrap_or(&empty);
    let new = b["snapshot"].as_array().unwrap_or(&empty);
    for r in 0..old.len().max(new.len()) {
        let was = old.get(r).and_then(Value::as_array).unwrap_or(&empty);
        let now = new.get(r).and_then(Value::as_array).unwrap_or(&empty);
        for c in 0..was.len().max(now.len()) {
            let old_text = was
                .get(c)
                .and_then(|cell| cell.get("text"))
                .unwrap_or(&Value::Null);
            let new_text = now
                .get(c)
                .and_then(|cell| cell.get("text"))
                .unwrap_or(&Value::Null);
            if changed(old_text, new_text) {
                push(
                    out,
                    &format!("{}, table row {}, column {}", ctx.place, r + 1, c + 1),
                    format!("'{}' → '{}'", text(old_text), text(new_text)),
                    old_text,
                    new_text,
                    technical,
                );
            }
        }
    }
    if !ox_deckcraft::same_table_formatting(a, b) {
        push(
            out,
            &ctx.place,
            "Table formatting or layout changed".into(),
            a,
            b,
            technical,
        );
    }
}

fn chart_changes(
    ctx: &Context,
    a: &Value,
    b: &Value,
    theme: &Value,
    technical: &Value,
    out: &mut Vec<Value>,
) {
    let empty = Vec::new();
    let old = a.as_array().unwrap_or(&empty);
    let new = b.as_array().unwrap_or(&empty);
    for i in 0..old.len().max(new.len()) {
        let was = old.get(i).unwrap_or(&Value::Null);
        let now = new.get(i).unwrap_or(&Value::Null);
        let evidence = json!({"chart":i + 1,"before":was,"after":now,"sheet":technical});
        if was.is_null() || now.is_null() {
            push(
                out,
                &ctx.place,
                format!(
                    "Chart {} {}",
                    i + 1,
                    if was.is_null() { "added" } else { "removed" }
                ),
                was,
                now,
                &evidence,
            );
            continue;
        }
        if ox_gridcraft::charts_equivalent(&json!([now]), &json!([was]), theme) {
            continue;
        }
        let mut same_anchor = now.clone();
        same_anchor["anchor"] = was["anchor"].clone();
        let description = if ox_gridcraft::charts_equivalent(
            &json!([same_anchor]),
            &json!([was]),
            theme,
        ) {
            let mut dimensions = Vec::new();
            for key in ["width", "height"] {
                let mut dimension = was.clone();
                dimension["anchor"][key] = now["anchor"][key].clone();
                if !ox_gridcraft::charts_equivalent(&json!([dimension]), &json!([was]), theme)
                    && let (Some(old), Some(new)) =
                        (was["anchor"][key].as_f64(), now["anchor"][key].as_f64())
                {
                    dimensions.push(format!("{key} {old:.2} → {new:.2} px"));
                }
            }
            let mut position = now.clone();
            for key in ["width", "height"] {
                position["anchor"][key] = was["anchor"][key].clone();
            }
            let moved = !ox_gridcraft::charts_equivalent(&json!([position]), &json!([was]), theme);
            if dimensions.is_empty() {
                format!("Chart {} moved: anchor position or mode changed", i + 1)
            } else {
                format!(
                    "Chart {} {}: {}",
                    i + 1,
                    if moved {
                        "moved and resized"
                    } else {
                        "resized"
                    },
                    dimensions.join("; ")
                )
            }
        } else if changed(&was["title"], &now["title"]) {
            format!(
                "Chart {} title changed: '{}' → '{}'",
                i + 1,
                text(&was["title"]),
                text(&now["title"])
            )
        } else {
            let data = |chart: &Value| -> Vec<Value> {
                chart["series"].as_array().into_iter().flatten().map(|series| json!({"name":series["name"],"values":series["values"],"categories":series["categories"]})).collect()
            };
            if data(was) != data(now) {
                format!("Chart {} data or series changed", i + 1)
            } else {
                format!("Chart {} formatting changed", i + 1)
            }
        };
        push(out, &ctx.place, description, was, now, &evidence);
    }
}

fn sheet_changes(
    ctx: &Context,
    a: &Value,
    b: &Value,
    theme: &Value,
    technical: &Value,
    out: &mut Vec<Value>,
) {
    if changed(&a["name"], &b["name"]) {
        push(
            out,
            &ctx.place,
            format!(
                "Sheet renamed: '{}' → '{}'",
                text(&a["name"]),
                text(&b["name"])
            ),
            &a["name"],
            &b["name"],
            technical,
        );
    }
    if changed(&a["print"], &b["print"]) || changed(&a["defaults"], &b["defaults"]) {
        let mut detail = Vec::new();
        settings(&a["print"], &b["print"], &mut detail);
        settings(&a["defaults"], &b["defaults"], &mut detail);
        push(
            out,
            &ctx.place,
            format!(
                "Page setup changed{}",
                if detail.is_empty() {
                    String::new()
                } else {
                    format!(": {}", detail.join("; "))
                }
            ),
            &json!({"print":a["print"],"defaults":a["defaults"]}),
            &json!({"print":b["print"],"defaults":b["defaults"]}),
            technical,
        );
    }
    let keys: BTreeSet<_> = a
        .as_object()
        .into_iter()
        .flatten()
        .chain(b.as_object().into_iter().flatten())
        .map(|(k, _)| k.as_str())
        .collect();
    for key in keys {
        if ["name", "print", "defaults"].contains(&key) || !changed(&a[key], &b[key]) {
            continue;
        }
        if key == "charts" {
            chart_changes(ctx, &a[key], &b[key], theme, technical, out);
        } else if key == "comments" {
            let cells: BTreeSet<_> = a[key]
                .as_object()
                .into_iter()
                .flatten()
                .chain(b[key].as_object().into_iter().flatten())
                .map(|(k, _)| k)
                .collect();
            for cell in cells {
                let was = &a[key][cell];
                let now = &b[key][cell];
                if changed(was, now) {
                    push(
                        out,
                        &format!("Sheet {}, cell {cell}", ctx.sheet),
                        if now.is_null() {
                            "Comment removed"
                        } else if was.is_null() {
                            "Comment added"
                        } else {
                            "Comment changed"
                        }
                        .into(),
                        was,
                        now,
                        technical,
                    );
                }
            }
        } else {
            let label = match key {
                "merges" => "Merged cells",
                "validations" => "Data validation",
                "freeze" => "Frozen panes",
                "rows" => "Row sizes",
                "cols" => "Column sizes",
                "cellStyles" => "Cell formatting",
                "pictures" => "Pictures",
                "charts" => "Charts",
                "view" => "Sheet view",
                "protection" => "Sheet protection",
                _ => "Sheet properties",
            };
            push(
                out,
                &ctx.place,
                format!(
                    "{label} {}",
                    if b[key].is_null() {
                        "removed"
                    } else if a[key].is_null() {
                        "added"
                    } else {
                        "changed"
                    }
                ),
                &a[key],
                &b[key],
                technical,
            );
        }
    }
}

// Match the native grid adapter's rendered-style comparison. These spellings have the
// same displayed number format; original spellings remain in the technical evidence.
fn normalized_formats(value: &Value) -> Value {
    let mut value = value.clone();
    fn normalize(value: &mut Value) {
        match value {
            Value::Object(map) => {
                if let Some(Value::String(format)) = map.get_mut("num_fmt") {
                    format.retain(|ch| ch != '"' && ch != '\\');
                }
                for child in map.values_mut() {
                    normalize(child);
                }
            }
            Value::Array(values) => {
                for child in values {
                    normalize(child);
                }
            }
            _ => {}
        }
    }
    normalize(&mut value);
    value
}

fn styled_cells(value: &Value) -> Option<BTreeMap<(usize, usize), usize>> {
    let mut out = BTreeMap::new();
    for (style, ranges) in value.as_object().into_iter().flatten() {
        let style = style.parse::<usize>().ok()?;
        for range in ranges.as_array()? {
            let range = range.as_str()?;
            let (start_col, start_row) = origin(range);
            let (end_col, end_row) = origin(range.split(':').next_back().unwrap_or(range));
            // Larger maps use the ordinary explicit formatting report, without assuming
            // equivalence. This bound affects grouping only, never the stored evidence.
            let area = (end_col.checked_sub(start_col)? + 1)
                .checked_mul(end_row.checked_sub(start_row)? + 1)?;
            if area > 100_000 || out.len() + area > 100_000 {
                return None;
            }
            for row in start_row..=end_row {
                for col in start_col..=end_col {
                    out.insert((col, row), style);
                }
            }
        }
    }
    Some(out)
}

fn formatting_followed_move(
    sheet_path: &str,
    a: &Value,
    b: &Value,
    old: &Index<'_>,
    new: &Index<'_>,
    old_styles: &Value,
    new_styles: &Value,
) -> Option<Vec<String>> {
    let was = styled_cells(a)?;
    let now = styled_cells(b)?;
    let cells: BTreeSet<_> = was.keys().chain(now.keys()).copied().collect();
    let mut related = BTreeSet::new();
    for at in cells {
        let old_style = was.get(&at).map(|i| normalized_formats(&old_styles[*i]));
        let new_style = now.get(&at).map(|i| normalized_formats(&new_styles[*i]));
        if old_style == new_style {
            continue;
        }
        let mut follows = false;
        for (path, (_, ctx)) in new {
            if ctx.parent != sheet_path || ctx.range.contains(':') || origin(&ctx.range) != at {
                continue;
            }
            let Some((_, source)) = old.get(path) else {
                continue;
            };
            if source.range.contains(':') || source.range == ctx.range {
                continue;
            }
            let source_at = origin(&source.range);
            let source_style = was
                .get(&source_at)
                .map(|i| normalized_formats(&old_styles[*i]));
            if source_style.as_ref().is_some_and(|style| !style.is_null())
                && source_style == new_style
            {
                related.insert(path.clone());
                follows = true;
                break;
            }
        }
        if !follows {
            return None;
        }
    }
    (!related.is_empty()).then(|| related.into_iter().collect())
}

pub(super) fn describe(before: &Composed, after: &Composed, edit: &Layer) -> Vec<Value> {
    let old = index(before);
    let new = index(after);
    let touched: BTreeSet<_> = edit.data.iter().map(|n| n.path.as_str()).collect();
    let mut out = Vec::new();
    let theme = after
        .attributes
        .get("grid::workbook")
        .and_then(|v| v.get("theme"))
        .or_else(|| {
            before
                .attributes
                .get("grid::workbook")
                .and_then(|v| v.get("theme"))
        })
        .unwrap_or(&Value::Null);
    let old_styles = before
        .attributes
        .get("grid::workbook")
        .map(|v| &v["styles"])
        .unwrap_or(&Value::Null);
    let new_styles = after
        .attributes
        .get("grid::workbook")
        .map(|v| &v["styles"])
        .unwrap_or(&Value::Null);
    let mut related_evidence: Vec<(Vec<String>, Value)> = Vec::new();
    for (from, to, added) in [(&old, &new, false), (&new, &old, true)] {
        for (path, (node, ctx)) in from {
            if to.contains_key(path)
                || (!touched.contains(path.as_str()) && !touched.contains(ctx.parent.as_str()))
            {
                continue;
            }
            if !ctx.parent.is_empty() && !to.contains_key(&ctx.parent) {
                continue;
            }
            let kind = if node.attributes.contains_key("word::paragraph") {
                "Paragraph"
            } else if node.attributes.contains_key("deck::slide") {
                "Slide"
            } else if node.attributes.contains_key("grid::sheet") {
                "Sheet"
            } else if node.attributes.contains_key("deck::picture") {
                "Picture"
            } else if node.attributes.contains_key("office::data::table")
                || node.attributes.contains_key("deck::table")
            {
                "Table"
            } else if node.attributes.contains_key("office::data::value") {
                "Cell"
            } else {
                "Object"
            };
            let snippet = node
                .attributes
                .get("word::paragraph")
                .map(|v| content(node, "word::paragraph", v))
                .or_else(|| {
                    node.attributes
                        .get("deck::text")
                        .map(|v| content(node, "deck::text", v))
                })
                .unwrap_or_default();
            let description = format!(
                "{kind} {}{}",
                if added { "added" } else { "removed" },
                if snippet.is_empty() {
                    String::new()
                } else {
                    format!(": '{snippet}'")
                }
            );
            let value = serde_json::to_value(node).unwrap_or(Value::Null);
            push(
                &mut out,
                &ctx.place,
                description,
                if added { &Value::Null } else { &value },
                if added { &value } else { &Value::Null },
                &json!({"path":path,"node":node}),
            );
        }
    }
    let mut described = BTreeSet::new();
    for entry in &edit.data {
        let (Some((was_node, old_ctx)), Some((now_node, new_ctx))) =
            (old.get(&entry.path), new.get(&entry.path))
        else {
            continue;
        };
        let ctx = if old_ctx.slide > 0 { old_ctx } else { new_ctx };
        for key in entry.attributes.keys() {
            if key.starts_with("ox::") || !described.insert((entry.path.as_str(), key.as_str())) {
                continue;
            }
            let a = was_node.attributes.get(key).unwrap_or(&Value::Null);
            let b = now_node.attributes.get(key).unwrap_or(&Value::Null);
            if !changed(a, b) {
                continue;
            }
            let technical = json!({"path":entry.path,"attribute":key,"before":a,"after":b});
            match key.as_str() {
                "grid::sheet" => {
                    let followed = if changed(&a["cellStyles"], &b["cellStyles"]) {
                        formatting_followed_move(
                            &entry.path,
                            &a["cellStyles"],
                            &b["cellStyles"],
                            &old,
                            &new,
                            old_styles,
                            new_styles,
                        )
                    } else {
                        None
                    };
                    if let Some(paths) = followed {
                        let mut shown = b.clone();
                        if let Some(map) = shown.as_object_mut() {
                            if let Some(styles) = a.get("cellStyles") {
                                map.insert("cellStyles".into(), styles.clone());
                            } else {
                                map.remove("cellStyles");
                            }
                        }
                        sheet_changes(ctx, a, &shown, theme, &technical, &mut out);
                        related_evidence.push((paths, json!({"reason":"Cell formatting followed the moved cell without changing its rendered style", "evidence":technical})));
                    } else {
                        sheet_changes(ctx, a, b, theme, &technical, &mut out);
                    }
                }
                "grid::workbook" => {
                    if changed(&normalized_formats(a), &normalized_formats(b)) {
                        push(
                            &mut out,
                            &ctx.place,
                            "Workbook properties or formatting changed".into(),
                            a,
                            b,
                            &technical,
                        );
                    } else {
                        related_evidence.push((Vec::new(), json!({"reason":"Equivalent number-format spellings; rendered style unchanged", "evidence":technical})));
                    }
                }
                "office::data::table" => table_changes(ctx, a, b, &technical, &mut out),
                "deck::table" => deck_table_changes(ctx, a, b, &technical, &mut out),
                "grid::formula" => push(
                    &mut out,
                    &ctx.place,
                    format!(
                        "Formula changed: {} → {}",
                        text(&a["text"]),
                        text(&b["text"])
                    ),
                    &a["text"],
                    &b["text"],
                    &technical,
                ),
                "office::data::value" => {
                    let description = if changed(&a["value"], &b["value"]) {
                        format!("{} → {}", text(&a["value"]), text(&b["value"]))
                    } else {
                        "Value formatting changed".into()
                    };
                    if changed(&a["value"], &b["value"]) {
                        push(
                            &mut out,
                            &ctx.place,
                            description,
                            &a["value"],
                            &b["value"],
                            &technical,
                        );
                    } else {
                        push(&mut out, &ctx.place, description, a, b, &technical);
                    }
                }
                "word::paragraph" | "deck::text" => {
                    let old_text = content(was_node, key, a);
                    let new_text = content(now_node, key, b);
                    let pictures = |value: &Value| -> Vec<Value> {
                        value["runs"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|run| run.get("picture").cloned())
                            .collect()
                    };
                    let old_pictures = pictures(a);
                    let new_pictures = pictures(b);
                    let description = if old_text != new_text {
                        format!("'{old_text}' → '{new_text}'")
                    } else if old_pictures != new_pictures {
                        if old_pictures.len() > new_pictures.len() {
                            "Picture removed".into()
                        } else if old_pictures.len() < new_pictures.len() {
                            "Picture added".into()
                        } else {
                            "Picture replaced or formatting changed".into()
                        }
                    } else {
                        "Text formatting changed".into()
                    };
                    push(&mut out, &ctx.place, description, a, b, &technical);
                }
                "grid::range" => {
                    if changed(&a["ref"], &b["ref"]) {
                        push(
                            &mut out,
                            &ctx.place,
                            format!(
                                "Range moved or resized: {} → {}",
                                text(&a["ref"]),
                                text(&b["ref"])
                            ),
                            a,
                            b,
                            &technical,
                        );
                    }
                }
                "deck::slide" => {
                    if changed(&a["notes"], &b["notes"]) {
                        push(
                            &mut out,
                            &ctx.place,
                            format!(
                                "Notes changed: '{}' → '{}'",
                                text(&a["notes"]),
                                text(&b["notes"])
                            ),
                            a,
                            b,
                            &technical,
                        );
                    }
                    let mut x = a.clone();
                    let mut y = b.clone();
                    for v in [&mut x, &mut y] {
                        if let Some(m) = v.as_object_mut() {
                            m.remove("notes");
                        }
                    }
                    if changed(&x, &y) {
                        push(
                            &mut out,
                            &ctx.place,
                            "Slide properties changed".into(),
                            &x,
                            &y,
                            &technical,
                        );
                    }
                }
                "deck::picture" => push(
                    &mut out,
                    &ctx.place,
                    if changed(&a["blob"], &b["blob"]) {
                        "Picture replaced"
                    } else {
                        "Picture formatting changed"
                    }
                    .into(),
                    a,
                    b,
                    &technical,
                ),
                "deck::frame" => push(
                    &mut out,
                    &ctx.place,
                    "Object moved or resized".into(),
                    a,
                    b,
                    &technical,
                ),
                _ => {
                    let description = match key.as_str() {
                        "word::table" => "Table formatting or cells changed",
                        "word::document" => "Document styles or page setup changed",
                        "grid::workbook" => "Workbook properties or formatting changed",
                        "deck::deck" => "Slide size or deck properties changed",
                        "deck::shape" => "Shape formatting changed",
                        "deck::line" => "Line formatting or endpoints changed",
                        "deck::chart" => "Chart changed",
                        _ => "Content properties changed",
                    };
                    push(&mut out, &ctx.place, description.into(), a, b, &technical);
                }
            }
        }
        if entry.order.is_some() {
            let old_order: Vec<_> = was_node.children.iter().map(|c| c.path.as_str()).collect();
            let new_order: Vec<_> = now_node.children.iter().map(|c| c.path.as_str()).collect();
            let common: BTreeSet<_> = old_order
                .iter()
                .filter(|p| new_order.contains(p))
                .copied()
                .collect();
            if old_order
                .iter()
                .filter(|p| common.contains(**p))
                .collect::<Vec<_>>()
                != new_order
                    .iter()
                    .filter(|p| common.contains(**p))
                    .collect::<Vec<_>>()
            {
                push(
                    &mut out,
                    &ctx.place,
                    "Content reordered".into(),
                    &json!(old_order),
                    &json!(new_order),
                    &json!({"path":entry.path,"order":entry.order}),
                );
            }
        }
    }
    for (paths, evidence) in related_evidence {
        let related = out.iter().position(|item| {
            item["technical"]["attribute"] == "grid::range"
                && paths
                    .iter()
                    .any(|path| item["technical"]["path"].as_str() == Some(path.as_str()))
        });
        if let Some(index) = related {
            let item = &mut out[index];
            let detail = item["technical"]
                .as_object_mut()
                .expect("description technical evidence is an object");
            detail
                .entry("related_formatting")
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .expect("related formatting is an array")
                .push(evidence);
            let description = item["description"].as_str().unwrap_or("").to_string();
            item["description"] =
                json!(format!("{description}; cell formatting followed the move"));
        } else if let Some(item) = out.first_mut() {
            item["technical"]
                .as_object_mut()
                .expect("description technical evidence is an object")
                .entry("equivalent_format_rewrites")
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .expect("format rewrites are an array")
                .push(evidence);
        }
    }
    out
}
