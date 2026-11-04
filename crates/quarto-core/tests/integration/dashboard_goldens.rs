/*
 * tests/integration/dashboard_goldens.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * The dashboard compatibility corpus (bd-dashboard-format-k21hd4vy, Phase 0).
 */

//! Q2's `format: dashboard` output against Quarto 1 goldens.
//!
//! The corpus (`tests/fixtures/dashboard-goldens/`) holds Q1's own dashboard
//! fixtures (`q1/`), Q1 regression fixtures (`regressions/`) and engine-free
//! contract fixtures (`contract/`), listed in `manifest.toml`. Each fixture's
//! Q1 output was reduced to a *dashboard skeleton* by
//! `cargo xtask capture-dashboard-goldens`; this test reduces Q2's output with
//! the same function (`quarto_output_extract::dashboard::dashboard_skeleton`)
//! and compares.
//!
//! A fixture's manifest `status` says what is asserted:
//! - `pending` — not implemented yet; skipped (its note names the phase);
//! - `match` — Q2's skeleton must equal the Q1 golden;
//! - `divergence` — Q2's skeleton must equal the `.q2.skeleton` golden (a
//!   deliberate difference, explained in the note);
//! - `deferred` — out of scope for the epic; skipped (its note names the strand).
//!
//! Flipping a fixture from `pending` to `match` or `divergence` is how each
//! implementation phase lands its TDD targets. Plan:
//! `claude-notes/plans/2026-11-03-dashboard-format-port.md`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_output_extract::dashboard::dashboard_skeleton;
use quarto_output_extract::dashboard_corpus::{
    Fixture, Manifest, Status, q1_golden_path, q2_golden_path,
};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

const FIXTURE_DIRS: &[&str] = &["acceptance", "contract", "q1", "q1-atx", "regressions"];

fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dashboard-goldens")
}

fn manifest() -> Manifest {
    Manifest::load(&corpus_root()).unwrap_or_else(|e| panic!("{e}"))
}

/// Every `.qmd` / `.ipynb` under the fixture dirs, as corpus-relative paths
/// with `/` separators (the manifest's spelling on every platform).
fn fixture_files() -> BTreeSet<String> {
    let root = corpus_root();
    let mut out = BTreeSet::new();
    for dir in FIXTURE_DIRS {
        for entry in walkdir::WalkDir::new(root.join(dir)) {
            let entry = entry.unwrap();
            let p = entry.path();
            let ext = p.extension().and_then(|e| e.to_str());
            if entry.file_type().is_file() && matches!(ext, Some("qmd" | "ipynb")) {
                let rel = p.strip_prefix(&root).unwrap();
                let parts: Vec<String> = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                out.insert(parts.join("/"));
            }
        }
    }
    out
}

#[test]
fn manifest_lists_every_fixture_exactly_once() {
    let m = manifest();
    let mut listed = BTreeSet::new();
    for f in &m.fixtures {
        assert!(listed.insert(f.path.clone()), "{} is listed twice", f.path);
    }
    let on_disk = fixture_files();
    let missing: Vec<_> = on_disk.difference(&listed).collect();
    let stale: Vec<_> = listed.difference(&on_disk).collect();
    assert!(
        missing.is_empty(),
        "fixtures missing from manifest.toml: {missing:?}"
    );
    assert!(
        stale.is_empty(),
        "manifest.toml lists files that do not exist: {stale:?}"
    );
}

#[test]
fn every_fixture_has_the_goldens_its_entry_promises() {
    let root = corpus_root();
    for f in &manifest().fixtures {
        let q1 = root.join(q1_golden_path(&f.path));
        assert_eq!(
            f.q1_golden,
            q1.exists(),
            "{}: q1_golden = {} but {} {}",
            f.path,
            f.q1_golden,
            q1.display(),
            if q1.exists() {
                "exists"
            } else {
                "is missing (run `cargo xtask capture-dashboard-goldens`)"
            }
        );
        let q2 = root.join(q2_golden_path(&f.path));
        assert_eq!(
            f.status == Status::Divergence,
            q2.exists(),
            "{}: a `.q2.skeleton` golden exists exactly when status = divergence",
            f.path
        );
    }
}

#[test]
fn no_orphan_goldens() {
    let root = corpus_root();
    let expected: BTreeSet<PathBuf> = manifest()
        .fixtures
        .iter()
        .flat_map(|f| [q1_golden_path(&f.path), q2_golden_path(&f.path)])
        .collect();
    for entry in std::fs::read_dir(root.join("goldens")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "skeleton") {
            let rel = Path::new("goldens").join(path.file_name().unwrap());
            assert!(expected.contains(&rel), "orphan golden {}", rel.display());
        }
    }
}

#[test]
fn notes_explain_each_status() {
    for f in &manifest().fixtures {
        assert!(!f.note.trim().is_empty(), "{}: empty note", f.path);
        match f.status {
            Status::Pending => assert!(
                f.note.starts_with("Phase "),
                "{}: a pending note must name its phase",
                f.path
            ),
            Status::Deferred => assert!(
                f.note.contains("bd-"),
                "{}: a deferred note must name its follow-up strand",
                f.path
            ),
            Status::Excluded => assert!(
                f.note.contains("q1-atx/") || f.note.contains("Q2 "),
                "{}: an excluded note must say why and where the usable copy lives",
                f.path
            ),
            Status::Match | Status::Divergence => {}
        }
        if !f.q1_golden {
            assert!(
                f.status != Status::Match,
                "{}: a match fixture needs a Q1 golden",
                f.path
            );
        }
    }
}

fn required_engine_unavailable(requires: &[String]) -> Option<String> {
    let registry = quarto_core::engine::EngineRegistry::new();
    requires.iter().find_map(|name| {
        let ok = registry.get(name).is_some_and(|e| e.is_available());
        (!ok).then(|| format!("engine '{name}' not available"))
    })
}

/// Render a fixture as a dashboard in a scratch copy of its directory (so
/// sibling resources resolve and the corpus is never written to).
fn render_skeleton(f: &Fixture) -> Result<String, String> {
    let root = corpus_root();
    let src = root.join(&f.path);
    let temp = tempfile::TempDir::new().unwrap();
    let src_dir = src.parent().unwrap();
    for entry in walkdir::WalkDir::new(src_dir) {
        let entry = entry.unwrap();
        let rel = entry.path().strip_prefix(src_dir).unwrap();
        let dst = temp.path().join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&dst).unwrap();
        } else {
            std::fs::copy(entry.path(), &dst).unwrap();
        }
    }
    let input = temp.path().join(src.file_name().unwrap());
    let options = RenderToFileOptions {
        quiet: true,
        ..Default::default()
    };
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_to_file(&input, "dashboard", &options, runtime)
        .map_err(|e| format!("{}: dashboard render failed: {e:?}", f.path))?;
    let html = std::fs::read_to_string(&result.output_path).map_err(|e| e.to_string())?;
    Ok(dashboard_skeleton(&html))
}

/// The corpus assertion. Fixtures are checked one after another and all
/// mismatches are reported together, so one run shows every regression.
#[test]
fn dashboard_skeletons_match_goldens() {
    let root = corpus_root();
    let mut failures = Vec::new();
    let mut checked = 0usize;
    for f in &manifest().fixtures {
        let golden = match f.status {
            Status::Pending | Status::Deferred | Status::Excluded => continue,
            Status::Match => q1_golden_path(&f.path),
            Status::Divergence => q2_golden_path(&f.path),
        };
        if let Some(reason) = required_engine_unavailable(&f.requires) {
            eprintln!("skipping {}: {reason}", f.path);
            continue;
        }
        let expected = std::fs::read_to_string(root.join(&golden)).unwrap();
        checked += 1;
        let actual = match render_skeleton(f) {
            Ok(actual) => actual,
            Err(e) => {
                failures.push(e);
                continue;
            }
        };
        if actual != expected {
            let line = expected
                .lines()
                .zip(actual.lines())
                .position(|(e, a)| e != a)
                .unwrap_or_else(|| expected.lines().count().min(actual.lines().count()));
            failures.push(format!(
                "{} (vs {}), first difference at line {}:\n  expected: {}\n  actual:   {}",
                f.path,
                golden.display(),
                line + 1,
                expected.lines().nth(line).unwrap_or("<end>"),
                actual.lines().nth(line).unwrap_or("<end>"),
            ));
        }
    }
    eprintln!("checked {checked} dashboard fixture(s)");
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
