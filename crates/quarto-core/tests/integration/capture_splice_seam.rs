/*
 * tests/integration/capture_splice_seam.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * PC-B (bd-h4rhohhy, Bug B) + SC22 (bd-5jxcio5d, Plan 4c.2): binds the
 * q2-preview capture-splice contract both ways. The splice
 * (`apply_capture_splice` / `derive_cell_outputs` / `is_engine_output_block` in
 * `quarto-core/src/engine/capture_splice.rs`) maps each engine cell to the next
 * engine-OUTPUT block in the executed markdown — a `::: {.cell}` wrapper Div
 * (echo/julia/jupyter, via `mdFromCodeCell`) OR a bare `{=html}` `RawBlock`
 * island (marimo, which emits ZERO `.cell` Divs — see bd-5jxcio5d). This file
 * proves:
 *   1. a `.cell`-wrapped capture splices (cell replaced with executed output);
 *   2. a PROSE-shaped capture (bare paragraph — neither a `.cell` Div nor a
 *      `RawBlock` island — the OLD echo fixture shape that reproduced Bug B) is
 *      a documented NO-OP (cell survives as raw source); a prose block is never
 *      mis-paired as a cell's output;
 *   3. end-to-end: the REAL committed echo-engine fixture now emits a `.cell`
 *      wrapper, so a recorded echo capture splices (deno-gated);
 *   4. SC22 (`marimo_shaped_capture_splices`): a marimo-shaped `RawBlock` island
 *      capture splices — the unwrapped-engine-output leg of the contract.
 *
 * Revert hunk → RED: narrow `is_engine_output_block` back to `is_cell_wrapper`
 * (drop the `RawBlock` arm) and SC22 goes RED (marimo cell survives). Make it
 * stop recognizing `.cell` Divs and tests (1) + (3) go RED. Revert the echo
 * fixture to emit a bare paragraph, and (3) goes RED.
 */

#![cfg(not(target_arch = "wasm32"))]

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::engine::capture_splice::{apply_capture_splice, engine_cell_lang};
use quarto_core::engine::preview_record::record_capture;
use quarto_core::project::ProjectContext;
use quarto_pandoc_types::{Block, Pandoc};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn deno_available() -> bool {
    Command::new("deno")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn parse(md: &str) -> Pandoc {
    pampa::readers::qmd::read(
        md.as_bytes(),
        false,
        "seam.qmd",
        &mut std::io::sink(),
        false,
        None,
    )
    .unwrap()
    .0
}

/// A `{lang}` engine cell survived the splice as raw source (i.e. the splice did
/// NOT replace it with the engine's output wrapper).
fn cell_survived(p: &Pandoc) -> bool {
    p.blocks.iter().any(|b| engine_cell_lang(b).is_some())
}

fn debug_contains(p: &Pandoc, token: &str) -> bool {
    format!("{:?}", p.blocks).contains(token)
}

// (1) A `.cell`-wrapped capture (the shape real engines emit) splices: the
// source cell is replaced and the executed output is present.
#[test]
fn cell_wrapped_capture_splices() {
    let a1 = parse("```{echo}\nSRC\n```\n");
    let b1 =
        parse("::: {.cell}\n::: {.cell-output .cell-output-stdout}\nOUTPUT_MARKER\n:::\n:::\n");
    let a2 = parse("```{echo}\nSRC\n```\n");

    let out = apply_capture_splice(a2, &a1, &b1, "echo");

    assert!(
        !cell_survived(&out),
        "a `.cell`-wrapped capture must replace the source cell (splice fired); \
         blocks: {:?}",
        out.blocks
    );
    assert!(
        debug_contains(&out, "OUTPUT_MARKER"),
        "spliced output must contain the engine's executed marker; blocks: {:?}",
        out.blocks
    );
}

// (2) A PROSE-shaped capture (a bare paragraph — no `.cell` wrapper and not a
// `RawBlock` island — the OLD echo fixture shape that reproduced Bug B) is a
// documented NO-OP: the cell survives as raw source. This pins the splice's
// engine-output contract at the prose boundary: the matched B1 block must be an
// engine-output block (a `.cell` Div OR a `RawBlock` island — see
// `marimo_shaped_capture_splices`, where a `RawBlock` island now DOES splice);
// a prose block must never be mis-paired as a cell's output. A future change to
// that contract lands deliberately, not silently.
#[test]
fn bare_paragraph_capture_is_a_documented_noop() {
    let a1 = parse("```{echo}\nSRC\n```\n");
    let b1 = parse("**OUTPUT_MARKER**\n");
    let a2 = parse("```{echo}\nSRC\n```\n");

    let out = apply_capture_splice(a2, &a1, &b1, "echo");

    assert!(
        cell_survived(&out),
        "a bare-paragraph capture (no `.cell` wrapper) must be a no-op — the cell \
         survives as raw source; blocks: {:?}",
        out.blocks
    );
}

// (3) End-to-end: the REAL committed echo-engine fixture emits a `.cell` wrapper,
// so a recorded echo capture splices. Deno-gated (echo runs on the Deno host).
#[test]
fn real_echo_capture_splices() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — real_echo_capture_splices");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let ext_src =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/extensions/echo-engine");
    let dest = tmp.path().join("_extensions/echo-engine");
    copy_dir(&ext_src, &dest);
    crate::engine_fixture_build::ensure_bundle(&dest, "echo-engine");
    let input = tmp.path().join("index.qmd");
    std::fs::write(
        &input,
        "---\ntitle: PC-B\n---\n\n# Heading\n\n```{echo}\nPCB_SOURCE_TOKEN\n```\n",
    )
    .unwrap();

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project =
        ProjectContext::discover(&input, runtime.as_ref()).expect("discover echo project");
    let captures = pollster::block_on(record_capture(&input, &project, runtime.clone(), None))
        .expect("record echo capture");
    let cap = captures.first().expect("echo produced a capture");
    let result_md = cap
        .result
        .get("markdown")
        .and_then(|v| v.as_str())
        .expect("capture result.markdown");

    // Splice with A2 = parse(input_qmd), exactly as the WASM preview path does.
    let out = apply_capture_splice(
        parse(&cap.input_qmd),
        &parse(&cap.input_qmd),
        &parse(result_md),
        &cap.engine_name,
    );

    assert!(
        !cell_survived(&out),
        "the real echo capture must splice (source `{{echo}}` cell replaced); the \
         fixture must emit a `::: {{.cell}}` wrapper. result.markdown:\n{result_md}"
    );
    assert!(
        debug_contains(&out, "ECHO_EXECUTED"),
        "spliced output must contain ECHO_EXECUTED; result.markdown:\n{result_md}"
    );
}

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

// (SC22 — Plan 4c.2, bd-5jxcio5d) A marimo-shaped capture splices: the source
// engine cell is replaced by the engine's *unwrapped* output block.
//
// ## Why this is the RED that Phase A2's fix must turn GREEN
//
// The splice's `is_cell_wrapper` (capture_splice.rs) only recognizes a
// `::: {.cell}` Div as an engine cell's output. Echo/julia emit that wrapper;
// **marimo does not** — each executed cell is a bare ```` ```{=html} ```` block
// (a `RawBlock` with format `"html"`) carrying `<marimo-island>` /
// `<marimo-cell-output>` custom elements, ZERO `.cell` Divs. So today the
// matcher finds no wrapper at the cell's B1 position and the cell falls through
// to raw source in the preview pane (FINDING #5).
//
// ## Faithfulness of the synthetic B1 (Phase A0 characterization)
//
// The `B1` below is not invented — it is the verbatim shape a REAL marimo
// capture produces, recorded 2026-07-07 via `record_capture` against the
// committed marimo fixture for the exact `# SC22 heading` + `{python .marimo}`
// `40 + 2` doc used here (evidence in `.superpowers/sdd/a0-report.md`):
//   1. each executed cell is exactly ONE `RawBlock{format:"html"}` island at
//      the cell's position;
//   2. the `# SC22 heading` passes through as a `Header` block, lockstep with
//      `A1` (so `B1 = [Header, RawBlock]`, matching `A1 = [Header, CodeBlock]`);
//   3. the `__MARIMO_EXPORT_CONTEXT__` / `<marimo-code>` HEADER markers are
//      ABSENT from `B1.blocks` — they flow through `include-in-header` (the
//      HTML `<head>`), NOT the markdown body. (`<marimo-cell-code>` *does*
//      appear INSIDE the island RawBlock — the hidden per-cell source — which
//      is a different tag from the header `<marimo-code>`.)
// `B1` is built by `parse()`-ing that markdown, exactly as the real preview
// path re-parses `capture.result.markdown` — so it exercises the same
// `RawBlock`-producing parse, not a hand-built AST.
//
// ## Companion refactor-vacuity guards (frozen SC22 spec)
//
// The A2 generalization must NOT break `.cell`-wrapper matching. The two
// existing tests above — `cell_wrapped_capture_splices` (synthetic `.cell`
// splice) and `real_echo_capture_splices` (the real echo fixture, deno-gated) —
// ARE this test's companion guards: they must stay GREEN under the fix. They
// are not duplicated here; SC22 (native, unconditional) + SC21 (real marimo,
// e2e) bind as the frozen set — SC22 guards the matcher logic, SC21 guards that
// this synthetic B1 matched reality.
//
// NOTE: RED until the bd-5jxcio5d fix (Phase A2) lands — today `out.blocks[1]`
// is the raw `{python .marimo}` CodeBlock (cell survived), not the island.
#[test]
fn marimo_shaped_capture_splices() {
    // A1 = capture's pre-engine AST: [Header, {python .marimo} cell].
    let a1_md = "# SC22 heading\n\n```{python .marimo}\nimport marimo as mo\n40 + 2\n```\n";
    // B1 = capture's post-engine AST: the heading passes through; the cell
    // became a bare `{=html}` marimo island (verbatim from the A0 real capture).
    let b1_md = "# SC22 heading\n\n```{=html}\n<marimo-island\n    data-app-id=\"main\"\n    data-cell-id=\"Hbol\"\n    data-reactive=\"true\"\n>\n    <marimo-cell-output>\n    <pre class='text-xs'>42</pre>\n    </marimo-cell-output>\n    <marimo-cell-code hidden>import%20marimo%20as%20mo%0A40%20%2B%202</marimo-cell-code>\n</marimo-island>\n```\n";

    let a1 = parse(a1_md);
    let b1 = parse(b1_md);
    let a2 = parse(a1_md); // A2 == A1 (unedited)

    let out = apply_capture_splice(a2, &a1, &b1, "marimo");

    assert_eq!(out.blocks.len(), 2, "blocks: {:?}", out.blocks);
    // The cell's spliced block must be the marimo island (unwrapped engine
    // output), NOT the raw source CodeBlock.
    assert!(
        !cell_survived(&out),
        "a marimo-shaped capture (bare `{{=html}}` island, no `.cell` wrapper) \
         must replace the source cell with the engine's output — the cell must \
         NOT survive as raw source; blocks: {:?}",
        out.blocks
    );
    assert!(
        !matches!(out.blocks[1], Block::CodeBlock(_)),
        "the cell position must hold the spliced island, not a CodeBlock; \
         blocks: {:?}",
        out.blocks
    );
    let cell_dbg = format!("{:?}", out.blocks[1]);
    assert!(
        cell_dbg.contains("marimo-cell-output") && cell_dbg.contains("42"),
        "the spliced block must be the marimo island carrying the executed \
         value (`marimo-cell-output` + `42`); got: {cell_dbg}"
    );
}
