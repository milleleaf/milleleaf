//! The same edits made in LibreOffice and saved as ODT, ODS and ODP come back as exactly the
//! edit layers the OOXML versions give. Skipped (with a note) when LibreOffice isn't installed.

use std::path::PathBuf;

use ox_core::render::find;
use ox_core::{Composed, Composer, Header, Layer};

fn example(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/hello-table")
        .join(file)
}

fn trees() -> Vec<Composed> {
    let flat = ox_core::open(&example("project.ox.json")).expect("stack");
    Composer::new(&flat).compose_all().expect("composes")
}

fn header() -> Header {
    Header {
        id: "odf-test".into(),
        ox_version: String::new(),
        data_version: "1.0.0".into(),
        author: "test".into(),
        timestamp: "2026-10-15".into(),
        kind: None,
        ..Default::default()
    }
}

/// Node statements sorted by path, for comparing layers made from different files.
fn data(layer: &Layer) -> String {
    let mut nodes = layer.data.clone();
    nodes.sort_by(|a, b| a.path.cmp(&b.path));
    serde_json::to_string(&nodes).expect("json")
}

fn committed(file: &str) -> Layer {
    serde_json::from_slice(&std::fs::read(example(file)).expect("layer")).expect("parses")
}

fn odf(name: &str) -> Option<Vec<u8>> {
    if ox_odf::soffice().is_none() {
        eprintln!("skipped: LibreOffice isn't installed");
        return None;
    }
    let bytes = std::fs::read(example("fixtures").join(name)).expect("fixture");
    let ext = name.rsplit('.').next().unwrap_or("");
    Some(ox_odf::from_odf(&bytes, ext).expect("converts"))
}

#[test]
fn odt_edits_match_the_docx_edits() {
    let Some(docx) = odf("report.libreoffice-edited.odt") else {
        return;
    };
    let t = trees();
    let node = find(&t, "3f1c2a90-0000-4c1e-9a00-000000000020").expect("report");
    let doc = ox_wordcraft::from_docx(&docx).expect("reads");
    let (layer, report) = ox_wordcraft::import(node, &doc, header()).expect("imports");
    assert_eq!(report.conflicts.len(), 1);
    assert_eq!(data(&layer), data(&committed("word-edit.ox.json")));
}

#[test]
fn ods_edits_match_the_xlsx_edits() {
    let Some(xlsx) = odf("finance.libreoffice-edited.ods") else {
        return;
    };
    let t = trees();
    let node = find(&t, "3f1c2a90-0000-4c1e-9a00-000000000010").expect("workbook");
    let mut wb = ox_gridcraft::from_xlsx(&xlsx).expect("reads");
    // As the CLI does: this XLSX was written by LibreOffice.
    let (layer, report) = ox_gridcraft::import_saved_by(node, &mut wb, header(), Some("LibreOffice")).expect("imports");
    assert_eq!(report.renamed.len(), 1);
    assert_eq!(data(&layer), data(&committed("grid-edit.ox.json")));
}

#[test]
fn odp_edits_match_the_pptx_edits() {
    let Some(pptx) = odf("deck.libreoffice-edited.odp") else {
        return;
    };
    let t = trees();
    let node = find(&t, "2af44a75-c514-5c87-b208-689fa8b15950").expect("deck");
    let p = ox_deckcraft::from_pptx(&pptx).expect("reads");
    let (layer, report) = ox_deckcraft::import(node, &p, header()).expect("imports");
    assert_eq!(report.conflicts.len(), 1);
    assert_eq!(data(&layer), data(&committed("deck-edit.ox.json")));
}

#[test]
fn exports_convert_and_come_back_unchanged() {
    if ox_odf::soffice().is_none() {
        eprintln!("skipped: LibreOffice isn't installed");
        return;
    }
    let t = trees();
    let node = find(&t, "3f1c2a90-0000-4c1e-9a00-000000000010").expect("workbook");
    let (wb, _) = ox_gridcraft::export(node, std::path::Path::new("")).expect("exports");
    let ods = ox_odf::to_odf(&ox_gridcraft::to_xlsx(&wb).expect("xlsx"), "ods").expect("to ods");
    assert!(ods.starts_with(b"PK"));
    let mut back =
        ox_gridcraft::from_xlsx(&ox_odf::from_odf(&ods, "ods").expect("from ods")).expect("reads");
    let (layer, _) = ox_gridcraft::import_saved_by(node, &mut back, header(), Some("LibreOffice")).expect("imports");
    assert!(layer.data.is_empty(), "{:?}", layer.data);
}
