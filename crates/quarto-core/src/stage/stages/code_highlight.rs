/*
 * stage/stages/code_highlight.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Annotate CodeBlock / inline Code nodes with `data-hl-spans` so the
 * HTML writer (and any future format-specific writers) can emit
 * syntax-highlighted markup.
 */

//! Code-highlight annotation stage.
//!
//! Runs after user post-filters and before the HTML writer. For each
//! `CodeBlock` / inline `Code` whose first class resolves to a built-in
//! grammar or a user grammar (discovered under
//! `<project_dir>/_quarto/grammars/`), writes a JSON triple-array
//! encoding of tree-sitter highlight spans into the node's
//! `data-hl-spans` attribute value. Nodes that already carry the
//! attribute are left alone — filter-authored annotations win.
//!
//! The annotation logic itself lives in `quarto_highlight::annotate`;
//! this stage is a thin pipeline wrapper that handles context plumbing
//! (project dir discovery, diagnostics, tracing) and defers the work.

use async_trait::async_trait;
use quarto_error_reporting::DiagnosticMessage;

#[cfg(not(target_arch = "wasm32"))]
use crate::stage::EventLevel;
use crate::stage::{PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext};
#[cfg(not(target_arch = "wasm32"))]
use crate::trace_event;

/// Pipeline stage that annotates code blocks with syntax-highlight
/// spans.
pub struct CodeHighlightStage;

impl CodeHighlightStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CodeHighlightStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for CodeHighlightStage {
    fn name(&self) -> &str {
        "code-highlight"
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
        let PipelineData::DocumentAst(mut doc) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };

        // Priority: a provider preset on the StageContext (typically the
        // browser-side `JsUserGrammars` threaded in by hub-client) wins
        // over the native disk-loader path. This lets both targets share
        // one code path without cfg-gating at the call site.
        //
        // Manual `&mut **b` rather than `as_deref_mut()` because the
        // latter preserves the trait-object's implicit `'static` bound,
        // which then fights the async-trait `'static` future-lifetime
        // check. `&mut **b` produces a fresh borrow with the natural
        // (method-body) lifetime.
        let result = if ctx.user_grammar_provider.is_some() {
            let user = ctx
                .user_grammar_provider
                .as_mut()
                .map(|b| &mut **b as &mut dyn quarto_highlight::UserGrammarProvider);
            quarto_highlight::annotate_pandoc(&mut doc.ast, user)
        } else {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let mut user = load_user_grammars(ctx);
                let user_ref = user
                    .as_mut()
                    .map(|u| u as &mut dyn quarto_highlight::UserGrammarProvider);
                quarto_highlight::annotate_pandoc(&mut doc.ast, user_ref)
            }
            #[cfg(target_arch = "wasm32")]
            {
                // No context-provided JS bridge and no native disk path;
                // highlight with built-ins only.
                quarto_highlight::annotate_pandoc(&mut doc.ast, None)
            }
        };

        if let Err(e) = result {
            // Highlighting is non-essential — don't fail the pipeline
            // for a broken grammar/query. Surface as a warning diagnostic
            // and emit the document un-highlighted.
            ctx.diagnostics.push(DiagnosticMessage::warning(format!(
                "syntax-highlight annotation failed: {e}",
            )));
        }

        Ok(PipelineData::DocumentAst(doc))
    }
}

/// Discover and load user grammars from `<project>/_quarto/grammars/`.
/// Returns `None` if the directory doesn't exist, is empty of grammars,
/// or any load errors — user-facing errors are recorded on `ctx` as
/// diagnostics so a single broken grammar doesn't abort the pipeline.
#[cfg(not(target_arch = "wasm32"))]
fn load_user_grammars(ctx: &mut StageContext) -> Option<quarto_highlight::UserGrammars> {
    let grammars_dir = ctx.project.dir.join("_quarto").join("grammars");
    if !grammars_dir.is_dir() {
        return None;
    }

    let mut grammars = quarto_highlight::UserGrammars::new();
    match grammars.load_all_from_parent(&grammars_dir) {
        Ok(loaded) if !loaded.is_empty() => {
            trace_event!(
                ctx,
                EventLevel::Debug,
                "loaded {} user grammars from {}",
                loaded.len(),
                grammars_dir.display()
            );
            Some(grammars)
        }
        Ok(_) => None,
        Err(e) => {
            ctx.diagnostics.push(DiagnosticMessage::warning(format!(
                "failed to load user grammars from {}: {e}",
                grammars_dir.display()
            )));
            None
        }
    }
}
