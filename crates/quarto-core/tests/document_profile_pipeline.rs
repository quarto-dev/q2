/*
 * tests/document_profile_pipeline.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Integration tests for the DocumentProfile pipeline checkpoint.
 */

//! Pipeline-level integration tests for the Phase-0 profile checkpoint.
//!
//! See `claude-notes/plans/2026-04-23-websites-phase-0.md` §Tests
//! (items 10–12):
//!
//! 10. `pipeline_at_profile_to_end_produces_expected_html` — byte-identical
//!     clone-and-resume. Load-bearing test for checkpoint resumability.
//! 11. `pipeline_profile_matches_metadata` — profile fields match the
//!     post-merge AST metadata.
//! 12. `wasm_pipeline_includes_profile_stage` — WASM pipeline also runs
//!     the profile checkpoint.

use std::path::PathBuf;
use std::sync::Arc;

use quarto_core::document_profile::DocumentProfile;
use quarto_core::format::Format;
use quarto_core::pipeline::{
    HtmlRenderConfig, build_html_pipeline_stages, build_wasm_html_pipeline, render_qmd_to_html,
};
use quarto_core::project::{DocumentInfo, ProjectConfig, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_core::stage::{
    DocumentProfileStage, LoadedSource, Pipeline, PipelineData, PipelineDataKind, PipelineStage,
    StageContext, UnwrapProfileStage,
};

const FIXTURE_QMD: &[u8] = b"---\n\
title: Profile fixture\n\
author: Jane Example\n\
date: 2026-04-23\n\
categories: [alpha, beta]\n\
---\n\
\n\
# Section one\n\
\n\
Hello.\n\
\n\
## Subsection\n\
\n\
More hello.\n\
\n\
# Section two\n\
\n\
Goodbye.\n";

fn test_project_dir() -> PathBuf {
    std::env::current_dir().unwrap()
}

fn make_project() -> ProjectContext {
    ProjectContext {
        dir: test_project_dir(),
        config: ProjectConfig::default(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path(test_project_dir().join("test.qmd"))],
        output_dir: test_project_dir(),
    }
}

fn make_document() -> DocumentInfo {
    DocumentInfo::from_path(test_project_dir().join("test.qmd"))
}

fn make_stage_context() -> StageContext {
    let runtime = Arc::new(quarto_system_runtime::NativeRuntime::new());
    let format = Format::html();
    StageContext::new(runtime, format, make_project(), make_document()).expect("stage context")
}

/// Find the position in `stages` of a stage whose `name()` equals `target`.
fn position_of(stages: &[Box<dyn PipelineStage>], target: &str) -> Option<usize> {
    stages.iter().position(|s| s.name() == target)
}

// ---------------------------------------------------------------------------
// Test 12: wasm pipeline includes the checkpoint stages.
// ---------------------------------------------------------------------------

#[test]
fn wasm_pipeline_includes_profile_stage() {
    let pipeline = build_wasm_html_pipeline();
    let names = pipeline.stage_names();
    assert!(
        names.contains(&"document-profile"),
        "WASM HTML pipeline must include DocumentProfileStage; got {names:?}"
    );
    assert!(
        names.contains(&"unwrap-profile"),
        "WASM HTML pipeline must include UnwrapProfileStage; got {names:?}"
    );

    // The unwrap stage must come immediately after the profile stage —
    // otherwise downstream stages would see the AtProfile variant and
    // fail their input-kind check.
    let p = names
        .iter()
        .position(|n| *n == "document-profile")
        .expect("document-profile present");
    assert_eq!(
        names.get(p + 1).copied(),
        Some("unwrap-profile"),
        "unwrap-profile must immediately follow document-profile"
    );
}

#[test]
fn html_pipeline_includes_profile_stage() {
    // Same invariant for the native HTML pipeline builder.
    let stages = build_html_pipeline_stages();
    let names: Vec<&str> = stages.iter().map(|s| s.name()).collect();
    assert!(
        names.contains(&"document-profile"),
        "HTML pipeline must include DocumentProfileStage; got {names:?}"
    );
    assert!(
        names.contains(&"unwrap-profile"),
        "HTML pipeline must include UnwrapProfileStage; got {names:?}"
    );
    let p = names
        .iter()
        .position(|n| *n == "document-profile")
        .expect("document-profile present");
    assert_eq!(names.get(p + 1).copied(), Some("unwrap-profile"));

    // Position: checkpoint is between metadata-merge and pre-engine-sugaring.
    let merge = names
        .iter()
        .position(|n| *n == "metadata-merge")
        .expect("metadata-merge present");
    let sugar = names
        .iter()
        .position(|n| *n == "pre-engine-sugaring")
        .expect("pre-engine-sugaring present");
    assert!(
        merge < p && p < sugar,
        "checkpoint must sit between metadata-merge (at {merge}) and \
         pre-engine-sugaring (at {sugar}); profile stage at {p}"
    );
}

// ---------------------------------------------------------------------------
// Test 11: profile matches post-merge metadata.
// ---------------------------------------------------------------------------

/// Build a head pipeline that runs every stage up to (and including)
/// `DocumentProfileStage`. Returns the extracted `DocumentAtProfile`.
async fn run_head_pipeline(content: &[u8]) -> quarto_core::stage::DocumentAtProfile {
    let full = build_html_pipeline_stages();
    let checkpoint =
        position_of(&full, "document-profile").expect("document-profile stage present in pipeline");
    let head: Vec<Box<dyn PipelineStage>> = full.into_iter().take(checkpoint + 1).collect();

    let pipeline = Pipeline::new(head).expect("head pipeline valid");
    let mut ctx = make_stage_context();
    let input = PipelineData::LoadedSource(LoadedSource::new(
        test_project_dir().join("test.qmd"),
        content.to_vec(),
    ));

    let out = pipeline.run(input, &mut ctx).await.expect("head pipeline");
    assert_eq!(out.kind(), PipelineDataKind::AtProfile);
    out.into_at_profile().unwrap()
}

#[tokio::test]
async fn pipeline_profile_matches_metadata() {
    let bundle = run_head_pipeline(FIXTURE_QMD).await;
    let profile = &bundle.profile;

    assert_eq!(profile.profile_version, DocumentProfile::VERSION);
    assert_eq!(profile.format_id, "html");
    assert_eq!(profile.title.as_deref(), Some("Profile fixture"));
    assert_eq!(profile.authors, vec!["Jane Example".to_string()]);
    assert_eq!(profile.date.as_deref(), Some("2026-04-23"));
    assert_eq!(
        profile.categories,
        vec!["alpha".to_string(), "beta".to_string()]
    );
    assert!(!profile.draft);

    // Outline: two top-level sections, first has one subsection.
    assert_eq!(profile.outline.len(), 2, "two top-level headings");
    assert_eq!(profile.outline[0].title, "Section one");
    assert_eq!(profile.outline[0].level, 1);
    assert_eq!(profile.outline[0].children.len(), 1);
    assert_eq!(profile.outline[0].children[0].title, "Subsection");
    assert_eq!(profile.outline[1].title, "Section two");

    // The inner ast is still present and usable.
    assert!(
        !bundle.ast.ast.blocks.is_empty(),
        "inner AST preserves document body"
    );
}

// ---------------------------------------------------------------------------
// Test 10: clone-and-resume produces byte-identical HTML to end-to-end.
// ---------------------------------------------------------------------------

async fn render_full(content: &[u8]) -> String {
    let project = make_project();
    let doc = make_document();
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

    let runtime = Arc::new(quarto_system_runtime::NativeRuntime::new());
    let config = HtmlRenderConfig::default();

    let output = render_qmd_to_html(content, "test.qmd", &mut ctx, &config, runtime)
        .await
        .expect("full render");
    output.html
}

/// Run the head pipeline to the checkpoint, clone the `AtProfile`
/// value, then run the tail pipeline on the clone. Returns the
/// resulting HTML.
async fn render_clone_and_resume(content: &[u8]) -> String {
    let full = build_html_pipeline_stages();
    let checkpoint =
        position_of(&full, "document-profile").expect("document-profile stage present in pipeline");

    // Split the stage list at the checkpoint.
    let mut head: Vec<Box<dyn PipelineStage>> = Vec::new();
    let mut tail: Vec<Box<dyn PipelineStage>> = Vec::new();
    for (idx, stage) in full.into_iter().enumerate() {
        if idx <= checkpoint {
            head.push(stage);
        } else {
            tail.push(stage);
        }
    }

    let head_pipeline = Pipeline::new(head).expect("head pipeline");
    let tail_pipeline = Pipeline::new(tail).expect("tail pipeline");

    let mut ctx = make_stage_context();
    let input = PipelineData::LoadedSource(LoadedSource::new(
        test_project_dir().join("test.qmd"),
        content.to_vec(),
    ));

    // Run to the checkpoint.
    let at_profile_data = head_pipeline
        .run(input, &mut ctx)
        .await
        .expect("head pipeline runs");
    assert_eq!(at_profile_data.kind(), PipelineDataKind::AtProfile);

    // Clone at the checkpoint, then drop the original to prove the
    // clone is self-sufficient. `PipelineData` isn't Clone as a whole
    // (LoadedSource etc. aren't), but the `AtProfile` inner value is,
    // and that's the only branch we exercise here.
    let original = at_profile_data
        .into_at_profile()
        .expect("AtProfile variant");
    let clone = original.clone();
    drop(original);

    // Resume from the clone.
    let out = tail_pipeline
        .run(PipelineData::AtProfile(clone), &mut ctx)
        .await
        .expect("tail pipeline runs");
    let rendered = out.into_rendered_output().expect("RenderedOutput");
    rendered.content
}

#[tokio::test]
async fn pipeline_at_profile_to_end_produces_expected_html() {
    let html_full = render_full(FIXTURE_QMD).await;
    let html_clone = render_clone_and_resume(FIXTURE_QMD).await;

    assert_eq!(
        html_full.len(),
        html_clone.len(),
        "clone-and-resume HTML differs in length ({} vs {})",
        html_full.len(),
        html_clone.len()
    );
    assert_eq!(
        html_full, html_clone,
        "clone-and-resume must produce byte-identical HTML to end-to-end"
    );
}

// ---------------------------------------------------------------------------
// Safety-net: the stages themselves are constructible and name-stable.
// ---------------------------------------------------------------------------

#[test]
fn stages_report_stable_names() {
    assert_eq!(DocumentProfileStage::new().name(), "document-profile");
    assert_eq!(UnwrapProfileStage::new().name(), "unwrap-profile");
}

// ---------------------------------------------------------------------------
// bd-xfwx: IncludeExpansionStage must run BEFORE DocumentProfileStage so
// statically-knowable content declared via `{{< include child.qmd >}}`
// (headings, code blocks, crossref targets, …) appears in the profile
// consumed by downstream project features (sidebars, cross-doc links,
// incremental rebuild cache, eventual freeze).
//
// Plan: claude-notes/plans/2026-04-24-include-expansion-merge.md
// ---------------------------------------------------------------------------

/// Variant of [`run_head_pipeline`] that lets the caller control the
/// project directory and parent-document path. Needed for include
/// tests, where the parent's location on disk determines where
/// `{{< include child.qmd >}}` resolves relative paths against.
async fn run_head_pipeline_in_dir(
    project_dir: &std::path::Path,
    parent_path: &std::path::Path,
    content: &[u8],
) -> quarto_core::stage::DocumentAtProfile {
    let full = build_html_pipeline_stages();
    let checkpoint =
        position_of(&full, "document-profile").expect("document-profile stage present in pipeline");
    let head: Vec<Box<dyn PipelineStage>> = full.into_iter().take(checkpoint + 1).collect();
    let pipeline = Pipeline::new(head).expect("head pipeline valid");

    let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> =
        Arc::new(quarto_system_runtime::NativeRuntime::new());
    let project = ProjectContext {
        dir: project_dir.to_path_buf(),
        config: ProjectConfig::default(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path(parent_path)],
        output_dir: project_dir.to_path_buf(),
    };
    let doc = DocumentInfo::from_path(parent_path);
    let format = Format::html();
    let mut ctx = StageContext::new(runtime, format, project, doc).expect("stage context");
    let input = PipelineData::LoadedSource(LoadedSource::new(
        parent_path.to_path_buf(),
        content.to_vec(),
    ));

    let out = pipeline.run(input, &mut ctx).await.expect("head pipeline");
    assert_eq!(out.kind(), PipelineDataKind::AtProfile);
    out.into_at_profile().unwrap()
}

/// Walk every entry in an outline (and its nested children) and collect
/// their titles, so we can check for the presence of a specific heading
/// without assuming tree shape.
fn collect_outline_titles(outline: &[pampa::toc::TocEntry]) -> Vec<String> {
    fn walk(entry: &pampa::toc::TocEntry, out: &mut Vec<String>) {
        out.push(entry.title.clone());
        for child in &entry.children {
            walk(child, out);
        }
    }
    let mut titles = Vec::new();
    for entry in outline {
        walk(entry, &mut titles);
    }
    titles
}

#[tokio::test]
async fn profile_sees_heading_from_included_file() {
    // A parent document whose only content is `{{< include child.qmd >}}`.
    // For the profile to reflect the included heading, IncludeExpansion
    // must run before the DocumentProfile checkpoint.
    let temp = tempfile::TempDir::new().expect("tempdir");
    let project_dir = temp
        .path()
        .canonicalize()
        .unwrap_or_else(|_| temp.path().to_path_buf());

    let child_path = project_dir.join("child.qmd");
    std::fs::write(&child_path, "## Child Heading\n\nChild body.\n").expect("write child");

    let parent_path = project_dir.join("parent.qmd");
    let parent_content: &[u8] = b"---\ntitle: Parent\n---\n\n{{< include child.qmd >}}\n";

    let bundle = run_head_pipeline_in_dir(&project_dir, &parent_path, parent_content).await;

    let titles = collect_outline_titles(&bundle.profile.outline);
    assert!(
        titles.iter().any(|t| t == "Child Heading"),
        "DocumentProfile.outline must include the heading `## Child Heading` \
         from the included file (bd-xfwx); got outline titles: {titles:?}"
    );
}

#[test]
fn include_expansion_precedes_document_profile() {
    // Structural guard against future refactors silently reordering the
    // two stages. See bd-xfwx and the plan at
    // claude-notes/plans/2026-04-24-include-expansion-merge.md.
    let stages = build_html_pipeline_stages();
    let names: Vec<&str> = stages.iter().map(|s| s.name()).collect();

    let include_pos = names.iter().position(|n| *n == "include-expansion").expect(
        "include-expansion stage must be present in the HTML pipeline — \
         bd-xfwx requires IncludeExpansion to run before DocumentProfile \
         so statically-knowable content declared via `{{< include ... >}}` \
         is visible in the profile",
    );
    let profile_pos = names
        .iter()
        .position(|n| *n == "document-profile")
        .expect("document-profile stage must be present in the HTML pipeline");
    assert!(
        include_pos < profile_pos,
        "include-expansion (at {include_pos}) must come strictly before \
         document-profile (at {profile_pos}) — otherwise the profile is \
         computed from the pre-include AST and cross-document features \
         (sidebars, nav, incremental rebuilds) see inconsistent state"
    );
}
