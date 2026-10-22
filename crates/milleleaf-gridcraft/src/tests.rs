use std::path::PathBuf;

use gridcraft_core::{CellRef, Value as GValue};
use gridcraft_model::Workbook;
use milleleaf_core::render::{field_text, find};
use milleleaf_core::{Composed, Composer, Header, Layer};
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
    let mut loaded = milleleaf_core::load_stack(&example("project.mleaf.json")).expect("stack loads");
    for l in extra {
        loaded.push(milleleaf_core::LoadedLayer {
            source: PathBuf::from("memory"),
            layer: l.clone(),
        });
    }
    let flat = milleleaf_core::compose::flatten(&loaded);
    Composer::new(&flat).compose_all().expect("composes")
}

fn node(trees: &[Composed], path: &str) -> Composed {
    find(trees, path).expect("node in tree").clone()
}

fn header() -> Header {
    Header {
        id: "test-grid-edit".into(),
        mleaf_version: String::new(),
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
        names.contains(&name_for(TABLE).as_str()) && names.contains(&"mleaf_workbook"),
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
        assert!(milleleaf_core::matching::best_guesses(&report.matches).is_empty());
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
    let guesses = milleleaf_core::matching::best_guesses(&report.matches);
    assert!(guesses.iter().any(|m| m.path == TABLE), "{:?}", report.matches);
    assert!(report.warnings.iter().any(|w| w.contains("appears 2 times")), "{:?}", report.warnings);
}

#[test]
fn adopting_a_workbook_keeps_every_value_and_formula() {
    let wb = fixture("finance.msoffice-edited.xlsx");
    let (layer, report) = crate::adopt(&wb, "adopted-wb", "finance", header()).expect("adopts");
    assert_eq!((report.sheets, report.cells, report.formulas), (1, 11, 1));
    // Compose the adopted layer with the schema and export it: the total is recomputed.
    let mut loaded = milleleaf_core::load_stack(&example("base.mleaf.json")).expect("schema");
    loaded.push(milleleaf_core::LoadedLayer { source: PathBuf::from("memory"), layer });
    let flat = milleleaf_core::compose::flatten(&loaded);
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

/// The real life-cycle model (examples/real/lca-model.xlsx: a sodium-ion battery LCA, 9 sheets,
/// 15,897 formulas, 58 charts), adopted the way `mleaf adopt` does it.
fn lca_node() -> Composed {
    let real = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/real");
    // `mleaf adopt`'s path for this file: stable_id("adopt|" + its SHA-256).
    let root = "53b3afd9-1cce-56e1-a8f1-96cd7e5984e9";
    let wb = from_xlsx(&std::fs::read(real.join("lca-model.xlsx")).unwrap()).unwrap();
    let (layer, _) = crate::adopt(&wb, root, "lca-model", header()).expect("adopts");
    let mut loaded = milleleaf_core::load_stack(&example("../../schema/office.mleaf.json")).expect("schema");
    loaded.push(milleleaf_core::LoadedLayer { source: PathBuf::from("memory"), layer });
    let flat = milleleaf_core::compose::flatten(&loaded);
    let trees = Composer::new(&flat).compose_all().expect("composes");
    find(&trees, root).expect("workbook").clone()
}

fn real_fixture(name: &str) -> Workbook {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/real/fixtures").join(name);
    from_xlsx(&std::fs::read(p).expect("fixture")).expect("reads")
}

/// A fixture only Microsoft Office can make (the Office packet): `None`, said out loud, until it's there.
fn office_fixture(name: &str) -> Option<Workbook> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/real/fixtures").join(name);
    if !p.exists() {
        eprintln!("PENDING: examples/real/fixtures/{name} (a Microsoft Office re-save) isn't there yet; that part isn't checked");
        return None;
    }
    Some(from_xlsx(&std::fs::read(p).expect("fixture")).expect("reads"))
}

#[test]
#[ignore = "minutes in a debug build (GridCraft recalculates 16k formulas); step 18 of the hello-table script runs it"]
fn real_workbook_resaves_are_no_change_and_page_setup_edits_come_back() {
    let node = lca_node();
    // The legend picture on the Graphs sheet comes in by hash.
    let pics: Vec<&serde_json::Value> = node.children.iter().filter_map(|c| c.attributes.get("grid::sheet")?.get("pictures")).flat_map(|p| p.as_array().into_iter().flatten()).collect();
    assert_eq!(pics.len(), 1, "{pics:?}");
    assert!(pics.iter().all(|p| p["blob"].as_str().unwrap().starts_with("sha256-")));
    // Its 50 bar, line, scatter and combo charts come in, pointing at their cells (its 8 surface
    // charts are a listed loss: GridCraft has no surface charts).
    let charts: Vec<&serde_json::Value> = node.children.iter().filter_map(|c| c.attributes.get("grid::sheet")?.get("charts")).flat_map(|p| p.as_array().into_iter().flatten()).collect();
    assert_eq!(charts.len(), 50);
    assert_eq!(node.attributes["grid::workbook"]["theme"]["name"], json!("Office Theme"), "its own theme, not GridCraft's");
    // LibreOffice 24.2: recalculated digits, its own styles (formula cells given their inputs'
    // number format, rotated text's General alignment written as Left), chart colours with their
    // shades, titles and categories written out: all its rewrites. Three charts it saves without
    // their data: kept, with a warning.
    let mut lo = real_fixture("lca-model.libreoffice-resaved.xlsx");
    let (layer, report) =
        crate::import_saved_by(&node, &mut lo, header(), Some("LibreOffice/24.2.7.2$Linux_X86_64"), None).expect("imports");
    assert!(layer.data.is_empty() && report.changed.is_empty(), "{:?}", report.changed);
    assert_eq!(report.warnings.iter().filter(|w| w.contains("without its data")).count(), 3, "{:?}", report.warnings);

    // Excel 16 (Mac): sheet Graphs' left margin set to 1 in and its Standard Width to 12. Exactly
    // that comes back; Excel's own rewrites (row heights, default width, trailing spaces, and the
    // chart sizes those row heights give) don't. The new width does widen the sheet's five charts
    // and its legend picture, which sit between cells over default-width columns (their cells
    // unchanged in the file): that's the edit as the file shows it, and comes back too.
    if let Some(mut xl) = office_fixture("lca-model.msoffice-margins.xlsx") {
        let (layer, report) = crate::import_saved_by(&node, &mut xl, header(), Some("Microsoft Macintosh Excel"), None).expect("imports");
        assert_eq!(report.changed.len(), 1, "{:?}", report.changed);
        let gs = &layer.data[0].attributes["grid::sheet"];
        assert_eq!(gs["name"], "Graphs");
        assert_eq!(gs["print"]["margins"][0], json!(1.0));
        assert_eq!(gs["defaults"]["colWidth"], json!(90.0), "12 characters");
        let widths: Vec<i64> = gs["charts"].as_array().unwrap().iter().map(|c| c["anchor"]["width"].as_f64().unwrap().round() as i64).collect();
        assert_eq!(widths, [446; 5]);
        assert_eq!(gs["pictures"][0]["w"], json!(411.24));
    }
    if let Some(mut xl) = office_fixture("lca-model.msoffice-resaved.xlsx") {
        let (layer, report) = crate::import_saved_by(&node, &mut xl, header(), Some("Microsoft Macintosh Excel"), None).expect("imports");
        assert!(layer.data.is_empty() && report.changed.is_empty(), "{:?}", report.changed);
    }
}

/// A chart's size as a file read back gives it when its cells (`xdr:from`, `xdr:to`) stay where
/// they are and `resize` changes the sizes of the rows or columns between them.
fn chart_on_resized_lines(sh: &mut gridcraft_model::Sheet, resize: impl FnOnce(&mut gridcraft_model::Sheet)) {
    let a = sh.charts[0].anchor.clone();
    let (x2, y2) = (sh.col_left(a.cell.col) + (a.dx + a.width) as f64, sh.row_top(a.cell.row) + (a.dy + a.height) as f64);
    let (tc, tr) = (sh.col_at(x2), sh.row_at(y2));
    let (tdx, tdy) = (x2 - sh.col_left(tc), y2 - sh.row_top(tr));
    resize(sh);
    let width = (sh.col_left(tc) + tdx - sh.col_left(a.cell.col) - a.dx as f64) as f32;
    let height = (sh.row_top(tr) + tdy - sh.row_top(a.cell.row) - a.dy as f64) as f32;
    (sh.charts[0].anchor.width, sh.charts[0].anchor.height) = (width, height);
}

#[test]
fn row_heights_excel_works_out_dont_resize_a_chart_and_a_new_standard_width_does() {
    use gridcraft_model::{Anchor, AnchorMode, Chart, ChartKind, Color, LegendPos, LineInfo, Series};
    let mut wb = Workbook::new();
    let sh = wb.sheet_mut(0).unwrap();
    sh.name = "Data".into();
    for (r, v) in [3.0, 5.0, 4.0].iter().enumerate() {
        sh.set_value(CellRef::new(r as u32 + 1, 1), GValue::number(*v));
    }
    sh.charts.push(Chart {
        id: 1,
        kind: ChartKind::Line,
        anchor: Anchor { cell: CellRef::new(5, 1), dx: 0.0, dy: 0.0, width: 300.0, height: 200.0, mode: AnchorMode::MoveAndSize },
        title: Some("Totals".into()),
        series: vec![Series { values: "Data!$B$2:$B$4".into(), color: Some(Color::Theme(4, 0)), ..Series::default() }],
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
    let (layer, _) = crate::adopt(&wb, "adopted-chart", "charts", header()).expect("adopts");
    let mut loaded = milleleaf_core::load_stack(&example("../../schema/office.mleaf.json")).expect("schema");
    loaded.push(milleleaf_core::LoadedLayer { source: PathBuf::from("memory"), layer });
    let trees = Composer::new(&milleleaf_core::compose::flatten(&loaded)).compose_all().expect("composes");
    let node = node(&trees, "adopted-chart");
    let (out, _) = export(&node, std::path::Path::new("")).expect("exports");
    let file = to_xlsx(&out).unwrap();
    let excel = Some("Microsoft Macintosh Excel");

    // Excel's plain re-save: the rows under the chart get the height Excel works out itself (16
    // pt where the export has 15, not custom), the chart's cells stay. No change.
    let mut xl = from_xlsx(&file).unwrap();
    chart_on_resized_lines(xl.sheet_mut(0).unwrap(), |sh| {
        let h = sh.default_row_height * 16.0 / 15.0;
        for r in 1..30 {
            sh.rows.insert(r, LineInfo { size: Some(h), ..LineInfo::default() });
        }
    });
    assert!(xl.sheets[0].charts[0].anchor.height > 210.0, "the file reads taller");
    let (layer, report) = crate::import_saved_by(&node, &mut xl, header(), excel, None).expect("imports");
    assert!(report.changed.is_empty() && layer.data.is_empty(), "{:?}", layer.data);

    // A new Standard Width: the columns under the chart widen, and so does the chart, as the file
    // shows it. A change, with the chart's new width.
    let mut xl = from_xlsx(&file).unwrap();
    chart_on_resized_lines(xl.sheet_mut(0).unwrap(), |sh| sh.default_col_width += 10.0);
    let wide = xl.sheets[0].charts[0].anchor.width;
    assert!(wide > 330.0, "{wide}");
    let (layer, report) = crate::import_saved_by(&node, &mut xl, header(), excel, None).expect("imports");
    assert_eq!(report.changed.len(), 1, "{report:?}");
    let chart = &layer.data[0].attributes["grid::sheet"]["charts"][0];
    assert!((chart["anchor"]["width"].as_f64().unwrap() - wide as f64).abs() < 0.01, "{chart}");
    // Only its size changed: otherwise it's the model's chart, not GridCraft's reading of the
    // file's (which writes the theme colour as RGB).
    let mut model = node.children[0].attributes["grid::sheet"]["charts"][0].clone();
    model["anchor"] = chart["anchor"].clone();
    assert_eq!(chart, &model);
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
    crate::look::apply_look(&mut back, &look, "S", &mut vec![], milleleaf_core::PictureSource::StandIn(None)).unwrap();
    let mut stand_in = crate::Pictures::new();
    assert_eq!(crate::look::sheet_look(&back, &mut stand_in)["pictures"], look["pictures"]);
    assert!(stand_in.is_empty(), "a stand-in isn't a picture to store");

    // An editor nudging it by half a point is no change; a different picture is.
    let mut nudged = look.clone();
    nudged["pictures"][0]["dx"] = json!(2.5);
    crate::look::settle(&mut nudged, &look, false);
    assert_eq!(nudged["pictures"], look["pictures"]);
    let mut swapped = look.clone();
    swapped["pictures"][0]["blob"] = json!(milleleaf_core::integrity_of(b"another"));
    crate::look::settle(&mut swapped, &look, false);
    assert_ne!(swapped["pictures"], look["pictures"]);

    // An export without the picture's bytes stops, naming it.
    let err = crate::look::apply_look(&mut Sheet::new("S"), &look, "S", &mut vec![], milleleaf_core::PictureSource::Read(std::path::Path::new("/nonexistent"))).unwrap_err();
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
    crate::look::apply_look(out.sheet_mut(0).unwrap(), &look, "Data", &mut vec![], milleleaf_core::PictureSource::StandIn(None)).unwrap();
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

fn xlsx_part(bytes: &[u8], name: &str) -> String {
    use std::io::Read;
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut s = String::new();
    z.by_name(name).unwrap().read_to_string(&mut s).unwrap();
    s
}

#[test]
fn an_unchanged_chart_exports_its_own_xml_and_a_changed_one_gridcrafts() {
    use gridcraft_model::{Anchor, AnchorMode, Chart, ChartKind, LegendPos, Series};
    use std::io::Write;
    let mut wb = Workbook::new();
    let sh = wb.sheet_mut(0).unwrap();
    sh.name = "Data".into();
    for (r, v) in [3.0, 5.0, 4.0].iter().enumerate() {
        sh.set_value(CellRef::new(r as u32 + 1, 1), GValue::number(*v));
    }
    sh.charts.push(Chart {
        id: 1,
        kind: ChartKind::ColumnClustered,
        anchor: Anchor { cell: CellRef::new(5, 1), dx: 0.0, dy: 0.0, width: 300.0, height: 200.0, mode: AnchorMode::MoveAndSize },
        title: Some("Totals".into()),
        series: vec![Series { values: "Data!$B$2:$B$4".into(), ..Series::default() }],
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
    // The file's chart carries formatting GridCraft doesn't model: 14 pt text.
    const MARK: &str = r#"<c:txPr><a:bodyPr/><a:lstStyle/><a:p><a:pPr><a:defRPr sz="1400"/></a:pPr><a:endParaRPr lang="en-US"/></a:p></c:txPr>"#;
    let plain = to_xlsx(&wb).unwrap();
    let chart = xlsx_part(&plain, "xl/charts/chart1.xml");
    let marked = chart.replacen("</c:chartSpace>", &format!("{MARK}</c:chartSpace>"), 1);
    assert_ne!(marked, chart);
    let file = {
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(plain.as_slice())).unwrap();
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for i in 0..z.len() {
            let f = z.by_index(i).unwrap();
            if f.name() == "xl/charts/chart1.xml" {
                w.start_file(f.name(), zip::write::SimpleFileOptions::default()).unwrap();
                w.write_all(marked.as_bytes()).unwrap();
            } else {
                w.raw_copy_file(f).unwrap();
            }
        }
        w.finish().unwrap().into_inner()
    };

    // Adopted: the chart's own XML is kept as a blob, stored where an export reads it.
    let (layer, report) = crate::adopt_xlsx(&file, "adopted-chart", "charts", header()).expect("adopts");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let root = std::env::temp_dir().join(format!("mleaf-kept-{}", std::process::id()));
    let blobs = root.join(milleleaf_core::BLOB_DIR);
    std::fs::create_dir_all(&blobs).unwrap();
    for (blob, (_, bytes)) in &report.pictures {
        std::fs::write(blobs.join(blob.trim_start_matches("sha256-")), bytes).unwrap();
    }
    let export_xlsx = |layer: &Layer| {
        let mut loaded = milleleaf_core::load_stack(&example("../../schema/office.mleaf.json")).expect("schema");
        loaded.push(milleleaf_core::LoadedLayer { source: PathBuf::from("memory"), layer: layer.clone() });
        let trees = Composer::new(&milleleaf_core::compose::flatten(&loaded)).compose_all().expect("composes");
        let (out, r) = export(&node(&trees, "adopted-chart"), &root).expect("exports");
        let (_, memory) = crate::export_with(
            &node(&trees, "adopted-chart"), milleleaf_core::PictureSource::Memory(&report.pictures),
        ).expect("exports from memory");
        assert_eq!(memory.kept_charts, r.kept_charts, "memory keeps the same chart XML as disk");
        assert_eq!(memory.warnings, r.warnings);
        let bytes = crate::with_kept_charts(to_xlsx(&out).unwrap(), &r.kept_charts).unwrap();
        (xlsx_part(&bytes, "xl/charts/chart1.xml"), r.warnings)
    };
    let (xml, warnings) = export_xlsx(&layer);
    assert_eq!(xml, marked, "an unchanged chart is its own XML, byte for byte");
    assert!(warnings.is_empty(), "{warnings:?}");

    let edit = |f: &dyn Fn(&mut serde_json::Value)| {
        let mut l = layer.clone();
        let gs = l.data.iter_mut().find_map(|n| n.attributes.get_mut("grid::sheet")).unwrap();
        f(&mut gs["charts"][0]);
        l
    };
    // Moved and resized: the drawing holds the place, so the chart keeps its XML.
    let (xml, _) = export_xlsx(&edit(&|c| {
        c["anchor"]["cell"]["row"] = json!(9);
        c["anchor"]["width"] = json!(400.0);
    }));
    assert_eq!(xml, marked);
    // Retitled, and its data moved: the changes are made in its own XML, formatting kept.
    let (xml, warnings) = export_xlsx(&edit(&|c| {
        c["title"] = json!("New title");
        c["series"][0]["values"] = json!("Data!$B$3:$B$5");
        c["legend"] = json!("Right");
    }));
    assert!(xml.contains(MARK) && xml.contains(">New title<") && !xml.contains(">Totals<"), "{xml}");
    assert!(xml.contains("<c:f>Data!$B$3:$B$5</c:f>") && xml.contains(r#"<c:legendPos val="r"/>"#), "{xml}");
    assert!(warnings.is_empty(), "{warnings:?}");
    // Its title removed: gone from its XML too.
    let (xml, _) = export_xlsx(&edit(&|c| c["title"] = json!(null)));
    assert!(xml.contains(MARK) && !xml.contains("Totals") && !xml.contains("<c:title>"), "{xml}");
    // A new kind: GridCraft redraws it, and the export says what is lost.
    let (xml, warnings) = export_xlsx(&edit(&|c| c["kind"] = json!("Line")));
    assert!(!xml.contains(MARK) && xml.contains("lineChart"));
    assert!(warnings.iter().any(|w| w.contains("chart 1 was changed")), "{warnings:?}");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_sheets_filter_comes_back() {
    // An Imperial College bibliography workbook (Zenodo 3754178, from the real-file search) has
    // autoFilters on three sheets. They were dropped without a word (the inventory saw only the
    // hidden _xlnm._FilterDatabase names Excel keeps for them going).
    use gridcraft_core::RangeRef;
    use gridcraft_model::AutoFilter;
    let mut wb = Workbook::new();
    let sh = wb.sheet_mut(0).unwrap();
    sh.name = "Raw Data".into();
    for (r, v) in ["Title", "a", "b"].iter().enumerate() {
        sh.set_value(CellRef::new(r as u32, 0), GValue::text(*v));
    }
    sh.autofilter = Some(AutoFilter { range: RangeRef::parse("A1:A3").unwrap(), criteria: vec![] });
    let look = crate::look::sheet_look(&wb.sheets[0], &mut crate::Pictures::new());
    assert!(look.contains_key("autoFilter"), "{look:?}");
    let mut out = Workbook::new();
    out.sheet_mut(0).unwrap().name = "Raw Data".into();
    crate::look::apply_look(out.sheet_mut(0).unwrap(), &look, "Raw Data", &mut vec![], milleleaf_core::PictureSource::StandIn(None)).unwrap();
    let back = from_xlsx(&to_xlsx(&out).unwrap()).unwrap();
    assert_eq!(back.sheets[0].autofilter.as_ref().map(|f| f.range.to_string()), Some("A1:A3".to_string()));
}
