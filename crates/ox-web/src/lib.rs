//! Byte-only browser entry points. The adapters read files; ox-core composes their layers;
//! ox-store canonicalises, signs and checks them. No filesystem or network is used here.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};

use ox_core::{Composed, Composer, Flat, Header, Layer, Node};
use ox_formats::inventory::{Counts, inventory, losses};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use wasm_bindgen::prelude::*;

type Result<T> = std::result::Result<T, String>;
const MAX_FILE: usize = 64 * 1024 * 1024;
const MAX_EXPANDED: u64 = 256 * 1024 * 1024;
const MAX_PARTS: usize = 20_000;

type Pictures = BTreeMap<String, (String, Vec<u8>)>;
mod context;
mod describe;

struct Adopted {
    root: String,
    format: String,
    hash: String,
    layer: Layer,
    counts: Counts,
    warnings: Vec<String>,
    pictures: Pictures,
    parts: BTreeMap<String, String>,
    modified: Option<String>,
}

fn header(kind: &str, author: &str, at: &str) -> Header {
    Header {
        id: format!("milleleaf/demo/{kind}"),
        ox_version: ox_core::model::OX_VERSION.into(),
        data_version: "1".into(),
        author: author.into(),
        timestamp: at.into(),
        kind: Some(kind.into()),
        ..Header::default()
    }
}

// Bound expansion before handing the same archive to the format readers.
fn parts(bytes: &[u8]) -> Result<BTreeMap<String, String>> {
    if bytes.len() > MAX_FILE {
        return Err("This demo accepts files up to 64 MiB.".into());
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    if archive.len() > MAX_PARTS {
        return Err("The file contains more than 20,000 archive parts.".into());
    }
    let mut total = 0u64;
    let mut out = BTreeMap::new();
    for i in 0..archive.len() {
        let f = archive.by_index(i).map_err(|e| e.to_string())?;
        if f.is_dir() {
            continue;
        }
        total = total.checked_add(f.size()).ok_or("Archive size overflow")?;
        if total > MAX_EXPANDED {
            return Err("The expanded file exceeds this demo's 256 MiB limit.".into());
        }
        let name = f.name().to_string();
        if out.contains_key(&name) {
            return Err(format!("Duplicate archive part: {name}"));
        }
        let limit = f.size();
        let mut data = Vec::new();
        f.take(limit + 1)
            .read_to_end(&mut data)
            .map_err(|e| e.to_string())?;
        if data.len() as u64 > limit {
            return Err(format!("Archive part exceeds its stated size: {name}"));
        }
        out.insert(name, ox_core::integrity_of(&data));
    }
    Ok(out)
}

// Core timestamps order the two files; absence leaves the browser's lastModified fallback.
fn modified(bytes: &[u8]) -> Option<String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).ok()?;
    let mut xml = String::new();
    archive
        .by_name("docProps/core.xml")
        .ok()?
        .take(MAX_EXPANDED + 1)
        .read_to_string(&mut xml)
        .ok()?;
    // Resolve the namespace instead of assuming the producer's prefix spelling.
    let namespace = "http://purl.org/dc/terms/";
    for token in xml.split("xmlns:").skip(1) {
        let (prefix, declaration) = token.split_once('=')?;
        let declaration = declaration.trim_start();
        let quote = declaration.chars().next()?;
        if !['\"', '\''].contains(&quote) {
            continue;
        }
        if declaration[1..].split(quote).next() != Some(namespace) {
            continue;
        }
        let opening = format!("<{}:modified", prefix.trim());
        if let Some(start) = xml.find(&opening) {
            let body = xml[start..].split_once('>')?.1;
            let value = body.split('<').next()?.trim();
            if !value.is_empty() {
                return Some(value.into());
            }
        }
    }
    None
}

#[wasm_bindgen]
pub fn file_metadata(bytes: &[u8]) -> std::result::Result<String, JsError> {
    parts(bytes).map_err(|e| JsError::new(&e))?;
    Ok(json!({"modified": modified(bytes)}).to_string())
}

fn read(bytes: &[u8], h: Header) -> Result<Adopted> {
    let parts = parts(bytes)?;
    let (format, counts) = inventory(bytes)?;
    let fallback = ox_core::model::stable_id(&format!("adopt|{}", ox_store::hash_of(bytes)));
    let (root, layer, warnings, pictures) = match format {
        "docx" => {
            let doc = ox_wordcraft::from_docx(bytes).map_err(|e| e.to_string())?;
            let root = ox_wordcraft::exported_path(&doc).unwrap_or(fallback);
            let (l, r) = ox_wordcraft::adopt_for_comparison(&doc, &root, "Document", h)
                .map_err(|e| e.to_string())?;
            (root, l, r.warnings, r.pictures)
        }
        "xlsx" => {
            let wb = ox_gridcraft::from_xlsx(bytes).map_err(|e| e.to_string())?;
            let root = ox_gridcraft::exported_path(&wb).unwrap_or(fallback);
            let (l, r) = ox_gridcraft::adopt_for_comparison(bytes, &root, "Document", h)
                .map_err(|e| e.to_string())?;
            (root, l, r.warnings, r.pictures)
        }
        "pptx" => {
            let deck = ox_deckcraft::from_pptx(bytes).map_err(|e| e.to_string())?;
            let root = ox_deckcraft::exported_path(&deck).unwrap_or(fallback);
            let (l, r) = ox_deckcraft::adopt_for_comparison(&deck, &root, "Document", h)
                .map_err(|e| e.to_string())?;
            (root, l, r.warnings, r.pictures)
        }
        _ => return Err("Choose a .docx, .xlsx or .pptx file.".into()),
    };
    Ok(Adopted {
        root,
        format: format.into(),
        hash: ox_core::integrity_of(bytes),
        layer,
        counts,
        warnings,
        pictures,
        parts,
        modified: modified(bytes),
    })
}

fn tree(layer: &Layer) -> Result<Composed> {
    let mut flat = Flat::default();
    flat.apply(layer);
    Composer::new(&flat)
        .compose(
            &layer
                .data
                .iter()
                .find(|n| {
                    ["word::document", "grid::workbook", "deck::deck"]
                        .iter()
                        .any(|k| n.attributes.contains_key(*k))
                })
                .ok_or("No document root")?
                .path,
            None,
            &mut Vec::new(),
        )
        .map_err(|e| e.to_string())
}

fn inventory_changes(a: &Counts, b: &Counts) -> Vec<Value> {
    a.keys().chain(b.keys()).copied().collect::<BTreeSet<_>>().into_iter().filter_map(|feature| {
        let before = a.get(feature).copied().unwrap_or(0);
        let after = b.get(feature).copied().unwrap_or(0);
        let description = if feature.contains("value,") { format!("Edited file: {feature} changed from {before} to {after}.") }
            else if after < before { format!("Edited file: {} fewer {feature} ({before} → {after}); removal or intentional edit, not proof of which app caused it.", before-after) }
            else { format!("Edited file: {} more {feature} ({before} → {after}).", after-before) };
        (before != after).then(|| json!({"feature": feature, "before": before, "after": after, "side": "edited file", "description": description,
            "status": if feature.contains("value,") { "value changed" } else if after < before { "reduced: possible loss or intentional edit" } else { "increased" }}))
    }).collect()
}

fn summary(a: &Adopted) -> Value {
    json!({"format": a.format, "hash": a.hash, "inventory": a.counts,
        "adapter_losses": losses(&a.format, &a.counts, None), "warnings": a.warnings, "modified": a.modified})
}

fn compare_file(a: &Adopted, bytes: &[u8], h: Header) -> Result<(Layer, Value)> {
    let edited_parts = parts(bytes)?;
    let (format, counts) = inventory(bytes)?;
    if format != a.format {
        return Err("The two files must have the same document format.".into());
    }
    let before = tree(&a.layer)?;
    let (mut layer, normalized, import_report, pictures) = match format {
        "docx" => {
            let d = ox_wordcraft::from_docx(bytes).map_err(|e| e.to_string())?;
            let (l, r) =
                ox_wordcraft::import_for_comparison(&before, &d, h).map_err(|e| e.to_string())?;
            (
                l.clone(),
                l,
                serde_json::to_value(&r).map_err(|e| e.to_string())?,
                r.pictures,
            )
        }
        "xlsx" => {
            let mut w = ox_gridcraft::from_xlsx(bytes).map_err(|e| e.to_string())?;
            let app = ox_formats::saved_by(bytes);
            let (mut l, mut r) =
                ox_gridcraft::import_for_comparison(&before, &mut w, h, app.as_deref())
                    .map_err(|e| e.to_string())?;
            let normalized = l.clone();
            ox_gridcraft::keep_charts(&mut l, bytes, &mut r.pictures, &mut r.warnings);
            (
                l,
                normalized,
                serde_json::to_value(&r).map_err(|e| e.to_string())?,
                r.pictures,
            )
        }
        "pptx" => {
            let p = ox_deckcraft::from_pptx(bytes).map_err(|e| e.to_string())?;
            let (l, r) =
                ox_deckcraft::import_for_comparison(&before, &p, h).map_err(|e| e.to_string())?;
            (
                l.clone(),
                l,
                serde_json::to_value(&r).map_err(|e| e.to_string())?,
                r.pictures,
            )
        }
        _ => return Err("Choose a .docx, .xlsx or .pptx file.".into()),
    };
    let mut flat = Flat::default();
    flat.apply(&a.layer);
    flat.apply(&normalized);
    let after = Composer::new(&flat)
        .compose(&a.root, None, &mut Vec::new())
        .map_err(|e| e.to_string())?;
    let mut changes = describe::describe(&before, &after, &normalized);
    context::enrich(&before, &after, &mut changes, &a.pictures, &pictures);
    for change in &mut changes {
        let matching = import_report["matches"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|m| m["path"] == change["technical"]["path"])
            .cloned();
        change["confidence"] = json!(if matching
            .as_ref()
            .is_some_and(|m| m["confidence"] == "best-guess")
        {
            "Best guess; confirm the matched place in the file"
        } else if matching.is_some() {
            "Matched by file identity"
        } else {
            "Native import comparison"
        });
        if let Some(m) = matching {
            change["technical"]["matching"] = m;
        }
    }
    let part_changes: Vec<Value> = a.parts.keys().chain(edited_parts.keys()).collect::<BTreeSet<_>>().into_iter().filter_map(|p| {
        let before = a.parts.get(p); let after = edited_parts.get(p);
        (before != after).then(|| json!({"part": p, "before": before, "after": after,
            "status": if after.is_none() {"removed"} else if before.is_none() {"added"} else {"rewritten"}}))
    }).collect();
    let mut warnings = a.warnings.clone();
    warnings.extend(
        import_report["warnings"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(String::from),
    );
    warnings.push("Inventory differences describe the edited file against the original; a reduction may be an intentional edit. Adapter limitations describe Milleleaf, not another app. Equal counts do not prove preservation.".into());
    let mut report = json!({"format": format, "original_hash": a.hash, "edited_hash": ox_core::integrity_of(bytes),
        "summary": context::summary(format, &changes), "changes": changes, "inventory_changes": inventory_changes(&a.counts, &counts),
        "inventory": {"original": a.counts, "edited": counts},
        "adapter_losses": {"original": losses(format, &a.counts, None), "edited": losses(format, &counts, None)},
        "part_changes": part_changes, "warnings": warnings, "import_report": import_report});
    layer.header.base = Some(ox_store::hash_of(&ox_store::layer_bytes(&a.layer)));
    layer.data.push(Node { path: "milleleaf-demo-evidence".into(), attributes: serde_json::Map::from_iter([
        ("demo::comparison".into(), report.clone()),
        ("demo::assets".into(), json!({"original": a.pictures.keys().collect::<Vec<_>>(), "edited": pictures.keys().collect::<Vec<_>>()})),
    ]), ..Node::default() });
    report["layer"] = serde_json::to_value(&layer).map_err(|e| e.to_string())?;
    Ok((layer, report))
}

/// Counts and adapter limitations for a file, without adopting or signing it.
#[wasm_bindgen]
pub fn loss_report(bytes: &[u8]) -> std::result::Result<String, JsError> {
    parts(bytes).map_err(|e| JsError::new(&e))?;
    let (format, counts) = inventory(bytes).map_err(|e| JsError::new(&e))?;
    Ok(json!({"format": format, "inventory": counts, "adapter_losses": losses(format, &counts, None)}).to_string())
}

#[derive(Serialize, Deserialize)]
struct Proof {
    version: String,
    baseline: Layer,
    layer: Layer,
    hash: String,
    signatures: Vec<ox_store::Signature>,
    public_keys: Vec<(String, String)>,
}

/// A session's original file, comparison and demo key. Secrets never cross the JS boundary.
#[wasm_bindgen]
#[derive(Default)]
pub struct Demo {
    original: Option<Adopted>,
    pending: Option<Layer>,
    key: Option<ox_store::KeyPair>,
}

#[wasm_bindgen]
impl Demo {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn adopt(&mut self, bytes: &[u8]) -> std::result::Result<String, JsError> {
        self.pending = None;
        let a = read(bytes, header("base", "Original file", "")).map_err(|e| JsError::new(&e))?;
        let report = summary(&a).to_string();
        self.original = Some(a);
        Ok(report)
    }

    pub fn diff(
        &mut self,
        bytes: &[u8],
        author: &str,
        at: &str,
    ) -> std::result::Result<String, JsError> {
        self.pending = None;
        let a = self
            .original
            .as_ref()
            .ok_or_else(|| JsError::new("Choose the original file first."))?;
        if author.trim().is_empty() || at.trim().is_empty() {
            return Err(JsError::new("An author label and local time are required."));
        }
        let (layer, report) = compare_file(a, bytes, header("proposal", author.trim(), at))
            .map_err(|e| JsError::new(&e))?;
        self.pending = Some(layer);
        Ok(report.to_string())
    }

    pub fn sign(&mut self) -> std::result::Result<String, JsError> {
        let layer = self
            .pending
            .as_ref()
            .ok_or_else(|| JsError::new("Compare two files before signing."))?;
        if self.key.is_none() {
            self.key = Some(
                ox_store::KeyPair::generate_demo("Session demo key")
                    .map_err(|e| JsError::new(&e.to_string()))?,
            );
        }
        let key = self.key.as_ref().expect("generated");
        let hash = ox_store::hash_of(&ox_store::layer_bytes(layer));
        let signatures = ox_store::sign_hash(&hash, key, false, &layer.header.timestamp)
            .map_err(|e| JsError::new(&e.to_string()))?;
        let public_keys = vec![
            (key.name.clone(), key.public()),
            (
                key.name.clone(),
                key.mldsa_public().expect("demo key has ML-DSA"),
            ),
        ];
        let proof = Proof {
            version: "milleleaf-demo-a/1".into(),
            baseline: self.original.as_ref().expect("adopted").layer.clone(),
            layer: layer.clone(),
            hash,
            signatures,
            public_keys,
        };
        serde_json::to_string(&proof).map_err(|e| JsError::new(&e.to_string()))
    }

    pub fn verify(&self, bytes: &[u8], now: &str) -> std::result::Result<String, JsError> {
        verify_proof(bytes, now)
            .map(|v| v.to_string())
            .map_err(|e| JsError::new(&e))
    }
}

fn verify_proof(bytes: &[u8], now: &str) -> Result<Value> {
    if bytes.len() > MAX_FILE {
        return Err("The proof exceeds 64 MiB.".into());
    }
    if now.len() < 10
        || !now.as_bytes()[..4].iter().all(u8::is_ascii_digit)
        || now.as_bytes()[4] != b'-'
        || now.as_bytes()[7] != b'-'
    {
        return Err("Verification needs the verifier's current ISO date/time.".into());
    }
    let proof: Proof = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if proof.version != "milleleaf-demo-a/1" {
        return Err("Unknown proof version.".into());
    }
    let actual = ox_store::hash_of(&ox_store::layer_bytes(&proof.layer));
    let base = ox_store::hash_of(&ox_store::layer_bytes(&proof.baseline));
    let signatures: Vec<_> = proof
        .signatures
        .iter()
        .map(|s| ox_store::check_signature(s, &actual, &proof.public_keys))
        .collect();
    let policy = ox_store::Policy::default().check(&signatures, now, &["Session demo key".into()]);
    let algorithms: BTreeSet<_> = signatures
        .iter()
        .filter(|s| s.valid)
        .map(|s| s.alg.as_str())
        .collect();
    let hybrid =
        algorithms.contains(ox_store::pq::ED25519) && algorithms.contains(ox_store::pq::MLDSA65);
    let metadata = proof
        .signatures
        .iter()
        .all(|s| s.at == proof.layer.header.timestamp && s.signer == "Session demo key");
    let valid = proof.hash == actual
        && proof.layer.header.base.as_deref() == Some(base.as_str())
        && hybrid
        && signatures.iter().all(|s| s.valid && s.known_as.is_some())
        && policy.refused.is_empty()
        && metadata;
    Ok(
        json!({"valid": valid, "hash": actual, "signatures": signatures, "policy": {"refused": policy.refused, "warnings": policy.warnings}, "verified_at": now,
        "author": proof.layer.header.author, "at": proof.layer.header.timestamp,
        "trust": "Self-contained session demo key; author is claimed and time comes from the local clock. No independent identity or timestamp authority.",
        "files": "The signed layer covers file hashes and the comparison; the original files and asset bytes are not embedded in this proof."}),
    )
}

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod tests;
