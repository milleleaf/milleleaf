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
