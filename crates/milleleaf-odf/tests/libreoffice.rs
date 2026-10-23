//! The same edits made in LibreOffice and saved as ODT, ODS and ODP come back as exactly the
//! edit layers the OOXML versions give. They need LibreOffice, so they are ignored by default and
//! counted as ignored, not passed: run them with `cargo test -p milleleaf-odf --test libreoffice -- --ignored`.
//! Run that way, a missing LibreOffice fails them.

use std::path::PathBuf;

use milleleaf_core::render::find;
use milleleaf_core::{Composed, Composer, Header, Layer};

fn example(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/hello-table")
        .join(file)
}

fn trees() -> Vec<Composed> {
    let flat = milleleaf_core::open(&example("project.mleaf.json")).expect("stack");
    Composer::new(&flat).compose_all().expect("composes")
}

fn header() -> Header {
    Header {
        id: "odf-test".into(),
        mleaf_version: String::new(),
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

/// One LibreOffice at a time: two starting together can still fail a conversion now and then,
/// even with separate profiles and a retry, and these tests check results, not concurrency.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn need_soffice() -> std::sync::MutexGuard<'static, ()> {
    let turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    assert!(milleleaf_odf::soffice().is_some(), "these tests need LibreOffice (soffice on PATH, or MLEAF_SOFFICE)");
    turn
}

fn odf(name: &str) -> Vec<u8> {
    let _turn = need_soffice();
    let bytes = std::fs::read(example("fixtures").join(name)).expect("fixture");
    let ext = name.rsplit('.').next().unwrap_or("");
    milleleaf_odf::from_odf(&bytes, ext).expect("converts")
}

#[test]
#[ignore = "needs LibreOffice"]
fn odt_edits_match_the_docx_edits() {
    let docx = odf("report.libreoffice-edited.odt");
    let t = trees();
    let node = find(&t, "3f1c2a90-0000-4c1e-9a00-000000000020").expect("report");
    let doc = milleleaf_wordcraft::from_docx(&docx).expect("reads");
    let (layer, report) = milleleaf_wordcraft::import(node, &doc, header()).expect("imports");
    assert_eq!(report.conflicts.len(), 1);
    assert_eq!(data(&layer), data(&committed("word-edit.mleaf.json")));
}

#[test]
#[ignore = "needs LibreOffice"]
fn ods_edits_match_the_xlsx_edits() {
    let xlsx = odf("finance.libreoffice-edited.ods");
    let t = trees();
    let node = find(&t, "3f1c2a90-0000-4c1e-9a00-000000000010").expect("workbook");
    let mut wb = milleleaf_gridcraft::from_xlsx(&xlsx).expect("reads");
    // As the CLI does: this XLSX was written by LibreOffice.
    let (layer, report) = milleleaf_gridcraft::import_saved_by(node, &mut wb, header(), Some("LibreOffice"), None).expect("imports");
    assert_eq!(report.renamed.len(), 1);
    assert_eq!(data(&layer), data(&committed("grid-edit.mleaf.json")));
}

#[test]
#[ignore = "needs LibreOffice"]
fn odp_edits_match_the_pptx_edits() {
    let pptx = odf("deck.libreoffice-edited.odp");
    let t = trees();
    let node = find(&t, "2af44a75-c514-5c87-b208-689fa8b15950").expect("deck");
    let p = milleleaf_deckcraft::from_pptx(&pptx).expect("reads");
    let (layer, report) = milleleaf_deckcraft::import(node, &p, header()).expect("imports");
    assert_eq!(report.conflicts.len(), 1);
    assert_eq!(data(&layer), data(&committed("deck-edit.mleaf.json")));
}

#[test]
#[ignore = "needs LibreOffice"]
fn exports_convert_and_come_back_unchanged() {
    let _turn = need_soffice();
    let t = trees();
    let node = find(&t, "3f1c2a90-0000-4c1e-9a00-000000000010").expect("workbook");
    let (wb, _) = milleleaf_gridcraft::export(node, std::path::Path::new("")).expect("exports");
    let ods = milleleaf_odf::to_odf(&milleleaf_gridcraft::to_xlsx(&wb).expect("xlsx"), "ods").expect("to ods");
    assert!(ods.starts_with(b"PK"));
    let mut back =
        milleleaf_gridcraft::from_xlsx(&milleleaf_odf::from_odf(&ods, "ods").expect("from ods")).expect("reads");
    let (layer, _) = milleleaf_gridcraft::import_saved_by(node, &mut back, header(), Some("LibreOffice"), None).expect("imports");
    assert!(layer.data.is_empty(), "{:?}", layer.data);
}
