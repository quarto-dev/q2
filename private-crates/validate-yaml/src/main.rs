use anyhow::{Context, Result};
use clap::Parser;
use quarto_yaml_validation::{Schema, SchemaRegistry, ValidationDiagnostic, validate};
use std::fs;
use std::path::PathBuf;
use std::process;

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

    /// Output errors as JSON instead of text
    #[arg(long)]
    json: bool,
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
    let schema_yaml = quarto_yaml::parse_file(&schema_content, schema_filename).map_err(|e| {
        anyhow::anyhow!(
            "Failed to parse schema file {}: {}",
            args.schema.display(),
            e
        )
    })?;

    let schema = Schema::from_yaml(&schema_yaml).map_err(|e| {
        anyhow::anyhow!(
            "Failed to load schema from {}: {}",
            args.schema.display(),
            e
        )
    })?;

    // Read the input document
    let input_content = fs::read_to_string(&args.input)
        .with_context(|| format!("Failed to read input file: {}", args.input.display()))?;

    let input_filename = args
        .input
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("input.yaml");

    // Parse the input document
    let input_yaml = quarto_yaml::parse_file(&input_content, input_filename).map_err(|e| {
        anyhow::anyhow!("Failed to parse input file {}: {}", args.input.display(), e)
    })?;

    // Create a SourceContext and register the input file
    // This enables proper file name and line/column tracking in error messages
    let mut source_ctx = quarto_source_map::SourceContext::new();

    // Compute the same FileId that quarto-yaml uses (hash of filename)
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    input_filename.hash(&mut hasher);
    let expected_file_id = quarto_source_map::FileId(hasher.finish() as usize);

    // Register the file with the computed FileId
    let file_id = source_ctx.add_file_with_id(
        expected_file_id,
        args.input.to_string_lossy().to_string(),
        Some(input_content.clone())
    );

    // Verify we got the expected file_id
    debug_assert_eq!(file_id, expected_file_id,
        "FileId mismatch: quarto-yaml will use {:?} but SourceContext has {:?}",
        expected_file_id, file_id);

    // Create a schema registry (empty for now, but needed for $ref resolution)
    let registry = SchemaRegistry::new();

    // Validate the document against the schema
    match validate(&input_yaml, &schema, &registry, &source_ctx) {
        Ok(()) => {
            if args.json {
                // JSON success output
                println!(r#"{{"success": true}}"#);
            } else {
                // Human-readable success output
                println!("✓ Validation successful");
                println!("  Input: {}", args.input.display());
                println!("  Schema: {}", args.schema.display());
            }
            Ok(())
        }
        Err(error) => {
            // Convert ValidationError to ValidationDiagnostic
            let diagnostic = ValidationDiagnostic::from_validation_error(&error, &source_ctx);

            if args.json {
                // JSON error output with structured paths and source ranges
                let json = serde_json::json!({
                    "success": false,
                    "errors": [diagnostic.to_json()]
                });
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else {
                // Human-readable error output with ariadne-style rendering
                eprint!("{}", diagnostic.to_text(&source_ctx));
            }
            process::exit(1);
        }
    }
}
