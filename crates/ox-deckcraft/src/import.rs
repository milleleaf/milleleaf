//! Edited DeckCraft `Presentation` → minimal OX edit layer.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use deckcraft_model::style::{ColorBase, Fill};
use deckcraft_model::text::{Bullet, RunKind};
use deckcraft_model::{Presentation, RunProps, Shape, ShapeKind, TextBody};
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
    m
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
fn canonical_para(p: &Map<String, Value>) -> Value {
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
    json!({
        "runs": merge_runs(runs),
        "bullet": p.get("bullet").and_then(Value::as_bool).unwrap_or(false),
        "level": p.get("level").and_then(Value::as_u64).unwrap_or(0),
        "align": p.get("align").and_then(Value::as_str).unwrap_or("left"),
    })
}

/// Paragraphs of an edited text body, in OX form (no fields yet).
fn read_paragraphs(t: &TextBody) -> Vec<Map<String, Value>> {
    t.paragraphs
        .iter()
        .map(|p| {
            let mut runs = Vec::new();
            for r in &p.runs {
                let mut m = run_format(&r.props);
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
            let align = match p.props.align {
                Some(deckcraft_model::text::Align::Center) => "center",
                Some(deckcraft_model::text::Align::Right) => "right",
                Some(deckcraft_model::text::Align::Justify) => "justify",
                _ => "left",
            };
            para.insert("align".into(), json!(align));
            if matches!(
                p.props.bullet,
                Some(Bullet::Char { .. } | Bullet::AutoNum { .. })
            ) {
                para.insert("bullet".into(), json!(true));
                para.insert("level".into(), json!(p.level));
            }
            para
        })
        .collect()
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
    Some(m)
}

fn geometry_changed(old: &Map<String, Value>, new: &Map<String, Value>) -> bool {
    ["x", "y", "w", "h", "rotation"].iter().any(|k| {
        let a = old.get(*k).and_then(Value::as_f64).unwrap_or(0.0);
        let b = new.get(*k).and_then(Value::as_f64).unwrap_or(0.0);
        (a - b).abs() > 0.6
    })
}

/// Compares the edited presentation with the composed deck `node` it was exported from and
/// returns the edit layer.
pub fn import(
    deck: &Composed,
    edited: &Presentation,
    header: Header,
) -> Result<(Layer, ImportReport), DeckError> {
    let found = keyword(edited, KEY_DECK).ok_or(DeckError::NotExported)?;
    if found != deck.path {
        return Err(DeckError::WrongDeck {
            expected: deck.path.clone(),
            found,
        });
    }
    import_into(deck, edited, header, false)
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
    let attr = json!({
        "width": px(edited.slide_size.width),
        "height": px(edited.slide_size.height),
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
    let (mut layer, report) = import_into(&empty, edited, header, true)?;
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

fn import_into(
    deck: &Composed,
    edited: &Presentation,
    header: Header,
    adopting: bool,
) -> Result<(Layer, ImportReport), DeckError> {
    let mut report = ImportReport::default();
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
                    if let Some(new_g) = geometry(sh)
                        && geometry_changed(&old_g, &new_g)
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
                        if let Some(grid) = table_grid(sh) {
                            let snap: Vec<Vec<String>> = dt
                                .get("snapshot")
                                .and_then(Value::as_array)
                                .map(|rows| {
                                    rows.iter()
                                        .map(|r| {
                                            r.as_array()
                                                .into_iter()
                                                .flatten()
                                                .map(|c| {
                                                    c.get("text")
                                                        .and_then(Value::as_str)
                                                        .unwrap_or("")
                                                        .to_string()
                                                })
                                                .collect()
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();
                            if grid != snap {
                                let mut t = dt.clone();
                                t.insert(
                                    "snapshot".into(),
                                    json!(
                                        grid.iter()
                                            .map(|r| r
                                                .iter()
                                                .map(|c| json!({ "text": c }))
                                                .collect::<Vec<_>>())
                                            .collect::<Vec<_>>()
                                    ),
                                );
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
                        let mut new_paras =
                            sh.text.as_ref().map(read_paragraphs).unwrap_or_default();
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
                        let b: Vec<Value> = new_paras.iter().map(canonical_para).collect();
                        if a != b {
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
                    let Some(text) = sh.text.as_ref().filter(|t| !t.text().trim().is_empty())
                    else {
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
                    let paragraphs: Vec<Value> = read_paragraphs(text)
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
                    nodes.push(Node {
                        path: path.clone(),
                        attributes: Map::from_iter([
                            ("deck::frame".to_string(), Value::Object(g)),
                            (
                                "deck::text".to_string(),
                                json!({ "role": "body", "paragraphs": paragraphs }),
                            ),
                        ]),
                        ..Node::default()
                    });
                    report.added.push(path);
                }
            }
        }

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
