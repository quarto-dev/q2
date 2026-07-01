/*
 * tests/integration/julia_engine_e2e.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Plan 4 (Julia validation) — the julia+deno-gated END-TO-END rows. Drives the
 * committed julia-engine fixture (Phase 4A) through the REAL render path — a TS
 * engine spawning the Deno engine-host subprocess, which in turn spawns a real
 * Julia QuartoNotebookRunner control server — and asserts the whole chain works:
 * discovery → static resolution → LoadEngine / LaunchEngine / execute → jupyter
 * `toMarkdown` → HTML writer.
 *
 * These tests are julia+deno-gated: they early-return (skip, with an
 * `eprintln!("SKIP: …")`) when `deno` or `julia` is not on PATH. On a machine
 * with BOTH present they RUN — a skip there is a signal something is wrong, not a
 * pass (Test Seam Spec, CI gating).
 *
 * The real environmental prerequisite is more than `julia` in PATH: on first run
 * `ensure_environment.jl` instantiates the QuartoNotebookRunner project (network +
 * package downloads). Budget for a slow, network-dependent first render.
 *
 * Harness mirrors `echo_engine_e2e.rs`, with one structural difference: the
 * committed julia fixture root is a *project directory* that contains the engine
 * package under `_extensions/julia-engine/` (co-located with its `.jl` scripts and
 * `Project.toml`, which the bundle resolves via `import.meta.url`). So the TempDir
 * setup copies that inner package into `tmp/_extensions/julia-engine`, preserving
 * the co-location the engine relies on.
 */

// Native-only: TsEngine / TsEngineHost are behind cfg(not(target_arch = "wasm32")).
#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::Format;
use quarto_core::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::render_to_file::{RenderToFileOptions, render_document_to_file, render_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// Return `true` when `deno` is on PATH (the engine-host subprocess runtime).
fn deno_available() -> bool {
    Command::new("deno")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Return `true` when `julia` is on PATH (the QuartoNotebookRunner backend).
fn julia_available() -> bool {
    Command::new("julia")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Absolute path to the committed julia-engine fixture root
/// (`crates/quarto-core/tests/fixtures/extensions/julia-engine`). This root is a
/// project dir; the engine package itself lives under `_extensions/julia-engine`.
fn julia_fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/extensions/julia-engine")
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

/// Build a temp project with the committed julia-engine package installed under
/// `_extensions/julia-engine/` (co-located `.jl` scripts + bundle preserved).
/// Returns the `TempDir` (keep it alive for the test's duration).
fn setup_julia_project() -> TempDir {
    let tmp = TempDir::new().unwrap();
    copy_dir(
        &julia_fixture_root().join("_extensions/julia-engine"),
        &tmp.path().join("_extensions").join("julia-engine"),
    );
    tmp
}

/// P3 (bd-h4rhohhy) — recursively copy the whole ambient
/// `QUARTO_JULIA_PROJECT` directory (the env files `Project.toml` +
/// `Manifest.toml` plus whatever else sits beside them; the actual
/// package code lives in the shared `JULIA_DEPOT_PATH`, which this does
/// NOT touch) into a per-test temp dir, then override
/// `QUARTO_JULIA_PROJECT` to point at the copy for the rest of THIS
/// process.
///
/// This is on top of — not a replacement for — the temp-`HOME` override
/// below, which governs where `julia_transport.txt` /
/// `julia_server_log.txt` live (`quarto.path.runtime("julia")` in
/// julia-engine.ts resolves purely from `HOME`, independent of
/// `QUARTO_JULIA_PROJECT` — verified by reading + a standalone
/// `quarto_util::quarto_runtime_dir()` / `Deno.env.get("HOME")` check
/// under an overridden `HOME`, both honoring the override; see
/// task-p3-report.md). Isolating the PROJECT too closes the narrower
/// remaining exposure: without it, the detached julia server launches
/// with `--project=<shared, real path>`, so a test run could contend on
/// or (if `Pkg` ever touches `Manifest.toml`) mutate the shared
/// project directory the user's real preview sessions also resolve
/// against. A private copy removes that contact point entirely.
///
/// Returns `None` (caller should skip) when `QUARTO_JULIA_PROJECT` is
/// unset or the shared dir has no `Manifest.toml` — the same
/// precondition the harness's ambient-env contract already requires.
fn isolate_julia_project() -> Option<TempDir> {
    let shared = std::env::var("QUARTO_JULIA_PROJECT").ok()?;
    let shared_path = Path::new(&shared);
    if !shared_path.join("Manifest.toml").exists() {
        return None;
    }
    let tmp = TempDir::new().unwrap();
    copy_dir(shared_path, tmp.path());
    // SAFETY/scope: process-local under nextest (one process per test),
    // same contract as the HOME override below.
    unsafe {
        std::env::set_var("QUARTO_JULIA_PROJECT", tmp.path());
    }
    Some(tmp)
}

/// Snapshot of the SHARED (non-isolated) julia transport file's
/// existence + mtime, taken before a live run so the caller can prove
/// "the shared transport was untouched during the run" after — the
/// PC3 acceptance criterion for the isolation fix. Resolves the shared
/// path from `real_home` (the ambient `HOME`, captured BEFORE this
/// process overrides it) so the check targets the user's actual
/// `~/Library/Caches/quarto/julia/julia_transport.txt`, never the
/// isolated one.
struct SharedTransportSentinel {
    path: PathBuf,
    existed_before: bool,
    mtime_before: Option<std::time::SystemTime>,
}

impl SharedTransportSentinel {
    fn capture(real_home: &Path) -> Self {
        let path = real_home
            .join("Library")
            .join("Caches")
            .join("quarto")
            .join("julia")
            .join("julia_transport.txt");
        let meta = std::fs::metadata(&path).ok();
        Self {
            existed_before: meta.is_some(),
            mtime_before: meta.and_then(|m| m.modified().ok()),
            path,
        }
    }

    /// Panics with a diagnostic if the shared transport file's
    /// existence or mtime changed since `capture`.
    fn assert_untouched(&self) {
        let meta = std::fs::metadata(&self.path).ok();
        let existed_after = meta.is_some();
        let mtime_after = meta.and_then(|m| m.modified().ok());
        assert_eq!(
            self.existed_before, existed_after,
            "shared transport file {:?} existence changed during the run \
             (before: {}, after: {}) — isolation leaked onto the shared server",
            self.path, self.existed_before, existed_after
        );
        if self.existed_before {
            assert_eq!(
                self.mtime_before, mtime_after,
                "shared transport file {:?} mtime changed during the run \
                 — isolation leaked onto the shared server",
                self.path
            );
        }
    }
}

/// Render `input` through the real per-document render path (`render_to_file` →
/// `render_document_to_file`, the same entry `quarto render` uses) and return the
/// rendered HTML.
fn render_html(input: &Path) -> String {
    let options = RenderToFileOptions::default();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_to_file(input, "html", &options, runtime).expect("render_to_file");
    std::fs::read_to_string(&result.output_path).expect("read rendered HTML")
}

/// The Phase 4B minimal document: `engine: julia`, `execute: daemon: false`, one
/// `{julia}` cell computing `1 + 1`.
const MINIMAL_DOC: &str =
    "---\nengine: julia\nexecute:\n  daemon: false\n---\n\n```{julia}\n1 + 1\n```\n";

// ── J1: minimal Julia render (Phase 4B) ───────────────────────────────────────
//
// Renders the 4B minimal doc through the full chain with real Deno + real Julia
// (no mocks): discovery → static `Primary(julia)` resolution → LoadEngine /
// LaunchEngine / execute → QuartoNotebookRunner runs `1 + 1` → jupyter
// `toMarkdown` → HTML writer. Asserts the rendered HTML contains the cell result
// `2`.
//
// Named revert (Test Seam Spec J1): delete the `contributes.engines` entry from
// the fixture `_extension.yml` → no engine claims `julia` → the `{julia}` cell is
// never executed (it renders as an unexecuted code listing, no `2` output) →
// RED. (Smoke row: binds the fixture's registration.)
#[test]
fn j1_minimal_julia_render() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — j1_minimal_julia_render");
        return;
    }
    if !julia_available() {
        eprintln!("SKIP: julia not on PATH — j1_minimal_julia_render");
        return;
    }
    let tmp = setup_julia_project();
    let input = tmp.path().join("minimal.qmd");
    write_file(&input, MINIMAL_DOC);

    let html = render_html(&input);

    // The `{julia}` cell executed and produced the result `2` inside a
    // `cell-output` block. A missing engine (named revert) leaves `1 + 1` as an
    // unexecuted code listing with NO `cell-output` block and NO `<code>2</code>`
    // result — so both halves redden, and "render produced some stray 2" can't
    // fake a pass (asserting on the specific executed-output markup, not a bare
    // digit anywhere in the page chrome).
    assert!(
        html.contains("cell-output"),
        "rendered HTML must contain an executed cell-output block (julia engine ran); got:\n{}",
        body_excerpt(&html)
    );
    assert!(
        html.contains("<code>2</code>"),
        "rendered HTML must contain the cell result `2` as executed cell output \
         (`<code>2</code>`); got:\n{}",
        body_excerpt(&html)
    );
}

/// First ~800 chars of the `<body>` for assertion failure messages.
fn body_excerpt(html: &str) -> String {
    let start = html.find("<body").unwrap_or(0);
    html[start..].chars().take(800).collect()
}

/// The Phase 4E document-level `echo: false` document (seam J2): `engine:
/// julia`, `execute: {daemon: false, echo: false}`, one `{julia}` cell whose
/// SOURCE contains a distinctive token (`j2_hidden_source_variable`) that
/// must NEVER appear in the rendered HTML, and whose OUTPUT contains a
/// distinctive, different token (`j2 output present`) that must.
const ECHO_FALSE_DOC: &str = "---\nengine: julia\nexecute:\n  daemon: false\n  echo: false\n---\n\n```{julia}\nj2_hidden_source_variable = \"should-not-appear\"\nprintln(\"j2 output present\")\n```\n";

// ── J2: cell options via metadata threading — document-level `echo: false`
// (Phase 4E) ─────────────────────────────────────────────────────────────────
//
// Renders `ECHO_FALSE_DOC` and asserts BOTH halves: the executed cell's
// OUTPUT is present, and its SOURCE listing is absent. Binds the P1.1b
// metadata-threading hunk (`ctx.metadata.clone()` at
// `ts_engine.rs:394`) → `metadataAsFormat` peels the document-level
// `execute:` bin into `format.execute` → `applyExecuteDefaults` leaves the
// explicit `echo: false` untouched → jupyter `toMarkdown`'s `includeCode`
// omits the `.cell-code` fenced block entirely (verified against
// `to-markdown.ts:758-813`) while `includeOutput` still emits the
// `.cell-output` block.
//
// Named revert (Test Seam Spec J2 = the shared T14 hunk): restore
// `metadata: HashMap::new()` at `ts_engine.rs:394` → `format.metadata` (and
// therefore `format.execute`) arrives empty → `applyExecuteDefaults` fills
// the ABSENT `echo` key with its base default (`true`, per
// `kExecuteVisibilityDefaults` in `metadata-as-format.ts`) → the cell's
// source listing (with the distinctive `j2_hidden_source_variable` token) is
// included → RED.
#[test]
fn j2_document_level_echo_false_hides_source_keeps_output() {
    if !deno_available() {
        eprintln!(
            "SKIP: deno not on PATH — j2_document_level_echo_false_hides_source_keeps_output"
        );
        return;
    }
    if !julia_available() {
        eprintln!(
            "SKIP: julia not on PATH — j2_document_level_echo_false_hides_source_keeps_output"
        );
        return;
    }
    let tmp = setup_julia_project();
    let input = tmp.path().join("echo-false.qmd");
    write_file(&input, ECHO_FALSE_DOC);

    let html = render_html(&input);

    // Discriminator half 1: the cell executed and its output landed in the
    // page (an `.cell-output` block is present at all — a total render
    // failure or an all-cells-dropped render can't fake this).
    assert!(
        html.contains("cell-output"),
        "rendered HTML must contain an executed cell-output block; got:\n{}",
        body_excerpt(&html)
    );
    // Discriminator half 2: the distinctive OUTPUT token is present.
    assert!(
        html.contains("j2 output present"),
        "rendered HTML must contain the cell's println output; got:\n{}",
        body_excerpt(&html)
    );
    // Discriminator half 3 (the actual J2 property): the distinctive SOURCE
    // token must be ABSENT — document-level `echo: false` must suppress the
    // source listing.
    assert!(
        !html.contains("j2_hidden_source_variable"),
        "rendered HTML must NOT contain the cell's source listing \
         (document-level `execute: echo: false` should suppress it); got:\n{}",
        body_excerpt(&html)
    );
}

/// The Phase 4E `julia:` block fixture (seam J3 + env fold), split across
/// TWO files (see the vacuity trail in the J3 test comment):
///
/// - `_quarto.yml` (project config) carries the `julia:` block —
///   `exeflags: ["--threads=2"]`, `env: ["FOO=BAR"]`;
/// - the document carries only `engine: julia` + `execute: daemon: false`
///   and one `{julia}` cell printing both `Threads.nthreads()` and
///   `ENV["FOO"]` on distinctive, discriminating lines.
///
/// Placement rationale:
/// 1. GREEN requires the whole q2 delivery pipeline: `_quarto.yml` parse →
///    MetadataMergeStage project layer → merged `doc.ast.meta` → (both) the
///    serialized-QMD frontmatter handed to the engine AND the T14-threaded
///    wire `format.metadata` → QNR worker spawn. A DOCUMENT-level `julia:`
///    block would test none of that (QNR would read it from the document
///    text itself).
/// 2. Project-config strings stay literal (`InterpretationContext::
///    ProjectConfig` in `pampa/src/pandoc/meta.rs`), so the seam spec's
///    exact `--threads=2` survives here. In DOCUMENT frontmatter it would
///    be smart-typography-mangled to `–threads=2` (en dash) and QNR would
///    treat it as a file argument — a real q2 metadata-substrate bug,
///    filed as bd-uf4epv4w.
const JULIA_BLOCK_QUARTO_YML: &str =
    "project:\n  type: default\njulia:\n  exeflags: [\"--threads=2\"]\n  env: [\"FOO=BAR\"]\n";
const JULIA_BLOCK_DOC: &str = "---\nengine: julia\nexecute:\n  daemon: false\n---\n\n```{julia}\nprintln(\"nthreads: \", Threads.nthreads())\nprintln(\"FOO=\", get(ENV, \"FOO\", \"MISSING\"))\n```\n";

// ── J3: `exeflags` (+ folded `env`) through the `julia:` block (Phase 4E)
// ───────────────────────────────────────────────────────────────────────────
//
// STOP-POINT (resolved): confirmed the exact frontmatter schema against the
// INSTALLED QuartoNotebookRunner 0.17.4 source (matches the fixture's pinned
// `Project.toml` `QuartoNotebookRunner = "=0.17.4"`), not just its docs:
// `~/.julia/packages/QuartoNotebookRunner/*/src/server.jl`, function
// `_exeflags_and_env(options)` (~line 151): `options["format"]["metadata"]
// ["julia"]["exeflags"]` and `["env"]` — i.e. exactly a document-level
// `julia:` block with `exeflags`/`env` arrays, landing in
// `format.metadata.julia` (the `julia:` top-level key is NOT one of
// `metadataAsFormat`'s six bin names, so it classifies into the generic
// `format.metadata` bin verbatim). `env` entries are `KEY=VALUE` strings
// merged into the WORKER process environment via `addenv` (same file,
// `_get_worker_cmd`) — confirmed by reading the same source, not assumed.
//
// Renders `JULIA_BLOCK_DOC` (with `JULIA_BLOCK_QUARTO_YML` as the temp
// project's `_quarto.yml`) and asserts both `nthreads: 2` (exeflags reached
// QNR, spawned the worker with 2 threads) and `FOO=BAR` (env reached the
// worker process) in distinctive, discriminating output lines (not bare
// digits/strings page chrome could fake).
//
// VACUITY CORRECTION (found during RED verification, 2026-07-02; deviates
// from the frozen seam spec's named revert — flagged for controller
// sign-off in the 4E report): the spec's T14 revert (`metadata:
// HashMap::new()` at `ts_engine.rs:394`) CANNOT redden any QNR-observable
// assertion, for two stacked reasons, both verified empirically (the
// document-level fixture AND a project-level fixture both stayed GREEN
// under the T14 revert):
//
// 1. QNR merges the notebook FILE's own frontmatter under the wire options
//    (`_extract_relevant_options(file_frontmatter, options)`, QNR 0.17.4
//    `server.jl:366/460-530`) — so a document-level `julia:` block reaches
//    QNR even with the wire metadata emptied.
// 2. Deeper: julia-engine.ts sends `target.markdown` in the run command,
//    and QNR's socket layer uses it as a file-content OVERRIDE
//    (`_get_markdown`, `socket.jl:497-498` → `run(...; markdown)`,
//    `server.jl:347-365`). q2's engine input is the AST serialized AFTER
//    MetadataMergeStage (`serialize_ast_to_qmd` in engine_execution.rs),
//    so the override's frontmatter carries the FULL merged metadata —
//    project layers included. Anything in merged metadata therefore
//    reaches QNR through the markdown override even when the T14 wire
//    path is reverted. The T14 hunk itself stays revert-bound by J2
//    (host-side `format.execute` consumption, which has no QNR fallback).
//
// Named revert (J3, re-anchored with evidence): the PROJECT metadata layer
// in `MetadataMergeStage::run` (`stage/stages/metadata_merge.rs:~215`,
// `let project_layer = ctx.project.config.metadata.as_ref().map(…)` →
// replace with `let project_layer: Option<ConfigValue> = None;`) → the
// `_quarto.yml` `julia:` block never enters merged metadata → absent from
// BOTH the serialized frontmatter and the wire options → QNR's
// `default_frontmatter()` supplies `julia => D("env" => [], "exeflags" =>
// [])` → worker spawns with QNR's default thread count (1) and no `FOO`
// env var → RED (`nthreads: 1`, not `nthreads: 2`; `FOO=MISSING`, not
// `FOO=BAR`).
#[test]
fn j3_exeflags_and_env_through_julia_block() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — j3_exeflags_and_env_through_julia_block");
        return;
    }
    if !julia_available() {
        eprintln!("SKIP: julia not on PATH — j3_exeflags_and_env_through_julia_block");
        return;
    }
    let tmp = setup_julia_project();
    write_file(&tmp.path().join("_quarto.yml"), JULIA_BLOCK_QUARTO_YML);
    let input = tmp.path().join("julia-block.qmd");
    write_file(&input, JULIA_BLOCK_DOC);

    let html = render_html(&input);

    assert!(
        html.contains("cell-output"),
        "rendered HTML must contain an executed cell-output block; got:\n{}",
        body_excerpt(&html)
    );
    // exeflags: `--threads=2` (from _quarto.yml) must reach the QNR worker.
    assert!(
        html.contains("nthreads: 2"),
        "rendered HTML must show the worker started with 2 threads \
         (project-config julia: exeflags: [\"--threads=2\"] must reach \
         QuartoNotebookRunner via q2's metadata threading); got:\n{}",
        body_excerpt(&html)
    );
    // env (folded into this row): `FOO=BAR` must reach the QNR worker's
    // process environment.
    assert!(
        html.contains("FOO=BAR"),
        "rendered HTML must show the worker's FOO env var as BAR \
         (julia: env: [\"FOO=BAR\"] must reach the QuartoNotebookRunner worker); got:\n{}",
        body_excerpt(&html)
    );
}

/// The Phase 4D error document: `engine: julia`, `execute: daemon: false`, one
/// `{julia}` cell that calls `error(...)`.
const ERROR_DOC: &str = "---\nengine: julia\nexecute:\n  daemon: false\n---\n\n```{julia}\nerror(\"this should fail gracefully\")\n```\n";

// ── J4: error handling does not wedge the host (Phase 4D) ─────────────────────
//
// Renders a Julia doc whose only cell calls `error(...)`, then — through the
// SAME `TsEngineHost` (same Deno subprocess) — renders the J1 minimal doc, to
// prove a failed execute doesn't leave the host wedged (pending-request
// cleanup).
//
// "Same process" binding, explained: `ProjectContext::discover` is called
// EXACTLY ONCE for both renders. Per `render_document_to_file`
// (`render_to_file.rs`) and `run_pipeline`/`render_qmd_to_html`
// (`pipeline.rs`, "project registry is already in `stage_ctx.registry` from
// ... unless `ctx.engine_registry_override` is set"), a render that receives
// `Some(&project)` uses `project.registry` — the SAME `Arc<EngineRegistry>`,
// holding the SAME `Arc<TsEngineHost>` (constructed once, at
// `ProjectContext::discover` time, in `build_engine_registry`) — for every
// file rendered against that project. This is exactly the mechanism a real
// multi-page project render uses (discover once, call
// `render_document_to_file` per page against the shared `ProjectContext`;
// see J6/J8 in the Test Seam Spec) — so reusing one discovered `project` for
// both calls here is the faithful analog of "the same process" a real
// multi-doc session would share, without inventing new test-only plumbing.
//
// Named revert (Test Seam Spec J4, corrected round 3): the `FromEngine::Error`
// arm in `TsEngineHost::request` (`ts_process.rs:~693`) — reverting it makes
// `request()` no longer convert an error frame to `Err` first, so the error is
// swallowed/mistyped and assertion (a) reddens.
#[test]
fn j4_error_handling_does_not_wedge_host() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — j4_error_handling_does_not_wedge_host");
        return;
    }
    if !julia_available() {
        eprintln!("SKIP: julia not on PATH — j4_error_handling_does_not_wedge_host");
        return;
    }
    let tmp = setup_julia_project();
    let error_input = tmp.path().join("error.qmd");
    write_file(&error_input, ERROR_DOC);
    let minimal_input = tmp.path().join("minimal.qmd");
    write_file(&minimal_input, MINIMAL_DOC);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let options = RenderToFileOptions::default();

    // Discover ONCE — both renders below share `project.registry`'s
    // `Arc<TsEngineHost>` (see the binding note above).
    let project = ProjectContext::discover(&error_input, runtime.as_ref())
        .expect("project discovery for the shared-host project");

    // (a) + (b): render the error doc. Must return an `Err` whose message
    // contains the Julia error text; must NOT panic (a panic here fails the
    // test outright, which is the assertion for (b) — no separate check
    // needed).
    let error_result = render_document_to_file(
        &error_input,
        "html",
        &options,
        Some(&project),
        runtime.clone(),
        None,
        None,
        None,
    );
    let err = error_result.expect_err("error doc must fail the render, not succeed");
    let message = err.to_string();
    assert!(
        message.contains("this should fail gracefully"),
        "render error must surface the Julia error text; got: {message}"
    );
    // Discriminator: the message must be the PROPERLY TYPED
    // `ExecutionError::ExecutionFailed` (Display: "Execution failed in
    // {engine}: {message}"), not `TsEngine::execute`'s generic
    // "unexpected response to Execute: {other:?}" fallback (which is what
    // you get if `TsEngineHost::request`'s `FromEngine::Error` arm is
    // reverted: `request()` then returns `Ok(FromEngine::Error{..})`
    // instead of converting it to `Err` first, `execute()`'s `match
    // response` falls through to its `other` arm, and THAT arm's
    // `{other:?}` Debug-dump of the `FromEngine::Error` struct still
    // happens to embed the raw message text — so a bare `contains("this
    // should fail gracefully")` check alone does NOT redden on the named
    // revert; this second assertion is required for the row to actually
    // discriminate the revert, confirmed empirically below).
    assert!(
        message.contains("Execution failed in julia:"),
        "render error must be the typed ExecutionFailed error, not a \
         debug-dump fallback; got: {message}"
    );
    assert!(
        !message.contains("unexpected response to Execute"),
        "render error must not be TsEngine::execute's generic fallback \
         (a sign request() didn't convert the error frame); got: {message}"
    );

    // (c) A subsequent render THROUGH THE SAME HOST (same discovered
    // `project`, same `Arc<EngineRegistry>`, same `Arc<TsEngineHost>`) must
    // still succeed — proving the failed request didn't leave the Deno
    // subprocess / pending-request map wedged.
    let minimal_result = render_document_to_file(
        &minimal_input,
        "html",
        &options,
        Some(&project),
        runtime.clone(),
        None,
        None,
        None,
    )
    .expect("subsequent render through the same host must still succeed");
    let html = std::fs::read_to_string(&minimal_result.output_path)
        .expect("read second render's output HTML");
    assert!(
        html.contains("cell-output") && html.contains("<code>2</code>"),
        "second render (same host) must still execute correctly; got:\n{}",
        body_excerpt(&html)
    );
}

// ── Website-project (Phase 4H) shared setup ───────────────────────────────────

/// Absolute path to the committed julia-website fixture root
/// (`crates/quarto-core/tests/fixtures/extensions/julia-website`). It holds
/// ONLY `_quarto.yml` + `index.qmd` + `plot.qmd`; no `_extensions/` and no
/// notebook environment are committed there (they are copied in at runtime
/// from the sibling `julia-engine` fixture — the echo `setup_project` pattern).
fn julia_website_fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/extensions/julia-website")
}

/// Build a temp WEBSITE project for the Phase-4H rows: the committed
/// `julia-website` pages plus, copied in at runtime, the `julia-engine`
/// extension AND its notebook environment (`Project.toml` + `Manifest.toml`,
/// which declare `Plots` — the plot page needs them to render). Returns the
/// `TempDir` (keep alive for the test's duration).
fn setup_julia_website_project() -> TempDir {
    let tmp = TempDir::new().unwrap();
    // The two committed pages + project config.
    for name in ["_quarto.yml", "index.qmd", "plot.qmd"] {
        std::fs::copy(
            julia_website_fixture_root().join(name),
            tmp.path().join(name),
        )
        .unwrap();
    }
    // The engine package (co-located `.jl` + bundle), copied from the sibling
    // fixture — nothing julia-specific is committed under `julia-website/`.
    copy_dir(
        &julia_fixture_root().join("_extensions/julia-engine"),
        &tmp.path().join("_extensions").join("julia-engine"),
    );
    // The notebook Julia environment (Plots dependency), also from the sibling
    // fixture root — the plot worker resolves `@.` up from the notebook dir.
    for env_file in ["Project.toml", "Manifest.toml"] {
        std::fs::copy(
            julia_fixture_root().join(env_file),
            tmp.path().join(env_file),
        )
        .unwrap();
    }
    tmp
}

/// Render the whole temp WEBSITE project through the two-pass `ProjectPipeline`
/// (the same orchestrator `q2 render <dir>` drives), returning the discovered
/// `ProjectContext` and the render summary. The pipeline is dropped before
/// returning (its `&mut project` borrow ends), so `project` can be inspected.
fn render_julia_website(
    tmp: &TempDir,
) -> (
    ProjectContext,
    quarto_core::project::orchestrator::ProjectRenderSummary,
) {
    // Discover from the project DIRECTORY (not a single file) so the file
    // list enumerates BOTH pages — `discover(<file>)` returns a one-file
    // project, which would silently skip `plot.qmd`.
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project =
        ProjectContext::discover(tmp.path(), runtime.as_ref()).expect("website project discovery");
    let options = RenderToFileOptions::default();
    let summary;
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
        summary = pollster::block_on(pipeline.run()).expect("website orchestrator run");
    }
    (project, summary)
}

// ── J5: figures + supporting-forward in a website render (Phase 4C+4H) ─────────
//
// Renders the two-page julia website through the ProjectPipeline and asserts
// the Julia-emitted figure lands as a FILE at the per-page location
// (`_site/plot_files/figure-html/*.png`) and `plot.html` references it via
// `<img src=...>`, plus both pages produced HTML.
//
// CRITICAL vacuity note (from the 4CD MIME finding): a default Plots.jl GR
// plot is `text/html`-showable, and q2's faithfully-ported `displayDataMimeType`
// prefers `text/html` for HTML targets — so `plot(...)` alone yields an INLINE
// base64 `<img>` and NO figure file, which would make the `supporting`-forward
// assertion VACUOUS. The `plot.qmd` fixture therefore renders the plot to a
// PNG file with `savefig` and returns a `PngFigure` wrapper struct showable
// ONLY as `image/png` (no `text/html` show method): QuartoNotebookRunner's
// `render_mimetypes` then captures only `image/png`, so the wire `data`
// bundle carries ONLY `image/png` → `displayDataMimeType` selects it →
// jupyter `toMarkdown`'s `mdImageOutput` WRITES the file. Verified
// empirically (a PNG file materialized under `plot_files/figure-html/`) before
// this row was frozen.
//
// Named revert (Test Seam Spec J5): the `supporting` forward in
// `map_execute_result` (`ts_engine.rs`, `supporting_files` at ~:463/467) →
// `let supporting_files = Vec::new();` → the figure dir is not carried into
// the project artifacts → no `_site/plot_files/figure-html/*.png` → RED.
//
// RESOLVED (2026-07-02, bd-677297ca) — this row surfaced an architectural
// defect while it was being implemented: a website julia-figure render
// WROTE the figure to `_site/plot_files/figure-html/*.png` correctly
// (verified end-to-end via the `q2 render` binary — the file materialized),
// but the render then RETURNED AN ERROR from a *later* project step:
// `copy_resources_to_output_dir` (`project_resources.rs`) called
// `runtime.file_copy` on the whole `plot_files` DIRECTORY. julia-engine
// reports its supporting entry as the `_files` directory
// (`supporting: [base_dir/supporting_dir]`, a faithful Q1 port), which
// flowed through `engine_execution.rs add_engine_files` →
// `resolve_reported_resources` (which does not walk directories) →
// `file_copy(<dir>)` → "the source path is neither a regular file nor a
// symlink to a regular file". No existing test exercised a directory
// supporting entry (the unit tests at `project_resources.rs` all passed
// individual files), and 4CD's single-doc plot render produced an INLINE
// base64 figure (no file), so this was the first file-based julia figure to
// reach the project resource copier. The controller adjudicated option (c):
// `DocumentResourceReport::add_engine_files` now expands a reported
// directory into its contained files (recursively, via `SystemRuntime`)
// at report time, so the report — and everything downstream — stays
// file-only; `resolve_reported_resources` and `copy_resources_to_output_dir`
// are unchanged. This row is un-`#[ignore]`d now that the fix lands; the
// assertions below were already correct and the figure-file half was
// separately proven by the manual `q2 render` evidence in the 4H report.
#[test]
fn j5_website_figure_lands_as_file_and_is_referenced() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — j5_website_figure_lands_as_file_and_is_referenced");
        return;
    }
    if !julia_available() {
        eprintln!("SKIP: julia not on PATH — j5_website_figure_lands_as_file_and_is_referenced");
        return;
    }
    let tmp = setup_julia_website_project();
    let (_project, _summary) = render_julia_website(&tmp);

    let site = tmp.path().join("_site");
    let index_html = site.join("index.html");
    let plot_html = site.join("plot.html");
    assert!(
        index_html.is_file(),
        "_site/index.html must be produced; _site contents:\n{}",
        list_dir(&site)
    );
    assert!(
        plot_html.is_file(),
        "_site/plot.html must be produced; _site contents:\n{}",
        list_dir(&site)
    );

    // The Julia figure must land as a FILE at the per-page figure location,
    // NOT in site_libs (bound by the `supporting` forward).
    let figure_dir = site.join("plot_files").join("figure-html");
    let pngs: Vec<PathBuf> = std::fs::read_dir(&figure_dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("png"))
                .collect()
        })
        .unwrap_or_default();
    assert!(
        !pngs.is_empty(),
        "a PNG figure file must exist under _site/plot_files/figure-html/ \
         (the `supporting` forward carries the engine figure dir into _site); \
         found none. _site tree:\n{}",
        list_dir_recursive(&site)
    );

    // plot.html references the figure via `<img src=...plot_files/figure-html...>`.
    let plot_html_text = std::fs::read_to_string(&plot_html).expect("read plot.html");
    assert!(
        plot_html_text.contains("<img")
            && plot_html_text.contains("plot_files/figure-html")
            && !plot_html_text.contains("data:image/png;base64"),
        "plot.html must reference the figure FILE via <img src=...plot_files/figure-html...> \
         (NOT an inline base64 data-URI); got:\n{}",
        body_excerpt(&plot_html_text)
    );
}

// ── J6: one engine host per project render (Phase 4H) ─────────────────────────
//
// Renders the same two-page website with a tracing capture subscriber
// installed and asserts EXACTLY ONE `engine_host` spawn event (J8's net-new
// production event) across BOTH files — proving the `Arc<TsEngineHost>` built
// once in `ProjectContext::discover` is shared across every Pass-2 file, not
// reconstructed per page. `index.qmd` has no cells (no execute → no spawn);
// `plot.qmd` triggers the single spawn.
//
// Named revert (Test Seam Spec J6): move registry+host construction from the
// single `ProjectContext::discover` call into the orchestrator's per-file loop
// → two spawn events → RED. That revert is invasive (it restructures project
// discovery), so per the brief this row proves the capture DISCRIMINATES by an
// equivalent cheaper mechanism rather than physically performing it — see the
// `j6_capture_discriminates_two_hosts` companion test below, which builds two
// independent hosts and observes two events, demonstrating the capture would
// redden if host construction were duplicated per file.
//
// RESOLVED (2026-07-02, bd-677297ca) — same architectural defect as J5: this
// row drives the full website render, which used to error in
// `copy_resources_to_output_dir` on the `plot_files` directory before the
// render returned. The ONE-spawn property WAS observed manually before the
// fix: with `RUST_LOG=engine_host=info,engine_resolution=info` the
// `q2 render` of the website emitted exactly one `engine-host spawned pid=…`
// event (ordered AFTER two `engine resolution complete` events) — recorded
// verbatim in the 4H report. Now that `add_engine_files` expands supporting
// directories into files (see J5's comment above), the full render
// completes and this row is un-`#[ignore]`d. The capture's discrimination
// is separately proven GREEN by `j6_capture_discriminates_two_hosts`
// (deno-only, no project render).
//
// `QUARTO_JOBS=1` (below) forces the Pass-2 SEQUENTIAL dispatch
// (`render_batch_serial`), same precedent as `fail_fast.rs`. With the
// default worker count (`available_parallelism()`, > 1 on any real
// machine) this two-doc project takes the rayon `render_batch_parallel`
// fan-out instead, which renders `plot.qmd` — and so calls
// `TsEngineHost::ensure_started` — on a rayon worker thread. Rayon
// worker threads never call `tracing::subscriber::with_default`, so
// they run under tracing's `Unset` per-thread state and never observe
// a THREAD-LOCAL override — the `engine-host spawned` event is emitted
// but the capture layer never sees it (`capture.count("engine_host")`
// silently reads 0, independent of whether one host or two were
// spawned). Forcing sequential dispatch keeps the whole render on the
// test thread the capture is scoped to, making the one-spawn assertion
// actually observe what it claims to. This changes *how* the docs are
// dispatched, not *what* is being asserted: the shared
// `Arc<TsEngineHost>` reuse being tested lives in `ProjectContext::discover`,
// upstream of both the serial and parallel Pass-2 paths.
#[test]
fn j6_one_engine_host_per_project_render() {
    use tracing_subscriber::layer::SubscriberExt;
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — j6_one_engine_host_per_project_render");
        return;
    }
    if !julia_available() {
        eprintln!("SKIP: julia not on PATH — j6_one_engine_host_per_project_render");
        return;
    }
    // SAFETY/scope: process-local under nextest (one process per test).
    unsafe {
        std::env::set_var("QUARTO_JOBS", "1");
    }
    let tmp = setup_julia_website_project();

    let capture = TargetCapture::default();
    let subscriber = tracing_subscriber::registry().with(capture.clone());
    tracing::subscriber::with_default(subscriber, || {
        let (_project, _summary) = render_julia_website(&tmp);
    });

    // Exercised-guard: the render actually produced the figure (so "one spawn"
    // isn't one because nothing ran).
    assert!(
        tmp.path().join("_site/plot.html").is_file(),
        "exercised-guard: the plot page must have rendered for the spawn-count \
         assertion to be non-vacuous"
    );
    assert_eq!(
        capture.count("engine_host"),
        1,
        "exactly ONE engine_host spawn event across the whole two-page project \
         render (the shared Arc<TsEngineHost> is built once in discover, not \
         per file); saw {} events",
        capture.count("engine_host")
    );
}

// J6 discriminator companion (engine-agnostic, deno-only): proves the tracing
// capture used by `j6_one_engine_host_per_project_render` genuinely counts
// per-spawn — two independently-started hosts yield TWO `engine_host` events.
// This is the cheaper faithful stand-in for J6's invasive named revert (moving
// host construction into the per-file loop would duplicate the host exactly
// like this): if the capture could NOT tell one host from two, J6 would pass
// vacuously. Deno-gated only (no Julia needed — the event fires at spawn).
#[test]
fn j6_capture_discriminates_two_hosts() {
    use quarto_core::engine::ts_process::TsEngineHost;
    use quarto_core::engine::ts_protocol::HostGlobalConfig;
    use tracing_subscriber::layer::SubscriberExt;
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — j6_capture_discriminates_two_hosts");
        return;
    }
    let global = HostGlobalConfig {
        resource_dir: "/res".to_string(),
        runtime_dir: "/rt".to_string(),
        data_dir: "/data".to_string(),
        pandoc_path: None,
        is_interactive_session: false,
        running_in_ci: false,
        quarto_version: "0.1.0".to_string(),
    };

    let capture = TargetCapture::default();
    let subscriber = tracing_subscriber::registry().with(capture.clone());
    tracing::subscriber::with_default(subscriber, || {
        // TWO separate hosts, each spawning the real deno engine-host once.
        let h1 = TsEngineHost::new(global.clone());
        h1.ensure_started().expect("host 1 spawn");
        let h2 = TsEngineHost::new(global.clone());
        h2.ensure_started().expect("host 2 spawn");
        let _ = h1.shutdown();
        let _ = h2.shutdown();
    });
    assert_eq!(
        capture.count("engine_host"),
        2,
        "two independently-started hosts must emit two engine_host events — \
         proving the capture discriminates (so J6's single-event assertion is \
         non-vacuous); saw {}",
        capture.count("engine_host")
    );
}

/// A tracing-capture layer recording each event's `target`, shared across
/// threads via `Arc<Mutex<Vec<String>>>` (mirrors the unit-test helper in
/// `ts_process.rs`; the integration binary is a separate crate so it needs its
/// own copy).
#[derive(Clone, Default)]
struct TargetCapture {
    targets: Arc<std::sync::Mutex<Vec<String>>>,
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for TargetCapture {
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

impl TargetCapture {
    fn count(&self, target: &str) -> usize {
        self.targets
            .lock()
            .unwrap()
            .iter()
            .filter(|t| t.as_str() == target)
            .count()
    }
}

// ── PC4a: ABANDONED busy-worker close failure (Bug A repro) ───────────────────
//
// Reproduces bd-h4rhohhy's sibling defect "Bug A" (`Q-PREVIEW-CAP-1`) via the
// plan's specified mechanism (plan §P0): `record_capture` whose client is
// ABANDONED mid-run (the live EPIPE scenario), leaving the worker orphaned-busy,
// then a FRESH `record_capture` of the same doc through the same shared server —
// whose oneShot pre-run `close` hits that abandoned worker and gets a bare
// "worker is busy" protocol error, so the capture is discarded (`record_capture`
// returns `Err`; the real caller `record_eager_captures` then soft-fails and
// emits `Q-PREVIEW-CAP-1`). No busy handling exists in julia-engine.ts around the
// close (:703-718 pre-run, :742-749 post-run).
//
// WHY THE ABANDONED SCENARIO (not two concurrent live renders): the fix shape
// hinges on it. An ABANDONED worker (client gone) is safe to force-close — the
// PC2 recovery. A worker that is legitimately still serving a live concurrent
// render is NOT safe to force-close. This harness reproduces the ABANDONED case
// the user actually hit; the concurrent-live case is the plan's documented
// (not-gold-plated) oneShot-reuse design question for the upstream PR.
//
// MECHANISM (deterministic):
//   1. record_capture #1 runs a cell that writes a SENTINEL file, then sleeps.
//      The sentinel is the REAL "worker is executing" signal (not a timing
//      guess): once it appears, the worker is provably mid-run.
//   2. We ABANDON #1's client by KILLING its Deno engine-host (identified by the
//      isolated-HOME bundle path in its cmdline). Its QNR socket closes → EPIPE →
//      QuartoNotebookRunner leaves the worker BUSY (it does not cancel the task).
//   3. record_capture #2 (fresh host, same file, same isolated server) →
//      oneShot pre-run `isopen`→`close` → "worker is busy" → Err.
//
// VERBATIM pre-fix failure (record_capture #2) — the frozen RED shape:
//   Execution failed in julia: Julia server returned error after receiving
//   "close" command: … Tried to close file "…/sleepy.qmd" but the corresponding
//   worker is busy.
//
// POST-FIX assertion FROZEN (seam PC4, decision-rule branch YES = recovery):
// QNR exposes `forceclose` (live-confirmed), so the ratified fix recovers — the
// pre-run close falls back to forceclose on the abandoned worker and #2 SUCCEEDS
// with a real capture — NOT this bare protocol error, and NOT a discarded capture.
//
// ISOLATION + CLEANUP (mandatory — protects the user's real julia server,
// bd-l9jhy5u0): runs under a TEMP `HOME` so the transport file / server live in a
// throwaway cache dir, and an `IsolatedJuliaServerGuard` KILLS the detached
// server (its process group) on scope exit — even on panic. An isolated `HOME`
// also empties the julia depot (and juliaup would install a different julia), so
// the RUNNER must provide, in the ambient env:
//   * `JULIA_DEPOT_PATH=<real ~/.julia>`     (so QuartoNotebookRunner resolves)
//   * `QUARTO_JULIA_PROJECT=<instantiated>`  (e.g. real ~/Library/Caches/quarto/julia)
//   * a real `julia` on PATH matching the project's Manifest (NOT a juliaup shim
//     that re-resolves under the temp HOME)
// P3 (bd-h4rhohhy) ALSO isolates the julia PROJECT itself: `isolate_julia_project()`
// recursively copies the ambient `QUARTO_JULIA_PROJECT` dir (incl. its
// `Project.toml`/`Manifest.toml`)
// into a per-test temp dir and re-points `QUARTO_JULIA_PROJECT` at the copy, so
// the detached server's `--project=` flag never names the shared, real directory
// (the depot named by `JULIA_DEPOT_PATH` stays shared — no re-instantiation).
// `SharedTransportSentinel` proves the shared `julia_transport.txt` is untouched
// across the run (existence + mtime, captured from the REAL `HOME` before the
// override). Gated behind `QUARTO_PC4A_LIVE=1` AND `#[ignore]` so it never runs —
// nor touches any julia server — by accident. Unix only (cleanup uses process
// groups). Run it explicitly:
//   QUARTO_PC4A_LIVE=1 JULIA_DEPOT_PATH=~/.julia \
//     QUARTO_JULIA_PROJECT=~/Library/Caches/quarto/julia \
//     PATH="<real-julia-bin>:$PATH" \
//     cargo nextest run -p quarto-core --test integration \
//     -E 'test(pc4a_abandoned_worker_close_busy)' --run-ignored all

/// Build the sleeping-cell doc. The cell writes `sentinel` (the real
/// "worker executing" signal) BEFORE the long sleep, so the harness can abandon
/// the client at a provably-mid-run instant instead of guessing with a timer.
fn sleepy_doc(sentinel: &Path) -> String {
    format!(
        "---\nengine: julia\nexecute:\n  daemon: false\n---\n\n\
         ```{{julia}}\nwrite(raw\"{}\", \"running\")\nsleep(60)\n1 + 1\n```\n",
        sentinel.display()
    )
}

/// Parse the `"pid": N` field out of a julia transport-file JSON.
fn transport_pid(transport: &Path) -> Option<i32> {
    let text = std::fs::read_to_string(transport).ok()?;
    let idx = text.find("\"pid\"")?;
    text[idx + 5..]
        .trim_start_matches([':', ' '])
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .and_then(|s| s.parse::<i32>().ok())
}

/// Kills the isolated detached julia server (its whole process group, so QNR
/// worker children go too) on scope exit — even on panic. Never touches a
/// process it did not start: the pid is read from the isolated transport file.
struct IsolatedJuliaServerGuard {
    transport: PathBuf,
}

impl Drop for IsolatedJuliaServerGuard {
    fn drop(&mut self) {
        if let Some(pid) = transport_pid(&self.transport) {
            // Negative pid = process group (the detached server is a session
            // leader, so pid == pgid, and its QNR workers share the group).
            let _ = Command::new("kill")
                .arg("-TERM")
                .arg(format!("-{pid}"))
                .status();
            // Fallback: also target the bare pid in case group signalling is
            // unavailable for any reason.
            let _ = Command::new("kill").arg(format!("{pid}")).status();
        }
    }
}

/// Kill any Deno engine-host whose cmdline references this isolated HOME's
/// bundle dir (the process is unique to this test's temp HOME). This ABANDONS
/// the in-flight client the way the live EPIPE did.
fn kill_deno_under_home(home: &Path) {
    let pattern = home.join("Library/Caches/quarto/bundles");
    if let Ok(out) = Command::new("pgrep")
        .arg("-f")
        .arg(pattern.to_string_lossy().as_ref())
        .output()
    {
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            if let Ok(pid) = line.trim().parse::<i32>() {
                let _ = Command::new("kill").arg(format!("{pid}")).status();
            }
        }
    }
}

#[test]
#[ignore = "live-julia Bug A repro; RED pre-fix; needs QUARTO_PC4A_LIVE + isolation env (see docstring)"]
fn pc4a_abandoned_worker_close_busy() {
    if std::env::var("QUARTO_PC4A_LIVE").as_deref() != Ok("1") {
        eprintln!(
            "SKIP: set QUARTO_PC4A_LIVE=1 (+ isolation env) to run pc4a_abandoned_worker_close_busy"
        );
        return;
    }
    if !cfg!(unix) {
        eprintln!("SKIP: pc4a_abandoned_worker_close_busy cleanup requires unix process groups");
        return;
    }
    if !deno_available() || !julia_available() {
        eprintln!("SKIP: deno/julia not on PATH — pc4a_abandoned_worker_close_busy");
        return;
    }

    // Snapshot the REAL, shared transport file's state BEFORE any override —
    // proves at the end of the run that the isolation never touched it.
    let real_home = std::env::var("HOME").ok().map(PathBuf::from);
    let shared_sentinel = real_home.as_deref().map(SharedTransportSentinel::capture);

    // P3: copy the ambient QUARTO_JULIA_PROJECT dir into a per-test temp dir
    // and re-point the env var at the copy, BEFORE overriding HOME (isolate_
    // julia_project reads the still-ambient QUARTO_JULIA_PROJECT).
    let Some(_project_tmp) = isolate_julia_project() else {
        eprintln!(
            "SKIP: QUARTO_JULIA_PROJECT unset or not instantiated (no Manifest.toml) — \
             pc4a_abandoned_worker_close_busy"
        );
        return;
    };

    // Isolate the transport file/server under a temp HOME so we never reuse or
    // clobber the user's real server. (Depot/julia-bin come from the ambient
    // env — see the docstring; project is isolated above.)
    let home = TempDir::new().unwrap();
    // SAFETY/scope: process-local under nextest (one process per test).
    unsafe {
        std::env::set_var("HOME", home.path());
    }
    let transport = home
        .path()
        .join("Library/Caches/quarto/julia/julia_transport.txt");
    // Guard drops LAST (after `home`/`tmp`), killing the detached server + its
    // worker group even if an assertion below panics.
    let _server_guard = IsolatedJuliaServerGuard {
        transport: transport.clone(),
    };

    let tmp = setup_julia_project();
    let sentinel = tmp.path().join("pc4a_running.sentinel");
    let input = tmp.path().join("sleepy.qmd");
    write_file(&input, &sleepy_doc(&sentinel));

    // record_capture #1 on a worker thread: opens a Deno host + QNR socket and
    // launches the per-file worker (which writes the sentinel, then sleeps).
    // It returns Err once we kill its Deno host below — that's the abandonment.
    let input_1 = input.clone();
    let handle_1 = std::thread::spawn(move || {
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        let project = ProjectContext::discover(&input_1, runtime.as_ref())
            .expect("project discovery for record_capture #1");
        // Result ignored — #1's job is to occupy the worker until abandoned.
        let _ = pollster::block_on(quarto_core::engine::preview_record::record_capture(
            &input_1, &project, runtime, None,
        ));
    });

    // Real signal: the worker wrote the sentinel → it is provably executing the
    // sleeping cell (busy). No timing guess.
    let mut waited = 0;
    while !sentinel.exists() && waited < 120 {
        std::thread::sleep(std::time::Duration::from_secs(1));
        waited += 1;
    }
    assert!(
        sentinel.exists(),
        "record_capture #1's worker never signalled it was executing within 120s \
         (check the isolation env: JULIA_DEPOT_PATH / QUARTO_JULIA_PROJECT / julia on PATH)"
    );

    // ABANDON #1's client mid-run: kill its Deno engine-host. Its QNR socket
    // closes → EPIPE → the worker is LEFT BUSY (QNR does not cancel the task).
    kill_deno_under_home(home.path());
    let _ = handle_1.join();

    // record_capture #2: fresh host, same file, same shared (isolated) server.
    // The oneShot pre-run close hits the ABANDONED busy worker.
    //
    // FROZEN POST-FIX assertion (seam PC4, decision-rule branch YES = recovery).
    // The QNR socket protocol exposes a `forceclose` command (julia-engine.ts
    // ServerCommand union; live-confirmed by the upstream smoke test
    // "force-closing a running worker"), so the ratified fix RECOVERS: the
    // pre-run close falls back to forceclose on busy, reclaims the abandoned
    // worker, and #2 SUCCEEDS — instead of the bare "worker is busy" error that
    // discarded the capture pre-fix. See worker-close.ts (upstream q2-close-busy-fix).
    let runtime2: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project2 = ProjectContext::discover(&input, runtime2.as_ref())
        .expect("project discovery for record_capture #2");
    let result_2 = pollster::block_on(quarto_core::engine::preview_record::record_capture(
        &input, &project2, runtime2, None,
    ));

    let captures = result_2.unwrap_or_else(|e| {
        panic!(
            "PC4a: fresh record_capture against the ABANDONED busy worker must SUCCEED \
             post-fix (pre-run close recovers via forceclose); got Err:\n{e}"
        )
    });
    // Non-vacuous: the recovered run must actually produce the julia cell's
    // executed output (a real capture, not an empty Ok).
    let markdown = captures
        .iter()
        .find(|c| c.engine_name == "julia")
        .map_or_else(
            || panic!("PC4a: expected a julia EngineCapture in the recovered result"),
            |c| {
                c.result
                    .get("markdown")
                    .and_then(|m| m.as_str())
                    .unwrap_or("")
            },
        );
    assert!(
        markdown.contains("cell-output"),
        "PC4a: recovered capture must contain the executed cell output \
         (proves forceclose recovery ran the cell, not an inert listing); markdown:\n{markdown}"
    );

    // P3 acceptance: the shared, real transport file was never touched by
    // this run's (isolated) QNR servers.
    if let Some(sentinel) = &shared_sentinel {
        sentinel.assert_untouched();
    }
}

/// One-level directory listing for assertion messages.
fn list_dir(dir: &Path) -> String {
    std::fs::read_dir(dir).map_or_else(
        |_| "<unreadable>".to_string(),
        |rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(", ")
        },
    )
}

/// Recursive file listing (relative paths) for assertion messages.
fn list_dir_recursive(dir: &Path) -> String {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for entry in rd.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    walk(&p, base, out);
                } else if let Ok(rel) = p.strip_prefix(base) {
                    out.push(rel.to_string_lossy().into_owned());
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out.join("\n")
}
