/*
 * stage/stages/capture_splice.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Pipeline stage that splices server-recorded engine output into the
 * live pre-engine AST for q2 preview (bd-lucp).
 */

//! Capture-splice stage for `q2 preview` (bd-lucp).
//!
//! Sits between [`PreEngineSugaringStage`](super::PreEngineSugaringStage)
//! and [`EngineExecutionStage`](super::EngineExecutionStage) in the
//! q2-preview pipeline. When configured with an
//! [`EngineCapture`](quarto_trace::EngineCapture), it splices the
//! captured engine output blocks into the current pipeline AST,
//! leaving the rest of the pipeline to render an "as if executed"
//! document. When no capture is configured, the stage is a clean
//! pass-through.
//!
//! See `claude-notes/plans/2026-05-18-q2-preview-project-replay-engine.md`
//! for the architecture rationale: preview-time capture consumption
//! deliberately bypasses [`ReplayEngine`](crate::engine::ReplayEngine)'s
//! strict byte-equality miss policy in favor of AST-level splicing
//! keyed by `(structural_hash(cell), occurrence_index)`. `ReplayEngine`
//! itself is unchanged — it remains the deterministic regression-
//! testing tool bd-45yw designed.
//!
//! ## Post-splice behaviour of [`EngineExecutionStage`]
//!
//! After the splice runs, the AST's `{r}` / `{python}` / … code-blocks
//! have been replaced by `Div.cell` wrappers. `EngineExecutionStage`'s
//! engine detection still reads the document metadata (which still
//! says `engine: knitr`) and tries to look up an engine. In WASM the
//! lookup falls back to the markdown engine (no-op), which leaves the
//! spliced AST unchanged. On native, a real knitr/jupyter could in
//! principle re-execute, but the q2-preview pipeline isn't intended
//! to be run natively with a real engine after the splice; the native
//! use case is `q2 render`, which never goes through this stage.

use async_trait::async_trait;

use quarto_pandoc_types::config_value::ConfigValue;
use quarto_trace::EngineCapture;

use crate::engine::capture_splice::apply_capture_splice;
use crate::stage::data::PandocIncludes;
use crate::stage::{
    EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};
use crate::trace_event;

/// Fold a capture's engine-produced includes into the document `meta`'s
/// `rendered.includes.{header,before-body,after-body}` slots — the
/// preview pane's head/body delivery channel.
///
/// **Why the splice stage does this (bd-5oyk1xce).** On the `q2 render`
/// path, `EngineExecutionStage` accumulates `result.includes` onto
/// `ctx.includes` (`engine_execution.rs`) and `ApplyTemplateStage`
/// late-folds `ctx.includes` into `meta.rendered.includes.*`
/// (`apply_template.rs`). The `q2-preview` pipeline **excludes**
/// `ApplyTemplateStage` (`Q2_PREVIEW_STAGE_EXCLUDED`) and bypasses
/// `EngineExecutionStage`, so neither drain runs — engine header includes
/// (e.g. marimo's `@marimo-team/islands` `<script>`, the
/// `__MARIMO_EXPORT_CONTEXT__` marker, `<marimo-code>`) would be lost and
/// widgets never hydrate. The recorded capture already carries the full
/// `ExecuteResult` (`result.includes` included), so we re-inject those
/// includes here, directly into the meta slots the preview renderer reads
/// (`meta.rendered.includes.header` → `HeaderIncludesEffect`).
///
/// Fail-soft: a capture with no `includes` key, unparseable `includes`
/// JSON, or all-empty include lists is a clean no-op — never an error on
/// the every-keystroke preview path. Append (not overwrite) semantics
/// preserve any authored `include-in-header` already resolved into the
/// slot by `IncludeResolveStage`.
fn fold_capture_includes_into_meta(meta: &mut ConfigValue, capture: &EngineCapture) {
    let Some(includes_json) = capture.result.get("includes") else {
        return;
    };
    let Ok(includes) = serde_json::from_value::<PandocIncludes>(includes_json.clone()) else {
        return;
    };
    if includes.header_includes.is_empty()
        && includes.include_before.is_empty()
        && includes.include_after.is_empty()
    {
        return;
    }
    super::include_resolve::append_pandoc_includes(meta, &includes);
}

/// Splices recorded engine output into the live pre-engine AST.
///
/// Created with [`CaptureSpliceStage::new`] and [`with_capture`]; the
/// q2-preview pipeline builder inserts it after `PreEngineSugaringStage`
/// and before `EngineExecutionStage`.
///
/// [`with_capture`]: CaptureSpliceStage::with_capture
pub struct CaptureSpliceStage {
    /// Recorded engine captures, in execution order (bd-5yff4). One per
    /// engine that ran server-side. Empty = clean pass-through.
    captures: Vec<EngineCapture>,
}

impl CaptureSpliceStage {
    /// Build a splice stage with no captures attached. The stage runs
    /// as a clean pass-through. Used when the q2-preview pipeline is
    /// built without a recorded capture (no server-side execution
    /// happened yet, or the user has not opened a project with
    /// engine cells).
    pub fn new() -> Self {
        Self {
            captures: Vec::new(),
        }
    }

    /// Attach a single recorded capture. Convenience for the
    /// single-engine case; equivalent to `with_captures(vec![capture])`.
    pub fn with_capture(self, capture: EngineCapture) -> Self {
        self.with_captures(vec![capture])
    }

    /// Attach the recorded capture sequence (bd-5yff4). On every per-doc
    /// run, the stage folds the captures in order: for each, it re-parses
    /// `capture.input_qmd` and the markdown field of `capture.result`,
    /// derives a cell-output map, and splices it onto the current AST —
    /// so engine N+1's splice runs on engine N's spliced output. Held by
    /// value because the stage may run on multiple pipeline executions
    /// (e.g. the q2-preview project pipeline runs once per active page).
    pub fn with_captures(mut self, captures: Vec<EngineCapture>) -> Self {
        self.captures = captures;
        self
    }
}

impl Default for CaptureSpliceStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for CaptureSpliceStage {
    fn name(&self) -> &str {
        "capture-splice"
    }

    fn input_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    fn output_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let PipelineData::DocumentAst(mut doc_ast) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };

        if self.captures.is_empty() {
            trace_event!(ctx, EventLevel::Debug, "no captures attached; pass-through");
            return Ok(PipelineData::DocumentAst(doc_ast));
        }

        // bd-qbhp2cvv: before splicing, materialize each capture's
        // embedded supporting files (engine-generated figures) next to
        // the live document, so the relative image references in the
        // spliced output resolve. In WASM `ctx.runtime` writes to the
        // preview VFS — exactly where the SPA/hub-client image
        // resolvers (assetWalker / iframePostProcessor) read. The doc
        // dir comes from the AST's path (absolute in the WASM VFS and
        // in `q2 render`); a bare source name (some native test
        // harnesses) falls back to the project dir.
        let doc_dir = doc_ast
            .path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map_or_else(|| ctx.project.dir.clone(), std::path::Path::to_path_buf);
        for capture in &self.captures {
            if !capture.files.is_empty() {
                trace_event!(
                    ctx,
                    EventLevel::Debug,
                    "capture-splice: materializing {} supporting file(s) for engine={}",
                    capture.files.len(),
                    capture.engine_name
                );
                crate::engine::capture_files::materialize_capture_files(
                    ctx.runtime.as_ref(),
                    &doc_dir,
                    &capture.files,
                );
            }
        }

        // bd-5yff4: fold the captures in order. Engine N+1's splice runs
        // on engine N's spliced output, mirroring the server-side
        // sequence. Each capture is fail-soft: a parse failure or an
        // unmatched cell leaves that engine's cells as raw source without
        // taking the preview down.
        for capture in &self.captures {
            // Re-inject the engine's include-in-header / before-body /
            // after-body content into the document meta (bd-5oyk1xce). This
            // is what carries e.g. marimo's islands `<script>` +
            // `__MARIMO_EXPORT_CONTEXT__` marker to the preview pane's
            // `<head>`, so widgets actually hydrate. Done before (and
            // independent of) the block splice below: the header script must
            // reach the pane even if a cell's block output fails to splice.
            fold_capture_includes_into_meta(&mut doc_ast.ast.meta, capture);

            // Extract result.markdown from the opaque JSON. We use the
            // markdown field for the block splice below; the includes were
            // already folded into meta just above.
            let result_markdown = capture
                .result
                .get("markdown")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            // Parse both QMD strings via the same pampa reader the rest
            // of the pipeline uses. Slot in throwaway names — the parsed
            // ASTs are immediately consumed by the splice, so source-
            // attribution doesn't escape this stage.
            let Ok((a1, _, _)) = pampa::readers::qmd::read(
                capture.input_qmd.as_bytes(),
                false,
                "capture-input.rmarkdown",
                &mut std::io::sink(),
                false, // don't track source locations — splice ignores them
                None,
            ) else {
                trace_event!(
                    ctx,
                    EventLevel::Warn,
                    "failed to parse capture.input_qmd for engine={}; skipping this capture",
                    capture.engine_name
                );
                continue;
            };
            let Ok((b1, _, _)) = pampa::readers::qmd::read(
                result_markdown.as_bytes(),
                false,
                "capture-result.md",
                &mut std::io::sink(),
                false,
                None,
            ) else {
                trace_event!(
                    ctx,
                    EventLevel::Warn,
                    "failed to parse capture.result.markdown for engine={}; skipping this capture",
                    capture.engine_name
                );
                continue;
            };

            let before_blocks = doc_ast.ast.blocks.len();
            doc_ast.ast = apply_capture_splice(doc_ast.ast, &a1, &b1, &capture.engine_name);
            let after_blocks = doc_ast.ast.blocks.len();

            trace_event!(
                ctx,
                EventLevel::Debug,
                "capture-splice: engine={}, blocks {}→{}",
                capture.engine_name,
                before_blocks,
                after_blocks
            );
        }

        Ok(PipelineData::DocumentAst(doc_ast))
    }
}

#[cfg(test)]
mod tests {
    // bd-5oyk1xce: the engine include-in-header fold. Unit-tests the pure
    // `fold_capture_includes_into_meta` seam directly (a full `StageContext`
    // is heavy to construct); the marimo preview e2e proves the whole
    // stage → pane path in a real browser.
    use super::*;
    use quarto_source_map::SourceInfo;

    fn empty_meta() -> ConfigValue {
        ConfigValue::new_map(Vec::new(), SourceInfo::for_test())
    }

    /// Read `meta.rendered.includes.<slot>` as a `Vec<String>` (mirrors
    /// `include_resolve`'s test helper of the same name).
    fn rendered_strings(meta: &ConfigValue, slot: &str) -> Vec<String> {
        let arr = meta
            .get_path(&["rendered", "includes", slot])
            .and_then(|v| v.as_array())
            .unwrap_or(&[]);
        arr.iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect()
    }

    /// Build an `EngineCapture` whose serialized `result` carries the given
    /// includes (empty markdown body — the includes fold is independent of
    /// the block splice).
    fn capture_with_includes(
        engine: &str,
        header: &[&str],
        before: &[&str],
        after: &[&str],
    ) -> EngineCapture {
        EngineCapture {
            engine_name: engine.to_string(),
            input_qmd: String::new(),
            result: serde_json::json!({
                "markdown": "",
                "includes": {
                    "header_includes": header,
                    "include_before": before,
                    "include_after": after,
                },
            }),
            files: Vec::new(),
        }
    }

    #[test]
    fn fold_capture_includes_lands_in_rendered_header_slot() {
        // The marimo case: an islands <script> in header_includes must land
        // in meta.rendered.includes.header (the preview delivery slot).
        let script = "<script type=\"module\" src=\"https://cdn.jsdelivr.net/npm/@marimo-team/islands/main.js\"></script>";
        let mut meta = empty_meta();
        let cap = capture_with_includes("marimo", &[script], &[], &[]);
        fold_capture_includes_into_meta(&mut meta, &cap);
        assert_eq!(rendered_strings(&meta, "header"), vec![script.to_string()]);
    }

    #[test]
    fn fold_capture_includes_covers_all_three_slots() {
        let mut meta = empty_meta();
        let cap = capture_with_includes("marimo", &["<h/>"], &["<b/>"], &["<a/>"]);
        fold_capture_includes_into_meta(&mut meta, &cap);
        assert_eq!(rendered_strings(&meta, "header"), vec!["<h/>"]);
        assert_eq!(rendered_strings(&meta, "before-body"), vec!["<b/>"]);
        assert_eq!(rendered_strings(&meta, "after-body"), vec!["<a/>"]);
    }

    #[test]
    fn fold_capture_includes_appends_not_clobbers() {
        // An authored include-in-header already resolved into the slot by
        // IncludeResolveStage must survive; the engine's include appends.
        let mut meta = empty_meta();
        super::super::include_resolve::append_pandoc_includes(
            &mut meta,
            &PandocIncludes {
                header_includes: vec!["<authored/>".to_string()],
                include_before: vec![],
                include_after: vec![],
            },
        );
        let cap = capture_with_includes("marimo", &["<engine/>"], &[], &[]);
        fold_capture_includes_into_meta(&mut meta, &cap);
        assert_eq!(
            rendered_strings(&meta, "header"),
            vec!["<authored/>", "<engine/>"],
        );
    }

    #[test]
    fn fold_capture_includes_is_fail_soft_on_missing_or_empty() {
        // No `includes` key at all → clean no-op (no rendered slot created).
        let mut meta = empty_meta();
        let cap_no_includes = EngineCapture {
            engine_name: "marimo".to_string(),
            input_qmd: String::new(),
            result: serde_json::json!({ "markdown": "" }),
            files: Vec::new(),
        };
        fold_capture_includes_into_meta(&mut meta, &cap_no_includes);
        assert!(rendered_strings(&meta, "header").is_empty());

        // Present-but-empty includes → also a no-op.
        let cap_empty = capture_with_includes("marimo", &[], &[], &[]);
        fold_capture_includes_into_meta(&mut meta, &cap_empty);
        assert!(rendered_strings(&meta, "header").is_empty());
    }
}
