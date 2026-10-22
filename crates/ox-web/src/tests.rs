use super::*;

const DOCX: &[u8] = include_bytes!("../../../examples/real/board-report.docx");
const PPTX: &[u8] = include_bytes!("../../../examples/real/briefing.pptx");
const XLSX: &[u8] =
    include_bytes!("../../../examples/hello-table/fixtures/finance.msoffice-resaved.xlsx");

#[test]
fn real_documents_and_workbooks_adopt_with_pictures_and_inventory() {
    for (bytes, format) in [(DOCX, "docx"), (PPTX, "pptx"), (XLSX, "xlsx")] {
        let a = read(bytes, header("base", "Original", "")).unwrap();
        assert_eq!(a.format, format);
        assert!(!a.counts.is_empty());
        assert!(!a.layer.data.is_empty());
        assert!(!tree(&a.layer).unwrap().children.is_empty());
        if format != "xlsx" {
            assert!(!a.pictures.is_empty());
        }
    }
}

#[test]
fn identical_files_have_no_model_or_inventory_changes() {
    let mut demo = Demo::new();
    demo.adopt(DOCX).unwrap();
    let result: Value =
        serde_json::from_str(&demo.diff(DOCX, "Reviewer", "2026-10-22T08:30:00Z").unwrap())
            .unwrap();
    assert_eq!(result["changes"], json!([]));
    assert_eq!(result["inventory_changes"], json!([]));
    assert_eq!(result["part_changes"], json!([]));
}

#[test]
fn feature_reductions_and_adapter_limits_are_separate() {
    let a = Counts::from([("merged cells", 2), ("comments", 3)]);
    let b = Counts::from([("merged cells", 0), ("comments", 3)]);
    let changes = inventory_changes(&a, &b);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0]["feature"], "merged cells");
    assert!(
        changes[0]["status"]
            .as_str()
            .unwrap()
            .contains("possible loss")
    );
    let adopted = read(PPTX, header("base", "Original", "")).unwrap();
    assert!(!losses("pptx", &adopted.counts, None).is_empty());
}

fn signed() -> Value {
    let mut demo = Demo::new();
    demo.adopt(XLSX).unwrap();
    demo.diff(
        include_bytes!("../../../examples/hello-table/fixtures/finance.openpyxl-edited.xlsx"),
        "Reviewer",
        "2026-10-22T08:30:00Z",
    )
    .unwrap();
    serde_json::from_str(&demo.sign().unwrap()).unwrap()
}

fn valid(proof: &Value) -> bool {
    verify_proof(&serde_json::to_vec(proof).unwrap(), "2026-10-22T08:30:00Z").unwrap()["valid"]
        == true
}

#[test]
fn hybrid_proof_verifies_and_binds_author_time_baseline_and_file_hashes() {
    let proof = signed();
    assert!(valid(&proof));
    assert_eq!(proof["signatures"].as_array().unwrap().len(), 2);
    for field in ["author", "timestamp", "base"] {
        let mut changed = proof.clone();
        changed["layer"]["header"][field] = json!("tampered");
        assert!(!valid(&changed), "{field}");
    }
    let mut changed = proof.clone();
    changed["baseline"]["header"]["author"] = json!("tampered");
    assert!(!valid(&changed));
    let mut changed = proof.clone();
    changed["layer"]["data"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap()["attributes"]["demo::comparison"]["edited_hash"] = json!("tampered");
    assert!(!valid(&changed));
}

#[test]
fn deleting_pq_or_swapping_a_public_key_fails() {
    let proof = signed();
    let mut changed = proof.clone();
    changed["signatures"]
        .as_array_mut()
        .unwrap()
        .retain(|s| s["alg"] != ox_store::pq::MLDSA65);
    assert!(!valid(&changed));
    let mut changed = proof.clone();
    changed["public_keys"][0][1] = json!("ed25519:wrong");
    assert!(!valid(&changed));
    let mut changed = proof.clone();
    changed["signatures"][0]["at"] = json!("2099-01-01T00:00:00Z");
    assert!(!valid(&changed));
}

#[test]
fn invalid_archives_are_refused_before_format_readers() {
    assert!(read(b"not a zip", header("base", "", "")).is_err());
    assert!(verify_proof(b"{}", "2026-10-22T08:30:00Z").is_err());
}

#[test]
fn expiry_is_checked_at_verification_time_not_the_claimed_signing_time() {
    let proof = signed();
    let result =
        verify_proof(&serde_json::to_vec(&proof).unwrap(), "2040-01-01T00:00:00Z").unwrap();
    assert_eq!(result["valid"], false);
    assert!(!result["policy"]["refused"].as_array().unwrap().is_empty());
}

#[test]
fn actual_cell_formatting_is_not_grouped_away_as_a_rewrite() {
    let original = read(XLSX, header("base", "Original", "")).unwrap();
    let mut wb = ox_gridcraft::from_xlsx(XLSX).unwrap();
    let (at, mut cell) = wb.sheets[0]
        .cells
        .iter()
        .find(|(at, _)| at.a1() == "B3")
        .map(|(at, c)| (at, c.clone()))
        .unwrap();
    let mut style = wb.styles.get(cell.style).clone();
    style.font.bold = !style.font.bold;
    cell.style = wb.styles.intern(style);
    wb.sheet_mut(0).unwrap().set_cell(at, cell);
    let (_, report) = compare_file(
        &original,
        &ox_gridcraft::to_xlsx(&wb).unwrap(),
        header("proposal", "Reviewer", "2026-10-22T00:00:00Z"),
    )
    .unwrap();
    assert!(report["changes"].as_array().unwrap().iter().any(|c| {
        c["description"]
            .as_str()
            .unwrap()
            .contains("formatting changed")
    }));
}

#[test]
fn an_unmarked_workbooks_new_cell_is_not_silently_ignored() {
    let mut wb = ox_gridcraft::from_xlsx(XLSX).unwrap();
    wb.names.clear();
    let bytes = ox_gridcraft::to_xlsx(&wb).unwrap();
    let original = read(&bytes, header("base", "Original", "")).unwrap();
    let (mut at, cell) = wb.sheets[0]
        .cells
        .iter()
        .find(|(at, _)| at.a1() == "B3")
        .map(|(at, c)| (at, c.clone()))
        .unwrap();
    at.row = 20;
    at.col = 2;
    wb.sheet_mut(0).unwrap().set_cell(at, cell);
    let (_, report) = compare_file(
        &original,
        &ox_gridcraft::to_xlsx(&wb).unwrap(),
        header("proposal", "Reviewer", "2026-10-22T00:00:00Z"),
    )
    .unwrap();
    assert!(
        report["changes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["place"] == "Sheet Q3, cell C21" && c["kind"] == "added")
    );
}
