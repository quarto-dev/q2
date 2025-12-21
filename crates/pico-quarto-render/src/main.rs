/*
 * main.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Experimental prototype for rendering QMD files to HTML
 */

mod embedded_resolver;
mod format_writers;
mod template_context;

use anyhow::{Context, Result};
use clap::Parser;
use rayon::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use walkdir::WalkDir;

use embedded_resolver::EmbeddedResolver;
use format_writers::HtmlWriters;
use quarto_doctemplate::Template;
use template_context::{compile_template, prepare_template_metadata, render_with_template};

/// Result of processing a single QMD file.
struct ProcessResult {
    /// The input path that was processed.
    input_path: PathBuf,
    /// The result: Ok(output_path) or Err(error).
    result: Result<PathBuf>,
}

#[derive(Parser, Debug)]
#[command(name = "pico-quarto-render")]
#[command(about = "Experimental QMD to HTML batch renderer")]
struct Args {
    /// Input directory containing .qmd files
    #[arg(value_name = "INPUT_DIR")]
    input_dir: PathBuf,

    /// Output directory for .html files
    #[arg(value_name = "OUTPUT_DIR")]
    output_dir: PathBuf,

    /// Verbose output (can be used multiple times: -v, -vv)
    /// Level 1 (-v): Print filenames being rendered
    /// Level 2+ (-vv): Enable parser verbose mode
    #[arg(
        short = 'v',
        long = "verbose",
        action = clap::ArgAction::Count,
        help = "Verbose output (-v for filenames, -vv for parser debug)"
    )]
    verbose: u8,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Validate input directory exists
    if !args.input_dir.exists() {
        anyhow::bail!("Input directory does not exist: {:?}", args.input_dir);
    }

    if !args.input_dir.is_dir() {
        anyhow::bail!("Input path is not a directory: {:?}", args.input_dir);
    }

    // Create output directory if it doesn't exist
    fs::create_dir_all(&args.output_dir).context(format!(
        "Failed to create output directory: {:?}",
        args.output_dir
    ))?;

    // Find all .qmd files
    let qmd_files: Vec<PathBuf> = WalkDir::new(&args.input_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "qmd"))
        .map(|e| e.path().to_path_buf())
        .collect();

    if args.verbose >= 1 {
        eprintln!("Found {} .qmd files", qmd_files.len());
    }

    // Compile template once (shared across all threads)
    let template_source = embedded_resolver::get_main_template()
        .ok_or_else(|| anyhow::anyhow!("Main template not found"))?;
    let resolver = EmbeddedResolver;
    let template = compile_template(template_source, &resolver)?;

    // Check if single-threaded mode is requested (for cleaner profiling)
    let single_threaded = std::env::var("RAYON_NUM_THREADS")
        .map(|v| v == "1")
        .unwrap_or(false);

    // Process files, collecting results
    let results: Vec<ProcessResult> = if single_threaded {
        // Sequential processing (cleaner stack traces for profiling)
        qmd_files
            .iter()
            .map(|qmd_path| ProcessResult {
                input_path: qmd_path.clone(),
                result: process_qmd_file(qmd_path, &args.input_dir, &args.output_dir, &template),
            })
            .collect()
    } else {
        // Parallel processing with rayon
        // Template is now Send + Sync (SourceInfo uses Arc instead of Rc)
        let shared_template = Arc::new(template);
        qmd_files
            .par_iter()
            .map(|qmd_path| {
                let template = shared_template.as_ref();
                ProcessResult {
                    input_path: qmd_path.clone(),
                    result: process_qmd_file(qmd_path, &args.input_dir, &args.output_dir, template),
                }
            })
            .collect()
    };

    // Output results sequentially (preserves order, no interleaving)
    let mut success_count = 0;
    let mut error_count = 0;

    for process_result in results {
        match process_result.result {
            Ok(output_path) => {
                if args.verbose >= 1 {
                    eprintln!("Rendered {:?}", process_result.input_path);
                    eprintln!("  -> {:?}", output_path);
                }
                success_count += 1;
            }
            Err(e) => {
                eprintln!("✗ Error processing {:?}: {}", process_result.input_path, e);
                error_count += 1;
            }
        }
    }

    eprintln!(
        "\nProcessed {} files: {} succeeded, {} failed",
        success_count + error_count,
        success_count,
        error_count
    );

    if error_count > 0 {
        std::process::exit(1);
    }

    Ok(())
}

fn process_qmd_file(
    qmd_path: &Path,
    input_dir: &Path,
    output_dir: &Path,
    template: &Template,
) -> Result<PathBuf> {
    // Read the input file
    let input_content =
        fs::read(qmd_path).context(format!("Failed to read file: {:?}", qmd_path))?;

    // Parse QMD to AST
    let mut output_stream = std::io::sink();

    let (mut pandoc, _context, _warnings) = pampa::readers::qmd::read(
        &input_content,
        false, // loose mode
        qmd_path.to_str().unwrap_or("<unknown>"),
        &mut output_stream,
        true,
        None,
    )
    .map_err(|diagnostics| {
        // Format error messages using DiagnosticMessage API
        let error_text = diagnostics
            .iter()
            .map(|d| d.to_text(None))
            .collect::<Vec<_>>()
            .join("\n");
        anyhow::anyhow!("Parse errors:\n{}", error_text)
    })?;

    // Prepare template metadata (adds pagetitle from title, etc.)
    prepare_template_metadata(&mut pandoc);

    // Render with template
    let writers = HtmlWriters;
    let html_output = render_with_template(&pandoc, template, &writers)?;

    // Determine output path
    let relative_path = qmd_path
        .strip_prefix(input_dir)
        .context("Failed to compute relative path")?;

    let mut output_path = output_dir.join(relative_path);
    output_path.set_extension("html");

    // Create parent directories if needed
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .context(format!("Failed to create output directory: {:?}", parent))?;
    }

    // Write HTML to output file
    fs::write(&output_path, &html_output)
        .context(format!("Failed to write output file: {:?}", output_path))?;

    Ok(output_path)
}
