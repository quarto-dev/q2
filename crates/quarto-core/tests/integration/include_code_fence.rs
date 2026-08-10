/*
 * tests/integration/include_code_fence.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Full-pipeline contract for `{{< include >}}` inside a fenced code
 * block (bd-include-in-code-block-f8mvtczn).
 */

//! The Quarto 1 idiom for embedding a source file as a listing is an
//! include standing alone inside a fenced code block. Before this
//! change the whole fence body rendered as the single token
//! `?include`, with a `Q-17-4` warning whose hint ("put it in its own
//! paragraph") was actively wrong for this position.
//!
//! These tests drive the **real HTML pipeline** — through
//! `AstTransformsStage`, where `ShortcodeResolveTransform` produced
//! that `?include` — because the stage-level unit tests in
//! `include_expansion.rs` cannot see it. That transform is the reason
//! this file exists: a fix verified only at the stage would look
//! correct and still ship the bug.
//!
//! Plan: `claude-notes/plans/2026-08-10-include-in-code-block.md`.

use crate::include_expansion_diagnostics::{codes, render_fixture};

const APP_PY: &str = "import os\n\nprint(\"hello from app.py\")\n";

/// Strip tags and unescape the entities the HTML writer emits, leaving
/// the text a reader actually sees. Used where syntax-highlighting
/// markup would otherwise dominate the assertion.
fn visible_text(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

/// The rendered `<code>` body of the first listing in `html`.
///
/// Deliberately class-agnostic: a `.python` fence renders with
/// `sourceCode` highlighting classes while a `.markdown` one does not,
/// and both shapes appear in these tests.
fn listing_body(html: &str) -> String {
    let start = html
        .find("<pre")
        .unwrap_or_else(|| panic!("no listing in output:\n{html}"));
    let rest = &html[start..];
    let code_at = rest
        .find("<code")
        .unwrap_or_else(|| panic!("listing has no <code> element:\n{rest}"));
    let open = code_at + rest[code_at..].find('>').expect("unclosed <code> tag") + 1;
    let end = rest.find("</code>").expect("listing closes its <code>");
    rest[open..end].to_string()
}

#[tokio::test]
async fn code_fence_include_renders_the_file_contents() {
    let output = render_fixture(
        &[
            (
                "index.qmd",
                "---\ntitle: Listing\n---\n\n\
                 ```{.python filename=\"app.py\"}\n\
                 {{< include app.py >}}\n\
                 ```\n",
            ),
            ("app.py", APP_PY),
        ],
        "index.qmd",
    )
    .await;

    assert!(
        output.diagnostics.is_empty(),
        "expected a clean render, got {:?}",
        codes(&output)
    );

    let body = listing_body(&output.html);
    // The `?include` token is what this strand exists to remove.
    assert!(
        !output.html.contains("?include"),
        "fence body still shows the error token:\n{body}"
    );
    // Content is present and syntax-highlighted (it went through the
    // normal listing path, not a raw dump).
    assert!(body.contains("import"), "missing file content:\n{body}");
    assert!(
        body.contains("hello from app.py"),
        "missing file content:\n{body}"
    );
    assert!(
        body.contains("<span class=\"hl-"),
        "expected highlighting spans:\n{body}"
    );
    // The fence's own attributes survive the splice.
    assert!(
        output.html.contains("data-filename=\"app.py\""),
        "lost the filename attribute"
    );
    // D4: exactly one trailing newline trimmed, so the listing has no
    // blank final line. Q1's rendered output has none either.
    assert!(
        !body.ends_with('\n'),
        "listing gained a trailing blank line: {body:?}"
    );
}

#[tokio::test]
async fn code_fence_include_does_not_warn_q_17_4() {
    // The warning still exists for genuinely unsupported positions,
    // but this shape is now supported — and its old hint ("put the
    // shortcode in its own paragraph") would have told the author to
    // change what the page means.
    let output = render_fixture(
        &[
            (
                "index.qmd",
                "---\ntitle: Listing\n---\n\n```{.python}\n{{< include app.py >}}\n```\n",
            ),
            ("app.py", APP_PY),
        ],
        "index.qmd",
    )
    .await;

    assert!(
        !codes(&output).contains(&"Q-17-4"),
        "unexpected Q-17-4: {:?}",
        codes(&output)
    );
}

#[tokio::test]
async fn opted_out_code_fence_shows_the_shortcode_literally() {
    // `shortcodes="false"` is how documentation displays the syntax
    // itself; it must keep winning over expansion.
    let output = render_fixture(
        &[
            (
                "index.qmd",
                "---\ntitle: Listing\n---\n\n\
                 ```{.markdown shortcodes=\"false\"}\n\
                 {{< include app.py >}}\n\
                 ```\n",
            ),
            ("app.py", APP_PY),
        ],
        "index.qmd",
    )
    .await;

    assert!(
        output.diagnostics.is_empty(),
        "expected a clean render, got {:?}",
        codes(&output)
    );
    assert!(
        output.html.contains("{{&lt; include app.py &gt;}}"),
        "opted-out fence should show the shortcode literally:\n{}",
        output.html
    );
    assert!(
        !output.html.contains("hello from app.py"),
        "opted-out fence must not splice the file"
    );
}

#[tokio::test]
async fn mid_line_include_in_a_fence_stays_literal() {
    // Recognition is line-strict (D2, matching Q1), so this is *not*
    // an include — but before the fix the leftover shortcode still
    // reached ShortcodeResolveTransform and came out as `?include`,
    // corrupting the listing. Q1 renders the line untouched and
    // silently; so do we.
    let output = render_fixture(
        &[
            (
                "index.qmd",
                "---\ntitle: Listing\n---\n\n```{.python}\nx = 1  {{< include app.py >}}\n```\n",
            ),
            ("app.py", APP_PY),
        ],
        "index.qmd",
    )
    .await;

    let body = listing_body(&output.html);
    assert!(
        !body.contains("?include"),
        "mid-line include corrupted the listing:\n{body}"
    );
    // Compare the visible text: the line is syntax-highlighted, so the
    // markup around it is not what this test is about.
    assert_eq!(visible_text(&body), "x = 1  {{< include app.py >}}");
}

#[tokio::test]
async fn mid_line_include_in_inline_code_stays_literal() {
    // Same rule for inline code spans.
    let output = render_fixture(
        &[
            (
                "index.qmd",
                "---\ntitle: Listing\n---\n\nSee `x = {{< include app.py >}}` here.\n",
            ),
            ("app.py", APP_PY),
        ],
        "index.qmd",
    )
    .await;

    assert!(
        !output.html.contains("?include"),
        "inline code corrupted:\n{}",
        output.html
    );
    assert!(
        output
            .html
            .contains("<code>x = {{&lt; include app.py &gt;}}</code>"),
        "expected literal shortcode text in the code span:\n{}",
        output.html
    );
}

#[tokio::test]
async fn nested_code_fence_include_expands_without_token() {
    // The case that made "no recursion" untenable: with the spliced
    // text left alone, `{{< include inner.qmd >}}` survived to
    // ShortcodeResolveTransform and came out as `?include` — the
    // strand's own bug, one level down, plus a Q-17-4 pointing at a
    // fence the author never put an include in. Recursion (Q1's
    // behavior) removes the shortcode before that transform runs.
    let output = render_fixture(
        &[
            (
                "index.qmd",
                "---\ntitle: Listing\n---\n\n```{.markdown}\n{{< include outer.qmd >}}\n```\n",
            ),
            (
                "outer.qmd",
                "top line\n{{< include inner.qmd >}}\nbottom line\n",
            ),
            ("inner.qmd", "INNER-CONTENT\n"),
        ],
        "index.qmd",
    )
    .await;

    assert!(
        output.diagnostics.is_empty(),
        "expected a clean render, got {:?}",
        codes(&output)
    );
    assert!(
        !output.html.contains("?include"),
        "nested include leaked the error token:\n{}",
        output.html
    );
    let body = listing_body(&output.html);
    assert!(
        body.contains("INNER-CONTENT"),
        "nested include did not expand:\n{body}"
    );
    assert!(
        body.contains("top line") && body.contains("bottom line"),
        "lost the surrounding lines:\n{body}"
    );
}

#[tokio::test]
async fn self_including_code_fence_reports_a_cycle() {
    // Embedding a document in its own listing would recurse forever:
    // the spliced copy carries the same include line.
    let output = render_fixture(
        &[(
            "index.qmd",
            "---\ntitle: Listing\n---\n\n```{.markdown}\n{{< include index.qmd >}}\n```\n",
        )],
        "index.qmd",
    )
    .await;

    assert!(
        codes(&output).contains(&"Q-17-1"),
        "expected a circular-include warning, got {:?}",
        codes(&output)
    );
    assert!(
        !output.html.contains("?include"),
        "cycle leaked the error token:\n{}",
        output.html
    );
}

#[tokio::test]
async fn missing_code_fence_include_reports_q_17_2_without_token() {
    let output = render_fixture(
        &[(
            "index.qmd",
            "---\ntitle: Listing\n---\n\n```{.python}\nkept = 1\n{{< include gone.py >}}\n```\n",
        )],
        "index.qmd",
    )
    .await;

    assert!(
        codes(&output).contains(&"Q-17-2"),
        "expected a missing-file warning, got {:?}",
        codes(&output)
    );
    // The unresolved line is dropped rather than left to become
    // `?include` downstream.
    assert!(
        !output.html.contains("?include"),
        "unresolved include leaked the error token:\n{}",
        output.html
    );
    let body = listing_body(&output.html);
    assert!(body.contains("kept"), "lost the surrounding code:\n{body}");
}
