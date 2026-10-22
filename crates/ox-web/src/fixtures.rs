//! Exercise the same byte workflow used by the page, not a hand-supplied source stack.
use super::*;
use std::path::PathBuf;
use std::sync::OnceLock;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn kind<'a>(nodes: &'a [Composed], key: &str) -> Option<&'a Composed> {
    for n in nodes {
        if n.attributes.contains_key(key) {
            return Some(n);
        }
        if let Some(n) = kind(&n.children, key) {
            return Some(n);
        }
    }
    None
}

fn originals() -> &'static BTreeMap<&'static str, Vec<u8>> {
    static FILES: OnceLock<BTreeMap<&'static str, Vec<u8>>> = OnceLock::new();
    FILES.get_or_init(|| {
        let root = repo();
        let loaded =
            ox_core::load_stack(&root.join("examples/hello-table/project.ox.json")).unwrap();
        let flat = ox_core::compose::flatten(&loaded);
        let trees = Composer::new(&flat).compose_all().unwrap();
        let assets = root.join("examples/hello-table");
        let (doc, _) = ox_wordcraft::export(
            kind(&trees, "word::document").unwrap(),
            &flat.layers,
            &assets,
        )
        .unwrap();
        let (grid, _) =
            ox_gridcraft::export(kind(&trees, "grid::workbook").unwrap(), &assets).unwrap();
        let (deck, _) = ox_deckcraft::export(kind(&trees, "deck::deck").unwrap(), &assets).unwrap();
        let files = BTreeMap::from([
            ("report.docx", ox_wordcraft::to_docx(&doc).unwrap()),
            ("finance.xlsx", ox_gridcraft::to_xlsx(&grid).unwrap()),
            ("deck.pptx", ox_deckcraft::to_pptx(&deck).unwrap()),
        ]);
        std::fs::create_dir_all(root.join("_out/demo-a")).unwrap();
        for (name, bytes) in &files {
            std::fs::write(root.join("_out/demo-a").join(name), bytes).unwrap();
        }
        files
    })
}

fn compare_pair(source: &str, edited: &str) -> Value {
    let root = repo();
    let bytes = if let Some(name) = source.strip_prefix("hello/") {
        originals()[name].clone()
    } else {
        std::fs::read(root.join("examples").join(source)).unwrap()
    };
    let a = read(&bytes, header("base", "Original", "")).unwrap();
    let edited_bytes = std::fs::read(root.join("examples").join(edited)).unwrap();
    let (_, report) = compare_file(
        &a,
        &edited_bytes,
        header("proposal", "Reviewer", "2026-10-22T00:00:00Z"),
    )
    .unwrap();
    let out = root.join("_out/demo-a");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(
        out.join(format!("{}.json", edited.replace('/', "_"))),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    report
}

macro_rules! resave {
    ($test:ident,$source:expr,$edited:expr) => {
        #[test] fn $test() {
            let r=compare_pair($source,$edited);
            assert!(r["changes"].as_array().unwrap().is_empty(),"{}",serde_json::to_string(&r["changes"].as_array().unwrap().iter().map(|c|json!({"place":c["place"],"description":c["description"]})).collect::<Vec<_>>()).unwrap());
        }
    }
}

resave!(
    real_document_resaved_a,
    "real/board-report.docx",
    "real/fixtures/board-report.libreoffice-resaved.docx"
);
resave!(
    real_document_resaved_b,
    "real/board-report.docx",
    "real/fixtures/board-report.msoffice-resaved.docx"
);
resave!(
    real_deck_resaved_a,
    "real/briefing.pptx",
    "real/fixtures/briefing.libreoffice-resaved.pptx"
);
resave!(
    real_deck_resaved_b,
    "real/briefing.pptx",
    "real/fixtures/briefing.msoffice-resaved.pptx"
);
resave!(
    real_workbook_resaved,
    "real/finance.xlsx",
    "real/fixtures/finance.libreoffice-resaved.xlsx"
);
resave!(
    hello_document_resaved_a,
    "hello/report.docx",
    "hello-table/fixtures/report.libreoffice-resaved.docx"
);
resave!(
    hello_document_resaved_b,
    "hello/report.docx",
    "hello-table/fixtures/report.msoffice-resaved.docx"
);
resave!(
    hello_workbook_resaved_a,
    "hello/finance.xlsx",
    "hello-table/fixtures/finance.libreoffice-resaved.xlsx"
);
resave!(
    hello_workbook_resaved_b,
    "hello/finance.xlsx",
    "hello-table/fixtures/finance.msoffice-resaved.xlsx"
);
resave!(
    hello_deck_resaved_a,
    "hello/deck.pptx",
    "hello-table/fixtures/deck.libreoffice-resaved.pptx"
);
resave!(
    hello_deck_resaved_b,
    "hello/deck.pptx",
    "hello-table/fixtures/deck.msoffice-resaved.pptx"
);

// Each item must correspond to a known edit; an extra formatting row fails this test too.
#[test]
fn edited_fixture_reports() {
    for (source, edited) in [
        (
            "hello/report.docx",
            "hello-table/fixtures/report.libreoffice-edited.docx",
        ),
        (
            "hello/report.docx",
            "hello-table/fixtures/report.msoffice-edited.docx",
        ),
        (
            "hello/report.docx",
            "hello-table/fixtures/report.msoffice-unlocked-edited.docx",
        ),
        (
            "hello/finance.xlsx",
            "hello-table/fixtures/finance.libreoffice-edited.xlsx",
        ),
        (
            "hello/finance.xlsx",
            "hello-table/fixtures/finance.msoffice-edited.xlsx",
        ),
        (
            "hello/finance.xlsx",
            "hello-table/fixtures/finance.openpyxl-edited.xlsx",
        ),
        (
            "hello/deck.pptx",
            "hello-table/fixtures/deck.libreoffice-edited.pptx",
        ),
        (
            "hello/deck.pptx",
            "hello-table/fixtures/deck.msoffice-edited.pptx",
        ),
        (
            "real/finance.xlsx",
            "real/fixtures/finance.msoffice-margins.xlsx",
        ),
        (
            "real/board-report.docx",
            "real/fixtures/board-report.msoffice-blank-line.docx",
        ),
        (
            "real/briefing.pptx",
            "real/fixtures/briefing.msoffice-edited.pptx",
        ),
    ] {
        let r = compare_pair(source, edited);
        println!(
            "{edited}: {}",
            serde_json::to_string(
                &r["changes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|c| json!({"place":c["place"],"description":c["description"]}))
                    .collect::<Vec<_>>()
            )
            .unwrap()
        );
        let document = [
            ("Paragraph 3", "Prepared for the October board meeting."),
            ("Paragraph 2", "Group revenue for the quarter"),
        ];
        let table = ("Table 1, row 3, column 2", "541500 → 999");
        let workbook = [
            ("Sheet Q3 2026", "Sheet renamed: 'Q3' → 'Q3 2026'"),
            ("range A1:B5", "A1:B4 → A1:B5"),
            ("cell A4", "Row added: Perth, 120000"),
            ("cell B5", "297000 → 305250"),
            ("cell B6", "B5 → B6"),
            ("cell B6", "1250500 → 1378750"),
            ("cell B6", "SUM(B2:B4) → =SUM(B2:B5)"),
        ];
        let deck = [
            ("Slide 1, object 1", "Object removed"),
            ("Slide 3, object 4", "Owner: Finance"),
            ("Slide 1, object 2", "Object moved or resized"),
            (
                "Slide 1",
                "Open with the headline, then hand over to Finance.",
            ),
            (
                "Slide 2, object 2, table row 3, column 2",
                "'$541,500' → '$999'",
            ),
            ("Slide 2, object 3", "Group revenue:"),
            ("Slide 3, object 2", "Singapore pipeline review (two deals)"),
        ];
        let expected: Vec<_> = match edited {
            "hello-table/fixtures/report.libreoffice-edited.docx" => {
                document.into_iter().chain([table]).collect()
            }
            "hello-table/fixtures/report.msoffice-edited.docx" => document.to_vec(),
            "hello-table/fixtures/report.msoffice-unlocked-edited.docx" => vec![table],
            "hello-table/fixtures/finance.libreoffice-edited.xlsx"
            | "hello-table/fixtures/finance.msoffice-edited.xlsx" => workbook.to_vec(),
            "hello-table/fixtures/finance.openpyxl-edited.xlsx" => vec![
                ("Sheet Q3, cell B3", "541500 → 550000"),
                ("Sheet Q3, cell B5", "1250500 → 1259000"),
            ],
            "hello-table/fixtures/deck.libreoffice-edited.pptx"
            | "hello-table/fixtures/deck.msoffice-edited.pptx" => deck.to_vec(),
            // The saved file also grew chart 2 by 7.8%, beyond the native 5% tolerance.
            "real/fixtures/finance.msoffice-margins.xlsx" => vec![
                ("Sheet Output", "Page setup changed: left margin:"),
                ("Sheet Output", "Chart 2 resized: height 601.04 → 647.70 px"),
            ],
            "real/fixtures/board-report.msoffice-blank-line.docx" => {
                vec![("Paragraph 4", "Paragraph added")]
            }
            "real/fixtures/briefing.msoffice-edited.pptx" => {
                vec![("Slide 1, object 1", "(revised)")]
            }
            _ => panic!("fixture needs an exact expectation"),
        };
        let mut actual = r["changes"].as_array().unwrap().clone();
        assert_eq!(
            actual.len(),
            expected.len(),
            "{edited}: {}",
            serde_json::to_string(
                &actual
                    .iter()
                    .map(|c| json!({"place":c["place"],"description":c["description"]}))
                    .collect::<Vec<_>>()
            )
            .unwrap()
        );
        for (place, description) in expected {
            let i = actual
                .iter()
                .position(|c| {
                    c["place"].as_str().unwrap().contains(place)
                        && c["description"].as_str().unwrap().contains(description)
                })
                .unwrap_or_else(|| panic!("{edited}: missing {place}: {description}"));
            actual.remove(i);
        }
        if edited.ends_with("finance.msoffice-margins.xlsx") {
            let change = &r["changes"][0];
            assert_eq!(change["after"]["print"]["margins"][0], json!(1.0));
            assert_eq!(change["after"]["defaults"]["colWidth"], json!(90.0));
        }
    }
}
