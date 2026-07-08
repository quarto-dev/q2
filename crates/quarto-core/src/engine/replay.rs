/*
 * engine/replay.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Replay engine: deterministic in-Rust engine driven by a recorded trace.
 */

//! Replay engine for deterministic, no-runtime execution.
//!
//! `ReplayEngine` reproduces the behavior of a previously-recorded
//! engine run (knitr, jupyter, …) by returning the captured
//! [`ExecuteResult`] verbatim. It is used as a debugging and
//! regression-testing tool — running the pipeline against a checked-in
//! trace fixture exercises engine-channel features (resources,
//! filters, `ExecuteResult` fields, includes) without requiring R,
//! Python, or Jupyter installs.
//!
//! See `claude-notes/plans/2026-05-03-replay-engine.md` (bd-45yw) for
//! the design rationale.
//!
//! # Activation
//!
//! Replay is activated out-of-band by the orchestrator/CLI, not via
//! document metadata. When activated, the orchestrator constructs a
//! `ReplayEngine` from a deserialized [`EngineCapture`] and substitutes
//! it into the [`EngineRegistry`] under the recorded engine's name —
//! the document under investigation does not need to be modified.
//!
//! # Miss policy
//!
//! Hard-fail. If the document's input QMD does not exactly match the
//! recorded input, `execute()` returns
//! [`ExecutionError::ExecutionFailed`]. Quiet fallbacks would send
//! debugging investigators on wild-goose chases; we make that
//! impossible.
//!
//! # Source-info caveat
//!
//! Replay does not restore original-engine source provenance for
//! engine-emitted content. Diagnostics that map source positions into
//! engine output (e.g. Jupyter cell line numbers in error messages)
//! will not match between a real engine run and its replay. This is
//! a documented v1 limitation.

use std::sync::Arc;

use quarto_trace::EngineCapture;

use super::LanguageClaim;
use super::context::{ExecuteResult, ExecutionContext};
use super::error::ExecutionError;
use super::traits::ExecutionEngine;

/// Engine that replays a previously-recorded `ExecuteResult`.
///
/// Constructed from a [`quarto_trace::EngineCapture`]; surfaces
/// `name()` matching the recorded engine so it slots into the
/// [`super::EngineRegistry`] under the same key.
#[derive(Debug, Clone)]
pub struct ReplayEngine {
    capture: Arc<EngineCapture>,
}

impl ReplayEngine {
    /// Construct a replay engine from a captured engine run.
    ///
    /// Does not deserialize the captured `ExecuteResult` eagerly —
    /// that happens on each `execute()` call so a malformed capture
    /// surfaces with the actual call's diagnostics.
    pub fn new(capture: EngineCapture) -> Self {
        Self {
            capture: Arc::new(capture),
        }
    }

    /// Construct from a shared `Arc<EngineCapture>`.
    ///
    /// Useful when the same capture drives multiple registry instances.
    pub fn from_arc(capture: Arc<EngineCapture>) -> Self {
        Self { capture }
    }

    /// The recorded engine's name (e.g. `"jupyter"`, `"knitr"`).
    pub fn recorded_engine_name(&self) -> &str {
        &self.capture.engine_name
    }
}

impl ExecutionEngine for ReplayEngine {
    fn name(&self) -> &str {
        // Surface the recorded engine's name so the registry slot
        // matches what the document's `engine:` metadata declares.
        &self.capture.engine_name
    }

    /// Claim Primary(1) for the recorded engine's own name as a language.
    ///
    /// In test fixtures the cell language equals the engine name (e.g.
    /// `{mock-replay-engine}` cells are handled by a `ReplayEngine` named
    /// "mock-replay-engine"). This makes the resolver include the replay
    /// engine in the resolution sequence so the stage actually calls it.
    ///
    /// For real-world engines whose cell languages differ from their engine
    /// name (e.g. knitr uses `{r}` cells), this claim is harmless — there
    /// are no `{knitr}` cells, so the claim never fires. Those engines are
    /// reached via the explicit `engine:` list + T2 Fallback instead.
    fn claims_language(&self, language: &str, _first_class: Option<&str>) -> LanguageClaim {
        if language == self.capture.engine_name {
            LanguageClaim::Primary(1)
        } else {
            LanguageClaim::None
        }
    }

    /// Pure name comparison against the recorded capture, always static
    /// (Phase 4) — a `ReplayEngine` never loads anything.
    fn try_claims_language(
        &self,
        language: &str,
        first_class: Option<&str>,
    ) -> Option<LanguageClaim> {
        Some(self.claims_language(language, first_class))
    }

    fn execute(
        &self,
        input: &str,
        _ctx: &ExecutionContext,
    ) -> Result<ExecuteResult, ExecutionError> {
        if input != self.capture.input_qmd {
            return Err(ExecutionError::execution_failed(
                self.capture.engine_name.clone(),
                format!(
                    "replay miss: input QMD does not match recorded input \
                     (recorded {} bytes, got {} bytes). \
                     Replay is a deterministic regression-testing tool — \
                     re-record the trace if the input has changed.",
                    self.capture.input_qmd.len(),
                    input.len()
                ),
            ));
        }

        serde_json::from_value::<ExecuteResult>(self.capture.result.clone()).map_err(|e| {
            ExecutionError::execution_failed(
                self.capture.engine_name.clone(),
                format!("malformed engine_capture.result in trace: {e}"),
            )
        })
    }

    fn can_freeze(&self) -> bool {
        // Replay is read-only against a captured run; freeze is moot.
        false
    }

    fn is_available(&self) -> bool {
        // No external runtime to check. Always available once a
        // capture is loaded.
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage::PandocIncludes;
    use serde_json::json;
    use std::path::PathBuf;

    fn make_test_context() -> ExecutionContext {
        ExecutionContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/project"),
            PathBuf::from("/project/doc.qmd"),
            "html",
        )
    }

    fn sample_capture() -> EngineCapture {
        EngineCapture {
            engine_name: "jupyter".into(),
            input_qmd: "---\nengine: jupyter\n---\n\n```{python}\nprint('hi')\n```\n".into(),
            result: json!({
                "markdown": "---\nengine: jupyter\n---\n\nhi\n",
                "supporting_files": ["fig1.png"],
                "filters": ["quarto"],
                "includes": {
                    "header_includes": ["<style>.x{}</style>"],
                    "include_before": [],
                    "include_after": [],
                },
                "needs_postprocess": false,
            }),
            files: Vec::new(),
        }
    }

    #[test]
    fn test_replay_engine_name_matches_recorded() {
        let engine = ReplayEngine::new(sample_capture());
        assert_eq!(engine.name(), "jupyter");
    }

    #[test]
    fn test_replay_engine_always_available() {
        let engine = ReplayEngine::new(sample_capture());
        assert!(engine.is_available());
    }

    #[test]
    fn test_replay_engine_cannot_freeze() {
        let engine = ReplayEngine::new(sample_capture());
        assert!(!engine.can_freeze());
    }

    #[test]
    fn test_replay_engine_returns_recorded_result_on_match() {
        let capture = sample_capture();
        let recorded_input = capture.input_qmd.clone();
        let engine = ReplayEngine::new(capture);

        let ctx = make_test_context();
        let result = engine.execute(&recorded_input, &ctx).unwrap();

        assert_eq!(result.markdown, "---\nengine: jupyter\n---\n\nhi\n");
        assert_eq!(result.supporting_files, vec![PathBuf::from("fig1.png")]);
        assert_eq!(result.filters, vec!["quarto".to_string()]);
        assert_eq!(result.includes.header_includes, vec!["<style>.x{}</style>"]);
        assert!(result.includes.include_before.is_empty());
        assert!(result.includes.include_after.is_empty());
        assert!(!result.needs_postprocess);
    }

    #[test]
    fn test_replay_engine_hard_fails_on_input_mismatch() {
        let engine = ReplayEngine::new(sample_capture());
        let ctx = make_test_context();

        let err = engine.execute("totally different input", &ctx).unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("replay miss"),
            "expected 'replay miss' in error, got: {msg}"
        );
        assert!(
            matches!(err, ExecutionError::ExecutionFailed { .. }),
            "expected ExecutionFailed, got: {err:?}"
        );
    }

    #[test]
    fn test_replay_engine_hard_fails_on_byte_level_mismatch() {
        // A single-byte difference (trailing newline) must miss —
        // engine input passed to execute() has been canonicalized
        // by the pipeline, so byte-equality is the right contract.
        let mut capture = sample_capture();
        capture.input_qmd = "abc\n".into();
        let engine = ReplayEngine::new(capture);
        let ctx = make_test_context();

        let err = engine.execute("abc", &ctx).unwrap_err();
        assert!(matches!(err, ExecutionError::ExecutionFailed { .. }));
    }

    #[test]
    fn test_replay_engine_hard_fails_on_malformed_result() {
        // A capture whose `result` JSON does not match the
        // ExecuteResult schema must be rejected loudly. Serde
        // requires every non-Option field unless explicitly defaulted,
        // so a JSON object missing `markdown` is a hard fail. This
        // protects investigators from acting on a partially-decoded
        // replay.
        let capture = EngineCapture {
            engine_name: "jupyter".into(),
            input_qmd: "x".into(),
            result: json!({"not_an_execute_result": true}),
            files: Vec::new(),
        };
        let recorded_input = capture.input_qmd.clone();
        let engine = ReplayEngine::new(capture);
        let ctx = make_test_context();

        let err = engine.execute(&recorded_input, &ctx).unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("malformed engine_capture.result"),
            "expected malformed-result diagnostic, got: {msg}"
        );
        assert!(matches!(err, ExecutionError::ExecutionFailed { .. }));
    }

    #[test]
    fn test_replay_engine_round_trips_a_built_result() {
        // Building an ExecuteResult, serializing it through the
        // capture path, and replaying must yield the same fields.
        let original = ExecuteResult {
            markdown: "out\n".into(),
            supporting_files: vec![PathBuf::from("a.png"), PathBuf::from("b.csv")],
            filters: vec!["quarto".into()],
            includes: PandocIncludes {
                header_includes: vec!["<head>".into()],
                include_before: vec![],
                include_after: vec!["<body>".into()],
            },
            needs_postprocess: true,
            html_dependencies: Vec::new(),
            ..Default::default()
        };

        let result_value = serde_json::to_value(&original).unwrap();
        let capture = EngineCapture {
            engine_name: "knitr".into(),
            input_qmd: "input".into(),
            result: result_value,
            files: Vec::new(),
        };
        let engine = ReplayEngine::new(capture);
        let ctx = make_test_context();

        let replayed = engine.execute("input", &ctx).unwrap();
        assert_eq!(replayed.markdown, original.markdown);
        assert_eq!(replayed.supporting_files, original.supporting_files);
        assert_eq!(replayed.filters, original.filters);
        assert_eq!(
            replayed.includes.header_includes,
            original.includes.header_includes
        );
        assert_eq!(replayed.needs_postprocess, original.needs_postprocess);
    }

    #[test]
    fn test_replay_engine_from_arc_shares_capture() {
        let arc = Arc::new(sample_capture());
        let engine_a = ReplayEngine::from_arc(Arc::clone(&arc));
        let engine_b = ReplayEngine::from_arc(arc);
        assert_eq!(engine_a.name(), engine_b.name());
        assert_eq!(
            engine_a.recorded_engine_name(),
            engine_b.recorded_engine_name()
        );
    }
}
