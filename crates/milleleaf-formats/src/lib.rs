//! `milleleaf-formats`: every way into and out of the Milleleaf model, what each one keeps, and which
//! one to use on this machine.
//!
//! A *route* is one way to write (export) or read (import) one file format for one kind of
//! content. Each declares whether it **keeps identity** — the markers (content-control tags,
//! defined names, shape names) an import needs to recognise nodes and bring edits back — and
//! what it **needs** (nothing, LibreOffice, Node). For a file, `mleaf` takes the best route that is
//! available here: identity first, then fallbacks. Imports only ever use routes that keep
//! identity; without it an import would see every paragraph as new and every old one as
//! deleted. `mleaf formats` prints the table, with why a route is or isn't available.

pub mod inventory;

use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A `word::document` (`docx-export` / `docx-import`).
    Document,
    /// A `grid::workbook` (`xlsx-export` / `xlsx-import`).
    Workbook,
    /// A `deck::deck` (`pptx-export` / `pptx-import`).
    Deck,
    /// A composed document or deck as a page (`mleaf render`, `tools/html2mleaf`).
    View,
    /// An `office::pack` (`mleaf pack`).
    Pack,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Document => "document",
            Kind::Workbook => "workbook",
            Kind::Deck => "deck",
            Kind::View => "view",
            Kind::Pack => "pack",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Dir {
    Export,
    Import,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Needs {
    Nothing,
    LibreOffice,
    /// Node.js and `tools/html2mleaf` (with Playwright).
    Node,
}

#[derive(Debug, Clone, Serialize)]
pub struct Route {
    pub id: &'static str,
    pub ext: &'static str,
    pub kind: Kind,
    pub dir: Dir,
    /// What does the work, in words.
    pub via: &'static str,
    pub keeps_identity: bool,
    pub needs: Needs,
    /// The command that uses it, when it isn't the `*-export` / `*-import` for its kind.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<&'static str>,
    pub note: &'static str,
}

#[allow(clippy::too_many_arguments)]
const fn route(
    id: &'static str,
    ext: &'static str,
    kind: Kind,
    dir: Dir,
    via: &'static str,
    keeps_identity: bool,
    needs: Needs,
    note: &'static str,
) -> Route {
    Route {
        id,
        ext,
        kind,
        dir,
        via,
        keeps_identity,
        needs,
        command: None,
        note,
    }
}

use Dir::{Export, Import};
use Kind::{Deck, Document, Pack, View, Workbook};
use Needs::{LibreOffice, Node, Nothing};

/// Every route, best first within each (format, kind, direction).
pub fn routes() -> Vec<Route> {
    let mut r = vec![
        route(
            "wordcraft-docx",
            "docx",
            Document,
            Export,
            "WordCraft",
            true,
            Nothing,
            "content controls tagged mleaf:<path>",
        ),
        route(
            "wordcraft-docx",
            "docx",
            Document,
            Import,
            "WordCraft",
            true,
            Nothing,
            "content controls tagged mleaf:<path>",
        ),
        route(
            "libreoffice-odt",
            "odt",
            Document,
            Export,
            "WordCraft, then LibreOffice (DOCX → ODT)",
            true,
            LibreOffice,
            "LibreOffice keeps the content-control tags in ODT",
        ),
        route(
            "wordcraft-odt",
            "odt",
            Document,
            Export,
            "WordCraft's own ODT writer",
            false,
            Nothing,
            "drops content controls: a one-way copy",
        ),
        route(
            "libreoffice-odt",
            "odt",
            Document,
            Import,
            "LibreOffice (ODT → DOCX), then WordCraft",
            true,
            LibreOffice,
            "LibreOffice keeps the content-control tags in ODT",
        ),
        route(
            "wordcraft-odt",
            "odt",
            Document,
            Import,
            "WordCraft's own ODT reader",
            false,
            Nothing,
            "drops content controls, so nothing could be matched",
        ),
        route(
            "wordcraft-md",
            "md",
            Document,
            Export,
            "WordCraft (Markdown)",
            false,
            Nothing,
            "one-way copy",
        ),
        route(
            "wordcraft-html",
            "html",
            Document,
            Export,
            "WordCraft (HTML)",
            false,
            Nothing,
            "one-way copy; `mleaf render` writes an HTML view that keeps identity",
        ),
        route(
            "wordcraft-rtf",
            "rtf",
            Document,
            Export,
            "WordCraft (RTF)",
            false,
            Nothing,
            "one-way copy",
        ),
        route(
            "wordcraft-tex",
            "tex",
            Document,
            Export,
            "WordCraft (LaTeX)",
            false,
            Nothing,
            "one-way copy",
        ),
        route(
            "wordcraft-txt",
            "txt",
            Document,
            Export,
            "WordCraft (plain text)",
            false,
            Nothing,
            "one-way copy",
        ),
        route(
            "gridcraft-xlsx",
            "xlsx",
            Workbook,
            Export,
            "GridCraft, recalculated",
            true,
            Nothing,
            "defined names mleaf_<path>",
        ),
        route(
            "gridcraft-xlsx",
            "xlsx",
            Workbook,
            Import,
            "GridCraft, recalculated",
            true,
            Nothing,
            "defined names mleaf_<path>",
        ),
        route(
            "libreoffice-ods",
            "ods",
            Workbook,
            Export,
            "GridCraft, then LibreOffice (XLSX → ODS)",
            true,
            LibreOffice,
            "LibreOffice keeps the defined names as named ranges",
        ),
        route(
            "libreoffice-ods",
            "ods",
            Workbook,
            Import,
            "LibreOffice (ODS → XLSX), then GridCraft",
            true,
            LibreOffice,
            "LibreOffice keeps the named ranges",
        ),
        route(
            "gridcraft-ods",
            "ods",
            Workbook,
            Import,
            "GridCraft's own ODS reader",
            false,
            Nothing,
            "doesn't read named ranges, so nothing could be matched",
        ),
        route(
            "deckcraft-pptx",
            "pptx",
            Deck,
            Export,
            "DeckCraft",
            true,
            Nothing,
            "shape and slide names mleaf:<path>",
        ),
        route(
            "deckcraft-pptx",
            "pptx",
            Deck,
            Import,
            "DeckCraft",
            true,
            Nothing,
            "shape names mleaf:<path>; slides by name, else by their frames",
        ),
        route(
            "libreoffice-odp",
            "odp",
            Deck,
            Export,
            "DeckCraft, then LibreOffice (PPTX → ODP)",
            true,
            LibreOffice,
            "LibreOffice keeps shape names (slide names are lost)",
        ),
        route(
            "libreoffice-odp",
            "odp",
            Deck,
            Import,
            "LibreOffice (ODP → PPTX), then DeckCraft",
            true,
            LibreOffice,
            "shape names kept; slides matched by their frames (best guess)",
        ),
    ];
    let mut views = vec![
        route(
            "mleaf-render",
            "html",
            View,
            Export,
            "mleaf render",
            true,
            Nothing,
            "data-ox-path on every element; decks in the deck/0 profile",
        ),
        route(
            "html2mleaf",
            "html",
            View,
            Import,
            "html2mleaf (Node + Playwright)",
            true,
            Node,
            "decks in the deck/0 profile → a proposal layer",
        ),
        route(
            "milleleaf-pack",
            "pdf",
            Pack,
            Export,
            "the apps' PDFs, joined by PdfCraft",
            false,
            Nothing,
            "one-way; its last page lists the layers the numbers came from",
        ),
    ];
    views[0].command = Some("mleaf render");
    views[2].command = Some("mleaf pack");
    r.extend(views);
    r
}

/// Whether a route can run on this machine.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum Status {
    Ready { detail: String },
    Unavailable { why: String },
}

impl Status {
    pub fn ready(&self) -> bool {
        matches!(self, Status::Ready { .. })
    }
}

/// LibreOffice: where it is and its version, or why it can't be used.
pub fn libreoffice() -> Result<String, String> {
    #[cfg(feature = "native")]
    { milleleaf_odf::soffice_status().map(|p| match milleleaf_odf::version(&p) {
        Some(v) => format!("{v} ({})", p.display()),
        None => p.display().to_string(),
    }) }
    #[cfg(not(feature = "native"))]
    { Err("LibreOffice is not included in this browser build".into()) }
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

/// `tools/html2mleaf/html2mleaf.mjs`: `MLEAF_HTML2MLEAF`, else found above the `mleaf` executable or the
/// current folder (a checkout of Milleleaf).
fn html2ox_script() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("MLEAF_HTML2MLEAF").or_else(|_| std::env::var("OX_HTML2OX")) {
        return Some(PathBuf::from(p)).filter(|p| p.is_file());
    }
    let starts = [std::env::current_exe().ok(), std::env::current_dir().ok()];
    for start in starts.into_iter().flatten() {
        for dir in start.ancestors() {
            let p = dir.join("tools/html2mleaf/html2mleaf.mjs");
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Node and html2mleaf: ready, or why not.
pub fn html2mleaf() -> Result<String, String> {
    let node = which("node").ok_or("Node.js isn't on the PATH")?;
    let script =
        html2ox_script().ok_or("tools/html2mleaf not found (set MLEAF_HTML2MLEAF to html2mleaf.mjs)")?;
    // Resolve Playwright the way html2mleaf will (its own node_modules, or anywhere Node looks).
    let dir = script.parent().unwrap_or(Path::new("."));
    let found = std::process::Command::new(&node)
        .args(["-e", "require.resolve('playwright')"])
        .current_dir(dir)
        .output()
        .is_ok_and(|o| o.status.success());
    if !found {
        return Err(format!(
            "Playwright isn't installed for {} (run `npm install` there)",
            dir.display()
        ));
    }
    Ok(format!("{} with {}", node.display(), script.display()))
}

pub fn status(r: &Route) -> Status {
    #[cfg(not(feature = "native"))]
    if r.id.starts_with("wordcraft-") && r.ext != "docx" {
        return Status::Unavailable { why: "additional WordCraft writers are not included in this browser build".into() };
    }
    let res = match r.needs {
        Nothing => Ok("built in".to_string()),
        LibreOffice => {
            #[cfg(feature = "native")]
            { milleleaf_odf::soffice_status()
                .map(|p| format!("LibreOffice at {}", p.display()))
                .map_err(|why| format!("LibreOffice {why}")) }
            #[cfg(not(feature = "native"))]
            { libreoffice() }
        },
        Node => html2mleaf(),
    };
    match res {
        _ if r.dir == Import && !r.keeps_identity && r.kind != View => Status::Unavailable {
            why: format!("not used for imports: {}", r.note),
        },
        Ok(detail) => Status::Ready { detail },
        Err(why) => Status::Unavailable { why },
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("a {kind} can't be {verb} as .{ext}; it can be {verb} as {}", options.join(", "))]
    Unsupported {
        kind: &'static str,
        verb: &'static str,
        ext: String,
        options: Vec<String>,
    },
    #[error("can't {verb} .{ext} on this machine: {}", reasons.join("; "))]
    Unavailable {
        verb: &'static str,
        ext: String,
        reasons: Vec<String>,
    },
    #[error("{0}")]
    Failed(String),
}

/// The route `mleaf` will use for `.ext`, and the better ones it had to skip (with why).
#[derive(Debug, Clone)]
pub struct Choice {
    pub route: Route,
    pub skipped: Vec<(Route, String)>,
}

fn verb(dir: Dir) -> &'static str {
    match dir {
        Export => "exported",
        Import => "imported",
    }
}

/// Picks the best available route for writing or reading a `kind` as `.ext`.
pub fn choose(kind: Kind, dir: Dir, ext: &str) -> Result<Choice, FormatError> {
    let ext = ext.trim_start_matches('.').to_ascii_lowercase();
    let all: Vec<Route> = routes()
        .into_iter()
        .filter(|r| r.kind == kind && r.dir == dir)
        .collect();
    let candidates: Vec<&Route> = all.iter().filter(|r| r.ext == ext).collect();
    if candidates.is_empty() {
        let mut options: Vec<String> = Vec::new();
        for r in &all {
            let o = format!(".{}", r.ext);
            if !options.contains(&o) && (dir == Export || r.keeps_identity) {
                options.push(o);
            }
        }
        return Err(FormatError::Unsupported {
            kind: kind.name(),
            verb: verb(dir),
            ext,
            options,
        });
    }
    let mut skipped = Vec::new();
    for r in candidates {
        match status(r) {
            Status::Ready { .. } => {
                return Ok(Choice {
                    route: r.clone(),
                    skipped,
                });
            }
            Status::Unavailable { why } => skipped.push((r.clone(), why)),
        }
    }
    Err(FormatError::Unavailable {
        verb: if dir == Export { "export" } else { "import" },
        ext,
        reasons: skipped
            .iter()
            .map(|(r, why)| format!("{} — {why}", r.via))
            .collect(),
    })
}

/// What the adapter wrote (DOCX, XLSX or PPTX) → the file asked for, by the best route here.
/// Returns the bytes, the choice, and warnings to show.
pub fn export(
    kind: Kind,
    ext: &str,
    ooxml: Vec<u8>,
) -> Result<(Vec<u8>, Choice, Vec<String>), FormatError> {
    let choice = choose(kind, Export, ext)?;
    let mut warnings = Vec::new();
    for (r, why) in &choice.skipped {
        warnings.push(format!("not using {}: {why}", r.via));
    }
    let r = &choice.route;
    let bytes = match r.id {
        "wordcraft-docx" | "gridcraft-xlsx" | "deckcraft-pptx" => ooxml,
        id if id.starts_with("libreoffice-") => {
            #[cfg(feature = "native")]
            { milleleaf_odf::to_odf(&ooxml, r.ext).map_err(|e| FormatError::Failed(e.to_string()))? }
            #[cfg(not(feature = "native"))]
            { return Err(FormatError::Failed("LibreOffice is not included in this browser build".into())); }
        }
        id if id.starts_with("wordcraft-") => {
            let doc =
                milleleaf_wordcraft::from_docx(&ooxml).map_err(|e| FormatError::Failed(e.to_string()))?;
            milleleaf_wordcraft::to_other(&doc, r.ext)
                .unwrap_or_else(|| {
                    Err(milleleaf_wordcraft::WordError::Format(format!(
                        "WordCraft can't write .{}",
                        r.ext
                    )))
                })
                .map_err(|e| FormatError::Failed(e.to_string()))?
        }
        id => return Err(FormatError::Failed(format!("route {id} has no exporter"))),
    };
    if !r.keeps_identity {
        warnings.push(format!(
            "{} doesn't keep Milleleaf's markers ({}): edits made to this .{} can't be brought back",
            r.via, r.note, r.ext
        ));
    }
    Ok((bytes, choice, warnings))
}

/// A file to import → the DOCX, XLSX or PPTX the importers read, by the best route here (only
/// routes that keep identity).
pub fn import(kind: Kind, ext: &str, bytes: Vec<u8>) -> Result<(Vec<u8>, Choice), FormatError> {
    let choice = choose(kind, Import, ext)?;
    let r = &choice.route;
    let out = match r.id {
        "wordcraft-docx" | "gridcraft-xlsx" | "deckcraft-pptx" => bytes,
        id if id.starts_with("libreoffice-") => {
            #[cfg(feature = "native")]
            { milleleaf_odf::from_odf(&bytes, r.ext).map_err(|e| FormatError::Failed(e.to_string()))? }
            #[cfg(not(feature = "native"))]
            { return Err(FormatError::Failed("LibreOffice is not included in this browser build".into())); }
        }
        id => return Err(FormatError::Failed(format!("route {id} has no importer"))),
    };
    Ok((out, choice))
}

#[cfg(test)]
mod tests;

/// The application that last saved an Office file (`docProps/app.xml`), e.g.
/// `LibreOffice/24.2.7.2$Linux_X86_64 …` or `Microsoft Excel`.
pub fn saved_by(bytes: &[u8]) -> Option<String> {
    use std::io::Read;
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).ok()?;
    let mut s = String::new();
    z.by_name("docProps/app.xml").ok()?.read_to_string(&mut s).ok()?;
    let start = s.find("<Application>")? + "<Application>".len();
    let end = start + s[start..].find("</Application>")?;
    Some(s[start..end].trim().to_string())
}
