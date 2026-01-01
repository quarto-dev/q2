/*
 * integration_tests.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Integration tests for quarto-doctemplate using test fixtures.
 */

use quarto_doctemplate::{Template, TemplateContext, TemplateValue};
use std::path::Path;

/// Helper to get the path to test fixtures
fn fixture_path(name: &str) -> std::path::PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir).join("test-fixtures").join(name)
}

/// Helper to load a template from fixtures
fn load_template(name: &str) -> Template {
    let path = fixture_path(name);
    Template::compile_from_file(&path).unwrap_or_else(|_| panic!("Failed to load template: {}", name))
}

#[test]
fn test_simple_interpolation() {
    let template = load_template("simple.template");

    let mut ctx = TemplateContext::new();
    ctx.insert("name", TemplateValue::String("World".to_string()));

    let result = template.render(&ctx).unwrap();
    assert_eq!(result, "Hello, World!");
}

#[test]
fn test_conditional_true() {
    let template = load_template("conditional.template");

    let mut ctx = TemplateContext::new();
    ctx.insert("show_greeting", TemplateValue::Bool(true));
    ctx.insert("name", TemplateValue::String("Alice".to_string()));

    let result = template.render(&ctx).unwrap();
    assert_eq!(result, "Hello, Alice!");
}

#[test]
fn test_conditional_false() {
    let template = load_template("conditional.template");

    let mut ctx = TemplateContext::new();
    ctx.insert("show_greeting", TemplateValue::Bool(false));
    ctx.insert("name", TemplateValue::String("Alice".to_string()));

    let result = template.render(&ctx).unwrap();
    assert_eq!(result, "Goodbye.");
}

#[test]
fn test_forloop_with_separator() {
    let template = load_template("forloop.template");

    let mut ctx = TemplateContext::new();
    ctx.insert(
        "items",
        TemplateValue::List(vec![
            TemplateValue::String("apple".to_string()),
            TemplateValue::String("banana".to_string()),
            TemplateValue::String("cherry".to_string()),
        ]),
    );

    let result = template.render(&ctx).unwrap();
    assert_eq!(result, "Items: apple, banana, cherry");
}

#[test]
fn test_forloop_empty() {
    let template = load_template("forloop.template");

    let mut ctx = TemplateContext::new();
    ctx.insert("items", TemplateValue::List(vec![]));

    let result = template.render(&ctx).unwrap();
    assert_eq!(result, "Items: ");
}

#[test]
fn test_partial_resolution() {
    let template = load_template("with-partial.template");

    let mut ctx = TemplateContext::new();
    ctx.insert("name", TemplateValue::String("Test".to_string()));

    let result = template.render(&ctx).unwrap();
    assert_eq!(result, "Header\nHello, Test!\nFooter");
}

#[test]
fn test_missing_variable_renders_empty() {
    let template = load_template("simple.template");

    let ctx = TemplateContext::new();
    // No 'name' variable set

    let result = template.render(&ctx).unwrap();
    assert_eq!(result, "Hello, !");
}

#[test]
fn test_nested_map_access() {
    let source = "$person.name$ is $person.age$ years old.";
    let template = Template::compile(source).unwrap();

    let mut person = std::collections::HashMap::new();
    person.insert("name".to_string(), TemplateValue::String("Bob".to_string()));
    person.insert("age".to_string(), TemplateValue::String("30".to_string()));

    let mut ctx = TemplateContext::new();
    ctx.insert("person", TemplateValue::Map(person));

    let result = template.render(&ctx).unwrap();
    assert_eq!(result, "Bob is 30 years old.");
}

#[test]
fn test_escaped_dollar() {
    let source = "Price: $$100";
    let template = Template::compile(source).unwrap();

    let ctx = TemplateContext::new();
    let result = template.render(&ctx).unwrap();
    assert_eq!(result, "Price: $100");
}

#[test]
fn test_author_card_partial() {
    // Test the author-card.template partial
    let template = load_template("author-card.template");

    let mut ctx = TemplateContext::new();
    let mut it = std::collections::HashMap::new();
    it.insert(
        "name".to_string(),
        TemplateValue::String("Jane Doe".to_string()),
    );
    it.insert(
        "email".to_string(),
        TemplateValue::String("jane@example.com".to_string()),
    );
    ctx.insert("it", TemplateValue::Map(it));

    let result = template.render(&ctx).unwrap();
    assert_eq!(result, "[Jane Doe] (jane@example.com)");
}
