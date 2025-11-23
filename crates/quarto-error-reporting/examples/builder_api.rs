//! Builder API example.
//!
//! This example demonstrates the builder API which encodes tidyverse guidelines
//! for error messages: title, problem, details, and hints.

use quarto_error_reporting::DiagnosticMessageBuilder;

fn main() {
    println!("=== Example 1: Simple builder usage ===\n");

    let error1 = DiagnosticMessageBuilder::error("Invalid input")
        .problem("Value must be numeric")
        .add_detail("Found text in column 3")
        .add_hint("Check the input file format")
        .build();

    println!("{}", error1.to_text(None));

    println!("\n=== Example 2: Tidyverse four-part structure ===\n");

    let error2 = DiagnosticMessageBuilder::error("Incompatible types")
        .problem("Cannot combine date and datetime types")
        .add_detail("`x` has type `date`")
        .add_detail("`y` has type `datetime`")
        .add_info("Both values come from the same data source")
        .add_hint("Convert both to the same type first?")
        .build();

    println!("{}", error2.to_text(None));

    println!("\n=== Example 3: Multiple details and hints ===\n");

    let error3 = DiagnosticMessageBuilder::error("Schema validation failed")
        .problem("Configuration does not match expected schema")
        .add_detail("Property `title` has type `number`")
        .add_detail("Expected type is `string`")
        .add_detail("Property `author` is missing")
        .add_info("Schema is defined in `_quarto.yml`")
        .add_hint("Did you forget quotes around the title?")
        .add_hint("Add an `author` field to the configuration")
        .build();

    println!("{}", error3.to_text(None));

    println!("\n=== Example 4: Builder validation ===\n");

    // This will trigger validation warnings
    let (msg, warnings) = DiagnosticMessageBuilder::error("Validation test")
        .add_detail("Detail 1")
        .add_detail("Detail 2")
        .add_detail("Detail 3")
        .add_detail("Detail 4")
        .add_detail("Detail 5")
        .add_detail("Detail 6")  // Too many!
        .build_with_validation();

    println!("{}", msg.to_text(None));

    if !warnings.is_empty() {
        println!("\nValidation warnings:");
        for warning in warnings {
            println!("  ⚠ {}", warning);
        }
    }
}
