/*
 * tests/integration/marimo_fixture_fresh.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Freshness gate for the committed marimo fixture
 * (`tests/fixtures/extensions/marimo/_extensions/marimo`). The fixture is a
 * built copy of gordonwoodhull/quarto-marimo (`q2/node-port`), synced by
 * `scripts/sync-marimo-fixture.sh`, which records the source commit and a
 * sha256 per file in `FIXTURE-SOURCE`.
 *
 * Always checked: every recorded hash matches the file on disk and no file is
 * unrecorded, so a hand edit or half-finished re-sync fails here.
 *
 * Checked when `QUARTO_MARIMO_SRC` points at a quarto-marimo checkout: that
 * checkout is at the recorded commit and a fresh `npm run build` reproduces
 * the committed bundle byte for byte (the fork's build is deterministic).
 */

#![cfg(not(target_arch = "wasm32"))]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

fn extension_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/extensions/marimo/_extensions/marimo")
}

fn sha256_hex(path: &Path) -> String {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `(source commit, relative path -> sha256)` from `FIXTURE-SOURCE`.
fn read_manifest() -> (String, BTreeMap<String, String>) {
    let text = std::fs::read_to_string(extension_dir().join("FIXTURE-SOURCE"))
        .expect("FIXTURE-SOURCE missing: run scripts/sync-marimo-fixture.sh");
    let mut commit = String::new();
    let mut files = BTreeMap::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("source: ") {
            commit = rest.rsplit(' ').next().unwrap().to_string();
        } else if let Some((hash, path)) = line.split_once("  ") {
            files.insert(path.to_string(), hash.to_string());
        }
    }
    assert!(!commit.is_empty(), "FIXTURE-SOURCE has no `source:` line");
    (commit, files)
}

fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            walk(&path, base, out);
        } else {
            let rel = path.strip_prefix(base).unwrap();
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

#[test]
fn marimo_fixture_matches_recorded_hashes() {
    let (_, manifest) = read_manifest();
    let dir = extension_dir();
    let mut on_disk = Vec::new();
    walk(&dir, &dir, &mut on_disk);
    on_disk.retain(|p| p != "FIXTURE-SOURCE");
    on_disk.sort();
    let recorded: Vec<&String> = manifest.keys().collect();
    assert_eq!(
        on_disk.iter().collect::<Vec<_>>(),
        recorded,
        "fixture files differ from FIXTURE-SOURCE; re-run scripts/sync-marimo-fixture.sh"
    );
    for (rel, want) in &manifest {
        assert_eq!(
            &sha256_hex(&dir.join(rel)),
            want,
            "{rel} differs from the recorded build; re-run scripts/sync-marimo-fixture.sh \
             (do not hand-edit the fixture)"
        );
    }
}

#[test]
fn marimo_fixture_bundle_is_a_fresh_build_of_the_recorded_commit() {
    let Some(src) = std::env::var_os("QUARTO_MARIMO_SRC") else {
        eprintln!(
            "SKIP: QUARTO_MARIMO_SRC not set — marimo_fixture_bundle_is_a_fresh_build_of_the_recorded_commit"
        );
        return;
    };
    let src = PathBuf::from(src);
    let (commit, manifest) = read_manifest();
    let head = Command::new("git")
        .arg("-C")
        .arg(&src)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("git rev-parse");
    assert_eq!(
        String::from_utf8_lossy(&head.stdout).trim(),
        commit,
        "QUARTO_MARIMO_SRC is not at the commit recorded in FIXTURE-SOURCE"
    );
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let build = Command::new(npm)
        .args(["run", "build"])
        .current_dir(&src)
        .output()
        .expect("npm run build");
    assert!(build.status.success(), "npm run build failed");
    let version = manifest
        .keys()
        .find_map(|p| p.strip_prefix("assets/browser-v")?.strip_suffix(".js"))
        .expect("browser asset in manifest")
        .to_string();
    for (built, committed) in [
        (
            format!("dist/marimo-engine-v{version}.js"),
            "marimo-engine.js".to_string(),
        ),
        (
            format!("dist/browser-v{version}.js"),
            format!("assets/browser-v{version}.js"),
        ),
        (
            format!("dist/islands-bridge-v{version}.css"),
            format!("assets/islands-bridge-v{version}.css"),
        ),
    ] {
        assert_eq!(
            sha256_hex(&src.join(&built)),
            sha256_hex(&extension_dir().join(&committed)),
            "{committed} is stale relative to a fresh build of {commit}"
        );
    }
}
