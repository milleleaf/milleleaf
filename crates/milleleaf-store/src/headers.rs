//! Bitcoin block headers for the `light-client` header source (plan decision 5, amended 18 Oct
//! 2026). `milleleaf-headers sync` keeps a per-user file of every
//! header from genesis, 80 bytes per height; `mleaf verify --online` reads it. This module checks the
//! whole file without trusting whoever synced it: genesis, links, each header's proof of work, every
//! difficulty retarget, median time past, and the release checkpoint below (which also sets the
//! minimum chain work). Pure: no network, no new dependencies.

use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::path::PathBuf;

/// A block every release's chain must contain, with the chain work up to it (hex, as Bitcoin Core's
/// `chainwork`). A chain without it, or with less work at it, is someone else's. **Refreshed each
/// release**; the nightly warns when its block is 6 months old.
pub struct Checkpoint {
    pub height: u64,
    /// As explorers show it (byte-reversed).
    pub hash: &'static str,
    /// Block time (Unix).
    pub time: u32,
    pub work: &'static str,
}

/// Block 971800 (2026-10-17 03:30 UTC): from the spike's synced file, hash matched by mempool.space and
/// blockstream.info on 18 Oct 2026; work computed by this module and by the spike's Python check.
pub const CHECKPOINT: Checkpoint = Checkpoint {
    height: 971_800,
    hash: "000000000000000000018f8688983b23328129fe1758560bf5e838fe215381cf",
    time: 1_792_207_805,
    work: "0000000000000000000000000000000000000001503d63596ae501c43845d86a",
};

const GENESIS: &str = "0100000000000000000000000000000000000000000000000000000000000000000000003ba3edfd7a7b12b27ac72c3e67768f617fc81bc3888a51323a9fb8aa4b1e5e4a29ab5f49ffff001d1dac2b7c";
const POW_LIMIT: u32 = 0x1d00_ffff;
const SPAN: u64 = 14 * 24 * 60 * 60;

/// The genesis block's header.
pub fn genesis() -> Vec<u8> {
    crate::ots::unhex(GENESIS).expect("hex")
}

/// Where `milleleaf-headers` keeps the file when no path is given: the user's app-data directory, never a
/// project's `.mleaf/` (it is the same chain for every project).
pub fn default_file() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let base = if cfg!(windows) {
        env("APPDATA")
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library/Application Support"))
    } else {
        env("XDG_DATA_HOME").or_else(|| env("HOME").map(|h| h.join(".local/share")))
    };
    base.map(|b| b.join("mleaf").join("bitcoin-headers.bin"))
}

/// A checked header file.
#[derive(Debug, Clone, PartialEq)]
pub struct Chain {
    /// Of the last header.
    pub height: u64,
    pub tip_hash: String,
    pub tip_time: u32,
    /// Total chain work (hex).
    pub work: String,
}

/// 256-bit numbers as big-endian bytes, the order targets and hashes compare in.
type U256 = [u8; 32];

/// The target `bits` encodes (Bitcoin's "compact" form).
pub(crate) fn target(bits: u32) -> Result<U256, String> {
    let (exp, mant) = ((bits >> 24) as usize, bits & 0x007f_ffff);
    if !(3..=32).contains(&exp) || bits & 0x0080_0000 != 0 || mant == 0 {
        return Err(format!("difficulty bits {bits:08x} can't be read"));
    }
    let mut t = [0u8; 32];
    t[32 - exp..35 - exp].copy_from_slice(&mant.to_be_bytes()[1..4]);
    Ok(t)
}

fn compact(t: &U256) -> u32 {
    let Some(i) = t.iter().position(|&b| b != 0) else { return 0 };
    let byte = |j: usize| u32::from(t.get(j).copied().unwrap_or(0));
    let (mut size, mut mant) = ((32 - i) as u32, byte(i) << 16 | byte(i + 1) << 8 | byte(i + 2));
    if mant & 0x0080_0000 != 0 {
        mant >>= 8;
        size += 1;
    }
    size << 24 | mant
}

/// The bits a retarget block must have: Bitcoin Core's `CalculateNextWorkRequired` for mainnet. The
/// period is measured from its first block to its last (2015 intervals, the "off by one" Bitcoin
/// keeps), clamped to a quarter and four times two weeks, and the target never passes the limit.
pub(crate) fn next_bits(prev_bits: u32, first_time: u32, last_time: u32) -> Result<u32, String> {
    let span = (i64::from(last_time) - i64::from(first_time)).clamp((SPAN / 4) as i64, (SPAN * 4) as i64) as u64;
    // target × span / SPAN, in five 64-bit limbs (least significant first) so nothing overflows.
    let t = target(prev_bits)?;
    let mut limbs = [0u64; 5];
    for (k, chunk) in t.chunks(8).rev().enumerate() {
        limbs[k] = u64::from_be_bytes(chunk.try_into().expect("8 bytes"));
    }
    let mut carry = 0u128;
    for l in &mut limbs {
        let p = u128::from(*l) * u128::from(span) + carry;
        *l = p as u64;
        carry = p >> 64;
    }
    let mut rem = 0u128;
    for l in limbs.iter_mut().rev() {
        let cur = rem << 64 | u128::from(*l);
        *l = (cur / u128::from(SPAN)) as u64;
        rem = cur % u128::from(SPAN);
    }
    let limit = target(POW_LIMIT)?;
    if limbs[4] != 0 {
        return Ok(compact(&limit));
    }
    let mut new = [0u8; 32];
    for (k, chunk) in new.chunks_mut(8).rev().enumerate() {
        chunk.copy_from_slice(&limbs[k].to_be_bytes());
    }
    Ok(compact(if new > limit { &limit } else { &new }))
}

const ONE: U256 = {
    let mut one = [0u8; 32];
    one[31] = 1;
    one
};

fn add(a: &mut U256, b: &U256) {
    let mut carry = 0u16;
    for i in (0..32).rev() {
        let s = u16::from(a[i]) + u16::from(b[i]) + carry;
        a[i] = s as u8;
        carry = s >> 8;
    }
}

/// A block's expected work, 2^256 / (target + 1), as Bitcoin Core computes it: ~t / (t + 1) + 1.
fn work(t: &U256) -> U256 {
    let mut d = *t;
    add(&mut d, &ONE);
    let n: U256 = t.map(|b| !b);
    // Binary long division; the remainder stays below d, so one spare byte is enough.
    let (mut q, mut r) = ([0u8; 32], [0u8; 33]);
    let mut d33 = [0u8; 33];
    d33[1..].copy_from_slice(&d);
    for bit in 0..256 {
        let mut c = (n[bit / 8] >> (7 - bit % 8)) & 1;
        for x in r.iter_mut().rev() {
            let nc = *x >> 7;
            *x = *x << 1 | c;
            c = nc;
        }
        if r.cmp(&d33) != Ordering::Less {
            let mut borrow = 0i16;
            for i in (0..33).rev() {
                let v = i16::from(r[i]) - i16::from(d33[i]) - borrow;
                r[i] = v.rem_euclid(256) as u8;
                borrow = i16::from(v < 0);
            }
            q[bit / 8] |= 1 << (7 - bit % 8);
        }
    }
    add(&mut q, &ONE);
    q
}

fn sha256d(b: &[u8]) -> [u8; 32] {
    Sha256::digest(Sha256::digest(b)).into()
}

fn u32_at(h: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(h[at..at + 4].try_into().expect("4 bytes"))
}

/// Checks a whole header file (80 bytes per height, from genesis). With `cp`, the chain must reach
/// that checkpoint, with exactly its hash and chain work. Never panics on bad input.
pub fn check(file: &[u8], cp: Option<&Checkpoint>) -> Result<Chain, String> {
    if file.is_empty() || !file.len().is_multiple_of(80) {
        return Err(format!("the header file is {} bytes, not a whole number of 80-byte headers", file.len()));
    }
    if file[..80] != genesis()[..] {
        return Err("the header file doesn't start at Bitcoin's genesis block".into());
    }
    let n = file.len() / 80;
    let h = |i: usize| &file[i * 80..i * 80 + 80];
    let (mut total, mut block_work, mut work_bits) = ([0u8; 32], [0u8; 32], 0u32);
    let mut prev = [0u8; 32];
    for i in 0..n {
        let hdr = h(i);
        let bits = u32_at(hdr, 72);
        if i > 0 {
            if hdr[4..36] != prev {
                return Err(format!("block {i} doesn't link to block {}", i - 1));
            }
            let want = if i % 2016 == 0 {
                next_bits(u32_at(h(i - 1), 72), u32_at(h(i - 2016), 68), u32_at(h(i - 1), 68))?
            } else {
                u32_at(h(i - 1), 72)
            };
            if bits != want {
                return Err(format!("block {i} has difficulty bits {bits:08x}, not {want:08x}"));
            }
            let mut past: Vec<u32> = (i.saturating_sub(11)..i).map(|j| u32_at(h(j), 68)).collect();
            past.sort_unstable();
            if u32_at(hdr, 68) <= past[past.len() / 2] {
                return Err(format!("block {i}'s time isn't after the median of the 11 before it"));
            }
        }
        prev = sha256d(hdr);
        let t = target(bits).map_err(|x| format!("block {i}: {x}"))?;
        let mut be = prev;
        be.reverse();
        if be > t {
            return Err(format!("block {i} doesn't meet its own proof-of-work target"));
        }
        if bits != work_bits || i == 0 {
            (block_work, work_bits) = (work(&t), bits);
        }
        add(&mut total, &block_work);
        if let Some(cp) = cp.filter(|cp| cp.height == i as u64) {
            if crate::ots::hex(&be) != cp.hash {
                return Err(format!("block {i} isn't the release checkpoint {}: a different chain", cp.hash));
            }
            if crate::ots::hex(&total) != cp.work {
                return Err(format!("the chain work at block {i} isn't the release checkpoint's: a different chain"));
            }
        }
    }
    let height = (n - 1) as u64;
    if let Some(cp) = cp.filter(|cp| cp.height > height) {
        return Err(format!(
            "the header file ends at block {height}, below the release checkpoint (block {}): run `milleleaf-headers sync`",
            cp.height
        ));
    }
    let mut tip = prev;
    tip.reverse();
    Ok(Chain { height, tip_hash: crate::ots::hex(&tip), tip_time: u32_at(h(n - 1), 68), work: crate::ots::hex(&total) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/headers/first-2101.bin")).unwrap()
    }

    /// Block 2100 of the real chain: 2101 blocks at difficulty 1, 0x100010001 each.
    const CP: Checkpoint = Checkpoint {
        height: 2100,
        hash: "00000000d5092034c3a92473d42f28e3fd279f0652e2d5b0049c7d40fcbc990b",
        time: 1_233_136_967,
        work: "0000000000000000000000000000000000000000000000000000083508350835",
    };

    #[test]
    fn the_first_2101_real_headers_check() {
        let f = fixture();
        assert_eq!(f[..80], genesis()[..]);
        let c = check(&f, Some(&CP)).unwrap();
        assert_eq!((c.height, c.tip_hash.as_str(), c.tip_time, c.work.as_str()), (2100, CP.hash, CP.time, CP.work));
        assert_eq!(check(&f[..80 * 2000], None).unwrap().height, 1999);
        assert_eq!(check(&f[..80], None).unwrap().work, format!("{:064x}", 0x1_0001_0001u64));
    }

    /// Every mainnet retarget so far, against our arithmetic.
    #[test]
    fn every_real_retarget_is_recomputed() {
        let text = include_str!("../testdata/headers/retargets.txt");
        let mut n = 0;
        for l in text.lines().filter(|l| !l.starts_with('#')) {
            let f: Vec<&str> = l.split(' ').collect();
            let hex = |s: &str| u32::from_str_radix(s, 16).unwrap();
            assert_eq!(next_bits(hex(f[1]), f[2].parse().unwrap(), f[3].parse().unwrap()).unwrap(), hex(f[4]), "block {}", f[0]);
            n += 1;
        }
        assert!(n >= 482, "{n}");
        // The clamp: a period ten times too slow moves the target by four times only; ten times
        // too fast by a quarter. And never past the limit.
        let b = 0x1702_5ef3; // a 2026 value
        let t = |tenths: u64| next_bits(b, 0, (SPAN * tenths / 10) as u32).unwrap();
        assert_eq!((t(100), t(1)), (t(40), t(2)));
        assert_ne!(t(40), t(39));
        assert_ne!(t(2), t(3));
        let mut x = target(b).unwrap(); // × 4: two bits left
        let mut c = 0u8;
        for v in x.iter_mut().rev() {
            (*v, c) = (*v << 2 | c, *v >> 6);
        }
        assert_eq!(t(40), compact(&x));
        assert_eq!(next_bits(POW_LIMIT, 0, u32::MAX).unwrap(), POW_LIMIT);
    }

    #[test]
    fn broken_files_are_refused_never_panics() {
        let f = fixture();
        let refused = |b: &[u8], what: &str| {
            let e = check(b, Some(&CP)).unwrap_err();
            assert!(e.contains(what), "{what}: {e}");
        };
        refused(&f[..f.len() - 1], "whole number");
        refused(&[], "whole number");
        refused(&f[..80 * 1000], "below the release checkpoint");
        let mut b = f.clone();
        b[0] ^= 1;
        refused(&b, "genesis");
        // A broken link: block 1500's previous hash changed.
        let mut b = f.clone();
        b[1500 * 80 + 4] ^= 1;
        refused(&b, "block 1500 doesn't link");
        // Wrong bits on the last block (an easier target than the chain allows).
        let mut b = f.clone();
        b[2100 * 80 + 75] = 0x1e;
        refused(&b, "difficulty bits");
        // Bad proof of work: the last block's nonce changed (nothing links to it).
        let mut b = f.clone();
        b[2100 * 80 + 76] ^= 1;
        refused(&b, "proof-of-work");
        // A time that isn't after the median of the 11 before.
        let mut b = f.clone();
        b[2100 * 80 + 68..2100 * 80 + 72].copy_from_slice(&1_231_006_505u32.to_le_bytes());
        refused(&b, "median");
        // A different (low-work) chain: the checkpoint's hash or work isn't there.
        let other = Checkpoint { hash: "00000000839a8e6886ab5951d76f411475428afc90947ee320161bbf18eb6048", ..CP };
        assert!(check(&f, Some(&other)).unwrap_err().contains("a different chain"));
        let more = Checkpoint { work: "0000000000000000000000000000000000000000000000000000083508350836", ..CP };
        assert!(check(&f, Some(&more)).unwrap_err().contains("chain work"));
        let later = Checkpoint { height: 5000, ..CP };
        assert!(check(&f, Some(&later)).unwrap_err().contains("milleleaf-headers sync"));
        // A spray of changes: errors or (rarely) another valid chain, never a panic.
        let mut x: u32 = 2_463_534_242;
        for _ in 0..300 {
            let mut b = f.clone();
            for _ in 0..3 {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                let i = x as usize % b.len();
                b[i] = (x >> 8) as u8;
            }
            let _ = check(&b[..b.len() - (x as usize % 3) * 80], Some(&CP));
        }
        for bits in [0, 0x0080_0000, 0x2100_ffff, 0x0200_ffff, 0x1d80_ffff, u32::MAX] {
            assert!(target(bits).is_err(), "{bits:08x}");
        }
    }

    /// The release checkpoint against a whole synced file: `MLEAF_HEADERS_FILE=<path>`, by hand or on
    /// the box after `milleleaf-headers sync` (the file is 78 MB, not in the repo).
    #[test]
    #[ignore = "needs a synced header file (MLEAF_HEADERS_FILE)"]
    fn the_release_checkpoint_is_on_a_synced_chain() {
        let f = std::fs::read(milleleaf_core::env("HEADERS_FILE").unwrap()).unwrap();
        let t = std::time::Instant::now();
        let c = check(&f, Some(&CHECKPOINT)).unwrap();
        eprintln!("{} headers checked in {:?}: {c:?}", c.height + 1, t.elapsed());
        assert_eq!(u32_at(&f[CHECKPOINT.height as usize * 80..], 68), CHECKPOINT.time);
    }
}
