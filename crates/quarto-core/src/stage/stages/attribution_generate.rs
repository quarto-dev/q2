/*
 * stage/stages/attribution_generate.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Top-level attribution-generate pipeline stage.
//!
//! Reads `ctx.attribution_provider` (set by the CLI
//! `--attribution=git` flag or the WASM
//! `parse_qmd_to_ast_with_attribution` entry point), calls
//! `build()`, merges identities with any user-authored
//! `meta.attribution.identities`, and stores the result on
//! `ctx.attribution_data`.
//!
//! **Pipeline position:** runs immediately before
//! [`UserFiltersStage::pre`](super::UserFiltersStage) so user
//! Lua filters can query attribution via the `quarto.attribution.*`
//! host binding from any pre-phase or post-phase entry point. The
//! sidecar produced here is later bridged into the inner
//! `RenderContext` by [`AstTransformsStage`](super::AstTransformsStage)
//! so [`AttributionRenderTransform`](crate::transforms::AttributionRenderTransform)
//! can bake the writer-side lookup table.
//!
//! No-op (short-circuit return) when:
//! 1. The target format doesn't consume attribution
//!    ([`format_supports_attribution`] is `false`).
//! 2. The document opts out via `attribution: false` in meta.
//! 3. No provider is installed on [`StageContext`].
//!
//! Mirrors the skip ladder in the legacy
//! `AttributionGenerateTransform`. This stage replaces that transform
//! in the top-level pipeline; the transform no longer runs from inside
//! `AstTransformsStage`.

use std::sync::Arc;

use async_trait::async_trait;

use crate::attribution::{AttributionData, format_supports_attribution, identity_map_from_meta};
use crate::render::{BinaryDependencies, RenderContext};
use crate::stage::{
    EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};
use crate::trace_event;
use crate::transforms::is_feature_disabled;

/// Top-level stage that populates [`StageContext::attribution_data`].
///
/// See module docs for placement rationale and skip ladder.
pub struct AttributionGenerateStage;

impl AttributionGenerateStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AttributionGenerateStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for AttributionGenerateStage {
    fn name(&self) -> &str {
        "attribution-generate"
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
        let PipelineData::DocumentAst(doc) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };

        // Skip ladder, in order:
        //
        // 1. Format must consume the lookup. Bail first so opting in
        //    on a non-HTML target doesn't spawn the provider's
        //    subprocess for nothing.
        if !format_supports_attribution(&ctx.format) {
            return Ok(PipelineData::DocumentAst(doc));
        }

        // 2. Affirmative `attribution: false` opt-out wins over any
        //    provider installed by the CLI / WASM entry point.
        if is_feature_disabled(&doc.ast.meta, "attribution") {
            return Ok(PipelineData::DocumentAst(doc));
        }

        // 3. No provider installed → nothing to do; sidecar stays None.
        let Some(provider) = ctx.attribution_provider.clone() else {
            return Ok(PipelineData::DocumentAst(doc));
        };

        // 4. Construct a minimal RenderContext just to call
        //    `provider.build(...)`. The provider trait takes
        //    `&RenderContext` for historical reasons; only the
        //    `binaries.git` and `document.input` fields are read
        //    (GitBlameProvider). PreBuiltAttributionProvider ignores
        //    the argument entirely. The temp context's other fields
        //    are unused.
        let binaries = BinaryDependencies::discover(ctx.runtime.as_ref());
        let render_ctx = RenderContext::new(&ctx.project, &ctx.document, &ctx.format, &binaries);

        let AttributionData {
            runs,
            mut identities,
            file_id,
        } = provider
            .build(&render_ctx)
            .map_err(|e| PipelineError::stage_error(self.name(), e.to_string()))?;

        // Preserve provider Arc<str> keys on collision (the
        // interning invariant in `IdentityMap` and `AttributionRun`
        // depends on `Arc::ptr_eq` between them) — `HashMap::get_mut`
        // returns a `&mut Identity` without touching the key. Drop
        // non-colliding user keys (an actor named in YAML but with
        // no runs in this document is invisible at the writer and
        // would be dead weight in the map).
        for (user_key, user_id) in identity_map_from_meta(&doc.ast.meta) {
            if let Some(slot) = identities.get_mut(&user_key) {
                *slot = user_id;
            }
        }

        ctx.attribution_data = Some(Arc::new(AttributionData {
            runs,
            identities,
            file_id,
        }));

        trace_event!(
            ctx,
            EventLevel::Debug,
            "attribution-generate populated sidecar"
        );

        Ok(PipelineData::DocumentAst(doc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attribution::PreBuiltAttributionProvider;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectContext};
    use crate::stage::DocumentAst;
    use pampa::pandoc::ASTContext;
    use quarto_pandoc_types::pandoc::Pandoc;
    use quarto_source_map::SourceContext;
    use quarto_system_runtime::NativeRuntime;
    use std::path::PathBuf;

    fn make_ctx(
        format: Format,
        provider: Option<Arc<dyn crate::attribution::AttributionSourceProvider>>,
    ) -> StageContext {
        let runtime = Arc::new(NativeRuntime::new());
        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: crate::project::ProjectConfig::default(),
            is_single_file: true,
            files: vec![],
            output_dir: PathBuf::from("/project"),
        };
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let mut ctx = StageContext::new(runtime, format, project, doc).unwrap();
        ctx.attribution_provider = provider;
        ctx
    }

    fn make_doc() -> DocumentAst {
        DocumentAst {
            path: PathBuf::from("/project/test.qmd"),
            ast: Pandoc {
                meta: quarto_pandoc_types::ConfigValue::default(),
                blocks: vec![],
            },
            ast_context: ASTContext::default(),
            source_context: SourceContext::new(),
            warnings: vec![],
            recorded_includes: Vec::new(),
        }
    }

    #[tokio::test]
    async fn no_provider_leaves_attribution_data_none() {
        let mut ctx = make_ctx(Format::html(), None);
        let input = PipelineData::DocumentAst(make_doc());
        let _ = AttributionGenerateStage::new()
            .run(input, &mut ctx)
            .await
            .unwrap();
        assert!(
            ctx.attribution_data.is_none(),
            "no provider installed → sidecar must stay None"
        );
    }

    #[tokio::test]
    async fn provider_populates_attribution_data() {
        // Minimal transport JSON: one run, one identity.
        let json = r##"{
            "runs": [{"start": 0, "end": 5, "actor": "alice@example.com", "time": 1000}],
            "identities": {
                "alice@example.com": {"name": "Alice", "color": "#ff0000"}
            }
        }"##;
        let provider: Arc<dyn crate::attribution::AttributionSourceProvider> =
            Arc::new(PreBuiltAttributionProvider::new(json.to_string()));
        let mut ctx = make_ctx(Format::html(), Some(provider));
        let input = PipelineData::DocumentAst(make_doc());
        let _ = AttributionGenerateStage::new()
            .run(input, &mut ctx)
            .await
            .unwrap();
        let data = ctx.attribution_data.expect("provider populates sidecar");
        assert_eq!(data.runs.len(), 1);
        assert_eq!(data.identities.len(), 1);
    }

    #[tokio::test]
    async fn non_html_format_skips_provider_invocation() {
        // q2-debug parses as HTML but other formats short-circuit.
        // Use a JSON-like format identifier to hit the skip path.
        let mut format = Format::html();
        format.identifier = crate::format::FormatIdentifier::Revealjs;
        let json = r#"{"runs": [], "identities": {}}"#;
        let provider: Arc<dyn crate::attribution::AttributionSourceProvider> =
            Arc::new(PreBuiltAttributionProvider::new(json.to_string()));
        let mut ctx = make_ctx(format, Some(provider));
        let input = PipelineData::DocumentAst(make_doc());
        let _ = AttributionGenerateStage::new()
            .run(input, &mut ctx)
            .await
            .unwrap();
        assert!(
            ctx.attribution_data.is_none(),
            "non-attribution-consuming format must skip"
        );
    }
}
