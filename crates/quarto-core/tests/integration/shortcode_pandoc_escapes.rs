/*
 * tests/integration/shortcode_pandoc_escapes.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * bd-2uva9urq: escaped shortcodes in text contexts (code spans, code
 * blocks) must survive the pandoc path as the literal `{{< ... >}}` the
 * author asked for. Rust unescapes `{{{< x >}}}` once; a second, Lua-side
 * shortcodes pass used to see the unescaped text and expand it as live,
 * crashing on handlers (`brand`) that Rust does not know.
 *
 * Renders to `native` (pandoc AST dump) so there is no writer escaping and
 * no typst compile. `shortcode_text_contexts.rs` is HTML-only and never ran
 * the pandoc Lua stack.
 */

use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::render_to_file::{RenderToFileOptions, render_document_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// Render `body` (no front matter beyond a title) to `native` and return the output.
fn render_native(body: &str) -> String {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("esc.qmd");
    std::fs::write(&input_path, format!("---\ntitle: Esc\n---\n\n{body}\n")).unwrap();

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_document_to_file(
        &input_path,
        "native",
        &RenderToFileOptions::default(),
        None,
        runtime,
        None,
        None,
        None,
    )
    .expect("native render should succeed");
    std::fs::read_to_string(&result.output_path).expect("native output readable")
}

#[test]
fn escaped_bare_brand_in_code_span_stays_literal() {
    let out = render_native("Use `{{{< brand >}}}` here.");
    assert!(
        out.contains("{{< brand >}}"),
        "literal shortcode expected: {out}"
    );
}

#[test]
fn escaped_brand_with_arguments_in_code_span_stays_literal() {
    let out = render_native("Use `{{{< brand color COLOR_NAME VARIANT >}}}` here.");
    assert!(
        out.contains("{{< brand color COLOR_NAME VARIANT >}}"),
        "literal shortcode expected: {out}"
    );
}

#[test]
fn escaped_brand_in_fenced_code_block_stays_literal() {
    let out = render_native("```\n{{{< brand color COLOR_NAME VARIANT >}}}\n```");
    assert!(
        out.contains("{{< brand color COLOR_NAME VARIANT >}}"),
        "literal shortcode expected: {out}"
    );
}

/// Control: a live `brand` shortcode is unknown to Rust (warning only) and
/// was never reached by Lua; it must keep rendering without a crash.
#[test]
fn live_brand_shortcode_still_only_warns() {
    let out = render_native("Color: {{< brand color primary >}}");
    assert!(!out.is_empty());
}
