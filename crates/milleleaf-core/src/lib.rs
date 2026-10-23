//! `milleleaf-core`: the Milleleaf layer model.
//!
//! One project is a stack of JSON layers (base data, an agent's proposal, a reviewer's
//! corrections …). Composing the stack gives the single source of truth that WordCraft,
//! GridCraft, DeckCraft and PdfCraft adapters read from and write layers back to. See
//! the project notes.

pub mod compose;
pub mod doclang;
pub mod matching;
pub mod model;
pub mod render;
pub mod stack;
pub mod validate;

use std::path::{Path, PathBuf};

pub use compose::{Composed, Composer, Effective, Flat, Provenance};
pub use matching::{Confidence, Match};
pub use model::{Header, Import, Layer, Node, Purge, Sealed};
pub use stack::{BLOB_DIR, LoadedLayer, PictureSource, STAND_IN, blob_of, ext_for, picture_bytes, integrity_of, resolve_blob, unreadable_pictures, load_stack, media_type_for, read_blob, same};
pub use validate::{Issue, Level, validate};

#[derive(Debug, thiserror::Error)]
pub enum MleafError {
    #[error("{0}: {1}")]
    Io(PathBuf, #[source] std::io::Error),
    #[error("{0}: not a valid Milleleaf layer: {1}")]
    Parse(PathBuf, #[source] serde_json::Error),
    #[error("{file}: integrity mismatch (expected {expected}, file is {actual})")]
    Integrity {
        file: PathBuf,
        expected: String,
        actual: String,
    },
    #[error("remote imports aren't supported yet: {0}")]
    RemoteImport(String),
    #[error("import cycle: {0}")]
    ImportCycle(String),
    #[error("inherits cycle: {0}")]
    InheritCycle(String),
    #[error("children cycle: {0}")]
    ChildCycle(String),
    #[error("every node is referenced by another, so there is no root (a cycle?)")]
    NoRoots,
}

/// Loads and flattens the stack rooted at `file`.
pub fn open(file: &Path) -> Result<Flat, MleafError> {
    Ok(compose::flatten(&load_stack(file)?))
}

#[cfg(test)]
mod tests;

/// The folder a project keeps its store and blobs in: `.mleaf`.
pub fn project_dir(root: &std::path::Path) -> std::path::PathBuf {
    root.join(".mleaf")
}

/// The environment variable `MLEAF_<name>`.
pub fn env(name: &str) -> Option<String> {
    std::env::var(format!("MLEAF_{name}")).ok()
}

/// The short revision hash of exported content (one or more composed nodes), recorded in exported files
/// and checked when an edited file comes back.
pub fn revision_of<T: serde::Serialize>(parts: &[&T]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for p in parts {
        h.update(serde_json::to_string(p).unwrap_or_default());
    }
    h.finalize().iter().take(12).map(|b| format!("{b:02x}")).collect()
}
