use crate::{Dir, FormatError, Kind, choose, export, import, routes, status};

/// One test, because it changes `OX_SOFFICE` for the whole process.
#[test]
fn routes_fall_back_and_say_why() {
    let real = ox_odf::soffice();

    // Without LibreOffice.
    unsafe { std::env::set_var("OX_SOFFICE", "none") };
    let c = choose(Kind::Document, Dir::Export, "docx").unwrap();
    assert_eq!(c.route.id, "wordcraft-docx");
    assert!(c.skipped.is_empty());

    // ODT export falls back to WordCraft's own writer, and says what's lost.
    let c = choose(Kind::Document, Dir::Export, "odt").unwrap();
    assert_eq!(c.route.id, "wordcraft-odt");
    assert!(!c.route.keeps_identity);
    assert_eq!(c.skipped.len(), 1);
    assert!(
        c.skipped[0].1.contains("OX_SOFFICE=none"),
        "{:?}",
        c.skipped
    );

    // ODT import has no fallback: WordCraft's reader would lose every match.
    match choose(Kind::Document, Dir::Import, "odt") {
        Err(FormatError::Unavailable { reasons, .. }) => {
            assert!(reasons[0].contains("LibreOffice turned off"), "{reasons:?}");
            assert!(reasons[1].contains("not used for imports"), "{reasons:?}");
        }
        other => panic!("{other:?}"),
    }
    // Nothing writes ODP without LibreOffice.
    assert!(matches!(
        choose(Kind::Deck, Dir::Export, "odp"),
        Err(FormatError::Unavailable { .. })
    ));
    // Wrong kind for the format.
    match choose(Kind::Workbook, Dir::Export, "docx") {
        Err(e @ FormatError::Unsupported { .. }) => {
            let m = e.to_string();
            assert!(m.contains(".xlsx") && m.contains(".ods"), "{m}");
        }
        other => panic!("{other:?}"),
    }
    // A wrong path in OX_SOFFICE is reported as such.
    unsafe { std::env::set_var("OX_SOFFICE", "/nowhere/soffice") };
    let r = routes()
        .into_iter()
        .find(|r| r.id == "libreoffice-odp" && r.dir == Dir::Export)
        .unwrap();
    match status(&r) {
        crate::Status::Unavailable { why } => assert!(why.contains("/nowhere/soffice"), "{why}"),
        s => panic!("{s:?}"),
    }

    // A fallback export really writes a file WordCraft can read, without the markers.
    unsafe { std::env::set_var("OX_SOFFICE", "none") };
    let mut doc = wordcraft_doc::Document::default();
    doc.ensure_nonempty();
    let docx = ox_wordcraft::to_docx(&doc).unwrap();
    let (odt, choice, warnings) = export(Kind::Document, "odt", docx.clone()).unwrap();
    assert_eq!(choice.route.id, "wordcraft-odt");
    assert!(odt.starts_with(b"PK"));
    assert!(
        warnings.iter().any(|w| w.contains("can't be brought back")),
        "{warnings:?}"
    );
    let (md, _, _) = export(Kind::Document, "md", docx.clone()).unwrap();
    assert!(std::str::from_utf8(&md).is_ok());
    // DOCX passes straight through.
    let (same, _) = import(Kind::Document, "docx", docx.clone()).unwrap();
    assert_eq!(same, docx);

    // With LibreOffice (when this machine has it), ODF goes through it again.
    match real {
        Some(p) => {
            unsafe { std::env::set_var("OX_SOFFICE", &p) };
            assert_eq!(
                choose(Kind::Document, Dir::Export, "odt").unwrap().route.id,
                "libreoffice-odt"
            );
            assert_eq!(
                choose(Kind::Deck, Dir::Import, "odp").unwrap().route.id,
                "libreoffice-odp"
            );
        }
        None => eprintln!("LibreOffice not installed: skipped the with-LibreOffice half"),
    }
    unsafe { std::env::remove_var("OX_SOFFICE") };
}

/// The counts match `tools/inventory/ooxml_inventory.py` on the real files, and what the adapters
/// don't carry is listed (M2.5: no silent loss).
#[test]
fn inventory_counts_real_files_and_lists_losses() {
    use crate::inventory::{inventory, losses};
    let real = |f: &str| std::fs::read(format!("{}/../../examples/real/{f}", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let lost = |kind: &str, c: &crate::inventory::Counts| -> Vec<&'static str> {
        losses(kind, c, None).iter().map(|l| l.feature).collect()
    };

    let (k, c) = inventory(&real("board-report.docx")).unwrap();
    assert_eq!(k, "docx");
    assert_eq!((c["paragraphs (non-empty)"], c["list paragraphs (numPr)"], c["merged cells (gridSpan/vMerge)"]), (87, 42, 5));
    assert_eq!((c["body text chars"], c["bold text chars"], c["footnote separator lines"]), (10140, 1066, 1));
    let l = lost(k, &c);
    assert_eq!(l, ["media files", "tracked changes (insertions and deletions)"], "the header logo, and a leftover tracked insertion (accepted)");
    assert_eq!((c["page width (value, twips)"], c["default style size (value, half-points)"], c["default style justified (value)"]), (11906, 24, 1), "A4, 12 pt, justified");
    assert!(!l.contains(&"paragraphs (non-empty)") && !l.contains(&"list paragraphs (numPr)"), "{l:?}");

    let (k, c) = inventory(&real("finance.xlsx")).unwrap();
    assert_eq!((c["formulas"], c["comments"], c["defined names (not Milleleaf's)"], c["merged ranges"]), (25932, 219, 20, 444));
    let l = lost(k, &c);
    assert_eq!(c["data validations"], 7, "4 in the sheets, 3 in Excel 2010's x14 extension");
    for carried in ["formulas", "comments", "data validations", "merged ranges", "frozen panes", "defined names (not Milleleaf's)"] {
        assert!(!l.contains(&carried), "{carried}: {l:?}");
    }
    assert!(l.contains(&"charts") && l.contains(&"conditional formats"), "{l:?}");
    assert!(losses(k, &c, None).iter().any(|l| l.feature == "charts" && l.carried == "partly carried"), "the charts come in, their fine formatting doesn't");

    let (k, c) = inventory(&real("briefing.pptx")).unwrap();
    assert_eq!((c["slides"], c["shapes (sp)"], c["shapes without text"], c["notes slide-number fields"]), (28, 59, 1, 28));
    assert_eq!(c["table text chars"], 2152);
    let l = lost(k, &c);
    assert!(l.contains(&"shapes without text") && l.contains(&"tables") && !l.contains(&"slides"), "{l:?}");

    // On import, only what the file holds beyond Milleleaf's own export is reported; an empty
    // shape that isn't Milleleaf's (named ox:…) was added by someone, so it still is.
    let base = c.clone();
    let l: Vec<_> = losses(k, &c, Some(&base)).iter().map(|l| l.feature).collect();
    assert_eq!(l, ["shapes without text"]);
}

/// A minimal .docx from its parts (hand-written XML).
fn docx_of(parts: &[(&str, &str)]) -> Vec<u8> {
    use std::io::Write;
    let mut out = Vec::new();
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(&mut out));
    for (name, xml) in parts {
        z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(xml.as_bytes()).unwrap();
    }
    z.finish().unwrap();
    out
}

/// What the KonsortSWD contract (examples/real's search, Zenodo 10406480) showed the counts
/// missing: a table nested in a cell, a character style, tracked changes, a field whose code Word
/// split in two, and a default font given as the theme's.
#[test]
fn inventory_counts_nested_tables_character_styles_changes_split_fields_and_theme_fonts() {
    use crate::inventory::{inventory, losses};
    const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;
    let cell = |inner: &str| format!("<w:tc><w:p><w:r><w:t>x</w:t></w:r></w:p>{inner}</w:tc>");
    let nested = format!("<w:tbl><w:tblGrid><w:gridCol w:w=\"100\"/><w:gridCol w:w=\"200\"/></w:tblGrid><w:tr>{}{}</w:tr></w:tbl><w:p/>", cell(""), cell(""));
    let doc = format!(
        "<w:document {W}><w:body><w:tbl><w:tblGrid><w:gridCol w:w=\"500\"/></w:tblGrid><w:tr>{}</w:tr></w:tbl>\
         <w:p><w:r><w:rPr><w:rStyle w:val=\"Fett\"/></w:rPr><w:t>bold</w:t></w:r><w:ins w:id=\"1\" w:author=\"a\"><w:r><w:t>new</w:t></w:r></w:ins>\
         <w:del w:id=\"2\" w:author=\"a\"><w:r><w:delText>old</w:delText></w:r></w:del>\
         <w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText> HYP</w:instrText></w:r><w:r><w:instrText>ERLINK \\h </w:instrText></w:r>\
         <w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:t>8</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p></w:body></w:document>",
        cell(&nested)
    );
    let styles = format!(r#"<w:styles {W}><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:asciiTheme="minorHAnsi"/></w:rPr></w:rPrDefault></w:docDefaults></w:styles>"#);
    let theme = r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:themeElements><a:fontScheme><a:majorFont><a:latin typeface="Calibri Light"/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/></a:minorFont></a:fontScheme></a:themeElements></a:theme>"#;
    let bytes = docx_of(&[("word/document.xml", &doc), ("word/styles.xml", &styles), ("word/theme/theme1.xml", theme)]);
    let (k, c) = inventory(&bytes).unwrap();
    assert_eq!((c["tables"], c["nested tables"], c["rows in nested tables"], c["cells in nested tables"]), (2, 1, 1, 2));
    assert_eq!((c["character styles used"], c["styles used"]), (1, 0));
    assert_eq!(c["tracked changes (insertions and deletions)"], 2);
    assert_eq!(c["fields"], 1, "one field, though its code is in two pieces");
    // FNV-1a of "Calibri", the theme's minor font (the same in the Python counter).
    let fnv = |s: &str| s.bytes().fold(0x811C_9DC5u32, |h, b| (h ^ b as u32).wrapping_mul(0x0100_0193)) as u64;
    assert_eq!(c["default style font (value, name hash)"], fnv("Calibri"));
    // The nested table's widths go with it: only the outer table's grid is in the hash.
    assert_eq!(c["table column widths (value, hash)"], fnv("500"));
    let lost: Vec<&str> = losses(k, &c, None).iter().map(|l| l.feature).collect();
    for f in ["nested tables", "rows in nested tables", "cells in nested tables", "character styles used", "tracked changes (insertions and deletions)"] {
        assert!(lost.contains(&f), "{f} not listed: {lost:?}");
    }
}

/// What the real-file search's workbooks showed the counts getting wrong: a what-if data table
/// (SIB-LCA, Zenodo 4742246) counted as a formula, protection settings without protection on
/// (FAIR maturity model, Zenodo 3909563) counted as a protected sheet, Excel's
/// `_xlnm._FilterDatabase` counted as the file's own name, filters not counted at all (Imperial
/// College, Zenodo 3754178), and surface charts, which GridCraft reads as nothing (SIB-LCA).
#[test]
fn inventory_counts_data_tables_protection_filters_and_surface_charts() {
    use crate::inventory::{inventory, losses};
    const NS: &str = r#"xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main""#;
    let sheet = |extra: &str| {
        format!(
            "<worksheet {NS}><sheetData><row r=\"1\"><c r=\"A1\"><f>1+1</f><v>2</v></c>\
             <c r=\"B1\"><f t=\"dataTable\" ref=\"B1:B2\" dt2D=\"0\" dtr=\"0\" r1=\"A1\"/><v>3</v></c></row></sheetData>{extra}</worksheet>"
        )
    };
    let wb = format!(
        "<workbook {NS}><sheets><sheet name=\"S1\" sheetId=\"1\"/><sheet name=\"S2\" sheetId=\"2\"/></sheets><definedNames>\
         <definedName name=\"_xlnm._FilterDatabase\" localSheetId=\"0\" hidden=\"1\">S1!$A$1:$A$2</definedName>\
         <definedName name=\"Rate\">S1!$A$1</definedName></definedNames></workbook>"
    );
    let chart = r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart><c:plotArea><c:surface3DChart/></c:plotArea></c:chart></c:chartSpace>"#;
    let s1 = sheet("<sheetProtection selectLockedCells=\"1\"/><autoFilter ref=\"A1:A2\"/>");
    let s2 = sheet("<sheetProtection sheet=\"1\" objects=\"1\"/>");
    let bytes = docx_of(&[("xl/workbook.xml", &wb), ("xl/worksheets/sheet1.xml", &s1), ("xl/worksheets/sheet2.xml", &s2), ("xl/charts/chart1.xml", chart)]);
    let (k, c) = inventory(&bytes).unwrap();
    assert_eq!(k, "xlsx");
    assert_eq!((c["formulas"], c["what-if data tables"]), (2, 2));
    assert_eq!(c["protected sheets"], 1, "only the sheet with protection on");
    assert_eq!((c["defined names (not Milleleaf's)"], c["autofilters"]), (1, 1));
    assert_eq!(c["surface charts"], 1);
    let lost: Vec<&str> = losses(k, &c, None).iter().map(|l| l.feature).collect();
    assert!(lost.contains(&"what-if data tables") && lost.contains(&"surface charts") && !lost.contains(&"autofilters"), "{lost:?}");
}

/// Notes pages holding only their slide number (PowerPoint adds them; the DDI and Navarra decks
/// in the real-file search have them) hold no notes, so they don't count as notes slides.
#[test]
fn a_notes_page_with_only_its_slide_number_isnt_counted() {
    use crate::inventory::inventory;
    const NS: &str = r#"xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main""#;
    let notes = |body: &str| format!("<p:notes {NS}><p:cSld><p:spTree><p:sp><p:txBody><a:p>{body}</a:p></p:txBody></p:sp></p:spTree></p:cSld></p:notes>");
    let num = r#"<a:fld id="{1}" type="slidenum"><a:t>7</a:t></a:fld>"#;
    let pres = format!("<p:presentation {NS}/>");
    let slide = format!("<p:sld {NS}><p:cSld><p:spTree/></p:cSld></p:sld>");
    let n1 = notes(num);
    let n2 = notes(&format!("<a:r><a:t>Say hello</a:t></a:r>{num}"));
    let bytes = docx_of(&[("ppt/presentation.xml", &pres), ("ppt/slides/slide1.xml", &slide), ("ppt/notesSlides/notesSlide1.xml", &n1), ("ppt/notesSlides/notesSlide2.xml", &n2)]);
    let (_, c) = inventory(&bytes).unwrap();
    assert_eq!((c["notes slides"], c["notes slide-number fields"]), (1, 2));
}
