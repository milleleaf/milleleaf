//! Post-quantum signatures beside Ed25519 (plan decision 5, amended 18 Oct 2026): ML-DSA-65
//! (FIPS 204) on every signature, and SLH-DSA (FIPS 205) for owners, approvers and archives.
//! RustCrypto `ml-dsa` and `slh-dsa`, pure Rust. Every algorithm signs the same message as Ed25519,
//! with an empty FIPS context string. ox-store calls the two libraries only through this file, so
//! the NIST ACVP and Wycheproof vectors in its tests run through exactly this code.

use ml_dsa::{ExpandedSigningKey, Keypair, MlDsa65, SigningKey as MlKey};

/// The SLH-DSA parameter set. To switch (the maintainer may choose SHA2-128s), change both lines; a test
/// checks they agree, and the vector tests follow the type.
pub type Slh = slh_dsa::Sha2_192s;
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

/// An SLH-DSA private key as FIPS 205 stores it: SK.seed ‖ SK.prf ‖ PK.seed ‖ PK.root.
pub fn slh_generate() -> Option<Vec<u8>> {
    let n = slh_n();
    let mut s = vec![0u8; 3 * n];
    getrandom::getrandom(&mut s).ok()?;
    Some(slh_from_seeds(&s[..n], &s[n..2 * n], &s[2 * n..]))
}

/// FIPS 205 Algorithm 18 (slh_keygen_internal) from SK.seed, SK.prf and PK.seed.
pub(crate) fn slh_from_seeds(sk_seed: &[u8], sk_prf: &[u8], pk_seed: &[u8]) -> Vec<u8> {
    slh_dsa::SigningKey::<Slh>::slh_keygen_internal(sk_seed, sk_prf, pk_seed).to_vec()
}

/// The parameter set's n: the public key is PK.seed ‖ PK.root, n bytes each.
pub(crate) fn slh_n() -> usize {
    use ml_dsa::common::typenum::Unsigned;
    <<Slh as slh_dsa::VerifyingKeyLen>::VkLen as Unsigned>::USIZE / 2
}

pub fn slh_public(sk: &[u8]) -> Option<Vec<u8>> {
    use slh_dsa::signature::Keypair;
    Some(slh_dsa::SigningKey::<Slh>::try_from(sk).ok()?.verifying_key().to_vec())
}

/// FIPS 205 Algorithm 22 (slh_sign); `rand` = None is the deterministic variant.
pub fn slh_sign(sk: &[u8], ctx: &[u8], msg: &[u8], rand: Option<&[u8]>) -> Option<Vec<u8>> {
    let key = slh_dsa::SigningKey::<Slh>::try_from(sk).ok()?;
    Some(key.try_sign_with_context(msg, ctx, rand).ok()?.to_vec())
}

/// FIPS 205 Algorithm 24 (slh_verify); false for a malformed key or signature.
pub fn slh_verify(public: &[u8], ctx: &[u8], msg: &[u8], sig: &[u8]) -> bool {
    let (Ok(vk), Ok(sig)) = (
        slh_dsa::VerifyingKey::<Slh>::try_from(public),
        slh_dsa::Signature::<Slh>::try_from(sig),
    ) else {
        return false;
    };
    vk.try_verify_with_context(msg, ctx, &sig).is_ok()
}

#[cfg(test)]
mod tests {
    //! NIST ACVP-Server and Wycheproof vectors (a pinned subset; sources and sha256 in the file,
    //! remade by `testdata/pq-vectors-subset.py`), through the functions above.
    use super::*;
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

    #[test]
    fn slh_dsa_matches_nist_acvp() {
        // The one constant names the parameter set the type is.
        let name = <Slh as slh_dsa::ParameterSet>::NAME;
        assert_eq!(SLH, format!("slhdsa-{}", name.trim_start_matches("SLH-DSA-").to_lowercase()));
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
}
