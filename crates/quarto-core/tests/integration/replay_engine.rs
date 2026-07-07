/*
 * tests/replay_engine.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Integration tests for the replay engine (bd-45yw).
 *
 * Plan: claude-notes/plans/2026-05-03-replay-engine.md
 *
 * These tests exercise the orchestrator-level seam:
 * `RenderToFileOptions.replay_capture` reaches the pipeline through
 * `render_to_file` and substitutes a `ReplayEngine` for the recorded
 * engine name. The render path is the same `render_document_to_file`
 * that `quarto render` uses, so a passing test here means the CLI
 * path will activate replay correctly once the `--replay` flag lands
 * (Phase 4c).
 */

use std::path::Path;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::Format;
use quarto_core::engine::{ExecuteResult, ExecutionContext, ExecutionEngine};
use quarto_core::pipeline::{HtmlRenderConfig, render_qmd_to_html};
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};
use quarto_trace::EngineCapture;

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn runtime_arc() -> Arc<dyn SystemRuntime> {
    Arc::new(NativeRuntime::new())
}

/// Compute the QMD that `EngineExecutionStage` will hand to
/// `engine.execute()` for the given document. Renders once with a
/// probe registry whose engine registers under `engine_name` (the
/// engine the document declares) and just records `execute()`'s
/// input.
fn capture_engine_input(content: &[u8], source_name: &str, engine_name: &str) -> String {
    use std::sync::Mutex;

    struct ProbeEngine {
        captured: Arc<Mutex<Option<String>>>,
        name: String,
    }
    impl ExecutionEngine for ProbeEngine {
        fn name(&self) -> &str {
            &self.name
        }
        fn execute(
            &self,
            input: &str,
            _ctx: &ExecutionContext,
        ) -> std::result::Result<ExecuteResult, quarto_core::engine::ExecutionError> {
            *self.captured.lock().unwrap() = Some(input.to_string());
            Ok(ExecuteResult::passthrough(input))
        }
        fn is_available(&self) -> bool {
            true
        }
    }

    let captured = Arc::new(Mutex::new(None::<String>));
    let probe = Arc::new(ProbeEngine {
        captured: captured.clone(),
        name: engine_name.to_string(),
    });
    let mut registry = quarto_core::engine::EngineRegistry::new();
    registry.register(probe);

    let project = ProjectContext {
        dir: std::path::PathBuf::from("/project"),
        config: Default::default(),
        is_single_file: true,
        files: vec![],
        output_dir: std::path::PathBuf::from("/project"),
    };
    let doc = DocumentInfo::from_path("/project/dummy.qmd");
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

    let config = HtmlRenderConfig {
        resolver: None,
        engine_registry: Some(registry),
        ..Default::default()
    };
    let runtime = runtime_arc();
    let _ = pollster::block_on(render_qmd_to_html(
        content,
        source_name,
        &mut ctx,
        &config,
        runtime,
    ))
    .unwrap();

    captured
        .lock()
        .unwrap()
        .clone()
        .expect("probe engine should have captured input")
}

/// bd-45yw Phase 4b: `RenderToFileOptions.replay_capture` flows
/// through `render_to_file` -> `render_document_to_file` ->
/// `HtmlRenderConfig.engine_registry` -> the pipeline. A document
/// declaring an engine with no real impl renders correctly only when
/// the replay engine substitutes for it.
#[test]
fn replay_capture_in_options_overrides_engine_through_render_to_file() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp
        .path()
        .canonicalize()
        .unwrap_or_else(|_| temp.path().to_path_buf());

    // The document declares an engine name that no real engine
    // implements. Without replay, the EngineRegistry falls back to
    // markdown with a warning. With replay, the recorded ExecuteResult
    // produces a distinct, observable HTML marker.
    let qmd_path = project_dir.join("test.qmd");
    write_file(
        &qmd_path,
        "---\nengine: replay-only-engine-4b\ntitle: Test\n---\n\n# Original\n\nOriginal body line.\n",
    );

    // Capture the QMD that the stage will pass to execute().
    let recorded_input = capture_engine_input(
        std::fs::read(&qmd_path).unwrap().as_slice(),
        &qmd_path.to_string_lossy(),
        "replay-only-engine-4b",
    );

    let recorded_markdown = "---\nengine: replay-only-engine-4b\ntitle: Test\n---\n\n\
         # Replayed Heading\n\nDistinct replay marker QQQ.\n";

    let capture = EngineCapture {
        engine_name: "replay-only-engine-4b".into(),
        input_qmd: recorded_input,
        result: serde_json::json!({
            "markdown": recorded_markdown,
            "supporting_files": [],
            "filters": [],
            "includes": {
                "header_includes": [],
                "include_before": [],
                "include_after": [],
            },
            "needs_postprocess": false,
        }),
    };

    let options = RenderToFileOptions {
        replay_captures: vec![capture],
        ..Default::default()
    };

    let runtime = runtime_arc();
    let result = render_to_file(&qmd_path, "html", &options, runtime).expect("render_to_file");

    let html = std::fs::read_to_string(&result.output_path).expect("read rendered HTML");
    assert!(
        html.contains("Replayed Heading"),
        "rendered HTML must carry replay heading; got first 400 chars:\n{}",
        &html.chars().take(400).collect::<String>()
    );
    assert!(
        html.contains("Distinct replay marker QQQ"),
        "rendered HTML must carry replay marker; got first 400 chars:\n{}",
        &html.chars().take(400).collect::<String>()
    );
    assert!(
        !html.contains("Original body line"),
        "rendered HTML must not contain original body — replay should override"
    );
}

/// bd-45yw Phase 4b miss: a replay capture whose `input_qmd` does
/// not match the document's serialized engine input must surface as
/// a render failure, not as a silent fallback. This guards the
/// hard-fail policy through the public file-IO entry.
#[test]
fn replay_capture_miss_surfaces_as_render_error() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp
        .path()
        .canonicalize()
        .unwrap_or_else(|_| temp.path().to_path_buf());

    let qmd_path = project_dir.join("test.qmd");
    write_file(
        &qmd_path,
        "---\nengine: replay-only-engine-4b\n---\n\n# Real document\n",
    );

    let capture = EngineCapture {
        engine_name: "replay-only-engine-4b".into(),
        // Recorded input deliberately doesn't match the document.
        input_qmd: "completely unrelated content\n".into(),
        result: serde_json::json!({
            "markdown": "x\n",
            "supporting_files": [],
            "filters": [],
            "includes": {
                "header_includes": [],
                "include_before": [],
                "include_after": [],
            },
            "needs_postprocess": false,
        }),
    };

    let options = RenderToFileOptions {
        replay_captures: vec![capture],
        ..Default::default()
    };

    let runtime = runtime_arc();
    let err = render_to_file(&qmd_path, "html", &options, runtime)
        .expect_err("replay miss must surface as a render error");
    let msg = err.to_string();
    assert!(
        msg.contains("replay miss"),
        "expected 'replay miss' diagnostic, got: {msg}"
    );
}

/// bd-45yw Phase 4b: `replay_capture: None` (the default) leaves
/// the pipeline using the standard registry. Regression guard that
/// adding the option doesn't disturb the no-replay path.
#[test]
fn replay_capture_absent_uses_default_registry() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp
        .path()
        .canonicalize()
        .unwrap_or_else(|_| temp.path().to_path_buf());

    let qmd_path = project_dir.join("test.qmd");
    write_file(&qmd_path, "---\ntitle: Test\n---\n\n# Hello\n\nWorld\n");

    let options = RenderToFileOptions {
        replay_captures: Vec::new(),
        ..Default::default()
    };

    let runtime = runtime_arc();
    let result = render_to_file(&qmd_path, "html", &options, runtime).expect("render_to_file");

    let html = std::fs::read_to_string(&result.output_path).unwrap();
    assert!(
        html.contains("Hello"),
        "default-registry render must emit the document body"
    );
}
