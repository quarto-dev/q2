/*
 * tests/render_to_html_user_grammars.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * bd-izfv: regression test that `RenderToHtmlRenderer` forwards a
 * caller-supplied user-grammar provider to the per-page
 * `RenderContext`. Without this wiring the orchestrator's project-
 * render path silently drops grammars and any qmd that lives under
 * a `_quarto.yml` ancestor renders code blocks unhighlighted.
 *
 * The test installs a stub provider that emits a known `data-hl-spans`
 * triple-array for one custom class, drives `RenderToHtmlRenderer`
 * via `ProjectPipeline<…>::with_renderer` exactly like the WASM
 * entry point does, and asserts the rendered HTML carries the
 * matching `<span class="hl-…">` wrapper. The pampa HTML writer
 * turns `data-hl-spans` into the nested span markup (see
 * `crates/pampa/src/writers/html.rs`).
 */

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, RenderMode, project_type_for};
use quarto_core::project::pass2_renderer::{RenderToHtmlRenderer, WasmPassTwoOutput};
use quarto_highlight::{HighlightError, UserGrammarProvider};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// Stub provider that owns one class and emits a fixed triple-array
/// JSON covering the entire source. Used to verify the wiring without
/// pulling in real tree-sitter grammars.
struct StubProvider {
    class: String,
    capture: String,
}

impl UserGrammarProvider for StubProvider {
    fn contains(&self, class: &str) -> bool {
        class == self.class
    }

    fn highlight(&mut self, _class: &str, source: &str) -> Result<Option<String>, HighlightError> {
        let end = source.len();
        let json = format!(
            r#"[[0,{end},"{capture}"]]"#,
            end = end,
            capture = self.capture
        );
        Ok(Some(json))
    }
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn render_with_provider(
    active: &Path,
    provider: Rc<RefCell<dyn UserGrammarProvider>>,
) -> WasmPassTwoOutput {
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(active, runtime.as_ref()).unwrap();
    if !project.is_single_file {
        project = ProjectContext::discover(&project.dir, runtime.as_ref()).unwrap();
    }

    let project_type = project_type_for(&project);
    let vfs_root = project.dir.join(".quarto/project-artifacts");
    let renderer = RenderToHtmlRenderer::new(&vfs_root).with_user_grammars(provider);

    let mut pipeline = ProjectPipeline::with_renderer(
        &mut project,
        project_type,
        Format::html(),
        "html",
        runtime.clone(),
        renderer,
    )
    .with_mode(RenderMode::ActivePage(active.to_path_buf()));

    let summary = pollster::block_on(pipeline.run()).expect("pipeline run");
    assert!(
        summary.pass1_failures.is_empty(),
        "unexpected pass-1 failures: {:?}",
        summary.pass1_failures,
    );
    assert!(
        summary.pass2_failures.is_empty(),
        "unexpected pass-2 failures: {:?}",
        summary.pass2_failures,
    );
    summary
        .outputs
        .into_iter()
        .next()
        .expect("ActivePage mode should produce one output")
}

/// bd-izfv: when a project-rooted document is rendered via
/// `RenderToHtmlRenderer`, a user-grammar provider attached with
/// `with_user_grammars` must flow into the per-page `RenderContext`
/// so `CodeHighlightStage` consults it. The stub provider claims the
/// custom class `mygrammar` and emits one triple covering the whole
/// source; the pampa HTML writer should turn that into a single
/// `<span class="hl-marker">…</span>` wrapper inside the rendered
/// code block.
#[test]
fn project_render_threads_user_grammar_provider() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());

    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: default\n",
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Stub grammar\n---\n\n```mygrammar\nhello world\n```\n",
    );

    let active = canonical(&project_dir.join("index.qmd"));
    let provider: Rc<RefCell<dyn UserGrammarProvider>> = Rc::new(RefCell::new(StubProvider {
        class: "mygrammar".to_string(),
        capture: "marker".to_string(),
    }));

    let output = render_with_provider(&active, provider);

    assert!(
        output.html.contains("<span class=\"hl-marker\""),
        "expected the stub provider's hl-marker span to appear in rendered HTML; got:\n{}",
        snippet(&output.html),
    );
    assert!(
        !output.html.contains("data-hl-spans="),
        "raw data-hl-spans attribute should not leak into the rendered HTML; got:\n{}",
        snippet(&output.html),
    );
}

fn snippet(s: &str) -> String {
    if s.len() <= 400 {
        s.to_string()
    } else {
        format!("{}…", &s[..400])
    }
}
