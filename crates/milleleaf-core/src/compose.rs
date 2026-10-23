//! Composition, in three phases (the layered model's credits are in `model.rs`):
//!
//! 1. **Federate**: take every layer's nodes in stack order (bottom first).
//! 2. **Flatten**: one [`FlatNode`] per path; later statements win, `null` deletes.
//! 3. **Compose**: expand `inherits` and build the ordered tree from the roots.
//!
//! Every attribute keeps its provenance — the layer that set it and, when it arrived through
//! `inherits`, the node it came from — so any number in a report or slide can answer "where
//! did this come from?".

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::MleafError;
use crate::model::{Layer, Schema};
use crate::stack::LoadedLayer;

/// Where an attribute's value came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Provenance {
    /// `header.id` of the layer that set the value.
    pub layer: String,
    /// Path of the node it was inherited from, if it was inherited (outermost first).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub via: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FlatNode {
    pub path: String,
    pub children: BTreeMap<String, String>,
    pub order: Option<Vec<String>>,
    pub inherits: BTreeMap<String, String>,
    pub attributes: Map<String, Value>,
    pub provenance: BTreeMap<String, Provenance>,
}

/// The stack after flattening, plus the merged schemas.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Flat {
    pub nodes: BTreeMap<String, FlatNode>,
    pub schemas: BTreeMap<String, Schema>,
    /// Layer ids, bottom first.
    pub layers: Vec<String>,
}

pub fn flatten(stack: &[LoadedLayer]) -> Flat {
    let mut flat = Flat::default();
    for loaded in stack {
        flat.apply(&loaded.layer);
    }
    flat
}

impl Flat {
    /// Applies one more layer on top. `flatten` is this, bottom layer first, so a flattened
    /// stack can be cached and extended one layer at a time.
    pub fn apply(&mut self, layer: &Layer) {
        let layer_id = &layer.header.id;
        self.layers.push(layer_id.clone());
        for (k, s) in &layer.schemas {
            self.schemas.insert(k.clone(), s.clone());
        }
        for node in &layer.data {
            let f = self
                .nodes
                .entry(node.path.clone())
                .or_insert_with(|| FlatNode {
                    path: node.path.clone(),
                    ..FlatNode::default()
                });
            for (name, target) in &node.children {
                match target {
                    Some(p) => f.children.insert(name.clone(), p.clone()),
                    None => f.children.remove(name),
                };
            }
            for (name, target) in &node.inherits {
                match target {
                    Some(p) => f.inherits.insert(name.clone(), p.clone()),
                    None => f.inherits.remove(name),
                };
            }
            if let Some(order) = &node.order {
                f.order = Some(order.clone());
            }
            for (key, value) in &node.attributes {
                if value.is_null() {
                    f.attributes.remove(key);
                    f.provenance.remove(key);
                } else {
                    f.attributes.insert(key.clone(), value.clone());
                    // A checkpoint carries the id of the layer that originally set the value.
                    let by = node.provenance.get(key).unwrap_or(layer_id);
                    f.provenance.insert(
                        key.clone(),
                        Provenance {
                            layer: by.clone(),
                            via: vec![],
                        },
                    );
                }
            }
        }
    }

    /// The flattened state as one layer: what `mleaf checkpoint` writes. Applying it to an empty
    /// `Flat` gives back these nodes, schemas and provenance.
    pub fn to_layer(&self, header: crate::model::Header) -> Layer {
        let data = self
            .nodes
            .values()
            .map(|n| crate::model::Node {
                path: n.path.clone(),
                children: n
                    .children
                    .iter()
                    .map(|(k, v)| (k.clone(), Some(v.clone())))
                    .collect(),
                order: n.order.clone(),
                inherits: n
                    .inherits
                    .iter()
                    .map(|(k, v)| (k.clone(), Some(v.clone())))
                    .collect(),
                attributes: n.attributes.clone(),
                provenance: n
                    .provenance
                    .iter()
                    .map(|(k, p)| (k.clone(), p.layer.clone()))
                    .collect(),
            })
            .collect();
        Layer {
            header,
            imports: vec![],
            schemas: self.schemas.clone(),
            data,
        }
    }
}

const DERIVED: &str = "office::data::derived";
const TABLE: &str = "office::data::table";
const VALUE: &str = "office::data::value";

/// Computes an `office::data::derived` value from the source node's `office::data::table`:
/// `rowCount`, or `sum` / `average` / `min` / `max` of a named column.
pub fn derive(spec: &Map<String, Value>, source: &Effective) -> Option<Value> {
    let table = source.attributes.get(TABLE)?.as_object()?;
    let rows = table.get("rows")?.as_array()?;
    let op = spec.get("op").and_then(Value::as_str)?;
    let columns = table
        .get("columns")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let col = |name: &str| {
        columns
            .iter()
            .position(|c| c.get("name").and_then(Value::as_str) == Some(name))
    };
    let (n, natural_format) = if op == "rowCount" {
        (rows.len() as f64, "number")
    } else {
        let name = spec.get("column").and_then(Value::as_str)?;
        let i = col(name)?;
        let values: Vec<f64> = rows
            .iter()
            .filter_map(|r| r.get(i).and_then(Value::as_f64))
            .collect();
        let fmt = match columns
            .get(i)
            .and_then(|c| c.get("type"))
            .and_then(Value::as_str)
        {
            Some("currency") => "currency",
            Some("percent") => "percent",
            _ => "number",
        };
        let v = match op {
            "sum" => values.iter().sum(),
            "average" if !values.is_empty() => values.iter().sum::<f64>() / values.len() as f64,
            "min" => values.iter().copied().reduce(f64::min)?,
            "max" => values.iter().copied().reduce(f64::max)?,
            _ => return None,
        };
        (v, fmt)
    };
    let format = spec
        .get("format")
        .and_then(Value::as_str)
        .unwrap_or(natural_format);
    let num = if n.fract() == 0.0 && n.abs() < 1e15 {
        serde_json::json!(n as i64)
    } else {
        serde_json::json!(n)
    };
    Some(serde_json::json!({ "value": num, "format": format }))
}

/// A node with its inherits expanded, before the tree is built.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Effective {
    pub path: String,
    pub attributes: Map<String, Value>,
    pub provenance: BTreeMap<String, Provenance>,
    pub children: BTreeMap<String, String>,
    pub order: Option<Vec<String>>,
    /// Paths this node inherits from, directly or indirectly.
    pub sources: Vec<String>,
    /// Every path this result was computed from (inherit sources and derived-value sources,
    /// directly or indirectly), for invalidation.
    #[serde(skip)]
    pub depends: BTreeSet<String>,
}

/// A node in the composed tree.
#[derive(Debug, Clone, Serialize)]
pub struct Composed {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub attributes: Map<String, Value>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub provenance: BTreeMap<String, Provenance>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Composed>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Warning {
    pub path: String,
    pub message: String,
}

pub struct Composer<'a> {
    flat: &'a Flat,
    memo: HashMap<String, Effective>,
    pub warnings: Vec<Warning>,
}

impl<'a> Composer<'a> {
    pub fn new(flat: &'a Flat) -> Self {
        Self {
            flat,
            memo: HashMap::new(),
            warnings: Vec::new(),
        }
    }

    /// Moves the composer onto `flat` (the same stack with more layers applied), keeping every
    /// computed node except those in `touched` and the nodes computed from them. Returns how many
    /// were dropped. This is incremental compose: after a commit, only what the new layer
    /// touched is recomputed.
    pub fn rebase<'b>(self, flat: &'b Flat, touched: &BTreeSet<String>) -> (Composer<'b>, usize) {
        let before = self.memo.len();
        let memo: HashMap<String, Effective> = self
            .memo
            .into_iter()
            .filter(|(p, e)| !touched.contains(p) && e.depends.is_disjoint(touched))
            .collect();
        let dropped = before - memo.len();
        (
            Composer {
                flat,
                memo,
                warnings: Vec::new(),
            },
            dropped,
        )
    }

    /// Number of nodes computed so far (cached).
    pub fn cached(&self) -> usize {
        self.memo.len()
    }

    /// The node at `path` with its inherits expanded. Own values override inherited ones; among
    /// several inherits, later names (sorted) override earlier ones.
    pub fn effective(&mut self, path: &str) -> Result<Effective, MleafError> {
        let mut visiting = Vec::new();
        self.effective_inner(path, &mut visiting)
    }

    fn effective_inner(
        &mut self,
        path: &str,
        visiting: &mut Vec<String>,
    ) -> Result<Effective, MleafError> {
        if let Some(e) = self.memo.get(path) {
            return Ok(e.clone());
        }
        if visiting.iter().any(|p| p == path) {
            let mut chain = visiting.clone();
            chain.push(path.to_string());
            return Err(MleafError::InheritCycle(chain.join(" → ")));
        }
        let Some(node) = self.flat.nodes.get(path) else {
            self.warnings.push(Warning {
                path: path.to_string(),
                message: "referenced but never defined; composed as an empty node".into(),
            });
            let e = Effective {
                path: path.to_string(),
                ..Effective::default()
            };
            self.memo.insert(path.to_string(), e.clone());
            return Ok(e);
        };
        visiting.push(path.to_string());
        let mut eff = Effective {
            path: path.to_string(),
            ..Effective::default()
        };
        for source in node.inherits.values() {
            let base = self.effective_inner(source, visiting)?;
            for (k, v) in base.attributes {
                let mut prov = base.provenance.get(&k).cloned().unwrap_or(Provenance {
                    layer: String::new(),
                    via: vec![],
                });
                prov.via.insert(0, source.clone());
                eff.provenance.insert(k.clone(), prov);
                eff.attributes.insert(k, v);
            }
            eff.children.extend(base.children);
            if base.order.is_some() {
                eff.order = base.order;
            }
            eff.sources.push(source.clone());
            eff.sources.extend(base.sources);
            eff.depends.insert(source.clone());
            eff.depends.extend(base.depends);
        }
        for (k, v) in &node.attributes {
            eff.attributes.insert(k.clone(), v.clone());
            if let Some(p) = node.provenance.get(k) {
                eff.provenance.insert(k.clone(), p.clone());
            }
        }
        eff.children
            .extend(node.children.iter().map(|(k, v)| (k.clone(), v.clone())));
        if node.order.is_some() {
            eff.order = node.order.clone();
        }
        // Derived values (row counts, sums …) are computed from their source on every compose,
        // so text that quotes them can't drift from the data.
        if let Some(d) = eff
            .attributes
            .get(DERIVED)
            .and_then(Value::as_object)
            .cloned()
            && let Some(src) = d.get("source").and_then(Value::as_str)
        {
            let base = self.effective_inner(src, visiting)?;
            eff.depends.insert(src.to_string());
            eff.depends.extend(base.depends.iter().cloned());
            match derive(&d, &base) {
                Some(v) => {
                    let layer = base
                        .provenance
                        .get(TABLE)
                        .map(|p| p.layer.clone())
                        .unwrap_or_default();
                    eff.attributes.insert(VALUE.into(), v);
                    eff.provenance.insert(
                        VALUE.into(),
                        Provenance {
                            layer,
                            via: vec![src.to_string()],
                        },
                    );
                }
                None => self.warnings.push(Warning {
                    path: path.to_string(),
                    message: format!(
                        "can't derive {} from `{src}`",
                        d.get("op").and_then(Value::as_str).unwrap_or("?")
                    ),
                }),
            }
        }
        visiting.pop();
        self.memo.insert(path.to_string(), eff.clone());
        Ok(eff)
    }

    /// Paths no other node names as a child or an inherit source, sorted.
    pub fn roots(&self) -> Vec<String> {
        let mut referenced = BTreeSet::new();
        for n in self.flat.nodes.values() {
            referenced.extend(n.children.values().cloned());
            referenced.extend(n.inherits.values().cloned());
        }
        self.flat
            .nodes
            .keys()
            .filter(|p| !referenced.contains(*p))
            .cloned()
            .collect()
    }

    /// Composes the whole stack: one tree per root.
    pub fn compose_all(&mut self) -> Result<Vec<Composed>, MleafError> {
        let roots = self.roots();
        if roots.is_empty() && !self.flat.nodes.is_empty() {
            return Err(MleafError::NoRoots);
        }
        let mut out = Vec::new();
        for root in roots {
            out.push(self.compose(&root, None, &mut Vec::new())?);
        }
        // A cycle no root reaches would otherwise vanish silently. Every node composed above is
        // memoized with its warnings already given, so warnings from this pass (nodes no root
        // reaches, such as inherit sources) are dropped.
        let given = self.warnings.len();
        let mut done = BTreeSet::new();
        let paths: Vec<String> = self.flat.nodes.keys().cloned().collect();
        for p in paths {
            self.child_cycle(&p, &mut Vec::new(), &mut done)?;
        }
        self.warnings.truncate(given);
        Ok(out)
    }

    fn child_cycle(
        &mut self,
        path: &str,
        ancestors: &mut Vec<String>,
        done: &mut BTreeSet<String>,
    ) -> Result<(), MleafError> {
        if ancestors.iter().any(|p| p == path) {
            let mut chain = ancestors.clone();
            chain.push(path.to_string());
            return Err(MleafError::ChildCycle(chain.join(" → ")));
        }
        if done.contains(path) || !self.flat.nodes.contains_key(path) {
            return Ok(());
        }
        let children: Vec<String> = self.effective(path)?.children.into_values().collect();
        ancestors.push(path.to_string());
        for c in children {
            self.child_cycle(&c, ancestors, done)?;
        }
        ancestors.pop();
        done.insert(path.to_string());
        Ok(())
    }

    pub fn compose(
        &mut self,
        path: &str,
        name: Option<String>,
        ancestors: &mut Vec<String>,
    ) -> Result<Composed, MleafError> {
        if ancestors.iter().any(|p| p == path) {
            let mut chain = ancestors.clone();
            chain.push(path.to_string());
            return Err(MleafError::ChildCycle(chain.join(" → ")));
        }
        let eff = self.effective(path)?;
        let names = ordered_child_names(&eff, &mut self.warnings);
        ancestors.push(path.to_string());
        let mut children = Vec::new();
        for child_name in names {
            let child_path = eff.children[&child_name].clone();
            children.push(self.compose(&child_path, Some(child_name), ancestors)?);
        }
        ancestors.pop();
        Ok(Composed {
            path: path.to_string(),
            name,
            attributes: eff.attributes,
            provenance: eff.provenance,
            children,
        })
    }
}

/// Names from `order` that exist, then the rest sorted by name.
fn ordered_child_names(eff: &Effective, warnings: &mut Vec<Warning>) -> Vec<String> {
    let mut names = Vec::new();
    let mut seen = BTreeSet::new();
    if let Some(order) = &eff.order {
        for n in order {
            if eff.children.contains_key(n) {
                if seen.insert(n.clone()) {
                    names.push(n.clone());
                }
            } else {
                warnings.push(Warning {
                    path: eff.path.clone(),
                    message: format!("order names `{n}`, which is not a child"),
                });
            }
        }
    }
    for n in eff.children.keys() {
        if seen.insert(n.clone()) {
            names.push(n.clone());
        }
    }
    names
}
