/*
 * tests/integration/listing_description_precedence.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! End-to-end tests for listing description / image precedence
//! (bd-listing-description-precedence-x4bh6w3m, which also covers
//! bd-listing-default-no-derived-desc-m0wrr8ty).
//!
//! The rule, for an item backed by a project document:
//!
//! ```text
//! description: listing-item.description → description → abstract → derived
//! image:       listing-item.image       → image       → derived
//! ```
//!
//! "Derived" is the rendered page's first paragraph / preview image
//! (the L7 post-render upgrade), with the pre-engine first paragraph /
//! body image as the fallback inside the placeholder envelope. Quarto 1
//! uses the same rule (`website-listing-read.ts`: `description ||
//! abstract || placeholder`); `listing-item:` is a q2-only override
//! that wins over the top-level keys.
//!
//! Each test drives a fixture through `ProjectPipeline`, so the
//! listing reads real Pass-1 profiles.
//!
//! Plan: `claude-notes/plans/2026-10-09-listing-description-precedence.md`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::document_profile::DocumentProfile;
use quarto_core::format::Format;
use quarto_core::pipeline::{build_html_pipeline_stages, run_pipeline};
use quarto_core::project::orchestrator::{ProjectPipeline, ProjectRenderSummary, project_type_for};
use quarto_core::project::pass2_renderer::RenderToFileRenderer;
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_core::render_to_file::RenderToFileOptions;
use quarto_core::stage::PipelineStage;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

fn runtime_arc() -> Arc<dyn SystemRuntime> {
    Arc::new(NativeRuntime::new())
}

/// Write the fixture into a fresh temp project and return its
/// (canonical) directory. The temp dir is leaked so tests can read
/// outputs afterwards; cleanup happens at process exit.
fn project_with(fixture: impl FnOnce(&Path)) -> PathBuf {
    let temp = TempDir::new().unwrap();
    let project_dir = temp
        .path()
        .canonicalize()
        .unwrap_or_else(|_| temp.path().to_path_buf());
    fixture(&project_dir);
    std::mem::forget(temp);
    project_dir
}

fn new_pipeline<'a>(
    project: &'a mut ProjectContext,
    options: &'a RenderToFileOptions,
    runtime: Arc<dyn SystemRuntime>,
) -> ProjectPipeline<'a, RenderToFileRenderer<'a>> {
    let project_type = project_type_for(project);
    ProjectPipeline::new(
        project,
        project_type,
        Format::html(),
        "html",
        options,
        runtime,
    )
}

fn render(project_dir: &Path) -> ProjectRenderSummary {
    let runtime = runtime_arc();
    let mut project = ProjectContext::discover(project_dir, runtime.as_ref()).unwrap();
    let options = RenderToFileOptions::default();
    let mut pipeline = new_pipeline(&mut project, &options, runtime.clone());
    let summary = pollster::block_on(pipeline.run()).expect("pipeline");
    assert!(
        summary.pass1_failures.is_empty() && summary.pass2_failures.is_empty(),
        "unexpected failures: pass1={:?} pass2={:?}",
        summary.pass1_failures,
        summary.pass2_failures
    );
    summary
}

fn listing_page(listing_type: &str) -> String {
    let fields = if listing_type == "table" {
        "\n  fields: [title, description]"
    } else {
        ""
    };
    format!(
        "---\ntitle: {listing_type} listing\nlisting:\n  type: {listing_type}\n  contents: posts{fields}\n---\n"
    )
}

/// The repro from the investigation: one post per description source,
/// each with a body paragraph the derivation would otherwise use.
fn write_description_fixture(dir: &Path) {
    write(&dir.join("_quarto.yml"), "project:\n  type: website\n");
    write(
        &dir.join("posts/a.qmd"),
        "---\ntitle: Post A\ndescription: EXPLICIT-A\n---\n\nBODY-A paragraph.\n",
    );
    write(
        &dir.join("posts/b.qmd"),
        "---\ntitle: Post B\n---\n\nBODY-B paragraph.\n",
    );
    write(
        &dir.join("posts/c.qmd"),
        "---\ntitle: Post C\nlisting-item:\n  description: LISTING-ITEM-C\n---\n\nBODY-C paragraph.\n",
    );
    write(
        &dir.join("posts/d.qmd"),
        "---\ntitle: Post D\nabstract: ABSTRACT-D\n---\n\nBODY-D paragraph.\n",
    );
    write(
        &dir.join("posts/e.qmd"),
        "---\ntitle: Post E\ndescription: EXPLICIT-E\nlisting-item:\n  description: LISTING-ITEM-E\n---\n\nBODY-E paragraph.\n",
    );
    for t in ["default", "grid", "table"] {
        write(&dir.join(format!("{t}.qmd")), &listing_page(t));
    }
}

fn assert_shows(html: &str, page: &str, shown: &[&str], hidden: &[&str]) {
    for s in shown {
        assert!(html.contains(s), "{page}: expected `{s}` in the listing");
    }
    for s in hidden {
        assert!(
            !html.contains(s),
            "{page}: `{s}` must not appear in the listing"
        );
    }
}

#[test]
fn listing_description_follows_precedence_in_every_listing_type() {
    let dir = project_with(write_description_fixture);
    let summary = render(&dir);

    for t in ["default", "grid", "table"] {
        let page = format!("{t}.html");
        let html = read(&dir.join("_site").join(&page));
        assert_shows(
            &html,
            &page,
            &[
                // Top-level `description:` is authored: shown as written.
                "EXPLICIT-A",
                // Nothing authored: the first paragraph is derived.
                "BODY-B",
                // `listing-item.description` is authored.
                "LISTING-ITEM-C",
                // `abstract:` is the Q1 fallback before derivation.
                "ABSTRACT-D",
                // `listing-item.description` beats `description`.
                "LISTING-ITEM-E",
            ],
            &["BODY-A", "BODY-C", "BODY-D", "BODY-E", "EXPLICIT-E"],
        );
        assert!(
            !html.contains("desc-begin(") && !html.contains("desc-end("),
            "{page}: description envelopes must be substituted away"
        );
    }
    assert!(
        summary
            .project_diagnostics
            .iter()
            .all(|d| d.code.as_deref() != Some("Q-12-13")),
        "no Q-12-13 expected; got {:?}",
        summary.project_diagnostics
    );
}

/// A page whose only paragraph exists in the *rendered* output (the
/// shape of a page whose body starts with engine output): L1 finds no
/// paragraph, so the item has no description before L7 — the envelope
/// must still be emitted so L7 can fill it from the rendered page.
#[test]
fn listing_derives_description_present_only_in_rendered_output() {
    let dir = project_with(|dir| {
        write(&dir.join("_quarto.yml"), "project:\n  type: website\n");
        // A raw HTML block is not a Para/Plain to L1, but it is a
        // `<p>` in the rendered page — the same thing L7 sees for
        // engine-produced output.
        write(
            &dir.join("posts/engine.qmd"),
            "---\ntitle: Engine first\n---\n\n```{=html}\n<p>RENDERED-ONLY paragraph.</p>\n```\n",
        );
        for t in ["default", "grid", "table"] {
            write(&dir.join(format!("{t}.qmd")), &listing_page(t));
        }
    });
    render(&dir);
    for t in ["default", "grid", "table"] {
        let page = format!("{t}.html");
        let html = read(&dir.join("_site").join(&page));
        assert_shows(&html, &page, &["RENDERED-ONLY paragraph."], &[]);
    }
}

#[test]
fn listing_image_follows_precedence() {
    let dir = project_with(|dir| {
        write(&dir.join("_quarto.yml"), "project:\n  type: website\n");
        // Authored top-level `image:` plus a different body image:
        // the authored one wins.
        write(
            &dir.join("posts/explicit.qmd"),
            "---\ntitle: Explicit image\nimage: explicit-thumb.png\n---\n\nText.\n\n![](explicit-body.png)\n",
        );
        // No `image:`: the body image is derived.
        write(
            &dir.join("posts/derived.qmd"),
            "---\ntitle: Derived image\n---\n\nText.\n\n![](derived-body.png)\n",
        );
        // `listing-item.image` beats `image`.
        write(
            &dir.join("posts/both.qmd"),
            "---\ntitle: Both images\nimage: both-top.png\nlisting-item:\n  image: both-li.png\n---\n\nText.\n\n![](both-body.png)\n",
        );
        for name in [
            "explicit-thumb",
            "explicit-body",
            "derived-body",
            "both-top",
            "both-li",
            "both-body",
        ] {
            write(&dir.join(format!("posts/{name}.png")), "not really a png");
        }
        for t in ["default", "grid"] {
            write(&dir.join(format!("{t}.qmd")), &listing_page(t));
        }
    });
    render(&dir);
    for t in ["default", "grid"] {
        let page = format!("{t}.html");
        let html = read(&dir.join("_site").join(&page));
        assert_shows(
            &html,
            &page,
            &["explicit-thumb.png", "derived-body.png", "both-li.png"],
            &["explicit-body.png", "both-top.png", "both-body.png"],
        );
        assert!(
            !html.contains("img-begin(") && !html.contains("img-end("),
            "{page}: image envelopes must be substituted away"
        );
    }
}

// ─────────────────────────────────────────────────────────────────
// Pass-1 profiles must equal the full pipeline's profiles. Listings
// read Pass-1 profiles; when Pass-1 ran fewer pre-checkpoint stages
// than the full pipeline, every value those stages contribute
// (listing autofill, include-in-header dependencies) silently
// vanished from listings.
// ─────────────────────────────────────────────────────────────────

/// The full pipeline's profile for `doc`, computed through the same
/// `run_pipeline` entry point Pass-1 uses, with the full pipeline's
/// stages up to the profile checkpoint.
fn full_pipeline_profile(
    project: &ProjectContext,
    doc: &DocumentInfo,
    runtime: Arc<dyn SystemRuntime>,
) -> DocumentProfile {
    let stages: Vec<Box<dyn PipelineStage>> = build_html_pipeline_stages();
    let checkpoint = stages
        .iter()
        .position(|s| s.name() == "document-profile")
        .expect("document-profile stage present");
    // Keep everything through the checkpoint *and* the stages that
    // still write to the profile after it (link resolution runs on
    // the `AtProfile` value). Stop before the profile is unwrapped.
    let unwrap = stages
        .iter()
        .position(|s| s.name() == "unwrap-profile")
        .expect("unwrap-profile stage present");
    assert!(checkpoint < unwrap);
    let head: Vec<Box<dyn PipelineStage>> = stages.into_iter().take(unwrap).collect();

    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(project, doc, &format, &binaries);
    let source = std::fs::read(&doc.input).unwrap();
    let (output, _) = pollster::block_on(run_pipeline(
        &source,
        &doc.input.to_string_lossy(),
        &mut ctx,
        runtime,
        head,
    ))
    .expect("full-pipeline head");
    output
        .into_at_profile()
        .expect("head ends at the profile checkpoint")
        .profile
}

#[test]
fn pass1_profiles_equal_full_pipeline_profiles() {
    let dir = project_with(|dir| {
        write(
            &dir.join("_quarto.yml"),
            "project:\n  type: website\nlang: fr\n",
        );
        write(
            &dir.join("header.html"),
            "<meta name=\"x-test\" content=\"1\">\n",
        );
        write(&dir.join("_child.qmd"), "Included paragraph.\n");
        write(
            &dir.join("page.qmd"),
            "---\ntitle: Page\ninclude-in-header: header.html\n---\n\n{{< include _child.qmd >}}\n\nSecond paragraph.\n\n![](figure.png)\n",
        );
        write(&dir.join("figure.png"), "not really a png");
        write(&dir.join("index.qmd"), &listing_page("default"));
    });

    let runtime = runtime_arc();
    let mut project = ProjectContext::discover(&dir, runtime.as_ref()).unwrap();
    let options = RenderToFileOptions::default();
    let pass1 = {
        let pipeline = new_pipeline(&mut project, &options, runtime.clone());
        let (profiles, failures) = pollster::block_on(pipeline.__pass_one_for_test_only());
        assert!(failures.is_empty(), "pass-1 failures: {failures:?}");
        profiles
    };
    assert!(!pass1.is_empty());

    for p1 in &pass1 {
        let doc = project
            .files
            .iter()
            .find(|f| f.input.ends_with(&p1.source_path))
            .unwrap_or_else(|| panic!("no project file for {}", p1.source_path.display()));
        let full = full_pipeline_profile(&project, doc, runtime.clone());
        assert_eq!(
            p1,
            &full,
            "Pass-1 profile for {} differs from the full pipeline's",
            p1.source_path.display()
        );
    }
}
