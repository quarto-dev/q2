//! Native driver for measuring corpus parse throughput at a controlled
//! thread count. Built to quantify the bd-2ercw whitespace-regex recompile
//! fix in two regimes — fully serial and N-way parallel.
//!
//! It parses every `*.qmd` file in a directory through the same
//! `quarto_core::pipeline::parse_qmd_to_ast` path that `q2 render` Pass 2
//! and the hub-client both use (the path containing `native_visitor`, where
//! the recompile bug lived), spreading the file list across `threads` OS
//! worker threads.
//!
//! Usage:
//!     parse-corpus <dir> [threads] [iterations]
//!
//! Reports wall time and the process-global
//! `WHITESPACE_RE_COMPILE_COUNT` (which must be 1 after the fix, regardless
//! of thread count or file count).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use pampa::pandoc::treesitter::WHITESPACE_RE_COMPILE_COUNT;
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

        ..Default::default()
    }
}

/// Parse a single document through the full parse pass and serialize to
/// JSON (mirrors the hub-client / Pass-2 parse path). Returns JSON byte
/// length so the optimizer can't elide the work.
fn parse_one(content: &[u8]) -> usize {
    let virtual_path = Path::new("/input.qmd");
    let project = make_project_context(virtual_path);
    let doc = DocumentInfo::from_path(virtual_path);
    let binaries = BinaryDependencies::new();
    let format = Format::from_format_string("html").expect("html format");
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());

    let options = RenderOptions {
        verbose: false,
        execute: false,
        use_freeze: false,
        output_path: None,
    };
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries).with_options(options);

    // The recompile work we are measuring happens inside this call
    // (`native_visitor`). A handful of corpus files fail later pipeline
    // stages or the JSON writer; tolerate those — the parse pass still ran.
    let result = match pollster::block_on(quarto_core::pipeline::parse_qmd_to_ast(
        content,
        "/input.qmd",
        &mut ctx,
        Arc::clone(&runtime),
    )) {
        Ok(r) => r,
        Err(_) => return 0,
    };

    // Cheap anti-DCE: touch the parsed AST so the optimizer can't elide the
    // parse. We deliberately avoid JSON serialization here — it would add
    // hundreds of MB of writer work that dilutes the parse-path signal we
    // are isolating.
    result.ast.blocks.len()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let dir = args
        .next()
        .ok_or("usage: parse-corpus <dir> [threads] [iterations]")?;
    let threads: usize = args
        .next()
        .as_deref()
        .map(str::parse)
        .transpose()?
        .unwrap_or(1);
    let iterations: usize = args
        .next()
        .as_deref()
        .map(str::parse)
        .transpose()?
        .unwrap_or(1);

    // Read all .qmd files up front (I/O is not what we're measuring).
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "qmd"))
        .collect();
    paths.sort();
    let contents: Vec<Vec<u8>> = paths.iter().filter_map(|p| std::fs::read(p).ok()).collect();
    let contents = Arc::new(contents);

    let start = Instant::now();
    let mut total_bytes = 0usize;
    for _ in 0..iterations {
        if threads <= 1 {
            for c in contents.iter() {
                total_bytes += parse_one(c);
            }
        } else {
            // Static round-robin slicing across `threads` workers.
            let mut handles = Vec::new();
            for t in 0..threads {
                let contents = Arc::clone(&contents);
                handles.push(std::thread::spawn(move || {
                    let mut bytes = 0usize;
                    let mut i = t;
                    while i < contents.len() {
                        bytes += parse_one(&contents[i]);
                        i += threads;
                    }
                    bytes
                }));
            }
            for h in handles {
                total_bytes += h.join().expect("worker thread panicked");
            }
        }
    }
    let elapsed = start.elapsed();

    let compiles = WHITESPACE_RE_COMPILE_COUNT.load(Ordering::Relaxed);
    eprintln!(
        "parse-corpus dir={dir} files={} threads={threads} iterations={iterations} \
         wall_ms={:.1} whitespace_re_compiles={compiles} ast_block_checksum={total_bytes}",
        contents.len(),
        elapsed.as_secs_f64() * 1000.0,
    );
    Ok(())
}
