//! Custom rendering options example.
//!
//! This example shows how to customize the output format using TextRenderOptions,
//! particularly useful for disabling terminal hyperlinks in snapshot tests.

use quarto_error_reporting::{DiagnosticMessageBuilder, TextRenderOptions};
use quarto_source_map::{SourceContext, SourceInfo};

fn main() {
    println!("=== Example 1: Default rendering (with hyperlinks) ===\n");

    let mut ctx = SourceContext::new();
    let file_id = ctx.add_file(
        "document.qmd".to_string(),
        Some("# My Document\n\nSome content here.\n".to_string()),
    );

    let location = SourceInfo::original(file_id, 15, 27);

    let error = DiagnosticMessageBuilder::error("Parse error")
        .with_code("Q-2-100")
        .with_location(location)
        .problem("Invalid markdown syntax")
        .add_hint("Check the markdown formatting")
        .build();

    // Default rendering includes OSC 8 hyperlinks for file paths
    let default_text = error.to_text(Some(&ctx));
    println!("{}", default_text);

    println!("\n=== Example 2: Rendering without hyperlinks (for tests) ===\n");

    // Disable hyperlinks - useful for snapshot testing where absolute paths
    // would cause differences between machines
    let options = TextRenderOptions {
        enable_hyperlinks: false,
    };

    let no_hyperlink_text = error.to_text_with_options(Some(&ctx), &options);
    println!("{}", no_hyperlink_text);

    println!("\n=== Example 3: Comparing outputs ===\n");

    // Show the difference in output
    println!("With hyperlinks enabled:");
    println!("  Length: {} bytes", default_text.len());
    println!(
        "  Contains OSC 8 codes: {}",
        default_text.contains("\x1b]8;")
    );

    println!("\nWith hyperlinks disabled:");
    println!("  Length: {} bytes", no_hyperlink_text.len());
    println!(
        "  Contains OSC 8 codes: {}",
        no_hyperlink_text.contains("\x1b]8;")
    );

    println!("\n=== Example 4: JSON output (no hyperlinks) ===\n");

    let json = error.to_json();
    println!("{}", serde_json::to_string_pretty(&json).unwrap());

    println!("\n=== Example 5: Multiple diagnostics with custom rendering ===\n");

    let error2 = DiagnosticMessageBuilder::error("Type mismatch")
        .with_code("Q-1-15")
        .problem("Expected string, found number")
        .add_detail("Value: 42")
        .add_detail("Expected type: string")
        .build();

    let error3 = DiagnosticMessageBuilder::error("Missing field")
        .with_code("Q-1-20")
        .problem("Required field 'author' not found")
        .add_hint("Add an 'author' field to your configuration")
        .build();

    let errors = vec![error, error2, error3];

    // Render all with consistent options
    let no_hyperlinks = TextRenderOptions {
        enable_hyperlinks: false,
    };

    for (i, err) in errors.iter().enumerate() {
        println!("Error {}:", i + 1);
        println!("{}", err.to_text_with_options(Some(&ctx), &no_hyperlinks));
        println!();
    }
}
