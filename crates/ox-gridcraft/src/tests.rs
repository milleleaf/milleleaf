use std::path::PathBuf;

use gridcraft_core::{CellRef, Value as GValue};
use gridcraft_model::Workbook;
use ox_core::render::{field_text, find};
use ox_core::{Composed, Composer, Header, Layer};
use serde_json::json;

use crate::{export, from_xlsx, import, name_for, to_xlsx};

const WORKBOOK: &str = "3f1c2a90-0000-4c1e-9a00-000000000010";
const SHEET: &str = "3f1c2a90-0000-4c1e-9a00-000000000011";
const TABLE: &str = "3f1c2a90-0000-4c1e-9a00-000000000012";
const TOTAL: &str = "3f1c2a90-0000-4c1e-9a00-000000000013";
const REPORT: &str = "3f1c2a90-0000-4c1e-9a00-000000000020";
const SUMMARY: &str = "3f1c2a90-0000-4c1e-9a00-000000000023";

fn example(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/hello-table")
        .join(file)
}

/// The example project, plus extra in-memory layers on top.
fn compose(extra: &[Layer]) -> Vec<Composed> {
    let mut loaded = ox_core::load_stack(&example("project.ox.json")).expect("stack loads");
    for l in extra {
        loaded.push(ox_core::LoadedLayer {
            source: PathBuf::from("memory"),
            layer: l.clone(),
        });
    }
    let flat = ox_core::compose::flatten(&loaded);
    Composer::new(&flat).compose_all().expect("composes")
}

fn node(trees: &[Composed], path: &str) -> Composed {
    find(trees, path).expect("node in tree").clone()
}

fn header() -> Header {
    Header {
        id: "test-grid-edit".into(),
        ox_version: String::new(),
        data_version: "1.0.0".into(),
        author: "test".into(),
        timestamp: "2026-10-15".into(),
        kind: None,
        ..Default::default()
    }
}

fn fixture(name: &str) -> Workbook {
    from_xlsx(&std::fs::read(example("fixtures").join(name)).expect("fixture")).expect("reads")
}

fn exported() -> (Composed, Workbook) {
    let trees = compose(&[]);
    let wb_node = node(&trees, WORKBOOK);
    let (wb, report) = export(&wb_node).expect("exports");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert!(report.stale_values.is_empty(), "{:?}", report.stale_values);
    let wb = from_xlsx(&to_xlsx(&wb).expect("writes")).expect("reads back");
    (wb_node, wb)
}

#[test]
fn export_places_data_names_it_and_recalculates() {
    let (_, wb) = exported();
    let sh = wb.sheet(0).expect("sheet");
    assert_eq!(sh.name, "Q3");
    assert_eq!(
        sh.value(CellRef::parse("A3").expect("ref")),
        GValue::text("Hong Kong")
    );
    // The finance review's restated figure, and the formula's result over it.
    assert_eq!(
        sh.value(CellRef::parse("B3").expect("ref")),
        GValue::number(541500.0)
    );
    let total = sh
        .cell(CellRef::parse("B5").expect("ref"))
        .expect("total cell");
    assert_eq!(
        total.formula.as_ref().map(|f| f.text.as_str()),
        Some("SUM(B2:B4)")
    );
    assert_eq!(total.value, GValue::number(1250500.0));
    let names: Vec<&str> = wb.names.iter().map(|n| n.name.as_str()).collect();
    assert!(
        names.contains(&name_for(TABLE).as_str()) && names.contains(&"ox_workbook"),
        "{names:?}"
    );
    let table_name = wb
        .names
        .iter()
        .find(|n| n.name == name_for(TABLE))
        .expect("table name");
    // "Q3" reads as a cell reference, so the sheet name is quoted.
    assert_eq!(table_name.formula, "'Q3'!$A$1:$B$4");
}

#[test]
fn unedited_and_libreoffice_resaved_import_as_no_change() {
    let (wb_node, mut wb) = exported();
    let (layer, report) = import(&wb_node, &mut wb, header()).expect("imports");
    assert!(
        report.is_empty() && layer.data.is_empty() && !report.stale,
        "{report:?}"
    );
    for name in [
        "finance.libreoffice-resaved.xlsx",
        "finance.msoffice-resaved.xlsx",
    ] {
        let mut resaved = fixture(name);
        let (layer, report) = import(&wb_node, &mut resaved, header()).expect("imports");
        assert!(
            report.is_empty() && layer.data.is_empty(),
            "{name}: {report:?} {:?}",
            layer.data
        );
    }
}

#[test]
fn libreoffice_edits_come_back_with_moved_ranges() {
    edits_come_back_with_moved_ranges("finance.libreoffice-edited.xlsx");
}

#[test]
fn excel_edits_come_back_with_moved_ranges() {
    edits_come_back_with_moved_ranges("finance.msoffice-edited.xlsx");
}

fn edits_come_back_with_moved_ranges(name: &str) {
    // Singapore restated to 305,250, a Perth row inserted inside the table, the sheet renamed
    // "Q3 2026". LibreOffice and Excel both moved the names and widened SUM(B2:B4) to SUM(B2:B5).
    let wb_node = node(&compose(&[]), WORKBOOK);
    let mut wb = fixture(name);
    let (layer, report) = import(&wb_node, &mut wb, header()).expect("imports");
    assert_eq!(report.renamed, vec!["Q3 → Q3 2026".to_string()]);
    assert_eq!(report.moved, vec![TABLE.to_string(), TOTAL.to_string()]);
    assert_eq!(report.changed, vec![TABLE.to_string(), TOTAL.to_string()]);
    let table = layer.data.iter().find(|n| n.path == TABLE).expect("table");
    assert_eq!(
        table.attributes["office::data::table"]["rows"],
        json!([
            ["Adelaide", 412000],
            ["Hong Kong", 541500],
            ["Perth", 120000],
            ["Singapore", 305250]
        ])
    );
    assert_eq!(
        table.attributes["grid::range"],
        json!({ "sheet": "Q3 2026", "ref": "A1:B5" })
    );
    let total = layer.data.iter().find(|n| n.path == TOTAL).expect("total");
    assert_eq!(
        total.attributes["office::data::value"]["value"],
        json!(1378750)
    );
    assert_eq!(
        total.attributes["grid::formula"],
        json!({ "text": "=SUM(B2:B5)" })
    );
    let sheet = layer.data.iter().find(|n| n.path == SHEET).expect("sheet");
    assert_eq!(sheet.attributes["grid::sheet"]["name"], json!("Q3 2026"));

    // Stacked: the report's linked table and live total follow the workbook.
    let trees = compose(&[layer]);
    let report_node = node(&trees, REPORT);
    let summary = report_node
        .children
        .iter()
        .find(|c| c.path == SUMMARY)
        .expect("summary");
    assert_eq!(field_text(summary, "total"), "$1,378,750");
    let linked = report_node
        .children
        .iter()
        .find(|c| c.attributes.contains_key("office::data::table"))
        .expect("report table");
    assert_eq!(
        linked.attributes["office::data::table"]["rows"]
            .as_array()
            .map(Vec::len),
        Some(4)
    );
}

#[test]
fn gridcraft_recomputes_what_a_non_calculating_editor_left_stale() {
    // openpyxl changed Hong Kong to 550,000 but doesn't calculate: the file's cached total is
    // stale. GridCraft's calc engine recomputes it on import.
    let wb_node = node(&compose(&[]), WORKBOOK);
    let mut wb = fixture("finance.openpyxl-edited.xlsx");
    let (layer, report) = import(&wb_node, &mut wb, header()).expect("imports");
    assert_eq!(report.recomputed, vec![TOTAL.to_string()]);
    let total = layer.data.iter().find(|n| n.path == TOTAL).expect("total");
    assert_eq!(
        total.attributes["office::data::value"]["value"],
        json!(1259000)
    );
}

#[test]
fn wrong_workbook_is_refused() {
    let (mut wb_node, mut wb) = exported();
    wb_node.path = "another-workbook".into();
    assert!(import(&wb_node, &mut wb, header()).is_err());
}

#[test]
fn excel_and_libreoffice_matches_are_all_certain() {
    let wb_node = node(&compose(&[]), WORKBOOK);
    for name in [
        "finance.libreoffice-edited.xlsx",
        "finance.msoffice-edited.xlsx",
    ] {
        let mut wb = fixture(name);
        let (_, report) = import(&wb_node, &mut wb, header()).expect("imports");
        // The sheet and both ranges, by their defined names.
        assert_eq!(report.matches.len(), 3, "{name}: {:?}", report.matches);
        assert!(ox_core::matching::best_guesses(&report.matches).is_empty());
    }
}

#[test]
fn a_copied_sheets_names_make_the_match_a_best_guess() {
    let (wb_node, mut wb) = exported();
    // Copying a sheet copies its names as sheet-scoped ones pointing at the copy.
    let want = name_for(TABLE);
    let mut dup = wb
        .names
        .iter()
        .find(|n| n.name.eq_ignore_ascii_case(&want))
        .expect("table name")
        .clone();
    dup.scope = Some(0);
    wb.names.insert(0, dup);
    let (_layer, report) = import(&wb_node, &mut wb, header()).expect("imports");
    let guesses = ox_core::matching::best_guesses(&report.matches);
    assert!(guesses.iter().any(|m| m.path == TABLE), "{:?}", report.matches);
    assert!(report.warnings.iter().any(|w| w.contains("appears 2 times")), "{:?}", report.warnings);
}

#[test]
fn adopting_a_workbook_keeps_every_value_and_formula() {
    let wb = fixture("finance.msoffice-edited.xlsx");
    let (layer, report) = crate::adopt(&wb, "adopted-wb", "finance", header()).expect("adopts");
    assert_eq!((report.sheets, report.cells, report.formulas), (1, 11, 1));
    // Compose the adopted layer with the schema and export it: the total is recomputed.
    let mut loaded = ox_core::load_stack(&example("base.ox.json")).expect("schema");
    loaded.push(ox_core::LoadedLayer { source: PathBuf::from("memory"), layer });
    let flat = ox_core::compose::flatten(&loaded);
    let trees = Composer::new(&flat).compose_all().expect("composes");
    let (out, _) = export(&node(&trees, "adopted-wb")).expect("exports");
    let total = out.sheet(0).unwrap().value(CellRef::parse("B6").unwrap());
    assert_eq!(total, GValue::number(1378750.0));
}

/// Strings from tdf#173771 / tdf#150398 (emoji with variation selectors, non-BMP symbols, an
/// Anatolian hieroglyph) plus a ZWJ sequence and combining marks: Milleleaf must keep every code
/// point, whatever a renderer later does with them.
pub(crate) const UNICODE_CASES: [&str; 6] = [
    "5. ♖🪴🧾🚂🙭🎨🆞🆣🆫𝝿𔑲",
    "7,5. 🙭🆞🆣🆫𝝿𔑲",
    "VS16 ❤\u{FE0F} VS15 ❤\u{FE0E} ☎\u{FE0F}",
    "ZWJ 👩\u{200D}💻 flag 🇭🇰",
    "combining e\u{0301} n\u{0303} 한국어 العربية",
    "astral 𝔘𝔫𝔦𝔠𝔬𝔡𝔢 𓂀",
];

#[test]
fn unicode_survives_an_xlsx_round_trip_code_point_for_code_point() {
    let mut wb = Workbook::new();
    for (i, s) in UNICODE_CASES.iter().enumerate() {
        wb.sheet_mut(0).unwrap().set_value(CellRef { row: i as u32, col: 0 }, GValue::text(*s));
    }
    let back = from_xlsx(&to_xlsx(&wb).expect("writes")).expect("reads");
    for (i, s) in UNICODE_CASES.iter().enumerate() {
        let got = back.sheet(0).unwrap().value(CellRef { row: i as u32, col: 0 });
        assert_eq!(got, GValue::text(*s), "case {i}");
    }
}
