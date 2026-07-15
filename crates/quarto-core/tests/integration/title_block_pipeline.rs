/*
 * tests/title_block_pipeline.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Phase 0 of the HTML title-block parity epic (bd-gx9cic8z /
 * bd-xj96vafq): baseline snapshots of the `#title-block-header`
 * subtree, rendered through the real render-to-file orchestrator
 * (the same path `q2 render` drives).
 *
 * Plan: claude-notes/plans/2026-07-15-html-title-block-parity.md
 */

//! Title-block baseline snapshots.
//!
//! These snapshots pin the **current** title-block markup for a corpus
//! of fixtures covering the Quarto 1 title-block feature surface. They
//! are GREEN immediately: they assert existing behaviour, including
//! behaviour that is known to be wrong or missing relative to Q1
//! (structured authors rendering as concatenated booleans —
//! bd-8v34zny5 — and banner/categories/keywords fixtures rendering as
//! if the options were absent).
//!
//! Each parity phase (P1–P6, see the plan) changes this markup on
//! purpose; the snapshot diff is the review artifact for that phase.
//! Per the project snapshot policy, every snapshot change must be
//! called out in the commit message.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::render_to_file::RenderToFileOptions;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// Render a single-document project through the real orchestrator
/// (`ProjectPipeline` with `RenderToFileOptions`, the `q2 render`
/// path) and return the rendered HTML of `doc.html`.
fn render_doc_to_html(qmd: &str) -> String {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write(&project_dir.join("doc.qmd"), qmd);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project =
        ProjectContext::discover(&project_dir, runtime.as_ref()).expect("discover project");
    let options = RenderToFileOptions::default();
    let project_type = project_type_for(&project);
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        Format::html(),
        "html",
        &options,
        runtime.clone(),
    );
    let summary = pollster::block_on(pipeline.run()).expect("pipeline run");
    assert!(
        summary.pass1_failures.is_empty(),
        "pass1 failures: {:?}",
        summary
            .pass1_failures
            .iter()
            .map(|f| (&f.input, &f.error))
            .collect::<Vec<_>>()
    );
    assert!(
        summary.pass2_failures.is_empty(),
        "pass2 failures: {:?}",
        summary
            .pass2_failures
            .iter()
            .map(|f| (&f.input, &f.error))
            .collect::<Vec<_>>()
    );

    let output_path = project.output_dir.join("doc.html");
    std::fs::read_to_string(&output_path)
        .unwrap_or_else(|e| panic!("read rendered {}: {e}", output_path.display()))
}

/// Extract the `<header id="title-block-header" …>…</header>` subtree.
///
/// The title block never nests another `<header>`, so scanning to the
/// first `</header>` after the opening tag is sufficient.
fn extract_title_block(html: &str) -> String {
    let start = html
        .find("<header id=\"title-block-header\"")
        .expect("rendered HTML must contain #title-block-header");
    let end_rel = html[start..]
        .find("</header>")
        .expect("title block header must be closed");
    html[start..start + end_rel + "</header>".len()].to_string()
}

/// Extract the opening `<main …>` tag. Banner mode (P5) must add the
/// `quarto-banner-title-block` class here; the baseline pins its
/// current shape.
fn extract_main_open_tag(html: &str) -> String {
    let start = html
        .find("<main")
        .expect("rendered HTML must contain <main");
    let end_rel = html[start..].find('>').expect("main tag must close");
    html[start..start + end_rel + 1].to_string()
}

// ─────────────────────────────────────────────────────────────────────
// Fixtures — one concern per document, kept minimal on purpose.
// ─────────────────────────────────────────────────────────────────────

/// The basic surface that already renders: title, subtitle, scalar
/// author, date, abstract.
const SIMPLE: &str = r#"---
title: "A Simple Document"
subtitle: "With a subtitle"
author: "Norah Jones"
date: "2026-07-01"
abstract: |
  This is the abstract. It has more than one sentence so we can
  see how the abstract is laid out.
---

## Introduction

Some body text.
"#;

/// Structured authors with affiliations, ORCID, email, url. Today the
/// template flattens the config maps into boolean garbage
/// (bd-8v34zny5); the snapshot documents that until P2 (bd-ez0hiowa)
/// replaces it with the normalized author model.
const RICH_AUTHORS: &str = r#"---
title: "Structured Authors"
author:
  - name: Norah Jones
    orcid: 0000-0002-1825-0097
    email: norah@example.com
    url: https://example.com/norah
    corresponding: true
    affiliations:
      - name: Carnegie Mellon University
        department: School of Music
  - name: Bill Malone
    affiliations:
      - name: University of Texas
---

Body.
"#;

/// The rest of the metadata grid: date-modified, doi, keywords,
/// description, categories. None of these render today; P3
/// (bd-j6huijli) adds them.
const METADATA_GRID: &str = r#"---
title: "Metadata Grid"
author: "Norah Jones"
date: "2026-07-01"
date-modified: "2026-07-10"
doi: "10.1234/example.5678"
description: "A one-line description."
keywords:
  - music
  - texas
categories:
  - analysis
  - jazz
---

Body.
"#;

/// Banner mode. Today the option is ignored; P5 (bd-364ol5lu) emits
/// the `.quarto-title-banner` structure above `#quarto-content` and
/// adds `quarto-banner-title-block` to `<main>`.
const BANNER_TRUE: &str = r#"---
title: "Banner Document"
subtitle: "With a banner"
author: "Norah Jones"
date: "2026-07-01"
title-block-banner: true
---

Body.
"#;

/// Label overrides. Today they are ignored; P1 (bd-tezzk9vp) honours
/// them.
const LABEL_OVERRIDES: &str = r#"---
title: "Label Overrides"
author: "Norah Jones"
date: "2026-07-01"
abstract: "A short abstract."
author-title: "Written by"
published-title: "Posted"
abstract-title: "Summary"
---

Body.
"#;

/// `title-block-style: plain`. Today ignored; P6 (bd-vkiwhcny).
const STYLE_PLAIN: &str = r#"---
title: "Plain Style"
author: "Norah Jones"
title-block-style: plain
---

Body.
"#;

/// `title-block-style: none`. Today ignored; P6 (bd-vkiwhcny).
const STYLE_NONE: &str = r#"---
title: "No Title Block"
author: "Norah Jones"
title-block-style: none
---

Body.
"#;

// ─────────────────────────────────────────────────────────────────────
// Baseline snapshots
// ─────────────────────────────────────────────────────────────────────

#[test]
fn title_block_simple_baseline() {
    let html = render_doc_to_html(SIMPLE);
    insta::assert_snapshot!("title_block_simple", extract_title_block(&html));
}

#[test]
fn title_block_rich_authors_baseline() {
    let html = render_doc_to_html(RICH_AUTHORS);
    // Documents bd-8v34zny5: the snapshot currently contains the
    // flattened-boolean author rendering. P2 fixes it.
    insta::assert_snapshot!("title_block_rich_authors", extract_title_block(&html));
}

#[test]
fn title_block_metadata_grid_baseline() {
    let html = render_doc_to_html(METADATA_GRID);
    insta::assert_snapshot!("title_block_metadata_grid", extract_title_block(&html));
}

#[test]
fn title_block_banner_true_baseline() {
    let html = render_doc_to_html(BANNER_TRUE);
    insta::assert_snapshot!("title_block_banner_true", extract_title_block(&html));
    insta::assert_snapshot!(
        "title_block_banner_true_main_tag",
        extract_main_open_tag(&html)
    );
}

#[test]
fn title_block_label_overrides_baseline() {
    let html = render_doc_to_html(LABEL_OVERRIDES);
    insta::assert_snapshot!("title_block_label_overrides", extract_title_block(&html));
}

#[test]
fn title_block_style_plain_baseline() {
    let html = render_doc_to_html(STYLE_PLAIN);
    insta::assert_snapshot!("title_block_style_plain", extract_title_block(&html));
}

#[test]
fn title_block_style_none_baseline() {
    let html = render_doc_to_html(STYLE_NONE);
    insta::assert_snapshot!("title_block_style_none", extract_title_block(&html));
}
