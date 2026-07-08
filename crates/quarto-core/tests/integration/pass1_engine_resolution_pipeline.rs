/*
 * tests/integration/pass1_engine_resolution_pipeline.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Plan 6 Phase 5 — DocumentProfile.engine_resolution stamp, end to end
 * over a real discovered project (registry + tabled_engines wired
 * exactly as a real render would build them).
 */

//! Drives a three-document project through the head pipeline (up to and
//! including `DocumentProfileStage`) with the claims-less `legacy-python`
//! fixture extension registered, and asserts the stamped
//! `DocumentProfile.engine_resolution`:
//!
//! - doc A (markdown-only) lifts via P2 (empty language scan).
//! - doc B (its own `engine:`-entry claim-table sugar) lifts via P4.
//! - doc C (a computational cell, no table anywhere) falls through.
//!
//! Then the project variant: the SAME claim table added to `_quarto.yml`'s
//! `engines:` key lifts doc C too.
//!
//! These tests deliberately never run Pass-2 / engine execution — only
//! `resolve_engines_pass1` (the no-load predicate) is ever consulted, so
//! the fixture's stub `.js` (which throws if loaded) is never invoked.
//! The orchestrator-level warning-gate binding
//! (`fell_through > 0` vs `engines_needing_load` non-empty) is tested at
//! the crate-internal level in `project::orchestrator::tests`, where the
//! private `pass_one`/`pass1_engine_resolution_warning` are reachable —
//! this file only has access to `quarto-core`'s public surface.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use quarto_core::document_profile::ProfileEngineResolution;
use quarto_core::format::Format;
use quarto_core::pipeline::build_html_pipeline_stages;
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::stage::{DocumentAtProfile, LoadedSource, Pipeline, PipelineData, StageContext};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};
use tempfile::TempDir;

/// Absolute path to a committed fixture extension directory
/// (`crates/quarto-core/tests/fixtures/extensions/<name>`). Mirrors the
/// helper in `echo_engine_e2e.rs`.
fn fixture_ext_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/extensions")
        .join(name)
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to);
        } else {
            std::fs::copy(&from, &to).unwrap();
        }
    }
}

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// Run the head pipeline (every stage through `document-profile`) for one
/// document inside an ALREADY-DISCOVERED project, so the real
/// `ProjectContext::discover`-built registry and `tabled_engines` are
/// exercised exactly as a real render would build them. Never advances
/// past the profile checkpoint — Pass-2 / engine execution never runs.
async fn run_head_pipeline_for_doc(
    project: &ProjectContext,
    runtime: &Arc<dyn SystemRuntime>,
    doc_path: &Path,
) -> DocumentAtProfile {
    let full = build_html_pipeline_stages();
    let checkpoint = full
        .iter()
        .position(|s| s.name() == "document-profile")
        .expect("document-profile stage present in pipeline");
    let head = full.into_iter().take(checkpoint + 1).collect();
    let pipeline = Pipeline::new(head).expect("head pipeline valid");

    let content = std::fs::read(doc_path).expect("read fixture doc");
    let doc = DocumentInfo::from_path(doc_path);
    let format = Format::html();
    let mut ctx =
        StageContext::new(runtime.clone(), format, project.clone(), doc).expect("stage context");
    let input = PipelineData::LoadedSource(LoadedSource::new(doc_path.to_path_buf(), content));

    let out = pipeline.run(input, &mut ctx).await.expect("head pipeline");
    out.into_at_profile().expect("AtProfile variant")
}

const DOC_A: &str = "---\ntitle: Markdown only\n---\n\nJust prose, no computational cells.\n";

/// Doc B: its OWN frontmatter `engine:`-entry claim-table sugar covers
/// `legacy-python` for `python` — lifts via P4, doc-layer surface (the
/// project-level `engines:` variant is exercised separately).
fn doc_b_content(fixture_engine: &str) -> String {
    format!(
        "---\ntitle: Doc-level claim table\nengine:\n  - {fixture_engine}:\n      claims: [python]\n---\n\n```{{python}}\nprint('hi')\n```\n"
    )
}

const DOC_C: &str = "---\ntitle: No table anywhere\n---\n\n```{python}\nprint('hi')\n```\n";

/// Write the three-doc project (no project-level `engines:` table) with
/// the claims-less `legacy-python` extension installed under
/// `_extensions/`.
fn setup_no_table_project() -> TempDir {
    let tmp = TempDir::new().unwrap();
    copy_dir(
        &fixture_ext_dir("legacy-python"),
        &tmp.path().join("_extensions/legacy-python"),
    );
    write_file(
        &tmp.path().join("_quarto.yml"),
        "project:\n  type: default\n",
    );
    write_file(&tmp.path().join("a.qmd"), DOC_A);
    write_file(&tmp.path().join("b.qmd"), &doc_b_content("legacy-python"));
    write_file(&tmp.path().join("c.qmd"), DOC_C);
    tmp
}

#[tokio::test]
async fn doc_a_markdown_only_lifts_with_empty_resolution() {
    let tmp = setup_no_table_project();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(tmp.path(), runtime.as_ref()).expect("discover project");
    assert!(
        project.registry.has_engine("legacy-python"),
        "the claims-less fixture engine must be registered"
    );

    let a = run_head_pipeline_for_doc(&project, &runtime, &tmp.path().join("a.qmd")).await;
    assert_eq!(
        a.profile.engine_resolution,
        Some(ProfileEngineResolution {
            sequence: vec![],
            ownership: vec![],
        }),
        "a markdown-only doc must lift via P2 with empty sequence/ownership"
    );
}

#[tokio::test]
async fn doc_b_doc_level_claim_table_lifts_via_p4() {
    let tmp = setup_no_table_project();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(tmp.path(), runtime.as_ref()).expect("discover project");

    let b = run_head_pipeline_for_doc(&project, &runtime, &tmp.path().join("b.qmd")).await;
    let res = b
        .profile
        .engine_resolution
        .clone()
        .expect("doc B's own engine:-entry claim table must lift it via P4");
    assert_eq!(
        res.sequence,
        vec!["legacy-python".to_string()],
        "sequence must contain exactly the tabled engine"
    );
    assert_eq!(
        res.ownership,
        vec![("python".to_string(), "legacy-python".to_string())],
        "ownership must route python to the tabled engine, from the table \
         alone — legacy-python's own try_claims_language is never consulted"
    );
}

#[tokio::test]
async fn doc_c_no_table_falls_through() {
    let tmp = setup_no_table_project();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(tmp.path(), runtime.as_ref()).expect("discover project");
    assert!(
        project.tabled_engines.is_empty(),
        "no project-level engines: table exists in this variant"
    );

    let c = run_head_pipeline_for_doc(&project, &runtime, &tmp.path().join("c.qmd")).await;
    assert_eq!(
        c.profile.engine_resolution, None,
        "doc C has a computational cell but no claim table anywhere (doc \
         or project) — legacy-python is registered, untabled, and \
         claims-less, so it must fall through to Pass-2"
    );
}

/// Project variant: the SAME claim table lives in `_quarto.yml`'s
/// `engines:` key instead of doc B's frontmatter — now ALL THREE
/// documents lift, including doc C (project-level tables cover every
/// document, unlike doc-layer sugar).
#[tokio::test]
async fn project_level_engines_table_lifts_all_three_docs() {
    let tmp = TempDir::new().unwrap();
    copy_dir(
        &fixture_ext_dir("legacy-python"),
        &tmp.path().join("_extensions/legacy-python"),
    );
    write_file(
        &tmp.path().join("_quarto.yml"),
        "project:\n  type: default\nengines:\n  - legacy-python:\n      claims: [python]\n",
    );
    write_file(&tmp.path().join("a.qmd"), DOC_A);
    // Doc B and C both use the bare cell here — the project table alone
    // must cover them (no doc-level engine: sugar needed).
    write_file(&tmp.path().join("b.qmd"), DOC_C);
    write_file(&tmp.path().join("c.qmd"), DOC_C);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(tmp.path(), runtime.as_ref()).expect("discover project");
    assert!(
        project.tabled_engines.contains("legacy-python"),
        "the project-level engines: table must be reflected in tabled_engines"
    );

    for name in ["a.qmd", "b.qmd", "c.qmd"] {
        let bundle = run_head_pipeline_for_doc(&project, &runtime, &tmp.path().join(name)).await;
        assert!(
            name == "a.qmd" || bundle.profile.engine_resolution.is_some(),
            "{name} must lift once the project-level engines: table covers \
             legacy-python; got {:?}",
            bundle.profile.engine_resolution
        );
    }
}

/// Serde round-trip of the reduced `ProfileEngineResolution` shape as
/// actually produced by the pipeline (not a hand-built fixture value) —
/// closes the loop between this file's Pass-1 assertions and the
/// crate-root round-trip tests in `document_profile.rs`.
#[tokio::test]
async fn pipeline_stamped_resolution_round_trips_through_profile_json() {
    let tmp = setup_no_table_project();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(tmp.path(), runtime.as_ref()).expect("discover project");

    let b = run_head_pipeline_for_doc(&project, &runtime, &tmp.path().join("b.qmd")).await;
    let json = b.profile.to_json().expect("serialize");
    let restored =
        quarto_core::document_profile::DocumentProfile::from_json(&json).expect("deserialize");
    assert_eq!(restored.engine_resolution, b.profile.engine_resolution);
}
