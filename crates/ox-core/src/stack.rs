//! Loading a layer stack: a file plus everything it imports, bottom layer first.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::OxError;
use crate::model::Layer;

#[derive(Debug, Clone)]
pub struct LoadedLayer {
    pub source: PathBuf,
    pub layer: Layer,
}

/// `sha256-<hex>` of `bytes`, the form `Import::integrity` uses.
pub fn integrity_of(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256-{hex}")
}

/// Loads `root` and its imports, depth first, so that every layer comes after the layers it
/// imports. A file imported twice is loaded once, at its first position. Later layers win.
pub fn load_stack(root: &Path) -> Result<Vec<LoadedLayer>, OxError> {
    let mut out = Vec::new();
    let mut done = HashSet::new();
    let mut visiting = Vec::new();
    visit(root, None, &mut out, &mut done, &mut visiting)?;
    Ok(out)
}

fn visit(
    file: &Path,
    expected_integrity: Option<&str>,
    out: &mut Vec<LoadedLayer>,
    done: &mut HashSet<PathBuf>,
    visiting: &mut Vec<PathBuf>,
) -> Result<(), OxError> {
    let canon = fs::canonicalize(file).map_err(|e| OxError::Io(file.to_path_buf(), e))?;
    if visiting.contains(&canon) {
        let mut chain: Vec<String> = visiting.iter().map(|p| p.display().to_string()).collect();
        chain.push(canon.display().to_string());
        return Err(OxError::ImportCycle(chain.join(" → ")));
    }
    if done.contains(&canon) {
        return Ok(());
    }
    let bytes = fs::read(&canon).map_err(|e| OxError::Io(canon.clone(), e))?;
    if let Some(expected) = expected_integrity {
        let actual = integrity_of(&bytes);
        if actual != expected {
            return Err(OxError::Integrity {
                file: canon,
                expected: expected.to_string(),
                actual,
            });
        }
    }
    let layer = Layer::from_json(&bytes).map_err(|e| OxError::Parse(canon.clone(), e))?;
    visiting.push(canon.clone());
    let dir = canon.parent().map(Path::to_path_buf).unwrap_or_default();
    for import in &layer.imports {
        if import.uri.contains("://") {
            return Err(OxError::RemoteImport(import.uri.clone()));
        }
        visit(
            &dir.join(&import.uri),
            import.integrity.as_deref(),
            out,
            done,
            visiting,
        )?;
    }
    visiting.pop();
    done.insert(canon.clone());
    out.push(LoadedLayer {
        source: canon,
        layer,
    });
    Ok(())
}

/// Where a project keeps content-addressed blobs, relative to its root.
pub const BLOB_DIR: &str = ".ox/blobs";

/// The bytes a blob value names: `sha256-<hex>`, stored in the project's [`BLOB_DIR`] and checked
/// against its hash, or (older layers, stack files) a path relative to the project or stack folder.
pub fn read_blob(root: &Path, blob: &str) -> std::io::Result<Vec<u8>> {
    let Some(hex) = blob.strip_prefix("sha256-") else {
        return std::fs::read(root.join(blob));
    };
    let p = root.join(BLOB_DIR).join(hex);
    let bytes = std::fs::read(&p)?;
    if integrity_of(&bytes) != blob {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("{} doesn't match its hash", p.display()),
        ));
    }
    Ok(bytes)
}

/// A media type from a file name's extension (pictures and vectors).
pub fn media_type_for(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "svg" => "image/svg+xml",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "image/png",
    }
}
