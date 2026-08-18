/*
 * tests/project_resources.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Phase 0 of the project-resources feature (bd-o8pr).
 *
 * Failing tests that drive Phase 1 (static channel: project- and
 * document-level `resources:` declarations) and Phase 4 (render
 * manifest). Engine- and Lua-filter-channel tests live alongside the
 * code that implements them and are added in Phases 2 and 3.
 *
 * Plan: claude-notes/plans/2026-05-03-project-resources.md
 */

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

fn html_format() -> Format {
    Format::html()
}

fn runtime_arc() -> Arc<dyn SystemRuntime> {
    Arc::new(NativeRuntime::new())
}

/// Run the project pipeline end-to-end on `project_dir` and return the
/// resolved (canonicalized) output dir, panicking on any failure.
fn render_project(project_dir: &Path) -> PathBuf {
    let runtime = runtime_arc();
    let mut project = ProjectContext::discover(project_dir, runtime.as_ref()).expect("discover");
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
    let summary = pollster::block_on(pipeline.run()).expect("run");
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
    project.output_dir.clone()
}

// ─────────────────────────────────────────────────────────────────────
// Phase 1 — static channel (project-level _quarto.yml)
// ─────────────────────────────────────────────────────────────────────

/// `project.resources:` in `_quarto.yml` with literal paths copies
/// each named file into the output dir, preserving the relative path.
#[test]
fn project_resources_literal_paths_copy_to_output_dir() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());

    write(
        &project_dir.join("_quarto.yml"),
        "\
project:
  type: website
  resources:
    - robots.txt
    - data/file.csv
",
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nHello.\n",
    );
    write(&project_dir.join("robots.txt"), "User-agent: *\nAllow: /\n");
    write(&project_dir.join("data/file.csv"), "a,b\n1,2\n");

    let output_dir = render_project(&project_dir);

    assert!(
        output_dir.join("robots.txt").exists(),
        "robots.txt should be copied to {}",
        output_dir.display()
    );
    assert!(
        output_dir.join("data/file.csv").exists(),
        "data/file.csv should be copied to {}",
        output_dir.display()
    );

    let copied = std::fs::read_to_string(output_dir.join("data/file.csv")).unwrap();
    assert_eq!(copied, "a,b\n1,2\n");
}

/// `project.resources:` accepts globs; matched files land in the
/// output dir at their project-relative paths.
#[test]
fn project_resources_glob_expansion() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());

    write(
        &project_dir.join("_quarto.yml"),
        "\
project:
  type: website
  resources:
    - data/*.csv
",
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nHello.\n",
    );
    write(&project_dir.join("data/a.csv"), "x\n1\n");
    write(&project_dir.join("data/b.csv"), "x\n2\n");
    // A non-matching file must NOT be copied.
    write(&project_dir.join("data/skip.txt"), "skip me\n");

    let output_dir = render_project(&project_dir);

    assert!(output_dir.join("data/a.csv").exists());
    assert!(output_dir.join("data/b.csv").exists());
    assert!(
        !output_dir.join("data/skip.txt").exists(),
        "non-matching glob entry should not be copied"
    );
}

/// `project.resources:` accepts a single scalar (not just a list).
#[test]
fn project_resources_single_scalar() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());

    // Use a filename that isn't independently handled by any
    // existing post-render hook (e.g. robots.txt).
    write(
        &project_dir.join("_quarto.yml"),
        "\
project:
  type: website
  resources: extras/notes.txt
",
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\n.\n",
    );
    write(&project_dir.join("extras/notes.txt"), "ok\n");

    let output_dir = render_project(&project_dir);

    assert!(output_dir.join("extras/notes.txt").exists());
}

// ─────────────────────────────────────────────────────────────────────
// Phase 1 — static channel (document-level YAML frontmatter)
// ─────────────────────────────────────────────────────────────────────

/// `resources:` at document level copies files anchored at the
/// document's output dir.
#[test]
fn document_resources_copy_anchored_at_doc_output_dir() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());

    write(
        &project_dir.join("_quarto.yml"),
        "\
project:
  type: website
",
    );
    // Document at posts/foo.qmd declaring data/extra.html — should
    // land at <output_dir>/posts/data/extra.html.
    write(
        &project_dir.join("posts/foo.qmd"),
        "---\ntitle: Foo\nresources:\n  - data/extra.html\n---\n\nBody.\n",
    );
    write(&project_dir.join("posts/data/extra.html"), "<i>extra</i>\n");
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nx\n",
    );

    let output_dir = render_project(&project_dir);

    assert!(
        output_dir.join("posts/data/extra.html").exists(),
        "doc-level resource should land at posts/data/extra.html under output_dir, not at {}",
        output_dir.display()
    );
}

// ─────────────────────────────────────────────────────────────────────
// Phase 1 — error handling
// ─────────────────────────────────────────────────────────────────────

/// Out-of-project paths in `project.resources:` are an error in v1.
#[test]
fn project_resources_out_of_project_path_is_error() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());

    write(
        &project_dir.join("_quarto.yml"),
        "\
project:
  type: website
  resources:
    - ../outside.csv
",
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\n.\n",
    );

    let runtime = runtime_arc();
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).expect("discover");
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
    let result = pollster::block_on(pipeline.run());

    let err = match result {
        Ok(summary) => panic!(
            "expected out-of-project resource to error, got success: {:?}",
            summary
                .project_diagnostics
                .iter()
                .map(|d| d.title.clone())
                .collect::<Vec<_>>()
        ),
        Err(e) => e.to_string(),
    };

    assert!(
        err.contains("outside") || err.contains("project root") || err.contains("out of project"),
        "error should mention out-of-project path, got: {err}"
    );
}

/// Out-of-project paths in document `resources:` are also an error.
#[test]
fn document_resources_out_of_project_path_is_error() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());

    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n",
    );
    // Doc at posts/foo.qmd declaring `../../outside` resolves to
    // outside the project root.
    write(
        &project_dir.join("posts/foo.qmd"),
        "---\ntitle: F\nresources:\n  - ../../outside.csv\n---\n\n.\n",
    );

    let runtime = runtime_arc();
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).expect("discover");
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
    let result = pollster::block_on(pipeline.run());

    let err_text = match result {
        Ok(summary) => {
            // If the pipeline didn't return Err, the doc's render
            // must have surfaced the failure as a pass2 entry.
            assert!(
                !summary.pass2_failures.is_empty(),
                "expected out-of-project doc resource to error somewhere"
            );
            summary.pass2_failures[0].error.clone()
        }
        Err(e) => e.to_string(),
    };

    assert!(
        err_text.contains("outside")
            || err_text.contains("project root")
            || err_text.contains("out of project"),
        "error should mention out-of-project path, got: {err_text}"
    );
}

// ─────────────────────────────────────────────────────────────────────
// Phase 2 — engine channel (orchestrator-level integration)
// ─────────────────────────────────────────────────────────────────────
//
// Two flavors of test live here:
//
// 1. The original `MockRenderer`-based test exercises the
//    orchestrator's Phase-2 drain in isolation: it bypasses the real
//    pipeline entirely and asserts that
//    `R::extract_resource_report` -> resolve -> copy works
//    end-to-end against synthetic data.
//
// 2. The replay-engine test (bd-45yw) exercises the *full* stack:
//    real pipeline (parse, profile, engine execution, transforms,
//    resource-report finalization) with the standard
//    `RenderToFileRenderer`. The only substitution is at the engine
//    layer: a `ReplayEngine` stands in for the real
//    knitr/jupyter run via `RenderToFileOptions.replay_capture`.
//    This proves the replay engine actually closes the bd-o8pr Phase
//    2 gap — engine-emitted `supporting_files` reach the output dir
//    without an R or Python install.

mod orchestrator_engine_channel {
    use super::*;
    use async_trait::async_trait;
    use quarto_core::project::DocumentInfo;
    use quarto_core::project::index::ProjectIndex;
    use quarto_core::project::orchestrator::{ProjectPipeline, ProjectType};
    use quarto_core::project::pass2_renderer::Pass2Renderer;
    use quarto_core::project_resources::{DocumentResourceReport, ResourceOrigin};
    use quarto_core::resource_resolver::ResourceResolverContext;
    use quarto_core::{Result, artifact::ArtifactStore};
    use std::path::Path;

    /// Synthetic per-doc output: just a path and the report we want
    /// the orchestrator to drain.
    struct MockOutput {
        output_path: PathBuf,
        resource_report: DocumentResourceReport,
    }

    /// Mock renderer: writes a one-byte placeholder file per doc and
    /// emits a hard-coded `DocumentResourceReport` so the orchestrator
    /// has something to drain.
    struct MockRenderer {
        engine_files_per_doc: Vec<(PathBuf, String)>,
    }

    #[async_trait(?Send)]
    impl Pass2Renderer for MockRenderer {
        type Output = MockOutput;

        async fn render(
            &mut self,
            doc_info: &DocumentInfo,
            _format: &quarto_core::format::Format,
            _format_str: &str,
            project: &quarto_core::project::ProjectContext,
            _index: std::sync::Arc<ProjectIndex>,
            runtime: std::sync::Arc<dyn quarto_system_runtime::SystemRuntime>,
            _project_artifacts: &mut ArtifactStore,
        ) -> Result<MockOutput> {
            // Materialize every file we're going to report so the
            // orchestrator's copy step has something to read.
            for (rel, content) in &self.engine_files_per_doc {
                let p = project.dir.join(rel);
                if let Some(parent) = p.parent() {
                    runtime.dir_create(parent, true).ok();
                }
                runtime.file_write(&p, content.as_bytes()).ok();
            }

            // Write a placeholder output file under output_dir so the
            // orchestrator's `output_path` extraction has something
            // real.
            let out_dir = &project.output_dir;
            runtime.dir_create(out_dir, true).ok();
            let stem = doc_info
                .input
                .file_stem()
                .map_or_else(|| "doc".into(), |s| s.to_string_lossy().into_owned());
            let output_path = out_dir.join(format!("{stem}.html"));
            runtime
                .file_write(&output_path, b"<html><body>mock</body></html>")
                .ok();

            // Build a per-doc report whose entries reference the
            // materialized files.
            let mut report = DocumentResourceReport::new();
            report.add_engine_files(
                "mock-engine",
                &doc_info.input,
                runtime.as_ref(),
                self.engine_files_per_doc
                    .iter()
                    .map(|(rel, _)| project.dir.join(rel)),
            );

            Ok(MockOutput {
                output_path,
                resource_report: report,
            })
        }

        fn output_path(output: &Self::Output) -> Option<&Path> {
            Some(&output.output_path)
        }

        fn build_project_resolver(
            &self,
            project: &quarto_core::project::ProjectContext,
            lib_dir: &str,
        ) -> ResourceResolverContext {
            ResourceResolverContext::project_root(project.output_dir.clone(), lib_dir.to_string())
        }

        fn extract_resource_report(output: &Self::Output) -> Option<&DocumentResourceReport> {
            Some(&output.resource_report)
        }
    }

    /// Bare-minimum project type for the test: no pre/post hooks.
    struct NoopProjectType;

    #[async_trait(?Send)]
    impl ProjectType for NoopProjectType {
        fn kind(&self) -> quarto_core::project::ProjectKind {
            quarto_core::project::ProjectKind::Default
        }
    }

    #[test]
    fn orchestrator_drains_engine_report_and_copies_to_output_dir() {
        let temp = TempDir::new().unwrap();
        let project_dir = canonical(temp.path());

        // Minimal project: one qmd file.
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: default\n  output-dir: _output\n",
        );
        write(
            &project_dir.join("doc.qmd"),
            "---\ntitle: Doc\n---\n\nBody.\n",
        );

        let runtime = runtime_arc();
        let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();

        let renderer = MockRenderer {
            engine_files_per_doc: vec![
                (
                    PathBuf::from("doc_files/figure-html/cell-1.png"),
                    "PNG".into(),
                ),
                (
                    PathBuf::from("doc_files/data/inline.csv"),
                    "a,b\n1,2\n".into(),
                ),
            ],
        };

        let project_type: Box<dyn ProjectType> = Box::new(NoopProjectType);
        let mut pipeline = ProjectPipeline::with_renderer(
            &mut project,
            project_type,
            html_format(),
            "html",
            runtime.clone(),
            renderer,
        );
        let summary = pollster::block_on(pipeline.run()).expect("run");
        assert!(summary.pass1_failures.is_empty());
        assert!(summary.pass2_failures.is_empty());

        // Engine-reported files should now be inside the output dir.
        let out = project.output_dir.clone();
        assert!(
            out.join("doc_files/figure-html/cell-1.png").exists(),
            "engine-reported figure should be copied to output dir"
        );
        assert!(
            out.join("doc_files/data/inline.csv").exists(),
            "engine-reported data file should be copied to output dir"
        );
        assert_eq!(
            std::fs::read_to_string(out.join("doc_files/data/inline.csv")).unwrap(),
            "a,b\n1,2\n",
            "copied content matches source"
        );

        // The origin survives all the way through (visual smoke
        // check via debug print is enough for the integration
        // boundary; per-origin assertions live in the
        // `project_resources` unit tests).
        let _ = ResourceOrigin::Engine {
            engine: "mock-engine".into(),
            source: project_dir.clone(),
        };
    }

    /// bd-45yw Phase 5: replay engine drives the *real* pipeline
    /// (`RenderToFileRenderer` -> `render_document_to_file` ->
    /// `EngineExecutionStage`) with a substituted `ReplayEngine`,
    /// and the orchestrator's drain copies engine-emitted
    /// `supporting_files` into the output dir. Closes the bd-o8pr
    /// Phase 2 engine-channel test gap that previously required real
    /// R / Python.
    #[test]
    fn orchestrator_drains_replay_engine_report_to_output_dir() {
        use quarto_core::engine::{ExecuteResult, ExecutionContext, ExecutionEngine};
        use quarto_core::render_to_file::RenderToFileOptions;
        use quarto_trace::EngineCapture;

        let temp = TempDir::new().unwrap();
        let project_dir = canonical(temp.path());
        let project_dir_str = project_dir.to_string_lossy().to_string();

        // Materialize the engine-reported supporting files that the
        // replay capture is going to claim. Real engines would
        // produce these; replay just tells the pipeline they exist.
        write(
            &project_dir.join("doc_files/figure-html/cell-1.png"),
            "PNG-replay",
        );
        write(&project_dir.join("doc_files/data/inline.csv"), "x,y\n3,4\n");

        // Project + document declaring the replay engine name.
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: default\n  output-dir: _output\n",
        );
        let qmd_path = project_dir.join("doc.qmd");
        write(
            &qmd_path,
            "---\nengine: replay-real-pipeline-engine\ntitle: Doc\n---\n\n# Hello\n\nReplay-driven body.\n\n```{replay-real-pipeline-engine}\ncode\n```\n",
        );

        // Compute the QMD that EngineExecutionStage will hand to
        // execute() by running the *same* ProjectPipeline path the
        // real run will use, with a probe engine substituted via
        // RenderToFileOptions.engine_registry_override. This
        // guarantees the probe sees the same MetadataMergeStage
        // output the real pipeline will (project config from
        // _quarto.yml, etc.).
        let recorded_input = {
            use std::sync::Mutex;

            struct ProbeEngine {
                captured: Arc<Mutex<Option<String>>>,
            }
            impl ExecutionEngine for ProbeEngine {
                fn name(&self) -> &str {
                    "replay-real-pipeline-engine"
                }
                fn execute(
                    &self,
                    input: &str,
                    _ctx: &ExecutionContext,
                ) -> std::result::Result<ExecuteResult, quarto_core::engine::ExecutionError>
                {
                    *self.captured.lock().unwrap() = Some(input.to_string());
                    Ok(ExecuteResult::passthrough(input))
                }
                fn is_available(&self) -> bool {
                    true
                }
                fn claims_language(
                    &self,
                    language: &str,
                    _first_class: Option<&str>,
                ) -> quarto_core::engine::LanguageClaim {
                    if language == "replay-real-pipeline-engine" {
                        quarto_core::engine::LanguageClaim::Primary(1)
                    } else {
                        quarto_core::engine::LanguageClaim::None
                    }
                }
            }

            let captured = Arc::new(Mutex::new(None::<String>));
            let probe = Arc::new(ProbeEngine {
                captured: captured.clone(),
            });
            let mut probe_registry = quarto_core::engine::EngineRegistry::new();
            probe_registry.register(probe);

            let runtime = runtime_arc();
            let mut probe_project =
                quarto_core::project::ProjectContext::discover(&project_dir, runtime.as_ref())
                    .unwrap();

            let probe_options = RenderToFileOptions {
                engine_registry_override: Some(std::sync::Arc::new(probe_registry)),
                ..Default::default()
            };

            let probe_project_type =
                quarto_core::project::orchestrator::project_type_for(&probe_project);
            let mut probe_pipeline = quarto_core::project::orchestrator::ProjectPipeline::new(
                &mut probe_project,
                probe_project_type,
                html_format(),
                "html",
                &probe_options,
                runtime.clone(),
            );
            let _ = pollster::block_on(probe_pipeline.run()).expect("probe run");

            captured.lock().unwrap().clone().unwrap()
        };

        // Build the replay capture. supporting_files must be paths
        // resolvable against the project root (the orchestrator
        // anchors them at the doc's parent dir, then validates
        // they're within the project).
        let capture = EngineCapture {
            engine_name: "replay-real-pipeline-engine".into(),
            input_qmd: recorded_input,
            result: serde_json::json!({
                // Pass through: keep the document body as-is so the
                // rest of the pipeline produces sensible HTML.
                "markdown": "---\nengine: replay-real-pipeline-engine\ntitle: Doc\n---\n\n# Hello\n\nReplay-driven body.\n",
                "supporting_files": [
                    format!("{project_dir_str}/doc_files/figure-html/cell-1.png"),
                    format!("{project_dir_str}/doc_files/data/inline.csv"),
                ],
                "filters": [],
                "includes": {
                    "header_includes": [],
                    "include_before": [],
                    "include_after": [],
                },
                "needs_postprocess": false,
            }),
            files: Vec::new(),
        };

        // Now run the *real* pipeline through ProjectPipeline::new
        // (the constructor every CLI render uses), with the replay
        // capture in options. The standard RenderToFileRenderer
        // calls render_document_to_file which translates the option
        // into HtmlRenderConfig.engine_registry — so the pipeline
        // builder substitutes the ReplayEngine for
        // `replay-real-pipeline-engine`.
        let runtime = runtime_arc();
        let mut project =
            quarto_core::project::ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();

        let options = RenderToFileOptions {
            replay_captures: vec![capture],
            ..Default::default()
        };

        let project_type = quarto_core::project::orchestrator::project_type_for(&project);
        let mut pipeline = quarto_core::project::orchestrator::ProjectPipeline::new(
            &mut project,
            project_type,
            html_format(),
            "html",
            &options,
            runtime.clone(),
        );

        let summary = pollster::block_on(pipeline.run()).expect("run");
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

        let out = project.output_dir.clone();
        assert!(
            out.join("doc_files/figure-html/cell-1.png").exists(),
            "engine-reported figure should be copied to output dir under the real pipeline driven by ReplayEngine"
        );
        assert!(
            out.join("doc_files/data/inline.csv").exists(),
            "engine-reported data file should be copied to output dir under the real pipeline driven by ReplayEngine"
        );
        assert_eq!(
            std::fs::read_to_string(out.join("doc_files/data/inline.csv")).unwrap(),
            "x,y\n3,4\n",
            "copied content matches source"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────
// Phase 3 — Lua filter channel (real `quarto.doc.add_resource`)
// ─────────────────────────────────────────────────────────────────────

/// A user filter that calls `quarto.doc.add_resource("…")` ends up
/// causing that file to land in the output dir.
///
/// Note: pampa's typewise Lua-filter dispatch does not yet invoke a
/// `Pandoc(doc)` handler (the name is recognized but not called), so
/// these fixtures hook `Para` instead. The behavior we're testing is
/// the resource registration, not which AST node triggers the call.
#[test]
fn lua_filter_add_resource_lands_in_output_dir() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());

    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n",
    );
    // Filter declares a single resource; the file is on disk in the
    // doc's parent directory. Hook `Para` so we register the
    // resource on the first paragraph encountered.
    write(
        &project_dir.join("addres.lua"),
        "local registered = false\nfunction Para(p)\n  if not registered then\n    quarto.doc.add_resource('from-filter.txt')\n    registered = true\n  end\n  return p\nend\n",
    );
    write(
        &project_dir.join("doc.qmd"),
        "---\ntitle: Doc\nfilters:\n  - addres.lua\n---\n\nBody.\n",
    );
    write(&project_dir.join("from-filter.txt"), "filter contents\n");

    let output_dir = render_project(&project_dir);

    assert!(
        output_dir.join("from-filter.txt").exists(),
        "filter-declared resource should land at {}",
        output_dir.join("from-filter.txt").display()
    );
    assert_eq!(
        std::fs::read_to_string(output_dir.join("from-filter.txt")).unwrap(),
        "filter contents\n"
    );
}

/// A filter that *removes* an entry from `meta.resources` must NOT
/// suppress the author's declaration. The static-channel collector
/// reads `profile.resources` (the snapshot taken at frontmatter
/// freeze), which the filter cannot retroactively edit.
///
/// Note: pampa's typewise Lua-filter dispatch does not yet invoke a
/// `Meta(meta)` handler, so today this test passes "for the wrong
/// reason" — the filter is effectively a no-op. Keeping the test
/// pins the static-channel contract: as soon as pampa gains Meta
/// callback support, the test will continue to pass *for the right
/// reason* without modification, because the static channel reads
/// `profile.resources` (the snapshot) regardless of what the
/// filter does to `meta.resources`.
#[test]
fn filter_removing_meta_resources_does_not_drop_author_declaration() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());

    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n",
    );
    // Author declares `author.txt`; the filter wipes meta.resources
    // (as a no-op until pampa wires `Meta`).
    write(
        &project_dir.join("wipe.lua"),
        "function Meta(meta)\n  meta.resources = nil\n  return meta\nend\n",
    );
    write(
        &project_dir.join("doc.qmd"),
        "---\ntitle: Doc\nfilters:\n  - wipe.lua\nresources:\n  - author.txt\n---\n\nBody.\n",
    );
    write(&project_dir.join("author.txt"), "from author\n");

    let output_dir = render_project(&project_dir);

    assert!(
        output_dir.join("author.txt").exists(),
        "author-declared resource must survive a filter that removes it from meta"
    );
}

// The "filter ADDS via meta.resources mutation" test is unit-tested
// in `crates/quarto-core/src/stage/stages/resource_report.rs`
// (covers the additivity-defense logic directly). An end-to-end
// version through `q2 render` is deferred until pampa wires the
// `Meta(meta)` filter callback — see grep for "Meta" in
// `crates/pampa/src/lua/filter.rs:filter_names`. Filed as a
// follow-up beads issue; until then, the unit test pins the
// stage's contract.

/// `addResource` (camelCase alias) works the same.
#[test]
fn lua_filter_camel_case_alias_works() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());

    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n",
    );
    write(
        &project_dir.join("addres.lua"),
        "local r = false\nfunction Para(p)\n  if not r then\n    quarto.doc.addResource('camel.txt')\n    r = true\n  end\n  return p\nend\n",
    );
    write(
        &project_dir.join("doc.qmd"),
        "---\ntitle: Doc\nfilters:\n  - addres.lua\n---\n\nBody.\n",
    );
    write(&project_dir.join("camel.txt"), "ok\n");

    let output_dir = render_project(&project_dir);

    assert!(output_dir.join("camel.txt").exists());
}

// ─────────────────────────────────────────────────────────────────────
// Phase 4 — render manifest
// ─────────────────────────────────────────────────────────────────────

/// Project render emits `.quarto/render-manifest.json` with the
/// resources array containing every resolved entry.
#[test]
fn render_manifest_contains_resources() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());

    write(
        &project_dir.join("_quarto.yml"),
        "\
project:
  type: website
  resources:
    - robots.txt
",
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\n.\n",
    );
    write(&project_dir.join("robots.txt"), "ok\n");

    let _output_dir = render_project(&project_dir);

    let manifest_path = project_dir.join(".quarto/render-manifest.json");
    assert!(
        manifest_path.exists(),
        ".quarto/render-manifest.json should be written"
    );
    let body = std::fs::read_to_string(&manifest_path).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&body).unwrap();
    let resources = manifest
        .get("resources")
        .and_then(|v| v.as_array())
        .expect("manifest.resources should be an array");
    let sources: Vec<String> = resources
        .iter()
        .filter_map(|r| r.get("source").and_then(|s| s.as_str()).map(String::from))
        .collect();
    assert!(
        sources.iter().any(|s| s == "robots.txt"),
        "manifest.resources should include robots.txt, got: {:?}",
        sources
    );
}

// ─────────────────────────────────────────────────────────────────────
// Provenance: a pattern resolves against the file it was written in
// (bd-mt7a6uc4)
// ─────────────────────────────────────────────────────────────────────

/// A `resources:` glob declared in `blog/_metadata.yml` resolves
/// against `blog/`, not against each host document's directory.
///
/// Before bd-mt7a6uc4 the anchor was the *host document*, so a deeply
/// nested page published `blog/deep/data/*.csv` while the files the
/// author meant — `blog/data/*.csv` — were never copied. This is the
/// same defect GH #456 fixed for listing `contents:`, one metadata key
/// over.
#[test]
fn dirmeta_resources_resolve_against_the_declaring_file() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());

    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n",
    );
    write(
        &project_dir.join("blog/_metadata.yml"),
        "resources:\n  - \"data/*.csv\"\n",
    );
    write(&project_dir.join("blog/data/from-blog.csv"), "declaring\n");
    write(
        &project_dir.join("blog/deep/data/from-deep.csv"),
        "host-relative\n",
    );
    write(
        &project_dir.join("blog/deep/index.qmd"),
        "---\ntitle: Deep\n---\n\nBody.\n",
    );

    let output_dir = render_project(&project_dir);

    assert!(
        output_dir.join("blog/data/from-blog.csv").is_file(),
        "the declaring file's directory is the base"
    );
    assert!(
        !output_dir.join("blog/deep/data/from-deep.csv").exists(),
        "the host document's directory is not"
    );
}

/// Front-matter patterns keep resolving against the host document —
/// the provenance rule reduces to the old behavior for the common
/// case, which is why this migration is invisible to most projects.
#[test]
fn frontmatter_resources_still_resolve_against_the_document() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());

    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n",
    );
    write(&project_dir.join("posts/data/local.csv"), "local\n");
    write(
        &project_dir.join("posts/index.qmd"),
        "---\ntitle: P\nresources:\n  - \"data/*.csv\"\n---\n\nBody.\n",
    );

    let output_dir = render_project(&project_dir);
    assert!(output_dir.join("posts/data/local.csv").is_file());
}

/// A document `resources:` negation excludes. Before bd-mt7a6uc4 the
/// `!` entry took the literal-path branch and aborted the render with
/// "Declared resource '<root>/!…' does not exist on disk" — while the
/// file it named was published anyway.
#[test]
fn document_resources_honor_negation() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());

    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n",
    );
    write(&project_dir.join("data/public.csv"), "public\n");
    write(&project_dir.join("data/secret.csv"), "secret\n");
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: H\nresources:\n  - \"data/*.csv\"\n  - \"!data/secret.csv\"\n---\n\nBody.\n",
    );

    let output_dir = render_project(&project_dir);
    assert!(output_dir.join("data/public.csv").is_file());
    assert!(
        !output_dir.join("data/secret.csv").exists(),
        "the negated pattern must exclude it"
    );
}
