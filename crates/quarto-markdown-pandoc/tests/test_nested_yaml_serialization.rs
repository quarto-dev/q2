/*
 * test_nested_yaml_serialization.rs
 * Test to measure SourceInfo serialization size with deeply nested YAML
 */

use quarto_markdown_pandoc::readers;
use quarto_markdown_pandoc::writers;

/// Generate a .qmd file with nested YAML metadata of specified depth
fn generate_nested_yaml(depth: usize) -> String {
    let mut yaml = String::from("---\n");

    // Create nested structure: level1 -> level2 -> level3 -> ...
    for i in 0..depth {
        yaml.push_str(&format!("{}level{}: \n", "  ".repeat(i), i + 1));
    }

    // Add a value at the deepest level
    yaml.push_str(&format!("{}value: \"deep\"\n", "  ".repeat(depth)));

    yaml.push_str("---\n\nSome content.\n");
    yaml
}

#[test]
fn test_yaml_serialization_size_scaling() {
    println!("\n=== YAML Serialization Size Analysis ===\n");
    println!(
        "{:<10} {:<15} {:<15} {:<10}",
        "Depth", "QMD Size", "JSON Size", "Ratio"
    );
    println!("{:-<50}", "");

    for depth in [1, 2, 3, 5, 10, 15, 20] {
        let qmd_content = generate_nested_yaml(depth);
        let qmd_size = qmd_content.len();

        // Parse QMD to PandocAST
        let mut output_stream =
            quarto_markdown_pandoc::utils::output::VerboseOutput::Sink(std::io::sink());
        let (pandoc, context, _warnings) = readers::qmd::read(
            qmd_content.as_bytes(),
            false,
            "test.qmd",
            &mut output_stream,
            true, None,        )
        .expect("Failed to parse QMD");

        // Serialize to JSON
        let mut json_output = Vec::new();
        writers::json::write(&pandoc, &context, &mut json_output).expect("Failed to write JSON");

        let json_size = json_output.len();
        let ratio = json_size as f64 / qmd_size as f64;

        println!(
            "{:<10} {:<15} {:<15} {:<10.2}x",
            depth, qmd_size, json_size, ratio
        );

        // Verify roundtrip works
        let mut json_reader = std::io::Cursor::new(json_output);
        let (_pandoc_from_json, _context_from_json) =
            readers::json::read(&mut json_reader).expect("Failed to read JSON");
    }

    println!("\n");
}

#[test]
fn test_yaml_serialization_with_siblings() {
    println!("\n=== YAML Serialization with Sibling Nodes ===\n");
    println!(
        "{:<10} {:<15} {:<15} {:<10}",
        "Siblings", "QMD Size", "JSON Size", "Ratio"
    );
    println!("{:-<50}", "");

    for num_siblings in [1, 5, 10, 20, 50, 100] {
        // Create YAML with many sibling nodes at depth 3
        let mut yaml = String::from("---\n");
        yaml.push_str("level1:\n");
        yaml.push_str("  level2:\n");

        // Add multiple siblings at level 3
        for i in 0..num_siblings {
            yaml.push_str(&format!("    item{}: \"value\"\n", i));
        }

        yaml.push_str("---\n\nSome content.\n");

        let qmd_size = yaml.len();

        // Parse and serialize
        let mut output_stream =
            quarto_markdown_pandoc::utils::output::VerboseOutput::Sink(std::io::sink());
        let (pandoc, context, _warnings) =
            readers::qmd::read(yaml.as_bytes(), false, "test.qmd", &mut output_stream, true, None)
                .expect("Failed to parse QMD");

        let mut json_output = Vec::new();
        writers::json::write(&pandoc, &context, &mut json_output).expect("Failed to write JSON");

        let json_size = json_output.len();
        let ratio = json_size as f64 / qmd_size as f64;

        println!(
            "{:<10} {:<15} {:<15} {:<10.2}x",
            num_siblings, qmd_size, json_size, ratio
        );
    }

    println!("\n");
}

#[test]
fn test_analyze_json_structure() {
    // Create a moderately nested structure to analyze
    let yaml = r#"---
level1:
  level2:
    level3:
      item1: "value1"
      item2: "value2"
      item3: "value3"
---

Some content.
"#;

    let mut output_stream =
        quarto_markdown_pandoc::utils::output::VerboseOutput::Sink(std::io::sink());
    let (pandoc, context, _warnings) =
        readers::qmd::read(yaml.as_bytes(), false, "test.qmd", &mut output_stream, true, None)
            .expect("Failed to parse QMD");

    let mut json_output = Vec::new();
    writers::json::write(&pandoc, &context, &mut json_output).expect("Failed to write JSON");

    let json_str = String::from_utf8(json_output.clone()).unwrap();

    println!("\n=== JSON Structure Analysis ===\n");
    println!("Total JSON size: {} bytes", json_output.len());
    println!("QMD size: {} bytes", yaml.len());
    println!(
        "Ratio: {:.2}x",
        json_output.len() as f64 / yaml.len() as f64
    );

    // Count occurrences of "Substring" (parent chain duplication indicator)
    let substring_count = json_str.matches("\"Substring\"").count();
    println!("\nSubstring nodes in JSON: {}", substring_count);

    // Count occurrences of "Original"
    let original_count = json_str.matches("\"Original\"").count();
    println!("Original nodes in JSON: {}", original_count);

    // Estimate duplication by counting "file_id" (appears in every Original node in chain)
    let file_id_count = json_str.matches("\"file_id\"").count();
    println!(
        "file_id occurrences: {} (indicates parent chain duplication)",
        file_id_count
    );

    println!("\n");
}

/// Generate a complete binary tree of YAML metadata at specified depth
fn generate_binary_tree_yaml(depth: usize) -> String {
    fn generate_tree(current_depth: usize, max_depth: usize, indent: usize) -> String {
        if current_depth >= max_depth {
            // Leaf node
            return format!("{}leaf\n", "  ".repeat(indent));
        }

        // Internal node with left and right children
        let mut result = String::new();
        result.push_str(&format!("{}\n", "  ".repeat(indent)));
        result.push_str(&format!("{}left: ", "  ".repeat(indent)));
        result.push_str(&generate_tree(current_depth + 1, max_depth, indent + 1));
        result.push_str(&format!("{}right: ", "  ".repeat(indent)));
        result.push_str(&generate_tree(current_depth + 1, max_depth, indent + 1));
        result
    }

    let mut yaml = String::from("---\n");
    yaml.push_str("data: ");
    yaml.push_str(&generate_tree(0, depth, 1));
    yaml.push_str("---\n\nSome content.\n");
    yaml
}

#[test]
fn test_binary_tree_serialization() {
    println!("\n=== Binary Tree YAML Serialization ===\n");
    println!(
        "{:<10} {:<12} {:<15} {:<15} {:<10}",
        "Depth", "Nodes", "QMD Size", "JSON Size", "Ratio"
    );
    println!("{:-<62}", "");

    for depth in 1..=6 {
        let qmd_content = generate_binary_tree_yaml(depth);
        let qmd_size = qmd_content.len();
        let num_nodes = (1 << depth) - 1; // 2^depth - 1

        // Parse QMD to PandocAST
        let mut output_stream =
            quarto_markdown_pandoc::utils::output::VerboseOutput::Sink(std::io::sink());
        let (pandoc, context, _warnings) = readers::qmd::read(
            qmd_content.as_bytes(),
            false,
            "test.qmd",
            &mut output_stream,
            true, None,        )
        .expect("Failed to parse QMD");

        // Serialize to JSON
        let mut json_output = Vec::new();
        writers::json::write(&pandoc, &context, &mut json_output).expect("Failed to write JSON");

        let json_size = json_output.len();
        let ratio = json_size as f64 / qmd_size as f64;

        println!(
            "{:<10} {:<12} {:<15} {:<15} {:<10.2}x",
            depth, num_nodes, qmd_size, json_size, ratio
        );

        // Verify roundtrip works
        let mut json_reader = std::io::Cursor::new(json_output);
        let (_pandoc_from_json, _context_from_json) =
            readers::json::read(&mut json_reader).expect("Failed to read JSON");
    }

    println!("\n");
}
