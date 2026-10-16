//! `ox-core`: the Milleleaf layer model.
//!
//! One project is a stack of JSON layers (base data, an agent's proposal, a reviewer's
//! corrections …). Composing the stack gives the single source of truth that WordCraft,
//! GridCraft, DeckCraft and PdfCraft adapters read from and write layers back to. See
//! the project notes.

pub mod compose;
pub mod matching;
pub mod model;
pub mod render;
pub mod stack;
pub mod validate;

use std::path::{Path, PathBuf};

pub use compose::{Composed, Composer, Effective, Flat, Provenance};
pub use matching::{Confidence, Match};
pub use model::{Header, Import, Layer, Node, Purge, Sealed};
pub use stack::{BLOB_DIR, LoadedLayer, load_stack, media_type_for, read_blob};
pub use validate::{Issue, Level, validate};

#[derive(Debug, thiserror::Error)]
pub enum OxError {
    #[error("{0}: {1}")]
    Io(PathBuf, #[source] std::io::Error),
    #[error("{0}: not a valid OX layer: {1}")]
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
pub fn open(file: &Path) -> Result<Flat, OxError> {
    Ok(compose::flatten(&load_stack(file)?))
}

#[cfg(test)]
mod tests;
