//! Revision-store commands (`ox init`, `commit`, `log`, `blame`, `checkpoint`, `verify`, `sign`,
//! `timestamp`, `key`) and the helpers that let every other command take a project folder.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ox_core::compose::Flat;
use ox_core::render::find;
use ox_core::{Composed, Composer};
use ox_core::model::Layer;
use ox_core::stack::LoadedLayer;
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
                describe(signatures)
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
    Ok(if bad > 0 || refused > 0 { ExitCode::from(1) } else { ExitCode::SUCCESS })
}

/// "signed by sam (hybrid+slh-dsa, timestamped …); INVALID mldsa65 signature (…)".
fn describe(sigs: &[SigInfo]) -> String {
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
            format!(
                "signed by {w} ({}{})",
                ox_store::strength(&algs),
                stamp.map(|t| format!(", timestamped {t}, TSA signature not checked")).unwrap_or_default()
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

/// Time-stamps every signature file of a layer with an RFC 3161 TSA; keeps each token beside its
/// file (`<file>.tsr`).
fn timestamp(dir: &Path, pos: &[&String], args: &[String]) -> Result<ExitCode, String> {
    let s = Store::open(dir).map_err(e)?;
    let h = s.resolve(pos.get(2).ok_or("`timestamp` needs a layer hash")?).map_err(e)?;
    let url = match opt(args, "--tsa") {
        Some(u) => u.to_string(),
        None => s
            .policy()
            .map_err(e)?
            .timestamps
            .and_then(|t| t.tsa)
            .ok_or("no time-stamp authority: pass --tsa URL or set timestamps.tsa in .ox/policy.json (there is no default)")?,
    };
    let files = s.signature_files(&h);
    if files.is_empty() {
        return Err(format!("{} is unsigned; sign it first", short(&h)));
    }
    for f in files {
        let data = std::fs::read(&f).map_err(|x| format!("{}: {x}", f.display()))?;
        let (req, nonce) = ox_store::tsp::request(&data).map_err(e)?;
        let resp = http_post(&url, "application/timestamp-query", &req)?;
        let stamp = ox_store::tsp::check(&resp, &data).map_err(|x| format!("{url}: {x}"))?;
        let ours: Vec<u8> = nonce.iter().copied().skip_while(|b| *b == 0).collect();
        if stamp.nonce.as_deref() != Some(ours.as_slice()) {
            return Err(format!("{url}: the response doesn't carry our nonce (a replayed or unrelated token)"));
        }
        let t = ox_store::tsr_of(&f);
        std::fs::write(&t, &resp).map_err(|x| format!("{}: {x}", t.display()))?;
        println!("ox: timestamped {} at {} → {}", f.file_name().unwrap_or_default().to_string_lossy(), stamp.at, t.display());
    }
    Ok(ExitCode::SUCCESS)
}

/// POSTs `body` over plain HTTP/1.0 and returns the response body.
// ponytail: http:// only. A time-stamp token is signed, so transport security adds no integrity;
// add a TLS client when a TSA the maintainer chooses serves https only.
fn http_post(url: &str, content_type: &str, body: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::{Read, Write};
    let rest = url.strip_prefix("http://").ok_or_else(|| format!("{url}: only http:// time-stamp URLs are supported"))?;
    let (host, path) = match rest.split_once('/') {
        Some((h, p)) => (h, format!("/{p}")),
        None => (rest, "/".to_string()),
    };
    let addr = if host.contains(':') { host.to_string() } else { format!("{host}:80") };
    let err = |x: std::io::Error| format!("{url}: {x}");
    let mut c = std::net::TcpStream::connect(&addr).map_err(err)?;
    c.set_read_timeout(Some(std::time::Duration::from_secs(30))).map_err(err)?;
    write!(c, "POST {path} HTTP/1.0\r\nHost: {host}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).map_err(err)?;
    c.write_all(body).map_err(err)?;
    let mut r = Vec::new();
    c.read_to_end(&mut r).map_err(err)?;
    let split = r.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| format!("{url}: not an HTTP response"))?;
    let head = String::from_utf8_lossy(&r[..split]);
    let status = head.lines().next().unwrap_or_default();
    if status.split_whitespace().nth(1) != Some("200") {
        return Err(format!("{url}: {status}"));
    }
    Ok(r[split + 4..].to_vec())
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
