//! Basic error message example.
//!
//! This example shows the simplest way to create and display a diagnostic message.

use quarto_error_reporting::DiagnosticMessage;

fn main() {
    // Create a simple error message
    let error = DiagnosticMessage::error("File not found");

    // Render to text
    println!("{}", error.to_text(None));
    println!();

    // Create a warning
    let warning = DiagnosticMessage::warning("Deprecated feature used");
    println!("{}", warning.to_text(None));
    println!();

    // Create an info message
    let info = DiagnosticMessage::info("Processing 42 files");
    println!("{}", info.to_text(None));
}
