use std::path::PathBuf;

use deckcraft_model::{Presentation, ShapeKind};
use ox_core::render::field_text;
use ox_core::{Composed, Composer, Header, Layer};
use serde_json::json;

use crate::{export, from_pptx, import, to_pptx};

const TABLE: &str = "3f1c2a90-0000-4c1e-9a00-000000000012";

fn example(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/hello-table")
        .join(file)
}

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

/// The deck html2ox made from `deck.html`: the `deck::deck` node under the project.
fn deck(trees: &[Composed]) -> Composed {
    fn walk(n: &Composed) -> Option<&Composed> {
        if n.attributes.contains_key("deck::deck") {
            return Some(n);
        }
        n.children.iter().find_map(walk)
    }
    trees.iter().find_map(walk).expect("a deck").clone()
}

fn header() -> Header {
    Header {
        id: "test-deck-edit".into(),
        ox_version: String::new(),
        data_version: "1.0.0".into(),
        author: "test".into(),
        timestamp: "2026-10-15".into(),
        kind: None,
        ..Default::default()
    }
}

fn exported(trees: &[Composed]) -> (Composed, Presentation) {
    let d = deck(trees);
    let (p, report) = export(&d, &example("")).expect("exports");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    (
        d,
        from_pptx(&to_pptx(&p).expect("writes")).expect("reads back"),
    )
}

fn fixture(name: &str) -> Presentation {
    from_pptx(&std::fs::read(example("fixtures").join(name)).expect("fixture")).expect("reads")
}

fn frame<'a>(d: &'a Composed, slide: &str, name: &str) -> &'a Composed {
    let s = d
        .children
        .iter()
        .find(|s| s.name.as_deref() == Some(slide))
        .expect("slide");
    s.children
        .iter()
        .find(|f| f.name.as_deref() == Some(name))
        .expect("frame")
}

fn text_of(fr: &Composed) -> Vec<String> {
    fr.attributes["deck::text"]["paragraphs"]
        .as_array()
        .expect("paragraphs")
        .iter()
        .map(|p| {
            p["runs"]
                .as_array()
                .expect("runs")
                .iter()
                .map(|r| {
                    r["text"]
                        .as_str()
                        .map(String::from)
                        .unwrap_or_else(|| field_text(fr, r["field"].as_str().unwrap_or("")))
                })
                .collect()
        })
        .collect()
}

#[test]
fn export_names_everything_and_draws_tables_and_charts_from_data() {
    let trees = compose(&[]);
    let (d, p) = exported(&trees);
    assert_eq!(p.slides.len(), 3);
    let names: Vec<&str> = p
        .slides
        .iter()
        .flat_map(|s| s.shapes.iter().map(|sh| sh.name.as_str()))
        .collect();
    assert!(names.iter().all(|n| n.starts_with("ox:")), "{names:?}");
    let s2 = &p.slides[1];
    let chart = s2
        .shapes
        .iter()
        .find_map(|sh| {
            if let ShapeKind::Chart(c) = &sh.kind {
                Some(c)
            } else {
                None
            }
        })
        .expect("native chart");
    assert_eq!(chart.categories, vec!["Adelaide", "Hong Kong", "Singapore"]);
    assert_eq!(
        chart.series[0].values,
        vec![Some(412000.0), Some(541500.0), Some(297000.0)]
    );
    let table = s2
        .shapes
        .iter()
        .find_map(|sh| {
            if let ShapeKind::Table(t) = &sh.kind {
                Some(t)
            } else {
                None
            }
        })
        .expect("native table");
    assert_eq!(table.rows[2].cells[1].text.text().trim(), "$541,500");
    let summary = s2
        .shapes
        .iter()
        .find(|sh| {
            sh.text
                .as_ref()
                .is_some_and(|t| t.text().contains("Total revenue"))
        })
        .expect("summary");
    assert_eq!(
        summary
            .text
            .as_ref()
            .map(|t| t.text())
            .unwrap_or_default()
            .trim(),
        "Total revenue: $1,250,500 across 3 regions."
    );
    assert!(p.props.keywords.contains(&d.path));
}

#[test]
fn unedited_and_impress_resaved_import_as_no_change() {
    let trees = compose(&[]);
    let (d, p) = exported(&trees);
    let (layer, report) = import(&d, &p, header()).expect("imports");
    assert!(
        report.is_empty() && layer.data.is_empty() && !report.stale,
        "{report:?} {:?}",
        layer.data
    );
    // LibreOffice Impress drops slide names and rewrites much of the XML; still no change.
    // PowerPoint's own resave likewise.
    for name in [
        "deck.libreoffice-resaved.pptx",
        "deck.msoffice-resaved.pptx",
    ] {
        let (layer, report) = import(&d, &fixture(name), header()).expect("imports");
        assert!(
            report.is_empty() && report.conflicts.is_empty(),
            "{name}: {report:?}"
        );
        assert!(layer.data.is_empty(), "{name}: {:?}", layer.data);
    }
}

#[test]
fn impress_edits_come_back_and_compose() {
    edits_come_back_and_compose("deck.libreoffice-edited.pptx");
}

#[test]
fn powerpoint_edits_come_back_and_compose() {
    edits_come_back_and_compose("deck.msoffice-edited.pptx");
}

fn edits_come_back_and_compose(name: &str) {
    // In Impress and in PowerPoint 16: title moved down 1 cm, accent bar deleted, notes
    // rewritten, "Total revenue" → "Group revenue" (around the two linked numbers), a bullet
    // extended, a text box added, and $999 typed into the linked table.
    let trees = compose(&[]);
    let d = deck(&trees);
    let (layer, report) = import(&d, &fixture(name), header()).expect("imports");
    assert_eq!(report.moved.len(), 1, "{report:?}");
    assert_eq!(report.removed, vec!["shape-1".to_string()]);
    assert_eq!(report.added.len(), 1);
    assert!(report.reordered.is_empty(), "{report:?}");
    assert_eq!(
        report
            .conflicts
            .iter()
            .map(|c| c.path.as_str())
            .collect::<Vec<_>>(),
        vec!["987b7602-b694-53d2-b390-57f9d8ca3870"]
    );

    let after = deck(&compose(&[layer]));
    let title = frame(&after, "title", "text-1");
    assert!((title.attributes["deck::frame"]["y"].as_f64().unwrap_or(0.0) - 287.8).abs() < 0.5);
    let s1 = after
        .children
        .iter()
        .find(|s| s.name.as_deref() == Some("title"))
        .expect("title slide");
    assert!(
        s1.children
            .iter()
            .all(|f| f.name.as_deref() != Some("shape-1"))
    );
    assert_eq!(
        s1.attributes["deck::slide"]["notes"],
        json!("Open with the headline, then hand over to Finance.")
    );
    // The reworded summary keeps both live numbers as fields.
    let summary = frame(&after, "revenue", "text-2");
    assert_eq!(
        text_of(summary),
        vec!["Group revenue: $1,250,500 across 3 regions."]
    );
    let runs = summary.attributes["deck::text"]["paragraphs"][0]["runs"]
        .as_array()
        .expect("runs");
    assert_eq!(runs.iter().filter(|r| r.get("field").is_some()).count(), 2);
    assert_eq!(
        text_of(frame(&after, "next-steps", "text-2"))[1],
        "Singapore pipeline review (two deals)"
    );
    let s3 = after
        .children
        .iter()
        .find(|s| s.name.as_deref() == Some("next-steps"))
        .expect("slide 3");
    assert_eq!(s3.children.len(), 4);
    assert_eq!(
        text_of(s3.children.last().expect("new box")),
        vec!["Owner: Finance"]
    );
}

#[test]
fn reordering_slides_is_an_order_change() {
    let trees = compose(&[]);
    let (d, mut p) = exported(&trees);
    p.slides.swap(1, 2);
    let (layer, report) = import(&d, &p, header()).expect("imports");
    assert_eq!(report.reordered, vec![d.path.clone()]);
    assert_eq!(layer.data.len(), 1);
    assert_eq!(
        layer.data[0].order,
        Some(vec![
            "title".to_string(),
            "next-steps".to_string(),
            "revenue".to_string()
        ])
    );
}

#[test]
fn charts_follow_the_workbook() {
    // A workbook edit adds Perth: the exported chart, table and region count follow.
    let edit: Layer = serde_json::from_value(json!({
        "header": { "id": "grid-edit", "oxVersion": "ox_alpha", "dataVersion": "1.0.0", "author": "test", "timestamp": "2026-10-15" },
        "data": [{ "path": TABLE, "attributes": { "office::data::table": {
            "columns": [{ "name": "Region", "type": "text" }, { "name": "Revenue", "type": "currency" }],
            "rows": [["Adelaide", 412000], ["Hong Kong", 541500], ["Perth", 120000], ["Singapore", 305250]] } } }]
    }))
    .expect("layer");
    let trees = compose(&[edit]);
    let (_, p) = exported(&trees);
    let s2 = &p.slides[1];
    let chart = s2
        .shapes
        .iter()
        .find_map(|sh| {
            if let ShapeKind::Chart(c) = &sh.kind {
                Some(c)
            } else {
                None
            }
        })
        .expect("chart");
    assert_eq!(
        chart.categories,
        vec!["Adelaide", "Hong Kong", "Perth", "Singapore"]
    );
    let summary = s2
        .shapes
        .iter()
        .find(|sh| {
            sh.text
                .as_ref()
                .is_some_and(|t| t.text().contains("regions"))
        })
        .expect("summary");
    assert!(
        summary
            .text
            .as_ref()
            .map(|t| t.text())
            .unwrap_or_default()
            .contains("across 4 regions")
    );
}

#[test]
fn slides_matched_by_their_frames_are_best_guesses_and_flagged() {
    use ox_core::Confidence;
    use ox_core::matching::{MATCH_ATTR, best_guesses};
    let trees = compose(&[]);
    let d = deck(&trees);
    let slides: Vec<&str> = d.children.iter().map(|s| s.path.as_str()).collect();

    // PowerPoint keeps slide names: every match is certain, nothing is flagged.
    let (layer, report) =
        import(&d, &fixture("deck.msoffice-edited.pptx"), header()).expect("imports");
    assert!(
        best_guesses(&report.matches).is_empty(),
        "{:?}",
        report.matches
    );
    assert!(
        layer
            .data
            .iter()
            .all(|n| !n.attributes.contains_key(MATCH_ATTR))
    );

    // Impress drops them: each slide is recognised by its frames, a best guess, and the slide
    // nodes the edit layer touches carry `ox::match` for review.
    for name in [
        "deck.libreoffice-edited.pptx",
        "deck.libreoffice-resaved.pptx",
    ] {
        let (layer, report) = import(&d, &fixture(name), header()).expect("imports");
        let guesses: Vec<&str> = best_guesses(&report.matches)
            .iter()
            .map(|m| m.path.as_str())
            .collect();
        assert_eq!(guesses, slides, "{name}");
        assert!(
            report
                .matches
                .iter()
                .any(|m| m.confidence == Confidence::Certain)
        );
        for n in &layer.data {
            assert_eq!(
                n.attributes.contains_key(MATCH_ATTR),
                slides.contains(&n.path.as_str()),
                "{name}: {}",
                n.path
            );
        }
    }
}

#[test]
fn a_missing_picture_stops_the_export() {
    let trees = compose(&[]);
    let d = deck(&trees);
    let empty = std::env::temp_dir().join(format!("ox-no-assets-{}", std::process::id()));
    std::fs::create_dir_all(&empty).unwrap();
    let err = export(&d, &empty).err().expect("export must fail without its pictures");
    assert!(matches!(err, crate::DeckError::MissingAsset(_)), "{err}");
    let _ = std::fs::remove_dir_all(&empty);
}

#[test]
fn a_duplicated_slide_imports_as_best_guesses() {
    let trees = compose(&[]);
    let (d, mut p) = exported(&trees);
    let first = d.children[0].path.clone();
    let copy = p.slides[0].clone();
    p.slides.push(copy);
    let (_layer, report) = import(&d, &p, header()).expect("imports");
    let guesses = ox_core::matching::best_guesses(&report.matches);
    assert!(guesses.iter().any(|m| m.path == first), "{:?}", report.matches);
    // The copy's shapes carry the original names: the originals are best guesses too.
    let frame = d.children[0].children[0].path.clone();
    assert!(guesses.iter().any(|m| m.path == frame), "{:?}", report.matches);
    assert!(report.warnings.iter().any(|w| w.contains("appears 2 times")), "{:?}", report.warnings);
    assert!(!report.added.is_empty(), "the copy comes in as a new slide: {report:?}");
}

#[test]
fn adopting_a_deck_carries_text_and_names_what_it_cannot() {
    let p = fixture("deck.msoffice-edited.pptx");
    let (layer, report) = crate::adopt(&p, "adopted-deck", "Board deck", header()).expect("adopts");
    let root = layer.data.iter().find(|n| n.path == "adopted-deck").expect("deck node");
    assert!(root.attributes.contains_key("deck::deck"));
    assert!(layer.data.iter().any(|n| n.attributes.contains_key("deck::text")));
    for kind in ["(table)", "(chart)", "(picture)"] {
        assert!(report.warnings.iter().any(|w| w.contains(kind)), "{kind}: {:?}", report.warnings);
    }
}

#[test]
fn unicode_survives_a_pptx_round_trip_code_point_for_code_point() {
    // Same strings as the GridCraft test (tdf#173771, tdf#150398, VS15/VS16, ZWJ, combining marks).
    let cases = [
        "5. ♖🪴🧾🚂🙭🎨🆞🆣🆫𝝿𔑲",
        "VS16 ❤\u{FE0F} VS15 ❤\u{FE0E} ☎\u{FE0F}",
        "ZWJ 👩\u{200D}💻 flag 🇭🇰",
        "combining e\u{0301} n\u{0303} 한국어 العربية 𓂀",
    ];
    let paragraphs: Vec<_> = cases.iter().map(|c| json!({ "runs": [{ "text": c }] })).collect();
    let mut l = Layer { header: header(), imports: vec![], schemas: Default::default(), data: vec![] };
    l.data.push(ox_core::Node {
        path: "79cf7543-79e8-551e-992b-1899612ea818".into(),
        attributes: [("deck::text".to_string(), json!({ "role": "title", "paragraphs": paragraphs }))].into_iter().collect(),
        ..Default::default()
    });
    let trees = compose(&[l]);
    let (_d, p) = exported(&trees);
    let all: String = p.slides.iter().flat_map(|s| s.shapes.iter()).filter_map(|sh| sh.text.as_ref()).map(|t| t.text()).collect::<Vec<_>>().join("\n");
    for c in cases {
        assert!(all.contains(c), "lost or changed: {c:?}\n{all}");
    }
}

#[test]
fn adopted_placeholders_take_their_layouts_position() {
    // A real GOV.UK deck (examples/real, OGL v3): slide titles are placeholders with no position of
    // their own; they used to be dropped without a word.
    let path = example("../real/briefing.pptx");
    let p = from_pptx(&std::fs::read(path).expect("examples/real/briefing.pptx")).expect("reads");
    let (layer, report) = crate::adopt(&p, "real-deck", "briefing", header()).expect("adopts");
    let text: String = layer.data.iter().filter_map(|n| n.attributes.get("deck::text")).map(|v| v.to_string()).collect();
    for title in ["GCSE science grading", "Post-16 school/college measures", "Ofqual resources"] {
        assert!(text.contains(title), "{title} lost; warnings: {:?}", report.warnings);
    }
    assert!(!report.warnings.iter().any(|w| w.contains("no position")), "{:?}", report.warnings);
}

#[test]
fn adopted_text_takes_what_its_layout_gives_it() {
    // The Ofqual/DfE briefing: bullets, levels and their colour come from the master; the boxes
    // on slide 23 are filled and outlined, with centred text.
    let bytes = std::fs::read(example("../real/briefing.pptx")).expect("fixture");
    let p = from_pptx(&bytes).expect("reads");
    let (layer, _) = crate::adopt(&p, "real-deck", "briefing", header()).expect("adopts");
    let texts: Vec<&serde_json::Value> = layer.data.iter().filter_map(|n| n.attributes.get("deck::text")).collect();
    let paras = || texts.iter().flat_map(|t| t["paragraphs"].as_array().into_iter().flatten());
    let nested = paras()
        .find(|p| p["runs"][0]["text"] == json!("Policy, purposes and priorities"))
        .expect("a second-level bullet on slide 3");
    assert_eq!(nested["level"], json!(1));
    assert_eq!(nested["bullet"], json!(true));
    assert!(nested["bulletColor"].is_string() && nested["marginLeftPt"].is_number(), "{nested}");
    let english = layer
        .data
        .iter()
        .find(|n| n.attributes.get("deck::text").is_some_and(|t| t["paragraphs"][0]["runs"][0]["text"] == json!("English")))
        .expect("the English box");
    assert_eq!(english.attributes["deck::shape"]["fill"], json!("#ccecff"));
    assert!(english.attributes["deck::shape"]["stroke"].is_string());
    assert_eq!(english.attributes["deck::text"]["verticalAlign"], json!("middle"));
}

/// The real briefing deck, adopted the way `ox adopt` does it (`examples/real/briefing.pptx`).
fn briefing_node() -> Composed {
    let real = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/real");
    // `ox adopt`'s path for this file: stable_id("adopt|" + its SHA-256).
    let root = "0eff38b6-b913-5af3-91ae-86226a64270e";
    let p = from_pptx(&std::fs::read(real.join("briefing.pptx")).unwrap()).unwrap();
    let (layer, _) = crate::adopt(&p, root, "briefing", header()).expect("adopts");
    let mut loaded = ox_core::load_stack(&example("../../schema/office.ox.json")).expect("schema");
    loaded.push(ox_core::LoadedLayer { source: PathBuf::from("memory"), layer });
    let flat = ox_core::compose::flatten(&loaded);
    let trees = Composer::new(&flat).compose_all().expect("composes");
    ox_core::render::find(&trees, root).expect("deck").clone()
}

fn real_fixture(name: &str) -> Presentation {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/real/fixtures").join(name);
    from_pptx(&std::fs::read(p).expect("fixture")).expect("reads")
}

#[test]
fn real_deck_resaves_are_no_change_and_an_edit_keeps_the_layouts_look() {
    let node = briefing_node();
    for name in ["briefing.libreoffice-resaved.pptx", "briefing.msoffice-resaved.pptx"] {
        let (layer, report) = import(&node, &real_fixture(name), header()).expect("imports");
        assert!(layer.data.is_empty() && report.changed.is_empty(), "{name}: {:?}", report.changed);
    }
    // PowerPoint 16 (Mac): " (revised)" added to slide 1's subtitle frame. One frame changes, and
    // its paragraphs keep the indents and spacing adopt took from the layout.
    let (layer, report) = import(&node, &real_fixture("briefing.msoffice-edited.pptx"), header()).expect("imports");
    assert_eq!(report.changed.len(), 1, "{:?}", report.changed);
    let paras = layer.data[0].attributes["deck::text"]["paragraphs"].as_array().unwrap().clone();
    let text: String = paras.iter().flat_map(|p| p["runs"].as_array().cloned().unwrap_or_default()).filter_map(|r| r["text"].as_str().map(String::from)).collect();
    assert!(text.contains("(revised)"), "{text}");
    assert!(paras.iter().all(|p| p.get("marginLeftPt").is_some() && p.get("spaceBefore").is_some()), "{paras:?}");
}
