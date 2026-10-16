//! `ox`: compose, validate, inspect and render OX layer stacks.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ox_core::render::{deck_html, document_html, find};
use ox_core::{Composer, Level, validate};

mod repo;
use repo::{asset_root, baseline, load_any as load_stack, open_any as open};
use serde_json::json;

const USAGE: &str = "\
ox — Milleleaf layer tool

USAGE:
  Wherever <stack.ox.json> appears, a project folder with a .ox store works too (its head).

  ox init     <dir> [--from STACK.ox.json]        start a revision store (.ox/), optionally with
                                                  every layer of an existing stack committed
  ox commit   <dir> <LAYER.ox.json> [--expect HASH] [--rebase] [--sign [--key NAME]] [--seal-to A,B]
                                                  append a layer; refused if the head moved since
                                                  HASH (or the layer's header.base); --seal-to
                                                  encrypts it so only A and B can compose it
  ox head     <dir>                               the current stack hash
  ox log      <dir> [<node> [<attribute>]]        commits, newest first; or one node's history
  ox blame    <dir> <node> [<attribute>]          each value, the links it came through, and
                                                  every layer that set it
  ox checkpoint <dir> [--keep N] [--author NAME]  flatten all but the top N layers (default 1)
  ox verify   <dir> [--require]                   hash and signatures of every layer in the head
  ox purge    <dir> <layer> [--reason TEXT] [--key NAME] [--key-destroy]
                 delete a layer's content, keep its hash; signed by an owner or approver;
                 --key-destroy (sealed layers): destroy its content key in every copy instead
  ox key role <dir> <name> <owner|approver|editor|viewer>
  ox sign     <dir> <layer-hash> [--key NAME]     add a detached signature
  ox key new  <name> [<dir>]                      new Ed25519 + X-Wing key in ~/.config/ox/keys
                                                  (or $OX_KEYS); with <dir>, the project knows it
  ox key add  <dir> <name> <ed25519:…> [<xwing:…>]  the project knows someone else's key

  ox layers   <stack.ox.json>                     list the stack, bottom layer first
  ox validate <stack.ox.json>                     check schemas, references and composition
  ox compose  <stack.ox.json> [--provenance] [--out FILE]
                                                  print the composed tree as JSON
  ox get      <stack.ox.json> <path>              one node, inherits expanded, with provenance
  ox render   <stack.ox.json> <path> --out FILE   HTML view of a deck::deck or word::document node
  ox adopt    <FILE.docx|.xlsx|.pptx> --out LAYER.ox.json [--report FILE] [--path ID]
                 bring in a file Milleleaf didn't make; lists what isn't carried
  ox docx-export <stack.ox.json> <document-path> --out FILE.docx
                                                  a word::document as DOCX (via WordCraft)
  ox docx-import <stack.ox.json> <document-path> <FILE.docx> --out EDIT.ox.json
                 [--author NAME] [--id LAYER-ID]  edits made in Word → an edit layer to stack
                 [--report FILE]                 (every import: the report as JSON, with how each
                                                  node was matched: certain or best guess)
  ox xlsx-export <stack.ox.json> <workbook-path> --out FILE.xlsx
                                                  a grid::workbook as XLSX (via GridCraft, recalculated)
  ox xlsx-import <stack.ox.json> <workbook-path> <FILE.xlsx> --out EDIT.ox.json
                 [--author NAME] [--id LAYER-ID]  edits made in Excel → a recalculated edit layer
  ox pptx-export <stack.ox.json> <deck-path> --out FILE.pptx
                                                  a deck::deck as PPTX (via DeckCraft)
  ox pptx-import <stack.ox.json> <deck-path> <FILE.pptx> --out EDIT.ox.json
                 [--author NAME] [--id LAYER-ID]  edits made in PowerPoint → an edit layer
  ox pack <stack.ox.json> <pack-path> --out FILE.pdf
                                                  every part of an office::pack as one PDF

Other formats: give the export commands another --out extension (.odt, .md, .rtf, .tex, .txt,
.html for documents; .ods; .odp) and the import commands a .odt/.ods/.odp file. Each format goes
by the best route on this machine; imports only by routes that keep Milleleaf's markers.
  ox formats [--json]                             every route, what it keeps, and whether it's
                                                  ready here (and why not)
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("ox: {e}");
            ExitCode::from(2)
        }
    }
}

fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn opt_str<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn opt(args: &[String], name: &str) -> Option<PathBuf> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
}

fn positional(args: &[String]) -> Vec<&String> {
    let mut out = Vec::new();
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
            continue;
        }
        if matches!(
            a.as_str(),
            "--out" | "--author" | "--id" | "--expect" | "--key" | "--keep" | "--from" | "--report"
        ) {
            skip = true;
            continue;
        }
        if !a.starts_with("--") {
            out.push(a);
        }
    }
    out
}

fn emit(text: &str, out: Option<&Path>) -> Result<(), String> {
    match out {
        Some(p) => fs::write(p, text).map_err(|e| format!("{}: {e}", p.display())),
        None => {
            println!("{text}");
            Ok(())
        }
    }
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    let pos = positional(args);
    let Some(cmd) = pos.first().map(|s| s.as_str()) else {
        print!("{USAGE}");
        return Ok(ExitCode::from(2));
    };
    if cmd == "--help" || cmd == "help" || flag(args, "--help") {
        print!("{USAGE}");
        return Ok(ExitCode::SUCCESS);
    }
    if cmd == "formats" {
        return formats(args);
    }
    let file = pos
        .get(1)
        .map(PathBuf::from)
        .ok_or_else(|| format!("`{cmd}` needs a stack file\n\n{USAGE}"))?;
    let out = opt(args, "--out");
    if let Some(r) = repo::run(cmd, &file, &pos, args) {
        return r;
    }
    match cmd {
        "adopt" => {
            // A file Milleleaf didn't make: read it into a new layer, and say what isn't carried.
            let out = out.ok_or("`adopt` needs --out LAYER.ox.json")?;
            let bytes = fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            let sha = ox_store::hash_of(&bytes);
            let ext = file
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("adopted");
            let root = opt_str(args, "--path")
                .map(String::from)
                .unwrap_or_else(|| ox_core::model::stable_id(&format!("adopt|{sha}")));
            let header = ox_core::Header {
                id: opt_str(args, "--id")
                    .map(String::from)
                    .unwrap_or_else(|| format!("adopt/{stem}")),
                ox_version: String::new(),
                data_version: "1.0.0".into(),
                author: opt_str(args, "--author").unwrap_or("adopt").into(),
                timestamp: today(),
                kind: Some("adopt".into()),
                ..Default::default()
            };
            let (mut layer, warnings, summary) = match ext.as_str() {
                "docx" => {
                    let doc = ox_wordcraft::from_docx(&bytes).map_err(|e| e.to_string())?;
                    let (l, r) = ox_wordcraft::adopt(&doc, &root, stem, header).map_err(|e| e.to_string())?;
                    let n = r.added.len();
                    (l, r.warnings, format!("{n} paragraphs and tables"))
                }
                "pptx" => {
                    let p = ox_deckcraft::from_pptx(&bytes).map_err(|e| e.to_string())?;
                    let (l, r) = ox_deckcraft::adopt(&p, &root, stem, header).map_err(|e| e.to_string())?;
                    let n = r.added.len();
                    (l, r.warnings, format!("{n} slides and frames"))
                }
                "xlsx" => {
                    let wb = ox_gridcraft::from_xlsx(&bytes).map_err(|e| e.to_string())?;
                    let (l, r) = ox_gridcraft::adopt(&wb, &root, stem, header).map_err(|e| e.to_string())?;
                    let summary = format!("{} sheets, {} cells, {} formulas", r.sheets, r.cells, r.formulas);
                    (l, r.warnings, summary)
                }
                _ => return Err(format!("`adopt` reads .docx, .xlsx or .pptx, not `{ext}`")),
            };
            let src = serde_json::json!({ "format": ext, "file": file.file_name().and_then(|n| n.to_str()).unwrap_or(""), "sha256": sha });
            if let Some(n) = layer.data.iter_mut().find(|n| n.path == root) {
                n.attributes.insert("ox::source".into(), src);
            }
            let text = serde_json::to_string_pretty(&layer).unwrap_or_default();
            fs::write(&out, text + "\n").map_err(|e| format!("{}: {e}", out.display()))?;
            for w in &warnings {
                eprintln!("not carried: {w}");
            }
            if let Some(r) = opt(args, "--report") {
                let rep = serde_json::json!({ "root": root, "summary": summary, "not_carried": warnings });
                fs::write(&r, serde_json::to_string_pretty(&rep).unwrap_or_default() + "\n")
                    .map_err(|e| format!("{}: {e}", r.display()))?;
            }
            println!("adopted {} as {root}: {summary}; {} not carried", file.display(), warnings.len());
            Ok(ExitCode::SUCCESS)
        }
        "layers" => {
            for l in load_stack(&file).map_err(|e| e.to_string())? {
                let h = &l.layer.header;
                println!(
                    "{:<10} {:<48} {} nodes  ({})",
                    h.kind.as_deref().unwrap_or("-"),
                    h.id,
                    l.layer.data.len(),
                    l.source.display()
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        "validate" => {
            let flat = open(&file).map_err(|e| e.to_string())?;
            let issues = validate(&flat).map_err(|e| e.to_string())?;
            let errors = issues.iter().filter(|i| i.level == Level::Error).count();
            for i in &issues {
                let lvl = if i.level == Level::Error {
                    "error"
                } else {
                    "warning"
                };
                let attr = i
                    .attribute
                    .as_deref()
                    .map(|a| format!(" {a}"))
                    .unwrap_or_default();
                println!("{lvl}: {}{attr}: {}", i.path, i.message);
            }
            println!(
                "{} layers, {} nodes, {} schemas: {errors} errors, {} warnings",
                flat.layers.len(),
                flat.nodes.len(),
                flat.schemas.len(),
                issues.len() - errors
            );
            Ok(if errors > 0 {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            })
        }
        "compose" => {
            let flat = open(&file).map_err(|e| e.to_string())?;
            let mut c = Composer::new(&flat);
            let mut trees = c.compose_all().map_err(|e| e.to_string())?;
            if !flag(args, "--provenance") {
                fn strip(n: &mut ox_core::Composed) {
                    n.provenance.clear();
                    n.children.iter_mut().for_each(strip);
                }
                trees.iter_mut().for_each(strip);
            }
            for w in &c.warnings {
                eprintln!("warning: {}: {}", w.path, w.message);
            }
            let v = json!({"layers": flat.layers, "roots": trees});
            emit(
                &serde_json::to_string_pretty(&v).unwrap_or_default(),
                out.as_deref(),
            )?;
            Ok(ExitCode::SUCCESS)
        }
        "get" => {
            let path = pos.get(2).ok_or("`get` needs a node path")?;
            let flat = open(&file).map_err(|e| e.to_string())?;
            if !flat.nodes.contains_key(path.as_str()) {
                return Err(format!("no layer defines `{path}`"));
            }
            let e = Composer::new(&flat)
                .effective(path)
                .map_err(|e| e.to_string())?;
            emit(
                &serde_json::to_string_pretty(&e).unwrap_or_default(),
                out.as_deref(),
            )?;
            Ok(ExitCode::SUCCESS)
        }
        "render" => {
            let path = pos.get(2).ok_or("`render` needs a node path")?;
            let flat = open(&file).map_err(|e| e.to_string())?;
            let mut c = Composer::new(&flat);
            let trees = c.compose_all().map_err(|e| e.to_string())?;
            let node = find(&trees, path)
                .ok_or_else(|| format!("`{path}` is not in the composed tree"))?;
            let html = if node.attributes.contains_key("deck::deck") {
                deck_html(node)
            } else if node.attributes.contains_key("word::document") {
                document_html(node)
            } else {
                return Err(format!(
                    "`{path}` is neither a deck::deck nor a word::document"
                ));
            };
            emit(&html, out.as_deref())?;
            Ok(ExitCode::SUCCESS)
        }
        "docx-export" => {
            let path = pos
                .get(2)
                .ok_or("`docx-export` needs a document node path")?;
            let out = out.ok_or("`docx-export` needs --out FILE.docx")?;
            let flat = open(&file).map_err(|e| e.to_string())?;
            let trees = Composer::new(&flat)
                .compose_all()
                .map_err(|e| e.to_string())?;
            let node = find(&trees, path)
                .ok_or_else(|| format!("`{path}` is not in the composed tree"))?;
            let (doc, report) =
                ox_wordcraft::export(node, &flat.layers).map_err(|e| e.to_string())?;
            let bytes = ox_wordcraft::to_docx(&doc).map_err(|e| e.to_string())?;
            let bytes = to_target(ox_formats::Kind::Document, bytes, &out)?;
            fs::write(&out, bytes).map_err(|e| format!("{}: {e}", out.display()))?;
            for w in &report.warnings {
                eprintln!("warning: {w}");
            }
            eprintln!(
                "ox: {} paragraphs, {} tables, {} linked fields → {}",
                report.paragraphs,
                report.tables,
                report.fields,
                out.display()
            );
            Ok(ExitCode::SUCCESS)
        }
        "docx-import" => {
            let path = pos
                .get(2)
                .ok_or("`docx-import` needs a document node path")?;
            let docx = pos
                .get(3)
                .map(PathBuf::from)
                .ok_or("`docx-import` needs the edited .docx")?;
            let out = out.ok_or("`docx-import` needs --out EDIT.ox.json")?;
            let bytes = from_source(
                ox_formats::Kind::Document,
                fs::read(&docx).map_err(|e| format!("{}: {e}", docx.display()))?,
                &docx,
            )?;
            let doc = ox_wordcraft::from_docx(&bytes).map_err(|e| e.to_string())?;
            let (base, node) = baseline(
                &file,
                path,
                ox_wordcraft::exported_revision(&doc).as_deref(),
                ox_wordcraft::revision_of,
            )?;
            let stem = docx
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("document");
            let header = ox_core::Header {
                id: opt_str(args, "--id")
                    .map(String::from)
                    .unwrap_or_else(|| format!("edit/{stem}")),
                ox_version: String::new(),
                data_version: "1.0.0".into(),
                author: opt_str(args, "--author").unwrap_or("word").into(),
                timestamp: today(),
                kind: Some("edit".into()),
                base,
                ..Default::default()
            };
            let (layer, report) =
                ox_wordcraft::import(&node, &doc, header).map_err(|e| e.to_string())?;
            let text = serde_json::to_string_pretty(&layer).unwrap_or_default();
            fs::write(&out, text + "\n").map_err(|e| format!("{}: {e}", out.display()))?;
            for w in &report.warnings {
                eprintln!("warning: {w}");
            }
            show_matches(&report.matches, &report, args)?;
            for c in &report.conflicts {
                eprintln!("conflict: {}: {}", c.path, c.message);
            }
            eprintln!(
                "ox: {} changed, {} added, {} removed, {} conflicts → {}",
                report.changed.len(),
                report.added.len(),
                report.removed.len(),
                report.conflicts.len(),
                out.display()
            );
            Ok(ExitCode::SUCCESS)
        }
        "xlsx-export" => {
            let path = pos
                .get(2)
                .ok_or("`xlsx-export` needs a workbook node path")?;
            let out = out.ok_or("`xlsx-export` needs --out FILE.xlsx")?;
            let flat = open(&file).map_err(|e| e.to_string())?;
            let trees = Composer::new(&flat)
                .compose_all()
                .map_err(|e| e.to_string())?;
            let node = find(&trees, path)
                .ok_or_else(|| format!("`{path}` is not in the composed tree"))?;
            let (wb, report) = ox_gridcraft::export(node).map_err(|e| e.to_string())?;
            let bytes = ox_gridcraft::to_xlsx(&wb).map_err(|e| e.to_string())?;
            let bytes = to_target(ox_formats::Kind::Workbook, bytes, &out)?;
            fs::write(&out, bytes).map_err(|e| format!("{}: {e}", out.display()))?;
            for w in &report.warnings {
                eprintln!("warning: {w}");
            }
            for (p, stored, computed) in &report.stale_values {
                eprintln!(
                    "warning: {p}: the model stores {stored} but its formula gives {computed}"
                );
            }
            eprintln!(
                "ox: {} sheets, {} tables, {} values ({} formulas) → {}",
                report.sheets,
                report.tables,
                report.values,
                report.formulas,
                out.display()
            );
            Ok(ExitCode::SUCCESS)
        }
        "xlsx-import" => {
            let path = pos
                .get(2)
                .ok_or("`xlsx-import` needs a workbook node path")?;
            let xlsx = pos
                .get(3)
                .map(PathBuf::from)
                .ok_or("`xlsx-import` needs the edited .xlsx")?;
            let out = out.ok_or("`xlsx-import` needs --out EDIT.ox.json")?;
            let bytes = from_source(
                ox_formats::Kind::Workbook,
                fs::read(&xlsx).map_err(|e| format!("{}: {e}", xlsx.display()))?,
                &xlsx,
            )?;
            let mut wb = ox_gridcraft::from_xlsx(&bytes).map_err(|e| e.to_string())?;
            let (base, node) = baseline(
                &file,
                path,
                ox_gridcraft::exported_revision(&wb).as_deref(),
                ox_gridcraft::revision_of,
            )?;
            let stem = xlsx
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("workbook");
            let header = ox_core::Header {
                id: opt_str(args, "--id")
                    .map(String::from)
                    .unwrap_or_else(|| format!("edit/{stem}")),
                ox_version: String::new(),
                data_version: "1.0.0".into(),
                author: opt_str(args, "--author").unwrap_or("excel").into(),
                timestamp: today(),
                kind: Some("edit".into()),
                base,
                ..Default::default()
            };
            let (layer, report) =
                ox_gridcraft::import(&node, &mut wb, header).map_err(|e| e.to_string())?;
            let text = serde_json::to_string_pretty(&layer).unwrap_or_default();
            fs::write(&out, text + "\n").map_err(|e| format!("{}: {e}", out.display()))?;
            for w in &report.warnings {
                eprintln!("warning: {w}");
            }
            for p in &report.missing {
                eprintln!("warning: {p}: its named range is gone from the file; left unchanged");
            }
            show_matches(&report.matches, &report, args)?;
            for p in &report.conflicts {
                eprintln!("conflict: {p}: linked data edited in the workbook; not written back");
            }
            eprintln!(
                "ox: {} changed ({} recomputed by GridCraft), {} moved, {} sheets renamed, {} conflicts → {}",
                report.changed.len(),
                report.recomputed.len(),
                report.moved.len(),
                report.renamed.len(),
                report.conflicts.len(),
                out.display()
            );
            Ok(ExitCode::SUCCESS)
        }
        "pptx-export" => {
            let path = pos.get(2).ok_or("`pptx-export` needs a deck node path")?;
            let out = out.ok_or("`pptx-export` needs --out FILE.pptx")?;
            let flat = open(&file).map_err(|e| e.to_string())?;
            let trees = Composer::new(&flat)
                .compose_all()
                .map_err(|e| e.to_string())?;
            let node = find(&trees, path)
                .ok_or_else(|| format!("`{path}` is not in the composed tree"))?;
            let assets = asset_root(&file);
            let (p, report) = ox_deckcraft::export(node, &assets).map_err(|e| e.to_string())?;
            let bytes = ox_deckcraft::to_pptx(&p).map_err(|e| e.to_string())?;
            let bytes = to_target(ox_formats::Kind::Deck, bytes, &out)?;
            fs::write(&out, bytes).map_err(|e| format!("{}: {e}", out.display()))?;
            for w in &report.warnings {
                eprintln!("warning: {w}");
            }
            eprintln!(
                "ox: {} slides, {} frames ({} tables, {} charts, {} linked fields) → {}",
                report.slides,
                report.frames,
                report.tables,
                report.charts,
                report.fields,
                out.display()
            );
            Ok(ExitCode::SUCCESS)
        }
        "pptx-import" => {
            let path = pos.get(2).ok_or("`pptx-import` needs a deck node path")?;
            let pptx = pos
                .get(3)
                .map(PathBuf::from)
                .ok_or("`pptx-import` needs the edited .pptx")?;
            let out = out.ok_or("`pptx-import` needs --out EDIT.ox.json")?;
            let bytes = from_source(
                ox_formats::Kind::Deck,
                fs::read(&pptx).map_err(|e| format!("{}: {e}", pptx.display()))?,
                &pptx,
            )?;
            let p = ox_deckcraft::from_pptx(&bytes).map_err(|e| e.to_string())?;
            let (base, node) = baseline(
                &file,
                path,
                ox_deckcraft::exported_revision(&p).as_deref(),
                ox_deckcraft::revision_of,
            )?;
            let stem = pptx.file_stem().and_then(|s| s.to_str()).unwrap_or("deck");
            let header = ox_core::Header {
                id: opt_str(args, "--id")
                    .map(String::from)
                    .unwrap_or_else(|| format!("edit/{stem}")),
                ox_version: String::new(),
                data_version: "1.0.0".into(),
                author: opt_str(args, "--author").unwrap_or("powerpoint").into(),
                timestamp: today(),
                kind: Some("edit".into()),
                base,
                ..Default::default()
            };
            let (layer, report) =
                ox_deckcraft::import(&node, &p, header).map_err(|e| e.to_string())?;
            let text = serde_json::to_string_pretty(&layer).unwrap_or_default();
            fs::write(&out, text + "\n").map_err(|e| format!("{}: {e}", out.display()))?;
            for w in &report.warnings {
                eprintln!("warning: {w}");
            }
            show_matches(&report.matches, &report, args)?;
            for c in &report.conflicts {
                eprintln!("conflict: {}: {}", c.path, c.message);
            }
            eprintln!(
                "ox: {} moved, {} changed, {} added, {} removed, {} reordered, {} conflicts → {}",
                report.moved.len(),
                report.changed.len(),
                report.added.len(),
                report.removed.len(),
                report.reordered.len(),
                report.conflicts.len(),
                out.display()
            );
            Ok(ExitCode::SUCCESS)
        }
        "pack" => {
            let path = pos.get(2).ok_or("`pack` needs an office::pack node path")?;
            let out = out.ok_or("`pack` needs --out FILE.pdf")?;
            let stack = load_stack(&file).map_err(|e| e.to_string())?;
            let layers: Vec<ox_pack::LayerInfo> = stack
                .iter()
                .map(|l| ox_pack::LayerInfo {
                    id: l.layer.header.id.clone(),
                    kind: l.layer.header.kind.clone().unwrap_or_default(),
                    author: l.layer.header.author.clone(),
                    timestamp: l.layer.header.timestamp.clone(),
                })
                .collect();
            let flat = ox_core::compose::flatten(&stack);
            let trees = Composer::new(&flat)
                .compose_all()
                .map_err(|e| e.to_string())?;
            let assets = asset_root(&file);
            let (pdf, report) =
                ox_pack::build(&trees, path, &layers, &assets).map_err(|e| e.to_string())?;
            fs::write(&out, pdf).map_err(|e| format!("{}: {e}", out.display()))?;
            for part in &report.parts {
                for w in &part.warnings {
                    eprintln!("warning: {}: {w}", part.title);
                }
                eprintln!(
                    "  {:<20} {:<9} pages {}–{}",
                    part.title,
                    part.kind,
                    part.first_page + 1,
                    part.first_page + part.pages
                );
            }
            eprintln!(
                "ox: “{}”, revision {}, {} pages → {}",
                report.title,
                report.revision,
                report.pages,
                out.display()
            );
            Ok(ExitCode::SUCCESS)
        }
        other => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    }
}

/// Today's date (UTC) as YYYY-MM-DD, without a date crate.
fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Prints how sure the import was about each match, and writes the whole report with
/// `--report FILE`.
fn show_matches(
    matches: &[ox_core::Match],
    report: &impl serde::Serialize,
    args: &[String],
) -> Result<(), String> {
    let guesses = ox_core::matching::best_guesses(matches);
    eprintln!(
        "ox: {} matches: {} certain, {} best guess(es){}",
        matches.len(),
        matches.len() - guesses.len(),
        guesses.len(),
        if guesses.is_empty() {
            ""
        } else {
            " — flagged `ox::match` in the edit layer for review"
        }
    );
    for g in guesses {
        eprintln!("best guess: {}: {}", g.path, g.by);
    }
    if let Some(p) = opt(args, "--report") {
        let text = serde_json::to_string_pretty(report).unwrap_or_default();
        fs::write(&p, text + "\n").map_err(|e| format!("{}: {e}", p.display()))?;
    }
    Ok(())
}

fn ext(p: &Path) -> String {
    p.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// OOXML from an adapter → the file `out` asks for, by the best route on this machine
/// (`ox formats`).
fn to_target(kind: ox_formats::Kind, bytes: Vec<u8>, out: &Path) -> Result<Vec<u8>, String> {
    let (bytes, choice, warnings) =
        ox_formats::export(kind, &ext(out), bytes).map_err(|e| e.to_string())?;
    if choice.route.needs != ox_formats::Needs::Nothing || !choice.route.keeps_identity {
        eprintln!("ox: writing .{} via {}", ext(out), choice.route.via);
    }
    for w in warnings {
        eprintln!("warning: {w}");
    }
    Ok(bytes)
}

/// A file to import → the OOXML the importers read, by the best route that keeps identity.
fn from_source(kind: ox_formats::Kind, bytes: Vec<u8>, path: &Path) -> Result<Vec<u8>, String> {
    let (bytes, choice) = ox_formats::import(kind, &ext(path), bytes).map_err(|e| e.to_string())?;
    if choice.route.needs != ox_formats::Needs::Nothing {
        eprintln!("ox: reading .{} via {}", ext(path), choice.route.via);
    }
    Ok(bytes)
}

/// `ox formats`: every route, whether it's ready here, and which one each format uses.
fn formats(args: &[String]) -> Result<ExitCode, String> {
    let all = ox_formats::routes();
    if flag(args, "--json") {
        let rows: Vec<serde_json::Value> = all
            .iter()
            .map(|r| json!({ "route": r, "status": ox_formats::status(r) }))
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&rows).unwrap_or_default()
        );
        return Ok(ExitCode::SUCCESS);
    }
    println!(
        "{:<5} {:<9} {:<7} {:<44} {:<8} STATUS",
        "FILE", "KIND", "WAY", "ROUTE", "IDENTITY"
    );
    let mut last = (
        String::new(),
        ox_formats::Kind::Document,
        ox_formats::Dir::Export,
    );
    let mut used = false;
    for r in &all {
        let key = (r.ext.to_string(), r.kind, r.dir);
        if key != last {
            last = key;
            used = false;
        }
        let st = ox_formats::status(r);
        let text = match &st {
            ox_formats::Status::Ready { .. } if !used => {
                used = true;
                "ready — used".to_string()
            }
            ox_formats::Status::Ready { .. } => "ready (fallback)".to_string(),
            ox_formats::Status::Unavailable { why } => format!("unavailable: {why}"),
        };
        let via = match r.command {
            Some(c) if !r.via.starts_with(c) => format!("{} [{c}]", r.via),
            _ => r.via.to_string(),
        };
        println!(
            "{:<5} {:<9} {:<7} {:<44} {:<8} {text}",
            r.ext,
            r.kind.name(),
            if r.dir == ox_formats::Dir::Export {
                "export"
            } else {
                "import"
            },
            via,
            if r.keeps_identity { "kept" } else { "lost" },
        );
    }
    println!();
    match ox_formats::libreoffice() {
        Ok(v) => println!("LibreOffice: {v}"),
        Err(why) => println!(
            "LibreOffice: {why}. ODT, ODS and ODP can't be imported, and only .odt can be exported (without identity)."
        ),
    }
    match ox_formats::html2ox() {
        Ok(v) => println!("html2ox: {v}"),
        Err(why) => println!("html2ox: {why}"),
    }
    Ok(ExitCode::SUCCESS)
}
