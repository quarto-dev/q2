//! Drift guard for the warm pandoc executor's argv translator (H10a Task 2a).
//!
//! The browser's warm executor turns a request's argv into pandoc *defaults*
//! (`argvToDefaults` in `ts-packages/pandoc-host`). That translator handles a
//! fixed set of flags, listed in `ts-packages/pandoc-host/src/typst-argv-flags.json`.
//! This test renders a matrix of small documents (one per branching input of
//! `pandoc_write.rs`'s argv builder) as typst and pdf requests and requires that
//! the flags the builder emits and the flags the file lists are the same set:
//! a flag added to the builder fails here until the file and the translator
//! are updated; a listed flag no matrix document emits fails too.
//!
//! The file lists flag *names* only. Flag values (`-t typst-citations`, `-V`
//! keys) are covered by the translator's unit tests and the warm-versus-fresh
//! parity test, which asserts zero fallbacks over the same matrix.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::pandoc_request_typst::{render_with, scratch};

/// Flags that take a value in the next token (`--name=value` is split at `=`).
const VALUE_FLAGS: &[&str] = &[
    "-f",
    "-t",
    "-L",
    "-o",
    "-V",
    "--data-dir",
    "--wrap",
    "--default-image-extension",
    "--resource-path",
    "--shift-heading-level-by",
    "--template",
    "--reference-doc",
    "--top-level-division",
    "--defaults",
];

/// The flag names in an argv (program name first): a token starting with `-`
/// that is not the value of the flag before it. A value that itself starts
/// with `-` (`--shift-heading-level-by -1`) is consumed, not read as a flag.
fn flag_names(argv: &[String]) -> Vec<String> {
    let mut names = Vec::new();
    let mut i = 1;
    while i < argv.len() {
        let token = &argv[i];
        i += 1;
        if !token.starts_with('-') {
            continue; // the positional input
        }
        let name = token.split('=').next().unwrap().to_string();
        if !token.contains('=') && VALUE_FLAGS.contains(&name.as_str()) {
            i += 1;
        }
        names.push(name);
    }
    names
}

fn matrix_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pandoc-argv-matrix")
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn listed_flags() -> BTreeSet<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ts-packages/pandoc-host/src/typst-argv-flags.json");
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(
        json["version"], 1,
        "bump the translator's version with the file"
    );
    json["flags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn the_tokenizer_consumes_values_that_start_with_a_dash() {
    let argv: Vec<String> = [
        "pandoc",
        "--shift-heading-level-by",
        "-1",
        "--syntax-highlighting=idiomatic",
        "-V",
        "k=v",
        "-o",
        "/out.typ",
        "/in.json",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(
        flag_names(&argv),
        [
            "--shift-heading-level-by",
            "--syntax-highlighting",
            "-V",
            "-o"
        ]
    );
}

#[test]
fn the_matrix_emits_exactly_the_flags_the_translator_lists() {
    let listed = listed_flags();
    let mut emitted: BTreeSet<String> = BTreeSet::new();
    let mut cases = 0;
    let mut dirs: Vec<_> = std::fs::read_dir(matrix_root())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for case in dirs {
        let name = case.file_name().unwrap().to_string_lossy().to_string();
        for format in ["typst", "typst-pdf"] {
            let (_guard, root) = scratch();
            copy_dir(&case, &root);
            let fonts = (format == "typst-pdf").then(|| vec!["Libertinus Serif".to_string()]);
            let out = render_with(&root.join("doc.qmd"), format, fonts);
            assert!(out.error.is_none(), "{name} ({format}): {:?}", out.error);
            let request = out
                .request
                .unwrap_or_else(|| panic!("{name} ({format}): no request"));
            assert!(
                request.writer == "typst" || request.writer.starts_with("typst-"),
                "{name}: writer {}",
                request.writer
            );
            for flag in flag_names(&request.argv) {
                assert!(
                    listed.contains(&flag),
                    "{name} ({format}) emits `{flag}`, which typst-argv-flags.json does not list; \
                     add it there and teach `argvToDefaults` (ts-packages/pandoc-host) about it"
                );
                emitted.insert(flag);
            }
        }
        cases += 1;
    }
    assert!(cases >= 10, "matrix looks empty: {cases} cases");
    let unused: Vec<_> = listed.difference(&emitted).collect();
    assert!(
        unused.is_empty(),
        "typst-argv-flags.json lists flags no matrix document emits: {unused:?}; \
         add a matrix case that emits each, or remove the flag"
    );
}
