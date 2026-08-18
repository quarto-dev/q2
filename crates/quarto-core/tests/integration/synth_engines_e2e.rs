/*
 * tests/integration/synth_engines_e2e.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Plan 4b Phase A / Task A1 — resolution-shaped synthetic engine fixtures.
 *
 * Plan 4b validates the TS-engine subsystem's "shadow features" — code that
 * ships but that a single-Primary Julia render never exercises. The
 * resolution tier model (Primary / Interop / explicit+implicit Fallback /
 * whenClass / contribution_order tiebreak) is only observable with >=2
 * contending engines. THIS file is Task A1's smoke test: it proves each of
 * the well-formed synthetic fixtures under
 * `crates/quarto-core/tests/fixtures/extensions/` is a valid, loadable
 * TsEngine bundle (real `deno bundle` output, real `_extension.yml` parse) by
 * driving each ONE fixture at a time through the REAL render path —
 * `render_to_file` -> discovery -> `resolve_engines` -> LoadEngine /
 * LaunchEngine / execute -> a real Deno engine-host subprocess.
 *
 * Deliberately narrow scope: each test below installs exactly ONE synthetic
 * extension, so no fixture ever contends with another here — the tier
 * MATRIX (multiple contending engines, `contribution_order` / `engines:`
 * tiebreak, T3 presence-gating across engines, etc.) is Phase 4b-B's job,
 * not this file's. Harness mirrors `echo_engine_e2e.rs`.
 *
 * `mismatch` and `content-claim` are NOT smoke-tested by the tests above:
 * - `mismatch` statically declares one claim shape but its dynamic
 *   `claimsLanguage` disagrees — bound below by `b10_static_dynamic_mismatch_hard_errors`,
 *   which drives the real static-vs-dynamic validation hard-error in
 *   `TsEngine::ensure_loaded` (`crates/quarto-core/src/engine/ts_engine.rs`).
 * - `content-claim` omits a static `claims-files` declaration and relies on
 *   a content-inspecting dynamic `claims_file` — bound below by
 *   `b11_dynamic_claims_file_round_trip`, which drives both the positive
 *   (marker present → claimed) and negative (marker absent → not claimed)
 *   directions of the real `ClaimsFile` wire dispatch.
 * Both fixtures' `dist/<name>.js` bundles are regenerated at test time (not
 * committed) via `crate::engine_fixture_build`: they import only
 * `@quarto/api/claims` (pure, type-only transitive deps), so `deno bundle`
 * resolves hermetically from local `ts-packages/` with no network/lock —
 * real deno bundle output, built fresh into each test's tempdir copy.
 */

// Native-only: TsEngine / TsEngineHost are behind cfg(not(target_arch = "wasm32")).
#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use tempfile::TempDir;

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

/// Build a temp project with the named committed extension installed under
/// `_extensions/`, returning the `TempDir` (keep it alive for the test's
/// duration). Mirrors `echo_engine_e2e.rs::setup_project`.
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

// ── alpha: static Primary claim (synth, priority 1) ──────────────────────────
//
// Proves the `alpha` fixture's committed dist/alpha.js bundle is valid and its
// _extension.yml's `claims.synth: { kind: primary, priority: 1 }` parses and
// resolves: a `{synth}` cell must be claimed, loaded, and executed by alpha
// (real Deno subprocess LoadEngine/LaunchEngine/execute round trip).
#[test]
fn smoke_alpha_registers_and_loads() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — smoke_alpha_registers_and_loads");
        return;
    }
    let tmp = setup_project(&["alpha"]);
    let input = tmp.path().join("doc.qmd");
    write_file(
        &input,
        "---\ntitle: Alpha Smoke\n---\n\n```{synth}\nhello from alpha\n```\n",
    );

    let html = render_html(&input);
    assert!(
        html.contains("ALPHA_EXECUTED"),
        "rendered HTML must contain ALPHA_EXECUTED (alpha's static Primary(synth) \
         claim resolved, bundle loaded, cell executed); got:\n{}",
        body_excerpt(&html)
    );
}

// ── beta: static Primary claim (synth, priority 1) — identical shape to alpha ─
//
// beta is smoke-tested in ISOLATION (no alpha installed alongside it), so this
// proves beta's own bundle/yml are independently valid without exercising the
// alpha/beta contention pair — that tiebreak is Phase 4b-B's job.
#[test]
fn smoke_beta_registers_and_loads() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — smoke_beta_registers_and_loads");
        return;
    }
    let tmp = setup_project(&["beta"]);
    let input = tmp.path().join("doc.qmd");
    write_file(
        &input,
        "---\ntitle: Beta Smoke\n---\n\n```{synth}\nhello from beta\n```\n",
    );

    let html = render_html(&input);
    assert!(
        html.contains("BETA_EXECUTED"),
        "rendered HTML must contain BETA_EXECUTED (beta's static Primary(synth) \
         claim resolved, bundle loaded, cell executed); got:\n{}",
        body_excerpt(&html)
    );
}

// ── interop-r: static Primary claim (rsynth, priority 1) ─────────────────────
//
// Proves interop-r's bundle/yml are valid via its Primary("rsynth") claim.
// The presence-gated Interop("pysynth") half (T3: only extends ownership when
// interop-r is already `present` via its rsynth Primary claim in the SAME
// document) is a resolution-tier assertion — Phase 4b-B's job, not this
// smoke test's.
#[test]
fn smoke_interop_r_registers_and_loads() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — smoke_interop_r_registers_and_loads");
        return;
    }
    let tmp = setup_project(&["interop-r"]);
    let input = tmp.path().join("doc.qmd");
    write_file(
        &input,
        "---\ntitle: Interop R Smoke\n---\n\n```{rsynth}\nhello from interop-r\n```\n",
    );

    let html = render_html(&input);
    assert!(
        html.contains("INTEROP_R_RSYNTH_EXECUTED"),
        "rendered HTML must contain INTEROP_R_RSYNTH_EXECUTED (interop-r's static \
         Primary(rsynth) claim resolved, bundle loaded, cell executed); got:\n{}",
        body_excerpt(&html)
    );
}

// ── fallback-univ: universal Fallback claim (`claims.fallback`, priority 5) ──
//
// Proves fallback-univ's bundle/yml are valid: an otherwise-unclaimed
// language ("fsynth" — no built-in or fixture engine statically claims it)
// falls through to fallback-univ's universal Fallback claim (T4 implicit
// fallback). Priority 5 deterministically beats the built-in JupyterEngine's
// universal Fallback(0), so this does not depend on jupyter/knitr
// availability on the test machine.
#[test]
fn smoke_fallback_univ_registers_and_loads() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — smoke_fallback_univ_registers_and_loads");
        return;
    }
    let tmp = setup_project(&["fallback-univ"]);
    let input = tmp.path().join("doc.qmd");
    write_file(
        &input,
        "---\ntitle: Fallback Univ Smoke\n---\n\n```{fsynth}\nhello from fallback-univ\n```\n",
    );

    let html = render_html(&input);
    assert!(
        html.contains("FALLBACK_UNIV_EXECUTED_fsynth"),
        "rendered HTML must contain FALLBACK_UNIV_EXECUTED_fsynth (fallback-univ's \
         universal `claims.fallback` claim resolved, bundle loaded, cell executed); \
         got:\n{}",
        body_excerpt(&html)
    );
}

// ── whenclass-marimo: whenClass-conditioned Primary claim (pysynth, .marimo) ─
//
// Proves whenclass-marimo's bundle/yml are valid: `{pysynth .marimo}` (first
// class == "marimo") is claimed and executed. The negative case (bare
// `{pysynth}`, no `.marimo` first class, must NOT be claimed) is a
// resolution-tier assertion — Phase 4b-B's job, not this smoke test's.
#[test]
fn smoke_whenclass_marimo_registers_and_loads() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — smoke_whenclass_marimo_registers_and_loads");
        return;
    }
    let tmp = setup_project(&["whenclass-marimo"]);
    let input = tmp.path().join("doc.qmd");
    write_file(
        &input,
        "---\ntitle: WhenClass Marimo Smoke\n---\n\n```{pysynth .marimo}\nhello from whenclass-marimo\n```\n",
    );

    let html = render_html(&input);
    assert!(
        html.contains("WHENCLASS_MARIMO_EXECUTED"),
        "rendered HTML must contain WHENCLASS_MARIMO_EXECUTED (whenclass-marimo's \
         static whenClass-conditioned Primary(pysynth) claim resolved, bundle \
         loaded, cell executed); got:\n{}",
        body_excerpt(&html)
    );
}

// ── B10: static-vs-dynamic mismatch → execute-time load hard-error ───────────
//
// `mismatch`'s `_extension.yml` statically declares `claims.synth: { kind:
// primary }` (Primary(1)), but its dynamic `claimsLanguage("synth")` returns
// `interop()` (Interop(0)) — see the fixture's src/mismatch.ts header. This
// drives the REAL `TsEngine::ensure_loaded` static-vs-dynamic validation
// (`crates/quarto-core/src/engine/ts_engine.rs:243-296`): rendering a
// document with a `{synth}` cell forces a first execute-time load, which
// issues the real `ClaimsLanguage` wire probe against the now-loaded module
// and compares it to the recorded static answer.
//
// revert seam: comment out (or short-circuit to a no-op) the
// `if static_claim != dynamic_claim { return Err(...) }` block at
// `ts_engine.rs:287-294` (inside the `if !static_language_answers.is_empty()
// ...` block starting at line 251) ⇒ the mismatch is never detected ⇒ this
// test's `is_err()` assertion goes RED (the render succeeds instead, since
// `mismatch`'s dynamic `execute()` still transforms the `{synth}` cell into
// MISMATCH_EXECUTED regardless of the claim mismatch).
//
// Note: this is the RE (real-engine) leg. The RU (resolution-unit) leg —
// asserting `lookup_static_claim` / `combine_claims` recompute the same
// static answer the guard compares against — is already pinned by P1-13 at
// `ts_engine.rs:~2268`; this test does not duplicate that unit coverage.
#[test]
fn b10_static_dynamic_mismatch_hard_errors() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — b10_static_dynamic_mismatch_hard_errors");
        return;
    }
    let tmp = setup_project(&["mismatch"]);
    let input = tmp.path().join("doc.qmd");
    write_file(
        &input,
        "---\ntitle: Mismatch B10\n---\n\n```{synth}\nhello from mismatch\n```\n",
    );

    let options = RenderToFileOptions::default();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_to_file(&input, "html", &options, runtime);

    assert!(
        result.is_err(),
        "a static-vs-dynamic claim mismatch must hard-error the render, not \
         silently execute; got Ok"
    );
    let msg = format!("{:#}", result.unwrap_err());
    // Observed shipped message (ts_engine.rs:288-293) DOES name the specific
    // static claim ("Primary(1)"), the dynamic claim ("Interop(0)"), the
    // engine name, and directs the author to "_extension.yml" — assert that
    // shape rather than paraphrasing it. Message wording itself is OUT OF
    // SCOPE for Plan 4b (do not "improve" the guard to make this nicer).
    assert!(
        msg.contains("mismatch") && msg.contains("synth"),
        "error must name the engine ('mismatch') and the mismatched language \
         ('synth'); got: {msg}"
    );
    assert!(
        msg.contains("Primary(1)") && msg.contains("Interop(0)"),
        "error must name both the static claim (Primary(1)) and the dynamic \
         claim (Interop(0)) it disagreed with; got: {msg}"
    );
    assert!(
        msg.contains("_extension.yml"),
        "error must point the author at _extension.yml as the place to \
         reconcile the mismatch; got: {msg}"
    );
}

// ── B11: dynamic `claims_file` round-trip (content-inspecting whole-file claim) ─
//
// `content-claim` omits a static `claims-files:` declaration for `.syn`, so
// whole-file `.syn` ownership falls to its dynamic `claimsFile`, which
// inspects file CONTENT: it claims a `.syn` file whose first line is exactly
// `# synth-claim`, and declines otherwise (see the fixture's
// src/content-claim.ts header). This drives the REAL
// `SourceConversionStage` -> `TsEngine::claims_file` -> `ClaimsFile` wire
// dispatch (`crates/quarto-core/src/engine/ts_engine.rs:762-786`, the
// content-inspecting fallback branch reached when `self.claims_files` is
// `None`).
//
// Both directions are asserted so this isn't vacuous (a test that only
// checked the positive would still pass if the engine claimed every `.syn`
// file unconditionally):
// - WITH the `# synth-claim` marker: the file is claimed, converted, and
//   executed — CONTENT_CLAIM_EXECUTED appears in the rendered HTML.
// - WITHOUT the marker: no engine claims `.syn` (content-claim declines;
//   nothing else in this single-fixture project owns the extension), so
//   `SourceConversionStage` hard-errors with "Can't determine execution
//   engine" (`source_conversion.rs:24`) rather than silently rendering as
//   plain text or falling through to some other engine.
//
// revert seam: in `TsEngine::claims_file`'s dynamic branch
// (`ts_engine.rs:762-786`), short-circuit to always return `true` (or always
// `false`) instead of sending the `ToEngine::ClaimsFile` wire message and
// trusting its `result` ⇒ the marker is never consulted ⇒ EITHER direction's
// assertion goes RED (always-`true` reddens the "without marker" leg;
// always-`false` reddens the "with marker" leg).
#[test]
fn b11_dynamic_claims_file_round_trip() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — b11_dynamic_claims_file_round_trip");
        return;
    }

    // ── Positive: marker present → content-claim claims + executes the file.
    {
        let tmp = setup_project(&["content-claim"]);
        let input = tmp.path().join("marked.syn");
        write_file(&input, "# synth-claim\nbody line\n");

        let html = render_html(&input);
        assert!(
            html.contains("CONTENT_CLAIM_EXECUTED"),
            "a .syn file whose first line is '# synth-claim' must be claimed, \
             converted, and executed by content-claim; got:\n{}",
            body_excerpt(&html)
        );
    }

    // ── Negative: marker absent → content-claim does NOT claim the file.
    {
        let tmp = setup_project(&["content-claim"]);
        let input = tmp.path().join("unmarked.syn");
        write_file(&input, "just a plain first line\nbody line\n");

        let options = RenderToFileOptions::default();
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        let result = render_to_file(&input, "html", &options, runtime);

        assert!(
            result.is_err(),
            "a .syn file without the '# synth-claim' marker must NOT be \
             claimed by content-claim (no other engine owns .syn in this \
             fixture, so the render must fail); got Ok"
        );
        let msg = format!("{:#}", result.unwrap_err());
        assert!(
            msg.contains("Can't determine execution engine"),
            "an unclaimed .syn file must fail with SourceConversionStage's \
             'Can't determine execution engine' error, proving no engine \
             (including content-claim) claimed it; got: {msg}"
        );
    }
}
