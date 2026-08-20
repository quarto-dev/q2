/*
 * tests/integration/include_nested_expansion.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Full-pipeline contract for `{{< include >}}` inside container
 * blocks (bd-1fz3vh99).
 */

//! Includes are expanded at every block-list position — not just the
//! top level. These tests drive the real HTML pipeline over fixtures
//! with includes nested in a callout div and a list item, asserting
//! the included content reaches the rendered HTML *inside* the
//! container, with no "include not expanded" (Q-17-4) or "unknown
//! shortcode" (Q-16-3) diagnostics.
//!
//! Plan: `claude-notes/plans/2026-08-07-nested-include-expansion.md`.

use crate::include_expansion_diagnostics::{codes, render_fixture};

const MARKER: &str = "NESTED-INCLUDE-MARKER";

fn assert_no_include_position_diagnostics(codes: &[&str]) {
    for code in ["Q-16-3", "Q-17-4"] {
        assert!(
            !codes.contains(&code),
            "nested include must expand without {} — got {:?}",
            code,
            codes
        );
    }
}

#[tokio::test]
async fn include_inside_callout_reaches_html() {
    let output = render_fixture(
        &[
            (
                "index.qmd",
                "---\ntitle: Nested include\n---\n\n::: {.callout-note}\n{{< include \"_inc.qmd\" >}}\n:::\n",
            ),
            ("_inc.qmd", "NESTED-INCLUDE-MARKER\n\nsecond included paragraph\n"),
        ],
        "index.qmd",
    )
    .await;

    assert_no_include_position_diagnostics(&codes(&output));

    let marker_pos = output
        .html
        .find(MARKER)
        .unwrap_or_else(|| panic!("included content must reach the HTML:\n{}", output.html));
    let callout_pos = output.html.find("callout-note").expect("callout renders");
    assert!(
        callout_pos < marker_pos,
        "included content must appear inside the callout (callout at {}, marker at {})",
        callout_pos,
        marker_pos
    );
    assert!(
        output.html.contains("second included paragraph"),
        "all included blocks must land in the output"
    );
}

#[tokio::test]
async fn include_inside_list_item_reaches_html() {
    let output = render_fixture(
        &[
            (
                "index.qmd",
                "---\ntitle: Nested include\n---\n\n- {{< include \"_inc.qmd\" >}}\n- second item\n",
            ),
            ("_inc.qmd", "NESTED-INCLUDE-MARKER\n\nsecond included paragraph\n"),
        ],
        "index.qmd",
    )
    .await;

    assert_no_include_position_diagnostics(&codes(&output));

    // The marker must sit inside the first <li>.
    let li_start = output.html.find("<li").expect("list renders");
    let li_end = output.html[li_start..]
        .find("</li>")
        .map(|off| li_start + off)
        .expect("list item closes");
    let first_item = &output.html[li_start..li_end];
    assert!(
        first_item.contains(MARKER),
        "included content must land inside the first list item; item was:\n{}\nfull html:\n{}",
        first_item,
        output.html
    );
    assert!(
        output.html.contains("second item"),
        "sibling item must survive"
    );
}
