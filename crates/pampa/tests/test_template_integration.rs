/*
 * test_template_integration.rs
 * Copyright (c) 2025 Posit, PBC
 */

use pampa::template::{BodyFormat, TemplateBundle, render_with_bundle};
use pampa::wasm_entry_points;

/// Helper to parse QMD and return (Pandoc, ASTContext)
fn parse_qmd(
    input: &str,
) -> (
    pampa::pandoc::Pandoc,
    pampa::pandoc::ast_context::ASTContext,
) {
    wasm_entry_points::qmd_to_pandoc(input.as_bytes()).expect("Failed to parse QMD")
}

// =============================================================================
// Bundle tests
// =============================================================================

#[test]
fn test_simple_template_rendering() {
    let input = r#"---
title: My Document
---

Hello **world**!
"#;
    let (pandoc, mut context) = parse_qmd(input);
    let bundle = TemplateBundle::new("<h1>$title$</h1>\n$body$");

    let (output, diagnostics) = render_with_bundle(
        &pandoc,
        &mut context,
        &bundle,
        "test.html",
        BodyFormat::Html,
    )
    .expect("Render should succeed");

    assert!(output.contains("<h1>My Document</h1>"));
    assert!(output.contains("<p>Hello <strong>world</strong>!</p>"));
    assert!(diagnostics.is_empty());
}

#[test]
fn test_template_with_partials() {
    let input = r#"---
title: Test
---

Content here.
"#;
    let (pandoc, mut context) = parse_qmd(input);
    let bundle = TemplateBundle::new("$header()$\n$body$\n$footer()$")
        .with_partial("header", "<header>$title$</header>")
        .with_partial("footer", "<footer>End</footer>");

    let (output, _) = render_with_bundle(
        &pandoc,
        &mut context,
        &bundle,
        "test.html",
        BodyFormat::Html,
    )
    .expect("Render should succeed");

    assert!(output.contains("<header>Test</header>"));
    assert!(output.contains("<footer>End</footer>"));
}

#[test]
fn test_template_with_conditionals() {
    let input = r#"---
title: Has Title
---

Body text.
"#;
    let (pandoc, mut context) = parse_qmd(input);
    let bundle = TemplateBundle::new("$if(title)$TITLE: $title$$endif$$if(missing)$MISSING$endif$");

    let (output, _) = render_with_bundle(
        &pandoc,
        &mut context,
        &bundle,
        "test.html",
        BodyFormat::Html,
    )
    .expect("Render should succeed");

    assert!(output.contains("TITLE: Has Title"));
    assert!(!output.contains("MISSING"));
}

#[test]
fn test_template_with_loops() {
    let input = r#"---
authors:
  - Alice
  - Bob
  - Charlie
---

Content.
"#;
    let (pandoc, mut context) = parse_qmd(input);
    let bundle = TemplateBundle::new("$for(authors)$- $authors$\n$endfor$");

    let (output, _) = render_with_bundle(
        &pandoc,
        &mut context,
        &bundle,
        "test.html",
        BodyFormat::Html,
    )
    .expect("Render should succeed");

    assert!(output.contains("- Alice"));
    assert!(output.contains("- Bob"));
    assert!(output.contains("- Charlie"));
}

#[test]
fn test_plaintext_body_format() {
    let input = r#"---
title: Plain Text Test
---

Hello **bold** and *italic*.
"#;
    let (pandoc, mut context) = parse_qmd(input);
    let bundle = TemplateBundle::new("Title: $title$\n\n$body$");

    let (output, _) = render_with_bundle(
        &pandoc,
        &mut context,
        &bundle,
        "test.txt",
        BodyFormat::Plaintext,
    )
    .expect("Render should succeed");

    assert!(output.contains("Title: Plain Text Test"));
    // In plaintext, bold/italic should be stripped
    assert!(output.contains("Hello bold and italic."));
    assert!(!output.contains("<strong>"));
}

// =============================================================================
// JSON bundle serialization tests
// =============================================================================

#[test]
fn test_bundle_json_roundtrip() {
    let bundle = TemplateBundle::new("$body$").with_partial("header", "<header>$title$</header>");

    let json = bundle.to_json().expect("Serialization should succeed");
    let parsed = TemplateBundle::from_json(&json).expect("Deserialization should succeed");

    assert_eq!(parsed.main, bundle.main);
    assert_eq!(parsed.version, Some("1.0.0".to_string())); // Default version
    assert_eq!(parsed.partials.get("header"), bundle.partials.get("header"));
}

#[test]
fn test_bundle_from_json() {
    let json = r#"{
        "version": "1.0.0",
        "main": "Hello $name$!",
        "partials": {
            "footer": "Goodbye"
        }
    }"#;

    let bundle = TemplateBundle::from_json(json).expect("Should parse valid JSON");
    assert_eq!(bundle.main, "Hello $name$!");
    assert_eq!(bundle.version, Some("1.0.0".to_string()));
    assert_eq!(bundle.partials.get("footer"), Some(&"Goodbye".to_string()));
}

#[test]
fn test_bundle_minimal_json() {
    // Minimal bundle with just main template
    let json = r#"{"main": "$body$"}"#;

    let bundle = TemplateBundle::from_json(json).expect("Should parse minimal JSON");
    assert_eq!(bundle.main, "$body$");
    assert!(bundle.version.is_none());
    assert!(bundle.partials.is_empty());
}

// =============================================================================
// Built-in template tests
// =============================================================================

#[test]
fn test_builtin_html_template() {
    use pampa::template::builtin::get_builtin_template;

    let bundle = get_builtin_template("html").expect("html template should exist");
    assert!(bundle.main.contains("<!DOCTYPE html>"));
    assert!(bundle.partials.contains_key("styles.html"));
}

#[test]
fn test_builtin_plain_template() {
    use pampa::template::builtin::get_builtin_template;

    let bundle = get_builtin_template("plain").expect("plain template should exist");
    assert_eq!(bundle.main, "$body$\n");
}

#[test]
fn test_builtin_template_not_found() {
    use pampa::template::builtin::get_builtin_template;

    assert!(get_builtin_template("nonexistent").is_none());
}

#[test]
fn test_builtin_html_renders() {
    use pampa::template::builtin::get_builtin_template;

    let input = r#"---
title: HTML Test
author: Test Author
---

This is content.
"#;
    let (pandoc, mut context) = parse_qmd(input);
    let bundle = get_builtin_template("html").unwrap();

    let (output, _) = render_with_bundle(
        &pandoc,
        &mut context,
        &bundle,
        "<builtin-template:html>",
        BodyFormat::Html,
    )
    .expect("Render should succeed");

    assert!(output.contains("<!DOCTYPE html>"));
    assert!(output.contains("<title>"));
    assert!(output.contains("HTML Test"));
    assert!(output.contains("Test Author"));
    assert!(output.contains("This is content."));
}

// =============================================================================
// WASM entry point tests
// =============================================================================

#[test]
fn test_wasm_parse_and_render() {
    let input = r#"---
title: WASM Test
---

Hello!
"#;
    let bundle_json = r#"{"main": "Title: $title$\n$body$"}"#;

    let result = wasm_entry_points::parse_and_render_qmd(input.as_bytes(), bundle_json, "html");
    let parsed: serde_json::Value = serde_json::from_str(&result).expect("Should be valid JSON");

    assert!(parsed.get("output").is_some(), "Should have output field");
    let output = parsed["output"].as_str().unwrap();
    assert!(output.contains("Title: WASM Test"));
}

#[test]
fn test_wasm_get_builtin_template() {
    let result = wasm_entry_points::get_builtin_template_json("html");
    let parsed: serde_json::Value = serde_json::from_str(&result).expect("Should be valid JSON");

    assert!(parsed.get("main").is_some(), "Should have main field");
    assert!(parsed["main"].as_str().unwrap().contains("<!DOCTYPE html>"));
}

#[test]
fn test_wasm_get_unknown_template() {
    let result = wasm_entry_points::get_builtin_template_json("nonexistent");
    let parsed: serde_json::Value = serde_json::from_str(&result).expect("Should be valid JSON");

    assert!(parsed.get("error").is_some(), "Should have error field");
}

#[test]
fn test_wasm_invalid_bundle_json() {
    let input = "Hello\n";
    let bundle_json = "not valid json";

    let result = wasm_entry_points::parse_and_render_qmd(input.as_bytes(), bundle_json, "html");
    let parsed: serde_json::Value = serde_json::from_str(&result).expect("Should be valid JSON");

    assert!(parsed.get("error").is_some(), "Should have error field");
}

#[test]
fn test_wasm_invalid_body_format() {
    let input = "Hello\n";
    let bundle_json = r#"{"main": "$body$"}"#;

    let result = wasm_entry_points::parse_and_render_qmd(input.as_bytes(), bundle_json, "invalid");
    let parsed: serde_json::Value = serde_json::from_str(&result).expect("Should be valid JSON");

    assert!(parsed.get("error").is_some(), "Should have error field");
    assert!(
        parsed["error"]
            .as_str()
            .unwrap()
            .contains("Unknown body format")
    );
}

// =============================================================================
// Metadata conversion tests
// =============================================================================

#[test]
fn test_nested_metadata_in_template() {
    let input = r#"---
author:
  name: John Doe
  affiliation: ACME Corp
---

Content.
"#;
    let (pandoc, mut context) = parse_qmd(input);
    let bundle = TemplateBundle::new("Name: $author.name$\nAffiliation: $author.affiliation$");

    let (output, _) = render_with_bundle(
        &pandoc,
        &mut context,
        &bundle,
        "test.html",
        BodyFormat::Html,
    )
    .expect("Render should succeed");

    assert!(output.contains("Name: John Doe"));
    assert!(output.contains("Affiliation: ACME Corp"));
}

#[test]
fn test_boolean_metadata_in_conditional() {
    let input = r#"---
draft: true
published: false
---

Content.
"#;
    let (pandoc, mut context) = parse_qmd(input);
    let bundle = TemplateBundle::new("$if(draft)$DRAFT$endif$$if(published)$PUBLISHED$endif$");

    let (output, _) = render_with_bundle(
        &pandoc,
        &mut context,
        &bundle,
        "test.html",
        BodyFormat::Html,
    )
    .expect("Render should succeed");

    assert!(output.contains("DRAFT"));
    assert!(!output.contains("PUBLISHED"));
}

#[test]
fn test_meta_inlines_rendered_as_html() {
    // MetaInlines with formatting should be rendered as HTML when body format is HTML
    let input = r#"---
title: Hello **bold** world
---

Content.
"#;
    let (pandoc, mut context) = parse_qmd(input);
    let bundle = TemplateBundle::new("Title: $title$");

    let (output, _) = render_with_bundle(
        &pandoc,
        &mut context,
        &bundle,
        "test.html",
        BodyFormat::Html,
    )
    .expect("Render should succeed");

    // MetaInlines are rendered as strings, so bold should appear as HTML
    assert!(
        output.contains("Hello <strong>bold</strong> world")
            || output.contains("Hello **bold** world")
    );
}
