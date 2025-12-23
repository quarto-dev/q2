/*
 * render_integration.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Integration tests for the Quarto render pipeline.
 */

//! Integration tests for the render pipeline.
//!
//! These tests exercise the full render pipeline from QMD input to HTML output,
//! verifying that all components work together correctly.

use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// Helper to run the render pipeline on a QMD string and return the output HTML.
fn render_qmd(qmd_content: &str) -> RenderResult {
    let temp = TempDir::new().expect("Failed to create temp directory");
    let input_path = temp.path().join("test.qmd");
    let output_path = temp.path().join("test.html");

    // Write QMD content
    fs::write(&input_path, qmd_content).expect("Failed to write QMD file");

    // Run render
    let result = run_render(&input_path, &output_path);

    RenderResult {
        temp,
        output_path,
        result,
    }
}

struct RenderResult {
    temp: TempDir,
    output_path: std::path::PathBuf,
    result: Result<(), String>,
}

impl RenderResult {
    fn html(&self) -> String {
        fs::read_to_string(&self.output_path).expect("Failed to read output HTML")
    }

    fn resource_dir(&self) -> std::path::PathBuf {
        self.temp.path().join("test_files")
    }

    fn css_path(&self) -> std::path::PathBuf {
        self.resource_dir().join("styles.css")
    }
}

/// Run the render pipeline using the quarto-core APIs directly.
fn run_render(input_path: &Path, output_path: &Path) -> Result<(), String> {
    use quarto_core::{
        BinaryDependencies, CalloutResolveTransform, CalloutTransform, DocumentInfo, Format,
        MetadataNormalizeTransform, ProjectContext, RenderContext, RenderOptions,
        ResourceCollectorTransform, TransformPipeline,
    };
    use quarto_system_runtime::NativeRuntime;

    // Create runtime
    let runtime = NativeRuntime::new();

    // Read input
    let input_content = fs::read(input_path).map_err(|e| e.to_string())?;

    // Parse QMD
    let input_path_str = input_path.to_string_lossy();
    let mut output_stream = std::io::sink();

    let (mut pandoc, _context, _warnings) = pampa::readers::qmd::read(
        &input_content,
        false, // loose mode
        &input_path_str,
        &mut output_stream,
        true, // track source locations
        None, // file_id
    )
    .map_err(|diagnostics| {
        diagnostics
            .iter()
            .map(|d| d.to_text(None))
            .collect::<Vec<_>>()
            .join("\n")
    })?;

    // Set up render context
    let project = ProjectContext::discover(input_path, &runtime).map_err(|e| e.to_string())?;
    let doc_info = DocumentInfo::from_path(input_path);
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let options = RenderOptions {
        verbose: false,
        execute: false,
        use_freeze: false,
        output_path: Some(output_path.to_path_buf()),
    };
    let mut ctx = RenderContext::new(&project, &doc_info, &format, &binaries).with_options(options);

    // Run transform pipeline
    let mut pipeline = TransformPipeline::new();
    pipeline.push(Box::new(CalloutTransform::new()));
    pipeline.push(Box::new(CalloutResolveTransform::new()));
    pipeline.push(Box::new(MetadataNormalizeTransform::new()));
    pipeline.push(Box::new(ResourceCollectorTransform::new()));
    pipeline
        .execute(&mut pandoc, &mut ctx)
        .map_err(|e| e.to_string())?;

    // Get output directory and stem
    let output_dir = output_path.parent().unwrap();
    let output_stem = output_path.file_stem().unwrap().to_str().unwrap();

    // Write resources
    let resource_paths =
        quarto_core::resources::write_html_resources(output_dir, output_stem, &runtime)
            .map_err(|e| e.to_string())?;

    // Render HTML body using pampa's HTML writer
    let mut body_buf = Vec::new();
    pampa::writers::html::write_blocks_to(&pandoc.blocks, &mut body_buf)
        .map_err(|e| e.to_string())?;
    let body = String::from_utf8_lossy(&body_buf).into_owned();

    // Render with template
    let html =
        quarto_core::template::render_with_resources(&body, &pandoc.meta, &resource_paths.css)
            .map_err(|e| e.to_string())?;

    // Write output
    fs::create_dir_all(output_dir).map_err(|e| e.to_string())?;
    fs::write(output_path, html).map_err(|e| e.to_string())?;

    Ok(())
}

// ============================================================================
// Basic Document Tests
// ============================================================================

#[test]
fn test_simple_document_renders() {
    let result = render_qmd(
        r#"---
title: Hello World
---

This is a paragraph.
"#,
    );

    assert!(result.result.is_ok(), "Render failed: {:?}", result.result);
    assert!(result.output_path.exists());

    let html = result.html();
    assert!(html.contains("<!DOCTYPE html>"));
    assert!(html.contains("<title>Hello World</title>"));
    assert!(html.contains("This is a paragraph"));
}

#[test]
fn test_document_with_headers() {
    let result = render_qmd(
        r#"---
title: Headers Test
---

# First Header

Some text.

## Second Header

More text.

### Third Header

Even more text.
"#,
    );

    assert!(result.result.is_ok());
    let html = result.html();

    assert!(html.contains("<h1"));
    assert!(html.contains("First Header"));
    assert!(html.contains("<h2"));
    assert!(html.contains("Second Header"));
    assert!(html.contains("<h3"));
    assert!(html.contains("Third Header"));
}

#[test]
fn test_document_without_title() {
    let result = render_qmd(
        r#"
# Just Content

No YAML frontmatter here.
"#,
    );

    assert!(result.result.is_ok());
    let html = result.html();

    // Should still be valid HTML
    assert!(html.contains("<!DOCTYPE html>"));
    assert!(html.contains("Just Content"));
    // No title tag content (but tag structure should be valid)
}

// ============================================================================
// Callout Tests
// ============================================================================

#[test]
fn test_callout_note_renders() {
    let result = render_qmd(
        r#"---
title: Callout Test
---

::: {.callout-note}
## Note Title

This is a note callout.
:::
"#,
    );

    assert!(result.result.is_ok());
    let html = result.html();

    assert!(html.contains("callout"));
    assert!(html.contains("callout-note"));
    assert!(html.contains("Note Title"));
    assert!(html.contains("This is a note callout"));
}

#[test]
fn test_callout_warning_renders() {
    let result = render_qmd(
        r#"---
title: Warning Test
---

::: {.callout-warning}
## Warning!

Be careful here.
:::
"#,
    );

    assert!(result.result.is_ok());
    let html = result.html();

    assert!(html.contains("callout-warning"));
    assert!(html.contains("Warning!"));
}

#[test]
fn test_callout_tip_renders() {
    let result = render_qmd(
        r#"---
title: Tip Test
---

::: {.callout-tip}
## Pro Tip

Here's a helpful tip.
:::
"#,
    );

    assert!(result.result.is_ok());
    let html = result.html();

    assert!(html.contains("callout-tip"));
    assert!(html.contains("Pro Tip"));
}

#[test]
fn test_callout_important_renders() {
    let result = render_qmd(
        r#"---
title: Important Test
---

::: {.callout-important}
## Important

This is important.
:::
"#,
    );

    assert!(result.result.is_ok());
    let html = result.html();

    assert!(html.contains("callout-important"));
}

#[test]
fn test_callout_caution_renders() {
    let result = render_qmd(
        r#"---
title: Caution Test
---

::: {.callout-caution}
## Caution

Proceed with care.
:::
"#,
    );

    assert!(result.result.is_ok());
    let html = result.html();

    assert!(html.contains("callout-caution"));
}

#[test]
fn test_callout_without_title() {
    let result = render_qmd(
        r#"---
title: No Title Callout
---

::: {.callout-note}
Just content, no header.
:::
"#,
    );

    assert!(result.result.is_ok());
    let html = result.html();

    assert!(html.contains("callout-note"));
    assert!(html.contains("Just content, no header"));
}

#[test]
fn test_multiple_callouts() {
    let result = render_qmd(
        r#"---
title: Multiple Callouts
---

::: {.callout-note}
## Note
First callout.
:::

::: {.callout-warning}
## Warning
Second callout.
:::

::: {.callout-tip}
## Tip
Third callout.
:::
"#,
    );

    assert!(result.result.is_ok());
    let html = result.html();

    assert!(html.contains("callout-note"));
    assert!(html.contains("callout-warning"));
    assert!(html.contains("callout-tip"));
    assert!(html.contains("First callout"));
    assert!(html.contains("Second callout"));
    assert!(html.contains("Third callout"));
}

// ============================================================================
// Resource Tests
// ============================================================================

#[test]
fn test_css_file_created() {
    let result = render_qmd(
        r#"---
title: CSS Test
---

Content here.
"#,
    );

    assert!(result.result.is_ok());
    assert!(
        result.resource_dir().exists(),
        "Resource directory should exist"
    );
    assert!(result.css_path().exists(), "CSS file should exist");

    let css = fs::read_to_string(result.css_path()).unwrap();
    assert!(
        css.contains(".callout"),
        "CSS should contain callout styles"
    );
    assert!(
        css.contains("--font-family-sans"),
        "CSS should contain CSS variables"
    );
}

#[test]
fn test_css_link_in_html() {
    let result = render_qmd(
        r#"---
title: CSS Link Test
---

Content.
"#,
    );

    assert!(result.result.is_ok());
    let html = result.html();

    assert!(
        html.contains(r#"<link rel="stylesheet" href="test_files/styles.css">"#),
        "HTML should link to external CSS"
    );
}

#[test]
fn test_user_css_merged() {
    let result = render_qmd(
        r#"---
title: User CSS Test
css: custom.css
---

Content.
"#,
    );

    assert!(result.result.is_ok());
    let html = result.html();

    // Default CSS should come first
    assert!(html.contains(r#"href="test_files/styles.css"#));
    // User CSS should come after
    assert!(html.contains(r#"href="custom.css"#));

    // Verify order: default before user
    let default_pos = html.find("test_files/styles.css").unwrap();
    let user_pos = html.find("custom.css").unwrap();
    assert!(
        default_pos < user_pos,
        "Default CSS should appear before user CSS"
    );
}

#[test]
fn test_multiple_user_css() {
    let result = render_qmd(
        r#"---
title: Multiple CSS Test
css:
  - first.css
  - second.css
---

Content.
"#,
    );

    assert!(result.result.is_ok());
    let html = result.html();

    assert!(html.contains(r#"href="first.css"#));
    assert!(html.contains(r#"href="second.css"#));
}

// ============================================================================
// Content Type Tests
// ============================================================================

#[test]
fn test_code_blocks() {
    let result = render_qmd(
        r#"---
title: Code Test
---

```python
def hello():
    print("Hello, World!")
```
"#,
    );

    assert!(result.result.is_ok());
    let html = result.html();

    assert!(html.contains("<pre"));
    assert!(html.contains("<code"));
    assert!(html.contains("def hello()"));
}

#[test]
fn test_inline_code() {
    let result = render_qmd(
        r#"---
title: Inline Code Test
---

Use the `print()` function.
"#,
    );

    assert!(result.result.is_ok());
    let html = result.html();

    assert!(html.contains("<code>print()</code>"));
}

#[test]
fn test_lists() {
    let result = render_qmd(
        r#"---
title: Lists Test
---

- Item one
- Item two
- Item three

1. First
2. Second
3. Third
"#,
    );

    assert!(result.result.is_ok());
    let html = result.html();

    assert!(html.contains("<ul>"));
    assert!(html.contains("<li>"));
    assert!(html.contains("Item one"));
    assert!(html.contains("<ol")); // May have type attribute
    assert!(html.contains("First"));
}

#[test]
fn test_blockquote() {
    let result = render_qmd(
        r#"---
title: Blockquote Test
---

> This is a quote.
> It spans multiple lines.
"#,
    );

    assert!(result.result.is_ok());
    let html = result.html();

    assert!(html.contains("<blockquote>"));
    assert!(html.contains("This is a quote"));
}

#[test]
fn test_emphasis() {
    let result = render_qmd(
        r#"---
title: Emphasis Test
---

This is *italic* and **bold** text.
"#,
    );

    assert!(result.result.is_ok(), "Render failed: {:?}", result.result);
    let html = result.html();

    assert!(html.contains("<em>italic</em>"));
    assert!(html.contains("<strong>bold</strong>"));
}

#[test]
fn test_links() {
    let result = render_qmd(
        r#"---
title: Links Test
---

Check out [this link](https://example.com).
"#,
    );

    assert!(result.result.is_ok());
    let html = result.html();

    assert!(html.contains(r#"<a href="https://example.com">this link</a>"#));
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_empty_document() {
    let result = render_qmd("");

    assert!(result.result.is_ok());
    let html = result.html();

    assert!(html.contains("<!DOCTYPE html>"));
    assert!(html.contains("<body>"));
    assert!(html.contains("</body>"));
}

#[test]
fn test_special_characters_in_title() {
    let result = render_qmd(
        r#"---
title: "Title with <special> & characters"
---

Content.
"#,
    );

    assert!(result.result.is_ok());
    // The title should be properly escaped or handled
    // Note: exact escaping depends on template engine behavior
}

#[test]
fn test_unicode_content() {
    let result = render_qmd(
        r#"---
title: Unicode Test 日本語
---

Hello 世界! 🎉
"#,
    );

    assert!(result.result.is_ok());
    let html = result.html();

    assert!(html.contains("日本語"));
    assert!(html.contains("世界"));
    assert!(html.contains("🎉"));
}

#[test]
fn test_nested_callout() {
    // Nested callouts in a blockquote
    let result = render_qmd(
        r#"---
title: Nested Test
---

> Quote with content
>
> ::: {.callout-note}
> ## Nested Note
> Inside blockquote.
> :::
"#,
    );

    assert!(result.result.is_ok());
    let html = result.html();

    assert!(html.contains("<blockquote>"));
    assert!(html.contains("callout-note"));
}

#[test]
fn test_long_document() {
    // Test with a longer document to ensure no buffer issues
    let mut content = String::from("---\ntitle: Long Document\n---\n\n");
    for i in 0..100 {
        content.push_str(&format!(
            "## Section {}\n\nParagraph {} with some content.\n\n",
            i, i
        ));
    }

    let result = render_qmd(&content);

    assert!(result.result.is_ok());
    let html = result.html();

    assert!(html.contains("Section 0"));
    assert!(html.contains("Section 99"));
}
