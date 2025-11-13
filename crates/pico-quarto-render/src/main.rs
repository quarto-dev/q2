/*
 * main.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Experimental prototype for rendering QMD files to HTML
 */

use anyhow::{Context, Result};
use clap::Parser;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

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

    // Process each file
    let mut success_count = 0;
    let mut error_count = 0;

    for qmd_path in qmd_files {
        // Print filename at verbose level 1+
        if args.verbose >= 1 {
            eprintln!("Rendering {:?}", qmd_path);
        }

        match process_qmd_file(&qmd_path, &args.input_dir, &args.output_dir, args.verbose) {
            Ok(output_path) => {
                if args.verbose >= 1 {
                    eprintln!("  -> {:?}", output_path);
                }
                success_count += 1;
            }
            Err(e) => {
                eprintln!("✗ Error processing {:?}: {}", qmd_path, e);
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
    verbose: u8,
) -> Result<PathBuf> {
    // Read the input file
    let input_content =
        fs::read(qmd_path).context(format!("Failed to read file: {:?}", qmd_path))?;

    // Parse QMD to AST
    // Enable parser verbose mode at level 2+
    let mut output_stream: Box<dyn std::io::Write> = if verbose >= 2 {
        Box::new(std::io::stderr())
    } else {
        Box::new(std::io::sink())
    };

    let (pandoc, _context, warnings) = quarto_markdown_pandoc::readers::qmd::read(
        &input_content,
        false, // loose mode
        qmd_path.to_str().unwrap_or("<unknown>"),
        &mut output_stream,
        true,
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

    // Log warnings if verbose
    if verbose >= 2 {
        for warning in warnings {
            eprintln!("Warning: {}", warning.to_text(None));
        }
    }

    // Convert AST to HTML
    let mut html_buf = Vec::new();
    quarto_markdown_pandoc::writers::html::write(&pandoc, &mut html_buf)
        .context("Failed to write HTML")?;

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
    fs::write(&output_path, html_buf)
        .context(format!("Failed to write output file: {:?}", output_path))?;

    Ok(output_path)
}
