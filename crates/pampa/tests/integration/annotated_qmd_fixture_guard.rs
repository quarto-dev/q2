/*
 * annotated_qmd_fixture_guard.rs
 *
 * CI guard for the annotated-qmd example fixtures (bd-1d6io).
 *
 * The example JSON fixtures under `ts-packages/annotated-qmd/examples` are the
 * only artifacts in the tree that record full source ranges *and* assert the
 * substring invariant
 * (`source[range] == recorded text`). But they are static, hand-regenerated
 * files, inert to `cargo nextest` — the TS suite validates the frozen file's
 * internal consistency, not live pampa. So writer drift accumulated silently:
 *
 *   - The code-span left-widening rode along for ~7 months and only surfaced
 *     when Plan 7f Phase 5 forced a regeneration.
 *   - `div-attrs.json`'s `custom-key` range never matched live pampa at all,
 *     not even at the commit that introduced the fixture.
 *
 * This guard closes that hole: it runs the live writer over every example and
 * fails on any drift, so the next wire-format change fails at the PR that
 * introduces it rather than at the next forced regen.
 *
 * It regenerates to memory and never writes the committed fixtures.
 *
 * **This is the weaker of the two guards, deliberately.** It compares against a
 * snapshot, so regenerating the fixtures makes it green regardless of whether
 * the new ranges are *correct* — the same laundering the old workflow allowed,
 * one level up. The load-bearing check is `tiling_corpus_tests.rs`, which runs
 * the Plan 7g auditor over these same documents and asserts the
 * provenance-contract invariants as a *property*; no amount of regeneration can
 * launder a violation past that. Keep this one for what the auditor cannot see:
 * wire-format changes (pool packing, key renames, dropped fields) that are
 * invariant-preserving but still need a human to look.
 *
 * Copyright (c) 2026 Posit, PBC
 */

use std::path::{Path, PathBuf};
use std::process::Command;

/// Repo root, derived from this crate's manifest dir (`<root>/crates/pampa`).
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/pampa should be two levels below the repo root")
        .to_path_buf()
}

/// The examples directory, as a path relative to the repo root.
const EXAMPLES_REL: &str = "ts-packages/annotated-qmd/examples";

/// Collect the example `.qmd` basenames, sorted for deterministic reporting.
fn example_stems(examples_dir: &Path) -> Vec<String> {
    let mut stems: Vec<String> = std::fs::read_dir(examples_dir)
        .expect("annotated-qmd examples directory should exist")
        .filter_map(|entry| {
            let path = entry.expect("readable dir entry").path();
            if path.extension().and_then(|e| e.to_str()) == Some("qmd") {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .map(str::to_string)
            } else {
                None
            }
        })
        .collect();
    stems.sort();
    stems
}

#[test]
fn annotated_qmd_example_fixtures_match_live_writer() {
    let root = repo_root();
    let examples_dir = root.join(EXAMPLES_REL);
    let stems = example_stems(&examples_dir);
    assert!(
        !stems.is_empty(),
        "found no .qmd examples under {}",
        examples_dir.display()
    );

    let mut drifted: Vec<String> = Vec::new();
    let mut missing: Vec<String> = Vec::new();

    for stem in &stems {
        let json_path = examples_dir.join(format!("{stem}.json"));
        if !json_path.exists() {
            missing.push(stem.clone());
            continue;
        }

        // The fixtures embed the input path as a repo-relative string in
        // `astContext.files[].name`, so the writer must be invoked from the
        // repo root with the relative path — exactly as the regeneration
        // command in the failure message below does.
        let rel_qmd = format!("{EXAMPLES_REL}/{stem}.qmd");
        let output = Command::new(env!("CARGO_BIN_EXE_pampa"))
            .current_dir(&root)
            .args(["-t", "json", "-i", &rel_qmd])
            .output()
            .expect("failed to run the pampa binary");
        assert!(
            output.status.success(),
            "pampa failed on {rel_qmd}: status {:?}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr),
        );

        let committed = std::fs::read(&json_path).expect("committed fixture should be readable");
        if committed != output.stdout {
            drifted.push(stem.clone());
        }
    }

    assert!(
        missing.is_empty(),
        "these examples have a .qmd but no committed .json fixture: {}\n\
         Generate each with:\n  \
         cargo run --bin pampa -- -t json -i {EXAMPLES_REL}/<name>.qmd > {EXAMPLES_REL}/<name>.json",
        missing.join(", "),
    );

    assert!(
        drifted.is_empty(),
        "live pampa JSON output no longer matches these committed annotated-qmd \
         fixtures: {}\n\n\
         If your change intentionally alters the wire format or source ranges, \
         regenerate from the repo root:\n  \
         for f in {EXAMPLES_REL}/*.qmd; do \\\n    \
         cargo run --bin pampa -- -t json -i \"$f\" > \"${{f%.qmd}}.json\"; \\\n  \
         done\n\n\
         Then REVIEW THE DIFF: an unexplained range change here is a provenance \
         regression, not churn. Note that `npm test` in ts-packages/annotated-qmd \
         passing is NOT sufficient evidence the new ranges are right — its \
         substring invariant covers ~22 hand-picked nodes across 5 of the 20 \
         fixtures. The property-level check is \
         tests/integration/tiling_corpus_tests.rs; if it is green and the ranges \
         still look wrong, the invariant you need may not be encoded yet. \
         See claude-notes/designs/provenance-contract.md.",
        drifted.join(", "),
    );
}

#[test]
fn annotated_qmd_examples_use_lf_line_endings() {
    // The fixtures record byte offsets into the .qmd sources and
    // `astContext.files[].line_breaks`. A CRLF checkout shifts every offset,
    // so `.gitattributes` pins these sources to LF; this asserts the pin holds
    // (it would otherwise fail as ~20 opaque fixture mismatches on Windows).
    let examples_dir = repo_root().join(EXAMPLES_REL);
    let mut offenders: Vec<String> = Vec::new();

    for stem in example_stems(&examples_dir) {
        let bytes = std::fs::read(examples_dir.join(format!("{stem}.qmd")))
            .expect("example .qmd should be readable");
        if bytes.windows(2).any(|w| w == b"\r\n") {
            offenders.push(stem);
        }
    }

    assert!(
        offenders.is_empty(),
        "these example .qmd files contain CRLF line endings: {}\n\
         They must check out with LF — see the `{EXAMPLES_REL}/*.qmd text eol=lf` \
         rule in .gitattributes.",
        offenders.join(", "),
    );
}
