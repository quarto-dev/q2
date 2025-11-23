//! Error code example.
//!
//! This example shows how to use error codes from the catalog and get documentation URLs.

use quarto_error_reporting::{DiagnosticMessageBuilder, catalog};

fn main() {
    println!("=== Example 1: Using an error code ===\n");

    let error = DiagnosticMessageBuilder::error("Internal Error")
        .with_code("Q-0-1")
        .problem("An internal error occurred")
        .add_detail("This is a bug in Quarto")
        .add_hint("Please report this issue with the steps to reproduce")
        .build();

    println!("{}", error.to_text(None));

    // Get docs URL
    if let Some(url) = error.docs_url() {
        println!("\nDocumentation: {}", url);
    }

    println!("\n=== Example 2: Looking up error info from catalog ===\n");

    if let Some(info) = catalog::get_error_info("Q-0-1") {
        println!("Error code: Q-0-1");
        println!("Subsystem: {}", info.subsystem);
        println!("Title: {}", info.title);
        println!("Template: {}", info.message_template);
        if let Some(url) = &info.docs_url {
            println!("Docs URL: {}", url);
        }
        println!("Since version: {}", info.since_version);
    }

    println!("\n=== Example 3: YAML error with code ===\n");

    let yaml_error = DiagnosticMessageBuilder::error("YAML Syntax Error")
        .with_code("Q-1-1")
        .problem("Invalid YAML syntax in configuration file")
        .add_detail("Unexpected character '}' at line 5")
        .add_info("YAML does not use braces for objects")
        .add_hint("Remove the braces and use indentation instead")
        .build();

    println!("{}", yaml_error.to_text(None));

    println!("\n=== Example 4: Browsing the catalog ===\n");

    // List all markdown parsing errors (Q-2-*)
    println!("Markdown parsing errors in catalog:");
    let mut markdown_codes: Vec<_> = catalog::ERROR_CATALOG
        .keys()
        .filter(|k| k.starts_with("Q-2-"))
        .collect();
    markdown_codes.sort();

    for code in markdown_codes.iter().take(5) {
        if let Some(info) = catalog::ERROR_CATALOG.get(*code) {
            println!("  {} - {}", code, info.title);
        }
    }
    println!("  ... and {} more", markdown_codes.len().saturating_sub(5));
}
