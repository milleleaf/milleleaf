//! Sealed layers and crypto-shredding (plan M1.5 item 7, mode B).
//!
//! A sealed layer is stored with its plaintext header, empty `data`, and
//! `header.sealed = {scheme, key, content}`. `content` is a blob holding the layer's canonical
//! plaintext bytes under AES-256-GCM with a random one-time content key. That key is wrapped to
//! each recipient with HPKE X-Wing (`ox-seal`) in `.ox/wraps/<key id>/<recipient>.json`, outside
//! the immutable layer, so deleting the wraps (a `key` purge) makes every copy of the ciphertext
//! useless while the layer's hash and signatures still verify.
//!
//! Decrypted content lives only in memory: flats that include it are never cached.
//!
//! Honest limit: a recipient who opened the layer before the purge may have kept the plaintext
//! (or the content key); destroying keys stops future reads, not past ones.

use std::fs;
use std::path::PathBuf;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use ox_core::model::{Layer, Sealed};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{KeyPair, Result, Store, StoreError, io, layer_bytes, not_ijson, write_new};

const WRAP_INFO: &[u8] = b"Milleleaf content key v1";

/// What opening a layer gave.
#[derive(Debug, Clone, PartialEq)]
pub enum Opened {
    /// Not sealed.
    Plain,
    /// Sealed, and a local key opened it (content in memory only).
    Decrypted,
    /// Sealed and not readable here; composed as empty. Says why.
    Locked(String),
}

/// One recipient's wrapped content key.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Wrap {
    scheme: String,
    key: String,
    recipient: String,
    /// `xwing-sha256:<first 16 hex>` of the recipient's X-Wing public key.
    recipient_key: String,
    enc: String,
    wrapped: String,
}

/// What `ox verify` says about a sealed layer.
#[derive(Debug, Clone)]
pub struct SealInfo {
    /// Names with a wrap (empty after a `key` purge).
    pub recipients: Vec<String>,
    /// A missing or damaged ciphertext blob.
    pub problem: Option<String>,
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn fingerprint(pk: &[u8]) -> String {
    format!("xwing-sha256:{}", hex(&Sha256::digest(pk)[..8]))
}

/// Key ids are 32 hex characters; anything else (from a received layer) never becomes a path.
fn valid_key_id(k: &str) -> bool {
    k.len() == 32 && k.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn content_aad(key_id: &str, layer_id: &str) -> Vec<u8> {
    format!("Milleleaf sealed layer v1\n{key_id}\n{layer_id}").into_bytes()
}

fn has_blob(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Object(m) => m.contains_key("blob") || m.values().any(has_blob),
        serde_json::Value::Array(a) => a.iter().any(has_blob),
        _ => false,
    }
}

fn err(e: ox_seal::Error) -> StoreError {
    StoreError::Key(e.to_string())
}

impl Store {
    fn wraps_dir(&self, key_id: &str) -> PathBuf {
        self.dir.join("wraps").join(key_id)
    }

    /// Encrypts `layer` for `to` (names in the project's key list with an X-Wing key) and writes
    /// their wraps. Returns the layer to store: plaintext header, no data, `header.sealed` set.
    pub(crate) fn seal(&self, layer: &Layer, to: &[String]) -> Result<Layer> {
        if layer.header.sealed.is_some() {
            return Err(StoreError::Key("the layer is already sealed".into()));
        }
        if layer.data.iter().any(|n| n.attributes.values().any(has_blob)) {
            // ponytail: assets would be stored as plaintext blobs; seal them too when a sealed layer needs pictures.
            return Err(StoreError::NotAllowed(
                "a sealed layer can't carry pictures or other assets yet (they would be stored unencrypted)".into(),
            ));
        }
        let mut plain = layer.clone();
        plain.imports.clear();
        if let Some(why) = not_ijson(&serde_json::to_value(&plain).expect("json"), "layer") {
            return Err(StoreError::NotIJson(why));
        }
        let recipients = to
            .iter()
            .map(|n| {
                self.xwing_key(n).map(|k| (n.clone(), k)).ok_or_else(|| {
                    StoreError::Key(format!("`{n}` has no X-Wing key in this project (`ox key add <dir> {n} <ed25519:…> <xwing:…>`)"))
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let k = ox_seal::random32().map_err(err)?;
        let key_id = hex(&ox_seal::random32().map_err(err)?[..16]);
        let ct = ox_seal::encrypt(&k, &content_aad(&key_id, &plain.header.id), &layer_bytes(&plain)).map_err(err)?;
        let content = self.put_blob(&ct)?;
        let d = self.wraps_dir(&key_id);
        io(&d, fs::create_dir_all(&d))?;
        for (name, pk) in recipients {
            let (enc, wrapped) = ox_seal::wrap(&pk, WRAP_INFO, key_id.as_bytes(), &k).map_err(err)?;
            let w = Wrap {
                scheme: ox_seal::SCHEME.into(),
                key: key_id.clone(),
                recipient: name.clone(),
                recipient_key: fingerprint(&pk),
                enc: B64.encode(enc),
                wrapped: B64.encode(wrapped),
            };
            write_new(&d.join(format!("{name}.json")), &serde_json::to_vec_pretty(&w).expect("json"))?;
        }
        let mut out = plain;
        out.data.clear();
        out.schemas.clear();
        out.header.sealed = Some(Sealed { scheme: ox_seal::SCHEME.into(), key: key_id, content });
        Ok(out)
    }

    /// The content key, if a key in `self.keys` matches one of the layer's wraps.
    fn content_key(&self, key_id: &str) -> Option<[u8; 32]> {
        let mine: Vec<(String, KeyPair)> = KeyPair::all_in(&self.keys)
            .into_iter()
            .filter_map(|k| Some((fingerprint(&ox_seal::public_key(k.xwing_seed()?)), k)))
            .collect();
        for e in fs::read_dir(self.wraps_dir(key_id)).ok()?.flatten() {
            let Some(w) = fs::read(e.path()).ok().and_then(|b| serde_json::from_slice::<Wrap>(&b).ok()) else {
                continue;
            };
            let Some((_, kp)) = mine.iter().find(|(f, _)| *f == w.recipient_key) else { continue };
            let (Ok(enc), Ok(wrapped)) = (B64.decode(&w.enc), B64.decode(&w.wrapped)) else { continue };
            if let Some(k) = kp
                .xwing_seed()
                .and_then(|s| ox_seal::unwrap(s, &enc, WRAP_INFO, key_id.as_bytes(), &wrapped).ok())
                .and_then(|k| <[u8; 32]>::try_from(k).ok())
            {
                return Some(k);
            }
        }
        None
    }

    /// A stored layer, decrypted in memory when it is sealed and a local key opens it.
    pub fn open_layer(&self, hash: &str) -> Result<(Layer, Opened)> {
        let mut l = self.layer(hash)?;
        let Some(s) = l.header.sealed.clone() else { return Ok((l, Opened::Plain)) };
        let locked = |why: String| Ok((l.clone(), Opened::Locked(why)));
        if let Some(p) = self.purged(hash) {
            return locked(format!("purged (key destroyed) by {} on {}", p.by, p.at));
        }
        if s.scheme != ox_seal::SCHEME || !valid_key_id(&s.key) {
            return locked(format!("unknown sealing scheme {}", s.scheme));
        }
        let Some(k) = self.content_key(&s.key) else {
            return locked("no key here opens it".into());
        };
        let ct = match self.blob(&s.content) {
            Ok(ct) => ct,
            Err(e) => return locked(format!("its ciphertext is unusable: {e}")),
        };
        let Ok(pt) = ox_seal::decrypt(&k, &content_aad(&s.key, &l.header.id), &ct) else {
            return locked("its ciphertext doesn't decrypt (damaged, or the header was changed)".into());
        };
        let inner = match Layer::from_json(&pt) {
            Ok(i) if i.header.id == l.header.id => i,
            _ => return locked("its decrypted content doesn't match the header".into()),
        };
        l.data = inner.data;
        l.schemas = inner.schemas;
        Ok((l, Opened::Decrypted))
    }

    /// Recipients and ciphertext check of a sealed layer; `None` when it isn't sealed.
    pub fn sealed_info(&self, hash: &str) -> Result<Option<SealInfo>> {
        let Some(s) = self.index(hash)?.header.sealed else { return Ok(None) };
        let mut recipients: Vec<String> = if valid_key_id(&s.key) {
            fs::read_dir(self.wraps_dir(&s.key))
                .into_iter()
                .flatten()
                .flatten()
                .filter_map(|e| e.file_name().to_string_lossy().strip_suffix(".json").map(str::to_string))
                .collect()
        } else {
            vec![]
        };
        recipients.sort();
        let bytes_purged = self.purged(hash).is_some_and(|p| p.mode == "bytes");
        let problem = if bytes_purged { None } else { self.blob(&s.content).err().map(|e| e.to_string()) };
        Ok(Some(SealInfo { recipients, problem }))
    }

    /// Deletes every wrap of a sealed layer's content key (crypto-shredding).
    pub(crate) fn destroy_key(&self, layer_hash: &str) -> Result<()> {
        if let Some(s) = self.index(layer_hash).ok().and_then(|i| i.header.sealed)
            && valid_key_id(&s.key)
        {
            let d = self.wraps_dir(&s.key);
            if d.exists() {
                io(&d, fs::remove_dir_all(&d))?;
            }
        }
        Ok(())
    }

    /// For a `bytes` purge of a sealed layer: its ciphertext blob goes too.
    pub(crate) fn remove_ciphertext(&self, layer_hash: &str) -> Result<()> {
        if let Some(s) = self.index(layer_hash).ok().and_then(|i| i.header.sealed)
            && let Some(hex) = s.content.strip_prefix("sha256-")
            && hex.len() == 64
            && hex.bytes().all(|b| b.is_ascii_hexdigit())
        {
            let p = self.dir.join("blobs").join(hex);
            if p.exists() {
                io(&p, fs::remove_file(&p))?;
            }
        }
        Ok(())
    }
}
