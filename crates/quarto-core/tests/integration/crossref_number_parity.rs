//! P6 Task 5: figure/theorem number-parity goldens — schedulable without P7.
//!
//! **Scope correction, 2026-09-20 (decided with Gordon):** figure + theorem
//! only, **not callout**. Verified empirically (`q2 render --to html` of a
//! labeled, titled callout): Q2's native HTML crossref renderer never
//! numbers `Callout` at all — no "Note N:" prefix anywhere
//! (`crossref_render.rs`'s `CrossrefRenderTransform` explicitly does not
//! touch `CustomNode("Callout")`). This is a genuine **numbers** gap on the
//! HTML side, not the presentation-only divergence design doc §12 already
//! documents, so there is no callout number to compare parity against.
//! Tracked as `bd-pk3gtn2i`; recorded as a new §12 bullet in
//! `claude-notes/designs/pandoc-hybrid-architecture.md`.
//!
//! No production file changes — this task is a test only.
//!
//! **Two things this file must not conflate** (per D1,
//! `claude-notes/plans/2026-09-18-pandoc-hybrid-P6-implementation.md`): a
//! **parity** assertion (the docx leg's numbers equal the HTML leg's numbers
//! for the same source) and a **discrimination** assertion (Q1's numbering
//! was suppressed). They cannot be the same test — for a flat, natural
//! single-instance-per-type fixture, Q2's and Q1's own counters agree
//! (both count from 1 in document order), so a parity golden **survives the
//! revert of Task 1's hunk** and asserts nothing about suppression. Task 4
//! owns discrimination (injected orders Q1 would never independently
//! compute); this file owns parity only, as shape/gating.
//!
//! **HTML-vs-docx parity compares digits, not full prefix strings.**
//! Verified: Q2's HTML leg joins kind+number with NBSP for *both* Figure
//! and Theorem (`crossref_render.rs::theorem_label_inlines`,
//! `"Theorem\u{a0}1"`), while the docx leg (real Q1 Lua) uses NBSP for
//! Figure but a **plain space** for Theorem's in-place caption
//! (`theorem.lua:282-283`'s `pandoc.Space()` — already established by
//! `pandoc_shim.rs::test_theorem_caption_is_numbered`'s "Theorem 1"
//! assertion). Design doc §12 / the epic's DoD narrowed the identity
//! promise to *numbers*, not presentation (`bd-wqdi1pd2`) — comparing full
//! prefix text would therefore be a false negative on Theorem's separator
//! alone, unrelated to whether the numbers themselves agree.
//!
//! **Fixture note: the figure uses the `Div(#fig-..)` authoring form
//! (Shape 1 of `float_ref_target.rs`'s four recognized shapes), not the
//! bare implicit-figure `![caption](img){#fig-x}` form (Shape 2).**
//! Verified empirically (`q2 render --to html`): on *this pre-rebase
//! branch*, Shape 2's caption arrives as `Block::Plain` (an
//! implicit-figure promotion), and `crossref_render.rs`'s `prefix_caption`
//! only prepends the number when the caption's first block is
//! `Block::Paragraph` — so Shape 2 silently renders with **no** "Figure N:"
//! prefix at all on the HTML leg today. This is not a new bug: it is the
//! closed strand `bd-hb9a9ik8` (duplicate of `bd-n3sark9b`), already fixed
//! on `main` in PR #690 (`94b5d9db3`, merged 2026-09-17) — one day *after*
//! `feature/pandoc-writer-hybrid` was rebased onto `main` (`1827a5e19`,
//! 2026-09-16), so this branch simply predates the fix. Decided with
//! Gordon (2026-09-20): keep working the plan now rather than pull the fix
//! forward; the eventual squash-rebase onto `main` picks it up. Shape 1's
//! trailing caption paragraph is authored literally (not
//! implicit-figure-promoted), so its caption block is a genuine
//! `Block::Paragraph` and is unaffected by the bug on either branch.

use std::sync::Arc;

use quarto_core::pandoc_filters::harness::{assert_pandoc_available, run_main_lua};
use quarto_core::project::ProjectContext;
use quarto_core::render_to_file::{RenderToFileOptions, render_document_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

use crate::pandoc_shim::build_ast_and_params_from_content;

/// One `#fig-`, one `#thm-`, one `.proof` — natural document order, no
/// injected-order overrides. `#fig-a`/`#thm-p` both land on Q2's and Q1's
/// own count of `1` for their respective ref-type (D1's collapsed case,
/// deliberately — this file is parity/shape, not discrimination).
const PARITY_FIXTURE: &str = "---\ntitle: Parity\n---\n\n\
::: {#fig-a}\n\
![](img.png)\n\n\
A caption.\n\
:::\n\n\
::: {#thm-p .theorem name=\"My Special Title\"}\n\
Theorem body.\n\
:::\n\n\
::: {.proof}\n\
Proof body.\n\
:::\n";

/// Converts a docx file back to plain text via a real `pandoc` subprocess.
/// Duplicated per file (not shared), per the integration-test module
/// boundary convention `crossref_numbering_matrix.rs` established.
fn docx_to_plain(docx_path: &std::path::Path) -> String {
    let output = std::process::Command::new("pandoc")
        .arg("-f")
        .arg("docx")
        .arg("-t")
        .arg("plain")
        .arg(docx_path)
        .output()
        .expect("failed to execute pandoc for docx->plain conversion");
    assert!(
        output.status.success(),
        "docx->plain conversion should succeed, stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Extracts the integer immediately following `kind` in `text`, tolerating
/// either an NBSP or a plain-space separator (the two legs use different
/// ones for Theorem — see the module doc).
fn number_after(text: &str, kind: &str) -> Option<u32> {
    let idx = text.find(kind)?;
    let rest = &text[idx + kind.len()..];
    let rest = rest.trim_start_matches(['\u{a0}', ' ']);
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

/// Renders [`PARITY_FIXTURE`] through the docx leg (P4's real transport +
/// P5's shim + real Q1 Lua) and returns the extracted plain text.
fn render_docx_plain() -> String {
    assert_pandoc_available();

    let (ast_json, params_json) =
        build_ast_and_params_from_content("parity-fig-thm.qmd", PARITY_FIXTURE.as_bytes());

    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");
    let outcome = run_main_lua(&ast_json, "docx", &params_json, &out_path);
    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );

    docx_to_plain(&out_path)
}

/// Renders [`PARITY_FIXTURE`] through Q2's real, native HTML leg (per
/// CLAUDE.md's end-to-end rule: `render_document_to_file`, not
/// `render_qmd_to_html` with a default config) and returns the rendered
/// HTML text.
fn render_html_text() -> String {
    let project_dir = tempfile::tempdir().expect("failed to create temp project dir");
    let input_path = project_dir.path().join("parity-fig-thm.qmd");
    std::fs::write(&input_path, PARITY_FIXTURE).expect("failed to write fixture input");

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&input_path, runtime.as_ref())
        .expect("project discovery for the parity fixture");

    let result = render_document_to_file(
        &input_path,
        "html",
        &RenderToFileOptions::default(),
        Some(&project),
        runtime.clone(),
        None,
        None,
        None,
    )
    .unwrap_or_else(|e| panic!("HTML render must succeed: {e}"));

    std::fs::read_to_string(&result.output_path).expect("read rendered HTML")
}

/// T5.1: the docx leg end-to-end (shape/gating — see the module doc for why
/// this specific fixture cannot discriminate suppression). Committed as an
/// `insta` snapshot of extracted semantic text, per the design doc §11's
/// resolved golden strategy (avoids Pandoc-version byte-noise).
///
/// Revert hunk: **none owned by P6** — a revert of Task 1's
/// `crossref-numbering: external` insert does not change this snapshot at
/// all (D1: Q1's own counters produce the same `1`/`1` for this fixture).
/// What this row *does* bind: reverting P5's order assignment for either
/// Figure or Theorem makes that type's prefix disappear (or, if the
/// assignment lands on the wrong return value, aborts the render) —
/// already covered by P5's own `test_float_caption_is_numbered`/
/// `test_theorem_caption_is_numbered`. This row's purpose is the parity
/// comparison in T5.3, not an independent suppression guard.
#[test]
fn test_docx_leg_renders_expected_numbers() {
    let plain = render_docx_plain();
    insta::assert_snapshot!(plain);

    assert!(
        plain.contains("Figure\u{a0}1:"),
        "expected \"Figure\\u{{a0}}1:\", got:\n{plain}"
    );
    assert!(
        plain.contains("Theorem 1"),
        "expected \"Theorem 1\" (space, not NBSP — see module doc), got:\n{plain}"
    );
}

/// T5.2: Q2's native HTML leg, real entry point. A genuine local binding of
/// pre-existing Q2 code — negative-space guard on
/// `crossref_index.rs`'s `plain_data.order` write-back.
///
/// Revert hunk: removing the `obj.insert("order", …)` block in
/// `crates/quarto-core/src/transforms/crossref_index.rs` (~285-296) leaves
/// the HTML leg's `CrossrefRenderTransform` with no order to consume —
/// `number_after` returns `None` for both kinds, and this test's
/// `.expect()` panics.
#[test]
fn test_html_leg_renders_expected_numbers() {
    let html = render_html_text();

    let fig_n =
        number_after(&html, "Figure").expect("expected an HTML Figure caption carrying a number");
    let thm_n =
        number_after(&html, "Theorem").expect("expected an HTML Theorem label carrying a number");

    assert_eq!(
        fig_n, 1,
        "expected Figure to be numbered 1, got HTML:\n{html}"
    );
    assert_eq!(
        thm_n, 1,
        "expected Theorem to be numbered 1, got HTML:\n{html}"
    );
}

/// T5.3: the two legs jointly — the row that expresses the epic's
/// Definition-of-done clause ("crossref/callout/theorem **numbers**
/// computed once and identical across HTML and Pandoc formats", scoped here
/// to fig+thm per the module doc's scope correction) as an assertion.
///
/// Revert hunk: revert either leg's order source (P5's shim assignment for
/// docx, or `crossref_index.rs`'s write-back for HTML) — the triples
/// diverge, or one side loses its number entirely. **Extracts digits, not
/// prefix strings** (see module doc: the two legs' Theorem separator
/// differs, so a full-string comparison would falsely redden on presentation
/// alone).
#[test]
fn test_docx_and_html_legs_agree_on_numbers() {
    let plain = render_docx_plain();
    let html = render_html_text();

    let docx_fig = number_after(&plain, "Figure").expect("docx Figure number");
    let docx_thm = number_after(&plain, "Theorem").expect("docx Theorem number");
    let html_fig = number_after(&html, "Figure").expect("html Figure number");
    let html_thm = number_after(&html, "Theorem").expect("html Theorem number");

    assert_eq!(
        docx_fig, html_fig,
        "Figure number should match between docx ({plain:?}) and html ({html:?})"
    );
    assert_eq!(
        docx_thm, html_thm,
        "Theorem number should match between docx ({plain:?}) and html ({html:?})"
    );
}

/// T5.4: Finding 3's deliberately-unnumbered Proof — `accepted-untested`
/// for the absence itself (both sides independently agree a Proof gets no
/// number: Q2 writes no `ref_type` for it at all, `transforms/proof.rs:148`;
/// Q1's `proof.lua` renderer never reads `.order` even when
/// `crossref_theorems` assigns one). What **is** bound here is the "path
/// was actually exercised" companion (D4): the proof's `proof_types` label
/// ("Proof") and its body text are present, and no digit appears on the
/// same line as the label — so an empty or dropped Proof node cannot
/// satisfy this row by accident.
///
/// Revert hunk: reverting P5's `Proof` dispatch arm, or the
/// `plain_data.type` field P2 Task 3 added (without it Q1's constructor
/// crashes, `proof.lua:81`'s unguarded `proof_types[proof_tbl.type:lower()]`)
/// makes the label assertion RED.
#[test]
fn test_proof_has_label_and_body_but_no_number() {
    let plain = render_docx_plain();

    let proof_line = plain
        .lines()
        .find(|line| line.contains("Proof"))
        .unwrap_or_else(|| panic!("expected a line containing the Proof label, got:\n{plain}"));
    assert!(
        !proof_line.chars().any(|c| c.is_ascii_digit()),
        "expected no digit on the Proof label's line, got: {proof_line:?}"
    );
    assert!(
        plain.contains("Proof body."),
        "expected the proof's body text to survive, got:\n{plain}"
    );
}
