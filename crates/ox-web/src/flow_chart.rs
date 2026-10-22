//! SVG views of the linked chart data, without canvas or a codec dependency.
use ox_core::render::esc;
use serde_json::Value;
use std::fmt::Write;

const COLORS: [&str; 8] = [
    "#277c88", "#d2713b", "#7961a5", "#588650", "#be5379", "#4979b1", "#9c7b2f", "#626a75",
];
fn label(v: &Value) -> String {
    v.as_str().map(String::from).unwrap_or_else(|| {
        if v.is_null() {
            String::new()
        } else {
            v.to_string()
        }
    })
}
// Trigonometric last bits can differ between native and wasm. Coordinate precision is part
// of the renderer version; exact source labels and data in text/desc are never rounded.
fn stable_coordinates(svg: String) -> String {
    let mut out = String::new();
    let mut previous = "";
    for (i, part) in svg.split('"').enumerate() {
        if i > 0 {
            out.push('"');
        }
        let attr = previous
            .split_whitespace()
            .last()
            .unwrap_or("")
            .trim_end_matches('=');
        if i % 2 == 1
            && [
                "x",
                "y",
                "x1",
                "y1",
                "x2",
                "y2",
                "cx",
                "cy",
                "r",
                "width",
                "height",
                "viewBox",
                "transform",
            ]
            .contains(&attr)
        {
            out.push_str(
                &part
                    .split_whitespace()
                    .map(|v| {
                        v.parse::<f64>()
                            .ok()
                            .map(|n| format!("{n:.3}"))
                            .unwrap_or_else(|| v.into())
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            );
        } else {
            out.push_str(part);
        }
        previous = part;
    }
    out
}

pub(super) fn validate(table: &Value, spec: &Value) -> Result<(), String> {
    if spec["kind"] == "pie"
        && table["rows"].as_array().into_iter().flatten().any(|r| {
            r.as_array()
                .into_iter()
                .flatten()
                .skip(1)
                .filter_map(Value::as_f64)
                .any(|v| v < 0.0)
        })
    {
        return Err("The selected pie chart contains negative values. A pie cannot show negative shares. Choose a column, bar or line chart, or make a column chart from the range; the source values stay unchanged.".into());
    }
    Ok(())
}

pub(super) fn svg(table: &Value, spec: &Value) -> Result<Vec<u8>, String> {
    validate(table, spec)?;
    let rows = table["rows"]
        .as_array()
        .ok_or("The linked chart has no row data.")?;
    let columns = table["columns"]
        .as_array()
        .ok_or("The linked chart has no columns.")?;
    // Use precisely the series named by the chart spec. A numeric category is still a category.
    let series: Vec<(usize, String)> = spec["values"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|name| {
            columns
                .iter()
                .position(|c| c["name"] == *name)
                .map(|i| (i, label(name)))
        })
        .collect();
    let kind = spec["kind"].as_str().unwrap_or("column");
    if !["column", "bar", "line", "pie"].contains(&kind) {
        return Err(format!("Unsupported report chart kind: {kind}"));
    }
    let category = columns
        .iter()
        .position(|c| c["name"] == spec["category"])
        .unwrap_or(0);
    let cats: Vec<String> = rows.iter().map(|r| label(&r[category])).collect();
    let value = |r: usize, s: usize| rows[r][series[s].0].as_f64();
    let pie_columns = series.len().clamp(1, 3);
    let pie_rows = series.len().div_ceil(pie_columns).max(1);
    let legend_rows = if kind == "pie" {
        cats.len().div_ceil(3)
    } else {
        series.len().div_ceil(3)
    };
    let height = if kind == "pie" {
        70 + pie_rows * 250 + legend_rows * 22
    } else {
        400 + legend_rows * 22
    };
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"800\" height=\"{height}\" viewBox=\"0 0 800 {height}\" data-chart-kind=\"{kind}\"><rect width=\"800\" height=\"{height}\" fill=\"white\"/><title>{}</title><text x=\"32\" y=\"26\" font-family=\"Arial\" font-size=\"17\">{}</text><desc>",
        esc(&label(&spec["title"])),
        esc(&label(&spec["title"]))
    );
    for (r, cat) in cats.iter().enumerate() {
        for (s, (_, name)) in series.iter().enumerate() {
            let exact = value(r, s)
                .map(|v| v.to_string())
                .unwrap_or_else(|| "No value".into());
            let _ = writeln!(out, "{}", esc(&format!("{cat} / {name}: {exact}")));
        }
    }
    out.push_str("</desc>");
    if kind == "pie" {
        for (s, (_, name)) in series.iter().enumerate() {
            let cw = 800.0 / pie_columns as f64;
            let cx = cw * (s % pie_columns) as f64 + cw / 2.0;
            let cy = 170.0 + (s / pie_columns) as f64 * 250.0;
            let radius = (cw / 2.0 - 24.0).min(88.0);
            let scale = (0..rows.len())
                .filter_map(|r| value(r, s))
                .fold(1.0f64, f64::max);
            let total: f64 = (0..rows.len())
                .filter_map(|r| value(r, s))
                .map(|v| v / scale)
                .sum();
            let _ = write!(
                out,
                "<text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-family=\"Arial\" font-size=\"13\">{}</text>",
                cy - radius - 22.0,
                esc(name)
            );
            let mut angle = -std::f64::consts::FRAC_PI_2;
            if total == 0.0 {
                let _ = write!(
                    out,
                    "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{radius}\" fill=\"#eef1f3\"/><text x=\"{cx}\" y=\"{cy}\" text-anchor=\"middle\" font-size=\"12\">All values are zero</text>"
                );
            } else {
                for (r, cat) in cats.iter().enumerate() {
                    let Some(v) = value(r, s) else { continue };
                    if v == 0.0 {
                        continue;
                    }
                    let delta = (v / scale) / total * std::f64::consts::TAU;
                    let color = COLORS[r % COLORS.len()];
                    let title = esc(&format!("{cat} / {name}: {v}"));
                    if delta >= std::f64::consts::TAU - 1e-10 {
                        let _ = write!(
                            out,
                            "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{radius}\" fill=\"{color}\"><title>{title}</title></circle>"
                        );
                    } else {
                        let x1 = cx + radius * angle.cos();
                        let y1 = cy + radius * angle.sin();
                        let end = angle + delta;
                        let x2 = cx + radius * end.cos();
                        let y2 = cy + radius * end.sin();
                        let large = u8::from(delta > std::f64::consts::PI);
                        let _ = write!(
                            out,
                            "<path d=\"M {cx:.3} {cy:.3} L {x1:.3} {y1:.3} A {radius:.3} {radius:.3} 0 {large} 1 {x2:.3} {y2:.3} Z\" fill=\"{color}\" stroke=\"white\"><title>{title}</title></path>"
                        );
                    }
                    angle += delta;
                }
            }
        }
    } else {
        let (mut min, mut max) = (0.0f64, 0.0f64);
        for r in 0..rows.len() {
            for s in 0..series.len() {
                if let Some(v) = value(r, s) {
                    min = min.min(v);
                    max = max.max(v);
                }
            }
        }
        if min == max {
            max = min + 1.0;
        }
        let (left, right, top, bottom) =
            (if kind == "bar" { 180.0 } else { 70.0 }, 760.0, 60.0, 285.0);
        let scale = min.abs().max(max.abs()).max(1.0);
        let sy =
            |v: f64| top + (max / scale - v / scale) / (max / scale - min / scale) * (bottom - top);
        let sx = |v: f64| {
            left + (v / scale - min / scale) / (max / scale - min / scale) * (right - left)
        };
        if kind == "bar" {
            let zero = sx(0.0);
            let _ = write!(
                out,
                "<line x1=\"{zero}\" y1=\"{top}\" x2=\"{zero}\" y2=\"{bottom}\" stroke=\"#65717c\"/>"
            );
            let slot = (bottom - top) / rows.len().max(1) as f64;
            let band = slot * 0.8 / series.len().max(1) as f64;
            for (r, cat) in cats.iter().enumerate() {
                let y = top + slot * r as f64;
                let _ = write!(
                    out,
                    "<text x=\"8\" y=\"{}\" font-size=\"11\">{}</text>",
                    y + slot / 2.0,
                    esc(cat)
                );
                for (s, (_, name)) in series.iter().enumerate() {
                    if let Some(v) = value(r, s) {
                        let x = sx(v);
                        let color = COLORS[s % COLORS.len()];
                        let _ = write!(
                            out,
                            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{band}\" fill=\"{color}\"><title>{}</title></rect>",
                            x.min(zero),
                            y + slot * 0.1 + band * s as f64,
                            (x - zero).abs(),
                            esc(&format!("{cat} / {name}: {v}"))
                        );
                    }
                }
            }
            let _ = write!(
                out,
                "<text x=\"{left}\" y=\"310\" font-size=\"11\">{min}</text><text x=\"{right}\" y=\"310\" text-anchor=\"end\" font-size=\"11\">{max}</text>"
            );
        } else {
            let zero = sy(0.0);
            let _ = write!(
                out,
                "<line x1=\"{left}\" y1=\"{zero}\" x2=\"{right}\" y2=\"{zero}\" stroke=\"#65717c\"/><text x=\"4\" y=\"{top}\" font-size=\"11\">{max}</text><text x=\"4\" y=\"{bottom}\" font-size=\"11\">{min}</text>"
            );
            let slot = (right - left) / rows.len().max(1) as f64;
            for (r, cat) in cats.iter().enumerate() {
                let x = left + slot * (r as f64 + 0.5);
                let _ = write!(
                    out,
                    "<text x=\"{x}\" y=\"307\" transform=\"rotate(25 {x} 307)\" font-size=\"11\">{}</text>",
                    esc(cat)
                );
            }
            if kind == "column" {
                let band = slot * 0.8 / series.len().max(1) as f64;
                for (r, cat) in cats.iter().enumerate() {
                    for (s, (_, name)) in series.iter().enumerate() {
                        if let Some(v) = value(r, s) {
                            let y = sy(v);
                            let color = COLORS[s % COLORS.len()];
                            let x = left + slot * r as f64 + slot * 0.1 + band * s as f64;
                            let _ = write!(
                                out,
                                "<rect x=\"{x}\" y=\"{}\" width=\"{band}\" height=\"{}\" fill=\"{color}\"><title>{}</title></rect>",
                                y.min(zero),
                                (y - zero).abs(),
                                esc(&format!("{cat} / {name}: {v}"))
                            );
                        }
                    }
                }
            } else {
                for (s, (_, name)) in series.iter().enumerate() {
                    let color = COLORS[s % COLORS.len()];
                    let mut path = String::new();
                    let mut connected = false;
                    for (r, cat) in cats.iter().enumerate() {
                        if let Some(v) = value(r, s) {
                            let x = left + slot * (r as f64 + 0.5);
                            let y = sy(v);
                            let _ = write!(
                                path,
                                "{} {x:.3} {y:.3} ",
                                if connected { "L" } else { "M" }
                            );
                            connected = true;
                            let _ = write!(
                                out,
                                "<circle cx=\"{x}\" cy=\"{y}\" r=\"3\" fill=\"{color}\"><title>{}</title></circle>",
                                esc(&format!("{cat} / {name}: {v}"))
                            );
                        } else {
                            connected = false;
                        }
                    }
                    let _ = write!(
                        out,
                        "<path d=\"{path}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"2\"/>"
                    );
                }
            }
        }
    }
    let labels: Vec<String> = if kind == "pie" {
        cats
    } else {
        series.iter().map(|(_, name)| name.clone()).collect()
    };
    let legend_y = if kind == "pie" {
        70 + pie_rows * 250
    } else {
        390
    };
    for (i, name) in labels.iter().enumerate() {
        let x = 24 + (i % 3) * 260;
        let y = legend_y + (i / 3) * 22;
        let color = COLORS[i % COLORS.len()];
        let _ = write!(
            out,
            "<rect x=\"{x}\" y=\"{}\" width=\"10\" height=\"10\" fill=\"{color}\"/><text x=\"{}\" y=\"{y}\" font-family=\"Arial\" font-size=\"11\">{}</text>",
            y - 9,
            x + 15,
            esc(name)
        );
    }
    out.push_str("</svg>");
    Ok(stable_coordinates(out).into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn table() -> Value {
        json!({"columns":[{"name":"Day"},{"name":"First"},{"name":"Second"}],"rows":[["Mon",12,3],["Tue",18,5],["Wed",15,4]]})
    }
    #[test]
    fn chart_kinds_preserve_shapes_and_all_series() {
        for kind in ["column", "bar", "line", "pie"] {
            let spec =
                json!({"kind":kind,"title":"Figures","category":"Day","values":["First","Second"]});
            let text = String::from_utf8(svg(&table(), &spec).unwrap()).unwrap();
            assert!(text.contains(&format!("data-chart-kind=\"{kind}\"")));
            for text_label in ["Mon / First: 12", "Tue / Second: 5", "Wed / First: 15"] {
                assert!(text.contains(text_label));
            }
            if kind == "pie" {
                assert!(text.contains(" A "));
                assert_eq!(text.matches("text-anchor=\"middle\"").count(), 2);
            }
            if kind == "line" {
                assert!(text.contains("stroke-width=\"2\""));
                assert_eq!(text.matches("<circle").count(), 6);
            }
        }
    }
    #[test]
    fn negative_data_stays_signed_and_negative_pies_are_explicit() {
        let mut table = table();
        table["rows"][0][1] = json!(-12);
        for kind in ["column", "bar", "line"] {
            let text = String::from_utf8(
                svg(
                    &table,
                    &json!({"kind":kind,"category":"Day","values":["First","Second"]}),
                )
                .unwrap(),
            )
            .unwrap();
            assert!(text.contains("Mon / First: -12"));
            assert!(!text.contains("NaN"));
        }
        assert!(svg(
            &table,
            &json!({"kind":"pie","category":"Day","values":["First","Second"]})
        )
        .unwrap_err()
        .contains("negative"));
    }
    #[test]
    fn coordinate_precision_keeps_exact_source_labels() {
        let result = stable_coordinates(
            "<circle cx=\"1.23456789\" cy=\"-9.87654321\"/><desc>1.23456789</desc>".into(),
        );
        assert!(result.contains("cx=\"1.235\" cy=\"-9.877\""));
        assert!(result.contains("<desc>1.23456789</desc>"));
    }
    #[test]
    fn pie_arc_flags_are_single_integer_tokens() {
        let text = String::from_utf8(
            svg(
                &table(),
                &json!({"kind":"pie","category":"Day","values":["First","Second"]}),
            )
            .unwrap(),
        )
        .unwrap();
        let mut arcs = 0;
        for path in text.split("d=\"").skip(1) {
            let data = path.split('"').next().unwrap();
            let tokens: Vec<_> = data.split_whitespace().collect();
            for (i, t) in tokens.iter().enumerate() {
                if *t == "A" {
                    assert!(["0", "1"].contains(&tokens[i + 4]));
                    assert!(["0", "1"].contains(&tokens[i + 5]));
                    assert!(tokens[i + 1]
                        .split('.')
                        .nth(1)
                        .is_some_and(|v| v.len() == 3));
                    arcs += 1;
                }
            }
        }
        assert_eq!(arcs, 6);
    }
}
