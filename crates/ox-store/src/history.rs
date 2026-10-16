//! Per-node history and blame, read from the index written at commit time: no layer is
//! re-read and nothing is recomposed to answer "who changed this, and when?".

use ox_core::compose::Composer;
use serde::Serialize;
use serde_json::Value;

use crate::{Result, Store, StoreError};

const DERIVED: &str = "office::data::derived";
const TABLE: &str = "office::data::table";
const VALUE: &str = "office::data::value";

/// One layer's statement about one node.
#[derive(Debug, Clone, Serialize)]
pub struct Touch {
    /// Layer hash.
    pub layer: String,
    pub id: String,
    pub author: String,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Keys this layer set or removed on the node.
    pub keys: Vec<String>,
    /// Hash of the checkpoint this layer is now part of, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_checkpoint: Option<String>,
}

/// Where one attribute of a node comes from, and how it got its value.
#[derive(Debug, Clone, Serialize)]
pub struct BlameEntry {
    pub attribute: String,
    pub value: Value,
    /// Nodes the value came through (live links, styles), outermost first.
    pub via: Vec<String>,
    /// Node and key whose history decides the value (for a derived value: the source table).
    pub origin: String,
    pub origin_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub derived: Option<Value>,
    /// Every layer that set `origin_key` on `origin`, bottom (oldest) first. The last one wins.
    pub history: Vec<Touch>,
}

impl Store {
    /// Every layer in the head that touched `path` (checkpoints expanded to the layers they
    /// replace), oldest first. With `key`, only layers that set that key.
    pub fn history(&self, path: &str, key: Option<&str>) -> Result<Vec<Touch>> {
        let mut out = Vec::new();
        for h in self.head_layers()? {
            self.collect(&h, None, path, key, &mut out)?;
        }
        Ok(out)
    }

    fn collect(
        &self,
        hash: &str,
        checkpoint: Option<&str>,
        path: &str,
        key: Option<&str>,
        out: &mut Vec<Touch>,
    ) -> Result<()> {
        let e = self.index(hash)?;
        if !e.header.replaces.is_empty() {
            for r in &e.header.replaces {
                self.collect(r, Some(checkpoint.unwrap_or(hash)), path, key, out)?;
            }
            return Ok(());
        }
        if let Some(keys) = e.touches.get(path)
            && key.is_none_or(|k| keys.iter().any(|x| x == k))
        {
            out.push(Touch {
                layer: hash.to_string(),
                id: e.header.id,
                author: e.header.author,
                timestamp: e.header.timestamp,
                kind: e.header.kind,
                keys: keys.clone(),
                in_checkpoint: checkpoint.map(str::to_string),
            });
        }
        Ok(())
    }

    /// For each attribute of `path` in the composed head (or just `attribute`): its value, the
    /// nodes it came through, and every layer that set it.
    pub fn blame(&self, path: &str, attribute: Option<&str>) -> Result<Vec<BlameEntry>> {
        let head = self.head()?.ok_or(StoreError::Empty)?;
        let flat = self.flat(&head)?;
        let mut composer = Composer::new(&flat);
        let eff = composer
            .effective(path)
            .map_err(|e| StoreError::Key(e.to_string()))?;
        let mut out = Vec::new();
        for (attr, value) in &eff.attributes {
            if attribute.is_some_and(|a| a != attr) {
                continue;
            }
            let via = eff
                .provenance
                .get(attr)
                .map(|p| p.via.clone())
                .unwrap_or_default();
            let mut origin = via.last().cloned().unwrap_or_else(|| path.to_string());
            let mut origin_key = attr.clone();
            let mut derived = None;
            // A value may be the node's own, inherited (a live link), or derived from a table;
            // walk the chain to the node that holds it. A derived value's history is its
            // source table's.
            if attr == VALUE {
                let chain = std::iter::once(path.to_string()).chain(via.iter().cloned());
                for n in chain {
                    let Some(f) = flat.nodes.get(&n) else {
                        continue;
                    };
                    if f.attributes.contains_key(VALUE) {
                        origin = n;
                        break;
                    }
                    if let Some(d) = f.attributes.get(DERIVED)
                        && let Some(src) = d.get("source").and_then(Value::as_str)
                    {
                        derived = Some(d.clone());
                        origin = src.to_string();
                        origin_key = TABLE.into();
                        break;
                    }
                }
            }
            let history = self.history(&origin, Some(&origin_key))?;
            out.push(BlameEntry {
                attribute: attr.clone(),
                value: value.clone(),
                via,
                origin,
                origin_key,
                derived,
                history,
            });
        }
        Ok(out)
    }
}
