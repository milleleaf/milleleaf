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
//!   sigs/<sha256>/<key>.json  detached signatures (optional): Ed25519; `<key>.mldsa65.json` and
//!                             `<key>.slhdsa.json` beside it; `<file>.tsr` (`.2.tsr`, …) RFC 3161
//!                             time-stamps, `<file>.ots` an OpenTimestamps proof
//!   keys/<name>.pub           public keys the project knows (`.xwing`: X-Wing keys, `.mldsa65`,
//!                             `.slhdsa`: post-quantum signing keys, `.role`: roles)
//!   policy.json               which signatures `ox verify --require` requires; time-stamp
//!                             services and Bitcoin header sources (optional)
//!   blobs/<sha256>            assets, and the ciphertext of sealed layers
//!   wraps/<key id>/<name>.json  a sealed layer's content key wrapped to one recipient (deletable)
//!   purged/<sha256>.json      purges carried out here
//!   cache/                    derived, safe to delete (flattened stacks; search/<sha256>.json:
//!                             a layer's search index, sealed layers' encrypted to their content key)
//! ```
//!
//! Files are only ever added (except `HEAD` and the cache), so the folder works under git and
//! in synced folders. `HEAD` moves by compare-and-swap: a commit names the head it was made
//! against and is refused if the head has moved since.

mod history;
mod keys;
pub mod policy;
pub mod pq;
mod search;
mod seal;
mod time;
pub mod ots;
pub mod headers;
pub mod tsp;

pub use history::{BlameEntry, Touch};
pub use keys::{check_signature, sign_hash, KeyPair, LayerStatus, SigInfo, Signature, ots_of, strength, tsr_of, tsr_paths};
pub use policy::Policy;
pub use time::{now_rfc3339, rfc3339};
pub use seal::{Opened, SealInfo};
pub use search::{Found, Hit};

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
    #[error("asset {hash} is damaged: its bytes hash to {actual} (the picture was changed or replaced)")]
    BlobTampered { hash: String, actual: String },
    #[error("not allowed: {0}")]
    NotAllowed(String),
    #[error("asset not found: {0}")]
    MissingAsset(String),
    #[error("blob {0} isn't in this project's .ox/blobs")]
    MissingBlob(String),
    #[error("a layer must be I-JSON so its canonical form is exact: {0}")]
    NotIJson(String),
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

/// Bytes a layer is stored as: its RFC 8785 (JCS) canonical JSON. Its name is the SHA-256 of these
/// bytes, so a byte-for-byte copy keeps the name, and a program that only has the parsed JSON
/// (a re-saved file, a database, a browser store) gets the same bytes back by canonicalising it.
/// Signatures are kept apart, so signing never changes it.
pub fn layer_bytes(layer: &Layer) -> Vec<u8> {
    serde_json_canonicalizer::to_vec(layer).expect("layers serialize")
}

/// Largest integer every JSON implementation reads exactly (I-JSON, RFC 7493).
const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

/// A number canonical JSON can't carry exactly (an integer beyond ±2^53), by its path in the layer.
fn not_ijson(v: &serde_json::Value, at: &str) -> Option<String> {
    use serde_json::Value;
    match v {
        Value::Number(n) => {
            let too_big = n.as_u64().is_some_and(|u| u > MAX_SAFE_INTEGER)
                || n.as_i64().is_some_and(|i| i.unsigned_abs() > MAX_SAFE_INTEGER);
            too_big.then(|| format!("{at}: {n} is beyond ±2^53; store it as a string"))
        }
        Value::Array(a) => a.iter().enumerate().find_map(|(i, x)| not_ijson(x, &format!("{at}[{i}]"))),
        Value::Object(m) => m.iter().find_map(|(k, x)| not_ijson(x, &format!("{at}.{k}"))),
        _ => None,
    }
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
    /// Sign with SLH-DSA too, whatever the signer's role (`--archive`).
    pub archive: bool,
    /// Where `{"blob": path}` values are read from before they're stored by hash; defaults to the
    /// project root (a stack file's folder for `import_stack`).
    pub assets: Option<&'a Path>,
    /// Seal the layer to these people (names with an X-Wing key in the project's key list): its
    /// content is stored encrypted and only they can compose it (plan M1.5 item 7, mode B).
    pub seal_to: &'a [String],
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
    /// Where this person's private keys are looked for when opening sealed layers
    /// (`KeyPair::dir()` unless set).
    pub keys: PathBuf,
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
            keys: KeyPair::dir(),
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
            keys: KeyPair::dir(),
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
        if let Some(why) = not_ijson(&serde_json::to_value(&layer).expect("json"), "layer") {
            return Err(StoreError::NotIJson(why));
        }
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

    /// The layer stored under `hash`, checked against its hash. A purged layer comes back empty,
    /// with the header its index kept.
    pub fn layer(&self, hash: &str) -> Result<Layer> {
        let p = self.layer_path(hash);
        if !p.exists()
            && let Some(purge) = self.purged(hash)
        {
            let mut header = self.index(hash).map(|i| i.header).unwrap_or_default();
            header.purges = vec![purge];
            return Ok(Layer { header, imports: vec![], schemas: Default::default(), data: vec![] });
        }
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
    /// Sealed layers are opened in memory when a local key can, else come back empty.
    pub fn load(&self, stack: &str) -> Result<Vec<LoadedLayer>> {
        self.layers(stack)?
            .into_iter()
            .map(|h| {
                Ok(LoadedLayer {
                    source: self.layer_path(&h),
                    layer: self.opened(&h)?.0,
                })
            })
            .collect()
    }

    /// `open_layer`, saying on stderr when a sealed layer stays closed.
    fn opened(&self, hash: &str) -> Result<(Layer, Opened)> {
        let (l, o) = self.open_layer(hash)?;
        if let Opened::Locked(why) = &o {
            // ponytail: printed from the library; return notices to callers if a GUI needs them.
            eprintln!("note: layer {} ({}) is sealed and {why}; composed as empty", l.header.id, &hash[..12.min(hash.len())]);
        }
        Ok((l, o))
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
        // A flat that includes a sealed layer is never cached: decrypted content stays in memory,
        // and one composed without it would hide it from a recipient later. So no stack above a
        // sealed layer has a cache, and the walk above always reaches back past it.
        let mut sealed = false;
        for added in pending.into_iter().rev() {
            for h in &added {
                let (l, o) = self.opened(h)?;
                sealed |= o != Opened::Plain;
                flat.apply(&l);
            }
        }
        if !sealed {
            self.save_flat(stack, &flat);
        }
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
        let mut layer = if opts.seal_to.is_empty() { layer.clone() } else { self.seal(layer, opts.seal_to)? };
        self.intern_blobs(&mut layer, opts.assets.unwrap_or(&self.root))?;
        let layer = &layer;
        let (hash, dropped, new) = self.put_layer_new(layer)?;
        // A purge record only counts when an owner or approver signed it, here or where it came from.
        if !layer.header.purges.is_empty() {
            if let Some(k) = opts.sign {
                self.sign(&hash, k, opts.archive)?;
            }
            if !self.signed_by_role(&hash, &keys::MAY_PURGE)? {
                return Err(StoreError::NotAllowed(format!(
                    "a purge must be signed by a key this project lists as owner or approver; {} isn't",
                    &hash[..12]
                )));
            }
        }
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
            && layer.header.sealed.is_none()
            && let Some(mut flat) = self.cached_flat(prev)
        {
            flat.apply(&self.layer(&hash)?);
            self.save_flat(&stack, &flat);
            let _ = fs::remove_file(self.cache_path(prev));
        }
        let mut signed = false;
        if let Some(k) = opts.sign {
            self.sign(&hash, k, opts.archive)?;
            signed = true;
        }
        // A purge record, made here or received from another copy of the project: carry it out.
        self.apply_purges(layer)?;
        // A sealed layer arriving after its key was destroyed here: wraps that came with it go too.
        if layer.header.sealed.is_some() && self.purged(&hash).is_some() {
            self.destroy_key(&hash)?;
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

    /// Commits every layer of a stack file (imports first), as `ox init --from` does. Assets the
    /// layers name by path are read from beside the layer file and stored by hash.
    pub fn import_stack(&self, file: &Path) -> Result<Vec<Commit>> {
        let loaded = ox_core::load_stack(file).map_err(|e| StoreError::Key(e.to_string()))?;
        let mut out = Vec::new();
        for l in loaded {
            if l.layer.header.kind.as_deref() == Some("stack") && l.layer.data.is_empty() {
                continue;
            }
            let from = l.source.parent().map(Path::to_path_buf).unwrap_or_default();
            let opts = CommitOptions { assets: Some(&from), ..CommitOptions::default() };
            out.push(self.commit(&l.layer, opts)?);
        }
        Ok(out)
    }

    /// The purge that removed a layer's bytes from this store, if one did.
    pub fn purged(&self, hash: &str) -> Option<ox_core::Purge> {
        let bytes = fs::read(self.dir.join("purged").join(format!("{hash}.json"))).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Purges a layer's content (mode A, `bytes`): commits a purge record signed with `key`, which
    /// must be a project owner or approver, then deletes the layer's bytes here, keeping its hash and
    /// index entry. Every store the record (and its signature) reaches does the same.
    pub fn purge(&self, hash: &str, key: &KeyPair, reason: Option<&str>) -> Result<Commit> {
        self.purge_mode(hash, key, reason, "bytes")
    }

    /// `purge` with a mode: `bytes` (A), or `key` (B, crypto-shredding: a sealed layer's content
    /// key is destroyed, its wraps deleted here and in every store the record reaches; the
    /// ciphertext and the layer stay, unreadable).
    pub fn purge_mode(&self, hash: &str, key: &KeyPair, reason: Option<&str>, mode: &str) -> Result<Commit> {
        let by = key.name.as_str();
        let hash = self.resolve(hash)?;
        if self.stack_path(&hash).exists() {
            return Err(StoreError::Key(format!("{hash} is a commit, not a layer; purge the layer")));
        }
        match mode {
            "bytes" => {}
            "key" if self.index(&hash)?.header.sealed.is_some() => {}
            "key" => return Err(StoreError::Key(format!("{} isn't sealed, so it has no key to destroy; purge its bytes", &hash[..12]))),
            m => return Err(StoreError::Key(format!("purge mode must be `bytes` or `key`, not `{m}`"))),
        }
        let record = Layer {
            header: Header {
                id: format!("purge/{}", &hash[..12]),
                ox_version: OX_VERSION.into(),
                data_version: "1.0.0".into(),
                author: by.into(),
                timestamp: time::now_rfc3339(),
                kind: Some("purge".into()),
                purges: vec![ox_core::Purge {
                    layer: hash.clone(),
                    mode: mode.into(),
                    by: by.into(),
                    at: time::now_rfc3339(),
                    reason: reason.map(str::to_string),
                }],
                ..Header::default()
            },
            imports: vec![],
            schemas: Default::default(),
            data: vec![],
        };
        self.commit(&record, CommitOptions { sign: Some(key), ..CommitOptions::default() })
    }

    /// Carries out the purges a committed layer lists: bytes deleted, a marker kept, caches dropped
    /// (they hold composed content).
    fn apply_purges(&self, layer: &Layer) -> Result<()> {
        for p in &layer.header.purges {
            if p.mode != "bytes" && p.mode != "key" {
                continue;
            }
            // The layer's hash names a file; anything else in a received record is ignored.
            if p.layer.len() != 64 || !p.layer.bytes().all(|b| b.is_ascii_hexdigit()) {
                continue;
            }
            let d = self.dir.join("purged");
            io(&d, fs::create_dir_all(&d))?;
            let marker = d.join(format!("{}.json", p.layer));
            if !marker.exists() {
                write_new(&marker, &serde_json::to_vec_pretty(p).expect("json"))?;
            }
            // Either way the key goes; a `bytes` purge also removes the layer and its ciphertext.
            self.destroy_key(&p.layer)?;
            if p.mode == "bytes" {
                self.remove_ciphertext(&p.layer)?;
                let lp = self.layer_path(&p.layer);
                if lp.exists() {
                    io(&lp, fs::remove_file(&lp))?;
                }
            }
            let _ = fs::remove_dir_all(self.dir.join("cache"));
        }
        Ok(())
    }

    fn blob_path(&self, hex: &str) -> PathBuf {
        self.dir.join("blobs").join(hex)
    }

    /// Stores `bytes` under their hash (once) and returns the blob value, `sha256-<hex>`.
    pub fn put_blob(&self, bytes: &[u8]) -> Result<String> {
        let hex = hash_of(bytes);
        let p = self.blob_path(&hex);
        if !p.exists() {
            let d = self.dir.join("blobs");
            io(&d, fs::create_dir_all(&d))?;
            write_new(&p, bytes)?;
        }
        Ok(format!("sha256-{hex}"))
    }

    /// The bytes of a stored blob, checked against its hash.
    pub fn blob(&self, value: &str) -> Result<Vec<u8>> {
        let hex = value.strip_prefix("sha256-").unwrap_or(value);
        let p = self.blob_path(hex);
        let bytes = fs::read(&p).map_err(|_| StoreError::MissingBlob(value.to_string()))?;
        let actual = hash_of(&bytes);
        if actual != hex {
            return Err(StoreError::BlobTampered { hash: hex.into(), actual });
        }
        Ok(bytes)
    }

    /// Every `{"blob": …}` value in the layer, stored by hash: a path is read from `from`, stored,
    /// and replaced by its digest (with a `mediaType`); a digest must already be in the store.
    /// So the layer's own hash covers the bytes it shows.
    fn intern_blobs(&self, layer: &mut Layer, from: &Path) -> Result<()> {
        fn walk(store: &Store, v: &mut serde_json::Value, from: &Path) -> Result<()> {
            use serde_json::Value;
            match v {
                Value::Object(m) => {
                    if let Some(Value::String(b)) = m.get("blob").cloned() {
                        if b.starts_with("sha256-") {
                            store.blob(&b)?;
                        } else {
                            let rel = Path::new(&b);
                            if rel.is_absolute() || rel.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
                                return Err(StoreError::MissingAsset(format!("{b}: outside the project folder")));
                            }
                            let bytes = fs::read(from.join(rel))
                                .or_else(|_| fs::read(store.root.join(rel)))
                                .map_err(|e| StoreError::MissingAsset(format!("{b} (looked in {} and {}): {e}", from.display(), store.root.display())))?;
                            let digest = store.put_blob(&bytes)?;
                            m.insert("blob".into(), Value::String(digest));
                            m.entry("mediaType").or_insert_with(|| Value::String(ox_core::media_type_for(&b).into()));
                        }
                    }
                    for x in m.values_mut() {
                        walk(store, x, from)?;
                    }
                }
                Value::Array(a) => {
                    for x in a.iter_mut() {
                        walk(store, x, from)?;
                    }
                }
                _ => {}
            }
            Ok(())
        }
        for n in &mut layer.data {
            for v in n.attributes.values_mut() {
                walk(self, v, from)?;
            }
        }
        Ok(())
    }

    /// Blobs the head's layers name that are missing or don't match their hash.
    pub fn verify_blobs(&self) -> Result<Vec<String>> {
        fn digests(v: &serde_json::Value, out: &mut Vec<String>) {
            match v {
                serde_json::Value::Object(m) => {
                    if let Some(serde_json::Value::String(b)) = m.get("blob")
                        && b.starts_with("sha256-")
                    {
                        out.push(b.clone());
                    }
                    m.values().for_each(|x| digests(x, out));
                }
                serde_json::Value::Array(a) => a.iter().for_each(|x| digests(x, out)),
                _ => {}
            }
        }
        let mut all = Vec::new();
        for h in self.head_layers()? {
            for n in self.layer(&h)?.data {
                n.attributes.values().for_each(|v| digests(v, &mut all));
            }
        }
        all.sort();
        all.dedup();
        Ok(all
            .into_iter()
            .filter_map(|b| match self.blob(&b) {
                Ok(_) => None,
                Err(e) => Some(format!("{b}: {e}")),
            })
            .collect())
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
        // Never fold a sealed layer into a checkpoint: that would either write its plaintext into
        // an unsealed layer or drop it for its recipients. Fold only what lies below the first one.
        let first_sealed = layers.iter().position(|h| self.index(h).is_ok_and(|i| i.header.sealed.is_some()));
        let cut = first_sealed.map_or(layers.len() - keep, |i| i.min(layers.len() - keep));
        if cut < 2 {
            return Ok(None);
        }
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
