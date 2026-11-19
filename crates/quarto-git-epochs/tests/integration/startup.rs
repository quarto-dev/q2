use crate::support::read_fixture_json;
use quarto_git_epochs::startup::{
    FileStartup, StartupCase, StartupInput, StartupRecord, startup_decision,
};
use std::collections::BTreeMap;

fn blobs(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

fn record(head: &str, b: &[(&str, &str)], detached: &[&str]) -> StartupRecord {
    StartupRecord {
        head: head.into(),
        blobs: blobs(b),
        detached: detached.iter().map(|s| s.to_string()).collect(),
    }
}

#[test]
fn every_fixture_row() {
    let rows = read_fixture_json("decision-tables/startup.json");
    for r in rows.as_array().unwrap() {
        let rec: Option<StartupRecord> = serde_json::from_value(r["record"].clone()).unwrap();
        let head = r["head"].as_str().unwrap().to_string();
        // The rows name the overall case. Derive the current blobs from the record: when HEAD changed,
        // the first file's blob differs and the others are the same.
        let current: BTreeMap<String, String> = match &rec {
            Some(rec) => rec
                .blobs
                .iter()
                .enumerate()
                .map(|(i, (p, b))| {
                    let changed = rec.head != head && i == 0;
                    (
                        p.clone(),
                        if changed {
                            format!("{b}-new")
                        } else {
                            b.clone()
                        },
                    )
                })
                .collect(),
            None => blobs(&[("a.qmd", "b1")]),
        };
        let plan = startup_decision(&StartupInput {
            record: rec,
            head,
            blobs: current,
        });
        let expected: StartupCase = serde_json::from_value(r["expected"].clone()).unwrap();
        assert_eq!(plan.case, expected, "row {}", r["id"]);
    }
}

#[test]
fn reconciles_every_file_when_head_is_unchanged() {
    let p = startup_decision(&StartupInput {
        record: Some(record("h", &[("a", "1"), ("b", "2")], &[])),
        head: "h".into(),
        blobs: blobs(&[("a", "1"), ("b", "2")]),
    });
    assert!(p.files.values().all(|f| *f == FileStartup::Reconcile));
}

#[test]
fn arrives_for_changed_blobs_and_for_unseen_files() {
    let p = startup_decision(&StartupInput {
        record: Some(record("h", &[("a", "1"), ("b", "2")], &[])),
        head: "h2".into(),
        blobs: blobs(&[("a", "1"), ("b", "9"), ("c", "3")]),
    });
    assert_eq!(p.case, StartupCase::ArriveDiffering);
    assert_eq!(p.files["a"], FileStartup::Reconcile);
    assert_eq!(p.files["b"], FileStartup::Arrive);
    assert_eq!(p.files["c"], FileStartup::Arrive);
}

#[test]
fn no_record_never_merges() {
    let p = startup_decision(&StartupInput {
        record: None,
        head: "h".into(),
        blobs: blobs(&[("a", "1"), ("b", "2")]),
    });
    assert_eq!(p.case, StartupCase::ArriveAll);
    assert!(p.files.values().all(|f| *f == FileStartup::Arrive));
}

#[test]
fn a_detached_path_stays_detached_while_its_blob_is_unchanged() {
    let rec = record("h", &[("a", "1"), ("b", "2")], &["a", "b"]);
    let same = startup_decision(&StartupInput {
        record: Some(rec.clone()),
        head: "h".into(),
        blobs: blobs(&[("a", "1"), ("b", "2")]),
    });
    assert!(same.files.values().all(|f| *f == FileStartup::StayDetached));
    let moved = startup_decision(&StartupInput {
        record: Some(rec),
        head: "h2".into(),
        blobs: blobs(&[("a", "1"), ("b", "9")]),
    });
    assert_eq!(moved.files["a"], FileStartup::StayDetached);
    assert_eq!(moved.files["b"], FileStartup::Arrive);
}

#[test]
fn files_that_left_the_tree_are_dropped() {
    let p = startup_decision(&StartupInput {
        record: Some(record("h", &[("a", "1"), ("gone", "2")], &[])),
        head: "h2".into(),
        blobs: blobs(&[("a", "1")]),
    });
    assert_eq!(p.files.keys().collect::<Vec<_>>(), vec!["a"]);
}
