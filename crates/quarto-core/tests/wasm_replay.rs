//! WASM tests for stored-notebook replay (EXR-1b).
//!
//! These drive `IpynbReplayEngine` on the real wasm32 target with `WasmRuntime`: a stored
//! notebook's outputs render, and its figures land in the VFS through the runtime rather
//! than degrading to `[Image output]`.
//!
//! This is a top-level test target, not a module of `tests/integration/`, on purpose: the
//! integration binary depends on tokio, tempfile and other native-only dev-dependencies and
//! does not build for wasm32 (same reason as `crates/pampa/tests/wasm_lua.rs`). On native
//! targets this file compiles to an empty binary.
//!
//! **How to run:** (Linux/macOS only, requires nightly + Clang + wasm-sysroot)
//! ```text
//! CC_wasm32_unknown_unknown=clang \
//! CFLAGS_wasm32_unknown_unknown="-isystem $PWD/crates/wasm-quarto-hub-client/wasm-sysroot -fno-builtin" \
//! cargo test -p quarto-core --test wasm_replay --target wasm32-unknown-unknown \
//!   -Zbuild-std=std,panic_unwind,panic_abort
//! ```

#![cfg(target_arch = "wasm32")]

// Link the C stdlib shims (malloc, fprintf, snprintf, etc.) needed by tree-sitter on
// wasm32-unknown-unknown.
extern crate wasm_c_shim;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use quarto_core::engine::content_processors::ipynb::convert_notebook;
use quarto_core::engine::{ExecutionContext, ExecutionEngine, IpynbReplayEngine};
use quarto_source_map::SourceContext;
use quarto_system_runtime::{SystemRuntime, WasmRuntime};
use wasm_bindgen_test::*;

const STDOUT_NB: &str = r##"{"cells":[{"cell_type":"code","execution_count":1,"metadata":{},"outputs":[{"output_type":"stream","name":"stdout","text":["hello from the notebook\n"]}],"source":["print('hello from the notebook')\n"]}],"metadata":{"kernelspec":{"name":"python3","language":"python"}},"nbformat":4,"nbformat_minor":5}"##;

const PNG_OUTPUT_NB: &str = r##"{"cells":[{"cell_type":"code","execution_count":5,"metadata":{},"outputs":[{"output_type":"display_data","data":{"image/png":"iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==","text/plain":["<Figure>"]},"metadata":{}}],"source":["import matplotlib.pyplot as plt\n"]}],"metadata":{"kernelspec":{"name":"python3","language":"python"}},"nbformat":4,"nbformat_minor":5}"##;

/// A context mirroring what `ParseDocumentStage` registers: converted buffer at FileId(0),
/// the original notebook at `ORIGINAL_FILE_ID`, cell virtual files after. Returns the
/// context and the converted markdown (the engine's `input` in production).
fn ctx_for(notebook: &str, dir: &Path) -> (ExecutionContext, String) {
    let nb_path = dir.join("notebook.ipynb");
    let converted = convert_notebook(&nb_path, notebook).expect("notebook must convert");
    let mut sc = SourceContext::new();
    sc.add_file(
        "<notebook.ipynb (converted by ipynb)>".to_string(),
        Some(converted.markdown.clone()),
    );
    sc.add_file_with_id(
        quarto_core::engine::content_processors::ORIGINAL_FILE_ID,
        nb_path.display().to_string(),
        Some(notebook.to_string()),
    );
    for cell_file in &converted.files {
        sc.add_file(cell_file.label.clone(), Some(cell_file.text.clone()));
    }
    let ctx = ExecutionContext::new(dir.to_path_buf(), dir.to_path_buf(), nb_path, "html")
        .with_source_info(converted.source_info, Arc::new(sc));
    (ctx, converted.markdown)
}

/// A stored notebook renders its outputs on wasm32, with no kernel and no availability probe.
#[wasm_bindgen_test]
fn stored_notebook_renders_its_outputs() {
    let dir = PathBuf::from("/project");
    let (ctx, markdown) = ctx_for(STDOUT_NB, &dir);
    let runtime: Arc<dyn SystemRuntime> = Arc::new(WasmRuntime::new());

    let out = IpynbReplayEngine::with_runtime(runtime)
        .execute(&markdown, &ctx)
        .expect("replay must succeed");

    assert!(
        out.markdown.contains("hello from the notebook"),
        "stored stdout must appear, got:\n{}",
        out.markdown
    );
    assert!(
        out.markdown.contains(".cell-output-stdout"),
        "output must carry the canonical stdout class, got:\n{}",
        out.markdown
    );
}

/// A stored notebook's figures are written to the VFS through `WasmRuntime`, not degraded
/// to `[Image output]`.
#[wasm_bindgen_test]
fn stored_notebook_figures_are_written_to_the_vfs() {
    let dir = PathBuf::from("/project");
    let (ctx, markdown) = ctx_for(PNG_OUTPUT_NB, &dir);
    let runtime = Arc::new(WasmRuntime::new());
    let dyn_runtime: Arc<dyn SystemRuntime> = runtime.clone();

    let out = IpynbReplayEngine::with_runtime(dyn_runtime)
        .execute(&markdown, &ctx)
        .expect("replay must succeed");

    assert!(
        out.markdown
            .contains("![](notebook_files/figure-html/cell-1-output-1.png)"),
        "the figure must be referenced, not degraded to a placeholder; got:\n{}",
        out.markdown
    );
    assert!(
        !out.markdown.contains("[Image output]"),
        "figure must not degrade to a placeholder; got:\n{}",
        out.markdown
    );
    let png = runtime
        .file_read(&dir.join("notebook_files/figure-html/cell-1-output-1.png"))
        .expect("the figure must be in the VFS");
    assert_eq!(
        &png[..4],
        b"\x89PNG",
        "the VFS file must be the decoded PNG"
    );
}
