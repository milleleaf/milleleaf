//! Post-quantum signatures beside Ed25519 (plan decision 5, amended 18 Oct 2026): ML-DSA-65
//! (FIPS 204) on every signature, and SLH-DSA (FIPS 205) for owners, approvers and archives.
//! ML-DSA: RustCrypto `ml-dsa`. SLH-DSA: `fips205` (a final release), with
//! RustCrypto `slh-dsa` kept as a test-only cross-check. Pure Rust. Every algorithm signs the same
//! message as Ed25519, with an empty FIPS context string. ox-store calls the libraries only through
//! this file, so the NIST ACVP and Wycheproof vectors in its tests run through exactly this code.

use fips205::traits::{KeyGen, SerDes, Signer, Verifier};
use ml_dsa::{ExpandedSigningKey, Keypair, MlDsa65, SigningKey as MlKey};

/// The SLH-DSA parameter set. To switch (the maintainer may choose SHA2-128s), change both lines, the
/// `fips205` feature in Cargo.toml and `RcSlh` in the tests; a test checks they agree.
use fips205::slh_dsa_sha2_192s as slh;
pub const SLH: &str = "slhdsa-sha2-192s";

pub const ED25519: &str = "ed25519";
pub const MLDSA65: &str = "mldsa65";

/// An ML-DSA-65 private key is its 32-byte seed (FIPS 204 keygen ξ).
pub type MlSeed = [u8; 32];

pub fn mldsa_key(seed: &MlSeed) -> MlKey<MlDsa65> {
    MlKey::from_seed(&(*seed).into())
}

pub fn mldsa_public(seed: &MlSeed) -> Vec<u8> {
    mldsa_key(seed).verifying_key().encode().to_vec()
}

/// FIPS 204 Algorithm 2 (ML-DSA.Sign): M' = 0 ‖ |ctx| ‖ ctx ‖ M, then Sign_internal with `rnd`
/// (random for hedged signing, zeros for the deterministic variant).
pub fn mldsa_sign(key: &ExpandedSigningKey<MlDsa65>, ctx: &[u8], msg: &[u8], rnd: &[u8; 32]) -> Option<Vec<u8>> {
    let len = u8::try_from(ctx.len()).ok()?;
    Some(key.sign_internal(&[&[0, len], ctx, msg], &(*rnd).into()).encode().to_vec())
}

/// FIPS 204 Algorithm 3 (ML-DSA.Verify); false for a malformed key or signature.
pub fn mldsa_verify(public: &[u8], ctx: &[u8], msg: &[u8], sig: &[u8]) -> bool {
    let Ok(enc) = public.try_into() else { return false };
    let Ok(sig) = ml_dsa::Signature::<MlDsa65>::try_from(sig) else { return false };
    ml_dsa::VerifyingKey::<MlDsa65>::decode(enc).verify_with_context(msg, ctx, &sig)
}

/// Hands `fips205` exactly the bytes we drew (or the vectors give); any other request fails.
struct Given<'a>(&'a [u8]);

impl rand_core::RngCore for Given<'_> {
    fn next_u32(&mut self) -> u32 {
        unimplemented!("fips205 only calls try_fill_bytes")
    }
    fn next_u64(&mut self) -> u64 {
        unimplemented!("fips205 only calls try_fill_bytes")
    }
    fn fill_bytes(&mut self, out: &mut [u8]) {
        self.try_fill_bytes(out).expect("randomness of the right length")
    }
    fn try_fill_bytes(&mut self, out: &mut [u8]) -> Result<(), rand_core::Error> {
        if out.len() != self.0.len() {
            return Err(core::num::NonZeroU32::new(rand_core::Error::CUSTOM_START).expect("non-zero").into());
        }
        out.copy_from_slice(self.0);
        self.0 = &[];
        Ok(())
    }
}

impl rand_core::CryptoRng for Given<'_> {}

/// An SLH-DSA private key as FIPS 205 stores it: SK.seed ‖ SK.prf ‖ PK.seed ‖ PK.root.
pub fn slh_generate() -> Option<Vec<u8>> {
    let n = slh_n();
    let mut s = vec![0u8; 3 * n];
    getrandom::getrandom(&mut s).ok()?;
    Some(slh_from_seeds(&s[..n], &s[n..2 * n], &s[2 * n..]))
}

/// FIPS 205 Algorithm 18 (slh_keygen_internal) from SK.seed, SK.prf and PK.seed.
pub(crate) fn slh_from_seeds(sk_seed: &[u8], sk_prf: &[u8], pk_seed: &[u8]) -> Vec<u8> {
    let [a, b, c]: [[u8; slh::PK_LEN / 2]; 3] =
        [sk_seed, sk_prf, pk_seed].map(|s| s.try_into().expect("seeds of n bytes"));
    slh::KG::keygen_with_seeds(&a, &b, &c).1.into_bytes().to_vec()
}

/// The parameter set's n: the public key is PK.seed ‖ PK.root, n bytes each.
pub(crate) fn slh_n() -> usize {
    slh::PK_LEN / 2
}

fn slh_key(sk: &[u8]) -> Option<slh::PrivateKey> {
    slh::PrivateKey::try_from_bytes(sk.try_into().ok()?).ok()
}

pub fn slh_public(sk: &[u8]) -> Option<Vec<u8>> {
    Some(slh_key(sk)?.get_public_key().into_bytes().to_vec())
}

/// FIPS 205 Algorithm 22 (slh_sign); `rand` (n bytes) = None is the deterministic variant.
pub fn slh_sign(sk: &[u8], ctx: &[u8], msg: &[u8], rand: Option<&[u8]>) -> Option<Vec<u8>> {
    let key = slh_key(sk)?;
    Some(key.try_sign_with_rng(&mut Given(rand.unwrap_or_default()), msg, ctx, rand.is_some()).ok()?.to_vec())
}

/// FIPS 205 Algorithm 24 (slh_verify); false for a malformed key or signature.
pub fn slh_verify(public: &[u8], ctx: &[u8], msg: &[u8], sig: &[u8]) -> bool {
    let (Some(pk), Ok(sig)) = (
        public.try_into().ok().and_then(|b| slh::PublicKey::try_from_bytes(b).ok()),
        <&[u8; slh::SIG_LEN]>::try_from(sig),
    ) else {
        return false;
    };
    pk.verify(msg, sig, ctx)
}

#[cfg(test)]
mod tests {
    //! NIST ACVP-Server and Wycheproof vectors (a pinned subset; sources and sha256 in the file,
    //! remade by `testdata/pq-vectors-subset.py`), through the functions above.
    use super::*;
    use ml_dsa::common::typenum::Unsigned;
    use serde_json::Value;

    fn vectors() -> Value {
        let p = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/pq-vectors.json");
        serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
    }

    fn hex(v: &Value) -> Vec<u8> {
        let s = v.as_str().unwrap_or_default();
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    fn each<'a>(v: &'a Value, key: &str) -> &'a Vec<Value> {
        v[key].as_array().unwrap_or_else(|| panic!("no {key} vectors"))
    }

    #[test]
    #[allow(deprecated)] // ACVP gives expanded private keys; ox-store itself keeps only seeds
    fn ml_dsa_65_matches_nist_acvp_and_wycheproof() {
        let all = vectors();
        let v = &all["mldsa65"];
        for t in each(v, "keyGen") {
            let seed: MlSeed = hex(&t["seed"]).try_into().unwrap();
            assert_eq!(mldsa_public(&seed), hex(&t["pk"]), "keyGen {}", t["tcId"]);
            assert_eq!(mldsa_key(&seed).expanded_key().to_expanded().to_vec(), hex(&t["sk"]), "keyGen sk {}", t["tcId"]);
        }
        for t in each(v, "sigGen") {
            let sk = ExpandedSigningKey::<MlDsa65>::from_expanded(hex(&t["sk"]).as_slice().try_into().unwrap());
            let rnd: [u8; 32] = if t["deterministic"] == true { [0; 32] } else { hex(&t["rnd"]).try_into().unwrap() };
            let sig = mldsa_sign(&sk, &hex(&t["context"]), &hex(&t["message"]), &rnd);
            assert_eq!(sig, Some(hex(&t["signature"])), "sigGen {}", t["tcId"]);
        }
        for t in each(v, "sigVer") {
            let ok = mldsa_verify(&hex(&t["pk"]), &hex(&t["context"]), &hex(&t["message"]), &hex(&t["signature"]));
            assert_eq!(ok, t["testPassed"] == true, "sigVer {} ({})", t["tcId"], t["reason"]);
        }
        let mut n = 0;
        for g in each(v, "wycheproofVerify") {
            for t in each(g, "tests") {
                let ok = mldsa_verify(&hex(&g["publicKey"]), &hex(&t["ctx"]), &hex(&t["msg"]), &hex(&t["sig"]));
                assert_eq!(ok, t["result"] == "valid", "wycheproof {} {}", t["tcId"], t["comment"]);
                n += 1;
            }
        }
        for t in each(v, "wycheproofSignSeed") {
            let seed: MlSeed = hex(&t["privateSeed"]).try_into().unwrap();
            let sig = mldsa_sign(mldsa_key(&seed).expanded_key(), &hex(&t["ctx"]), &hex(&t["msg"]), &[0; 32]);
            if t["result"] == "valid" {
                assert_eq!(sig, Some(hex(&t["sig"])), "wycheproof sign {} {}", t["tcId"], t["comment"]);
            } else {
                assert_eq!(sig, None, "wycheproof sign {} {}", t["tcId"], t["comment"]);
            }
        }
        assert!(n >= 20, "{n} Wycheproof verify vectors");
    }

    /// The RustCrypto implementation of the same parameter set: a test-only cross-check.
    type RcSlh = slh_dsa::Sha2_192s;

    #[test]
    fn slh_dsa_matches_nist_acvp() {
        // The one constant names the parameter set both implementations are.
        let name = <RcSlh as slh_dsa::ParameterSet>::NAME;
        assert_eq!(SLH, format!("slhdsa-{}", name.trim_start_matches("SLH-DSA-").to_lowercase()));
        assert_eq!(slh::SIG_LEN, <RcSlh as slh_dsa::SignatureLen>::SigLen::USIZE);
        assert_eq!(slh::PK_LEN, <RcSlh as slh_dsa::VerifyingKeyLen>::VkLen::USIZE);
        let all = vectors();
        let v = &all["slhdsa"][name];
        for t in each(v, "keyGen") {
            let sk = slh_from_seeds(&hex(&t["skSeed"]), &hex(&t["skPrf"]), &hex(&t["pkSeed"]));
            assert_eq!(sk, hex(&t["sk"]), "keyGen {}", t["tcId"]);
            assert_eq!(slh_public(&sk), Some(hex(&t["pk"])), "keyGen pk {}", t["tcId"]);
        }
        for t in each(v, "sigGen") {
            let rand = (t["deterministic"] != true).then(|| hex(&t["additionalRandomness"]));
            let sig = slh_sign(&hex(&t["sk"]), &hex(&t["context"]), &hex(&t["message"]), rand.as_deref());
            assert_eq!(sig, Some(hex(&t["signature"])), "sigGen {}", t["tcId"]);
        }
        for t in each(v, "sigVer") {
            let ok = slh_verify(&hex(&t["pk"]), &hex(&t["context"]), &hex(&t["message"]), &hex(&t["signature"]));
            assert_eq!(ok, t["testPassed"] == true, "sigVer {} ({})", t["tcId"], t["reason"]);
        }
    }

    /// `fips205` and RustCrypto `slh-dsa` agree: same public key from the same private key, and
    /// each verifies the other's hedged signature over the layer message (empty context).
    #[test]
    fn slh_dsa_fips205_and_rustcrypto_verify_each_other() {
        use slh_dsa::signature::Keypair;
        let sk = slh_generate().unwrap();
        let msg = b"Milleleaf layer signature v1 sha256-0123";
        let mut rand = vec![0u8; slh_n()];
        getrandom::getrandom(&mut rand).unwrap();
        let rc = slh_dsa::SigningKey::<RcSlh>::try_from(sk.as_slice()).unwrap();
        assert_eq!(rc.verifying_key().to_vec(), slh_public(&sk).unwrap());

        let ours = slh_sign(&sk, &[], msg, Some(&rand)).unwrap();
        let theirs = slh_dsa::Signature::<RcSlh>::try_from(ours.as_slice()).unwrap();
        assert!(rc.verifying_key().try_verify_with_context(msg, &[], &theirs).is_ok());

        let theirs = rc.try_sign_with_context(msg, &[], Some(&rand)).unwrap().to_vec();
        assert!(slh_verify(&slh_public(&sk).unwrap(), &[], msg, &theirs));
        assert_eq!(theirs, ours, "same randomness, same signature");
        assert!(!slh_verify(&slh_public(&sk).unwrap(), &[], b"other", &theirs));
        assert_eq!(slh_sign(&sk, &[], msg, Some(&rand[1..])), None, "randomness of the wrong length");
    }
}
