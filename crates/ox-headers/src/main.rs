//! `ox-headers`: keeps the Bitcoin header file that `ox verify --online` reads for a `light-client`
//! header source. Headers only, from Bitcoin peers that serve
//! compact filters (BIP 157), through `bip157` 0.7.0, which checks links, work and retargets as it
//! syncs; `ox_store::headers::check` checks the whole file again before every write.
//!   ox-headers sync [--file PATH] [--peer HOST:PORT]... [--socks5 IP:PORT]
//!   ox-headers status [--file PATH]
//! The file is per user (`headers::default_file`), never in a project's `.ox/`.

use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use bip157::chain::{BlockHeaderChanges, ChainState, IndexedHeader};
use bip157::{Builder, Event, Network, Socks5Proxy, TrustedPeer};
use bitcoin::consensus::{deserialize, serialize};
use ox_store::headers::{self, CHECKPOINT, Chain};

const USAGE: &str =
    "usage: ox-headers sync [--file PATH] [--peer HOST:PORT]... [--socks5 IP:PORT] | ox-headers status [--file PATH]";
/// Progress is written to disk every this many new headers (a sync from genesis takes about 16 min).
const SAVE_EVERY: usize = 100_000;

fn main() -> ExitCode {
    match run(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ox-headers: {e}");
            ExitCode::FAILURE
        }
    }
}

struct Opts {
    file: PathBuf,
    peers: Vec<TrustedPeer>,
    socks5: Option<SocketAddr>,
}

fn run(args: &[String]) -> Result<(), String> {
    let (cmd, rest) = args.split_first().ok_or(USAGE)?;
    let o = parse(rest)?;
    match cmd.as_str() {
        "status" if o.peers.is_empty() && o.socks5.is_none() => status(&o.file),
        "sync" => sync(o),
        _ => Err(USAGE.into()),
    }
}

fn parse(args: &[String]) -> Result<Opts, String> {
    let (mut file, mut peers, mut socks5) = (None, Vec::new(), None);
    for pair in args.chunks(2) {
        let [flag, v] = pair else { return Err(USAGE.into()) };
        match flag.as_str() {
            "--file" => file = Some(PathBuf::from(v)),
            "--peer" => peers.push(v.clone()),
            "--socks5" => socks5 = Some(v.parse().map_err(|_| format!("--socks5 {v}: give IP:PORT"))?),
            _ => return Err(USAGE.into()),
        }
    }
    let file = file.or_else(headers::default_file).ok_or("no app-data directory found: give --file")?;
    let peers = peers.iter().map(|p| peer(p, socks5.is_some())).collect::<Result<_, _>>()?;
    Ok(Opts { file, peers, socks5 })
}

fn peer(p: &str, socks: bool) -> Result<TrustedPeer, String> {
    if let Ok(a) = p.parse::<SocketAddr>() {
        return Ok(TrustedPeer::from_socket_addr(a));
    }
    let (host, port) = p
        .rsplit_once(':')
        .and_then(|(h, port)| Some((h, port.parse::<u16>().ok()?)))
        .filter(|(h, _)| !h.is_empty())
        .ok_or(format!("--peer {p}: give HOST:PORT"))?;
    if socks {
        return Err(format!("--peer {p}: with --socks5, give IP:PORT (a host name would be looked up outside the proxy)"));
    }
    Ok(TrustedPeer::from_hostname(host, port))
}

/// Our own check, with the release checkpoint once the chain is long enough to contain it.
fn checked(b: &[u8]) -> Result<Chain, String> {
    headers::check(b, (b.len() / 80 > CHECKPOINT.height as usize).then_some(&CHECKPOINT))
}

fn status(path: &Path) -> Result<(), String> {
    let b = std::fs::read(path).map_err(|x| format!("{}: {x}: run `ox-headers sync`", path.display()))?;
    let c = checked(&b).map_err(|x| format!("{}: {x}", path.display()))?;
    println!("file: {}", path.display());
    println!("height: {}\ntip: {}\ntip time: {}\nchain work: {}", c.height, c.tip_hash, ox_store::rfc3339(i64::from(c.tip_time)), c.work);
    println!(
        "release checkpoint: block {} {} ({}): {}",
        CHECKPOINT.height,
        CHECKPOINT.hash,
        ox_store::rfc3339(i64::from(CHECKPOINT.time)),
        if c.height >= CHECKPOINT.height { "on this chain" } else { "not reached yet: run `ox-headers sync`" }
    );
    Ok(())
}

/// Puts a header at its height: appends it, or (a different one) replaces it and drops what's above.
fn put(buf: &mut Vec<u8>, ih: &IndexedHeader) -> Result<(), String> {
    let at = ih.height as usize * 80;
    if at > buf.len() {
        return Err(format!("a peer sent block {} but the file ends at block {}", ih.height, (buf.len() / 80).saturating_sub(1)));
    }
    let h = serialize(&ih.header);
    if buf.get(at..at + 80) != Some(&h[..]) {
        buf.truncate(at);
        buf.extend_from_slice(&h);
    }
    Ok(())
}

fn apply(buf: &mut Vec<u8>, change: BlockHeaderChanges) -> Result<(), String> {
    match change {
        BlockHeaderChanges::Connected(ih) => put(buf, &ih),
        BlockHeaderChanges::Reorganized { mut accepted, .. } => {
            accepted.sort_by_key(|h| h.height);
            for ih in &accepted {
                put(buf, ih)?;
            }
            if let Some(top) = accepted.last() {
                buf.truncate((top.height as usize + 1) * 80);
            }
            Ok(())
        }
        BlockHeaderChanges::ForkAdded(_) => Ok(()),
    }
}

/// Checks `buf`, then replaces the file with it whole: a temporary file, then a rename.
fn save(path: &Path, buf: &[u8]) -> Result<Chain, String> {
    let c = checked(buf).map_err(|x| format!("not saved: the synced headers fail our check: {x}"))?;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".part");
    let write = || -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(buf)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    };
    write().map_err(|x| format!("{}: {x}", path.display()))?;
    Ok(c)
}

fn sync(o: Opts) -> Result<(), String> {
    let mut buf = match std::fs::read(&o.file) {
        Ok(b) => {
            checked(&b).map_err(|x| format!("{}: {x}; move it away and sync again", o.file.display()))?;
            b
        }
        Err(x) if x.kind() == std::io::ErrorKind::NotFound => headers::genesis(),
        Err(x) => return Err(format!("{}: {x}", o.file.display())),
    };
    if let Some(d) = o.file.parent() {
        std::fs::create_dir_all(d).map_err(|x| format!("{}: {x}", d.display()))?;
    }
    let start = buf.len() / 80 - 1;
    eprintln!("ox-headers: {} reaches block {start}; asking Bitcoin peers for newer headers", o.file.display());
    let mut b = Builder::new(Network::Bitcoin).headers_only_sync();
    if !o.peers.is_empty() {
        b = b.add_peers(o.peers).whitelist_only();
    }
    if let Some(s) = o.socks5 {
        b = b.socks5_proxy(Socks5Proxy::new(s));
    }
    if start > 0 {
        // Two retarget periods back, so bip157 can check the next retarget. It trusts this
        // snapshot, which is why the file was checked above.
        let snap = ((start + 1).saturating_sub(4033)..=start)
            .map(|h| Ok(IndexedHeader { height: h as u32, header: deserialize(&buf[h * 80..h * 80 + 80]).map_err(|x| x.to_string())? }))
            .collect::<Result<_, String>>()?;
        b = b.chain_state(ChainState::Snapshot(snap));
    }
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().map_err(|x| x.to_string())?;
    let (outcome, ended) = rt.block_on(follow(b, &mut buf, &o.file));
    // Whatever came in and passes our check is kept, even if the sync stopped early.
    let c = if buf.len() / 80 - 1 > start { save(&o.file, &buf)? } else { checked(&buf)? };
    outcome?;
    if let Some(why) = ended {
        return Err(format!("stopped at block {} before reaching the peers' tip ({why}); run it again", c.height));
    }
    println!(
        "ox-headers: synced to block {} ({}, {}); {} new; chain work {}",
        c.height,
        c.tip_hash,
        ox_store::rfc3339(i64::from(c.tip_time)),
        c.height - start as u64,
        c.work
    );
    Ok(())
}

/// Runs the node until it reports the tip; returns the outcome and, if it never got there, why.
async fn follow(b: Builder, buf: &mut Vec<u8>, path: &Path) -> (Result<(), String>, Option<String>) {
    let (node, client) = b.build();
    let task = tokio::spawn(async move { node.run().await });
    let bip157::Client { requester, mut info_rx, mut warn_rx, mut event_rx } = client;
    let mut saved = buf.len();
    let mut synced = false;
    let outcome = loop {
        tokio::select! {
            Some(e) = event_rx.recv() => match e {
                Event::ChainUpdate(c) => {
                    if let Err(x) = apply(buf, c) {
                        break Err(x);
                    }
                    if buf.len() / 80 % 50_000 == 0 {
                        eprintln!("ox-headers: block {}", buf.len() / 80 - 1);
                    }
                    if buf.len() >= saved + SAVE_EVERY * 80 {
                        if let Err(x) = save(path, buf) {
                            break Err(x);
                        }
                        saved = buf.len();
                    }
                }
                Event::FiltersSynced(_) => {
                    synced = true;
                    break Ok(());
                }
                _ => {}
            },
            Some(_) = info_rx.recv() => {}
            Some(w) = warn_rx.recv() => eprintln!("ox-headers: {w}"),
            else => break Ok(()),
        }
    };
    let _ = requester.shutdown();
    let ended = task.await;
    (outcome, (!synced).then(|| format!("{ended:?}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn real() -> Vec<u8> {
        std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../ox-store/testdata/headers/first-2101.bin")).unwrap()
    }

    #[test]
    fn headers_go_in_by_height_and_reorgs_replace_the_top() {
        let real = real();
        let ih = |h: usize| IndexedHeader { height: h as u32, header: deserialize(&real[h * 80..h * 80 + 80]).unwrap() };
        let mut buf = real[..80 * 2000].to_vec();
        apply(&mut buf, BlockHeaderChanges::Connected(ih(2000))).unwrap();
        assert_eq!(buf, real[..80 * 2001]);
        apply(&mut buf, BlockHeaderChanges::Connected(ih(1999))).unwrap(); // already there
        assert_eq!(buf, real[..80 * 2001]);
        assert!(apply(&mut buf, BlockHeaderChanges::Connected(ih(2005))).unwrap_err().contains("ends at block 2000"));
        // Another block at 2000 replaces ours.
        let mut other = ih(2000);
        other.header.nonce ^= 1;
        let new = serialize(&other.header);
        apply(&mut buf, BlockHeaderChanges::Reorganized { accepted: vec![other], reorganized: vec![ih(2000)] }).unwrap();
        assert_eq!((buf.len(), &buf[2000 * 80..]), (80 * 2001, &new[..]));
        // A reorg onto a shorter chain with more work drops the blocks above it.
        apply(&mut buf, BlockHeaderChanges::Reorganized { accepted: vec![ih(1998)], reorganized: vec![ih(1999)] }).unwrap();
        assert_eq!(buf, real[..80 * 1999]);
        apply(&mut buf, BlockHeaderChanges::ForkAdded(ih(1990))).unwrap();
        assert_eq!(buf, real[..80 * 1999]);
    }

    #[test]
    fn saving_checks_first_and_replaces_the_file_whole() {
        let real = real();
        let d = std::env::temp_dir().join(format!("ox-headers-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("bitcoin-headers.bin");
        assert_eq!(save(&f, &real).unwrap().height, 2100);
        assert_eq!(std::fs::read(&f).unwrap(), real);
        assert!(!d.join("bitcoin-headers.bin.part").exists());
        let mut bad = real.clone();
        bad[1000 * 80 + 5] ^= 1;
        assert!(save(&f, &bad).unwrap_err().contains("not saved"));
        assert_eq!(std::fs::read(&f).unwrap(), real, "the old file stays");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn options_and_peers() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let o = parse(&s(&["--file", "/x/h.bin", "--peer", "1.2.3.4:8333", "--peer", "node.example:8333"])).unwrap();
        assert_eq!((o.file, o.peers.len(), o.socks5), (PathBuf::from("/x/h.bin"), 2, None));
        assert!(parse(&s(&["--socks5", "127.0.0.1:9050", "--peer", "[::1]:8333"])).unwrap().socks5.is_some());
        assert!(parse(&s(&["--socks5", "127.0.0.1:9050", "--peer", "node.example:8333"])).err().unwrap().contains("outside the proxy"));
        for bad in [&["--socks5", "tor:9050"][..], &["--peer", "node.example"], &["--peer", ":8333"], &["--file"], &["--bogus", "x"]] {
            assert!(parse(&s(bad)).is_err(), "{bad:?}");
        }
        assert!(run(&s(&["status", "--peer", "1.2.3.4:8333"])).unwrap_err().contains("usage"));
        assert!(run(&s(&["status", "--file", "/nonexistent/h.bin"])).unwrap_err().contains("ox-headers sync"));
        assert!(run(&s(&["frobnicate"])).unwrap_err().contains("usage"));
    }
}
