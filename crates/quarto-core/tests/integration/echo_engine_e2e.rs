/*
 * tests/integration/echo_engine_e2e.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Task 14 — the plan-1c END-TO-END gate. Drives the committed echo-engine /
 * echo-legacy fixtures (Task 13) through the REAL render path — a TS engine
 * spawning the Deno engine-host subprocess — and asserts the whole chain works:
 * discovery → registry → `EngineClaimsFileStage` (for `.echo`) → `resolve_engines`
 * → LoadEngine / LaunchEngine / execute → result, plus orchestrator-driven
 * teardown (`registry.shutdown_all()` reaping the Deno subprocess).
 *
 * These tests are Deno-gated: they early-return (skip) when `deno` is not on
 * PATH. On a machine with Deno present they RUN — a skip in CI-with-Deno is a
 * signal something is wrong.
 *
 * The `TsEngineHost` spawns the EMBEDDED (`include_str!`) engine-host-deno
 * bundle, and each engine extension imports its committed `dist` bundle. Both are
 * built from current TS source (harness rebuilt in Task 14; fixture bundles
 * committed in Task 13). See the brief and plan lines 1413-1452.
 */

// Native-only: TsEngine / TsEngineHost are behind cfg(not(target_arch = "wasm32")).
#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// Return `true` when `deno` is on PATH (the E2E's subprocess runtime).
fn deno_available() -> bool {
    Command::new("deno")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Absolute path to a committed fixture extension directory
/// (`crates/quarto-core/tests/fixtures/extensions/<name>`).
fn fixture_ext_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/extensions")
        .join(name)
}

/// Recursively copy `src` into `dst` (dst is created).
fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to);
        } else {
            std::fs::copy(&from, &to).unwrap();
        }
    }
}

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// Build a temp project with the named committed extension(s) installed under
/// `_extensions/`, returning the `TempDir` (keep it alive for the test's
/// duration). No `_quarto.yml` is written, so a single input file renders via
/// the single-doc path while the extension is still discovered by walking up
/// to `_extensions/`.
fn setup_project(ext_names: &[&str]) -> TempDir {
    let tmp = TempDir::new().unwrap();
    for name in ext_names {
        copy_dir(
            &fixture_ext_dir(name),
            &tmp.path().join("_extensions").join(name),
        );
    }
    tmp
}

/// Render `input` through the real per-document render path
/// (`render_to_file` → `render_document_to_file`, the same entry `quarto render`
/// uses) and return the rendered HTML.
fn render_html(input: &Path) -> String {
    let options = RenderToFileOptions::default();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_to_file(input, "html", &options, runtime).expect("render_to_file");
    std::fs::read_to_string(&result.output_path).expect("read rendered HTML")
}

// ── P3-1a: language claim (static Primary, zero-load resolution + execute) ────
//
// Renders a `.qmd` with `{echo}` cells. Exercises registry → `resolve_engines`
// (echo's static `Primary(echo)` → ownership, no dynamic load) → LaunchEngine →
// execute. RED: stub echo's Primary resolution ⇒ echo not selected ⇒
// ECHO_EXECUTED absent.
#[test]
fn p3_1a_language_claim_executes_echo() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — p3_1a_language_claim_executes_echo");
        return;
    }
    let tmp = setup_project(&["echo-engine"]);
    let input = tmp.path().join("lang.qmd");
    write_file(
        &input,
        "---\ntitle: Echo Lang\n---\n\nIntro.\n\n```{echo}\nHello from echo!\n```\n",
    );

    let html = render_html(&input);
    assert!(
        html.contains("ECHO_EXECUTED"),
        "rendered HTML must contain ECHO_EXECUTED (echo language claim executed); \
         got:\n{}",
        body_excerpt(&html)
    );
}

// ── P3-1b: whole-file `.echo` claim + §8 single-engine pass-through ───────────
//
// Renders a whole-file `.echo`. Exercises `EngineClaimsFileStage` (claims_file
// on `.echo`) → `markdown_for_file` (real subprocess converts the file, wrapping
// it as an `{echo}` block plus an appended non-echo `{python}` cell) →
// ParseDocumentStage synthetic name → `claimed_engine_name` → `resolve_engines`
// single-engine short-circuit → execute.
//
// The render SUCCEEDING with ECHO_EXECUTED proves the file was converted
// (`LoadedSource.conversion` populated — a `.echo` with no conversion fails with
// "Can't determine execution engine"). The appended `{python}` cell appearing as
// an UNEXECUTED code listing (its `print('not run by echo')` source verbatim,
// with the `{python}` cell class intact) proves the resolution was SINGLE-ENGINE
// `[echo]`: no second engine (jupyter) was pulled in to steal the python cell.
//
// RED: restore the seed/steal logic so a 2nd engine claims `{python}` ⇒ the
// python cell is executed (or resolution errors on unavailable jupyter) ⇒ the
// "unexecuted python listing" assertions go RED.
#[test]
fn p3_1b_file_claim_single_engine_passthrough() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — p3_1b_file_claim_single_engine_passthrough");
        return;
    }
    let tmp = setup_project(&["echo-engine"]);
    let input = tmp.path().join("file.echo");
    write_file(
        &input,
        "This is a whole-file .echo fixture claimed by echo.\n",
    );

    let html = render_html(&input);

    // The {echo}-wrapped file body was executed by the echo engine.
    assert!(
        html.contains("ECHO_EXECUTED"),
        "rendered .echo HTML must contain ECHO_EXECUTED (file claimed + converted \
         + executed); got:\n{}",
        body_excerpt(&html)
    );
    // The appended {python} cell is passed through UNEXECUTED: its source appears
    // verbatim as a code listing (single-engine pass-through, §8).
    assert!(
        html.contains("not run by echo"),
        "the appended {{python}} cell's source must appear (unexecuted pass-through); \
         got:\n{}",
        body_excerpt(&html)
    );
    // The {python} cell class survived: it rendered as a code cell, NOT executed
    // by a second engine. If jupyter had been pulled in it would have consumed
    // the fenced cell (and, being unavailable, failed the render).
    assert!(
        html.contains("{python}"),
        "the {{python}} cell must remain an unexecuted code listing (single-engine \
         [echo] — no 2nd engine stole it); got:\n{}",
        body_excerpt(&html)
    );
}

// ── P3-2: dynamic fallback (path-only extension → LoadEngine + claimsLanguage) ─
//
// The echo-legacy extension declares ONLY a path (no name / claims). Resolving
// its `{echolegacy}` language must take the DYNAMIC path: register under the
// extension id, then on first `claims_language` load the module (LoadEngine),
// populate the runtime_name → extension_id alias, and round-trip
// `claimsLanguage("echolegacy")`. The wire frames after LoadEngine address the
// harness by the module's RUNTIME name ("echolegacy"), not the ext-id key.
//
// RED: remove the `claims == None → dynamic load` fallback ⇒ echolegacy
// unresolved ⇒ ECHOLEGACY_EXECUTED absent (jupyter fallback / render error).
#[test]
fn p3_2_dynamic_echolegacy_fallback() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — p3_2_dynamic_echolegacy_fallback");
        return;
    }
    let tmp = setup_project(&["echo-legacy"]);
    let input = tmp.path().join("legacy.qmd");
    write_file(
        &input,
        "---\ntitle: Echo Legacy\n---\n\n```{echolegacy}\nHello from legacy!\n```\n",
    );

    let html = render_html(&input);
    assert!(
        html.contains("ECHOLEGACY_EXECUTED"),
        "rendered HTML must contain ECHOLEGACY_EXECUTED (dynamic LoadEngine + \
         claimsLanguage round-trip executed); got:\n{}",
        body_excerpt(&html)
    );
}

// ── P1-7: declared-name vs runtime-name mismatch → hard error (load-time) ─────
//
// An extension declaring `name: echo-wrongname` whose loaded module reports
// `name: echo` must produce a hard error pointing at the YAML mismatch. Binds
// the `ensure_loaded` name-validation from Task 4 (no prior E2E test).
//
// RED: drop the `result.name != self.name` check in `ensure_loaded` ⇒ the render
// succeeds silently ⇒ this `is_err` + message assertion goes RED.
#[test]
fn p1_7_engine_name_mismatch_hard_errors() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — p1_7_engine_name_mismatch_hard_errors");
        return;
    }
    let tmp = TempDir::new().unwrap();
    // Copy the echo-engine bundle but declare a mismatched name.
    let ext_dir = tmp.path().join("_extensions/echo-wrong");
    copy_dir(&fixture_ext_dir("echo-engine"), &ext_dir);
    write_file(
        &ext_dir.join("_extension.yml"),
        "title: Echo Wrong Name\n\
         author: Test\n\
         version: 0.1.0\n\
         contributes:\n\
         \x20 engines:\n\
         \x20   - path: dist/echo-engine.js\n\
         \x20     name: echo-wrongname\n\
         \x20     claims:\n\
         \x20       echo:\n\
         \x20         kind: primary\n\
         \x20         priority: 1\n\
         \x20     file-extensions:\n\
         \x20       - .echo\n\
         \x20     claims-files:\n\
         \x20       - .echo\n",
    );
    let input = tmp.path().join("lang.qmd");
    write_file(&input, "---\ntitle: T\n---\n\n```{echo}\nhi\n```\n");

    let options = RenderToFileOptions::default();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_to_file(&input, "html", &options, runtime);

    assert!(
        result.is_err(),
        "a declared-name vs runtime-name mismatch must fail the render"
    );
    let msg = format!("{:#}", result.unwrap_err());
    assert!(
        msg.contains("echo-wrongname") && msg.contains("name: echo"),
        "error must name both the declared and the module-reported name; got: {msg}"
    );
}

// ── P3-3: orchestrator-driven teardown reaps the Deno subprocess ─────────────
//
// Drives a render through the orchestrator (`ProjectPipeline::run`), which calls
// `registry.shutdown_all()` on every exit path before `ProjectContext` drops.
// After `run()` returns — with `project` (and thus the registry + TsEngine + its
// host) still in scope, so Drop has NOT fired — the echo engine's backing Deno
// subprocess must be GONE (`is_alive() == false`).
//
// Exercised-guard: the rendered output contains ECHO_EXECUTED, proving the
// subprocess actually spawned and executed (otherwise "gone after teardown"
// would pass vacuously).
//
// RED: remove the `registry.shutdown_all()` call in `ProjectPipeline::run` ⇒ the
// subprocess lingers (Drop hasn't run — project is still alive) ⇒ `is_alive()`
// stays true ⇒ the "gone after teardown" assertion goes RED.
#[test]
fn p3_3_orchestrator_teardown_reaps_subprocess() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — p3_3_orchestrator_teardown_reaps_subprocess");
        return;
    }
    let tmp = setup_project(&["echo-engine"]);
    let input = tmp.path().join("lang.qmd");
    write_file(
        &input,
        "---\ntitle: Echo Teardown\n---\n\n```{echo}\nHello!\n```\n",
    );

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&input, runtime.as_ref()).unwrap();
    assert!(
        project.registry.has_engine("echo"),
        "echo engine must be registered from the installed extension"
    );

    let options = RenderToFileOptions::default();
    let output_path;
    {
        let project_type = project_type_for(&project);
        let mut pipeline = ProjectPipeline::new(
            &mut project,
            project_type,
            Format::html(),
            "html",
            &options,
            runtime.clone(),
        );
        let summary = pollster::block_on(pipeline.run()).expect("orchestrator run");
        assert_eq!(summary.outputs.len(), 1, "one page should render");
        output_path = summary.outputs[0].output_path.clone();
        // `pipeline` (holding `&mut project`) drops here.
    }

    // Exercised-guard: the subprocess really executed.
    let html = std::fs::read_to_string(&output_path).expect("read rendered HTML");
    assert!(
        html.contains("ECHO_EXECUTED"),
        "exercised-guard: the echo subprocess must have executed (ECHO_EXECUTED \
         present) for the teardown assertion to be non-vacuous; got:\n{}",
        body_excerpt(&html)
    );

    // The orchestrator's explicit shutdown_all() must have reaped the child —
    // and `project` is still alive here, so Drop has NOT run: only the explicit
    // call can be responsible.
    let echo = project
        .registry
        .get("echo")
        .expect("echo engine still registered after render");
    assert!(
        !echo.is_alive(),
        "the echo Deno subprocess must be reaped by orchestrator shutdown_all() \
         at end-of-render (before ProjectContext drops)"
    );
}

/// First ~600 chars of the `<body>` for assertion failure messages.
fn body_excerpt(html: &str) -> String {
    let start = html.find("<body").unwrap_or(0);
    html[start..].chars().take(600).collect()
}
