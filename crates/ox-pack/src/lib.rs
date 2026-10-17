//! Milleleaf pack: **one composed snapshot → one PDF**.
//!
//! A pack node (`office::pack`) lists parts — a `word::document`, a `grid::workbook`, a
//! `deck::deck` — in order. [`build`] composes nothing itself: it takes the composed tree, sends
//! each part through its Craft adapter and that app's own PDF writer (WordCraft's layout engine,
//! GridCraft's print layout, DeckCraft's slide renderer), then assembles the parts with
//! PdfCraft: one bookmark per part, the deck's slide titles nested under it, document info
//! naming the stack revision, and an "About this pack" page listing every layer the numbers came
//! from. Because every part comes from the same composition, the report, the workbook and the
//! slides can't disagree.

use std::path::Path;
use std::sync::Arc;

use ox_core::Composed;
use ox_core::render::find;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const PACK: &str = "office::pack";

#[derive(Debug, thiserror::Error)]
pub enum PackError {
    #[error("`{0}` is not an office::pack node")]
    NotAPack(String),
    #[error("part `{0}` is not in the composed tree")]
    MissingPart(String),
    #[error("part `{0}` is not a word::document, grid::workbook or deck::deck")]
    UnknownPart(String),
    #[error("{part}: {message}")]
    Part { part: String, message: String },
    #[error("assembling the pack: {0}")]
    Assemble(String),
}

/// One layer of the stack, for the "About this pack" page.
#[derive(Debug, Clone, Serialize)]
pub struct LayerInfo {
    pub id: String,
    pub kind: String,
    pub author: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PartReport {
    pub title: String,
    pub path: String,
    pub kind: &'static str,
    pub pages: usize,
    /// First page of the part in the pack (0-based).
    pub first_page: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PackReport {
    pub title: String,
    /// Hash of the composed pack node and every part: the "version of the numbers".
    pub revision: String,
    pub parts: Vec<PartReport>,
    pub pages: usize,
}

fn part_err(part: &str, e: impl std::fmt::Display) -> PackError {
    PackError::Part {
        part: part.to_string(),
        message: e.to_string(),
    }
}

fn page_count(pdf: &[u8]) -> Result<usize, PackError> {
    let doc = pdfcraft_cos::Document::open(Arc::new(pdf.to_vec()))
        .map_err(|e| PackError::Assemble(e.to_string()))?;
    pdfcraft_organize::page_count(&doc).map_err(|e| PackError::Assemble(e.to_string()))
}

/// Revision of the pack: a hash over the composed parts, in order.
pub fn revision_of(parts: &[&Composed]) -> String {
    let mut h = Sha256::new();
    for p in parts {
        h.update(serde_json::to_vec(p).unwrap_or_default());
    }
    h.finalize()
        .iter()
        .take(12)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A part rendered to PDF by its own app.
fn render_part(
    node: &Composed,
    layers: &[String],
    assets: &Path,
) -> Result<(Vec<u8>, &'static str, Vec<String>), PackError> {
    let at = |e| part_err(&node.path, e);
    if node.attributes.contains_key("word::document") {
        let (doc, report) = ox_wordcraft::export(node, layers, assets).map_err(at)?;
        let pdf = wordcraft_pdf::export(&doc, &wordcraft_pdf::PdfOptions::default())
            .map_err(|e| part_err(&node.path, e))?;
        return Ok((pdf, "document", report.warnings));
    }
    if node.attributes.contains_key("grid::workbook") {
        let (wb, report) = ox_gridcraft::export(node, assets).map_err(|e| part_err(&node.path, e))?;
        let opts = gridcraft_engine::cmd::print::PrintOptions {
            all_sheets: true,
            ..Default::default()
        };
        let lays = gridcraft_engine::cmd::print::layouts(&wb, &opts);
        let name = node
            .attributes
            .get("grid::workbook")
            .and_then(|g| g.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("workbook.xlsx");
        let (pdf, _) = gridcraft_engine::cmd::print::render_pdf(&wb, &lays, name, name);
        let mut warnings = report.warnings;
        warnings.extend(
            report
                .stale_values
                .iter()
                .map(|(p, a, b)| format!("{p}: model stores {a}, formula gives {b}")),
        );
        return Ok((pdf, "workbook", warnings));
    }
    if node.attributes.contains_key("deck::deck") {
        let (p, report) =
            ox_deckcraft::export(node, assets).map_err(|e| part_err(&node.path, e))?;
        let opts = deckcraft_pdf::PdfOptions {
            frame_slides: false,
            ..Default::default()
        };
        let pdf = deckcraft_pdf::export(&p, &opts).map_err(|e| part_err(&node.path, e))?;
        return Ok((pdf, "deck", report.warnings));
    }
    Err(PackError::UnknownPart(node.path.clone()))
}

/// The "About this pack" page: what's in it, and every layer the numbers came from.
fn about_page(
    title: &str,
    revision: &str,
    parts: &[PartReport],
    layers: &[LayerInfo],
) -> Result<Vec<u8>, PackError> {
    use wordcraft_doc::{Cell, CharProps, Document, Paragraph, Table, para_block};
    let mut doc = Document::new();
    doc.body.clear();
    doc.core.title = format!("{title} — about this pack");
    let p = |text: &str, style: &str| {
        para_block(Paragraph::with_text(text, CharProps::default()).styled(style))
    };
    doc.body.push(p("About this pack", "Heading1"));
    doc.body.push(p(
        &format!("Every part of “{title}” was produced from one composition of the Milleleaf stack (revision {revision}), so the figures in the report, the workbook and the slides are the same figures."),
        "Normal",
    ));
    doc.body.push(p("Parts", "Heading2"));
    let table = |rows: Vec<Vec<String>>, doc: &Document| {
        let cols = rows.first().map(Vec::len).unwrap_or(1);
        let width =
            doc.last_section.page_w - doc.last_section.margin_left - doc.last_section.margin_right;
        let mut t = Table::new(rows.len(), cols, width);
        for (r, row) in rows.iter().enumerate() {
            if let Some(trow) = t.rows.get_mut(r) {
                trow.props.header = r == 0;
                for (c, text) in row.iter().enumerate() {
                    if let Some(cell) = trow.cells.get_mut(c) {
                        let w = cell.props.width;
                        let props = CharProps {
                            bold: (r == 0).then_some(true),
                            size: Some(9.0),
                            ..CharProps::default()
                        };
                        *cell = Cell::with_text("");
                        cell.blocks = vec![para_block(Paragraph::with_text(text, props))];
                        cell.props.width = w;
                    }
                }
            }
        }
        Arc::new(wordcraft_doc::Block::Table(t))
    };
    let mut rows = vec![vec![
        "Part".to_string(),
        "Made with".to_string(),
        "Pages".to_string(),
    ]];
    for part in parts {
        let app = match part.kind {
            "document" => "WordCraft",
            "workbook" => "GridCraft",
            _ => "DeckCraft",
        };
        rows.push(vec![
            part.title.clone(),
            app.to_string(),
            format!("{}–{}", part.first_page + 1, part.first_page + part.pages),
        ]);
    }
    let parts_table = table(rows, &doc);
    doc.body.push(parts_table);
    doc.body.push(p("Layers, bottom first", "Heading2"));
    let mut rows = vec![vec![
        "Layer".to_string(),
        "Kind".to_string(),
        "Author".to_string(),
        "Date".to_string(),
    ]];
    for l in layers {
        rows.push(vec![
            l.id.clone(),
            l.kind.clone(),
            l.author.clone(),
            l.timestamp.clone(),
        ]);
    }
    let layers_table = table(rows, &doc);
    doc.body.push(layers_table);
    doc.body.push(p("Later layers win. Each figure's own provenance — which layer set it, and through which links — is in the stack (`ox get`).", "Normal"));
    wordcraft_pdf::export(&doc, &wordcraft_pdf::PdfOptions::default())
        .map_err(|e| part_err("about", e))
}

/// Slide titles of a composed deck, in order: the first `title` text frame on each slide.
fn slide_titles(deck: &Composed) -> Vec<String> {
    deck.children
        .iter()
        .enumerate()
        .map(|(i, s)| {
            s.children
                .iter()
                .find_map(|f| {
                    let t = f.attributes.get("deck::text")?;
                    (t.get("role").and_then(Value::as_str) == Some("title")).then(|| {
                        t["paragraphs"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .flat_map(|p| p["runs"].as_array().into_iter().flatten())
                            .filter_map(|r| r.get("text").and_then(Value::as_str))
                            .collect::<String>()
                    })
                })
                .unwrap_or_else(|| format!("Slide {}", i + 1))
        })
        .collect()
}

/// Builds the pack PDF for the composed pack node `pack_path`.
pub fn build(
    trees: &[Composed],
    pack_path: &str,
    layers: &[LayerInfo],
    assets: &Path,
) -> Result<(Vec<u8>, PackReport), PackError> {
    let pack = find(trees, pack_path).ok_or_else(|| PackError::NotAPack(pack_path.to_string()))?;
    let spec = pack
        .attributes
        .get(PACK)
        .and_then(Value::as_object)
        .ok_or_else(|| PackError::NotAPack(pack_path.to_string()))?;
    let title = spec
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("Pack")
        .to_string();
    let layer_ids: Vec<String> = layers.iter().map(|l| l.id.clone()).collect();

    let mut nodes: Vec<(&Composed, String)> = Vec::new();
    for part in spec
        .get("parts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let path = part.get("source").and_then(Value::as_str).unwrap_or("");
        let node = find(trees, path).ok_or_else(|| PackError::MissingPart(path.to_string()))?;
        let t = part
            .get("title")
            .and_then(Value::as_str)
            .map(String::from)
            .unwrap_or_else(|| node.name.clone().unwrap_or_else(|| path.to_string()));
        nodes.push((node, t));
    }
    let revision = revision_of(&nodes.iter().map(|(n, _)| *n).collect::<Vec<_>>());

    let mut pdfs: Vec<Vec<u8>> = Vec::new();
    let mut reports: Vec<PartReport> = Vec::new();
    let mut first = 0;
    for (node, t) in &nodes {
        let (pdf, kind, warnings) = render_part(node, &layer_ids, assets)?;
        let pages = page_count(&pdf)?;
        reports.push(PartReport {
            title: t.clone(),
            path: node.path.clone(),
            kind,
            pages,
            first_page: first,
            warnings,
        });
        first += pages;
        pdfs.push(pdf);
    }
    let about = about_page(&title, &revision, &reports, layers)?;
    let about_pages = page_count(&about)?;

    // Assemble with PdfCraft.
    let open = |b: &Vec<u8>| {
        pdfcraft_cos::Document::open(Arc::new(b.clone()))
            .map_err(|e| PackError::Assemble(e.to_string()))
    };
    let docs: Vec<pdfcraft_cos::Document> = pdfs.iter().map(open).collect::<Result<_, _>>()?;
    let about_doc = open(&about)?;
    let mut sources: Vec<(&str, &pdfcraft_cos::Document)> = reports
        .iter()
        .map(|r| r.title.as_str())
        .zip(docs.iter())
        .collect();
    sources.push(("About this pack", &about_doc));
    let mut doc =
        pdfcraft_organize::combine(&sources).map_err(|e| PackError::Assemble(e.to_string()))?;
    // DeckCraft bookmarks each slide "Slide N"; name them by their titles instead.
    let existing = pdfcraft_organize::bookmarks(&doc);
    for (i, (node, _)) in nodes.iter().enumerate() {
        if !node.attributes.contains_key("deck::deck") {
            continue;
        }
        let kids = existing
            .iter()
            .filter(|b| b.path.len() == 2 && b.path[0] == i)
            .count();
        for (j, st) in slide_titles(node).iter().enumerate() {
            let r = if j < kids {
                pdfcraft_organize::rename_bookmark(&mut doc, &[i, j], st)
            } else {
                pdfcraft_organize::add_bookmark(&mut doc, &[i], j, st, reports[i].first_page + j)
                    .map(|_| ())
            };
            r.map_err(|e| PackError::Assemble(e.to_string()))?;
        }
    }
    // A running footer on every page: pack and part on the left, the revision in the middle,
    // continuous page numbers on the right (PdfCraft marks them as pagination artifacts, so
    // screen readers and text extraction can skip them).
    let mut ranges: Vec<(String, usize, usize, bool)> = reports
        .iter()
        .map(|r| (r.title.clone(), r.first_page, r.pages, r.kind == "deck"))
        .collect();
    ranges.push(("About this pack".to_string(), first, about_pages, false));
    for (part, start, pages, slides) in &ranges {
        if *pages == 0 {
            continue;
        }
        let hf = pdfcraft_edit::HeaderFooter {
            text: [
                String::new(),
                String::new(),
                String::new(),
                format!("{title} \u{b7} {part}"),
                format!("revision {}", &revision[..8.min(revision.len())]),
                "<<Page 1 of n>>".to_string(),
            ],
            font_size: if *slides { 7.0 } else { 7.5 },
            color: [0.35, 0.38, 0.45],
            // top, bottom, left, right: slides have no margin to spare, so hug the edge.
            margins: if *slides {
                [10.0, 8.0, 18.0, 18.0]
            } else {
                [24.0, 22.0, 36.0, 36.0]
            },
            start_number: (*start + 1) as u32,
            ..pdfcraft_edit::HeaderFooter::default()
        };
        let idx: Vec<usize> = (*start..*start + *pages).collect();
        pdfcraft_edit::add_header_footer(
            &mut doc,
            &idx,
            &hf,
            true,
            &pdfcraft_edit::Context::default(),
        )
        .map_err(|e| PackError::Assemble(format!("footer on {part}: {e}")))?;
    }
    let set = |doc: &mut pdfcraft_cos::Document, k: &str, v: &str| {
        pdfcraft_organize::set_info(doc, k, v).map_err(|e| PackError::Assemble(e.to_string()))
    };
    set(&mut doc, "Title", &title)?;
    set(
        &mut doc,
        "Subject",
        "Milleleaf pack: every part from one composed snapshot",
    )?;
    set(
        &mut doc,
        "Keywords",
        &format!("oxpack={pack_path} oxrevision={revision}"),
    )?;
    set(
        &mut doc,
        "Creator",
        "Milleleaf (WordCraft, GridCraft, DeckCraft, PdfCraft)",
    )?;
    let bytes = pdfcraft_cos::write_full(&doc, &pdfcraft_cos::SaveOptions::default())
        .map_err(|e| PackError::Assemble(e.to_string()))?;
    Ok((
        bytes,
        PackReport {
            title,
            revision,
            pages: first + about_pages,
            parts: reports,
        },
    ))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use ox_core::Composer;

    use super::*;

    const PACK_NODE: &str = "3f1c2a90-0000-4c1e-9a00-000000000040";

    fn example(file: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/hello-table")
            .join(file)
    }

    fn build_from(stack_file: &str) -> (Vec<u8>, PackReport) {
        let stack = ox_core::load_stack(&example(stack_file)).expect("stack");
        let layers: Vec<LayerInfo> = stack
            .iter()
            .map(|l| LayerInfo {
                id: l.layer.header.id.clone(),
                kind: l.layer.header.kind.clone().unwrap_or_default(),
                author: l.layer.header.author.clone(),
                timestamp: l.layer.header.timestamp.clone(),
            })
            .collect();
        let flat = ox_core::compose::flatten(&stack);
        let trees = Composer::new(&flat).compose_all().expect("composes");
        build(&trees, PACK_NODE, &layers, &example("")).expect("builds")
    }

    #[test]
    fn one_pdf_with_every_part_bookmarked_and_the_revision_recorded() {
        let (pdf, report) = build_from("project.ox.json");
        let kinds: Vec<&str> = report.parts.iter().map(|p| p.kind).collect();
        assert_eq!(kinds, vec!["document", "workbook", "deck"]);
        assert_eq!(report.parts[2].pages, 3, "one page per slide");
        let doc = pdfcraft_cos::Document::open(Arc::new(pdf)).expect("opens");
        assert_eq!(
            pdfcraft_organize::page_count(&doc).expect("pages"),
            report.pages
        );
        let titles: Vec<String> = pdfcraft_organize::bookmarks(&doc)
            .into_iter()
            .map(|b| b.title)
            .collect();
        for t in [
            "Q3 Board Report",
            "Finance workbook",
            "Board deck",
            "Revenue by region",
            "About this pack",
        ] {
            assert!(titles.iter().any(|x| x == t), "{t} in {titles:?}");
        }
        assert!(
            !titles.iter().any(|t| t.starts_with("Slide ")),
            "{titles:?}"
        );
        let keywords = pdfcraft_organize::info(&doc, "Keywords").unwrap_or_default();
        assert!(
            keywords.contains(&format!("oxrevision={}", report.revision)),
            "{keywords}"
        );
    }

    #[test]
    fn the_revision_follows_the_numbers() {
        // Stacking the workbook edits changes what the pack says, so its revision changes.
        let (_, before) = build_from("project.ox.json");
        let (_, after) = build_from("with-edits.ox.json");
        assert_ne!(before.revision, after.revision);
    }
}
