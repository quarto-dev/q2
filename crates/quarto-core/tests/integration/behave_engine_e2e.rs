/*
 * tests/integration/behave_engine_e2e.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Plan 4b Phase A / Task A2 — the `behave` behavioral synthetic engine
 * fixture (`crates/quarto-core/tests/fixtures/extensions/behave/`).
 *
 * `behave` is DELIBERATELY a single engine with multiple sentinel-gated
 * `execute()` branches (see `behave.ts`'s module comment), not a family of
 * engines — mirroring `echo-engine`'s `QUARTO_ECHO_CRASH` precedent. This
 * file is that fixture's permanent home: Task A2 (this task) binds ONLY the
 * default (no-sentinel) branch with a smoke test proving the committed
 * `dist/behave.js` bundle loads and executes for real under a live Deno
 * subprocess. The other branches are bound later, in later phases of Plan 4b:
 *
 *   - `intermediate_files` instance verb   -> Phase 4b-F
 *   - `QUARTO_PANDOC` / `QUARTO_EXEC` sentinels -> Phase 4b-D (optional stretch)
 *   - `QUARTO_SLOW` sentinel (cancellation)      -> Phase 4b-F (F1/F3/F4:
 *     cancel / timeout-poison-relaunch)
 *   - `BEHAVE_CRASH` sentinel (crash mid-execute) -> Phase 4b-F (F-crash relaunch)
 *   - FC-1 result-field carriage (metadata/pandoc/resourceFiles/preserve/
 *     postProcess/engineDependencies sentinel payload) -> Phase 4b-F, F6
 *
 * These are intentionally NOT tested here — they read as "orphan branches"
 * in `behave.ts` until Phase 4b-D/F land their binding assertions in THIS
 * file. See the Task A2 report (`.superpowers/sdd/plan4b-task-A2-report.md`)
 * for the full sentinel -> branch table and the FC-1 field-name mapping.
 *
 * Deno-gated exactly like `echo_engine_e2e.rs` / `synth_engines_e2e.rs`:
 * early-return (skip) when `deno` is not on PATH, NOT `#[ignore]`.
 *
 * Task F-relaunch (Phase 4b-F) status: F3 (timeout -> poison -> transparent
 * relaunch) is bound below (`f3_timeout_poisons_then_transparently_relaunches`).
 * F4 (crash -> ProcessCrashed -> transparent relaunch) was diagnosed as
 * BLOCKED by an earlier task in this phase (F-relaunch): empirically, a real
 * `BEHAVE_CRASH` crash left NO relaunch path at all under production code as
 * it stood then -- `execute()`'s poison guard only fired on
 * `Cancelled | Timeout`, never `ProcessCrashed`; `TsEngineHost`'s write
 * transport was a `OnceLock` that, once set, could never reset, so no later
 * call ever re-spawned the subprocess even after the child had exited. The
 * user approved fixing this production gap (2026-07-08, task F4-fix): the
 * poison guard now also matches `ProcessCrashed` (`ts_engine.rs::execute`),
 * which additionally calls `TsEngineHost::reset_after_crash`
 * (`ts_process.rs`) to tear down the dead transport/threads so the next
 * `ensure_started()` performs a genuine fresh spawn; `TsEngine::ensure_loaded`
 * also compares the host's spawn generation to detect that a fresh
 * subprocess needs `LoadEngine` resent (a crash wipes the harness's
 * `loadedByPath`/`engineByName` registration, which persists only for the
 * lifetime of one subprocess). F4 is now bound below
 * (`f4_crash_yields_process_crashed_then_transparently_relaunches`). See
 * `.superpowers/sdd/plan4b-task-F4fix-report.md` for the full fix writeup;
 * the original diagnostic trail remains at
 * `.superpowers/sdd/plan4b-task-Frelaunch-report.md`.
 */

// Native-only: TsEngine / TsEngineHost are behind cfg(not(target_arch = "wasm32")).
#![cfg(not(target_arch = "wasm32"))]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use tempfile::TempDir;

use quarto_core::engine::ExecutionContext;
use quarto_core::engine::ts_process::TsEngineHost;
use quarto_core::engine::ts_protocol::{
    EngineProjectContext, FromEngine, HostGlobalConfig, ToEngine, TsDependenciesOptions,
    TsFormatIdentifier, TsFormatInfo, TsMetadataValue, TsPandocIncludes,
};
use quarto_core::project::ProjectContext;
use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_core::stage::Cancellation;
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

/// Build a temp project with the named committed extension installed under
/// `_extensions/`, returning the `TempDir` (keep it alive for the test's
/// duration). Mirrors `echo_engine_e2e.rs::setup_project` /
/// `synth_engines_e2e.rs::setup_project`.
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
/// (`render_to_file` -> `render_document_to_file`, the same entry `quarto
/// render` uses) and return the rendered HTML.
fn render_html(input: &Path) -> String {
    let options = RenderToFileOptions::default();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_to_file(input, "html", &options, runtime).expect("render_to_file");
    std::fs::read_to_string(&result.output_path).expect("read rendered HTML")
}

fn body_excerpt(html: &str) -> String {
    let start = html.find("<body").unwrap_or(0);
    html[start..].chars().take(600).collect()
}

// ── default branch: static Primary claim, no sentinel ────────────────────────
//
// Task A2's binding: proves the `behave` fixture's committed dist/behave.js
// bundle is valid, its `_extension.yml`'s `claims.behave: {kind: primary,
// priority: 1}` resolves, and the engine's DEFAULT `execute()` branch (no
// sentinel present) runs to completion through a real Deno engine-host
// subprocess -- LoadEngine / LaunchEngine / execute round trip, same as
// `synth_engines_e2e.rs::smoke_alpha_registers_and_loads`. Does NOT assert
// the FC-1 sentinel payload (metadata/pandoc/resourceFiles/preserve/
// postProcess) -- that verbatim-value assertion is Phase 4b-F's F6, once a
// consumer exists to read the wire-carried `TsExecuteResult` fields back out
// in a test-observable way.
#[test]
fn smoke_behave_default_branch_executes() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — smoke_behave_default_branch_executes");
        return;
    }
    let tmp = setup_project(&["behave"]);
    let input = tmp.path().join("doc.qmd");
    write_file(
        &input,
        "---\ntitle: Behave Smoke\n---\n\n```{behave}\nhello from behave\n```\n",
    );

    let html = render_html(&input);
    assert!(
        html.contains("BEHAVE_EXECUTED"),
        "rendered HTML must contain BEHAVE_EXECUTED (behave's static \
         Primary(behave) claim resolved, bundle loaded, default execute() \
         branch ran under a real Deno subprocess); got:\n{}",
        body_excerpt(&html)
    );
}

// ── F1: intermediateFiles verb round-trip (real Deno, direct TsEngine entry) ──
//
// `intermediate_files` is a pure prediction (source path -> the engine's
// generated sidecar paths, used to EXCLUDE those paths from the project's
// input-file set -- see `ExecutionEngine::intermediate_files`'s doc comment).
// It is NOT observable in rendered HTML, so -- per the brief -- this drives
// `TsEngine::intermediate_files` directly: `ProjectContext::discover` builds
// the real registry (real Deno `TsEngineHost`, same construction path as the
// A2 smoke test and `echo_engine_e2e.rs`'s teardown test), then
// `registry.get("behave")` hands back the live `Arc<dyn ExecutionEngine>` to
// call `.intermediate_files(&input)` on directly -- no render, no HTML.
//
// Ties to `behave.ts`'s `intermediateFiles(input) => [`${input}.behave-intermediate`]`
// (the exact wire string the Rust sender passes is `input_path.to_string_lossy()`).
//
// revert seam: reverting the sender fn (`ts_engine.rs:~827-860` -- e.g. short-
// circuiting to `Vec::new()` instead of sending `ToEngine::IntermediateFiles`
// and mapping `FromEngine::IntermediateFilesResult` back to `PathBuf`s) makes
// this assertion RED (files absent).
#[test]
fn f1_intermediate_files_round_trip() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — f1_intermediate_files_round_trip");
        return;
    }
    let tmp = setup_project(&["behave"]);
    let input = tmp.path().join("doc.qmd");
    write_file(
        &input,
        "---\ntitle: Behave Intermediate Files\n---\n\n```{behave}\nhello\n```\n",
    );

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&input, runtime.as_ref()).expect("discover project");
    assert!(
        project.registry.has_engine("behave"),
        "behave engine must be registered from the installed extension"
    );
    let behave = project
        .registry
        .get("behave")
        .expect("behave engine still registered after discover");

    let files = behave.intermediate_files(&input);

    let expected = PathBuf::from(format!("{}.behave-intermediate", input.to_string_lossy()));
    assert_eq!(
        files,
        vec![expected],
        "intermediate_files must round-trip behave.ts's \
         `[`${{input}}.behave-intermediate`]` return value verbatim through a real \
         Deno subprocess; got {files:?}"
    );
}

// ── F6: FC-1 result-field carriage (real Deno, direct TsEngine entry) ────────
//
// behave's default (no-sentinel) `execute()` branch populates 5 sentinel FC-1
// fields (metadata / pandoc / resourceFiles / preserve / postProcess) -- the
// frozen A2 contract (`behave.ts`'s default-branch return, confirmed against
// the Task A2 report's field-mapping table). This is CARRIAGE, not
// consumption: assert the sentinel values arrive VERBATIM in the mapped
// `ExecuteResult` -- no downstream consumer exists yet (those land with the
// features that need them). `dependencies` / `engineDependencies` is
// deliberately NOT asserted here -- it is not an F6 field (F5/F-deps' job;
// see behave.ts's default-branch comment + the A2 report's "judgment call"
// section).
//
// Like F1, this bypasses the full render/HTML path: `ProjectContext::discover`
// builds the real registry, `registry.get("behave")` hands back the live
// engine, and `.execute(qmd, &ctx)` is called directly so the mapped
// `ExecuteResult`'s carried-and-ignored fields are test-observable (they are
// NOT surfaced in rendered HTML).
//
// revert seam: reverting the field copies in `map_execute_result`
// (`ts_engine.rs:~514-519` -- e.g. hardcoding `metadata: None, pandoc: None,
// resource_files: Vec::new(), preserve: HashMap::new()` instead of copying
// from the wire `TsExecuteResult`) makes these assertions RED (sentinels
// lost). The `post_process` leg of this same hunk is already pinned by
// `test_map_execute_result_wires_post_process` (`ts_engine.rs:~1851`, a
// mock-transport unit test); this test is the real-Deno carriage proof for
// the other four fields, plus a real-subprocess confirmation of
// `post_process` too.
#[test]
fn f6_fc1_result_field_carriage() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — f6_fc1_result_field_carriage");
        return;
    }
    let tmp = setup_project(&["behave"]);
    let input = tmp.path().join("doc.qmd");
    write_file(
        &input,
        "---\ntitle: Behave FC-1\n---\n\n```{behave}\nhello\n```\n",
    );

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&input, runtime.as_ref()).expect("discover project");
    let behave = project
        .registry
        .get("behave")
        .expect("behave engine must be registered from the installed extension");

    let temp_dir = tmp.path().join("_temp");
    std::fs::create_dir_all(&temp_dir).unwrap();
    let exec_ctx = ExecutionContext::new(temp_dir, tmp.path().to_path_buf(), input.clone(), "html");

    // Plain body markdown, no sentinel words present -> behave.ts's default
    // branch (the one that populates the FC-1 sentinel payload).
    let result = behave
        .execute("hello from the F6 test\n", &exec_ctx)
        .expect("behave's default-branch execute must succeed under real Deno");

    let mut expected_metadata = HashMap::new();
    expected_metadata.insert(
        "behaveFc1".to_string(),
        TsMetadataValue::String("md".to_string()),
    );
    assert_eq!(
        result.metadata,
        Some(expected_metadata),
        "metadata: {{behaveFc1: \"md\"}} must survive the wire round-trip verbatim; got {:?}",
        result.metadata
    );

    let mut expected_pandoc = HashMap::new();
    expected_pandoc.insert(
        "behaveFc1".to_string(),
        TsMetadataValue::String("pandoc".to_string()),
    );
    assert_eq!(
        result.pandoc,
        Some(expected_pandoc),
        "pandoc: {{behaveFc1: \"pandoc\"}} must survive the wire round-trip verbatim; got {:?}",
        result.pandoc
    );

    assert_eq!(
        result.resource_files,
        vec!["behave.res".to_string()],
        "resourceFiles: [\"behave.res\"] must survive the wire round-trip verbatim; got {:?}",
        result.resource_files
    );

    let mut expected_preserve = HashMap::new();
    expected_preserve.insert("BEHAVE_KEY".to_string(), "behave-preserve".to_string());
    assert_eq!(
        result.preserve, expected_preserve,
        "preserve: {{BEHAVE_KEY: \"behave-preserve\"}} must survive the wire round-trip \
         verbatim; got {:?}",
        result.preserve
    );

    assert!(
        result.needs_postprocess,
        "postProcess: true must wire-feed needs_postprocess: true (real-Deno confirmation \
         of the mock-pinned test_map_execute_result_wires_post_process)"
    );
}

// ── F3: timeout -> poison -> transparent relaunch (real Deno, direct TsEngine entry) ──
//
// Cancel and Timeout BOTH poison the cached `LaunchEngineResult` on the Rust
// side (`ts_engine.rs::execute`'s `.inspect_err` on `Cancelled | Timeout`,
// ~:805-815) -- there is no clean-timeout-without-poison. This test drives
// that path for real: a `QUARTO_SLOW` execute under a 1s window times out,
// then a SECOND execute on the SAME `Arc<dyn ExecutionEngine>` handle must
// succeed via `ensure_launched()` transparently re-issuing `LaunchEngine`
// (`ts_engine.rs::ensure_launched`, ~:361-372) to the same live Deno
// subprocess -- no new subprocess spawn, just a fresh engine instance.
//
// THE WITNESS (anti-vacuity crux, mandatory per the task brief): "execute-2
// succeeds" alone is NOT sufficient -- it would pass even if execute-1 never
// actually poisoned anything. This test additionally proves a FRESH
// `LaunchEngine` was issued and genuinely re-ran `behave.ts`'s `launch()`
// function BETWEEN execute-1 and execute-2.
//
// Witness mechanism: neither a Rust-side launch counter nor a subprocess PID
// is observable from an external integration-test crate without a
// production-code change --
//   - `TsEngineHost`'s `spawn_count`/`load_engine_count`/`markdown_for_file_count`
//     fields AND their accessors are `#[cfg(test)]`-gated (ts_process.rs
//     ~:404-411, ~:845-861) -- invisible outside quarto-core's OWN unit-test
//     build; an external `tests/integration/*.rs` binary links the normal
//     (non-`cfg(test)`) rlib and cannot see them. No `launch_engine_count`
//     equivalent exists at all, gated or not.
//   - `TsEngineHost::is_alive()` IS a plain `pub fn`, but `TsEngineHost` is
//     private to `TsEngine`, which is itself reached only through the
//     `Arc<dyn ExecutionEngine>` trait object returned by
//     `registry.get("behave")` -- the trait has no introspection method and
//     no `Any`/downcast escape hatch.
//   - The subprocess PID is logged once (`ts_process.rs:582`) but only at
//     SPAWN time; a poison+relaunch reuses the same live subprocess, so the
//     PID never changes across it -- PID is not a discriminating witness for
//     THIS scenario even where it is observable.
// So this test instruments the TEST FIXTURE (`behave.ts`, not production
// code) with a module-level launch counter, logged via `console.error` on
// every real `launch()` call (`behave.ts`'s `launch()`, `dist/behave.js`
// rebuilt via `cargo run --bin q2 -- call build-ts-extension
// crates/quarto-core/tests/fixtures/extensions/behave --workspace`). That
// stderr line reaches the test through an EXISTING, UNMODIFIED production
// mechanism: `ts_process.rs::stderr_loop` already forwards every child
// stderr line to `tracing::info!(target: "engine_host", "{}", line)`
// (~:1116) -- the same "engine_host" target J6/J8/J9
// (`julia_engine_e2e.rs`/`echo_engine_e2e.rs`/`ts_process.rs`) already use as
// the designed cross-crate observability surface for subprocess lifecycle
// events. This test's capture layer additionally visits the event's
// `message` field (those precedents only counted by `target()`) to
// discriminate "BEHAVE_LAUNCH_MARKER:1" (execute-1's OWN initial launch)
// from "BEHAVE_LAUNCH_MARKER:2" (the relaunch). Because `stderr_loop` runs on
// a background thread that does NOT inherit a `with_default`-scoped
// thread-local dispatcher (confirmed empirically -- see the Frelaunch task
// report), the capture is installed via `tracing::dispatcher::set_global_default`
// rather than `tracing::subscriber::with_default`; safe here because nextest
// runs each `#[test]` in its own process (no cross-test interference from a
// process-global dispatcher).
//
// Exercised-guard: execute-2's returned markdown must show behave's default
// branch actually ran (`BEHAVE_EXECUTED`), so "succeeds" isn't a vacuous Ok
// with empty/wrong content.
//
// revert seam: reverting EITHER (a) the poison guard in `ts_engine.rs::execute`
// (~:805-815, e.g. narrowing `Cancelled | Timeout` to just `Cancelled`) OR
// (b) `stashedContextByName` reconstruction in `host.ts` (~:600-628, e.g.
// short-circuiting Step 0 to always throw `engine not launched`) makes
// execute-2 fail (RED) -- (a) also drops the relaunch WITNESS (no second
// `LaunchEngine` -> no `BEHAVE_LAUNCH_MARKER:2`), (b) preserves the witness
// (Rust still issues a fresh `LaunchEngine`) but execute-2 itself goes RED
// via a TS-side `engine not launched` error, not `Ok`.
#[test]
fn f3_timeout_poisons_then_transparently_relaunches() {
    use quarto_core::engine::ExecutionError;
    use std::time::Duration;
    use tracing_subscriber::layer::SubscriberExt;

    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — f3_timeout_poisons_then_transparently_relaunches");
        return;
    }
    let tmp = setup_project(&["behave"]);
    let input = tmp.path().join("doc.qmd");
    write_file(
        &input,
        "---\ntitle: Behave Timeout Relaunch\n---\n\nplaceholder\n",
    );

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&input, runtime.as_ref()).expect("discover project");
    let behave = project
        .registry
        .get("behave")
        .expect("behave engine must be registered from the installed extension");

    let temp_dir = tmp.path().join("_temp");
    std::fs::create_dir_all(&temp_dir).unwrap();

    let capture = LaunchMarkerCapture::default();
    let subscriber = tracing_subscriber::registry().with(capture.clone());
    // GLOBAL default (not thread-scoped `with_default`): `stderr_loop`
    // forwards the fixture's `console.error` lines on a background thread
    // spawned by `ensure_started_inner`, which does not inherit a
    // `with_default` thread-local scope. Safe under nextest's one-process-
    // per-test isolation. See the module comment above for the empirical
    // confirmation.
    tracing::dispatcher::set_global_default(tracing::Dispatch::new(subscriber))
        .expect("install global tracing dispatch for engine_host capture");

    // Execute-1: QUARTO_SLOW sentinel under a 1s window -- times out well
    // before behave.ts's 60s branch would ever resolve on its own.
    let ctx1 = ExecutionContext::new(
        temp_dir.clone(),
        tmp.path().to_path_buf(),
        input.clone(),
        "html",
    )
    .with_execute_timeout(Some(Duration::from_secs(1)));
    let r1 = behave.execute("QUARTO_SLOW\n", &ctx1);

    // Assertion 1: first execute times out.
    assert!(
        matches!(r1, Err(ExecutionError::Timeout { .. })),
        "expected Err(Timeout) from the QUARTO_SLOW execute under a 1s window; got {r1:?}"
    );

    // Give the background stderr-forwarding thread a moment to deliver
    // execute-1's own launch marker before checking the pre-execute-2 count.
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        capture.count_containing("BEHAVE_LAUNCH_MARKER:1"),
        1,
        "expected exactly one initial launch (execute-1's own ensure_launched); \
         captured engine_host messages: {:?}",
        capture.all()
    );
    assert_eq!(
        capture.count_containing("BEHAVE_LAUNCH_MARKER:2"),
        0,
        "no relaunch should have happened yet -- this assertion, together with the \
         post-execute-2 check below, proves the relaunch is genuinely bracketed \
         between execute-1 and execute-2, not an artifact of execute-1's own launch; \
         captured engine_host messages: {:?}",
        capture.all()
    );

    // Execute-2: plain input (no sentinel), generous timeout -- must succeed
    // via transparent relaunch of the poisoned instance. A real `{behave}`
    // fence (not bare text) so the default branch's regex-replace has
    // something to transform -- required for the exercised-guard below.
    let ctx2 = ExecutionContext::new(temp_dir, tmp.path().to_path_buf(), input, "html")
        .with_execute_timeout(Some(Duration::from_secs(30)));
    let r2 = behave.execute("```{behave}\nhello from F3\n```\n", &ctx2);

    // THE WITNESS: a fresh `LaunchEngine` genuinely re-ran `launch()` between
    // execute-1 and execute-2 -- not just that Rust's local cache was
    // cleared. Without this, "execute-2 succeeds" is vacuous theater: it
    // would pass identically even if execute-1 never poisoned anything.
    // Bounded poll, not a bare count: the marker is delivered by the
    // background stderr forwarder, so counting the instant `execute()`
    // returns races it. See `wait_for_count_containing`.
    let relaunches =
        capture.wait_for_count_containing("BEHAVE_LAUNCH_MARKER:2", 1, Duration::from_secs(10));
    assert_eq!(
        relaunches,
        1,
        "expected exactly one RELAUNCH (a second, independent `launch()` call) \
         between execute-1 and execute-2; captured engine_host messages: {:?}",
        capture.all()
    );

    // Assertion 3 (+ exercised-guard): second execute succeeds, and its
    // content proves behave's default branch actually ran.
    let result2 =
        r2.expect("second execute must succeed via transparent relaunch after Timeout poison");
    assert!(
        result2.markdown.contains("BEHAVE_EXECUTED"),
        "exercised-guard: second execute's markdown must show behave's default \
         branch actually ran (not a vacuous/empty Ok); got: {}",
        result2.markdown
    );
}

// ── F4: crash -> ProcessCrashed -> transparent relaunch (real Deno, direct TsEngine entry) ──
//
// Companion to F3, but for a DEAD process instead of a merely-slow one.
// `BEHAVE_CRASH` makes behave.ts write a stderr marker then call
// `Deno.exit(1)` mid-`execute()` (mirrors echo-engine's t13 crash-detection
// test, `echo_engine_e2e.rs::t13_crash_mid_execute_yields_process_crashed_with_stderr`
// -- t13 proves DETECTION only, never attempts a second execute). This test
// adds the RELAUNCH half: execute-1 crashes (`Err(ProcessCrashed)`),
// execute-2 on the SAME `Arc<dyn ExecutionEngine>` handle must transparently
// respawn a FRESH Deno subprocess, re-load the engine module, re-launch the
// engine instance, and succeed.
//
// WHY THIS TEST'S WITNESS SHAPE DIFFERS FROM F3'S "marker :1 then :2":
// F3's timeout/cancel poison reuses the SAME live subprocess -- behave.ts's
// module-level `_behaveLaunchCount` survives across the poison, so a genuine
// relaunch increments it to a NEW value (":2"). A CRASH kills the entire
// Deno process, wiping ALL of its in-memory state -- `_behaveLaunchCount`
// resets to 0 in the fresh process, so a genuine relaunch here produces
// ANOTHER "BEHAVE_LAUNCH_MARKER:1", not ":2". A naive `count_containing(":2")
// == 0` assertion would therefore be satisfied VACUOUSLY even if crash
// recovery were completely broken (no relaunch attempted at all -- absence
// of evidence, not evidence of anything). This test instead uses TWO
// independent, non-vacuous witnesses that together can only both be
// satisfied by a GENUINE fresh-subprocess relaunch:
//   1. `count_containing("BEHAVE_LAUNCH_MARKER:1") == 2` -- TWO independent
//      processes each ran their OWN first `launch()` call (proves the
//      engine was launch()'d twice, not that Rust's cache was merely
//      cleared without a real round trip).
//   2. `count_containing("engine-host spawned") == 2` -- TWO real subprocess
//      spawns (this is production Rust code's OWN observability event,
//      `ts_process.rs::ensure_started_inner` ~:582, already used by J6/J8/J9
//      -- an entirely independent signal from the TS fixture's counter).
// Both must hold; neither alone rules out every "vacuous pass" shape (e.g.
// (1) alone doesn't rule out a same-process double-launch some other way;
// (2) alone doesn't rule out a fresh process that never actually re-ran the
// engine's `launch()`).
//
// Exercised-guard: execute-2's returned markdown must show behave's default
// branch actually ran (`BEHAVE_EXECUTED`), so "succeeds" isn't vacuous.
//
// revert seam: reverting EITHER (a) the poison guard in `ts_engine.rs::execute`
// (adding `ProcessCrashed` alongside `Cancelled | Timeout`) OR (b) the
// transport-reset logic (`TsEngineHost::reset_after_crash`, called from that
// same poison guard) makes execute-2 fail (RED) -- without (a), `execute()`
// never poisons the instance or resets the transport on `ProcessCrashed`, so
// execute-2 tries to reuse the dead transport directly and fails with a
// transport-level error ("Broken pipe" / "transport send error") -- NOT
// `Ok`, and NEITHER witness increments (no second spawn, no second launch).
// Without (b) alone (poison guard fires but transport never resets),
// `ensure_started()`'s fast path still sees a (dead) transport as "already
// started" and never respawns -- same dead-transport failure on execute-2.
#[test]
fn f4_crash_yields_process_crashed_then_transparently_relaunches() {
    use quarto_core::engine::ExecutionError;
    use std::time::Duration;
    use tracing_subscriber::layer::SubscriberExt;

    if !deno_available() {
        eprintln!(
            "SKIP: deno not on PATH — f4_crash_yields_process_crashed_then_transparently_relaunches"
        );
        return;
    }
    let tmp = setup_project(&["behave"]);
    let input = tmp.path().join("doc.qmd");
    write_file(
        &input,
        "---\ntitle: Behave Crash Relaunch\n---\n\nplaceholder\n",
    );

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&input, runtime.as_ref()).expect("discover project");
    let behave = project
        .registry
        .get("behave")
        .expect("behave engine must be registered from the installed extension");

    let temp_dir = tmp.path().join("_temp");
    std::fs::create_dir_all(&temp_dir).unwrap();

    let capture = LaunchMarkerCapture::default();
    let subscriber = tracing_subscriber::registry().with(capture.clone());
    // GLOBAL default (not thread-scoped `with_default`): the "engine-host
    // spawned" event fires synchronously on the CALLING thread inside
    // `ensure_started_inner`, while `BEHAVE_LAUNCH_MARKER`/`BEHAVE_CRASH_MARKER`
    // arrive via the background stderr-forwarding thread -- neither inherits
    // a `with_default` thread-local scope. Same rationale as F3.
    tracing::dispatcher::set_global_default(tracing::Dispatch::new(subscriber))
        .expect("install global tracing dispatch for engine_host capture");

    // Execute-1: BEHAVE_CRASH sentinel -- crashes mid-execute, AFTER
    // ensure_launched()'s own LaunchEngine (marker :1 in process #1) already
    // succeeded.
    let ctx1 = ExecutionContext::new(
        temp_dir.clone(),
        tmp.path().to_path_buf(),
        input.clone(),
        "html",
    )
    .with_execute_timeout(Some(Duration::from_secs(30)));
    let r1 = behave.execute("BEHAVE_CRASH\n", &ctx1);

    // Assertion 1: first execute observes the crash as ProcessCrashed (not
    // Timeout, not a generic error, not Ok).
    assert!(
        matches!(r1, Err(ExecutionError::ProcessCrashed { .. })),
        "expected Err(ProcessCrashed) from the BEHAVE_CRASH execute; got {r1:?}"
    );

    // Give the background stderr-forwarding thread a moment to deliver
    // process #1's launch marker + crash marker before checking the
    // pre-execute-2 counts. `handle_crash` itself already sleeps ~250ms
    // waiting for the stderr thread to drain before broadcasting
    // ProcessCrashed, so this is a small top-up, not the primary wait.
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        capture.count_containing("BEHAVE_CRASH_MARKER"),
        1,
        "expected the crash marker behave.ts writes immediately before \
         Deno.exit(1); captured engine_host messages: {:?}",
        capture.all()
    );
    assert_eq!(
        capture.count_containing("BEHAVE_LAUNCH_MARKER:1"),
        1,
        "expected exactly one initial launch in process #1 (execute-1's own \
         ensure_launched, before the crash); captured engine_host messages: {:?}",
        capture.all()
    );
    assert_eq!(
        capture.count_containing("engine-host spawned"),
        1,
        "expected exactly one real subprocess spawn so far (process #1); no \
         relaunch should have happened yet; captured engine_host messages: {:?}",
        capture.all()
    );

    // Execute-2: plain `{behave}` input (no sentinel), generous timeout --
    // must succeed via a transparent CRASH relaunch: fresh subprocess, fresh
    // LoadEngine, fresh LaunchEngine, then a successful Execute.
    let ctx2 = ExecutionContext::new(temp_dir, tmp.path().to_path_buf(), input, "html")
        .with_execute_timeout(Some(Duration::from_secs(30)));
    let r2 = behave.execute("```{behave}\nhello from F4\n```\n", &ctx2);

    // WITNESS 1: a second, independent real subprocess spawn happened --
    // production Rust's OWN observability event (ts_process.rs ~:582),
    // entirely independent of the TS fixture's own counter.
    assert_eq!(
        capture.count_containing("engine-host spawned"),
        2,
        "expected exactly TWO real subprocess spawns (process #1, then a \
         fresh process #2 after the crash); captured engine_host messages: {:?}",
        capture.all()
    );

    // WITNESS 2: TWO independent processes each ran their OWN first
    // `launch()` call -- process #2's `_behaveLaunchCount` starts fresh at 0
    // (crash wiped process #1's in-memory state), so a genuine relaunch
    // prints "BEHAVE_LAUNCH_MARKER:1" AGAIN, not ":2" (see the module
    // comment above for why this differs from F3's witness shape).
    assert_eq!(
        capture.count_containing("BEHAVE_LAUNCH_MARKER:1"),
        2,
        "expected TWO independent first-launches (one per process) between \
         execute-1 and execute-2; captured engine_host messages: {:?}",
        capture.all()
    );

    // Assertion 3 (+ exercised-guard): second execute succeeds, and its
    // content proves behave's default branch actually ran on the fresh
    // process.
    let result2 =
        r2.expect("second execute must succeed via transparent relaunch after ProcessCrashed");
    assert!(
        result2.markdown.contains("BEHAVE_EXECUTED"),
        "exercised-guard: second execute's markdown must show behave's default \
         branch actually ran (not a vacuous/empty Ok); got: {}",
        result2.markdown
    );
}

// ── F5: Dependencies verb round-trip (real Deno, direct TsEngineHost entry) ──
//
// The `Dependencies` protocol verb (`ts_protocol.rs:~84-93` / `~403-465`) has
// NO production Rust sender: `ExecutionEngine` (`traits.rs`) exposes no
// `dependencies()` method, and `TsEngine::execute`'s `FromEngine` response
// match treats `DependenciesResult` as an unexpected variant via its
// catch-all arm. `test_fc2_dependencies_verb_round_trip` (`ts_protocol.rs`,
// serde-only) already proves the wire SHAPE round-trips through
// `serde_json::Value`. F5 adds the missing leg: a LIVE `behave` engine
// actually responding to a `Dependencies` request over the real transport.
//
// Reachability: since there is no `pub` sender on `TsEngine`/`ExecutionEngine`
// and `Arc<dyn ExecutionEngine>` has no downcast, this does NOT go through
// the registry (unlike F1/F6's `.intermediate_files()`/`.execute()`, which
// ride pub `ExecutionEngine` trait methods). Instead it drives
// `TsEngineHost` (`ts_process.rs`) DIRECTLY — `new`, `load_engine`,
// `launch_engine`, and `request` are all plain `pub fn`, none
// `#[cfg(test)]`-gated (unlike `with_transport`/`start_with_command`, which
// ARE `#[cfg(test)]`-only and therefore invisible to this external
// integration-test crate). This is the SAME production spawn path
// `TsEngine`/the registry use under the hood (`ensure_started`'s
// `extracted_bundle_path()` + `deno run --allow-all <bundle>`), just called
// without the `TsEngine` wrapper, because `TsEngine` has no `Dependencies`
// verb to expose. No production code was added or changed to make this
// reachable.
//
// behave.ts's `dependencies()` handler (`behave.ts:244-247`) is a thin
// passthrough: `return { includes: {} }` — no supporting-file paths. Q2's
// render orchestrator does NOT consume `Dependencies` yet (RTQ FC-2,
// deferred to the book feature), so this is a pure WIRE round-trip proof,
// not a consumer test — it asserts the verb round-trips and behave's
// response arrives, not that anything acts on it.
//
// revert seam: `host.ts`'s `case "dependencies":` handler (`host.ts:~798–880`)
// is the wire-side seam. Commenting out its terminal
// `await writeFrame(writer, { id, msg: { type: "dependenciesResult", ... } })`
// (or the whole case arm) makes the harness never respond to this request's
// `id` — `host.request(...)` below then times out (`ExecutionError::Timeout`)
// instead of returning `Ok(FromEngine::DependenciesResult { .. })` → RED.
#[test]
fn f5_dependencies_verb_round_trip() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — f5_dependencies_verb_round_trip");
        return;
    }
    let tmp = setup_project(&["behave"]);
    let engine_path = tmp
        .path()
        .join("_extensions")
        .join("behave")
        .join("dist")
        .join("behave.js");

    let global = HostGlobalConfig {
        resource_dir: "/res".to_string(),
        runtime_dir: "/rt".to_string(),
        data_dir: "/data".to_string(),
        pandoc_path: None,
        is_interactive_session: false,
        running_in_ci: false,
        quarto_version: quarto_core::version().to_string(),
    };
    let host = TsEngineHost::new(global);
    let cancellation = Cancellation::new();

    host.load_engine(&engine_path, &cancellation)
        .expect("LoadEngine must succeed against the real committed behave.js bundle");
    host.launch_engine("behave", EngineProjectContext::default(), &cancellation)
        .expect("LaunchEngine must succeed for behave under a real Deno subprocess");

    let options = TsDependenciesOptions {
        input: "# Hello".to_string(),
        source_path: "/project/doc.qmd".to_string(),
        source_map: vec![],
        format: TsFormatInfo {
            identifier: TsFormatIdentifier {
                base_format: "html".to_string(),
                target_format: "html".to_string(),
                display_name: "HTML".to_string(),
                extension_name: None,
            },
            metadata: HashMap::new(),
        },
        output: "/project/doc.html".to_string(),
        temp_dir: tmp.path().to_string_lossy().into_owned(),
        lib_dir: None,
        project_dir: None,
        dependencies: vec![],
        quiet: false,
    };
    let msg = ToEngine::Dependencies {
        engine: "behave".to_string(),
        options,
    };

    let response = host
        .request(msg, Some(Duration::from_secs(10)), &cancellation)
        .expect("Dependencies request must round-trip against live behave.ts");

    match response {
        FromEngine::DependenciesResult { includes } => {
            // Exercised-guard: behave.ts's dependencies() (behave.ts:244-247)
            // returns `{ includes: {} }` verbatim -- an empty includes object,
            // no supporting-file paths. Proves the response was genuinely
            // produced by the live handler (not a vacuous default/mock).
            assert_eq!(
                includes,
                TsPandocIncludes {
                    in_header: None,
                    before_body: None,
                    after_body: None,
                },
                "behave.ts's dependencies() must return `{{ includes: {{}} }}` \
                 verbatim through the real wire round-trip; got {includes:?}"
            );
        }
        other => {
            panic!("expected FromEngine::DependenciesResult from live behave.ts, got {other:?}")
        }
    }
}

/// A tracing-capture layer recording, for every `target: "engine_host"`
/// event, its `message` field text (unlike the target-only `TargetCapture`/
/// `OrderedCapture` helpers in `julia_engine_e2e.rs`/`echo_engine_e2e.rs`,
/// which only count occurrences and cannot discriminate WHICH child-stderr
/// line fired). Shared across threads via `Arc<Mutex<Vec<String>>>` — reader
/// threads deliver from the background stderr-forwarding thread (see the F3
/// module comment on why `set_global_default` is required here instead of
/// the `with_default` thread-scoping those siblings use).
#[derive(Clone, Default)]
struct LaunchMarkerCapture {
    messages: Arc<std::sync::Mutex<Vec<String>>>,
}

struct MessageFieldVisitor<'a> {
    out: &'a mut Option<String>,
}

impl<'a> tracing::field::Visit for MessageFieldVisitor<'a> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            *self.out = Some(format!("{value:?}"));
        }
    }
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for LaunchMarkerCapture {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if event.metadata().target() != "engine_host" {
            return;
        }
        let mut msg = None;
        event.record(&mut MessageFieldVisitor { out: &mut msg });
        if let Some(msg) = msg {
            self.messages.lock().unwrap().push(msg);
        }
    }
}

impl LaunchMarkerCapture {
    fn count_containing(&self, needle: &str) -> usize {
        self.messages
            .lock()
            .unwrap()
            .iter()
            .filter(|m| m.contains(needle))
            .count()
    }

    /// Wait up to `timeout` for `needle` to appear at least `expected` times,
    /// then let the stream settle briefly and return the final count.
    ///
    /// Launch markers reach this capture ASYNCHRONOUSLY: the fixture writes
    /// them to stderr via `console.error`, and `stderr_loop` forwards them
    /// into `tracing` from a background thread. A bare `count_containing()`
    /// therefore races that forwarder. That race is not hypothetical — it is
    /// how `f3_timeout_poisons_then_transparently_relaunches` failed on a
    /// loaded ubuntu CI runner (0 relaunches observed) while passing on
    /// macos-latest in the same run, on the same commit (2026-08-18).
    ///
    /// Polling instead of sleeping a fixed interval keeps the common case
    /// fast, and the binding the caller's assertion exists for is preserved
    /// rather than weakened: if the relaunch genuinely never happens, this
    /// still returns the wrong count and the assertion still fires. The
    /// trailing settle keeps the caller's `== expected` upper bound
    /// meaningful, so a spurious EXTRA relaunch is still caught — which a
    /// bare `count_containing()` could also have missed.
    fn wait_for_count_containing(
        &self,
        needle: &str,
        expected: usize,
        timeout: std::time::Duration,
    ) -> usize {
        let deadline = std::time::Instant::now() + timeout;
        while self.count_containing(needle) < expected {
            if std::time::Instant::now() >= deadline {
                return self.count_containing(needle);
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        // Settle: give a spurious extra marker the same chance to land that
        // the fixed 200ms sleep before the marker-1 check gives it.
        std::thread::sleep(std::time::Duration::from_millis(200));
        self.count_containing(needle)
    }

    fn all(&self) -> Vec<String> {
        self.messages.lock().unwrap().clone()
    }
}
