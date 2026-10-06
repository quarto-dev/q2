/*
 * tests/integration/pandoc_execute_defaults.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * P7 Task 5 — per-format `execute` defaults (pptx's `echo: false` /
 * `warning: false`, both formats' figure sizes), asserted at the scope-
 * assembly seam in `EngineExecutionStage` (`engine_execution.rs:481`).
 */

//! `T5.2`, `T5.3`, `T5.4` from the P7 implementation companion.
//!
//! The engine under test is a probe (no Python/R) that records the
//! `ExecutionContext.execute_scope` it observes and returns a
//! passthrough result — the unit under test is the *scope assembly*,
//! never a real engine, per the plan's vacuity note. Modeled on
//! `replay_engine.rs`'s `capture_engine_input` probe pattern, capturing
//! `ctx.execute_scope` instead of `input`.

use std::sync::{Arc, Mutex};

use quarto_core::engine::{ExecuteResult, ExecutionContext, ExecutionEngine};
use quarto_core::format::Format;
use quarto_core::pipeline::render_qmd_to_pandoc;
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_pandoc_types::ConfigValue;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

struct ScopeProbeEngine {
    captured: Arc<Mutex<Option<ConfigValue>>>,
}

impl ExecutionEngine for ScopeProbeEngine {
    fn name(&self) -> &str {
        "probeeng"
    }

    fn execute(
        &self,
        input: &str,
        ctx: &ExecutionContext,
    ) -> std::result::Result<ExecuteResult, quarto_core::engine::ExecutionError> {
        *self.captured.lock().unwrap() = ctx.execute_scope.clone();
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
        if language == "probeeng" {
            quarto_core::engine::LanguageClaim::Primary(1)
        } else {
            quarto_core::engine::LanguageClaim::None
        }
    }
}

/// Render `content` through `render_qmd_to_pandoc` (or the html pipeline
/// via `to_format == "html"`) with the probe engine substituted in, and
/// return the `execute_scope` the probe observed.
fn observed_execute_scope(content: &str, to_format: &str) -> Option<ConfigValue> {
    // `render_qmd_to_pandoc` shells out to a real `pandoc` and writes a
    // real output file (unlike `render_qmd_to_html`, which returns a
    // string) — it needs a real, writable project directory.
    let temp = tempfile::TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let doc_path = project_dir.join(format!("f-{to_format}.qmd"));
    std::fs::write(&doc_path, content).unwrap();

    let captured = Arc::new(Mutex::new(None::<ConfigValue>));
    let probe = Arc::new(ScopeProbeEngine {
        captured: captured.clone(),
    });
    let mut registry = quarto_core::engine::EngineRegistry::new();
    registry.register(probe);
    let registry = Arc::new(registry);

    let project = ProjectContext {
        dir: project_dir.clone(),
        config: Default::default(),
        is_single_file: true,
        files: vec![],
        output_dir: project_dir.clone(),
        ..Default::default()
    };
    let doc = DocumentInfo::from_path(&doc_path);
    let format = if to_format == "html" {
        Format::html()
    } else {
        Format::from_format_string(to_format)
            .unwrap_or_else(|e| panic!("failed to build Format for {to_format}: {e}"))
    };
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());

    if to_format == "html" {
        // `render_qmd_to_html` unconditionally overwrites
        // `ctx.engine_registry_override` from `config.engine_registry`
        // (`pipeline.rs:955`) — the registry must ride in through the
        // config, not the context, or it is silently clobbered back to
        // `None`.
        let config = quarto_core::pipeline::HtmlRenderConfig {
            engine_registry: Some(registry),
            ..Default::default()
        };
        let _ = pollster::block_on(quarto_core::pipeline::render_qmd_to_html(
            content.as_bytes(),
            "f.qmd",
            &mut ctx,
            &config,
            runtime,
        ))
        .expect("html render should succeed");
    } else {
        ctx.engine_registry_override = Some(registry);
        let _ = pollster::block_on(render_qmd_to_pandoc(
            content.as_bytes(),
            "f.qmd",
            &mut ctx,
            runtime,
        ))
        .expect("pandoc render should succeed");
    }

    captured.lock().unwrap().clone()
}

const CELL_FIXTURE: &str = "---\ntitle: F\n---\n\n```{probeeng}\n1 + 1\n```\n";

/// T5.2 (real stage, real `RenderContext`): a pptx render's observed
/// `execute_scope` has `echo == false` and `warning == false` — the
/// format defaults reaching the engine through
/// `engine_execution.rs`'s merge.
#[test]
fn pptx_execute_defaults_reach_engine() {
    let scope = observed_execute_scope(CELL_FIXTURE, "pptx").expect("pptx must produce a scope");
    assert_eq!(
        scope.get("echo").and_then(|v| v.as_bool()),
        Some(false),
        "pptx's echo:false default did not reach the engine"
    );
    assert_eq!(
        scope.get("warning").and_then(|v| v.as_bool()),
        Some(false),
        "pptx's warning:false default did not reach the engine"
    );
}

/// T5.3: an explicit document `execute: {echo: true}` — the value that
/// *disagrees* with pptx's `echo: false` default — must win. This is
/// the row that fails if a future edit "fixes" the merge by making the
/// format authoritative.
#[test]
fn document_execute_wins_over_format_default() {
    let content = "---\ntitle: F\nexecute:\n  echo: true\n---\n\n```{probeeng}\n1 + 1\n```\n";
    let scope = observed_execute_scope(content, "pptx").expect("pptx must produce a scope");
    assert_eq!(
        scope.get("echo").and_then(|v| v.as_bool()),
        Some(true),
        "the document's execute:{{echo: true}} must win over pptx's echo:false default"
    );
    // The default the document did not mention must still survive.
    assert_eq!(scope.get("warning").and_then(|v| v.as_bool()), Some(false));
}

/// T5.4: an **html** render of the same fixture observes no `echo`
/// override at all — the per-format defaults must not leak into
/// formats that have no opinion.
#[test]
fn html_execute_defaults_unchanged() {
    let scope = observed_execute_scope(CELL_FIXTURE, "html");
    let has_echo_override = scope
        .as_ref()
        .and_then(|s| s.get("echo"))
        .and_then(|v| v.as_bool())
        .is_some();
    assert!(
        !has_echo_override,
        "html must not receive pptx's echo override: {scope:?}"
    );
}

// === long-tail Phase 2: Tier A bulk tail ===

/// Long-tail Phase 2: an **odt** render (wordprocessor family) observes
/// the fig 5×4 defaults through the real stage merge.
#[test]
fn odt_execute_defaults_reach_engine() {
    let scope = observed_execute_scope(CELL_FIXTURE, "odt").expect("odt must produce a scope");
    assert_eq!(
        scope.get("fig-width").and_then(|v| v.as_f64_lenient()),
        Some(5.0),
        "odt's fig-width:5 default did not reach the engine"
    );
    assert_eq!(
        scope.get("fig-height").and_then(|v| v.as_f64_lenient()),
        Some(4.0),
        "odt's fig-height:4 default did not reach the engine"
    );
}

/// Long-tail Phase 2: **fb2** (ebook family) observes the same fig 5×4.
#[test]
fn fb2_execute_defaults_reach_engine() {
    let scope = observed_execute_scope(CELL_FIXTURE, "fb2").expect("fb2 must produce a scope");
    assert_eq!(
        scope.get("fig-width").and_then(|v| v.as_f64_lenient()),
        Some(5.0),
        "fb2's fig-width:5 default did not reach the engine"
    );
    assert_eq!(
        scope.get("fig-height").and_then(|v| v.as_f64_lenient()),
        Some(4.0),
        "fb2's fig-height:4 default did not reach the engine"
    );
}

/// Long-tail Phase 2: **plain** (plaintext family) observes no figure
/// defaults — Q1's `plaintextFormat` declares no `execute:` defaults, so
/// the scope must stay free of them.
#[test]
fn plain_execute_defaults_absent() {
    let scope = observed_execute_scope(CELL_FIXTURE, "plain");
    let has_fig_defaults = scope
        .as_ref()
        .is_some_and(|s| s.get("fig-width").is_some() || s.get("fig-height").is_some());
    assert!(
        !has_fig_defaults,
        "plain must not receive wordprocessor figure defaults: {scope:?}"
    );
}

// === long-tail Phase 5: Tier D JS slides ===

/// Long-tail Phase 5: the Tier D JS slide formats observe the
/// `createHtmlPresentationFormat` defaults — fig 9.5×6.5 and
/// echo/warning false — through the real stage merge.
#[test]
fn tier_d_execute_defaults_reach_engine() {
    for target_format in ["s5", "dzslides", "slidy", "slideous"] {
        let scope = observed_execute_scope(CELL_FIXTURE, target_format)
            .unwrap_or_else(|| panic!("{target_format} must produce a scope"));
        assert_eq!(
            scope.get("echo").and_then(|v| v.as_bool()),
            Some(false),
            "{target_format}'s echo:false default did not reach the engine"
        );
        assert_eq!(
            scope.get("warning").and_then(|v| v.as_bool()),
            Some(false),
            "{target_format}'s warning:false default did not reach the engine"
        );
        assert_eq!(
            scope.get("fig-width").and_then(|v| v.as_f64_lenient()),
            Some(9.5),
            "{target_format}'s fig-width:9.5 default did not reach the engine"
        );
        assert_eq!(
            scope.get("fig-height").and_then(|v| v.as_f64_lenient()),
            Some(6.5),
            "{target_format}'s fig-height:6.5 default did not reach the engine"
        );
    }
}
