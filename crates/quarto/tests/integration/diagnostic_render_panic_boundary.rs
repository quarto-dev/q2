//! Panic boundary around per-diagnostic rendering (Task E1,
//! bd-ariadne-config-span-char-boundary-panic-rkqmhzrg Phase 5).
//!
//! `print_render_diagnostics` runs *after* the render it describes
//! has already completed and its output already written (see the
//! call sites in `execute_single_doc` / `execute_project`, both of
//! which call it after `pipeline.run()` and before
//! `should_exit_nonzero`). Before this task, a panic while rendering
//! any *one* queued diagnostic (ariadne text, or JSON conversion
//! under `--json-errors`) would abort the whole process — discarding
//! every diagnostic queued behind it and (had the panic hook not
//! already run to completion) leaving the exit code wrong, even
//! though the render itself had already succeeded and its output was
//! already on disk.
//!
//! The panic this hardens against is not reachable today — Phases
//! 1-4 of the epic already floor the bad source offsets that used to
//! reach `quarto-error-reporting`'s renderers. These tests exercise
//! the boundary itself via the `QUARTO_FAULT_INJECT_DIAGNOSTIC_RENDER`
//! seam (`cfg(debug_assertions)`-gated in `render.rs`, so it does not
//! exist in a release build), which panics deliberately on the Nth
//! diagnostic rendered.
//!
//! TDD note: written before `render_diagnostic_guarded` wrapped the
//! render call sites; see the task report for the observed RED (a raw
//! panic that aborts the process, drops the second diagnostic, and
//! exits non-zero) captured before the fix landed.

use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn run_q2_render_with_fault(dir: &Path, extra_args: &[&str], fault_index: usize) -> Output {
    Command::new(Q2_BIN)
        .current_dir(dir)
        .arg("render")
        .arg(".")
        .args(extra_args)
        .env(
            "QUARTO_FAULT_INJECT_DIAGNOSTIC_RENDER",
            fault_index.to_string(),
        )
        .output()
        .expect("spawn q2 binary")
}

/// Two-page project where each page's own `{{< ... >}}` shortcode is
/// unrecognized: Q-16-3, warning severity, one per page, each
/// anchored at that page's own location. Unlike the shared
/// `_quarto.yml`-span fixture in `coalesced_diagnostics.rs` (which
/// `coalesce_by_source` collapses into a single group across all
/// pages), these stay two distinct groups — exactly the "one bad
/// diagnostic must not discard the ones behind it" shape this task
/// guards.
fn write_two_unknown_shortcode_pages(dir: &Path) {
    write_file(
        &dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n",
    );
    write_file(
        &dir.join("a.qmd"),
        "---\ntitle: A\n---\n\n{{< bogus_alpha >}}\n",
    );
    write_file(
        &dir.join("b.qmd"),
        "---\ntitle: B\n---\n\n{{< bogus_beta >}}\n",
    );
}

/// The core E1 contract, all four assertions the brief calls out
/// together: a panic while rendering one queued diagnostic must not
/// abort the (already-successful) render, must not discard the
/// diagnostics queued behind it, must surface loudly rather than
/// vanish silently, and must not change the exit code.
#[test]
fn panicking_diagnostic_render_does_not_abort_render_or_hide_the_rest() {
    let dir = TempDir::new().unwrap();
    write_two_unknown_shortcode_pages(dir.path());

    // Index 0: the first diagnostic rendered through
    // `render_diagnostic_guarded` in the process panics deliberately.
    let output = run_q2_render_with_fault(dir.path(), &[], 0);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "a caught panic on one warning must not change the exit code:\n{stderr}"
    );
    assert!(
        stderr.contains("internal error rendering diagnostic Q-16-3"),
        "the boundary must surface loudly, not swallow silently:\n{stderr}"
    );
    let alpha_survived = stderr.contains("bogus_alpha");
    let beta_survived = stderr.contains("bogus_beta");
    assert!(
        alpha_survived != beta_survived,
        "exactly one of the two queued diagnostics must have survived — \
         the other panicked mid-render and was replaced by the \
         internal-error line, not silently dropped without a trace; \
         got alpha_survived={alpha_survived} beta_survived={beta_survived}:\n{stderr}"
    );
    assert!(
        dir.path().join("_site/a.html").exists() && dir.path().join("_site/b.html").exists(),
        "the render already completed and wrote its output before \
         diagnostics were printed; both outputs must exist regardless \
         of the printing panic"
    );
}

/// Focused exit-code invariant (brief item 4): a caught panic while
/// printing a *warning* diagnostic does not change the exit code,
/// even when that warning is the only diagnostic in the run.
#[test]
fn caught_panic_on_a_warning_keeps_exit_code_zero() {
    let dir = TempDir::new().unwrap();
    write_file(
        &dir.path().join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n",
    );
    write_file(
        &dir.path().join("a.qmd"),
        "---\ntitle: A\n---\n\n{{< bogus_alpha >}}\n",
    );

    let output = run_q2_render_with_fault(dir.path(), &[], 0);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "the only diagnostic in this run is a warning; a caught panic \
         while rendering it must not force a non-zero exit:\n{stderr}"
    );
    assert!(
        stderr.contains("internal error rendering diagnostic Q-16-3"),
        "expected the boundary's internal-error line:\n{stderr}"
    );
    assert!(
        dir.path().join("_site/a.html").exists(),
        "the render itself must still have completed"
    );
}

/// The `--json-errors` branch's `diagnostic_to_json` call sites are
/// wrapped the same way: a panic there is caught and that one
/// diagnostic is omitted from the NDJSON stream rather than aborting
/// the process.
#[test]
fn json_errors_path_survives_a_panicking_diagnostic() {
    let dir = TempDir::new().unwrap();
    write_two_unknown_shortcode_pages(dir.path());

    let output = run_q2_render_with_fault(dir.path(), &["--json-errors"], 0);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "warnings alone must not fail the render, even under --json-errors:\n{stderr}"
    );
    assert!(
        stderr.contains("internal error rendering diagnostic Q-16-3"),
        "expected the boundary's internal-error line under --json-errors too:\n{stderr}"
    );
    assert!(
        stderr
            .lines()
            .any(|l| l.trim_start().starts_with('{') && l.contains("Q-16-3")),
        "the surviving diagnostic must still reach stderr as an NDJSON line:\n{stderr}"
    );
}

/// Regression guard: with the fault-injection env var unset (the
/// production default), the two-page fixture behaves exactly as
/// `coalesced_diagnostics.rs` expects elsewhere — both warnings print,
/// nothing panics.
#[test]
fn fault_injection_disarmed_by_default() {
    let dir = TempDir::new().unwrap();
    write_two_unknown_shortcode_pages(dir.path());

    let output = Command::new(Q2_BIN)
        .current_dir(dir.path())
        .args(["render", "."])
        .output()
        .expect("spawn q2 binary");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(output.status.success(), "clean warnings exit 0:\n{stderr}");
    assert!(
        !stderr.contains("internal error rendering diagnostic"),
        "the boundary must stay silent when nothing panics:\n{stderr}"
    );
    assert!(stderr.contains("bogus_alpha") && stderr.contains("bogus_beta"));
}

/// Two tables sharing one crossref id: renders successfully with
/// exactly one error diagnostic (`Q-15-1`) and no warnings.
///
/// Transcribed from `DUPLICATE_ID_DOC` in `render_exit_codes.rs:29-31`
/// (whose doc comment certifies that property) rather than shared
/// across modules, so that green test file stays untouched. The copy
/// is self-checking: if it ever stopped producing exactly the
/// `Q-15-1` error, the `internal error rendering diagnostic Q-15-1`
/// assertion below would fail.
const DUPLICATE_ID_DOC: &str = "---\ntitle: Duplicate ids\n---\n\n\
| a | b |\n|---|---|\n| 1 | 2 |\n\n: First {#tbl-dup}\n\n\
| c | d |\n|---|---|\n| 3 | 4 |\n\n: Second {#tbl-dup}\n";

/// T9 — **an invariant pin, not a regression test.** It asserts the
/// panic guard's most dangerous *hypothetical* failure mode: that a
/// swallowed render panic could rescue an exit code which must stay
/// non-zero. It cannot fail today for any edit to
/// `render_diagnostic_guarded`, and that is the point of pinning it.
///
/// **Why no guard mutation can redden this test.** This test drives
/// `render .`, so the path is `execute_project`: it calls
/// `print_render_diagnostics(&summary, …)` at `render.rs:1024`
/// and `should_exit_nonzero(&summary, …)` at `:1041`.
/// `execute_single_doc` has the same shape (`:836` /
/// `:848`). Both calls take `&summary`, and every closure the
/// guard runs borrows `&CoalescedDiagnostic` / `&DiagnosticMessage` /
/// `&SourceContext` only — the guard requires `UnwindSafe` without
/// `AssertUnwindSafe` (`:1290-1293`). So the exit gate counts a summary
/// that a swallowed panic cannot have touched. Note the ordering is
/// *printing first, gate second*: the invariant is immutability,
/// **not** "counting happens before printing" (Plan 2 § Hand-off
/// item 9 stated it the other way round and is corrected there,
/// dated 2026-08-23).
///
/// **The one hunk that would redden it** is not an edit to the guard
/// at all: "compute the exit status from the diagnostics that were
/// actually printed" — a refactor someone could plausibly make, at
/// which point a caught panic on the only error in a run would exit
/// 0. Nothing in the tree does that today.
///
/// The *absence* assertion uses the diagnostic's **own title** — the
/// string q2 renders, built at
/// `quarto-core/src/transforms/crossref_index.rs:381` — not
/// its code: the guard's own replacement line contains `Q-15-1`, so
/// the code cannot discriminate "the diagnostic rendered" from "the
/// guard fired". Note this is *not* the catalog title
/// (`error_catalog.json` says "Duplicate Crossref Identifier", title
/// case); nothing in the render path reads `ErrorInfo::title`, which
/// is consumed only for `docs_url`. `str::contains` is
/// case-sensitive, so asserting the catalog spelling would hold
/// unconditionally and the assertion would be vacuous. Verified by
/// running the same fixture with the fault *disarmed*: stderr then
/// reads `Error: [Q-15-1] Duplicate crossref identifier`.
#[test]
fn caught_panic_on_an_error_keeps_exit_code_nonzero() {
    let dir = TempDir::new().unwrap();
    write_file(
        &dir.path().join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n",
    );
    write_file(&dir.path().join("index.qmd"), DUPLICATE_ID_DOC);

    let output = run_q2_render_with_fault(dir.path(), &[], 0);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("internal error rendering diagnostic Q-15-1"),
        "the fault must have hit the one Q-15-1 error diagnostic:\n{stderr}"
    );
    assert!(
        !stderr.contains("Duplicate crossref identifier"),
        "the diagnostic's rendered body must be absent — its render is \
         exactly what panicked; seeing its title would mean the fault \
         landed on some other diagnostic:\n{stderr}"
    );
    assert!(
        !output.status.success(),
        "the only diagnostic in this run is an error; swallowing its \
         render panic must not rescue the exit code:\n{stderr}"
    );
}
