//! Source location example.
//!
//! This example shows how to attach source location information to diagnostic messages
//! for integration with ariadne and source context rendering.

use quarto_error_reporting::DiagnosticMessageBuilder;
use quarto_source_map::{SourceContext, SourceInfo};

fn main() {
    println!("=== Example 1: Error with source location ===\n");

    // Create a source context
    let mut ctx = SourceContext::new();
    let file_id = ctx.add_file(
        "example.qmd".to_string(),
        Some("title: My Document\nauthor: John Doe\ndate: 2024-01-01\n".to_string()),
    );

    // Create a location (let's say there's an error in "My Document" - offsets 7 to 18)
    let location = SourceInfo::original(file_id, 7, 18);

    let error = DiagnosticMessageBuilder::error("Invalid title format")
        .with_code("Q-1-10")
        .with_location(location)
        .problem("Title must be a string, not a complex object")
        .add_detail("Title value starts at this location")
        .add_hint("Ensure the title is a simple quoted string")
        .build();

    // Render WITHOUT context - shows offset
    println!("Without context:");
    println!("{}", error.to_text(None));

    println!("\n---\n");

    // Render WITH context - shows file path and line:column
    println!("With context:");
    println!("{}", error.to_text(Some(&ctx)));

    println!("\n=== Example 2: Multiple locations ===\n");

    let another_ctx = SourceContext::new();

    // Note: This example shows the API, but without actual file content,
    // the rendering will still show offsets. In real usage with proper
    // SourceContext, this would show rich source snippets via ariadne.

    let location2 = SourceInfo::original(quarto_source_map::FileId(0), 100, 110);

    let error2 = DiagnosticMessageBuilder::error("Unclosed code block")
        .with_code("Q-2-301")
        .with_location(location2)
        .problem("Code block started but never closed")
        .add_detail("The opening ``` was found but no closing ``` before end of block")
        .add_hint("Add a closing ``` on a new line")
        .build();

    println!("{}", error2.to_text(Some(&another_ctx)));

    println!("\n=== Example 3: JSON output with location ===\n");

    let json = error.to_json();
    println!("{}", serde_json::to_string_pretty(&json).unwrap());
}
