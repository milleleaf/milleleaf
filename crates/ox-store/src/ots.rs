//! OpenTimestamps proofs of signature files (`<sigfile>.ots`; plan decision 5, amended 18 Oct
//! 2026). Our own code for the `.ots` format:
//! parse, write, replay the operations, list the attestations, and check a Bitcoin block header.
//! Pure: the calendars and the header sources are the CLI's network calls.
//!
//! A `.ots` file is untrusted input: depth, item count and lengths are bounded, and anything we
//! don't support is an error, never a panic. Supported: SHA-256 file digests and the `sha256`,
//! `append` and `prepend` operations, which is all current calendars use, and `ripemd160`, which
//! 2015-era proofs use. sha1 (0x02), keccak256 (0x67) and reverse/hexlify (0xf2/0xf3) are refused.

use sha2::{Digest, Sha256};

pub const MAGIC: &[u8] = b"\x00OpenTimestamps\x00\x00Proof\x00\xbf\x89\xe2\xe8\x84\xe8\x92\x94";
const BITCOIN: [u8; 8] = [0x05, 0x88, 0x96, 0x0d, 0x73, 0xd7, 0x19, 0x01];
const PENDING: [u8; 8] = [0x83, 0xdf, 0xe3, 0x0d, 0x2e, 0xf9, 0x0c, 0x8e];
/// The reference client's limit on a message and on an operation's argument.
const MAX_MSG: usize = 4096;
const MAX_DEPTH: usize = 256;
const MAX_ITEMS: usize = 10_000;
const MAX_PAYLOAD: usize = 8192;
const MAX_URI: usize = 1000;

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Sha256,
    Ripemd160,
    Append(Vec<u8>),
    Prepend(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Att {
    /// The message is the merkle root of this Bitcoin block (byte-reversed as explorers show it).
    Bitcoin(u64),
    /// A calendar promised to finish the path; the URI is the calendar's own claim.
    Pending(String),
    /// Another notary; kept byte for byte, never trusted.
    Unknown([u8; 8], Vec<u8>),
}

/// One step of a proof tree, in file order.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Att(Att),
    Op(Op, Node),
}

/// What applies to one message: attestations of it and operations leading on (several = forks).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Node(pub Vec<Item>);

#[derive(Debug, Clone, PartialEq)]
pub struct Proof {
    /// SHA-256 of the stamped file.
    pub digest: [u8; 32],
    pub root: Node,
}

struct R<'a> {
    b: &'a [u8],
    items: usize,
}

impl<'a> R<'a> {
    fn bytes(&mut self, n: usize) -> Result<&'a [u8], String> {
        if n > self.b.len() {
            return Err("truncated proof".into());
        }
        let (a, rest) = self.b.split_at(n);
        self.b = rest;
        Ok(a)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.bytes(1)?[0])
    }
    fn varuint(&mut self) -> Result<u64, String> {
        let (mut v, mut shift) = (0u64, 0);
        loop {
            let b = self.u8()?;
            if shift > 63 {
                return Err("number too long".into());
            }
            v |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return Ok(v);
            }
            shift += 7;
        }
    }
    fn varbytes(&mut self, max: usize) -> Result<&'a [u8], String> {
        let n = usize::try_from(self.varuint()?).unwrap_or(usize::MAX);
        if n > max {
            return Err(format!("a field of {n} bytes (limit {max})"));
        }
        self.bytes(n)
    }

    fn node(&mut self, depth: usize) -> Result<Node, String> {
        if depth > MAX_DEPTH {
            return Err("proof nested too deep".into());
        }
        let mut items = Vec::new();
        let mut tag = self.u8()?;
        while tag == 0xff {
            let t = self.u8()?;
            items.push(self.item(t, depth)?);
            tag = self.u8()?;
        }
        items.push(self.item(tag, depth)?);
        Ok(Node(items))
    }

    fn item(&mut self, tag: u8, depth: usize) -> Result<Item, String> {
        self.items += 1;
        if self.items > MAX_ITEMS {
            return Err("proof has too many steps".into());
        }
        let op = match tag {
            0x00 => {
                let t: [u8; 8] = self.bytes(8)?.try_into().expect("8 bytes");
                let payload = self.varbytes(MAX_PAYLOAD)?;
                let mut p = R { b: payload, items: 0 };
                let att = match t {
                    BITCOIN => Att::Bitcoin(p.varuint()?),
                    PENDING => Att::Pending(
                        String::from_utf8(p.varbytes(MAX_URI)?.to_vec()).map_err(|_| "a calendar URI that isn't text")?,
                    ),
                    _ => return Ok(Item::Att(Att::Unknown(t, payload.to_vec()))),
                };
                if !p.b.is_empty() {
                    return Err("trailing bytes in an attestation".into());
                }
                return Ok(Item::Att(att));
            }
            0x08 => Op::Sha256,
            0x03 => Op::Ripemd160,
            0xf0 => Op::Append(self.varbytes(MAX_MSG)?.to_vec()),
            0xf1 => Op::Prepend(self.varbytes(MAX_MSG)?.to_vec()),
            t => return Err(format!("unsupported operation 0x{t:02x}")),
        };
        Ok(Item::Op(op, self.node(depth + 1)?))
    }
}

fn put_varuint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

fn put_varbytes(out: &mut Vec<u8>, b: &[u8]) {
    put_varuint(out, b.len() as u64);
    out.extend_from_slice(b);
}

impl Op {
    fn apply(&self, msg: &[u8]) -> Result<Vec<u8>, String> {
        let next = match self {
            Op::Sha256 => Sha256::digest(msg).to_vec(),
            Op::Ripemd160 => <ripemd::Ripemd160 as ripemd::Digest>::digest(msg).to_vec(),
            Op::Append(a) => [msg, a].concat(),
            Op::Prepend(p) => [p, msg].concat(),
        };
        if next.len() > MAX_MSG {
            return Err("a message over 4096 bytes".into());
        }
        Ok(next)
    }
}

impl Node {
    /// A calendar's answer: a tree without the file header (`POST /digest`, `GET /timestamp/…`).
    pub fn parse(bytes: &[u8]) -> Result<Node, String> {
        let mut r = R { b: bytes, items: 0 };
        let n = r.node(0)?;
        if !r.b.is_empty() {
            return Err("trailing bytes after the proof".into());
        }
        Ok(n)
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        for (i, item) in self.0.iter().enumerate() {
            if i + 1 < self.0.len() {
                out.push(0xff);
            }
            match item {
                Item::Att(a) => {
                    out.push(0x00);
                    let (tag, payload) = match a {
                        Att::Bitcoin(h) => {
                            let mut p = Vec::new();
                            put_varuint(&mut p, *h);
                            (BITCOIN, p)
                        }
                        Att::Pending(uri) => {
                            let mut p = Vec::new();
                            put_varbytes(&mut p, uri.as_bytes());
                            (PENDING, p)
                        }
                        Att::Unknown(t, p) => (*t, p.clone()),
                    };
                    out.extend_from_slice(&tag);
                    put_varbytes(out, &payload);
                }
                Item::Op(op, next) => {
                    match op {
                        Op::Sha256 => out.push(0x08),
                        Op::Ripemd160 => out.push(0x03),
                        Op::Append(a) => {
                            out.push(0xf0);
                            put_varbytes(out, a);
                        }
                        Op::Prepend(p) => {
                            out.push(0xf1);
                            put_varbytes(out, p);
                        }
                    }
                    next.write(out);
                }
            }
        }
    }

    fn walk<'a>(&'a self, msg: &[u8], out: &mut Vec<(Vec<u8>, &'a Att)>) -> Result<(), String> {
        for item in &self.0 {
            match item {
                Item::Att(a) => out.push((msg.to_vec(), a)),
                Item::Op(op, next) => next.walk(&op.apply(msg)?, out)?,
            }
        }
        Ok(())
    }

    /// Replaces each pending attestation `answer(calendar, its message)` completes with the
    /// calendar's tree for that message; returns how many it replaced.
    fn upgrade(&mut self, msg: &[u8], answer: &mut impl FnMut(&str, &[u8]) -> Option<Node>) -> Result<usize, String> {
        let mut done = 0;
        let mut items = Vec::with_capacity(self.0.len());
        for item in std::mem::take(&mut self.0) {
            match item {
                Item::Att(Att::Pending(uri)) => match answer(&uri, msg) {
                    Some(tree) => {
                        tree.walk(msg, &mut Vec::new())?;
                        items.extend(tree.0);
                        done += 1;
                    }
                    None => items.push(Item::Att(Att::Pending(uri))),
                },
                Item::Op(op, mut next) => {
                    done += next.upgrade(&op.apply(msg)?, answer)?;
                    items.push(Item::Op(op, next));
                }
                other => items.push(other),
            }
        }
        self.0 = items;
        Ok(done)
    }
}

impl Proof {
    pub fn parse(bytes: &[u8]) -> Result<Proof, String> {
        let mut r = R { b: bytes, items: 0 };
        if r.bytes(MAGIC.len()).ok() != Some(MAGIC) {
            return Err("not an OpenTimestamps proof".into());
        }
        if r.varuint()? != 1 {
            return Err("an OpenTimestamps version other than 1".into());
        }
        if r.u8()? != 0x08 {
            return Err("only SHA-256 file digests are supported".into());
        }
        let digest = r.bytes(32)?.try_into().expect("32 bytes");
        let root = r.node(0)?;
        if !r.b.is_empty() {
            return Err("trailing bytes after the proof".into());
        }
        let p = Proof { digest, root };
        p.attestations()?;
        Ok(p)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        put_varuint(&mut out, 1);
        out.push(0x08);
        out.extend_from_slice(&self.digest);
        self.root.write(&mut out);
        out
    }

    /// Every attestation with the message it attests, by replaying the operations.
    pub fn attestations(&self) -> Result<Vec<(Vec<u8>, &Att)>, String> {
        let mut out = Vec::new();
        self.root.walk(&self.digest, &mut out)?;
        Ok(out)
    }

    /// See `Node::upgrade`.
    pub fn upgrade(&mut self, mut answer: impl FnMut(&str, &[u8]) -> Option<Node>) -> Result<usize, String> {
        let digest = self.digest;
        self.root.upgrade(&digest, &mut answer)
    }

    /// A new proof for `file`: the file's digest, a random `nonce` appended (so calendars learn
    /// nothing about the file) and hashed into the commitment that each calendar's `answers`
    /// continue from. `commitment` gives the bytes to send.
    pub fn stamped(file: &[u8], nonce: &[u8; 16], answers: Vec<Node>) -> Proof {
        let digest: [u8; 32] = Sha256::digest(file).into();
        let forks = Node(answers.into_iter().flat_map(|n| n.0).collect());
        let hashed = Node(vec![Item::Op(Op::Sha256, forks)]);
        Proof { digest, root: Node(vec![Item::Op(Op::Append(nonce.to_vec()), hashed)]) }
    }
}

/// What a new proof's calendars are asked to stamp: SHA-256(SHA-256(file) ‖ nonce).
pub fn commitment(file: &[u8], nonce: &[u8; 16]) -> [u8; 32] {
    Sha256::digest([Sha256::digest(file).as_slice(), nonce].concat()).into()
}

/// What a proof says about `file`, offline.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Summary {
    /// Calendars that haven't finished (as the proof names them).
    pub pending: Vec<String>,
    /// (block height, the merkle root the proof claims for it).
    pub bitcoin: Vec<(u64, Vec<u8>)>,
    /// Notaries this build doesn't know (8-byte tags, hex); never trusted.
    pub unknown: Vec<String>,
}

pub fn summarize(file: &[u8], proof: &[u8]) -> Result<Summary, String> {
    let p = Proof::parse(proof)?;
    if p.digest[..] != Sha256::digest(file)[..] {
        return Err("the proof stamps different bytes (file digest doesn't match)".into());
    }
    let mut s = Summary::default();
    for (msg, a) in p.attestations()? {
        match a {
            Att::Bitcoin(h) => s.bitcoin.push((*h, msg)),
            // The proof's own claim, printed: no control characters.
            Att::Pending(u) => s.pending.push(u.chars().filter(|c| !c.is_control()).collect()),
            Att::Unknown(t, _) => s.unknown.push(hex(t)),
        }
    }
    Ok(s)
}

pub fn unhex(s: &str) -> Result<Vec<u8>, String> {
    let s = s.trim();
    if s.len() % 2 != 0 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("not hex".into());
    }
    Ok((0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex")).collect())
}

/// A random 16-byte nonce for `Proof::stamped`.
pub fn nonce() -> Result<[u8; 16], String> {
    let mut n = [0u8; 16];
    getrandom::getrandom(&mut n).map_err(|e| e.to_string())?;
    Ok(n)
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn sha256d(b: &[u8]) -> [u8; 32] {
    Sha256::digest(Sha256::digest(b)).into()
}

/// A block header's hash as explorers show it (byte-reversed hex).
pub fn block_hash(header: &[u8]) -> String {
    let mut h = sha256d(header);
    h.reverse();
    hex(&h)
}

/// Checks an 80-byte Bitcoin block header against the merkle root a proof claims (`root`, as the
/// proof's message) and against its own proof-of-work target; returns the block time (Unix).
/// Not checked: that the header is in the best chain (it comes from the header sources).
pub fn check_header(header: &[u8], root: &[u8]) -> Result<u32, String> {
    if header.len() != 80 {
        return Err("a block header isn't 80 bytes".into());
    }
    if header[36..68] != *root {
        return Err("the block's merkle root isn't the one the proof reaches".into());
    }
    let bits = u32::from_le_bytes(header[72..76].try_into().expect("4 bytes"));
    let target = crate::headers::target(bits).map_err(|_| "the block header's difficulty can't be read")?;
    let mut hash = sha256d(header);
    hash.reverse();
    if hash > target {
        return Err("the block header doesn't meet its own proof-of-work target".into());
    }
    Ok(u32::from_le_bytes(header[68..72].try_into().expect("4 bytes")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex(s: &str) -> Vec<u8> {
        super::unhex(s).unwrap()
    }

    /// A proof like the ones calendars make, from our own writer: two pending calendars.
    fn pending_proof(file: &[u8]) -> Proof {
        let cal = |t: u8, uri: &str| {
            Node(vec![Item::Op(
                Op::Prepend(vec![0x6a, 0xd4, 0x78, t]),
                Node(vec![Item::Op(Op::Append(vec![t; 8]), Node(vec![Item::Att(Att::Pending(uri.into()))]))]),
            )])
        };
        Proof::stamped(file, &[7; 16], vec![cal(1, "https://a.example"), cal(2, "https://b.example")])
    }

    #[test]
    fn a_proof_round_trips_and_replays() {
        let p = pending_proof(b"signature file");
        let bytes = p.to_bytes();
        assert_eq!(Proof::parse(&bytes).unwrap(), p);
        assert_eq!(Proof::parse(&bytes).unwrap().to_bytes(), bytes);
        let s = summarize(b"signature file", &bytes).unwrap();
        assert_eq!(s.pending, ["https://a.example", "https://b.example"]);
        assert!(s.bitcoin.is_empty());
        assert!(summarize(b"other bytes", &bytes).unwrap_err().contains("different bytes"));
        // The calendars saw only the commitment, which hides the file behind the nonce.
        let atts = p.attestations().unwrap();
        let c = commitment(b"signature file", &[7; 16]);
        assert_eq!(atts[0].0, [&[0x6a, 0xd4, 0x78, 1], &c[..], &[1; 8]].concat());
    }

    #[test]
    fn upgrading_replaces_only_the_pending_attestations_answered() {
        let mut p = pending_proof(b"f");
        let mut asked = Vec::new();
        let n = p
            .upgrade(|uri, msg| {
                asked.push(uri.to_string());
                (uri == "https://b.example").then(|| {
                    assert_eq!(msg.len(), 4 + 32 + 8);
                    Node(vec![Item::Op(Op::Sha256, Node(vec![Item::Att(Att::Bitcoin(912_095))]))])
                })
            })
            .unwrap();
        assert_eq!((n, asked.len()), (1, 2));
        let s = summarize(b"f", &p.to_bytes()).unwrap();
        assert_eq!(s.pending, ["https://a.example"]);
        assert_eq!(s.bitcoin.len(), 1);
        assert_eq!(s.bitcoin[0].0, 912_095);
    }

    #[test]
    fn garbled_or_truncated_files_are_errors_never_panics() {
        let good = pending_proof(b"f").to_bytes();
        for n in 0..good.len() {
            assert!(Proof::parse(&good[..n]).is_err(), "truncated at {n}");
        }
        // Every byte flipped in turn, and a pseudo-random spray of changes: errors or other trees, no panic.
        let mut x: u32 = 2_463_534_242;
        for round in 0..20_000 {
            let mut b = good.clone();
            if round < good.len() {
                b[round] ^= 0xff;
            } else {
                for _ in 0..1 + round % 4 {
                    x ^= x << 13;
                    x ^= x >> 17;
                    x ^= x << 5;
                    let i = x as usize % b.len();
                    b[i] = (x >> 8) as u8;
                }
            }
            let _ = Proof::parse(&b);
            let _ = Node::parse(&b[MAGIC.len()..]);
        }
        // Hostile shapes: deep nesting, a huge length, too many forks, an over-long message, old ops.
        let head = |rest: &[u8]| [MAGIC, &[1, 0x08], &[0; 32], rest].concat();
        assert!(Proof::parse(&head(&[0x08; 300])).unwrap_err().contains("deep"));
        assert!(Proof::parse(&head(&[0xf0, 0xff, 0xff, 0xff, 0xff, 0x0f])).is_err());
        let fork: Vec<u8> = [&[0xff, 0x00][..], &BITCOIN, &[1, 1]].concat();
        assert!(Proof::parse(&head(&fork.repeat(MAX_ITEMS + 1))).unwrap_err().contains("too many"));
        let grow: Vec<u8> = [0xf0, 0x80, 0x20].iter().copied().chain([0; 4096]).collect();
        assert!(Proof::parse(&head(&[grow.as_slice(), &[0x00], &BITCOIN, &[1, 1]].concat())).unwrap_err().contains("4096"));
        for op in [0x02, 0x67, 0xf2, 0xf3] {
            assert!(Proof::parse(&head(&[op])).unwrap_err().contains("unsupported"), "op {op:#x}");
        }
        assert!(Proof::parse(&[MAGIC, &[1, 0x02]].concat()).unwrap_err().contains("SHA-256"));
        assert!(Proof::parse(&[good.as_slice(), &[0]].concat()).unwrap_err().contains("trailing"));
    }

    /// 2015-era proofs hash with RIPEMD-160 along the path. A proof built here (no upstream fixture:
    /// those are LGPL), checked against the published RIPEMD-160 test vectors.
    #[test]
    fn ripemd160_proofs_parse_and_replay() {
        assert_eq!(hex(&Op::Ripemd160.apply(b"").unwrap()), "9c1185a5c5e9fc54612808977ee8f548b2258d31");
        assert_eq!(hex(&Op::Ripemd160.apply(b"abc").unwrap()), "8eb208f7e05d987a9b044a8e98c6b087f15a0bfc");
        let file = b"stamped in 2015";
        let digest: [u8; 32] = Sha256::digest(file).into();
        let tail = Node(vec![Item::Op(Op::Sha256, Node(vec![Item::Att(Att::Bitcoin(358_391))]))]);
        let p = Proof { digest, root: Node(vec![Item::Op(Op::Prepend(vec![1; 4]), Node(vec![Item::Op(Op::Ripemd160, tail)]))]) };
        let bytes = p.to_bytes();
        assert_eq!(bytes[MAGIC.len() + 2 + 32 + 6], 0x03);
        assert_eq!(Proof::parse(&bytes).unwrap(), p);
        let s = summarize(file, &bytes).unwrap();
        let ripe = Op::Ripemd160.apply(&[&[1; 4], &digest[..]].concat()).unwrap();
        assert_eq!(s.bitcoin, [(358_391, Sha256::digest(&ripe).to_vec())]);
    }

    #[test]
    fn a_block_header_is_checked_for_merkle_root_and_work() {
        // Bitcoin block 912095 (from a 2025 proof's `ots info` in the spike); header as both
        // explorers served it on 18 Oct 2026. Public chain data.
        let header = unhex(include_str!("../testdata/ots/912095.header"));
        let mut root = unhex("936131994129988e1932d8c1c6a9780fd42ae4c87204cfb0ac852be1250039ae");
        root.reverse();
        let t = check_header(&header, &root).unwrap();
        assert_eq!(crate::time::rfc3339(i64::from(t)).get(..7), Some("2025-08"));
        assert!(check_header(&header, &[0; 32]).unwrap_err().contains("merkle"));
        let mut weak = header.clone();
        weak[0] ^= 1; // a different header: its hash no longer meets the target
        assert!(check_header(&weak, &root).unwrap_err().contains("proof-of-work"));
        assert!(check_header(&header[..79], &root).is_err());
    }

    /// A real proof: 32 random bytes (`random32.bin`) stamped at four public calendars at
    /// 2026-10-18T07:42:42Z (the spike's live stamp), upgraded by the reference client at 09:04Z once
    /// alice.btc.calendar.opentimestamps.org's transaction had six confirmations in block 971959 (the
    /// other three branches are still pending in it), with the block's header as mempool.space and
    /// blockstream.info both served it. Our own fixture: no project data.
    #[test]
    fn a_real_confirmed_proof_checks_offline() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/ots");
        let file = std::fs::read(dir.join("random32.bin")).unwrap();
        let bytes = std::fs::read(dir.join("random32.bin.ots")).unwrap();
        // Another implementation wrote it; we read and write it byte for byte.
        assert_eq!(Proof::parse(&bytes).unwrap().to_bytes(), bytes);
        let s = summarize(&file, &bytes).unwrap();
        assert!(!s.bitcoin.is_empty(), "{s:?}");
        for (height, root) in &s.bitcoin {
            let header = unhex(&std::fs::read_to_string(dir.join(format!("{height}.header"))).unwrap());
            let t = check_header(&header, root).unwrap();
            assert!(crate::time::rfc3339(i64::from(t)).as_str() > "2026-10-18T05:42:42Z", "block {height}");
        }
        let mut other = file.clone();
        other[0] ^= 1;
        assert!(summarize(&other, &bytes).is_err());
    }
}
