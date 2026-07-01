/*
 * tests/integration/capture_splice_seam.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * PC-B (bd-h4rhohhy, Bug B): binds the q2-preview capture-splice contract both
 * ways. The splice (`apply_capture_splice` / `derive_cell_outputs` /
 * `is_cell_wrapper` in `quarto-core/src/engine/capture_splice.rs`) maps each
 * engine cell to the next `::: {.cell}` wrapper a real engine emits. This file
 * proves:
 *   1. a `.cell`-wrapped capture splices (cell replaced with executed output);
 *   2. a bare-paragraph capture is a documented NO-OP (cell survives as raw
 *      source) — the shape the OLD echo fixture emitted, which reproduced Bug B;
 *   3. end-to-end: the REAL committed echo-engine fixture now emits a `.cell`
 *      wrapper, so a recorded echo capture splices (deno-gated).
 *
 * Revert hunk → RED: change `is_cell_wrapper` (capture_splice.rs) to stop
 * recognizing `.cell` Divs, and tests (1) + (3) go RED (cell survives). Revert
 * the echo fixture to emit a bare paragraph, and (3) goes RED.
 */

#![cfg(not(target_arch = "wasm32"))]

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::engine::capture_splice::{apply_capture_splice, engine_cell_lang};
use quarto_core::engine::preview_record::record_capture;
use quarto_core::project::ProjectContext;
use quarto_pandoc_types::Pandoc;
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

// (2) A bare-paragraph capture (no `.cell` wrapper — the OLD echo fixture shape
// that reproduced Bug B) is a documented NO-OP: the cell survives as raw source.
// This pins the splice's `.cell`-requirement contract so a future change to it
// lands deliberately, not silently.
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
    copy_dir(&ext_src, &tmp.path().join("_extensions/echo-engine"));
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
