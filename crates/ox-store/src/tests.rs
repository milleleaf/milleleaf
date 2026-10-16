use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use ox_core::compose::{Composer, Flat};
use ox_core::model::{Header, Layer, Node};
use serde_json::{Value, json};

use crate::{CommitOptions, KeyPair, LayerStatus, Store, StoreError};

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
    assert_eq!(composed(&s.flat(&head).unwrap()), composed(&from_files));
    // Without the cache, from the layers themselves.
    fs::remove_dir_all(s.dir.join("cache")).unwrap();
    assert_eq!(composed(&s.flat(&head).unwrap()), composed(&from_files));
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
    s.sign(&before[1], &stranger).unwrap();
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
