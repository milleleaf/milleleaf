//! OpenDocument for Milleleaf.
//!
//! The ArtCraft apps don't round-trip ODF yet: WordCraft reads and writes ODT but drops content
//! controls (where Milleleaf keeps node identity), GridCraft reads ODS but not its named
//! ranges, and DeckCraft neither reads nor writes ODP. LibreOffice, on the other hand, keeps
//! all of Milleleaf's markers through ODF (content-control tags in ODT, named ranges in ODS,
//! shape and page names in ODP — checked with LibreOffice 24.2).
//!
//! So ODF goes through LibreOffice, as a converter at the edge: exports are made as OOXML by
//! the ArtCraft adapters and converted to ODF; imports are converted from ODF to OOXML and read by
//! the same importers. Nothing in the model depends on it, and every command says when
//! LibreOffice is missing.

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum OdfError {
    #[error(
        "LibreOffice is needed for {0} (Milleleaf uses it to convert OpenDocument files); install it, or set MLEAF_SOFFICE to its `soffice`"
    )]
    NoLibreOffice(String),
    #[error("LibreOffice couldn't convert {from} to {to}: {detail}")]
    Convert {
        from: String,
        to: String,
        detail: String,
    },
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

/// The OOXML format and LibreOffice export filter matching an ODF extension (and back).
pub fn pair(ext: &str) -> Option<(&'static str, &'static str, &'static str)> {
    // (odf ext, ooxml ext, filter to make the ooxml, filter to make the odf)
    match ext {
        "odt" => Some(("docx", "docx:MS Word 2007 XML", "odt:writer8")),
        "ods" => Some(("xlsx", "xlsx:Calc MS Excel 2007 XML", "ods:calc8")),
        "odp" => Some((
            "pptx",
            "pptx:Impress MS PowerPoint 2007 XML",
            "odp:impress8",
        )),
        _ => None,
    }
}

pub fn is_odf(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
        .and_then(pair)
        .is_some()
}

/// Where LibreOffice's `soffice` is, or why there isn't one: `MLEAF_SOFFICE` (a path, or `none` to
/// turn LibreOffice off), then `soffice` / `libreoffice` on the PATH, then the macOS and Windows
/// default install locations.
pub fn soffice_status() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("MLEAF_SOFFICE") {
        if p.is_empty() || p.eq_ignore_ascii_case("none") {
            return Err("turned off (MLEAF_SOFFICE=none)".into());
        }
        let p = PathBuf::from(p);
        return if p.exists() {
            Ok(p)
        } else {
            Err(format!(
                "MLEAF_SOFFICE is {}, which doesn't exist",
                p.display()
            ))
        };
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path) {
        for name in ["soffice", "libreoffice", "soffice.exe"] {
            let c = dir.join(name);
            if c.is_file() {
                return Ok(c);
            }
        }
    }
    [
        "/Applications/LibreOffice.app/Contents/MacOS/soffice",
        "C:\\Program Files\\LibreOffice\\program\\soffice.exe",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|p| p.exists())
    .ok_or_else(|| {
        "not installed: no soffice or libreoffice on the PATH, none in /Applications/LibreOffice.app \
         or C:\\Program Files\\LibreOffice, and MLEAF_SOFFICE isn't set"
            .into()
    })
}

/// LibreOffice's `soffice`, if there is one (see [`soffice_status`]).
pub fn soffice() -> Option<PathBuf> {
    soffice_status().ok()
}

/// `soffice --version`'s first line, e.g. "LibreOffice 24.2.7.2 420(Build:2)".
pub fn version(exe: &Path) -> Option<String> {
    let out = Command::new(exe).arg("--version").output().ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
}

/// Converts `bytes` (a file with extension `from`) with LibreOffice's `filter`
/// (`"odt:writer8"`, `"docx:MS Word 2007 XML"` …). Runs headless with a throwaway profile, so
/// it doesn't touch, or wait for, a LibreOffice the user has open.
pub fn convert(bytes: &[u8], from: &str, filter: &str) -> Result<Vec<u8>, OdfError> {
    let to = filter.split(':').next().unwrap_or(filter);
    let exe = soffice_status()
        .map_err(|why| OdfError::NoLibreOffice(format!(".{from} → .{to} (LibreOffice: {why})")))?;
    let dir = std::env::temp_dir().join(format!("milleleaf-odf-{}-{}", std::process::id(), unique()));
    std::fs::create_dir_all(&dir)?;
    let input = dir.join(format!("in.{from}"));
    std::fs::write(&input, bytes)?;
    let profile = dir.join("profile");
    let result = dir.join(format!("in.{to}"));
    let run = || {
        Command::new(&exe)
            .arg(format!(
                "-env:UserInstallation=file://{}",
                profile.display()
            ))
            .args([
                "--headless",
                "--norestore",
                "--convert-to",
                filter,
                "--outdir",
            ])
            .arg(&dir)
            .arg(&input)
            .output()
    };
    let mut out = run()?;
    // A brand-new profile: LibreOffice sometimes sets it up and exits without converting (seen in CI,
    // with two conversions starting at once). The profile exists now, so one retry is enough.
    if std::fs::metadata(&result).map_or(true, |m| m.len() == 0) {
        out = run()?;
    }
    let res = match std::fs::read(&result) {
        Ok(b) if !b.is_empty() => Ok(b),
        _ => Err(OdfError::Convert {
            from: from.into(),
            to: to.into(),
            detail: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        }),
    };
    let _ = std::fs::remove_dir_all(&dir);
    res
}

fn unique() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// OOXML bytes from the ArtCraft adapters → ODF bytes for a `.odt` / `.ods` / `.odp` path.
pub fn to_odf(ooxml: &[u8], odf_ext: &str) -> Result<Vec<u8>, OdfError> {
    let (ooxml_ext, _, to_odf) = pair(odf_ext).ok_or_else(|| OdfError::Convert {
        from: odf_ext.into(),
        to: "?".into(),
        detail: "not an ODF extension".into(),
    })?;
    convert(ooxml, ooxml_ext, to_odf)
}

/// ODF bytes → OOXML bytes the ArtCraft adapters' importers read.
pub fn from_odf(odf: &[u8], odf_ext: &str) -> Result<Vec<u8>, OdfError> {
    let (_, to_ooxml, _) = pair(odf_ext).ok_or_else(|| OdfError::Convert {
        from: odf_ext.into(),
        to: "?".into(),
        detail: "not an ODF extension".into(),
    })?;
    convert(odf, odf_ext, to_ooxml)
}
