//! `ox-store`: the Milleleaf revision store.
//!
//! A project folder gets a `.ox/` directory:
//!
//! ```text
//! .ox/
//!   HEAD                      hash of the current stack
//!   layers/<sha256>.json      every layer ever committed, immutable, named by its hash
//!   stacks/<sha256>.json      one per commit: imports the previous stack + the new layer
//!   index/<sha256>.json       per layer, written once: header + which nodes and keys it touches
//!   sigs/<sha256>/<key>.json  detached Ed25519 signatures (optional)
//!   keys/<name>.pub           public keys the project knows
//!   cache/                    derived, safe to delete (flattened stacks)
//! ```
//!
//! Files are only ever added (except `HEAD` and the cache), so the folder works under git and
//! in synced folders. `HEAD` moves by compare-and-swap: a commit names the head it was made
//! against and is refused if the head has moved since.

mod history;
mod keys;
mod time;

pub use history::{BlameEntry, Touch};
pub use keys::{KeyPair, LayerStatus, SigInfo, Signature};

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ox_core::compose::Flat;
use ox_core::model::{Header, Import, Layer, OX_VERSION};
use ox_core::stack::{LoadedLayer, integrity_of};
use serde::{Deserialize, Serialize};

pub const DIR: &str = ".ox";

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("{0}: {1}")]
    Io(PathBuf, #[source] std::io::Error),
    #[error("{0}: not a valid OX layer: {1}")]
    Parse(PathBuf, #[source] serde_json::Error),
    #[error("{0} has no .ox folder (run `ox init` there first)")]
    NotARepo(PathBuf),
    #[error("{0} already has a .ox folder")]
    AlreadyARepo(PathBuf),
    #[error("nothing committed yet")]
    Empty,
    #[error("{0}: unknown layer or stack")]
    Unknown(String),
    #[error("layer {hash} is damaged: its content hashes to {actual}")]
    Tampered { hash: String, actual: String },
    #[error(
        "stale: this layer was made against {expected}, but the head is now {head}; {} layer(s) \
         were committed since ({}). Rebase it onto the new head (`--rebase`) and commit again.",
        since.len(), since.join(", ")
    )]
    Stale {
        expected: String,
        head: String,
        /// Ids of the layers committed since `expected`.
        since: Vec<String>,
    },
    #[error(
        "can't rebase: layers committed since also changed {}",
        overlaps.iter().map(|(l, p, k)| format!("{p} {k} (in {l})")).collect::<Vec<_>>().join("; ")
    )]
    RebaseConflict {
        /// (layer id, node path, key) both sides set.
        overlaps: Vec<(String, String, String)>,
    },
    #[error("layer {0} is already in the stack")]
    AlreadyCommitted(String),
    #[error("a stack can't be committed as a layer; use `ox init --from` to import a stack")]
    StackAsLayer,
    #[error("the head is locked by another commit ({0}, held by `{1}` = pid host); if no commit is running there, delete it")]
    Busy(PathBuf, String),
    #[error("{0}")]
    Key(String),
}

type Result<T> = std::result::Result<T, StoreError>;

fn io<T>(p: &Path, r: std::io::Result<T>) -> Result<T> {
    r.map_err(|e| StoreError::Io(p.to_path_buf(), e))
}

/// `sha256` hex of `bytes`: the name of a layer or stack in the store.
pub fn hash_of(bytes: &[u8]) -> String {
    integrity_of(bytes)
        .strip_prefix("sha256-")
        .unwrap_or_default()
        .to_string()
}

/// Bytes a layer is stored as. Its hash is the hash of these bytes; signatures are kept apart,
/// so signing never changes it.
pub fn layer_bytes(layer: &Layer) -> Vec<u8> {
    let mut v = serde_json::to_vec_pretty(layer).expect("layers serialize");
    v.push(b'\n');
    v
}

/// What one layer touches, written once when it's stored (`.ox/index/<hash>.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub header: Header,
    /// node path → keys set or removed: attribute names, `children/<name>`, `inherits/<name>`,
    /// `order`.
    pub touches: BTreeMap<String, Vec<String>>,
}

pub fn touches_of(layer: &Layer) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for n in &layer.data {
        let e = out.entry(n.path.clone()).or_default();
        e.extend(n.attributes.keys().cloned());
        e.extend(n.children.keys().map(|k| format!("children/{k}")));
        e.extend(n.inherits.keys().map(|k| format!("inherits/{k}")));
        if n.order.is_some() {
            e.insert("order".into());
        }
    }
    out.into_iter()
        .map(|(k, v)| (k, v.into_iter().collect()))
        .collect()
}

/// One commit: a stack file in `.ox/stacks/`.
#[derive(Debug, Clone)]
pub struct StackInfo {
    pub hash: String,
    pub header: Header,
    /// The stack this one builds on (its first import), if any. Differs from `header.parent`
    /// after a checkpoint, which starts a new chain.
    pub base: Option<String>,
    /// Layers this stack adds on top of `base`, bottom first.
    pub added: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CommitOptions<'a> {
    /// The head the layer was made against; defaults to `header.base`, then to "whatever the
    /// head is" (append without checking).
    pub expect: Option<String>,
    /// If the head moved, stack the layer on the new head anyway, as long as nothing committed
    /// since touched the same keys.
    pub rebase: bool,
    /// Author of the commit (the stack); defaults to the layer's author.
    pub author: Option<String>,
    pub sign: Option<&'a KeyPair>,
}

#[derive(Debug, Clone)]
pub struct Commit {
    pub layer: String,
    pub stack: String,
    pub previous: Option<String>,
    /// Imports dropped from the layer: in the store, the stack decides what is beneath it.
    pub dropped_imports: usize,
    /// Layers committed between `expect` and the head the layer landed on (when rebased).
    pub rebased_over: Vec<String>,
    pub signed: bool,
}

#[derive(Debug, Clone)]
pub struct Store {
    pub root: PathBuf,
    pub dir: PathBuf,
}

impl Store {
    pub fn init(project: &Path) -> Result<Store> {
        let dir = project.join(DIR);
        if dir.exists() {
            return Err(StoreError::AlreadyARepo(project.to_path_buf()));
        }
        for sub in ["layers", "stacks", "index", "sigs", "keys", "cache"] {
            io(&dir, fs::create_dir_all(dir.join(sub)))?;
        }
        io(
            &dir,
            fs::write(
                dir.join(".gitignore"),
                "# derived; safe to delete\ncache/\nHEAD.lock\n",
            ),
        )?;
        Ok(Store {
            root: project.to_path_buf(),
            dir,
        })
    }

    pub fn open(project: &Path) -> Result<Store> {
        let dir = project.join(DIR);
        if !dir.is_dir() {
            return Err(StoreError::NotARepo(project.to_path_buf()));
        }
        Ok(Store {
            root: project.to_path_buf(),
            dir,
        })
    }

    /// `Some` when `path` is a project folder with a `.ox` store.
    pub fn find(path: &Path) -> Option<Store> {
        if path.is_dir() {
            Store::open(path).ok()
        } else {
            None
        }
    }

    fn layer_path(&self, hash: &str) -> PathBuf {
        self.dir.join("layers").join(format!("{hash}.json"))
    }

    pub fn stack_path(&self, hash: &str) -> PathBuf {
        self.dir.join("stacks").join(format!("{hash}.json"))
    }

    fn index_path(&self, hash: &str) -> PathBuf {
        self.dir.join("index").join(format!("{hash}.json"))
    }

    fn cache_path(&self, stack: &str) -> PathBuf {
        self.dir.join("cache").join(format!("{stack}.flat.json"))
    }

    pub fn head(&self) -> Result<Option<String>> {
        let p = self.dir.join("HEAD");
        match fs::read_to_string(&p) {
            Ok(s) => Ok(Some(s.trim().to_string()).filter(|s| !s.is_empty())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(StoreError::Io(p, e)),
        }
    }

    /// Resolves a hash prefix (at least 4 characters) or `HEAD` to a full layer or stack hash.
    pub fn resolve(&self, what: &str) -> Result<String> {
        if what.eq_ignore_ascii_case("head") {
            return self.head()?.ok_or(StoreError::Empty);
        }
        let what = what.strip_prefix("sha256-").unwrap_or(what);
        if what.len() < 4 {
            return Err(StoreError::Unknown(what.into()));
        }
        let mut found = BTreeSet::new();
        for sub in ["layers", "stacks"] {
            let d = self.dir.join(sub);
            for e in io(&d, fs::read_dir(&d))?.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if let Some(h) = name.strip_suffix(".json")
                    && h.starts_with(what)
                {
                    found.insert(h.to_string());
                }
            }
        }
        match found.len() {
            1 => Ok(found.pop_first().unwrap_or_default()),
            _ => Err(StoreError::Unknown(what.into())),
        }
    }

    /// Stores `layer` (imports dropped) and its index entry, if not already there. Returns its
    /// hash and how many imports were dropped.
    pub fn put_layer(&self, layer: &Layer) -> Result<(String, usize)> {
        self.put_layer_new(layer).map(|(h, d, _)| (h, d))
    }

    /// As `put_layer`, also saying whether the layer was new to the store.
    fn put_layer_new(&self, layer: &Layer) -> Result<(String, usize, bool)> {
        let mut layer = layer.clone();
        if layer.header.kind.as_deref() == Some("stack") && layer.data.is_empty() {
            return Err(StoreError::StackAsLayer);
        }
        let dropped = std::mem::take(&mut layer.imports).len();
        let bytes = layer_bytes(&layer);
        let hash = hash_of(&bytes);
        let path = self.layer_path(&hash);
        let new = !path.exists();
        if new {
            write_new(&path, &bytes)?;
        }
        let ip = self.index_path(&hash);
        if !ip.exists() {
            let entry = IndexEntry {
                header: layer.header.clone(),
                touches: touches_of(&layer),
            };
            write_new(&ip, &serde_json::to_vec_pretty(&entry).expect("json"))?;
        }
        Ok((hash, dropped, new))
    }

    /// The layer stored under `hash`, checked against its hash.
    pub fn layer(&self, hash: &str) -> Result<Layer> {
        let p = self.layer_path(hash);
        let bytes = io(&p, fs::read(&p))?;
        let actual = hash_of(&bytes);
        if actual != hash {
            return Err(StoreError::Tampered {
                hash: hash.into(),
                actual,
            });
        }
        Layer::from_json(&bytes).map_err(|e| StoreError::Parse(p, e))
    }

    /// Index entry for a layer (written at commit; rebuilt if missing).
    pub fn index(&self, hash: &str) -> Result<IndexEntry> {
        let p = self.index_path(hash);
        if let Ok(bytes) = fs::read(&p)
            && let Ok(e) = serde_json::from_slice(&bytes)
        {
            return Ok(e);
        }
        let layer = self.layer(hash)?;
        let entry = IndexEntry {
            header: layer.header.clone(),
            touches: touches_of(&layer),
        };
        let _ = fs::write(&p, serde_json::to_vec_pretty(&entry).expect("json"));
        Ok(entry)
    }

    pub fn stack(&self, hash: &str) -> Result<StackInfo> {
        let p = self.stack_path(hash);
        let bytes = io(&p, fs::read(&p))?;
        let actual = hash_of(&bytes);
        if actual != hash {
            return Err(StoreError::Tampered {
                hash: hash.into(),
                actual,
            });
        }
        let layer = Layer::from_json(&bytes).map_err(|e| StoreError::Parse(p, e))?;
        let mut base = None;
        let mut added = Vec::new();
        for imp in &layer.imports {
            let name = imp.uri.rsplit('/').next().unwrap_or_default();
            let h = name.strip_suffix(".json").unwrap_or(name).to_string();
            if imp.uri.starts_with("../layers/") {
                added.push(h);
            } else {
                base = Some(h);
            }
        }
        Ok(StackInfo {
            hash: hash.into(),
            header: layer.header,
            base,
            added,
        })
    }

    /// Layer hashes of a stack, bottom first.
    pub fn layers(&self, stack: &str) -> Result<Vec<String>> {
        let mut chain = Vec::new();
        let mut cur = Some(stack.to_string());
        while let Some(h) = cur {
            let s = self.stack(&h)?;
            cur = s.base.clone();
            chain.push(s.added);
        }
        Ok(chain.into_iter().rev().flatten().collect())
    }

    /// Layer hashes of the head, bottom first (empty before the first commit).
    pub fn head_layers(&self) -> Result<Vec<String>> {
        match self.head()? {
            Some(h) => self.layers(&h),
            None => Ok(vec![]),
        }
    }

    /// The layers of a stack, loaded and checked, as `ox_core::load_stack` would give them.
    pub fn load(&self, stack: &str) -> Result<Vec<LoadedLayer>> {
        self.layers(stack)?
            .into_iter()
            .map(|h| {
                Ok(LoadedLayer {
                    source: self.layer_path(&h),
                    layer: self.layer(&h)?,
                })
            })
            .collect()
    }

    /// A stack flattened, from the cache when possible (`.ox/cache`), else from the nearest
    /// cached ancestor plus the layers since, else from scratch. The result is cached.
    pub fn flat(&self, stack: &str) -> Result<Flat> {
        // Walk back to the nearest cached stack (or the start of the chain), then forward.
        let mut pending = Vec::new();
        let mut cur = Some(stack.to_string());
        let mut flat = Flat::default();
        while let Some(h) = cur {
            if let Some(f) = self.cached_flat(&h) {
                flat = f;
                break;
            }
            let s = self.stack(&h)?;
            cur = s.base.clone();
            pending.push(s.added);
        }
        for added in pending.into_iter().rev() {
            for h in &added {
                flat.apply(&self.layer(h)?);
            }
        }
        self.save_flat(stack, &flat);
        Ok(flat)
    }

    fn cached_flat(&self, stack: &str) -> Option<Flat> {
        let bytes = fs::read(self.cache_path(stack)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    fn save_flat(&self, stack: &str, flat: &Flat) {
        let p = self.cache_path(stack);
        let _ = fs::create_dir_all(p.parent().unwrap_or(Path::new(".")));
        let tmp = p.with_extension(format!("tmp{}", std::process::id()));
        if fs::write(&tmp, serde_json::to_vec(flat).unwrap_or_default()).is_ok() {
            let _ = fs::rename(&tmp, &p);
        }
    }

    /// Layers in `head` that `expected` doesn't have (checkpoints expanded), as hashes.
    fn since(&self, expected: &str, head: &str) -> Result<Vec<String>> {
        let mut have = HashSet::new();
        for h in self.layers(expected)? {
            self.expand(&h, &mut |x| {
                have.insert(x.to_string());
            })?;
        }
        let mut out = Vec::new();
        for h in self.layers(head)? {
            let mut all = Vec::new();
            self.expand(&h, &mut |x| all.push(x.to_string()))?;
            if all.iter().any(|x| !have.contains(x)) {
                out.push(h);
            }
        }
        Ok(out)
    }

    /// Calls `f` with `hash`, or with the layers a checkpoint replaces (recursively).
    fn expand(&self, hash: &str, f: &mut dyn FnMut(&str)) -> Result<()> {
        let e = self.index(hash)?;
        if e.header.replaces.is_empty() {
            f(hash);
        } else {
            for r in &e.header.replaces {
                self.expand(r, f)?;
            }
        }
        Ok(())
    }

    /// Commits `layer` on top of the head by compare-and-swap.
    pub fn commit(&self, layer: &Layer, opts: CommitOptions) -> Result<Commit> {
        let (hash, dropped, new) = self.put_layer_new(layer)?;
        let expect = opts.expect.clone().or(layer.header.base.clone());
        let _lock = HeadLock::acquire(&self.dir)?;
        let head = self.head()?;
        let mut rebased_over = Vec::new();
        if let Some(exp) = &expect
            && head.as_deref() != Some(exp.as_str())
        {
            let cur = head.clone().ok_or(StoreError::Empty)?;
            let since = self.since(exp, &cur)?;
            if !opts.rebase {
                return Err(StoreError::Stale {
                    expected: exp.clone(),
                    head: cur,
                    since: since
                        .iter()
                        .map(|h| self.index(h).map(|e| e.header.id))
                        .collect::<Result<_>>()?,
                });
            }
            let mine = self.index(&hash)?.touches;
            let mut overlaps = Vec::new();
            for h in &since {
                let mut originals = Vec::new();
                self.expand(h, &mut |x| originals.push(x.to_string()))?;
                for o in originals {
                    let e = self.index(&o)?;
                    for (path, keys) in &e.touches {
                        if let Some(my) = mine.get(path) {
                            for k in keys.iter().filter(|k| my.contains(k)) {
                                overlaps.push((e.header.id.clone(), path.clone(), k.clone()));
                            }
                        }
                    }
                }
            }
            if !overlaps.is_empty() {
                return Err(StoreError::RebaseConflict { overlaps });
            }
            rebased_over = since;
        }
        // Only a layer the store already had can already be in the stack.
        if !new
            && let Some(h) = &head
            && self.layers(h)?.contains(&hash)
        {
            return Err(StoreError::AlreadyCommitted(hash));
        }
        let author = opts
            .author
            .clone()
            .unwrap_or_else(|| layer.header.author.clone());
        let stack = self.write_stack(
            head.as_deref(),
            head.as_deref(),
            std::slice::from_ref(&hash),
            &author,
        )?;
        self.set_head(&stack)?;
        drop(_lock);
        // Derived data, after the head moved: extend the parent's cached flat by one layer.
        if let Some(prev) = &head
            && let Some(mut flat) = self.cached_flat(prev)
        {
            flat.apply(&self.layer(&hash)?);
            self.save_flat(&stack, &flat);
            let _ = fs::remove_file(self.cache_path(prev));
        }
        let mut signed = false;
        if let Some(k) = opts.sign {
            self.sign(&hash, k)?;
            signed = true;
        }
        Ok(Commit {
            layer: hash,
            stack,
            previous: head,
            dropped_imports: dropped,
            rebased_over,
            signed,
        })
    }

    /// Writes a stack file: `base` (a stack, or none) + `added` layers. `parent` is the head it
    /// replaces, for history.
    fn write_stack(
        &self,
        base: Option<&str>,
        parent: Option<&str>,
        added: &[String],
        author: &str,
    ) -> Result<String> {
        let mut imports = Vec::new();
        if let Some(b) = base {
            imports.push(Import {
                uri: format!("{b}.json"),
                integrity: Some(format!("sha256-{b}")),
            });
        }
        for h in added {
            imports.push(Import {
                uri: format!("../layers/{h}.json"),
                integrity: Some(format!("sha256-{h}")),
            });
        }
        let stack = Layer {
            header: Header {
                id: format!(
                    "stack/{}",
                    added
                        .last()
                        .map(|h| &h[..12.min(h.len())])
                        .unwrap_or("empty")
                ),
                ox_version: OX_VERSION.into(),
                data_version: "1.0.0".into(),
                author: author.into(),
                timestamp: time::now_rfc3339(),
                kind: Some("stack".into()),
                parent: parent.map(str::to_string),
                ..Header::default()
            },
            imports,
            schemas: BTreeMap::new(),
            data: vec![],
        };
        let bytes = layer_bytes(&stack);
        let hash = hash_of(&bytes);
        write_new(&self.stack_path(&hash), &bytes)?;
        Ok(hash)
    }

    fn set_head(&self, stack: &str) -> Result<()> {
        let p = self.dir.join("HEAD");
        let tmp = self.dir.join(format!("HEAD.tmp{}", std::process::id()));
        io(&tmp, fs::write(&tmp, format!("{stack}\n")))?;
        io(&p, fs::rename(&tmp, &p))
    }

    /// Commits every layer of a stack file (imports first), as `ox init --from` does.
    pub fn import_stack(&self, file: &Path) -> Result<Vec<Commit>> {
        let loaded = ox_core::load_stack(file).map_err(|e| StoreError::Key(e.to_string()))?;
        let mut out = Vec::new();
        for l in loaded {
            if l.layer.header.kind.as_deref() == Some("stack") && l.layer.data.is_empty() {
                continue;
            }
            out.push(self.commit(&l.layer, CommitOptions::default())?);
        }
        Ok(out)
    }

    /// Commits, newest first, following `header.parent` from `from` (default: the head).
    pub fn log(&self, from: Option<&str>) -> Result<Vec<StackInfo>> {
        let mut out = Vec::new();
        let mut cur = match from {
            Some(h) => Some(h.to_string()),
            None => self.head()?,
        };
        while let Some(h) = cur {
            let s = self.stack(&h)?;
            cur = s.header.parent.clone();
            out.push(s);
        }
        Ok(out)
    }

    /// Flattens all but the top `keep` layers of the head into one checkpoint layer and commits
    /// a new stack: checkpoint + the kept layers. Composition (values and provenance) is
    /// unchanged; `ox log` and `ox blame` still see the original layers.
    pub fn checkpoint(&self, keep: usize, author: &str) -> Result<Option<Commit>> {
        let _lock = HeadLock::acquire(&self.dir)?;
        let head = self.head()?.ok_or(StoreError::Empty)?;
        let layers = self.layers(&head)?;
        if layers.len() <= keep + 1 {
            return Ok(None);
        }
        let cut = layers.len() - keep;
        let (old, kept) = layers.split_at(cut);
        let mut flat = Flat::default();
        for h in old {
            flat.apply(&self.layer(h)?);
        }
        let header = Header {
            id: format!("checkpoint/{}", &head[..12]),
            ox_version: OX_VERSION.into(),
            data_version: "1.0.0".into(),
            author: author.into(),
            timestamp: time::now_rfc3339(),
            kind: Some("checkpoint".into()),
            replaces: old.to_vec(),
            ..Header::default()
        };
        let (ck, _) = self.put_layer(&flat.to_layer(header))?;
        let mut added = vec![ck.clone()];
        added.extend(kept.iter().cloned());
        let stack = self.write_stack(None, Some(&head), &added, author)?;
        self.set_head(&stack)?;
        drop(_lock);
        Ok(Some(Commit {
            layer: ck,
            stack,
            previous: Some(head),
            dropped_imports: 0,
            rebased_over: vec![],
            signed: false,
        }))
    }
}

/// Creates `path` with `bytes` unless it exists (content-addressed: same name, same bytes).
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    io(&tmp, fs::write(&tmp, bytes))?;
    io(path, fs::rename(&tmp, path))
}

/// `.ox/HEAD.lock`, created exclusively; released on drop. Waits up to two seconds. It holds
/// `<pid> <host>`; a lock left by a process on this machine that no longer runs (a crash or
/// `kill -9`) is removed, so one crash doesn't block every later commit.
struct HeadLock(PathBuf);

impl HeadLock {
    fn acquire(dir: &Path) -> Result<HeadLock> {
        let p = dir.join("HEAD.lock");
        let start = Instant::now();
        loop {
            match fs::OpenOptions::new().write(true).create_new(true).open(&p) {
                Ok(mut f) => {
                    let _ = writeln!(f, "{} {}", std::process::id(), host());
                    return Ok(HeadLock(p));
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let held = fs::read_to_string(&p).unwrap_or_default();
                    if lock_is_stale(&held) {
                        // ponytail: read-check-remove isn't atomic; two processes recovering the
                        // same stale lock in the same instant could both proceed. Rare; an atomic
                        // rename-and-check would close it if it ever matters.
                        let _ = fs::remove_file(&p);
                        continue;
                    }
                    if start.elapsed() > Duration::from_secs(2) {
                        return Err(StoreError::Busy(p, held.trim().to_string()));
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(e) => return Err(StoreError::Io(p, e)),
            }
        }
    }
}

/// This machine's name, so a lock from another machine (a synced folder) is never taken as stale.
fn host() -> String {
    #[cfg(unix)]
    {
        let mut buf = [0u8; 256];
        // SAFETY: gethostname writes at most buf.len() bytes into buf.
        if unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } == 0 {
            let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
            return String::from_utf8_lossy(&buf[..end]).into_owned();
        }
    }
    std::env::var("COMPUTERNAME").unwrap_or_default()
}

/// A lock is stale when it names this machine and a process that isn't running. Anything else
/// (another machine, an unreadable or old-format lock, a live process) is left alone.
fn lock_is_stale(held: &str) -> bool {
    let mut parts = held.split_whitespace();
    let (Some(pid), Some(h)) = (parts.next().and_then(|p| p.parse::<i32>().ok()), parts.next()) else {
        return false;
    };
    if h != host() || pid <= 0 {
        return false;
    }
    !process_alive(pid)
}

#[cfg(unix)]
fn process_alive(pid: i32) -> bool {
    // SAFETY: signal 0 only checks that the process exists; nothing is sent.
    let r = unsafe { libc::kill(pid, 0) };
    r == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

#[cfg(not(unix))]
fn process_alive(_pid: i32) -> bool {
    // ponytail: no liveness check on Windows yet; a stale lock there still needs deleting by hand.
    true
}

impl Drop for HeadLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests;
