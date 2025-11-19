use quarto_markdown_pandoc::readers::qmd;
use quarto_markdown_pandoc::writers::json::{JsonConfig, write_with_config};
use std::io;

#[test]
fn test_json_location_disabled_by_default() {
    let input = "Hello *world*!";
    let mut output = io::sink();

    let (pandoc, context, _errors) =
        qmd::read(input.as_bytes(), false, "test.qmd", &mut output, true, None)
            .expect("Failed to parse QMD");

    let mut buf = Vec::new();
    let config = JsonConfig::default();
    write_with_config(&pandoc, &context, &mut buf, &config).expect("Failed to write JSON");

    let json: serde_json::Value = serde_json::from_slice(&buf).expect("Invalid JSON");

    // Check that the first inline in the paragraph has no 'l' field
    let first_inline = &json["blocks"][0]["c"][0];
    assert_eq!(first_inline["t"], "Str");
    assert_eq!(first_inline["c"], "Hello");
    assert!(first_inline["s"].is_number());
    assert!(first_inline["l"].is_null());
}

#[test]
fn test_json_location_enabled() {
    let input = "Hello *world*!";
    let mut output = io::sink();

    let (pandoc, context, _errors) =
        qmd::read(input.as_bytes(), false, "test.qmd", &mut output, true, None)
            .expect("Failed to parse QMD");

    let mut buf = Vec::new();
    let config = JsonConfig {
        include_inline_locations: true,
    };
    write_with_config(&pandoc, &context, &mut buf, &config).expect("Failed to write JSON");

    let json: serde_json::Value = serde_json::from_slice(&buf).expect("Invalid JSON");

    // Check that the first inline in the paragraph has the 'l' field
    let first_inline = &json["blocks"][0]["c"][0];
    assert_eq!(first_inline["t"], "Str");
    assert_eq!(first_inline["c"], "Hello");
    assert!(first_inline["s"].is_number());

    // Verify the 'l' field structure
    let location = &first_inline["l"];
    assert!(!location.is_null(), "'l' field should be present");

    // Check file_id
    assert_eq!(location["f"], 0);

    // Check begin position
    let begin = &location["b"];
    assert_eq!(begin["o"], 0); // offset 0
    assert_eq!(begin["l"], 1); // line 1 (1-based)
    assert_eq!(begin["c"], 1); // column 1 (1-based)

    // Check end position
    let end = &location["e"];
    assert_eq!(end["o"], 5); // offset 5 ("Hello" is 5 chars)
    assert_eq!(end["l"], 1); // line 1
    assert_eq!(end["c"], 6); // column 6 (after "Hello")
}

#[test]
fn test_json_location_multiline() {
    let input = "Line 1\n\nLine 3 with *emphasis*.";
    let mut output = io::sink();

    let (pandoc, context, _errors) =
        qmd::read(input.as_bytes(), false, "test.qmd", &mut output, true, None)
            .expect("Failed to parse QMD");

    let mut buf = Vec::new();
    let config = JsonConfig {
        include_inline_locations: true,
    };
    write_with_config(&pandoc, &context, &mut buf, &config).expect("Failed to write JSON");

    let json: serde_json::Value = serde_json::from_slice(&buf).expect("Invalid JSON");

    // First paragraph: first word is "Line"
    let first_para_first_inline = &json["blocks"][0]["c"][0];
    assert_eq!(first_para_first_inline["c"], "Line");
    let location = &first_para_first_inline["l"];
    assert_eq!(location["b"]["l"], 1); // line 1
    assert_eq!(location["b"]["c"], 1); // column 1

    // Second paragraph: first word is "Line"
    let second_para_first_inline = &json["blocks"][1]["c"][0];
    assert_eq!(second_para_first_inline["c"], "Line");
    let location = &second_para_first_inline["l"];
    assert_eq!(location["b"]["l"], 3); // line 3
    assert_eq!(location["b"]["c"], 1); // column 1
}

#[test]
fn test_json_location_1_indexed() {
    // Verify that line and column are 1-based, not 0-based
    let input = "Test";
    let mut output = io::sink();

    let (pandoc, context, _errors) =
        qmd::read(input.as_bytes(), false, "test.qmd", &mut output, true, None)
            .expect("Failed to parse QMD");

    let mut buf = Vec::new();
    let config = JsonConfig {
        include_inline_locations: true,
    };
    write_with_config(&pandoc, &context, &mut buf, &config).expect("Failed to write JSON");

    let json: serde_json::Value = serde_json::from_slice(&buf).expect("Invalid JSON");

    let first_inline = &json["blocks"][0]["c"][0];
    let location = &first_inline["l"];

    // First character should be at line 1, column 1 (not 0, 0)
    assert_eq!(location["b"]["l"], 1);
    assert_eq!(location["b"]["c"], 1);
}
