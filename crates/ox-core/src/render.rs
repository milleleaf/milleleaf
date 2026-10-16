//! Model → HTML views of a composed tree.
//!
//! Decks render to the OX HTML deck profile, so the output can be
//! measured back into a layer; every element carries `data-ox-path`, so a re-import edits the
//! same nodes instead of creating new ones. Documents render to plain semantic HTML.

use std::fmt::Write as _;

use serde_json::{Map, Value};

use crate::compose::Composed;

pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            _ => o.push(c),
        }
    }
    o
}

/// `1250500` → `1,250,500`; keeps up to two decimals.
pub fn group_thousands(n: f64) -> String {
    let neg = n < 0.0;
    let s = format!("{:.2}", n.abs());
    let (int, frac) = s.split_once('.').unwrap_or((&s, "00"));
    let mut out = String::new();
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    if frac != "00" {
        out.push('.');
        out.push_str(frac);
    }
    if neg { format!("-{out}") } else { out }
}

/// Formats a cell or value by an `office::data` column type / value format.
pub fn format_value(v: &Value, format: &str) -> String {
    match (v.as_f64(), format) {
        (Some(n), "currency") => format!("${}", group_thousands(n)),
        (Some(n), "percent") => format!("{}%", group_thousands(n * 100.0)),
        (Some(n), "number") => group_thousands(n),
        _ => match v {
            Value::String(s) => s.clone(),
            Value::Null => String::new(),
            other => other.to_string(),
        },
    }
}

fn attr<'a>(n: &'a Composed, key: &str) -> Option<&'a Map<String, Value>> {
    n.attributes.get(key).and_then(Value::as_object)
}

fn s<'a>(m: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    m.get(key).and_then(Value::as_str)
}

fn f(m: &Map<String, Value>, key: &str) -> f64 {
    m.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

/// Finds the first node in `trees` (depth first) whose path is `path`.
pub fn find<'a>(trees: &'a [Composed], path: &str) -> Option<&'a Composed> {
    for t in trees {
        if t.path == path {
            return Some(t);
        }
        if let Some(x) = find(&t.children, path) {
            return Some(x);
        }
    }
    None
}

/// The text a `{"field": name}` run shows: the named child's `office::data::value`.
pub fn field_text(owner: &Composed, name: &str) -> String {
    let Some(child) = owner
        .children
        .iter()
        .find(|c| c.name.as_deref() == Some(name))
    else {
        return format!("[missing field {name}]");
    };
    match attr(child, "office::data::value") {
        Some(v) => format_value(
            v.get("value").unwrap_or(&Value::Null),
            s(v, "format").unwrap_or("number"),
        ),
        None => format!("[field {name} has no value]"),
    }
}

fn run_style(r: &Map<String, Value>) -> String {
    let mut style = String::new();
    if r.get("bold").and_then(Value::as_bool) == Some(true) {
        style.push_str("font-weight:700;");
    }
    if r.get("italic").and_then(Value::as_bool) == Some(true) {
        style.push_str("font-style:italic;");
    }
    if r.get("underline").and_then(Value::as_bool) == Some(true) {
        style.push_str("text-decoration:underline;");
    }
    if let Some(c) = s(r, "color") {
        let _ = write!(style, "color:{};", esc(c));
    }
    if let Some(fam) = s(r, "font") {
        let _ = write!(style, "font-family:'{}';", esc(fam));
    }
    if let Some(pt) = r.get("sizePt").and_then(Value::as_f64) {
        let _ = write!(style, "font-size:{pt}pt;");
    }
    style
}

fn runs_html(owner: &Composed, runs: &[Value]) -> String {
    let mut o = String::new();
    for r in runs {
        let Some(r) = r.as_object() else { continue };
        if let Some(field) = s(r, "field") {
            let child_path = owner
                .children
                .iter()
                .find(|c| c.name.as_deref() == Some(field))
                .map(|c| c.path.as_str())
                .unwrap_or("");
            let link = owner
                .children
                .iter()
                .find(|c| c.name.as_deref() == Some(field))
                .and_then(|c| attr(c, "ox::link"))
                .and_then(|l| s(l, "source"))
                .unwrap_or("");
            let _ = write!(
                o,
                "<span data-ox-field=\"{}\" data-ox-path=\"{}\" data-ox-link=\"{}\" style=\"{}\">{}</span>",
                esc(field),
                esc(child_path),
                esc(link),
                run_style(r),
                esc(&field_text(owner, field))
            );
            continue;
        }
        let text = esc(s(r, "text").unwrap_or("")).replace('\n', "<br>");
        let style = run_style(r);
        if style.is_empty() {
            o.push_str(&text);
        } else {
            let _ = write!(o, "<span style=\"{style}\">{text}</span>");
        }
    }
    o
}

fn table_html(n: &Composed, extra_attrs: &str, style: &str) -> String {
    let mut o = String::new();
    let mut style = style.to_string();
    if let Some(ts) = attr(n, "deck::table")
        .and_then(|t| t.get("style"))
        .and_then(Value::as_object)
    {
        if let Some(fam) = s(ts, "font") {
            let _ = write!(style, "font-family:'{}';", esc(fam));
        }
        if let Some(pt) = ts.get("sizePt").and_then(Value::as_f64) {
            let _ = write!(style, "font-size:{pt}pt;");
        }
    }
    let _ = write!(
        o,
        "<table data-ox-path=\"{}\"{extra_attrs} style=\"{style}\">",
        esc(&n.path)
    );
    if let Some(t) = attr(n, "office::data::table") {
        let cols: Vec<&Map<String, Value>> = t
            .get("columns")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_object).collect())
            .unwrap_or_default();
        o.push_str("<thead><tr>");
        for c in &cols {
            let _ = write!(o, "<th>{}</th>", esc(s(c, "name").unwrap_or("")));
        }
        o.push_str("</tr></thead><tbody>");
        for row in t
            .get("rows")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            o.push_str("<tr>");
            for (i, cell) in row.as_array().into_iter().flatten().enumerate() {
                let fmt = cols.get(i).and_then(|c| s(c, "type")).unwrap_or("text");
                let align = if matches!(fmt, "currency" | "number" | "percent") {
                    " style=\"text-align:right\""
                } else {
                    ""
                };
                let _ = write!(o, "<td{align}>{}</td>", esc(&format_value(cell, fmt)));
            }
            o.push_str("</tr>");
        }
        o.push_str("</tbody>");
    } else if let Some(snap) = attr(n, "deck::table")
        .and_then(|t| t.get("snapshot"))
        .and_then(Value::as_array)
    {
        for row in snap {
            o.push_str("<tr>");
            for cell in row.as_array().into_iter().flatten() {
                let text = cell.get("text").and_then(Value::as_str).unwrap_or("");
                let _ = write!(o, "<td>{}</td>", esc(text));
            }
            o.push_str("</tr>");
        }
    }
    o.push_str("</table>");
    o
}

fn link_attr(n: &Composed) -> String {
    match attr(n, "ox::link") {
        Some(l) => format!(
            " data-ox-link=\"{}\" data-ox-link-mode=\"{}\"",
            esc(s(l, "source").unwrap_or("")),
            esc(s(l, "mode").unwrap_or("live"))
        ),
        None => String::new(),
    }
}

/// Renders a composed `deck::deck` node as an OX HTML deck.
pub fn deck_html(deck: &Composed) -> String {
    let d = attr(deck, "deck::deck").cloned().unwrap_or_default();
    let (w, h) = (f(&d, "width").max(1.0), f(&d, "height").max(1.0));
    let title = s(&d, "title").unwrap_or("Deck");
    let mut o = String::new();
    let _ = write!(
        o,
        "<!doctype html>\n<html data-ox-profile=\"deck/0\"><head><meta charset=\"utf-8\"><title>{}</title>\n<style>\nbody{{margin:0;background:#888}}\nsection[data-ox-slide]{{position:relative;overflow:hidden;width:{w}px;height:{h}px;margin:0 auto 24px;background:#fff;font-family:Inter,Arial,sans-serif}}\n[data-ox-frame]{{position:absolute;box-sizing:border-box;margin:0}}\n[data-ox-frame] p{{margin:0}}\n[data-ox-frame] p[data-ox-bullet]::before{{content:\"\\2022\\00a0\";margin-left:-1em;display:inline-block;width:1em}}\ntable[data-ox-frame]{{border-collapse:collapse}}\naside[data-ox-notes]{{display:none}}\ntable[data-ox-frame] td,table[data-ox-frame] th{{border:1px solid #ccc;padding:6px 10px}}\n</style></head>\n<body>\n<main data-ox-deck data-ox-path=\"{}\" data-ox-width=\"{w}\" data-ox-height=\"{h}\">\n",
        esc(title),
        esc(&deck.path)
    );
    for slide in &deck.children {
        let sa = attr(slide, "deck::slide").cloned().unwrap_or_default();
        let bg = s(&sa, "background")
            .map(|b| format!(" style=\"background:{}\"", esc(b)))
            .unwrap_or_default();
        let _ = writeln!(
            o,
            "<section data-ox-slide data-ox-path=\"{}\" data-ox-name=\"{}\"{bg}>",
            esc(&slide.path),
            esc(slide.name.as_deref().unwrap_or(""))
        );
        for fr in &slide.children {
            o.push_str(&frame_html(fr));
            o.push('\n');
        }
        if let Some(notes) = s(&sa, "notes") {
            let _ = writeln!(o, "<aside data-ox-notes>{}</aside>", esc(notes));
        }
        o.push_str("</section>\n");
    }
    o.push_str("</main>\n</body></html>\n");
    o
}

fn frame_html(fr: &Composed) -> String {
    let g = attr(fr, "deck::frame").cloned().unwrap_or_default();
    let mut style = format!(
        "left:{}px;top:{}px;width:{}px;height:{}px;",
        f(&g, "x"),
        f(&g, "y"),
        f(&g, "w"),
        f(&g, "h")
    );
    if let Some(r) = g
        .get("rotation")
        .and_then(Value::as_f64)
        .filter(|r| *r != 0.0)
    {
        let _ = write!(style, "transform:rotate({r}deg);");
    }
    let name = esc(fr.name.as_deref().unwrap_or(""));
    let common = format!(
        " data-ox-path=\"{}\" data-ox-name=\"{name}\"",
        esc(&fr.path)
    );
    if let Some(t) = attr(fr, "deck::text") {
        let mut inner = String::new();
        for p in t
            .get("paragraphs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(p) = p.as_object() else { continue };
            let mut ps = String::new();
            if let Some(a) = s(p, "align") {
                let _ = write!(ps, "text-align:{};", esc(a));
            }
            let level = p.get("level").and_then(Value::as_u64).unwrap_or(0);
            let bullet = p.get("bullet").and_then(Value::as_bool) == Some(true);
            if bullet {
                let _ = write!(ps, "margin-left:{}em;", 1.2 * (level + 1) as f64);
            }
            let runs = runs_html(
                fr,
                p.get("runs")
                    .and_then(Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
            );
            // The bullet glyph comes from CSS (`::before`), so it never becomes part of the text.
            let _ = write!(
                inner,
                "<p data-ox-level=\"{level}\"{} style=\"{ps}\">{runs}</p>",
                if bullet { " data-ox-bullet" } else { "" }
            );
        }
        let role = s(t, "role").unwrap_or("body");
        return format!(
            "<div data-ox-frame=\"text\" data-ox-role=\"{}\"{common} style=\"{style}\">{inner}</div>",
            esc(role)
        );
    }
    if let Some(c) = attr(fr, "deck::chart") {
        return chart_svg(fr, c, f(&g, "w"), f(&g, "h"), &common, &style);
    }
    if attr(fr, "deck::table").is_some() || attr(fr, "office::data::table").is_some() {
        let extra = format!(
            " data-ox-frame=\"table\" data-ox-name=\"{name}\"{}",
            link_attr(fr)
        );
        return table_html(fr, &extra, &style);
    }
    for (key, kind) in [("deck::picture", "picture"), ("deck::vector", "vector")] {
        if let Some(p) = attr(fr, key) {
            let blob = p.get("blob").and_then(Value::as_str).unwrap_or("");
            // ponytail: the path is right for a view saved at the project root; elsewhere the image
            // breaks. Embed it as a data: URI if views are moved around.
            let digest_path;
            let src = match blob.strip_prefix("sha256-") {
                Some(hex) => {
                    digest_path = format!("{}/{hex}", crate::BLOB_DIR);
                    digest_path.as_str()
                }
                None => blob,
            };
            let alt = s(p, "alt").unwrap_or("");
            return format!(
                "<img data-ox-frame=\"{kind}\"{common} src=\"{}\" alt=\"{}\" style=\"{style}\">",
                esc(src),
                esc(alt)
            );
        }
    }
    if let Some(sh) = attr(fr, "deck::shape") {
        if let Some(fill) = s(sh, "fill") {
            let _ = write!(style, "background:{};", esc(fill));
        }
        if let Some(stroke) = s(sh, "stroke") {
            let _ = write!(
                style,
                "border:{}px solid {};",
                f(sh, "strokeWidth").max(1.0),
                esc(stroke)
            );
        }
        match s(sh, "kind") {
            Some("ellipse") => style.push_str("border-radius:50%;"),
            Some("roundRect") => {
                let _ = write!(style, "border-radius:{}px;", f(sh, "radius"));
            }
            _ => {}
        }
        return format!("<div data-ox-frame=\"shape\"{common} style=\"{style}\"></div>");
    }
    format!("<div data-ox-frame=\"unknown\"{common} style=\"{style}\"></div>")
}

/// Renders a composed `word::document` node as simple semantic HTML.
pub fn document_html(doc: &Composed) -> String {
    let d = attr(doc, "word::document").cloned().unwrap_or_default();
    let mut o = format!(
        "<!doctype html>\n<html><head><meta charset=\"utf-8\"><title>{}</title>\n<style>body{{font-family:Georgia,serif;max-width:42em;margin:2em auto;line-height:1.5}}table{{border-collapse:collapse}}td,th{{border:1px solid #ccc;padding:4px 10px}}caption{{caption-side:bottom;font-size:.85em;color:#555}}</style></head>\n<body data-ox-path=\"{}\">\n",
        esc(s(&d, "title").unwrap_or("Document")),
        esc(&doc.path)
    );
    for block in &doc.children {
        if let Some(p) = attr(block, "word::paragraph") {
            let tag = match s(p, "style") {
                Some("Title") | Some("Heading 1") => "h1",
                Some("Heading 2") => "h2",
                Some("Heading 3") => "h3",
                _ => "p",
            };
            let runs = runs_html(
                block,
                p.get("runs")
                    .and_then(Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
            );
            let _ = writeln!(
                o,
                "<{tag} data-ox-path=\"{}\">{runs}</{tag}>",
                esc(&block.path)
            );
        } else if attr(block, "office::data::table").is_some() {
            let caption = attr(block, "word::table")
                .and_then(|t| s(t, "caption"))
                .map(String::from);
            let mut t = table_html(block, &link_attr(block), "");
            if let Some(c) = caption {
                t = t.replacen(
                    "<thead>",
                    &format!("<caption>{}</caption><thead>", esc(&c)),
                    1,
                );
            }
            o.push_str(&t);
            o.push('\n');
        }
    }
    o.push_str("</body></html>\n");
    o
}

/// Default series colours for charts drawn from data.
pub const CHART_COLORS: [&str; 4] = ["#1b6a8f", "#f2b134", "#5aa469", "#c4515a"];

/// Category labels and one value series per `values` column, from the frame's
/// `office::data::table` (inherited from the source when the chart is linked).
pub fn chart_data(fr: &Composed, c: &Map<String, Value>) -> (Vec<String>, Vec<(String, Vec<f64>)>) {
    let Some(t) = attr(fr, "office::data::table") else {
        return (vec![], vec![]);
    };
    let cols: Vec<&str> = t
        .get("columns")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|c| c.get("name").and_then(Value::as_str).unwrap_or(""))
                .collect()
        })
        .unwrap_or_default();
    let rows: Vec<&Vec<Value>> = t
        .get("rows")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_array).collect())
        .unwrap_or_default();
    let idx = |name: &str| cols.iter().position(|c| *c == name);
    let cat = s(c, "category").and_then(idx).unwrap_or(0);
    let cats = rows
        .iter()
        .map(|r| {
            r.get(cat)
                .map(|v| {
                    v.as_str()
                        .map(String::from)
                        .unwrap_or_else(|| v.to_string())
                })
                .unwrap_or_default()
        })
        .collect();
    let series = c
        .get("values")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|name| {
            idx(name).map(|i| {
                (
                    name.to_string(),
                    rows.iter()
                        .map(|r| r.get(i).and_then(Value::as_f64).unwrap_or(0.0))
                        .collect(),
                )
            })
        })
        .collect();
    (cats, series)
}

/// A `deck::chart` frame as inline SVG drawn from its data; its `data-ox-*` attributes let
/// `html2ox` read it back as the same chart node.
fn chart_svg(
    fr: &Composed,
    c: &Map<String, Value>,
    w: f64,
    h: f64,
    common: &str,
    style: &str,
) -> String {
    let kind = s(c, "kind").unwrap_or("column");
    let (cats, series) = chart_data(fr, c);
    let colors: Vec<String> = c
        .get("colors")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .filter(|v: &Vec<String>| !v.is_empty())
        .unwrap_or_else(|| CHART_COLORS.iter().map(|s| s.to_string()).collect());
    let values: Vec<String> = series.iter().map(|(n, _)| n.clone()).collect();
    let mut o = format!(
        "<svg data-ox-frame=\"chart\" data-ox-chart=\"{}\" data-ox-category=\"{}\" data-ox-values=\"{}\"{}{common} viewBox=\"0 0 {w} {h}\" role=\"img\" aria-label=\"{}\" style=\"{style}\">",
        esc(kind),
        esc(s(c, "category").unwrap_or("")),
        esc(&values.join(",")),
        link_attr(fr),
        esc(s(c, "title").unwrap_or("chart"))
    );
    let label_h = 26.0;
    let max = series
        .iter()
        .flat_map(|(_, v)| v.iter().copied())
        .fold(0.0_f64, f64::max)
        .max(1e-9);
    let n = cats.len().max(1) as f64;
    let ns = series.len().max(1) as f64;
    let plot_h = (h - label_h).max(1.0);
    let slot = w / n;
    match kind {
        "line" => {
            for (si, (_, vals)) in series.iter().enumerate() {
                let pts: Vec<String> = vals
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        format!(
                            "{:.1},{:.1}",
                            slot * (i as f64 + 0.5),
                            plot_h - v / max * (plot_h - 4.0)
                        )
                    })
                    .collect();
                let _ = write!(
                    o,
                    "<polyline fill=\"none\" stroke=\"{}\" stroke-width=\"3\" points=\"{}\"/>",
                    esc(&colors[si % colors.len()]),
                    pts.join(" ")
                );
            }
        }
        "bar" => {
            let band = plot_h.min(h) / n;
            let lw = w * 0.25;
            for (i, cat) in cats.iter().enumerate() {
                for (si, (_, vals)) in series.iter().enumerate() {
                    let bh = band * 0.7 / ns;
                    let y = band * i as f64 + band * 0.15 + bh * si as f64;
                    let bw = vals.get(i).copied().unwrap_or(0.0) / max * (w - lw - 4.0);
                    let _ = write!(
                        o,
                        "<rect x=\"{lw:.1}\" y=\"{y:.1}\" width=\"{bw:.1}\" height=\"{bh:.1}\" fill=\"{}\"/>",
                        esc(&colors[si % colors.len()])
                    );
                }
                let _ = write!(
                    o,
                    "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"Arial\" font-size=\"16\" text-anchor=\"end\" fill=\"#1d2433\">{}</text>",
                    lw - 8.0,
                    band * (i as f64 + 0.5) + 6.0,
                    esc(cat)
                );
            }
            o.push_str("</svg>");
            return o;
        }
        "pie" => {
            let total: f64 = series
                .first()
                .map(|(_, v)| v.iter().sum())
                .unwrap_or(0.0)
                .max(1e-9);
            let (cx, cy, r) = (w / 2.0, plot_h / 2.0, plot_h.min(w) / 2.0 - 4.0);
            let mut a0 = -std::f64::consts::FRAC_PI_2;
            for (i, v) in series
                .first()
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
                .iter()
                .enumerate()
            {
                let a1 = a0 + v / total * std::f64::consts::TAU;
                let large = if a1 - a0 > std::f64::consts::PI { 1 } else { 0 };
                let _ = write!(
                    o,
                    "<path d=\"M{cx:.1},{cy:.1} L{:.1},{:.1} A{r:.1},{r:.1} 0 {large} 1 {:.1},{:.1} Z\" fill=\"{}\"/>",
                    cx + r * a0.cos(),
                    cy + r * a0.sin(),
                    cx + r * a1.cos(),
                    cy + r * a1.sin(),
                    esc(&colors[i % colors.len()])
                );
                a0 = a1;
            }
        }
        _ => {
            for (i, _) in cats.iter().enumerate() {
                for (si, (_, vals)) in series.iter().enumerate() {
                    let bw = slot * 0.66 / ns;
                    let x = slot * i as f64 + slot * 0.17 + bw * si as f64;
                    let bh = vals.get(i).copied().unwrap_or(0.0) / max * (plot_h - 4.0);
                    let _ = write!(
                        o,
                        "<rect x=\"{x:.1}\" y=\"{:.1}\" width=\"{bw:.1}\" height=\"{bh:.1}\" fill=\"{}\"/>",
                        plot_h - bh,
                        esc(&colors[si % colors.len()])
                    );
                }
            }
        }
    }
    if kind != "pie" {
        for (i, cat) in cats.iter().enumerate() {
            let _ = write!(
                o,
                "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"Arial\" font-size=\"16\" text-anchor=\"middle\" fill=\"#1d2433\">{}</text>",
                slot * (i as f64 + 0.5),
                h - 6.0,
                esc(cat)
            );
        }
    }
    o.push_str("</svg>");
    o
}
