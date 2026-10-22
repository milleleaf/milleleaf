//! `mleaf search`: find text in a project, on this machine, after decryption (plan M4 item 5; the maintainer,
//! 18 Oct 2026: "mleaf search + sealed index").
//!
//! The head is searched in its composed form, so a hit is what the project says now. `--history`
//! searches every stored layer, superseded values included, through a per-layer index in
//! `.mleaf/cache/search/<hash>.json` (term → node paths), built the first time it's needed. A sealed
//! layer's index is encrypted under that layer's own content key, so only its recipients can read
//! it, and a key purge shreds it with the layer; a purge also drops the whole cache.
//!
//! Terms are lowercased runs of letters and digits; each CJK character is a term of its own (no
//! spaces between words), and a CJK query must also appear as written. Every hit is checked
//! against the text itself, so the index only narrows where to look.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use milleleaf_core::compose::{Composed, Composer};
use milleleaf_core::model::Layer;
use serde::Serialize;
use serde_json::Value;

use crate::{Opened, Result, Store, StoreError, io};

/// One place the query matched.
#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    /// `header.id` of the layer that set the value.
    pub layer: String,
    /// The stored layer's hash (`--history` only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    /// The layer's timestamp (`--history` only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    pub path: String,
    pub attribute: String,
    /// The matching text, cut to about 120 characters around the first match.
    pub text: String,
}

/// What a search found, and the sealed layers it couldn't look in.
#[derive(Debug, Default, Serialize)]
pub struct Found {
    pub hits: Vec<Hit>,
    /// Hashes of sealed layers no local key opens (`--history`; the head's are noted on stderr).
    pub locked: Vec<String>,
}

type Index = BTreeMap<String, BTreeSet<String>>;

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
}

/// Search terms of a text.
pub fn terms(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut word = String::new();
    for c in text.chars().flat_map(char::to_lowercase) {
        if is_cjk(c) {
            out.insert(c.to_string());
        }
        if c.is_alphanumeric() && !is_cjk(c) {
            word.push(c);
        } else if !word.is_empty() {
            out.insert(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        out.insert(word);
    }
    out
}

/// Whether `text` holds every term of the query, and each CJK run of it as written.
fn matches(text: &str, query: &BTreeSet<String>, cjk_runs: &[String]) -> bool {
    let have = terms(text);
    let lower = text.to_lowercase();
    query.is_subset(&have) && cjk_runs.iter().all(|r| lower.contains(r.as_str()))
}

fn cjk_runs(q: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut run = String::new();
    for c in q.chars().flat_map(char::to_lowercase) {
        if is_cjk(c) {
            run.push(c);
        } else if !run.is_empty() {
            out.push(std::mem::take(&mut run));
        }
    }
    if !run.is_empty() {
        out.push(run);
    }
    out
}

/// Every string in a value, joined by spaces.
fn text_of(v: &Value) -> String {
    fn walk(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::String(s) => out.push(s.clone()),
            Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            Value::Object(m) => m.values().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(v, &mut out);
    out.join(" ")
}

/// About 120 characters of `text` around the first query term.
fn snippet(text: &str, query: &BTreeSet<String>, cjk: &[String]) -> String {
    let chars: Vec<char> = text.chars().collect();
    let lower: Vec<char> = text.chars().map(|c| c.to_lowercase().next().unwrap_or(c)).collect();
    let lower_s: String = lower.iter().collect();
    let first = cjk
        .iter()
        .chain(query.iter())
        .filter_map(|t| lower_s.find(t.as_str()).map(|b| lower_s[..b].chars().count()))
        .min()
        .unwrap_or(0);
    let start = first.saturating_sub(50);
    let end = (start + 120).min(chars.len());
    let mut s: String = chars[start..end].iter().collect();
    if start > 0 {
        s.insert(0, '…');
    }
    if end < chars.len() {
        s.push('…');
    }
    s
}

fn layer_index(l: &Layer) -> Index {
    let mut ix = Index::new();
    for n in &l.data {
        for v in n.attributes.values() {
            for t in terms(&text_of(v)) {
                ix.entry(t).or_default().insert(n.path.clone());
            }
        }
    }
    ix
}

fn index_aad(key_id: &str, layer_id: &str) -> Vec<u8> {
    format!("milleleaf search index v1\n{key_id}\n{layer_id}").into_bytes()
}

/// Indexes sealed before the rename to Milleleaf (23 Oct 2026) used this label; they still open.
fn old_index_aad(key_id: &str, layer_id: &str) -> Vec<u8> {
    format!("Milleleaf search index v1\n{key_id}\n{layer_id}").into_bytes()
}

impl Store {
    /// Finds `query` in the head, composed: what the project says now.
    pub fn search_head(&self, query: &str) -> Result<Found> {
        let (q, cjk) = (terms(query), cjk_runs(query));
        let mut found = Found::default();
        if q.is_empty() {
            return Ok(found);
        }
        let Some(head) = self.head()? else { return Ok(found) };
        let flat = self.flat(&head)?;
        let trees = Composer::new(&flat).compose_all().map_err(|e| StoreError::Key(e.to_string()))?;
        fn walk(n: &Composed, q: &BTreeSet<String>, cjk: &[String], out: &mut Vec<Hit>) {
            for (k, v) in &n.attributes {
                let text = text_of(v);
                if matches(&text, q, cjk) {
                    out.push(Hit {
                        layer: n.provenance.get(k).map(|p| p.layer.clone()).unwrap_or_default(),
                        hash: None,
                        at: None,
                        path: n.path.clone(),
                        attribute: k.clone(),
                        text: snippet(&text, q, cjk),
                    });
                }
            }
            n.children.iter().for_each(|c| walk(c, q, cjk, out));
        }
        trees.iter().for_each(|t| walk(t, &q, &cjk, &mut found.hits));
        Ok(found)
    }

    /// Finds `query` in every stored layer, oldest first, superseded values included.
    pub fn search_history(&self, query: &str) -> Result<Found> {
        let (q, cjk) = (terms(query), cjk_runs(query));
        let mut found = Found::default();
        if q.is_empty() {
            return Ok(found);
        }
        let mut layers: Vec<(String, String)> = fs::read_dir(self.dir.join("layers"))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.file_name().to_string_lossy().strip_suffix(".json").map(str::to_string))
            .filter_map(|h| self.index(&h).ok().map(|i| (i.header.timestamp, h)))
            .collect();
        layers.sort();
        for (at, h) in layers {
            let Some(ix) = self.search_index(&h)? else {
                if self.index(&h)?.header.sealed.is_some() && self.purged(&h).is_none() {
                    found.locked.push(h);
                }
                continue;
            };
            // Nodes holding every term, then each checked against its text.
            let mut paths: Option<BTreeSet<String>> = None;
            for t in &q {
                let p = ix.get(t).cloned().unwrap_or_default();
                paths = Some(match paths {
                    None => p,
                    Some(prev) => prev.intersection(&p).cloned().collect(),
                });
            }
            let paths = paths.unwrap_or_default();
            if paths.is_empty() {
                continue;
            }
            let (l, _) = self.open_layer(&h)?;
            for n in l.data.iter().filter(|n| paths.contains(&n.path)) {
                for (k, v) in &n.attributes {
                    let text = text_of(v);
                    if matches(&text, &q, &cjk) {
                        found.hits.push(Hit {
                            layer: l.header.id.clone(),
                            hash: Some(h.clone()),
                            at: Some(at.clone()),
                            path: n.path.clone(),
                            attribute: k.clone(),
                            text: snippet(&text, &q, &cjk),
                        });
                    }
                }
            }
        }
        Ok(found)
    }

    /// A layer's search index, from the cache or built now; `None` when its content can't be read
    /// here (purged, or sealed and no local key opens it).
    fn search_index(&self, hash: &str) -> Result<Option<Index>> {
        if self.purged(hash).is_some() {
            return Ok(None);
        }
        let p = self.dir.join("cache").join("search").join(format!("{hash}.json"));
        let sealed = self.index(hash)?.header.sealed;
        if let Ok(bytes) = fs::read(&p) {
            let cached = match &sealed {
                None => serde_json::from_slice::<Index>(&bytes).ok(),
                Some(s) => serde_json::from_slice::<Value>(&bytes)
                    .ok()
                    .and_then(|v| B64.decode(v["sealed"].as_str()?).ok())
                    .and_then(|ct| {
                        let k = self.content_key(&s.key)?;
                        {
                            let id = self.index(hash).ok()?.header.id;
                            milleleaf_seal::decrypt(&k, &index_aad(&s.key, &id), &ct)
                                .or_else(|_| milleleaf_seal::decrypt(&k, &old_index_aad(&s.key, &id), &ct))
                                .ok()
                        }
                    })
                    .and_then(|pt| serde_json::from_slice::<Index>(&pt).ok()),
            };
            if cached.is_some() {
                return Ok(cached);
            }
        }
        let (l, opened) = self.open_layer(hash)?;
        if let Opened::Locked(_) = opened {
            return Ok(None);
        }
        let ix = layer_index(&l);
        let json = serde_json::to_vec(&ix).expect("json");
        let bytes = match (&sealed, opened) {
            (Some(s), Opened::Decrypted) => {
                let k = self.content_key(&s.key).ok_or_else(|| StoreError::Key("content key went away".into()))?;
                let ct = milleleaf_seal::encrypt(&k, &index_aad(&s.key, &l.header.id), &json).map_err(|e| StoreError::Key(e.to_string()))?;
                serde_json::to_vec(&serde_json::json!({"sealed": B64.encode(ct)})).expect("json")
            }
            _ => json,
        };
        let d = p.parent().expect("parent");
        io(d, fs::create_dir_all(d))?;
        // ponytail: a plain write; two searches at once write the same bytes.
        io(&p, fs::write(&p, bytes))?;
        Ok(Some(ix))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terms_split_words_and_cjk_characters() {
        let t = terms("Q3 Revenue, 香港 office");
        assert_eq!(t, ["q3", "revenue", "香", "港", "office"].iter().map(|s| s.to_string()).collect());
        let q = terms("香港");
        assert!(matches("香港 office", &q, &cjk_runs("香港")));
        // Both characters, but not as written: no match.
        assert!(!matches("港 and 香", &q, &cjk_runs("香港")));
        assert!(!matches("revenues", &terms("revenue"), &[]), "whole words only");
        assert_eq!(snippet("abc", &terms("b"), &[]), "abc");
    }
}
