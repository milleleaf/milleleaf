//! Detached Ed25519 signatures on layers.
//!
//! A layer's name is the hash of its unsigned bytes; a signature is a separate file
//! (`.ox/sigs/<layer hash>/<key id>.json`) over that hash, so signing never changes a layer and
//! one layer can carry several signatures (author, reviewer). The project lists the keys it
//! knows in `.ox/keys/<name>.pub`.

use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{Result, Store, StoreError, hash_of, io, time};

const CONTEXT: &[u8] = b"Milleleaf layer signature v1\n";

/// Roles a key can have in a project: owner, approver, editor, viewer.
pub const ROLES: [&str; 4] = ["owner", "approver", "editor", "viewer"];
/// Who may purge (plan M1.5 item 7).
pub const MAY_PURGE: [&str; 2] = ["owner", "approver"];

/// A person's signing key. Stored at `<keys dir>/<name>.key` (mode 600), never in the project.
#[derive(Debug, Clone)]
pub struct KeyPair {
    pub name: String,
    key: SigningKey,
    /// X-Wing seed for opening sealed layers (plan M1.5 item 7, mode B). `None` in key files made
    /// before sealing existed; such a key signs but can't open.
    xwing: Option<ox_seal::Seed>,
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
}

impl KeyPair {
    pub fn generate(name: &str) -> Result<KeyPair> {
        let mut seed = [0u8; 32];
        getrandom::getrandom(&mut seed).map_err(|e| StoreError::Key(e.to_string()))?;
        Ok(KeyPair {
            name: name.into(),
            key: SigningKey::from_bytes(&seed),
            xwing: Some(ox_seal::random32().map_err(|e| StoreError::Key(e.to_string()))?),
        })
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
        let kf = KeyFile {
            name: self.name.clone(),
            secret: B64.encode(self.key.to_bytes()),
            public: self.public(),
            xwing: self.xwing.map(|s| B64.encode(s)),
        };
        io(
            &p,
            fs::write(&p, serde_json::to_vec_pretty(&kf).expect("json")),
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            io(
                &p,
                fs::set_permissions(&p, fs::Permissions::from_mode(0o600)),
            )?;
        }
        Ok(p)
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
        let xwing = match &kf.xwing {
            None => None,
            Some(x) => Some(
                B64.decode(x)
                    .ok()
                    .and_then(|v| v.try_into().ok())
                    .ok_or_else(|| StoreError::Key(format!("{}: bad xwing seed", path.display())))?,
            ),
        };
        Ok(KeyPair {
            name: kf.name,
            key: SigningKey::from_bytes(&seed),
            xwing,
        })
    }

    /// `<keys dir>/<name>.key`.
    pub fn load_named(name: &str) -> Result<KeyPair> {
        KeyPair::load(&KeyPair::dir().join(format!("{name}.key")))
    }
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

/// First 16 hex characters of the key's SHA-256: names the signature file.
fn key_id(k: &VerifyingKey) -> String {
    Sha256::digest(k.to_bytes())
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn message(layer_hash: &str) -> Vec<u8> {
    let mut m = CONTEXT.to_vec();
    m.extend_from_slice(format!("sha256-{layer_hash}").as_bytes());
    m
}

/// A signature file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signature {
    /// `sha256-<hex>` of the layer signed.
    pub layer: String,
    /// `ed25519:<base64>` public key.
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
    pub valid: bool,
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

    /// Public keys the project knows: (name, `ed25519:…`).
    pub fn known_keys(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        if let Ok(rd) = fs::read_dir(self.dir.join("keys")) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                if let Some(name) = n.strip_suffix(".pub")
                    && let Ok(s) = fs::read_to_string(e.path())
                {
                    out.push((name.to_string(), s.trim().to_string()));
                }
            }
        }
        out.sort();
        out
    }

    /// Signs a stored layer; writes `.ox/sigs/<hash>/<key id>.json`.
    pub fn sign(&self, layer_hash: &str, key: &KeyPair) -> Result<PathBuf> {
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
        let sig = key.key.sign(&message(layer_hash));
        let s = Signature {
            layer: format!("sha256-{layer_hash}"),
            key: key.public(),
            signer: key.name.clone(),
            signature: B64.encode(sig.to_bytes()),
            at: time::now_rfc3339(),
        };
        let d = self.dir.join("sigs").join(layer_hash);
        io(&d, fs::create_dir_all(&d))?;
        let p = d.join(format!("{}.json", key.id()));
        io(
            &p,
            fs::write(&p, serde_json::to_vec_pretty(&s).expect("json")),
        )?;
        Ok(p)
    }

    /// Checks one stored layer: its hash, then every signature on it.
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
        let known = self.known_keys();
        let mut sigs = Vec::new();
        if let Ok(rd) = fs::read_dir(self.dir.join("sigs").join(layer_hash)) {
            let mut files: Vec<_> = rd.flatten().map(|e| e.path()).collect();
            files.sort();
            for f in files {
                let Ok(s) = fs::read(&f)
                    .map_err(|_| ())
                    .and_then(|b| serde_json::from_slice::<Signature>(&b).map_err(|_| ()))
                else {
                    sigs.push(SigInfo {
                        known_as: None,
                        claimed: f.display().to_string(),
                        key_id: String::new(),
                        valid: false,
                    });
                    continue;
                };
                let vk = parse_public(&s.key);
                let valid = s.layer == format!("sha256-{layer_hash}")
                    && vk.is_some_and(|vk| {
                        B64.decode(&s.signature)
                            .ok()
                            .and_then(|b| ed25519_dalek::Signature::from_slice(&b).ok())
                            .is_some_and(|sig| vk.verify(&message(layer_hash), &sig).is_ok())
                    });
                sigs.push(SigInfo {
                    known_as: known
                        .iter()
                        .find(|(_, k)| *k == s.key.trim())
                        .map(|(n, _)| n.clone()),
                    claimed: s.signer,
                    key_id: vk.map(|k| key_id(&k)).unwrap_or_default(),
                    valid,
                });
            }
        }
        Ok(if sigs.is_empty() {
            LayerStatus::Unsigned
        } else if sigs.iter().any(|s| !s.valid) {
            LayerStatus::Invalid { signatures: sigs }
        } else {
            LayerStatus::Signed { signatures: sigs }
        })
    }
}
