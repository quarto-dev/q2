//! bd-knitr-executes-nested-display-fence-atbtktdj — render-tier (tier R)
//! failing tests for the nested-cell-masking fix, plan
//! `.superpowers/sdd/2026-09-02-nested-cell-masking/spec.md`.
//!
//! Every fixture is a document with a **live** `{r}` cell (the bug is
//! unreachable without one — AST-based resolution declines to start knitr
//! for a document whose only `{r}` fence is displayed) plus a display
//! construct under test. All fixtures render through the real per-document
//! path (`render_document_to_file`, the same entry `q2 render` uses), with
//! real knitr — no mocks.
//!
//! **These tests are RED by design and stay RED until a later task wires
//! `nested_cell_mask::mask`/`unmask` into `engine_execution.rs`.** Nothing
//! here calls `mask`/`unmask` directly (that's tier U, in
//! `crates/quarto-core/src/engine/nested_cell_mask.rs`); these assert on the
//! rendered HTML of the *current, unfixed* pipeline, so they fail on wrong
//! output rather than on a stub panic.
//!
//! **The runtime-only marker rule** (spec, "Test seam spec (frozen)"):
//! every executed marker is written `paste0("WORD", "-RAN")` rather than a
//! single string literal, so the substring `WORD-RAN` exists in the
//! rendered HTML *only* if the cell actually ran — never as an echo of the
//! (HTML-escaped, comma-split) source.
//!
//! **Gate.** Unlike `knitr_display_fence.rs`, which gates on
//! `EngineRegistry::default().get("knitr").is_some_and(|e| e.is_available())`
//! (binary only), these tests gate on both the `Rscript` binary and the R
//! `knitr` package — copied from `marimo_engine_e2e.rs:76-94`
//! (`rscript_available` / `knitr_r_package_available`). On this machine both
//! are present (knitr 1.50), so no test here should skip.
//!
//! **T8 deviation, recorded up front.** The seam table's T8 row calls for "a
//! four-space-indented display block (no info string)". qmd has **no**
//! CommonMark indented-code-block production at all — it is a deliberate,
//! documented, blanket known limitation (`grammar.js` `_indented_code_block_error`
//! / scanner.c's `INDENTED_CODE_BLOCK_DISALLOWED`, diagnostic Q-2-35),
//! verified here to fire regardless of nesting context (top-level and
//! inside a list item both error; there is no grammar path that accepts
//! raw 4-space-indented text as a code block). The only way to construct a
//! `CodeBlock` with **empty** classes — the AST shape T8's row actually
//! targets, per `nested_cell_mask.rs`'s own scope doc: "classes empty (no
//! info string, which also covers a 4-space indented code block)" — is a
//! bare fenced block with no info string at all (` ``` ` / ` ``` `, no
//! language). T8 below uses that. Verified empirically (this task) to
//! reproduce the bug identically to the `["markdown"]`-classed fixtures.

#![cfg(not(target_arch = "wasm32"))]

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::ProjectContext;
use quarto_core::render_to_file::{RenderToFileOptions, render_document_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// Return `true` when `Rscript` is on PATH. Copied from
/// `marimo_engine_e2e.rs:76-81` per the task brief — the binary check alone
/// (`KnitrEngine::is_available()` / `knitr_display_fence.rs`'s
/// `knitr_available()`) is not sufficient; we also need the R package.
fn rscript_available() -> bool {
    Command::new("Rscript")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Return `true` when the R `knitr` package is installed. Copied from
/// `marimo_engine_e2e.rs:87-95`.
fn knitr_r_package_available() -> bool {
    Command::new("Rscript")
        .args([
            "-e",
            "if (!requireNamespace(\"knitr\", quietly = TRUE)) quit(status = 1)",
        ])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// The gate every test in this file uses. A skip here is a signal something
/// is wrong on the machine, not a pass (task brief).
fn knitr_render_available() -> bool {
    rscript_available() && knitr_r_package_available()
}

/// Render `content` to HTML through the real render path
/// (`render_document_to_file`, the same entry `q2 render` uses) and return
/// the rendered HTML. Panics with the render error on failure.
fn render_html(tmp: &TempDir, name: &str, content: &str) -> String {
    render_html_result(tmp, name, content).unwrap_or_else(|e| panic!("render must succeed: {e}"))
}

/// Same as `render_html`, but surfaces a render failure as `Err` instead of
/// panicking. Used only by T5, whose fixture is expected to hard-fail the
/// render on this machine (vacuity note 3) — a render error there is a
/// legitimate RED, not a harness bug.
fn render_html_result(tmp: &TempDir, name: &str, content: &str) -> Result<String, String> {
    let input = tmp.path().join(name);
    std::fs::write(&input, content).unwrap();

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&input, runtime.as_ref())
        .expect("project discovery for the nested-cell-mask render fixture");

    let result = render_document_to_file(
        &input,
        "html",
        &RenderToFileOptions::default(),
        Some(&project),
        runtime.clone(),
        None,
        None,
        None,
    )
    .map_err(|e| e.to_string())?;

    Ok(read_html(&result.output_path))
}

fn read_html(path: &Path) -> String {
    std::fs::read_to_string(path).expect("read rendered HTML")
}

/// The T1/T2/T3 fixture: a live `{r}` cell plus a ` ````markdown ` block
/// displaying `{r}`. The real cell's marker is `REAL-RAN`; the display
/// block's is `DISPLAY-RAN` — both runtime-only (`paste0`-split).
fn t1_t2_t3_fixture() -> String {
    "---\ntitle: T1-T2-T3\nengine: knitr\n---\n\n\
     ```{r}\ncat(paste0(\"REAL\", \"-RAN\"), \"\\n\")\n```\n\n\
     ````markdown\n```{r}\ncat(paste0(\"DISPLAY\", \"-RAN\"), \"\\n\")\n```\n````\n"
        .to_string()
}

// ── T1 (H1): mask rewrites an opener ────────────────────────────────────
//
// `mask` rewriting `{r}` -> `{.r q2-nested-executable}` is what stops
// knitr's own chunk scanner from claiming the *displayed* `{r}` opener.
// Reverting H1 leaves the opener bare, knitr's `all_patterns$md$chunk.begin`
// matches it, and the displayed example executes -- `DISPLAY-RAN` appears.
#[test]
fn t1_displayed_r_fence_does_not_execute() {
    if !knitr_render_available() {
        eprintln!(
            "SKIP: knitr (Rscript + package) not available — t1_displayed_r_fence_does_not_execute"
        );
        return;
    }
    let tmp = TempDir::new().unwrap();
    let html = render_html(&tmp, "t1.qmd", &t1_t2_t3_fixture());

    assert!(
        !html.contains("DISPLAY-RAN"),
        "the displayed {{r}} example must not execute (DISPLAY-RAN is \
         runtime-only — it cannot appear from an echo of the source); \
         html:\n{html}"
    );
}

// ── T2 (H3): unmask restores a marked opener ────────────────────────────
//
// Even once masking stops the displayed fence from executing, the reader
// must still see the *original* `{r}` opener, not our internal
// `{.r q2-nested-executable}` marker or knitr's cell scaffolding. That
// restoration is `unmask`'s job; reverting H3 (dropping the marker
// requirement, or not restoring at all) leaves the marked/mangled opener in
// the rendered `<pre>`.
#[test]
fn t2_displayed_r_fence_shows_original_opener_no_marker_anywhere() {
    if !knitr_render_available() {
        eprintln!(
            "SKIP: knitr (Rscript + package) not available — t2_displayed_r_fence_shows_original_opener_no_marker_anywhere"
        );
        return;
    }
    let tmp = TempDir::new().unwrap();
    let html = render_html(&tmp, "t2.qmd", &t1_t2_t3_fixture());

    // The display <pre> must show the literal, unmangled opener line. At
    // baseline knitr replaces it with cell scaffolding
    // (` ```{.r .cell-code} `), so this substring is absent pre-fix.
    assert!(
        html.contains("```{r}"),
        "the display block must show the original `{{r}}` opener, \
         unmangled; html:\n{html}"
    );
    assert!(
        !html.contains("q2-nested-executable"),
        "the internal masking marker must never reach rendered output; \
         html:\n{html}"
    );
}

// ── T3 (H2): the in-scope predicate must not widen to the real cell ────
//
// Green-at-baseline guard, like the Control below — NOT a RED. The current
// bug only under-masks (it executes displayed fences); it never over-masks
// (blocks the real cell), so this assertion is already true with zero
// masking in place. It is bound to H2 (a hunk that does not exist until
// Phase 2), not to a symptom of the present bug. Do not "fix" this test to
// force a failure.
//
// The real, executable `{r}` cell sitting next to the display block must
// keep running. This guards against an over-broad fix that masks *every*
// CodeBlock (widening H2's classes-empty-or-["markdown"] predicate to all
// blocks) — that would also mask the real cell.
#[test]
fn t3_real_cell_still_executes() {
    if !knitr_render_available() {
        eprintln!("SKIP: knitr (Rscript + package) not available — t3_real_cell_still_executes");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let html = render_html(&tmp, "t3.qmd", &t1_t2_t3_fixture());

    assert!(
        html.contains("REAL-RAN"),
        "the real {{r}} cell must still execute; html:\n{html}"
    );
}

// ── T4 (H1): echo=FALSE display block — two-surface, non-vacuous check ──
//
// Vacuity note 1: a *count* of the marker collapses (it appears once
// before the fix — output only, source consumed — and once after — source
// only, no output). This asserts on the two surfaces that genuinely
// differ: the runtime-only string (must be absent) AND the author's own
// source literal (must be present — knitr consumes it pre-fix).
#[test]
fn t4_echo_false_display_block_neither_runs_nor_loses_its_source() {
    if !knitr_render_available() {
        eprintln!(
            "SKIP: knitr (Rscript + package) not available — t4_echo_false_display_block_neither_runs_nor_loses_its_source"
        );
        return;
    }
    let tmp = TempDir::new().unwrap();
    let content = "---\ntitle: T4\nengine: knitr\n---\n\n\
                   ```{r}\ncat(paste0(\"REAL\", \"-RAN\"), \"\\n\")\n```\n\n\
                   ````markdown\n```{r, echo=FALSE}\ncat(paste0(\"OPTS\", \"-RAN\"), \"\\n\")\n```\n````\n";
    let html = render_html(&tmp, "t4.qmd", content);

    assert!(
        !html.contains("OPTS-RAN"),
        "the echo=FALSE displayed example must not execute (runtime-only \
         marker must be absent); html:\n{html}"
    );
    // The author's source, HTML-escaped as plain (non-highlighted) text —
    // verified empirically: the writer escapes `"` to `&quot;` even inside
    // an unexecuted `markdown`-classed <pre>. Pre-fix, knitr consumes the
    // echo=FALSE chunk's source entirely (only its output survives), so
    // this substring is absent before the fix and present after.
    assert!(
        html.contains("paste0(&quot;OPTS&quot;"),
        "the author's own source must survive (echo=FALSE must not cost \
         the reader the example) — this is what a naive occurrence-count \
         cannot discriminate (vacuity note 1); html:\n{html}"
    );
}

// ── T5 (H1): a displayed {python} example must not kill the whole render ─
//
// The most user-visible face of the bug: knitr hands the *displayed*
// {python} example to reticulate, which fails outright on this machine
// (python3 is installed, but reticulate's own discovery misses it via a
// stale uv build-cache path) and the ENTIRE render dies. Vacuity note 3:
// "the render succeeds" is not assertable here (it would be vacuous on a
// machine with a working reticulate/Python), so we assert on content: the
// display <pre> literally contains ```{python}`, and nothing inside it
// looks like it executed. A render failure here (as currently, on this
// machine) is itself a legitimate RED — the fix's job is precisely to stop
// the fatal failure, not just to fix content once rendering happens to
// succeed.
#[test]
fn t5_displayed_python_in_r_document_does_not_kill_the_render() {
    if !knitr_render_available() {
        eprintln!(
            "SKIP: knitr (Rscript + package) not available — t5_displayed_python_in_r_document_does_not_kill_the_render"
        );
        return;
    }
    let tmp = TempDir::new().unwrap();
    let content = "---\ntitle: T5\nengine: knitr\n---\n\n\
                   ```{r}\ncat(paste0(\"REAL\", \"-RAN\"), \"\\n\")\n```\n\n\
                   ````markdown\n```{python}\nprint(\"hi\")\n```\n````\n";

    match render_html_result(&tmp, "t5.qmd", content) {
        Err(e) => {
            // The fatal symptom itself: knitr handed the displayed
            // {python} fence to reticulate and the whole render died.
            // This is exactly the bug the fix must eliminate (spec, "Two
            // symptoms worse than 'wrong output'") — a legitimate RED, not
            // a harness bug. But not *any* error qualifies: an unrelated
            // environment breakage (a broken R install, say) must not be
            // silently misattributed to this bug, and — once the fix
            // lands — a CI runner with independently-broken reticulate
            // would otherwise keep this test red for a reason the fix
            // cannot address. So check the error names the expected
            // cause before treating it as this bug's RED.
            //
            // Structural gap, discovered implementing this check (see the
            // report addendum): `render_document_to_file`'s
            // `ExecutionContext` defaults `quiet: false`
            // (`engine/context.rs:179`), and nothing in the render call
            // chain (`render_to_file.rs` / `pipeline.rs` /
            // `engine_execution.rs`) ever calls `with_quiet(true)` —
            // `RenderToFileOptions::quiet` is a *logging* flag, never
            // threaded into `ExecutionContext`. With `quiet: false`,
            // `knitr/subprocess.rs` pipes R's stderr to `Stdio::inherit()`
            // rather than `Stdio::piped()` (`subprocess.rs:373-374`), so
            // `convert_r_error_to_execution_error`'s classifier always
            // receives an EMPTY stderr string for this failure and can
            // only produce the bare fallback `"R process failed"`
            // (`subprocess.rs:487`) — never `"python"`/`"reticulate"`. The
            // real reticulate detail genuinely exists (verified verbatim
            // in this task's evidence, streamed straight to this
            // process's own inherited stderr) but structurally cannot
            // reach the Rust-level error string through this public API
            // today. Filed as bd-jxhvy3pi ("Engine subprocess failures
            // lose their stderr detail in the normal render path"), which
            // names this test as its acceptance test — when that strand
            // lands (fix direction: capture stderr always, tee it when
            // not quiet, rather than choosing one or the other), TIGHTEN
            // this back to requiring the detailed python/reticulate
            // message and drop the bare-fallback acceptance below. Until
            // then: accept either the detailed message or today's
            // documented bare-fallback shape. This still rejects the
            // failure modes Finding 2 is actually guarding against — a
            // missing-package, a missing-runtime, a spawn failure, or a
            // well-formed knitr execution message all classify distinctly
            // and do not produce this exact bare text.
            let lower = e.to_lowercase();
            let names_python_reticulate = lower.contains("python") || lower.contains("reticulate");
            let is_documented_bare_fallback = lower.contains("r process failed");
            assert!(
                names_python_reticulate || is_documented_bare_fallback,
                "T5 (H1) render failed, but with neither the expected \
                 displayed-{{python}}-handed-to-reticulate symptom (no \
                 \"python\"/\"reticulate\") nor the documented bare-fallback \
                 shape (\"R process failed\" — the only shape this specific \
                 failure can take through render_document_to_file's \
                 default, non-quiet ExecutionContext; see the comment \
                 above) — this failure looks unrelated to the \
                 nested-cell-masking bug and must be investigated \
                 separately, not attributed to this task: {e}"
            );
            panic!(
                "T5 (H1): render of a document with a live {{r}} cell and a \
                 displayed {{python}} example failed outright — this fatal \
                 failure mode is exactly what the fix must eliminate: {e}"
            );
        }
        Ok(html) => {
            // After the fix, `mask` rewrites the displayed {python}
            // opener before knitr ever sees it, so knitr never hands it
            // to reticulate and this branch becomes the one that runs.
            //
            // UNREACHED as of this task: on this machine the render
            // always takes the `Err` branch above (reticulate can't find
            // the installed python3), so these two assertions — the
            // exact ones vacuity note 3 calls for — have never actually
            // executed. A bug in the `split_once("REAL-RAN")` scoping or
            // the substring checks would not surface here; it will
            // surface the first time this branch runs for real, which is
            // after the fix lands. Re-verify this branch by hand (or with
            // a temporary working-tree Python shim) the first time T5
            // takes this path, rather than trusting it on faith.
            let after_real_cell = html
                .split_once("REAL-RAN")
                .map_or(html.as_str(), |(_, rest)| rest);
            assert!(
                after_real_cell.contains("```{python}"),
                "the display block must show the literal ```{{python}} \
                 opener, unexecuted; html:\n{html}"
            );
            assert!(
                !after_real_cell.contains("cell-output"),
                "the display block must contain no .cell-output — nothing \
                 inside it may look like it executed; html:\n{html}"
            );
        }
    }
}

// ── T6 (H4): unmask must be prefix-tolerant, not `^`-anchored ──────────
//
// A blockquoted display block: mask never sees the `> ` prefix (the reader
// strips it before mask ever sees the block's text), but unmask runs
// textually over the engine's *output*, which still carries `> `. An
// `^`-anchored unmask misses every blockquoted fence, and (per the spec's
// measured baseline) the cell escapes the blockquote entirely: two `.cell`
// divs are emitted and the display block renders empty.
#[test]
fn t6_blockquoted_display_block_stays_inside_the_blockquote() {
    if !knitr_render_available() {
        eprintln!(
            "SKIP: knitr (Rscript + package) not available — t6_blockquoted_display_block_stays_inside_the_blockquote"
        );
        return;
    }
    let tmp = TempDir::new().unwrap();
    let content = "---\ntitle: T6\nengine: knitr\n---\n\n\
                   ```{r}\ncat(paste0(\"REAL\", \"-RAN\"), \"\\n\")\n```\n\n\
                   > ````markdown\n> ```{r}\n> cat(paste0(\"QUOTE\", \"-RAN\"), \"\\n\")\n> ```\n> ````\n";
    let html = render_html(&tmp, "t6.qmd", content);

    // The display block's body text must survive, non-empty, inside the
    // blockquote — checked as the author's own source literal (plain,
    // unhighlighted escaping: `&quot;` around each string). Pre-fix this
    // text is only ever seen *highlighted* (span-wrapped, because knitr
    // executed it as a real cell), so this contiguous substring is absent
    // before the fix.
    assert!(
        html.contains("paste0(&quot;QUOTE&quot;"),
        "the blockquoted display block's body must survive intact, \
         unexecuted, inside the blockquote; html:\n{html}"
    );
    // Exactly one `.cell` div — the real cell's. Pre-fix the nested `{r}`
    // escapes the blockquote as its own second cell (measured baseline: 2).
    let cell_count = html.matches("class=\"cell\"").count();
    assert_eq!(
        cell_count, 1,
        "exactly one `.cell` div (the real cell's) must be present — the \
         blockquoted display block must not escape as a second cell; \
         html:\n{html}"
    );
}

// ── T7 (H1): fence-opener whitespace variants ───────────────────────────
//
// A space between the fence and the brace (` ``` {r} `), and trailing
// whitespace after the closing brace (` ```{r}   `), must both stay
// unexecuted. knitr's own `all_patterns$md$chunk.begin` matches both
// spellings; mask's opener pattern must too.
#[test]
fn t7_whitespace_variant_openers_do_not_execute() {
    if !knitr_render_available() {
        eprintln!(
            "SKIP: knitr (Rscript + package) not available — t7_whitespace_variant_openers_do_not_execute"
        );
        return;
    }
    let tmp = TempDir::new().unwrap();
    let content = "---\ntitle: T7\nengine: knitr\n---\n\n\
                   ```{r}\ncat(paste0(\"REAL\", \"-RAN\"), \"\\n\")\n```\n\n\
                   ````markdown\n``` {r}\ncat(paste0(\"WS\", \"-RAN\"), \"\\n\")\n```\n````\n\n\
                   ````markdown\n```{r}   \ncat(paste0(\"WS\", \"-RAN\"), \"\\n\")\n```\n````\n";
    let html = render_html(&tmp, "t7.qmd", content);

    assert!(
        !html.contains("WS-RAN"),
        "neither whitespace-variant opener (space-before-brace or \
         trailing-whitespace) may execute; html:\n{html}"
    );
}

// ── T8 (H1): classes-empty display block (no info string) ──────────────
//
// See this file's top-level doc comment for why this uses a bare fenced
// block with no info string (` ``` `) rather than a literal 4-space-indented
// block: qmd has no indented-code-block grammar production at all (Q-2-35,
// a deliberate blanket known limitation, verified to fire both at document
// top level and inside a list item — there is no context in which raw
// indentation parses as code). Both shapes produce the identical AST target
// H2 actually predicates on: a `CodeBlock` with **empty** classes.
#[test]
fn t8_bare_fenced_no_info_string_display_block_does_not_execute() {
    if !knitr_render_available() {
        eprintln!(
            "SKIP: knitr (Rscript + package) not available — t8_bare_fenced_no_info_string_display_block_does_not_execute"
        );
        return;
    }
    let tmp = TempDir::new().unwrap();
    let content = "---\ntitle: T8\nengine: knitr\n---\n\n\
                   ```{r}\ncat(paste0(\"REAL\", \"-RAN\"), \"\\n\")\n```\n\n\
                   ````\n```{r}\ncat(paste0(\"IND\", \"-RAN\"), \"\\n\")\n```\n````\n";
    let html = render_html(&tmp, "t8.qmd", content);

    assert!(
        !html.contains("IND-RAN"),
        "a bare fenced (no info string) display block must not execute; \
         html:\n{html}"
    );
}

// Control: characterization only, not revert-bound.
//
// A document containing a display block and NO live cell at all renders
// unchanged. This pins the plan's claim that the bug is "reachable only
// when the engine is already live" — AST-based resolution declines to
// start knitr for a document whose only `{r}` fence is displayed, so this
// test is expected to be GREEN both before and after the fix. It does not
// gate on knitr availability in the same load-bearing way as T1-T8 (no
// engine ever runs for this fixture), but we keep the same gate for
// consistency with the rest of this file and because the fixture declares
// no `engine:` field at all (nothing to check availability of, but a
// skipped run here would still hide a regression on a knitr-equipped CI
// runner).
#[test]
fn control_display_block_with_no_live_cell_renders_unchanged() {
    if !knitr_render_available() {
        eprintln!(
            "SKIP: knitr (Rscript + package) not available — control_display_block_with_no_live_cell_renders_unchanged"
        );
        return;
    }
    let tmp = TempDir::new().unwrap();
    let content = "---\ntitle: Control\n---\n\n\
                   ````markdown\n```{r}\ncat(paste0(\"CTRL\", \"-RAN\"), \"\\n\")\n```\n````\n";
    let html = render_html(&tmp, "control.qmd", content);

    assert!(
        html.contains("```{r}"),
        "with no live cell anywhere, knitr must never start, so the \
         display block's opener must survive untouched; html:\n{html}"
    );
    assert!(
        !html.contains("CTRL-RAN"),
        "with no live cell anywhere, the displayed example must not \
         execute; html:\n{html}"
    );
    assert!(
        html.matches("class=\"cell\"").count() == 0,
        "with no live cell anywhere, no `.cell` div may appear at all; \
         html:\n{html}"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// bd-0gwekaem — inline expressions inside a display block
//
// A *new tier*, not an extension of T1–T24: the seam spec above is frozen,
// and these rows bind to their own hunks.
//
//   H14 — `mask` rewrites an inline expression marker inside an in-scope
//         display block: `` `r x` `` -> `` `q2-nested-executable r x` ``,
//         and likewise for the brace spelling `` `{r} x` ``.
//   H15 — `unmask` restores it (backtick + marker + one space -> backtick).
//   H16 — the inline pattern's `(^|[^`])` guard, so a fence's own backtick
//         can never anchor an inline match.
//
// The unit-tier rows for these hunks live in `nested_cell_mask.rs`; these
// two prove the fix through the real render path with real knitr.
// ═══════════════════════════════════════════════════════════════════════

/// The T25/T26 fixture: a live `{r}` cell that defines `v` (with `echo:
/// false`, so its own source never reaches the HTML) plus a ` ````markdown `
/// block displaying both inline spellings.
///
/// `v` is built with `paste0` so the marker `INLINERAN` is **runtime-only**
/// — it cannot appear in the HTML as an echo of anything, only as the value
/// of an inline expression that actually evaluated.
///
/// **Vacuity note (measured).** The marker deliberately contains no
/// markdown-special character. An earlier draft used `INLINE-RAN` and T25
/// passed even with H14 reverted: q2's classic inline spelling escapes
/// markdown specials in the value, so the executed expression reaches the
/// HTML as `INLINE\-RAN` and a `contains("INLINE-RAN")` check is blind to
/// it. Any runtime-only marker used with an inline expression must survive
/// `.QuartoInlineRender`'s escaping unchanged.
fn t25_t26_fixture() -> String {
    "---\ntitle: T25-T26\nengine: knitr\n---\n\n\
     ```{r}\n#| echo: false\nv <- paste0(\"INLINE\", \"RAN\")\n```\n\n\
     ````markdown\nClassic: `r v`\n\nBrace: `{r} v`\n````\n"
        .to_string()
}

// ── T25 (H14): a displayed inline expression does not execute ───────────
//
// The nested-cell mask stops a displayed *fence opener* from executing, but
// an inline expression in the same display block was never rewritten, so
// knitr's `all_patterns$md$inline.code` (and q2's own
// `resolve_inline_r_expressions` pass, which runs over the whole serialized
// document and is equally fence-unaware) claimed it anyway. Reverting H14
// leaves the marker bare and `INLINERAN` — runtime-only — appears.
#[test]
fn t25_displayed_inline_expression_does_not_execute() {
    if !knitr_render_available() {
        eprintln!(
            "SKIP: knitr (Rscript + package) not available — \
             t25_displayed_inline_expression_does_not_execute"
        );
        return;
    }
    let tmp = TempDir::new().unwrap();
    let html = render_html(&tmp, "t25.qmd", &t25_t26_fixture());

    assert!(
        !html.contains("INLINERAN"),
        "a displayed inline expression must not execute (INLINERAN is \
         runtime-only — it cannot appear from an echo of the source); \
         html:\n{html}"
    );
}

// ── T26 (H15): the reader sees the original inline expression ───────────
//
// Two surfaces, because they fail for different reasons. The *classic*
// spelling discriminates H14 and H15 together on `main` today. The *brace*
// spelling's literal is currently produced either way — q2 does not
// evaluate `{r}` yet (bd-inline-r-brace-spelling-not-evaluated-lk9s3iwe) —
// so today it binds only to **H15**: masking does fire on it, so a missing
// or wrong restore leaves `q2-nested-executable` in the `<pre>`. Once that
// strand lands, the same assertion starts discriminating H14 as well, which
// is why the brace row is written now rather than deferred.
#[test]
fn t26_displayed_inline_expression_shows_original_spelling_no_marker() {
    if !knitr_render_available() {
        eprintln!(
            "SKIP: knitr (Rscript + package) not available — \
             t26_displayed_inline_expression_shows_original_spelling_no_marker"
        );
        return;
    }
    let tmp = TempDir::new().unwrap();
    let html = render_html(&tmp, "t26.qmd", &t25_t26_fixture());

    assert!(
        html.contains("Classic: `r v`"),
        "the display block must show the classic inline spelling exactly as \
         written; html:\n{html}"
    );
    assert!(
        html.contains("Brace: `{r} v`"),
        "the display block must show the brace inline spelling exactly as \
         written; html:\n{html}"
    );
    assert!(
        !html.contains("q2-nested-executable"),
        "the internal mask marker must never survive into the rendered HTML; \
         html:\n{html}"
    );
}
