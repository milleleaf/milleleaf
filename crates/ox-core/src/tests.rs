use std::fs;
use std::path::PathBuf;

use serde_json::json;

use crate::compose::{Composer, flatten};
use crate::render::group_thousands;
use crate::stack::integrity_of;
use crate::{Level, OxError, load_stack, validate};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let p = std::env::temp_dir().join(format!("ox-core-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn write(&self, file: &str, v: serde_json::Value) -> PathBuf {
        let p = self.0.join(file);
        fs::write(&p, serde_json::to_vec_pretty(&v).unwrap()).unwrap();
        p
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn header(id: &str) -> serde_json::Value {
    json!({"id": id, "oxVersion": "ox_alpha", "dataVersion": "1.0.0", "author": "test", "timestamp": "2026-10-15"})
}

#[test]
fn later_layers_win_and_null_deletes() {
    let t = TempDir::new("win");
    t.write("a.json", json!({"header": header("a"), "data": [
        {"path": "root", "children": {"x": "x", "y": "y"}, "attributes": {"t::v": 1, "t::gone": true}},
        {"path": "x"}, {"path": "y"}
    ]}));
    let top = t.write(
        "b.json",
        json!({"header": header("b"), "imports": [{"uri": "a.json"}], "data": [
            {"path": "root", "children": {"y": null}, "attributes": {"t::v": 2, "t::gone": null}}
        ]}),
    );
    let flat = flatten(&load_stack(&top).unwrap());
    let root = &flat.nodes["root"];
    assert_eq!(root.attributes["t::v"], json!(2));
    assert_eq!(root.provenance["t::v"].layer, "b");
    assert!(!root.attributes.contains_key("t::gone"));
    assert_eq!(root.children.keys().collect::<Vec<_>>(), vec!["x"]);
    assert_eq!(flat.layers, vec!["a", "b"]);
}

#[test]
fn order_then_unlisted_children_by_name() {
    let t = TempDir::new("order");
    let f = t.write("a.json", json!({"header": header("a"), "data": [
        {"path": "doc", "children": {"p1": "p1", "p2": "p2", "p3": "p3", "zz": "zz"}, "order": ["p3", "p1", "nope"]},
        {"path": "p1"}, {"path": "p2"}, {"path": "p3"}, {"path": "zz"}
    ]}));
    let flat = flatten(&load_stack(&f).unwrap());
    let mut c = Composer::new(&flat);
    let trees = c.compose_all().unwrap();
    let names: Vec<_> = trees[0]
        .children
        .iter()
        .map(|c| c.name.clone().unwrap())
        .collect();
    assert_eq!(names, vec!["p3", "p1", "p2", "zz"]);
    assert!(c.warnings.iter().any(|w| w.message.contains("nope")));
}

#[test]
fn inherits_expand_with_provenance_and_own_values_win() {
    let t = TempDir::new("inherit");
    let f = t.write("a.json", json!({"header": header("base"), "data": [
        {"path": "sheet", "children": {"table": "table"}},
        {"path": "table", "attributes": {"office::data::table": {"rows": [[1]]}, "grid::range": "A1"}},
        {"path": "deck", "children": {"t": "frame"}},
        {"path": "frame", "inherits": {"source": "table"}, "attributes": {"grid::range": "override"}}
    ]}));
    let flat = flatten(&load_stack(&f).unwrap());
    let mut c = Composer::new(&flat);
    let e = c.effective("frame").unwrap();
    assert_eq!(e.attributes["office::data::table"], json!({"rows": [[1]]}));
    assert_eq!(e.provenance["office::data::table"].via, vec!["table"]);
    assert_eq!(e.attributes["grid::range"], json!("override"));
    assert!(e.provenance["grid::range"].via.is_empty());
    // `table` is an inherit source, so it is not a root; `sheet` and `deck` are.
    assert_eq!(c.roots(), vec!["deck", "sheet"]);
}

#[test]
fn cycles_are_errors() {
    let t = TempDir::new("cycle");
    let f = t.write(
        "a.json",
        json!({"header": header("a"), "data": [
            {"path": "r", "children": {"a": "a"}},
            {"path": "a", "inherits": {"x": "b"}}, {"path": "b", "inherits": {"x": "a"}}
        ]}),
    );
    let flat = flatten(&load_stack(&f).unwrap());
    assert!(matches!(
        Composer::new(&flat).effective("a"),
        Err(OxError::InheritCycle(_))
    ));

    t.write(
        "p.json",
        json!({"header": header("p"), "imports": [{"uri": "q.json"}]}),
    );
    let q = t.write(
        "q.json",
        json!({"header": header("q"), "imports": [{"uri": "p.json"}]}),
    );
    assert!(matches!(load_stack(&q), Err(OxError::ImportCycle(_))));
}

#[test]
fn integrity_is_checked() {
    let t = TempDir::new("integrity");
    let a = t.write("a.json", json!({"header": header("a")}));
    let good = integrity_of(&fs::read(&a).unwrap());
    let ok = t.write(
        "ok.json",
        json!({"header": header("ok"), "imports": [{"uri": "a.json", "integrity": good}]}),
    );
    assert!(load_stack(&ok).is_ok());
    let bad = t.write(
        "bad.json",
        json!({"header": header("bad"), "imports": [{"uri": "a.json", "integrity": "sha256-00"}]}),
    );
    assert!(matches!(load_stack(&bad), Err(OxError::Integrity { .. })));
}

#[test]
fn validation_reports_schema_problems() {
    let t = TempDir::new("validate");
    let f = t.write("a.json", json!({"header": header("a"),
        "schemas": {
            "t::mode": {"value": {"dataType": "Enum", "enumRestrictions": {"options": ["live", "pinned"]}}},
            "t::obj": {"value": {"dataType": "Object", "objectRestrictions": {"values": {
                "need": {"dataType": "Real"}, "maybe": {"dataType": "String", "optional": true}}}}},
            "t::ref": {"value": {"dataType": "Reference"}}
        },
        "data": [
            {"path": "n", "attributes": {"t::mode": "bogus", "t::obj": {"maybe": 3}, "t::ref": "missing", "t::unknown": 1, "bad": 1}}
        ]}));
    let issues = validate(&flatten(&load_stack(&f).unwrap())).unwrap();
    let errors: Vec<_> = issues
        .iter()
        .filter(|i| i.level == Level::Error)
        .map(|i| i.message.clone())
        .collect();
    assert!(errors.iter().any(|m| m.contains("`bogus` is not one of")));
    assert!(errors.iter().any(|m| m.contains("missing `need`")));
    assert!(errors.iter().any(|m| m.contains("expected a string")));
    assert!(errors.iter().any(|m| m.contains("reference to `missing`")));
    assert!(errors.iter().any(|m| m.contains("namespace::name")));
    assert!(
        issues
            .iter()
            .any(|i| i.level == Level::Warning && i.attribute.as_deref() == Some("t::unknown"))
    );
}

#[test]
fn thousands() {
    assert_eq!(group_thousands(1250500.0), "1,250,500");
    assert_eq!(group_thousands(999.5), "999.50");
    assert_eq!(group_thousands(-1000.0), "-1,000");
}

#[test]
fn stable_id_matches_html2ox() {
    // node -e "…stableId('deck|deck.html')" in tools/html2ox gives the same value.
    assert_eq!(
        crate::model::stable_id("deck|deck.html"),
        "2af44a75-c514-5c87-b208-689fa8b15950"
    );
}

#[test]
fn derived_values_follow_their_source() {
    let t = TempDir::new("derived");
    t.write("a.json", json!({"header": header("base"), "data": [
        {"path": "root", "children": {"t": "table", "n": "count", "s": "sum", "f": "field"}},
        {"path": "table", "attributes": {"office::data::table": {
            "columns": [{"name": "Region", "type": "text"}, {"name": "Revenue", "type": "currency"}],
            "rows": [["A", 10], ["B", 20], ["C", 30]]}}},
        {"path": "count", "attributes": {"office::data::derived": {"source": "table", "op": "rowCount"}}},
        {"path": "sum", "attributes": {"office::data::derived": {"source": "table", "op": "sum", "column": "Revenue"}}},
        {"path": "field", "inherits": {"source": "count"}}
    ]}));
    let top = t.write("b.json", json!({"header": header("edit"), "imports": [{"uri": "a.json"}], "data": [
        {"path": "table", "attributes": {"office::data::table": {
            "columns": [{"name": "Region", "type": "text"}, {"name": "Revenue", "type": "currency"}],
            "rows": [["A", 10], ["B", 20], ["C", 30], ["D", 40]]}}}
    ]}));
    let flat = flatten(&load_stack(&top).unwrap());
    let mut c = Composer::new(&flat);
    assert_eq!(
        c.effective("count").unwrap().attributes["office::data::value"],
        json!({"value": 4, "format": "number"})
    );
    assert_eq!(
        c.effective("sum").unwrap().attributes["office::data::value"],
        json!({"value": 100, "format": "currency"})
    );
    let field = c.effective("field").unwrap();
    assert_eq!(field.attributes["office::data::value"]["value"], json!(4));
    assert_eq!(field.provenance["office::data::value"].layer, "edit");
}
