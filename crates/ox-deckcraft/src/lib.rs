//! Milleleaf ↔ DeckCraft.
//!
//! **Export** ([`export`]): a composed `deck::deck` node becomes a DeckCraft
//! [`Presentation`](deckcraft_model::Presentation) and, through `deckcraft-pptx`, a `.pptx`.
//! Slides and shapes are named `ox:<node path>` (PowerPoint and Impress keep names: they're what
//! the Selection pane shows), linked tables become native tables and `deck::chart` frames native
//! charts drawn from the data. Linked numbers in text are written as their text and found again
//! on import by position (Impress drops text fields). The presentation's keywords record the
//! deck and the composed revision.
//!
//! **Import** ([`import`]): an edited `.pptx` is compared with the composed deck and turned into
//! an edit layer: moved and resized frames, edited text, new text boxes, deleted frames and
//! slides, new order, speaker notes. Slides are recognised by their name when the editor kept it
//! and otherwise by the frames on them (Impress drops slide names; every editor keeps shape
//! names). Edits to live-linked tables, charts and fields are reported
//! as conflicts; the workbook owns that data.

mod export;
mod import;

pub use export::{ExportReport, export};
pub use import::{ImportReport, adopt, exported_revision, import, import_in};

use sha2::{Digest, Sha256};

/// Prefix of the slide and shape names (and text field types) that carry node identity.
pub const NAME_PREFIX: &str = "ox:";
/// Markers in the presentation keywords: `oxdeck=<path> oxrevision=<hash>`. (No colons:
/// LibreOffice reads `:` as a keyword separator and rewrites it.)
pub const KEY_DECK: &str = "oxdeck=";
pub const KEY_REVISION: &str = "oxrevision=";
/// CSS px (OX decks) per point (DeckCraft).
pub const PX_PER_PT: f64 = 96.0 / 72.0;

#[derive(Debug, thiserror::Error)]
pub enum DeckError {
    #[error("`{0}` is not a deck::deck node")]
    NotADeck(String),
    #[error("PPTX: {0}")]
    Pptx(String),
    #[error("this file was exported from deck `{found}`, not `{expected}`")]
    WrongDeck { expected: String, found: String },
    #[error("this file has no {KEY_DECK} keyword; it wasn't exported by Milleleaf")]
    NotExported,
    #[error("{0}")]
    Invalid(String),
    #[error("missing asset: {0}")]
    MissingAsset(String),
}

/// Revision id of a composed node (hash of its JSON), as in the other adapters.
pub fn revision_of(node: &ox_core::Composed) -> String {
    let bytes = serde_json::to_vec(node).unwrap_or_default();
    Sha256::digest(&bytes)
        .iter()
        .take(12)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Writes a DeckCraft presentation as PPTX bytes.
pub fn to_pptx(p: &deckcraft_model::Presentation) -> Result<Vec<u8>, DeckError> {
    deckcraft_pptx::export(p).map_err(|e| DeckError::Pptx(e.to_string()))
}

/// Reads PPTX bytes into a DeckCraft presentation.
pub fn from_pptx(bytes: &[u8]) -> Result<deckcraft_model::Presentation, DeckError> {
    deckcraft_pptx::import(bytes).map_err(|e| DeckError::Pptx(e.to_string()))
}

pub(crate) fn px(pt: f64) -> f64 {
    (pt * PX_PER_PT * 100.0).round() / 100.0
}

pub(crate) fn pt(px: f64) -> f64 {
    px / PX_PER_PT
}

#[cfg(test)]
mod tests;
