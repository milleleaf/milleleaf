//! The Milleleaf layer file: what is stored on disk.
//!
//! Four sections (`header`, `imports`, `schemas`, `data`); nodes carry an `order` (paragraphs,
//! slides and sheets are ordered), and the `Any` data type covers values a schema can't constrain
//! yet. `schema/mleaf.tsp` describes this format.
//!
//! Milleleaf's layered model follows the composition ideas of Pixar's Universal Scene Description
//! (OpenUSD), as adopted by buildingSMART's IFC5. Its export for AI tools follows the DocLang 0.8
//! specification (doclang.ai, Apache-2.0) and is checked with the DocLang validator. The schema and
//! code are written independently; no USD, IFC5 or DocLang code or schema text is included.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Version string written by this crate.
pub const MLEAF_VERSION: &str = "mleaf_alpha";

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Header {
    /// Identifier of this layer, e.g. `milleleaf/examples/hello-table/base`.
    pub id: String,
    pub mleaf_version: String,
    pub data_version: String,
    pub author: String,
    /// ISO 8601 date or date-time.
    pub timestamp: String,
    /// `base`, `proposal`, `review`, `schema`, `stack` … Informational; composition ignores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// *Revision store.* On a stack: the stack it replaced (`<sha256 hex>`), so the heads form a
    /// history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// *Revision store.* On a layer: the head (stack hash) it was made against. `mleaf commit`
    /// refuses it if the head has moved since, unless it is rebased.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// *Revision store.* On a checkpoint: the layers (hashes, bottom first) it stands for.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replaces: Vec<String>,
    /// *Revision store.* The layer's content is encrypted (crypto-shredding, plan M1.5 item 7 B): its
    /// `data` is empty and the ciphertext is a blob. Destroying the key purges every copy at once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sealed: Option<Sealed>,
    /// *Revision store.* On a purge record: the layers it purges (plan M1.5 item 7). A store that
    /// receives it deletes their bytes and keeps their hashes; compose treats them as empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub purges: Vec<Purge>,
}

/// How a sealed layer's content is encrypted. The scheme (hybrid X25519 + ML-KEM, plan M4 item 5)
/// is decided when encryption is built; until then a sealed layer composes as empty.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sealed {
    pub scheme: String,
    /// The content key's id: purging the layer means destroying this key everywhere.
    pub key: String,
    /// The ciphertext, as a blob value (`sha256-<hex>`).
    pub content: String,
}

/// One purged layer: what, how, by whom, when and why.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Purge {
    /// The purged layer's hash (hex).
    pub layer: String,
    /// `bytes` (delete the content, keep the hash) or `key` (destroy a sealed layer's key).
    pub mode: String,
    pub by: String,
    pub at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Import {
    /// Relative file path (resolved against the importing file) or URL.
    pub uri: String,
    /// Optional `sha256-<hex>` of the imported file; checked when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrity: Option<String>,
}

/// One statement about one node. Many layers may say things about the same `path`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Node {
    pub path: String,
    /// child name → child path; `null` removes a child an earlier layer added.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub children: BTreeMap<String, Option<String>>,
    /// Order of child names. Replaces any earlier order; unlisted children follow, sorted by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<Vec<String>>,
    /// name → path of a node whose attributes and children this node takes on (styles, masters,
    /// live links); `null` removes an inherit an earlier layer added.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inherits: BTreeMap<String, Option<String>>,
    /// Namespaced attributes (`ns::name`); `null` removes the attribute.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub attributes: Map<String, Value>,
    /// *Checkpoints only:* attribute → id of the layer that originally set it, so provenance
    /// survives flattening many layers into one.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub provenance: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataType {
    Real,
    Boolean,
    Integer,
    String,
    DateTime,
    Enum,
    Array,
    Object,
    Reference,
    Blob,
    /// Any JSON value, not checked.
    Any,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumRestrictions {
    pub options: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArrayRestrictions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<usize>,
    pub value: Box<ValueDescription>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObjectRestrictions {
    pub values: BTreeMap<String, ValueDescription>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValueDescription {
    pub data_type: DataType,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub optional: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enum_restrictions: Option<EnumRestrictions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub array_restrictions: Option<ArrayRestrictions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_restrictions: Option<ObjectRestrictions>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Schema {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    pub value: ValueDescription,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub header: Header,
    #[serde(default)]
    pub imports: Vec<Import>,
    #[serde(default)]
    pub schemas: BTreeMap<String, Schema>,
    #[serde(default)]
    pub data: Vec<Node>,
}

impl Layer {
    pub fn from_json(bytes: &[u8]) -> serde_json::Result<Self> {
        serde_json::from_slice(bytes)
    }
}

/// True when `key` has the `namespace::name` shape every attribute key must have.
pub fn is_namespaced(key: &str) -> bool {
    match key.split_once("::") {
        Some((ns, rest)) => !ns.is_empty() && !rest.is_empty(),
        None => false,
    }
}

/// Deterministic UUID-shaped node path from a seed (same scheme as `tools/html2mleaf`), so importing
/// the same content twice gives the same paths.
pub fn stable_id(seed: &str) -> String {
    use sha2::{Digest, Sha256};
    let h: String = Sha256::digest(seed.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let variant = (u8::from_str_radix(&h[16..17], 16).unwrap_or(0) & 3) | 8;
    format!(
        "{}-{}-5{}-{:x}{}-{}",
        &h[0..8],
        &h[8..12],
        &h[13..16],
        variant,
        &h[17..20],
        &h[20..32]
    )
}
