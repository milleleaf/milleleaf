//! Revision-store commands (`ox init`, `commit`, `log`, `blame`, `checkpoint`, `verify`, `sign`,
//! `key`) and the helpers that let every other command take a project folder.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ox_core::compose::Flat;
use ox_core::render::find;
use ox_core::{Composed, Composer};
use ox_core::model::Layer;
use ox_core::stack::LoadedLayer;
use ox_store::{CommitOptions, KeyPair, LayerStatus, Store, StoreError};

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
    KeyPair::load_named(&name)
        .map_err(|err| format!("{err}\n(make one with `ox key new {name}`, or pass --key NAME)"))
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
    let opts = CommitOptions {
        expect: opt(args, "--expect")
            .map(|h| s.resolve(h))
            .transpose()
            .map_err(e)?,
        rebase: has(args, "--rebase"),
        author: opt(args, "--author").map(String::from),
        sign: key.as_ref(),
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
                "ox: committed {} ({}){} → head {}",
                layer.header.id,
                short(&c.layer),
                if c.signed { ", signed" } else { "" },
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
    let (mut bad, mut unsigned, mut untrusted) = (0, 0, 0);
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
                signatures
                    .iter()
                    .map(|g| {
                        let who = match &g.known_as {
                            Some(n) => n.clone(),
                            None => format!("unknown key {} (says {})", g.key_id, g.claimed),
                        };
                        if g.valid {
                            format!("signed by {who}")
                        } else {
                            format!("INVALID signature ({who})")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("; ")
            }
            LayerStatus::Unsigned => {
                unsigned += 1;
                "unsigned".into()
            }
            LayerStatus::Purged { by, at, reason } => format!(
                "purged by {by} on {at}{}",
                reason.as_ref().map(|r| format!(" ({r})")).unwrap_or_default()
            ),
        };
        println!("{}  {:<44} {text}", short(&h), id);
    }
    for problem in s.verify_blobs().map_err(e)? {
        bad += 1;
        println!("BLOB  {problem}");
    }
    if untrusted > 0 {
        eprintln!(
            "note: {untrusted} layer(s) signed only by keys this project doesn't list \
             (`ox key add` to trust one); `--require` counts them as unsigned"
        );
    }
    Ok(if bad > 0 || (require && unsigned + untrusted > 0) {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

/// Deletes a layer's content, keeping its hash; the purge record replicates like a layer.
fn purge(dir: &Path, pos: &[&String], args: &[String]) -> Result<ExitCode, String> {
    let s = Store::open(dir).map_err(e)?;
    let h = s.resolve(pos.get(2).ok_or("`purge` needs a layer hash")?).map_err(e)?;
    let k = key_for(args)?;
    let c = s.purge(&h, &k, opt(args, "--reason")).map_err(e)?;
    println!("ox: purged {} (record {}); head {}", short(&h), short(&c.layer), short(&c.stack));
    Ok(ExitCode::SUCCESS)
}

fn sign(dir: &Path, pos: &[&String], args: &[String]) -> Result<ExitCode, String> {
    let s = Store::open(dir).map_err(e)?;
    let h = s
        .resolve(pos.get(2).ok_or("`sign` needs a layer hash")?)
        .map_err(e)?;
    let k = key_for(args)?;
    let p = s.sign(&h, &k).map_err(e)?;
    println!("ox: signed {} as {} → {}", short(&h), k.name, p.display());
    Ok(ExitCode::SUCCESS)
}

fn key(pos: &[&String]) -> Result<ExitCode, String> {
    match pos.get(1).map(|s| s.as_str()) {
        Some("new") => {
            let name = pos.get(2).ok_or("`key new` needs a name")?;
            let k = KeyPair::generate(name).map_err(e)?;
            let p = k.save(&KeyPair::dir()).map_err(e)?;
            println!("ox: new key {name} → {}", p.display());
            println!("{}", k.public());
            if let Some(dir) = pos.get(3) {
                let s = Store::open(Path::new(dir.as_str())).map_err(e)?;
                let q = s.add_key(name, &k.public()).map_err(e)?;
                println!("ox: the project knows it ({})", q.display());
            }
            Ok(ExitCode::SUCCESS)
        }
        Some("add") => {
            let (Some(dir), Some(name), Some(public)) = (pos.get(2), pos.get(3), pos.get(4)) else {
                return Err("usage: ox key add <dir> <name> <ed25519:…>".into());
            };
            let s = Store::open(Path::new(dir.as_str())).map_err(e)?;
            let p = s.add_key(name, public).map_err(e)?;
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
        _ => Err("usage: ox key new <name> [<dir>] | ox key add <dir> <name> <ed25519:…> | ox key role <dir> <name> <role>".into()),
    }
}
