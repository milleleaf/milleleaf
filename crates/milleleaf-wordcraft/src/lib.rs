//! Milleleaf ↔ WordCraft.
//!
//! **Export** ([`export`]): a composed `word::document` node becomes a
//! [`wordcraft_doc::Document`] and, through `wordcraft-docx`, a `.docx`. Every block is wrapped in
//! a content control tagged `mleaf:<node path>` (Word keeps `w:sdt` tags through editing), linked
//! fields and tables are content-locked, and custom document properties record which document
//! and which composed revision the file came from.
//!
//! **Import** ([`import`]): an edited `.docx` is compared with the composed document it was
//! exported from and turned into a minimal **edit layer**: changed paragraphs, new paragraphs and
//! tables, removed children and the new order. Edits to live-linked content are never written
//! back to the source; they are reported as conflicts.

mod export;
mod import;

pub use export::{ExportReport, export, export_with, page_size};
pub use import::{Conflict, ImportReport, adopt, adopt_for_comparison, exported_path, exported_revision, import, import_for_comparison, import_in};


/// Tag prefix on the content controls that carry node identity.
pub const TAG_PREFIX: &str = "mleaf:";
/// Suffix of the tag on a table caption paragraph.
pub const CAPTION_SUFFIX: &str = "#caption";
/// Custom document property: path of the exported `word::document` node.
pub const PROP_DOCUMENT: &str = "mleaf.document";
/// Custom document property: revision of the composed document at export.
pub const PROP_REVISION: &str = "mleaf.revision";
/// Custom document property: the layer ids the export was composed from, bottom first.
pub const PROP_LAYERS: &str = "mleaf.layers";

#[derive(Debug, thiserror::Error)]
pub enum WordError {
    #[error("`{0}` is not a word::document node")]
    NotADocument(String),
    #[error("DOCX: {0}")]
    Docx(String),
    #[error("{0}")]
    Format(String),
    #[error("this file was exported from document `{found}`, not `{expected}`")]
    WrongDocument { expected: String, found: String },
    #[error("this file has no {PROP_DOCUMENT} property; it wasn't exported by Milleleaf")]
    NotExported,
    #[error("missing asset: {0}")]
    MissingAsset(String),
}

/// Revision id of a composed node: a hash of its JSON, so an import can tell whether the stack
/// changed since export.
pub fn revision_of(node: &milleleaf_core::Composed) -> String {
    milleleaf_core::revision_of(&[node])
}

/// Writes a WordCraft document as DOCX bytes.
pub fn to_docx(doc: &wordcraft_doc::Document) -> Result<Vec<u8>, WordError> {
    wordcraft_docx::write(doc).map_err(|e| WordError::Docx(e.to_string()))
}

/// Reads DOCX bytes into a WordCraft document.
pub fn from_docx(bytes: &[u8]) -> Result<wordcraft_doc::Document, WordError> {
    wordcraft_docx::read(&milleleaf_core::legacy::upgrade(bytes)).map_err(|e| WordError::Docx(e.to_string()))
}

/// Writes a document in one of WordCraft's other formats: `odt`, `md`, `html`, `rtf`, `tex`,
/// `txt`. These drop content controls, so an edited copy can't be brought back; Milleleaf uses
/// them only as one-way exports (see `mleaf formats`).
pub fn to_other(doc: &wordcraft_doc::Document, ext: &str) -> Option<Result<Vec<u8>, WordError>> {
    #[cfg(feature = "other-formats")]
    { wordcraft_formats::export(ext, doc).map(|r| r.map_err(WordError::Format)) }
    #[cfg(not(feature = "other-formats"))]
    { let _ = (doc, ext); None }
}

#[cfg(test)]
mod tests;
