/*
 * pandoc_equiv_tests.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Tests to verify quarto-doctemplate produces equivalent output to Pandoc's doctemplates.
 *
 * These tests ensure our template evaluation matches Pandoc's behavior, particularly
 * for newline handling in multiline directives.
 */

use quarto_doctemplate::{Template, TemplateContext, TemplateValue};
use std::path::Path;

/// Helper to get the path to test fixtures
fn fixture_path(name: &str) -> std::path::PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir)
        .join("tests")
        .join("pandoc-equiv")
        .join(name)
}

/// Helper to load a template from fixtures
fn load_template(name: &str) -> Template {
    let path = fixture_path(name);
    Template::compile_from_file(&path)
        .unwrap_or_else(|_| panic!("Failed to load template: {}", name))
}

fn test_context() -> TemplateContext {
    let mut ctx = TemplateContext::new();
    ctx.insert("show", TemplateValue::Bool(true));
    ctx.insert(
        "items",
        TemplateValue::List(vec![
            TemplateValue::String("one".to_string()),
            TemplateValue::String("two".to_string()),
            TemplateValue::String("three".to_string()),
        ]),
    );
    ctx.insert("title", TemplateValue::String("Hello World".to_string()));
    ctx
}

/// Test multiline if: newlines after $if$ and $endif$ should be consumed.
///
/// Template:
/// ```
/// before
/// $if(show)$
/// content
/// $endif$
/// after
/// ```
#[test]
fn test_multiline_if_matches_pandoc() {
    let template = load_template("01-multiline-if.template");
    let result = template.render(&test_context()).unwrap();

    // Pandoc output (verified with: pandoc -t plain --template=01-multiline-if.template --metadata-file=context.json < /dev/null)
    assert_eq!(result, "before\ncontent\nafter\n");
}

/// Test inline if: no newlines should be consumed.
///
/// Template: `before $if(show)$content$endif$ after`
#[test]
fn test_inline_if_matches_pandoc() {
    let template = load_template("02-inline-if.template");
    let result = template.render(&test_context()).unwrap();

    assert_eq!(result, "before content after\n");
}

/// Test multiline for: newlines after $for$ and $endfor$ should be consumed.
///
/// Template:
/// ```
/// before
/// $for(items)$
/// - $it$
/// $endfor$
/// after
/// ```
#[test]
fn test_multiline_for_matches_pandoc() {
    let template = load_template("03-multiline-for.template");
    let result = template.render(&test_context()).unwrap();

    assert_eq!(result, "before\n- one\n- two\n- three\nafter\n");
}

/// Test inline for with separator.
///
/// Template: `items: $for(items)$$it$$sep$, $endfor$`
#[test]
fn test_for_with_separator_matches_pandoc() {
    let template = load_template("04-for-with-sep.template");
    let result = template.render(&test_context()).unwrap();

    assert_eq!(result, "items: one, two, three\n");
}

/// Test nested multiline if and for.
///
/// Template:
/// ```
/// $if(items)$
/// List:
/// $for(items)$
///   - $it$
/// $endfor$
/// $endif$
/// ```
#[test]
fn test_nested_if_for_matches_pandoc() {
    let template = load_template("05-nested-if-for.template");
    let result = template.render(&test_context()).unwrap();

    assert_eq!(result, "List:\n  - one\n  - two\n  - three\n");
}

/// Test multiline if with else branch (else branch not taken).
///
/// Template:
/// ```
/// $if(show)$
/// shown
/// $else$
/// hidden
/// $endif$
/// ```
#[test]
fn test_else_branch_matches_pandoc() {
    let template = load_template("06-else-branch.template");
    let result = template.render(&test_context()).unwrap();

    assert_eq!(result, "shown\n");
}

/// Test else branch when condition is false.
#[test]
fn test_else_branch_taken_matches_pandoc() {
    let template = load_template("06-else-branch.template");
    let mut ctx = test_context();
    ctx.insert("show", TemplateValue::Bool(false));
    let result = template.render(&ctx).unwrap();

    assert_eq!(result, "hidden\n");
}

/// Test variable with trailing newline in value - should be stripped.
#[test]
fn test_variable_final_newline_stripped() {
    let source = "Value: $name$!";
    let template = Template::compile(source).unwrap();
    let mut ctx = TemplateContext::new();
    // Value has trailing newline
    ctx.insert("name", TemplateValue::String("test\n".to_string()));
    let result = template.render(&ctx).unwrap();

    // Trailing newline should be stripped from the value
    assert_eq!(result, "Value: test!");
}
