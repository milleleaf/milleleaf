use super::{Aead, native, rust};

const INFO: &[u8] = b"Milleleaf interop test";

fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex")).collect()
}

fn seed() -> [u8; 32] {
    super::random32().expect("rng")
}

fn sample(n: usize) -> Vec<u8> {
    (0..n as u32).map(|i| (i.wrapping_mul(2654435761) >> 24) as u8).collect()
}

#[test]
fn both_implementations_derive_the_same_public_key() {
    for _ in 0..8 {
        let s = seed();
        let pk = native::public_key(&s);
        assert_eq!(pk.len(), 1216);
        assert_eq!(pk, rust::public_key(&s));
    }
}

#[test]
fn boringssl_seals_and_rustcrypto_opens_and_the_reverse() {
    for n in [0, 1, 32, 1 << 20] {
        let pt = sample(n);
        let s = seed();
        let pk = native::public_key(&s);
        let aad = b"key 0123456789abcdef";
        // BoringSSL -> RustCrypto
        let (enc, ct) = native::seal(Aead::Aes256Gcm, &pk, INFO, aad, &pt).unwrap();
        assert_eq!(enc.len(), 1120);
        assert_eq!(rust::open(Aead::Aes256Gcm, &s, &enc, INFO, aad, &ct).unwrap(), pt, "boring -> rustcrypto, {n} B");
        // RustCrypto -> BoringSSL
        let (enc, ct) = rust::seal(Aead::Aes256Gcm, &pk, INFO, aad, &pt).unwrap();
        assert_eq!(native::open(Aead::Aes256Gcm, &s, &enc, INFO, aad, &ct).unwrap(), pt, "rustcrypto -> boring, {n} B");
        // Both refuse a wrong AAD, a wrong info, a damaged ciphertext and someone else's key.
        let mut bad = ct.clone();
        bad[0] ^= 1;
        let other = seed();
        for open in [native::open, rust::open] {
            assert!(open(Aead::Aes256Gcm, &s, &enc, INFO, b"other", &ct).is_err());
            assert!(open(Aead::Aes256Gcm, &s, &enc, b"other info", aad, &ct).is_err());
            assert!(open(Aead::Aes256Gcm, &s, &enc, INFO, aad, &bad).is_err());
            assert!(open(Aead::Aes256Gcm, &other, &enc, INFO, aad, &ct).is_err());
        }
    }
}

#[test]
fn content_encryption_interoperates() {
    let k = seed();
    let pt = sample(100_000);
    let a = native::encrypt(&k, b"aad", &pt).unwrap();
    assert_eq!(rust::decrypt(&k, b"aad", &a).unwrap(), pt);
    let b = rust::encrypt(&k, b"aad", &pt).unwrap();
    assert_eq!(native::decrypt(&k, b"aad", &b).unwrap(), pt);
    assert!(native::decrypt(&k, b"other", &b).is_err());
    assert!(rust::decrypt(&seed(), b"aad", &a).is_err());
}

/// draft-ietf-hpke-pq test vector for X-Wing (0x647a) + HKDF-SHA256 + ChaCha20-Poly1305, base mode.
/// Source: https://github.com/hpkewg/hpke-pq/blob/6433c8fce0b8b749dfc86c1095081a88698ccfab/test-vectors.json
/// (commit 2026-07-06; the same file hpke 0.14.1 ships as test-vectors/pq-6433c8f.json, SHA-256
/// 35c59f4a0132e5631e50ac039d8ca3a72e99f5e92dfd94d45338d6ae243f613c). No published vector uses
/// X-Wing with AES-256-GCM; the AEAD step is RFC 9180's and checked by the interop test above.
#[test]
fn both_implementations_match_the_hpke_pq_test_vector() {
    let v: serde_json::Value = serde_json::from_str(include_str!("../vectors/xwing-hkdfsha256-chacha.json")).unwrap();
    let h = |k: &str| hex(v[k].as_str().unwrap());
    assert_eq!((v["kem_id"].as_u64(), v["kdf_id"].as_u64(), v["aead_id"].as_u64(), v["mode"].as_u64()), (Some(0x647a), Some(1), Some(3), Some(0)));
    let sk: [u8; 32] = h("skRm").try_into().unwrap();
    let (info, enc, pk) = (h("info"), h("enc"), h("pkRm"));
    let first = &v["encryptions"][0];
    let (aad, pt, ct) = (hex(first["aad"].as_str().unwrap()), hex(first["pt"].as_str().unwrap()), hex(first["ct"].as_str().unwrap()));
    assert_eq!(hex(first["nonce"].as_str().unwrap()), h("base_nonce"), "first encryption uses sequence 0");

    assert_eq!(native::public_key(&sk), pk, "BoringSSL pkRm");
    assert_eq!(rust::public_key(&sk), pk, "RustCrypto pkRm");
    assert_eq!(native::open(Aead::ChaCha20Poly1305, &sk, &enc, &info, &aad, &ct).unwrap(), pt, "BoringSSL open");
    assert_eq!(rust::open(Aead::ChaCha20Poly1305, &sk, &enc, &info, &aad, &ct).unwrap(), pt, "RustCrypto open");
    for x in v["exports"].as_array().unwrap() {
        let ctx = hex(x["exporter_context"].as_str().unwrap());
        let len = x["L"].as_u64().unwrap() as usize;
        let want = hex(x["exported_value"].as_str().unwrap());
        assert_eq!(native::export(Aead::ChaCha20Poly1305, &sk, &enc, &info, &ctx, len).unwrap(), want, "BoringSSL export");
        assert_eq!(rust::export(Aead::ChaCha20Poly1305, &sk, &enc, &info, &ctx, len).unwrap(), want, "RustCrypto export");
    }
    // Sender side, deterministic from ikmE: BoringSSL reproduces enc and the first ciphertext.
    let (e2, c2) = native::seal_with_seed_for_testing(Aead::ChaCha20Poly1305, &pk, &info, &aad, &pt, &h("ikmE")).unwrap();
    assert_eq!(e2, enc, "BoringSSL enc from ikmE");
    assert_eq!(c2, ct, "BoringSSL ciphertext from ikmE");
}
