//! Migration helpers example.
//!
//! This example shows the generic_error! and generic_warning! macros used during
//! migration from old error systems. These macros create errors with code Q-0-99
//! and include file/line tracking for easier debugging during the transition period.

use quarto_error_reporting::{generic_error, generic_warning};

fn main() {
    println!("=== Example 1: Using generic_error! macro ===\n");

    // The generic_error! macro creates a DiagnosticMessage with:
    // - Code: Q-0-99 (generic migration error)
    // - File and line number where the macro was invoked
    // - The provided message
    let error = generic_error!("Something went wrong during migration");

    println!("{}", error.to_text(None));
    println!();

    // Check the error code
    println!("Error code: {:?}", error.code);
    println!();

    println!("=== Example 2: Using generic_warning! macro ===\n");

    let warning = generic_warning!("This feature is not yet fully migrated");

    println!("{}", warning.to_text(None));
    println!();

    println!("=== Example 3: Migration pattern in practice ===\n");

    // During migration, you might replace old error handling like this:
    //
    // OLD CODE:
    //   eprintln!("Error: File not found: {}", path);
    //   return Err(...);
    //
    // NEW CODE (migration phase):
    //   let error = generic_error!(format!("File not found: {}", path));
    //   eprintln!("{}", error.to_text(None));
    //   return Err(...);
    //
    // FINAL CODE:
    //   let error = DiagnosticMessageBuilder::error("File not found")
    //       .with_code("Q-X-Y")  // Proper error code
    //       .problem(format!("Could not open file: {}", path))
    //       .add_hint("Check that the file exists and you have permission")
    //       .build();

    let path = "/nonexistent/file.qmd";
    let migration_error = generic_error!(format!("File not found: {}", path));

    println!("Migration-style error:");
    println!("{}", migration_error.to_text(None));
    println!();

    println!("=== Example 4: JSON output shows file/line info ===\n");

    let error_with_location = generic_error!("Error with source tracking");
    let json = error_with_location.to_json();

    println!("{}", serde_json::to_string_pretty(&json).unwrap());
    println!();

    println!("Note: The generic_error! and generic_warning! macros are intended");
    println!("for migration purposes only. New code should use DiagnosticMessageBuilder");
    println!("with proper error codes (Q-X-Y) instead of Q-0-99.");
}
