use std::path::PathBuf;
use std::sync::Arc;

use ox_core::render::find;
use ox_core::{Composed, Composer, Header, Layer};
use serde_json::json;
use wordcraft_doc::para::OBJ;
use wordcraft_doc::{Block, CharProps, Document, InlineObject, Paragraph};

use crate::{TAG_PREFIX, export, from_docx, import, to_docx};

const REPORT: &str = "3f1c2a90-0000-4c1e-9a00-000000000020";
const SUMMARY: &str = "3f1c2a90-0000-4c1e-9a00-000000000023";
const HEADING: &str = "3f1c2a90-0000-4c1e-9a00-000000000021";
const TABLE: &str = "3f1c2a90-0000-4c1e-9a00-000000000022";

fn example(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/hello-table")
        .join(file)
}

fn compose(stack: &[Layer]) -> (Vec<Composed>, Vec<String>) {
    // Load the example project, then add extra in-memory layers on top.
    let mut loaded = ox_core::load_stack(&example("project.ox.json")).expect("stack loads");
    for l in stack {
        loaded.push(ox_core::LoadedLayer {
            source: PathBuf::from("memory"),
            layer: l.clone(),
        });
    }
    let flat = ox_core::compose::flatten(&loaded);
    let trees = Composer::new(&flat).compose_all().expect("composes");
    (trees, flat.layers)
}

fn report_node(trees: &[Composed]) -> Composed {
    find(trees, REPORT).expect("report in tree").clone()
}

fn header() -> Header {
    Header {
        id: "test-edit".into(),
        ox_version: String::new(),
        data_version: "1.0.0".into(),
        author: "test".into(),
        timestamp: "2026-10-15".into(),
        kind: None,
        ..Default::default()
    }
}

/// Exports the report, writes DOCX, reads it back (as Word would hand it over).
fn round_trip_docx(node: &Composed, layers: &[String]) -> Document {
    let (doc, report) = export(node, layers, std::path::Path::new("")).expect("exports");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    from_docx(&to_docx(&doc).expect("writes docx")).expect("reads docx")
}

fn para_with_tag<'a>(doc: &'a mut Document, path: &str) -> &'a mut Paragraph {
    let tag = format!("{TAG_PREFIX}{path}");
    let i = doc
        .body
        .iter()
        .position(|b| {
            b.as_para().is_some_and(|p| {
                p.objects.iter().any(
                    |o| matches!(o, InlineObject::ControlStart { control } if control.tag == tag),
                )
            })
        })
        .expect("tagged paragraph");
    match Arc::make_mut(&mut doc.body[i]) {
        Block::Para(p) => p,
        Block::Table(_) => unreachable!(),
    }
}

#[test]
fn export_carries_identity_through_docx() {
    let (trees, layers) = compose(&[]);
    let node = report_node(&trees);
    let doc = round_trip_docx(&node, &layers);
    let tags: Vec<String> = doc
        .body
        .iter()
        .flat_map(|b| match &**b {
            Block::Para(p) => p
                .objects
                .iter()
                .filter_map(|o| {
                    if let InlineObject::ControlStart { control } = o {
                        Some(control.tag.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>(),
            Block::Table(t) => t.controls.open.iter().map(|c| c.tag.clone()).collect(),
        })
        .collect();
    assert!(tags.contains(&format!("{TAG_PREFIX}{SUMMARY}")));
    assert!(tags.contains(&format!("{TAG_PREFIX}{TABLE}")));
    assert!(tags.contains(&format!("{TAG_PREFIX}{TABLE}#caption")));
    // The live total and the corrected Hong Kong figure from the finance layer.
    let text: String = doc
        .body
        .iter()
        .filter_map(|b| b.as_para())
        .map(|p| p.plain_text())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("$1,250,500"), "{text}");
    let table = doc.body.iter().find_map(|b| b.as_table()).expect("table");
    let hk: String = table.rows[2].cells[1]
        .blocks
        .iter()
        .filter_map(|b| b.as_para())
        .map(|p| p.plain_text())
        .collect();
    assert_eq!(hk.trim_matches(OBJ), "$541,500");
    assert_eq!(
        doc.custom_props
            .iter()
            .find(|p| p.name == "ox.document")
            .map(|p| p.value.as_str()),
        Some(REPORT)
    );
}

#[test]
fn unedited_file_imports_as_no_change() {
    let (trees, layers) = compose(&[]);
    let node = report_node(&trees);
    let doc = round_trip_docx(&node, &layers);
    let (layer, report) = import(&node, &doc, header()).expect("imports");
    assert!(report.is_empty(), "{report:?}");
    assert!(layer.data.is_empty(), "{:?}", layer.data);
    assert!(report.conflicts.is_empty() && !report.stale, "{report:?}");
}

#[test]
fn edits_become_a_minimal_layer_that_composes() {
    let (trees, layers) = compose(&[]);
    let node = report_node(&trees);
    let mut doc = round_trip_docx(&node, &layers);

    // 1. Reword the summary (keeping its live total), 2. type a new paragraph at the end,
    // 3. delete the heading, 4. change a number in the linked table.
    let p = para_with_tag(&mut doc, SUMMARY);
    let start = p.text.find("Total").expect("summary text");
    p.delete(start, start + "Total revenue".len())
        .expect("delete");
    p.insert_text(start, "Group revenue", &CharProps::default())
        .expect("insert");
    let mut fresh = Paragraph::with_text(
        "Prepared for the October board meeting.",
        CharProps {
            italic: Some(true),
            ..CharProps::default()
        },
    );
    fresh.props.style = Some("Normal".into());
    doc.body.push(Arc::new(Block::Para(fresh)));
    let h = doc.body.iter().position(|b| b.as_para().is_some_and(|p| p.objects.iter().any(|o| matches!(o, InlineObject::ControlStart { control } if control.tag.ends_with(HEADING))))).expect("heading");
    doc.body.remove(h);
    let ti = doc
        .body
        .iter()
        .position(|b| b.as_table().is_some())
        .expect("table");
    if let Block::Table(t) = Arc::make_mut(&mut doc.body[ti]) {
        // Typed over in Word: the cell's paragraph (and its right alignment) stays.
        let cell = &mut t.rows[1].cells[1];
        let mut typed = Paragraph::with_text("$999", CharProps::default());
        typed.props = cell.blocks[0].as_para().expect("cell paragraph").props.clone();
        cell.blocks = vec![wordcraft_doc::para_block(typed)];
    }
    // Through DOCX again, as if saved by Word.
    let doc = from_docx(&to_docx(&doc).expect("writes")).expect("reads");

    let (layer, report) = import(&node, &doc, header()).expect("imports");
    assert_eq!(report.changed, vec![SUMMARY.to_string()]);
    assert_eq!(report.added.len(), 1);
    assert_eq!(report.removed, vec!["heading".to_string()]);
    assert_eq!(report.conflicts.len(), 1, "{report:?}");
    assert_eq!(report.conflicts[0].path, TABLE);

    // The edit layer: the document node (order, new child, removed heading), the summary, the new paragraph.
    let paths: Vec<&str> = layer.data.iter().map(|n| n.path.as_str()).collect();
    assert_eq!(paths.len(), 3, "{paths:?}");
    let summary = layer
        .data
        .iter()
        .find(|n| n.path == SUMMARY)
        .expect("summary node");
    assert_eq!(
        summary.attributes["word::paragraph"]["runs"],
        json!([
            { "text": "Group revenue for the quarter was " },
            { "field": "total" },
            { "text": " across " },
            { "field": "regions" },
            { "text": " regions." }
        ])
    );

    // Stacked on the project, the report now reads the edit and still shows the live total.
    let (trees, _) = compose(&[layer]);
    let node = report_node(&trees);
    let names: Vec<_> = node
        .children
        .iter()
        .map(|c| c.name.clone().unwrap_or_default())
        .collect();
    assert_eq!(names.len(), 3, "{names:?}");
    assert_eq!(names[0], "summary");
    assert_eq!(names[1], "revenue-table");
    let total = &node.children[0];
    assert_eq!(ox_core::render::field_text(total, "total"), "$1,250,500");
    let added = &node.children[2];
    assert_eq!(
        added.attributes["word::paragraph"]["runs"],
        json!([{ "italic": true, "text": "Prepared for the October board meeting." }])
    );
}

#[test]
fn wrong_document_is_refused() {
    let (trees, layers) = compose(&[]);
    let node = report_node(&trees);
    let doc = round_trip_docx(&node, &layers);
    let mut other = node.clone();
    other.path = "someone-else".into();
    assert!(import(&other, &doc, header()).is_err());
}

/// Files saved by LibreOffice 24.2, which turns our block-level controls into inline ones and
/// moves the table's control into its first cell. Identity must survive anyway.
fn fixture(name: &str) -> Document {
    from_docx(&std::fs::read(example("fixtures").join(name)).expect("fixture")).expect("reads")
}

#[test]
fn libreoffice_resave_imports_as_no_change() {
    let (trees, _) = compose(&[]);
    let node = report_node(&trees);
    for name in [
        "report.libreoffice-resaved.docx",
        "report.msoffice-resaved.docx",
    ] {
        let (layer, report) = import(&node, &fixture(name), header()).expect("imports");
        assert!(
            report.is_empty() && report.conflicts.is_empty(),
            "{name}: {report:?}"
        );
        assert!(layer.data.is_empty(), "{name}");
    }
}

#[test]
fn word_edits_import_like_libreoffice() {
    // Same edits in Word 16 (AppleScript), except the $999: Word enforces the contentLocked
    // lock on the linked table and refuses the overwrite, so there is no conflict.
    let (trees, _) = compose(&[]);
    let node = report_node(&trees);
    let (lo, _) =
        import(&node, &fixture("report.libreoffice-edited.docx"), header()).expect("imports");
    let (ms, report) =
        import(&node, &fixture("report.msoffice-edited.docx"), header()).expect("imports");
    assert_eq!(report.changed, vec![SUMMARY.to_string()]);
    assert_eq!(report.added.len(), 1);
    assert!(
        report.removed.is_empty() && report.conflicts.is_empty(),
        "{report:?}"
    );
    assert_eq!(
        serde_json::to_value(&ms.data).expect("json"),
        serde_json::to_value(&lo.data).expect("json")
    );

    // With the locks removed first (as a user can in the control's properties), Word lets
    // the number be typed over, and it comes back as a conflict.
    let (layer, report) = import(
        &node,
        &fixture("report.msoffice-unlocked-edited.docx"),
        header(),
    )
    .expect("imports");
    assert!(layer.data.is_empty(), "{:?}", layer.data);
    assert_eq!(
        report
            .conflicts
            .iter()
            .map(|c| c.path.as_str())
            .collect::<Vec<_>>(),
        vec![TABLE]
    );
}

#[test]
fn libreoffice_edits_import() {
    // Edited in LibreOffice: "Total revenue" → "Group revenue", an italic paragraph appended,
    // and Hong Kong's revenue overwritten with $999 in the content-locked linked table.
    let (trees, _) = compose(&[]);
    let node = report_node(&trees);
    let (layer, report) =
        import(&node, &fixture("report.libreoffice-edited.docx"), header()).expect("imports");
    assert_eq!(report.changed, vec![SUMMARY.to_string()]);
    assert_eq!(report.added.len(), 1);
    assert!(report.removed.is_empty());
    assert_eq!(
        report
            .conflicts
            .iter()
            .map(|c| c.path.as_str())
            .collect::<Vec<_>>(),
        vec![TABLE]
    );
    let summary = layer
        .data
        .iter()
        .find(|n| n.path == SUMMARY)
        .expect("summary");
    assert_eq!(
        summary.attributes["word::paragraph"]["runs"][0]["text"],
        json!("Group revenue for the quarter was ")
    );
    assert_eq!(
        summary.attributes["word::paragraph"]["runs"][1],
        json!({ "field": "total" })
    );
}

#[test]
fn word_and_libreoffice_matches_are_all_certain() {
    let (trees, _) = compose(&[]);
    let node = report_node(&trees);
    for name in [
        "report.libreoffice-edited.docx",
        "report.msoffice-edited.docx",
    ] {
        let (_, report) = import(&node, &fixture(name), header()).expect("imports");
        assert!(!report.matches.is_empty(), "{name}");
        assert!(
            ox_core::matching::best_guesses(&report.matches).is_empty(),
            "{name}: {:?}",
            report.matches
        );
    }
}

#[test]
fn a_pasted_copy_of_a_paragraph_is_new_content_not_the_original() {
    let (trees, layers) = compose(&[]);
    let node = report_node(&trees);
    let mut doc = round_trip_docx(&node, &layers);
    // Paste a copy of the summary right after it (it carries the same tag), then reword the copy.
    let i = doc
        .body
        .iter()
        .position(|b| b.as_para().is_some_and(|p| p.objects.iter().any(|o| matches!(o, InlineObject::ControlStart { control } if control.tag.ends_with(SUMMARY)))))
        .expect("summary");
    let copy = doc.body[i].clone();
    doc.body.insert(i + 1, copy);
    if let Block::Para(p) = Arc::make_mut(&mut doc.body[i + 1]) {
        let start = p.text.find("Total").expect("summary text");
        p.delete(start, start + "Total revenue".len()).expect("delete");
        p.insert_text(start, "Copied revenue", &CharProps::default()).expect("insert");
    }
    let doc = from_docx(&to_docx(&doc).expect("writes")).expect("reads");
    let (layer, report) = import(&node, &doc, header()).expect("imports");
    // The original is untouched; the copy is new; both are flagged.
    assert!(!report.changed.contains(&SUMMARY.to_string()), "{report:?}");
    assert_eq!(report.added.len(), 1, "{report:?}");
    assert!(report.warnings.iter().any(|w| w.contains("appears more than once")), "{:?}", report.warnings);
    assert!(ox_core::matching::best_guesses(&report.matches).iter().any(|m| m.path == SUMMARY));
    let summary_text = layer
        .data
        .iter()
        .filter(|n| n.path == SUMMARY)
        .flat_map(|n| n.attributes.get("word::paragraph"))
        .map(|v| v.to_string())
        .collect::<String>();
    assert!(!summary_text.contains("Copied revenue"), "{summary_text}");
}

#[test]
fn adopting_a_word_file_brings_in_every_paragraph_and_table() {
    // A file Word saved (it happens to carry our tags, which adopt ignores as identities).
    let doc = from_docx(&std::fs::read(example("fixtures/report.msoffice-edited.docx")).unwrap()).unwrap();
    let (layer, report) = crate::adopt(&doc, "adopted-report", "Q3 report", header()).expect("adopts");
    let root = layer.data.iter().find(|n| n.path == "adopted-report").expect("document node");
    assert!(root.attributes.contains_key("word::document"));
    let paras = layer.data.iter().filter(|n| n.attributes.contains_key("word::paragraph")).count();
    let tables = layer.data.iter().filter(|n| n.attributes.contains_key("office::data::table")).count();
    // Heading, summary, the caption (kept as a paragraph), the new note; one table.
    assert_eq!((paras, tables), (4, 1), "{report:?}");
}

#[test]
fn unicode_survives_a_docx_round_trip_code_point_for_code_point() {
    let cases = [
        "5. ♖🪴🧾🚂🙭🎨🆞🆣🆫𝝿𔑲",
        "VS16 ❤\u{FE0F} VS15 ❤\u{FE0E} ☎\u{FE0F}",
        "ZWJ 👩\u{200D}💻 flag 🇭🇰",
        "combining e\u{0301} n\u{0303} 한국어 العربية 𓂀",
    ];
    let mut doc = Document::default();
    doc.body.clear();
    for c in cases {
        doc.body.push(Arc::new(Block::Para(Paragraph::with_text(c, CharProps::default()))));
    }
    let back = from_docx(&to_docx(&doc).expect("writes")).expect("reads");
    let got: Vec<String> = back.body.iter().filter_map(|b| b.as_para()).map(|p| p.plain_text().to_string()).filter(|t| !t.is_empty()).collect();
    assert_eq!(got, cases.iter().map(|s| s.to_string()).collect::<Vec<_>>());
}

/// Adopts `doc` into an empty project and exports it again, through DOCX both ways.
fn adopt_and_export(doc: &Document) -> Document {
    let doc = from_docx(&to_docx(doc).unwrap()).unwrap();
    let (layer, _) = crate::adopt(&doc, "adopted", "adopted", header()).expect("adopts");
    let mut loaded = ox_core::load_stack(&example("../../schema/office.ox.json")).expect("schema");
    loaded.push(ox_core::LoadedLayer { source: PathBuf::from("memory"), layer });
    let flat = ox_core::compose::flatten(&loaded);
    let trees = Composer::new(&flat).compose_all().expect("composes");
    let (out, report) = export(find(&trees, "adopted").expect("document"), &[], std::path::Path::new("")).expect("exports");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    from_docx(&to_docx(&out).unwrap()).unwrap()
}

#[test]
fn adopt_carries_headers_footers_page_fields_and_note_separators() {
    use wordcraft_doc::PartKind;
    let mut doc = Document::new();
    let header = doc.add_part(PartKind::Header, vec![wordcraft_doc::para_block(Paragraph::with_text("[FOR PUBLICATION]", CharProps::default()))]);
    let mut page = Paragraph::with_text("Page ", CharProps::default());
    let off = page.len();
    page.insert_object(off, InlineObject::Field { instr: " PAGE ".into(), result: "1".into(), locked: false, dirty: false, code: false, form: String::new() }, &CharProps::default()).unwrap();
    let footer = doc.add_part(PartKind::Footer, vec![wordcraft_doc::para_block(page)]);
    doc.last_section.headers.first = Some(header);
    doc.last_section.footers.default = Some(footer);
    doc.last_section.title_page = true;
    // A template with no footnote separator line (Ask LibreOffice, 16 Oct 2026).
    doc.footnote_separators.separator = Some(vec![wordcraft_doc::para_block(Paragraph::new())]);

    let out = adopt_and_export(&doc);
    let text = |id: Option<u32>| out.parts[&id.expect("part")].blocks.iter().filter_map(|b| b.as_para()).map(|p| p.plain_text()).collect::<String>();
    assert_eq!(text(out.last_section.headers.first), "[FOR PUBLICATION]");
    assert!(out.last_section.title_page);
    let f = &out.parts[&out.last_section.footers.default.unwrap()].blocks[0];
    let p = f.as_para().unwrap();
    assert!(p.objects.iter().any(|o| matches!(o, InlineObject::Field { instr, .. } if instr.trim() == "PAGE")), "PAGE stays a field");
    assert!(!out.footnote_separators.is_empty(), "the template's own (empty) separator survives");
    assert_eq!(out.footnote_separators, from_docx(&to_docx(&doc).unwrap()).unwrap().footnote_separators);
}

#[test]
fn adopt_carries_lists_merged_cells_and_a_plain_first_row() {
    use wordcraft_doc::numbering::ListKind;
    use wordcraft_doc::props::{NumRef, VMerge};
    use wordcraft_doc::{Cell, Table};
    let mut doc = Document::new();
    doc.body.clear();
    let num = doc.numbering.add_list(ListKind::Numbered);
    for (text, level) in [("first", 0u8), ("nested", 1), ("second", 0)] {
        let mut p = Paragraph::with_text(text, CharProps::default());
        p.props.numbering = Some(NumRef { num, level });
        doc.body.push(Arc::new(Block::Para(p)));
    }
    // 3 × 3: row 1 spans columns 0–1; column 2 is merged down rows 1–2. The first row isn't a header.
    let mut t = Table::new(3, 3, 400.0);
    for (r, row) in [["a", "b", "c"], ["wide", "", "tall"], ["x", "y", ""]].iter().enumerate() {
        for (c, text) in row.iter().enumerate() {
            t.rows[r].cells[c] = Cell::with_text(text);
        }
    }
    t.rows[1].cells[0].props.span = 2;
    t.rows[1].cells.remove(1);
    t.rows[1].cells[1].props.vmerge = VMerge::Restart;
    t.rows[2].cells[2].props.vmerge = VMerge::Continue;
    doc.body.push(Arc::new(Block::Table(t)));
    let doc = from_docx(&to_docx(&doc).unwrap()).unwrap();

    let (layer, _) = crate::adopt(&doc, "adopted", "lists", header()).expect("adopts");
    let mut loaded = ox_core::load_stack(&example("../../schema/office.ox.json")).expect("schema");
    loaded.push(ox_core::LoadedLayer { source: PathBuf::from("memory"), layer });
    let flat = ox_core::compose::flatten(&loaded);
    let trees = Composer::new(&flat).compose_all().expect("composes");
    let node = find(&trees, "adopted").expect("document");
    let (out, report) = export(node, &[], std::path::Path::new("")).expect("exports");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let out = from_docx(&to_docx(&out).unwrap()).unwrap();

    let levels: Vec<(u8, wordcraft_doc::section::NumFormat)> = out
        .body
        .iter()
        .filter_map(|b| b.as_para()?.props.numbering)
        .map(|n| (n.level, out.numbering.level(n.num, n.level).unwrap().format))
        .collect();
    let want: Vec<_> = [0u8, 1, 0].iter().map(|&l| (l, doc.numbering.level(num, l).unwrap().format)).collect();
    assert_eq!(levels, want);
    let t = out.body.iter().find_map(|b| match &**b {
        Block::Table(t) => Some(t),
        _ => None,
    });
    let t = t.expect("table");
    assert!(!t.rows[0].props.header, "a plain first row stays plain");
    assert_eq!(t.rows[1].cells.len(), 2);
    assert_eq!(t.rows[1].cells[0].props.span, 2);
    assert_eq!(t.rows[1].cells[1].props.vmerge, VMerge::Restart);
    assert_eq!(t.rows[2].cells[2].props.vmerge, VMerge::Continue);
}

#[test]
fn a_merge_made_in_word_on_an_existing_table_comes_back() {
    use wordcraft_doc::props::VMerge;
    let (trees, layers) = compose(&[]);
    let node = report_node(&trees);
    let mut doc = round_trip_docx(&node, &layers);
    let t = doc
        .body
        .iter_mut()
        .find_map(|b| match Arc::make_mut(b) {
            Block::Table(t) => Some(t),
            _ => None,
        })
        .expect("table");
    t.rows[1].cells[0].props.vmerge = VMerge::Restart;
    t.rows[2].cells[0].props.vmerge = VMerge::Continue;
    let doc = from_docx(&to_docx(&doc).unwrap()).unwrap();
    let (layer, report) = import(&node, &doc, header()).expect("imports");
    assert!(report.changed.contains(&TABLE.to_string()), "{report:?}");
    let wt = &layer.data.iter().find(|n| n.path == TABLE).expect("table node").attributes["word::table"];
    assert_eq!(wt["merges"], json!([{ "row": 1, "col": 0, "rows": 2, "cols": 1 }]));
}

/// The real board report, adopted the way `ox adopt` does it, then imported from files Word 16.114
/// and LibreOffice 24.2 saved from Milleleaf's export (`examples/real/fixtures`).
fn board_report_node() -> Composed {
    let real = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/real");
    let bytes = std::fs::read(real.join("board-report.docx")).unwrap();
    // `ox adopt`'s path for this file: stable_id("adopt|" + its SHA-256).
    let root = "f5bb6935-f60f-5b56-96e8-6ed886cc9246";
    let (layer, _) = crate::adopt(&from_docx(&bytes).unwrap(), root, "board-report", header()).expect("adopts");
    let mut loaded = ox_core::load_stack(&example("../../schema/office.ox.json")).expect("schema");
    loaded.push(ox_core::LoadedLayer { source: PathBuf::from("memory"), layer });
    let flat = ox_core::compose::flatten(&loaded);
    let trees = Composer::new(&flat).compose_all().expect("composes");
    find(&trees, root).expect("document").clone()
}

fn real_fixture(name: &str) -> Document {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/real/fixtures").join(name);
    from_docx(&std::fs::read(p).expect("fixture")).expect("reads")
}

#[test]
fn a_blank_line_typed_in_word_comes_back_and_word_resaves_stay_clean() {
    let node = board_report_node();
    let (layer, report) = import(&node, &real_fixture("board-report.msoffice-resaved.docx"), header()).expect("imports");
    assert!(report.is_empty() && layer.data.is_empty(), "{report:?}");

    // Word: Enter at the end of paragraph 3. One new, empty paragraph; nothing else changes.
    let (layer, report) = import(&node, &real_fixture("board-report.msoffice-blank-line.docx"), header()).expect("imports");
    assert_eq!((report.added.len(), report.changed.len(), report.removed.len()), (1, 0, 0), "{report:?}");
    let added = layer.data.iter().find(|n| n.path == report.added[0]).expect("new node");
    let runs = added.attributes["word::paragraph"]["runs"].as_array().cloned().unwrap_or_default();
    assert!(runs.iter().all(|r| r["text"].as_str().is_none_or(|t| t.trim().is_empty())), "{added:?}");

    // LibreOffice drops the tag of an empty paragraph: it is matched by place, as a best guess,
    // not reported as one removed and one added. (Its re-save still differs in list numbers,
    // border colours, tabs and table borders: known, see the plan.)
    let (_, report) = import(&node, &real_fixture("board-report.libreoffice-resaved.docx"), header()).expect("imports");
    assert_eq!((report.added.len(), report.removed.len()), (0, 0), "{report:?}");
    assert!(report.matches.iter().any(|m| m.by == "an empty paragraph in the same place"), "{:?}", report.matches);
}

#[test]
fn the_report_logo_comes_in_and_a_swapped_one_is_a_change() {
    let node = board_report_node();
    let logo = |v: &serde_json::Value| -> Option<serde_json::Value> {
        v["headers"].as_object()?.values().flat_map(|ps| ps.as_array().cloned().unwrap_or_default())
            .flat_map(|p| p["runs"].as_array().cloned().unwrap_or_default())
            .find_map(|r| r.get("picture").cloned())
    };
    // Ofsted's logo floats in the first page's header, in front of the text.
    let pic = logo(&node.attributes["word::document"]).expect("the header logo");
    assert_eq!(pic["float"]["wrap"], json!("inFrontOfText"), "{pic}");
    assert!(pic["blob"].as_str().unwrap().starts_with("sha256-"));

    // LibreOffice's re-save keeps it.
    let (layer, report) = import(&node, &real_fixture("board-report.libreoffice-resaved.docx"), header()).expect("imports");
    let doc_changed = layer.data.iter().find(|n| n.path == node.path).and_then(|n| logo(&n.attributes["word::document"]));
    assert!(doc_changed.is_none(), "LibreOffice changed the logo: {doc_changed:?} {:?}", report.warnings);

    // Someone swaps the picture in Word.
    let mut doc = real_fixture("board-report.msoffice-resaved.docx");
    let swapped = b"\xff\xd8\xffnot really a jpeg".to_vec();
    let key = doc.media.keys().next().expect("a picture").clone();
    doc.media.insert(key, Arc::new(swapped.clone()));
    let (layer, report) = import(&node, &doc, header()).expect("imports");
    assert_eq!(report.changed, vec![node.path.clone()], "{:?}", report.warnings);
    let now = logo(&layer.data[0].attributes["word::document"]).expect("the new logo");
    let blob = ox_core::integrity_of(&swapped);
    assert_eq!(now["blob"], json!(blob));
    assert_eq!(report.pictures[&blob].1, swapped);
}

#[test]
fn a_resave_is_no_change_against_a_node_from_the_store() {
    // The store keeps layers as canonical JSON (RFC 8785), which writes 12.0 as 12: numbers
    // must compare by value, or every paragraph with a spacing would come back changed.
    fn as_stored(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::Number(n) => {
                if let Some(f) = n.as_f64().filter(|f| n.is_f64() && f.fract() == 0.0 && f.abs() < 9e15) {
                    *v = json!(f as i64);
                }
            }
            serde_json::Value::Array(a) => a.iter_mut().for_each(as_stored),
            serde_json::Value::Object(m) => m.values_mut().for_each(as_stored),
            _ => {}
        }
    }
    fn node_as_stored(n: &mut Composed) {
        n.attributes.values_mut().for_each(as_stored);
        n.children.iter_mut().for_each(node_as_stored);
    }
    let mut node = board_report_node();
    node_as_stored(&mut node);
    let (layer, report) = import(&node, &real_fixture("board-report.msoffice-resaved.docx"), header()).expect("imports");
    assert!(report.is_empty() && layer.data.is_empty(), "{:?}", report.changed);
}

#[test]
fn hyperlinks_come_in_go_out_and_a_changed_address_is_a_change() {
    let link = |url: &str| CharProps { link: Some(url.into()), ..CharProps::default() };
    let mut doc = Document::default();
    doc.body.clear();
    doc.body.push(Arc::new(Block::Para(Paragraph::with_text("Ofqual's guidance", link("https://www.gov.uk/ofqual")))));
    doc.body.push(Arc::new(Block::Para(Paragraph::with_text("public.enquiries@Ofqual.gov.uk", link("mailto:public.enquiries@Ofqual.gov.uk")))));
    let doc = from_docx(&to_docx(&doc).unwrap()).unwrap();
    let (layer, _) = crate::adopt(&doc, "adopted", "adopted", header()).expect("adopts");
    assert!(serde_json::to_string(&layer).unwrap().contains("\"link\":\"mailto:public.enquiries@Ofqual.gov.uk\""));
    let mut loaded = ox_core::load_stack(&example("../../schema/office.ox.json")).expect("schema");
    loaded.push(ox_core::LoadedLayer { source: PathBuf::from("memory"), layer });
    let flat = ox_core::compose::flatten(&loaded);
    let trees = Composer::new(&flat).compose_all().expect("composes");
    let node = find(&trees, "adopted").expect("document").clone();
    let (out, _) = export(&node, &[], std::path::Path::new("")).expect("exports");
    let mut back = from_docx(&to_docx(&out).unwrap()).unwrap();
    let links = |d: &Document| -> Vec<String> {
        d.body.iter().filter_map(|b| b.as_para()).flat_map(|p| p.runs.iter().filter_map(|r| r.props.link.clone())).collect()
    };
    assert_eq!(links(&back), ["https://www.gov.uk/ofqual", "mailto:public.enquiries@Ofqual.gov.uk"]);
    let (layer, report) = import(&node, &back, header()).expect("imports");
    assert!(layer.data.is_empty() && report.changed.is_empty(), "{:?}", report.changed);
    // Someone points the first link elsewhere: one paragraph changes, with its new address.
    for b in back.body.iter_mut() {
        if let Block::Para(p) = Arc::make_mut(b) {
            for r in &mut p.runs {
                if r.props.link.as_deref() == Some("https://www.gov.uk/ofqual") {
                    r.props.link = Some("https://example.org/".into());
                }
            }
        }
    }
    let (layer, report) = import(&node, &back, header()).expect("imports");
    assert_eq!(report.changed.len(), 1, "{:?}", report.changed);
    assert!(serde_json::to_string(&layer).unwrap().contains("https://example.org/"));
}

// ---- Losses found on the real files (examples/real), each with a minimal document ----

fn first_table(d: &Document) -> &wordcraft_doc::Table {
    d.body
        .iter()
        .find_map(|b| match &**b {
            Block::Table(t) => Some(t),
            _ => None,
        })
        .expect("a table")
}

fn texts(c: &wordcraft_doc::Cell) -> Vec<String> {
    c.blocks.iter().filter_map(|b| b.as_para()).map(|p| p.plain_text()).collect()
}

#[test]
fn a_cells_paragraphs_lists_and_spaces_come_back() {
    // The "10 simple rules" paper: a cell holding a sentence, then a bulleted list; the sentence
    // ends in a space. It came back as one paragraph (line breaks), unbulleted, trimmed.
    use wordcraft_doc::numbering::ListKind;
    use wordcraft_doc::props::NumRef;
    use wordcraft_doc::{Cell, Table, para_block};
    let mut doc = Document::new();
    doc.body.clear();
    let num = doc.numbering.add_list(ListKind::Bullet);
    let mut t = Table::new(2, 1, 400.0);
    t.rows[0].cells[0] = Cell::with_text("Rule");
    let mut c = Cell::with_text("One entity is identified by no more than one URI ");
    for text in ["Eliminates costly mapping problems and", "avoids false negatives"] {
        let mut p = Paragraph::with_text(text, CharProps::default());
        p.props.numbering = Some(NumRef { num, level: 0 });
        c.blocks.push(para_block(p));
    }
    t.rows[1].cells[0] = c;
    doc.body.push(Arc::new(Block::Table(t)));
    let out = adopt_and_export(&doc);
    let cell = &first_table(&out).rows[1].cells[0];
    assert_eq!(texts(cell), ["One entity is identified by no more than one URI ", "Eliminates costly mapping problems and", "avoids false negatives"]);
    let levels: Vec<Option<u8>> = cell.blocks.iter().filter_map(|b| b.as_para()).map(|p| p.props.numbering.filter(|n| n.num != 0).map(|n| n.level)).collect();
    assert_eq!(levels, [None, Some(0), Some(0)]);
}

#[test]
fn a_table_nested_in_a_cell_keeps_its_text() {
    // The KonsortSWD contract: numbered clauses laid out as a table inside a cell. The nested
    // table was dropped with its text; now its text stays, as the cell's paragraphs (the nested
    // table itself is a listed loss: ox_formats::inventory "nested tables").
    use wordcraft_doc::{Cell, Table};
    let mut doc = Document::new();
    doc.body.clear();
    let mut inner = Table::new(2, 2, 300.0);
    for (r, row) in [["1)", "Pflicht zur Folgenabschätzung"], ["2)", "Pflicht zur Konsultation"]].iter().enumerate() {
        for (c, text) in row.iter().enumerate() {
            inner.rows[r].cells[c] = Cell::with_text(text);
        }
    }
    let mut outer = Table::new(1, 1, 400.0);
    outer.rows[0].cells[0].blocks.push(Arc::new(Block::Table(inner)));
    outer.rows[0].cells[0].blocks.insert(0, wordcraft_doc::para_block(Paragraph::with_text("Der Auftragsverarbeiter unterstützt:", CharProps::default())));
    doc.body.push(Arc::new(Block::Table(outer)));
    let out = adopt_and_export(&doc);
    let all: Vec<String> = texts(&first_table(&out).rows[0].cells[0]);
    for want in ["Der Auftragsverarbeiter unterstützt:", "1)", "Pflicht zur Folgenabschätzung", "2)", "Pflicht zur Konsultation"] {
        assert!(all.iter().any(|t| t == want), "{want:?} lost: {all:?}");
    }
}

#[test]
fn numbering_turned_off_on_a_numbered_style_stays_off() {
    // The KonsortSWD contract's clause headings use a heading style that numbers, with
    // numbering turned off (numId 0) on each. The export dropped the "off", so they got numbers.
    use wordcraft_doc::numbering::ListKind;
    use wordcraft_doc::props::{NumRef, ParaProps};
    use wordcraft_doc::styles::{Style, StyleKind};
    let mut doc = Document::new();
    doc.body.clear();
    let num = doc.numbering.add_list(ListKind::Numbered);
    doc.styles.upsert(Style {
        id: "Clause".into(),
        name: "Clause".into(),
        kind: StyleKind::Paragraph,
        para: ParaProps { numbering: Some(NumRef { num, level: 0 }), ..ParaProps::default() },
        ..Style::default()
    });
    let numbered = Paragraph::with_text("Clause 1. Scope", CharProps::default()).styled("Clause");
    let mut plain = Paragraph::with_text("Clause 2. Interpretation", CharProps::default()).styled("Clause");
    plain.props.numbering = Some(NumRef { num: 0, level: 0 });
    doc.body.push(Arc::new(Block::Para(numbered)));
    doc.body.push(Arc::new(Block::Para(plain)));
    let out = adopt_and_export(&doc);
    let off: Vec<bool> = out.body.iter().filter_map(|b| b.as_para()).filter(|p| !p.text.is_empty()).map(|p| p.props.numbering.is_some_and(|n| n.num == 0)).collect();
    assert_eq!(off, [false, true]);
}

#[test]
fn tracked_changes_come_in_accepted() {
    // The KonsortSWD contract "mit Nachverfolgung": deleted text came in as live text, next to the
    // insertion that replaced it ("Anhang Anhang EU-Standardvertragsklauseln …").
    use wordcraft_doc::{Revision, RevisionKind};
    let mut doc = Document::new();
    doc.body.clear();
    doc.revisions.push(Revision::new(RevisionKind::Delete, "Autor", "2024-01-01T00:00:00Z"));
    doc.revisions.push(Revision::new(RevisionKind::Insert, "Autor", "2024-01-01T00:00:00Z"));
    let mut p = Paragraph::with_text("gelten die im ", CharProps::default());
    p.append(Paragraph::with_text("Anhang ", CharProps { del: Some(0), ..CharProps::default() }));
    p.append(Paragraph::with_text("Anhang EU-Standardvertragsklauseln", CharProps { ins: Some(1), ..CharProps::default() }));
    p.append(Paragraph::with_text(" beschriebenen Bedingungen", CharProps::default()));
    doc.body.push(Arc::new(Block::Para(p)));
    let out = adopt_and_export(&doc);
    let text: Vec<String> = out.body.iter().filter_map(|b| b.as_para()).map(|p| p.plain_text()).filter(|t| !t.is_empty()).collect();
    assert_eq!(text, ["gelten die im Anhang EU-Standardvertragsklauseln beschriebenen Bedingungen"]);
}

#[test]
fn a_character_styles_formatting_stays_on_its_text() {
    // The KonsortSWD contract makes text bold with a character style ("Fett"); the model has no
    // character styles, and the text came back plain.
    use wordcraft_doc::styles::{Style, StyleKind};
    let mut doc = Document::new();
    doc.body.clear();
    doc.styles.upsert(Style {
        id: "Fett".into(),
        name: "Fett".into(),
        kind: StyleKind::Character,
        chr: CharProps { bold: Some(true), ..CharProps::default() },
        ..Style::default()
    });
    let mut p = Paragraph::with_text("plain ", CharProps::default());
    p.append(Paragraph::with_text("strong", CharProps { style: Some("Fett".into()), ..CharProps::default() }));
    doc.body.push(Arc::new(Block::Para(p)));
    let out = adopt_and_export(&doc);
    let p = out.body.iter().filter_map(|b| b.as_para()).find(|p| p.plain_text() == "plain strong").expect("the paragraph");
    let bold = |off: usize| {
        let cp = p.props_of_char(off);
        cp.bold == Some(true) || cp.style.as_deref().and_then(|s| out.styles.get(s)).is_some_and(|s| s.chr.bold == Some(true))
    };
    assert!(!bold(p.text.find("plain").unwrap()) && bold(p.text.find("strong").unwrap()), "{p:?}");
}

#[test]
fn a_field_kept_as_a_range_stays_a_field() {
    // The KonsortSWD contract: a REF field whose result holds a tracked insertion is read as a
    // range (start, result text, end). The field was dropped; its result stayed as plain text.
    use wordcraft_doc::{Revision, RevisionKind};
    let mut doc = Document::new();
    doc.body.clear();
    doc.revisions.push(Revision::new(RevisionKind::Insert, "Autor", "2024-01-01T00:00:00Z"));
    let mut p = Paragraph::with_text("see ", CharProps::default());
    p.insert_object(4, InlineObject::FieldStart { instr: "REF _Ref153888714 \\h".into(), locked: false, dirty: false }, &CharProps::default()).unwrap();
    p.append(Paragraph::with_text("Annex ", CharProps::default()));
    p.append(Paragraph::with_text("A", CharProps { ins: Some(0), ..CharProps::default() }));
    let end = p.text.len();
    p.insert_object(end, InlineObject::FieldEnd, &CharProps::default()).unwrap();
    p.append(Paragraph::with_text(" below", CharProps::default()));
    doc.body.push(Arc::new(Block::Para(p)));
    let out = adopt_and_export(&doc);
    let p = out.body.iter().filter_map(|b| b.as_para()).find(|p| p.plain_text().contains("Annex A")).expect("the paragraph");
    let instrs: Vec<String> = p
        .objects
        .iter()
        .filter_map(|o| match o {
            InlineObject::Field { instr, .. } | InlineObject::FieldStart { instr, .. } => Some(instr.clone()),
            _ => None,
        })
        .collect();
    assert!(instrs.iter().any(|i| i.contains("REF _Ref153888714")), "{instrs:?}");
    assert_eq!(p.plain_text(), "see Annex A below");
}
