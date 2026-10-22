//! Composed tree → DocLang 0.8 (`https://www.doclang.ai/ns/v0`), the open XML format AI tools read
//! documents in (plan M2.7 item 2; mapping spiked in the project notes).
//!
//! One-way: text, headings, emphasis, links, tables with merges, slides (one page each, with their
//! frames' locations), bullet lists, charts with their data and pictures go into DocLang elements.
//! What DocLang has no element for (styles, geometry units, formulas, reviews, losses) rides along
//! as JSON in `<custom>` under the `od:` namespace, so the export says what it carries instead of
//! dropping it silently; no DocLang reader interprets it. Provenance and signatures don't travel:
//! sign the exported bytes.

use serde_json::{Map, Value, json};

use crate::compose::Composed;
use crate::render::{esc, group_thousands};

pub const NS: &str = "https://www.doclang.ai/ns/v0";
pub const VERSION: &str = "0.8";
const OD: &str = "urn:milleleaf:doclang:1";

/// The DocLang document for `roots` (in an `office::pack`'s part order when there is one, else the
/// project's), and the pictures it names. `uri` turns a picture's blob value into its `src`.
pub fn to_doclang(roots: &[Composed], generator: &str, sha256: Option<&str>, uri: &dyn Fn(&str) -> String) -> (String, Vec<String>) {
    let roots: Vec<Value> = roots.iter().map(|r| serde_json::to_value(r).unwrap_or_default()).collect();
    from_values(&roots, generator, sha256, uri)
}

/// [`to_doclang`] over composed nodes as JSON (`mleaf compose`'s `roots`).
pub fn from_values(roots: &[Value], generator: &str, sha256: Option<&str>, uri: &dyn Fn(&str) -> String) -> (String, Vec<String>) {
    let mut all = Vec::new();
    for r in roots {
        index(r, &mut all);
    }
    let find = |key: &str| all.iter().find_map(|n| attrs(n).get(key));
    let deck = find("deck::deck");
    let size = |k: &str| deck.and_then(|d| d[k].as_f64()).unwrap_or(512.0);
    let mut o = Out { parts: String::new(), assets: Vec::new(), w: size("width"), h: size("height"), uri };
    let order: Vec<&Value> = match find("office::pack") {
        Some(pack) => pack["parts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| all.iter().copied().find(|n| n["path"] == p["source"]))
            .collect(),
        None => roots.iter().flat_map(children).collect(),
    };
    for n in order {
        o.node(n);
    }
    // Annex D (informative) metadata.
    let mut head = String::from("<head>");
    if let Some(t) = find("office::project").and_then(|p| p["title"].as_str()) {
        head += &format!("<title>{}</title>", esc(t));
    }
    if let Some(h) = sha256 {
        head += &format!("<document_hash hash_function=\"sha256\">{h}</document_hash>");
    }
    head += &format!(
        "<generated_by>{}</generated_by><default_resolution width=\"{}\" height=\"{}\"/></head>",
        esc(generator),
        o.w as i64,
        o.h as i64
    );
    let xml = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<doclang xmlns=\"{NS}\" version=\"{VERSION}\">{head}{}</doclang>\n", o.parts);
    (xml, o.assets)
}

fn index<'a>(n: &'a Value, out: &mut Vec<&'a Value>) {
    out.push(n);
    for c in children(n) {
        index(c, out);
    }
}

fn children(n: &Value) -> impl Iterator<Item = &Value> {
    n["children"].as_array().into_iter().flatten()
}

fn attrs(n: &Value) -> &Map<String, Value> {
    static EMPTY: std::sync::OnceLock<Map<String, Value>> = std::sync::OnceLock::new();
    n["attributes"].as_object().unwrap_or_else(|| EMPTY.get_or_init(Map::new))
}

/// A value as shown: numbers grouped by thousands (`currency` adds `$`, `percent` scales and adds `%`).
fn fmt(v: &Value, format: &str) -> String {
    match v.as_f64() {
        Some(n) if !v.is_boolean() => match format {
            "currency" => format!("${}", group_thousands(n)),
            "percent" => format!("{}%", group_thousands(n * 100.0)),
            _ => group_thousands(n),
        },
        _ => match v {
            Value::String(s) => s.clone(),
            Value::Null => String::new(),
            other => other.to_string(),
        },
    }
}

/// `<custom>` with the node's path and the attributes (of `keys`) DocLang can't hold, as JSON.
fn custom(n: &Value, keys: &[&str]) -> String {
    let a = attrs(n);
    let kept: Map<String, Value> = keys.iter().filter_map(|k| a.get(*k).map(|v| (k.to_string(), v.clone()))).collect();
    let mut out = format!("<od:path>{}</od:path>", esc(n["path"].as_str().unwrap_or_default()));
    if !kept.is_empty() {
        out += &format!("<od:attrs>{}</od:attrs>", esc(&Value::Object(kept).to_string()));
    }
    format!("<custom xmlns:od=\"{OD}\">{out}</custom>")
}

/// Rows of strings → OTSL tokens; `merges` are `word::table` merges `{row, col, rows, cols}`.
pub fn otsl(rows: &[Vec<String>], header: usize, merges: &[Value]) -> String {
    let mut span = std::collections::HashMap::new();
    for m in merges {
        let at = |k: &str, d: u64| m[k].as_u64().unwrap_or(d) as usize;
        let (r0, c0) = (at("row", 0), at("col", 0));
        for r in r0..r0 + at("rows", 1) {
            for c in c0..c0 + at("cols", 1) {
                if (r, c) != (r0, c0) {
                    span.insert((r, c), if r > r0 && c > c0 { "xcel" } else if r > r0 { "ucel" } else { "lcel" });
                }
            }
        }
    }
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut out = String::new();
    for (i, row) in rows.iter().enumerate() {
        for j in 0..width {
            let text = row.get(j).map(String::as_str).unwrap_or("");
            match span.get(&(i, j)) {
                Some(t) => out += &format!("<{t}/>"),
                None if text.is_empty() => out += "<ecel/>",
                None => out += &format!("{}{}", if i < header { "<ched/>" } else { "<fcel/>" }, esc(text)),
            }
        }
        out += "<nl/>";
    }
    out
}

struct Out<'a> {
    parts: String,
    assets: Vec<String>,
    w: f64,
    h: f64,
    uri: &'a dyn Fn(&str) -> String,
}

const FRAME_META: [&str; 10] = [
    "deck::frame", "deck::text", "deck::table", "deck::chart", "deck::picture", "deck::shape", "mleaf::link", "mleaf::review",
    "mleaf::loss", "grid::range",
];

impl Out<'_> {
    fn w(&mut self, s: &str) {
        self.parts.push_str(s);
    }

    /// Inline runs; a live field shows the value it has now.
    fn runs(&self, runs: &Value, owner: &Value) -> String {
        let mut out = String::new();
        for r in runs.as_array().into_iter().flatten() {
            let mut t = match r["field"].as_str() {
                Some(field) => {
                    let f = children(owner).find(|c| c["name"] == field).map(attrs);
                    let v = f.and_then(|f| f.get("office::data::value")).cloned().unwrap_or(json!({}));
                    let format = f
                        .and_then(|f| f.get("word::field"))
                        .and_then(|w| w["format"].as_str())
                        .or(v["format"].as_str())
                        .unwrap_or("");
                    esc(&fmt(&v["value"], format))
                }
                None => esc(r["text"].as_str().unwrap_or_default()),
            };
            for (flag, tag) in [("underline", "underline"), ("italic", "italic"), ("bold", "bold")] {
                if r[flag].as_bool() == Some(true) {
                    t = format!("<{tag}>{t}</{tag}>");
                }
            }
            if let Some(link) = r["link"].as_str() {
                t = format!("<text><href uri=\"{}\"/>{t}</text>", esc(link));
            }
            out += &t;
        }
        // Plain text keeps its whitespace in <content>; mixed content can't be wrapped.
        if !out.is_empty() && !out.contains('<') { format!("<content>{out}</content>") } else { out }
    }

    fn para(&mut self, n: &Value, p: &Value, head: &str) {
        let style = p["style"].as_str().unwrap_or_default();
        let body = self.runs(&p["runs"], n);
        let level = style.chars().last().filter(char::is_ascii_digit);
        if style.to_lowercase().starts_with("heading") && level.is_some() {
            self.w(&format!("<heading level=\"{}\">{head}{body}</heading>", level.unwrap_or('1')));
        } else if style.eq_ignore_ascii_case("title") {
            self.w(&format!("<heading>{head}{body}</heading>"));
        } else {
            self.w(&format!("<text>{head}{body}</text>"));
        }
    }

    /// Element head order: location, caption, …, custom.
    fn table(&mut self, n: &Value, caption: Option<&str>, cus: &str, loc: &str) {
        let a = attrs(n);
        let (rows, header) = match a.get("office::data::table") {
            Some(d) => {
                let cols = d["columns"].as_array().cloned().unwrap_or_default();
                let mut rows = vec![cols.iter().map(|c| c["name"].as_str().unwrap_or_default().to_string()).collect()];
                for r in d["rows"].as_array().into_iter().flatten() {
                    let cells = r.as_array().into_iter().flatten();
                    rows.push(cells.zip(&cols).map(|(v, c)| fmt(v, c["type"].as_str().unwrap_or(""))).collect());
                }
                (rows, 1)
            }
            // A deck table without a linked data table: its snapshot text.
            None => {
                let t = a.get("deck::table").cloned().unwrap_or(json!({}));
                let rows = t["snapshot"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|r| r.as_array().into_iter().flatten().map(|c| c["text"].as_str().unwrap_or_default().to_string()).collect())
                    .collect();
                (rows, usize::from(t["props"]["firstRow"].as_bool() == Some(true)))
            }
        };
        let merges = a.get("word::table").and_then(|t| t["merges"].as_array()).cloned().unwrap_or_default();
        let cap = caption.map(|c| format!("<caption>{}</caption>", esc(c))).unwrap_or_default();
        self.w(&format!("<table>{loc}{cap}{cus}{}</table>", otsl(&rows, header, &merges)));
    }

    fn group(&mut self, label: &str, cus: &str, heading: Option<String>, n: &Value) {
        self.w(&format!("<group><label value=\"{label}\"/>{cus}"));
        if let Some(h) = heading {
            self.w(&h);
        }
        self.children(n);
        self.w("</group>");
    }

    fn node(&mut self, n: &Value) {
        let a = attrs(n);
        let cus = |keys: &[&str]| custom(n, keys);
        if let Some(d) = a.get("word::document") {
            let h = format!("<heading>{}</heading>", esc(d["title"].as_str().unwrap_or_default()));
            self.group("word_document", &cus(&["word::document"]), Some(h), n);
        } else if let Some(p) = a.get("word::paragraph") {
            let head = if p.get("list").is_some() { cus(&["word::paragraph"]) } else { String::new() };
            self.para(n, p, &head);
        } else if let Some(t) = a.get("word::table") {
            self.table(n, t["caption"].as_str(), &cus(&["word::table", "mleaf::link", "mleaf::review", "grid::range"]), "");
        } else if a.contains_key("grid::workbook") {
            self.group("workbook", &cus(&["grid::workbook"]), None, n);
        } else if let Some(s) = a.get("grid::sheet") {
            let h = format!("<heading level=\"2\">{}</heading>", esc(s["name"].as_str().unwrap_or_default()));
            self.group("sheet", &cus(&["grid::sheet"]), Some(h), n);
        } else if a.contains_key("office::data::table") && a.contains_key("grid::range") && !a.contains_key("deck::frame") {
            let r = &a["grid::range"];
            let cap = format!("{}!{}", r["sheet"].as_str().unwrap_or_default(), r["ref"].as_str().unwrap_or_default());
            self.table(n, Some(&cap), &cus(&["grid::range", "mleaf::review"]), "");
        } else if let Some(v) = a.get("office::data::value").filter(|_| !a.contains_key("deck::frame")) {
            let at = a.get("grid::range").and_then(|r| r["ref"].as_str()).or(n["name"].as_str()).unwrap_or_default();
            let keys = ["grid::range", "grid::formula", "office::data::derived", "office::data::value"];
            self.w(&format!(
                "<field_region>{}<field_item><key>{}</key><value>{}</value></field_item></field_region>",
                cus(&keys),
                esc(at),
                esc(&fmt(&v["value"], v["format"].as_str().unwrap_or("")))
            ));
        } else if let Some(d) = a.get("deck::deck") {
            self.w(&format!(
                "<group><label value=\"deck\"/>{}<heading>{}</heading></group>",
                cus(&["deck::deck", "mleaf::source"]),
                esc(d["title"].as_str().unwrap_or_default())
            ));
            // One page per slide; locations are in the deck's own pixel grid.
            for s in children(n) {
                self.w("<page_break/>");
                self.node(s);
            }
        } else if a.contains_key("deck::slide") {
            // Slide-level properties (background, notes, losses) ride on a furniture group.
            self.w(&format!("<group><layer value=\"furniture\"/>{}</group>", cus(&["deck::slide", "mleaf::loss", "mleaf::match"])));
            let mut frames: Vec<&Value> = children(n).collect();
            frames.sort_by(|x, y| {
                let z = |f: &Value| attrs(f).get("deck::frame").and_then(|f| f["z"].as_f64()).unwrap_or(0.0);
                z(x).total_cmp(&z(y))
            });
            for f in frames {
                self.node(f);
            }
        } else if a.contains_key("deck::frame") {
            self.frame(n);
        } else if a.is_empty() || a.contains_key("office::pack") || a.contains_key("office::project") {
            self.children(n);
        } else {
            // Anything else: say it wasn't carried.
            let keys: Vec<&str> = a.keys().map(String::as_str).collect();
            self.w(&format!("<group><layer value=\"furniture\"/>{}</group>", cus(&keys)));
        }
    }

    fn children(&mut self, n: &Value) {
        for c in children(n) {
            self.node(c);
        }
    }

    fn frame(&mut self, n: &Value) {
        let a = attrs(n);
        let f = &a["deck::frame"];
        let g = |k: &str| f[k].as_f64().unwrap_or(0.0);
        let clamp = |v: f64, lim: f64| v.round().clamp(0.0, (lim - 1.0).max(0.0)) as i64;
        let loc: String = [(g("x"), self.w), (g("y"), self.h), (g("x") + g("w"), self.w), (g("y") + g("h"), self.h)]
            .iter()
            .map(|&(v, lim)| format!("<location value=\"{}\"/>", clamp(v, lim)))
            .collect();
        let cus = custom(n, &FRAME_META);
        if let Some(t) = a.get("deck::text") {
            let paras = t["paragraphs"].as_array().cloned().unwrap_or_default();
            let role = t["role"].as_str().unwrap_or_default();
            if matches!(role, "title" | "subtitle") && paras.len() == 1 {
                let lvl = if role == "title" { "" } else { " level=\"2\"" };
                let body = self.runs(&paras[0]["runs"], n);
                self.w(&format!("<heading{lvl}>{loc}{cus}{body}</heading>"));
            } else if paras.iter().any(|p| p["bullet"].as_bool() == Some(true) || p["numbered"].as_bool() == Some(true)) {
                let list = self.deck_list(&paras, n);
                self.w(&format!("<group>{loc}{cus}{list}</group>"));
            } else {
                let body: String = paras.iter().map(|p| format!("<text>{}</text>", self.runs(&p["runs"], n))).collect();
                self.w(&format!("<group>{loc}{cus}{body}</group>"));
            }
        } else if a.contains_key("deck::table") {
            self.table(n, None, &cus, &loc);
        } else if let Some(c) = a.get("deck::chart") {
            let d = a.get("office::data::table").cloned().unwrap_or(json!({"columns": [], "rows": []}));
            let label = match c["kind"].as_str() {
                Some("column" | "bar") => "bar_chart",
                Some("line") => "line_chart",
                Some("pie") => "pie_chart",
                Some("scatter") => "scatter_plot",
                _ => "other",
            };
            let mut rows = vec![d["columns"].as_array().into_iter().flatten().map(|x| x["name"].as_str().unwrap_or_default().to_string()).collect()];
            for r in d["rows"].as_array().into_iter().flatten() {
                rows.push(r.as_array().into_iter().flatten().map(|v| fmt(v, "")).collect());
            }
            let cap = c["title"].as_str().map(|t| format!("<caption>{}</caption>", esc(t))).unwrap_or_default();
            self.w(&format!(
                "<picture class=\"chart\"><label value=\"{label}\"/>{loc}{cap}{cus}<tabular>{}</tabular></picture>",
                otsl(&rows, 1, &[])
            ));
        } else if let Some(p) = a.get("deck::picture") {
            let blob = p["blob"].as_str().unwrap_or_default();
            if !self.assets.iter().any(|b| b == blob) {
                self.assets.push(blob.to_string());
            }
            let desc = p["alt"].as_str().map(|t| format!("<description>{}</description>", esc(t))).unwrap_or_default();
            self.w(&format!("<picture>{loc}{desc}{cus}<src uri=\"{}\"/></picture>", esc(&(self.uri)(blob))));
        } else {
            // Shapes, lines, vectors: decoration with no DocLang element.
            self.w(&format!("<group><layer value=\"furniture\"/>{loc}{cus}</group>"));
        }
    }

    /// Bulleted paragraphs → nested `<list>`s by level.
    fn deck_list(&self, paras: &[Value], owner: &Value) -> String {
        let (mut out, mut depth) = (String::new(), -1i64);
        for p in paras {
            let lvl = p["level"].as_i64().unwrap_or(0);
            while depth < lvl {
                out += if p["numbered"].as_bool() == Some(true) { "<list class=\"ordered\">" } else { "<list>" };
                depth += 1;
            }
            while depth > lvl {
                out += "</list>";
                depth -= 1;
            }
            out += &format!("<ldiv/>{}", self.runs(&p["runs"], owner));
        }
        out + &"</list>".repeat((depth + 1) as usize)
    }
}

/// The OPC parts a `.dclx` package needs besides `document.xml` and `assets/` (spec Annex B, as
/// `doclang pack` 0.8.0 writes them).
pub const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="png" ContentType="image/png"/>
  <Default Extension="jpg" ContentType="image/jpeg"/>
  <Default Extension="jpeg" ContentType="image/jpeg"/>
  <Default Extension="gif" ContentType="image/gif"/>
  <Default Extension="webp" ContentType="image/webp"/>
  <Default Extension="svg" ContentType="image/svg+xml"/>
  <Default Extension="bin" ContentType="application/octet-stream"/>
  <Override PartName="/document.xml" ContentType="application/vnd.doclang.document+xml"/>
</Types>
"#;
pub const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://doclang.ai/ns/package/2026/relationships/document" Target="document.xml"/>
</Relationships>
"#;

/// A picture's file extension from its first bytes (`bin` when unknown).
pub fn picture_ext(bytes: &[u8]) -> &'static str {
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => "png",
        [0xFF, 0xD8, 0xFF, ..] => "jpg",
        [b'G', b'I', b'F', b'8', ..] => "gif",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "webp",
        b if b.starts_with(b"<svg") || b.starts_with(b"<?xml") => "svg",
        _ => "bin",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_links_merges_and_pictures_map_to_doclang() {
        let roots = [json!({"path": "p", "children": [
            {"path": "d", "attributes": {"word::document": {"title": "T"}}, "children": [
                {"path": "h", "attributes": {"word::paragraph": {"style": "Heading 2", "runs": [{"text": "H & co"}]}}},
                {"path": "l", "attributes": {"word::paragraph": {"style": "Normal", "runs": [
                    {"text": "see "}, {"text": "site", "link": "https://example.com", "bold": true}]}}},
                {"path": "t", "attributes": {"word::table": {"merges": [{"row": 1, "col": 0, "rows": 2, "cols": 2}]},
                    "office::data::table": {"columns": [{"name": "A"}, {"name": "B", "type": "currency"}],
                                            "rows": [["x", 1250500], ["", ""]]}}}]},
            {"path": "k", "attributes": {"deck::deck": {"title": "D", "width": 960, "height": 540}}, "children": [
                {"path": "s", "attributes": {"deck::slide": {}}, "children": [
                    {"path": "f", "attributes": {"deck::frame": {"x": -5, "y": 10, "w": 2000, "h": 20},
                                                 "deck::picture": {"blob": "sha256-ab", "alt": "logo"}}}]}]}]})];
        let (xml, blobs) = from_values(&roots, "mleaf test", Some("00"), &|b| format!("assets/{b}.png"));
        assert!(xml.starts_with("<?xml") && xml.contains(&format!("<doclang xmlns=\"{NS}\" version=\"0.8\">")), "{xml}");
        assert!(xml.contains("<heading level=\"2\"><content>H &amp; co</content></heading>"), "{xml}");
        assert!(xml.contains("<text><href uri=\"https://example.com\"/><bold>site</bold></text>"), "{xml}");
        // Merged cells: the covered ones are spans, not empty cells.
        assert!(xml.contains("<ched/>A<ched/>B<nl/><fcel/>x<lcel/><nl/><ucel/><xcel/><nl/>"), "{xml}");
        assert_eq!(fmt(&json!(1250500), "currency"), "$1,250,500");
        // Locations are clamped to the slide's pixel grid.
        assert!(xml.contains("<page_break/>") && xml.contains("<location value=\"0\"/><location value=\"10\"/><location value=\"959\"/>"), "{xml}");
        assert!(xml.contains("<description>logo</description>") && xml.contains("<src uri=\"assets/sha256-ab.png\"/>"), "{xml}");
        assert_eq!(blobs, vec!["sha256-ab"]);
        assert!(xml.contains("<document_hash hash_function=\"sha256\">00</document_hash>"));
        assert_eq!(picture_ext(b"\x89PNG\r\n"), "png");
    }

    #[test]
    fn otsl_marks_spans() {
        let rows = vec![vec!["a".into(), "".into()], vec!["".into(), "".into()]];
        let m = [json!({"row": 0, "col": 0, "rows": 2, "cols": 2})];
        assert_eq!(otsl(&rows, 0, &m), "<fcel/>a<lcel/><nl/><ucel/><xcel/><nl/>");
    }
}
