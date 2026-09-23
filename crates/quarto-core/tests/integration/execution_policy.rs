//! `ExecutionPolicy` (bd-sl79jjiq, plan
//! `claude-notes/plans/2026-09-22-q2-preview-static.md` § Lazy code
//! execution): which documents may execute code during a render.
//!
//! Driven through `render_to_file` with a `FixtureEngine` registry, so
//! "execution" is deterministic and needs no Python or R. The fixture
//! replaces its ```` ```{fixture-a} ```` cell with a recorded result; an
//! inert render keeps the cell's source instead.

#![cfg(not(target_arch = "wasm32"))]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use quarto_core::engine::{EngineRegistry, ExecutionPolicy, FixtureEngine};
use quarto_core::{RenderToFileOptions, RenderToFileResult, render_to_file};
use quarto_error_reporting::DiagnosticKind;
use quarto_system_runtime::NativeRuntime;

const ENGINE_DOC: &str = "---\ntitle: Engine doc\nengine: fixture-a\n---\n\nBefore.\n\n```{fixture-a}\nseed-cell-source\n```\n\nAfter.\n";
const PLAIN_DOC: &str = "---\ntitle: Plain doc\n---\n\nNo code here.\n";
const RESULT: &str = "EXECUTED-RESULT-MARKER";

fn registry() -> Arc<EngineRegistry> {
    let mut registry = EngineRegistry::new();
    registry.register(Arc::new(FixtureEngine::with_results(
        "fixture-a",
        vec![RESULT.to_string()],
    )));
    Arc::new(registry)
}

fn write(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, contents).unwrap();
    path
}

fn render(input: &Path, policy: ExecutionPolicy) -> RenderToFileResult {
    let options = RenderToFileOptions {
        engine_registry_override: Some(registry()),
        execution_policy: policy,
        quiet: true,
        ..Default::default()
    };
    render_to_file(input, "html", &options, Arc::new(NativeRuntime::new()))
        .expect("render succeeds")
}

fn html_of(result: &RenderToFileResult) -> String {
    std::fs::read_to_string(&result.output_path).expect("output written")
}

fn problems(result: &RenderToFileResult) -> Vec<String> {
    result
        .render_output
        .diagnostics
        .iter()
        .filter(|d| matches!(d.kind, DiagnosticKind::Error | DiagnosticKind::Warning))
        .map(|d| d.title.clone())
        .collect()
}

#[test]
fn default_policy_executes_every_document() {
    let temp = tempfile::TempDir::new().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    let a = write(&dir, "a.qmd", ENGINE_DOC);
    let result = render(&a, ExecutionPolicy::default());
    let html = html_of(&result);
    assert!(html.contains(RESULT), "cell executed: {html}");
    assert!(
        !html.contains("seed-cell-source"),
        "source consumed: {html}"
    );
    assert!(!result.render_output.execution_skipped);
}

#[test]
fn policy_none_renders_cells_inert_without_any_diagnostic() {
    let temp = tempfile::TempDir::new().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    let a = write(&dir, "a.qmd", ENGINE_DOC);
    let result = render(&a, ExecutionPolicy::None);
    let html = html_of(&result);
    assert!(!html.contains(RESULT), "must not execute: {html}");
    assert!(
        html.contains("seed-cell-source"),
        "cell source kept: {html}"
    );
    assert!(html.contains("After."), "rest of the page intact: {html}");
    assert!(
        result.render_output.execution_skipped,
        "the pipeline reports that execution was skipped"
    );
    assert_eq!(
        problems(&result),
        Vec::<String>::new(),
        "no warning, no error"
    );
}

#[test]
fn policy_none_on_a_document_without_code_is_not_a_skip() {
    let temp = tempfile::TempDir::new().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    let p = write(&dir, "plain.qmd", PLAIN_DOC);
    let result = render(&p, ExecutionPolicy::None);
    assert!(html_of(&result).contains("No code here."));
    assert!(
        !result.render_output.execution_skipped,
        "nothing to execute, so nothing was skipped"
    );
}

#[test]
fn policy_only_executes_the_listed_inputs_and_skips_the_rest() {
    let temp = tempfile::TempDir::new().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    let a = write(&dir, "a.qmd", ENGINE_DOC);
    let b = write(&dir, "b.qmd", ENGINE_DOC);
    let only_a = ExecutionPolicy::Only(BTreeSet::from([a.clone()]));

    let result_a = render(&a, only_a.clone());
    assert!(html_of(&result_a).contains(RESULT), "listed input executes");
    assert!(!result_a.render_output.execution_skipped);

    let result_b = render(&b, only_a);
    let html_b = html_of(&result_b);
    assert!(
        !html_b.contains(RESULT),
        "unlisted input stays inert: {html_b}"
    );
    assert!(html_b.contains("seed-cell-source"), "{html_b}");
    assert!(result_b.render_output.execution_skipped);
    assert_eq!(problems(&result_b), Vec::<String>::new());
}

#[test]
fn allows_is_a_plain_set_test() {
    let a = PathBuf::from("/p/a.qmd");
    let b = PathBuf::from("/p/b.qmd");
    assert!(ExecutionPolicy::All.allows(&a));
    assert!(!ExecutionPolicy::None.allows(&a));
    let only_a = ExecutionPolicy::Only(BTreeSet::from([a.clone()]));
    assert!(only_a.allows(&a));
    assert!(!only_a.allows(&b));
}
