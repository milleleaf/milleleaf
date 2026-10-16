//! Encryption for sealed layers (plan M1.5 item 7, mode B; M4 item 5).
//!
//! A sealed layer's content is encrypted once with a random 32-byte content key (AES-256-GCM), and
//! that key is wrapped to each recipient with HPKE (RFC 9180, base mode) using the X-Wing hybrid KEM
//! (X25519 + ML-KEM-768), HKDF-SHA256 and AES-256-GCM. Destroying every wrap destroys the key, so
//! every copy of the ciphertext becomes unreadable (crypto-shredding).
//!
//! Two implementations of the same thing:
//! - [`native`]: BoringSSL's own HPKE X-Wing, through boring-sys FFI (feature `boring`, not wasm).
//! - [`rust`]: RustCrypto (`hpke` 0.14.1 with `x-wing`), pure Rust, builds for wasm32.
//!
//! Neither key schedule is ours: both libraries implement RFC 9180 themselves. The tests check that
//! each opens what the other sealed, and both against the draft-ietf-hpke-pq test vector.
//! The top-level functions use `native` when it is built, else `rust`.

use std::fmt;

/// `Header.sealed.scheme` for layers sealed by this crate.
pub const SCHEME: &str = "hpke-xwing-sha256-aes256gcm/v1";
/// X-Wing private keys are 32-byte seeds.
pub type Seed = [u8; 32];

const NONCE: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub &'static str);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// The HPKE AEAD. Sealed layers use `Aes256Gcm`; `ChaCha20Poly1305` exists because the published
/// X-Wing / HKDF-SHA256 test vector uses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aead {
    Aes256Gcm,
    ChaCha20Poly1305,
}

#[cfg(feature = "boring")]
pub use native as default_impl;
#[cfg(not(feature = "boring"))]
pub use rust as default_impl;

/// 32 random bytes from the OS.
pub fn random32() -> Result<[u8; 32]> {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).map_err(|_| Error("no randomness"))?;
    Ok(b)
}

/// The X-Wing public key (1216 bytes) of a seed.
pub fn public_key(seed: &Seed) -> Vec<u8> {
    default_impl::public_key(seed)
}

/// Wraps (HPKE-seals) `pt` to `pk`: returns (enc, ciphertext).
pub fn wrap(pk: &[u8], info: &[u8], aad: &[u8], pt: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    default_impl::seal(Aead::Aes256Gcm, pk, info, aad, pt)
}

/// Opens what [`wrap`] made, with the recipient's seed.
pub fn unwrap(seed: &Seed, enc: &[u8], info: &[u8], aad: &[u8], ct: &[u8]) -> Result<Vec<u8>> {
    default_impl::open(Aead::Aes256Gcm, seed, enc, info, aad, ct)
}

/// AES-256-GCM with a one-time key: returns nonce ‖ ciphertext ‖ tag.
pub fn encrypt(key: &[u8; 32], aad: &[u8], pt: &[u8]) -> Result<Vec<u8>> {
    default_impl::encrypt(key, aad, pt)
}

pub fn decrypt(key: &[u8; 32], aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>> {
    default_impl::decrypt(key, aad, sealed)
}

/// RustCrypto: `hpke` 0.14.1 (X-Wing from `x-wing` 0.1.1) and `aes-gcm` 0.11.1. Pure Rust.
pub mod rust {
    use super::{Aead, Error, NONCE, Result, Seed};
    use aes_gcm::aead::{Aead as _, KeyInit, Payload};
    use hpke::aead::{AesGcm256, ChaCha20Poly1305};
    use hpke::kdf::HkdfSha256;
    use hpke::kem::XWing;
    use hpke::{Deserializable, Kem, OpModeR, OpModeS, Serializable};

    type Sk = <XWing as Kem>::PrivateKey;
    type Pk = <XWing as Kem>::PublicKey;
    type Enc = <XWing as Kem>::EncappedKey;

    fn sk(seed: &Seed) -> Result<Sk> {
        Sk::from_bytes(seed).map_err(|_| Error("bad X-Wing seed"))
    }

    pub fn public_key(seed: &Seed) -> Vec<u8> {
        XWing::sk_to_pk(&sk(seed).expect("32 bytes")).to_bytes().to_vec()
    }

    pub fn seal(aead: Aead, pk: &[u8], info: &[u8], aad: &[u8], pt: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
        let pk = Pk::from_bytes(pk).map_err(|_| Error("bad X-Wing public key"))?;
        let (enc, ct) = match aead {
            Aead::Aes256Gcm => hpke::single_shot_seal::<AesGcm256, HkdfSha256, XWing>(&OpModeS::Base, &pk, info, pt, aad),
            Aead::ChaCha20Poly1305 => {
                hpke::single_shot_seal::<ChaCha20Poly1305, HkdfSha256, XWing>(&OpModeS::Base, &pk, info, pt, aad)
            }
        }
        .map_err(|_| Error("HPKE seal failed"))?;
        Ok((enc.to_bytes().to_vec(), ct))
    }

    pub fn open(aead: Aead, seed: &Seed, enc: &[u8], info: &[u8], aad: &[u8], ct: &[u8]) -> Result<Vec<u8>> {
        let sk = sk(seed)?;
        let enc = Enc::from_bytes(enc).map_err(|_| Error("bad HPKE enc"))?;
        match aead {
            Aead::Aes256Gcm => hpke::single_shot_open::<AesGcm256, HkdfSha256, XWing>(&OpModeR::Base, &sk, &enc, info, ct, aad),
            Aead::ChaCha20Poly1305 => {
                hpke::single_shot_open::<ChaCha20Poly1305, HkdfSha256, XWing>(&OpModeR::Base, &sk, &enc, info, ct, aad)
            }
        }
        .map_err(|_| Error("HPKE open failed (wrong key or damaged)"))
    }

    /// The receiver's HPKE exporter output (RFC 9180 §5.3); used against the test vector.
    pub fn export(aead: Aead, seed: &Seed, enc: &[u8], info: &[u8], ctx: &[u8], len: usize) -> Result<Vec<u8>> {
        let sk = sk(seed)?;
        let enc = Enc::from_bytes(enc).map_err(|_| Error("bad HPKE enc"))?;
        let mut out = vec![0u8; len];
        let r = match aead {
            Aead::Aes256Gcm => hpke::setup_receiver::<AesGcm256, HkdfSha256, XWing>(&OpModeR::Base, &sk, &enc, info)
                .and_then(|c| c.export(ctx, &mut out)),
            Aead::ChaCha20Poly1305 => {
                hpke::setup_receiver::<ChaCha20Poly1305, HkdfSha256, XWing>(&OpModeR::Base, &sk, &enc, info)
                    .and_then(|c| c.export(ctx, &mut out))
            }
        };
        r.map_err(|_| Error("HPKE export failed"))?;
        Ok(out)
    }

    pub fn encrypt(key: &[u8; 32], aad: &[u8], pt: &[u8]) -> Result<Vec<u8>> {
        let mut nonce = [0u8; NONCE];
        getrandom::fill(&mut nonce).map_err(|_| Error("no randomness"))?;
        let c = aes_gcm::Aes256Gcm::new(key.into());
        let ct = c.encrypt((&nonce).into(), Payload { msg: pt, aad }).map_err(|_| Error("AES-GCM seal failed"))?;
        Ok([nonce.as_slice(), &ct].concat())
    }

    pub fn decrypt(key: &[u8; 32], aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>> {
        if sealed.len() < NONCE + 16 {
            return Err(Error("ciphertext too short"));
        }
        let (nonce, ct) = sealed.split_at(NONCE);
        let nonce: &[u8; NONCE] = nonce.try_into().expect("12 bytes");
        aes_gcm::Aes256Gcm::new(key.into())
            .decrypt(nonce.into(), Payload { msg: ct, aad })
            .map_err(|_| Error("AES-GCM open failed (wrong key or damaged)"))
    }
}

/// BoringSSL (boring-sys 5.3.0): `EVP_hpke_xwing`, `EVP_hpke_hkdf_sha256`, `EVP_hpke_aes_256_gcm`.
#[cfg(feature = "boring")]
pub mod native {
    use super::{Aead, Error, NONCE, Result, Seed};
    use boring_sys as ffi;

    fn aead_of(a: Aead) -> *const ffi::EVP_HPKE_AEAD {
        // SAFETY: these return pointers to static tables.
        unsafe {
            match a {
                Aead::Aes256Gcm => ffi::EVP_hpke_aes_256_gcm(),
                Aead::ChaCha20Poly1305 => ffi::EVP_hpke_chacha20_poly1305(),
            }
        }
    }

    /// Owns an `EVP_HPKE_KEY` (X-Wing) or an `EVP_HPKE_CTX`, freed on drop.
    struct Key(*mut ffi::EVP_HPKE_KEY);
    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: allocated by EVP_HPKE_KEY_new; freed once. Frees (and zeroes) the private key.
            unsafe { ffi::EVP_HPKE_KEY_free(self.0) }
        }
    }
    struct Ctx(*mut ffi::EVP_HPKE_CTX);
    impl Drop for Ctx {
        fn drop(&mut self) {
            // SAFETY: allocated by EVP_HPKE_CTX_new; freed once.
            unsafe { ffi::EVP_HPKE_CTX_free(self.0) }
        }
    }

    fn ok(r: i32, what: &'static str) -> Result<()> {
        if r == 1 {
            Ok(())
        } else {
            // SAFETY: clears this thread's BoringSSL error queue.
            unsafe { ffi::ERR_clear_error() };
            Err(Error(what))
        }
    }

    fn key(seed: &Seed) -> Result<Key> {
        // SAFETY: KEY_new returns an owned key (null on allocation failure); init reads seed.len() bytes.
        unsafe {
            let k = ffi::EVP_HPKE_KEY_new();
            if k.is_null() {
                return Err(Error("out of memory"));
            }
            let k = Key(k);
            ok(ffi::EVP_HPKE_KEY_init(k.0, ffi::EVP_hpke_xwing(), seed.as_ptr(), seed.len()), "bad X-Wing seed")?;
            Ok(k)
        }
    }

    fn ctx() -> Result<Ctx> {
        // SAFETY: CTX_new returns an owned, zeroed context or null.
        let c = unsafe { ffi::EVP_HPKE_CTX_new() };
        if c.is_null() { Err(Error("out of memory")) } else { Ok(Ctx(c)) }
    }

    pub fn public_key(seed: &Seed) -> Vec<u8> {
        let k = key(seed).expect("32-byte seed");
        let mut pk = vec![0u8; ffi::EVP_HPKE_MAX_PUBLIC_KEY_LENGTH as usize];
        let mut n = 0usize;
        // SAFETY: writes at most pk.len() bytes and their count into n.
        let r = unsafe { ffi::EVP_HPKE_KEY_public_key(k.0, pk.as_mut_ptr(), &mut n, pk.len()) };
        assert_eq!(r, 1, "EVP_HPKE_KEY_public_key");
        pk.truncate(n);
        pk
    }

    /// Seals after setting up the sender, from the OS RNG or (for test vectors) a fixed 64-byte seed.
    fn seal_inner(aead: Aead, pk: &[u8], info: &[u8], aad: &[u8], pt: &[u8], eph: Option<&[u8]>) -> Result<(Vec<u8>, Vec<u8>)> {
        let c = ctx()?;
        let mut enc = vec![0u8; ffi::EVP_HPKE_MAX_ENC_LENGTH as usize];
        let mut enc_len = 0usize;
        // SAFETY: every pointer/length pair describes a live slice; outputs are bounded by their lengths.
        unsafe {
            let r = match eph {
                None => ffi::EVP_HPKE_CTX_setup_sender(
                    c.0, enc.as_mut_ptr(), &mut enc_len, enc.len(),
                    ffi::EVP_hpke_xwing(), ffi::EVP_hpke_hkdf_sha256(), aead_of(aead),
                    pk.as_ptr(), pk.len(), info.as_ptr(), info.len()),
                Some(s) => ffi::EVP_HPKE_CTX_setup_sender_with_seed_for_testing(
                    c.0, enc.as_mut_ptr(), &mut enc_len, enc.len(),
                    ffi::EVP_hpke_xwing(), ffi::EVP_hpke_hkdf_sha256(), aead_of(aead),
                    pk.as_ptr(), pk.len(), info.as_ptr(), info.len(), s.as_ptr(), s.len()),
            };
            ok(r, "bad X-Wing public key")?;
            enc.truncate(enc_len);
            let mut ct = vec![0u8; pt.len() + ffi::EVP_HPKE_CTX_max_overhead(c.0)];
            let mut n = 0usize;
            ok(ffi::EVP_HPKE_CTX_seal(c.0, ct.as_mut_ptr(), &mut n, ct.len(), pt.as_ptr(), pt.len(), aad.as_ptr(), aad.len()), "HPKE seal failed")?;
            ct.truncate(n);
            Ok((enc, ct))
        }
    }

    pub fn seal(aead: Aead, pk: &[u8], info: &[u8], aad: &[u8], pt: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
        seal_inner(aead, pk, info, aad, pt, None)
    }

    /// Deterministic sealing from the vector's `ikmE`. Test use only (BoringSSL's `_for_testing`).
    #[doc(hidden)]
    pub fn seal_with_seed_for_testing(aead: Aead, pk: &[u8], info: &[u8], aad: &[u8], pt: &[u8], ikm_e: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
        seal_inner(aead, pk, info, aad, pt, Some(ikm_e))
    }

    fn recipient(aead: Aead, seed: &Seed, enc: &[u8], info: &[u8]) -> Result<Ctx> {
        let k = key(seed)?;
        let c = ctx()?;
        // SAFETY: as above; the context copies what it needs from the key.
        ok(unsafe {
            ffi::EVP_HPKE_CTX_setup_recipient(c.0, k.0, ffi::EVP_hpke_hkdf_sha256(), aead_of(aead),
                enc.as_ptr(), enc.len(), info.as_ptr(), info.len())
        }, "bad HPKE enc")?;
        Ok(c)
    }

    pub fn open(aead: Aead, seed: &Seed, enc: &[u8], info: &[u8], aad: &[u8], ct: &[u8]) -> Result<Vec<u8>> {
        let c = recipient(aead, seed, enc, info)?;
        let mut pt = vec![0u8; ct.len()];
        let mut n = 0usize;
        // SAFETY: plaintext is never longer than the ciphertext.
        ok(unsafe { ffi::EVP_HPKE_CTX_open(c.0, pt.as_mut_ptr(), &mut n, pt.len(), ct.as_ptr(), ct.len(), aad.as_ptr(), aad.len()) },
            "HPKE open failed (wrong key or damaged)")?;
        pt.truncate(n);
        Ok(pt)
    }

    pub fn export(aead: Aead, seed: &Seed, enc: &[u8], info: &[u8], ctx: &[u8], len: usize) -> Result<Vec<u8>> {
        let c = recipient(aead, seed, enc, info)?;
        let mut out = vec![0u8; len];
        // SAFETY: writes exactly out.len() bytes.
        ok(unsafe { ffi::EVP_HPKE_CTX_export(c.0, out.as_mut_ptr(), out.len(), ctx.as_ptr(), ctx.len()) }, "HPKE export failed")?;
        Ok(out)
    }

    pub fn encrypt(key: &[u8; 32], aad: &[u8], pt: &[u8]) -> Result<Vec<u8>> {
        use boring::symm::{Cipher, encrypt_aead};
        let mut nonce = [0u8; NONCE];
        boring::rand::rand_bytes(&mut nonce).map_err(|_| Error("no randomness"))?;
        let mut tag = [0u8; 16];
        let ct = encrypt_aead(Cipher::aes_256_gcm(), key, Some(&nonce), aad, pt, &mut tag)
            .map_err(|_| Error("AES-GCM seal failed"))?;
        Ok([nonce.as_slice(), &ct, &tag].concat())
    }

    pub fn decrypt(key: &[u8; 32], aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>> {
        use boring::symm::{Cipher, decrypt_aead};
        if sealed.len() < NONCE + 16 {
            return Err(Error("ciphertext too short"));
        }
        let (nonce, rest) = sealed.split_at(NONCE);
        let (ct, tag) = rest.split_at(rest.len() - 16);
        decrypt_aead(Cipher::aes_256_gcm(), key, Some(nonce), aad, ct, tag)
            .map_err(|_| Error("AES-GCM open failed (wrong key or damaged)"))
    }
}

// The interop tests need both implementations.
#[cfg(all(test, feature = "boring"))]
mod tests;
