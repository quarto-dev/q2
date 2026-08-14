/*
 * toc_title_context.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * bd-website-toc-title-wn80ymab: the TOC title term is context-keyed —
 * website pages get `toc-title-website` ("On this page"), everything
 * else gets `toc-title-document` ("Table of contents").
 */

//! End-to-end coverage for **which** language term the TOC title uses.
//!
//! Quarto 1 picks between two catalog keys
//! (`src/command/render/pandoc.ts:493`):
//!
//! ```text
//! projectIsWebsite(project) && !projectIsBook(project)
//!     && isHtmlOutput(format.pandoc, /* strict */ true)
//!     ? "toc-title-website"      // "On this page"
//!     : "toc-title-document"     // "Table of contents"
//! ```
//!
//! q2 always used `toc-title-document`, so every website page in a
//! ported site said "Table of contents" (~324 of 352 pages in the Posit
//! Connect docs port).
//!
//! The unit tests in `crates/quarto-core/src/transforms/toc_generate.rs`
//! cover the predicate itself against a hand-built `RenderContext`.
//! These tests exist because that harness **bypasses the real
//! project-config path**: they drive `ProjectPipeline` over a temp
//! project on disk, so `project.type` is resolved by
//! `ProjectContext::discover` exactly as it is under `q2 render`.
//!
//! Plan: `claude-notes/plans/2026-08-14-website-toc-title-on-this-page.md`.

use crate::toc_markup::{render_index_with_toc, toc_nav};

/// The rendered `<h2 id="toc-title">…</h2>` text, sliced from the TOC
/// nav so a matching string elsewhere on the page cannot mask a defect.
fn toc_title(html: &str) -> String {
    let nav = toc_nav(html);
    let open = "<h2 id=\"toc-title\">";
    let start = nav
        .find(open)
        .unwrap_or_else(|| panic!("no {open} in TOC nav:\n{nav}"))
        + open.len();
    let rest = &nav[start..];
    let end = rest
        .find("</h2>")
        .unwrap_or_else(|| panic!("unterminated toc-title in TOC nav:\n{nav}"));
    rest[..end].to_string()
}

const BODY: &str = "## Section One\n\nBody.\n\n## Section Two\n\nMore body.\n";

/// The headline case: a website project renders "On this page".
#[test]
fn website_project_uses_the_website_term() {
    let html = render_index_with_toc(BODY, "", None);
    assert_eq!(
        toc_title(&html),
        "On this page",
        "a website page must render the `toc-title-website` term"
    );
}

/// A website project needs no `website:` key to be a website — the
/// project *type* is what decides.
///
/// This case is why the fix keys off `ProjectKind` rather than the
/// presence of a `website:` key in merged metadata: this config is a
/// valid website (the CLI reports `type: website`) and Q1 gives it
/// "On this page", but it has no `website:` key at all.
#[test]
fn website_project_without_a_website_key_uses_the_website_term() {
    let html = render_index_with_toc(BODY, "", Some("project:\n  type: website\n"));
    assert_eq!(
        toc_title(&html),
        "On this page",
        "website-ness comes from `project.type`, not from a `website:` key"
    );
}

/// Regression guard: a non-website project keeps "Table of contents".
#[test]
fn default_project_uses_the_document_term() {
    let html = render_index_with_toc(BODY, "", Some("project:\n  type: default\n"));
    assert_eq!(
        toc_title(&html),
        "Table of contents",
        "a default project must keep the `toc-title-document` term"
    );
}

/// Q1 excludes books explicitly (`!projectIsBook`, because its
/// `projectIsWebsite` is true for books too). q2 gets this free from
/// `ProjectKind::Book` being a distinct variant — pinned so a future
/// "website-like" helper cannot silently absorb books.
#[test]
fn book_project_uses_the_document_term() {
    let html = render_index_with_toc(BODY, "", Some("project:\n  type: book\n"));
    assert_eq!(
        toc_title(&html),
        "Table of contents",
        "a book must keep the `toc-title-document` term, matching Q1's !projectIsBook guard"
    );
}

/// An explicit `toc-title` outranks the localized term on a website —
/// the precedence chain from bd-llhlzd7p is unchanged.
#[test]
fn user_toc_title_outranks_the_website_term() {
    let html = render_index_with_toc(BODY, "toc-title: \"My Contents\"\n", None);
    assert_eq!(
        toc_title(&html),
        "My Contents",
        "an explicit `toc-title` must win over the website term"
    );
}

/// Localization flows through the website branch: with `lang: pt` the
/// website term comes from `_language-pt.yml`, not from a hardcoded
/// English string.
#[test]
fn website_term_is_localized() {
    let html = render_index_with_toc(BODY, "", Some("project:\n  type: website\nlang: pt\n"));
    assert_eq!(
        toc_title(&html),
        "Nesta página",
        "the website term must be read from the language catalog"
    );
}
