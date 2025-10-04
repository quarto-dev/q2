/*
 * test_inline_locations.rs
 * Copyright (c) 2025 Posit, PBC
 */

use quarto_markdown_pandoc::pandoc::treesitter_to_pandoc;
use quarto_markdown_pandoc::writers;
use tree_sitter_qmd::MarkdownParser;

#[test]
fn test_inline_source_locations() {
    let input = "hello _world_.";
    let mut parser = MarkdownParser::default();
    let input_bytes = input.as_bytes();
    let tree = parser
        .parse(input_bytes, None)
        .expect("Failed to parse input");

    let pandoc = treesitter_to_pandoc(&mut std::io::sink(), &tree, &input_bytes)
        .expect("Failed to convert to Pandoc AST");

    let mut buf = Vec::new();
    writers::json::write(&pandoc, &mut buf).unwrap();
    let json_output = String::from_utf8(buf).expect("Invalid UTF-8 in output");

    let json_value: serde_json::Value =
        serde_json::from_str(&json_output).expect("Failed to parse JSON output");

    // Check that the source locations are correct for the inline nodes
    let blocks = json_value["blocks"].as_array().unwrap();
    let para = &blocks[0];
    let inlines = para["c"].as_array().unwrap();

    // First inline should be "hello" with correct location
    let hello_str = &inlines[0];
    assert_eq!(hello_str["t"], "Str");
    assert_eq!(hello_str["c"], "hello");
    let hello_loc = &hello_str["l"];
    assert_eq!(hello_loc["start"]["column"], 0);
    assert_eq!(hello_loc["start"]["offset"], 0);
    assert_eq!(hello_loc["end"]["column"], 5);
    assert_eq!(hello_loc["end"]["offset"], 5);

    // Second inline should be a Space
    let space = &inlines[1];
    assert_eq!(space["t"], "Space");
    let space_loc = &space["l"];
    assert_eq!(space_loc["start"]["column"], 5);
    assert_eq!(space_loc["start"]["offset"], 5);
    assert_eq!(space_loc["end"]["column"], 6);
    assert_eq!(space_loc["end"]["offset"], 6);

    // Third inline should be Emph containing "world"
    let emph = &inlines[2];
    assert_eq!(emph["t"], "Emph");
    let emph_loc = &emph["l"];
    assert_eq!(emph_loc["start"]["column"], 6);
    assert_eq!(emph_loc["start"]["offset"], 6);
    assert_eq!(emph_loc["end"]["column"], 13);
    assert_eq!(emph_loc["end"]["offset"], 13);

    // Check the "world" string inside Emph
    let emph_content = emph["c"].as_array().unwrap();
    let world_str = &emph_content[0];
    assert_eq!(world_str["t"], "Str");
    assert_eq!(world_str["c"], "world");
    let world_loc = &world_str["l"];
    assert_eq!(world_loc["start"]["column"], 7);
    assert_eq!(world_loc["start"]["offset"], 7);
    assert_eq!(world_loc["end"]["column"], 12);
    assert_eq!(world_loc["end"]["offset"], 12);

    // Fourth inline should be "."
    let period = &inlines[3];
    assert_eq!(period["t"], "Str");
    assert_eq!(period["c"], ".");
    let period_loc = &period["l"];
    assert_eq!(period_loc["start"]["column"], 13);
    assert_eq!(period_loc["start"]["offset"], 13);
    assert_eq!(period_loc["end"]["column"], 14);
    assert_eq!(period_loc["end"]["offset"], 14);
}

#[test]
fn test_merged_strings_preserve_location() {
    // Test that when truly adjacent strings are merged, the location spans both
    // Using input that will produce adjacent Str nodes that get merged
    let input = "hello world";
    let mut parser = MarkdownParser::default();
    let input_bytes = input.as_bytes();
    let tree = parser
        .parse(input_bytes, None)
        .expect("Failed to parse input");

    let pandoc = treesitter_to_pandoc(&mut std::io::sink(), &tree, &input_bytes)
        .expect("Failed to convert to Pandoc AST");

    let mut buf = Vec::new();
    writers::json::write(&pandoc, &mut buf).unwrap();
    let json_output = String::from_utf8(buf).expect("Invalid UTF-8 in output");

    let json_value: serde_json::Value =
        serde_json::from_str(&json_output).expect("Failed to parse JSON output");

    let blocks = json_value["blocks"].as_array().unwrap();
    let para = &blocks[0];
    let inlines = para["c"].as_array().unwrap();

    // The parser should produce "hello", Space, "world"
    // Check that each has correct location
    assert_eq!(inlines.len(), 3);

    // First inline should be "hello"
    let hello = &inlines[0];
    assert_eq!(hello["t"], "Str");
    assert_eq!(hello["c"], "hello");
    let hello_loc = &hello["l"];
    assert_eq!(hello_loc["start"]["column"], 0);
    assert_eq!(hello_loc["start"]["offset"], 0);
    assert_eq!(hello_loc["end"]["column"], 5);
    assert_eq!(hello_loc["end"]["offset"], 5);

    // Second should be Space
    let space = &inlines[1];
    assert_eq!(space["t"], "Space");

    // Third should be "world"
    let world = &inlines[2];
    assert_eq!(world["t"], "Str");
    assert_eq!(world["c"], "world");
    let world_loc = &world["l"];
    assert_eq!(world_loc["start"]["column"], 6);
    assert_eq!(world_loc["start"]["offset"], 6);
    assert_eq!(world_loc["end"]["column"], 11);
    assert_eq!(world_loc["end"]["offset"], 11);
}

#[test]
fn test_separate_strings_keep_separate_locations() {
    // Test that strings separated by other inline elements aren't merged
    // and each keeps its own location
    let input = "a**b**c";
    let mut parser = MarkdownParser::default();
    let input_bytes = input.as_bytes();
    let tree = parser
        .parse(input_bytes, None)
        .expect("Failed to parse input");

    let pandoc = treesitter_to_pandoc(&mut std::io::sink(), &tree, &input_bytes)
        .expect("Failed to convert to Pandoc AST");

    let mut buf = Vec::new();
    writers::json::write(&pandoc, &mut buf).unwrap();
    let json_output = String::from_utf8(buf).expect("Invalid UTF-8 in output");

    let json_value: serde_json::Value =
        serde_json::from_str(&json_output).expect("Failed to parse JSON output");

    let blocks = json_value["blocks"].as_array().unwrap();
    let para = &blocks[0];
    let inlines = para["c"].as_array().unwrap();

    // Should have three elements: "a", Strong("b"), "c"
    assert_eq!(inlines.len(), 3);

    // First inline should be "a"
    let a_str = &inlines[0];
    assert_eq!(a_str["t"], "Str");
    assert_eq!(a_str["c"], "a");
    let a_loc = &a_str["l"];
    assert_eq!(a_loc["start"]["column"], 0);
    assert_eq!(a_loc["start"]["offset"], 0);
    assert_eq!(a_loc["end"]["column"], 1);
    assert_eq!(a_loc["end"]["offset"], 1);

    // Second inline should be Strong containing "b"
    let strong = &inlines[1];
    assert_eq!(strong["t"], "Strong");
    let strong_loc = &strong["l"];
    assert_eq!(strong_loc["start"]["column"], 1);
    assert_eq!(strong_loc["start"]["offset"], 1);
    assert_eq!(strong_loc["end"]["column"], 6);
    assert_eq!(strong_loc["end"]["offset"], 6);

    // Third inline should be "c"
    let c_str = &inlines[2];
    assert_eq!(c_str["t"], "Str");
    assert_eq!(c_str["c"], "c");
    let c_loc = &c_str["l"];
    assert_eq!(c_loc["start"]["column"], 6);
    assert_eq!(c_loc["start"]["offset"], 6);
    assert_eq!(c_loc["end"]["column"], 7);
    assert_eq!(c_loc["end"]["offset"], 7);
}
