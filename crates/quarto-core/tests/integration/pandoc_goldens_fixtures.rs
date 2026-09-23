/*
 * pandoc_goldens_fixtures.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * P7 Task 12 (claude-notes/plans/2026-09-18-pandoc-hybrid-P7-implementation.md):
 * evaluates whether any pandoc-goldens fixture exercises the `algorithm`
 * theorem type (selected by the div id prefix `#alg-`, not the string
 * `.algorithm`, which isn't Quarto syntax). As of 2026-09-20 the answer is
 * no — see tests/fixtures/pandoc-goldens/README.md for the recorded
 * artifact and bd- follow-on strand. T12.2: if this test ever reddens
 * (someone added a fixture using `#alg-`/`@alg-`/`@Alg-`), that means the
 * condition now fires: land the `alg`/`Algorithm` entries in
 * `THEOREM_CLASSES` (crates/quarto-core/src/transforms/theorem.rs) and
 * `RefTypeRegistry::BUILTINS` (crates/quarto-core/src/crossref/registry.rs)
 * before trusting any golden captured from this fixture set, per the
 * plan's Task 12 acceptance criterion.
 */

use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pandoc-goldens")
}

fn qmd_files(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in walkdir(dir) {
        if entry.extension().and_then(|e| e.to_str()) == Some("qmd") {
            out.push(entry);
        }
    }
    out
}

fn walkdir(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walkdir(&path));
        } else {
            out.push(path);
        }
    }
    out
}

#[test]
fn test_no_fixture_uses_alg() {
    let dir = fixtures_dir();
    let files = qmd_files(&dir);
    assert!(
        files.len() >= 10,
        "expected at least the 10 pandoc-goldens fixtures, found {} under {}",
        files.len(),
        dir.display()
    );

    let mut hits = Vec::new();
    for file in &files {
        let content = std::fs::read_to_string(file).unwrap();
        if content.contains("#alg-") || content.contains("@alg-") || content.contains("@Alg-") {
            hits.push(file.display().to_string());
        }
    }

    assert!(
        hits.is_empty(),
        "fixture(s) {hits:?} use the `algorithm` theorem type (`#alg-`/`@alg-`/`@Alg-`); \
         per P7 Task 12, land `alg`/`Algorithm` in both `THEOREM_CLASSES` \
         (crates/quarto-core/src/transforms/theorem.rs) and \
         `RefTypeRegistry::BUILTINS` (crates/quarto-core/src/crossref/registry.rs) \
         before capturing goldens from this fixture set."
    );
}
