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
    let (wb, report) = export(&wb_node, std::path::Path::new("")).expect("exports");
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
    let (out, _) = export(&node(&trees, "adopted-wb"), std::path::Path::new("")).expect("exports");
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

#[test]
fn comments_merges_panes_and_names_added_in_excel_come_back() {
    use gridcraft_core::RangeRef;
    let (wb_node, wb) = exported();
    let mut wb = from_xlsx(&to_xlsx(&wb).unwrap()).unwrap();
    {
        let sh = std::sync::Arc::make_mut(&mut wb.sheets[0]);
        sh.comments.insert(
            CellRef::parse("B2").unwrap(),
            gridcraft_model::Comment {
                author: "Reviewer".into(),
                text: "Check this".into(),
                replies: vec![],
                threaded: false,
                resolved: false,
                visible: false,
                dates: vec![],
            },
        );
        sh.merges.push(RangeRef::parse("A12:B12").unwrap());
        sh.freeze = Some((1, 0));
    }
    // A cell made bold with a yellow fill.
    let mut st = wb.styles.get(wb.sheets[0].cell(CellRef::parse("A2").unwrap()).unwrap().style).clone();
    st.font.bold = true;
    st.fill = gridcraft_model::Fill::solid(gridcraft_model::Color::Rgb(0xFFFF00));
    let sid = wb.styles.intern(st);
    {
        let sh = std::sync::Arc::make_mut(&mut wb.sheets[0]);
        let mut c = sh.cell(CellRef::parse("A2").unwrap()).cloned().unwrap();
        c.style = sid;
        sh.set_cell(CellRef::parse("A2").unwrap(), c);
    }
    wb.names.push(gridcraft_model::DefinedName {
        name: "Regions".into(),
        scope: None,
        formula: "Q3!$A$2:$A$5".into(),
        comment: String::new(),
        hidden: false,
    });
    let mut wb = from_xlsx(&to_xlsx(&wb).unwrap()).unwrap();
    let (layer, report) = import(&wb_node, &mut wb, header()).expect("imports");
    assert!(report.changed.contains(&SHEET.to_string()), "{report:?}");
    let gs = &layer.data.iter().find(|n| n.path == SHEET).expect("sheet").attributes["grid::sheet"];
    assert_eq!(gs["comments"]["B2"]["text"], json!("Check this"));
    assert_eq!(gs["merges"], json!(["A12:B12"]));
    assert_eq!(gs["freeze"], json!({ "rows": 1, "cols": 0 }));
    let gw = &layer.data.iter().find(|n| n.path == WORKBOOK).expect("workbook").attributes["grid::workbook"];
    assert_eq!(gw["names"], json!([{ "name": "Regions", "formula": "'Q3'!$A$2:$A$5" }]), "GridCraft quotes the sheet name");

    // And they export again.
    let trees = compose(&[layer]);
    let (out, report) = export(&node(&trees, WORKBOOK), std::path::Path::new("")).expect("exports");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let sh = &out.sheets[0];
    assert_eq!(sh.comments[&CellRef::parse("B2").unwrap()].text, "Check this");
    assert_eq!(sh.freeze, Some((1, 0)));
    assert!(out.names.iter().any(|n| n.name == "Regions"));
    let a2 = out.styles.get(sh.cell(CellRef::parse("A2").unwrap()).unwrap().style);
    assert!(a2.font.bold && a2.fill.pattern != gridcraft_model::PatternType::None, "the bold, filled cell exports as it was");
}

/// The real finance workbook, adopted the way `ox adopt` does it (`examples/real/finance.xlsx`).
fn finance_node() -> Composed {
    let real = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/real");
    // `ox adopt`'s path for this file: stable_id("adopt|" + its SHA-256).
    let root = "382bc55d-0f5e-5e81-a9ba-10f117e753d2";
    let wb = from_xlsx(&std::fs::read(real.join("finance.xlsx")).unwrap()).unwrap();
    let (layer, _) = crate::adopt(&wb, root, "finance", header()).expect("adopts");
    let mut loaded = ox_core::load_stack(&example("../../schema/office.ox.json")).expect("schema");
    loaded.push(ox_core::LoadedLayer { source: PathBuf::from("memory"), layer });
    let flat = ox_core::compose::flatten(&loaded);
    let trees = Composer::new(&flat).compose_all().expect("composes");
    find(&trees, root).expect("workbook").clone()
}

fn real_fixture(name: &str) -> Workbook {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/real/fixtures").join(name);
    from_xlsx(&std::fs::read(p).expect("fixture")).expect("reads")
}

#[test]
#[ignore = "about 2.5 min in a debug build (GridCraft recalculates 26k formulas); run.sh step 18 runs it"]
fn real_workbook_resaves_are_no_change_and_page_setup_edits_come_back() {
    let node = finance_node();
    // The two floating pictures on its first sheet come in by hash.
    let pics: Vec<&serde_json::Value> = node.children.iter().filter_map(|c| c.attributes.get("grid::sheet")?.get("pictures")).flat_map(|p| p.as_array().into_iter().flatten()).collect();
    assert_eq!(pics.len(), 2, "{pics:?}");
    assert!(pics.iter().all(|p| p["blob"].as_str().unwrap().starts_with("sha256-")));
    // Its nine line and pie charts come in, pointing at their cells.
    let charts: Vec<&serde_json::Value> = node.children.iter().filter_map(|c| c.attributes.get("grid::sheet")?.get("charts")).flat_map(|p| p.as_array().into_iter().flatten()).collect();
    assert_eq!(charts.len(), 9);
    assert_eq!(node.attributes["grid::workbook"]["theme"]["name"], json!("Office 2013 - 2022 Theme"), "its own theme, not GridCraft's");
    // LibreOffice 24.2: recalculated digits, its own row heights, borders, styles, comment authors,
    // header/footer codes… all its rewrites. It drops the structure lock: kept, with a warning.
    let mut lo = real_fixture("finance.libreoffice-resaved.xlsx");
    let (layer, report) =
        crate::import_saved_by(&node, &mut lo, header(), Some("LibreOffice/24.2.7.2$Linux_X86_64"), None).expect("imports");
    assert!(layer.data.is_empty() && report.changed.is_empty(), "{:?}", report.changed);
    assert!(report.warnings.iter().any(|w| w.contains("structure lock")), "{:?}", report.warnings);

    // Excel 16 (Mac): sheet Output's left margin set to 1 in and its Standard Width to 12. Exactly
    // that comes back; Excel's own rewrites (row heights, default width, trailing spaces) don't.
    let mut xl = real_fixture("finance.msoffice-margins.xlsx");
    let (layer, report) = crate::import_saved_by(&node, &mut xl, header(), Some("Microsoft Macintosh Excel"), None).expect("imports");
    assert_eq!(report.changed.len(), 1, "{:?}", report.changed);
    let gs = &layer.data[0].attributes["grid::sheet"];
    assert_eq!(gs["name"], "Output");
    assert_eq!(gs["print"]["margins"][0], json!(1.0));
    assert_eq!(gs["defaults"]["colWidth"], json!(90.0), "12 characters");
}

#[test]
fn sheet_pictures_come_back_and_a_swapped_one_is_a_change() {
    use gridcraft_model::{Anchor, AnchorMode, Image, Sheet};
    let mut sh = Sheet::new("S");
    let anchor = Anchor { cell: CellRef::new(1, 1), dx: 2.0, dy: 3.0, width: 40.0, height: 30.0, mode: AnchorMode::MoveOnly };
    sh.images.push(Image { id: 1, anchor, data: b"picture bytes".to_vec(), mime: "image/png".into(), alt: "logo".into() });
    let mut pics = crate::Pictures::new();
    let look = crate::look::sheet_look(&sh, &mut pics);
    let blob = look["pictures"][0]["blob"].as_str().unwrap().to_string();
    assert_eq!(pics[&blob].1, b"picture bytes");
    assert_eq!(look["pictures"][0]["at"], json!("B2"));

    // The importer's comparison export has no files: a stand-in reads back as the same value.
    let mut back = Sheet::new("S");
    crate::look::apply_look(&mut back, &look, "S", &mut vec![], ox_core::PictureSource::StandIn(None)).unwrap();
    let mut stand_in = crate::Pictures::new();
    assert_eq!(crate::look::sheet_look(&back, &mut stand_in)["pictures"], look["pictures"]);
    assert!(stand_in.is_empty(), "a stand-in isn't a picture to store");

    // An editor nudging it by half a point is no change; a different picture is.
    let mut nudged = look.clone();
    nudged["pictures"][0]["dx"] = json!(2.5);
    crate::look::settle(&mut nudged, &look, false);
    assert_eq!(nudged["pictures"], look["pictures"]);
    let mut swapped = look.clone();
    swapped["pictures"][0]["blob"] = json!(ox_core::integrity_of(b"another"));
    crate::look::settle(&mut swapped, &look, false);
    assert_ne!(swapped["pictures"], look["pictures"]);

    // An export without the picture's bytes stops, naming it.
    let err = crate::look::apply_look(&mut Sheet::new("S"), &look, "S", &mut vec![], ox_core::PictureSource::Read(std::path::Path::new("/nonexistent"))).unwrap_err();
    assert!(err.to_string().contains(&blob), "{err}");
}

#[test]
fn sheet_charts_come_back_through_xlsx_and_an_edited_one_is_a_change() {
    use gridcraft_model::{Anchor, AnchorMode, Chart, ChartKind, Color, LegendPos, Series};
    let mut wb = Workbook::new();
    let sh = wb.sheet_mut(0).unwrap();
    sh.name = "Data".into();
    let series = |v: &str, c| Series { name: Some("Data!$B$1".into()), categories: Some("Data!$A$2:$A$4".into()), values: v.into(), color: Some(c), ..Series::default() };
    sh.charts.push(Chart {
        id: 1,
        kind: ChartKind::Line,
        anchor: Anchor { cell: CellRef::new(5, 1), dx: 0.0, dy: 0.0, width: 300.0, height: 200.0, mode: AnchorMode::MoveAndSize },
        title: Some("Totals".into()),
        series: vec![series("Data!$B$2:$B$4", Color::Theme(4, 0)), series("'Gone'!#REF!", Color::Theme(5, 0))],
        legend: LegendPos::Bottom,
        data_labels: false,
        gridlines: true,
        style: 2,
        x_title: None,
        y_title: None,
        source: None,
        by_rows: false,
        pivot: None,
    });
    let look = crate::look::sheet_look(&wb.sheets[0], &mut crate::Pictures::new());
    assert_eq!(look["charts"][0]["series"][0]["values"], json!("Data!$B$2:$B$4"), "a chart points at its cells");

    // Exported, written by GridCraft and read back (theme colours become RGB, `'Gone'!#REF!`
    // becomes `#REF!`): the same chart.
    let mut out = Workbook::new();
    out.sheet_mut(0).unwrap().name = "Data".into();
    crate::look::apply_look(out.sheet_mut(0).unwrap(), &look, "Data", &mut vec![], ox_core::PictureSource::StandIn(None)).unwrap();
    let back = from_xlsx(&to_xlsx(&out).unwrap()).unwrap();
    let now = crate::look::sheet_look(&back.sheets[0], &mut crate::Pictures::new());
    assert_ne!(now["charts"], look["charts"], "GridCraft rewrites the colours");
    assert!(crate::look::same_charts(&now["charts"], &look["charts"], &back.theme, false));

    // LibreOffice's rewrites: re-anchored two rows down as move-only, a broken series dropped, a
    // literal name quoted. No change from LibreOffice; from anyone else they would be.
    let mut lo = now.clone();
    lo["charts"][0]["anchor"]["cell"]["row"] = json!(7);
    lo["charts"][0]["anchor"]["mode"] = json!("MoveOnly");
    lo["charts"][0]["series"].as_array_mut().unwrap().pop();
    lo["charts"][0]["series"][0]["name"] = json!("\"Data!$B$1\"");
    assert!(crate::look::same_charts(&lo["charts"], &look["charts"], &back.theme, true));
    assert!(!crate::look::same_charts(&lo["charts"], &look["charts"], &back.theme, false));

    // A retitled chart or one pointing at other cells is a change.
    let mut retitled = now.clone();
    retitled["charts"][0]["title"] = json!("Totals 2026");
    assert!(!crate::look::same_charts(&retitled["charts"], &look["charts"], &back.theme, false));
    let mut moved = now.clone();
    moved["charts"][0]["series"][0]["values"] = json!("Data!$C$2:$C$4");
    assert!(!crate::look::same_charts(&moved["charts"], &look["charts"], &back.theme, false));
}
