//! Milleleaf ↔ GridCraft.
//!
//! **Export** ([`export`]): a composed `grid::workbook` node becomes a GridCraft
//! [`Workbook`](gridcraft_model::Workbook): each `grid::sheet` child a sheet, each
//! `office::data::table` / `office::data::value` written at its `grid::range`, formulas from
//! `grid::formula`, then recalculated by GridCraft's calc engine. Every node gets a **defined
//! name** `ox_<path>` over its range — Excel and LibreOffice keep names and move them when rows
//! or columns are inserted — and the workbook carries `ox_workbook` / `ox_revision` names.
//!
//! **Import** ([`import`]): an edited `.xlsx` is recalculated with GridCraft's calc engine, read
//! back through its `ox_` names and compared with the composed workbook. Changed data, values,
//! formulas, moved ranges and renamed sheets become an **edit layer**. Formula results are
//! recomputed here, not trusted from the file, so the layer carries consistent totals even when
//! the editor didn't save cached values.

mod adopt;
mod export;
mod import;
mod look;

pub use adopt::{AdoptReport, adopt};
pub use export::{ExportReport, export};
pub use import::{ImportReport, exported_revision, import};

use sha2::{Digest, Sha256};

/// Prefix of the defined names that carry node identity.
pub const NAME_PREFIX: &str = "ox_";
/// Defined name holding the exported workbook node's path (a string constant).
pub const NAME_WORKBOOK: &str = "ox_workbook";
/// Defined name holding the composed revision at export (a string constant).
pub const NAME_REVISION: &str = "ox_revision";

#[derive(Debug, thiserror::Error)]
pub enum GridError {
    #[error("`{0}` is not a grid::workbook node")]
    NotAWorkbook(String),
    #[error("XLSX: {0}")]
    Xlsx(String),
    #[error("this file was exported from workbook `{found}`, not `{expected}`")]
    WrongWorkbook { expected: String, found: String },
    #[error("this file has no {NAME_WORKBOOK} name; it wasn't exported by Milleleaf")]
    NotExported,
    #[error("{0}")]
    Invalid(String),
}

/// The defined name for a node path: `ox_` + the path with characters names can't hold
/// replaced by `_` (a UUID's hyphens).
pub fn name_for(path: &str) -> String {
    let body: String = path
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{NAME_PREFIX}{body}")
}

/// Revision id of a composed node (hash of its JSON), as in the WordCraft adapter.
pub fn revision_of(node: &ox_core::Composed) -> String {
    let bytes = serde_json::to_vec(node).unwrap_or_default();
    Sha256::digest(&bytes)
        .iter()
        .take(12)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Writes a GridCraft workbook as XLSX bytes.
pub fn to_xlsx(wb: &gridcraft_model::Workbook) -> Result<Vec<u8>, GridError> {
    gridcraft_xlsx::write_xlsx(wb).map_err(|e| GridError::Xlsx(e.to_string()))
}

/// Reads XLSX bytes into a GridCraft workbook.
pub fn from_xlsx(bytes: &[u8]) -> Result<gridcraft_model::Workbook, GridError> {
    gridcraft_xlsx::read_xlsx(bytes)
        .map(|(wb, _)| wb)
        .map_err(|e| GridError::Xlsx(e.to_string()))
}

/// Recalculates every formula with GridCraft's calc engine.
pub fn recalc(wb: &mut gridcraft_model::Workbook) {
    gridcraft_calc::Calc::new().recalc_all(wb);
}

/// `'Q3 2026'!$A$1:$B$4` → (`Q3 2026`, A1:B4).
pub(crate) fn split_ref(formula: &str) -> Option<(String, gridcraft_core::addr::RangeRef)> {
    let f = formula.trim().trim_start_matches('=');
    let (sheet, range) = f.rsplit_once('!')?;
    let sheet = sheet
        .strip_prefix('\'')
        .and_then(|s| s.strip_suffix('\''))
        .map(|s| s.replace("''", "'"))
        .unwrap_or_else(|| sheet.to_string());
    let range = gridcraft_core::addr::RangeRef::parse(&range.replace('$', ""))?;
    Some((sheet, range))
}

#[cfg(test)]
mod tests;
