/*
 * test_code_block_attributes.rs
 *
 * Tests for code block attribute parsing, specifically the new syntax:
 * ```{language #id .class key=value}
 *
 * Copyright (c) 2025 Posit, PBC
 */

use pampa::pandoc::Block;
use pampa::readers;

fn parse_qmd(input: &str) -> pampa::pandoc::Pandoc {
    let result = readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    );
    result.expect("Failed to parse QMD").0
}

/// Helper to parse QMD and extract the first CodeBlock's attributes
fn parse_code_block_attrs(input: &str) -> (String, Vec<String>, Vec<(String, String)>) {
    let result = parse_qmd(input);
    let block = result
        .blocks
        .into_iter()
        .find(|b| matches!(b, Block::CodeBlock(_)))
        .expect("No CodeBlock found");

    match block {
        Block::CodeBlock(cb) => {
            let (id, classes, attrs) = cb.attr;
            let attrs_vec: Vec<(String, String)> = attrs.into_iter().collect();
            (id, classes, attrs_vec)
        }
        _ => unreachable!(),
    }
}

#[test]
fn test_simple_language_specifier() {
    // {python} should produce classes: ["{python}"], id: "", attrs: []
    let input = "```{python}\ncode()\n```\n";
    let (id, classes, attrs) = parse_code_block_attrs(input);

    assert_eq!(id, "");
    assert_eq!(classes, vec!["{python}"]);
    assert!(attrs.is_empty());
}

#[test]
fn test_language_with_id() {
    // {python #fig-foo} should produce:
    // - id: "fig-foo"
    // - classes: ["{python}"]
    // - attrs: []
    let input = "```{python #fig-foo}\ncode()\n```\n";
    let (id, classes, attrs) = parse_code_block_attrs(input);

    assert_eq!(id, "fig-foo");
    assert_eq!(classes, vec!["{python}"]);
    assert!(attrs.is_empty());
}

#[test]
fn test_language_with_class() {
    // {python .myclass} should produce:
    // - id: ""
    // - classes: ["{python}", "myclass"]
    // - attrs: []
    let input = "```{python .myclass}\ncode()\n```\n";
    let (id, classes, attrs) = parse_code_block_attrs(input);

    assert_eq!(id, "");
    assert_eq!(classes, vec!["{python}", "myclass"]);
    assert!(attrs.is_empty());
}

#[test]
fn test_language_with_id_and_class() {
    // {python #fig-foo .myclass} should produce:
    // - id: "fig-foo"
    // - classes: ["{python}", "myclass"]
    // - attrs: []
    let input = "```{python #fig-foo .myclass}\ncode()\n```\n";
    let (id, classes, attrs) = parse_code_block_attrs(input);

    assert_eq!(id, "fig-foo");
    assert_eq!(classes, vec!["{python}", "myclass"]);
    assert!(attrs.is_empty());
}

#[test]
fn test_language_with_key_value() {
    // {python key=value} should produce:
    // - id: ""
    // - classes: ["{python}"]
    // - attrs: [("key", "value")]
    let input = "```{python key=value}\ncode()\n```\n";
    let (id, classes, attrs) = parse_code_block_attrs(input);

    assert_eq!(id, "");
    assert_eq!(classes, vec!["{python}"]);
    assert_eq!(attrs, vec![("key".to_string(), "value".to_string())]);
}

#[test]
fn test_language_with_all_attributes() {
    // {python #fig-test .myclass key=value} should produce:
    // - id: "fig-test"
    // - classes: ["{python}", "myclass"]
    // - attrs: [("key", "value")]
    let input = "```{python #fig-test .myclass key=value}\ncode()\n```\n";
    let (id, classes, attrs) = parse_code_block_attrs(input);

    assert_eq!(id, "fig-test");
    assert_eq!(classes, vec!["{python}", "myclass"]);
    assert_eq!(attrs, vec![("key".to_string(), "value".to_string())]);
}

#[test]
fn test_language_with_multiple_classes() {
    // {python .class1 .class2} should produce:
    // - id: ""
    // - classes: ["{python}", "class1", "class2"]
    // - attrs: []
    let input = "```{python .class1 .class2}\ncode()\n```\n";
    let (id, classes, attrs) = parse_code_block_attrs(input);

    assert_eq!(id, "");
    assert_eq!(classes, vec!["{python}", "class1", "class2"]);
    assert!(attrs.is_empty());
}

#[test]
fn test_language_with_multiple_key_values() {
    // {python key1=value1 key2=value2} should produce:
    // - id: ""
    // - classes: ["{python}"]
    // - attrs: [("key1", "value1"), ("key2", "value2")]
    let input = "```{python key1=value1 key2=value2}\ncode()\n```\n";
    let (id, classes, attrs) = parse_code_block_attrs(input);

    assert_eq!(id, "");
    assert_eq!(classes, vec!["{python}"]);
    assert_eq!(
        attrs,
        vec![
            ("key1".to_string(), "value1".to_string()),
            ("key2".to_string(), "value2".to_string())
        ]
    );
}

#[test]
fn test_r_language_specifier() {
    // {r} should work the same as {python}
    let input = "```{r}\ncode()\n```\n";
    let (id, classes, attrs) = parse_code_block_attrs(input);

    assert_eq!(id, "");
    assert_eq!(classes, vec!["{r}"]);
    assert!(attrs.is_empty());
}

#[test]
fn test_r_with_attributes() {
    // {r #fig-plot .centered width=100} should produce:
    // - id: "fig-plot"
    // - classes: ["{r}", "centered"]
    // - attrs: [("width", "100")]
    let input = "```{r #fig-plot .centered width=100}\nplot(x)\n```\n";
    let (id, classes, attrs) = parse_code_block_attrs(input);

    assert_eq!(id, "fig-plot");
    assert_eq!(classes, vec!["{r}", "centered"]);
    assert_eq!(attrs, vec![("width".to_string(), "100".to_string())]);
}

#[test]
fn test_quoted_attribute_value() {
    // {python key="value with spaces"} should handle quoted values
    let input = "```{python key=\"value with spaces\"}\ncode()\n```\n";
    let (id, classes, attrs) = parse_code_block_attrs(input);

    assert_eq!(id, "");
    assert_eq!(classes, vec!["{python}"]);
    assert_eq!(
        attrs,
        vec![("key".to_string(), "value with spaces".to_string())]
    );
}

#[test]
fn test_info_string_without_braces() {
    // ```python (no braces) should produce classes: ["python"]
    let input = "```python\ncode()\n```\n";
    let (id, classes, attrs) = parse_code_block_attrs(input);

    assert_eq!(id, "");
    assert_eq!(classes, vec!["python"]);
    assert!(attrs.is_empty());
}
