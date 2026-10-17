//! Native tables as `deck::table`: `cols` and `rows` (px), and `snapshot`, a grid of cells, each
//! `{text, paragraphs, look}`: its plain text (what the HTML view shows), its text as `deck::text`
//! paragraphs, and its look in DeckCraft's form (`Cell` without its text). The table style is
//! worked out into every cell (fill, borders, text colour and bold) with the slide's theme
//! colours as RGB: editors disagree on styles (LibreOffice writes every cell's look out and drops
//! the style), and the export has a theme of its own. `props` keeps the header/banding flags.

use deckcraft_model::style::{Fill, Line};
use deckcraft_model::table::{Cell, Row, Table, TableBorder, TablePart, TablePartStyle, TableStyleDef};
use deckcraft_model::text::Anchor;
use deckcraft_model::{ColorRef, Rgba, Shape, ShapeKind, TextBody, resolve};
use serde_json::{Map, Value, json};

use crate::import::{canonical_para, near, plain_line, read_paragraphs, rgb_fill};
use crate::{pt, px};

/// What a table style gives one cell.
#[derive(Default)]
struct StyleLook {
    paint: Option<(Fill, Option<Rgba>)>,
    text: Option<Rgba>,
    bold: Option<bool>,
    italic: Option<bool>,
    /// Left, right, top, bottom (a transparent colour for no line).
    sides: [Option<(Rgba, f64)>; 4],
}

// ponytail: ported from deckcraft-render's private `table::look_def` (1dc6972); drop this copy if
// DeckCraft makes it public, and keep it in step with it until then.
/// The look of cell (`r`, `c`) from a table style the file defines: each part that applies to the
/// cell (whole table, bands, first and last rows and columns, corners, in that order) sets the
/// fill, text and the borders on its region's edges (its inside lines within it).
fn style_look(ctx: &resolve::Ctx, def: &TableStyleDef, t: &Table, r: usize, c: usize) -> StyleLook {
    let (rows, cols) = (t.rows.len(), t.cols.len());
    let (r1, c1) = t.cell(r, c).map(|cell| (r + cell.row_span.max(1) as usize - 1, c + cell.grid_span.max(1) as usize - 1)).unwrap_or((r, c));
    let (r1, c1) = (r1.min(rows.saturating_sub(1)), c1.min(cols.saturating_sub(1)));
    let (last_r, last_c) = (rows.saturating_sub(1), cols.saturating_sub(1));
    let header = t.first_row && r == 0;
    let total = t.last_row && r1 == last_r && rows > 1;
    let first_col = t.first_col && c == 0;
    let last_col = t.last_col && c1 == last_c && cols > 1;
    let body_r = r.saturating_sub(usize::from(t.first_row));
    let body_c = c.saturating_sub(usize::from(t.first_col));
    let mut look = StyleLook::default();
    for part in TablePart::ALL {
        let (on, region) = match part {
            TablePart::WholeTbl => (true, (0, last_r, 0, last_c)),
            TablePart::Band1H => (t.band_row && !header && !total && body_r % 2 == 0, (r, r1, 0, last_c)),
            TablePart::Band2H => (t.band_row && !header && !total && body_r % 2 == 1, (r, r1, 0, last_c)),
            TablePart::Band1V => (t.band_col && !first_col && !last_col && body_c % 2 == 0, (0, last_r, c, c1)),
            TablePart::Band2V => (t.band_col && !first_col && !last_col && body_c % 2 == 1, (0, last_r, c, c1)),
            TablePart::LastCol => (last_col, (0, last_r, c, last_c)),
            TablePart::FirstCol => (first_col, (0, last_r, 0, c1)),
            TablePart::LastRow => (total, (r, last_r, 0, last_c)),
            TablePart::FirstRow => (header, (0, r1, 0, last_c)),
            TablePart::SeCell => (total && last_col, (r, r1, c, c1)),
            TablePart::SwCell => (total && first_col, (r, r1, c, c1)),
            TablePart::NeCell => (header && last_col, (r, r1, c, c1)),
            TablePart::NwCell => (header && first_col, (r, r1, c, c1)),
        };
        let Some(ps) = def.part(part).filter(|_| on) else { continue };
        if let Some(p) = style_fill(ctx, ps) {
            look.paint = Some(p);
        }
        if let Some(tc) = &ps.text_color {
            look.text = Some(ctx.color(tc, None));
        }
        look.bold = ps.bold.or(look.bold);
        look.italic = ps.italic.or(look.italic);
        let (r0, rr1, c0, cc1) = region;
        let pick = [if c == c0 { 0 } else { 5 }, if c1 == cc1 { 1 } else { 5 }, if r == r0 { 2 } else { 4 }, if r1 == rr1 { 3 } else { 4 }];
        for (side, i) in pick.iter().enumerate() {
            if let Some(Some(b)) = ps.borders.get(*i) {
                look.sides[side] = Some(border_look(ctx, b));
            }
        }
    }
    look
}

/// The look of cell (`r`, `c`) from one of DeckCraft's own styles (`medium2-accent1`…), which it
/// writes without a definition: generated from the theme's accent colour (ported from
/// deckcraft-render's `table::look`, like [`style_look`]).
fn builtin_look(ctx: &resolve::Ctx, t: &Table, r: usize, c: usize) -> StyleLook {
    use deckcraft_color::{ColorTransform, SchemeSlot};
    let slot = match t.style.rsplit('-').next().unwrap_or("accent1") {
        "accent2" => SchemeSlot::Accent2,
        "accent3" => SchemeSlot::Accent3,
        "accent4" => SchemeSlot::Accent4,
        "accent5" => SchemeSlot::Accent5,
        "accent6" => SchemeSlot::Accent6,
        "tx1" => SchemeSlot::Tx1,
        _ => SchemeSlot::Accent1,
    };
    let a = ctx.color(&ColorRef::scheme(slot), None);
    let header = t.first_row && r == 0;
    let total = t.last_row && r + 1 == t.rows.len();
    let first_col = t.first_col && c == 0;
    let last_col = t.last_col && c + 1 == t.cols.len();
    let body_r = if t.first_row { r.wrapping_sub(1) } else { r };
    let band = t.band_row && !header && !total && body_r % 2 == 0;
    let band_c = t.band_col && c.is_multiple_of(2);
    let tint = |k: i32| deckcraft_color::apply(a, &[ColorTransform::Tint(k)]);
    let white = Rgba::WHITE;
    let dark_text = ctx.color(&ColorRef::scheme(SchemeSlot::Tx1), None);
    let edge = header || total || first_col || last_col;
    let (fill, text, bold, border): (Option<Rgba>, Option<Rgba>, bool, Option<(Rgba, f64)>) = match t.style.split('-').next().unwrap_or("medium2") {
        "none" => (None, None, false, None),
        "grid" => (None, None, false, Some((dark_text, 0.75))),
        "light1" => ((band || band_c).then(|| tint(20000)), None, edge, header.then_some((a, 1.0))),
        "light2" => (header.then_some(a), header.then_some(white), header || total || first_col, Some((a, 1.0))),
        "dark1" => (
            Some(if header {
                Rgba::BLACK
            } else if band || band_c {
                deckcraft_color::apply(a, &[ColorTransform::Shade(60000)])
            } else {
                deckcraft_color::apply(a, &[ColorTransform::Shade(75000)])
            }),
            Some(white),
            header || total || first_col,
            Some((white, 1.0)),
        ),
        "medium4" => (Some(if header || band || band_c { tint(40000) } else { tint(20000) }), None, edge, Some((a, 0.75))),
        _ => (Some(if edge { a } else if band || band_c { tint(40000) } else { tint(20000) }), edge.then_some(white), edge, Some((white, 1.0))),
    };
    StyleLook {
        paint: fill.map(|f| (Fill::solid(ColorRef::rgb(f)), None)),
        text,
        bold: Some(bold),
        italic: None,
        sides: [border; 4],
    }
}

/// A style part's fill, a theme fill reference resolved to the theme's fill with its colour as
/// the placeholder colour.
fn style_fill(ctx: &resolve::Ctx, ps: &TablePartStyle) -> Option<(Fill, Option<Rgba>)> {
    if let Some((idx, col)) = &ps.fill_ref {
        let ph = ctx.color(col, None);
        return Some(match ctx.master.theme.format.fills.get((*idx as usize).saturating_sub(1)) {
            Some(f) if *idx > 0 => (f.clone(), Some(ph)),
            _ if *idx == 0 => (Fill::None, None),
            _ => (Fill::solid(ColorRef::rgb(ph)), None),
        });
    }
    ps.fill.clone().map(|f| (f, None))
}

/// A style border's colour and width (a transparent colour for no line).
fn border_look(ctx: &resolve::Ctx, b: &TableBorder) -> (Rgba, f64) {
    match b {
        TableBorder::Line { line } => match &line.fill {
            Some(Fill::Solid { color: c }) => (ctx.color(c, None), line.width.unwrap_or(0.75)),
            Some(Fill::Gradient(g)) => (g.stops.first().map(|s| ctx.color(&s.color, None)).unwrap_or(Rgba::BLACK), line.width.unwrap_or(0.75)),
            _ => (Rgba::TRANSPARENT, 0.0),
        },
        TableBorder::Ref { idx, color } => {
            let w = ctx.master.theme.format.lines.get((*idx as usize).saturating_sub(1)).and_then(|l| l.width).unwrap_or(0.75);
            if *idx == 0 { (Rgba::TRANSPARENT, 0.0) } else { (ctx.color(color, None), w) }
        }
    }
}

/// A line with its colour as RGB; a line with no colour is no line.
fn rgb_line(ctx: &resolve::Ctx, l: &Line) -> Line {
    match &l.fill {
        None | Some(Fill::None) => Line::none(),
        Some(f) => Line { fill: Some(rgb_fill(ctx, f, None)), ..l.clone() },
    }
}

/// A table shape as `deck::table` (see the module docs). A style the file doesn't define is
/// warned about: its look isn't carried.
pub(crate) fn table_of(ctx: &resolve::Ctx, sh: &Shape, warnings: &mut Vec<String>, si: usize) -> Option<Value> {
    let ShapeKind::Table(t) = &sh.kind else { return None };
    let def = ctx.pres.table_styles.iter().find(|d| d.id.eq_ignore_ascii_case(&t.style));
    let builtin = deckcraft_model::table::table_styles().iter().any(|(id, _)| *id == t.style);
    if def.is_none() && !builtin && !t.style.is_empty() {
        warnings.push(format!("slide {}: `{}`'s table style `{}` isn't in the file; its look isn't carried", si + 1, sh.name, t.style));
    }
    let mut grid = Vec::new();
    for (r, row) in t.rows.iter().enumerate() {
        let mut cells = Vec::new();
        for (c, cell) in row.cells.iter().enumerate() {
            let look = match def {
                Some(d) => style_look(ctx, d, t, r, c),
                None if builtin => builtin_look(ctx, t, r, c),
                None => StyleLook::default(),
            };
            let mut baked = Cell { text: TextBody::default(), ..cell.clone() };
            baked.fill = match (&cell.fill, look.paint) {
                (Some(f), _) => Some(rgb_fill(ctx, f, None)),
                (None, Some((f, ph))) => Some(rgb_fill(ctx, &f, ph)),
                (None, None) => None,
            };
            for (i, side) in look.sides.iter().enumerate() {
                baked.borders[i] = match (&cell.borders[i], side) {
                    (Some(l), _) => Some(rgb_line(ctx, l)),
                    (None, Some((col, _))) if col.a == 0 => Some(Line::none()),
                    (None, Some((col, w))) => Some(Line::solid(ColorRef::rgb(*col), *w)),
                    (None, None) => None,
                };
            }
            baked.diag_down = cell.diag_down.as_ref().map(|l| rgb_line(ctx, l));
            baked.diag_up = cell.diag_up.as_ref().map(|l| rgb_line(ctx, l));
            // The text: the style's colour, bold and italic where the run sets none, theme
            // colours and fonts worked out, and the size the shape's text style gives.
            let mut body = cell.text.clone();
            for p in &mut body.paragraphs {
                let para = p.clone();
                for run in &mut p.runs {
                    let rp = &mut run.props;
                    if rp.fill.is_none()
                        && let Some(c) = look.text
                    {
                        rp.fill = Some(Fill::solid(ColorRef::rgb(c)));
                    }
                    rp.bold = rp.bold.or(look.bold.filter(|b| *b));
                    rp.italic = rp.italic.or(look.italic.filter(|b| *b));
                    rp.fill = rp.fill.as_ref().map(|f| rgb_fill(ctx, f, None));
                    rp.font = Some(resolve::font_family(ctx, rp));
                    if rp.size.is_none() {
                        rp.size = resolve::run(ctx, sh, &para, rp).size;
                    }
                }
            }
            let paragraphs: Vec<Value> = read_paragraphs(&body, None)
                .into_iter()
                .zip(&body.paragraphs)
                .map(|(mut m, p)| {
                    // Indents and spacing set in the cell itself.
                    if let Some(v) = p.props.margin_left {
                        m.insert("marginLeftPt".into(), json!(v));
                    }
                    if let Some(v) = p.props.indent {
                        m.insert("indentPt".into(), json!(v));
                    }
                    for (k, v) in [("spaceBefore", &p.props.space_before), ("spaceAfter", &p.props.space_after), ("lineSpacing", &p.props.line_spacing)] {
                        if let Some(v) = v.as_ref().and_then(|v| serde_json::to_value(v).ok()) {
                            m.insert(k.into(), v);
                        }
                    }
                    Value::Object(m)
                })
                .collect();
            let mut look = serde_json::to_value(&baked).unwrap_or(Value::Null);
            if let Some(o) = look.as_object_mut() {
                o.remove("text");
            }
            cells.push(json!({ "text": cell.text.text().trim(), "paragraphs": paragraphs, "look": look }));
        }
        grid.push(Value::Array(cells));
    }
    Some(json!({
        "cols": t.cols.iter().map(|w| px(*w)).collect::<Vec<_>>(),
        "rows": t.rows.iter().map(|r| px(r.height)).collect::<Vec<_>>(),
        "snapshot": grid,
        "props": {
            "firstRow": t.first_row, "firstCol": t.first_col, "lastRow": t.last_row,
            "lastCol": t.last_col, "bandRow": t.band_row, "bandCol": t.band_col,
        },
    }))
}

/// A cell look as compared: what an editor may write either way means the same (no fill, no
/// line, default margins, top anchor).
fn canonical_look(look: &Value) -> Value {
    let mut cell: Cell = serde_json::from_value(look.clone()).unwrap_or_default();
    cell.fill = Some(cell.fill.unwrap_or(Fill::None));
    let none = |l: &Option<Line>| match l {
        Some(l) if !matches!(l.fill, None | Some(Fill::None)) => Some(l.clone()),
        _ => None,
    };
    for b in &mut cell.borders {
        *b = none(b);
    }
    cell.diag_down = none(&cell.diag_down);
    cell.diag_up = none(&cell.diag_up);
    cell.margins = Some(cell.margins.unwrap_or([7.2, 7.2, 3.6, 3.6]));
    cell.anchor = Some(cell.anchor.unwrap_or(Anchor::Top));
    let mut v = serde_json::to_value(&cell).unwrap_or(Value::Null);
    for k in ["borders", "diagDown", "diagUp"] {
        match v.get_mut(k) {
            Some(Value::Array(a)) => a.iter_mut().for_each(|l| *l = plain_line(l)),
            Some(l) if !l.is_null() => *l = plain_line(l),
            _ => {}
        }
    }
    v
}

/// Gives both cells on each inside edge the same line, the wider of the two (PowerPoint writes it
/// so on save; LibreOffice keeps each cell's own).
fn shared_edges(grid: &mut [Vec<Value>]) {
    let width = |l: &Value| if l.is_null() { -1.0 } else { l.get("width").and_then(Value::as_f64).unwrap_or(0.75) };
    let unify = |a: &mut Value, b: &mut Value| {
        let w = if width(a) >= width(b) { a.clone() } else { b.clone() };
        (*a, *b) = (w.clone(), w);
    };
    for r in 0..grid.len() {
        for c in 0..grid[r].len() {
            // Right edge with the cell on the right (left side is 0, right 1, top 2, bottom 3).
            if c + 1 < grid[r].len() {
                let (left, right) = grid[r].split_at_mut(c + 1);
                let (mut a, mut b) = (left[c]["borders"][1].take(), right[0]["borders"][0].take());
                unify(&mut a, &mut b);
                (left[c]["borders"][1], right[0]["borders"][0]) = (a, b);
            }
            if r + 1 < grid.len() && c < grid[r + 1].len() {
                let (up, down) = grid.split_at_mut(r + 1);
                let (mut a, mut b) = (up[r][c]["borders"][3].take(), down[0][c]["borders"][2].take());
                unify(&mut a, &mut b);
                (up[r][c]["borders"][3], down[0][c]["borders"][2]) = (a, b);
            }
        }
    }
}

/// A `deck::table` as compared: `cols`, `rows`, and per cell its paragraphs as text frames compare
/// them and its look with shared edges given one line.
pub(crate) fn canonical_table(v: &Value) -> Value {
    let rows = v.get("snapshot").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut looks: Vec<Vec<Value>> = rows
        .iter()
        .map(|row| row.as_array().into_iter().flatten().map(|c| canonical_look(c.get("look").unwrap_or(&Value::Null))).collect())
        .collect();
    shared_edges(&mut looks);
    let cells: Vec<Value> = rows
        .iter()
        .zip(looks)
        .map(|(row, looks)| {
            row.as_array()
                .into_iter()
                .flatten()
                .zip(looks)
                .map(|(c, look)| {
                    // LibreOffice drops the level of a paragraph without a bullet in a cell.
                    let paras: Vec<Value> = c
                        .get("paragraphs")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_object)
                        .map(|p| {
                            let mut p = canonical_para(p);
                            if p["bullet"] != json!(true) {
                                p["level"] = json!(0);
                            }
                            p
                        })
                        .collect();
                    json!({ "paragraphs": paras, "look": look })
                })
                .collect()
        })
        .collect();
    json!({ "cols": v.get("cols"), "rows": v.get("rows"), "cells": cells })
}

/// Whether two `deck::table` values show the same table: column widths and row heights within
/// 0.6 px, cells' text exactly and their looks within 0.1 (an editor's own units).
pub(crate) fn same_table(a: &Value, b: &Value) -> bool {
    let (a, b) = (canonical_table(a), canonical_table(b));
    near(&a["cols"], &b["cols"], 0.6)
        && near(&a["rows"], &b["rows"], 0.6)
        && a["cells"].as_array().map(Vec::len) == b["cells"].as_array().map(Vec::len)
        && a["cells"].as_array().into_iter().flatten().zip(b["cells"].as_array().into_iter().flatten()).all(|(p, q)| {
            p.as_array().map(Vec::len) == q.as_array().map(Vec::len)
                && p.as_array().into_iter().flatten().zip(q.as_array().into_iter().flatten()).all(|(x, y)| x["paragraphs"] == y["paragraphs"] && near(&x["look"], &y["look"], 0.1))
        })
}

/// A `deck::table` with cells (not a linked table) as a native table: cell looks as they are,
/// no style. `text` builds a cell's text from its paragraphs (or its plain text).
pub(crate) fn table_shape(t: &Map<String, Value>, w: f64, h: f64, text: &mut dyn FnMut(&Map<String, Value>) -> TextBody) -> ShapeKind {
    let grid: Vec<Vec<Map<String, Value>>> = t
        .get("snapshot")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|r| r.as_array().into_iter().flatten().filter_map(Value::as_object).cloned().collect())
        .collect();
    let n_cols = grid.iter().map(Vec::len).max().unwrap_or(1).max(1);
    let n_rows = grid.len().max(1);
    let sizes = |k: &str, n: usize, total: f64| -> Vec<f64> {
        let v: Vec<f64> = t.get(k).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_f64).map(pt).collect();
        if v.len() == n { v } else { vec![total / n as f64; n] }
    };
    let mut table = Table::new(n_rows, n_cols, w, h / n_rows as f64);
    table.cols = sizes("cols", n_cols, w);
    table.style = "none".into();
    let flag = |k: &str| t.get("props").and_then(|p| p.get(k)).and_then(Value::as_bool);
    table.first_row = flag("firstRow").unwrap_or(table.first_row);
    table.first_col = flag("firstCol").unwrap_or(false);
    table.last_row = flag("lastRow").unwrap_or(false);
    table.last_col = flag("lastCol").unwrap_or(false);
    table.band_row = flag("bandRow").unwrap_or(table.band_row);
    table.band_col = flag("bandCol").unwrap_or(false);
    let heights = sizes("rows", n_rows, h);
    table.rows = (0..n_rows)
        .map(|r| Row {
            height: heights[r],
            cells: (0..n_cols)
                .map(|c| match grid.get(r).and_then(|row| row.get(c)) {
                    Some(m) => {
                        let mut cell: Cell = m.get("look").cloned().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_else(|| Cell::new(""));
                        cell.text = text(m);
                        cell
                    }
                    None => Cell::new(""),
                })
                .collect(),
        })
        .collect();
    ShapeKind::Table(table)
}
