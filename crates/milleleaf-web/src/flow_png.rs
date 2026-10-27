//! The picture package as Word writes it: the `a:blip` names a raster PNG part, and the SVG
//! rides along as the blip's own `asvg:svgBlip` extension. Readers without a raster decoder
//! (and Word itself) draw the raster; the vector stays in the file and stays authoritative
//! for comparison, the same structure the import side reads back.

use super::chart_svg;

type Result<T> = std::result::Result<T, String>;

const SVG_BLIP_URI: &str = "{96DAC541-7B7A-43D3-8B79-37D633B846F1}";
const IMAGE_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";

/// An XML attribute's value inside one opening tag.
fn attr(tag: &str, key: &str) -> Option<String> {
    let name = format!("{key}=\"");
    let at = tag.find(&name)? + name.len();
    let end = tag[at..].find('"')? + at;
    Some(tag[at..end].to_string())
}

/// A package path with `.` and `..` resolved, absolute from the package root.
fn normalize(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            p => out.push(p),
        }
    }
    out.join("/")
}

/// A relationship `Target` resolved against the source part's directory.
fn resolve_target(dir: &str, target: &str) -> String {
    if target.starts_with('/') {
        normalize(target)
    } else {
        normalize(&format!("{dir}/{target}"))
    }
}

/// A relationship `Target` for `part`, relative to the source part's directory `dir`.
fn relative_target(dir: &str, part: &str) -> String {
    let from: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    let to: Vec<&str> = part.split('/').filter(|s| !s.is_empty()).collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    format!(
        "{}{}",
        "../".repeat(from.len() - common),
        to[common..].join("/")
    )
}

/// Rewrites every `a:blip` that names an SVG part: the blip names a PNG part drawn from that
/// SVG, and the SVG's relationship moves into the blip's `asvg:svgBlip` extension. Blips that
/// already carry the extension are left as they are. The SVG parts themselves are unchanged.
pub(super) fn add_png_fallbacks(bytes: &[u8]) -> Result<Vec<u8>> {
    use std::collections::BTreeMap;
    use std::io::{Cursor, Read, Write};
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut names: Vec<String> = Vec::new();
    let mut parts: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for i in 0..archive.len() {
        let mut f = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = f.name().map_err(|e| e.to_string())?.into_owned();
        let mut data = Vec::new();
        f.read_to_end(&mut data).map_err(|e| e.to_string())?;
        names.push(name.clone());
        parts.insert(name, data);
    }
    // New media parts, XML rewrites per part and relationship additions per rels part.
    let mut added: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut xml_edits: BTreeMap<String, Vec<(usize, usize, String)>> = BTreeMap::new();
    let mut rel_edits: BTreeMap<String, Vec<String>> = BTreeMap::new();
    // One PNG part per SVG part, and one relationship to it per rels part.
    let mut png_of: BTreeMap<String, String> = BTreeMap::new();
    let mut rel_of: BTreeMap<(String, String), String> = BTreeMap::new();
    for name in &names {
        if !name.ends_with(".xml") || name.ends_with(".rels") {
            continue;
        }
        let Ok(text) = std::str::from_utf8(&parts[name]) else {
            continue;
        };
        if !text.contains("<a:blip") {
            continue;
        }
        let Some((dir, file)) = name.rsplit_once('/') else {
            continue;
        };
        let rels_name = format!("{dir}/_rels/{file}.rels");
        let Some(rels) = parts.get(&rels_name) else {
            continue;
        };
        let Ok(rels_text) = std::str::from_utf8(rels) else {
            continue;
        };
        let mut images: Vec<(String, String)> = Vec::new();
        let mut ids: Vec<String> = Vec::new();
        for tag in rels_text.split("<Relationship ").skip(1) {
            let Some(end) = tag.find('>') else { continue };
            let tag = &tag[..end];
            let (Some(id), Some(target)) = (attr(tag, "Id"), attr(tag, "Target")) else {
                continue;
            };
            ids.push(id.clone());
            if attr(tag, "Type").as_deref() == Some(IMAGE_REL) {
                images.push((id, resolve_target(dir, &target)));
            }
        }
        if !images
            .iter()
            .any(|(_, t)| t.to_ascii_lowercase().ends_with(".svg"))
        {
            continue;
        }
        let mut edits: Vec<(usize, usize, String)> = Vec::new();
        let mut at = 0;
        while let Some(i) = text[at..].find("<a:blip") {
            let start = at + i;
            let Some(gt) = text[start..].find('>') else {
                break;
            };
            let opening_end = start + gt + 1;
            let self_closing = text[start..opening_end].ends_with("/>");
            let Some(embed) = attr(&text[start..opening_end], "r:embed") else {
                at = opening_end;
                continue;
            };
            let Some((_, svg_path)) = images.iter().find(|(id, _)| *id == embed) else {
                at = opening_end;
                continue;
            };
            let svg_path = svg_path.as_str();
            if !svg_path.to_ascii_lowercase().ends_with(".svg") {
                at = opening_end;
                continue;
            }
            let (element_end, inner) = if self_closing {
                (opening_end, "")
            } else {
                let Some(close) = text[opening_end..].find("</a:blip>") else {
                    break;
                };
                (
                    opening_end + close + "</a:blip>".len(),
                    &text[opening_end..opening_end + close],
                )
            };
            at = element_end;
            if inner.contains("svgBlip") {
                continue;
            }
            let svg = parts
                .get(svg_path)
                .ok_or_else(|| format!("The report picture names a missing drawing: {svg_path}"))?;
            let svg = std::str::from_utf8(svg)
                .map_err(|_| format!("The report picture part isn't text: {svg_path}"))?;
            let png_path = match png_of.get(svg_path) {
                Some(path) => path.clone(),
                None => {
                    let png = chart_svg::chart_png(svg)?;
                    // The PNG part sits beside the SVG part, under the same name.
                    let stem = svg_path.strip_suffix(".svg").unwrap_or(svg_path);
                    let mut n = 0;
                    let path = loop {
                        let candidate = if n == 0 {
                            format!("{stem}.png")
                        } else {
                            format!("{stem}-{}.png", n + 1)
                        };
                        if !parts.contains_key(&candidate) && !added.contains_key(&candidate) {
                            break candidate;
                        }
                        n += 1;
                    };
                    added.insert(path.clone(), png);
                    png_of.insert(svg_path.to_string(), path.clone());
                    path
                }
            };
            let key = (rels_name.clone(), svg_path.to_string());
            let rel_id: String = rel_of
                .entry(key)
                .or_insert_with(|| {
                    let id = (1..)
                        .map(|n| format!("rId{n}"))
                        .find(|id| !ids.contains(id))
                        .unwrap_or_else(|| "rIdPicture".into());
                    ids.push(id.clone());
                    rel_edits
                        .entry(rels_name.clone())
                        .or_default()
                        .push(format!(
                            "<Relationship Id=\"{id}\" Type=\"{IMAGE_REL}\" Target=\"{}\"/>",
                            relative_target(dir, &png_path)
                        ));
                    id
                })
                .clone();
            let opening = text[start..opening_end].replace(
                &format!("r:embed=\"{embed}\""),
                &format!("r:embed=\"{rel_id}\""),
            );
            let extension = format!(
                "<a:extLst><a:ext uri=\"{SVG_BLIP_URI}\"><asvg:svgBlip xmlns:asvg=\"http://schemas.microsoft.com/office/drawing/2016/SVG/main\" r:embed=\"{embed}\"/></a:ext></a:extLst>"
            );
            let replacement = if self_closing {
                let head = opening.trim_end_matches("/>").trim_end();
                format!("{head}>{extension}</a:blip>")
            } else {
                format!("{opening}{inner}{extension}</a:blip>")
            };
            edits.push((start, element_end, replacement));
        }
        if !edits.is_empty() {
            xml_edits.insert(name.clone(), edits);
        }
    }
    if added.is_empty() {
        return Ok(bytes.to_vec());
    }
    let content_types = "[Content_Types].xml";
    let mut content = std::str::from_utf8(&parts[content_types])
        .map_err(|_| "The package's content types aren't text.".to_string())?
        .to_string();
    let original_len = content.len();
    if !content.contains("Extension=\"png\"")
        && let Some(types_at) = content.find("<Types")
        && let Some(gt) = content[types_at..].find('>')
    {
        let at = types_at + gt + 1;
        content.insert_str(at, "<Default Extension=\"png\" ContentType=\"image/png\"/>");
        xml_edits.insert(content_types.into(), vec![(0, original_len, content)]);
    }
    // The new relationships go in before each rels part's closing tag.
    for (rels_name, rels) in rel_edits {
        let text = std::str::from_utf8(&parts[&rels_name])
            .map_err(|_| format!("{rels_name} isn't text"))?;
        let close = text
            .rfind("</Relationships>")
            .ok_or_else(|| format!("{rels_name} has no closing Relationships tag"))?;
        xml_edits
            .entry(rels_name)
            .or_default()
            .push((close, close, rels.concat()));
    }
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for name in &names {
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let data = match xml_edits.remove(name) {
            Some(edits) => {
                let mut text = std::str::from_utf8(&parts[name])
                    .map_err(|_| format!("{name} isn't text"))?
                    .to_string();
                for (start, end, replacement) in edits.into_iter().rev() {
                    text.replace_range(start..end, &replacement);
                }
                text.into_bytes()
            }
            None => parts[name].clone(),
        };
        out.start_file(name, options).map_err(|e| e.to_string())?;
        out.write_all(&data).map_err(|e| e.to_string())?;
    }
    for (name, data) in &added {
        out.start_file(
            name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .map_err(|e| e.to_string())?;
        out.write_all(data).map_err(|e| e.to_string())?;
    }
    Ok(out.finish().map_err(|e| e.to_string())?.into_inner())
}
