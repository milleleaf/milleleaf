//! Detached signatures on layers: Ed25519, plus ML-DSA-65 and SLH-DSA (plan decision 5, 18 Oct 2026).
//!
//! A layer's name is the hash of its unsigned bytes; a signature is a separate file
//! (`.ox/sigs/<layer hash>/<key id>.json`) over that hash, so signing never changes a layer and
//! one layer can carry several signatures (author, reviewer). A person's post-quantum signatures sit
//! beside their Ed25519 one as `<key id>.mldsa65.json` and `<key id>.slhdsa.json` (the key id is the
//! Ed25519 one, naming the person); each says its `alg`. Two independent signatures, not a
//! composite. The project lists the keys it knows in `.ox/keys/<name>.pub` (Ed25519),
//! `<name>.mldsa65` and `<name>.slhdsa`.

use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::pq::{self, ED25519, MLDSA65, SLH};
use crate::{Result, Store, StoreError, hash_of, io, ots, time, tsp};

const CONTEXT: &[u8] = b"Milleleaf layer signature v1\n";

/// Roles a key can have in a project: owner, approver, editor, viewer.
pub const ROLES: [&str; 4] = ["owner", "approver", "editor", "viewer"];
/// Who may purge (plan M1.5 item 7).
pub const MAY_PURGE: [&str; 2] = ["owner", "approver"];
/// Whose signatures also get SLH-DSA, a hash-based algorithm that doesn't share ML-DSA's lattice
/// assumptions; anyone else with `--archive`.
pub const ARCHIVE_ROLES: [&str; 2] = ["owner", "approver"];

/// A person's signing key. Stored at `<keys dir>/<name>.key` (mode 600), never in the project.
#[derive(Debug, Clone)]
pub struct KeyPair {
    pub name: String,
    key: SigningKey,
    /// X-Wing seed for opening sealed layers (plan M1.5 item 7, mode B). `None` in key files made
    /// before sealing existed; such a key signs but can't open.
    xwing: Option<ox_seal::Seed>,
    /// ML-DSA-65 seed; `None` in key files made before 18 Oct 2026 (`ox key upgrade` adds it).
    mldsa: Option<pq::MlSeed>,
    /// SLH-DSA private key (FIPS 205 encoding); `None` likewise.
    slh: Option<Vec<u8>>,
}

#[derive(Serialize, Deserialize)]
struct KeyFile {
    name: String,
    /// base64 Ed25519 seed.
    secret: String,
    public: String,
    /// base64 X-Wing seed (32 bytes); absent in older key files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    xwing: Option<String>,
    /// base64 ML-DSA-65 seed (32 bytes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mldsa65: Option<String>,
    /// `<SLH>:<base64 private key>`, so a key of another parameter set is never misread.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    slhdsa: Option<String>,
}

impl KeyPair {
    pub fn generate(name: &str) -> Result<KeyPair> {
        let mut k = KeyPair { name: name.into(), key: SigningKey::from_bytes(&random32()?), xwing: None, mldsa: None, slh: None };
        k.fill()?;
        Ok(k)
    }

    /// An in-memory demo signing key: Ed25519 and ML-DSA-65, without sealing or archive keys.
    pub fn generate_demo(name: &str) -> Result<KeyPair> {
        Ok(KeyPair {
            name: name.into(), key: SigningKey::from_bytes(&random32()?),
            xwing: None, mldsa: Some(random32()?), slh: None,
        })
    }

    /// Adds whatever this key lacks (X-Wing, ML-DSA, SLH-DSA); keeps what it has. Returns what it added.
    fn fill(&mut self) -> Result<Vec<&'static str>> {
        let mut added = Vec::new();
        if self.xwing.is_none() {
            self.xwing = Some(random32()?);
            added.push("xwing");
        }
        if self.mldsa.is_none() {
            self.mldsa = Some(random32()?);
            added.push(MLDSA65);
        }
        if self.slh.is_none() {
            self.slh = Some(pq::slh_generate().ok_or_else(|| StoreError::Key("no randomness".into()))?);
            added.push(SLH);
        }
        Ok(added)
    }

    /// `mldsa65:<base64>` of the 1952-byte ML-DSA-65 public key, if this key has one.
    pub fn mldsa_public(&self) -> Option<String> {
        self.mldsa.map(|s| format!("{MLDSA65}:{}", B64.encode(pq::mldsa_public(&s))))
    }

    /// `slhdsa-sha2-192s:<base64>` (the parameter set in `pq::SLH`), if this key has one.
    pub fn slh_public(&self) -> Option<String> {
        Some(format!("{SLH}:{}", B64.encode(pq::slh_public(self.slh.as_ref()?)?)))
    }

    /// Every public key this key has: Ed25519 first, then X-Wing, ML-DSA, SLH-DSA.
    pub fn publics(&self) -> Vec<String> {
        [Some(self.public()), self.xwing_public(), self.mldsa_public(), self.slh_public()].into_iter().flatten().collect()
    }

    /// Adds the post-quantum keys an older key file lacks (`ox key upgrade`). Existing seeds are
    /// never replaced; the file keeps mode 600.
    pub fn upgrade(path: &Path) -> Result<(KeyPair, Vec<&'static str>)> {
        let mut k = KeyPair::load(path)?;
        let added = k.fill()?;
        if !added.is_empty() {
            let tmp = path.with_extension("key.tmp");
            k.write(&tmp)?;
            io(path, fs::rename(&tmp, path))?;
        }
        Ok((k, added))
    }

    /// `xwing:<base64>` of the 1216-byte X-Wing public key, if this key can open sealed layers.
    pub fn xwing_public(&self) -> Option<String> {
        self.xwing.map(|s| format!("xwing:{}", B64.encode(ox_seal::public_key(&s))))
    }

    pub(crate) fn xwing_seed(&self) -> Option<&ox_seal::Seed> {
        self.xwing.as_ref()
    }

    /// Every key in a keys folder (unreadable files skipped).
    pub fn all_in(dir: &Path) -> Vec<KeyPair> {
        let mut out: Vec<KeyPair> = fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".key"))
            .filter_map(|e| KeyPair::load(&e.path()).ok())
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// `ed25519:<base64>`.
    pub fn public(&self) -> String {
        public_string(&self.key.verifying_key())
    }

    pub fn id(&self) -> String {
        key_id(&self.key.verifying_key())
    }

    /// Default folder for private keys: `$OX_KEYS`, else `~/.config/ox/keys`.
    pub fn dir() -> PathBuf {
        if let Ok(d) = std::env::var("OX_KEYS") {
            return PathBuf::from(d);
        }
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        PathBuf::from(home).join(".config/ox/keys")
    }

    pub fn save(&self, dir: &Path) -> Result<PathBuf> {
        io(dir, fs::create_dir_all(dir))?;
        let p = dir.join(format!("{}.key", self.name));
        if p.exists() {
            return Err(StoreError::Key(format!(
                "{} already exists; not overwriting a key",
                p.display()
            )));
        }
        self.write(&p)?;
        Ok(p)
    }

    fn write(&self, p: &Path) -> Result<()> {
        let kf = KeyFile {
            name: self.name.clone(),
            secret: B64.encode(self.key.to_bytes()),
            public: self.public(),
            xwing: self.xwing.map(|s| B64.encode(s)),
            mldsa65: self.mldsa.map(|s| B64.encode(s)),
            slhdsa: self.slh.as_ref().map(|s| format!("{SLH}:{}", B64.encode(s))),
        };
        io(
            p,
            fs::write(p, serde_json::to_vec_pretty(&kf).expect("json")),
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            io(
                p,
                fs::set_permissions(p, fs::Permissions::from_mode(0o600)),
            )?;
        }
        Ok(())
    }

    pub fn load(path: &Path) -> Result<KeyPair> {
        let bytes = io(path, fs::read(path))?;
        let kf: KeyFile = serde_json::from_slice(&bytes)
            .map_err(|e| StoreError::Key(format!("{}: {e}", path.display())))?;
        let seed: [u8; 32] = B64
            .decode(&kf.secret)
            .ok()
            .and_then(|v| v.try_into().ok())
            .ok_or_else(|| StoreError::Key(format!("{}: bad secret", path.display())))?;
        let seed32 = |v: &Option<String>, what: &str| -> Result<Option<[u8; 32]>> {
            v.as_ref()
                .map(|x| {
                    B64.decode(x)
                        .ok()
                        .and_then(|v| v.try_into().ok())
                        .ok_or_else(|| StoreError::Key(format!("{}: bad {what} seed", path.display())))
                })
                .transpose()
        };
        let slh = kf
            .slhdsa
            .as_ref()
            .map(|x| {
                x.strip_prefix(SLH)
                    .and_then(|x| x.strip_prefix(':'))
                    .and_then(|x| B64.decode(x).ok())
                    .filter(|b| pq::slh_public(b).is_some())
                    .ok_or_else(|| StoreError::Key(format!("{}: its SLH-DSA key isn't {SLH}", path.display())))
            })
            .transpose()?;
        Ok(KeyPair {
            xwing: seed32(&kf.xwing, "xwing")?,
            mldsa: seed32(&kf.mldsa65, "ML-DSA")?,
            slh,
            name: kf.name,
            key: SigningKey::from_bytes(&seed),
        })
    }

    /// `<keys dir>/<name>.key`.
    pub fn load_named(name: &str) -> Result<KeyPair> {
        KeyPair::load(&KeyPair::dir().join(format!("{name}.key")))
    }
}

fn random32() -> Result<[u8; 32]> {
    ox_seal::random32().map_err(|e| StoreError::Key(e.to_string()))
}

fn public_string(k: &VerifyingKey) -> String {
    format!("ed25519:{}", B64.encode(k.to_bytes()))
}

fn parse_public(s: &str) -> Option<VerifyingKey> {
    let b: [u8; 32] = B64
        .decode(s.trim().strip_prefix("ed25519:")?)
        .ok()?
        .try_into()
        .ok()?;
    VerifyingKey::from_bytes(&b).ok()
}

pub(crate) fn parse_xwing(s: &str) -> Option<Vec<u8>> {
    let b = B64.decode(s.trim().strip_prefix("xwing:")?).ok()?;
    (b.len() == 1216).then_some(b)
}

/// First 16 hex characters of the key's SHA-256: an Ed25519 key's names the signature files.
fn key_id(k: &VerifyingKey) -> String {
    id_of(&k.to_bytes())
}

fn id_of(public: &[u8]) -> String {
    Sha256::digest(public)
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The bytes of an `<alg>:<base64>` public key.
fn decode_public(alg: &str, s: &str) -> Option<Vec<u8>> {
    B64.decode(s.trim().strip_prefix(alg)?.strip_prefix(':')?).ok()
}

/// Checks a signature of `alg` over the layer message. `None`: an algorithm this build doesn't know.
fn check(alg: &str, public: &str, sig: &str, msg: &[u8]) -> Option<(bool, String)> {
    let (Some(key), Ok(sig)) = (decode_public(alg, public), B64.decode(sig)) else {
        return [ED25519, MLDSA65, SLH].contains(&alg).then(|| (false, String::new()));
    };
    let id = id_of(&key);
    let ok = match alg {
        ED25519 => <[u8; 32]>::try_from(key.as_slice())
            .ok()
            .and_then(|b| VerifyingKey::from_bytes(&b).ok())
            .zip(ed25519_dalek::Signature::from_slice(&sig).ok())
            .is_some_and(|(vk, s)| vk.verify(msg, &s).is_ok()),
        MLDSA65 => pq::mldsa_verify(&key, &[], msg, &sig),
        SLH => pq::slh_verify(&key, &[], msg, &sig),
        _ => return None,
    };
    Some((ok, id))
}

/// How strong one signer's valid signatures are together: `hybrid` (Ed25519 + ML-DSA),
/// `hybrid+slh-dsa`, `classical only` (Ed25519 alone), else the algorithms listed.
pub fn strength(algs: &[&str]) -> String {
    let has = |a| algs.contains(&a);
    match (has(ED25519), has(MLDSA65), has(SLH)) {
        (true, true, true) => "hybrid+slh-dsa".into(),
        (true, true, false) => "hybrid".into(),
        (true, false, false) if algs.len() == 1 => "classical only".into(),
        _ => algs.join(" + "),
    }
}

fn message(layer_hash: &str) -> Vec<u8> {
    let mut m = CONTEXT.to_vec();
    m.extend_from_slice(format!("sha256-{layer_hash}").as_bytes());
    m
}

/// A signature file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signature {
    /// `mldsa65` or `slhdsa-sha2-192s`; absent means Ed25519 (every file before 18 Oct 2026).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alg: Option<String>,
    /// `sha256-<hex>` of the layer signed.
    pub layer: String,
    /// `<alg>:<base64>` public key.
    pub key: String,
    /// The name the signer gave; trusted only if the project knows the key.
    pub signer: String,
    pub signature: String,
    pub at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SigInfo {
    /// Name from `.ox/keys` when the project knows the key; otherwise `None`.
    pub known_as: Option<String>,
    pub claimed: String,
    pub key_id: String,
    pub alg: String,
    pub valid: bool,
    /// The earliest RFC 3161 time (`<sigfile>.tsr`, `.2.tsr`, …) of tokens that stamp this file;
    /// TSA signatures not checked.
    pub timestamp: Option<String>,
    /// What `<sigfile>.ots` (OpenTimestamps) says, offline: pending calendars and Bitcoin blocks.
    pub ots: Option<ots::Summary>,
    /// Why it isn't valid, when that's more than a failed signature check.
    pub problem: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum LayerStatus {
    /// Its bytes were purged; the hash and the purge record remain.
    Purged {
        /// `bytes`, or `key` (a sealed layer whose content key was destroyed).
        mode: String,
        by: String,
        at: String,
        reason: Option<String>,
    },
    /// Content no longer matches its hash.
    Tampered {
        actual: String,
    },
    /// At least one signature doesn't verify.
    Invalid {
        signatures: Vec<SigInfo>,
    },
    Signed {
        signatures: Vec<SigInfo>,
    },
    Unsigned,
}

/// Detached signatures over a layer hash, without a store. `at` is the claimed signing time.
/// Ed25519 and ML-DSA-65 are always used when present; `slh` also uses the archive key.
pub fn sign_hash(layer_hash: &str, key: &KeyPair, slh: bool, at: &str) -> Result<Vec<Signature>> {
    let msg = message(layer_hash);
    let mut sigs = vec![(None, key.public(), key.key.sign(&msg).to_bytes().to_vec())];
    if let (Some(seed), Some(public)) = (&key.mldsa, key.mldsa_public()) {
        // Hedged signing (FIPS 204 recommends it): fresh randomness per signature.
        let sig = pq::mldsa_sign(pq::mldsa_key(seed).expanded_key(), &[], &msg, &random32()?).expect("empty context");
        sigs.push((Some(MLDSA65), public, sig));
    }
    if let (true, Some(sk), Some(public)) = (slh, &key.slh, key.slh_public()) {
        let mut rand = vec![0u8; pq::slh_n()];
        getrandom::getrandom(&mut rand).map_err(|e| StoreError::Key(e.to_string()))?;
        let sig = pq::slh_sign(sk, &[], &msg, Some(&rand)).ok_or_else(|| StoreError::Key("SLH-DSA signing failed".into()))?;
        sigs.push((Some(SLH), public, sig));
    }
    Ok(sigs.into_iter().map(|(alg, public, sig)| Signature {
        alg: alg.map(String::from), layer: format!("sha256-{layer_hash}"), key: public,
        signer: key.name.clone(), signature: B64.encode(sig), at: at.into(),
    }).collect())
}

/// Checks one detached signature, without time-stamp files or a store. `known` lists trusted
/// (name, public key) pairs; a valid signature from an unlisted key remains untrusted.
pub fn check_signature(s: &Signature, layer_hash: &str, known: &[(String, String)]) -> SigInfo {
    let alg = s.alg.clone().unwrap_or_else(|| ED25519.into());
    let mut info = SigInfo {
        known_as: None, claimed: s.signer.clone(), key_id: String::new(), alg: alg.clone(),
        valid: false, timestamp: None, ots: None, problem: None,
    };
    match check(&alg, &s.key, &s.signature, &message(layer_hash)) {
        None => info.problem = Some(format!("unknown algorithm `{alg}`; not accepted")),
        Some((ok, id)) => {
            info.valid = ok && s.layer == format!("sha256-{layer_hash}");
            info.key_id = id;
            info.known_as = known.iter().find(|(_, k)| *k == s.key.trim()).map(|(n, _)| n.clone());
        }
    }
    info
}

impl Store {
    /// Adds a public key to the project (`.ox/keys/<name>.pub`).
    pub fn add_key(&self, name: &str, public: &str) -> Result<PathBuf> {
        if parse_public(public).is_none() {
            return Err(StoreError::Key(format!(
                "not an ed25519 public key: {public}"
            )));
        }
        let d = self.dir.join("keys");
        io(&d, fs::create_dir_all(&d))?;
        let p = d.join(format!("{name}.pub"));
        io(&p, fs::write(&p, format!("{}\n", public.trim())))?;
        Ok(p)
    }

    /// Adds a person's X-Wing public key (`.ox/keys/<name>.xwing`), so layers can be sealed to them.
    pub fn add_xwing_key(&self, name: &str, public: &str) -> Result<PathBuf> {
        if parse_xwing(public).is_none() {
            return Err(StoreError::Key(format!("not an X-Wing public key (xwing:<base64 of 1216 bytes>): {}…", &public[..public.len().min(24)])));
        }
        let d = self.dir.join("keys");
        io(&d, fs::create_dir_all(&d))?;
        let p = d.join(format!("{name}.xwing"));
        io(&p, fs::write(&p, format!("{}\n", public.trim())))?;
        Ok(p)
    }

    /// A known person's X-Wing public key bytes.
    pub fn xwing_key(&self, name: &str) -> Option<Vec<u8>> {
        parse_xwing(&fs::read_to_string(self.dir.join("keys").join(format!("{name}.xwing"))).ok()?)
    }

    /// Sets a known key's role: `owner`, `approver`, `editor` or `viewer` (plan M4 item 5).
    pub fn set_role(&self, name: &str, role: &str) -> Result<()> {
        if !ROLES.contains(&role) {
            return Err(StoreError::Key(format!("role must be one of {}, not `{role}`", ROLES.join(", "))));
        }
        if !self.known_keys().iter().any(|(n, _)| n == name) {
            return Err(StoreError::Key(format!("no key named `{name}` in this project (`ox key add` first)")));
        }
        let p = self.dir.join("keys").join(format!("{name}.role"));
        io(&p, fs::write(&p, format!("{role}\n")))
    }

    /// A known key's role; `editor` when none is set.
    pub fn role(&self, name: &str) -> String {
        fs::read_to_string(self.dir.join("keys").join(format!("{name}.role")))
            .map(|r| r.trim().to_string())
            .unwrap_or_else(|_| "editor".into())
    }

    /// Whether a layer carries a valid signature by a key this project lists with one of `roles`.
    pub fn signed_by_role(&self, layer_hash: &str, roles: &[&str]) -> Result<bool> {
        Ok(match self.verify(layer_hash)? {
            LayerStatus::Signed { signatures } => signatures
                .iter()
                .any(|g| g.valid && g.known_as.as_ref().is_some_and(|n| roles.contains(&self.role(n).as_str()))),
            _ => false,
        })
    }

    /// Adds a person's ML-DSA-65 (`mldsa65:…`) or SLH-DSA (`slhdsa-…:…`) public key
    /// (`.ox/keys/<name>.mldsa65` / `.slhdsa`).
    pub fn add_pq_key(&self, name: &str, public: &str) -> Result<PathBuf> {
        let ext = match public.split(':').next() {
            Some(MLDSA65) if decode_public(MLDSA65, public).is_some_and(|b| b.len() == 1952) => "mldsa65",
            Some(SLH) if decode_public(SLH, public).is_some_and(|b| b.len() == 2 * pq::slh_n()) => "slhdsa",
            _ => {
                return Err(StoreError::Key(format!(
                    "not an ML-DSA-65 ({MLDSA65}:…) or SLH-DSA ({SLH}:…) public key: {}…",
                    &public[..public.len().min(24)]
                )));
            }
        };
        let d = self.dir.join("keys");
        io(&d, fs::create_dir_all(&d))?;
        let p = d.join(format!("{name}.{ext}"));
        io(&p, fs::write(&p, format!("{}\n", public.trim())))?;
        Ok(p)
    }

    /// Public keys the project knows: (name, `ed25519:…`).
    pub fn known_keys(&self) -> Vec<(String, String)> {
        self.known("pub")
    }

    /// Public keys of one kind (`.ox/keys/<name>.<ext>`): (name, key).
    pub(crate) fn known(&self, ext: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        if let Ok(rd) = fs::read_dir(self.dir.join("keys")) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                if let Some(name) = n.strip_suffix(&format!(".{ext}"))
                    && let Ok(s) = fs::read_to_string(e.path())
                {
                    out.push((name.to_string(), s.trim().to_string()));
                }
            }
        }
        out.sort();
        out
    }

    /// Signs a stored layer: `.ox/sigs/<hash>/<key id>.json` (Ed25519), `<key id>.mldsa65.json`
    /// when the key has ML-DSA, and `<key id>.slhdsa.json` when it has SLH-DSA and either `archive`
    /// is set or the project lists the signer as owner or approver. Returns the files written.
    pub fn sign(&self, layer_hash: &str, key: &KeyPair, archive: bool) -> Result<Vec<PathBuf>> {
        let bytes = {
            let p = self.dir.join("layers").join(format!("{layer_hash}.json"));
            io(&p, fs::read(&p))?
        };
        let actual = hash_of(&bytes);
        if actual != layer_hash {
            return Err(StoreError::Tampered {
                hash: layer_hash.into(),
                actual,
            });
        }
        let role = self.known_keys().into_iter().find(|(_, k)| *k == key.public()).map(|(n, _)| self.role(&n));
        let slh = archive || role.is_some_and(|r| ARCHIVE_ROLES.contains(&r.as_str()));
        let sigs = sign_hash(layer_hash, key, slh, &time::now_rfc3339())?;
        let d = self.dir.join("sigs").join(layer_hash);
        io(&d, fs::create_dir_all(&d))?;
        let mut out = Vec::new();
        for s in sigs {
            let p = d.join(match s.alg.as_deref() {
                None => format!("{}.json", key.id()),
                Some(SLH) => format!("{}.slhdsa.json", key.id()),
                Some(a) => format!("{}.{a}.json", key.id()),
            });
            io(&p, fs::write(&p, serde_json::to_vec_pretty(&s).expect("json")))?;
            // A time-stamp of the file it replaced no longer matches.
            for t in tsr_paths(&p).into_iter().chain([ots_of(&p)]) {
                let _ = fs::remove_file(t);
            }
            out.push(p);
        }
        Ok(out)
    }

    /// Checks one stored layer: its hash, then every signature on it (and any time-stamp beside it).
    pub fn verify(&self, layer_hash: &str) -> Result<LayerStatus> {
        let p = self.dir.join("layers").join(format!("{layer_hash}.json"));
        // Mode A removes the file; mode B keeps the (now unreadable) layer.
        if let Some(g) = self.purged(layer_hash)
            && (!p.exists() || g.mode == "key")
        {
            return Ok(LayerStatus::Purged { mode: g.mode, by: g.by, at: g.at, reason: g.reason });
        }
        let bytes = io(&p, fs::read(&p))?;
        let actual = hash_of(&bytes);
        if actual != layer_hash {
            return Ok(LayerStatus::Tampered { actual });
        }
        let known: Vec<(String, String)> = [(ED25519, "pub"), (MLDSA65, "mldsa65"), (SLH, "slhdsa")]
            .into_iter().flat_map(|(alg, ext)| self.known(ext).into_iter()
                .filter(move |(_, key)| key.starts_with(&format!("{alg}:")))).collect();
        let mut sigs = Vec::new();
        for f in self.signature_files(layer_hash) {
            let raw = fs::read(&f).unwrap_or_default();
            let Ok(s) = serde_json::from_slice::<Signature>(&raw) else {
                sigs.push(SigInfo {
                    known_as: None,
                    claimed: f.display().to_string(),
                    key_id: String::new(),
                    alg: String::new(),
                    valid: false,
                    timestamp: None,
                    ots: None,
                    problem: Some("not a signature file".into()),
                });
                continue;
            };
            let mut info = check_signature(&s, layer_hash, &known);
            for t in tsr_paths(&f) {
                match fs::read(&t).map_err(|e| e.to_string()).and_then(|token| tsp::check(&token, &raw)) {
                    Ok(stamp) => info.timestamp = info.timestamp.take().into_iter().chain([stamp.at]).min(),
                    Err(why) => {
                        info.valid = false;
                        info.problem = Some(format!("time-stamp: {why}"));
                    }
                }
            }
            if let Ok(proof) = fs::read(ots_of(&f)) {
                match ots::summarize(&raw, &proof) {
                    Ok(s) => info.ots = Some(s),
                    Err(why) => {
                        info.valid = false;
                        info.problem = Some(format!("OpenTimestamps proof: {why}"));
                    }
                }
            }
            sigs.push(info);
        }
        Ok(if sigs.is_empty() {
            LayerStatus::Unsigned
        } else if sigs.iter().any(|s| !s.valid) {
            LayerStatus::Invalid { signatures: sigs }
        } else {
            LayerStatus::Signed { signatures: sigs }
        })
    }

    /// The signature files of a layer (`.ox/sigs/<hash>/*.json`).
    pub fn signature_files(&self, layer_hash: &str) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = fs::read_dir(self.dir.join("sigs").join(layer_hash))
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|f| f.extension().is_some_and(|x| x == "json"))
            .collect();
        files.sort();
        files
    }
}

/// Where an RFC 3161 time-stamp of a signature file is kept: `<file>.tsr` for the first TSA,
/// `<file>.<n>.tsr` for the n-th (n from 2).
pub fn tsr_of(sigfile: &Path, n: usize) -> PathBuf {
    let mut s = sigfile.as_os_str().to_owned();
    s.push(if n <= 1 { ".tsr".to_string() } else { format!(".{n}.tsr") });
    PathBuf::from(s)
}

/// The RFC 3161 time-stamps kept beside a signature file, in order (they are numbered without gaps).
pub fn tsr_paths(sigfile: &Path) -> Vec<PathBuf> {
    (1..).map(|n| tsr_of(sigfile, n)).take_while(|t| t.exists()).collect()
}

/// Where the OpenTimestamps proof of a signature file is kept: `<file>.ots` (one for all calendars).
pub fn ots_of(sigfile: &Path) -> PathBuf {
    let mut s = sigfile.as_os_str().to_owned();
    s.push(".ots");
    PathBuf::from(s)
}
