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

        ..Default::default()
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

    // After Phase-8 sub-phase 8.0d, link-resolution sits between
    // document-profile and unwrap-profile. Both stages take/produce
    // AtProfile, so downstream stages still receive DocumentAst from
    // unwrap-profile as before.
    let p = names
        .iter()
        .position(|n| *n == "document-profile")
        .expect("document-profile present");
    assert_eq!(
        names.get(p + 1).copied(),
        Some("link-resolution"),
        "link-resolution must follow document-profile"
    );
    assert_eq!(
        names.get(p + 2).copied(),
        Some("unwrap-profile"),
        "unwrap-profile must follow link-resolution"
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
    // Phase-8 sub-phase 8.0d inserts `link-resolution` between
    // `document-profile` and `unwrap-profile`. Both relative orders
    // (document-profile → link-resolution → unwrap-profile) are
    // invariants other consumers rely on.
    assert_eq!(names.get(p + 1).copied(), Some("link-resolution"));
    assert_eq!(names.get(p + 2).copied(), Some("unwrap-profile"));

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

        ..Default::default()
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

#[tokio::test]
async fn profile_records_direct_include_in_includes_field() {
    // bd-r82e: a parent that pulls in `{{< include child.qmd >}}`
    // should land an `IncludeEntry` in `profile.includes` so the
    // Phase-8 cache key can invalidate when the child's content
    // changes.
    let temp = tempfile::TempDir::new().expect("tempdir");
    let project_dir = temp
        .path()
        .canonicalize()
        .unwrap_or_else(|_| temp.path().to_path_buf());

    let child_path = project_dir.join("child.qmd");
    let child_bytes: &[u8] = b"## Child Heading\n\nChild body.\n";
    std::fs::write(&child_path, child_bytes).expect("write child");

    let parent_path = project_dir.join("parent.qmd");
    let parent_content: &[u8] = b"---\ntitle: Parent\n---\n\n{{< include child.qmd >}}\n";

    let bundle = run_head_pipeline_in_dir(&project_dir, &parent_path, parent_content).await;

    assert_eq!(
        bundle.profile.includes.len(),
        1,
        "profile.includes should record exactly one direct include; got {:?}",
        bundle.profile.includes
    );
    let entry = &bundle.profile.includes[0];
    assert!(
        entry.path.ends_with("child.qmd"),
        "IncludeEntry.path should reference the resolved child file; got {:?}",
        entry.path
    );
    assert_eq!(
        entry.content_hash,
        quarto_core::document_profile::IncludeEntry::hash_bytes(child_bytes),
        "IncludeEntry.content_hash must match SHA-256(child bytes) — \
         this is what Phase-8 cache invalidation keys on (bd-r82e)"
    );
}

#[tokio::test]
async fn profile_records_transitive_includes() {
    // grandchild.qmd → child.qmd → parent.qmd. The profile of
    // parent.qmd must record both child *and* grandchild so a
    // change to the grandchild correctly invalidates parent's
    // cache entry.
    let temp = tempfile::TempDir::new().expect("tempdir");
    let project_dir = temp
        .path()
        .canonicalize()
        .unwrap_or_else(|_| temp.path().to_path_buf());

    let grandchild_path = project_dir.join("grandchild.qmd");
    std::fs::write(&grandchild_path, "## Grandchild Heading\n").expect("write grandchild");

    let child_path = project_dir.join("child.qmd");
    std::fs::write(&child_path, "{{< include grandchild.qmd >}}\n").expect("write child");

    let parent_path = project_dir.join("parent.qmd");
    let parent_content: &[u8] = b"---\ntitle: Parent\n---\n\n{{< include child.qmd >}}\n";

    let bundle = run_head_pipeline_in_dir(&project_dir, &parent_path, parent_content).await;

    let recorded_names: Vec<String> = bundle
        .profile
        .includes
        .iter()
        .filter_map(|e| {
            e.path
                .file_name()
                .and_then(|n| n.to_str())
                .map(String::from)
        })
        .collect();
    assert!(
        recorded_names.iter().any(|n| n == "child.qmd"),
        "parent profile should record the direct include child.qmd; got {recorded_names:?}"
    );
    assert!(
        recorded_names.iter().any(|n| n == "grandchild.qmd"),
        "parent profile should record the transitive include grandchild.qmd; \
         got {recorded_names:?}"
    );
}

// ---------------------------------------------------------------------------
// L0 — listings epic (`bd-n8a4`): pipeline-level extraction of
// `listing-item:` frontmatter. The full pipeline (MetadataMergeStage,
// IncludeExpansionStage, DocumentProfileStage) runs end-to-end here;
// the assertion is that author values land on
// `profile.listing_item.*` after the checkpoint.
// ---------------------------------------------------------------------------

// NOTE: Do not use `\<newline>` line continuations inside this fixture
// — they strip leading whitespace, which silently flattens the YAML
// nesting into a malformed listing-item: null with sibling keys.
const LISTING_ITEM_FIXTURE_QMD: &[u8] = b"---
title: Outer
listing-item:
  title: Listing title
  description: Listing desc
  reading-time-minutes: 12
  categories: [a, b]
  extra:
    status: draft
---

Body paragraph.
";

#[tokio::test]
async fn pipeline_extracts_listing_item_from_frontmatter() {
    let bundle = run_head_pipeline(LISTING_ITEM_FIXTURE_QMD).await;
    let li = &bundle.profile.listing_item;
    assert_eq!(li.title.as_deref(), Some("Listing title"));
    assert_eq!(li.description.as_deref(), Some("Listing desc"));
    assert_eq!(li.reading_time_minutes, Some(12));
    assert_eq!(li.categories, vec!["a".to_string(), "b".to_string()]);
    let status = li
        .extra
        .get("status")
        .expect("status entry present in extra");
    assert_eq!(status.as_plain_text().as_deref(), Some("draft"));
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

// ---------------------------------------------------------------------------
// L1 listings (`bd-izqh`): ListingItemInfoStage runs between
// IncludeExpansionStage and DocumentProfileStage and auto-fills
// `meta.listing-item.*` from the post-include AST. The extracted
// `profile.listing_item` then carries those values for the
// listings consumer (L3).
//
// Plan: claude-notes/plans/2026-05-05-listings-L1-autofill-stage.md
// ---------------------------------------------------------------------------

const L1_FIXTURE_QMD: &[u8] = b"---\n\
title: Hello\n\
---\n\
\n\
This is the first paragraph that L1 will harvest as the description.\n\
\n\
![](figs/cover.png)\n";

#[tokio::test]
async fn pipeline_listing_item_autofill_end_to_end() {
    // No `listing-item:` key in frontmatter; L1 derives every field
    // from the AST and mtime, then DocumentProfileStage extracts a
    // populated ListingItemInfo into the profile.
    let bundle = run_head_pipeline(L1_FIXTURE_QMD).await;
    let li = &bundle.profile.listing_item;

    assert_eq!(
        li.description.as_deref(),
        Some("This is the first paragraph that L1 will harvest as the description.")
    );
    assert_eq!(li.image.as_deref(), Some("figs/cover.png"));
    // 12 words in the paragraph (image alt-text excluded; image is
    // its own block-level paragraph in the AST but contributes no
    // word-bearing prose).
    assert_eq!(li.word_count, Some(12));
    // 12 / 200 wpm → ceiling 1 minute.
    assert_eq!(li.reading_time_minutes, Some(1));
}

#[tokio::test]
async fn pipeline_listing_item_author_overrides_winner() {
    // Author sets `listing-item.description` explicitly; L1 must not
    // overwrite. The body paragraph is what the auto-fill *would*
    // produce — proving the override (not the fallback) flows
    // through.
    // NOTE: must NOT use Rust `\<newline>` line continuations inside
    // YAML fixtures. The continuation eats leading whitespace, which
    // collapses two-space-indented sub-keys to top-level keys and the
    // YAML parser then sees `listing-item:` (null) and a sibling
    // `description:` instead of a nested map. Risk handoff item 8 in
    // the L1 sub-plan.
    let fixture: &[u8] = b"---\ntitle: Hello\nlisting-item:\n  description: Author wrote this\n---\n\nAuto-fill would have used this paragraph instead.\n";
    let bundle = run_head_pipeline(fixture).await;
    let li = &bundle.profile.listing_item;
    assert_eq!(li.description.as_deref(), Some("Author wrote this"));
}

#[tokio::test]
async fn pipeline_clone_and_resume_listing_item_visible_in_profile() {
    // Companion to `pipeline_at_profile_to_end_produces_expected_html`.
    // L1's auto-fills land in `meta.listing-item.*` *before* the
    // checkpoint clone, so resume-from-clone tests still see the
    // populated `profile.listing_item`. The byte-identical render
    // assertion is the existing test's job; this one locks in the
    // checkpoint-side observation.
    let bundle = run_head_pipeline(FIXTURE_QMD).await;
    let li = &bundle.profile.listing_item;

    // FIXTURE_QMD has paragraphs ("Hello.", "More hello.", "Goodbye.")
    // separated by headings. L1's first non-empty paragraph wins.
    assert_eq!(li.description.as_deref(), Some("Hello."));
    // Word count: "Hello.", "More hello.", "Goodbye." plus heading
    // text ("Section one", "Subsection", "Section two") = 9 words.
    assert_eq!(li.word_count, Some(9));
    assert_eq!(li.reading_time_minutes, Some(1));
}
