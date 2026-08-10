/*
 * tests/integration/shortcode_text_contexts.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * End-to-end tests for shortcode substitution in text contexts:
 * code blocks, inline code, raw blocks/inlines, math, element
 * attributes, image src, and link targets (bd-fz6gwfq0).
 */

//! Quarto 1's shortcode filter applies text-level substitution
//! (`apply_code_shortcode`) to contexts where shortcodes exist as
//! plain text rather than parsed AST nodes. q2 must match:
//!
//! | Context | What expands | Opt-outs |
//! |---|---|---|
//! | `CodeBlock`, `Code`, `RawBlock`, `RawInline` | `.text` | class `cell-code`; attr `shortcodes="false"` |
//! | `Math` | `.text` | none |
//! | `Header`, `Div`, `Span`, `Image`, `Link` | attribute values (not id/classes) | none |
//! | `Image` | additionally `src` | — |
//! | `Link` | additionally `target` | — |
//!
//! Escaped `{{{< … >}}}` renders as literal `{{< … >}}`. Unresolved
//! shortcodes emit a plain `?key` marker in the text plus a Q-16-5
//! diagnostic (q2 policy — deliberately louder than Q1's silent
//! leave-literal).
//!
//! Q1 ground truth + design decisions:
//! `claude-notes/plans/2026-08-10-shortcodes-in-code-blocks.md`.
//!
//! Tests use `{{< meta … >}}` so they stay independent of process
//! environment and `_environment` files.

use std::path::Path;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

/// Render a single qmd document (front matter defines `vendor: LDAP`
/// plus any extra keys the body needs) and return the HTML.
fn render_doc(body: &str) -> String {
    render_doc_with_meta("", body)
}

fn render_doc_with_meta(extra_meta: &str, body: &str) -> String {
    let temp = TempDir::new().unwrap();
    let qmd_path = temp.path().join("doc.qmd");
    write_file(
        &qmd_path,
        &format!(
            "---\ntitle: Text Contexts\nvendor: LDAP\n{}---\n\n{}",
            extra_meta, body
        ),
    );
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let options = RenderToFileOptions::default();
    let result = render_to_file(&qmd_path, "html", &options, runtime).expect("render failed");
    read(&result.output_path)
}

// ── Code contexts: text substitution ────────────────────────────────

/// The original bd-shortcodes-in-code-blocks-hhpus9da symptom: a
/// fenced code block containing `{{< meta vendor >}}` substitutes the
/// metadata value (Q1 parity; Connect docs LDAP pages).
#[test]
fn fenced_code_block_substitutes_meta_shortcode() {
    let html = render_doc(
        "```{.ini filename=\"example.gcfg\"}\n[Section \"corporate {{< meta vendor >}}\"]\n```\n",
    );
    assert!(
        html.contains("[Section &quot;corporate LDAP&quot;]"),
        "code block text must substitute the shortcode; got code line: {}",
        line_containing(&html, "Section")
    );
    assert!(
        !html.contains("{{&lt; meta"),
        "no escaped literal shortcode may remain in output"
    );
}

/// Plain (attribute-less) fenced code blocks substitute too.
#[test]
fn plain_fenced_code_block_substitutes() {
    let html = render_doc("```\nvendor = {{< meta vendor >}}\n```\n");
    assert!(
        html.contains("vendor = LDAP"),
        "plain code block must substitute; got: {}",
        line_containing(&html, "vendor")
    );
}

/// Inline code substitutes.
#[test]
fn inline_code_substitutes() {
    let html = render_doc("Run `connect --vendor {{< meta vendor >}}` now.\n");
    assert!(
        html.contains("connect --vendor LDAP"),
        "inline code must substitute; got: {}",
        line_containing(&html, "connect")
    );
}

/// The escaped form `{{{< … >}}}` renders as a literal single-brace
/// shortcode, not a substituted value.
#[test]
fn escaped_shortcode_in_code_block_stays_literal() {
    let html = render_doc("```\nuse {{{< meta vendor >}}} to reference metadata\n```\n");
    assert!(
        html.contains("use {{&lt; meta vendor &gt;}} to reference metadata"),
        "escaped shortcode must render as single-brace literal; got: {}",
        line_containing(&html, "reference metadata")
    );
    assert!(
        !html.contains("use LDAP"),
        "escaped shortcode must not substitute"
    );
}

/// A bare escaped shortcode as the *entire* code text unescapes to
/// the single-brace form (regression: the segment-count heuristic in
/// `parse_text_shortcodes` used to report "nothing parsed" for a
/// single escape-only segment, leaving the triple braces in place).
#[test]
fn bare_escaped_shortcode_in_inline_code_unescapes() {
    let html = render_doc("Use `{{{< meta vendor >}}}` in your docs.\n");
    assert!(
        html.contains("<code>{{&lt; meta vendor &gt;}}</code>"),
        "bare escaped shortcode in inline code must unescape once; got: {}",
        line_containing(&html, "meta vendor")
    );
}

/// `shortcodes="false"` on a code block opts the whole block out
/// (Q1's documented technique for displaying shortcode syntax).
#[test]
fn shortcodes_false_attribute_opts_out() {
    let html =
        render_doc("```{.markdown shortcodes=\"false\"}\nvendor is {{< meta vendor >}}\n```\n");
    assert!(
        html.contains("vendor is {{&lt; meta vendor &gt;}}"),
        "shortcodes=false block must keep the literal text; got: {}",
        line_containing(&html, "vendor is")
    );
}

/// Raw HTML blocks substitute textually.
#[test]
fn raw_block_substitutes() {
    let html =
        render_doc("```{=html}\n<div id=\"raw-target\">vendor: {{< meta vendor >}}</div>\n```\n");
    assert!(
        html.contains("<div id=\"raw-target\">vendor: LDAP</div>"),
        "raw block must substitute; got: {}",
        line_containing(&html, "raw-target")
    );
}

/// Raw inlines substitute textually.
#[test]
fn raw_inline_substitutes() {
    let html = render_doc("Before `<b id=\"raw-inline\">{{< meta vendor >}}</b>`{=html} after.\n");
    assert!(
        html.contains("<b id=\"raw-inline\">LDAP</b>"),
        "raw inline must substitute; got: {}",
        line_containing(&html, "raw-inline")
    );
}

/// Math text substitutes (Q1 walks Math through the same text
/// substitution).
#[test]
fn math_text_substitutes() {
    let html = render_doc_with_meta(
        "coeff: \"42\"\n",
        "The value $x = {{< meta coeff >}}y$ holds.\n",
    );
    assert!(
        html.contains("x = 42y"),
        "math text must substitute; got: {}",
        line_containing(&html, "x =")
    );
}

// ── Attribute values, image src, link target ────────────────────────

/// Span attribute values substitute (id and classes do not).
#[test]
fn span_attribute_value_substitutes() {
    let html = render_doc("A [tagged]{#anchor data-vendor=\"{{< meta vendor >}}\"} word.\n");
    assert!(
        html.contains("data-vendor=\"LDAP\""),
        "span attribute value must substitute; got: {}",
        line_containing(&html, "data-vendor")
    );
    assert!(
        html.contains("id=\"anchor\""),
        "span id must be untouched; got: {}",
        line_containing(&html, "anchor")
    );
}

/// Header attribute values substitute.
#[test]
fn header_attribute_value_substitutes() {
    let html = render_doc("## Section {data-vendor=\"{{< meta vendor >}}\"}\n\nBody.\n");
    assert!(
        html.contains("data-vendor=\"LDAP\""),
        "header attribute value must substitute; got: {}",
        line_containing(&html, "data-vendor")
    );
}

/// Div attribute values substitute.
#[test]
fn div_attribute_value_substitutes() {
    let html = render_doc("::: {data-vendor=\"{{< meta vendor >}}\"}\nInside.\n:::\n");
    assert!(
        html.contains("data-vendor=\"LDAP\""),
        "div attribute value must substitute; got: {}",
        line_containing(&html, "data-vendor")
    );
}

/// A shortcode as a link target resolves to the target URL.
#[test]
fn link_target_substitutes() {
    let html = render_doc_with_meta(
        "docs-url: \"https://example.com/docs\"\n",
        "See [the docs]({{< meta docs-url >}}) for more.\n",
    );
    assert!(
        html.contains("href=\"https://example.com/docs\""),
        "link target must substitute; got: {}",
        line_containing(&html, "href")
    );
}

/// A shortcode as an image src resolves to the image path.
#[test]
fn image_src_substitutes() {
    let html = render_doc_with_meta(
        "logo-path: \"assets/logo.png\"\n",
        "![Logo]({{< meta logo-path >}})\n",
    );
    assert!(
        html.contains("src=\"assets/logo.png\""),
        "image src must substitute; got: {}",
        line_containing(&html, "src=")
    );
}

// ── Unresolved policy ───────────────────────────────────────────────

/// An unresolved shortcode in code text leaves a plain `?key` marker
/// (and emits a Q-16-5 diagnostic — asserted at the unit level, where
/// diagnostics are directly observable).
#[test]
fn unresolved_shortcode_in_code_block_leaves_marker() {
    let html = render_doc("```\nvalue: {{< meta no-such-key >}}\n```\n");
    assert!(
        html.contains("value: ?meta:no-such-key"),
        "unresolved shortcode must leave the ?key marker; got: {}",
        line_containing(&html, "value:")
    );
}

/// Body text still substitutes on the same page as code contexts —
/// the two paths must not interfere.
#[test]
fn body_and_code_both_substitute_on_same_page() {
    let html = render_doc("Body {{< meta vendor >}}.\n\n```\ncode {{< meta vendor >}}\n```\n");
    assert!(html.contains("Body LDAP."), "body must substitute");
    assert!(html.contains("code LDAP"), "code must substitute");
}

fn line_containing<'a>(html: &'a str, needle: &str) -> &'a str {
    html.lines()
        .find(|l| l.contains(needle))
        .unwrap_or("<no line found>")
}
