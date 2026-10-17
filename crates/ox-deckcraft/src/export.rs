//! Composed `deck::deck` → DeckCraft `Presentation`.

use std::path::Path;
use std::sync::Arc;

use deckcraft_color::Rgba;
use deckcraft_geom::{Size, Xfrm};
use deckcraft_model::chart::{ChartType, Series};
use deckcraft_model::defaults::{blank_presentation, layout_of_kind};
use deckcraft_model::style::{Fill, Line, PictureFill};
use deckcraft_model::text::{Action, Align, Anchor, AutoFit, BodyProps, Bullet, Hyperlink, ParaProps, RunKind};
use deckcraft_model::{
    Background, Chart, ColorRef, Geom, LayoutType, Paragraph, Presentation, Run, RunProps, Shape,
    ShapeId, ShapeKind, Slide, SlideId, Table, TextBody, Theme,
};
use ox_core::Composed;
use ox_core::render::{CHART_COLORS, chart_data, field_text, format_value};
use serde_json::{Map, Value};

use crate::{DeckError, KEY_DECK, KEY_REVISION, NAME_PREFIX, pt, revision_of};

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ExportReport {
    pub slides: usize,
    pub frames: usize,
    pub tables: usize,
    pub charts: usize,
    pub fields: usize,
    pub warnings: Vec<String>,
}

fn obj<'a>(n: &'a Composed, key: &str) -> Option<&'a Map<String, Value>> {
    n.attributes.get(key).and_then(Value::as_object)
}

fn s<'a>(m: &'a Map<String, Value>, k: &str) -> Option<&'a str> {
    m.get(k).and_then(Value::as_str)
}

fn num(m: &Map<String, Value>, k: &str) -> f64 {
    m.get(k).and_then(Value::as_f64).unwrap_or(0.0)
}

pub(crate) fn solid(hex: &str) -> Option<Fill> {
    let h = hex.trim_start_matches('#');
    let rgb = Rgba::from_hex(&h[..h.len().min(6)])?;
    Some(Fill::Solid {
        color: ColorRef::rgb(rgb),
    })
}

pub(crate) fn run_props(r: &Map<String, Value>) -> RunProps {
    let mut p = RunProps::default();
    if r.get("bold").and_then(Value::as_bool) == Some(true) {
        p.bold = Some(true);
    }
    if r.get("italic").and_then(Value::as_bool) == Some(true) {
        p.italic = Some(true);
    }
    if r.get("underline").and_then(Value::as_bool) == Some(true) {
        p.underline = Some("sng".into());
    }
    if let Some(c) = s(r, "color").and_then(solid) {
        p.fill = Some(c);
    }
    if let Some(f) = s(r, "font") {
        p.font = Some(f.to_string());
    }
    if let Some(sz) = r.get("sizePt").and_then(Value::as_f64) {
        p.size = Some(sz);
    }
    if let Some(url) = s(r, "link") {
        p.link = Some(Hyperlink {
            action: Action::Url { url: url.to_string() },
            tooltip: String::new(),
            highlight_click: false,
            sound: None,
            stop_sound: false,
            zoom_return: None,
        });
    }
    p
}

fn text_run(text: &str, props: RunProps) -> Run {
    Run {
        text: text.to_string(),
        props,
        kind: RunKind::Text,
    }
}

/// `deck::shape`: the shape's geometry, fill and outline.
fn shape_look(shape: &mut Shape, sh: &Map<String, Value>) {
    shape.geom = Geom::preset(match s(sh, "kind") {
        Some("ellipse") => "ellipse",
        Some("roundRect") => "roundRect",
        _ => "rect",
    });
    shape.fill = Some(s(sh, "fill").and_then(solid).unwrap_or(Fill::None));
    shape.line = Some(match s(sh, "stroke").and_then(solid) {
        Some(f) => Line {
            fill: Some(f),
            width: Some(pt(num(sh, "strokeWidth").max(1.0))),
            ..Line::default()
        },
        None => Line {
            fill: Some(Fill::None),
            ..Line::default()
        },
    });
}

fn plain_body(paragraphs: Vec<Paragraph>) -> TextBody {
    TextBody {
        body: BodyProps {
            inset_l: Some(0.0),
            inset_t: Some(0.0),
            inset_r: Some(0.0),
            inset_b: Some(0.0),
            anchor: Some(Anchor::Top),
            wrap: Some(true),
            autofit: Some(AutoFit::None),
            ..BodyProps::default()
        },
        paragraphs,
        ..TextBody::default()
    }
}

fn align(a: Option<&str>) -> Option<Align> {
    match a? {
        "center" => Some(Align::Center),
        "right" => Some(Align::Right),
        "justify" => Some(Align::Justify),
        _ => Some(Align::Left),
    }
}

fn text_body(fr: &Composed, t: &Map<String, Value>, report: &mut ExportReport) -> TextBody {
    let mut paragraphs = Vec::new();
    for p in t
        .get("paragraphs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(p) = p.as_object() else { continue };
        let level = p.get("level").and_then(Value::as_u64).unwrap_or(0) as u8;
        let mut props = ParaProps {
            align: align(s(p, "align")),
            ..ParaProps::default()
        };
        if p.get("bullet").and_then(Value::as_bool) == Some(true) {
            props.bullet = Some(match (s(p, "bulletChar"), p.get("autoNum")) {
                (_, Some(n)) => Bullet::AutoNum {
                    scheme: s(n.as_object().unwrap_or(&Map::new()), "scheme").unwrap_or("arabicPeriod").to_string(),
                    start_at: n.get("startAt").and_then(Value::as_u64).unwrap_or(1) as u32,
                },
                (Some(c), None) => Bullet::Char { char: c.to_string() },
                (None, None) => Bullet::Char { char: "•".into() },
            });
            props.margin_left = Some(18.0 * (level as f64 + 1.0));
            props.indent = Some(-18.0);
        } else {
            props.bullet = Some(Bullet::None);
        }
        // Indents the source deck's layout gave (an adopted deck).
        if let Some(m) = p.get("marginLeftPt").and_then(Value::as_f64) {
            props.margin_left = Some(m);
        }
        if let Some(i) = p.get("indentPt").and_then(Value::as_f64) {
            props.indent = Some(i);
        }
        let spacing = |k: &str| p.get(k).and_then(|v| serde_json::from_value(v.clone()).ok());
        props.space_before = spacing("spaceBefore");
        props.space_after = spacing("spaceAfter");
        props.line_spacing = spacing("lineSpacing");
        if let Some(Fill::Solid { color }) = s(p, "bulletColor").and_then(solid) {
            props.bullet_color = Some(color);
        }
        props.bullet_font = s(p, "bulletFont").map(String::from);
        props.bullet_size = p.get("bulletSize").and_then(Value::as_f64);
        let mut runs = Vec::new();
        let mut last_props = RunProps::default();
        for r in p
            .get("runs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(r) = r.as_object() else { continue };
            let props = run_props(r);
            last_props = props.clone();
            if let Some(field) = s(r, "field") {
                // Plain text: PowerPoint and Impress don't keep custom field types, so import
                // finds the value again by position (see `import::reanchor`).
                runs.push(text_run(&field_text(fr, field), props));
                report.fields += 1;
            } else if let Some(text) = s(r, "text") {
                // OX keeps line breaks inside runs; DeckCraft has break runs.
                for (i, line) in text.split('\n').enumerate() {
                    if i > 0 {
                        runs.push(Run {
                            text: String::new(),
                            props: props.clone(),
                            kind: RunKind::Break,
                        });
                    }
                    if !line.is_empty() {
                        runs.push(text_run(line, props.clone()));
                    }
                }
            }
        }
        paragraphs.push(Paragraph {
            level,
            props,
            runs,
            // Text typed at the end of the paragraph doesn't join a link.
            end_props: RunProps { link: None, ..last_props },
            ..Paragraph::default()
        });
    }
    plain_body(paragraphs)
}

fn table_kind(fr: &Composed, w: f64, h: f64) -> ShapeKind {
    let data = obj(fr, "office::data::table").cloned().unwrap_or_default();
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
    let size = obj(fr, "deck::table")
        .and_then(|t| t.get("style"))
        .and_then(Value::as_object)
        .and_then(|st| st.get("sizePt"))
        .and_then(Value::as_f64);
    let font = obj(fr, "deck::table")
        .and_then(|t| t.get("style"))
        .and_then(Value::as_object)
        .and_then(|st| s(st, "font").map(String::from));
    let n_rows = rows.len() + 1;
    let mut t = Table::new(n_rows, cols.len().max(1), w, h / n_rows as f64);
    t.first_row = true;
    t.band_row = true;
    let cell_text = |text: String, bold: bool, right: bool| {
        let props = RunProps {
            bold: bold.then_some(true),
            size,
            font: font.clone(),
            ..RunProps::default()
        };
        let para = Paragraph {
            props: ParaProps {
                align: Some(if right { Align::Right } else { Align::Left }),
                ..ParaProps::default()
            },
            runs: vec![text_run(&text, props.clone())],
            end_props: props,
            ..Paragraph::default()
        };
        TextBody {
            paragraphs: vec![para],
            ..TextBody::default()
        }
    };
    let numeric = |i: usize| {
        matches!(
            cols.get(i).and_then(|c| s(c, "type")),
            Some("currency" | "number" | "percent")
        )
    };
    for (c, col) in cols.iter().enumerate() {
        if let Some(cell) = t.cell_mut(0, c) {
            cell.text = cell_text(s(col, "name").unwrap_or("").to_string(), true, numeric(c));
        }
    }
    for (r, row) in rows.iter().enumerate() {
        for c in 0..cols.len() {
            let kind = cols.get(c).and_then(|col| s(col, "type")).unwrap_or("text");
            if let Some(cell) = t.cell_mut(r + 1, c) {
                cell.text = cell_text(
                    format_value(row.get(c).unwrap_or(&Value::Null), kind),
                    false,
                    numeric(c),
                );
            }
        }
    }
    ShapeKind::Table(t)
}

fn chart_kind(fr: &Composed, c: &Map<String, Value>) -> ShapeKind {
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
    let kind = match s(c, "kind") {
        Some("bar") => ChartType::Bar,
        Some("line") => ChartType::Line,
        Some("pie") => ChartType::Pie,
        _ => ChartType::Column,
    };
    let chart = Chart {
        kind,
        title: s(c, "title").map(String::from),
        categories: cats,
        series: series
            .into_iter()
            .enumerate()
            .map(|(i, (name, values))| Series {
                name,
                values: values.into_iter().map(Some).collect(),
                fill: solid(&colors[i % colors.len()]),
                ..Series::default()
            })
            .collect(),
        vary_colors: kind == ChartType::Pie,
        ..Chart::default()
    };
    ShapeKind::Chart(Box::new(chart))
}

/// A picture's crop (left, top, right, bottom fractions); none when absent.
fn crop_of(pic: &Map<String, Value>) -> [f64; 4] {
    let mut c = [0.0; 4];
    for (i, v) in pic.get("crop").and_then(Value::as_array).into_iter().flatten().take(4).enumerate() {
        c[i] = v.as_f64().unwrap_or(0.0);
    }
    c
}

/// Builds a DeckCraft presentation from a composed `deck::deck` node. `assets` is the directory
/// blob paths (pictures, SVG) are relative to — the project directory.
pub fn export(deck: &Composed, assets: &Path) -> Result<(Presentation, ExportReport), DeckError> {
    export_with(deck, ox_core::PictureSource::Read(assets))
}

/// [`export`], with where pictures' bytes come from (an importer's comparison export uses
/// stand-ins, see [`ox_core::PictureSource`]).
pub(crate) fn export_with(deck: &Composed, assets: ox_core::PictureSource) -> Result<(Presentation, ExportReport), DeckError> {
    let d = obj(deck, "deck::deck")
        .ok_or_else(|| DeckError::NotADeck(deck.path.clone()))?
        .clone();
    let mut report = ExportReport::default();
    let size = Size::new(
        pt(num(&d, "width").max(1.0)),
        pt(num(&d, "height").max(1.0)),
    );
    let mut p = blank_presentation(size, Theme::default(), false);
    p.props.title = s(&d, "title").unwrap_or("").to_string();
    p.props.keywords = format!(
        "{KEY_DECK}{} {KEY_REVISION}{}",
        deck.path,
        revision_of(deck)
    );
    let layout = layout_of_kind(&p, LayoutType::Blank)
        .or_else(|| {
            p.masters
                .first()
                .and_then(|m| m.layouts.first())
                .map(|l| l.id)
        })
        .unwrap_or_default();
    let alloc = |p: &mut Presentation| {
        p.alloc_id()
            .ok_or_else(|| DeckError::Invalid("the deck ran out of object ids".into()))
    };

    for slide_node in &deck.children {
        let sa = obj(slide_node, "deck::slide").cloned().unwrap_or_default();
        let mut slide = Slide {
            id: SlideId(alloc(&mut p)?),
            layout,
            name: format!("{NAME_PREFIX}{}", slide_node.path),
            ..Slide::default()
        };
        if let Some(bg) = s(&sa, "background").and_then(solid) {
            slide.background = Some(Background::Fill { fill: bg });
        }
        if let Some(notes) = s(&sa, "notes") {
            slide.notes = plain_body(
                notes
                    .split('\n')
                    .map(|l| Paragraph {
                        runs: vec![text_run(l, RunProps::default())],
                        ..Paragraph::default()
                    })
                    .collect(),
            );
        }
        // Shapes by frame path, and connector ends to attach once every shape is made.
        let mut ids: std::collections::HashMap<&str, ShapeId> = std::collections::HashMap::new();
        let mut ends: Vec<(usize, Option<(String, u32)>, Option<(String, u32)>)> = Vec::new();
        for fr in &slide_node.children {
            let g = obj(fr, "deck::frame").cloned().unwrap_or_default();
            let xfrm = Xfrm {
                x: pt(num(&g, "x")),
                y: pt(num(&g, "y")),
                w: pt(num(&g, "w")),
                h: pt(num(&g, "h")),
                rot: num(&g, "rotation"),
                flip_h: g.get("flipH").and_then(Value::as_bool) == Some(true),
                flip_v: g.get("flipV").and_then(Value::as_bool) == Some(true),
            };
            let mut shape = Shape {
                id: ShapeId(alloc(&mut p)?),
                name: format!("{NAME_PREFIX}{}", fr.path),
                xfrm: Some(xfrm),
                kind: ShapeKind::Shape,
                geom: Geom::preset("rect"),
                ..Shape::default()
            };
            if let Some(c) = obj(fr, "deck::chart") {
                shape.kind = chart_kind(fr, c);
                report.charts += 1;
            } else if obj(fr, "office::data::table").is_some() {
                shape.kind = table_kind(fr, xfrm.w, xfrm.h);
                report.tables += 1;
            } else if let Some(t) = obj(fr, "deck::table") {
                shape.kind = crate::table::table_shape(t, xfrm.w, xfrm.h, &mut |cell| {
                    let paragraphs = match cell.get("paragraphs") {
                        Some(_) => text_body(fr, cell, &mut report).paragraphs,
                        None => TextBody::from_text(s(cell, "text").unwrap_or("")).paragraphs,
                    };
                    TextBody { paragraphs, ..TextBody::default() }
                });
                report.tables += 1;
            } else if let Some(t) = obj(fr, "deck::text") {
                let mut body = text_body(fr, t, &mut report);
                if let Some(i) = t.get("insets").and_then(Value::as_object) {
                    let side = |k: &str| i.get(k).and_then(Value::as_f64).map(pt);
                    (body.body.inset_l, body.body.inset_t, body.body.inset_r, body.body.inset_b) =
                        (side("left"), side("top"), side("right"), side("bottom"));
                }
                body.body.anchor = Some(match s(t, "verticalAlign") {
                    Some("middle") => Anchor::Middle,
                    Some("bottom") => Anchor::Bottom,
                    _ => Anchor::Top,
                });
                shape.text = Some(body);
                shape.text_box = true;
                shape.fill = Some(Fill::None);
                shape.line = Some(Line {
                    fill: Some(Fill::None),
                    ..Line::default()
                });
                // A box with a look of its own (an adopted deck's filled or outlined text box).
                if let Some(sh) = obj(fr, "deck::shape") {
                    shape_look(&mut shape, sh);
                    shape.text_box = false;
                }
            } else if let Some(pic) = obj(fr, "deck::picture").or_else(|| obj(fr, "deck::vector")) {
                let blob = s(pic, "blob").unwrap_or("");
                // A missing asset stops the export: a deck with a hole in it is worse than no deck.
                let bytes = ox_core::picture_bytes(assets, blob).map_err(|e| {
                    DeckError::MissingAsset(format!("{}: can't read {blob}: {e}", fr.path))
                })?;
                let ct = s(pic, "mediaType").unwrap_or_else(|| ox_core::media_type_for(blob));
                let ext = match ct {
                    "image/svg+xml" => "svg",
                    "image/jpeg" => "jpg",
                    "image/gif" => "gif",
                    "image/webp" => "webp",
                    _ => "png",
                };
                let named = format!("image.{ext}");
                let name = if blob.starts_with("sha256-") {
                    named.as_str()
                } else {
                    Path::new(blob).file_name().and_then(|n| n.to_str()).unwrap_or("image")
                };
                let media = p
                    .add_media(name, ct, bytes)
                    .ok_or_else(|| DeckError::Invalid("the deck ran out of media ids".into()))?;
                shape.kind = ShapeKind::Picture {
                    fill: PictureFill {
                        media,
                        crop: crop_of(pic),
                        ..PictureFill::default()
                    },
                };
                shape.descr = s(pic, "alt").unwrap_or("").to_string();
            } else if let Some(l) = obj(fr, "deck::line") {
                shape.geom = l.get("geom").cloned().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_else(|| Geom::preset("line"));
                shape.line = l.get("line").cloned().and_then(|v| serde_json::from_value(v).ok());
                shape.fill = Some(Fill::None);
                let end = |k: &str| {
                    let e = l.get(k)?;
                    Some((s(e.as_object()?, "frame")?.to_string(), e.get("site").and_then(Value::as_u64).unwrap_or(0) as u32))
                };
                shape.kind = ShapeKind::Connector { start: None, end: None };
                ends.push((slide.shapes.len(), end("from"), end("to")));
            } else if let Some(sh) = obj(fr, "deck::shape") {
                shape_look(&mut shape, sh);
            } else {
                report.warnings.push(format!(
                    "{}: nothing DeckCraft can show yet; skipped",
                    fr.path
                ));
                continue;
            }
            ids.insert(fr.path.as_str(), shape.id);
            slide.shapes.push(shape);
            report.frames += 1;
        }
        for (i, from, to) in ends {
            let at = |e: Option<(String, u32)>| e.and_then(|(p, site)| Some((*ids.get(p.as_str())?, site)));
            if let Some(ShapeKind::Connector { start, end }) = slide.shapes.get_mut(i).map(|sh| &mut sh.kind) {
                (*start, *end) = (at(from), at(to));
            }
        }
        p.slides.push(Arc::new(slide));
        report.slides += 1;
    }
    Ok((p, report))
}
