use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use ox_core::compose::{Composer, Flat};
use ox_core::model::{Header, Layer, Node};
use serde_json::{Value, json};

use crate::{CommitOptions, KeyPair, LayerStatus, SigInfo, Store, StoreError, strength, tsr_of};

const TABLE: &str = "3f1c2a90-0000-4c1e-9a00-000000000012";
const TOTAL: &str = "3f1c2a90-0000-4c1e-9a00-000000000013";
const REGION_COUNT: &str = "3f1c2a90-0000-4c1e-9a00-000000000031";

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let p = std::env::temp_dir().join(format!("ox-store-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn detached_signatures_agree_with_the_store() {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as B64;
    use crate::{check_signature, sign_hash};
    let t = TempDir::new("detached-signatures");
    let store = hello(&t);
    let layer = layer("detached", vec![(TOTAL, "office::data::value", json!({"value": 12}))]);
    let (hash, _) = store.put_layer(&layer).unwrap();
    let key = KeyPair::generate_demo("Demo editor").unwrap();
    let known: Vec<_> = key.publics().into_iter().map(|p| (key.name.clone(), p)).collect();
    assert_eq!(known.len(), 2, "demo keys have only Ed25519 and ML-DSA-65");
    let signatures = sign_hash(&hash, &key, false, "2026-10-22T10:00:00Z").unwrap();
    assert_eq!(signatures.len(), 2);
    for signature in &signatures {
        let info = check_signature(signature, &hash, &known);
        assert!(info.valid);
        assert_eq!(info.known_as.as_deref(), Some("Demo editor"));
        let unknown = check_signature(signature, &hash, &[]);
        assert!(unknown.valid);
        assert!(unknown.known_as.is_none());
        assert!(!check_signature(signature, &"0".repeat(64), &known).valid);
        let mut damaged = signature.clone();
        let mut bytes = B64.decode(&damaged.signature).unwrap();
        bytes[0] ^= 1;
        damaged.signature = B64.encode(bytes);
        assert!(!check_signature(&damaged, &hash, &known).valid);
    }
    for file in store.sign(&hash, &key, false).unwrap() {
        let signature = serde_json::from_slice(&fs::read(file).unwrap()).unwrap();
        assert!(check_signature(&signature, &hash, &known).valid);
    }
    let full_key = KeyPair::generate("archive").unwrap();
    let archive = sign_hash(&hash, &full_key, true, "2026-10-22T10:00:00Z").unwrap();
    assert_eq!(archive.len(), 3);
    assert!(archive.iter().all(|s| check_signature(s, &hash, &[]).valid));
    let mut future = signatures[0].clone();
    future.alg = Some("future-algorithm".into());
    let info = check_signature(&future, &hash, &known);
    assert!(!info.valid);
    assert!(info.problem.unwrap().contains("unknown algorithm"));
}

fn example(file: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/hello-table")
        .join(file)
}

fn read_layer(file: &str) -> Layer {
    Layer::from_json(&fs::read(example(file)).unwrap()).unwrap()
}

fn composed(flat: &Flat) -> Value {
    let trees = Composer::new(flat).compose_all().unwrap();
    serde_json::to_value(trees).unwrap()
}

fn hello(t: &TempDir) -> Store {
    let s = Store::init(&t.0).unwrap();
    s.import_stack(&example("project.ox.json")).unwrap();
    s
}

fn layer(id: &str, nodes: Vec<(&str, &str, Value)>) -> Layer {
    Layer {
        header: Header {
            id: id.into(),
            ox_version: "ox_alpha".into(),
            data_version: "1.0.0".into(),
            author: "test".into(),
            timestamp: "2026-10-16".into(),
            kind: Some("edit".into()),
            ..Header::default()
        },
        imports: vec![],
        schemas: Default::default(),
        data: nodes
            .into_iter()
            .map(|(path, key, v)| Node {
                path: path.into(),
                attributes: [(key.to_string(), v)].into_iter().collect(),
                ..Node::default()
            })
            .collect(),
    }
}

#[test]
fn store_composes_like_the_stack_files() {
    let t = TempDir::new("same");
    let s = hello(&t);
    let head = s.head().unwrap().unwrap();
    assert_eq!(
        s.layers(&head).unwrap().len(),
        5,
        "schema, base, proposal, review, pack"
    );
    let from_files = ox_core::open(&example("project.ox.json")).unwrap();
    // The stack files name pictures by path; the store by the hash of their bytes (M1.5 item 1).
    // Swap each path for its digest, then the two must compose identically.
    let digests_of = |mut v: Value| {
        fn walk(v: &mut Value) {
            match v {
                Value::Object(m) => {
                    if let Some(Value::String(b)) = m.get("blob").cloned()
                        && !b.starts_with("sha256-")
                    {
                        let bytes = fs::read(example(&b)).unwrap();
                        m.insert("blob".into(), json!(ox_core::stack::integrity_of(&bytes)));
                        m.insert("mediaType".into(), json!(ox_core::media_type_for(&b)));
                    }
                    m.values_mut().for_each(walk);
                }
                Value::Array(a) => a.iter_mut().for_each(walk),
                _ => {}
            }
        }
        walk(&mut v);
        v
    };
    let expected = digests_of(composed(&from_files));
    assert_ne!(expected, composed(&from_files), "the example has a picture");
    assert_eq!(composed(&s.flat(&head).unwrap()), expected);
    // Without the cache, from the layers themselves.
    fs::remove_dir_all(s.dir.join("cache")).unwrap();
    assert_eq!(composed(&s.flat(&head).unwrap()), expected);
    // Five commits, each pointing at the one before.
    assert_eq!(s.log(None).unwrap().len(), 5);
}

#[test]
fn stale_commits_are_refused_then_rebased() {
    let t = TempDir::new("cas");
    let s = hello(&t);
    let h0 = s.head().unwrap().unwrap();
    let opts = || CommitOptions {
        expect: Some(h0.clone()),
        ..CommitOptions::default()
    };
    // Two people start from h0. The first commit wins.
    s.commit(&layer("a", vec![(TOTAL, "ox::note", json!("a"))]), opts())
        .unwrap();
    let b = layer("b", vec![(TABLE, "ox::note", json!("b"))]);
    match s.commit(&b, opts()) {
        Err(StoreError::Stale { since, .. }) => assert_eq!(since, vec!["a".to_string()]),
        other => panic!("expected stale, got {other:?}"),
    }
    // Rebased: nothing since touched TABLE's ox::note, so it goes on top.
    let c = s
        .commit(
            &b,
            CommitOptions {
                rebase: true,
                ..opts()
            },
        )
        .unwrap();
    assert_eq!(c.rebased_over.len(), 1);
    // A third change to the same key `a` set can't be rebased silently.
    let clash = layer("c", vec![(TOTAL, "ox::note", json!("c"))]);
    match s.commit(
        &clash,
        CommitOptions {
            rebase: true,
            ..opts()
        },
    ) {
        Err(StoreError::RebaseConflict { overlaps }) => {
            assert_eq!(
                overlaps,
                vec![("a".into(), TOTAL.into(), "ox::note".into())]
            )
        }
        other => panic!("expected a rebase conflict, got {other:?}"),
    }
    // The layer's own `header.base` works like --expect.
    let mut d = layer("d", vec![(TOTAL, "ox::flag", json!(true))]);
    d.header.base = Some(h0.clone());
    assert!(matches!(
        s.commit(&d, CommitOptions::default()),
        Err(StoreError::Stale { .. })
    ));
}

#[test]
fn concurrent_appends_one_wins_others_retry() {
    let t = TempDir::new("race");
    let s = hello(&t);
    let h0 = s.head().unwrap().unwrap();
    let n = 8;
    let results: Vec<(bool, bool)> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..n)
            .map(|i| {
                let s = s.clone();
                let h0 = h0.clone();
                scope.spawn(move || {
                    let path = format!("node-{i}");
                    let l = layer(&format!("w{i}"), vec![(&path, "ox::note", json!(i))]);
                    let first = s.commit(
                        &l,
                        CommitOptions {
                            expect: Some(h0),
                            ..CommitOptions::default()
                        },
                    );
                    let won = first.is_ok();
                    let retried = match first {
                        Ok(_) => true,
                        Err(StoreError::Stale { head, .. }) => s
                            .commit(
                                &l,
                                CommitOptions {
                                    expect: Some(head),
                                    rebase: true,
                                    ..CommitOptions::default()
                                },
                            )
                            .is_ok(),
                        Err(e) => panic!("{e}"),
                    };
                    (won, retried)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert_eq!(
        results.iter().filter(|(won, _)| *won).count(),
        1,
        "exactly one wins"
    );
    // A retry against the head it saw can still lose to another retry; keep retrying.
    let head = s.head().unwrap().unwrap();
    let layers = s.layers(&head).unwrap();
    let mut missing = Vec::new();
    for i in 0..n {
        let id = format!("w{i}");
        if !layers.iter().any(|h| s.index(h).unwrap().header.id == id) {
            missing.push(i);
        }
    }
    for i in missing {
        let path = format!("node-{i}");
        let l = layer(&format!("w{i}"), vec![(&path, "ox::note", json!(i))]);
        s.commit(&l, CommitOptions::default()).unwrap();
    }
    assert_eq!(s.head_layers().unwrap().len(), 5 + n);
    assert!(!s.dir.join("HEAD.lock").exists());
}

#[test]
fn blame_follows_the_q3_total_through_review_and_workbook_edit() {
    let t = TempDir::new("blame");
    let s = hello(&t);
    s.commit(&read_layer("grid-edit.ox.json"), CommitOptions::default())
        .unwrap();
    let ids = |hist: &[crate::Touch]| hist.iter().map(|h| h.id.clone()).collect::<Vec<_>>();

    let b = s.blame(TOTAL, Some("office::data::value")).unwrap();
    assert_eq!(b.len(), 1);
    assert_eq!(b[0].value["value"], json!(1378750));
    assert_eq!(
        ids(&b[0].history),
        vec![
            "milleleaf/examples/hello-table/base",
            "milleleaf/examples/hello-table/finance-review",
            "edit/finance-libreoffice"
        ]
    );

    // The deck's live field for the total shows the same history, through the link.
    let deck = read_layer("deck.proposal.ox.json");
    let field = deck
        .data
        .iter()
        .find(|n| n.inherits.values().any(|v| v.as_deref() == Some(TOTAL)))
        .map(|n| n.path.clone())
        .expect("a deck field linked to the total");
    let b = s.blame(&field, Some("office::data::value")).unwrap();
    assert_eq!(b[0].via, vec![TOTAL.to_string()]);
    assert_eq!(b[0].origin, TOTAL);
    assert_eq!(
        ids(&b[0].history).last().unwrap(),
        "edit/finance-libreoffice"
    );

    // A derived value (the region count) blames its source table.
    let b = s.blame(REGION_COUNT, Some("office::data::value")).unwrap();
    assert_eq!(b[0].origin, TABLE);
    assert_eq!(b[0].origin_key, "office::data::table");
    assert_eq!(b[0].value["value"], json!(4));
    assert_eq!(ids(&b[0].history).len(), 3);
}

#[test]
fn checkpoints_keep_composition_provenance_and_history() {
    let t = TempDir::new("ckpt");
    let s = hello(&t);
    s.commit(&read_layer("grid-edit.ox.json"), CommitOptions::default())
        .unwrap();
    let before_head = s.head().unwrap().unwrap();
    let before = composed(&s.flat(&before_head).unwrap());
    let history = s.history(TOTAL, None).unwrap();

    let c = s.checkpoint(1, "sam").unwrap().expect("checkpointed");
    let head = s.head().unwrap().unwrap();
    assert_eq!(c.stack, head);
    assert_eq!(
        s.layers(&head).unwrap().len(),
        2,
        "checkpoint + the kept layer"
    );
    fs::remove_dir_all(s.dir.join("cache")).unwrap();
    assert_eq!(
        composed(&s.flat(&head).unwrap()),
        before,
        "same values and provenance"
    );

    let after = s.history(TOTAL, None).unwrap();
    assert_eq!(
        after.iter().map(|h| &h.id).collect::<Vec<_>>(),
        history.iter().map(|h| &h.id).collect::<Vec<_>>()
    );
    assert!(after[0].in_checkpoint.is_some() && after[2].in_checkpoint.is_none());
    // History of commits continues through the checkpoint.
    assert_eq!(s.log(None).unwrap().len(), 7);
    // A layer made against the pre-checkpoint head rebases across it.
    let l = layer("late", vec![(TABLE, "ox::note", json!("late"))]);
    s.commit(
        &l,
        CommitOptions {
            expect: Some(before_head),
            rebase: true,
            ..CommitOptions::default()
        },
    )
    .unwrap();
}

#[test]
fn incremental_compose_recomputes_only_what_a_layer_touches() {
    let t = TempDir::new("incr");
    let s = hello(&t);
    let head = s.head().unwrap().unwrap();
    let flat0 = s.flat(&head).unwrap();
    let mut c = Composer::new(&flat0);
    c.compose_all().unwrap();
    let all = c.cached();

    let edit = read_layer("grid-edit.ox.json");
    let mut flat1 = flat0.clone();
    flat1.apply(&edit);
    let touched: BTreeSet<String> = edit.data.iter().map(|n| n.path.clone()).collect();
    let (mut c, dropped) = c.rebase(&flat1, &touched);
    assert!(dropped >= touched.len(), "{dropped}");
    assert!(dropped < all, "kept most of {all}");
    let incremental = serde_json::to_value(c.compose_all().unwrap()).unwrap();
    assert_eq!(incremental, composed(&flat1));
}

#[test]
fn signatures_are_detached_and_checked() {
    let t = TempDir::new("sig");
    let s = hello(&t);
    let sam = KeyPair::generate("sam").unwrap();
    let stranger = KeyPair::generate("finance").unwrap();
    s.add_key("sam", &sam.public()).unwrap();

    let before = s.head_layers().unwrap();
    let c = s
        .commit(
            &read_layer("grid-edit.ox.json"),
            CommitOptions {
                sign: Some(&sam),
                ..CommitOptions::default()
            },
        )
        .unwrap();
    assert!(c.signed);
    // Signing didn't change any layer's hash.
    assert_eq!(s.head_layers().unwrap()[..5], before[..]);

    match s.verify(&c.layer).unwrap() {
        LayerStatus::Signed { signatures } => {
            assert_eq!(signatures[0].known_as.as_deref(), Some("sam"));
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        s.verify(&before[0]).unwrap(),
        LayerStatus::Unsigned
    ));

    // An unknown key's signature verifies but isn't trusted by name.
    s.sign(&before[1], &stranger, false).unwrap();
    match s.verify(&before[1]).unwrap() {
        LayerStatus::Signed { signatures } => {
            assert_eq!(signatures[0].known_as, None);
            assert_eq!(signatures[0].claimed, "finance");
        }
        other => panic!("{other:?}"),
    }

    // A signature copied onto another layer fails.
    let from = s
        .dir
        .join("sigs")
        .join(&c.layer)
        .join(format!("{}.json", sam.id()));
    let to_dir = s.dir.join("sigs").join(&before[2]);
    fs::create_dir_all(&to_dir).unwrap();
    fs::copy(&from, to_dir.join("copied.json")).unwrap();
    assert!(matches!(
        s.verify(&before[2]).unwrap(),
        LayerStatus::Invalid { .. }
    ));

    // A layer edited after the fact no longer matches its name.
    let p = s.dir.join("layers").join(format!("{}.json", c.layer));
    let text = fs::read_to_string(&p)
        .unwrap()
        .replace("1378750", "9999999");
    fs::write(&p, text).unwrap();
    assert!(matches!(
        s.verify(&c.layer).unwrap(),
        LayerStatus::Tampered { .. }
    ));
    assert!(matches!(
        s.layer(&c.layer),
        Err(StoreError::Tampered { .. })
    ));

    // Keys round-trip through their file.
    let kd = t.0.join("keys");
    let path = sam.save(&kd).unwrap();
    assert_eq!(KeyPair::load(&path).unwrap().public(), sam.public());
    assert!(sam.save(&kd).is_err(), "never overwrites a key");
}

#[test]
fn a_thousand_layers_compose_quickly() {
    let t = TempDir::new("perf");
    let s = hello(&t);
    // 1,000 small edits to a 50-node model, as a busy project would see.
    for i in 0..1000 {
        let p = format!("cell-{}", i % 50);
        s.commit(
            &layer(&format!("edit/{i}"), vec![(&p, "ox::note", json!(i))]),
            CommitOptions::default(),
        )
        .unwrap();
    }
    let head = s.head().unwrap().unwrap();
    assert_eq!(s.layers(&head).unwrap().len(), 1005);

    let start = Instant::now();
    let flat = s.flat(&head).unwrap();
    let trees = Composer::new(&flat).compose_all().unwrap();
    let cached = start.elapsed();
    assert!(!trees.is_empty());

    fs::remove_dir_all(s.dir.join("cache")).unwrap();
    let start = Instant::now();
    let flat2 = s.flat(&head).unwrap();
    Composer::new(&flat2).compose_all().unwrap();
    let scratch = start.elapsed();
    eprintln!("1,005 layers: cached {cached:?}, from scratch {scratch:?}");
    // Release builds are far under; debug builds get headroom.
    let limit = if cfg!(debug_assertions) { 10.0 } else { 1.0 };
    assert!(cached.as_secs_f64() < 1.0, "{cached:?}");
    assert!(scratch.as_secs_f64() < limit, "{scratch:?}");
    assert_eq!(flat.nodes.len(), flat2.nodes.len());
}

#[cfg(unix)]
#[test]
fn a_lock_left_by_a_dead_process_is_cleared_a_live_one_is_not() {
    let t = TempDir::new("stale-lock");
    let s = hello(&t);
    let lock = s.dir.join("HEAD.lock");
    let edit = |id: &str| layer(id, vec![("3f1c2a90-0000-4c1e-9a00-000000000023", "test::note", json!(id))]);

    // A crashed commit: a process that has exited, on this machine.
    let mut child = std::process::Command::new("true").spawn().unwrap();
    let dead = child.id();
    child.wait().unwrap();
    fs::write(&lock, format!("{dead} {}\n", crate::host())).unwrap();
    s.commit(&edit("after-crash"), CommitOptions::default())
        .expect("a dead process's lock doesn't block");
    assert!(!lock.exists());

    // A live holder (this process) and a lock from another machine both still block.
    for held in [format!("{} {}", std::process::id(), crate::host()), format!("{dead} some-other-machine")] {
        fs::write(&lock, &held).unwrap();
        let err = s.commit(&edit("blocked"), CommitOptions::default()).unwrap_err();
        assert!(matches!(err, StoreError::Busy(..)), "{err}");
        assert!(err.to_string().contains(held.trim()), "{err}");
    }
    let _ = fs::remove_file(&lock);
}

#[test]
fn layers_are_stored_canonical_so_a_resaved_copy_keeps_its_name() {
    // RFC 8785 §3.2.2's example: sorted keys, ECMAScript numbers, minimal string escaping.
    let input = r#"{"numbers":[333333333.33333329,1E30,4.50,2e-3,0.000000000000000000000000001],"string":"\u20ac$\u000F\u000aA'\u0042\u0022\u005c\\\"\/","literals":[null,true,false]}"#;
    let v: Value = serde_json::from_str(input).unwrap();
    let canon = String::from_utf8(serde_json_canonicalizer::to_vec(&v).unwrap()).unwrap();
    assert_eq!(canon, r#"{"literals":[null,true,false],"numbers":[333333333.3333333,1e+30,4.5,0.002,1e-27],"string":"€$\u000f\nA'B\"\\\\\"/"}"#);

    let t = TempDir::new("canonical");
    let s = hello(&t);
    let head = s.head_layers().unwrap();
    let name = head.last().unwrap().clone();
    let stored = fs::read(s.dir.join("layers").join(format!("{name}.json"))).unwrap();
    assert!(!stored.contains(&b'\n'), "stored canonical, one line");
    // Someone re-saves it pretty-printed (an editor, a database, a browser store): same name.
    let pretty = serde_json::to_vec_pretty(&serde_json::from_slice::<Value>(&stored).unwrap()).unwrap();
    assert_ne!(pretty, stored);
    let again = crate::layer_bytes(&Layer::from_json(&pretty).unwrap());
    assert_eq!(crate::hash_of(&again), name);
}

#[test]
fn an_integer_beyond_2_53_is_refused() {
    let t = TempDir::new("ijson");
    let s = hello(&t);
    let l = layer("big", vec![("3f1c2a90-0000-4c1e-9a00-000000000023", "test::n", json!(9007199254740993u64))]);
    let err = s.commit(&l, CommitOptions::default()).unwrap_err();
    assert!(matches!(err, StoreError::NotIJson(_)), "{err}");
    let ok = layer("ok", vec![("3f1c2a90-0000-4c1e-9a00-000000000023", "test::n", json!(9007199254740991u64))]);
    s.commit(&ok, CommitOptions::default()).expect("2^53 - 1 is fine");
}

#[test]
fn pictures_are_stored_by_hash_and_a_swapped_one_fails_verify() {
    let t = TempDir::new("blobs");
    let s = hello(&t);
    assert!(s.verify_blobs().unwrap().is_empty());
    // The deck's picture is in .ox/blobs under the hash of its bytes, and the layer names that hash.
    let blobs: Vec<_> = fs::read_dir(s.dir.join("blobs")).unwrap().map(|e| e.unwrap().path()).collect();
    assert_eq!(blobs.len(), 1, "{blobs:?}");
    // Swap the picture's bytes: the signed layer still verifies, but the blob it names doesn't.
    fs::write(&blobs[0], b"not the picture").unwrap();
    let problems = s.verify_blobs().unwrap();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("is damaged"), "{problems:?}");
    // And a layer naming an asset that isn't there is refused.
    let mut l = layer("missing-pic", vec![]);
    l.data.push(Node {
        path: "pic-x".into(),
        attributes: [("deck::picture".to_string(), json!({ "blob": "assets/nowhere.png" }))].into_iter().collect(),
        ..Node::default()
    });
    let err = s.commit(&l, CommitOptions::default()).unwrap_err();
    assert!(matches!(err, StoreError::MissingAsset(_)), "{err}");
}

#[test]
fn a_purged_layer_loses_its_bytes_keeps_its_hash_and_composes_empty() {
    let t = TempDir::new("purge");
    let s = hello(&t);
    // A layer with something that must go (say, personal data typed by mistake).
    let secret = layer("oops", vec![("3f1c2a90-0000-4c1e-9a00-000000000023", "test::note", json!("private phone number"))]);
    let c = s.commit(&secret, CommitOptions::default()).unwrap();
    let composed_with = serde_json::to_string(&composed(&s.flat(&s.head().unwrap().unwrap()).unwrap())).unwrap();
    assert!(composed_with.contains("private phone number"));

    // Only an owner or approver may purge: an editor's signature is refused.
    let owner = KeyPair::generate("owner").unwrap();
    let editor = KeyPair::generate("editor").unwrap();
    s.add_key("owner", &owner.public()).unwrap();
    s.add_key("editor", &editor.public()).unwrap();
    s.set_role("owner", "owner").unwrap();
    let refused = s.purge(&c.layer, &editor, Some("personal data")).unwrap_err();
    assert!(matches!(refused, StoreError::NotAllowed(_)), "{refused}");
    assert!(s.dir.join("layers").join(format!("{}.json", c.layer)).exists(), "an editor can't purge");
    let rec = s.purge(&c.layer, &owner, Some("personal data")).unwrap();
    // Bytes gone, hash and index kept, the head's composition no longer has it.
    assert!(!s.dir.join("layers").join(format!("{}.json", c.layer)).exists());
    assert!(s.index(&c.layer).is_ok());
    let composed_after = serde_json::to_string(&composed(&s.flat(&s.head().unwrap().unwrap()).unwrap())).unwrap();
    assert!(!composed_after.contains("private phone number"));
    // No copy left in caches either.
    let leftover = walk_files(&s.dir).into_iter().any(|p| fs::read(&p).is_ok_and(|b| String::from_utf8_lossy(&b).contains("private phone number")));
    assert!(!leftover, "content still on disk somewhere in .ox");
    match s.verify(&c.layer).unwrap() {
        LayerStatus::Purged { by, reason, .. } => assert_eq!((by.as_str(), reason.as_deref()), ("owner", Some("personal data"))),
        other => panic!("{other:?}"),
    }

    // Another copy of the project receives the purge record and does the same.
    let t2 = TempDir::new("purge-copy");
    let s2 = hello(&t2);
    s2.commit(&secret, CommitOptions::default()).unwrap();
    let record = s.layer(&rec.layer).unwrap();
    assert_eq!(record.header.purges.len(), 1);
    // Without the owner's signature (or with an owner this copy doesn't know) it's refused …
    assert!(matches!(s2.commit(&record, CommitOptions::default()), Err(StoreError::NotAllowed(_))));
    // … with the signature file and the owner known here, it's carried out.
    s2.add_key("owner", &owner.public()).unwrap();
    s2.set_role("owner", "owner").unwrap();
    let sigs = s.dir.join("sigs").join(&rec.layer);
    let dst = s2.dir.join("sigs").join(&rec.layer);
    fs::create_dir_all(&dst).unwrap();
    for f in fs::read_dir(&sigs).unwrap().flatten() {
        fs::copy(f.path(), dst.join(f.file_name())).unwrap();
    }
    s2.commit(&record, CommitOptions::default()).unwrap();
    assert!(!s2.dir.join("layers").join(format!("{}.json", c.layer)).exists());
    assert!(matches!(s2.verify(&c.layer).unwrap(), LayerStatus::Purged { .. }));
}

fn walk_files(d: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in fs::read_dir(d).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() { out.extend(walk_files(&p)); } else { out.push(p); }
    }
    out
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for f in fs::read_dir(from).unwrap().flatten() {
        fs::copy(f.path(), to.join(f.file_name())).unwrap();
    }
}

fn has_text(d: &Path, text: &str) -> bool {
    walk_files(d).into_iter().any(|p| fs::read(&p).is_ok_and(|b| String::from_utf8_lossy(&b).contains(text)))
}

#[test]
fn sealed_layers_open_only_for_recipients_and_a_key_purge_shreds_every_copy() {
    const SECRET: &str = "salary 123456 (sealed)";
    let t = TempDir::new("seal");
    let (ka, kb) = (TempDir::new("seal-keys-alice"), TempDir::new("seal-keys-bob"));
    let s = hello(&t);
    let alice = KeyPair::generate("alice").unwrap();
    let bob = KeyPair::generate("bob").unwrap();
    let owner = KeyPair::generate("owner").unwrap();
    alice.save(&ka.0).unwrap();
    bob.save(&kb.0).unwrap();
    for k in [&alice, &bob, &owner] {
        s.add_key(&k.name, &k.public()).unwrap();
        s.add_xwing_key(&k.name, &k.xwing_public().unwrap()).unwrap();
    }
    s.set_role("owner", "owner").unwrap();
    let as_ = |dir: &Path, keys: &Path| {
        let mut x = Store::open(dir).unwrap();
        x.keys = keys.to_path_buf();
        x
    };
    let flat_text = |st: &Store| serde_json::to_string(&composed(&st.flat(&st.head().unwrap().unwrap()).unwrap())).unwrap();

    let secret = layer("pay", vec![("3f1c2a90-0000-4c1e-9a00-000000000023", "test::note", json!(SECRET))]);
    let to = vec!["alice".to_string()];
    // Someone without an X-Wing key in the project can't be a recipient.
    assert!(s.commit(&secret, CommitOptions { seal_to: &["nobody".to_string()], ..CommitOptions::default() }).is_err());
    let c = s.commit(&secret, CommitOptions { seal_to: &to, ..CommitOptions::default() }).unwrap();
    let stored = s.layer(&c.layer).unwrap();
    let sealed = stored.header.sealed.clone().expect("sealed header");
    assert_eq!(sealed.scheme, "hpke-xwing-sha256-aes256gcm/v1");
    assert!(stored.data.is_empty());
    assert!(s.dir.join("wraps").join(&sealed.key).join("alice.json").exists());

    // The recipient composes it; bob (in the project, not a recipient) and a stranger see it empty.
    let sa = as_(&t.0, &ka.0);
    assert!(flat_text(&sa).contains(SECRET));
    assert_eq!(sa.open_layer(&c.layer).unwrap().1, crate::Opened::Decrypted);
    let sb = as_(&t.0, &kb.0);
    assert!(!flat_text(&sb).contains(SECRET));
    assert!(matches!(sb.open_layer(&c.layer).unwrap().1, crate::Opened::Locked(_)));
    // Nothing in .ox holds the plaintext: not the layer, the ciphertext blob, the index or a cache.
    assert!(!has_text(&s.dir, SECRET), "plaintext on disk after opening");
    assert!(!has_text(&s.dir, "salary"), "plaintext on disk after opening");
    // A checkpoint folds only the layers below the sealed one, so the recipient still sees it.
    assert!(sa.checkpoint(0, "test").unwrap().is_some());
    assert!(flat_text(&sa).contains(SECRET));
    assert!(!has_text(&s.dir, SECRET));
    let info = s.sealed_info(&c.layer).unwrap().unwrap();
    assert_eq!((info.recipients, info.problem), (vec!["alice".to_string()], None));

    // A second copy of the project receives the layer, its ciphertext and wraps.
    let t2 = TempDir::new("seal-copy");
    let s2 = hello(&t2);
    let c2 = s2.commit(&stored, CommitOptions::default()).unwrap();
    assert_eq!(c2.layer, c.layer, "a byte-for-byte copy keeps its hash");
    let hex = sealed.content.strip_prefix("sha256-").unwrap();
    fs::create_dir_all(s2.dir.join("blobs")).unwrap();
    fs::copy(s.dir.join("blobs").join(hex), s2.dir.join("blobs").join(hex)).unwrap();
    copy_dir(&s.dir.join("wraps").join(&sealed.key), &s2.dir.join("wraps").join(&sealed.key));
    for k in [&alice, &owner] {
        s2.add_key(&k.name, &k.public()).unwrap();
    }
    s2.set_role("owner", "owner").unwrap();
    let s2a = as_(&t2.0, &ka.0);
    assert!(flat_text(&s2a).contains(SECRET));

    // Only an owner or approver may destroy the key; a key purge needs a sealed layer.
    assert!(matches!(s.purge_mode(&c.layer, &alice, None, "key"), Err(StoreError::NotAllowed(_))));
    let plain = s.commit(&layer("plain", vec![("3f1c2a90-0000-4c1e-9a00-000000000023", "test::x", json!(1))]), CommitOptions::default()).unwrap();
    assert!(s.purge_mode(&plain.layer, &owner, None, "key").is_err());
    let backup = TempDir::new("seal-wrap-backup");
    copy_dir(&s.dir.join("wraps").join(&sealed.key), &backup.0);
    let rec = s.purge_mode(&c.layer, &owner, Some("left the company"), "key").unwrap();
    assert!(!s.dir.join("wraps").join(&sealed.key).exists(), "wraps destroyed");
    assert!(s.dir.join("layers").join(format!("{}.json", c.layer)).exists(), "the layer stays");
    assert!(s.dir.join("blobs").join(hex).exists(), "the (useless) ciphertext stays");
    assert!(!flat_text(&sa).contains(SECRET), "the recipient can no longer open it");
    match s.verify(&c.layer).unwrap() {
        LayerStatus::Purged { mode, by, reason, .. } => {
            assert_eq!((mode.as_str(), by.as_str(), reason.as_deref()), ("key", "owner", Some("left the company")))
        }
        other => panic!("{other:?}"),
    }

    // The copy receives the signed record and destroys its wraps too.
    let record = s.layer(&rec.layer).unwrap();
    copy_dir(&s.dir.join("sigs").join(&rec.layer), &s2.dir.join("sigs").join(&rec.layer));
    s2.commit(&record, CommitOptions::default()).unwrap();
    assert!(!s2.dir.join("wraps").join(&sealed.key).exists());
    assert!(!flat_text(&s2a).contains(SECRET));
    assert!(matches!(s2.verify(&c.layer).unwrap(), LayerStatus::Purged { mode, .. } if mode == "key"));
    // A wrap that turns up later (an old backup) doesn't reopen it.
    copy_dir(&backup.0, &s2.dir.join("wraps").join(&sealed.key));
    assert!(matches!(s2a.open_layer(&c.layer).unwrap().1, crate::Opened::Locked(_)));
}

#[test]
fn hybrid_signatures_old_keys_upgrade_and_policy() {
    use crate::pq::{MLDSA65, SLH};
    let t = TempDir::new("pq");
    let s = hello(&t);
    let owner = KeyPair::generate("owner").unwrap();
    let editor = KeyPair::generate("editor").unwrap();
    for k in [&owner, &editor] {
        s.add_key(&k.name, &k.public()).unwrap();
        s.add_pq_key(&k.name, &k.mldsa_public().unwrap()).unwrap();
        s.add_pq_key(&k.name, &k.slh_public().unwrap()).unwrap();
    }
    s.set_role("owner", "owner").unwrap();
    let layers = s.head_layers().unwrap();
    let signed = |h: &str| match s.verify(h).unwrap() {
        LayerStatus::Signed { signatures } => signatures,
        other => panic!("{other:?}"),
    };
    let strength_of = |sigs: &[SigInfo], who: &str| {
        strength(&sigs.iter().filter(|g| g.valid && g.known_as.as_deref() == Some(who)).map(|g| g.alg.as_str()).collect::<Vec<_>>())
    };

    // An editor signs Ed25519 + ML-DSA; an owner adds SLH-DSA; anyone does with `archive`.
    assert_eq!(s.sign(&layers[0], &editor, false).unwrap().len(), 2);
    assert_eq!(strength_of(&signed(&layers[0]), "editor"), "hybrid");
    assert_eq!(s.sign(&layers[1], &owner, false).unwrap().len(), 3);
    assert_eq!(strength_of(&signed(&layers[1]), "owner"), "hybrid+slh-dsa");
    s.sign(&layers[2], &editor, true).unwrap();
    assert_eq!(strength_of(&signed(&layers[2]), "editor"), "hybrid+slh-dsa");
    // The Ed25519 file keeps its old form (no `alg`), so an older ox reads it as before.
    let ed = fs::read_to_string(s.dir.join("sigs").join(&layers[0]).join(format!("{}.json", editor.id()))).unwrap();
    assert!(!ed.contains("\"alg\""), "{ed}");
    let policy = s.policy().unwrap();
    let now = "2026-10-18T00:00:00Z";
    for h in &layers[..3] {
        let v = policy.check(&signed(h), now, &s.pq_listed());
        assert!(v.refused.is_empty() && v.warnings.is_empty(), "{v:?}");
    }

    // A key made before 18 Oct 2026 (Ed25519 and X-Wing only) still signs: `classical only`,
    // accepted with a warning until 2035, refused from then.
    let keys = t.0.join("keys");
    let path = KeyPair::generate("old").unwrap().save(&keys).unwrap();
    let mut kf: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    kf.as_object_mut().unwrap().retain(|k, _| ["name", "secret", "public", "xwing"].contains(&k.as_str()));
    fs::write(&path, serde_json::to_vec_pretty(&kf).unwrap()).unwrap();
    let old = KeyPair::load(&path).unwrap();
    assert!(old.mldsa_public().is_none() && old.slh_public().is_none());
    s.add_key("old", &old.public()).unwrap();
    assert_eq!(s.sign(&layers[3], &old, false).unwrap().len(), 1);
    let sigs = signed(&layers[3]);
    assert_eq!(strength_of(&sigs, "old"), "classical only");
    let v = policy.check(&sigs, now, &s.pq_listed());
    assert!(v.refused.is_empty() && v.warnings.len() == 1, "{v:?}");
    let v = policy.check(&sigs, "2035-01-01T00:00:00Z", &s.pq_listed());
    assert!(v.refused.iter().any(|r| r.starts_with("old:")), "{v:?}");

    // `upgrade` adds the post-quantum keys, keeps the others and mode 600; a second run changes nothing.
    let (up, added) = KeyPair::upgrade(&path).unwrap();
    assert_eq!(added, vec![MLDSA65, SLH]);
    assert_eq!((up.public(), up.xwing_public()), (old.public(), old.xwing_public()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }
    let (again, added) = KeyPair::upgrade(&path).unwrap();
    assert!(added.is_empty());
    assert_eq!((again.mldsa_public(), again.slh_public()), (up.mldsa_public(), up.slh_public()));
    s.add_pq_key("old", &up.mldsa_public().unwrap()).unwrap();
    s.sign(&layers[3], &up, false).unwrap();
    let sigs = signed(&layers[3]);
    assert_eq!(strength_of(&sigs, "old"), "hybrid");
    assert!(policy.check(&sigs, now, &s.pq_listed()).warnings.is_empty());
    // Once the project lists their ML-DSA key, Ed25519 alone is refused even before 2035: deleting
    // the ML-DSA file must not downgrade the layer.
    fs::remove_file(s.dir.join("sigs").join(&layers[3]).join(format!("{}.mldsa65.json", up.id()))).unwrap();
    let v = policy.check(&signed(&layers[3]), now, &s.pq_listed());
    assert!(v.warnings.is_empty() && v.refused.iter().any(|r| r.starts_with("old: no valid mldsa65")), "{v:?}");

    // Refused: a signer the project doesn't list, an unsigned layer, algorithms the policy
    // doesn't allow or no longer trusts.
    s.sign(&layers[0], &KeyPair::generate("stranger").unwrap(), false).unwrap();
    assert!(policy.check(&signed(&layers[0]), now, &s.pq_listed()).refused.iter().any(|r| r.contains("stranger")));
    assert_eq!(policy.check(&[], now, &s.pq_listed()).refused, vec!["unsigned"]);
    fs::write(
        s.dir.join("policy.json"),
        r#"{"signatures":{"require":["mldsa65"]},"algorithms":{"mldsa65":{"until":"2026-01-01"}}}"#,
    )
    .unwrap();
    let v = s.policy().unwrap().check(&signed(&layers[1]), now, &s.pq_listed());
    assert!(v.refused.iter().any(|r| r.contains("ed25519 isn't")), "{v:?}");
    assert!(v.refused.iter().any(|r| r.contains("mldsa65 is no longer trusted")), "{v:?}");

    // A changed ML-DSA signature fails; an algorithm this build doesn't know is reported, not accepted.
    let ml = s.dir.join("sigs").join(&layers[1]).join(format!("{}.mldsa65.json", owner.id()));
    let mut sig: Value = serde_json::from_slice(&fs::read(&ml).unwrap()).unwrap();
    let b = sig["signature"].as_str().unwrap().to_string();
    sig["signature"] = json!(format!("{}{}", if b.starts_with('A') { "B" } else { "A" }, &b[1..]));
    fs::write(&ml, serde_json::to_vec(&sig).unwrap()).unwrap();
    sig["alg"] = json!("falcon512");
    fs::write(ml.with_file_name("x.falcon512.json"), serde_json::to_vec(&sig).unwrap()).unwrap();
    match s.verify(&layers[1]).unwrap() {
        LayerStatus::Invalid { signatures } => {
            assert!(signatures.iter().any(|g| g.alg == MLDSA65 && !g.valid && g.problem.is_none()));
            assert!(signatures.iter().any(|g| g.alg == "falcon512" && !g.valid && g.problem.as_deref().is_some_and(|p| p.contains("unknown algorithm"))));
            assert!(signatures.iter().filter(|g| g.valid).count() >= 2, "the owner's Ed25519 and SLH-DSA still verify");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_time_stamp_token_is_checked_against_its_signature_file() {
    use sha2::Digest;
    // An `openssl ts -reply` response (tools/local-tsa.mjs, 18 Oct 2026) for signed.json.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/tsa");
    let data = fs::read(dir.join("signed.json")).unwrap();
    let resp = fs::read(dir.join("response.tsr")).unwrap();
    assert_eq!(crate::tsp::check(&resp, &data).unwrap().at, "2026-10-18T06:00:04Z");
    assert!(crate::tsp::check(&resp, b"other bytes").unwrap_err().contains("imprint"));
    assert!(crate::tsp::check(&resp[..200], &data).is_err());
    let (req, _) = crate::tsp::request(&data).unwrap();
    assert!(req.windows(32).any(|w| w == sha2::Sha256::digest(&data).as_slice()));

    // In a store: a token for other bytes makes the signature invalid; signing again drops it.
    let t = TempDir::new("tsr");
    let s = hello(&t);
    let k = KeyPair::generate("sam").unwrap();
    let h = s.head_layers().unwrap()[0].clone();
    let files = s.sign(&h, &k, false).unwrap();
    fs::write(tsr_of(&files[0], 1), &resp).unwrap();
    match s.verify(&h).unwrap() {
        LayerStatus::Invalid { signatures } => {
            assert!(signatures.iter().any(|g| g.problem.as_deref().is_some_and(|p| p.starts_with("time-stamp"))))
        }
        other => panic!("{other:?}"),
    }
    s.sign(&h, &k, false).unwrap();
    assert!(!tsr_of(&files[0], 1).exists());
    assert!(matches!(s.verify(&h).unwrap(), LayerStatus::Signed { .. }));
}

#[test]
fn search_finds_what_the_project_says_now_its_history_and_sealed_text_only_for_recipients() {
    let t = TempDir::new("search");
    let (ka, none) = (TempDir::new("search-keys-alice"), TempDir::new("search-keys-none"));
    let s = hello(&t);
    let alice = KeyPair::generate("alice").unwrap();
    let owner = KeyPair::generate("owner").unwrap();
    alice.save(&ka.0).unwrap();
    for k in [&alice, &owner] {
        s.add_key(&k.name, &k.public()).unwrap();
        s.add_xwing_key(&k.name, &k.xwing_public().unwrap()).unwrap();
    }
    s.set_role("owner", "owner").unwrap();
    let as_ = |keys: &Path| {
        let mut x = Store::open(&t.0).unwrap();
        x.keys = keys.to_path_buf();
        x
    };
    let notes = |f: crate::Found| f.hits.into_iter().filter(|h| h.attribute.starts_with("test::")).collect::<Vec<_>>();
    const NODE: &str = "3f1c2a90-0000-4c1e-9a00-000000000023";

    assert!(!s.search_head("Adelaide").unwrap().hits.is_empty(), "the example's own table");
    s.commit(&layer("v1", vec![(NODE, "test::note", json!("draft budget for the 香港 office"))]), CommitOptions::default()).unwrap();
    s.commit(&layer("v2", vec![(NODE, "test::note", json!("final budget"))]), CommitOptions::default()).unwrap();
    // The head says "final budget"; the draft is only in the history.
    let now = notes(s.search_head("BUDGET").unwrap());
    assert_eq!((now.len(), now[0].layer.as_str(), now[0].path.as_str()), (1, "v2", NODE));
    assert!(notes(s.search_head("draft").unwrap()).is_empty());
    let past = notes(s.search_history("draft budget").unwrap());
    assert_eq!(past.iter().map(|h| h.layer.as_str()).collect::<Vec<_>>(), ["v1"]);
    assert!(past[0].hash.is_some() && past[0].text.contains("香港"));
    assert_eq!(notes(s.search_history("香港").unwrap()).len(), 1);
    assert!(notes(s.search_history("港香").unwrap()).is_empty(), "CJK as written");

    // A sealed layer: found by its recipient, now and in the history; its index on disk is encrypted.
    let c = s
        .commit(&layer("pay", vec![(NODE, "test::secret", json!("salary 123456"))]), CommitOptions { seal_to: &["alice".to_string()], ..CommitOptions::default() })
        .unwrap();
    let sa = as_(&ka.0);
    assert_eq!(notes(sa.search_head("salary").unwrap()).len(), 1);
    assert_eq!(notes(sa.search_history("salary").unwrap()).len(), 1);
    let cache = s.dir.join("cache").join("search").join(format!("{}.json", c.layer));
    assert!(cache.exists());
    assert!(!has_text(&s.dir, "salary"), "a sealed layer's text on disk");
    assert_eq!(notes(sa.search_history("salary").unwrap()).len(), 1, "from the encrypted cache");
    // Anyone else: no hits, and the history says which layer it couldn't look in.
    let so = as_(&none.0);
    assert!(notes(so.search_head("salary").unwrap()).is_empty());
    let f = so.search_history("salary").unwrap();
    assert!(f.hits.is_empty() && f.locked == vec![c.layer.clone()], "{f:?}");
    // A key purge shreds the index with the layer.
    s.purge_mode(&c.layer, &owner, None, "key").unwrap();
    assert!(!cache.exists());
    assert!(notes(sa.search_history("salary").unwrap()).is_empty());
}

#[test]
fn time_stamp_services_and_opentimestamps_proofs_beside_signature_files() {
    use crate::ots::{Att, Item, Node, Op, Proof};
    use crate::policy::{HeaderSource, Service};
    // The policy lists the services the user chose; the older `tsa` still counts as one.
    let t = TempDir::new("ots");
    let s = hello(&t);
    fs::write(
        s.dir.join("policy.json"),
        r#"{"signatures": {"require": ["ed25519"]}, "algorithms": {"ed25519": {}},
            "timestamps": {"tsa": "http://old.example/tsa",
              "services": [{"kind": "rfc3161", "url": "http://tsa.example"},
                           {"kind": "opentimestamps", "calendars": ["https://a.example", "https://b.example"]}],
              "bitcoinHeaders": [{"kind": "explorers", "urls": ["https://x.example/api"]},
                                 {"kind": "bitcoind", "url": "http://127.0.0.1:8332", "cookieFile": "/n/.cookie"},
                                 {"kind": "light-client", "file": "/h/headers.bin"}, {"kind": "light-client"}]}}"#,
    )
    .unwrap();
    let ts = s.policy().unwrap().timestamps.unwrap();
    assert_eq!(
        ts.all(),
        [
            Service::Rfc3161 { url: "http://tsa.example".into() },
            Service::Opentimestamps { calendars: vec!["https://a.example".into(), "https://b.example".into()] },
            Service::Rfc3161 { url: "http://old.example/tsa".into() },
        ]
    );
    let h = ts.headers();
    assert!(matches!(&h[1], HeaderSource::Bitcoind { cookie_file, .. } if cookie_file == "/n/.cookie"));
    assert_eq!(h[2], HeaderSource::LightClient { file: Some("/h/headers.bin".into()) });
    assert_eq!(h[3], HeaderSource::LightClient { file: None });
    assert_eq!(crate::policy::Timestamps::default().headers().len(), 1, "the two explorers by default");
    assert!(crate::policy::Timestamps::default().all().is_empty(), "no default service");

    // A pending proof beside a signature file is reported; one for other bytes makes it invalid.
    let k = KeyPair::generate("sam").unwrap();
    let layer = s.head_layers().unwrap()[0].clone();
    let files = s.sign(&layer, &k, false).unwrap();
    let data = fs::read(&files[0]).unwrap();
    let pending = Node(vec![Item::Op(Op::Append(vec![9; 8]), Node(vec![Item::Att(Att::Pending("https://a.example".into()))]))]);
    let proof = Proof::stamped(&data, &[1; 16], vec![pending]);
    fs::write(crate::ots_of(&files[0]), proof.to_bytes()).unwrap();
    let sigs = |s: &Store| match s.verify(&layer).unwrap() {
        LayerStatus::Signed { signatures } | LayerStatus::Invalid { signatures } => signatures,
        other => panic!("{other:?}"),
    };
    let got = sigs(&s).into_iter().find_map(|g| g.ots).unwrap();
    assert_eq!(got.pending, ["https://a.example"]);
    fs::write(crate::ots_of(&files[0]), Proof::stamped(b"other", &[1; 16], vec![Node(vec![Item::Att(Att::Bitcoin(1))])]).to_bytes()).unwrap();
    assert!(sigs(&s).iter().any(|g| !g.valid && g.problem.as_deref().is_some_and(|p| p.contains("OpenTimestamps"))));
    fs::write(crate::ots_of(&files[0]), b"garbage").unwrap();
    assert!(matches!(s.verify(&layer).unwrap(), LayerStatus::Invalid { .. }));
    // Several TSAs: `.tsr`, `.2.tsr`, … are all read; signing again removes every stale stamp.
    fs::write(tsr_of(&files[0], 2), b"x").unwrap();
    assert_eq!(crate::tsr_paths(&files[0]), Vec::<std::path::PathBuf>::new(), "numbered without gaps: .tsr first");
    fs::write(tsr_of(&files[0], 1), b"x").unwrap();
    assert_eq!(crate::tsr_paths(&files[0]).len(), 2);
    s.sign(&layer, &k, false).unwrap();
    assert!(!crate::ots_of(&files[0]).exists() && crate::tsr_paths(&files[0]).is_empty() && !tsr_of(&files[0], 2).exists());
}
