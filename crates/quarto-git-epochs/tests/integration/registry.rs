use crate::support::read_fixture_json;
use automerge::{ActorId, AutoCommit, ObjType, ROOT, ReadDoc, transaction::Transactable};
use quarto_git_epochs::registry::doc;
use quarto_git_epochs::registry::{
    Entry, RegistryOp, RegistryView, canonical, case_collisions, entries_for, live_elsewhere,
    next_generation, parse_key, plan_move_forward, plan_register, plan_repair, plan_retire,
    registry_key,
};

pub fn entry(path: &str, blob: &str, d: &str, generation: i64) -> Entry {
    Entry {
        path: path.into(),
        blob: blob.into(),
        doc: d.into(),
        generation,
    }
}

pub fn sorted(mut es: Vec<Entry>) -> Vec<Entry> {
    es.sort_by(|a, b| {
        registry_key(&a.path, &a.blob, &a.doc)
            .unwrap()
            .cmp(&registry_key(&b.path, &b.blob, &b.doc).unwrap())
    });
    es
}

/// A sidecar doc holding exactly these entries and retired entries.
pub fn build(entries: &[Entry], retired: &[Entry]) -> AutoCommit {
    let mut d = doc::create_sidecar();
    let mut ops = Vec::new();
    for e in entries {
        ops.push(RegistryOp::Put {
            key: registry_key(&e.path, &e.blob, &e.doc).unwrap(),
            generation: e.generation,
        });
    }
    for e in retired {
        ops.push(RegistryOp::Retire {
            key: registry_key(&e.path, &e.blob, &e.doc).unwrap(),
            generation: e.generation,
        });
    }
    doc::apply(&mut d, &ops);
    d
}

fn actors(base: &AutoCommit) -> (AutoCommit, AutoCommit) {
    let mut a = base.clone().fork();
    let mut b = base.clone().fork();
    a.set_actor(ActorId::from(vec![0xaa; 16]));
    b.set_actor(ActorId::from(vec![0xbb; 16]));
    (a, b)
}

fn rows() -> Vec<serde_json::Value> {
    read_fixture_json("decision-tables/registry.json")
        .as_array()
        .unwrap()
        .clone()
}

fn entries_of(v: &serde_json::Value) -> Vec<Entry> {
    serde_json::from_value(v.clone()).unwrap()
}

#[test]
fn shared_fixture_rows() {
    let rows = rows();
    assert!(rows.len() >= 16);
    for r in rows {
        let id = r["id"].as_str().unwrap();
        let base = build(&entries_of(&r["entries"]), &entries_of(&r["retired"]));
        let args = &r["args"];
        let expected = &r["expected"];
        let s = |k: &str| args[k].as_str().map(str::to_string);
        match r["op"].as_str().unwrap() {
            "canonical" => {
                let v = doc::view_of(&base);
                let got =
                    canonical(&v, &s("path").unwrap(), &s("blob").unwrap()).map(|e| e.doc.clone());
                assert_eq!(got, expected["doc"].as_str().map(str::to_string), "{id}");
            }
            "create" => {
                let v = doc::view_of(&base);
                assert_eq!(
                    next_generation(&v, &s("path").unwrap()),
                    expected["gen"].as_i64().unwrap(),
                    "{id}"
                );
            }
            "moveForward" => {
                let mut d = base;
                doc::move_forward(
                    &mut d,
                    &s("path").unwrap(),
                    &s("oldBlob").unwrap(),
                    &s("newBlob").unwrap(),
                    &s("doc").unwrap(),
                );
                assert_eq!(
                    sorted(doc::read_entries(&d)),
                    sorted(entries_of(&expected["entries"])),
                    "{id}"
                );
            }
            "union" => {
                let (mut x, mut y) = actors(&base);
                for (d, side) in [(&mut x, 0), (&mut y, 1)] {
                    for e in entries_of(&args["sides"][side]) {
                        doc::register(d, &e);
                    }
                }
                x.merge(&mut y).unwrap();
                let mut keys: Vec<(String, String, String)> = doc::read_entries(&x)
                    .into_iter()
                    .map(|e| (e.path, e.blob, e.doc))
                    .collect();
                keys.sort();
                let mut want: Vec<(String, String, String)> = expected["keys"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|k| {
                        (
                            k[0].as_str().unwrap().into(),
                            k[1].as_str().unwrap().into(),
                            k[2].as_str().unwrap().into(),
                        )
                    })
                    .collect();
                want.sort();
                assert_eq!(keys, want, "{id}");
            }
            "putDelete" => {
                let k = &args["key"];
                let key = registry_key(
                    k[0].as_str().unwrap(),
                    k[1].as_str().unwrap(),
                    k[2].as_str().unwrap(),
                )
                .unwrap();
                let put_gen = args["putGen"].as_i64().unwrap();
                let want = sorted(entries_of(&expected["entries"]));
                for order in [0, 1] {
                    let (mut x, mut y) = actors(&base);
                    doc::apply(&mut x, &[RegistryOp::Delete { key: key.clone() }]);
                    doc::apply(
                        &mut y,
                        &[RegistryOp::Put {
                            key: key.clone(),
                            generation: put_gen,
                        }],
                    );
                    if order == 0 {
                        x.merge(&mut y).unwrap();
                        assert_eq!(sorted(doc::read_entries(&x)), want, "{id}");
                    } else {
                        y.merge(&mut x).unwrap();
                        assert_eq!(sorted(doc::read_entries(&y)), want, "{id}");
                    }
                }
            }
            "retire" => {
                let mut d = base;
                doc::retire(
                    &mut d,
                    &s("path").unwrap(),
                    &s("blob").unwrap(),
                    &s("doc").unwrap(),
                );
                assert_eq!(
                    sorted(doc::read_entries(&d)),
                    sorted(entries_of(&expected["entries"])),
                    "{id}"
                );
                assert_eq!(
                    sorted(doc::read_retired(&d)),
                    sorted(entries_of(&expected["retired"])),
                    "{id}"
                );
            }
            "repair" => {
                let mut d = base;
                let a = &args["attached"];
                doc::repair(
                    &mut d,
                    a["path"].as_str().unwrap(),
                    a["doc"].as_str().unwrap(),
                    a["gen"].as_i64().unwrap(),
                    args["headBlob"].as_str().unwrap(),
                );
                assert_eq!(
                    sorted(doc::read_entries(&d)),
                    sorted(entries_of(&expected["entries"])),
                    "{id}"
                );
            }
            other => panic!("row {id}: unknown op {other}"),
        }
    }
}

#[test]
fn keys_are_path_nul_blob_nul_doc_and_round_trip() {
    let k = registry_key("a/b.qmd", "b1", "doc").unwrap();
    assert_eq!(k, "a/b.qmd\0b1\0doc");
    let p = parse_key(&k).unwrap();
    assert_eq!(
        (p.path.as_str(), p.blob.as_str(), p.doc.as_str()),
        ("a/b.qmd", "b1", "doc")
    );
    assert!(parse_key("no-nul").is_none());
}

#[test]
fn a_path_with_backslashes_is_rejected() {
    assert!(registry_key("a\\b.qmd", "b1", "d").is_err());
}

#[test]
fn register_is_put_if_absent() {
    let mut d = doc::create_sidecar();
    assert!(doc::register(&mut d, &entry("a", "b", "d", 0)));
    assert!(!doc::register(&mut d, &entry("a", "b", "d", 7)));
    assert_eq!(doc::read_entries(&d)[0].generation, 0);
    let v = doc::view_of(&d);
    assert!(plan_register(&v, &entry("a", "b", "d", 7)).is_empty());
}

#[test]
fn move_forward_is_a_single_automerge_change() {
    let mut d = build(&[entry("a", "b1", "d", 2)], &[]);
    let before = d.get_heads();
    doc::move_forward(&mut d, "a", "b1", "b2", "d");
    assert_eq!(d.get_changes(&before).len(), 1);
    let v = doc::view_of(&d);
    assert_eq!(v.entries, vec![entry("a", "b2", "d", 2)]);
    // The pure plan carries both halves.
    let before_view = doc::view_of(&build(&[entry("a", "b1", "d", 2)], &[]));
    assert_eq!(
        plan_move_forward(&before_view, "a", "b1", "b2", "d").len(),
        2
    );
}

#[test]
fn retire_never_removes_and_canonical_then_skips() {
    let mut d = build(&[entry("a", "b", "d", 0)], &[]);
    doc::retire(&mut d, "a", "b", "d");
    let v = doc::view_of(&d);
    assert_eq!(v.entries.len(), 1);
    assert!(canonical(&v, "a", "b").is_none());
    assert!(
        plan_retire(&v, "a", "b", "d").is_empty(),
        "retiring twice changes nothing"
    );
}

#[test]
fn entries_for_returns_all_blobs_of_a_path() {
    let d = build(
        &[
            entry("a", "1", "x", 0),
            entry("a", "2", "y", 0),
            entry("b", "1", "z", 0),
        ],
        &[],
    );
    assert_eq!(entries_for(&doc::view_of(&d), "a").len(), 2);
}

#[test]
fn case_collisions_find_paths_equal_when_lowercased_and_only_those() {
    let d = build(
        &[
            entry("A.qmd", "1", "x", 0),
            entry("a.qmd", "1", "y", 0),
            entry("c.qmd", "1", "z", 0),
        ],
        &[],
    );
    let got: Vec<String> = case_collisions(&doc::view_of(&d)).into_iter().collect();
    assert_eq!(got, vec!["A.qmd", "a.qmd"]);
}

#[test]
fn live_elsewhere_reports_a_doc_registered_under_another_path() {
    let d = build(&[entry("a", "1", "x", 0)], &[]);
    let v = doc::view_of(&d);
    assert!(live_elsewhere(&v, "x", "b"));
    assert!(!live_elsewhere(&v, "x", "a"));
}

#[test]
fn canonical_breaks_ties_bytewise_on_the_doc_id() {
    let d = build(
        &[
            entry("a", "1", "docB", 0),
            entry("a", "1", "docA", 0),
            entry("a", "1", "Zdoc", 0),
        ],
        &[],
    );
    assert_eq!(canonical(&doc::view_of(&d), "a", "1").unwrap().doc, "Zdoc");
}

#[test]
fn repair_plan_registers_only_a_doc_with_no_entry_and_no_retirement() {
    let v = RegistryView::default();
    assert_eq!(plan_repair(&v, "a", "d", 1, "b1").len(), 1);
}

#[test]
fn the_sidecar_is_created_with_both_maps_in_its_first_change() {
    let mut d = doc::create_sidecar();
    assert_eq!(d.get_changes(&[]).len(), 1);
    assert!(matches!(
        d.get(ROOT, "entries").unwrap().unwrap().0,
        automerge::Value::Object(ObjType::Map)
    ));
    assert!(matches!(
        d.get(ROOT, "retired").unwrap().unwrap().0,
        automerge::Value::Object(ObjType::Map)
    ));
    let v = d.get(ROOT, "version").unwrap().unwrap().0;
    assert_eq!(v.as_i64(), Some(1));
    // The doc has no other writes yet.
    let _ = d.put(ROOT, "x", 1_i64);
}
