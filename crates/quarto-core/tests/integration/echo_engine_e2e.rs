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
    let spawn_indices = capture.indices_of("engine_host");

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
    targets: Arc<std::sync::Mutex<Vec<String>>>,
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for OrderedCapture {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        self.targets
            .lock()
            .unwrap()
            .push(event.metadata().target().to_string());
    }
}

impl OrderedCapture {
    /// Indices (in firing order) of every event whose target equals `target`.
    fn indices_of(&self, target: &str) -> Vec<usize> {
        self.targets
            .lock()
            .unwrap()
            .iter()
            .enumerate()
            .filter(|(_, t)| t.as_str() == target)
            .map(|(i, _)| i)
            .collect()
    }

    fn all_targets(&self) -> Vec<String> {
        self.targets.lock().unwrap().clone()
    }
}
