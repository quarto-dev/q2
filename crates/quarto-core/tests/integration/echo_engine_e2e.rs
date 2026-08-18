/*
 * tests/integration/echo_engine_e2e.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Task 14 — the plan-1c END-TO-END gate. Drives the committed echo-engine /
 * echo-legacy fixtures (Task 13) through the REAL render path — a TS engine
 * spawning the Deno engine-host subprocess — and asserts the whole chain works:
 * discovery → registry → `SourceConversionStage` (for `.echo`) → `resolve_engines`
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

use async_trait::async_trait;

use quarto_core::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::ProjectKind;
use quarto_core::project::index::ProjectIndex;
use quarto_core::project::orchestrator::{ProjectPipeline, ProjectType, project_type_for};
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
        let dest = tmp.path().join("_extensions").join(name);
        copy_dir(&fixture_ext_dir(name), &dest);
        crate::engine_fixture_build::ensure_bundle(&dest, name);
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
// Renders a whole-file `.echo`. Exercises `SourceConversionStage` (claims_file
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
    crate::engine_fixture_build::build_bundle(&ext_dir);
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

/// Extract and parse the `CONTEXT_JSON_START{…}CONTEXT_JSON_END` marker the echo
/// engine emits in `execute()` (a JSON snapshot of the `EngineProjectContext`
/// captured at `launch()`). Panics with a body excerpt if the marker is absent.
fn extract_context_json(html: &str) -> serde_json::Value {
    const START: &str = "CONTEXT_JSON_START";
    const END: &str = "CONTEXT_JSON_END";
    let start = html
        .find(START)
        .unwrap_or_else(|| panic!("CONTEXT_JSON marker absent; got:\n{}", body_excerpt(html)));
    let after = &html[start + START.len()..];
    let end = after.find(END).unwrap_or_else(|| {
        panic!(
            "CONTEXT_JSON_END marker absent; got:\n{}",
            body_excerpt(html)
        )
    });
    // The marker rides inside a `<pre><code>` block, so Pandoc HTML-escapes the
    // JSON's structural characters. Unescape the entities that can appear.
    let json = after[..end]
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&");
    serde_json::from_str(&json)
        .unwrap_or_else(|e| panic!("failed to parse echoed context JSON ({e}):\n{json}"))
}

// ── P1.1 T1: per-render EngineProjectContext (project leg) ────────────────────
//
// A temp project with a `_quarto.yml` (`project.output-dir: out` + top-level
// `engines: [echo]`) and a `page.qmd` with an `{echo}` cell. `render_to_file`
// goes through `ProjectContext::discover` (the only live path) → the threaded
// context reaches `TsEngine::set_project` at registry build → launch → execute
// echoes the context back.
//
// Binds the project_dir / output_dir / config field-source lines of the P1.1
// threading hunk. RED (revert the `set_project(engine_project_context.clone())`
// call at the TsEngine::new site): dir "" / outputDir "" / config null → these
// discriminators go RED. Exercised-guard: the CONTEXT_JSON marker is present.
#[test]
fn p1_1_project_context_threaded_project_leg() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — p1_1_project_context_threaded_project_leg");
        return;
    }
    let tmp = setup_project(&["echo-engine"]);
    // Project config: raw relative output-dir under `project`, registered engine
    // name in the top-level `engines` list (Plan 4b-safe).
    write_file(
        &tmp.path().join("_quarto.yml"),
        "project:\n  output-dir: out\nengines:\n  - echo\n",
    );
    let input = tmp.path().join("page.qmd");
    write_file(
        &input,
        "---\ntitle: Echo Ctx\n---\n\n```{echo}\nHello!\n```\n",
    );

    let html = render_html(&input);
    let ctx = extract_context_json(&html);

    // project_dir threaded (default would be "").
    let dir = ctx["dir"].as_str().expect("dir must be a string");
    assert!(
        !dir.is_empty(),
        "project_dir must be threaded (non-empty); ctx: {ctx}"
    );

    // Resolved output_dir: absolute, ends with the project output-dir name, and
    // is NOT the raw relative value (raw-vs-resolved discriminator).
    let resolved = ctx["outputDir"]
        .as_str()
        .expect("outputDir must be a string");
    assert!(
        resolved.ends_with("out") && resolved != "out" && resolved.len() > "out".len(),
        "outputDir must be the RESOLVED absolute dir ending in 'out', not the raw \
         relative value; got {resolved:?}"
    );

    // config.engines carries the registered engine name (the only ConfigValue
    // lowering).
    assert_eq!(
        ctx["config"]["engines"],
        serde_json::json!(["echo"]),
        "config.engines must be lowered from metadata; ctx: {ctx}"
    );

    // config.project.outputDir carries the RAW relative value (the host bridges
    // the flat wire `output-dir` into this rich nested key).
    assert_eq!(
        ctx["config"]["project"]["outputDir"],
        serde_json::json!("out"),
        "config.project.outputDir must carry the raw relative output-dir; ctx: {ctx}"
    );
}

// ── P1.1 T2: per-render EngineProjectContext (single-file `.echo` leg) ─────────
//
// A temp dir with NO `_quarto.yml` and a whole-file `.echo`. `render_to_file` →
// `ProjectContext::discover`'s single-file branch (is_single_file computed at
// :744, project_dir None, config None). markdownForFile converts the file to an
// `{echo}` cell; execute (reading the context captured before the FIRST launch)
// echoes it.
//
// Binds the is_single_file field-source line. RED (revert the is_single_file
// source): stays false → the `isSingleFile: true` discriminator goes RED.
// `config: null` is a shape assertion (guards the builder's absent-config
// branch). Exercised-guard: the CONTEXT_JSON marker is present.
#[test]
fn p1_1_project_context_threaded_single_file_leg() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — p1_1_project_context_threaded_single_file_leg");
        return;
    }
    let tmp = setup_project(&["echo-engine"]);
    let input = tmp.path().join("file.echo");
    write_file(&input, "Single-file echo body.\n");

    let html = render_html(&input);
    let ctx = extract_context_json(&html);

    // is_single_file threaded (default false).
    assert_eq!(
        ctx["isSingleFile"],
        serde_json::json!(true),
        "is_single_file must be true on the single-file leg; ctx: {ctx}"
    );

    // Shape: single-file renders carry no project config.
    assert_eq!(
        ctx["config"],
        serde_json::Value::Null,
        "single-file config must be null (no project config); ctx: {ctx}"
    );
}

/// Extract and parse the `FORMAT_JSON_START{…}FORMAT_JSON_END` marker the echo
/// engine emits in `execute()` (a JSON snapshot of `{ execute, customKey }`
/// derived from the per-execute `Format` the host builds via
/// `metadataAsFormat`). Panics with a body excerpt if the marker is absent.
fn extract_format_json(html: &str) -> serde_json::Value {
    const START: &str = "FORMAT_JSON_START";
    const END: &str = "FORMAT_JSON_END";
    let start = html
        .find(START)
        .unwrap_or_else(|| panic!("FORMAT_JSON marker absent; got:\n{}", body_excerpt(html)));
    let after = &html[start + START.len()..];
    let end = after.find(END).unwrap_or_else(|| {
        panic!(
            "FORMAT_JSON_END marker absent; got:\n{}",
            body_excerpt(html)
        )
    });
    // The marker rides inside a `<pre><code>` block, so Pandoc HTML-escapes the
    // JSON's structural characters. Unescape the entities that can appear.
    let json = after[..end]
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&");
    serde_json::from_str(&json)
        .unwrap_or_else(|e| panic!("failed to parse echoed format JSON ({e}):\n{json}"))
}

// ── P1.1b T14: merged document metadata reaches TsFormatInfo.metadata ─────────
//
// A single-file `.qmd` (no `_quarto.yml`) whose frontmatter carries
// `execute: {daemon: false}` (a non-default binned value) plus a custom
// top-level key `echo-custom-key`. `EngineExecutionStage` resolves the merged
// document metadata (`doc_ast.ast.meta`) and — per the P1.1b threading —
// lowers it into `ExecutionContext.metadata`, which `build_execute_options`
// now carries into `TsFormatInfo.metadata` instead of hardcoding
// `HashMap::new()`. The Deno host's `metadataAsFormat` (already complete,
// untouched here) partitions that flat map into the six-bin `Format`; the
// echo fixture's `execute()` echoes `opts.format.execute` +
// `opts.format.metadata["echo-custom-key"]` back via the FORMAT_JSON marker.
//
// Binds the metadata-threading half of `build_execute_options`
// (`ts_engine.rs:~367-414`, the `metadata: HashMap::new()` line).
//
// RED (named revert: restore `metadata: HashMap::new()` in
// `build_execute_options`): `format.execute` arrives empty (no `daemon` key)
// and `format.metadata["echo-custom-key"]` is absent (`customKey: undefined`
// → JSON `null`) — both discriminators go RED. Exercised-guard: the
// FORMAT_JSON marker itself is present (proves the engine ran and the
// marker channel works, independent of the threading).
#[test]
fn p1_1b_document_metadata_threaded_into_format() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — p1_1b_document_metadata_threaded_into_format");
        return;
    }
    let tmp = setup_project(&["echo-engine"]);
    let input = tmp.path().join("page.qmd");
    write_file(
        &input,
        "---\n\
         title: Echo Format\n\
         execute:\n\
         \x20 daemon: false\n\
         echo-custom-key: hello-from-frontmatter\n\
         ---\n\n\
         ```{echo}\nHello!\n```\n",
    );

    let html = render_html(&input);
    let format = extract_format_json(&html);

    // Exercised-guard: the marker fired at all.
    assert!(
        html.contains("FORMAT_JSON_START"),
        "exercised-guard: the FORMAT_JSON marker must be present; got:\n{}",
        body_excerpt(&html)
    );

    // Discriminator 1: `execute: {daemon: false}` — a non-default binned
    // value — must round-trip through the wire and the host's six-bin
    // partition into `format.execute.daemon`.
    assert_eq!(
        format["execute"]["daemon"],
        serde_json::json!(false),
        "format.execute.daemon must round-trip from frontmatter execute.daemon; format: {format}"
    );

    // Discriminator 2: a custom top-level frontmatter key (not in any of the
    // four Q1 key-lists) must fall through to `format.metadata`.
    assert_eq!(
        format["customKey"],
        serde_json::json!("hello-from-frontmatter"),
        "format.metadata['echo-custom-key'] must round-trip from frontmatter; format: {format}"
    );
}

/// First ~600 chars of the `<body>` for assertion failure messages.
fn body_excerpt(html: &str) -> String {
    let start = html.find("<body").unwrap_or(0);
    html[start..].chars().take(600).collect()
}

// ── J9: zero-load resolution ordering (Phase 4I) ────────────────────────────
//
// Pass 1 advances every project file to the `DocumentProfile` checkpoint
// without running engines; Pass-2 resolution (`resolve_engines`) then decides
// which engine owns each document WITHOUT spawning a subprocess — echo's
// claims are static (`Primary(echo)` answered from the `claims` map, no
// dynamic load), so the discriminating property is "no spawn during
// resolution": the `engine_host` spawn event (J8) must order strictly AFTER
// the `engine_resolution` "resolution complete" event (added at the end of
// `resolve_engines`, `resolution.rs:~334`), i.e. the subprocess spawns lazily
// at the first execute, never during resolution.
//
// Seam check (2026-07-02): "spawn happens after Pass 1" alone is vacuous — a
// legacy DYNAMIC-claims engine also spawns after Pass 1 (during resolution,
// not execute). The discriminating assertion is ordering relative to
// resolution-complete, and — critically — that the resolution-complete event
// actually fired: a naive "assert spawn index > 0" would vacuously pass if
// the resolution-complete event were missing entirely (no event to compare
// against). So this test asserts BOTH events are present (exactly one
// resolution-complete, exactly one spawn) before comparing their order.
//
// Named revert (Test Seam Spec J9): remove the static early-answer branch in
// `claims_language` (`ts_engine.rs:~592` — the `if let Some(map) = &self.claims`
// branch), falling through unconditionally to the dynamic wire-call path
// (`ClaimsLanguage` request over the wire, which itself requires
// `ensure_loaded`/`ensure_started` — i.e. a spawn — before it can answer).
// With the static branch removed, the very first `claims_language` call
// during resolution spawns the host, so the spawn event fires DURING
// resolution, before the resolution-complete event → ordering assertion RED.
#[test]
fn j9_resolution_before_spawn_zero_load() {
    use tracing_subscriber::layer::SubscriberExt;
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — j9_resolution_before_spawn_zero_load");
        return;
    }
    // SAFETY/scope: process-local under nextest (one process per test) — same
    // QUARTO_JOBS=1 lesson as J6 (`julia_engine_e2e.rs`): keeps the whole
    // render on the test thread the tracing capture is scoped to, so a
    // rayon Pass-2 worker thread (which never sees a thread-local
    // subscriber override) can't silently swallow the events.
    unsafe {
        std::env::set_var("QUARTO_JOBS", "1");
    }
    let tmp = setup_project(&["echo-engine"]);
    let input = tmp.path().join("zero_load.qmd");
    write_file(
        &input,
        "---\ntitle: Echo Zero Load\n---\n\nIntro.\n\n```{echo}\nHello from echo!\n```\n",
    );

    let capture = OrderedCapture::default();
    let subscriber = tracing_subscriber::registry().with(capture.clone());
    let html = tracing::subscriber::with_default(subscriber, || render_html(&input));

    // Exercised-guard: the render actually executed the echo cell (so the
    // spawn/resolution events below reflect a real render, not a no-op).
    assert!(
        html.contains("ECHO_EXECUTED"),
        "exercised-guard: rendered HTML must contain ECHO_EXECUTED; got:\n{}",
        body_excerpt(&html)
    );

    let resolution_indices = capture.indices_of("engine_resolution");
    let spawn_indices = capture.spawn_indices();

    // A missing resolution-complete event must FAIL the test outright, not
    // vacuously pass the ordering check below.
    assert_eq!(
        resolution_indices.len(),
        1,
        "expected exactly one engine_resolution (resolution-complete) event; saw {} \
         (all captured targets: {:?})",
        resolution_indices.len(),
        capture.all_targets()
    );
    assert_eq!(
        spawn_indices.len(),
        1,
        "expected exactly one engine_host spawn event (zero-load resolution — the \
         subprocess spawns lazily at first execute, not during resolution); saw {} \
         (all captured targets: {:?})",
        spawn_indices.len(),
        capture.all_targets()
    );

    let resolution_index = resolution_indices[0];
    let spawn_index = spawn_indices[0];
    assert!(
        spawn_index > resolution_index,
        "engine_host spawn (index {spawn_index}) must order AFTER engine_resolution \
         resolution-complete (index {resolution_index}) — a subprocess spawn during \
         resolution means the zero-load property is broken; all captured targets in \
         order: {:?}",
        capture.all_targets()
    );
}

/// A tracing-capture layer recording each event's `target` IN THE ORDER
/// they fired (the underlying `Vec` push is serialized by the mutex, so
/// insertion order reflects temporal firing order even across threads).
/// Mirrors `TargetCapture` in `julia_engine_e2e.rs` / `ts_process.rs`, plus
/// an `indices_of` accessor J9 needs to compare event ORDER, not just count.
#[derive(Clone, Default)]
struct OrderedCapture {
    /// (target, message) pairs, in firing order.
    events: Arc<std::sync::Mutex<Vec<(String, String)>>>,
}

/// Grabs the `message` field off a tracing event so capture layers can
/// discriminate same-target events by their message text (see the
/// plan1a.6 Phase-3 TCP flip: engine-host spawn now fires TWO
/// `target: "engine_host"` events — "engine-host spawned" and
/// "engine-host connected over loopback TCP" — so counting by target
/// alone double-counts spawns).
#[derive(Default)]
struct MsgVisitor(String);
impl tracing::field::Visit for MsgVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for OrderedCapture {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut visitor = MsgVisitor::default();
        event.record(&mut visitor);
        self.events
            .lock()
            .unwrap()
            .push((event.metadata().target().to_string(), visitor.0));
    }
}

impl OrderedCapture {
    /// Indices (in firing order) of every event whose target equals `target`.
    fn indices_of(&self, target: &str) -> Vec<usize> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .enumerate()
            .filter(|(_, (t, _))| t.as_str() == target)
            .map(|(i, _)| i)
            .collect()
    }

    /// Indices (in firing order) of the `"engine-host spawned"` lifecycle
    /// events specifically — the per-spawn marker — as distinct from other
    /// `target == "engine_host"` events (e.g. the TCP-connect marker).
    fn spawn_indices(&self) -> Vec<usize> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .enumerate()
            .filter(|(_, (t, m))| t.as_str() == "engine_host" && m.contains("engine-host spawned"))
            .map(|(i, _)| i)
            .collect()
    }

    fn all_targets(&self) -> Vec<String> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .map(|(t, _)| t.clone())
            .collect()
    }

    /// Count of `target == "engine_host"` events whose message contains
    /// `needle`. Used by the plan1a.6 Phase-3 seams #6a/#7 tests as the
    /// exercised-guard: proof that a stdout-garbage/console.log marker the
    /// fixture wrote was actually observed on the `stdout_loop` drain (a
    /// captured `engine_host`-target tracing event), not just that the
    /// render happened to succeed.
    fn count_containing(&self, needle: &str) -> usize {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|(t, m)| t.as_str() == "engine_host" && m.contains(needle))
            .count()
    }
}

// ── P3-6a (plan1a.6 Phase 3): raw stdout garbage is harmless over TCP ────────
//
// Binds the Phase-3 production flip (engine-host protocol moved off the
// child's stdout onto a private loopback-TCP socket): under the flipped
// transport, anything the Deno child writes DIRECTLY to its stdout fd
// (bypassing the protocol framing entirely) can never corrupt a request,
// because stdout is no longer the protocol channel -- it is drained by
// `stdout_loop` (`ts_process.rs`, ~line 1734) and forwarded line-by-line to
// `tracing::info!(target: "engine_host", "{line}")`.
//
// The echo fixture's `execute()` recognizes the sentinel
// `QUARTO_ECHO_STDOUT_GARBAGE` in the cell body and, unlike
// `QUARTO_ECHO_CRASH`, does NOT exit -- it writes a burst of 20 non-JSON
// lines to `Deno.stdout` and then falls through to the normal
// echo-transform, so a correct render still succeeds.
//
// Two assertions:
//   (i)  the render round-trips: `html` contains `ECHO_EXECUTED` -- proves
//        the in-flight execute request was NOT killed by the garbage.
//   (ii) EXERCISED-GUARD (vacuity guard): a captured `engine_host`-target
//        tracing event's message contains `ECHO_STDOUT_GARBAGE_MARKER` --
//        proves the garbage was REALLY emitted and REALLY flowed through
//        `stdout_loop`'s drain, not merely that the fixture silently
//        skipped the write (which would pass (i) vacuously under BOTH the
//        correct-TCP state AND a reverted-stdio state where the branch
//        never fires).
//
// Why 20 lines, not one: the demux reader tolerates up to
// `MAX_CONSECUTIVE_MALFORMED_LINES = 5` consecutive malformed lines
// (log-and-skip) before the 6th escalates -- setting `shutting_down`,
// broadcasting an error to every pending request, and killing the
// subprocess (`ts_process.rs` `reader_loop`, the `RecvError::Malformed` arm,
// ~lines 1565-1600). A single stray stdout line would be silently tolerated
// even on the REVERTED (stdio-transport) path, making the test vacuous
// against that revert. 20 lines is comfortably above the threshold, so the
// reverted path escalates -> kills the subprocess -> the in-flight execute
// request receives the broadcast error -> `render_html`'s internal `.expect`
// panics -> RED.
//
// Named revert: D-MAIN (`main.ts` wiring `runHost` back to
// `Deno.stdin`/`Deno.stdout` instead of the loopback-TCP socket) -- under
// that revert stdout IS the protocol channel again, so this fixture's raw
// writes interleave with real protocol frames and trigger the
// malformed-line escalation described above.
//
// Capture note: `stdout_loop` runs on a background thread spawned inside
// `spawn_into_tcp`, so a thread-local `tracing::subscriber::with_default`
// (as J9 uses) would NOT see its events. This test installs a **process-
// global** subscriber via `tracing::dispatcher::set_global_default`, the
// same pattern as `behave_engine_e2e.rs`'s
// `f3_timeout_poisons_then_transparently_relaunches` -- safe because
// nextest runs each `#[test]` in its own process.
#[test]
fn p3_6a_stdout_garbage_harmless() {
    use tracing_subscriber::layer::SubscriberExt;

    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — p3_6a_stdout_garbage_harmless");
        return;
    }
    let tmp = setup_project(&["echo-engine"]);
    let input = tmp.path().join("stdout_garbage.qmd");
    write_file(
        &input,
        "---\ntitle: Echo Stdout Garbage\n---\n\n```{echo}\nQUARTO_ECHO_STDOUT_GARBAGE\n```\n",
    );

    let capture = OrderedCapture::default();
    let subscriber = tracing_subscriber::registry().with(capture.clone());
    // GLOBAL default (not thread-scoped `with_default`): see the doc
    // comment above -- `stdout_loop` runs on a background thread that does
    // not inherit a `with_default` thread-local scope.
    tracing::dispatcher::set_global_default(tracing::Dispatch::new(subscriber))
        .expect("install global tracing dispatch for engine_host capture");

    let html = render_html(&input);

    // Assertion (i): the render round-tripped -- the in-flight execute
    // request was not killed by the stdout garbage.
    assert!(
        html.contains("ECHO_EXECUTED"),
        "expected render to succeed over the TCP transport despite raw \
         stdout garbage; got:\n{}",
        body_excerpt(&html)
    );

    // Give the background stdout-drain thread a moment to forward the
    // burst's lines through `tracing` before checking the capture (same
    // reason behave F3 sleeps before checking its background-thread
    // capture).
    std::thread::sleep(std::time::Duration::from_millis(200));

    // Assertion (ii): EXERCISED-GUARD -- the garbage marker was actually
    // observed on the stdout drain. `>= 1`, not `== 20`: some of the 20
    // lines may still be in flight by the time the sleep above elapses, and
    // this test only needs to prove the branch fired and flowed through
    // `stdout_loop`, not that every line landed.
    assert!(
        capture.count_containing("ECHO_STDOUT_GARBAGE_MARKER") >= 1,
        "exercised-guard: expected at least one engine_host-target tracing \
         event carrying the ECHO_STDOUT_GARBAGE_MARKER (proves the garbage \
         was really emitted and really drained via stdout_loop); captured \
         targets: {:?}",
        capture.all_targets()
    );
}

// ── P3-7 (plan1a.6 Phase 3): console.log is harmless over TCP ───────────────
//
// Same seam family as P3-6a, but through `console.log` instead of a raw
// `Deno.stdout.writeSync` -- `console.log` also writes to stdout (Deno's
// console implementation), so it exercises the same stdout-is-harmless
// property via a different, more idiomatic TS-code path (a fixture calling
// `console.log` for debugging is a much more realistic accident than a raw
// fd write).
//
// The echo fixture's `execute()` recognizes the sentinel
// `QUARTO_ECHO_CONSOLE_LOG` and writes a burst of 20 `console.log` lines
// carrying a distinct marker (`ECHO_CONSOLE_LOG_MARK`), then falls through
// to the normal echo-transform.
//
// Same two assertions as P3-6a:
//   (i)  the render round-trips (`html` contains `ECHO_EXECUTED`).
//   (ii) EXERCISED-GUARD (vacuity guard): a captured `engine_host`-target
//        tracing event's message contains `ECHO_CONSOLE_LOG_MARK`.
//
// Same 20-line rationale (above `MAX_CONSECUTIVE_MALFORMED_LINES = 5`) and
// same named revert (D-MAIN) as P3-6a -- see that test's doc comment for the
// full explanation; not repeated here to avoid duplication drift.
//
// Same `set_global_default` capture note as P3-6a applies here too.
#[test]
fn p3_7_console_log_harmless() {
    use tracing_subscriber::layer::SubscriberExt;

    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — p3_7_console_log_harmless");
        return;
    }
    let tmp = setup_project(&["echo-engine"]);
    let input = tmp.path().join("console_log.qmd");
    write_file(
        &input,
        "---\ntitle: Echo Console Log\n---\n\n```{echo}\nQUARTO_ECHO_CONSOLE_LOG\n```\n",
    );

    let capture = OrderedCapture::default();
    let subscriber = tracing_subscriber::registry().with(capture.clone());
    tracing::dispatcher::set_global_default(tracing::Dispatch::new(subscriber))
        .expect("install global tracing dispatch for engine_host capture");

    let html = render_html(&input);

    // Assertion (i): the render round-tripped.
    assert!(
        html.contains("ECHO_EXECUTED"),
        "expected render to succeed over the TCP transport despite \
         console.log noise; got:\n{}",
        body_excerpt(&html)
    );

    std::thread::sleep(std::time::Duration::from_millis(200));

    // Assertion (ii): EXERCISED-GUARD.
    assert!(
        capture.count_containing("ECHO_CONSOLE_LOG_MARK") >= 1,
        "exercised-guard: expected at least one engine_host-target tracing \
         event carrying the ECHO_CONSOLE_LOG_MARK (proves console.log's \
         stdout output was really drained via stdout_loop); captured \
         targets: {:?}",
        capture.all_targets()
    );
}

// ── T8 (plan 1c.2 P2): discovery-set union + echo conversion, via the walk ────
//
// A temp PROJECT (`_quarto.yml`) with `a.echo` at the project root plus the
// echo-engine extension installed under `_extensions/`. `ProjectContext::discover`
// on the PROJECT DIRECTORY (not a single file, unlike every other test in this
// file) exercises the walk in `discover_project_files`, which must admit
// `a.echo` via the FIXED_RENDERABLE ∪ claimed_file_extensions union computed
// BEFORE the walk (the Corollary-0 split of
// `discover_extensions_and_build_registry`). A full `ProjectPipeline::run()`
// then drives the real render→index flow (`pass2_renderer.rs` /
// `orchestrator.rs`, same production path as `project_pipeline.rs`'s Test 14).
//
// Field asserted: the `ProjectIndex` entry's `outline[0].title`, NOT `title`.
// This repo's `title_block` transform only goes `title:` metadata -> synthesized
// H1 (crates/quarto-core/src/transforms/title_block.rs); there is no reverse
// "H1 promotes to title" pass, so `DocumentProfile::extract` leaves `title`
// `None` when frontmatter has no `title:` key (see
// `profile_extract_handles_missing_title` in document_profile.rs) even though
// the converted `a.echo` body has an H1. `outline` is populated from the AST's
// real headings regardless of `title`, so it is the field that can actually
// distinguish "converted" from "raw fall-through" here.
//
// Two named reverts, BOTH demonstrated live during this task's TDD sequencing
// (not via a synthetic `git revert` — the plan's own step order walks through
// both RED states before the final GREEN):
// (1) discovery-set union reverted (`RenderableExtensions::fixed()` at the
//     `discover` call site instead of the real union): `a.echo` never enters
//     `project.files` -> the walk never sees it -> NO `ProjectIndex` entry for
//     `a.echo` -> RED (proves *discovery*). This is the base-commit state this
//     test was written against, before the Corollary-0 wiring landed.
// (2) echo's `markdownForFile` heading reverted (dist rebuilt from the
//     pre-2b `.ts`): the entry EXISTS (discovery still finds `a.echo`) but its
//     converted body carries no heading of its own, so `outline` is empty ->
//     `profile.outline.first()` is `None` -> RED (proves *conversion*). This
//     is the state after step (1)'s wiring lands but before the echo heading
//     edit, also walked through live during this task.
#[test]
fn t8_echo_file_converted_via_project_walk_when_listed() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — t8_echo_file_converted_via_project_walk_when_listed");
        return;
    }
    let tmp = setup_project(&["echo-engine"]);
    // The `render:` key is required now. Engine-claimed extensions are no
    // longer auto-discovered (this supersedes runbook D1; see
    // `project::discovery::effective_render_patterns`), so without a pattern
    // `a.echo` would not enter `project.files` and this test would be proving
    // nothing about conversion. `**/*.qmd` is kept because a positive pattern
    // REPLACES the default rather than adding to it.
    write_file(
        &tmp.path().join("_quarto.yml"),
        "project:\n  type: default\n  render:\n    - \"**/*.qmd\"\n    - \"**/*.echo\"\n",
    );
    write_file(&tmp.path().join("a.echo"), "Whole-file echo body.\n");

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(tmp.path(), runtime.as_ref())
        .expect("ProjectContext::discover over the project dir");
    assert!(
        !project.is_single_file,
        "a _quarto.yml-rooted directory input must NOT be single-file"
    );
    assert!(
        project
            .files
            .iter()
            .any(|f| f.input.file_name().and_then(|n| n.to_str()) == Some("a.echo")),
        "a.echo must be admitted by the project walk once a `render:` pattern \
         selects it (gate 1: FIXED_RENDERABLE ∪ claimed_file_extensions; \
         gate 2: the pattern); discovered files: {:?}",
        project.files.iter().map(|f| &f.input).collect::<Vec<_>>()
    );

    let options = RenderToFileOptions::default();
    let captured: std::rc::Rc<std::cell::RefCell<Option<ProjectIndex>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let project_type: Box<dyn ProjectType> = Box::new(IndexCapture {
        captured: captured.clone(),
    });

    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        Format::html(),
        "html",
        &options,
        runtime.clone(),
    );
    let summary = pollster::block_on(pipeline.run()).expect("orchestrator run");
    assert_eq!(summary.outputs.len(), 1, "one page (a.echo) should render");

    let index = captured
        .borrow()
        .clone()
        .expect("pre_render must have observed the ProjectIndex");
    let profile = index
        .lookup_by_source(Path::new("a.echo"))
        .unwrap_or_else(|| {
            panic!(
                "ProjectIndex must contain an entry for a.echo (discovery); \
                 profiles: {:?}",
                index.profiles()
            )
        });
    let heading = profile.outline.first().unwrap_or_else(|| {
        panic!(
            "a.echo's outline must have at least one heading (echo's \
             markdownForFile conversion prepends `# Echoed: <basename>`); \
             profile: {:?}",
            profile
        )
    });
    // `TocEntry::title` carries inlines as of profile v11
    // (bd-toc-smart-quotes-6nro57ed), so project it to plain text to compare.
    assert_eq!(
        pampa::writers::plaintext::inlines_to_string(&heading.title).0,
        "Echoed: a.echo",
        "a.echo's first outline heading must be the echo-conversion-added \
         title; profile: {:?}",
        profile
    );
}

// ── T13 (plan 1c.2 P4, OPTIONAL): real-process crash mid-execute ─────────────
//
// Only the demux reader-thread's EOF->broadcast path -- real Deno subprocess,
// real pipe EOF, real `handle_crash` -- is not yet covered by an E2E test (the
// MockTransport unit tests in `ts_process.rs` exercise the broadcast logic
// but never spawn a real child). T13 renders a `.qmd` whose `{echo}` cell
// body carries the sentinel `QUARTO_ECHO_CRASH`; the echo fixture's
// `execute()` recognizes it, writes an identifiable marker to stderr, then
// calls `Deno.exit(1)` BEFORE returning any result -- i.e. mid-execute, while
// the request is in flight on the demux. The engine-host's stdout pipe then
// closes, the reader thread observes EOF, `handle_crash` reaps the child,
// snapshots the stderr ring, and broadcasts `ExecutionError::ProcessCrashed`
// to the in-flight slot.
//
// Shape assertion: `ExecutionError` does NOT survive as a structured type to
// `render_to_file`'s caller -- `EngineExecutionStage::run`
// (`stage/stages/engine_execution.rs:370`) converts it with
// `.map_err(|e| PipelineError::stage_error(self.name(), e.to_string()))`,
// stringifying it into a `DiagnosticMessage` title before the pipeline error
// is folded into `QuartoError::Parse` (`pipeline.rs`'s `run_pipeline`
// `result.map_err` -- the `StageError { diagnostics, .. } if
// !diagnostics.is_empty()` arm). So there is no `downcast`/`matches!` seam
// left by the time the error reaches this test -- the strongest available
// signal that `ProcessCrashed` (and not some other `ExecutionError` variant)
// produced the failure is `ProcessCrashed`'s OWN `#[error(...)]` Display
// text, which is unique among the enum's variants:
// "engine host subprocess crashed while serving '{engine}' (exit code:
// {code:?})\n{stderr}" (`engine/error.rs:109-111`). This test asserts that
// substring is present (shape) AND that the captured stderr contains the
// crash marker (content) -- together they rule out both "some other
// execution error happened to mention 'crashed'" and "ProcessCrashed fired
// but the stderr ring lost the marker."
//
// Named revert (reasoned, not live-forced -- see report): reverting the
// reader thread's EOF->`handle_crash`->broadcast wiring in `ts_process.rs`
// (i.e. `reader_loop`'s unexpected-EOF arm no longer calling `handle_crash`,
// or `handle_crash` no longer draining+broadcasting to `pending`) leaves
// every `PendingSlot` in the `pending` map un-notified: no `ProcessCrashed`
// is ever delivered to the in-flight execute request. The request does NOT,
// however, hang forever -- `TsEngineHost::request`'s tick loop independently
// enforces the `execute_timeout` window (`ts_process.rs`, the
// `start.elapsed() >= window` check), and this fixture sets no
// `execute: timeout:` key, so `resolve_execute_timeout` supplies the 300s
// default. So with the wiring reverted the request returns
// `Err(ExecutionError::Timeout { .. })` after that bounded wait, via a code
// path completely independent of the EOF->broadcast wiring under test. Either
// way the test goes RED: `Timeout`'s Display text ("timed out during
// execute") matches NEITHER the ProcessCrashed-identity assertion nor the
// `ECHO_CRASH_MARKER` stderr assertion below -- so the crash-specific seam is
// still discriminating (a non-crash outcome fails both assertions), just via
// a bounded-timeout error rather than an infinite hang. (Note: `.config/
// nextest.toml` sets no `slow-timeout.terminate-after`, so nextest would NOT
// kill a genuine hang here -- which is exactly why the real revert-RED path
// is the bounded `execute_timeout`, not a nextest-killed hang.) Reasoned from
// the code rather than forced live because deliberately reverting production
// crash-recovery code and waiting out a 300s timeout is a slow, disruptive
// action reserved for the controller if independent confirmation is wanted.
#[test]
fn t13_crash_mid_execute_yields_process_crashed_with_stderr() {
    if !deno_available() {
        eprintln!(
            "SKIP: deno not on PATH -- t13_crash_mid_execute_yields_process_crashed_with_stderr"
        );
        return;
    }
    let tmp = setup_project(&["echo-engine"]);
    let input = tmp.path().join("crash.qmd");
    write_file(
        &input,
        "---\ntitle: Echo Crash\n---\n\n```{echo}\nQUARTO_ECHO_CRASH\n```\n",
    );

    let options = RenderToFileOptions::default();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_to_file(&input, "html", &options, runtime);

    assert!(
        result.is_err(),
        "a mid-execute crash (stderr marker + Deno.exit(1) before returning) \
         must fail the render, not succeed or panic"
    );
    let msg = format!("{:#}", result.unwrap_err());

    // Shape: ProcessCrashed's own Display text is the only source of this
    // exact phrase among ExecutionError's variants (see doc comment above).
    assert!(
        msg.contains("engine host subprocess crashed while serving"),
        "error message must carry ProcessCrashed's uniquely-identifying \
         Display text (proves the reader-thread EOF->broadcast path fired, \
         not some other ExecutionError variant); got: {msg}"
    );
    // The crashed engine is named in the same Display text.
    assert!(
        msg.contains("'echo'"),
        "ProcessCrashed's Display text must name the crashed engine \
         ('echo'); got: {msg}"
    );
    // Content: the captured stderr ring must carry the marker the fixture
    // wrote immediately before Deno.exit(1) -- proves handle_crash's stderr
    // snapshot actually captured the child's last words, not an empty tail.
    assert!(
        msg.contains("ECHO_CRASH_MARKER"),
        "error message must carry the captured stderr marker the crashing \
         engine wrote before Deno.exit(1); got: {msg}"
    );
}

/// A `ProjectType` that captures the `ProjectIndex` handed to `pre_render`,
/// for tests that need to inspect the index the driver built (T8). Mirrors
/// `IndexObserver` in `project_pipeline.rs` (Test 14), but clones the whole
/// index (not just source paths) so the caller can `lookup_by_source`.
struct IndexCapture {
    captured: std::rc::Rc<std::cell::RefCell<Option<ProjectIndex>>>,
}

#[async_trait(?Send)]
impl ProjectType for IndexCapture {
    fn kind(&self) -> ProjectKind {
        ProjectKind::Default
    }

    async fn pre_render(
        &self,
        _project: &mut ProjectContext,
        index: &ProjectIndex,
    ) -> quarto_core::Result<()> {
        *self.captured.borrow_mut() = Some(index.clone());
        Ok(())
    }
}
