//! Diagnostic collector pattern example.
//!
//! This example shows how to use DiagnosticCollector to accumulate multiple errors
//! during processing - a common pattern in Quarto subsystems like YAML validation
//! and markdown parsing.
//!
//! Note: DiagnosticCollector is in quarto-markdown-pandoc, so this example shows
//! the pattern manually. In real code, use the DiagnosticCollector utility.

use quarto_error_reporting::{DiagnosticKind, DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_source_map::{SourceContext, SourceInfo};

/// Simple collector for accumulating diagnostic messages
struct SimpleCollector {
    diagnostics: Vec<DiagnosticMessage>,
}

impl SimpleCollector {
    fn new() -> Self {
        Self {
            diagnostics: Vec::new(),
        }
    }

    fn add(&mut self, diagnostic: DiagnosticMessage) {
        self.diagnostics.push(diagnostic);
    }

    fn error(&mut self, message: impl Into<String>) {
        self.add(DiagnosticMessage::error(message.into()));
    }

    fn warn(&mut self, message: impl Into<String>) {
        self.add(DiagnosticMessage::warning(message.into()));
    }

    fn error_at(&mut self, message: impl Into<String>, location: SourceInfo) {
        self.add(
            DiagnosticMessageBuilder::error(message.into())
                .with_location(location)
                .build(),
        );
    }

    fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.kind == DiagnosticKind::Error)
    }

    fn diagnostics(&self) -> &[DiagnosticMessage] {
        &self.diagnostics
    }

    fn to_text(&self, ctx: Option<&SourceContext>) -> Vec<String> {
        self.diagnostics.iter().map(|d| d.to_text(ctx)).collect()
    }
}

fn main() {
    println!("=== Example 1: Accumulating multiple errors ===\n");

    let mut collector = SimpleCollector::new();

    // Simulate validating a YAML file
    collector.error("Missing required field 'title'");
    collector.warn("Field 'description' is deprecated");
    collector.error("Invalid value for 'format': expected string, got number");

    if collector.has_errors() {
        println!(
            "Validation failed with {} diagnostics:",
            collector.diagnostics().len()
        );
        for text in collector.to_text(None) {
            println!("{}", text);
        }
    }

    println!("\n=== Example 2: Errors with source locations ===\n");

    let mut ctx = SourceContext::new();
    let file_id = ctx.add_file(
        "config.yml".to_string(),
        Some("title: 123\nformat: html\nauthor: John\n".to_string()),
    );

    let mut collector2 = SimpleCollector::new();

    // Error in "title: 123" (offsets 7-10)
    let loc1 = SourceInfo::original(file_id, 7, 10);
    collector2.error_at("Title must be a string", loc1);

    // Warning at "John" (offsets 33-37)
    let loc2 = SourceInfo::original(file_id, 33, 37);
    let warning = DiagnosticMessageBuilder::warning("Author field should include email")
        .with_location(loc2)
        .add_hint("Use format: 'Name <email@example.com>'")
        .build();
    collector2.add(warning);

    println!("Collected diagnostics:");
    for text in collector2.to_text(Some(&ctx)) {
        println!("{}", text);
        println!();
    }

    println!("=== Example 3: JSON output for all diagnostics ===\n");

    let json_array: Vec<_> = collector2
        .diagnostics()
        .iter()
        .map(|d| d.to_json())
        .collect();

    println!("{}", serde_json::to_string_pretty(&json_array).unwrap());

    println!("\n=== Example 4: Continuing vs. failing fast ===\n");

    let mut collector3 = SimpleCollector::new();

    // In some subsystems, we collect all errors before failing
    for i in 1..=3 {
        collector3.error(format!("Error in item {}", i));
    }

    // Check at the end
    if collector3.has_errors() {
        eprintln!(
            "Processing failed with {} errors",
            collector3.diagnostics().len()
        );
        eprintln!("\nErrors:");
        for diag in collector3.diagnostics() {
            eprintln!("  - {}", diag.title);
        }
    }
}
