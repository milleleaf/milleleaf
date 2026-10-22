//! Files written before the rename to Milleleaf (23 Oct 2026) carry `ox` markers: Word control tags
//! `ox:<path>`, document properties `ox.layers`, Excel names `ox_<path>`, slide shape names `ox:<path>`,
//! deck keywords `oxdeck=` / `oxrevision=` / `oxpack=`, and picture stand-ins `ox-stand-in:`. The readers
//! know only the `mleaf` forms, so `upgrade` rewrites the old ones in an Office file before it is parsed.
//! A file without old markers comes back unchanged (borrowed); nothing else in the file is touched.
use std::borrow::Cow;
use std::io::{Cursor, Read, Write};

const SWAPS: [(&[u8], &[u8]); 7] = [
    (b"w:val=\"ox:", b"w:val=\"mleaf:"),
    (b"name=\"ox:", b"name=\"mleaf:"),
    (b"name=\"ox_", b"name=\"mleaf_"),
    (b"name=\"ox.", b"name=\"mleaf."),
    (b"oxdeck=", b"mleafdeck="),
    (b"oxrevision=", b"mleafrevision="),
    (b"oxpack=", b"mleafpack="),
];
const OLD_STAND_IN: &[u8] = b"ox-stand-in:";

fn swap(part: &[u8]) -> Option<Vec<u8>> {
    if let Some(rest) = part.strip_prefix(OLD_STAND_IN) {
        return Some([crate::STAND_IN, rest].concat());
    }
    let mut out = part.to_vec();
    for (old, new) in SWAPS {
        if out.windows(old.len()).any(|w| w == old) {
            let mut next = Vec::with_capacity(out.len());
            let mut i = 0;
            while i < out.len() {
                if out[i..].starts_with(old) {
                    next.extend_from_slice(new);
                    i += old.len();
                } else {
                    next.push(out[i]);
                    i += 1;
                }
            }
            out = next;
        }
    }
    (out != part).then_some(out)
}

/// An Office file (zip) with its old `ox` markers rewritten to `mleaf`; anything else as given.
pub fn upgrade(bytes: &[u8]) -> Cow<'_, [u8]> {
    let Ok(mut zip) = zip::ZipArchive::new(Cursor::new(bytes)) else { return Cow::Borrowed(bytes) };
    let mut changed = Vec::new();
    for i in 0..zip.len() {
        let Ok(mut f) = zip.by_index(i) else { return Cow::Borrowed(bytes) };
        let mut part = Vec::new();
        if f.is_dir() || f.read_to_end(&mut part).is_err() {
            continue;
        }
        if let Some(new) = swap(&part) {
            changed.push((i, new));
        }
    }
    if changed.is_empty() {
        return Cow::Borrowed(bytes);
    }
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let written = match changed.iter().find(|(j, _)| *j == i) {
            Some((_, new)) => zip.by_index(i).and_then(|f| {
                let opts = zip::write::SimpleFileOptions::default().compression_method(f.compression());
                let name = f.name().to_string();
                drop(f);
                out.start_file(name, opts)?;
                out.write_all(new).map_err(zip::result::ZipError::Io)
            }),
            None => zip.by_index_raw(i).and_then(|f| out.raw_copy_file(f)),
        };
        if written.is_err() {
            return Cow::Borrowed(bytes);
        }
    }
    match out.finish() {
        Ok(c) => Cow::Owned(c.into_inner()),
        Err(_) => Cow::Borrowed(bytes),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn office(parts: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, body) in parts {
            w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(body).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    fn part(bytes: &[u8], name: &str) -> Vec<u8> {
        let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut v = Vec::new();
        z.by_name(name).unwrap().read_to_end(&mut v).unwrap();
        v
    }

    #[test]
    fn old_markers_become_mleaf_and_nothing_else_changes() {
        let pic = [b"ox-stand-in:".as_slice(), b"blob/ab"].concat();
        let old = office(&[
            ("word/document.xml", b"<w:tag w:val=\"ox:report/summary\"/><w:t>ox: oxen</w:t>"),
            ("docProps/custom.xml", b"<property name=\"ox.layers\"><vt:lpwstr>a b</vt:lpwstr></property>"),
            ("xl/workbook.xml", b"<definedName name=\"ox_workbook\">x</definedName><definedName name=\"Print_Area\"/>"),
            ("ppt/slides/slide1.xml", b"<p:cNvPr id=\"2\" name=\"ox:deck/s1\"/>"),
            ("docProps/core.xml", b"<cp:keywords>oxdeck=d oxrevision=r oxpack=p</cp:keywords>"),
            ("word/media/image1.png", &pic),
        ]);
        let new = upgrade(&old);
        assert!(matches!(new, Cow::Owned(_)));
        assert_eq!(part(&new, "word/document.xml"), b"<w:tag w:val=\"mleaf:report/summary\"/><w:t>ox: oxen</w:t>");
        assert_eq!(part(&new, "docProps/custom.xml"), b"<property name=\"mleaf.layers\"><vt:lpwstr>a b</vt:lpwstr></property>");
        assert_eq!(part(&new, "xl/workbook.xml"), b"<definedName name=\"mleaf_workbook\">x</definedName><definedName name=\"Print_Area\"/>");
        assert_eq!(part(&new, "ppt/slides/slide1.xml"), b"<p:cNvPr id=\"2\" name=\"mleaf:deck/s1\"/>");
        assert_eq!(part(&new, "docProps/core.xml"), b"<cp:keywords>mleafdeck=d mleafrevision=r mleafpack=p</cp:keywords>");
        assert_eq!(part(&new, "word/media/image1.png"), [crate::STAND_IN, b"blob/ab"].concat());
        // Already new: borrowed, byte for byte.
        assert!(matches!(upgrade(&new), Cow::Borrowed(_)));
        assert!(matches!(upgrade(b"not a zip"), Cow::Borrowed(_)));
    }

    #[test]
    fn old_layer_json_reads_as_mleaf() {
        let old = br#"{"header": {"id": "a", "oxVersion": "ox_alpha", "dataVersion": "1", "author": "t", "timestamp": "2026"},
            "schemas": {"ox::note": {"value": {"dataType": "String"}}},
            "data": [{"path": "p", "attributes": {"ox::note": "hi", "word::style": "x"}, "provenance": {"ox::note": "a"}}]}"#;
        let l = crate::model::Layer::from_json(old).unwrap();
        assert_eq!(l.header.mleaf_version, "ox_alpha");
        assert!(l.schemas.contains_key("mleaf::note"));
        let n = &l.data[0];
        assert_eq!(n.attributes.get("mleaf::note").and_then(|v| v.as_str()), Some("hi"));
        assert!(n.attributes.contains_key("word::style") && !n.attributes.contains_key("ox::note"));
        assert_eq!(n.provenance.get("mleaf::note").map(String::as_str), Some("a"));
        let written = serde_json::to_string(&l).unwrap();
        assert!(written.contains("mleafVersion") && !written.contains("ox::") && !written.contains("oxVersion"));
    }
}
