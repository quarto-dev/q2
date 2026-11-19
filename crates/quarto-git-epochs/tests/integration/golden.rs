//! The golden documents written by the TypeScript fixture generator decode to what the manifest says,
//! and the operations of this crate reproduce the before/after documents.

use crate::registry::{build, entry, sorted};
use crate::support::{fixtures_dir, read_fixture_json};
use automerge::AutoCommit;
use quarto_git_epochs::registry::{Entry, doc, registry_key};
use std::path::PathBuf;

fn golden(name: &str) -> PathBuf {
    fixtures_dir().join("golden").join(name)
}

fn load(name: &str) -> AutoCommit {
    AutoCommit::load(&std::fs::read(golden(name)).unwrap()).unwrap()
}

fn dump(es: Vec<Entry>) -> Vec<(String, i64)> {
    let mut v: Vec<(String, i64)> = es
        .into_iter()
        .map(|e| {
            (
                registry_key(&e.path, &e.blob, &e.doc).unwrap(),
                e.generation,
            )
        })
        .collect();
    v.sort();
    v
}

fn manifest_pairs(v: &serde_json::Value) -> Vec<(String, i64)> {
    let mut out: Vec<(String, i64)> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["key"].as_str().unwrap().to_string(),
                e["gen"].as_i64().unwrap(),
            )
        })
        .collect();
    out.sort();
    out
}

#[test]
fn reads_the_logical_entries_of_every_sidecar_golden_document() {
    let manifest = read_fixture_json("golden/manifest.json");
    let mut n = 0;
    for (file, expected) in manifest.as_object().unwrap() {
        if expected.get("entries").is_none() {
            continue;
        }
        n += 1;
        let d = load(file);
        assert_eq!(
            dump(doc::read_entries(&d)),
            manifest_pairs(&expected["entries"]),
            "{file}"
        );
        assert_eq!(
            dump(doc::read_retired(&d)),
            manifest_pairs(&expected["retired"]),
            "{file}"
        );
    }
    assert!(n >= 8, "found {n} sidecar documents");
}

#[test]
fn move_forward_applied_to_the_before_doc_equals_the_after_doc() {
    let mut d = load("move-forward-before.bin");
    doc::move_forward(&mut d, "a.qmd", "b1", "b2", "docA");
    assert_eq!(
        sorted(doc::read_entries(&d)),
        sorted(doc::read_entries(&load("move-forward-after.bin")))
    );
}

#[test]
fn retire_applied_equals_the_retire_doc() {
    let mut d = build(&[entry("a.qmd", "b1", "docA", 0)], &[]);
    doc::retire(&mut d, "a.qmd", "b1", "docA");
    assert_eq!(
        sorted(doc::read_retired(&d)),
        sorted(doc::read_retired(&load("retire.bin")))
    );
}

#[test]
fn repair_applied_to_an_empty_registry_equals_the_repair_doc() {
    let mut d = doc::create_sidecar();
    doc::repair(&mut d, "a.qmd", "docA", 1, "b1");
    assert_eq!(
        sorted(doc::read_entries(&d)),
        sorted(doc::read_entries(&load("repair.bin")))
    );
}

#[test]
fn reads_the_epochs_pointer_in_both_the_text_and_the_scalar_form() {
    let manifest = read_fixture_json("golden/manifest.json");
    for f in ["index-v4-text-form.bin", "index-v4-scalar-form.bin"] {
        let d = load(f);
        let got = doc::read_index_fields(&d);
        let want = &manifest[f]["index"];
        assert_eq!(got.epochs.as_deref(), want["epochs"].as_str(), "{f}");
        assert_eq!(got.owner.as_deref(), want["owner"].as_str(), "{f}");
        let repo = got.repo.unwrap();
        assert_eq!(repo.prefix, want["repo"]["prefix"].as_str().unwrap(), "{f}");
        let roots: Vec<&str> = want["repo"]["roots"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r.as_str().unwrap())
            .collect();
        assert_eq!(repo.roots, roots, "{f}");
    }
}

/// The document this crate writes: generations stored as `int`, read by the TypeScript package (GE-14
/// cross-checks it). `UPDATE_GOLDEN=1` rewrites the `.bin`; the manifest entry is added by hand with `jq`.
#[test]
fn the_doc_this_crate_writes_is_committed_and_decodes_to_what_the_operations_produce() {
    let file = "from-rust-registry-ops.bin";
    let mut d = doc::create_sidecar();
    doc::register(&mut d, &entry("a.qmd", "b1", "docA", 0));
    doc::register(&mut d, &entry("b.qmd", "c1", "docB", 0));
    doc::move_forward(&mut d, "a.qmd", "b1", "b2", "docA");
    doc::retire(&mut d, "b.qmd", "c1", "docB");
    let next = quarto_git_epochs::registry::next_generation(&doc::view_of(&d), "b.qmd");
    doc::register(&mut d, &entry("b.qmd", "c1", "docC", next));
    if std::env::var("UPDATE_GOLDEN").as_deref() == Ok("1") {
        std::fs::write(golden(file), d.save()).unwrap();
    }
    let on_disk = load(file);
    assert_eq!(
        dump(doc::read_entries(&on_disk)),
        dump(doc::read_entries(&d))
    );
    assert_eq!(
        dump(doc::read_retired(&on_disk)),
        dump(doc::read_retired(&d))
    );
    let manifest = read_fixture_json("golden/manifest.json");
    assert_eq!(
        dump(doc::read_entries(&on_disk)),
        manifest_pairs(&manifest[file]["entries"])
    );
    assert_eq!(
        dump(doc::read_retired(&on_disk)),
        manifest_pairs(&manifest[file]["retired"])
    );
}
