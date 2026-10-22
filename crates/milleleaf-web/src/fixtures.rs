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
            milleleaf_core::load_stack(&root.join("examples/hello-table/project.mleaf.json")).unwrap();
        let flat = milleleaf_core::compose::flatten(&loaded);
        let trees = Composer::new(&flat).compose_all().unwrap();
        let assets = root.join("examples/hello-table");
        let (doc, _) = milleleaf_wordcraft::export(
            kind(&trees, "word::document").unwrap(),
            &flat.layers,
            &assets,
        )
        .unwrap();
        let (grid, _) =
            milleleaf_gridcraft::export(kind(&trees, "grid::workbook").unwrap(), &assets).unwrap();
        let (deck, _) = milleleaf_deckcraft::export(kind(&trees, "deck::deck").unwrap(), &assets).unwrap();
        let files = BTreeMap::from([
            ("report.docx", milleleaf_wordcraft::to_docx(&doc).unwrap()),
            ("finance.xlsx", milleleaf_gridcraft::to_xlsx(&grid).unwrap()),
            ("deck.pptx", milleleaf_deckcraft::to_pptx(&deck).unwrap()),
        ]);
        std::fs::create_dir_all(root.join("_out/demo-a")).unwrap();
        for (name, bytes) in &files {
            std::fs::write(root.join("_out/demo-a").join(name), bytes).unwrap();
        }
        files
    })
}

/// Microsoft Office fixtures made by hand (examples/real/SOURCES.md): until a file exists its pair
/// is skipped with a PENDING line, as run.sh step 18 does.
fn pending(edited: &str) -> bool {
    let missing = !repo().join("examples").join(edited).exists();
    if missing {
        println!("PENDING {edited}: not made yet");
    }
    missing
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
            if pending($edited) { return; }
            let r=compare_pair($source,$edited);
            assert_eq!(r["summary"], "No content changes. The file was re-saved.");
            assert!(r["changes"].as_array().unwrap().is_empty(),"{}",serde_json::to_string(&r["changes"].as_array().unwrap().iter().map(|c|json!({"place":c["place"],"description":c["description"]})).collect::<Vec<_>>()).unwrap());
        }
    }
}

resave!(
    real_document_resaved_a,
    "real/paper.docx",
    "real/fixtures/paper.libreoffice-resaved.docx"
);
resave!(
    real_document_resaved_b,
    "real/paper.docx",
    "real/fixtures/paper.msoffice-resaved.docx"
);
resave!(
    real_deck_resaved_a,
    "real/milleleaf-deck.impress.pptx",
    "real/fixtures/milleleaf-deck.libreoffice-resaved.pptx"
);
resave!(
    real_deck_resaved_b,
    "real/milleleaf-deck.impress.pptx",
    "real/fixtures/milleleaf-deck.msoffice-resaved.pptx"
);
resave!(
    real_workbook_resaved_a,
    "real/lca-model.xlsx",
    "real/fixtures/lca-model.libreoffice-resaved.xlsx"
);
resave!(
    real_workbook_resaved_b,
    "real/lca-model.xlsx",
    "real/fixtures/lca-model.msoffice-resaved.xlsx"
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
            "real/lca-model.xlsx",
            "real/fixtures/lca-model.msoffice-margins.xlsx",
        ),
        (
            "real/paper.docx",
            "real/fixtures/paper.msoffice-blank-line.docx",
        ),
        (
            "real/milleleaf-deck.impress.pptx",
            "real/fixtures/milleleaf-deck.msoffice-edited.pptx",
        ),
    ] {
        if pending(edited) {
            continue;
        }
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
            // Expectations for the maintainer's hand-made files (packet README): check them when the files land.
            // the maintainer's edit, and what it does to the sheet's floating objects: the charts and the
            // legend picture on Graphs sit between cells (their `xdr:from`/`xdr:to` cells are the
            // same in the file as in the export), over columns of the default width, so the new
            // Standard Width (12) widens them, as the file shows them. Real changes, not Excel's
            // rewrites: its plain re-save (`real_workbook_resaved_b`) changes nothing.
            "real/fixtures/lca-model.msoffice-margins.xlsx" => {
                let mut v = vec![("Sheet Graphs", "Page setup changed: left margin:")];
                v.extend(
                    [
                        "Chart 1 resized: width 396.00 → 446.00 px",
                        "Chart 2 resized: width 396.00 → 446.00 px",
                        "Chart 3 resized: width 396.00 → 446.00 px",
                        "Chart 4 resized: width 396.00 → 446.00 px",
                        "Chart 5 resized: width 396.00 → 446.00 px",
                        "Pictures changed",
                    ]
                    .map(|d| ("Sheet Graphs", d)),
                );
                v
            }
            // The maintainer pressed Enter at the end of "Throughout this document…" (the third body
            // paragraph after the heading). The demo counts every paragraph, the paper's empty
            // ones too (Introduction, Life science…, empty, Building on…, empty, Throughout…), so
            // the new one is paragraph 7.
            "real/fixtures/paper.msoffice-blank-line.docx" => {
                vec![("Paragraph 7", "Paragraph added")]
            }
            "real/fixtures/milleleaf-deck.msoffice-edited.pptx" => {
                vec![("Slide 1", "(revised)")]
            }
            _ => panic!("fixture needs an exact expectation"),
        };
        for item in r["changes"].as_array().unwrap() {
            assert!(
                item["locator"].as_str().is_some_and(|s| !s.is_empty()),
                "{edited}: missing locator: {item}"
            );
            assert!(
                item["context"]["before"]["lines"].is_array(),
                "{edited}: missing before context"
            );
            assert!(
                item["context"]["after"]["lines"].is_array(),
                "{edited}: missing after context"
            );
        }
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
        if edited.ends_with("paper.msoffice-blank-line.docx") {
            assert_eq!(
                r["summary"],
                "One change: an empty line was added after ‘Throughout this document…’. Nothing else changed."
            );
            let change = &r["changes"][0];
            assert!(
                change["locator"]
                    .as_str()
                    .unwrap()
                    .contains("Under ‘Introduction’")
            );
            assert!(
                change["locator"]
                    .as_str()
                    .unwrap()
                    .contains("after the paragraph starting ‘Throughout this document")
            );
            assert_eq!(change["spacing_only"], true);
            assert!(
                change["context"]["before"]["lines"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|l| l["text"]
                        .as_str()
                        .is_some_and(|t| t.starts_with("Throughout this document")))
            );
            assert!(
                change["context"]["after"]["lines"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|l| l["text"] == "⏎ Empty line" && l["changed"] == true)
            );
        }
        if edited.ends_with("milleleaf-deck.msoffice-edited.pptx") {
            assert_eq!(r["summary"], "One change: text changed on slide 1.");
            let change = &r["changes"][0];
            assert!(
                change["locator"]
                    .as_str()
                    .unwrap()
                    .starts_with("Slide 1: ‘")
            );
            assert!(change["locator"].as_str().unwrap().contains("object ‘"));
            assert!(
                change["context"]["after"]["lines"][0]["text"]
                    .as_str()
                    .unwrap()
                    .contains("(revised)")
            );
        }
        if edited.ends_with("lca-model.msoffice-margins.xlsx") {
            assert_eq!(
                r["summary"],
                "7 changes on sheet ‘Graphs’: page setup, 5 chart sizes, 1 picture size."
            );
            assert!(r["changes"].as_array().unwrap().iter().all(|c| {
                c["locator"]
                    .as_str()
                    .is_some_and(|l| l.starts_with("Sheet ‘Graphs’"))
            }));
            let change = |d: &str| {
                r["changes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|c| c["description"].as_str().unwrap().contains(d))
                    .unwrap()
                    .clone()
            };
            let page = change("Page setup");
            assert_eq!(page["after"]["print"]["margins"][0], json!(1.0));
            assert_eq!(page["after"]["defaults"]["colWidth"], json!(90.0));
            // The legend picture spans the same cells, so it widens with them: 361.24 → 411.24.
            let pictures = change("Pictures changed");
            assert_eq!(
                (
                    pictures["before"][0]["w"].clone(),
                    pictures["after"][0]["w"].clone()
                ),
                (json!(361.24), json!(411.24))
            );
        }
    }
}
