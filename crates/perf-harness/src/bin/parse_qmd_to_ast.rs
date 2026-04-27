//! Native driver for profiling the hub-client's `parse_qmd_to_ast` path.
//!
//! Mirrors the call chain in `crates/wasm-quarto-hub-client/src/lib.rs::parse_qmd_to_ast`:
//!
//!   1. `quarto_core::pipeline::parse_qmd_to_ast` (Parse → EngineExecution → MetadataMerge)
//!   2. Build an `ASTContext` from the returned `SourceContext`
//!   3. Serialize via `pampa::writers::json::write_with_config` with
//!      `JsonConfig { include_inline_locations: true }` (hub-client setting)
//!
//! Usage:
//!     parse-qmd-to-ast <fixture.qmd> [iterations]
//!
//! Designed for use under `samply record`. The iteration loop gives samply
//! enough samples when the fixture is small; each iteration repeats the full
//! pipeline from scratch. Stdout is kept quiet; result size is reported on
//! stderr so samply's output stream is not polluted.

use std::path::Path;
use std::sync::Arc;

use pampa::pandoc::ASTContext;
use quarto_core::{
    BinaryDependencies, DocumentInfo, Format, ProjectConfig, ProjectContext, RenderContext,
    RenderOptions,
};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn make_project_context(path: &Path) -> ProjectContext {
    let dir = path.parent().unwrap_or(Path::new("/")).to_path_buf();
    ProjectContext {
        dir: dir.clone(),
        config: ProjectConfig::default(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path(path)],
        output_dir: dir,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let fixture_arg = args
        .next()
        .ok_or("usage: parse-qmd-to-ast <fixture.qmd> [iterations]")?;
    let iterations: usize = args
        .next()
        .as_deref()
        .map(str::parse)
        .transpose()?
        .unwrap_or(1);

    let content = std::fs::read(&fixture_arg)?;
    let virtual_path = Path::new("/input.qmd");

    let project = make_project_context(virtual_path);
    let doc = DocumentInfo::from_path(virtual_path);
    let binaries = BinaryDependencies::new();
    let format = Format::from_format_string("html")
        .map_err(|e| format!("Format::from_format_string(html): {}", e))?;

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());

    let mut last_json_len = 0usize;

    for _ in 0..iterations {
        let options = RenderOptions {
            verbose: false,
            execute: false,
            use_freeze: false,
            output_path: None,
        };
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries).with_options(options);

        let result = pollster::block_on(quarto_core::pipeline::parse_qmd_to_ast(
            &content,
            "/input.qmd",
            &mut ctx,
            Arc::clone(&runtime),
        ))
        .map_err(|e| format!("parse_qmd_to_ast: {}", e))?;

        let ast_context = ASTContext {
            filenames: vec!["/input.qmd".to_string()],
            example_list_counter: std::cell::Cell::new(1),
            source_context: result.source_context.clone(),
            parent_source_info: None,
        };

        let mut buf = Vec::new();
        let json_config = pampa::writers::json::JsonConfig {
            include_inline_locations: true,
        };
        pampa::writers::json::write_with_config(&result.ast, &ast_context, &mut buf, &json_config)
            .map_err(|e| format!("json::write_with_config: {:?}", e))?;

        last_json_len = buf.len();
    }

    eprintln!(
        "parse-qmd-to-ast fixture={} iterations={} last_json_bytes={}",
        fixture_arg, iterations, last_json_len
    );

    Ok(())
}
