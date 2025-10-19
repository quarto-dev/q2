mod error_codes;
mod error_conversion;

use anyhow::{Context, Result};
use clap::Parser;
use quarto_error_reporting::{DetailKind, DiagnosticMessage};
use quarto_yaml_validation::{Schema, SchemaRegistry, validate};
use std::fs;
use std::path::PathBuf;
use std::process;

use error_conversion::validation_error_to_diagnostic;

/// Validate a YAML document against a schema
#[derive(Parser, Debug)]
#[command(name = "validate-yaml")]
#[command(about = "Validate YAML documents against schemas", long_about = None)]
struct Args {
    /// Path to the YAML document to validate
    #[arg(long, value_name = "FILE")]
    input: PathBuf,

    /// Path to the YAML schema file
    #[arg(long, value_name = "FILE")]
    schema: PathBuf,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {:#}", e);
        process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Args::parse();

    // Read the schema file
    let schema_content = fs::read_to_string(&args.schema)
        .with_context(|| format!("Failed to read schema file: {}", args.schema.display()))?;

    let schema_filename = args
        .schema
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("schema.yaml");

    // Parse the schema file
    let schema_yaml = quarto_yaml::parse_file(&schema_content, schema_filename)
        .map_err(|e| anyhow::anyhow!("Failed to parse schema file {}: {}", args.schema.display(), e))?;

    let schema = Schema::from_yaml(&schema_yaml)
        .map_err(|e| anyhow::anyhow!("Failed to load schema from {}: {}", args.schema.display(), e))?;

    // Read the input document
    let input_content = fs::read_to_string(&args.input)
        .with_context(|| format!("Failed to read input file: {}", args.input.display()))?;

    let input_filename = args
        .input
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("input.yaml");

    // Parse the input document
    let input_yaml = quarto_yaml::parse_file(&input_content, input_filename)
        .map_err(|e| anyhow::anyhow!("Failed to parse input file {}: {}", args.input.display(), e))?;

    // Create a schema registry (empty for now, but needed for $ref resolution)
    let registry = SchemaRegistry::new();

    // Validate the document against the schema
    match validate(&input_yaml, &schema, &registry) {
        Ok(()) => {
            println!("✓ Validation successful");
            println!("  Input: {}", args.input.display());
            println!("  Schema: {}", args.schema.display());
            Ok(())
        }
        Err(error) => {
            // Convert ValidationError to DiagnosticMessage for better presentation
            let diagnostic = validation_error_to_diagnostic(&error);
            display_diagnostic(&diagnostic);
            process::exit(1);
        }
    }
}

/// Display a diagnostic message in tidyverse style.
///
/// This provides a simple text-based rendering. In Phase 2, this could be replaced
/// with ariadne for visual error reports with source context.
fn display_diagnostic(diagnostic: &DiagnosticMessage) {
    // Title with error code
    if let Some(code) = &diagnostic.code {
        eprintln!("Error: {} ({})", diagnostic.title, code);
    } else {
        eprintln!("Error: {}", diagnostic.title);
    }
    eprintln!();

    // Problem statement
    if let Some(problem) = &diagnostic.problem {
        eprintln!("Problem: {}", problem.as_str());
        eprintln!();
    }

    // Details with bullets
    if !diagnostic.details.is_empty() {
        for detail in &diagnostic.details {
            let bullet = match detail.kind {
                DetailKind::Error => "  ✖",
                DetailKind::Info => "  ℹ",
                DetailKind::Note => "  •",
            };
            eprintln!("{} {}", bullet, detail.content.as_str());
        }
        eprintln!();
    }

    // Hints
    if !diagnostic.hints.is_empty() {
        for hint in &diagnostic.hints {
            eprintln!("  ? {}", hint.as_str());
        }
        eprintln!();
    }

    // Documentation link
    if let Some(url) = diagnostic.docs_url() {
        eprintln!("See {} for more information", url);
        eprintln!();
    }
}
