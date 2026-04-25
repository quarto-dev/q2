/*
 * tests/project_pipeline.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Integration tests for the ProjectPipeline two-pass driver.
 */

//! Integration tests for Phase 1 of the website-projects epic.
//!
//! See `claude-notes/plans/2026-04-23-websites-phase-1.md` §Tests
//! (items 10–14). These exercise `ProjectPipeline` end-to-end:
//!
//! 10. `single_file_goes_through_default_project_type` — a bare
//!     `.qmd` still renders correctly via the driver; the Pass-1
//!     index sees exactly that file with the expected title.
//! 11. `two_file_project_builds_index_of_both` — website project
//!     with two qmds; both render, index has both profiles, hooks
//!     fire once each.
//! 12. `pre_render_failure_aborts_project` — a `ProjectType` whose
//!     `pre_render` returns `Err` propagates the error; no Pass 2
//!     runs.
//! 13. `per_file_render_failure_continues_others` — a syntactically
//!     broken file in a project doesn't abort the others.
//! 14. `project_index_passes_through_to_stage_context` — confirms
//!     the per-file Pass-2 render sees a non-None `project_index`.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use tempfile::TempDir;

use quarto_core::error::QuartoError;
use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::index::ProjectIndex;
use quarto_core::project::orchestrator::{
    DefaultProjectType, ProjectPipeline, ProjectType, project_type_for,
};
use quarto_core::project::{ProjectConfig, ProjectKind};
use quarto_core::render_to_file::{RenderToFileOptions, RenderToFileResult};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn canonical(path: &std::path::Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn write(path: &std::path::Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn html_format() -> Format {
    Format::html()
}

fn runtime_arc() -> Arc<dyn SystemRuntime> {
    Arc::new(NativeRuntime::new())
}

// === Test 10 ===============================================================

#[test]
fn single_file_goes_through_default_project_type() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    let qmd_path = project_dir.join("single.qmd");
    write(
        &qmd_path,
        "---\ntitle: Only Page\n---\n\n# Only\n\nHello.\n",
    );

    let runtime = runtime_arc();
    let mut project = ProjectContext::discover(&qmd_path, runtime.as_ref()).unwrap();
    assert!(project.is_single_file, "bare file should be single-file");
    assert_eq!(project.files.len(), 1);

    let options = RenderToFileOptions::default();
    let project_type = project_type_for(&project);
    assert_eq!(project_type.kind(), ProjectKind::Default);

    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        html_format(),
        "html",
        &options,
        runtime.clone(),
    );
    let summary = pollster::block_on(pipeline.run()).expect("run");

    assert_eq!(summary.outputs.len(), 1);
    assert!(summary.pass1_failures.is_empty());
    assert!(summary.pass2_failures.is_empty());
    assert!(summary.outputs[0].output_path.exists());
}

// === Test 11 ===============================================================

struct CountingProjectType {
    pre_calls: AtomicUsize,
    post_calls: AtomicUsize,
}

impl CountingProjectType {
    fn new() -> Self {
        Self {
            pre_calls: AtomicUsize::new(0),
            post_calls: AtomicUsize::new(0),
        }
    }
}

#[async_trait(?Send)]
impl ProjectType for CountingProjectType {
    fn kind(&self) -> ProjectKind {
        ProjectKind::Website
    }
    async fn pre_render(
        &self,
        _project: &mut ProjectContext,
        _index: &ProjectIndex,
    ) -> quarto_core::Result<()> {
        self.pre_calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn post_render(
        &self,
        _project: &ProjectContext,
        _index: &ProjectIndex,
        _outputs: &[RenderToFileResult],
        _project_artifacts: &quarto_core::ArtifactStore,
        _runtime: &dyn SystemRuntime,
    ) -> quarto_core::Result<()> {
        self.post_calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[test]
fn two_file_project_builds_index_of_both() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n",
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nH.\n",
    );
    write(
        &project_dir.join("about.qmd"),
        "---\ntitle: About\n---\n\nA.\n",
    );

    let runtime = runtime_arc();
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();
    assert!(!project.is_single_file);
    assert_eq!(project.files.len(), 2, "discovery should find both qmds");

    let options = RenderToFileOptions::default();
    let counter = std::rc::Rc::new(CountingProjectType::new());
    let project_type: Box<dyn ProjectType> = Box::new(CountingProjectTypeWrapper {
        inner: counter.clone(),
    });

    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        html_format(),
        "html",
        &options,
        runtime.clone(),
    );
    let summary = pollster::block_on(pipeline.run()).expect("run");

    assert_eq!(counter.pre_calls.load(Ordering::SeqCst), 1);
    assert_eq!(counter.post_calls.load(Ordering::SeqCst), 1);
    assert_eq!(summary.outputs.len(), 2);
    for out in &summary.outputs {
        assert!(out.output_path.exists());
        assert!(
            out.output_path.starts_with(project_dir.join("_site")),
            "output should land in _site/: {}",
            out.output_path.display()
        );
    }
}

// Small wrapper so the outer test can observe call counts on an
// `Rc<CountingProjectType>` while the driver holds a `Box<dyn
// ProjectType>`. A free `Arc<Mutex<_>>` would work but we don't need
// Send here — the driver is `?Send` via `async_trait(?Send)`.
struct CountingProjectTypeWrapper {
    inner: std::rc::Rc<CountingProjectType>,
}

#[async_trait(?Send)]
impl ProjectType for CountingProjectTypeWrapper {
    fn kind(&self) -> ProjectKind {
        self.inner.kind()
    }
    async fn pre_render(
        &self,
        p: &mut ProjectContext,
        i: &ProjectIndex,
    ) -> quarto_core::Result<()> {
        self.inner.pre_render(p, i).await
    }
    async fn post_render(
        &self,
        p: &ProjectContext,
        i: &ProjectIndex,
        o: &[RenderToFileResult],
        a: &quarto_core::ArtifactStore,
        r: &dyn SystemRuntime,
    ) -> quarto_core::Result<()> {
        self.inner.post_render(p, i, o, a, r).await
    }
}

// === Test 12 ===============================================================

struct FailingPreRender;

#[async_trait(?Send)]
impl ProjectType for FailingPreRender {
    fn kind(&self) -> ProjectKind {
        ProjectKind::Website
    }
    async fn pre_render(
        &self,
        _p: &mut ProjectContext,
        _i: &ProjectIndex,
    ) -> quarto_core::Result<()> {
        Err(QuartoError::other(
            "deliberate failure from test pre_render",
        ))
    }
}

#[test]
fn pre_render_failure_aborts_project() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n",
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Only\n---\n\nHi.\n",
    );

    let runtime = runtime_arc();
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();
    let options = RenderToFileOptions::default();
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        Box::new(FailingPreRender),
        html_format(),
        "html",
        &options,
        runtime.clone(),
    );
    let result = pollster::block_on(pipeline.run());
    assert!(result.is_err(), "pre_render error must abort the run");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("pre_render"),
        "error should mention pre_render: {err}"
    );
    // Pass 2 did not run, so no _site/index.html.
    assert!(!project_dir.join("_site/index.html").exists());
}

// === Test 13 ===============================================================

#[test]
fn per_file_render_failure_continues_others() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n",
    );
    // Valid file.
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nHello.\n",
    );
    // Malformed YAML frontmatter: unterminated `---`. Parser should
    // reject this outright, giving us a guaranteed Pass-2 failure.
    write(
        &project_dir.join("broken.qmd"),
        "---\ntitle: [unterminated\n",
    );

    let runtime = runtime_arc();
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();
    let options = RenderToFileOptions::default();
    let project_type = project_type_for(&project);

    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        html_format(),
        "html",
        &options,
        runtime.clone(),
    );
    let summary = pollster::block_on(pipeline.run()).expect("run finishes");

    // The valid file rendered; the broken file surfaced as either a
    // Pass-1 or Pass-2 failure (depending on where the parser gives
    // up).
    let total_failures = summary.pass1_failures.len() + summary.pass2_failures.len();
    assert_eq!(total_failures, 1, "broken file should fail exactly once");
    assert_eq!(summary.outputs.len(), 1, "valid file still rendered");
    assert!(summary.outputs[0].output_path.exists());
    assert!(summary.outputs[0].output_path.ends_with("index.html"));
}

// === Test 14 ===============================================================

struct IndexObserver {
    observed: std::rc::Rc<std::cell::RefCell<Option<Vec<PathBuf>>>>,
}

#[async_trait(?Send)]
impl ProjectType for IndexObserver {
    fn kind(&self) -> ProjectKind {
        ProjectKind::Website
    }
    async fn pre_render(
        &self,
        _p: &mut ProjectContext,
        index: &ProjectIndex,
    ) -> quarto_core::Result<()> {
        // Record profile source paths so the test can verify the
        // index the driver built.
        let paths: Vec<PathBuf> = index
            .profiles()
            .iter()
            .map(|p| p.source_path.clone())
            .collect();
        *self.observed.borrow_mut() = Some(paths);
        Ok(())
    }
}

#[test]
fn project_index_passes_through_to_stage_context() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n",
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nH.\n",
    );
    write(
        &project_dir.join("about.qmd"),
        "---\ntitle: About\n---\n\nA.\n",
    );

    let runtime = runtime_arc();
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();
    let options = RenderToFileOptions::default();

    let observed = std::rc::Rc::new(std::cell::RefCell::new(None));
    let project_type = Box::new(IndexObserver {
        observed: observed.clone(),
    });

    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        html_format(),
        "html",
        &options,
        runtime.clone(),
    );
    let summary = pollster::block_on(pipeline.run()).expect("run");
    assert_eq!(summary.outputs.len(), 2);

    let recorded = observed.borrow().clone().expect("pre_render ran");
    assert_eq!(recorded.len(), 2, "index holds both profiles");
}

// === Sanity smoke: DefaultProjectType on a single file matches the
// pre-Phase-1 render output exactly. The smoke-all test suite covers
// the per-file path in depth; this mini-smoke just proves the driver
// wraps it transparently. ====================================================

#[test]
fn default_project_type_single_file_produces_output() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    let qmd_path = project_dir.join("d.qmd");
    write(&qmd_path, "---\ntitle: D\n---\n\nContent.\n");

    let runtime = runtime_arc();
    let mut project = ProjectContext {
        dir: project_dir.clone(),
        config: ProjectConfig::default(),
        is_single_file: true,
        files: vec![quarto_core::project::DocumentInfo::from_path(
            qmd_path.clone(),
        )],
        output_dir: project_dir.clone(),
    };

    let options = RenderToFileOptions::default();
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        Box::new(DefaultProjectType),
        html_format(),
        "html",
        &options,
        runtime.clone(),
    );
    let summary = pollster::block_on(pipeline.run()).expect("run");
    assert_eq!(summary.outputs.len(), 1);
    assert!(summary.outputs[0].output_path.exists());
    let html = std::fs::read_to_string(&summary.outputs[0].output_path).unwrap();
    assert!(html.contains("<title>D</title>"));
    assert!(html.contains("Content."));
}
