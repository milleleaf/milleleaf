//! Loading a layer stack: a file plus everything it imports, bottom layer first.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::MleafError;
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
/// imports. A file imported twice is loaded once, at its first position, and every import's
/// integrity pin is checked against it, not only the first one. Later layers win.
pub fn load_stack(root: &Path) -> Result<Vec<LoadedLayer>, MleafError> {
    let mut out = Vec::new();
    let mut done = HashMap::new();
    let mut visiting = Vec::new();
    visit(root, None, &mut out, &mut done, &mut visiting)?;
    Ok(out)
}

fn visit(
    file: &Path,
    expected_integrity: Option<&str>,
    out: &mut Vec<LoadedLayer>,
    done: &mut HashMap<PathBuf, String>,
    visiting: &mut Vec<PathBuf>,
) -> Result<(), MleafError> {
    let canon = fs::canonicalize(file).map_err(|e| MleafError::Io(file.to_path_buf(), e))?;
    if visiting.contains(&canon) {
        let mut chain: Vec<String> = visiting.iter().map(|p| p.display().to_string()).collect();
        chain.push(canon.display().to_string());
        return Err(MleafError::ImportCycle(chain.join(" → ")));
    }
    let check = |canon: &PathBuf, actual: &str| match expected_integrity {
        Some(expected) if actual != expected => Err(MleafError::Integrity {
            file: canon.clone(),
            expected: expected.to_string(),
            actual: actual.to_string(),
        }),
        _ => Ok(()),
    };
    if let Some(actual) = done.get(&canon) {
        return check(&canon, actual);
    }
    let bytes = fs::read(&canon).map_err(|e| MleafError::Io(canon.clone(), e))?;
    let actual = integrity_of(&bytes);
    check(&canon, &actual)?;
    let layer = Layer::from_json(&bytes).map_err(|e| MleafError::Parse(canon.clone(), e))?;
    visiting.push(canon.clone());
    let dir = canon.parent().map(Path::to_path_buf).unwrap_or_default();
    for import in &layer.imports {
        if import.uri.contains("://") {
            return Err(MleafError::RemoteImport(import.uri.clone()));
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
    done.insert(canon.clone(), actual);
    out.push(LoadedLayer {
        source: canon,
        layer,
    });
    Ok(())
}

/// Where a project keeps content-addressed blobs, relative to its root.
pub const BLOB_DIR: &str = ".mleaf/blobs";

/// The bytes a blob value names: `sha256-<hex>`, stored in the project's [`BLOB_DIR`] and checked
/// against its hash, or (older layers, stack files) a path relative to the project or stack folder.
pub fn read_blob(root: &Path, blob: &str) -> std::io::Result<Vec<u8>> {
    let Some(hex) = blob.strip_prefix("sha256-") else {
        return std::fs::read(root.join(blob));
    };
    let p = crate::project_dir(root).join("blobs").join(hex);
    let bytes = std::fs::read(&p)?;
    if integrity_of(&bytes) != blob {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("{} doesn't match its hash", p.display()),
        ));
    }
    Ok(bytes)
}

/// What an importer's comparison export writes in place of a picture's bytes (it has no files):
/// these bytes and the blob value, so reading the export back gives the same value
/// ([`blob_of`]).
pub const STAND_IN: &[u8] = b"mleaf-stand-in:";

/// A picture's blob value: the hash of its bytes, or the value a [`STAND_IN`] carries.
pub fn blob_of(bytes: &[u8]) -> String {
    match bytes.strip_prefix(STAND_IN) {
        Some(b) => String::from_utf8_lossy(b).into_owned(),
        None => integrity_of(bytes),
    }
}

/// Where an export gets pictures' bytes.
#[derive(Clone, Copy, Debug)]
pub enum PictureSource<'a> {
    /// The project (or stack) folder: bytes read with [`read_blob`].
    Read(&'a Path),
    /// Bytes held in memory by blob value (a browser has no project folder).
    Memory(&'a std::collections::BTreeMap<String, (String, Vec<u8>)>),
    /// An importer's comparison export: a [`STAND_IN`] carrying the picture's hash (a picture
    /// still named by a path is read from this folder and hashed, see [`resolve_blob`]).
    StandIn(Option<&'a Path>),
}

/// A picture's bytes for an export (see [`PictureSource`]).
pub fn picture_bytes(src: PictureSource, blob: &str) -> std::io::Result<Vec<u8>> {
    match src {
        PictureSource::Read(root) => read_blob(root, blob),
        PictureSource::Memory(pictures) => {
            let (_, bytes) = pictures.get(blob).ok_or_else(|| std::io::Error::new(
                std::io::ErrorKind::NotFound, format!("missing asset: {blob}"),
            ))?;
            if blob.starts_with("sha256-") && integrity_of(bytes) != blob {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData, format!("{blob} doesn't match its hash"),
                ));
            }
            Ok(bytes.clone())
        }
        PictureSource::StandIn(root) => {
            Ok([STAND_IN, resolve_blob(root, blob).as_deref().unwrap_or(blob).as_bytes()].concat())
        }
    }
}

/// A picture's hash value: `blob` itself when it is one (`sha256-…`), else the hash of the file
/// it names, read from `root` (a stack file's pictures, not yet stored by hash). `None` when
/// that file can't be read.
pub fn resolve_blob(root: Option<&Path>, blob: &str) -> Option<String> {
    if blob.starts_with("sha256-") {
        return Some(blob.to_string());
    }
    std::fs::read(root?.join(blob)).ok().map(|b| integrity_of(&b))
}

/// The pictures in `node` (any `{"blob": path}` value, at any depth) named by a path that can't
/// be read from `root`: an importer can't compare them by hash, so it says so.
pub fn unreadable_pictures(node: &crate::Composed, root: Option<&Path>) -> Vec<String> {
    fn walk(v: &serde_json::Value, root: Option<&Path>, out: &mut Vec<String>) {
        match v {
            serde_json::Value::Object(m) => {
                if let Some(b) = m.get("blob").and_then(serde_json::Value::as_str)
                    && resolve_blob(root, b).is_none()
                    && !out.iter().any(|x| x == b)
                {
                    out.push(b.to_string());
                }
                m.values().for_each(|x| walk(x, root, out));
            }
            serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, root, out)),
            _ => {}
        }
    }
    fn visit(n: &crate::Composed, root: Option<&Path>, out: &mut Vec<String>) {
        n.attributes.values().for_each(|v| walk(v, root, out));
        n.children.iter().for_each(|c| visit(c, root, out));
    }
    let mut out = Vec::new();
    visit(node, root, &mut out);
    out
}

/// A media type from a file name's extension (pictures and vectors).
pub fn media_type_for(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "svg" => "image/svg+xml",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "emf" => "image/x-emf",
        "wmf" => "image/x-wmf",
        "tif" | "tiff" => "image/tiff",
        "bmp" => "image/bmp",
        _ => "image/png",
    }
}

/// JSON equality with numbers compared by value: a stored layer is canonical JSON (RFC 8785),
/// which writes `12.0` as `12`, and `serde_json` holds those as different numbers. Two integers
/// compare exactly: distinct ones above 2^53 would round to the same `f64`.
pub fn same(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    use serde_json::Value;
    match (a, b) {
        (Value::Number(x), Value::Number(y)) if !x.is_f64() && !y.is_f64() => x == y,
        (Value::Number(x), Value::Number(y)) => x == y || x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(a, b)| same(a, b)),
        (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w))),
        _ => a == b,
    }
}

/// A file extension for a picture's media type (the reverse of [`media_type_for`]).
pub fn ext_for(media_type: &str) -> &str {
    match media_type {
        "image/jpeg" => "jpg",
        "image/svg+xml" => "svg",
        "image/x-emf" | "image/emf" => "emf",
        "image/x-wmf" | "image/wmf" => "wmf",
        t if t.ends_with("+xml") => "xml",
        t => t.strip_prefix("image/").unwrap_or("bin"),
    }
}
