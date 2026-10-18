//! Revision-store commands (`ox init`, `commit`, `log`, `blame`, `checkpoint`, `verify`, `sign`,
//! `timestamp`, `key`) and the helpers that let every other command take a project folder.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ox_core::compose::Flat;
use ox_core::render::find;
use ox_core::{Composed, Composer};
use ox_core::model::Layer;
use ox_core::stack::LoadedLayer;
use ox_store::{headers, ots};
use ox_store::policy::{HeaderSource, Service};
use ox_store::{CommitOptions, KeyPair, LayerStatus, SigInfo, Store, StoreError};

/// Layers of a stack file, or of a project folder's head.
pub fn load_any(file: &Path) -> Result<Vec<LoadedLayer>, String> {
    match Store::find(file) {
        Some(s) => {
            let head = s.head().map_err(e)?.ok_or("nothing committed yet")?;
            s.load(&head).map_err(e)
        }
        None => ox_core::load_stack(file).map_err(e),
    }
}

/// A stack file flattened, or a project folder's head (from its cache when possible).
pub fn open_any(file: &Path) -> Result<Flat, String> {
    match Store::find(file) {
        Some(s) => {
            let head = s.head().map_err(e)?.ok_or("nothing committed yet")?;
            s.flat(&head).map_err(e)
        }
        None => ox_core::open(file).map_err(e),
    }
}

/// What an edited file is compared with: the composed `path` node at the head the file was
/// exported from, and that head as the edit layer's base. That head is the newest stack whose
/// composition of `path` has the file's revision (`rev`). Comparing with the current head
/// instead would turn every value changed since the export into an "edit" that reverts it.
/// Without a store, or when no stack matches, the current composition is used.
pub fn baseline(
    file: &Path,
    path: &str,
    rev: Option<&str>,
    revision_of: impl Fn(&Composed) -> String,
) -> Result<(Option<String>, Composed), String> {
    let current = |flat: &Flat| -> Result<Composed, String> {
        let trees = Composer::new(flat).compose_all().map_err(e)?;
        find(&trees, path)
            .cloned()
            .ok_or_else(|| format!("`{path}` is not in the composed tree"))
    };
    let Some(s) = Store::find(file) else {
        return Ok((None, current(&open_any(file)?)?));
    };
    let head = s.head().map_err(e)?;
    if let Some(rev) = rev {
        // ponytail: composes each past stack newest first; index revisions per node if
        // histories get long.
        for (back, info) in s.log(None).map_err(e)?.into_iter().enumerate() {
            let trees = Composer::new(&s.flat(&info.hash).map_err(e)?)
                .compose_all()
                .map_err(e)?;
            if let Some(n) = find(&trees, path).filter(|n| revision_of(n) == rev) {
                if back > 0 {
                    eprintln!(
                        "note: this file was exported at {} ({back} commit(s) before the head); \
                         the edit layer is based there, so `ox commit` will ask for --rebase",
                        short(&info.hash)
                    );
                }
                return Ok((Some(info.hash), n.clone()));
            }
        }
        eprintln!(
            "warning: the head this file was exported from isn't in this project's history; \
             it's compared with the current head, so values changed since its export can come \
             back as edits. Review the layer before committing."
        );
    }
    let head_flat = match &head {
        Some(h) => s.flat(h).map_err(e)?,
        None => open_any(file)?,
    };
    Ok((head, current(&head_flat)?))
}

/// Where a stack's assets (`{"blob": …}` paths) are found: a project's root folder, or the
/// folder of a stack file.
pub fn asset_root(file: &Path) -> PathBuf {
    match Store::find(file) {
        Some(s) => s.root,
        None => file.parent().map(Path::to_path_buf).unwrap_or_default(),
    }
}

/// Puts an import's pictures where `ox commit` finds them. One the project already holds stays a
/// `sha256-…` value; a new one is written beside the layer in `<name>.assets/<hex>.<ext>` and
/// the layer names it by that path, so commit stores it by hash. Returns how many were written.
pub fn place_pictures(
    layer: &mut ox_core::Layer,
    pictures: &std::collections::BTreeMap<String, (String, Vec<u8>)>,
    out: &Path,
    root: &Path,
) -> Result<usize, String> {
    let name = out.file_name().and_then(|n| n.to_str()).unwrap_or("layer");
    let dir = format!("{}.assets", name.trim_end_matches(".json").trim_end_matches(".ox"));
    // Only the pictures the layer names: an unchanged one the import saw isn't in it.
    let named = serde_json::to_string(&layer.data).map_err(e)?;
    let mut paths = std::collections::HashMap::new();
    for (blob, (media_type, bytes)) in pictures {
        let hex = blob.trim_start_matches("sha256-");
        if !named.contains(blob.as_str()) || root.join(ox_core::BLOB_DIR).join(hex).exists() {
            continue;
        }
        let ext = ox_core::ext_for(media_type);
        let rel = format!("{dir}/{hex}.{ext}");
        let p = out.parent().unwrap_or(Path::new("")).join(&rel);
        std::fs::create_dir_all(p.parent().unwrap_or(Path::new(""))).map_err(e)?;
        std::fs::write(&p, bytes).map_err(|x| format!("{}: {x}", p.display()))?;
        paths.insert(blob.as_str(), rel);
    }
    fn walk(v: &mut serde_json::Value, paths: &std::collections::HashMap<&str, String>) {
        match v {
            serde_json::Value::Object(m) => {
                if let Some(rel) = m.get("blob").and_then(|b| b.as_str()).and_then(|b| paths.get(b)) {
                    m.insert("blob".into(), rel.clone().into());
                }
                m.values_mut().for_each(|x| walk(x, paths));
            }
            serde_json::Value::Array(a) => a.iter_mut().for_each(|x| walk(x, paths)),
            _ => {}
        }
    }
    if !paths.is_empty() {
        for n in &mut layer.data {
            n.attributes.values_mut().for_each(|v| walk(v, &paths));
        }
    }
    Ok(paths.len())
}

fn e(x: impl std::fmt::Display) -> String {
    x.to_string()
}

fn opt<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn has(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn short(h: &str) -> &str {
    &h[..12.min(h.len())]
}

fn key_for(args: &[String]) -> Result<KeyPair, String> {
    let name = opt(args, "--key")
        .map(String::from)
        .or_else(|| std::env::var("OX_KEY").ok())
        .unwrap_or_else(|| "default".into());
    let k = KeyPair::load_named(&name)
        .map_err(|err| format!("{err}\n(make one with `ox key new {name}`, or pass --key NAME)"))?;
    if k.mldsa_public().is_none() {
        eprintln!("warning: key {name} has no post-quantum keys, so it signs with Ed25519 only (`ox key upgrade {name}`)");
    }
    Ok(k)
}

pub fn run(
    cmd: &str,
    file: &Path,
    pos: &[&String],
    args: &[String],
) -> Option<Result<ExitCode, String>> {
    let r = match cmd {
        "init" => init(file, args),
        "commit" => commit(file, pos, args),
        "head" => Store::open(file).map_err(e).and_then(|s| {
            println!("{}", s.head().map_err(e)?.ok_or("nothing committed yet")?);
            Ok(ExitCode::SUCCESS)
        }),
        "log" => log(file, pos),
        "blame" => blame(file, pos),
        "checkpoint" => checkpoint(file, args),
        "verify" => verify(file, args),
        "sign" => sign(file, pos, args),
        "timestamp" => timestamp(file, pos, args),
        "purge" => purge(file, pos, args),
        "search" => search(file, pos, args),
        "key" => key(pos),
        _ => return None,
    };
    Some(r)
}

fn init(dir: &Path, args: &[String]) -> Result<ExitCode, String> {
    std::fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    let s = Store::init(dir).map_err(e)?;
    println!("ox: started a revision store in {}", s.dir.display());
    if let Some(from) = opt(args, "--from") {
        let commits = s.import_stack(&PathBuf::from(from)).map_err(e)?;
        for c in &commits {
            let h = s.index(&c.layer).map_err(e)?.header;
            println!("  {}  {}", short(&c.layer), h.id);
        }
        let blobs = std::fs::read_dir(s.dir.join("blobs")).map(|d| d.count()).unwrap_or(0);
        println!(
            "ox: {} layers committed from {from}; head {}{}",
            commits.len(),
            short(&s.head().map_err(e)?.unwrap_or_default()),
            if blobs > 0 { format!("; {blobs} asset(s) stored by hash") } else { String::new() }
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn commit(dir: &Path, pos: &[&String], args: &[String]) -> Result<ExitCode, String> {
    let s = Store::open(dir).map_err(e)?;
    let file = pos.get(2).ok_or("`commit` needs a layer file")?;
    let bytes = std::fs::read(file.as_str()).map_err(|err| format!("{file}: {err}"))?;
    let layer = Layer::from_json(&bytes).map_err(|err| format!("{file}: {err}"))?;
    let layer_dir = Path::new(file.as_str()).parent().map(Path::to_path_buf);
    let key = if has(args, "--sign") {
        Some(key_for(args)?)
    } else {
        None
    };
    let seal_to: Vec<String> = opt(args, "--seal-to")
        .map(|v| v.split(',').map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect())
        .unwrap_or_default();
    let opts = CommitOptions {
        seal_to: &seal_to,
        expect: opt(args, "--expect")
            .map(|h| s.resolve(h))
            .transpose()
            .map_err(e)?,
        rebase: has(args, "--rebase"),
        author: opt(args, "--author").map(String::from),
        sign: key.as_ref(),
        archive: has(args, "--archive"),
        // A layer's pictures are found beside the layer file, then at the project root.
        assets: layer_dir.as_deref(),
    };
    match s.commit(&layer, opts) {
        Ok(c) => {
            if c.dropped_imports > 0 {
                eprintln!(
                    "note: dropped {} import(s); in the store, the stack decides what is beneath a layer",
                    c.dropped_imports
                );
            }
            if !c.rebased_over.is_empty() {
                eprintln!("rebased over {} layer(s)", c.rebased_over.len());
            }
            println!(
                "ox: committed {} ({}){}{} → head {}",
                layer.header.id,
                short(&c.layer),
                if c.signed { ", signed" } else { "" },
                if seal_to.is_empty() { String::new() } else { format!(", sealed for {}", seal_to.join(", ")) },
                short(&c.stack)
            );
            Ok(ExitCode::SUCCESS)
        }
        Err(err @ (StoreError::Stale { .. } | StoreError::RebaseConflict { .. })) => {
            eprintln!("ox: {err}");
            Ok(ExitCode::from(3))
        }
        Err(err) => Err(e(err)),
    }
}

/// Exit 1 when nothing matched, as grep does.
fn search(dir: &Path, pos: &[&String], args: &[String]) -> Result<ExitCode, String> {
    let s = Store::open(dir).map_err(e)?;
    let query = pos.iter().skip(2).map(|p| p.as_str()).collect::<Vec<_>>().join(" ");
    if query.trim().is_empty() {
        return Err("`search` needs words to look for".into());
    }
    let found = if has(args, "--history") { s.search_history(&query) } else { s.search_head(&query) }.map_err(e)?;
    if has(args, "--json") {
        println!("{}", serde_json::to_string_pretty(&found).unwrap_or_default());
    } else {
        for h in &found.hits {
            let at = match (&h.hash, &h.at) {
                (Some(hash), Some(at)) => format!("{}  {at}  ", short(hash)),
                _ => String::new(),
            };
            println!("{at}{}  {}  {}\n    {}", h.path, h.attribute, h.layer, h.text);
        }
        for l in &found.locked {
            eprintln!("note: layer {} is sealed and no key here opens it; not searched", short(l));
        }
        eprintln!("ox: {} match(es)", found.hits.len());
    }
    Ok(if found.hits.is_empty() { ExitCode::from(1) } else { ExitCode::SUCCESS })
}

fn log(dir: &Path, pos: &[&String]) -> Result<ExitCode, String> {
    let s = Store::open(dir).map_err(e)?;
    if let Some(node) = pos.get(2) {
        let key = pos.get(3).map(|k| k.as_str());
        for t in s.history(node, key).map_err(e)? {
            println!(
                "{}  {:<24} {:<12} {:<44} {}{}",
                short(&t.layer),
                t.timestamp,
                t.author,
                t.id,
                t.keys.join(", "),
                t.in_checkpoint
                    .map(|c| format!("  [in checkpoint {}]", short(&c)))
                    .unwrap_or_default()
            );
        }
        return Ok(ExitCode::SUCCESS);
    }
    for st in s.log(None).map_err(e)? {
        // A checkpoint starts a new chain: checkpoint layer + the layers it kept.
        let what = if st.base.is_none() && st.added.len() > 1 {
            let first = s.index(&st.added[0]).map_err(e)?.header;
            format!("{} + {} layer(s) kept", first.id, st.added.len() - 1)
        } else {
            st.added
                .iter()
                .map(|h| s.index(h).map(|i| i.header.id))
                .collect::<Result<Vec<_>, _>>()
                .map_err(e)?
                .join(", ")
        };
        println!(
            "{}  {:<24} {:<12} {}",
            short(&st.hash),
            st.header.timestamp,
            st.header.author,
            what
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn blame(dir: &Path, pos: &[&String]) -> Result<ExitCode, String> {
    let s = Store::open(dir).map_err(e)?;
    let node = pos.get(2).ok_or("`blame` needs a node path")?;
    let entries = s.blame(node, pos.get(3).map(|k| k.as_str())).map_err(e)?;
    for b in entries {
        let value = serde_json::to_string(&b.value).unwrap_or_default();
        let value = if value.len() > 80 {
            format!("{}…", &value[..value.floor_char_boundary(79)])
        } else {
            value
        };
        println!("{} = {value}", b.attribute);
        if !b.via.is_empty() {
            println!("  via {}", b.via.join(" → "));
        }
        if let Some(d) = &b.derived {
            println!(
                "  derived: {} of {}",
                d.get("op").and_then(|v| v.as_str()).unwrap_or("?"),
                b.origin
            );
        }
        let chain: Vec<String> = b
            .history
            .iter()
            .map(|t| format!("{} ({}, {})", t.id, t.author, t.timestamp))
            .collect();
        println!("  set by: {}", chain.join(" → "));
    }
    Ok(ExitCode::SUCCESS)
}

fn checkpoint(dir: &Path, args: &[String]) -> Result<ExitCode, String> {
    let s = Store::open(dir).map_err(e)?;
    let keep = opt(args, "--keep")
        .map(|k| {
            k.parse::<usize>()
                .map_err(|_| format!("--keep {k}: not a number"))
        })
        .transpose()?
        .unwrap_or(1);
    let author = opt(args, "--author").unwrap_or("ox");
    match s.checkpoint(keep, author).map_err(e)? {
        Some(c) => {
            let replaced = s.index(&c.layer).map_err(e)?.header.replaces.len();
            println!(
                "ox: {replaced} layers → checkpoint {}; head {}",
                short(&c.layer),
                short(&c.stack)
            );
        }
        None => println!("ox: nothing to checkpoint"),
    }
    Ok(ExitCode::SUCCESS)
}

fn verify(dir: &Path, args: &[String]) -> Result<ExitCode, String> {
    let s = Store::open(dir).map_err(e)?;
    let require = has(args, "--require");
    let policy = if require { Some(s.policy().map_err(e)?) } else { None };
    let now = ox_store::now_rfc3339();
    let pq_listed = s.pq_listed();
    let (mut bad, mut refused, mut untrusted) = (0, 0, 0);
    // OpenTimestamps Bitcoin attestations: offline, only named; --online, checked against a header.
    let online = has(args, "--online").then(|| s.policy().map(|p| p.timestamps.unwrap_or_default().headers())).transpose().map_err(e)?;
    let mut checked: std::collections::HashMap<(u64, Vec<u8>), String> = std::collections::HashMap::new();
    let mut btc_bad = 0;
    let mut btc = |height: u64, root: &[u8]| -> String {
        let Some(sources) = &online else { return format!("in Bitcoin block {height} (not checked here)") };
        checked
            .entry((height, root.to_vec()))
            .or_insert_with(|| match bitcoin_header(sources, height, &headers::CHECKPOINT)
                .and_then(|(h, from)| Ok((ots::check_header(&h, root)?, from)))
            {
                Ok((t, from)) => format!("existed by {} (Bitcoin block {height}, {from})", ox_store::rfc3339(i64::from(t))),
                Err(x) => {
                    btc_bad += 1;
                    format!("NOT CONFIRMED in Bitcoin block {height}: {x}")
                }
            })
            .clone()
    };
    for h in s.head_layers().map_err(e)? {
        let id = s.index(&h).map(|i| i.header.id).unwrap_or_default();
        let status = s.verify(&h).map_err(e)?;
        let text = match &status {
            LayerStatus::Tampered { actual } => {
                bad += 1;
                format!("TAMPERED (content hashes to {})", short(actual))
            }
            LayerStatus::Invalid { signatures } | LayerStatus::Signed { signatures } => {
                if matches!(status, LayerStatus::Invalid { .. }) {
                    bad += 1;
                } else if !signatures.iter().any(|g| g.valid && g.known_as.is_some()) {
                    // Valid, but only by keys the project doesn't list: anyone can make a key.
                    untrusted += 1;
                }
                describe(signatures, &mut btc)
            }
            LayerStatus::Unsigned => "unsigned".into(),
            LayerStatus::Purged { mode, by, at, reason } => format!(
                "purged{} by {by} on {at}{}",
                if mode == "key" { " (key destroyed)" } else { "" },
                reason.as_ref().map(|r| format!(" ({r})")).unwrap_or_default()
            ),
        };
        let text = match s.sealed_info(&h).map_err(e)? {
            Some(info) if !matches!(status, LayerStatus::Purged { .. }) => {
                if let Some(p) = &info.problem {
                    bad += 1;
                    println!("BLOB  sealed layer {}: {p}", short(&h));
                }
                let who = if info.recipients.is_empty() { "nobody (no wraps here)".to_string() } else { info.recipients.join(", ") };
                format!("{text}; sealed for {who}")
            }
            _ => text,
        };
        println!("{}  {:<44} {text}", short(&h), id);
        if let Some(policy) = &policy {
            let sigs: &[SigInfo] = match &status {
                LayerStatus::Signed { signatures } | LayerStatus::Invalid { signatures } => signatures,
                LayerStatus::Unsigned => &[],
                _ => continue,
            };
            let v = policy.check(sigs, &now, &pq_listed);
            for w in &v.warnings {
                println!("              warning: {w}");
            }
            for r in &v.refused {
                println!("              REFUSED by policy: {r}");
            }
            refused += usize::from(!v.refused.is_empty());
        }
    }
    for problem in s.verify_blobs().map_err(e)? {
        bad += 1;
        println!("BLOB  {problem}");
    }
    if untrusted > 0 && !require {
        eprintln!(
            "note: {untrusted} layer(s) signed only by keys this project doesn't list \
             (`ox key add` to trust one); `--require` refuses them"
        );
    }
    if require && refused > 0 {
        eprintln!("ox: {refused} layer(s) don't meet the signature policy ({})", if s.dir.join("policy.json").exists() { ".ox/policy.json" } else { "the built-in default" });
    }
    if btc_bad > 0 {
        eprintln!("ox: {btc_bad} OpenTimestamps Bitcoin attestation(s) couldn't be confirmed");
    }
    Ok(if bad > 0 || refused > 0 || btc_bad > 0 { ExitCode::from(1) } else { ExitCode::SUCCESS })
}

/// "signed by sam (hybrid+slh-dsa, timestamped …, OpenTimestamps pending at …); INVALID mldsa65 signature (…)".
fn describe(sigs: &[SigInfo], btc: &mut impl FnMut(u64, &[u8]) -> String) -> String {
    let who = |g: &SigInfo| match &g.known_as {
        Some(n) => n.clone(),
        None => format!("unknown {} key {} (says {})", if g.alg.is_empty() { "?" } else { &g.alg }, g.key_id, g.claimed),
    };
    let mut by: Vec<(String, Vec<&SigInfo>)> = Vec::new();
    for g in sigs.iter().filter(|g| g.valid) {
        let w = who(g);
        match by.iter_mut().find(|(n, _)| *n == w) {
            Some((_, v)) => v.push(g),
            None => by.push((w, vec![g])),
        }
    }
    let mut out: Vec<String> = by
        .iter()
        .map(|(w, gs)| {
            let algs: Vec<&str> = gs.iter().map(|g| g.alg.as_str()).collect();
            let stamp = gs.iter().filter_map(|g| g.timestamp.as_deref()).min();
            let mut proofs: Vec<String> = Vec::new();
            for o in gs.iter().filter_map(|g| g.ots.as_ref()) {
                let t = if !o.bitcoin.is_empty() {
                    o.bitcoin.iter().map(|(h, root)| btc(*h, root)).collect::<Vec<_>>().join(", ")
                } else if !o.pending.is_empty() {
                    format!("pending at {}", o.pending.join(", "))
                } else {
                    "no attestation this ox knows".into()
                };
                if !proofs.contains(&t) {
                    proofs.push(t);
                }
            }
            format!(
                "signed by {w} ({}{}{})",
                ox_store::strength(&algs),
                stamp.map(|t| format!(", timestamped {t}, TSA signature not checked")).unwrap_or_default(),
                if proofs.is_empty() { String::new() } else { format!(", OpenTimestamps {}", proofs.join("; ")) }
            )
        })
        .collect();
    out.extend(sigs.iter().filter(|g| !g.valid).map(|g| {
        format!(
            "INVALID {} signature ({}){}",
            if g.alg.is_empty() { "?" } else { &g.alg },
            who(g),
            g.problem.as_ref().map(|p| format!(": {p}")).unwrap_or_default()
        )
    }));
    out.join("; ")
}

/// Time-stamps every signature file of a layer with each service the project chose (or the
/// command line names): RFC 3161 tokens as `<file>.tsr` (`.2.tsr`, …), OpenTimestamps as
/// `<file>.ots`. `--upgrade` asks the chosen calendars to complete pending OpenTimestamps proofs.
fn timestamp(dir: &Path, pos: &[&String], args: &[String]) -> Result<ExitCode, String> {
    let s = Store::open(dir).map_err(e)?;
    let h = s.resolve(pos.get(2).ok_or("`timestamp` needs a layer hash")?).map_err(e)?;
    let mut services: Vec<Service> = opts(args, "--tsa").map(|u| Service::Rfc3161 { url: u.into() }).collect();
    let flags: Vec<String> = opts(args, "--ots").map(String::from).collect();
    if !flags.is_empty() {
        services.push(Service::Opentimestamps { calendars: flags });
    }
    if services.is_empty() {
        services = s.policy().map_err(e)?.timestamps.unwrap_or_default().all();
    }
    let calendars: Vec<String> = services
        .iter()
        .flat_map(|x| match x {
            Service::Opentimestamps { calendars } => calendars.clone(),
            Service::Rfc3161 { .. } => vec![],
        })
        .collect();
    let files = s.signature_files(&h);
    if files.is_empty() {
        return Err(format!("{} is unsigned; sign it first", short(&h)));
    }
    if has(args, "--upgrade") {
        if calendars.is_empty() {
            return Err("no OpenTimestamps calendars chosen: pass --ots CALENDAR or list them in .ox/policy.json; \
                        only calendars you chose are asked, never one named in a proof file"
                .into());
        }
        for f in &files {
            ots_upgrade(f, &calendars)?;
        }
        return Ok(ExitCode::SUCCESS);
    }
    if services.is_empty() {
        return Err("no time-stamp service chosen (there is no default): pass --tsa URL or --ots CALENDAR, or list them \
                    in .ox/policy.json, e.g. {\"timestamps\": {\"services\": [{\"kind\": \"rfc3161\", \"url\": \"http://…\"}, \
                    {\"kind\": \"opentimestamps\", \"calendars\": [\"https://…\"]}]}}"
            .into());
    }
    for f in &files {
        let data = std::fs::read(f).map_err(|x| format!("{}: {x}", f.display()))?;
        let mut n = 0;
        for svc in &services {
            if let Service::Rfc3161 { url } = svc {
                n += 1;
                rfc3161(url, f, &data, n)?;
            }
        }
        if !calendars.is_empty() {
            ots_stamp(f, &data, &calendars)?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn opts<'a>(args: &'a [String], name: &'a str) -> impl Iterator<Item = &'a str> {
    args.windows(2).filter(move |w| w[0] == name).map(|w| w[1].as_str())
}

fn name_of(f: &Path) -> String {
    f.file_name().unwrap_or_default().to_string_lossy().into_owned()
}

fn rfc3161(url: &str, f: &Path, data: &[u8], n: usize) -> Result<(), String> {
    let (req, nonce) = ox_store::tsp::request(data).map_err(e)?;
    let resp = ok200(url, http("POST", url, &[("Content-Type", "application/timestamp-query")], &req)?)?;
    let stamp = ox_store::tsp::check(&resp, data).map_err(|x| format!("{url}: {x}"))?;
    let ours: Vec<u8> = nonce.iter().copied().skip_while(|b| *b == 0).collect();
    if stamp.nonce.as_deref() != Some(ours.as_slice()) {
        return Err(format!("{url}: the response doesn't carry our nonce (a replayed or unrelated token)"));
    }
    let t = ox_store::tsr_of(f, n);
    std::fs::write(&t, &resp).map_err(|x| format!("{}: {x}", t.display()))?;
    println!("ox: timestamped {} at {} → {}", name_of(f), stamp.at, t.display());
    Ok(())
}

const OTS_ACCEPT: (&str, &str) = ("Accept", "application/vnd.opentimestamps.v1");

/// Sends the file's commitment (its digest with a random nonce, hashed: calendars learn nothing
/// about it) to each calendar; joins their answers into `<file>.ots`, beside any proof already there.
fn ots_stamp(f: &Path, data: &[u8], calendars: &[String]) -> Result<(), String> {
    let nonce = ots::nonce()?;
    let c = ots::commitment(data, &nonce);
    let mut answers = Vec::new();
    for cal in calendars {
        let url = format!("{}/digest", cal.trim_end_matches('/'));
        match http("POST", &url, &[OTS_ACCEPT], &c).and_then(|r| ok200(&url, r)).and_then(|b| ots::Node::parse(&b)) {
            Ok(n) => answers.push(n),
            Err(x) => eprintln!("warning: OpenTimestamps calendar {cal}: {x}"),
        }
    }
    if answers.is_empty() {
        return Err(format!("{}: no OpenTimestamps calendar answered", name_of(f)));
    }
    let n = answers.len();
    let mut proof = ots::Proof::stamped(data, &nonce, answers);
    let p = ox_store::ots_of(f);
    if let Ok(old) = std::fs::read(&p) {
        let old = ots::Proof::parse(&old).map_err(|x| format!("{}: {x}", p.display()))?;
        if old.digest == proof.digest {
            proof.root.0.splice(0..0, old.root.0);
        }
    }
    write_proof(&p, &proof)?;
    println!(
        "ox: stamped {} at {n} OpenTimestamps calendar(s); pending until they reach Bitcoin (hours: `ox timestamp <dir> <layer> --upgrade`) → {}",
        name_of(f),
        p.display()
    );
    Ok(())
}

fn write_proof(p: &Path, proof: &ots::Proof) -> Result<(), String> {
    let bytes = proof.to_bytes();
    ots::Proof::parse(&bytes).map_err(|x| format!("{}: the new proof wouldn't read back: {x}", p.display()))?;
    std::fs::write(p, bytes).map_err(|x| format!("{}: {x}", p.display()))
}

/// Asks the chosen calendars (never a URL read from the proof: it is untrusted input) to complete
/// each pending attestation, and rewrites `<file>.ots` with what they return.
fn ots_upgrade(f: &Path, calendars: &[String]) -> Result<(), String> {
    let p = ox_store::ots_of(f);
    let Ok(bytes) = std::fs::read(&p) else { return Ok(()) };
    let data = std::fs::read(f).map_err(|x| format!("{}: {x}", f.display()))?;
    ots::summarize(&data, &bytes).map_err(|x| format!("{}: {x}", p.display()))?;
    let mut proof = ots::Proof::parse(&bytes)?;
    let mut unlisted = Vec::new();
    let done = proof.upgrade(|uri, msg| {
        let Some(cal) = calendars.iter().find(|c| c.trim_end_matches('/') == uri.trim_end_matches('/')) else {
            unlisted.push(uri.chars().filter(|c| !c.is_control()).collect::<String>());
            return None;
        };
        let url = format!("{}/timestamp/{}", cal.trim_end_matches('/'), ots::hex(msg));
        match http("GET", &url, &[OTS_ACCEPT], &[]) {
            Ok((200, b)) => ots::Node::parse(&b).map_err(|x| eprintln!("warning: {cal}: {x}")).ok(),
            Ok((404, _)) => None, // not in Bitcoin yet
            Ok((st, _)) => {
                eprintln!("warning: {cal}: HTTP {st}");
                None
            }
            Err(x) => {
                eprintln!("warning: {x}");
                None
            }
        }
    })?;
    if done > 0 {
        write_proof(&p, &proof)?;
    }
    for u in &unlisted {
        eprintln!("note: {} names calendar {u}, which you didn't choose; not asked", p.display());
    }
    let s = ots::summarize(&data, &proof.to_bytes())?;
    let blocks: Vec<String> = s.bitcoin.iter().map(|(h, _)| h.to_string()).collect();
    println!(
        "ox: {}: {done} attestation(s) completed; {}{}",
        name_of(f),
        if blocks.is_empty() { String::new() } else { format!("in Bitcoin block {}; ", blocks.join(", ")) },
        if s.pending.is_empty() { "none pending".to_string() } else { format!("pending at {}", s.pending.join(", ")) }
    );
    Ok(())
}

fn ok200(url: &str, (status, body): (u16, Vec<u8>)) -> Result<Vec<u8>, String> {
    if status == 200 { Ok(body) } else { Err(format!("{url}: HTTP {status}")) }
}

/// One HTTP/1.0 request; returns the status and body. `https://` goes through BoringSSL (already
/// in `ox` for sealed layers); trust as in `trust`.
pub(crate) fn http(method: &str, url: &str, headers: &[(&str, &str)], body: &[u8]) -> Result<(u16, Vec<u8>), String> {
    use std::io::{Read, Write};
    let (tls, rest) = match (url.strip_prefix("https://"), url.strip_prefix("http://")) {
        (Some(r), _) => (true, r),
        (None, Some(r)) => (false, r),
        _ => return Err(format!("{url}: only http:// and https:// URLs are supported")),
    };
    let (host, path) = match rest.split_once('/') {
        Some((h, p)) => (h, format!("/{p}")),
        None => (rest, "/".to_string()),
    };
    let name = host.rsplit_once(':').map_or(host, |(n, _)| n);
    let addr = if host.contains(':') { host.to_string() } else { format!("{host}:{}", if tls { 443 } else { 80 }) };
    let err = |x: &dyn std::fmt::Display| format!("{url}: {x}");
    let c = std::net::TcpStream::connect(&addr).map_err(|x| err(&x))?;
    for t in [c.set_read_timeout(Some(std::time::Duration::from_secs(30))), c.set_write_timeout(Some(std::time::Duration::from_secs(30)))] {
        t.map_err(|x| err(&x))?;
    }
    let mut req = format!("{method} {path} HTTP/1.0\r\nHost: {host}\r\nUser-Agent: ox\r\nConnection: close\r\nContent-Length: {}\r\n", body.len());
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    let mut r = Vec::new();
    fn exchange(mut s: impl Read + Write, req: &[u8], body: &[u8], r: &mut Vec<u8>) -> std::io::Result<()> {
        s.write_all(req)?;
        s.write_all(body)?;
        s.flush()?;
        s.read_to_end(r).map(|_| ())
    }
    if tls {
        let cert_file = std::env::var_os("SSL_CERT_FILE").map(PathBuf::from);
        let system = !cfg!(windows) && (Path::new("/etc/ssl/cert.pem").is_file() || Path::new("/etc/ssl/certs").is_dir());
        let s = trust(cert_file.as_deref(), system).map_err(|x| err(&x))?.connect(name, c).map_err(|x| err(&format!("TLS: {x}")))?;
        exchange(s, req.as_bytes(), body, &mut r)
    } else {
        exchange(c, req.as_bytes(), body, &mut r)
    }
    .map_err(|x| err(&x))?;
    let split = r.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| err(&"not an HTTP response"))?;
    let head = String::from_utf8_lossy(&r[..split]);
    let line = head.lines().next().unwrap_or_default();
    let status = line.split_whitespace().nth(1).and_then(|x| x.parse().ok()).ok_or_else(|| err(&line))?;
    Ok((status, r[split + 4..].to_vec()))
}

/// Who `https://` trusts: the certificates in `cert_file` (`SSL_CERT_FILE`) alone when given; else
/// the system's (`system`: BoringSSL's default paths under /etc/ssl exist); else (Windows, or no
/// system store) the Mozilla root set bundled in `webpki-root-certs` (CDLA-Permissive-2.0).
/// Chain and host name checks are BoringSSL's in every case.
fn trust(cert_file: Option<&Path>, system: bool) -> Result<boring::ssl::SslConnector, String> {
    use boring::x509::{X509, store::X509StoreBuilder};
    let mut b = boring::ssl::SslConnector::builder(boring::ssl::SslMethod::tls()).map_err(|x| x.to_string())?;
    let certs = match cert_file {
        Some(f) => X509::stack_from_pem(&std::fs::read(f).map_err(|x| format!("SSL_CERT_FILE {}: {x}", f.display()))?)
            .map_err(|x| format!("SSL_CERT_FILE {}: {x}", f.display()))?,
        None if system => return Ok(b.build()),
        None => webpki_root_certs::TLS_SERVER_ROOT_CERTS.iter().map(|c| X509::from_der(c)).collect::<Result<_, _>>().map_err(|x| x.to_string())?,
    };
    let mut store = X509StoreBuilder::new().map_err(|x| x.to_string())?;
    for c in certs {
        store.add_cert(c).map_err(|x| x.to_string())?;
    }
    b.set_cert_store(store.build());
    Ok(b.build())
}

/// Headers the light client's file holds, checked once per file per run.
fn light_chain(path: &Path, cp: &headers::Checkpoint) -> Result<std::rc::Rc<(Vec<u8>, headers::Chain)>, String> {
    thread_local!(static CHAINS: std::cell::RefCell<HashMap<PathBuf, std::rc::Rc<(Vec<u8>, headers::Chain)>>> = Default::default());
    if let Some(c) = CHAINS.with(|m| m.borrow().get(path).cloned()) {
        return Ok(c);
    }
    let bytes = std::fs::read(path).map_err(|x| format!("light client: no header file at {} ({x}): run `ox-headers sync`", path.display()))?;
    let chain = headers::check(&bytes, Some(cp)).map_err(|x| format!("light client: {}: {x}", path.display()))?;
    let c = std::rc::Rc::new((bytes, chain));
    CHAINS.with(|m| m.borrow_mut().insert(path.to_path_buf(), c.clone()));
    Ok(c)
}

/// An 80-byte block header that every source agrees on, and where it came from ("header from …").
/// `cp`: the checkpoint a light client's chain must contain (`headers::CHECKPOINT`).
pub(crate) fn bitcoin_header(sources: &[HeaderSource], height: u64, cp: &headers::Checkpoint) -> Result<(Vec<u8>, String), String> {
    let mut got: Vec<(String, Vec<u8>)> = Vec::new();
    let mut light = None;
    for src in sources {
        match src {
            HeaderSource::Explorers { urls } => {
                for u in urls {
                    let b = u.trim_end_matches('/');
                    let text = |url: String| -> Result<String, String> {
                        Ok(String::from_utf8_lossy(&ok200(&url, http("GET", &url, &[], &[])?)?).trim().to_string())
                    };
                    let hash = text(format!("{b}/block-height/{height}"))?;
                    let header = ots::unhex(&text(format!("{b}/block/{hash}/header"))?).map_err(|x| format!("{b}: header: {x}"))?;
                    if ots::block_hash(&header) != hash {
                        return Err(format!("{b}: the header it gave doesn't hash to the block it named"));
                    }
                    got.push((b.to_string(), header));
                }
            }
            HeaderSource::Bitcoind { url, cookie_file } => {
                let hash = rpc(url, cookie_file, "getblockhash", serde_json::json!([height]))?;
                let hash = hash.as_str().ok_or_else(|| format!("{url}: getblockhash: not a hash"))?.to_string();
                let header = rpc(url, cookie_file, "getblockheader", serde_json::json!([hash, false]))?;
                let header = ots::unhex(header.as_str().unwrap_or_default()).map_err(|x| format!("{url}: header: {x}"))?;
                if ots::block_hash(&header) != hash {
                    return Err(format!("{url}: the header it gave doesn't hash to the block it named"));
                }
                got.push((url.clone(), header));
            }
            HeaderSource::LightClient { file } => {
                let path = file.as_ref().map(PathBuf::from).or_else(headers::default_file).ok_or("light client: no app-data directory; give the source a `file`")?;
                let c = light_chain(&path, cp)?;
                if height > c.1.height {
                    return Err(format!("light client: {} reaches block {}, not {height} yet: run `ox-headers sync`", path.display(), c.1.height));
                }
                let at = height as usize * 80;
                light = Some(c.1.height);
                got.push((format!("the light client ({})", path.display()), c.0[at..at + 80].to_vec()));
            }
        }
    }
    let Some((first, header)) = got.first() else { return Err("no Bitcoin header sources chosen".into()) };
    if let Some((other, _)) = got.iter().find(|(_, h)| h != header) {
        return Err(format!("header sources disagree at block {height}: {first} and {other} gave different headers"));
    }
    let others = got.len() - usize::from(light.is_some());
    let s = |n: usize| if n == 1 { "" } else { "s" };
    let from = match light {
        Some(tip) if others == 0 => format!("header from the light client (synced to {tip})"),
        Some(tip) => format!("header from the light client (synced to {tip}) and {others} other source{}", s(others)),
        None => format!("header from {others} source{}", s(others)),
    };
    Ok((header.clone(), from))
}

/// A Bitcoin Core JSON-RPC call, authenticated with the node's cookie file. The cookie is a
/// credential: read only here, never printed, logged or kept.
fn rpc(url: &str, cookie_file: &str, method: &str, params: serde_json::Value) -> Result<serde_json::Value, String> {
    use base64::Engine;
    let cookie = std::fs::read(cookie_file).map_err(|x| format!("bitcoind cookie file {cookie_file}: {x}"))?;
    let auth = format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(cookie.trim_ascii()));
    let body = serde_json::json!({"jsonrpc": "1.0", "id": "ox", "method": method, "params": params}).to_string();
    let (status, resp) = http("POST", url, &[("Content-Type", "application/json"), ("Authorization", &auth)], body.as_bytes())?;
    if status == 401 || status == 403 {
        return Err(format!("{url}: the node refused the credentials in {cookie_file} (HTTP {status})"));
    }
    let v: serde_json::Value = serde_json::from_slice(&resp).map_err(|_| format!("{url}: {method}: HTTP {status}, not JSON-RPC"))?;
    if !v["error"].is_null() {
        return Err(format!("{url}: {method}: {}", v["error"]["message"].as_str().unwrap_or("error")));
    }
    Ok(v["result"].clone())
}

/// Deletes a layer's content, keeping its hash; the purge record replicates like a layer.
fn purge(dir: &Path, pos: &[&String], args: &[String]) -> Result<ExitCode, String> {
    let s = Store::open(dir).map_err(e)?;
    let h = s.resolve(pos.get(2).ok_or("`purge` needs a layer hash")?).map_err(e)?;
    let k = key_for(args)?;
    let mode = if has(args, "--key-destroy") { "key" } else { opt(args, "--mode").unwrap_or("bytes") };
    let c = s.purge_mode(&h, &k, opt(args, "--reason"), mode).map_err(e)?;
    println!(
        "ox: purged {}{} (record {}); head {}",
        short(&h),
        if mode == "key" { " (key destroyed; a recipient who already opened it may have kept a copy)" } else { "" },
        short(&c.layer),
        short(&c.stack)
    );
    Ok(ExitCode::SUCCESS)
}

fn sign(dir: &Path, pos: &[&String], args: &[String]) -> Result<ExitCode, String> {
    let s = Store::open(dir).map_err(e)?;
    let h = s
        .resolve(pos.get(2).ok_or("`sign` needs a layer hash")?)
        .map_err(e)?;
    let k = key_for(args)?;
    for p in s.sign(&h, &k, has(args, "--archive")).map_err(e)? {
        println!("ox: signed {} as {} → {}", short(&h), k.name, p.display());
    }
    Ok(ExitCode::SUCCESS)
}

/// Adds every public key of `k` to a project.
fn add_all(s: &Store, k: &KeyPair) -> Result<(), String> {
    s.add_key(&k.name, &k.public()).map_err(e)?;
    if let Some(x) = k.xwing_public() {
        s.add_xwing_key(&k.name, &x).map_err(e)?;
    }
    for p in [k.mldsa_public(), k.slh_public()].into_iter().flatten() {
        s.add_pq_key(&k.name, &p).map_err(e)?;
    }
    Ok(())
}

fn key(pos: &[&String]) -> Result<ExitCode, String> {
    match pos.get(1).map(|s| s.as_str()) {
        Some("new") => {
            let name = pos.get(2).ok_or("`key new` needs a name")?;
            let k = KeyPair::generate(name).map_err(e)?;
            let p = k.save(&KeyPair::dir()).map_err(e)?;
            println!("ox: new key {name} → {}", p.display());
            for public in k.publics() {
                println!("{public}");
            }
            if let Some(dir) = pos.get(3) {
                add_all(&Store::open(Path::new(dir.as_str())).map_err(e)?, &k)?;
                println!("ox: the project knows it (Ed25519, ML-DSA-65 and SLH-DSA for signatures, X-Wing for sealed layers)");
            }
            Ok(ExitCode::SUCCESS)
        }
        Some("upgrade") => {
            let name = pos.get(2).map(|n| n.to_string()).or_else(|| std::env::var("OX_KEY").ok()).unwrap_or_else(|| "default".into());
            let (k, added) = KeyPair::upgrade(&KeyPair::dir().join(format!("{name}.key"))).map_err(e)?;
            if added.is_empty() {
                println!("ox: key {name} already has every key; nothing changed");
            } else {
                println!("ox: added {} to key {name} (its existing keys are unchanged)", added.join(", "));
            }
            for public in k.publics() {
                println!("{public}");
            }
            if let Some(dir) = pos.get(3) {
                add_all(&Store::open(Path::new(dir.as_str())).map_err(e)?, &k)?;
                println!("ox: the project knows its new keys");
            }
            Ok(ExitCode::SUCCESS)
        }
        Some("add") => {
            let (Some(dir), Some(name), Some(public)) = (pos.get(2), pos.get(3), pos.get(4)) else {
                return Err("usage: ox key add <dir> <name> <ed25519:…> [<xwing:…>] [<mldsa65:…>] [<slhdsa-…:…>]".into());
            };
            let s = Store::open(Path::new(dir.as_str())).map_err(e)?;
            let p = s.add_key(name, public).map_err(e)?;
            for extra in &pos[5..] {
                if extra.starts_with("xwing:") {
                    s.add_xwing_key(name, extra).map_err(e)?;
                } else {
                    s.add_pq_key(name, extra).map_err(e)?;
                }
            }
            println!("ox: the project knows {name} ({})", p.display());
            Ok(ExitCode::SUCCESS)
        }
        Some("role") => {
            let (Some(dir), Some(name), Some(role)) = (pos.get(2), pos.get(3), pos.get(4)) else {
                return Err("usage: ox key role <dir> <name> <owner|approver|editor|viewer>".into());
            };
            let s = Store::open(Path::new(dir.as_str())).map_err(e)?;
            s.set_role(name, role).map_err(e)?;
            println!("ox: {name} is {role} in this project");
            Ok(ExitCode::SUCCESS)
        }
        _ => Err("usage: ox key new <name> [<dir>] | ox key upgrade [<name>] [<dir>] | ox key add <dir> <name> <ed25519:…> [<other keys>…] | ox key role <dir> <name> <role>".into()),
    }
}

#[cfg(test)]
mod tests {
    //! The network code against local fakes (no public calendar, explorer or node is contacted).
    use super::*;
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};

    type Handler = dyn Fn(&str, &str, &str, &[u8]) -> (u16, Vec<u8>) + Send + Sync;

    /// A one-request-per-connection HTTP server on 127.0.0.1; `handler(method, path, head, body)`.
    fn serve(handler: Arc<Handler>) -> String {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        std::thread::spawn(move || {
            for mut c in l.incoming().flatten() {
                let mut r = Vec::new();
                let mut buf = [0u8; 4096];
                let (head, body) = loop {
                    let n = c.read(&mut buf).unwrap_or(0);
                    r.extend_from_slice(&buf[..n]);
                    if let Some(i) = r.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&r[..i]).to_string();
                        let len: usize = head
                            .lines()
                            .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length: ").map(|v| v.trim().parse().unwrap()))
                            .unwrap_or(0);
                        if r.len() >= i + 4 + len || n == 0 {
                            break (head, r[i + 4..].to_vec());
                        }
                    } else if n == 0 {
                        break (String::new(), Vec::new());
                    }
                };
                let mut first = head.split_whitespace();
                let (m, p) = (first.next().unwrap_or_default().to_string(), first.next().unwrap_or_default().to_string());
                let (st, out) = handler(&m, &p, &head, &body);
                let _ = write!(c, "HTTP/1.0 {st} X\r\nContent-Length: {}\r\n\r\n", out.len());
                let _ = c.write_all(&out);
            }
        });
        url
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ox-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn opentimestamps_stamp_and_upgrade_against_a_local_calendar() {
        let seen = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
        let confirmed = Arc::new(Mutex::new(false));
        let me = Arc::new(Mutex::new(String::new()));
        let (s2, c2, m2) = (seen.clone(), confirmed.clone(), me.clone());
        let url = serve(Arc::new(move |m, p, _, body| {
            use ots::{Att, Item, Node, Op};
            match (m, p) {
                ("POST", "/digest") => {
                    s2.lock().unwrap().push(body.to_vec());
                    let pending = Node(vec![Item::Att(Att::Pending(m2.lock().unwrap().clone()))]);
                    let mut out = Vec::new();
                    Node(vec![Item::Op(Op::Prepend(vec![1, 2, 3, 4]), pending)]).write(&mut out);
                    (200, out)
                }
                ("GET", p) if p.starts_with("/timestamp/") && *c2.lock().unwrap() => {
                    let mut out = Vec::new();
                    Node(vec![Item::Op(Op::Sha256, Node(vec![Item::Att(Att::Bitcoin(912_095))]))]).write(&mut out);
                    (200, out)
                }
                _ => (404, b"Pending confirmation in Bitcoin blockchain".to_vec()),
            }
        }));
        *me.lock().unwrap() = url.clone();
        let d = tmp("ots");
        let f = d.join("sig.json");
        std::fs::write(&f, b"a signature file").unwrap();
        let cals = vec![url.clone()];

        ots_stamp(&f, b"a signature file", &cals).unwrap();
        let read = || ots::summarize(b"a signature file", &std::fs::read(ox_store::ots_of(&f)).unwrap()).unwrap();
        assert_eq!(read().pending, [url.as_str()]);
        // The calendar got a 32-byte commitment that isn't the file's digest.
        use sha2::Digest;
        let got = seen.lock().unwrap()[0].clone();
        assert_eq!(got.len(), 32);
        assert_ne!(got, sha2::Sha256::digest(b"a signature file").to_vec());
        // Stamping again keeps the first proof beside the new one.
        ots_stamp(&f, b"a signature file", &cals).unwrap();
        assert_eq!(read().pending.len(), 2);
        // A calendar no one chose is never asked, even if the proof names it.
        ots_upgrade(&f, &["https://elsewhere.example".into()]).unwrap();
        assert_eq!(read().pending.len(), 2);
        // Still pending (404): nothing changes; confirmed: both completed.
        ots_upgrade(&f, &cals).unwrap();
        assert_eq!(read().pending.len(), 2);
        *confirmed.lock().unwrap() = true;
        ots_upgrade(&f, &cals).unwrap();
        let s = read();
        assert!(s.pending.is_empty() && s.bitcoin.len() == 2 && s.bitcoin.iter().all(|(h, _)| *h == 912_095), "{s:?}");
        // No calendar answering is an error, not an empty proof.
        assert!(ots_stamp(&f, b"x", &["http://127.0.0.1:9".into()]).unwrap_err().contains("no OpenTimestamps calendar"));
        let _ = std::fs::remove_dir_all(&d);
    }

    fn header() -> Vec<u8> {
        ots::unhex(include_str!("../../ox-store/testdata/ots/912095.header")).unwrap()
    }

    /// An Esplora-like explorer serving `h` at height 912095.
    fn explorer(h: Vec<u8>) -> String {
        explorer_at(912_095, h)
    }

    fn explorer_at(height: u64, h: Vec<u8>) -> String {
        serve(Arc::new(move |_, p, _, _| {
            let hash = ots::block_hash(&h);
            match p {
                p if p == format!("/api/block-height/{height}") => (200, hash.into_bytes()),
                p if p == format!("/api/block/{hash}/header") => (200, ots::hex(&h).into_bytes()),
                _ => (404, vec![]),
            }
        })) + "/api"
    }

    /// A Bitcoin Core-like JSON-RPC node that wants `Basic base64(cookie)`.
    fn bitcoind(h: Vec<u8>, cookie: &str) -> String {
        use base64::Engine;
        let want = format!("authorization: basic {}", base64::engine::general_purpose::STANDARD.encode(cookie)).to_ascii_lowercase();
        serve(Arc::new(move |_, _, head, body| {
            if !head.to_ascii_lowercase().lines().any(|l| l == want) {
                return (401, vec![]);
            }
            let req: serde_json::Value = serde_json::from_slice(body).unwrap();
            let result = match req["method"].as_str() {
                Some("getblockhash") if req["params"][0] == 912_095 => serde_json::json!(ots::block_hash(&h)),
                Some("getblockheader") if req["params"][1] == false => serde_json::json!(ots::hex(&h)),
                _ => return (500, br#"{"result":null,"error":{"message":"bad request"},"id":"ox"}"#.to_vec()),
            };
            (200, serde_json::json!({"result": result, "error": null, "id": "ox"}).to_string().into_bytes())
        }))
    }

    #[test]
    fn bitcoin_headers_come_from_the_chosen_sources_and_must_agree() {
        let h = header();
        let mut other = h.clone();
        other[4] ^= 1; // another (fake) header: different previous-block hash
        let (a, b, bad) = (explorer(h.clone()), explorer(h.clone()), explorer(other.clone()));
        let ex = |urls: &[&String]| HeaderSource::Explorers { urls: urls.iter().map(|u| u.to_string()).collect() };

        let get = |src: &[HeaderSource]| bitcoin_header(src, 912_095, &headers::CHECKPOINT);
        assert_eq!(get(&[ex(&[&a, &b])]).unwrap(), (h.clone(), "header from 2 sources".into()));
        let err = get(&[ex(&[&a, &bad])]).unwrap_err();
        assert!(err.contains("disagree") && err.contains(&a) && err.contains(&bad), "{err}");

        let d = tmp("cookie");
        let cookie = d.join(".cookie");
        std::fs::write(&cookie, "__cookie__:s3cret-value\n").unwrap();
        let node = |h: Vec<u8>, c: &str| HeaderSource::Bitcoind { url: bitcoind(h, c), cookie_file: cookie.display().to_string() };
        assert_eq!(get(&[node(h.clone(), "__cookie__:s3cret-value")]).unwrap(), (h.clone(), "header from 1 source".into()));
        assert_eq!(get(&[node(h.clone(), "__cookie__:s3cret-value"), ex(&[&a])]).unwrap().1, "header from 2 sources");
        let err = get(&[node(h.clone(), "__cookie__:other")]).unwrap_err();
        assert!(err.contains("refused the credentials") && !err.contains("s3cret"), "{err}");
        let err = get(&[node(other, "__cookie__:s3cret-value"), ex(&[&a])]).unwrap_err();
        assert!(err.contains("disagree"), "{err}");
        std::fs::remove_file(&cookie).unwrap();
        let err = get(&[node(h.clone(), "x")]).unwrap_err();
        assert!(err.contains("cookie file"), "{err}");
        assert!(get(&[]).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The light client's header file: checked from genesis against a checkpoint, then read at the
    /// proof's height; with other sources, all must agree. Fixture: the first 2101 real headers.
    #[test]
    fn bitcoin_headers_from_the_light_clients_file() {
        let real = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../ox-store/testdata/headers/first-2101.bin")).unwrap();
        let cp = headers::Checkpoint {
            height: 2100,
            hash: "00000000d5092034c3a92473d42f28e3fd279f0652e2d5b0049c7d40fcbc990b",
            time: 1_233_136_967,
            work: "0000000000000000000000000000000000000000000000000000083508350835",
        };
        let d = tmp("light");
        let f = d.join("bitcoin-headers.bin");
        std::fs::write(&f, &real).unwrap();
        let lc = |f: &Path| HeaderSource::LightClient { file: Some(f.display().to_string()) };
        let at = |h: u64| real[h as usize * 80..h as usize * 80 + 80].to_vec();

        let (h, from) = bitcoin_header(&[lc(&f)], 2000, &cp).unwrap();
        assert_eq!((h, from.as_str()), (at(2000), "header from the light client (synced to 2100)"));
        let err = bitcoin_header(&[lc(&f)], 2101, &cp).unwrap_err();
        assert!(err.contains("reaches block 2100") && err.contains("ox-headers sync"), "{err}");
        // With explorers: all must agree.
        let (good, bad) = (explorer_at(1500, at(1500)), explorer_at(1500, at(1499)));
        let ex = |u: &String| HeaderSource::Explorers { urls: vec![u.clone()] };
        assert_eq!(bitcoin_header(&[lc(&f), ex(&good)], 1500, &cp).unwrap().1, "header from the light client (synced to 2100) and 1 other source");
        assert!(bitcoin_header(&[lc(&f), ex(&bad)], 1500, &cp).unwrap_err().contains("disagree"));
        // Missing, stale against the checkpoint, or broken: refused with the reason.
        let err = bitcoin_header(&[lc(&d.join("none.bin"))], 1, &cp).unwrap_err();
        assert!(err.contains("no header file") && err.contains("ox-headers sync"), "{err}");
        let short = d.join("short.bin");
        std::fs::write(&short, &real[..80 * 2000]).unwrap();
        assert!(bitcoin_header(&[lc(&short)], 1, &cp).unwrap_err().contains("below the release checkpoint"));
        let broken = d.join("broken.bin");
        let mut b = real.clone();
        b[700 * 80 + 10] ^= 1;
        std::fs::write(&broken, &b).unwrap();
        assert!(bitcoin_header(&[lc(&broken)], 1, &cp).unwrap_err().contains("block 700 doesn't link"));
        // The release checkpoint is far above this fixture: the real verify refuses it.
        let fresh = d.join("fresh.bin");
        std::fs::write(&fresh, &real).unwrap();
        assert!(bitcoin_header(&[lc(&fresh)], 1, &headers::CHECKPOINT).unwrap_err().contains("below the release checkpoint"));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A local TLS server whose certificate comes from a test CA: refused against the bundled roots
    /// and against a CA file of another name, accepted with the test CA as `SSL_CERT_FILE`.
    #[test]
    fn https_trust_bundled_roots_or_ssl_cert_file() {
        use boring::{asn1::Asn1Time, bn::BigNum, ec::{EcGroup, EcKey}, hash::MessageDigest, nid::Nid, pkey::{PKey, Private}};
        use boring::ssl::{SslAcceptor, SslMethod};
        use boring::x509::{X509, X509NameBuilder, extension::{BasicConstraints, SubjectAlternativeName}};
        let key = || PKey::from_ec_key(EcKey::generate(&EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap()).unwrap()).unwrap();
        let cert = |cn: &str, k: &PKey<Private>, ca: Option<(&X509, &PKey<Private>)>| {
            let mut n = X509NameBuilder::new().unwrap();
            n.append_entry_by_nid(Nid::COMMONNAME, cn).unwrap();
            let n = n.build();
            let mut b = X509::builder().unwrap();
            b.set_version(2).unwrap();
            b.set_serial_number(&BigNum::from_u32(if ca.is_some() { 2 } else { 1 }).unwrap().to_asn1_integer().unwrap()).unwrap();
            b.set_subject_name(&n).unwrap();
            b.set_issuer_name(ca.map_or(&*n, |(c, _)| c.subject_name())).unwrap();
            b.set_pubkey(k).unwrap();
            b.set_not_before(&Asn1Time::days_from_now(0).unwrap()).unwrap();
            b.set_not_after(&Asn1Time::days_from_now(2).unwrap()).unwrap();
            match ca {
                None => b.append_extension(&BasicConstraints::new().critical().ca().build().unwrap()).unwrap(),
                Some((c, _)) => {
                    let san = SubjectAlternativeName::new().dns("localhost").build(&b.x509v3_context(Some(&**c), None)).unwrap();
                    b.append_extension(&san).unwrap();
                }
            }
            b.sign(ca.map_or(k, |(_, ck)| ck), MessageDigest::sha256()).unwrap();
            b.build()
        };
        let (ca_key, server_key) = (key(), key());
        let ca = cert("ox test CA", &ca_key, None);
        let server = cert("localhost", &server_key, Some((&ca, &ca_key)));
        let other_ca = cert("another CA", &key(), None);
        let mut acc = SslAcceptor::mozilla_intermediate_v5(SslMethod::tls()).unwrap();
        acc.set_private_key(&server_key).unwrap();
        acc.set_certificate(&server).unwrap();
        let acc = acc.build();
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            for c in l.incoming().flatten() {
                if let Ok(mut s) = acc.accept(c) {
                    let _ = s.write_all(b"HTTP/1.0 200 OK\r\n\r\nhello");
                    let _ = s.shutdown();
                }
            }
        });
        let d = tmp("tls");
        let (ca_file, other_file) = (d.join("ca.pem"), d.join("other.pem"));
        std::fs::write(&ca_file, ca.to_pem().unwrap()).unwrap();
        std::fs::write(&other_file, other_ca.to_pem().unwrap()).unwrap();
        let get = |file: Option<&Path>, system: bool, name: &str| -> Result<String, String> {
            let mut s = trust(file, system)?.connect(name, std::net::TcpStream::connect(addr).unwrap()).map_err(|x| x.to_string())?;
            let mut out = String::new();
            s.read_to_string(&mut out).map_err(|x| x.to_string())?;
            Ok(out)
        };
        assert!(webpki_root_certs::TLS_SERVER_ROOT_CERTS.len() > 100);
        assert!(get(None, false, "localhost").is_err(), "the bundled roots don't know the test CA");
        assert!(get(Some(&other_file), true, "localhost").is_err(), "another CA");
        assert!(get(Some(&ca_file), false, "localhost").unwrap().ends_with("hello"));
        assert!(get(Some(&ca_file), false, "127.0.0.2").is_err(), "the host name is checked");
        assert!(trust(Some(&d.join("missing.pem")), true).unwrap_err().contains("SSL_CERT_FILE"));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The real public explorers over https (BoringSSL): run by hand, never in CI
    /// (`--ignored`); reads public chain data only.
    #[test]
    #[ignore = "network: contacts mempool.space and blockstream.info"]
    fn the_default_explorers_over_https() {
        let sources = ox_store::policy::Timestamps::default().headers();
        assert_eq!(bitcoin_header(&sources, 912_095, &headers::CHECKPOINT).unwrap(), (header(), "header from 2 sources".into()));
    }

    /// The real confirmed fixture proof (block 971959) against a header file `ox-headers sync` made
    /// (`OX_HEADERS_FILE`), alone and with the default explorers: by hand, never in CI.
    #[test]
    #[ignore = "needs a synced header file (OX_HEADERS_FILE); the second half contacts the explorers"]
    fn the_fixture_proof_against_a_synced_light_client() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../ox-store/testdata/ots");
        let s = ots::summarize(&std::fs::read(dir.join("random32.bin")).unwrap(), &std::fs::read(dir.join("random32.bin.ots")).unwrap()).unwrap();
        let (height, root) = s.bitcoin.iter().find(|(h, _)| *h == 971_959).unwrap();
        let lc = HeaderSource::LightClient { file: Some(std::env::var("OX_HEADERS_FILE").unwrap()) };
        let mut sources = vec![lc];
        for _ in 0..2 {
            let t = std::time::Instant::now();
            let (h, from) = bitcoin_header(&sources, *height, &headers::CHECKPOINT).unwrap();
            let time = ots::check_header(&h, root).unwrap();
            eprintln!("existed by {} (Bitcoin block {height}, {from}) in {:?}", ox_store::rfc3339(i64::from(time)), t.elapsed());
            assert!(from.starts_with("header from the light client (synced to "), "{from}");
            sources.extend(ox_store::policy::Timestamps::default().headers());
        }
    }
}
