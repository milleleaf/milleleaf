//! Edited WordCraft `Document` → minimal Milleleaf edit layer.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use milleleaf_core::Composed;
use milleleaf_core::model::{Header, Layer, Node, MLEAF_VERSION, stable_id};
use milleleaf_core::render::format_value;
use serde::Serialize;
use serde_json::{Map, Value, json};
use wordcraft_doc::para::OBJ;
use wordcraft_doc::props::{Underline, VMerge};
use wordcraft_doc::section::HeaderSet;
use wordcraft_doc::styles::{PartFormat, PartSet};
use wordcraft_doc::{Block, CharProps, Document, InlineObject, Paragraph, Table, TextColor};

use crate::{CAPTION_SUFFIX, PROP_DOCUMENT, PROP_REVISION, TAG_PREFIX, WordError, revision_of};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Conflict {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ImportReport {
    /// Existing nodes whose own attributes changed.
    pub changed: Vec<String>,
    /// New nodes (paragraphs or tables typed into the file).
    pub added: Vec<String>,
    /// Child names removed from the document.
    pub removed: Vec<String>,
    /// Edits to live-linked content, which are not written back.
    pub conflicts: Vec<Conflict>,
    pub warnings: Vec<String>,
    /// The stack changed since this file was exported.
    pub stale: bool,
    /// How each paragraph and table in the file was recognised (all by content-control tag, so
    /// all certain).
    pub matches: Vec<milleleaf_core::Match>,
    /// The bytes of every picture the file shows, by blob value (`sha256-<hex>`), with their media
    /// type: the caller stores the ones its project doesn't have yet.
    #[serde(skip)]
    pub pictures: Pictures,
}

/// Pictures by blob value: (media type, bytes).
pub type Pictures = BTreeMap<String, (String, Vec<u8>)>;

/// An inline picture as a run, `{"picture": {blob, mediaType, w, h, alt?, crop?, float?}}`
/// (points; `float` is WordCraft's placement, when it isn't inline), its bytes kept in `pics`.
/// `None` for an OLE object's picture (the object itself isn't carried) or missing media.
fn picture_run(doc_media: &BTreeMap<String, std::sync::Arc<Vec<u8>>>, o: &InlineObject, pics: &mut Pictures) -> Option<Value> {
    let InlineObject::Image { media, w, h, alt, float, crop, ole } = o else { return None };
    let bytes = doc_media.get(media).filter(|_| ole.is_none())?;
    let blob = milleleaf_core::blob_of(bytes);
    let media_type = milleleaf_core::media_type_for(media).to_string();
    let r2 = |x: f32| ((x as f64) * 100.0).round() / 100.0;
    let mut pic = Map::from_iter([
        ("blob".to_string(), json!(blob)),
        ("mediaType".to_string(), json!(media_type)),
        ("w".to_string(), json!(r2(*w))),
        ("h".to_string(), json!(r2(*h))),
    ]);
    if !alt.is_empty() {
        pic.insert("alt".into(), json!(alt));
    }
    if crop.iter().any(|c| c.abs() > 1e-4) {
        pic.insert("crop".into(), json!(crop.map(|c| ((c as f64) * 1e4).round() / 1e4)));
    }
    if *float != Default::default()
        && let Ok(Value::Object(mut f)) = serde_json::to_value(float) {
            // f32 points, to 0.01 pt (401.04998779296875 is 401.05).
            for v in f.values_mut() {
                if let Some(x) = v.as_f64().filter(|_| v.is_f64()) {
                    *v = json!((x * 100.0).round() / 100.0);
                } else if let Value::Array(a) = v {
                    a.iter_mut().filter(|x| x.is_f64()).for_each(|x| *x = json!((x.as_f64().unwrap_or(0.0) * 100.0).round() / 100.0));
                }
            }
            pic.insert("float".into(), Value::Object(f));
        }
    if !bytes.starts_with(milleleaf_core::STAND_IN) {
        pics.insert(blob, (media_type, bytes.to_vec()));
    }
    Some(json!({ "picture": pic }))
}

impl ImportReport {
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.added.is_empty() && self.removed.is_empty()
    }
}

fn obj<'a>(n: &'a Composed, key: &str) -> Option<&'a Map<String, Value>> {
    n.attributes.get(key).and_then(Value::as_object)
}

/// The composed revision a file was exported at (`mleaf.revision`), if it carries one.
pub fn exported_revision(edited: &Document) -> Option<String> {
    custom_prop(edited, PROP_REVISION).map(String::from)
}

fn custom_prop<'a>(doc: &'a Document, name: &str) -> Option<&'a str> {
    doc.custom_props
        .iter()
        .find(|p| p.name == name)
        .map(|p| p.value.as_str())
}

fn hex(c: &TextColor) -> Option<String> {
    match c {
        TextColor::Rgb(rgb) => Some(format!("#{:02x}{:02x}{:02x}", rgb.0, rgb.1, rgb.2)),
        TextColor::Auto => None,
    }
}

/// Direct character formatting as run keys (the subset Milleleaf runs carry).
fn run_format(cp: &CharProps) -> Map<String, Value> {
    let mut m = Map::new();
    if cp.bold == Some(true) {
        m.insert("bold".into(), json!(true));
    }
    if cp.italic == Some(true) {
        m.insert("italic".into(), json!(true));
    }
    if cp.underline.is_some_and(|u| u != Underline::None) {
        m.insert("underline".into(), json!(true));
    }
    if let Some(c) = cp.color.as_ref().and_then(hex) {
        m.insert("color".into(), json!(c));
    }
    if let Some(f) = &cp.font {
        m.insert("font".into(), json!(f));
    }
    if let Some(s) = cp.size {
        m.insert("sizePt".into(), json!(s));
    }
    // A web or mail address, or `#bookmark` inside the document.
    if let Some(l) = &cp.link {
        m.insert("link".into(), json!(l));
    }
    m
}

/// LibreOffice (24.2) extends a link back over the text before it, to the start of the content
/// control it makes of Milleleaf's paragraph control: the file links text the stack doesn't. When
/// the text is the same and every character whose link differs is unlinked in the stack and, in
/// the file, carries the link of the next linked text in the stack, the stack's links are kept
/// (`Some`, with how many characters were LibreOffice's). Any other link change is an edit.
fn settle_spread_links(old: &[Value], new: &[Value]) -> Option<(Vec<Value>, usize)> {
    // Text runs only, one entry per character: (char, formatting).
    let chars = |runs: &[Value]| -> Option<Vec<(char, Map<String, Value>)>> {
        let mut out = Vec::new();
        for r in canonical(runs) {
            let mut fmt = r.as_object()?.clone();
            let text = fmt.remove("text")?.as_str()?.to_string();
            if fmt.keys().any(|k| matches!(k.as_str(), "field" | "fieldPath" | "docField" | "picture")) {
                return None;
            }
            out.extend(text.chars().map(|c| (c, fmt.clone())));
        }
        Some(out)
    };
    let (a, mut b) = (chars(old)?, chars(new)?);
    if a.len() != b.len() || a.iter().zip(&b).any(|(x, y)| x.0 != y.0) {
        return None;
    }
    let mut spread = 0;
    for i in 0..a.len() {
        let (la, lb) = (a[i].1.get("link"), b[i].1.get("link"));
        if la == lb {
            continue;
        }
        let next = a[i..].iter().find_map(|(_, f)| f.get("link"));
        if la.is_some() || lb.is_none() || lb != next {
            return None;
        }
        b[i].1.remove("link");
        spread += 1;
    }
    if spread == 0 {
        return None;
    }
    let runs: Vec<Value> = b
        .into_iter()
        .map(|(c, mut f)| {
            f.insert("text".into(), json!(c.to_string()));
            Value::Object(f)
        })
        .collect();
    Some((canonical(&runs), spread))
}

/// A HYPERLINK field's address (`url`, `url#anchor`, `#anchor`), `Some("")` for one with none.
fn hyperlink_field(instr: &str) -> Option<String> {
    let rest = instr.trim().strip_prefix("HYPERLINK")?;
    let mut quoted = rest.split('"').skip(1).step_by(2);
    let mut url = String::new();
    let mut anchor = None;
    let words: Vec<&str> = rest.split_whitespace().collect();
    if words.first().is_some_and(|w| w.starts_with('"')) {
        url = quoted.next().unwrap_or("").to_string();
    }
    if let Some(i) = words.iter().position(|w| *w == "\\l") {
        anchor = words.get(i + 1).map(|a| a.trim_matches('"').to_string());
    }
    Some(match anchor {
        Some(a) => format!("{url}#{a}"),
        None => url,
    })
}

/// Runs as compared on import: [`canonical`], with HYPERLINK fields read as what they are, a link
/// on their text (a target-less one: plain text), and links in `lost` left out. LibreOffice
/// (24.2) turns links into HYPERLINK fields and back, drops target-less ones, and turns a link
/// to an anchor in another file into an empty field at the paragraph's start, unlinking its text
/// (`lost`: those links, a LibreOffice loss the stack's version keeps).
fn comparable(runs: &[Value], lost: &BTreeSet<String>) -> Vec<Value> {
    let read: Vec<Value> = runs
        .iter()
        .filter_map(|r| {
            let mut m = r.as_object()?.clone();
            if let Some(link) = m.get("docField").and_then(Value::as_str).and_then(hyperlink_field) {
                m.remove("docField");
                if m.get("text").and_then(Value::as_str).unwrap_or("").is_empty() {
                    return None;
                }
                if !link.is_empty() {
                    m.insert("link".into(), json!(link));
                }
                // A field's result has one formatting; see `field_formatting`.
                m.insert("_field".into(), json!(true));
            }
            if m.get("link").and_then(Value::as_str).is_some_and(|l| lost.contains(l)) {
                m.remove("link");
            }
            Some(Value::Object(m))
        })
        .collect();
    canonical(&read)
}

/// A HYPERLINK field's text (from [`comparable`], marked `_field`) is held with one formatting,
/// its start's: where the text is the same, its characters take the stack's formatting when they
/// carry the stack's link there (LibreOffice writes a link as a field; the text looks the same).
fn field_formatting(old: &[Value], new: &[Value]) -> Vec<Value> {
    let chars = |runs: &[Value]| -> Vec<(char, Map<String, Value>)> {
        runs.iter()
            .filter_map(|r| r.as_object())
            .flat_map(|r| {
                let mut f = r.clone();
                let text = f.remove("text").and_then(|t| t.as_str().map(String::from)).unwrap_or_default();
                text.chars().map(move |c| (c, f.clone())).collect::<Vec<_>>()
            })
            .collect()
    };
    let strip = unmark;
    let (a, mut b) = (chars(old), chars(new));
    // Fields and pictures hold no characters here: only text runs can be lined up.
    let only_text = |runs: &[Value]| runs.iter().all(|r| !["field", "fieldPath", "docField", "picture"].iter().any(|k| r.get(*k).is_some()));
    if a.len() != b.len() || a.iter().zip(&b).any(|(x, y)| x.0 != y.0) || !only_text(old) || !only_text(new) {
        return strip(new);
    }
    for (x, y) in a.iter().zip(b.iter_mut()) {
        if y.1.contains_key("_field") && x.1.get("link") == y.1.get("link") {
            y.1 = x.1.clone();
        }
    }
    let runs: Vec<Value> = b
        .into_iter()
        .map(|(c, mut f)| {
            f.insert("text".into(), json!(c.to_string()));
            Value::Object(f)
        })
        .collect();
    strip(&runs)
}

/// Runs without [`comparable`]'s `_field` marks, in canonical form.
fn unmark(runs: &[Value]) -> Vec<Value> {
    let runs: Vec<Value> = runs
        .iter()
        .map(|r| {
            let mut r = r.clone();
            if let Some(o) = r.as_object_mut() {
                o.remove("_field");
            }
            r
        })
        .collect();
    canonical(&runs)
}

/// The colour a paragraph's style gives its text (`#rrggbb`): the document defaults, then the
/// style and each style it's based on.
fn style_colour(doc: &Document, p: &Paragraph) -> Option<String> {
    let mut chr = doc.styles.default_chr.clone();
    let pstyle = p.props.style.clone().or_else(|| doc.styles.find("Normal").map(|s| s.id.clone()));
    for st in pstyle.as_deref().map(|id| doc.styles.chain(id)).unwrap_or_default() {
        chr.overlay(&st.chr);
    }
    chr.color.as_ref().and_then(hex)
}

/// Runs without a colour their paragraph's style already gives them, in canonical form: a reader
/// sees the same text. Word 16 drops such a run colour when it saves. Seen in paper.docx's
/// paragraph "…used with any version: UniProt:P12345.3 → http://www.uniprot.org/uniprot/P12345.3.":
/// its style LO-normal sets `<w:color w:val="000000"/>`, Milleleaf's export gives five of its runs
/// the same `<w:color w:val="000000"/>`, and Word's re-save writes those runs with no `w:color`
/// (the style unchanged); nobody touched them.
fn without_style_colour(runs: &[Value], colour: Option<&str>) -> Vec<Value> {
    let Some(colour) = colour else { return runs.to_vec() };
    let runs: Vec<Value> = runs
        .iter()
        .map(|r| {
            let mut r = r.clone();
            if let Some(o) = r.as_object_mut()
                && o.get("color").and_then(Value::as_str).is_some_and(|c| c.eq_ignore_ascii_case(colour))
            {
                o.remove("color");
            }
            r
        })
        .collect();
    canonical(&runs)
}

/// Links a file shows only as empty HYPERLINK fields (see [`comparable`]).
fn emptied_links(runs: &[Value]) -> BTreeSet<String> {
    runs.iter()
        .filter(|r| r.get("text").and_then(Value::as_str).unwrap_or("").is_empty())
        .filter_map(|r| r.get("docField").and_then(Value::as_str).and_then(hyperlink_field))
        .filter(|l| !l.is_empty())
        .collect()
}

/// Canonical form for comparing runs: text runs merged when formatting matches, colours in
/// lower case, field runs reduced to their name.
fn canonical(runs: &[Value]) -> Vec<Value> {
    let mut out: Vec<Map<String, Value>> = Vec::new();
    for r in runs {
        let Some(r) = r.as_object() else { continue };
        if r.contains_key("fieldPath") || r.contains_key("docField") || r.contains_key("picture") {
            out.push(r.clone());
            continue;
        }
        if let Some(f) = r.get("field") {
            out.push(Map::from_iter([("field".to_string(), f.clone())]));
            continue;
        }
        let text = r.get("text").and_then(Value::as_str).unwrap_or("");
        if text.is_empty() {
            continue;
        }
        let mut fmt = r.clone();
        fmt.remove("text");
        if let Some(Value::String(c)) = fmt.get_mut("color") {
            *c = c.to_lowercase();
        }
        for k in ["bold", "italic", "underline"] {
            if fmt.get(k) == Some(&json!(false)) {
                fmt.remove(k);
            }
        }
        if let Some(last) = out.last_mut() {
            let mut last_fmt = last.clone();
            last_fmt.remove("text");
            if !last.contains_key("field") && last_fmt == fmt {
                let joined = format!(
                    "{}{}",
                    last.get("text").and_then(Value::as_str).unwrap_or(""),
                    text
                );
                last.insert("text".into(), json!(joined));
                continue;
            }
        }
        let mut m = fmt;
        m.insert("text".into(), json!(text));
        out.push(m);
    }
    out.into_iter().map(Value::Object).collect()
}

/// A list's id in the model: the name Milleleaf gave its definition on export, else `list-<num>`.
/// The list a numbering instance is: the name Milleleaf gave its definition, else what the
/// paragraphs matched by tag say (`known`: LibreOffice drops the names and renumbers), else new.
fn list_id(doc: &Document, num: u32, known: &HashMap<u32, String>) -> String {
    doc.numbering
        .abstract_of(num)
        .and_then(|a| a.name.clone())
        .filter(|n| n.starts_with("list-"))
        .or_else(|| known.get(&num).cloned())
        .unwrap_or_else(|| format!("list-{num}"))
}

/// A list's definition as data: its levels (WordCraft's `Level`), with the list's own overrides.
fn list_def(doc: &Document, num: u32) -> Option<Value> {
    let a = doc.numbering.abstract_of(num)?;
    let n = doc.numbering.num(num)?;
    let mut levels = a.levels.clone();
    for (l, def) in &n.level_overrides {
        if let Some(x) = levels.get_mut(*l as usize) {
            *x = def.clone();
        }
    }
    for (l, start) in &n.start_overrides {
        if let Some(x) = levels.get_mut(*l as usize) {
            x.start = *start;
        }
    }
    Some(json!({ "levels": serde_json::to_value(levels).ok()? }))
}

/// A paragraph's direct formatting (alignment, spacing, indents, tabs, borders, keep with next…)
/// as data: WordCraft's paragraph properties, without what the model holds elsewhere (style,
/// list) and without tracked changes. `None` when it has none.
/// Settings that only repeat what the paragraph's style already gives are left out: editors write
/// them out on save (Word puts Heading 1's spacing on every heading), and they aren't an edit.
fn para_format(doc: &Document, p: &Paragraph) -> Option<Value> {
    let mut v = serde_json::to_value(&p.props).ok()?;
    let o = v.as_object_mut()?;
    for k in ["style", "numbering", "numChange", "fmtChange"] {
        o.remove(k);
    }
    let from_style = style_para(doc, p);
    o.retain(|k, v| from_style.get(k) != Some(v));
    // A tab stop cleared where the style sets none clears nothing (LibreOffice writes them).
    if let Some(Value::Array(tabs)) = o.get_mut("tabs") {
        let style_tabs: Vec<f64> = from_style
            .get("tabs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|t| t.get("pos").and_then(Value::as_f64))
            .collect();
        // And one the style already sets the same way is repeated (LibreOffice repeats them).
        let style_set: Vec<&Value> = from_style.get("tabs").and_then(Value::as_array).into_iter().flatten().collect();
        tabs.retain(|t| {
            let clear = t.get("align").and_then(Value::as_str) == Some("clear");
            let pos = t.get("pos").and_then(Value::as_f64);
            if clear {
                pos.is_some_and(|p| style_tabs.iter().any(|s| (s - p).abs() < 0.5))
            } else {
                !style_set.contains(&t)
            }
        });
        if tabs.is_empty() {
            o.remove("tabs");
        }
    }
    (!o.is_empty()).then_some(v)
}

/// A paragraph format as a reader sees it, for comparing: a border in automatic colour is drawn
/// black (LibreOffice writes it as black).
fn format_seen(v: Option<&Value>) -> Option<Value> {
    let mut v = v?.clone();
    if let Some(b) = v.get_mut("borders").and_then(Value::as_object_mut) {
        for side in b.values_mut() {
            if let Some(c) = side.get_mut("color")
                && c.is_null()
            {
                *c = json!([0, 0, 0]);
            }
        }
    }
    Some(v)
}

/// What a paragraph's style gives it: the document defaults, then each style it is based on.
fn style_para(doc: &Document, p: &Paragraph) -> Map<String, Value> {
    let mut m: Map<String, Value> = serde_json::to_value(&doc.styles.default_para)
        .ok()
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    let id = p.props.style.clone().or_else(|| doc.styles.find("Normal").map(|s| s.id.clone()));
    for st in id.as_deref().map(|id| doc.styles.chain(id)).unwrap_or_default() {
        if let Ok(Value::Object(sp)) = serde_json::to_value(&st.para) {
            m.extend(sp);
        }
    }
    m
}

/// Whether the paragraph's style puts it in a list.
fn style_numbered(doc: &Document, p: &Paragraph) -> bool {
    style_para(doc, p).get("numbering").is_some_and(|v| !v.is_null())
}

/// The section's page setup as data (WordCraft's section properties: size, orientation,
/// margins, columns, page numbering, borders…), without the headers and footers, which the
/// model holds as `headers` / `footers` / `titlePage`.
fn page_setup(doc: &Document) -> Option<Value> {
    let mut v = serde_json::to_value(&doc.last_section).ok()?;
    let o = v.as_object_mut()?;
    for k in ["headers", "footers", "titlePage", "fmtChange"] {
        o.remove(k);
    }
    Some(v)
}

/// The names of the styles the document's paragraphs use, and Normal.
fn used_style_names(doc: &Document) -> BTreeSet<String> {
    let mut out: BTreeSet<String> = BTreeSet::from(["Normal".to_string()]);
    let parts = doc.parts.values().flat_map(|p| p.blocks.iter());
    for b in doc.body.iter().chain(parts) {
        if let Some(id) = b.as_para().and_then(|p| p.props.style.as_deref()) {
            out.insert(doc.styles.get(id).map(|s| s.name.clone()).unwrap_or_else(|| id.to_string()));
        }
    }
    out
}

/// How each style named in `names` looks, as a reader sees it: its effective font,
/// size, emphasis, colour, alignment, spacing and indents (document defaults, then the styles it
/// is based on). Editors rewrite style definitions on save (LibreOffice moves the defaults into
/// Normal, adds complex-script fonts) without changing this.
fn used_styles(doc: &Document, names: &BTreeSet<String>) -> BTreeMap<String, Value> {
    const SEEN: [&str; 15] = [
        "align", "spaceBefore", "spaceAfter", "lineSpacing", "indentLeft", "indentRight", "indentFirst",
        "font", "size", "bold", "italic", "underline", "color", "caps", "strike",
    ];
    let ids: Vec<String> = names.iter().filter_map(|n| doc.styles.find(n).map(|s| s.id.clone())).collect();
    let obj = |v: Option<Value>| v.and_then(|v| v.as_object().cloned()).unwrap_or_default();
    let mut out = BTreeMap::new();
    for id in &ids {
        let mut m = obj(serde_json::to_value(&doc.styles.default_para).ok());
        m.extend(obj(serde_json::to_value(&doc.styles.default_chr).ok()));
        for st in doc.styles.chain(id) {
            m.extend(obj(serde_json::to_value(&st.para).ok()));
            m.extend(obj(serde_json::to_value(&st.chr).ok()));
        }
        m.retain(|k, v| {
            SEEN.contains(&k.as_str())
                && !matches!((k.as_str(), &*v), ("align", Value::String(a)) if a == "left")
                && !matches!((k.as_str(), &*v), ("color", Value::String(c)) if c == "Auto")
                && *v != Value::Bool(false)
                && v.as_f64() != Some(0.0)
        });
        out.insert(doc.styles.get(id).map(|s| s.name.clone()).unwrap_or_else(|| id.clone()), Value::Object(m));
    }
    out
}

/// A paragraph's named style ("Normal" when it has none).
fn style_name(doc: &Document, p: &Paragraph) -> String {
    p.props
        .style
        .as_deref()
        .map(|id| doc.styles.get(id).map(|s| s.name.clone()).unwrap_or_else(|| id.to_string()))
        .unwrap_or_else(|| "Normal".into())
}

/// A header's or footer's paragraphs as `{style, runs}` (what isn't carried is counted in
/// `dropped`), by which page they're for: `default`, `first`, `even`.
fn header_set(doc: &Document, set: &HeaderSet, dropped: &mut BTreeMap<&'static str, usize>, pics: &mut Pictures) -> Map<String, Value> {
    let mut out = Map::new();
    for (which, id) in [("default", set.default), ("first", set.first), ("even", set.even)] {
        let Some(part) = id.and_then(|id| doc.parts.get(&id)) else { continue };
        let mut paras = Vec::new();
        let d = dropped.entry(which).or_default();
        for b in &part.blocks {
            match b.as_para() {
                Some(p) => {
                    let ex = extract(doc, p, &|_| false, pics);
                    *d += ex.dropped_objects;
                    let mut wp = json!({ "style": style_name(doc, p), "runs": ex.runs });
                    if let Some(f) = para_format(doc, p) {
                        wp["format"] = f;
                    }
                    paras.push(wp);
                }
                None => *d += 1,
            }
        }
        out.insert(which.into(), Value::Array(paras));
    }
    out
}

/// A header or footer as a reader sees it, for comparing: a field's code without its formatting
/// switch (`\* MERGEFORMAT`, which LibreOffice drops) and without the result it last showed (page
/// numbers are worked out again), paragraph formats as seen.
fn header_seen(v: Option<&Value>) -> Value {
    let mut v = v.cloned().unwrap_or(Value::Null);
    for p in v.as_array_mut().into_iter().flatten() {
        if let Some(f) = format_seen(p.get("format")) {
            p["format"] = f;
        }
        for r in p.get_mut("runs").and_then(Value::as_array_mut).into_iter().flatten() {
            // A picture that text doesn't wrap around: its distance from text means nothing, and
            // LibreOffice puts every header picture behind the text when it saves (24.2).
            if let Some(f) = r.pointer_mut("/picture/float").and_then(Value::as_object_mut)
                && matches!(f.get("wrap").and_then(Value::as_str), Some("inFrontOfText" | "behindText"))
            {
                f.insert("wrap".into(), json!("none"));
                for k in ["dist", "distTop", "distBottom"] {
                    f.remove(k);
                }
            }
            if let Some(code) = r.get("docField").and_then(Value::as_str) {
                let code = code.replace("\\* MERGEFORMAT", "").split_whitespace().collect::<Vec<_>>().join(" ");
                r["docField"] = json!(code);
                if let Some(o) = r.as_object_mut() {
                    o.remove("text");
                }
            }
        }
    }
    v
}

/// A second block with the same tag (pasted copy): the original's match becomes a best guess, and
/// the copy is imported as new content.
fn duplicate(report: &mut ImportReport, path: &str, what: &str) {
    let already = report
        .matches
        .iter()
        .any(|m| m.path == path && m.confidence == milleleaf_core::matching::Confidence::BestGuess);
    if !already {
        report.warnings.push(format!(
            "{path}: this {what}'s content control tag appears more than once (pasted copy?): the first is taken as the original, the rest as new; best guess"
        ));
        report.matches.push(milleleaf_core::Match::best_guess(path, format!("{what} tag appears more than once")));
    }
}

/// The tag of the block-level `mleaf:` control a paragraph starts with.
/// The `mleaf:` tags of the controls opened in a paragraph, in order.
fn mleaf_tags(p: &Paragraph) -> impl Iterator<Item = &str> {
    p.objects.iter().filter_map(|o| match o {
        InlineObject::ControlStart { control } => control.tag.strip_prefix(TAG_PREFIX),
        _ => None,
    })
}

/// The node a paragraph stands for: the first `mleaf:` control in it that isn't a linked field.
///
/// Word keeps our block-level controls around the paragraph; LibreOffice (24.2) turns them into
/// inline controls inside it. Either way the tag names the node, so the tag's target decides,
/// not the control's shape.
fn para_tag(p: &Paragraph, is_field: &dyn Fn(&str) -> bool) -> Option<String> {
    mleaf_tags(p).find(|t| !is_field(t)).map(str::to_string)
}

/// The node a table stands for: a control around it, or (after LibreOffice moved it) a control
/// at the start of its first cell.
fn table_tag(t: &Table, is_table: &dyn Fn(&str) -> bool) -> Option<String> {
    if let Some(tag) = t
        .controls
        .open
        .iter()
        .find_map(|c| c.tag.strip_prefix(TAG_PREFIX))
    {
        return Some(tag.to_string());
    }
    let first = t.rows.first()?.cells.first()?.blocks.first()?.as_para()?;
    mleaf_tags(first).find(|tag| is_table(tag)).map(str::to_string)
}

/// What a paragraph says: runs (with `mleaf:` inline controls as field runs, by field node path),
/// and the text each field shows.
struct Extracted {
    runs: Vec<Value>,
    /// (field node path, shown text), in order.
    fields: Vec<(String, String)>,
    text: String,
    dropped_objects: usize,
}

/// A run's formatting as a reader sees it: its character style's (`w:rStyle`, with what that is
/// based on), then its own. The model has no character styles, so their formatting goes on the run.
fn effective_chr(doc: &Document, cp: &CharProps) -> CharProps {
    let Some(id) = cp.style.as_deref() else { return cp.clone() };
    let mut out = CharProps::default();
    for st in doc.styles.chain(id) {
        out.overlay(&st.chr);
    }
    out.overlay(cp);
    out
}

fn extract(doc: &Document, p: &Paragraph, is_field: &dyn Fn(&str) -> bool, pics: &mut Pictures) -> Extracted {
    let media = &doc.media;
    let mut runs: Vec<Value> = Vec::new();
    let mut fields = Vec::new();
    let mut text = String::new();
    let mut dropped_objects = 0;
    // Open inline controls: Some(path) for an mleaf field, None for anything else.
    let mut open: Vec<Option<String>> = Vec::new();
    let mut field_text = String::new();
    // A Word field kept as a range (its result is ordinary text between start and end marks:
    // REF, TOC, a field with a tracked change inside…): one field run, its result the text in
    // between. Nested ones are part of the outer one's result.
    let (mut range, mut range_depth): (Option<Map<String, Value>>, usize) = (None, 0);
    for (off, ch) in p.text.char_indices() {
        // Tracked changes come in accepted: deleted text (and what sits in it) isn't content.
        if p.props_of_char(off).del.is_some() {
            continue;
        }
        let in_field = open.iter().any(Option::is_some);
        if ch == OBJ {
            match p.object_at(off) {
                Some(InlineObject::ControlStart { control }) => {
                    match control.tag.strip_prefix(TAG_PREFIX) {
                        // A linked field: its shown text is replaced by a field run.
                        Some(path) if !in_field && is_field(path) => {
                            runs.push(json!({ "fieldPath": path }));
                            open.push(Some(path.to_string()));
                            field_text.clear();
                        }
                        // Anything else (the paragraph's own identity control, a form field) is
                        // transparent: its content is ordinary text.
                        _ => open.push(None),
                    }
                }
                Some(InlineObject::ControlEnd) => {
                    if let Some(Some(path)) = open.pop() {
                        fields.push((path, field_text.trim().to_string()));
                    }
                }
                Some(InlineObject::FieldStart { instr, .. }) if !in_field => {
                    if range_depth == 0 {
                        let mut fmt = run_format(&effective_chr(doc, p.props_of_char(off)));
                        fmt.insert("docField".into(), json!(instr.trim()));
                        fmt.insert("text".into(), json!(""));
                        range = Some(fmt);
                    }
                    range_depth += 1;
                }
                Some(InlineObject::FieldEnd) if !in_field && range_depth > 0 => {
                    range_depth -= 1;
                    if range_depth == 0
                        && let Some(fmt) = range.take()
                    {
                        runs.push(Value::Object(fmt));
                    }
                }
                Some(InlineObject::Field { result, .. }) if !in_field && range_depth > 0 => {
                    text.push_str(result);
                    if let Some(r) = range.as_mut() {
                        r["text"] = json!(format!("{}{result}", r["text"].as_str().unwrap_or("")));
                    }
                }
                // A Word field (PAGE, DATE…): its code, with the result it showed.
                Some(InlineObject::Field { instr, result, .. }) if !in_field => {
                    let mut fmt = run_format(&effective_chr(doc, p.props_of_char(off)));
                    fmt.insert("docField".into(), json!(instr.trim()));
                    fmt.insert("text".into(), json!(result));
                    text.push_str(result);
                    runs.push(Value::Object(fmt));
                }
                Some(o) if o.is_marker() => {}
                Some(o) if !in_field => match picture_run(media, o, pics) {
                    Some(r) => runs.push(r),
                    None => dropped_objects += 1,
                },
                Some(_) => dropped_objects += 1,
                None => {}
            }
            continue;
        }
        if in_field {
            field_text.push(ch);
            continue;
        }
        text.push(ch);
        if let Some(r) = range.as_mut() {
            r["text"] = json!(format!("{}{ch}", r["text"].as_str().unwrap_or("")));
            continue;
        }
        let mut fmt = run_format(&effective_chr(doc, p.props_of_char(off)));
        fmt.insert("text".into(), json!(ch.to_string()));
        runs.push(Value::Object(fmt));
    }
    // A range the paragraph doesn't close (it ends in a later paragraph): its result so far.
    if let Some(fmt) = range.take() {
        runs.push(Value::Object(fmt));
    }
    Extracted {
        runs: canonical(&runs),
        fields,
        text,
        dropped_objects,
    }
}

/// A cell's paragraphs in reading order, including those of a table nested in it (whose text is
/// carried as the cell's paragraphs; the nested table itself isn't).
fn cell_paras(c: &wordcraft_doc::Cell) -> Vec<&Paragraph> {
    let mut out = Vec::new();
    for b in &c.blocks {
        match &**b {
            Block::Para(p) => out.push(p),
            Block::Table(t) => out.extend(t.rows.iter().flat_map(|r| &r.cells).flat_map(cell_paras)),
        }
    }
    out
}

/// What a cell paragraph shows (a field's result for its object), accepted: deleted text isn't the
/// cell's. With the links on it: `(start, end, address)` in characters, adjacent stretches with one
/// address joined (editors split a link into pieces as they like).
fn shown_para(p: &Paragraph) -> (String, Vec<(usize, usize, String)>) {
    let (mut out, mut k, mut n) = (String::new(), 0usize, 0usize);
    let mut links: Vec<(usize, usize, String)> = Vec::new();
    for (off, c) in p.text.char_indices() {
        let obj = (c == OBJ).then(|| p.objects.get(k));
        k += usize::from(c == OBJ);
        let cp = p.props_of_char(off);
        if cp.del.is_some() {
            continue;
        }
        let s = match obj {
            Some(o) => o.map(|o| o.plain_text()).unwrap_or_default().to_string(),
            None => c.to_string(),
        };
        let len = s.chars().count();
        out.push_str(&s);
        if let Some(l) = cp.link.as_ref().filter(|_| len > 0) {
            match links.last_mut() {
                Some(last) if last.1 == n && &last.2 == l => last.1 += len,
                _ => links.push((n, n + len, l.clone())),
            }
        }
        n += len;
    }
    (out, links)
}

fn cell_text(c: &wordcraft_doc::Cell) -> String {
    cell_paras(c).into_iter().map(|p| shown_para(p).0).collect::<Vec<_>>().join("\n")
}

/// A cell's links, by its paragraphs (see [`shown_para`]): `{para, start, end, text, link}`.
pub(crate) fn cell_links(c: &wordcraft_doc::Cell) -> Vec<Value> {
    let mut out = Vec::new();
    for (i, p) in cell_paras(c).into_iter().enumerate() {
        let (text, links) = shown_para(p);
        for (s, e, l) in links {
            let t: String = text.chars().skip(s).take(e - s).collect();
            out.push(json!({ "para": i, "start": s, "end": e, "text": t, "link": l }));
        }
    }
    out
}

/// The table's text on its grid: a cell spanning columns fills its first column and leaves the
/// rest empty, so every row has a cell for every column.
fn table_grid(t: &Table) -> Vec<Vec<String>> {
    t.rows
        .iter()
        .map(|r| {
            r.cells
                .iter()
                .flat_map(|c| {
                    std::iter::once(cell_text(c)).chain((1..c.props.span.max(1)).map(|_| String::new()))
                })
                .collect()
        })
        .collect()
}

/// How a table looks beyond its data: merged cells, and `header: false` when its first row isn't
/// a header row (its text becomes the column names either way).
fn table_look(t: &Table) -> Map<String, Value> {
    let mut wt = Map::new();
    if !t.rows.first().is_some_and(|r| r.props.header) {
        wt.insert("header".into(), json!(false));
    }
    let merges = table_merges(t);
    if !merges.is_empty() {
        wt.insert("merges".into(), json!(merges));
    }
    wt
}

/// How a table is drawn: its column widths (`grid`, points), its own properties (`props`:
/// borders, alignment, cell margins, shading… WordCraft's form) and each cell's (`cells`, by row
/// and cell: the cell's properties, its first paragraph's formatting and character format, its
/// paragraphs' lists when any is in one, and its links). `list_of` names a numbering instance's list.
fn table_style_look(doc: &Document, t: &Table, list_of: &mut dyn FnMut(u32) -> String) -> Map<String, Value> {
    let strip = |v: Option<Value>, keys: &[&str]| -> Option<Value> {
        let mut v = v?;
        let o = v.as_object_mut()?;
        for k in keys {
            o.remove(*k);
        }
        o.retain(|_, x| !x.is_null());
        (!o.is_empty()).then_some(v)
    };
    let mut m = Map::new();
    m.insert("grid".into(), json!(t.grid));
    // The table style by name; "" for none (else the export would give it WordCraft's Table Grid,
    // whose text size LibreOffice puts over the paragraph style's).
    let style = t.props.style.as_deref().map(|id| doc.styles.get(id).map(|s| s.name.clone()).unwrap_or_else(|| id.to_string()));
    m.insert("style".into(), json!(style.unwrap_or_default()));
    if let Some(p) = strip(serde_json::to_value(&t.props).ok(), &["style", "caption", "description", "fmtChange"]) {
        m.insert("props".into(), p);
    }
    let cells: Vec<Value> = t
        .rows
        .iter()
        .map(|r| {
            Value::Array(
                r.cells
                    .iter()
                    .map(|c| {
                        let mut cm = Map::new();
                        if let Some(p) = strip(serde_json::to_value(&c.props).ok(), &["span", "vmerge", "width", "widthPct", "fmtChange"]) {
                            cm.insert("props".into(), p);
                        }
                        let para_lists: Vec<Value> = cell_paras(c)
                            .into_iter()
                            .map(|p| match p.props.numbering.filter(|n| n.num != 0) {
                                Some(n) => json!({ "id": list_of(n.num), "level": n.level }),
                                None => Value::Null,
                            })
                            .collect();
                        if para_lists.iter().any(Value::is_object) {
                            cm.insert("lists".into(), Value::Array(para_lists));
                        }
                        let links = cell_links(c);
                        if !links.is_empty() {
                            cm.insert("links".into(), Value::Array(links));
                        }
                        if let Some(p) = c.blocks.iter().find_map(|b| b.as_para()) {
                            if let Some(f) = para_format(doc, p) {
                                cm.insert("para".into(), f);
                            }
                            if !p.text.is_empty()
                                && let Some(ch) = strip(serde_json::to_value(p.props_of_char(0)).ok(), &["fmtChange", "change", "link", "ins", "del"]) {
                                    cm.insert("chr".into(), ch);
                                }
                        }
                        Value::Object(cm)
                    })
                    .collect(),
            )
        })
        .collect();
    m.insert("cells".into(), Value::Array(cells));
    m
}

/// The column widths a table asks for, points. A fixed-layout table's are its grid. An autofit
/// table's grid (`w:tblGrid`) is only the editor's last layout: Word 16 lays the table out again
/// when it opens it and saves the result, while the widths asked for, the cells' preferred widths
/// (`w:tcW`), stay as they were. Seen in paper.docx's tables 2 and 6 (`w:tblW w:type="auto"`, no
/// `w:tblLayout`): Word's re-save of Milleleaf's export wrote `w:gridCol` 9923 → 9872 and
/// 6567/3356 → 6510/3423 twips, every `w:tcW` unchanged (9923; 6567/3356), and nobody touched
/// them. Dragging a column border in Word changes the cells' `w:tcW` too, so that still shows.
/// So an autofit table's widths are its first row's preferred widths when each of that row's
/// cells has one and spans one column; else its grid.
fn column_widths(t: &Table) -> Vec<f32> {
    if !t.props.fixed
        && let Some(r) = t.rows.first()
        && r.cells.len() == t.grid.len()
        && let Some(w) = r.cells.iter().map(|c| c.props.width.filter(|_| c.props.span <= 1)).collect::<Option<Vec<f32>>>()
    {
        return w;
    }
    t.grid.clone()
}

/// What a reader sees of a table: column widths (to the half point), its alignment and indent,
/// and per cell the effective shading, emphasis, colour, size and alignment (its table style's
/// parts, then the cell's own formatting). Editors move table-style formatting into the cells on
/// save (LibreOffice does) without changing this.
fn table_seen(doc: &Document, t: &Table) -> Value {
    let style = t.props.style.as_deref().and_then(|id| doc.styles.table_style(id)).unwrap_or_default();
    let half = |x: f32| (x * 2.0).round() as i64;
    let nrows = t.rows.len();
    let ncols = t.grid.len();
    let mut rows = Vec::new();
    for (ri, row) in t.rows.iter().enumerate() {
        let mut cells = Vec::new();
        let mut g = 0usize;
        for c in &row.cells {
            let span = c.props.span.max(1) as usize;
            let mut pf = PartFormat::default();
            for part in PartSet::of_cell(&t.props.look, 1, (ri, nrows), (g, span, ncols)).iter() {
                pf.overlay(&style.parts.part(part));
            }
            let p = c.blocks.iter().find_map(|b| b.as_para());
            let ch = p.filter(|p| !p.text.is_empty()).map(|p| p.props_of_char(0).clone()).unwrap_or_default();
            // Document defaults, the paragraph's style, the table style, its parts, the run.
            let mut chr = doc.styles.default_chr.clone();
            let pstyle = p.and_then(|p| p.props.style.clone()).or_else(|| doc.styles.find("Normal").map(|s| s.id.clone()));
            for st in pstyle.as_deref().map(|id| doc.styles.chain(id)).unwrap_or_default() {
                chr.overlay(&st.chr);
            }
            chr.overlay(&style.chr);
            chr.overlay(&pf.chr);
            chr.overlay(&ch);
            let align = p.and_then(|p| p.props.align).or(pf.align);
            // Which paragraphs are in a list, at what level (not which list: editors renumber).
            let levels: Vec<Option<u8>> = cell_paras(c).into_iter().map(|p| p.props.numbering.filter(|n| n.num != 0).map(|n| n.level)).collect();
            cells.push(json!([
                levels,
                c.props.shading.or(pf.fill).map(|r| r.hex()),
                chr.bold == Some(true),
                chr.italic == Some(true),
                serde_json::to_value(chr.color.as_ref().filter(|c| !matches!(c, TextColor::Auto))).ok(),
                chr.size.map(half),
                serde_json::to_value(align.filter(|a| *a != wordcraft_doc::Align::Left)).ok(),
                serde_json::to_value(c.props.valign.or(pf.v_align)).ok(),
            ]));
            g += span;
        }
        rows.push(Value::Array(cells));
    }
    json!({
        "grid": column_widths(t).iter().map(|w| half(*w)).collect::<Vec<_>>(),
        "align": serde_json::to_value(t.props.align.filter(|a| *a != wordcraft_doc::Align::Left)).ok(),
        "indent": t.props.indent.map(half).filter(|i| *i != 0),
        "rows": rows,
    })
}

/// Whether two `table_seen` values look the same, over the rows both have: a linked table can
/// have gained or lost rows since the file was exported (the data changed, not the look).
fn same_look(ours: Option<&Value>, now: &Value) -> bool {
    let Some(ours) = ours else { return false };
    let row_cells = |v: &Value| v["rows"].as_array().cloned().unwrap_or_default();
    let (a, b) = (row_cells(ours), row_cells(now));
    // Column widths to within a half point: editors store twips and round them their own way
    // (LibreOffice: 221.25 pt comes back as 221.2).
    let grid = |v: &Value| v["grid"].as_array().cloned().unwrap_or_default();
    let (ga, gb) = (grid(ours), grid(now));
    let same_grid = ga.len() == gb.len() && ga.iter().zip(&gb).all(|(x, y)| (x.as_i64().unwrap_or(0) - y.as_i64().unwrap_or(0)).abs() <= 1);
    same_grid && ["align", "indent"].iter().all(|k| ours[k] == now[k]) && a.iter().zip(&b).all(|(x, y)| x == y)
}

/// Merged cells on the grid: `{row, col, rows, cols}`, row 0 being the first (header) row.
fn table_merges(t: &Table) -> Vec<Value> {
    // (row, grid column) → (vmerge, span) for every cell.
    let mut at: Vec<Vec<(VMerge, u32)>> = Vec::new();
    for r in &t.rows {
        let mut row = Vec::new();
        for c in &r.cells {
            row.push((c.props.vmerge, c.props.span.max(1)));
            row.extend((1..c.props.span.max(1)).map(|_| (VMerge::None, 0)));
        }
        at.push(row);
    }
    let mut out = Vec::new();
    for (ri, row) in at.iter().enumerate() {
        for (ci, &(vm, span)) in row.iter().enumerate() {
            if span == 0 || vm == VMerge::Continue {
                continue;
            }
            let rows = if vm == VMerge::Restart {
                1 + at[ri + 1..]
                    .iter()
                    .take_while(|r| r.get(ci).is_some_and(|c| c.0 == VMerge::Continue))
                    .count()
            } else {
                1
            };
            if rows > 1 || span > 1 {
                out.push(json!({ "row": ri, "col": ci, "rows": rows, "cols": span }));
            }
        }
    }
    out
}

/// The grid a composed `office::data::table` exports as: header names, then formatted rows.
fn expected_grid(node: &Composed) -> (Vec<Map<String, Value>>, Vec<Vec<String>>) {
    let data = obj(node, "office::data::table")
        .cloned()
        .unwrap_or_default();
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
    (cols, grid)
}

/// Parses a shown cell back to a value of the column's type.
fn parse_cell(text: &str, kind: &str) -> Option<Value> {
    if kind == "text" || kind == "date" {
        return Some(json!(text));
    }
    let cleaned: String = text
        .chars()
        .filter(|c| !matches!(c, '$' | ',' | ' ' | '%' | '\u{a0}'))
        .collect();
    let n: f64 = cleaned.parse().ok()?;
    let n = if kind == "percent" { n / 100.0 } else { n };
    Some(if n.fract() == 0.0 && n.abs() < 1e15 {
        json!(n as i64)
    } else {
        json!(n)
    })
}

fn guess_type(cells: &[&str]) -> &'static str {
    if cells.is_empty() || cells.iter().any(|c| parse_cell(c, "number").is_none()) {
        "text"
    } else if cells.iter().any(|c| c.contains('$')) {
        "currency"
    } else if cells.iter().all(|c| c.ends_with('%')) {
        "percent"
    } else {
        "number"
    }
}

fn is_linked(n: &Composed, key: &str) -> bool {
    n.provenance.get(key).is_some_and(|p| !p.via.is_empty())
}

/// Compares `edited` with the composed document `node` it was exported from and returns the
/// edit layer that would turn one into the other.
pub fn import(
    node: &Composed,
    edited: &Document,
    header: Header,
) -> Result<(Layer, ImportReport), WordError> {
    import_in(node, edited, header, None)
}

/// [`import`], with the folder a stack's pictures are read from when they're still named by a
/// path (a stack file's, not stored by hash): they're compared by the hash of that file.
pub fn import_in(
    node: &Composed,
    edited: &Document,
    header: Header,
    assets: Option<&std::path::Path>,
) -> Result<(Layer, ImportReport), WordError> {
    let found = custom_prop(edited, PROP_DOCUMENT).ok_or(WordError::NotExported)?;
    if found != node.path {
        return Err(WordError::WrongDocument {
            expected: node.path.clone(),
            found: found.to_string(),
        });
    }
    import_into(node, edited, header, false, assets, false)
}

/// Brings a document Milleleaf didn't make into a project: it is imported into an empty
/// `word::document` node at `path`, so every paragraph and table becomes new content.
/// What the importer can't carry is in the report's warnings.
pub fn adopt(
    edited: &Document,
    path: &str,
    title: &str,
    header: Header,
) -> Result<(Layer, ImportReport), WordError> {
    adopt_file(edited, path, title, header, false)
}

/// Adopts file values while retaining any Milleleaf content-control identities. Links to a stack are
/// not inferred from tags: the adopted file remains a self-contained snapshot.
pub fn adopt_for_comparison(
    edited: &Document,
    path: &str,
    title: &str,
    header: Header,
) -> Result<(Layer, ImportReport), WordError> {
    adopt_file(edited, path, title, header, true)
}

/// Compares a file with its adopted snapshot, including files without Milleleaf export stamps.
/// Untagged blocks matched by place are explicitly reported as best guesses.
pub fn import_for_comparison(
    node: &Composed,
    edited: &Document,
    header: Header,
) -> Result<(Layer, ImportReport), WordError> {
    if let Some(found) = exported_path(edited)
        && found != node.path {
            return Err(WordError::WrongDocument { expected: node.path.clone(), found });
        }
    let (mut layer, report) = import_into(node, edited, header, false, None, true)?;
    milleleaf_core::matching::flag_best_guesses(&mut layer, &report.matches);
    Ok((layer, report))
}

/// The document identity recorded by a Milleleaf export, when present.
pub fn exported_path(edited: &Document) -> Option<String> {
    custom_prop(edited, PROP_DOCUMENT).map(String::from)
}

fn adopt_file(
    edited: &Document,
    path: &str,
    title: &str,
    header: Header,
    file_comparison: bool,
) -> Result<(Layer, ImportReport), WordError> {
    let empty = Composed {
        path: path.to_string(),
        name: None,
        attributes: Map::from_iter([("word::document".into(), json!({ "title": title }))]),
        provenance: Default::default(),
        children: vec![],
    };
    let (mut layer, report) = import_into(&empty, edited, header, true, None, file_comparison)?;
    match layer.data.iter_mut().find(|n| n.path == path) {
        Some(n) => {
            let wd = n.attributes.entry("word::document").or_insert_with(|| json!({}));
            wd["title"] = json!(title);
        }
        None => layer.data.insert(
            0,
            Node {
                path: path.to_string(),
                attributes: Map::from_iter([("word::document".into(), json!({ "title": title }))]),
                ..Node::default()
            },
        ),
    }
    Ok((layer, report))
}

/// `node` with its paragraphs' pictures named by a readable path renamed by their file's hash.
fn paragraph_pictures_by_hash(node: &Composed, root: &std::path::Path) -> Composed {
    let mut out = node.clone();
    for c in &mut out.children {
        for r in c.attributes.get_mut("word::paragraph").and_then(|p| p.get_mut("runs")).and_then(Value::as_array_mut).into_iter().flatten() {
            if let Some(b) = r.pointer_mut("/picture/blob")
                && let Some(h) = b.as_str().and_then(|p| milleleaf_core::resolve_blob(Some(root), p))
            {
                *b = json!(h);
            }
        }
    }
    out
}

/// A paragraph holding only a page break, before another paragraph, written the way Word does:
/// the next paragraph starts on a new page (`pageBreakBefore`), and the break's paragraph is empty.
/// LibreOffice (24.2) saves "page break before" the other way round; a reader sees the same.
fn page_breaks_as_properties(doc: &Document) -> std::borrow::Cow<'_, Document> {
    // (A page break is whitespace to `trim`: compare the text itself, without control marks.)
    let only_break = |b: &Block| b.as_para().is_some_and(|p| p.text.chars().filter(|c| *c != OBJ).eq(['\u{c}']));
    let at: Vec<usize> = (0..doc.body.len().saturating_sub(1))
        .filter(|&i| only_break(&doc.body[i]) && doc.body[i + 1].as_para().is_some())
        .collect();
    if at.is_empty() {
        return std::borrow::Cow::Borrowed(doc);
    }
    let mut out = doc.clone();
    for i in at {
        if let Block::Para(p) = std::sync::Arc::make_mut(&mut out.body[i]) {
            let props = p.props.clone();
            *p = Paragraph::new();
            p.props = props;
        }
        if let Block::Para(p) = std::sync::Arc::make_mut(&mut out.body[i + 1]) {
            p.props.page_break_before = Some(true);
        }
    }
    std::borrow::Cow::Owned(out)
}

fn import_into(
    node: &Composed,
    edited: &Document,
    header: Header,
    adopting: bool,
    assets: Option<&std::path::Path>,
    file_comparison: bool,
) -> Result<(Layer, ImportReport), WordError> {
    let edited = &*page_breaks_as_properties(edited);
    let mut report = ImportReport::default();
    for b in milleleaf_core::unreadable_pictures(node, assets) {
        report.warnings.push(format!("picture `{b}` can't be read, so it's compared by its path and may come back as changed"));
    }
    if !adopting && !file_comparison && custom_prop(edited, PROP_REVISION) != Some(revision_of(node).as_str()) {
        report.stale = true;
        report.warnings.push("the document changed in the stack since this file was exported; review the edit layer before stacking it".into());
    }
    // A stack file names its pictures by path: body paragraphs' pictures are compared by the hash
    // of their file, as the file's own are (a re-save then isn't a change).
    let resolved = assets.map(|root| paragraph_pictures_by_hash(node, root));
    let node = resolved.as_ref().unwrap_or(node);

    let by_path: HashMap<&str, &Composed> =
        node.children.iter().map(|c| (c.path.as_str(), c)).collect();
    // What Milleleaf's own export writes for this node, read back the same way: page setup,
    // styles and tables' looks are carried when the file's differ from it.
    let ours_doc = crate::export::export_with(node, &[], milleleaf_core::PictureSource::StandIn(assets))
        .ok()
        .and_then(|(d, _)| crate::from_docx(&crate::to_docx(&d).ok()?).ok());
    let mut used_names: BTreeSet<String> = node
        .children
        .iter()
        .filter_map(|c| c.name.clone())
        .collect();
    // Field nodes anywhere in the document: path → (owner paragraph, name, the field node).
    let mut fields: HashMap<&str, (&str, &str, &Composed)> = HashMap::new();
    for c in &node.children {
        for f in &c.children {
            if obj(f, "mleaf::link").is_some() {
                fields.insert(
                    f.path.as_str(),
                    (c.path.as_str(), f.name.as_deref().unwrap_or(""), f),
                );
            }
        }
    }

    let field_paths: BTreeSet<String> = fields.keys().map(|k| k.to_string()).collect();
    let is_field = |t: &str| field_paths.contains(t);
    let is_table = |t: &str| {
        by_path
            .get(t)
            .is_some_and(|c| c.attributes.contains_key("office::data::table"))
    };

    let ours_tables: HashMap<String, Value> = ours_doc
        .iter()
        .flat_map(|d| d.body.iter().map(move |b| (d, b)))
        .filter_map(|(d, b)| match &**b {
            Block::Table(t) => Some((table_tag(t, &is_table)?, table_seen(d, t))),
            _ => None,
        })
        .collect();
    let mut nodes: Vec<Node> = Vec::new();
    // Lists the paragraphs use: list id → numbering instance in the edited file.
    let mut lists_used: BTreeMap<String, u32> = BTreeMap::new();
    let mut known_lists: HashMap<u32, String> = HashMap::new();
    for b in &edited.body {
        if let (Block::Para(p), Some(n)) = (&**b, b.as_para().and_then(|p| p.props.numbering).filter(|n| n.num != 0)) {
            let old = para_tag(p, &is_field)
                .and_then(|t| by_path.get(t.as_str()).copied())
                .and_then(|c| obj(c, "word::paragraph")?.get("list")?.get("id")?.as_str().map(String::from));
            if let Some(id) = old {
                known_lists.entry(n.num).or_insert(id);
            }
        }
    }
    let mut seen: BTreeSet<String> = BTreeSet::new();
    // Characters LibreOffice linked by extending a link back (see `settle_spread_links`).
    let mut spread_links = 0usize;
    // Links LibreOffice turned into empty fields, unlinking their text (see `comparable`).
    let mut lost_links = 0usize;
    let mut captions_seen: BTreeSet<String> = BTreeSet::new();
    let mut new_order: Vec<String> = Vec::new();
    // Where the last matched block sits among the original's children.
    let child_idx: HashMap<&str, usize> = node.children.iter().enumerate().map(|(i, c)| (c.path.as_str(), i)).collect();
    let mut last_seen: Option<usize> = None;
    // A new block's path: from where it is and what it holds, unless a node has that path already.
    // An adopted document's blocks got theirs the same way, so a block typed where the adopted one
    // had a block with the same content would take that block's path: one node for two blocks. Seen
    // on Word 16's blank line in the paper: the new empty paragraph sits at body index 6, as the
    // adopted empty paragraph after "Throughout this document…" did, and took its path.
    let new_path = |base: String, nodes: &[Node]| {
        let taken = |p: &str| by_path.contains_key(p) || nodes.iter().any(|n| n.path == p);
        let mut path = stable_id(&base);
        let mut i = 2;
        while taken(&path) {
            path = stable_id(&format!("{base}|{i}"));
            i += 1;
        }
        path
    };
    let tags_in_file: BTreeSet<String> = edited
        .body
        .iter()
        .filter_map(|b| match &**b {
            Block::Para(p) => para_tag(p, &is_field),
            _ => None,
        })
        .collect();
    let mut new_children: BTreeMap<String, Option<String>> = BTreeMap::new();
    let fresh_name = |prefix: &str, path: &str, used: &mut BTreeSet<String>| {
        let base = format!("{prefix}-{}", &path[..8.min(path.len())]);
        let mut name = base.clone();
        let mut i = 2;
        while used.contains(&name) {
            name = format!("{base}-{i}");
            i += 1;
        }
        used.insert(name.clone());
        name
    };

    for (index, block) in edited.body.iter().enumerate() {
        match &**block {
            Block::Para(p) => {
                let tag = para_tag(p, &is_field);
                let ex = extract(edited, p, &is_field, &mut report.pictures);
                if ex.dropped_objects > 0 {
                    report.warnings.push(format!(
                        "paragraph {}: {} inline object(s) (shapes, charts, embedded objects…) are not carried yet",
                        index + 1,
                        ex.dropped_objects
                    ));
                }
                // Captions belong to their table node, including a tagged file adopted as a
                // self-contained snapshot rather than reconstructed from its original stack.
                if adopting && file_comparison
                    && let Some(table_path) = tag.as_deref().and_then(|t| t.strip_suffix(CAPTION_SUFFIX))
                        && let Some(table) = nodes.iter_mut().find(|n| n.path == table_path && n.attributes.contains_key("word::table")) {
                            if let Some(look) = table.attributes.get_mut("word::table") {
                                look["caption"] = json!(ex.text.trim());
                            }
                            continue;
                        }
                // Captions belong to their table node.
                if let Some(table_path) =
                    tag.as_deref().and_then(|t| t.strip_suffix(CAPTION_SUFFIX))
                    && let Some(t) = by_path.get(table_path) {
                        captions_seen.insert(table_path.to_string());
                        let wt = obj(t, "word::table").cloned().unwrap_or_default();
                        let caption = ex.text.trim().to_string();
                        if wt.get("caption").and_then(Value::as_str) != Some(caption.as_str()) {
                            let mut wt2 = wt.clone();
                            wt2.insert("caption".into(), json!(caption));
                            nodes.push(Node {
                                path: table_path.to_string(),
                                attributes: Map::from_iter([(
                                    "word::table".into(),
                                    Value::Object(wt2),
                                )]),
                                ..Node::default()
                            });
                            report.changed.push(table_path.to_string());
                        }
                        continue;
                    }
                    // A caption for a table this document doesn't have: keep it as a paragraph
                    // rather than dropping its text.
                let style = style_name(edited, p);
                let existing = tag
                    .as_deref()
                    .and_then(|t| by_path.get(t).copied())
                    .filter(|c| c.attributes.contains_key("word::paragraph"));
                // A pasted copy carries its original's tag: the first is the original, a copy is new.
                let existing = match existing {
                    Some(c) if seen.contains(&c.path) => {
                        duplicate(&mut report, &c.path, "paragraph");
                        None
                    }
                    e => e,
                };
                // An untagged empty paragraph is a blank line someone typed, and comes back, unless
                // it's one Word must have: after a table that ends the body, the only paragraph of
                // an empty body, or one holding a section break (sections are reported as not
                // carried).
                let required = index + 1 == edited.body.len()
                    && (index == 0 || matches!(&*edited.body[index - 1], Block::Table(_)))
                    || p.section.is_some();
                let blank = existing.is_none() && tag.is_none() && ex.text.trim().is_empty() && ex.fields.is_empty();
                if !adopting && blank && required {
                    continue;
                }
                // Editors drop the tag from a paragraph without text (LibreOffice does, from an empty
                // one or one holding only a picture): one where the original had such a paragraph,
                // right after the last block matched, is that one. Comparing two files (the demo),
                // an untagged paragraph is also matched by place.
                let by_position = !adopting && (blank || (file_comparison && tag.is_none()));
                let existing = if by_position {
                    node.children
                        .get(last_seen.map_or(0, |i| i + 1))
                        .filter(|c| !seen.contains(&c.path) && !tags_in_file.contains(c.path.as_str()))
                        .filter(|c| {
                            obj(c, "word::paragraph").is_some_and(|w| {
                                !blank || w.get("runs").and_then(Value::as_array).is_none_or(|r| {
                                    r.iter().all(|r| r.get("picture").is_some() || r.get("text").and_then(Value::as_str).is_some_and(|t| t.trim().is_empty()))
                                })
                            })
                        })
                } else {
                    existing
                };
                let para_path = match existing {
                    Some(c) => c.path.clone(),
                    None => tag.clone().filter(|t| adopting && file_comparison && !nodes.iter().any(|n| &n.path == t))
                        .unwrap_or_else(|| new_path(format!("{}|word-edit|{}|{}", node.path, index, ex.text), &nodes)),
                };
                // Resolve field runs to child names, adding a child when a field was copied here.
                let mut children: BTreeMap<String, Option<String>> = BTreeMap::new();
                let mut extra_nodes = Vec::new();
                let mut field_iter = ex.fields.iter();
                let mut runs = Vec::new();
                for r in &ex.runs {
                    let Some(fp) = r.get("fieldPath").and_then(Value::as_str) else {
                        runs.push(r.clone());
                        continue;
                    };
                    let shown = field_iter
                        .next()
                        .map(|(_, t)| t.clone())
                        .unwrap_or_default();
                    let Some((owner, name, fnode)) = fields.get(fp).copied() else {
                        report.warnings.push(format!(
                            "{para_path}: field `{fp}` is unknown; kept its text"
                        ));
                        runs.push(json!({ "text": shown }));
                        continue;
                    };
                    let expected = existing
                        .or(by_path.get(owner).copied())
                        .map(|o| milleleaf_core::render::field_text(o, name))
                        .unwrap_or_default();
                    if shown != expected {
                        report.conflicts.push(Conflict { path: fp.to_string(), message: format!("linked field shows `{shown}`, source says `{expected}`; edit the source instead") });
                    }
                    if owner == para_path {
                        runs.push(json!({ "field": name }));
                    } else {
                        // Copied into another paragraph: a new field node inheriting the same source.
                        let new_path = stable_id(&format!("{para_path}|field|{fp}"));
                        let link = obj(fnode, "mleaf::link").cloned().unwrap_or_default();
                        let source = link
                            .get("source")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        let new_name = format!("field-{}", &new_path[..8]);
                        children.insert(new_name.clone(), Some(new_path.clone()));
                        extra_nodes.push(Node {
                            path: new_path,
                            inherits: BTreeMap::from([("source".to_string(), Some(source))]),
                            attributes: Map::from_iter([("mleaf::link".into(), Value::Object(link))]),
                            ..Node::default()
                        });
                        runs.push(json!({ "field": new_name }));
                    }
                }
                let mut new_para = json!({ "style": style, "runs": runs });
                if let Some(f) = para_format(edited, p) {
                    new_para["format"] = f;
                }
                if let Some(n) = p.props.numbering.filter(|n| n.num != 0) {
                    let id = list_id(edited, n.num, &known_lists);
                    lists_used.entry(id.clone()).or_insert(n.num);
                    new_para["list"] = json!({ "id": id, "level": n.level });
                } else if let Some(n) = p.props.numbering.filter(|_| style_numbered(edited, p)) {
                    // Numbering turned off on a paragraph whose style numbers it (a heading
                    // style with numbering, used for an unnumbered heading): `id` "".
                    new_para["list"] = json!({ "id": "", "level": n.level });
                }
                match existing {
                    Some(c) => {
                        seen.insert(c.path.clone());
                        last_seen = child_idx.get(c.path.as_str()).copied();
                        report.matches.push(if by_position {
                            milleleaf_core::Match::best_guess(&c.path, if blank { "an empty paragraph in the same place" } else { "an untagged paragraph in the same place" })
                        } else {
                            milleleaf_core::Match::certain(&c.path, "content control tag")
                        });
                        new_order.push(c.name.clone().unwrap_or_default());
                        let old = obj(c, "word::paragraph").cloned().unwrap_or_default();
                        // Numbering turned off where the stack has no list: an editor writing the
                        // "off" out (LibreOffice does, for headings its outline numbering covers),
                        // still unnumbered. Not an edit.
                        if old.get("list").is_none() && new_para.get("list").and_then(|l| l.get("id")).and_then(Value::as_str) == Some("")
                            && let Some(o) = new_para.as_object_mut() {
                                o.remove("list");
                            }
                        let old_runs: Vec<Value> = old
                            .get("runs")
                            .and_then(Value::as_array)
                            .cloned()
                            .unwrap_or_default();
                        // Numbers by value: a layer from the store holds 12.0 as 12.
                        let new_runs = new_para["runs"].as_array().cloned().unwrap_or_default();
                        let lost = emptied_links(&new_runs);
                        let old_c = comparable(&old_runs, &lost);
                        let mut new_c = field_formatting(&old_c, &comparable(&new_runs, &lost));
                        let old_c = unmark(&old_c);
                        let mut spread = 0;
                        if let Some((runs, n)) = settle_spread_links(&old_c, &new_c) {
                            new_c = runs;
                            spread = n;
                        }
                        let styled = style_colour(edited, p);
                        let (old_c, new_c) = (without_style_colour(&old_c, styled.as_deref()), without_style_colour(&new_c, styled.as_deref()));
                        let same_runs = milleleaf_core::same(&Value::Array(old_c), &Value::Array(new_c));
                        if same_runs {
                            spread_links += spread;
                        }
                        if same_runs && !lost.is_empty() {
                            lost_links += lost.len();
                        }
                        let opt = |v: Option<Value>| v.unwrap_or(Value::Null);
                        let same_style =
                            old.get("style").and_then(Value::as_str).unwrap_or("Normal") == style
                                && milleleaf_core::same(&opt(old.get("list").cloned()), &opt(new_para.get("list").cloned()))
                                && milleleaf_core::same(&opt(format_seen(old.get("format"))), &opt(format_seen(new_para.get("format"))));
                        if !(same_runs && same_style) || !children.is_empty() {
                            // Keep the original field runs' formatting where the field is unchanged.
                            let mut merged = new_para.clone();
                            if let Some(rs) = merged["runs"].as_array_mut() {
                                for r in rs.iter_mut() {
                                    if let Some(f) = r.get("field").and_then(Value::as_str)
                                        && let Some(orig) = old_runs.iter().find(|o| {
                                            o.get("field").and_then(Value::as_str) == Some(f)
                                        })
                                    {
                                        *r = orig.clone();
                                    }
                                }
                            }
                            nodes.push(Node {
                                path: c.path.clone(),
                                children,
                                attributes: Map::from_iter([("word::paragraph".into(), merged)]),
                                ..Node::default()
                            });
                            report.changed.push(c.path.clone());
                        }
                    }
                    None => {
                        let name = fresh_name("p", &para_path, &mut used_names);
                        new_children.insert(name.clone(), Some(para_path.clone()));
                        new_order.push(name);
                        nodes.push(Node {
                            path: para_path.clone(),
                            children,
                            attributes: Map::from_iter([("word::paragraph".into(), new_para)]),
                            ..Node::default()
                        });
                        report.added.push(para_path);
                    }
                }
                nodes.extend(extra_nodes);
            }
            Block::Table(t) => {
                let grid = table_grid(t);
                let tag = table_tag(t, &|p| file_comparison || is_table(p));
                let by_position = !adopting && file_comparison && tag.is_none();
                let existing = tag.as_ref()
                    .and_then(|tag| by_path.get(tag.as_str()).copied())
                    .filter(|c| c.attributes.contains_key("office::data::table"))
                    .or_else(|| by_position.then(|| node.children.get(index)).flatten()
                        .filter(|c| c.attributes.contains_key("office::data::table")));
                let existing = match existing {
                    Some(c) if seen.contains(&c.path) => {
                        duplicate(&mut report, &c.path, "table");
                        None
                    }
                    e => e,
                };
                match existing {
                    Some(c) => {
                        seen.insert(c.path.clone());
                        last_seen = child_idx.get(c.path.as_str()).copied();
                        let by = if t
                            .controls
                            .open
                            .iter()
                            .any(|c| c.tag.starts_with(TAG_PREFIX))
                        {
                            "content control tag"
                        } else {
                            "content control tag in its first cell"
                        };
                        report.matches.push(if by_position {
                            milleleaf_core::Match::best_guess(&c.path, "an untagged table in the same place")
                        } else {
                            milleleaf_core::Match::certain(&c.path, by)
                        });
                        new_order.push(c.name.clone().unwrap_or_default());
                        // Merges or the header row changed in Word.
                        let old = obj(c, "word::table").cloned().unwrap_or_default();
                        let mut wt = old.clone();
                        wt.remove("header");
                        wt.remove("merges");
                        wt.extend(table_look(t));
                        // The table's look changed in Word when what a reader sees differs from
                        // what Milleleaf would export: then the file's look comes back whole.
                        if !same_look(ours_tables.get(&c.path), &table_seen(edited, t)) {
                            let mut list_of = |num| {
                                let id = list_id(edited, num, &known_lists);
                                lists_used.entry(id.clone()).or_insert(num);
                                id
                            };
                            for (k, v) in table_style_look(edited, t, &mut list_of) {
                                wt.insert(k, v);
                            }
                        } else {
                            // Links in cells come back with the rest of the look, not on their
                            // own: say when the file's differ rather than drop them silently.
                            let ours: Vec<Value> = old.get("cells").and_then(Value::as_array).into_iter().flatten()
                                .flat_map(|r| r.as_array().cloned().unwrap_or_default())
                                .flat_map(|c| c.get("links").and_then(Value::as_array).cloned().unwrap_or_default())
                                .collect();
                            let file: Vec<Value> = t.rows.iter().flat_map(|r| &r.cells).flat_map(cell_links).collect();
                            if !milleleaf_core::same(&Value::Array(ours.clone()), &Value::Array(file.clone())) {
                                report.warnings.push(format!(
                                    "{}: the links in its cells differ from the stack's ({} in the file, {} in the stack); a link edit inside a table cell isn't read back on its own yet, so the stack's links are kept",
                                    c.path, file.len(), ours.len()
                                ));
                            }
                        }
                        if !milleleaf_core::same(&Value::Object(wt.clone()), &Value::Object(old.clone())) {
                            nodes.push(Node {
                                path: c.path.clone(),
                                attributes: Map::from_iter([("word::table".into(), Value::Object(wt))]),
                                ..Node::default()
                            });
                            report.changed.push(c.path.clone());
                        }
                        let (cols, expected) = expected_grid(c);
                        if grid != expected {
                            if is_linked(c, "office::data::table") {
                                report.conflicts.push(Conflict {
                                    path: c.path.clone(),
                                    message: "linked table was edited in the document; the source wins — edit it in the workbook, or detach the link".into(),
                                });
                            } else {
                                let mut new_cols = cols.clone();
                                for (i, col) in new_cols.iter_mut().enumerate() {
                                    if let Some(h) = grid.first().and_then(|r| r.get(i)) {
                                        col.insert("name".into(), json!(h));
                                    }
                                }
                                let rows: Vec<Value> = grid
                                    .iter()
                                    .skip(1)
                                    .map(|r| {
                                        Value::Array(
                                            r.iter()
                                                .enumerate()
                                                .map(|(i, cell)| {
                                                    let kind = new_cols
                                                        .get(i)
                                                        .and_then(|c| c.get("type"))
                                                        .and_then(Value::as_str)
                                                        .unwrap_or("text");
                                                    parse_cell(cell, kind)
                                                        .unwrap_or_else(|| json!(cell))
                                                })
                                                .collect(),
                                        )
                                    })
                                    .collect();
                                let data = json!({ "columns": new_cols, "rows": rows });
                                nodes.push(Node {
                                    path: c.path.clone(),
                                    attributes: Map::from_iter([(
                                        "office::data::table".into(),
                                        data,
                                    )]),
                                    ..Node::default()
                                });
                                report.changed.push(c.path.clone());
                            }
                        }
                    }
                    None => {
                        let path = tag.filter(|t| adopting && file_comparison && !nodes.iter().any(|n| &n.path == t)).unwrap_or_else(|| new_path(format!(
                            "{}|word-edit-table|{}|{:?}",
                            node.path,
                            index,
                            grid.first()
                        ), &nodes));
                        let header = grid.first().cloned().unwrap_or_default();
                        let mut wt = table_look(t);
                        let mut list_of = |num| {
                            let id = list_id(edited, num, &known_lists);
                            lists_used.entry(id.clone()).or_insert(num);
                            id
                        };
                        wt.extend(table_style_look(edited, t, &mut list_of));
                        let body: Vec<&Vec<String>> = grid.iter().skip(1).collect();
                        let cols: Vec<Value> = header
                            .iter()
                            .enumerate()
                            .map(|(i, h)| {
                                let cells: Vec<&str> = body
                                    .iter()
                                    .filter_map(|r| r.get(i).map(String::as_str))
                                    .collect();
                                json!({ "name": h, "type": guess_type(&cells) })
                            })
                            .collect();
                        let rows: Vec<Value> = body
                            .iter()
                            .map(|r| {
                                Value::Array(
                                    r.iter()
                                        .enumerate()
                                        .map(|(i, cell)| {
                                            let kind = cols
                                                .get(i)
                                                .and_then(|c| c.get("type"))
                                                .and_then(Value::as_str)
                                                .unwrap_or("text");
                                            parse_cell(cell, kind).unwrap_or_else(|| json!(cell))
                                        })
                                        .collect(),
                                )
                            })
                            .collect();
                        let name = fresh_name("table", &path, &mut used_names);
                        new_children.insert(name.clone(), Some(path.clone()));
                        new_order.push(name);
                        nodes.push(Node {
                            path: path.clone(),
                            attributes: Map::from_iter([
                                (
                                    "office::data::table".into(),
                                    json!({ "columns": cols, "rows": rows }),
                                ),
                                ("word::table".into(), Value::Object(wt)),
                            ]),
                            ..Node::default()
                        });
                        report.added.push(path);
                    }
                }
            }
        }
    }

    // Captions deleted from tables that are still there.
    for c in &node.children {
        if seen.contains(&c.path)
            && !captions_seen.contains(&c.path)
            && let Some(wt) = obj(c, "word::table").filter(|wt| wt.contains_key("caption"))
        {
            let mut wt2 = wt.clone();
            wt2.remove("caption");
            nodes.push(Node {
                path: c.path.clone(),
                attributes: Map::from_iter([("word::table".into(), Value::Object(wt2))]),
                ..Node::default()
            });
            report.changed.push(c.path.clone());
        }
    }
    // Removed children, and the new order.
    for c in &node.children {
        if !seen.contains(&c.path) {
            let name = c.name.clone().unwrap_or_default();
            new_children.insert(name.clone(), None);
            report.removed.push(name);
        }
    }
    let old_order: Vec<String> = node
        .children
        .iter()
        .filter_map(|c| c.name.clone())
        .collect();
    if !new_children.is_empty() || new_order != old_order {
        nodes.insert(
            0,
            Node {
                path: node.path.clone(),
                children: new_children,
                order: Some(new_order),
                ..Node::default()
            },
        );
    }
    if lost_links > 0 {
        report.warnings.push(format!(
            "{lost_links} links to an anchor in another file came back as empty HYPERLINK fields, their text unlinked (LibreOffice does this when it saves); kept the stack's links"
        ));
    }
    if spread_links > 0 {
        report.warnings.push(format!(
            "{spread_links} characters before links came back linked to them (LibreOffice extends a link back to the start of the paragraph's control when it saves); kept the stack's links"
        ));
    }
    // Definitions of lists the document doesn't have yet (a list made in Word, or adopted).
    let wd = obj(node, "word::document").cloned().unwrap_or_default();
    let known = wd.get("lists").and_then(Value::as_object).cloned().unwrap_or_default();
    let new_lists: Map<String, Value> = lists_used
        .iter()
        .filter(|(id, _)| !known.contains_key(*id))
        .filter_map(|(id, num)| Some((id.clone(), list_def(edited, *num)?)))
        .collect();
    let mut wd2 = wd.clone();
    if !new_lists.is_empty() {
        let mut all = known.clone();
        all.extend(new_lists);
        wd2.insert("lists".into(), Value::Object(all));
    }
    let ours = ours_doc.as_ref().and_then(page_setup);
    // The style sheet (WordCraft's form), when a style the document uses is defined differently:
    // an adopted file's own Normal (its font, size, alignment), or a style changed in Word.
    if let Some(od) = &ours_doc {
        let ids = used_style_names(edited);
        if used_styles(edited, &ids) != used_styles(od, &ids)
            && let Ok(v) = serde_json::to_value(&edited.styles) {
                wd2.insert("styles".into(), v);
            }
    }
    // Compared on what a reader sees (size, orientation, margins, columns): editors rewrite the
    // rest (grid, header distance) on every save.
    let seen = |v: &Option<Value>| {
        v.as_ref().map(|v| {
            let n = |k: &str| (v.get(k).and_then(Value::as_f64).unwrap_or(0.0) * 2.0).round() as i64;
            let cols = v.pointer("/columns/count").and_then(Value::as_u64);
            (n("pageW"), n("pageH"), v.get("landscape") == Some(&json!(true)), [n("marginTop"), n("marginBottom"), n("marginLeft"), n("marginRight")], cols)
        })
    };
    let page = page_setup(edited);
    if seen(&page) != seen(&ours)
        && let Some(p) = page {
            wd2.insert("page".into(), p);
        }
    // Headers and footers, and which pages they're for.
    // Compared, page kind by page kind, with what Milleleaf's own export reads back as: an
    // editor's rewrite (field switches, tab stops repeated from the style) isn't an edit. A part
    // holding something Milleleaf can't read yet (LibreOffice turns a framed page number into a
    // text frame) keeps the stack's version rather than losing what can't be seen.
    let sect = &edited.last_section;
    let mut dropped = 0;
    for (key, set) in [("headers", &sect.headers), ("footers", &sect.footers)] {
        let mut d_now = BTreeMap::new();
        let v = header_set(edited, set, &mut d_now, &mut report.pictures);
        dropped += d_now.values().sum::<usize>();
        if adopting {
            if v.is_empty() {
                wd2.remove(key);
            } else {
                wd2.insert(key.into(), Value::Object(v));
            }
            continue;
        }
        let mut d_ours = BTreeMap::new();
        let ours_v = ours_doc.as_ref().map(|od| {
            let s = &od.last_section;
            header_set(od, if key == "headers" { &s.headers } else { &s.footers }, &mut d_ours, &mut Pictures::new())
        });
        let stored = wd.get(key).and_then(Value::as_object).cloned().unwrap_or_default();
        let mut merged = Map::new();
        for which in ["default", "first", "even"] {
            let (now, ours) = (v.get(which), ours_v.as_ref().and_then(|o| o.get(which)));
            // A first-page header without a different first page, or an even-page one without
            // different odd and even pages, isn't shown: LibreOffice writes them anyway (24.2).
            let shown = match which {
                "first" => sect.title_page,
                "even" => edited.settings.even_odd_headers,
                _ => true,
            };
            let keep = if !shown || header_seen(now) == header_seen(ours) {
                true
            } else if d_now.get(which).copied().unwrap_or(0) > d_ours.get(which).copied().unwrap_or(0) {
                report.warnings.push(format!(
                    "{key} ({which}): the file holds something Milleleaf can't read there yet (a frame or shape); the stack's version is kept"
                ));
                true
            } else {
                false
            };
            if let Some(x) = if keep { stored.get(which) } else { now } {
                merged.insert(which.into(), x.clone());
            }
        }
        if merged.is_empty() {
            wd2.remove(key);
        } else {
            wd2.insert(key.into(), Value::Object(merged));
        }
    }
    for (key, on) in [("titlePage", sect.title_page), ("evenOddHeaders", edited.settings.even_odd_headers)] {
        if on {
            wd2.insert(key.into(), json!(true));
        } else {
            wd2.remove(key);
        }
    }
    // The document's own footnote and endnote separator lines (WordCraft's form, as data): a
    // template that removes the line would otherwise get it back.
    let mut seps = Map::new();
    for (k, ns) in [("footnote", &edited.footnote_separators), ("endnote", &edited.endnote_separators)] {
        if !ns.is_empty()
            && let Ok(v) = serde_json::to_value(ns) {
                seps.insert(k.into(), v);
            }
    }
    // A file with no notes may write no separators (LibreOffice): nothing to compare then.
    let ours_seps = ours_doc.as_ref().map(|od| (serde_json::to_value(&od.footnote_separators).ok(), serde_json::to_value(&od.endnote_separators).ok()));
    let now_seps = Some((serde_json::to_value(&edited.footnote_separators).ok(), serde_json::to_value(&edited.endnote_separators).ok()));
    if !adopting && (seps.is_empty() || now_seps == ours_seps) {
        // keep the stack's
    } else if seps.is_empty() {
        wd2.remove("noteSeparators");
    } else {
        wd2.insert("noteSeparators".into(), Value::Object(seps));
    }
    if dropped > 0 {
        report.warnings.push(format!(
            "headers and footers: {dropped} shape(s), chart(s), embedded object(s) or table(s) are not carried yet"
        ));
    }
    if !milleleaf_core::same(&Value::Object(wd2.clone()), &Value::Object(wd.clone())) {
        if !adopting {
            report.changed.push(node.path.clone());
        }
        nodes.push(Node {
            path: node.path.clone(),
            attributes: Map::from_iter([("word::document".into(), Value::Object(wd2))]),
            ..Node::default()
        });
    }
    // Merge statements about the same path (a paragraph changed twice, a caption and a table…).
    let mut merged: Vec<Node> = Vec::new();
    for n in nodes {
        match merged.iter_mut().find(|m| m.path == n.path) {
            Some(m) => {
                m.children.extend(n.children);
                m.inherits.extend(n.inherits);
                m.attributes.extend(n.attributes);
                if n.order.is_some() {
                    m.order = n.order;
                }
            }
            None => merged.push(n),
        }
    }
    let header = Header {
        mleaf_version: MLEAF_VERSION.into(),
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
