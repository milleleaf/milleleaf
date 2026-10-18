//! RFC 3161 time-stamps on signature files (plan decision 5, 18 Oct 2026). A time-stamp authority
//! (TSA) signs the SHA-256 of a signature file's bytes and the time it saw them; the response is
//! kept beside the file as `<sigfile>.tsr`. Here: build the request and check a response's message
//! imprint and time. The network call is the CLI's (`ox timestamp`), so this stays pure.
//!
//! Not checked here: the TSA's own signature and certificate chain. RustCrypto has no certificate
//! path validation, so `verify` says "TSA signature not checked". TSA tokens are RSA or ECDSA
//! signatures: they need renewing (RFC 4998 evidence records) before those can be forged.

use cms::content_info::ContentInfo;
use cms::signed_data::SignedData;
use cms::spki::AlgorithmIdentifierOwned;
use der::asn1::{ObjectIdentifier, OctetString, Uint};
use der::{Any, Decode, Encode, Sequence, Tagged};
use sha2::{Digest, Sha256};

const SHA256: ObjectIdentifier = ObjectIdentifier::new_unwrap("2.16.840.1.101.3.4.2.1");
const SIGNED_DATA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.2");
const TST_INFO: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.16.1.4");

#[derive(Sequence)]
struct MessageImprint {
    hash_algorithm: AlgorithmIdentifierOwned,
    hashed_message: OctetString,
}

#[derive(Sequence)]
struct TimeStampReq {
    version: u8,
    message_imprint: MessageImprint,
    nonce: Option<Uint>,
    cert_req: Option<bool>,
}

#[derive(Sequence)]
struct PkiStatusInfo {
    status: u8,
    status_string: Option<Vec<String>>,
    fail_info: Option<der::asn1::BitString>,
}

#[derive(Sequence)]
struct TimeStampResp {
    status: PkiStatusInfo,
    token: Option<ContentInfo>,
}

#[derive(Sequence)]
struct Accuracy {
    seconds: Option<Uint>,
    #[asn1(context_specific = "0", tag_mode = "IMPLICIT", optional = "true")]
    millis: Option<Uint>,
    #[asn1(context_specific = "1", tag_mode = "IMPLICIT", optional = "true")]
    micros: Option<Uint>,
}

#[derive(Sequence)]
struct TstInfo {
    version: u8,
    policy: ObjectIdentifier,
    message_imprint: MessageImprint,
    serial_number: Uint,
    /// Read as text: RFC 3161 allows fractional seconds, which DER's GeneralizedTime type refuses.
    gen_time: Any,
    accuracy: Option<Accuracy>,
    ordering: Option<bool>,
    nonce: Option<Uint>,
    #[asn1(context_specific = "0", tag_mode = "EXPLICIT", optional = "true")]
    tsa: Option<Any>,
    #[asn1(context_specific = "1", tag_mode = "IMPLICIT", optional = "true")]
    extensions: Option<cms::cert::x509::ext::Extensions>,
}

/// A DER `TimeStampReq` for `data` (SHA-256 imprint, a random nonce, the TSA's certificate asked
/// for) and its nonce, which the response must echo.
pub fn request(data: &[u8]) -> Result<(Vec<u8>, [u8; 8]), String> {
    let mut nonce = [0u8; 8];
    getrandom::getrandom(&mut nonce).map_err(|e| e.to_string())?;
    let req = TimeStampReq {
        version: 1,
        message_imprint: imprint(data),
        nonce: Some(Uint::new(&nonce).map_err(|e| e.to_string())?),
        cert_req: Some(true),
    };
    Ok((req.to_der().map_err(|e| e.to_string())?, nonce))
}

fn imprint(data: &[u8]) -> MessageImprint {
    MessageImprint {
        hash_algorithm: AlgorithmIdentifierOwned { oid: SHA256, parameters: None },
        hashed_message: OctetString::new(Sha256::digest(data).to_vec()).expect("digest"),
    }
}

/// What a time-stamp response says about `data`.
#[derive(Debug, Clone, PartialEq)]
pub struct Stamp {
    /// The TSA's time, RFC 3339 (`genTime`, UTC; fractions dropped).
    pub at: String,
    /// The nonce the TSA echoed, as big-endian bytes without leading zeros.
    pub nonce: Option<Vec<u8>>,
}

/// Checks that a DER `TimeStampResp` was granted and stamps exactly `data` (SHA-256 imprint).
pub fn check(response: &[u8], data: &[u8]) -> Result<Stamp, String> {
    let resp = TimeStampResp::from_der(response).map_err(|e| format!("not an RFC 3161 response: {e}"))?;
    if resp.status.status > 1 {
        let why = resp.status.status_string.unwrap_or_default().join("; ");
        return Err(format!("the TSA refused (status {}{})", resp.status.status, if why.is_empty() { String::new() } else { format!(": {why}") }));
    }
    let token = resp.token.ok_or("granted, but no token in the response")?;
    if token.content_type != SIGNED_DATA {
        return Err("the token isn't CMS SignedData".into());
    }
    let sd: SignedData = token.content.decode_as().map_err(|e| format!("token: {e}"))?;
    if sd.encap_content_info.econtent_type != TST_INFO {
        return Err("the token doesn't hold a TSTInfo".into());
    }
    let tst = sd
        .encap_content_info
        .econtent
        .ok_or("the token has no TSTInfo")?
        .decode_as::<OctetString>()
        .ok()
        .and_then(|o| TstInfo::from_der(o.as_bytes()).ok())
        .ok_or("the token's TSTInfo can't be read")?;
    let want = imprint(data);
    if tst.message_imprint.hash_algorithm.oid != SHA256 {
        return Err(format!("imprint uses {}, not SHA-256", tst.message_imprint.hash_algorithm.oid));
    }
    if tst.message_imprint.hashed_message != want.hashed_message {
        return Err("the token stamps different bytes (message imprint doesn't match)".into());
    }
    let t = tst.gen_time.value();
    if tst.gen_time.tag() != der::Tag::GeneralizedTime || t.len() < 15 || !t[..14].iter().all(u8::is_ascii_digit) {
        return Err("the token's genTime can't be read".into());
    }
    let d = |r: std::ops::Range<usize>| std::str::from_utf8(&t[r]).unwrap_or_default();
    Ok(Stamp {
        at: format!("{}-{}-{}T{}:{}:{}Z", d(0..4), d(4..6), d(6..8), d(8..10), d(10..12), d(12..14)),
        nonce: tst.nonce.map(|n| n.as_bytes().to_vec()),
    })
}
