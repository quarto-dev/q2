/*
 * engine/replay.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Replay engine: deterministic in-Rust engine driven by a recorded trace.
 */

//! Replay engine for deterministic, no-runtime execution.
//!
//! `ReplayEngine` reproduces the behavior of a previously-recorded
//! engine run (knitr, jupyter, …). It is used as a debugging and
//! regression-testing tool — running the pipeline against a checked-in
//! trace fixture exercises engine-channel features (resources,
//! filters, `ExecuteResult` fields, includes) without requiring R,
//! Python, or Jupyter installs.
//!
//! Engines that return markdown (knitr, marimo, …) replay the captured
//! [`ExecuteResult`] verbatim. A Jupyter capture replays its **record**
//! (EXR-6, epic review D2): the trace stores the engine's execution record
//! on `engine_capture.record` (EXR-5), the replay returns that record byte
//! for byte and runs the presenter on it again, with the current `execute:`
//! scope and target format. So replayed Jupyter markdown depends on the
//! current presenter and the format, not on what the recording machine
//! saw; the engine-produced record is what stays fixed. A Jupyter capture
//! without a record predates EXR-5 and fails with a "re-record" error,
//! never a silent fallback to its stored markdown.
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
use super::units::units_for_engine;
use crate::presenter::document::present_document;
use crate::presenter::figures::FsFigureSink;
use crate::presenter::inline::InlineTarget;
use crate::record::{AuthorTexts, EngineRecord, fill_ids, read_record_value};

/// The engine whose captures replay from their record.
const JUPYTER: &str = "jupyter";

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

impl ReplayEngine {
    /// A Jupyter capture: present the recorded execution record over `input` with the
    /// stage's units (a fresh scan with ids filled when no stage supplied them), the
    /// current `execute:` scope and target format. The stored result's side data
    /// (filters, includes, warnings, …) is kept; its markdown and supporting files
    /// are recomputed, and the record travels on the result byte for byte.
    fn present_record(
        &self,
        input: &str,
        ctx: &ExecutionContext,
        stored: ExecuteResult,
    ) -> Result<ExecuteResult, ExecutionError> {
        let fail = |message: String| ExecutionError::execution_failed(JUPYTER, message);
        let Some(record) = self.capture.record.as_ref() else {
            return Err(fail(
                "replay: this trace's jupyter capture has no execution record \
                 (`engine_capture.record`), so it was recorded before records existed and \
                 cannot be presented. Re-record the trace with `trace: true`."
                    .to_string(),
            ));
        };
        let record = read_record_value(record.clone())
            .map_err(|e| fail(format!("malformed engine_capture.record in trace: {e}")))?;

        let mut units = units_for_engine(&ctx.units, input);
        if ctx.units.is_empty() {
            let texts = AuthorTexts::from_scanned(&units);
            fill_ids(&mut units, &texts).map_err(|e| fail(format!("replay: {e}")))?;
        }
        let mut fig = FsFigureSink::new(&ctx.source_path);
        let presented = present_document(
            &record.cells,
            input,
            &units,
            ctx.execute_scope.as_ref(),
            InlineTarget::from_format(&ctx.format),
            &ctx.format,
            &mut fig,
        )
        .map_err(|e| {
            fail(format!(
                "replay: presenting the recorded execution failed: {e}"
            ))
        })?;

        let mut result = stored;
        result.markdown = presented.markdown;
        result.supporting_files = presented.files_written;
        result.warnings.extend(
            presented
                .notes
                .iter()
                .map(|note| crate::engine::diagnostics::note_diagnostic(note, &units, ctx)),
        );
        result.record = record
            .metadata
            .quarto_engines
            .first()
            .map(|engine| EngineRecord {
                engine: engine.clone(),
                cells: record.cells.clone(),
            });
        Ok(result)
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
        ctx: &ExecutionContext,
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

        let stored =
            serde_json::from_value::<ExecuteResult>(self.capture.result.clone()).map_err(|e| {
                ExecutionError::execution_failed(
                    self.capture.engine_name.clone(),
                    format!("malformed engine_capture.result in trace: {e}"),
                )
            })?;
        if self.capture.engine_name == JUPYTER {
            self.present_record(input, ctx, stored)
        } else {
            Ok(stored)
        }
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

    /// A markdown-replaying engine's capture (knitr: its markdown is returned verbatim).
    fn sample_capture() -> EngineCapture {
        EngineCapture {
            engine_name: "knitr".into(),
            input_qmd: "---\nengine: knitr\n---\n\n```{r}\nprint('hi')\n```\n".into(),
            result: json!({
                "markdown": "---\nengine: knitr\n---\n\nhi\n",
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
            record: None,
        }
    }

    #[test]
    fn test_replay_engine_name_matches_recorded() {
        let engine = ReplayEngine::new(sample_capture());
        assert_eq!(engine.name(), "knitr");
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

        assert_eq!(result.markdown, "---\nengine: knitr\n---\n\nhi\n");
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
            engine_name: "knitr".into(),
            input_qmd: "x".into(),
            result: json!({"not_an_execute_result": true}),
            record: None,
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
            record: None,
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

    // ── EXR-6 Phase 3b: Jupyter captures replay from their record ────────

    use crate::engine::units::{Unit, UnitId, scan_units};
    use crate::record::{EngineEntryInput, HashDefinition, RecordBuilder, UnitResult, coalesce};
    use serde_json::Value;

    fn stdout(text: &str) -> Vec<Value> {
        vec![json!({"output_type": "stream", "name": "stdout", "text": text})]
    }

    fn plain_and_html(plain: &str, html: &str) -> Vec<Value> {
        vec![json!({
            "output_type": "execute_result",
            "execution_count": 1,
            "data": {"text/plain": plain, "text/html": html},
            "metadata": {}
        })]
    }

    /// A Jupyter capture whose record holds `results` for the units of `text`, with
    /// `stored_markdown` as the recorded (pre-flip style) markdown. `ids` overrides
    /// the units' ids before the record is built, to simulate a sugared cell whose
    /// recorded id a fresh scan cannot reproduce.
    fn jupyter_capture(
        text: &str,
        results: &[(usize, Vec<Value>)],
        stored_markdown: &str,
        ids: Option<&[&str]>,
    ) -> EngineCapture {
        let mut units = scan_units(text);
        let texts = AuthorTexts::from_scanned(&units);
        fill_ids(&mut units, &texts).unwrap();
        if let Some(ids) = ids {
            for (unit, id) in units.iter_mut().zip(ids) {
                match unit {
                    Unit::Cell(c) => c.id = Some(UnitId(id.to_string())),
                    Unit::Inline(i) => i.id = Some(UnitId(id.to_string())),
                }
            }
        }
        let def = HashDefinition::conservative("jupyter");
        let mut builder = RecordBuilder::new("jupyter", &def, &units, &texts).unwrap();
        for (index, outputs) in results {
            builder
                .push(
                    *index,
                    UnitResult {
                        outputs: outputs.clone(),
                        execution_count: Some(1),
                        kernel: Some("python3".to_string()),
                    },
                )
                .unwrap();
        }
        let engine_record = builder.finish(EngineEntryInput::default()).unwrap();
        let record = coalesce(std::slice::from_ref(&engine_record), &units).unwrap();
        EngineCapture {
            engine_name: "jupyter".into(),
            input_qmd: text.into(),
            result: json!({
                "markdown": stored_markdown,
                "supporting_files": [],
                "filters": ["quarto"],
                "includes": {
                    "header_includes": [],
                    "include_before": [],
                    "include_after": [],
                },
                "needs_postprocess": false,
            }),
            files: Vec::new(),
            record: Some(serde_json::to_value(&record).unwrap()),
        }
    }

    /// A context whose document lives in a temp dir (figures would land there).
    fn tmp_context(format: &str) -> (tempfile::TempDir, ExecutionContext) {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ExecutionContext::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            dir.path().join("doc.qmd"),
            format,
        );
        (dir, ctx)
    }

    const CELL_MD: &str = "::: {.cell}\n\n```{.python .cell-code}\nprint('hi')\n```\n\n\
        ::: {.cell-output .cell-output-stdout}\n\n```\nhi\n```\n\n:::\n\n:::\n";

    #[test]
    fn replay_trace_recorded_before_exr5_fails_with_rerecord_error() {
        let mut capture = sample_capture();
        capture.engine_name = "jupyter".into();
        capture.record = None;
        let input = capture.input_qmd.clone();
        let engine = ReplayEngine::new(capture);
        let err = engine.execute(&input, &make_test_context()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("no execution record"), "{msg}");
        assert!(msg.contains("Re-record"), "{msg}");
        assert!(matches!(err, ExecutionError::ExecutionFailed { .. }));
    }

    #[test]
    fn replay_trace_recorded_between_exr5_and_flip_replays() {
        // The stored markdown is not what the presenter makes of the record (a
        // pre-flip recording); replay presents the record, never returns the string.
        let text = "Before.\n\n```{python}\nprint('hi')\n```\n";
        let capture = jupyter_capture(text, &[(0, stdout("hi\n"))], "STALE STORED MARKDOWN", None);
        let (_dir, ctx) = tmp_context("html");
        let result = ReplayEngine::new(capture).execute(text, &ctx).unwrap();
        assert_eq!(result.markdown, format!("Before.\n\n{CELL_MD}"));
        // The side data is the stored result's.
        assert_eq!(result.filters, ["quarto"]);
    }

    #[test]
    fn replay_engine_returns_byte_identical_record() {
        let text = "```{python}\nprint('hi')\n```\n";
        let capture = jupyter_capture(text, &[(0, stdout("hi\n"))], "", None);
        let recorded = read_record_value(capture.record.clone().unwrap()).unwrap();
        let (_dir, ctx) = tmp_context("html");
        let result = ReplayEngine::new(capture).execute(text, &ctx).unwrap();
        let replayed = result.record.expect("the record travels on the result");
        assert_eq!(replayed.cells, recorded.cells);
        assert_eq!(replayed.engine, recorded.metadata.quarto_engines[0]);
    }

    #[test]
    fn replay_id_mismatch_falls_back_to_content_hash() {
        // The recorded id is one a fresh scan cannot reproduce (a sugared cell's);
        // the content hash still matches the cell.
        let text = "```{python}\nprint('hi')\n```\n";
        let capture = jupyter_capture(
            text,
            &[(0, stdout("hi\n"))],
            "",
            Some(&["0-deadbeef-fig-sugared"]),
        );
        let (_dir, ctx) = tmp_context("html");
        let result = ReplayEngine::new(capture).execute(text, &ctx).unwrap();
        assert_eq!(result.markdown, CELL_MD);
    }

    #[test]
    fn replay_knitr_trace_unchanged() {
        // The markdown replay path: a knitr capture's result comes back verbatim,
        // record or not.
        let capture = sample_capture();
        let input = capture.input_qmd.clone();
        let result = ReplayEngine::new(capture)
            .execute(&input, &make_test_context())
            .unwrap();
        assert_eq!(result.markdown, "---\nengine: knitr\n---\n\nhi\n");
        assert_eq!(result.supporting_files, vec![PathBuf::from("fig1.png")]);
    }

    #[test]
    fn replay_miss_policy_still_hard_fails() {
        let text = "```{python}\nprint('hi')\n```\n";
        let capture = jupyter_capture(text, &[(0, stdout("hi\n"))], "", None);
        let (_dir, ctx) = tmp_context("html");
        let changed = "```{python}\nprint('hi') \n```\n";
        let err = ReplayEngine::new(capture)
            .execute(changed, &ctx)
            .unwrap_err();
        assert!(err.to_string().contains("replay miss"), "{err}");
    }

    #[test]
    fn replay_output_follows_target_format() {
        // One trace, two target formats: the span's value is raw html for html and
        // the plain text for a format with no html.
        let text = "Value `{python} v` here.\n";
        let capture = jupyter_capture(
            text,
            &[(0, plain_and_html("'plain'", "<b>rich</b>"))],
            "",
            None,
        );
        let (_dir, html) = tmp_context("html");
        let (_dir2, docx) = tmp_context("docx");
        let engine = ReplayEngine::new(capture);
        let for_html = engine.execute(text, &html).unwrap().markdown;
        let for_docx = engine.execute(text, &docx).unwrap().markdown;
        assert!(for_html.contains("<b>rich</b>"), "{for_html}");
        assert!(for_docx.contains("plain"), "{for_docx}");
        assert!(!for_docx.contains("<b>rich</b>"), "{for_docx}");
        assert_ne!(for_html, for_docx);
    }
}
