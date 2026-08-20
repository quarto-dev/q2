//! bd-nn2fou8h: execute-visibility options (`echo`, `output`,
//! `warning`, `include`) must be honoured by both engines, at document
//! scope (`execute:`) and at cell scope (`#|`), with the cell winning.
//!
//! Reported as GH issue #523 ("`code-fold: true` silently overrides
//! `execute: echo: false`"). `code-fold` was a red herring — it is
//! unimplemented and inert; the real defect is that jupyter ignored the
//! whole family and knitr discarded document-scope `execute:` entirely.
//!
//! Drives the real engines through `record_capture` (the same producer
//! path `q2 render` / `q2 preview` / `q2 provide-hub` use) and asserts
//! on the post-engine markdown, mirroring `engine_error_policy.rs`.
//! Tests skip when the engine isn't installed.
//!
//! **Assertion style.** q2 is not required to reproduce Q1's
//! post-engine markdown byte-for-byte, so these tests assert on
//! observable semantics — "is the source echoed", "did the warning
//! survive" — via the structural markers downstream consumers key on
//! (`.cell`, `.cell-code`, `.cell-output`) plus content markers.
//!
//! **Content markers are split across concatenation on purpose.** A
//! cell whose body reads `print("O" + "UT")` produces the output text
//! `OUT` while its *source* contains no `OUT` substring, so a single
//! `contains("OUT")` distinguishes "the output survived" from "the
//! echoed source survived". Writing `print("OUT")` instead would make
//! every output assertion silently also match the echo.

use std::path::PathBuf;
use std::sync::Arc;

use quarto_core::engine::EngineRegistry;
use quarto_core::engine::preview_record::record_capture;
use quarto_core::project::ProjectContext;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn engine_available(name: &str) -> bool {
    EngineRegistry::default()
        .get(name)
        .is_some_and(|e| e.is_available())
}

fn fixture(
    content: &str,
) -> (
    tempfile::TempDir,
    PathBuf,
    ProjectContext,
    Arc<dyn SystemRuntime>,
) {
    let dir = tempfile::tempdir().unwrap();
    let qmd_path = dir.path().join("doc.qmd");
    std::fs::write(&qmd_path, content).unwrap();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&qmd_path, runtime.as_ref()).unwrap();
    (dir, qmd_path, project, runtime)
}

/// Post-engine markdown for `content`, or a panic with the engine error.
fn run(content: &str) -> String {
    let (_tmp, path, project, runtime) = fixture(content);
    let captures = pollster::block_on(record_capture(&path, &project, runtime, None))
        .unwrap_or_else(|e| panic!("record_capture failed: {e:?}"));
    assert_eq!(captures.len(), 1, "expected exactly one engine capture");
    captures[0]
        .result
        .get("markdown")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

// ---------------------------------------------------------------------
// Document fixtures
//
// `OUT` is produced only at runtime (see the module note on split
// markers); `SRCMARK` appears only in the source. Prose before and
// after the cell proves the document itself rendered when a cell is
// expected to vanish entirely.
// ---------------------------------------------------------------------

/// Python cell printing `OUT`; its source contains `SRCMARK` but no
/// literal `OUT`.
const PY_CELL: &str = "SRCMARK = \"O\" + \"UT\"\nprint(SRCMARK)";

/// R cell printing `OUT`; same split-marker property.
const R_CELL: &str = "SRCMARK <- paste0(\"O\", \"UT\")\ncat(SRCMARK, \"\\n\")";

/// Python cell emitting a stderr warning (`WARNME`) *and* stdout
/// (`OUT`), so a warning-only assertion can tell the two apart.
const PY_WARN_CELL: &str =
    "import warnings\nwarnings.warn(\"WARN\" + \"ME\")\nprint(\"O\" + \"UT\")";

/// R equivalent of [`PY_WARN_CELL`].
const R_WARN_CELL: &str = "warning(\"WARN\", \"ME\")\ncat(paste0(\"O\", \"UT\"), \"\\n\")";

fn doc(engine: &str, lang: &str, doc_execute: &str, cell_options: &str, cell: &str) -> String {
    format!(
        "---\ntitle: Visibility\nengine: {engine}\n{doc_execute}---\n\nBefore.\n\n\
         ```{{{lang}}}\n{cell_options}{cell}\n```\n\nAfter.\n"
    )
}

fn jupyter_doc(doc_execute: &str, cell_options: &str) -> String {
    doc("jupyter", "python", doc_execute, cell_options, PY_CELL)
}

fn knitr_doc(doc_execute: &str, cell_options: &str) -> String {
    doc("knitr", "r", doc_execute, cell_options, R_CELL)
}

/// Both prose blocks survived — i.e. the document rendered and only the
/// cell's visibility is under test.
fn assert_prose_intact(md: &str) {
    assert!(
        md.contains("Before.") && md.contains("After."),
        "surrounding prose must be untouched; got:\n{md}"
    );
}

fn assert_source_echoed(md: &str, echoed: bool) {
    assert_eq!(
        md.contains(".cell-code"),
        echoed,
        "expected source echoed = {echoed}; got:\n{md}"
    );
    assert_eq!(
        md.contains("SRCMARK"),
        echoed,
        "expected cell source text present = {echoed}; got:\n{md}"
    );
}

fn assert_output_present(md: &str, present: bool) {
    assert_eq!(
        md.contains("OUT"),
        present,
        "expected cell output present = {present}; got:\n{md}"
    );
}

/// No trace of the cell at all — not even an empty `.cell` wrapper.
/// `.cell` is a prefix of `.cell-code`/`.cell-output`, so this single
/// check covers the wrapper and everything that could live inside it.
fn assert_no_cell_at_all(md: &str) {
    assert!(
        !md.contains(".cell"),
        "cell must leave no markup behind (not even an empty wrapper); got:\n{md}"
    );
    assert!(
        !md.contains("SRCMARK") && !md.contains("OUT"),
        "neither source nor output may survive; got:\n{md}"
    );
    assert_prose_intact(md);
}

// =====================================================================
// Jupyter — document scope
// =====================================================================

#[test]
fn jupyter_doc_echo_false_hides_source_keeps_output() {
    if !engine_available("jupyter") {
        eprintln!("Skipping test: jupyter not available");
        return;
    }
    let md = run(&jupyter_doc("execute:\n  echo: false\n", ""));
    assert_source_echoed(&md, false);
    assert_output_present(&md, true);
    assert_prose_intact(&md);
}

#[test]
fn jupyter_doc_output_false_hides_output_keeps_source() {
    if !engine_available("jupyter") {
        eprintln!("Skipping test: jupyter not available");
        return;
    }
    let md = run(&jupyter_doc("execute:\n  output: false\n", ""));
    assert_source_echoed(&md, true);
    assert_output_present(&md, false);
    assert!(
        !md.contains(".cell-output"),
        "no output div may survive output: false; got:\n{md}"
    );
}

#[test]
fn jupyter_doc_warning_false_drops_stderr_keeps_stdout() {
    if !engine_available("jupyter") {
        eprintln!("Skipping test: jupyter not available");
        return;
    }
    let md = run(&doc(
        "jupyter",
        "python",
        "execute:\n  warning: false\n",
        "",
        PY_WARN_CELL,
    ));
    assert!(
        !md.contains("WARNME"),
        "stderr warning must be filtered out; got:\n{md}"
    );
    assert!(
        md.contains("OUT"),
        "stdout must survive warning: false; got:\n{md}"
    );
}

#[test]
fn jupyter_doc_include_false_emits_nothing() {
    if !engine_available("jupyter") {
        eprintln!("Skipping test: jupyter not available");
        return;
    }
    let md = run(&jupyter_doc("execute:\n  include: false\n", ""));
    assert_no_cell_at_all(&md);
}

// =====================================================================
// Jupyter — cell scope
// =====================================================================

#[test]
fn jupyter_cell_echo_false_hides_source_keeps_output() {
    if !engine_available("jupyter") {
        eprintln!("Skipping test: jupyter not available");
        return;
    }
    let md = run(&jupyter_doc("", "#| echo: false\n"));
    assert_source_echoed(&md, false);
    assert_output_present(&md, true);
}

#[test]
fn jupyter_cell_output_false_hides_output_keeps_source() {
    if !engine_available("jupyter") {
        eprintln!("Skipping test: jupyter not available");
        return;
    }
    let md = run(&jupyter_doc("", "#| output: false\n"));
    assert_source_echoed(&md, true);
    assert_output_present(&md, false);
}

#[test]
fn jupyter_cell_warning_false_drops_stderr_keeps_stdout() {
    if !engine_available("jupyter") {
        eprintln!("Skipping test: jupyter not available");
        return;
    }
    let md = run(&doc(
        "jupyter",
        "python",
        "",
        "#| warning: false\n",
        PY_WARN_CELL,
    ));
    assert!(
        !md.contains("WARNME"),
        "stderr warning must be filtered out; got:\n{md}"
    );
    assert!(md.contains("OUT"), "stdout must survive; got:\n{md}");
}

#[test]
fn jupyter_cell_include_false_emits_nothing() {
    if !engine_available("jupyter") {
        eprintln!("Skipping test: jupyter not available");
        return;
    }
    let md = run(&jupyter_doc("", "#| include: false\n"));
    assert_no_cell_at_all(&md);
}

// =====================================================================
// Jupyter — precedence and wrapper suppression
// =====================================================================

#[test]
fn jupyter_cell_echo_true_overrides_document_echo_false() {
    if !engine_available("jupyter") {
        eprintln!("Skipping test: jupyter not available");
        return;
    }
    let md = run(&jupyter_doc("execute:\n  echo: false\n", "#| echo: true\n"));
    assert_source_echoed(&md, true);
    assert!(
        !md.contains("#|"),
        "option lines must be stripped from the echo; got:\n{md}"
    );
}

#[test]
fn jupyter_cell_echo_false_overrides_document_echo_true() {
    if !engine_available("jupyter") {
        eprintln!("Skipping test: jupyter not available");
        return;
    }
    let md = run(&jupyter_doc("execute:\n  echo: true\n", "#| echo: false\n"));
    assert_source_echoed(&md, false);
    assert_output_present(&md, true);
}

/// A cell with neither visible source nor visible output must not leave
/// an empty `::: {.cell}` wrapper behind. Q1 builds the div opener but
/// writes it only "if there is actually content in the div"
/// (`jupyter.ts`); q2's `render_cell` emitted it unconditionally.
#[test]
fn jupyter_fully_hidden_cell_leaves_no_wrapper() {
    if !engine_available("jupyter") {
        eprintln!("Skipping test: jupyter not available");
        return;
    }
    let md = run(&jupyter_doc("", "#| echo: false\n#| output: false\n"));
    assert_no_cell_at_all(&md);
}

/// The exact fixture from GH issue #523. `code-fold: true` is inert in
/// q2 (unimplemented — bd-g1prx), so it must not affect `echo` either
/// way: the source is hidden because `execute: echo: false` says so.
#[test]
fn issue_523_code_fold_does_not_resurrect_hidden_source() {
    if !engine_available("jupyter") {
        eprintln!("Skipping test: jupyter not available");
        return;
    }
    let md = run(
        "---\ntitle: t\nformat:\n  html:\n    code-fold: true\nengine: jupyter\nexecute:\n  echo: false\n---\n\n```{python}\nSRCMARK = \"O\" + \"UT\"\nprint(SRCMARK)\n```\n",
    );
    assert_source_echoed(&md, false);
    assert_output_present(&md, true);
}

// =====================================================================
// Knitr
//
// The R scripts already read every one of these keys off
// `format$execute` (resources/rmd/execute.R); the defect was that
// nothing populated it from the document. Cell scope has always worked
// (knitr resolves `#|` options itself), so the cell-scope test here is
// a regression guard rather than a new behaviour.
// =====================================================================

#[test]
fn knitr_doc_echo_false_hides_source_keeps_output() {
    if !engine_available("knitr") {
        eprintln!("Skipping test: knitr not available");
        return;
    }
    let md = run(&knitr_doc("execute:\n  echo: false\n", ""));
    assert_source_echoed(&md, false);
    assert_output_present(&md, true);
    assert_prose_intact(&md);
}

#[test]
fn knitr_doc_output_false_hides_output_keeps_source() {
    if !engine_available("knitr") {
        eprintln!("Skipping test: knitr not available");
        return;
    }
    let md = run(&knitr_doc("execute:\n  output: false\n", ""));
    assert_source_echoed(&md, true);
    assert_output_present(&md, false);
}

#[test]
fn knitr_doc_warning_false_drops_warning_keeps_stdout() {
    if !engine_available("knitr") {
        eprintln!("Skipping test: knitr not available");
        return;
    }
    let md = run(&doc(
        "knitr",
        "r",
        "execute:\n  warning: false\n",
        "",
        R_WARN_CELL,
    ));
    assert!(
        !md.contains("WARNME"),
        "warning must be filtered out; got:\n{md}"
    );
    assert!(md.contains("OUT"), "stdout must survive; got:\n{md}");
}

#[test]
fn knitr_doc_include_false_emits_nothing() {
    if !engine_available("knitr") {
        eprintln!("Skipping test: knitr not available");
        return;
    }
    let md = run(&knitr_doc("execute:\n  include: false\n", ""));
    assert_no_cell_at_all(&md);
}

/// Regression guard: knitr resolves `#|` options itself, so this
/// already worked. It must keep working once the document scope is
/// forwarded (a naive `execute:` passthrough that dropped the
/// `include: true` default would blank every chunk instead).
#[test]
fn knitr_cell_echo_false_still_hides_source() {
    if !engine_available("knitr") {
        eprintln!("Skipping test: knitr not available");
        return;
    }
    let md = run(&knitr_doc("", "#| echo: false\n"));
    assert_source_echoed(&md, false);
    assert_output_present(&md, true);
}

/// Defaults must survive the document-scope plumbing: with no
/// `execute:` key at all, source and output are both visible.
#[test]
fn knitr_no_execute_key_keeps_defaults() {
    if !engine_available("knitr") {
        eprintln!("Skipping test: knitr not available");
        return;
    }
    let md = run(&knitr_doc("", ""));
    assert_source_echoed(&md, true);
    assert_output_present(&md, true);
}

#[test]
fn jupyter_no_execute_key_keeps_defaults() {
    if !engine_available("jupyter") {
        eprintln!("Skipping test: jupyter not available");
        return;
    }
    let md = run(&jupyter_doc("", ""));
    assert_source_echoed(&md, true);
    assert_output_present(&md, true);
}
