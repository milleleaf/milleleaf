//! Readable snapshots and locators for the native import evidence.
use super::Pictures;
use milleleaf_core::Composed;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Default)]
struct Location<'a> {
    node: Option<&'a Composed>,
    parent: Option<&'a Composed>,
    heading: String,
    slide: usize,
    title: String,
    sheet: String,
}
type Index<'a> = BTreeMap<String, Location<'a>>;

fn runs(node: &Composed, v: &Value) -> String {
    v.as_array()
        .into_iter()
        .flatten()
        .map(|r| {
            r["field"]
                .as_str()
                .map(|f| milleleaf_core::render::field_text(node, f))
                .unwrap_or_else(|| r["text"].as_str().unwrap_or("").into())
        })
        .collect()
}
fn value(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "(empty)".into(),
        Value::Array(a) => a.iter().map(value).collect::<Vec<_>>().join(" | "),
        Value::Object(o) => o
            .iter()
            .filter(|(k, _)| !["blob", "xml"].contains(&k.as_str()))
            .map(|(k, v)| format!("{}: {}", label(k), value(v)))
            .collect::<Vec<_>>()
            .join("; "),
        _ => v.to_string(),
    }
}
fn label(key: &str) -> String {
    match key {
        "w" => "width (px)".into(),
        "h" => "height (px)".into(),
        "colWidth" => "default column width (px)".into(),
        "rowHeight" => "default row height (px)".into(),
        "margins" => "margins in inches (left, right, top, bottom, header, footer)".into(),
        "at" => "anchored at cell".into(),
        "ref" => "cells".into(),
        _ => {
            let mut s = String::new();
            for c in key.chars() {
                if c.is_uppercase() {
                    s.push(' ');
                }
                s.push(c.to_ascii_lowercase());
            }
            s
        }
    }
}
fn plain(n: &Composed) -> String {
    if let Some(p) = n.attributes.get("word::paragraph") {
        return runs(n, &p["runs"]);
    }
    if let Some(t) = n.attributes.get("deck::text") {
        return t["paragraphs"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| runs(n, &p["runs"]))
            .collect::<Vec<_>>()
            .join("\n");
    }
    if let Some(t) = n.attributes.get("office::data::table") {
        let header = t["columns"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| value(&c["name"]))
            .collect::<Vec<_>>()
            .join(" | ");
        let rows = t["rows"].as_array().into_iter().flatten().map(value);
        return std::iter::once(header)
            .chain(rows)
            .collect::<Vec<_>>()
            .join("\n");
    }
    if let Some(t) = n.attributes.get("deck::table") {
        return t["snapshot"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|r| {
                r.as_array()
                    .into_iter()
                    .flatten()
                    .map(|c| value(&c["text"]))
                    .collect::<Vec<_>>()
                    .join(" | ")
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    if let Some(v) = n.attributes.get("office::data::value") {
        return value(&v["value"]);
    }
    n.children
        .iter()
        .map(plain)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}
fn snippet(s: &str) -> String {
    let start = s.split_whitespace().take(7).collect::<Vec<_>>().join(" ");
    if s.split_whitespace().count() > 7 {
        format!("{start}…")
    } else {
        start
    }
}
fn index(root: &Composed) -> Index<'_> {
    fn walk<'a>(n: &'a Composed, mut loc: Location<'a>, slides: &mut usize, out: &mut Index<'a>) {
        loc.node = Some(n);
        if n.attributes.contains_key("deck::slide") {
            *slides += 1;
            loc.slide = *slides;
            loc.title = n
                .children
                .iter()
                .map(plain)
                .find(|s| !s.trim().is_empty())
                .unwrap_or_default();
        }
        if let Some(s) = n.attributes.get("grid::sheet") {
            loc.sheet = s["name"].as_str().unwrap_or("Untitled").into();
        }
        out.insert(n.path.clone(), loc.clone());
        let mut heading = loc.heading.clone();
        for child in &n.children {
            let mut next = loc.clone();
            next.parent = Some(n);
            next.heading = heading.clone();
            let style = child
                .attributes
                .get("word::paragraph")
                .and_then(|p| p["style"].as_str())
                .unwrap_or("")
                .to_lowercase();
            if style.contains("heading") || style.contains("title") {
                heading = plain(child);
                next.heading = heading.clone();
            }
            walk(child, next, slides, out);
        }
    }
    let mut out = BTreeMap::new();
    walk(root, Location::default(), &mut 0, &mut out);
    out
}
fn paragraph_locator(loc: &Location<'_>) -> String {
    let node = loc.node.expect("indexed node");
    let prefix = if loc.heading.is_empty() {
        String::new()
    } else {
        format!("Under ‘{}’, ", loc.heading)
    };
    let text = plain(node);
    if !text.trim().is_empty() {
        return format!("{prefix}paragraph starting ‘{}’", snippet(&text));
    }
    let previous = loc
        .parent
        .and_then(|p| {
            p.children
                .iter()
                .position(|c| c.path == node.path)
                .map(|i| &p.children[..i])
        })
        .into_iter()
        .flatten()
        .rev()
        .map(plain)
        .find(|s| !s.trim().is_empty());
    match previous {
        Some(s) => format!("{prefix}after the paragraph starting ‘{}’", snippet(&s)),
        None => format!("{prefix}empty paragraph at the start"),
    }
}
fn display(s: String) -> String {
    if s.trim().is_empty() {
        "⏎ Empty line".into()
    } else {
        s
    }
}
fn gap_lines(loc: &Location<'_>, other: &Index<'_>) -> Value {
    let node = loc.node.expect("indexed node");
    let Some(parent) = loc.parent else {
        return json!({"lines":[]});
    };
    let Some(i) = parent.children.iter().position(|n| n.path == node.path) else {
        return json!({"lines":[]});
    };
    let mut lines = Vec::new();
    if let Some(previous) = parent.children[..i]
        .iter()
        .rev()
        .find_map(|n| other.get(&n.path))
    {
        lines.push(json!({"text":display(plain(previous.node.unwrap())),"changed":false}));
    }
    lines.push(json!({"text":"(no paragraph here)","changed":true}));
    if let Some(next) = parent.children[i + 1..]
        .iter()
        .find_map(|n| other.get(&n.path))
    {
        lines.push(json!({"text":display(plain(next.node.unwrap())),"changed":false}));
    }
    json!({"lines":lines})
}
fn lines(loc: Option<&Location<'_>>, fallback: &Value) -> Value {
    let Some(loc) = loc else {
        return json!({"lines":[{"text":"(not present)","changed":true}]});
    };
    let node = loc.node.expect("indexed node");
    if node.attributes.contains_key("word::paragraph")
        || node.attributes.contains_key("word::table")
    {
        let mut rows = Vec::new();
        if let Some(parent) = loc.parent
            && let Some(i) = parent.children.iter().position(|n| n.path == node.path)
        {
            if i > 0 {
                rows.push(json!({"text":display(plain(&parent.children[i-1])),"changed":false}));
            }
            rows.push(json!({"text":display(plain(node)),"changed":true}));
            if let Some(next) = parent.children.get(i + 1) {
                rows.push(json!({"text":display(plain(next)),"changed":false}));
            }
        } else {
            rows.push(json!({"text":display(plain(node)),"changed":true}));
        }
        return json!({"lines": rows});
    }
    let text = plain(node);
    json!({"lines":[{"text":if text.is_empty() { readable_snapshot(Some(loc),fallback) } else {text},"changed":true}]})
}
fn coordinate(address: &str) -> (usize, usize) {
    let mut col = 0;
    let mut row = String::new();
    for c in address.split(':').next().unwrap_or("A1").chars() {
        if c.is_ascii_alphabetic() {
            col = col * 26 + (c.to_ascii_uppercase() as usize - 'A' as usize + 1);
        } else if c.is_ascii_digit() {
            row.push(c);
        }
    }
    (col.max(1), row.parse().unwrap_or(1))
}
fn column(mut n: usize) -> String {
    let mut s = String::new();
    while n > 0 {
        n -= 1;
        s.insert(0, (b'A' + (n % 26) as u8) as char);
        n /= 26;
    }
    s
}
fn cells(index: &Index<'_>, sheet: &str) -> BTreeMap<(usize, usize), String> {
    let mut out = BTreeMap::new();
    for loc in index.values().filter(|l| l.sheet == sheet) {
        let n = loc.node.expect("indexed node");
        let Some(range) = n
            .attributes
            .get("grid::range")
            .and_then(|r| r["ref"].as_str())
        else {
            continue;
        };
        let (c, r) = coordinate(range);
        if let Some(table) = n.attributes.get("office::data::table") {
            for (i, col) in table["columns"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
            {
                out.insert((c + i, r), value(&col["name"]));
            }
            for (i, row) in table["rows"].as_array().into_iter().flatten().enumerate() {
                for (j, v) in row.as_array().into_iter().flatten().enumerate() {
                    out.insert((c + j, r + i + 1), value(v));
                }
            }
        } else {
            let mut text = plain(n);
            if let Some(f) = n
                .attributes
                .get("grid::formula")
                .and_then(|f| f["text"].as_str())
            {
                text = format!("{f} = {text}");
            }
            out.insert((c, r), text);
        }
    }
    out
}
fn grid(index: &Index<'_>, sheet: &str, at: (usize, usize)) -> Value {
    let values = cells(index, sheet);
    let (c, r) = at;
    let columns: Vec<_> = (c.saturating_sub(1).max(1)..=c + 1).map(column).collect();
    let rows:Vec<_>=(r.saturating_sub(1).max(1)..=r+1).map(|y| json!({"label":y,"cells":(c.saturating_sub(1).max(1)..=c+1).map(|x|json!({"address":format!("{}{y}",column(x)),"text":values.get(&(x,y)).cloned().unwrap_or_default(),"changed":(x,y)==at})).collect::<Vec<_>>()})).collect();
    json!({"columns":columns,"rows":rows})
}

fn table_lines(loc: Option<&Location<'_>>, index: &Index<'_>, row: usize) -> Value {
    let Some(loc) = loc else {
        return json!({"lines":[{"text":"(row not present)","changed":true}]});
    };
    let n = loc.node.expect("indexed node");
    let table = &n.attributes["office::data::table"];
    let container = loc
        .parent
        .and_then(|p| index.get(&p.path))
        .filter(|p| {
            p.node
                .is_some_and(|n| n.attributes.contains_key("word::table"))
        })
        .unwrap_or(loc);
    let mut rows = Vec::new();
    let surrounding = container.parent.and_then(|p| {
        p.children
            .iter()
            .position(|n| Some(&n.path) == container.node.map(|n| &n.path))
            .map(|i| (p, i))
    });
    if let Some((p, i)) = surrounding
        && i > 0
    {
        rows.push(json!({"text":display(plain(&p.children[i-1])),"changed":false}));
    }
    rows.push(json!({"text":table["columns"].as_array().into_iter().flatten().map(|c|value(&c["name"])).collect::<Vec<_>>().join(" | "),"changed":row==1}));
    if row > 1 {
        rows.push(json!({"text":table["rows"].get(row-2).map(value).unwrap_or_else(||"(row not present)".into()),"changed":true}));
    }
    if let Some((p, i)) = surrounding
        && let Some(next) = p.children.get(i + 1)
    {
        rows.push(json!({"text":display(plain(next)),"changed":false}));
    }
    json!({"lines":rows})
}
fn setting_value(value: &Value) -> String {
    if let Some(number) = value.as_f64() {
        format!("{number:.3}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    } else {
        self::value(value)
    }
}
fn changed_settings(before: &Value, after: &Value) -> (Vec<Value>, Vec<Value>) {
    fn walk(a: &Value, b: &Value, path: &mut Vec<String>, out: &mut Vec<(String, String, String)>) {
        if milleleaf_core::same(a, b) {
            return;
        }
        if a.is_object() || b.is_object() {
            let keys: std::collections::BTreeSet<_> = a
                .as_object()
                .into_iter()
                .flatten()
                .chain(b.as_object().into_iter().flatten())
                .map(|(k, _)| k)
                .collect();
            for key in keys {
                path.push(key.clone());
                walk(&a[key], &b[key], path, out);
                path.pop();
            }
            return;
        }
        let key = path.last().map(String::as_str).unwrap_or("setting");
        if key == "margins" && (a.is_array() || b.is_array()) {
            let count = a
                .as_array()
                .map_or(0, Vec::len)
                .max(b.as_array().map_or(0, Vec::len));
            for i in 0..count {
                if !milleleaf_core::same(&a[i], &b[i]) {
                    let side = ["Left", "Right", "Top", "Bottom", "Header", "Footer"]
                        .get(i)
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| format!("Additional {}", i + 1));
                    out.push((
                        format!("{side} margin"),
                        format!("{} in", setting_value(&a[i])),
                        format!("{} in", setting_value(&b[i])),
                    ));
                }
            }
            return;
        }
        let (name, unit) = match key {
            "colWidth" => ("Default column width".into(), " px"),
            "rowHeight" => ("Default row height".into(), " px"),
            _ => {
                let words = path
                    .iter()
                    .filter(|k| !["print", "defaults"].contains(&k.as_str()))
                    .map(|k| label(k).replace('_', " "))
                    .collect::<Vec<_>>()
                    .join(" / ");
                let mut chars = words.chars();
                let name = chars
                    .next()
                    .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
                    .unwrap_or_else(|| "Setting".into());
                (name, "")
            }
        };
        let mut was = setting_value(a);
        let mut now = setting_value(b);
        // Preserve evidence for genuinely tiny changes that would round to the same label.
        if was == now && a != b {
            was = value(a);
            now = value(b);
        }
        out.push((name, format!("{was}{unit}"), format!("{now}{unit}")));
    }
    let mut differences = Vec::new();
    walk(before, after, &mut Vec::new(), &mut differences);
    let lines = |after: bool| {
        differences.iter().map(|(name,was,now)|json!({"text":format!("{name}: {}",if after{now}else{was}),"changed":true})).collect()
    };
    (lines(false), lines(true))
}
fn readable_snapshot(loc: Option<&Location<'_>>, snapshot: &Value) -> String {
    if snapshot.get("path").is_none() || snapshot.get("attributes").is_none() {
        return value(snapshot);
    }
    let Some(loc) = loc else {
        return "(not present)".into();
    };
    let node = loc.node.expect("indexed node");
    let mut text = plain(node);
    if let Some(formula) = node
        .attributes
        .get("grid::formula")
        .and_then(|f| f["text"].as_str())
    {
        text = format!("{formula} = {text}");
    }
    if text.is_empty() {
        if let Some(sheet) = node.attributes.get("grid::sheet") {
            text = format!("Sheet ‘{}’", sheet["name"].as_str().unwrap_or("Untitled"));
            for key in ["print", "defaults"] {
                if let Some(settings) = sheet.get(key) {
                    text.push_str(&format!("; {}: {}", label(key), value(settings)));
                }
            }
        } else if node.attributes.contains_key("grid::range") {
            text = "(empty cell)".into();
        } else {
            let frame = node.attributes.get("deck::frame");
            let picture = node.attributes.get("deck::picture");
            let kind = if picture.is_some() {
                "Picture"
            } else if node.attributes.contains_key("deck::shape") {
                "Shape"
            } else if node.attributes.contains_key("deck::line") {
                "Line"
            } else {
                "Object"
            };
            let name = frame
                .and_then(|f| f["name"].as_str())
                .or_else(|| picture.and_then(|p| p["alt"].as_str().or_else(|| p["name"].as_str())))
                .or(node.name.as_deref());
            text = name
                .filter(|s| !s.is_empty())
                .map(|n| format!("{kind} ‘{n}’"))
                .unwrap_or_else(|| kind.into());
            if let Some(frame) = frame {
                for (key, label, unit) in [
                    ("x", "horizontal position", "px"),
                    ("y", "vertical position", "px"),
                    ("w", "width", "px"),
                    ("h", "height", "px"),
                    ("rotation", "rotation", "degrees"),
                ] {
                    if let Some(v) = frame.get(key) {
                        text.push_str(&format!("; {label}: {} {unit}", setting_value(v)));
                    }
                }
            }
        }
    }
    text
}
fn cell_labels(
    index: &Index<'_>,
    sheet: &str,
    at: (usize, usize),
) -> (Option<String>, Option<String>) {
    for loc in index.values().filter(|l| l.sheet == sheet) {
        let n = loc.node.expect("indexed node");
        if let (Some(table), Some(range)) = (
            n.attributes.get("office::data::table"),
            n.attributes
                .get("grid::range")
                .and_then(|r| r["ref"].as_str()),
        ) {
            let (c, r) = coordinate(range);
            let row = at.1.checked_sub(r + 1);
            let col = at.0.checked_sub(c);
            if let (Some(row), Some(col)) = (row, col)
                && table["rows"].get(row).is_some()
                && table["columns"].get(col).is_some()
            {
                return (
                    table["rows"][row][0].as_str().map(String::from),
                    table["columns"][col]["name"].as_str().map(String::from),
                );
            }
        }
    }
    let values = cells(index, sheet);
    (
        values.get(&(1, at.1)).cloned(),
        values.get(&(at.0, 1)).cloned(),
    )
}
pub(super) fn enrich(
    before: &Composed,
    after: &Composed,
    changes: &mut [Value],
    old_pictures: &Pictures,
    new_pictures: &Pictures,
) {
    let old = index(before);
    let new = index(after);
    for item in changes {
        let path = item["technical"]["path"]
            .as_str()
            .or_else(|| item["technical"]["sheet"]["path"].as_str())
            .unwrap_or("");
        let a = old.get(path);
        let b = new.get(path);
        let Some(loc) = b.or(a) else {
            continue;
        };
        let node = loc.node.expect("indexed node");
        let mut locator = if !loc.sheet.is_empty() {
            format!("Sheet ‘{}’", loc.sheet)
        } else if loc.slide > 0 {
            format!("Slide {}: ‘{}’", loc.slide, loc.title)
        } else if node.attributes.contains_key("word::paragraph") {
            paragraph_locator(loc)
        } else if node.attributes.contains_key("word::table")
            || node.attributes.contains_key("office::data::table")
        {
            format!(
                "{}table containing ‘{}’",
                if loc.heading.is_empty() {
                    String::new()
                } else {
                    format!("Under ‘{}’, ", loc.heading)
                },
                snippet(&plain(node))
            )
        } else {
            "Document".into()
        };
        let mut was = lines(a, &item["before"]);
        let mut now = lines(b, &item["after"]);
        let mut kind = "text";
        if node.attributes.contains_key("word::paragraph") {
            if a.is_none() {
                was = gap_lines(loc, &old);
            }
            if b.is_none() {
                now = gap_lines(loc, &new);
            }
        }
        if loc.sheet.is_empty()
            && loc.slide == 0
            && node.attributes.contains_key("office::data::table")
            && let Some(row) = item["place"]
                .as_str()
                .and_then(|s| s.split("row ").nth(1))
                .and_then(|s| s.split(',').next())
                .and_then(|s| s.parse::<usize>().ok())
        {
            was = table_lines(a, &old, row);
            now = table_lines(b, &new, row);
            if row == 1 {
                locator.push_str(", header row");
            } else if let Some(label) =
                node.attributes["office::data::table"]["rows"][row - 2][0].as_str()
            {
                locator.push_str(&format!(", row ‘{label}’"));
            }
        }
        if !loc.sheet.is_empty() {
            let address = item["place"]
                .as_str()
                .and_then(|p| p.split("cell ").nth(1))
                .map(String::from)
                .or_else(|| {
                    node.attributes
                        .get("grid::range")
                        .and_then(|r| r["ref"].as_str())
                        .map(String::from)
                });
            if let Some(address) = address {
                let at = coordinate(&address);
                let (row_label, column_label) = cell_labels(&new, &loc.sheet, at);
                locator.push_str(&format!(", cell {address}"));
                if let Some(label) = row_label {
                    locator.push_str(&format!(", row ‘{label}’"));
                }
                if let Some(label) = column_label {
                    locator.push_str(&format!(", column ‘{label}’"));
                }
                was["grid"] = grid(&old, a.map(|l| l.sheet.as_str()).unwrap_or(&loc.sheet), at);
                now["grid"] = grid(&new, b.map(|l| l.sheet.as_str()).unwrap_or(&loc.sheet), at);
                kind = "grid";
            }
            was["lines"] = json!([{"text":readable_snapshot(a,&item["before"]),"changed":true}]);
            now["lines"] = json!([{"text":readable_snapshot(b,&item["after"]),"changed":true}]);
            if item["description"]
                .as_str()
                .is_some_and(|s| s.starts_with("Page setup changed"))
            {
                let (old_settings, new_settings) =
                    changed_settings(&item["before"], &item["after"]);
                was["lines"] = json!(old_settings);
                now["lines"] = json!(new_settings);
            }
            if let Some(number) = item["technical"]["chart"].as_u64() {
                let title = item["after"]["title"]
                    .as_str()
                    .or_else(|| item["before"]["title"].as_str())
                    .filter(|s| !s.is_empty());
                let name = title
                    .map(|s| format!("chart ‘{s}’"))
                    .unwrap_or_else(|| format!("Chart {number}"));
                locator.push_str(&format!(", {name}"));
                for (side, key) in [(&mut was, "before"), (&mut now, "after")] {
                    let chart = &item[key];
                    let dimensions = ["width", "height"]
                        .into_iter()
                        .filter_map(|k| {
                            chart["anchor"][k]
                                .as_f64()
                                .map(|n| format!("{k} {n:.2} px"))
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    let dimensions_only = item["description"]
                        .as_str()
                        .is_some_and(|s| s.contains("resized") || s.contains("moved"));
                    side["lines"] = json!([{"text":if dimensions_only{format!("{name}: {dimensions}")}else{value(chart)},"changed":true}]);
                }
            }
        }
        if loc.slide > 0 {
            if item["technical"]["attribute"] == "deck::slide"
                && item["description"]
                    .as_str()
                    .is_some_and(|d| d.starts_with("Notes changed"))
            {
                was["lines"] = json!([{"text":format!("Speaker notes: {}",value(&item["before"]["notes"])),"changed":true}]);
                now["lines"] = json!([{"text":format!("Speaker notes: {}",value(&item["after"]["notes"])),"changed":true}]);
                locator.push_str(", speaker notes");
            }
            let text = plain(node);
            if !text.is_empty() && text != loc.title {
                locator.push_str(&format!(", object ‘{}’", snippet(&text)));
            } else if let Some(name) = node
                .attributes
                .get("deck::frame")
                .and_then(|f| f["name"].as_str())
            {
                locator.push_str(&format!(", object ‘{name}’"));
            }
        }
        let mut old_media = Vec::new();
        let mut new_media = Vec::new();
        media(&item["before"], old_pictures, &mut old_media);
        media(&item["after"], new_pictures, &mut new_media);
        if !old_media.is_empty() || !new_media.is_empty() {
            was["media"] = json!(old_media);
            now["media"] = json!(new_media);
            kind = "media";
        }
        let old_text = a.map(|l| plain(l.node.unwrap())).unwrap_or_default();
        let new_text = b.map(|l| plain(l.node.unwrap())).unwrap_or_default();
        let spacing = node.attributes.contains_key("word::paragraph")
            && old_text.split_whitespace().eq(new_text.split_whitespace())
            && (a.is_none() || b.is_none() || old_text != new_text);
        item["locator"] = json!(locator);
        item["spacing_only"] = json!(spacing);
        item["context"] = json!({"kind":kind,"before":was,"after":now});
    }
}
fn media(v: &Value, pictures: &Pictures, out: &mut Vec<Value>) {
    if let Some(blob) = v.get("blob").and_then(Value::as_str) {
        let mut m = json!({"name":v["alt"].as_str().or_else(||v["name"].as_str()).unwrap_or("Picture"),"width":v.get("w").or_else(||v.get("width")),"height":v.get("h").or_else(||v.get("height"))});
        if let Some((mime, bytes)) = pictures.get(blob) {
            m["bytes"] = json!(bytes.len());
            if ["image/png", "image/jpeg", "image/gif", "image/webp"].contains(&mime.as_str()) {
                m["thumbnail"] = json!(format!("data:{mime};base64,{}", base64(bytes)));
            }
        }
        out.push(m);
    }
    match v {
        Value::Object(o) => {
            for v in o.values() {
                media(v, pictures, out)
            }
        }
        Value::Array(a) => {
            for v in a {
                media(v, pictures, out)
            }
        }
        _ => {}
    }
}
fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let n = ((c[0] as u32) << 16)
            | ((c.get(1).copied().unwrap_or(0) as u32) << 8)
            | c.get(2).copied().unwrap_or(0) as u32;
        for i in 0..4 {
            out.push(if i > c.len() {
                '='
            } else {
                ABC[((n >> (18 - i * 6)) & 63) as usize] as char
            });
        }
    }
    out
}
fn change_category(change: &Value) -> &'static str {
    if change["spacing_only"] == true {
        return "spacing change";
    }
    let description = change["description"].as_str().unwrap_or("");
    let attribute = change["technical"]["attribute"].as_str().unwrap_or("");
    if description.starts_with("Row added") {
        return "table row addition";
    }
    if description.starts_with("Row removed") {
        return "table row removal";
    }
    if attribute.is_empty() && ["added", "removed"].contains(&change["kind"].as_str().unwrap_or(""))
    {
        let added = change["kind"] == "added";
        return match description.split_whitespace().next().unwrap_or("Object") {
            "Paragraph" => {
                if added {
                    "paragraph addition"
                } else {
                    "paragraph removal"
                }
            }
            "Sheet" => {
                if added {
                    "sheet addition"
                } else {
                    "sheet removal"
                }
            }
            "Slide" => {
                if added {
                    "slide addition"
                } else {
                    "slide removal"
                }
            }
            "Picture" => {
                if added {
                    "picture addition"
                } else {
                    "picture removal"
                }
            }
            "Table" => {
                if added {
                    "table addition"
                } else {
                    "table removal"
                }
            }
            "Cell" => {
                if added {
                    "cell addition"
                } else {
                    "cell removal"
                }
            }
            _ => {
                if added {
                    "object addition"
                } else {
                    "object removal"
                }
            }
        };
    }
    if description.starts_with("Chart ") || attribute == "deck::chart" {
        return "chart change";
    }
    if description.starts_with("Picture") || attribute == "deck::picture" {
        return "picture change";
    }
    if description.starts_with("Page setup") {
        return "page setup change";
    }
    if description.starts_with("Sheet renamed") {
        return "sheet rename";
    }
    if description.starts_with("Notes changed") {
        return "speaker note edit";
    }
    match attribute {
        "word::paragraph" => "paragraph edit",
        "office::data::table" | "deck::table" => {
            if description.contains("formatting") {
                "table formatting change"
            } else {
                "table value change"
            }
        }
        "word::table" => "table change",
        "office::data::value" => "cell value change",
        "grid::formula" => "formula change",
        "grid::range" => "cell or range move",
        "grid::sheet" => "sheet setting change",
        "grid::workbook" => "workbook setting change",
        "deck::text" => "text edit",
        "deck::frame" | "deck::slide" | "deck::deck" => "layout change",
        "word::document" => "document setting change",
        _ => {
            if description.contains("formatting") {
                "formatting change"
            } else if description.contains("reordered") {
                "content reorder"
            } else {
                "content change"
            }
        }
    }
}
fn common_place(changes: &[Value]) -> String {
    let place = |change: &Value| {
        change["locator"].as_str().and_then(|s| {
            if s.starts_with("Sheet ‘") {
                s.split_once('’')
                    .map(|(p, _)| format!("{}’", p.replacen("Sheet", "sheet", 1)))
            } else if s.starts_with("Slide ") {
                s.split_once(':').map(|(p, _)| p.to_lowercase())
            } else {
                None
            }
        })
    };
    let Some(first) = changes.first().and_then(place) else {
        return String::new();
    };
    if changes.iter().all(|c| place(c).as_ref() == Some(&first)) {
        format!(" on {first}")
    } else {
        String::new()
    }
}
pub(super) fn summary(format: &str, changes: &[Value]) -> String {
    if changes.is_empty() {
        return "No content changes. The file was re-saved.".into();
    }
    if changes.len() == 1 && changes[0]["spacing_only"] == true {
        let c = &changes[0];
        let locator = c["locator"].as_str().unwrap_or("in the document");
        let after = locator
            .split("after the paragraph starting ‘")
            .nth(1)
            .and_then(|s| s.strip_suffix('’'));
        let action = if c["kind"] == "removed" {
            "removed"
        } else {
            "added"
        };
        if let Some(text) = after {
            let words = text
                .split_whitespace()
                .take(3)
                .collect::<Vec<_>>()
                .join(" ");
            let quote = if text.split_whitespace().count() > 3 {
                format!("{}…", words.trim_end_matches([',', '.']))
            } else {
                words
            };
            return format!(
                "One change: an empty line was {action} after ‘{quote}’. Nothing else changed."
            );
        }
        return format!(
            "One change: empty lines or spacing changed {locator}. Nothing else changed."
        );
    }
    if format == "xlsx" {
        let sheets: std::collections::BTreeSet<_> = changes
            .iter()
            .filter_map(|c| {
                c["locator"]
                    .as_str()
                    .and_then(|l| l.split('’').next())
                    .map(|s| format!("{s}’"))
            })
            .collect();
        if sheets.len() == 1
            && changes.iter().all(|c| {
                c["description"].as_str().is_some_and(|d| {
                    d.starts_with("Page setup")
                        || d.starts_with("Chart ") && d.contains("resized")
                        || d == "Pictures changed"
                })
            })
        {
            let pages = changes
                .iter()
                .filter(|c| {
                    c["description"]
                        .as_str()
                        .is_some_and(|d| d.starts_with("Page setup"))
                })
                .count();
            let charts = changes
                .iter()
                .filter(|c| {
                    c["description"]
                        .as_str()
                        .is_some_and(|d| d.starts_with("Chart "))
                })
                .count();
            let pictures = changes
                .iter()
                .filter(|c| c["description"] == "Pictures changed")
                .count();
            let mut categories = Vec::new();
            if pages > 0 {
                categories.push("page setup".into());
            }
            if charts > 0 {
                categories.push(format!(
                    "{charts} chart {}",
                    if charts == 1 { "size" } else { "sizes" }
                ));
            }
            if pictures > 0 {
                categories.push(format!(
                    "{pictures} picture {}",
                    if pictures == 1 { "size" } else { "sizes" }
                ));
            }
            return format!(
                "{} changes on {}: {}.",
                changes.len(),
                sheets.iter().next().unwrap().replacen("Sheet", "sheet", 1),
                categories.join(", ")
            );
        }
    }
    if changes.len() == 1 && format == "pptx" {
        return format!(
            "One change: text changed on {}.",
            changes[0]["locator"]
                .as_str()
                .unwrap_or("a slide")
                .split(':')
                .next()
                .unwrap_or("a slide")
                .to_lowercase()
        );
    }
    if changes.len() == 1 {
        return format!(
            "One change at {}: {}.",
            changes[0]["locator"].as_str().unwrap_or("the document"),
            changes[0]["description"]
                .as_str()
                .unwrap_or("content changed")
                .trim_end_matches('.')
        );
    }
    let mut categories = BTreeMap::new();
    for change in changes {
        *categories.entry(change_category(change)).or_insert(0usize) += 1;
    }
    let detail = categories
        .into_iter()
        .map(|(category, count)| format!("{count} {category}{}", if count == 1 { "" } else { "s" }))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{} changes{}: {detail}.",
        changes.len(),
        common_place(changes)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removed_empty_deck_object_shows_name_and_dimensions_without_internal_paths() {
        let object = node(
            "SENTINEL_INTERNAL_PATH",
            json!({"deck::frame":{"name":"Accent bar","x":12,"y":20,"w":100,"h":8},"deck::shape":{"kind":"rectangle"}}),
            vec![],
        );
        let old = node(
            "deck",
            json!({"deck::deck":{}}),
            vec![node(
                "slide",
                json!({"deck::slide":{}}),
                vec![object.clone()],
            )],
        );
        let mut new = old.clone();
        new.children[0].children.clear();
        let mut changes = vec![
            json!({"technical":{"path":"SENTINEL_INTERNAL_PATH"},"kind":"removed","before":serde_json::to_value(&object).unwrap(),"after":null}),
        ];
        enrich(&old, &new, &mut changes, &Pictures::new(), &Pictures::new());
        let primary = changes[0]["context"]["before"]["lines"][0]["text"]
            .as_str()
            .unwrap();
        assert!(primary.contains("Accent bar"));
        assert!(primary.contains("width: 100 px"));
        assert!(primary.contains("height: 8 px"));
        assert!(!primary.contains("SENTINEL"));
        assert!(!primary.contains("path"));
        assert!(!primary.contains("attributes"));
        assert!(!primary.contains("empty cell"));
        assert_eq!(changes[0]["before"]["path"], "SENTINEL_INTERNAL_PATH");
    }
    #[test]
    fn page_setup_context_names_only_changed_settings_without_float_noise() {
        let before = json!({"print":{"orientation":"Portrait","paper":"A4","title_rows":[1,2],"margins":[0.69999999999999,0.7,0.75,0.75,0.3,0.3]},"defaults":{"colWidth":80,"rowHeight":20}});
        let after = json!({"print":{"orientation":"Portrait","paper":"A4","title_rows":[1,2],"margins":[1,0.7,0.75,0.75,0.3,0.3]},"defaults":{"colWidth":90,"rowHeight":20}});
        let old = workbook("Graph", 1, None);
        let new = old.clone();
        let mut changes = vec![
            json!({"technical":{"path":"sheet","attribute":"grid::sheet"},"description":"Page setup changed","before":before,"after":after}),
        ];
        enrich(&old, &new, &mut changes, &Pictures::new(), &Pictures::new());
        for (side, expected) in [
            (
                "before",
                vec!["Default column width: 80 px", "Left margin: 0.7 in"],
            ),
            (
                "after",
                vec!["Default column width: 90 px", "Left margin: 1 in"],
            ),
        ] {
            let texts: Vec<_> = changes[0]["context"][side]["lines"]
                .as_array()
                .unwrap()
                .iter()
                .map(|l| l["text"].as_str().unwrap())
                .collect();
            assert_eq!(texts, expected);
        }
        assert_eq!(changes[0]["technical"]["attribute"], "grid::sheet");
        assert_eq!(changes[0]["before"], before);
    }
    #[test]
    fn mixed_change_summary_answers_what_changed_with_counts_and_locations() {
        let changes = vec![
            json!({"technical":{"attribute":"word::paragraph"},"description":"Text changed"}),
            json!({"technical":{"attribute":"word::paragraph"},"description":"Text changed"}),
            json!({"technical":{"attribute":"office::data::table"},"description":"541500 → 999"}),
            json!({"technical":{"node":{}},"description":"Object added: Owner","kind":"added"}),
        ];
        assert_eq!(
            summary("docx", &changes),
            "4 changes: 1 object addition, 2 paragraph edits, 1 table value change."
        );
        let grid = vec![
            json!({"technical":{"attribute":"office::data::value"},"locator":"Sheet ‘Revenue’, cell B2"}),
            json!({"technical":{"attribute":"grid::formula"},"locator":"Sheet ‘Revenue’, cell B5"}),
        ];
        assert_eq!(
            summary("xlsx", &grid),
            "2 changes on sheet ‘Revenue’: 1 cell value change, 1 formula change."
        );
    }
    fn node(path: &str, attributes: Value, children: Vec<Composed>) -> Composed {
        Composed {
            path: path.into(),
            name: None,
            attributes: attributes.as_object().unwrap().clone(),
            provenance: BTreeMap::new(),
            children,
        }
    }
    fn workbook(sheet: &str, number: i32, chart: Option<Value>) -> Composed {
        let mut settings = json!({"name":sheet});
        if let Some(chart) = chart {
            settings["charts"] = json!([chart]);
        }
        node(
            "book",
            json!({"grid::workbook":{}}),
            vec![node(
                "sheet",
                json!({"grid::sheet":settings}),
                vec![node(
                    "cell",
                    json!({"grid::range":{"ref":"B2"},"office::data::value":{"value":number}}),
                    vec![],
                )],
            )],
        )
    }
    fn document(text: Option<&str>) -> Composed {
        let node = |path: &str, attributes: serde_json::Map<String, Value>, children| Composed {
            path: path.into(),
            name: None,
            attributes,
            provenance: BTreeMap::new(),
            children,
        };
        let children = text
            .map(|t| {
                vec![node(
                    "paragraph",
                    serde_json::Map::from_iter([(
                        "word::paragraph".into(),
                        json!({"runs":[{"text":t}]}),
                    )]),
                    Vec::new(),
                )]
            })
            .unwrap_or_default();
        node(
            "document",
            serde_json::Map::from_iter([("word::document".into(), json!({}))]),
            children,
        )
    }
    #[test]
    fn spacing_groups_internal_whitespace_without_calling_it_an_empty_line() {
        let old = document(Some("Two words here"));
        let new = document(Some("Two  words\there"));
        let mut changes = vec![
            json!({"technical":{"path":"paragraph"},"before":"Two words here","after":"Two  words\there","kind":"changed"}),
        ];
        enrich(&old, &new, &mut changes, &Pictures::new(), &Pictures::new());
        assert_eq!(changes[0]["spacing_only"], true);
        let answer = summary("docx", &changes);
        assert!(answer.contains("spacing changed"));
        assert!(!answer.contains("empty line was added"));
        let mut insertion =
            vec![json!({"technical":{"path":"paragraph"},"before":null,"after":"","kind":"added"})];
        enrich(
            &document(None),
            &document(Some("")),
            &mut insertion,
            &Pictures::new(),
            &Pictures::new(),
        );
        assert_eq!(insertion[0]["spacing_only"], true);
        let mut substantive = vec![
            json!({"technical":{"path":"paragraph"},"before":"Two words here","after":"Three words here","kind":"changed"}),
        ];
        enrich(
            &old,
            &document(Some("Three words here")),
            &mut substantive,
            &Pictures::new(),
            &Pictures::new(),
        );
        assert_eq!(substantive[0]["spacing_only"], false);
    }
    #[test]
    fn renamed_sheet_keeps_both_grid_snapshots() {
        let old = workbook("Original", 10, None);
        let new = workbook("Renamed", 20, None);
        let mut changes = vec![
            json!({"place":"Sheet Renamed, cell B2","technical":{"path":"cell"},"before":10,"after":20}),
        ];
        enrich(&old, &new, &mut changes, &Pictures::new(), &Pictures::new());
        let cells = |side: &str| {
            changes[0]["context"][side]["grid"]["rows"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|r| r["cells"].as_array().unwrap())
                .map(|c| c["text"].as_str().unwrap())
                .collect::<Vec<_>>()
                .join(" ")
        };
        assert!(cells("before").contains("10"));
        assert!(cells("after").contains("20"));
    }
    #[test]
    fn table_header_change_never_highlights_the_first_data_row() {
        let old = node(
            "doc",
            json!({"word::document":{}}),
            vec![node(
                "table",
                json!({"office::data::table":{"columns":[{"name":"Region"}],"rows":[["Adelaide"]]}}),
                vec![],
            )],
        );
        let new = node(
            "doc",
            json!({"word::document":{}}),
            vec![node(
                "table",
                json!({"office::data::table":{"columns":[{"name":"City"}],"rows":[["Adelaide"]]}}),
                vec![],
            )],
        );
        let mut changes = vec![
            json!({"place":"Table 1, row 1, column 1","technical":{"path":"table"},"before":{"name":"Region"},"after":{"name":"City"}}),
        ];
        enrich(&old, &new, &mut changes, &Pictures::new(), &Pictures::new());
        assert!(
            changes[0]["locator"]
                .as_str()
                .unwrap()
                .ends_with("header row")
        );
        for (side, expected) in [("before", "Region"), ("after", "City")] {
            let highlighted: Vec<_> = changes[0]["context"][side]["lines"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|l| l["changed"] == true)
                .collect();
            assert_eq!(highlighted.len(), 1);
            assert_eq!(highlighted[0]["text"], expected);
        }
    }
    #[test]
    fn added_cell_snapshot_shows_the_value_without_its_internal_node() {
        let new = workbook("Sheet1", 42, None);
        let mut old = new.clone();
        old.children[0].children.clear();
        let raw = serde_json::to_value(&new.children[0].children[0]).unwrap();
        let mut changes = vec![
            json!({"place":"Sheet Sheet1, cell B2","technical":{"path":"cell"},"before":null,"after":raw}),
        ];
        enrich(&old, &new, &mut changes, &Pictures::new(), &Pictures::new());
        assert_eq!(changes[0]["context"]["after"]["lines"][0]["text"], "42");
        let sheet_raw = serde_json::to_value(&new.children[0]).unwrap();
        assert!(!readable_snapshot(index(&new).get("sheet"), &sheet_raw).contains("path"));
    }
    #[test]
    fn chart_title_and_series_edits_show_the_changed_properties() {
        for (before, after, expected_old, expected_new) in [
            (
                json!({"title":"Old title","anchor":{"width":100,"height":80}}),
                json!({"title":"New title","anchor":{"width":100,"height":80}}),
                "Old title",
                "New title",
            ),
            (
                json!({"series":[{"name":"Sales","values":"B2:B3"}]}),
                json!({"series":[{"name":"Sales","values":"B2:B4"}]}),
                "B2:B3",
                "B2:B4",
            ),
        ] {
            let old = workbook("Charts", 10, Some(before.clone()));
            let new = workbook("Charts", 10, Some(after.clone()));
            let mut changes = vec![
                json!({"place":"Sheet Charts","description":"Chart 1 data or title changed","technical":{"chart":1,"sheet":{"path":"sheet"}},"before":before,"after":after}),
            ];
            enrich(&old, &new, &mut changes, &Pictures::new(), &Pictures::new());
            assert!(
                changes[0]["context"]["before"]["lines"][0]["text"]
                    .as_str()
                    .unwrap()
                    .contains(expected_old)
            );
            assert!(
                changes[0]["context"]["after"]["lines"][0]["text"]
                    .as_str()
                    .unwrap()
                    .contains(expected_new)
            );
        }
    }
    #[test]
    fn speaker_notes_context_shows_notes_instead_of_unchanged_slide_text() {
        let old = node(
            "deck",
            json!({"deck::deck":{}}),
            vec![node(
                "slide",
                json!({"deck::slide":{"notes":"Old note"}}),
                vec![node(
                    "title",
                    json!({"deck::text":{"paragraphs":[{"runs":[{"text":"Visible title"}]}]}}),
                    vec![],
                )],
            )],
        );
        let mut new = old.clone();
        new.children[0].attributes.get_mut("deck::slide").unwrap()["notes"] = json!("New note");
        let mut changes = vec![
            json!({"description":"Notes changed: 'Old note' → 'New note'","technical":{"path":"slide","attribute":"deck::slide"},"before":{"notes":"Old note"},"after":{"notes":"New note"}}),
        ];
        enrich(&old, &new, &mut changes, &Pictures::new(), &Pictures::new());
        assert_eq!(
            changes[0]["context"]["before"]["lines"][0]["text"],
            "Speaker notes: Old note"
        );
        assert_eq!(
            changes[0]["context"]["after"]["lines"][0]["text"],
            "Speaker notes: New note"
        );
    }
}
