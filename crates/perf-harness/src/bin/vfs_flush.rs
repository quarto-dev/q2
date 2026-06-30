//! Native driver for the bd-q3bxnq2e VFS artifact-flush investigation.
//!
//! Mirrors the WASM single-doc render tail in
//! `crates/wasm-quarto-hub-client/src/lib.rs` (`render_qmd`):
//!
//!   1. `RenderContext` with a `vfs_root("/.quarto/project-artifacts")`
//!      resolver (Phase 5 contract);
//!   2. `quarto_core::pipeline::render_qmd_to_html` — the same pipeline
//!      the hub-client runs per keystroke, producing `ctx.artifacts`;
//!   3. the unconditional flush loop (lib.rs:1417-1425 at the time of
//!      writing): for every artifact with a path and non-empty content,
//!      `content.clone()` + insert into the session-persistent
//!      `VirtualFileSystem`.
//!
//! The VFS persists across iterations — exactly like `WasmRuntime`'s
//! session VFS across keystroke renders — so from iteration 2 onward
//! every flushed byte is a byte-identical re-write, which is the waste
//! this investigation quantifies.
//!
//! Usage:
//!     vfs-flush <fixture.qmd> [iterations] [pad_bytes] [mode]
//!
//! `pad_bytes` (default 0) adds one synthetic binary artifact of that
//! size to the store after each render, with identical bytes every
//! iteration. This is the geometric-scaling knob for total artifact
//! bytes (stands in for plot images / webfonts that a heavier document
//! would accumulate).
//!
//! `mode` selects the flush implementation for before/after timing in
//! a single binary/session:
//!   - `legacy` (default): the pre-bd-q3bxnq2e unconditional
//!     clone+insert loop, preserved inline below;
//!   - `skip`: `quarto_core::flush_artifacts_to_vfs` — the shared
//!     change-aware flush the WASM render tail now uses.
//!
//! Output (stderr, machine-readable): one `perf.vfs-flush` line per
//! iteration with render/flush wall times and byte counters diffed from
//! `VirtualFileSystem::write_stats()`.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use quarto_core::{
    Artifact, BinaryDependencies, DocumentInfo, Format, HtmlRenderConfig, ProjectConfig,
    ProjectContext, RenderContext, RenderOptions, ResourceResolverContext, render_qmd_to_html,
};
use quarto_system_runtime::{NativeRuntime, SystemRuntime, VirtualFileSystem};

fn make_project_context(path: &Path) -> ProjectContext {
    let dir = path.parent().unwrap_or(Path::new("/")).to_path_buf();
    ProjectContext {
        dir: dir.clone(),
        config: ProjectConfig::default(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path(path)],
        output_dir: dir,

        ..Default::default()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let fixture_arg = args
        .next()
        .ok_or("usage: vfs-flush <fixture.qmd> [iterations] [pad_bytes] [legacy|skip]")?;
    let iterations: usize = args
        .next()
        .as_deref()
        .map(str::parse)
        .transpose()?
        .unwrap_or(5);
    let pad_bytes: usize = args
        .next()
        .as_deref()
        .map(str::parse)
        .transpose()?
        .unwrap_or(0);
    let mode = args.next().unwrap_or_else(|| "legacy".to_string());
    if mode != "legacy" && mode != "skip" {
        return Err(format!("mode must be 'legacy' or 'skip', got '{mode}'").into());
    }

    let content = std::fs::read(&fixture_arg)?;
    let virtual_path = Path::new("/input.qmd");

    let project = make_project_context(virtual_path);
    let doc = DocumentInfo::from_path(virtual_path);
    let binaries = BinaryDependencies::new();
    let format = Format::from_format_string("html")
        .map_err(|e| format!("Format::from_format_string(html): {}", e))?;

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());

    // Session-persistent VFS, like WasmRuntime's across keystrokes.
    let mut vfs = VirtualFileSystem::new();

    for iter in 0..iterations {
        let options = RenderOptions {
            verbose: false,
            execute: false,
            use_freeze: false,
            output_path: None,
        };
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries).with_options(options);

        // Phase 5 VFS-root resolver — mirrors lib.rs `render_qmd`.
        let resolver = ResourceResolverContext::vfs_root("/.quarto/project-artifacts");
        ctx.resource_resolver = Some(resolver.clone());
        let config = HtmlRenderConfig::with_resolver(resolver.clone());

        let t_render = Instant::now();
        let out = pollster::block_on(render_qmd_to_html(
            &content,
            "/input.qmd",
            &mut ctx,
            &config,
            Arc::clone(&runtime),
        ))
        .map_err(|e| format!("render_qmd_to_html: {}", e))?;
        let render_us = t_render.elapsed().as_secs_f64() * 1e6;

        // Synthetic padding artifact: identical bytes each iteration,
        // standing in for stable plot images / webfonts. The store()
        // is summed into producer_bytes, like any real producer.
        if pad_bytes > 0 {
            ctx.artifacts.store(
                "pad:blob",
                Artifact::from_bytes(vec![0xAB; pad_bytes], "application/octet-stream")
                    .with_path("pad/blob.bin"),
            );
        }

        // Producer-side cost (bd-w5qyuzeg): bytes materialized into the
        // store this render. Summed here in the driver — ArtifactStore
        // itself carries no counters (a Drop-based gauge was tried and
        // removed; see the plan's "Instrumentation trim" note).
        let producer_stores = ctx.artifacts.len();
        let producer_bytes: usize = ctx.artifacts.iter().map(|(_, a)| a.content.len()).sum();
        let before = vfs.write_stats();

        // === The flush under investigation ===
        let t_flush = Instant::now();
        match mode.as_str() {
            // The pre-bd-q3bxnq2e behavior: byte-for-byte mirror of the
            // unconditional loop that lived at wasm lib.rs:1417-1425
            // (including the bd-3gtn empty-content skip), with
            // `vfs.add_file` standing in for `runtime.add_file` (a
            // one-line RwLock passthrough).
            "legacy" => {
                for (_key, artifact) in ctx.artifacts.iter() {
                    if let Some(artifact_path) = &artifact.path {
                        if artifact.content.is_empty() {
                            continue;
                        }
                        let vfs_path = resolver.on_disk_path_for(artifact.scope, artifact_path);
                        vfs.add_file(&vfs_path, artifact.content.clone());
                    }
                }
            }
            // The fixed behavior: the exact shared function the WASM
            // render tail calls.
            _ => quarto_core::flush_artifacts_to_vfs(&ctx.artifacts, &resolver, &mut vfs),
        }
        let flush_us = t_flush.elapsed().as_secs_f64() * 1e6;

        let after = vfs.write_stats();
        eprintln!(
            "perf.vfs-flush mode={} iter={} render_us={:.0} flush_us={:.1} writes={} bytes_written={} skipped_writes={} bytes_skipped={} producer_stores={} producer_bytes={} html_bytes={}",
            mode,
            iter,
            render_us,
            flush_us,
            after.writes - before.writes,
            after.bytes_written - before.bytes_written,
            after.skipped_writes - before.skipped_writes,
            after.bytes_skipped - before.bytes_skipped,
            producer_stores,
            producer_bytes,
            out.html.len(),
        );
    }

    Ok(())
}
