//! A chart's own XML, kept from the file it was adopted from (plan M2.5, option B).
//!
//! GridCraft rebuilds every chart from its model on save, so fonts, per-point colours, axis
//! scales, manual layout, line dashes and label positions become its defaults (upstream
//! gridcraft#1668). Adopting a workbook therefore keeps each chart's part as a blob, with the
//! chart as GridCraft read it from that part: `grid::sheet.charts[i].xml = {blob, was}`. On
//! export, a chart still equal to `was` gets its original XML back in place of the part GridCraft
//! wrote; a changed one gets it patched where [`patch`] can make the change, else keeps
//! GridCraft's, and the export says so. An import keeps the editor's XML for the charts the editor
//! changed ([`keep_charts`]).
//!
//! `was` leaves out the anchor (the drawing part holds it, not the chart part), so a moved or
//! resized chart keeps its formatting. Excel and LibreOffice both redraw a chart from
//! its cells, not from the values cached in its XML (checked 18 Oct 2026 on the real workbook:
//! a wrong cached value came back as the cell's on a re-save by Excel 16 and LibreOffice 24.2).
//! A part that points at other parts (`r:id`: user shapes, an embedded workbook) isn't kept;
//! Excel's chart-style and colour parts (the style gallery's state) aren't either.

use std::collections::HashMap;
use std::io::{Cursor, Read, Write};

use gridcraft_core::CellRef;
use gridcraft_model::{Anchor, Chart};
use gridcraft_model::LegendPos;
use quick_xml::events::{BytesStart, BytesText, Event};
use serde_json::{Value, json};

/// Media type of a kept chart part.
pub const CHART_XML: &str = "application/vnd.openxmlformats-officedocument.drawingml.chart+xml";

/// A chart as its own part describes it: without its id and its place on the sheet.
pub(crate) fn unplaced(c: &Chart) -> Chart {
    Chart { id: 0, anchor: Anchor::default(), ..c.clone() }
}

fn zip_part(z: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str) -> Option<Vec<u8>> {
    let mut f = z.by_name(name).ok()?;
    let mut b = Vec::new();
    f.read_to_end(&mut b).ok()?;
    Some(b)
}

/// `target` of a relationship from `part`, as a part name.
fn resolve(part: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_string();
    }
    let mut out: Vec<&str> = part.split('/').collect();
    out.pop();
    for seg in target.split('/') {
        match seg {
            ".." => {
                out.pop();
            }
            "." | "" => {}
            s => out.push(s),
        }
    }
    out.join("/")
}

fn attr(e: &quick_xml::events::BytesStart, local: &[u8]) -> Option<String> {
    e.attributes().flatten().find(|a| a.key.local_name().as_ref() == local).map(|a| {
        let raw = String::from_utf8_lossy(&a.value).into_owned();
        quick_xml::escape::unescape(&raw).map(|s| s.into_owned()).unwrap_or(raw)
    })
}

/// A part's relationships: id → (type, target part name).
fn rels(z: &mut zip::ZipArchive<Cursor<&[u8]>>, part: &str) -> HashMap<String, (String, String)> {
    let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
    let name = if dir.is_empty() { format!("_rels/{file}.rels") } else { format!("{dir}/_rels/{file}.rels") };
    let mut out = HashMap::new();
    let Some(xml) = zip_part(z, &name) else { return out };
    let mut r = quick_xml::Reader::from_reader(xml.as_slice());
    loop {
        match r.read_event() {
            Ok(Event::Empty(e) | Event::Start(e)) if e.local_name().as_ref() == b"Relationship" => {
                if attr(&e, b"TargetMode").as_deref() == Some("External") {
                    continue;
                }
                if let (Some(id), Some(ty), Some(t)) = (attr(&e, b"Id"), attr(&e, b"Type"), attr(&e, b"Target")) {
                    out.insert(id, (ty, resolve(part, &t)));
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// The chart parts in a drawing, in order, each with its anchor's top-left cell (`None` for an
/// absolute anchor). In a markup-compatibility block only the first choice counts, as in GridCraft.
fn drawing_charts(xml: &[u8]) -> Vec<(Option<CellRef>, String)> {
    let mut r = quick_xml::Reader::from_reader(xml);
    let (mut out, mut cell, mut in_from, mut field, mut col, mut row) = (Vec::new(), None, false, None::<Vec<u8>>, 0u32, 0u32);
    let mut fallback = 0usize;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                b"twoCellAnchor" | b"oneCellAnchor" | b"absoluteAnchor" => cell = None,
                b"from" => in_from = true,
                b"col" | b"row" if in_from => field = Some(e.local_name().as_ref().to_vec()),
                b"Fallback" => fallback += 1,
                _ => {}
            },
            Ok(Event::Text(t)) if field.is_some() => {
                let n = t.decode().ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
                if field.as_deref() == Some(b"col") { col = n } else { row = n }
            }
            Ok(Event::End(e)) => match e.local_name().as_ref() {
                b"from" => {
                    in_from = false;
                    cell = Some(CellRef::new(row, col));
                }
                b"col" | b"row" => field = None,
                b"Fallback" => fallback = fallback.saturating_sub(1),
                _ => {}
            },
            Ok(Event::Empty(e)) if fallback == 0 && e.local_name().as_ref() == b"chart" => {
                if let Some(id) = attr(&e, b"id") {
                    out.push((cell, id));
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// Every chart part in an XLSX, by sheet name: (anchor cell, the part's bytes).
pub(crate) fn chart_parts(bytes: &[u8]) -> HashMap<String, Vec<(Option<CellRef>, Vec<u8>)>> {
    let mut out = HashMap::new();
    let Ok(mut z) = zip::ZipArchive::new(Cursor::new(bytes)) else { return out };
    let wb_rels = rels(&mut z, "xl/workbook.xml");
    let Some(wb_xml) = zip_part(&mut z, "xl/workbook.xml") else { return out };
    let mut sheets = Vec::new();
    let mut r = quick_xml::Reader::from_reader(wb_xml.as_slice());
    loop {
        match r.read_event() {
            Ok(Event::Empty(e) | Event::Start(e)) if e.local_name().as_ref() == b"sheet" => {
                if let (Some(name), Some(id)) = (attr(&e, b"name"), attr(&e, b"id")) {
                    sheets.push((name, id));
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    for (name, id) in sheets {
        let Some((_, sheet_part)) = wb_rels.get(&id) else { continue };
        let sheet_part = sheet_part.clone();
        let mut charts = Vec::new();
        for (ty, drawing) in rels(&mut z, &sheet_part).into_values() {
            if !ty.ends_with("/drawing") {
                continue;
            }
            let Some(xml) = zip_part(&mut z, &drawing) else { continue };
            let d_rels = rels(&mut z, &drawing);
            for (cell, rid) in drawing_charts(&xml) {
                if let Some(b) = d_rels.get(&rid).and_then(|(_, p)| zip_part(&mut z, p)) {
                    charts.push((cell, b));
                }
            }
        }
        out.insert(name, charts);
    }
    out
}

/// Adds `xml: {blob, was}` to each adopted chart whose part `parts` holds (matched by anchor cell),
/// storing the part in `pics`.
pub(crate) fn keep(charts: &mut [Value], parts: &[(Option<CellRef>, Vec<u8>)], sheet: &str, pics: &mut crate::Pictures, warnings: &mut Vec<String>) {
    for (i, c) in charts.iter_mut().enumerate() {
        if c.get("xml").is_some() {
            continue;
        }
        let Ok(ch) = serde_json::from_value::<Chart>(c.clone()) else { continue };
        let mut found = parts.iter().filter(|(cell, _)| *cell == Some(ch.anchor.cell));
        let (Some((_, xml)), None) = (found.next(), found.next()) else {
            warnings.push(format!("{sheet}: chart {} (at {}): its own XML couldn't be matched, so its formatting becomes GridCraft's on export", i + 1, ch.anchor.cell.a1()));
            continue;
        };
        if [&b"r:id=\""[..], b"r:embed=\""].iter().any(|p| xml.windows(p.len()).any(|w| w == *p)) {
            warnings.push(format!("{sheet}: chart {} (at {}) points at other parts (shapes on it or an embedded workbook), so its formatting becomes GridCraft's on export", i + 1, ch.anchor.cell.a1()));
            continue;
        }
        let blob = ox_core::blob_of(xml);
        pics.insert(blob.clone(), (CHART_XML.into(), xml.clone()));
        c["xml"] = json!({ "blob": blob, "was": unplaced(&ch) });
    }
}

/// Keeps the file's own XML for every chart in `layer` that has none yet: an adopted workbook's,
/// or the charts an editor changed (an import), so the editor's formatting comes back too.
pub fn keep_charts(layer: &mut ox_core::Layer, bytes: &[u8], pics: &mut crate::Pictures, warnings: &mut Vec<String>) {
    let mut parts = chart_parts(bytes);
    for node in &mut layer.data {
        let Some(gs) = node.attributes.get_mut("grid::sheet").and_then(Value::as_object_mut) else { continue };
        let sheet = gs.get("name").and_then(Value::as_str).unwrap_or("").to_string();
        if let Some(charts) = gs.get_mut("charts").and_then(Value::as_array_mut) {
            let found = parts.remove(&sheet).unwrap_or_default();
            keep(charts, &found, &sheet, pics, warnings);
        }
    }
}

/// The kept XML for an exported chart: its bytes when the chart is unchanged since its XML was
/// kept or the changes can be made in that XML ([`patch`]), else `None` with a warning.
pub(crate) fn original(c: &Value, ch: &Chart, assets: ox_core::PictureSource, at: &str, n: usize, warnings: &mut Vec<String>) -> Option<Vec<u8>> {
    let x = c.get("xml")?;
    if matches!(assets, ox_core::PictureSource::StandIn(_)) { return None; }
    let blob = x.get("blob").and_then(Value::as_str)?;
    let xml = match ox_core::picture_bytes(assets, blob) {
        Ok(b) => b,
        Err(e) => {
            warnings.push(format!("{at}: chart {n}'s own XML can't be read ({e}); GridCraft's is used"));
            return None;
        }
    };
    let Some(was) = x.get("was").and_then(|w| serde_json::from_value::<Chart>(w.clone()).ok()) else { return None };
    let now = unplaced(ch);
    if was == now {
        return Some(xml);
    }
    let patched = patch(&xml, &was, &now);
    if patched.is_none() {
        warnings.push(format!("{at}: chart {n} was changed in a way its own XML can't take (its kind, series count, colours, labels, gridlines, axis titles or trendlines), so GridCraft redraws it: its fonts, per-point colours, axis scales, manual layout, line dashes and label positions become GridCraft's defaults"));
    }
    patched
}

/// Makes the changes from `was` to `now` in a chart's own XML, keeping all its formatting: the
/// title's text (or removing it), the cells each series reads (name, categories, values, bubble
/// sizes), a literal series name, and the legend's position (or removing it). `None` when anything
/// else changed. Cached values are left as they were: Excel and LibreOffice redraw from the cells.
pub(crate) fn patch(xml: &[u8], was: &Chart, now: &Chart) -> Option<Vec<u8>> {
    fn rest(c: &Chart) -> impl PartialEq + '_ {
        (c.kind, c.style, &c.source, c.by_rows, &c.pivot, c.data_labels, c.gridlines, &c.x_title, &c.y_title)
    }
    if rest(was) != rest(now) || was.series.len() != now.series.len() {
        return None;
    }
    if was.title.is_none() && now.title.is_some() || was.legend == LegendPos::None && now.legend != LegendPos::None {
        return None;
    }
    let refs = |s: &Option<String>| s.as_deref().is_some_and(|n| n.contains('!'));
    for (a, b) in was.series.iter().zip(&now.series) {
        let same = (&a.color, a.secondary, a.kind, a.smooth, &a.trendlines) == (&b.color, b.secondary, b.kind, b.smooth, &b.trendlines);
        let names = a.name == b.name || a.name.is_some() && b.name.is_some() && refs(&a.name) == refs(&b.name);
        let cats = a.categories.is_some() == b.categories.is_some();
        let bubbles = a.bubble_sizes.is_some() == b.bubble_sizes.is_some();
        if !(same && names && cats && bubbles) {
            return None;
        }
    }

    let mut r = quick_xml::Reader::from_reader(xml);
    let mut w = quick_xml::Writer::new(Vec::with_capacity(xml.len()));
    let mut stack: Vec<Vec<u8>> = Vec::new();
    let (mut ser, mut skip, mut title_runs, mut series_seen) = (None::<usize>, 0usize, 0usize, 0usize);
    // Text to write in place of the current element's text, until its end.
    let mut replace: Option<String> = None;
    let pos = |l: LegendPos| match l {
        LegendPos::Bottom => "b",
        LegendPos::Top => "t",
        LegendPos::Left => "l",
        LegendPos::Right => "r",
        LegendPos::None => "",
    };
    loop {
        let ev = r.read_event().ok()?;
        if skip > 0 {
            match ev {
                Event::Start(_) => skip += 1,
                Event::End(_) => skip -= 1,
                Event::Eof => return None,
                _ => {}
            }
            continue;
        }
        match &ev {
            Event::Eof => break,
            Event::Start(e) | Event::Empty(e) => {
                let local = e.local_name().as_ref().to_vec();
                let under = |names: &[&[u8]]| stack.len() >= names.len() + 1 && stack[1..=names.len()].iter().zip(names).all(|(a, b)| a == b);
                let in_chart = stack.len() == 2 && stack[1] == b"chart";
                // Removed parts: the title, the legend.
                if in_chart && (local == b"title" && now.title.is_none() || local == b"legend" && now.legend == LegendPos::None) {
                    if matches!(ev, Event::Start(_)) {
                        skip = 1;
                    }
                    continue;
                }
                if in_chart && local == b"autoTitleDeleted" && now.title.is_none() {
                    let mut e2 = BytesStart::new(String::from_utf8_lossy(e.name().as_ref()).into_owned());
                    e2.push_attribute(("val", "1"));
                    w.write_event(Event::Empty(e2)).ok()?;
                    if matches!(ev, Event::Start(_)) {
                        skip = 1;
                    }
                    continue;
                }
                if under(&[b"chart", b"legend"]) && stack.len() == 3 && local == b"legendPos" && was.legend != now.legend {
                    let mut e2 = BytesStart::new(String::from_utf8_lossy(e.name().as_ref()).into_owned());
                    e2.push_attribute(("val", pos(now.legend)));
                    w.write_event(Event::Empty(e2)).ok()?;
                    if matches!(ev, Event::Start(_)) {
                        skip = 1;
                    }
                    continue;
                }
                if local == b"ser" && stack.len() == 4 && under(&[b"chart", b"plotArea"]) {
                    ser = Some(series_seen);
                    series_seen += 1;
                }
                if matches!(ev, Event::Start(_)) {
                    // The element whose text is replaced.
                    if local == b"t" && under(&[b"chart", b"title", b"tx", b"rich"]) && was.title != now.title {
                        replace = Some(if title_runs == 0 { now.title.clone().unwrap_or_default() } else { String::new() });
                        title_runs += 1;
                    }
                    if let Some(i) = ser.filter(|_| local == b"f" || local == b"v") {
                        let (a, b) = (&was.series[i], &now.series[i]);
                        let parent = stack.get(5).map(Vec::as_slice);
                        let text = match (parent, local.as_slice()) {
                            (Some(b"tx"), _) => (a.name != b.name).then(|| b.name.clone().unwrap_or_default()),
                            (Some(b"cat" | b"xVal"), b"f") => (a.categories != b.categories).then(|| b.categories.clone().unwrap_or_default()),
                            (Some(b"val" | b"yVal"), b"f") => (a.values != b.values).then(|| b.values.clone()),
                            (Some(b"bubbleSize"), b"f") => (a.bubble_sizes != b.bubble_sizes).then(|| b.bubble_sizes.clone().unwrap_or_default()),
                            _ => None,
                        };
                        // A name's `v` inside a cache isn't the literal name.
                        let cached = stack.iter().any(|n| n.ends_with(b"Cache"));
                        if !cached {
                            replace = text.or(replace.take());
                        }
                    }
                    stack.push(local);
                }
                w.write_event(ev.borrow()).ok()?;
            }
            Event::End(e) => {
                if let Some(t) = replace.take() {
                    w.write_event(Event::Text(BytesText::new(&t))).ok()?;
                }
                if e.local_name().as_ref() == b"ser" {
                    ser = None;
                }
                stack.pop();
                w.write_event(ev.borrow()).ok()?;
            }
            Event::Text(_) | Event::GeneralRef(_) | Event::CData(_) if replace.is_some() => {}
            _ => w.write_event(ev.borrow()).ok()?,
        }
    }
    let changed_title = was.title != now.title && now.title.is_some();
    if series_seen != was.series.len() || changed_title && title_runs == 0 {
        return None;
    }
    Some(w.into_inner())
}

/// Puts kept chart XML back into an XLSX GridCraft wrote: `kept[i]` is the `i`-th chart in sheet
/// order, which GridCraft writes as `xl/charts/chart{i+1}.xml` (`chartEx…` for the new chart
/// types). A kept part replaces only a part of the same family.
pub fn with_kept_charts(bytes: Vec<u8>, kept: &[Option<Vec<u8>>]) -> Result<Vec<u8>, crate::GridError> {
    if kept.iter().all(Option::is_none) {
        return Ok(bytes);
    }
    let err = |e: zip::result::ZipError| crate::GridError::Xlsx(e.to_string());
    let mut swap = HashMap::new();
    for (i, x) in kept.iter().enumerate() {
        let Some(x) = x else { continue };
        let needle = b"2014/chartex\"";
        let ex = x.windows(needle.len()).any(|w| w == needle);
        swap.insert(format!("xl/charts/chart{}{}.xml", if ex { "Ex" } else { "" }, i + 1), x);
    }
    let mut z = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).map_err(err)?;
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..z.len() {
        let f = z.by_index(i).map_err(err)?;
        match swap.get(f.name()) {
            Some(x) => {
                let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
                w.start_file(f.name(), opts).map_err(err)?;
                w.write_all(x).map_err(|e| crate::GridError::Xlsx(e.to_string()))?;
            }
            None => w.raw_copy_file(f).map_err(err)?,
        }
    }
    Ok(w.finish().map_err(err)?.into_inner())
}
