//! Edited DeckCraft `Presentation` → minimal OX edit layer.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use deckcraft_model::style::{ColorBase, Fill, PictureAdjust, PictureMode};
use deckcraft_model::text::{Action, Anchor, Bullet, RunKind};
use deckcraft_model::{ColorRef, Presentation, RunProps, Shape, ShapeId, ShapeKind, TextBody, resolve};
use ox_core::Composed;
use ox_core::model::{Header, Layer, Node, OX_VERSION, stable_id};
use ox_core::render::{chart_data, field_text, format_value};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::{DeckError, KEY_DECK, KEY_REVISION, NAME_PREFIX, px, revision_of};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Conflict {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ImportReport {
    /// Frames moved or resized.
    pub moved: Vec<String>,
    /// Frames whose text (or fill) changed, slides whose notes changed.
    pub changed: Vec<String>,
    /// New slides and frames.
    pub added: Vec<String>,
    /// Slides and frames removed (child names).
    pub removed: Vec<String>,
    /// Slides reordered, or frames restacked.
    pub reordered: Vec<String>,
    /// Edits to live-linked tables, charts and fields, not written back.
    pub conflicts: Vec<Conflict>,
    pub warnings: Vec<String>,
    /// The stack changed since this file was exported.
    pub stale: bool,
    /// How each slide, frame and live field in the file was recognised, and how sure that is.
    pub matches: Vec<ox_core::Match>,
    /// The bytes of every picture the layer names, by blob value (`sha256-<hex>`), with their
    /// media type: the caller stores the ones its project doesn't have yet.
    #[serde(skip)]
    pub pictures: BTreeMap<String, (String, Vec<u8>)>,
}

impl ImportReport {
    pub fn is_empty(&self) -> bool {
        self.moved.is_empty()
            && self.changed.is_empty()
            && self.added.is_empty()
            && self.removed.is_empty()
            && self.reordered.is_empty()
    }
}

fn obj<'a>(n: &'a Composed, key: &str) -> Option<&'a Map<String, Value>> {
    n.attributes.get(key).and_then(Value::as_object)
}

/// The composed revision a deck was exported at (`oxrevision=`), if it carries one.
pub fn exported_revision(p: &Presentation) -> Option<String> {
    keyword(p, KEY_REVISION)
}

fn keyword(p: &Presentation, key: &str) -> Option<String> {
    p.props
        .keywords
        .split_whitespace()
        .find_map(|w| w.strip_prefix(key))
        .map(String::from)
}

fn fill_hex(f: &Option<Fill>) -> Option<String> {
    match f {
        Some(Fill::Solid { color }) => match &color.base {
            ColorBase::Rgb { rgb } => Some(format!("#{:02x}{:02x}{:02x}", rgb.r, rgb.g, rgb.b)),
            _ => None,
        },
        _ => None,
    }
}

/// The run keys OX carries (direct formatting only).
fn run_format(p: &RunProps) -> Map<String, Value> {
    let mut m = Map::new();
    if p.bold == Some(true) {
        m.insert("bold".into(), json!(true));
    }
    if p.italic == Some(true) {
        m.insert("italic".into(), json!(true));
    }
    if p.underline.as_deref().is_some_and(|u| u != "none") {
        m.insert("underline".into(), json!(true));
    }
    if let Some(c) = fill_hex(&p.fill) {
        m.insert("color".into(), json!(c));
    }
    if let Some(f) = &p.font {
        m.insert("font".into(), json!(f));
    }
    if let Some(s) = p.size {
        m.insert("sizePt".into(), json!(s));
    }
    // Web and mail links; other actions are warned about (`link_warnings`).
    if let Some(Action::Url { url }) = p.link.as_ref().map(|h| &h.action) {
        m.insert("link".into(), json!(url));
    }
    m
}

/// What a shape's links and actions hold that the layer doesn't carry: actions other than a web
/// or mail link, tooltips, mouse-over actions and actions on the shape itself.
fn link_warnings(sh: &Shape, si: usize, warnings: &mut Vec<String>) {
    let mut lost = BTreeSet::new();
    if sh.click.is_some() || sh.hover.is_some() {
        lost.insert("action on the shape");
    }
    let mut bodies: Vec<&TextBody> = sh.text.iter().collect();
    if let ShapeKind::Table(t) = &sh.kind {
        bodies.extend(t.rows.iter().flat_map(|r| r.cells.iter().map(|c| &c.text)));
    }
    for r in bodies.iter().flat_map(|b| b.paragraphs.iter().flat_map(|p| p.runs.iter())) {
        if let Some(h) = &r.props.link {
            if !matches!(h.action, Action::Url { .. }) {
                lost.insert("link to a slide or show action");
            }
            if !h.tooltip.is_empty() {
                lost.insert("link tooltip");
            }
        }
        if r.props.hover.is_some() {
            lost.insert("mouse-over action");
        }
    }
    if !lost.is_empty() {
        warnings.push(format!("slide {}: `{}`'s {} not carried", si + 1, sh.name, lost.into_iter().collect::<Vec<_>>().join(", ")));
    }
}

/// Merges adjacent text runs with the same formatting; drops empty ones.
fn merge_runs(runs: Vec<Map<String, Value>>) -> Vec<Map<String, Value>> {
    let mut out: Vec<Map<String, Value>> = Vec::new();
    for mut r in runs {
        if let Some(Value::String(c)) = r.get_mut("color") {
            *c = c.to_lowercase();
        }
        // 54 and 54.0 are the same size.
        if let Some(sz) = r.get("sizePt").and_then(Value::as_f64) {
            r.insert("sizePt".into(), json!(sz));
        }
        if r.contains_key("field") {
            out.push(r);
            continue;
        }
        let text = r
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if text.is_empty() {
            continue;
        }
        let mut fmt = r.clone();
        fmt.remove("text");
        if let Some(last) = out.last_mut()
            && !last.contains_key("field")
        {
            let mut lf = last.clone();
            lf.remove("text");
            if lf == fmt {
                let joined = format!(
                    "{}{text}",
                    last.get("text").and_then(Value::as_str).unwrap_or("")
                );
                last.insert("text".into(), json!(joined));
                continue;
            }
        }
        out.push(r);
    }
    out
}

/// A paragraph as OX sees it, for comparison: text runs merged, fields by name only.
pub(crate) fn canonical_para(p: &Map<String, Value>) -> Value {
    let runs: Vec<Map<String, Value>> = p
        .get("runs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .map(|r| match r.get("field") {
            Some(f) => Map::from_iter([("field".to_string(), f.clone())]),
            None => r.clone(),
        })
        .collect();
    // A line break joins the run before it: LibreOffice writes it as a run of its own.
    let mut joined: Vec<Map<String, Value>> = Vec::new();
    for r in runs {
        match (r.get("text").and_then(Value::as_str), joined.last_mut()) {
            (Some("\n"), Some(prev)) if prev.get("text").is_some_and(Value::is_string) => {
                let t = format!("{}\n", prev["text"].as_str().unwrap_or(""));
                prev.insert("text".into(), json!(t));
            }
            _ => joined.push(r),
        }
    }
    // Spaces at the end of a paragraph can't be seen.
    if let Some(t) = joined.last_mut().and_then(|r| r.get_mut("text")).and_then(|t| t.as_str().map(|s| s.trim_end_matches(' ').to_string())) {
        if let Some(r) = joined.last_mut() {
            r.insert("text".into(), json!(t));
        }
    }
    let runs = joined;
    // An empty line shows neither a bullet nor a level (LibreOffice resets both).
    let blank = runs.iter().all(|r| r.get("text").and_then(Value::as_str).is_none_or(|t| t.trim().is_empty()));
    let bullet = !blank && p.get("bullet").and_then(Value::as_bool).unwrap_or(false);
    json!({
        "runs": merge_runs(runs),
        "bullet": bullet,
        "level": if blank { 0 } else { p.get("level").and_then(Value::as_u64).unwrap_or(0) },
        "align": p.get("align").and_then(Value::as_str).unwrap_or("left"),
    })
}

/// Paragraphs of an edited text body, in OX form (no fields yet).
/// A text body's paragraphs as `deck::text` paragraphs. With `inherit` (the slide's context and
/// the shape), each paragraph and run takes its effective formatting: what the layout and master
/// give it (bullets by level, sizes, colours, fonts), since an adopted deck's layouts aren't kept.
pub(crate) fn read_paragraphs(t: &TextBody, inherit: Option<(&resolve::Ctx, &Shape)>) -> Vec<Map<String, Value>> {
    t.paragraphs
        .iter()
        .map(|p| {
            let pp = match inherit {
                Some((ctx, sh)) => resolve::para(ctx, sh, p),
                None => p.props.clone(),
            };
            let mut runs = Vec::new();
            for r in &p.runs {
                let mut m = match inherit {
                    Some((ctx, sh)) => {
                        let rp = resolve::run(ctx, sh, p, &r.props);
                        let mut m = run_format(&rp);
                        let c = resolve::text_color(ctx, &rp);
                        m.insert("color".into(), json!(format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)));
                        m.insert("font".into(), json!(resolve::font_family(ctx, &rp)));
                        m
                    }
                    None => run_format(&r.props),
                };
                let text = match r.kind {
                    RunKind::Break => "\n".to_string(),
                    _ => r.text.clone(),
                };
                m.insert("text".into(), json!(text));
                runs.push(m);
            }
            let mut para = Map::new();
            para.insert(
                "runs".into(),
                Value::Array(merge_runs(runs).into_iter().map(Value::Object).collect()),
            );
            let align = match pp.align {
                Some(deckcraft_model::text::Align::Center) => "center",
                Some(deckcraft_model::text::Align::Right) => "right",
                Some(deckcraft_model::text::Align::Justify) => "justify",
                _ => "left",
            };
            para.insert("align".into(), json!(align));
            match &pp.bullet {
                Some(Bullet::Char { char }) => {
                    para.insert("bullet".into(), json!(true));
                    if inherit.is_some() {
                        para.insert("bulletChar".into(), json!(char));
                    }
                }
                Some(Bullet::AutoNum { scheme, start_at }) => {
                    para.insert("bullet".into(), json!(true));
                    if inherit.is_some() {
                        para.insert("autoNum".into(), json!({ "scheme": scheme, "startAt": start_at }));
                    }
                }
                _ => {}
            }
            if p.level > 0 || para.contains_key("bullet") {
                para.insert("level".into(), json!(p.level));
            }
            if let Some((ctx, _)) = inherit {
                // Indents in points, spacing (DeckCraft's form) and the bullet's look, as the
                // layout gave them.
                if let Some(m) = pp.margin_left {
                    para.insert("marginLeftPt".into(), json!(m));
                }
                if let Some(i) = pp.indent {
                    para.insert("indentPt".into(), json!(i));
                }
                for (k, v) in [("spaceBefore", &pp.space_before), ("spaceAfter", &pp.space_after), ("lineSpacing", &pp.line_spacing)] {
                    if let Some(v) = v.as_ref().and_then(|v| serde_json::to_value(v).ok()) {
                        para.insert(k.into(), v);
                    }
                }
                if para.contains_key("bullet") {
                    if let Some(c) = &pp.bullet_color {
                        let c = ctx.color(c, None);
                        para.insert("bulletColor".into(), json!(format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)));
                    }
                    if let Some(f) = &pp.bullet_font {
                        para.insert("bulletFont".into(), json!(f));
                    }
                    if let Some(sz) = pp.bullet_size {
                        para.insert("bulletSize".into(), json!(sz));
                    }
                }
            }
            para
        })
        .collect()
}

/// A shape's effective fill, outline and geometry as `deck::shape`, or `None` when it shows
/// neither fill nor outline. Gradients and pictures become their first colour (said in a warning).
fn shape_look(ctx: &resolve::Ctx, sh: &Shape, warnings: &mut Vec<String>, si: usize) -> Option<Value> {
    let hex = |c: deckcraft_model::Rgba| format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b);
    let mut colour = |f: &Fill, ph: Option<deckcraft_model::Rgba>, what: &str| match f {
        Fill::Solid { color } => Some(hex(ctx.color(color, ph))),
        Fill::Gradient(g) => {
            warnings.push(format!("slide {}: `{}`'s {what} gradient isn't carried; used its first colour", si + 1, sh.name));
            g.stops.first().map(|s| hex(ctx.color(&s.color, ph)))
        }
        Fill::None | Fill::Background | Fill::Group => None,
        _ => {
            warnings.push(format!("slide {}: `{}`'s {what} (picture or pattern) isn't carried", si + 1, sh.name));
            None
        }
    };
    let (fill, fph) = resolve::fill(ctx, sh);
    let fill = fill.as_ref().and_then(|f| colour(f, fph, "fill"));
    let (line, lph) = resolve::line(ctx, sh);
    let stroke = line.fill.as_ref().and_then(|f| colour(f, lph, "outline"));
    if fill.is_none() && stroke.is_none() {
        return None;
    }
    let kind = match &sh.geom {
        deckcraft_model::Geom::Preset { name, .. } if name == "ellipse" || name == "roundRect" => name.as_str(),
        deckcraft_model::Geom::Preset { name, .. } if name != "rect" => {
            warnings.push(format!("slide {}: `{}` is a `{name}`; drawn as a rectangle", si + 1, sh.name));
            "rect"
        }
        _ => "rect",
    };
    let mut m = Map::from_iter([("kind".to_string(), json!(kind))]);
    if let Some(f) = fill {
        m.insert("fill".into(), json!(f));
    }
    if let Some(st) = stroke {
        m.insert("stroke".into(), json!(st));
        m.insert("strokeWidth".into(), json!(px(line.width.unwrap_or(0.75))));
    }
    Some(Value::Object(m))
}

/// How many times `shown` stands in a paragraph's text runs.
fn occurrences(para: &Map<String, Value>, shown: &str) -> usize {
    if shown.is_empty() {
        return 0;
    }
    para.get("runs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|r| r.get("text").and_then(Value::as_str))
        .map(|t| t.matches(shown).count())
        .sum()
}

/// Finds a field's shown text in a paragraph's runs and turns it back into a field run.
/// `false` when the text is no longer there (the field was edited or deleted).
fn reanchor(para: &mut Map<String, Value>, name: &str, shown: &str) -> bool {
    let Some(runs) = para.get("runs").and_then(Value::as_array).cloned() else {
        return false;
    };
    for (i, r) in runs.iter().enumerate() {
        let Some(text) = r.get("text").and_then(Value::as_str) else {
            continue;
        };
        if let Some(at) = text.find(shown) {
            let fmt: Map<String, Value> = r
                .as_object()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|(k, _)| k != "text")
                .collect();
            let mut pieces = Vec::new();
            let (before, after) = (&text[..at], &text[at + shown.len()..]);
            if !before.is_empty() {
                let mut m = fmt.clone();
                m.insert("text".into(), json!(before));
                pieces.push(Value::Object(m));
            }
            let mut f = fmt.clone();
            f.insert("field".into(), json!(name));
            pieces.push(Value::Object(f));
            if !after.is_empty() {
                let mut m = fmt;
                m.insert("text".into(), json!(after));
                pieces.push(Value::Object(m));
            }
            let mut new_runs = runs.clone();
            new_runs.splice(i..=i, pieces);
            para.insert("runs".into(), Value::Array(new_runs));
            return true;
        }
    }
    false
}

fn table_grid(sh: &Shape) -> Option<Vec<Vec<String>>> {
    let ShapeKind::Table(t) = &sh.kind else {
        return None;
    };
    Some(
        t.rows
            .iter()
            .map(|r| {
                r.cells
                    .iter()
                    .map(|c| c.text.text().trim().to_string())
                    .collect()
            })
            .collect(),
    )
}

fn expected_grid(fr: &Composed) -> Vec<Vec<String>> {
    let data = obj(fr, "office::data::table").cloned().unwrap_or_default();
    let cols: Vec<Map<String, Value>> = data
        .get("columns")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|c| c.as_object().cloned()).collect())
        .unwrap_or_default();
    let mut grid = vec![
        cols.iter()
            .map(|c| {
                c.get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string()
            })
            .collect::<Vec<_>>(),
    ];
    for row in data
        .get("rows")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let cells = row.as_array().cloned().unwrap_or_default();
        grid.push(
            (0..cols.len())
                .map(|i| {
                    format_value(
                        cells.get(i).unwrap_or(&Value::Null),
                        cols[i]
                            .get("type")
                            .and_then(Value::as_str)
                            .unwrap_or("text"),
                    )
                })
                .collect(),
        );
    }
    grid
}

fn ox_name(sh: &Shape) -> Option<&str> {
    sh.name.strip_prefix(NAME_PREFIX)
}

/// The position a placeholder inherits: the same placeholder (by index, else by kind) on the slide's
/// layout, else on its master.
fn inherited_xfrm(p: &Presentation, slide: &deckcraft_model::Slide, sh: &Shape) -> Option<deckcraft_model::Xfrm> {
    let ph = sh.ph.as_ref()?;
    let same = |o: &Shape| {
        o.ph.as_ref().is_some_and(|q| (ph.idx != 0 && q.idx == ph.idx) || q.kind == ph.kind) && o.xfrm.is_some()
    };
    for m in &p.masters {
        if let Some(l) = m.layout(slide.layout) {
            return l.shapes.iter().find(|o| same(o)).or_else(|| m.shapes.iter().find(|o| same(o))).and_then(|o| o.xfrm);
        }
    }
    None
}

fn geometry(sh: &Shape) -> Option<Map<String, Value>> {
    let x = sh.xfrm?;
    let mut m = Map::new();
    for (k, v) in [("x", x.x), ("y", x.y), ("w", x.w), ("h", x.h)] {
        m.insert(k.into(), json!(px(v)));
    }
    if x.rot.abs() > 0.01 {
        m.insert("rotation".into(), json!((x.rot * 100.0).round() / 100.0));
    }
    // A line's direction is its flips.
    for (k, on) in [("flipH", x.flip_h), ("flipV", x.flip_v)] {
        if on {
            m.insert(k.into(), json!(true));
        }
    }
    Some(m)
}

/// A colour fill with its theme colour worked out as RGB (the export has a theme of its own); a
/// gradient becomes its first colour.
pub(crate) fn rgb_fill(ctx: &resolve::Ctx, f: &Fill, ph: Option<deckcraft_model::Rgba>) -> Fill {
    match f {
        Fill::Solid { color } => Fill::solid(ColorRef::rgb(ctx.color(color, ph))),
        Fill::Gradient(g) => match g.stops.first() {
            Some(s) => Fill::solid(ColorRef::rgb(ctx.color(&s.color, ph))),
            None => Fill::None,
        },
        f => f.clone(),
    }
}

/// A line or connector as `deck::line`: its shape (DeckCraft's geometry), its effective outline
/// with theme colours as RGB, and the frames its ends are attached to (`{"frame", "site"}`; a
/// shape not known yet is `{"shape": id}` until the slide is done, see `attach_ends`).
fn line_of(ctx: &resolve::Ctx, sh: &Shape, ids: &HashMap<ShapeId, String>) -> Value {
    let (mut line, ph) = resolve::line(ctx, sh);
    if let Some(f) = &line.fill {
        line.fill = Some(rgb_fill(ctx, f, ph));
    }
    let mut m = Map::new();
    m.insert("geom".into(), serde_json::to_value(&sh.geom).unwrap_or(Value::Null));
    m.insert("line".into(), serde_json::to_value(&line).unwrap_or(Value::Null));
    if let ShapeKind::Connector { start, end } = &sh.kind {
        for (k, e) in [("from", start), ("to", end)] {
            if let Some((id, site)) = e {
                m.insert(k.into(), match ids.get(id) {
                    Some(p) => json!({ "frame": p, "site": site }),
                    None => json!({ "shape": id.0, "site": site }),
                });
            }
        }
    }
    Value::Object(m)
}

/// A line shape the layer carries as `deck::line`: a connector or a line-like preset, without text.
fn is_line(sh: &Shape) -> bool {
    sh.is_line() && sh.text.as_ref().is_none_or(|t| t.text().trim().is_empty())
}

/// JSON equality with numbers within `tol` (editors round lengths to their own units).
pub(crate) fn near(a: &Value, b: &Value, tol: f64) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => (x.as_f64().unwrap_or(0.0) - y.as_f64().unwrap_or(0.0)).abs() <= tol,
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(a, b)| near(a, b, tol)),
        (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| near(v, w, tol))),
        _ => a == b,
    }
}

/// A DeckCraft `Line` (as JSON) without the values editors write out for an unset one: flat cap,
/// single line, solid dash, round join, no arrowheads.
pub(crate) fn plain_line(l: &Value) -> Value {
    let mut l = l.clone();
    if let Some(m) = l.as_object_mut() {
        for (k, v) in [("cap", json!("flat")), ("compound", json!("single")), ("dash", json!({ "kind": "preset", "name": "solid" })), ("join", json!("round"))] {
            if m.get(k) == Some(&v) {
                m.remove(k);
            }
        }
        for k in ["head", "tail"] {
            if m.get(k).and_then(|e| e.get("kind")) == Some(&json!("none")) {
                m.remove(k);
            }
        }
        m.retain(|_, v| !v.is_null());
    }
    l
}

/// A `deck::line` as compared: a straight `line` and a `straightConnector1` draw the same
/// (LibreOffice writes one as the other), and default line values don't count.
fn canonical_line(v: &Value) -> Value {
    let mut v = v.clone();
    if v.pointer("/geom/name") == Some(&json!("line")) {
        v["geom"]["name"] = json!("straightConnector1");
    }
    if let Some(l) = v.get("line") {
        v["line"] = plain_line(l);
    }
    v
}

/// LibreOffice saves every link in the theme's hyperlink colour (`hl`): on a link, that colour is
/// the editor's, not an edit, so such a run takes the colour the matching run in `like` has.
fn editor_link_colour(b: &mut [Value], like: &[Value], hl: &str) {
    for (pb, pl) in b.iter_mut().zip(like) {
        let (Some(rb), Some(rl)) = (pb.get_mut("runs").and_then(Value::as_array_mut), pl.get("runs").and_then(Value::as_array)) else { continue };
        for (r, l) in rb.iter_mut().zip(rl) {
            if r.get("link").is_some() && r.get("color").and_then(Value::as_str) == Some(hl)
                && let Some(m) = r.as_object_mut()
            {
                match l.get("color") {
                    Some(c) => m.insert("color".into(), c.clone()),
                    None => m.remove("color"),
                };
            }
        }
    }
}

/// Points `deck::line` ends still naming a shape id (`{"shape": id}`) at the frame that shape became
/// on this slide; an end on something the layer doesn't carry is dropped, with a warning.
fn attach_ends(nodes: &mut [Node], ids: &HashMap<ShapeId, String>, si: usize, warnings: &mut Vec<String>) {
    for n in nodes {
        let Some(Value::Object(l)) = n.attributes.get_mut("deck::line") else { continue };
        for k in ["from", "to"] {
            let Some(id) = l.get(k).and_then(|e| e.get("shape")).and_then(Value::as_u64) else { continue };
            let site = l[k].get("site").cloned().unwrap_or(json!(0));
            match ids.get(&ShapeId(id as u32)) {
                Some(p) => {
                    l.insert(k.into(), json!({ "frame": p, "site": site }));
                }
                None => {
                    l.remove(k);
                    warnings.push(format!("slide {}: a connector's end is attached to something not carried; it is left unattached", si + 1));
                }
            }
        }
    }
}

/// A picture shape as `deck::picture` (blob, media type, alt text, crop), its bytes kept in the
/// report. A linked picture isn't carried, and looks the layer can't hold yet are warnings.
fn picture(edited: &Presentation, sh: &Shape, si: usize, report: &mut ImportReport) -> Option<Map<String, Value>> {
    let ShapeKind::Picture { fill } = &sh.kind else {
        return None;
    };
    let Some(m) = edited.media(fill.media).filter(|m| m.link.is_none() && !m.data.is_empty()) else {
        report.warnings.push(format!("slide {}: `{}` is a linked picture; not carried", si + 1, sh.name));
        return None;
    };
    let blob = ox_core::stack::integrity_of(&m.data);
    report.pictures.insert(blob.clone(), (m.content_type.clone(), m.data.to_vec()));
    let mut p = Map::from_iter([("blob".to_string(), json!(blob)), ("mediaType".to_string(), json!(m.content_type))]);
    if !sh.descr.is_empty() {
        p.insert("alt".into(), json!(sh.descr));
    }
    if fill.crop.iter().any(|c| c.abs() > 1e-4) {
        p.insert("crop".into(), json!(fill.crop.map(|c| (c * 1e4).round() / 1e4)));
    }
    let lost: Vec<&str> = [
        (fill.alpha.is_some(), "transparency"),
        (fill.adjust != PictureAdjust::default(), "colour corrections"),
        (fill.artistic.is_some(), "artistic effect (kept as it looks)"),
        (matches!(fill.mode, PictureMode::Tile { .. }), "tiling"),
    ]
    .into_iter()
    .filter_map(|(on, what)| on.then_some(what))
    .collect();
    if !lost.is_empty() {
        report.warnings.push(format!("slide {}: `{}`'s {} not carried", si + 1, sh.name, lost.join(", ")));
    }
    Some(p)
}

fn geometry_changed(old: &Map<String, Value>, new: &Map<String, Value>) -> bool {
    let flip = |m: &Map<String, Value>, k: &str| m.get(k).and_then(Value::as_bool).unwrap_or(false);
    ["x", "y", "w", "h", "rotation"].iter().any(|k| {
        let a = old.get(*k).and_then(Value::as_f64).unwrap_or(0.0);
        let b = new.get(*k).and_then(Value::as_f64).unwrap_or(0.0);
        (a - b).abs() > 0.6
    }) || ["flipH", "flipV"].iter().any(|k| flip(old, k) != flip(new, k))
}

/// Compares the edited presentation with the composed deck `node` it was exported from and
/// returns the edit layer.
pub fn import(
    deck: &Composed,
    edited: &Presentation,
    header: Header,
) -> Result<(Layer, ImportReport), DeckError> {
    import_in(deck, edited, header, None)
}

/// [`import`], with the folder a stack's pictures are read from when they're still named by a
/// path (a stack file's, not stored by hash): they're compared by the hash of that file.
pub fn import_in(
    deck: &Composed,
    edited: &Presentation,
    header: Header,
    assets: Option<&std::path::Path>,
) -> Result<(Layer, ImportReport), DeckError> {
    let found = keyword(edited, KEY_DECK).ok_or(DeckError::NotExported)?;
    if found != deck.path {
        return Err(DeckError::WrongDeck {
            expected: deck.path.clone(),
            found,
        });
    }
    import_into(deck, edited, header, false, assets)
}

/// Brings a presentation Milleleaf didn't make into a project: it is imported into an empty
/// `deck::deck` node at `path` (the file's slide size), so every slide and text box becomes new
/// content. What the importer can't carry yet is in the report's warnings.
pub fn adopt(
    edited: &Presentation,
    path: &str,
    title: &str,
    header: Header,
) -> Result<(Layer, ImportReport), DeckError> {
    // The slide size unrounded: px() rounds to 0.01 px, which moves a 1280 pt wide slide by
    // 32 EMU (16256000 → 16256032), a different slide size to every reader.
    let exact = |pt: f64| pt * crate::PX_PER_PT;
    let attr = json!({
        "width": exact(edited.slide_size.width),
        "height": exact(edited.slide_size.height),
        "unit": "px",
        "title": title,
    });
    let empty = Composed {
        path: path.to_string(),
        name: None,
        attributes: Map::from_iter([("deck::deck".into(), attr.clone())]),
        provenance: Default::default(),
        children: vec![],
    };
    let (mut layer, report) = import_into(&empty, edited, header, true, None)?;
    match layer.data.iter_mut().find(|n| n.path == path) {
        Some(n) => {
            n.attributes.insert("deck::deck".into(), attr);
        }
        None => layer.data.insert(
            0,
            Node {
                path: path.to_string(),
                attributes: Map::from_iter([("deck::deck".into(), attr)]),
                ..Node::default()
            },
        ),
    }
    Ok((layer, report))
}

/// A child's box on the slide, given its group's box and child space (DeckCraft's ungroup).
fn child_to_parent(group: &deckcraft_geom::Xfrm, ch: &deckcraft_geom::Xfrm, c: &deckcraft_geom::Xfrm) -> deckcraft_geom::Xfrm {
    use deckcraft_geom::{Point, Rect};
    let sx = if ch.w.abs() > 1e-9 { group.w / ch.w } else { 1.0 };
    let sy = if ch.h.abs() > 1e-9 { group.h / ch.h } else { 1.0 };
    let a = deckcraft_geom::group_child_affine(group, Rect::new(ch.x, ch.y, ch.x + ch.w, ch.y + ch.h));
    let center = a * Point::new(c.x + c.w / 2.0, c.y + c.h / 2.0);
    let (w, h) = (c.w * sx.abs(), c.h * sy.abs());
    let mut rot = c.rot + group.rot;
    let (mut fh, mut fv) = (c.flip_h, c.flip_v);
    if group.flip_h {
        fh = !fh;
        rot = -c.rot + group.rot;
    }
    if group.flip_v {
        fv = !fv;
        rot = -rot + 2.0 * group.rot;
    }
    deckcraft_geom::Xfrm { x: center.x - w / 2.0, y: center.y - h / 2.0, w, h, rot: ((rot % 360.0) + 360.0) % 360.0, flip_h: fh, flip_v: fv }
}

/// Shapes with every group taken apart, each child where its group put it on the slide.
fn flatten_groups(shapes: Vec<Shape>, depth: usize) -> Vec<Shape> {
    let mut out = Vec::with_capacity(shapes.len());
    for sh in shapes {
        match (&sh.kind, sh.xfrm) {
            (ShapeKind::Group { children, child }, Some(gx)) if depth < 64 => {
                let kids = children
                    .iter()
                    .cloned()
                    .map(|mut c| {
                        if let Some(cx) = c.xfrm {
                            c.xfrm = Some(child_to_parent(&gx, child, &cx));
                        }
                        c
                    })
                    .collect();
                out.extend(flatten_groups(kids, depth + 1));
            }
            _ => out.push(sh),
        }
    }
    out
}

/// The model has no groups: a group's shapes come in on their own, where the group showed them
/// (the grouping itself is a listed loss, "groups").
fn ungrouped(p: &Presentation) -> std::borrow::Cow<'_, Presentation> {
    if !p.slides.iter().any(|s| s.shapes.iter().any(Shape::is_group)) {
        return std::borrow::Cow::Borrowed(p);
    }
    let mut out = p.clone();
    for s in &mut out.slides {
        if s.shapes.iter().any(Shape::is_group) {
            let s = std::sync::Arc::make_mut(s);
            s.shapes = flatten_groups(std::mem::take(&mut s.shapes), 0);
        }
    }
    std::borrow::Cow::Owned(out)
}

fn import_into(
    deck: &Composed,
    edited: &Presentation,
    header: Header,
    adopting: bool,
    assets: Option<&std::path::Path>,
) -> Result<(Layer, ImportReport), DeckError> {
    let edited = &*ungrouped(edited);
    let mut report = ImportReport::default();
    for b in ox_core::unreadable_pictures(deck, assets) {
        report.warnings.push(format!("picture `{b}` can't be read, so it isn't compared: the stack's picture stays"));
    }
    if !adopting && keyword(edited, KEY_REVISION).as_deref() != Some(revision_of(deck).as_str()) {
        report.stale = true;
        report.warnings.push("the deck changed in the stack since this file was exported; review the edit layer before stacking it".into());
    }

    // Where every composed frame lives.
    let slide_by_path: HashMap<&str, &Composed> =
        deck.children.iter().map(|s| (s.path.as_str(), s)).collect();
    let mut frame_home: HashMap<&str, (&Composed, &Composed)> = HashMap::new();
    for s in &deck.children {
        for f in &s.children {
            frame_home.insert(f.path.as_str(), (s, f));
        }
    }

    // What Milleleaf's own export of each text frame reads back as, compared the same way: a
    // file whose text differs from the stack only in how an editor wrote it (LibreOffice moves
    // formatting into list styles and spells out what the layout gives) isn't an edit.
    let ours_p: Option<Presentation> = if adopting {
        None
    } else {
        crate::export::export_with(deck, ox_core::PictureSource::StandIn(assets))
            .ok()
            .and_then(|(p, _)| crate::to_pptx(&p).ok())
            .and_then(|b| crate::from_pptx(&b).ok())
    };
    let ours: HashMap<String, Vec<Value>> = ours_p
        .iter()
        .flat_map(|p| p.slides.iter().flat_map(|s| s.shapes.iter()))
        .filter_map(|sh| {
            let paras = sh.text.as_ref().map(|t| read_paragraphs(t, None)).unwrap_or_default();
            Some((ox_name(sh)?.to_string(), paras.iter().map(canonical_para).collect()))
        })
        .collect();
    // Lines and connectors the same way (their ends by the frames' names).
    let mut ours_lines: HashMap<String, Value> = HashMap::new();
    let mut ours_tables: HashMap<String, Value> = HashMap::new();
    if let Some(p) = &ours_p {
        for slide in &p.slides {
            let Some(ctx) = resolve::Ctx::for_slide(p, slide) else { continue };
            let ids: HashMap<ShapeId, String> = slide.shapes.iter().filter_map(|sh| Some((sh.id, ox_name(sh)?.to_string()))).collect();
            for sh in slide.shapes.iter().filter(|sh| is_line(sh)) {
                if let Some(n) = ox_name(sh) {
                    ours_lines.insert(n.to_string(), line_of(&ctx, sh, &ids));
                }
            }
            for sh in &slide.shapes {
                if let (Some(n), Some(t)) = (ox_name(sh), crate::table::table_of(&ctx, sh, &mut Vec::new(), 0)) {
                    ours_tables.insert(n.to_string(), t);
                }
            }
        }
    }
    let mut nodes: Vec<Node> = Vec::new();
    let mut claimed_slides: BTreeSet<String> = BTreeSet::new();
    let mut claimed_frames: BTreeSet<String> = BTreeSet::new();
    let mut deck_order: Vec<String> = Vec::new();
    let mut deck_children: BTreeMap<String, Option<String>> = BTreeMap::new();
    let mut used_slide_names: BTreeSet<String> = deck
        .children
        .iter()
        .filter_map(|s| s.name.clone())
        .collect();

    // Names a copy carries over from its original (a duplicated slide or pasted shape).
    let dup_slides = ox_core::matching::duplicates(
        edited.slides.iter().filter_map(|s| s.name.strip_prefix(NAME_PREFIX)),
    );
    let dup_shapes = ox_core::matching::duplicates(
        edited.slides.iter().flat_map(|s| s.shapes.iter().filter_map(ox_name)),
    );
    for (name, n) in dup_slides.iter().chain(dup_shapes.iter()) {
        report.warnings.push(format!(
            "{name} appears {n} times (a duplicated slide or pasted shape?): the first is taken as the original, the rest as new; all are best guesses"
        ));
    }

    for (si, slide) in edited.slides.iter().enumerate() {
        for sh in &slide.shapes {
            link_warnings(sh, si, &mut report.warnings);
        }
        let slide_ctx = resolve::Ctx::for_slide(edited, slide);
        // Which frame each shape on the slide is, for connector ends; new frames join as they're made.
        let mut ids: HashMap<ShapeId, String> = slide
            .shapes
            .iter()
            .filter_map(|sh| ox_name(sh).filter(|p| frame_home.contains_key(p)).map(|p| (sh.id, p.to_string())))
            .collect();
        let slide_nodes = nodes.len();
        // Which composed slide is this? Its name, else the slide most of its frames came from.
        let by_name = slide
            .name
            .strip_prefix(NAME_PREFIX)
            .filter(|p| slide_by_path.contains_key(p))
            .map(String::from);
        let (by_frames, votes_for, named) = {
            let mut votes: BTreeMap<&str, usize> = BTreeMap::new();
            let mut named = 0;
            for sh in &slide.shapes {
                if let Some((s, _)) = ox_name(sh).and_then(|p| frame_home.get(p)) {
                    *votes.entry(s.path.as_str()).or_default() += 1;
                    named += 1;
                }
            }
            match votes.into_iter().max_by_key(|(_, n)| *n) {
                Some((p, n)) => (Some(p.to_string()), n, named),
                None => (None, 0, named),
            }
        };
        let dup_name = by_name.as_deref().and_then(|p| dup_slides.get(p)).copied();
        let certain = by_name.is_some() && dup_name.is_none();
        let matched = by_name
            .or(by_frames)
            .filter(|p| !claimed_slides.contains(p));
        if let Some(p) = &matched {
            report.matches.push(if certain {
                ox_core::Match::certain(p, "slide name")
            } else {
                match dup_name {
                    Some(n) => ox_core::Match::best_guess(
                        p,
                        format!("slide name appears {n} times; the first slide is taken as the original"),
                    ),
                    // The editor dropped the slide's name (Impress does); the frames on it say
                    // which slide it was.
                    None => ox_core::Match::best_guess(
                        p,
                        format!(
                            "slide {}: {votes_for} of its {named} named frame(s) came from this slide",
                            si + 1
                        ),
                    ),
                }
            });
        }
        let (slide_path, composed_slide) = match &matched {
            Some(p) => {
                claimed_slides.insert(p.clone());
                (p.clone(), slide_by_path.get(p.as_str()).copied())
            }
            None => {
                let p = stable_id(&format!("{}|pptx-slide|{si}", deck.path));
                (p, None)
            }
        };
        let slide_name = match composed_slide {
            Some(s) => s.name.clone().unwrap_or_default(),
            None => {
                let mut n = format!("slide-{}", &slide_path[..8]);
                let mut i = 2;
                while used_slide_names.contains(&n) {
                    n = format!("slide-{}-{i}", &slide_path[..8]);
                    i += 1;
                }
                used_slide_names.insert(n.clone());
                deck_children.insert(n.clone(), Some(slide_path.clone()));
                report.added.push(slide_path.clone());
                n
            }
        };
        deck_order.push(slide_name.clone());

        let mut slide_children: BTreeMap<String, Option<String>> = BTreeMap::new();
        let mut slide_order: Vec<String> = Vec::new();
        let mut used_frame_names: BTreeSet<String> = composed_slide
            .map(|s| s.children.iter().filter_map(|c| c.name.clone()).collect())
            .unwrap_or_default();

        for (zi, sh) in slide.shapes.iter().enumerate() {
            let existing = ox_name(sh)
                .filter(|p| !claimed_frames.contains(*p))
                .and_then(|p| frame_home.get(p))
                .copied();
            match existing {
                Some((home, fr)) => {
                    claimed_frames.insert(fr.path.clone());
                    report.matches.push(match dup_shapes.get(fr.path.as_str()) {
                        Some(n) => ox_core::Match::best_guess(
                            &fr.path,
                            format!("shape name appears {n} times; the first is taken as the original"),
                        ),
                        None => ox_core::Match::certain(&fr.path, "shape name"),
                    });
                    let name = fr.name.clone().unwrap_or_default();
                    if home.path != slide_path {
                        // Moved to another slide.
                        let mut n = name.clone();
                        let mut i = 2;
                        while used_frame_names.contains(&n) {
                            n = format!("{name}-{i}");
                            i += 1;
                        }
                        used_frame_names.insert(n.clone());
                        slide_children.insert(n.clone(), Some(fr.path.clone()));
                        slide_order.push(n);
                        report.reordered.push(fr.path.clone());
                    } else {
                        slide_order.push(name);
                    }
                    let mut attrs = Map::new();
                    // Geometry.
                    let old_g = obj(fr, "deck::frame").cloned().unwrap_or_default();
                    // A table's size is its columns' and rows' (editors recompute the frame from them).
                    let sized = |mut g: Map<String, Value>| {
                        if matches!(sh.kind, ShapeKind::Table(_)) {
                            g.remove("w");
                            g.remove("h");
                        }
                        g
                    };
                    if let Some(new_g) = geometry(sh)
                        && geometry_changed(&sized(old_g.clone()), &sized(new_g.clone()))
                    {
                        let mut g = new_g;
                        g.insert("z".into(), json!(zi));
                        attrs.insert("deck::frame".into(), Value::Object(g));
                        report.moved.push(fr.path.clone());
                    }
                    // Content.
                    if let Some(c) = obj(fr, "deck::chart") {
                        if let ShapeKind::Chart(chart) = &sh.kind {
                            let (cats, series) = chart_data(fr, c);
                            let now: Vec<Vec<f64>> = chart
                                .series
                                .iter()
                                .map(|s| s.values.iter().map(|v| v.unwrap_or(0.0)).collect())
                                .collect();
                            let want: Vec<Vec<f64>> =
                                series.iter().map(|(_, v)| v.clone()).collect();
                            if chart.categories != cats || now != want {
                                report.conflicts.push(Conflict { path: fr.path.clone(), message: "linked chart data was edited in the slide; the source wins — edit the workbook".into() });
                            }
                        }
                    } else if obj(fr, "office::data::table").is_some() {
                        if let Some(grid) = table_grid(sh)
                            && grid != expected_grid(fr)
                        {
                            report.conflicts.push(Conflict { path: fr.path.clone(), message: "linked table was edited in the slide; the source wins — edit the workbook".into() });
                        }
                    } else if let Some(dt) = obj(fr, "deck::table") {
                        if let Some(new) = slide_ctx.as_ref().and_then(|ctx| crate::table::table_of(ctx, sh, &mut report.warnings, si)) {
                            let was = ours_tables.get(&fr.path).cloned().unwrap_or_else(|| Value::Object(dt.clone()));
                            if !crate::table::same_table(&new, &was) {
                                // Keeps `style` (the HTML view's font).
                                let mut t = dt.clone();
                                t.extend(new.as_object().cloned().unwrap_or_default());
                                attrs.insert("deck::table".into(), Value::Object(t));
                                report.changed.push(fr.path.clone());
                            }
                        }
                    } else if let Some(t) = obj(fr, "deck::text") {
                        let old_paras: Vec<Map<String, Value>> = t
                            .get("paragraphs")
                            .and_then(Value::as_array)
                            .map(|a| a.iter().filter_map(|p| p.as_object().cloned()).collect())
                            .unwrap_or_default();
                        // An adopted frame carries each paragraph's effective formatting (indents,
                        // spacing, bullets as the layout gave them): read the file the same way, or
                        // an edit would drop them.
                        let effective = old_paras.iter().any(|p| {
                            ["marginLeftPt", "indentPt", "spaceBefore", "spaceAfter", "bulletChar", "autoNum"].iter().any(|k| p.contains_key(*k))
                        });
                        let ctx = effective.then(|| resolve::Ctx::for_slide(edited, slide)).flatten();
                        let mut new_paras = sh
                            .text
                            .as_ref()
                            .map(|t| read_paragraphs(t, ctx.as_ref().map(|c| (c, sh))))
                            .unwrap_or_default();
                        // Put linked fields back where their values still stand.
                        let mut lost = Vec::new();
                        for (i, op) in old_paras.iter().enumerate() {
                            for r in op
                                .get("runs")
                                .and_then(Value::as_array)
                                .into_iter()
                                .flatten()
                            {
                                let Some(name) = r.get("field").and_then(Value::as_str) else {
                                    continue;
                                };
                                let shown = field_text(fr, name);
                                // How many times the value stands in the paragraph now: once
                                // is unambiguous; more, and the first is a guess.
                                let times = new_paras
                                    .get(i)
                                    .map(|np| occurrences(np, &shown))
                                    .unwrap_or(0);
                                let ok = new_paras
                                    .get_mut(i)
                                    .is_some_and(|np| reanchor(np, name, &shown));
                                if !ok {
                                    lost.push(name.to_string());
                                } else if times > 1 || old_paras.len() != new_paras.len() {
                                    let why = if times > 1 {
                                        format!(
                                            "live field `{name}` found by its value \"{shown}\", the first of {times} in paragraph {}",
                                            i + 1
                                        )
                                    } else {
                                        format!(
                                            "live field `{name}` found by its value \"{shown}\" in paragraph {}, though paragraphs were added or removed",
                                            i + 1
                                        )
                                    };
                                    report
                                        .matches
                                        .push(ox_core::Match::best_guess(&fr.path, why));
                                } else {
                                    report.matches.push(ox_core::Match::certain(
                                        &fr.path,
                                        format!("live field `{name}` found by its value, shown once in paragraph {}", i + 1),
                                    ));
                                }
                            }
                        }
                        let a: Vec<Value> = old_paras.iter().map(canonical_para).collect();
                        let mut b: Vec<Value> = new_paras.iter().map(canonical_para).collect();
                        if let Some(ctx) = &slide_ctx {
                            let c = ctx.color(&ColorRef::scheme(deckcraft_color::SchemeSlot::Hlink), None);
                            editor_link_colour(&mut b, ours.get(&fr.path).unwrap_or(&a), &format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b));
                        }
                        if a != b && ours.get(&fr.path) != Some(&b) {
                            if lost.is_empty() {
                                let mut t2 = t.clone();
                                // Keep the original formatting on field runs.
                                for (i, np) in new_paras.iter_mut().enumerate() {
                                    if let Some(runs) =
                                        np.get_mut("runs").and_then(Value::as_array_mut)
                                    {
                                        for r in runs.iter_mut() {
                                            if let Some(f) = r
                                                .get("field")
                                                .and_then(Value::as_str)
                                                .map(String::from)
                                                && let Some(orig) = old_paras
                                                    .get(i)
                                                    .and_then(|p| p.get("runs"))
                                                    .and_then(Value::as_array)
                                                    .and_then(|rs| {
                                                        rs.iter().find(|o| {
                                                            o.get("field").and_then(Value::as_str)
                                                                == Some(f.as_str())
                                                        })
                                                    })
                                            {
                                                *r = orig.clone();
                                            }
                                        }
                                    }
                                }
                                t2.insert(
                                    "paragraphs".into(),
                                    Value::Array(
                                        new_paras.into_iter().map(Value::Object).collect(),
                                    ),
                                );
                                attrs.insert("deck::text".into(), Value::Object(t2));
                                report.changed.push(fr.path.clone());
                            } else {
                                report.conflicts.push(Conflict {
                                    path: fr.path.clone(),
                                    message: format!("linked field(s) {} were edited or removed in the slide; the text edit was not applied", lost.join(", ")),
                                });
                            }
                        }
                    } else if let Some(old) = obj(fr, "deck::picture") {
                        // A picture still named by a path (a stack file's) is compared by the hash
                        // of that file; one that can't be read stays as it is (warned above).
                        let old_blob = old.get("blob").and_then(Value::as_str).unwrap_or("");
                        let old_hash = ox_core::resolve_blob(assets, old_blob);
                        if let (Some(mut new), Some(old_hash)) = (picture(edited, sh, si, &mut report), old_hash) {
                            if new["blob"] == json!(old_hash) {
                                // The same picture: keep how the stack names it.
                                new.insert("blob".into(), old["blob"].clone());
                                match old.get("mediaType") {
                                    Some(t) => new.insert("mediaType".into(), t.clone()),
                                    None => new.remove("mediaType"),
                                };
                            }
                            // LibreOffice rounds a crop a little (0.1724 comes back 0.1723).
                            let crop = |m: &Map<String, Value>| {
                                let c: Vec<f64> = m.get("crop").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_f64).collect();
                                if c.is_empty() { vec![0.0; 4] } else { c }
                            };
                            let crop_moved = crop(old).iter().zip(crop(&new)).any(|(a, b)| (a - b).abs() > 1e-3);
                            if crop_moved || ["blob", "alt"].iter().any(|k| old.get(*k) != new.get(*k)) {
                                attrs.insert("deck::picture".into(), Value::Object(new));
                                report.changed.push(fr.path.clone());
                            }
                        }
                    } else if let Some(old) = obj(fr, "deck::line") {
                        if let (true, Some(ctx)) = (is_line(sh), &slide_ctx) {
                            let new = line_of(ctx, sh, &ids);
                            let was = ours_lines.get(&fr.path).cloned().unwrap_or_else(|| Value::Object(old.clone()));
                            // Lengths within 0.1 pt: LibreOffice writes widths in its own units.
                            if !near(&canonical_line(&new), &canonical_line(&was), 0.1) {
                                attrs.insert("deck::line".into(), new);
                                report.changed.push(fr.path.clone());
                            }
                        }
                    } else if let Some(shape) = obj(fr, "deck::shape") {
                        let fill = fill_hex(&sh.fill);
                        if fill.is_some()
                            && fill.as_deref()
                                != shape
                                    .get("fill")
                                    .and_then(Value::as_str)
                                    .map(str::to_lowercase)
                                    .as_deref()
                        {
                            let mut s2 = shape.clone();
                            s2.insert("fill".into(), json!(fill));
                            attrs.insert("deck::shape".into(), Value::Object(s2));
                            report.changed.push(fr.path.clone());
                        }
                    }
                    if !attrs.is_empty() {
                        nodes.push(Node {
                            path: fr.path.clone(),
                            attributes: attrs,
                            ..Node::default()
                        });
                    }
                }
                None => {
                    // New on this slide: text boxes are carried; other new objects are reported.
                    if let (Some(ctx), Some(mut g)) = (&slide_ctx, geometry(sh))
                        && let Some(t) = crate::table::table_of(ctx, sh, &mut report.warnings, si)
                    {
                        g.insert("z".into(), json!(zi));
                        let path = stable_id(&format!("{slide_path}|pptx-table|{zi}|{}", sh.name));
                        let mut n = format!("table-{}", &path[..8]);
                        let mut i = 2;
                        while used_frame_names.contains(&n) {
                            n = format!("table-{}-{i}", &path[..8]);
                            i += 1;
                        }
                        used_frame_names.insert(n.clone());
                        slide_children.insert(n.clone(), Some(path.clone()));
                        slide_order.push(n);
                        let attributes = Map::from_iter([("deck::frame".to_string(), Value::Object(g)), ("deck::table".to_string(), t)]);
                        ids.insert(sh.id, path.clone());
                        nodes.push(Node { path: path.clone(), attributes, ..Node::default() });
                        report.added.push(path);
                        continue;
                    }
                    if let (true, Some(ctx), Some(mut g)) = (is_line(sh), &slide_ctx, geometry(sh)) {
                        g.insert("z".into(), json!(zi));
                        let path = stable_id(&format!("{slide_path}|pptx-line|{zi}|{}", sh.name));
                        let mut n = format!("line-{}", &path[..8]);
                        let mut i = 2;
                        while used_frame_names.contains(&n) {
                            n = format!("line-{}-{i}", &path[..8]);
                            i += 1;
                        }
                        used_frame_names.insert(n.clone());
                        slide_children.insert(n.clone(), Some(path.clone()));
                        slide_order.push(n);
                        let attributes = Map::from_iter([
                            ("deck::frame".to_string(), Value::Object(g)),
                            ("deck::line".to_string(), line_of(ctx, sh, &ids)),
                        ]);
                        ids.insert(sh.id, path.clone());
                        nodes.push(Node { path: path.clone(), attributes, ..Node::default() });
                        report.added.push(path);
                        continue;
                    }
                    let Some(text) = sh.text.as_ref().filter(|t| !t.text().trim().is_empty())
                    else {
                        if let Some(pic) = picture(edited, sh, si, &mut report) {
                            // A picture placeholder may take its position from the layout.
                            let mut placed = sh.clone();
                            if placed.xfrm.is_none() {
                                placed.xfrm = inherited_xfrm(edited, slide, sh);
                            }
                            if let Some(mut g) = geometry(&placed) {
                                g.insert("z".into(), json!(zi));
                                let blob = pic["blob"].as_str().unwrap_or_default();
                                let path = stable_id(&format!("{slide_path}|pptx-picture|{zi}|{blob}"));
                                let mut n = format!("picture-{}", &path[..8]);
                                let mut i = 2;
                                while used_frame_names.contains(&n) {
                                    n = format!("picture-{}-{i}", &path[..8]);
                                    i += 1;
                                }
                                used_frame_names.insert(n.clone());
                                slide_children.insert(n.clone(), Some(path.clone()));
                                slide_order.push(n);
                                let attributes = Map::from_iter([
                                    ("deck::frame".to_string(), Value::Object(g)),
                                    ("deck::picture".to_string(), Value::Object(pic)),
                                ]);
                                ids.insert(sh.id, path.clone());
                                nodes.push(Node { path: path.clone(), attributes, ..Node::default() });
                                report.added.push(path);
                                continue;
                            }
                        }
                        // Shapes without text too: an empty placeholder, but also a filled box.
                        report.warnings.push(format!(
                            "slide {}: new `{}` ({}{}) isn't carried yet",
                            si + 1,
                            sh.name,
                            kind_name(&sh.kind),
                            if matches!(sh.kind, ShapeKind::Shape) { ", no text" } else { "" }
                        ));
                        continue;
                    };
                    let path = stable_id(&format!("{slide_path}|pptx-frame|{zi}|{}", text.text()));
                    // A placeholder with no position of its own (most titles) sits where its
                    // layout's (or master's) placeholder sits.
                    let placed;
                    let sh = match sh.xfrm {
                        Some(_) => sh,
                        None => {
                            let mut c = sh.clone();
                            c.xfrm = inherited_xfrm(edited, slide, sh);
                            placed = c;
                            &placed
                        }
                    };
                    let Some(mut g) = geometry(sh) else {
                        report.warnings.push(format!(
                            "slide {}: text `{}` has no position, here or on its layout; not carried",
                            si + 1,
                            text.text().chars().take(40).collect::<String>()
                        ));
                        continue;
                    };
                    g.insert("z".into(), json!(zi));
                    let ctx = resolve::Ctx::for_slide(edited, slide);
                    let paragraphs: Vec<Value> = read_paragraphs(text, ctx.as_ref().map(|c| (c, sh)))
                        .into_iter()
                        .map(Value::Object)
                        .collect();
                    let mut n = format!("text-{}", &path[..8]);
                    let mut i = 2;
                    while used_frame_names.contains(&n) {
                        n = format!("text-{}-{i}", &path[..8]);
                        i += 1;
                    }
                    used_frame_names.insert(n.clone());
                    slide_children.insert(n.clone(), Some(path.clone()));
                    slide_order.push(n);
                    let mut dt = json!({ "role": "body", "paragraphs": paragraphs });
                    let mut attributes = Map::from_iter([("deck::frame".to_string(), Value::Object(g))]);
                    if let Some(ctx) = &ctx {
                        // The box's effective look: its fill, outline and shape, and where the
                        // text sits in it.
                        if let Some(look) = shape_look(ctx, sh, &mut report.warnings, si) {
                            attributes.insert("deck::shape".into(), look);
                        }
                        let body = resolve::body(ctx, sh);
                        // Inner margins (PowerPoint's defaults when unset), in OX pixels.
                        dt["insets"] = json!({
                            "left": px(body.inset_l.unwrap_or(7.2)),
                            "top": px(body.inset_t.unwrap_or(3.6)),
                            "right": px(body.inset_r.unwrap_or(7.2)),
                            "bottom": px(body.inset_b.unwrap_or(3.6)),
                        });
                        let anchor = match body.anchor {
                            Some(Anchor::Middle) => Some("middle"),
                            Some(Anchor::Bottom) => Some("bottom"),
                            _ => None,
                        };
                        if let Some(a) = anchor {
                            dt["verticalAlign"] = json!(a);
                        }
                    }
                    attributes.insert("deck::text".into(), dt);
                    ids.insert(sh.id, path.clone());
                    nodes.push(Node { path: path.clone(), attributes, ..Node::default() });
                    report.added.push(path);
                }
            }
        }

        attach_ends(&mut nodes[slide_nodes..], &ids, si, &mut report.warnings);

        // Frames gone from this slide.
        if let Some(cs) = composed_slide {
            for f in &cs.children {
                if !claimed_frames.contains(&f.path)
                    && !edited.slides.iter().any(|s| {
                        s.shapes
                            .iter()
                            .any(|sh| ox_name(sh) == Some(f.path.as_str()))
                    })
                {
                    let name = f.name.clone().unwrap_or_default();
                    slide_children.insert(name.clone(), None);
                    report.removed.push(name);
                }
            }
        }
        // Frames that moved here from elsewhere leave their old slide.
        for p in report.reordered.clone() {
            if let Some((home, fr)) = frame_home.get(p.as_str())
                && home.path != slide_path
                && slide_children
                    .values()
                    .any(|v| v.as_deref() == Some(p.as_str()))
            {
                let name = fr.name.clone().unwrap_or_default();
                nodes.push(Node {
                    path: home.path.clone(),
                    children: BTreeMap::from([(name, None)]),
                    ..Node::default()
                });
            }
        }

        let mut slide_node = Node {
            path: slide_path.clone(),
            ..Node::default()
        };
        let old_order: Vec<String> = composed_slide
            .map(|s| s.children.iter().filter_map(|c| c.name.clone()).collect())
            .unwrap_or_default();
        if !slide_children.is_empty() || slide_order != old_order {
            if composed_slide.is_some()
                && same_relative_order(&old_order, &slide_order) == Some(false)
            {
                report.reordered.push(slide_path.clone());
            }
            slide_node.children = slide_children;
            slide_node.order = Some(slide_order);
        }
        // Slide attributes: notes (and the name for a new slide).
        let old_sa = composed_slide
            .and_then(|s| obj(s, "deck::slide"))
            .cloned()
            .unwrap_or_default();
        let notes = slide.notes.text().trim().to_string();
        let old_notes = old_sa
            .get("notes")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if composed_slide.is_none() || notes != old_notes {
            let mut sa = old_sa.clone();
            sa.insert("name".into(), json!(slide_name));
            if notes.is_empty() {
                sa.remove("notes");
            } else {
                sa.insert("notes".into(), json!(notes));
            }
            slide_node
                .attributes
                .insert("deck::slide".into(), Value::Object(sa));
            if composed_slide.is_some() {
                report.changed.push(slide_path.clone());
            }
        }
        if !slide_node.children.is_empty()
            || slide_node.order.is_some()
            || !slide_node.attributes.is_empty()
        {
            nodes.push(slide_node);
        }
    }

    // Slides gone, and the deck's order.
    for s in &deck.children {
        if !claimed_slides.contains(&s.path) {
            let name = s.name.clone().unwrap_or_default();
            deck_children.insert(name.clone(), None);
            report.removed.push(name);
        }
    }
    let old_deck_order: Vec<String> = deck
        .children
        .iter()
        .filter_map(|s| s.name.clone())
        .collect();
    if !deck_children.is_empty() || deck_order != old_deck_order {
        if same_relative_order(&old_deck_order, &deck_order) == Some(false) {
            report.reordered.push(deck.path.clone());
        }
        nodes.insert(
            0,
            Node {
                path: deck.path.clone(),
                children: deck_children,
                order: Some(deck_order),
                ..Node::default()
            },
        );
    }

    let mut merged: Vec<Node> = Vec::new();
    for n in nodes {
        match merged.iter_mut().find(|m| m.path == n.path) {
            Some(m) => {
                m.children.extend(n.children);
                m.attributes.extend(n.attributes);
                if n.order.is_some() {
                    m.order = n.order;
                }
            }
            None => merged.push(n),
        }
    }
    let header = Header {
        ox_version: OX_VERSION.into(),
        kind: Some(header.kind.unwrap_or_else(|| "edit".into())),
        ..header
    };
    let mut layer = Layer {
        header,
        imports: vec![],
        schemas: BTreeMap::new(),
        data: merged,
    };
    ox_core::matching::flag_best_guesses(&mut layer, &report.matches);
    Ok((layer, report))
}

fn kind_name(k: &ShapeKind) -> &'static str {
    match k {
        ShapeKind::Shape => "shape",
        ShapeKind::Picture { .. } => "picture",
        ShapeKind::Group { .. } => "group",
        ShapeKind::Connector { .. } => "connector",
        ShapeKind::Table(_) => "table",
        ShapeKind::Chart(_) => "chart",
        ShapeKind::Media(_) => "media",
        ShapeKind::Ink { .. } => "ink",
        ShapeKind::Model3d { .. } => "3D model",
        ShapeKind::Opaque { .. } => "object",
    }
}

/// Whether the names both lists share appear in the same order (`None` if they share none):
/// additions and deletions alone aren't a reorder.
fn same_relative_order(old: &[String], new: &[String]) -> Option<bool> {
    let a: Vec<&String> = old.iter().filter(|n| new.contains(n)).collect();
    let b: Vec<&String> = new.iter().filter(|n| old.contains(n)).collect();
    (!a.is_empty()).then_some(a == b)
}
