/*
 * engine/jupyter/presenter_goldens.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * EXR-1 Phase 0: golden characterization of the cell presenter.
 */

//! Golden characterization of the presenter (cell outputs to markdown).
//!
//! The goldens under `tests/fixtures/presenter-goldens/` were generated from the
//! unmodified tree, before EXR-1 moved anything, and must never change in a later
//! phase (`scripts/check-presenter-goldens-unchanged.sh` enforces it). Only the
//! `use` lines below are re-pointed when the presenter functions move.
//!
//! Two kinds of case, both kernel-free:
//!
//! - `notebooks/<case>/notebook.ipynb`: driven through [`IpynbReplayEngine`], which
//!   exercises `render_cell`, `format_outputs`, `FsFigureSink`, `CellVisibility`,
//!   `resolve_cell_options` and the nbformat reader together. An optional
//!   `scope.json` (flat map of booleans) becomes the document `execute:` scope.
//! - `direct/<case>/expected.md`: results built by hand in [`direct_cases`] and passed
//!   straight to `render_cell`, for states the stored reader cannot produce.
//!
//! Each notebook case checks the markdown (`expected.md`), the supporting files
//! reported (`expected-supporting-files.txt`) and every file written beside the
//! notebook (`expected-files/`, names and bytes).
//!
//! Regenerating (Phase 0 only): `PRESENTER_GOLDENS_BLESS=1`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use quarto_pandoc_types::{ConfigMapEntry, ConfigValue};
use quarto_source_map::{SourceContext, SourceInfo};

use super::execute::{CellOutput, CellResult, ExecuteStatus};
use super::stored::IpynbReplayEngine;
use crate::engine::content_processors::ORIGINAL_FILE_ID;
use crate::engine::content_processors::ipynb::convert_notebook;
use crate::engine::context::ExecutionContext;
use crate::engine::traits::ExecutionEngine;
use crate::presenter::cell::render_cell;
use crate::presenter::figures::{FigureSink, FsFigureSink};
use crate::presenter::visibility::CellVisibility;

fn goldens_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/presenter-goldens")
}

fn bless() -> bool {
    std::env::var_os("PRESENTER_GOLDENS_BLESS").is_some()
}

/// Production-shaped context for a notebook (see `stored::tests::ctx_for`).
fn ctx_for(notebook: &str, dir: &Path, scope: Option<ConfigValue>) -> (ExecutionContext, String) {
    let nb_path = dir.join("notebook.ipynb");
    let converted = convert_notebook(&nb_path, notebook).expect("notebook must convert");
    let mut sc = SourceContext::new();
    sc.add_file(
        "<notebook.ipynb (converted by ipynb)>".to_string(),
        Some(converted.markdown.clone()),
    );
    // Test harness: id, path and content all come from the one notebook, exactly as
    // `ParseDocumentStage` registers them.
    // lint:allow(add-file-with-id) single-notebook registration mirroring production
    sc.add_file_with_id(
        ORIGINAL_FILE_ID,
        nb_path.display().to_string(),
        Some(notebook.to_string()),
    );
    for cell_file in &converted.files {
        sc.add_file(cell_file.label.clone(), Some(cell_file.text.clone()));
    }
    let ctx = ExecutionContext::new(dir.to_path_buf(), dir.to_path_buf(), nb_path, "html")
        .with_source_info(converted.source_info, Arc::new(sc))
        .with_execute_scope(scope);
    (ctx, converted.markdown)
}

/// `scope.json` (`{"echo": false}`) as a document `execute:` scope.
fn scope_from_json(text: &str) -> ConfigValue {
    let map: BTreeMap<String, bool> = serde_json::from_str(text).expect("scope.json");
    let entries = map
        .into_iter()
        .map(|(key, value)| ConfigMapEntry {
            key,
            key_source: SourceInfo::for_test(),
            value: ConfigValue::new_bool(value, SourceInfo::for_test()),
        })
        .collect();
    ConfigValue::new_map(entries, SourceInfo::for_test())
}

/// Every file under `dir` except `skip`, keyed by forward-slash relative path.
fn collect_files(dir: &Path, skip: &[&str]) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, skip: &[&str], out: &mut BTreeMap<String, Vec<u8>>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(base, &path, skip, out);
            } else {
                let rel = path
                    .strip_prefix(base)
                    .unwrap()
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                if !skip.contains(&rel.as_str()) {
                    out.insert(rel, std::fs::read(&path).unwrap());
                }
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, skip, &mut out);
    out
}

/// Compare `actual` against the golden file `path`, or write it when blessing.
/// Returns a description of the mismatch.
fn check_text(path: &Path, actual: &str, problems: &mut Vec<String>) {
    if bless() {
        std::fs::write(path, actual).unwrap();
        return;
    }
    match std::fs::read_to_string(path) {
        Ok(expected) if expected == actual => {}
        Ok(expected) => problems.push(format!(
            "{}: differs\n--- expected\n{expected}\n--- actual\n{actual}",
            path.display()
        )),
        Err(e) => problems.push(format!("{}: unreadable ({e})", path.display())),
    }
}

fn check_files(
    expected_dir: &Path,
    actual: &BTreeMap<String, Vec<u8>>,
    problems: &mut Vec<String>,
) {
    if bless() {
        let _ = std::fs::remove_dir_all(expected_dir);
        for (rel, bytes) in actual {
            let target = expected_dir.join(rel);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(target, bytes).unwrap();
        }
        return;
    }
    let expected = collect_files(expected_dir, &[]);
    if &expected != actual {
        problems.push(format!(
            "{}: written files differ\n  expected: {:?}\n  actual:   {:?}",
            expected_dir.display(),
            expected.keys().collect::<Vec<_>>(),
            actual.keys().collect::<Vec<_>>()
        ));
    }
}

fn run_notebook_case(case_dir: &Path, problems: &mut Vec<String>) {
    let notebook = std::fs::read_to_string(case_dir.join("notebook.ipynb")).unwrap();
    let scope = std::fs::read_to_string(case_dir.join("scope.json"))
        .ok()
        .map(|t| scope_from_json(&t));

    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let (ctx, markdown) = ctx_for(&notebook, dir, scope);

    let (md, supporting) = match IpynbReplayEngine::new().execute(&markdown, &ctx) {
        Ok(result) => {
            let supporting = result
                .supporting_files
                .iter()
                .map(|p| {
                    p.strip_prefix(dir)
                        .unwrap_or(p)
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("/")
                })
                .collect::<Vec<_>>()
                .join("\n");
            (result.markdown, supporting)
        }
        // Normalize the temp dir out of error text.
        Err(e) => (
            format!("ERROR: {e}").replace(&dir.display().to_string(), "<dir>"),
            String::new(),
        ),
    };

    check_text(&case_dir.join("expected.md"), &md, problems);
    check_text(
        &case_dir.join("expected-supporting-files.txt"),
        &supporting,
        problems,
    );
    let written = collect_files(dir, &["notebook.ipynb"]);
    check_files(&case_dir.join("expected-files"), &written, problems);
}

fn stream(name: &str, text: &str) -> CellOutput {
    CellOutput::Stream {
        name: name.to_string(),
        text: text.to_string(),
    }
}

/// Results `stored.rs` cannot produce (it always sets `ExecuteStatus::Ok`).
fn direct_cases() -> Vec<(&'static str, CellResult, CellVisibility)> {
    let visible = || CellVisibility::resolve(None);
    let with = |json: &str| CellVisibility::resolve(Some(&scope_from_json(json)));
    let error_status = || ExecuteStatus::Error {
        ename: "ValueError".to_string(),
        evalue: "bad value".to_string(),
        traceback: vec![
            "\u{1b}[0;31mTraceback\u{1b}[0m".to_string(),
            "  frame two".to_string(),
        ],
    };
    let result = |status, outputs| CellResult {
        status,
        outputs,
        execution_count: Some(1),
    };
    vec![
        (
            "status-error-no-outputs",
            result(error_status(), vec![]),
            visible(),
        ),
        (
            "status-error-with-outputs-not-duplicated",
            result(error_status(), vec![stream("stdout", "partial\n")]),
            visible(),
        ),
        (
            "status-error-no-outputs-output-hidden",
            result(error_status(), vec![]),
            with(r#"{"output": false}"#),
        ),
        (
            "status-aborted-no-outputs",
            result(ExecuteStatus::Aborted, vec![]),
            visible(),
        ),
        (
            "status-ok-no-outputs-echo-false",
            result(ExecuteStatus::Ok, vec![]),
            with(r#"{"echo": false}"#),
        ),
        (
            "all-hidden-emits-nothing",
            result(ExecuteStatus::Ok, vec![stream("stdout", "x\n")]),
            with(r#"{"include": false}"#),
        ),
    ]
}

fn run_direct_cases(problems: &mut Vec<String>) {
    for (name, result, visibility) in direct_cases() {
        let tmp = tempfile::tempdir().unwrap();
        let mut fig = FsFigureSink::new(&tmp.path().join("doc.qmd"));
        fig.begin_cell();
        let md = render_cell("python", "code()\n", &result, &mut fig, visibility);
        let case_dir = goldens_root().join("direct").join(name);
        if bless() {
            std::fs::create_dir_all(&case_dir).unwrap();
        }
        check_text(&case_dir.join("expected.md"), &md, problems);
    }
}

#[test]
fn presenter_goldens() {
    let mut problems = Vec::new();

    let notebooks = goldens_root().join("notebooks");
    let mut cases: Vec<PathBuf> = std::fs::read_dir(&notebooks)
        .expect("notebooks dir")
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    cases.sort();
    assert!(cases.len() >= 50, "corpus went missing: {}", cases.len());
    for case_dir in &cases {
        let before = problems.len();
        run_notebook_case(case_dir, &mut problems);
        for p in &mut problems[before..] {
            *p = format!("[{}] {p}", case_dir.file_name().unwrap().to_string_lossy());
        }
    }

    run_direct_cases(&mut problems);

    assert!(
        problems.is_empty(),
        "{} golden mismatch(es):\n{}",
        problems.len(),
        problems.join("\n\n")
    );
}
