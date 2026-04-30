/*
 * pipeline.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Unified render pipeline for Quarto.
 */

//! Unified render pipeline.
//!
//! This module provides the core render pipeline used by both the CLI and WASM
//! clients. By using the same pipeline, we ensure feature parity between
//! different rendering contexts.
//!
//! ## Pipeline Stages
//!
//! 1. **Parse**: QMD source → Pandoc AST (via `pampa`)
//! 2. **Engine execution**: Execute code cells (Jupyter, Knitr, or markdown passthrough)
//! 3. **Transform**: Apply Quarto-specific transforms (callouts, metadata, etc.)
//! 4. **Render body**: Pandoc AST → HTML body (via `pampa`)
//! 5. **Apply template**: Wrap body with HTML template
//!
//! ## Usage
//!
//! The main entry point is the async [`render_qmd_to_html`] function:
//!
//! ```ignore
//! use quarto_core::pipeline::{render_qmd_to_html, HtmlRenderConfig};
//!
//! // Async usage (WASM or native async context)
//! let output = render_qmd_to_html(
//!     content.as_bytes(),
//!     "input.qmd",
//!     &mut render_ctx,
//!     &HtmlRenderConfig::default(),
//! ).await?;
//!
//! // Sync usage on native (CLI)
//! let output = pollster::block_on(render_qmd_to_html(
//!     content.as_bytes(),
//!     "input.qmd",
//!     &mut render_ctx,
//!     &HtmlRenderConfig::default(),
//! ))?;
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use quarto_error_reporting::DiagnosticMessage;
use quarto_pandoc_types::Pandoc;
use quarto_source_map::SourceContext;

use crate::Result;
use crate::render::RenderContext;
use crate::stage::CodeHighlightStage;
use crate::stage::stages::ApplyTemplateConfig;
use crate::stage::{
    ApplyTemplateStage, AstTransformsStage, CompileThemeCssStage, DocumentProfileStage,
    EngineExecutionStage, IncludeExpansionStage, LinkResolutionStage, LoadedSource,
    MetadataMergeStage, ParseDocumentStage, Pipeline, PipelineData, PipelineStage,
    PreEngineSugaringStage, RenderHtmlBodyStage, StageContext, UnwrapProfileStage,
    UserFiltersStage,
};
use crate::transform::TransformPipeline;
use crate::transforms::{
    AppendixStructureTransform, CalloutResolveTransform, CalloutTransform, CrossrefIndexTransform,
    CrossrefRenderTransform, CrossrefResolveTransform, EquationLabelTransform,
    FloatRefTargetSugarTransform, FooterGenerateTransform, FooterRenderTransform,
    FootnotesTransform, LinkRewriteTransform, MetadataNormalizeTransform, NavbarGenerateTransform,
    NavbarRenderTransform, PageNavGenerateTransform, PageNavRenderTransform, ProofSugarTransform,
    ResourceCollectorTransform, SectionizeTransform, ShortcodeResolveTransform,
    SidebarGenerateTransform, SidebarRenderTransform, TheoremSugarTransform, TitleBlockTransform,
    TocGenerateTransform, TocRenderTransform, WebsiteBootstrapIconsTransform,
    WebsiteCanonicalUrlTransform, WebsiteFaviconTransform, WebsiteTitlePrefixTransform,
};

/// Well-known path for the default CSS artifact in WASM context.
///
/// This path is used by both the render pipeline (to store the artifact)
/// and the browser post-processor (to resolve the CSS reference).
pub const DEFAULT_CSS_ARTIFACT_PATH: &str = "/.quarto/project-artifacts/styles.css";

/// Configuration for HTML rendering.
///
/// Phase 5: the legacy `css_paths` + `resource_prefix` pair has
/// been replaced by an optional [`ResourceResolverContext`].
/// When `resolver` is provided (CLI render via `render_to_file`,
/// project pipeline, or any caller that knows where the output
/// HTML will live on disk), every CSS / JS artifact in the store
/// gets its `<link>` / `<script>` URL computed by the resolver.
/// When `resolver` is absent (in-memory test renders), each
/// artifact's bare `path` is used verbatim.
#[derive(Debug, Default)]
pub struct HtmlRenderConfig {
    /// Scope-aware resolver passed through to
    /// [`ApplyTemplateConfig::resolver`]. See its docs.
    pub resolver: Option<crate::resource_resolver::ResourceResolverContext>,
}

impl HtmlRenderConfig {
    /// Create a new configuration with a resolver attached.
    pub fn with_resolver(resolver: crate::resource_resolver::ResourceResolverContext) -> Self {
        Self {
            resolver: Some(resolver),
        }
    }
}

/// Output from the render pipeline.
#[derive(Debug)]
pub struct RenderOutput {
    /// The rendered HTML content.
    pub html: String,
    /// Diagnostics (warnings, errors, info) collected during rendering.
    pub diagnostics: Vec<DiagnosticMessage>,
    /// Source context for mapping locations in diagnostics.
    pub source_context: SourceContext,
}

pub struct AstOutput {
    /// The AST serialized as JSON.
    pub ast: Pandoc,
    /// Non-fatal warnings collected during rendering.
    pub warnings: Vec<DiagnosticMessage>,
    /// Source context for mapping locations in diagnostics.
    pub source_context: SourceContext,
}

/// Build the standard HTML pipeline stages.
///
/// Returns the stages as a vector, allowing callers to customize before
/// creating the pipeline. For most uses, prefer [`build_html_pipeline`].
///
/// This creates stages for:
/// 1. `ParseDocumentStage` - Parse QMD to Pandoc AST
/// 2. `MetadataMergeStage` - Merge project/directory/document/runtime metadata
/// 3. `IncludeExpansionStage` - Splice in `{{< include child.qmd >}}` bodies
/// 4. `DocumentProfileStage` - Extract the static profile at the checkpoint
/// 5. `LinkResolutionStage` - Walk AST for cross-doc body-link targets (Phase 8)
/// 6. `UnwrapProfileStage` - Hand the AST back to downstream stages
/// 6. `PreEngineSugaringStage` - Seed crossref registry / desugar shorthand
/// 7. `EngineExecutionStage` - Execute code cells (jupyter, knitr, or markdown passthrough)
/// 8. `CompileThemeCssStage` - Compile theme CSS from merged metadata
/// 9. `UserFiltersStage::pre()` - Apply user filters before Quarto transforms
/// 10. `AstTransformsStage` - Run Quarto transforms (callouts, metadata, etc.)
/// 11. `UserFiltersStage::post()` - Apply user filters after Quarto transforms
/// 12. `CodeHighlightStage` - Annotate CodeBlock/Code with `data-hl-spans`
/// 13. `RenderHtmlBodyStage` - Render AST to HTML body
/// 14. `ApplyTemplateStage` - Apply HTML template
pub fn build_html_pipeline_stages() -> Vec<Box<dyn PipelineStage>> {
    build_html_pipeline_stages_with_apply_config(None)
}

/// Like [`build_html_pipeline_stages`], but allows the caller to supply
/// a customized [`ApplyTemplateConfig`] (e.g. CSS paths and resource
/// prefix from `render_to_file`). The rest of the pipeline — including
/// [`CodeHighlightStage`] — is identical to [`build_html_pipeline_stages`].
///
/// This helper exists so that the CLI render path (which needs custom
/// CSS paths) and the default in-memory path (which doesn't) share a
/// single source of truth for the stage list. Without it, the two
/// branches drift silently — in particular, a previous version of
/// `render_qmd_to_html` inlined its own stage vec for the CSS-paths
/// case and omitted the highlight stage, causing `quarto render` to
/// emit un-highlighted HTML while all in-process tests passed.
pub fn build_html_pipeline_stages_with_apply_config(
    apply_config: Option<ApplyTemplateConfig>,
) -> Vec<Box<dyn PipelineStage>> {
    let mut stages: Vec<Box<dyn PipelineStage>> = vec![
        Box::new(ParseDocumentStage::new()),
        Box::new(MetadataMergeStage::new()),
        // Include-shortcode expansion runs before the profile
        // checkpoint so content spliced in via `{{< include … >}}`
        // (headings, code blocks, crossref targets) is visible to
        // DocumentProfile — see bd-xfwx and
        // `claude-notes/plans/2026-04-24-include-expansion-merge.md`.
        Box::new(IncludeExpansionStage::new()),
        // Profile checkpoint: post-merge, pre-mutation. See
        // `claude-notes/designs/document-profile-contract.md`.
        Box::new(DocumentProfileStage::new()),
        // Pass-1 cross-doc body-link resolution. Walks the AST
        // (read-only) and writes each link target into
        // `profile.body_link_targets` so the Phase-8 dependency
        // graph can use them. See
        // `claude-notes/designs/body-link-resolution-contract.md`.
        Box::new(LinkResolutionStage::new()),
        Box::new(UnwrapProfileStage::new()),
        Box::new(PreEngineSugaringStage::new()),
        Box::new(EngineExecutionStage::new()),
        Box::new(CompileThemeCssStage::new()),
        Box::new(UserFiltersStage::pre()),
        Box::new(AstTransformsStage::new()),
        Box::new(UserFiltersStage::post()),
    ];
    stages.push(Box::new(CodeHighlightStage::new()));
    stages.push(Box::new(RenderHtmlBodyStage::new()));
    let apply_stage = match apply_config {
        Some(cfg) => ApplyTemplateStage::with_config(cfg),
        None => ApplyTemplateStage::new(),
    };
    stages.push(Box::new(apply_stage));
    stages
}

/// Build the standard HTML pipeline.
///
/// This creates a pipeline with the following stages:
/// 1. `ParseDocumentStage` - Parse QMD to Pandoc AST
/// 2. `MetadataMergeStage` - Merge project/directory/document/runtime metadata
/// 3. `EngineExecutionStage` - Execute code cells (jupyter, knitr, or markdown passthrough)
/// 4. `CompileThemeCssStage` - Compile theme CSS from merged metadata
/// 5. `UserFiltersStage::pre()` - Apply user filters before Quarto transforms
/// 6. `AstTransformsStage` - Run Quarto transforms (callouts, metadata, etc.)
/// 7. `UserFiltersStage::post()` - Apply user filters after Quarto transforms
/// 8. `RenderHtmlBodyStage` - Render AST to HTML body
/// 9. `ApplyTemplateStage` - Apply HTML template
///
/// # Returns
///
/// A validated `Pipeline` ready for execution.
///
/// # Panics
///
/// Panics if the pipeline stages have incompatible types (should never happen
/// with the standard stages).
pub fn build_html_pipeline() -> Pipeline {
    Pipeline::new(build_html_pipeline_stages()).expect("HTML pipeline stages should be compatible")
}

/// Build a WASM-compatible HTML pipeline (no engine execution).
///
/// This creates a pipeline suitable for browser environments where code
/// execution is not available. It includes all AST transforms for feature
/// parity with native rendering (callouts, TOC, sectionize, etc.), but
/// skips the engine execution stage.
///
/// Stages:
/// 1. `ParseDocumentStage` - Parse QMD to Pandoc AST
/// 2. `MetadataMergeStage` - Merge project/directory/document/runtime metadata
/// 3. `IncludeExpansionStage` - Splice in `{{< include child.qmd >}}` bodies
/// 4. `DocumentProfileStage` - Extract the static profile at the checkpoint
/// 5. `LinkResolutionStage` - Walk AST for cross-doc body-link targets (Phase 8)
/// 6. `UnwrapProfileStage` - Hand the AST back to downstream stages
/// 6. `CompileThemeCssStage` - Compile theme CSS from merged metadata
/// 7. `UserFiltersStage::pre()` - Apply user filters before Quarto transforms
/// 8. `AstTransformsStage` - Run Quarto transforms (callouts, metadata, TOC, etc.)
/// 9. `UserFiltersStage::post()` - Apply user filters after Quarto transforms
/// 10. `RenderHtmlBodyStage` - Render AST to HTML body
/// 11. `ApplyTemplateStage` - Apply HTML template
///
/// # Returns
///
/// A validated `Pipeline` ready for execution.
///
/// # Panics
///
/// Panics if the pipeline stages have incompatible types (should never happen
/// with the standard stages).
pub fn build_wasm_html_pipeline() -> Pipeline {
    let mut stages: Vec<Box<dyn PipelineStage>> = vec![
        Box::new(ParseDocumentStage::new()),
        // No EngineExecutionStage - code cells pass through as-is
        Box::new(MetadataMergeStage::new()),
        // Include expansion before the profile checkpoint — bd-xfwx.
        Box::new(IncludeExpansionStage::new()),
        // Profile checkpoint: post-merge, pre-mutation. Hub-client
        // Phase 9 will intercept this variant to build project-wide
        // nav state.
        Box::new(DocumentProfileStage::new()),
        // Pass-1 cross-doc body-link resolution (Phase 8 sub-phase 8.0d).
        Box::new(LinkResolutionStage::new()),
        Box::new(UnwrapProfileStage::new()),
        Box::new(PreEngineSugaringStage::new()),
        Box::new(CompileThemeCssStage::new()),
        Box::new(UserFiltersStage::pre()),
        Box::new(AstTransformsStage::new()),
        Box::new(UserFiltersStage::post()),
    ];
    stages.push(Box::new(CodeHighlightStage::new()));
    stages.push(Box::new(RenderHtmlBodyStage::new()));
    stages.push(Box::new(ApplyTemplateStage::new()));

    Pipeline::new(stages).expect("WASM HTML pipeline stages should be compatible")
}

/// Build an HTML pipeline from custom stages.
///
/// This allows full control over which stages are included in the pipeline.
/// Use this when you need a specialized pipeline configuration.
///
/// # Arguments
///
/// * `stages` - The stages to include in the pipeline
///
/// # Returns
///
/// A `Result` containing the validated `Pipeline`, or an error if the
/// stages have incompatible input/output types.
///
/// # Example
///
/// ```ignore
/// use quarto_core::pipeline::build_html_pipeline_with_stages;
/// use quarto_core::stage::{ParseDocumentStage, AstTransformsStage, RenderHtmlBodyStage};
///
/// // Build a minimal pipeline without template application
/// let stages: Vec<Box<dyn PipelineStage>> = vec![
///     Box::new(ParseDocumentStage::new()),
///     Box::new(AstTransformsStage::new()),
///     Box::new(RenderHtmlBodyStage::new()),
/// ];
/// let pipeline = build_html_pipeline_with_stages(stages)?;
/// ```
pub fn build_html_pipeline_with_stages(
    stages: Vec<Box<dyn PipelineStage>>,
) -> std::result::Result<Pipeline, crate::stage::PipelineValidationError> {
    Pipeline::new(stages)
}

/// Build the transform pipeline used by LSP-style document analysis.
///
/// This is the analysis-time equivalent of [`build_transform_pipeline`]. It
/// runs the minimal set of transforms needed to leave the AST in an
/// outline-ready state:
///
/// - Sugaring transforms (`Callout`, `Theorem`, `Proof`, `FloatRefTarget`,
///   `EquationLabel`) so `::: {#fig-…}` / `::: {#thm-…}` / `$$ … $$ {#eq-…}`
///   become canonical `CustomNode`s with `plain_data.ref_type`, `kind`, and
///   `identifier`.
/// - `CrossrefIndexTransform` so each target's `plain_data.order` carries
///   the section-scoped number that will appear in the rendered document.
///
/// **Deliberately omitted** (compared to [`build_transform_pipeline`]):
///
/// - `ShortcodeResolveTransform` — runs Lua, costly at LSP speed. Simple
///   `{{< meta key >}}` resolution is handled by the lightweight
///   `quarto_analysis::MetaShortcodeTransform` in `quarto-lsp-core`.
/// - `MetadataNormalizeTransform`, `TitleBlockTransform`, `SectionizeTransform`,
///   `FootnotesTransform` — render-shape transforms that don't affect the
///   outline.
/// - `CalloutResolveTransform` — converts callout custom nodes back into
///   render-visible Divs; the outline walker wants the custom-node form.
/// - `CrossrefResolveTransform` — rewrites `@fig-1` citations; not needed
///   for outline.
/// - TOC phase — the outline *is* our TOC; no need to build another one.
/// - Finalization phase (`AppendixStructure`, `CrossrefRender`,
///   `ResourceCollector`) — `CrossrefRender` would destroy the crossref
///   custom nodes we rely on; the others are render-only.
pub fn build_analysis_transform_pipeline() -> TransformPipeline {
    let mut pipeline: TransformPipeline = TransformPipeline::new();

    // Normalization (subset): sugaring transforms only.
    pipeline.push(Box::new(CalloutTransform::new()));
    pipeline.push(Box::new(TheoremSugarTransform::new()));
    pipeline.push(Box::new(ProofSugarTransform::new()));
    pipeline.push(Box::new(FloatRefTargetSugarTransform::new()));
    pipeline.push(Box::new(EquationLabelTransform::new()));

    // Crossref indexing for section-scoped numbering.
    pipeline.push(Box::new(CrossrefIndexTransform::new()));

    pipeline
}

/// Build a pipeline suitable for LSP-style document analysis (outline,
/// symbols, folding ranges, diagnostics) without any rendering, engine
/// execution, or user-filter side effects.
///
/// ## Stages
///
/// 1. [`ParseDocumentStage`] — QMD → Pandoc AST
/// 2. [`MetadataMergeStage`] — merge project / directory / document /
///    runtime metadata into `pandoc.meta`
/// 3. [`PreEngineSugaringStage`] — seed the [`RefTypeRegistry`] from
///    `crossref.custom` metadata, seed a [`CrossrefIndex`], desugar
///    code-block shorthand
/// 4. [`AstTransformsStage`] with the [`build_analysis_transform_pipeline`]
///    subset — apply sugaring + crossref indexing
///
/// After this pipeline runs, the AST is in its outline-ready state:
/// cross-referenceable blocks are `CustomNode`s with
/// `plain_data.{ref_type, kind, identifier, order}` populated, theorem
/// titles have been absorbed into their CustomNode's `title` slot, and
/// figure / table captions live in the `caption_long` / `caption_short`
/// slots.
///
/// [`RefTypeRegistry`]: crate::crossref::RefTypeRegistry
/// [`CrossrefIndex`]: crate::crossref::CrossrefIndex
pub fn build_analysis_pipeline() -> Pipeline {
    let stages: Vec<Box<dyn PipelineStage>> = vec![
        Box::new(ParseDocumentStage::new()),
        Box::new(MetadataMergeStage::new()),
        Box::new(IncludeExpansionStage::new()),
        Box::new(PreEngineSugaringStage::new()),
        Box::new(AstTransformsStage::with_pipeline(
            build_analysis_transform_pipeline(),
        )),
    ];

    Pipeline::new(stages).expect("analysis pipeline stages should be compatible")
}

pub async fn run_pipeline(
    content: &[u8],
    source_name: &str,
    ctx: &mut RenderContext<'_>,
    runtime: Arc<dyn quarto_system_runtime::SystemRuntime>,
    stages: Vec<Box<dyn PipelineStage>>,
) -> Result<(PipelineData, Vec<DiagnosticMessage>)> {
    // Create StageContext from RenderContext data
    let mut stage_ctx = StageContext::new(
        runtime,
        ctx.format.clone(),
        ctx.project.clone(),
        ctx.document.clone(),
    )
    .map_err(|e| crate::error::QuartoError::Other(e.to_string()))?;

    // Transfer artifacts from RenderContext to StageContext
    stage_ctx.artifacts = std::mem::take(&mut ctx.artifacts);
    // Transfer user-grammar provider (browser path sets this; native CLI
    // leaves it None and falls back to `CodeHighlightStage`'s disk scan).
    stage_ctx.user_grammar_provider = ctx.user_grammar_provider.take();
    // Transfer the project index (set by ProjectPipeline::pass_two).
    // Cloning the `Arc` is cheap and keeps the RenderContext usable
    // after the stage context is built.
    stage_ctx.project_index = ctx.project_index.clone();
    // Phase 6: thread the per-page resource resolver through to the
    // stage so that `AstTransformsStage` can re-bridge it back into
    // the inner `RenderContext` consumed by AST transforms (notably
    // `LinkRewriteTransform`).
    stage_ctx.resource_resolver = ctx.resource_resolver.clone();

    // Create input from content
    let input = PipelineData::LoadedSource(LoadedSource::new(
        PathBuf::from(source_name),
        content.to_vec(),
    ));

    let pipeline = Pipeline::new(stages).expect("Pipeline stages should be compatible");

    let result = pipeline.run(input, &mut stage_ctx).await;

    // Transfer artifacts back to RenderContext
    ctx.artifacts = stage_ctx.artifacts;

    result
        .map_err(|e| match e {
            crate::stage::PipelineError::StageError { diagnostics, .. }
                if !diagnostics.is_empty() =>
            {
                // Create a SourceContext for the parse error
                let mut source_context = SourceContext::new();
                let content_str = String::from_utf8_lossy(content).to_string();
                source_context.add_file(source_name.to_string(), Some(content_str));
                crate::error::QuartoError::Parse(crate::error::ParseError::new(
                    diagnostics,
                    source_context,
                ))
            }
            other => crate::error::QuartoError::Other(other.to_string()),
        })
        .map(|d| (d, stage_ctx.diagnostics))
}

pub async fn parse_qmd_to_ast(
    content: &[u8],
    source_name: &str,
    ctx: &mut RenderContext<'_>,
    runtime: Arc<dyn quarto_system_runtime::SystemRuntime>,
) -> Result<AstOutput> {
    // Build pipeline based on config
    // If custom CSS or template is specified, use a customized ApplyTemplateStage
    let stages: Vec<Box<dyn PipelineStage>> = vec![
        Box::new(ParseDocumentStage::new()),
        Box::new(EngineExecutionStage::new()),
        Box::new(MetadataMergeStage::new()),
    ];

    let (output, warnings) = run_pipeline(content, source_name, ctx, runtime, stages).await?;
    // Extract the rendered output
    let ast = output.into_document_ast().ok_or_else(|| {
        crate::error::QuartoError::Other("Pipeline did not produce ast".to_string())
    })?;

    // Create source context for the output
    let mut source_context = SourceContext::new();
    let content_str = String::from_utf8_lossy(content).to_string();
    source_context.add_file(source_name.to_string(), Some(content_str));

    Ok(AstOutput {
        ast: ast.ast,
        warnings,
        source_context,
    })
}

/// Render QMD content to HTML.
///
/// This is the unified async render pipeline used by both CLI and WASM. It:
/// 1. Parses the QMD content to a Pandoc AST
/// 2. Runs the transform pipeline (callouts, metadata normalization, etc.)
/// 3. Renders the AST to HTML body
/// 4. Applies the HTML template
///
/// # Arguments
///
/// * `content` - The QMD source content as bytes
/// * `source_name` - Name of the source file (for error messages)
/// * `ctx` - Render context containing project, document, format info
/// * `config` - HTML render configuration (CSS paths, template)
/// * `runtime` - System runtime for filesystem operations
///
/// # Returns
///
/// A `RenderOutput` containing the HTML and any collected artifacts.
///
/// # Errors
///
/// Returns an error if parsing fails, transforms fail, or rendering fails.
///
/// # Example
///
/// ```ignore
/// // WASM usage (async)
/// let output = render_qmd_to_html(
///     content, "input.qmd", &mut ctx, &config, runtime
/// ).await?;
///
/// // Native CLI usage (sync via pollster)
/// let output = pollster::block_on(render_qmd_to_html(
///     content, "input.qmd", &mut ctx, &config, runtime
/// ))?;
/// ```
pub async fn render_qmd_to_html(
    content: &[u8],
    source_name: &str,
    ctx: &mut RenderContext<'_>,
    config: &HtmlRenderConfig,
    runtime: Arc<dyn quarto_system_runtime::SystemRuntime>,
) -> Result<RenderOutput> {
    // Build pipeline based on config. Both branches share the same
    // stage list (via `build_html_pipeline_stages_with_apply_config`);
    // the only difference is whether the final `ApplyTemplateStage`
    // carries a scope-aware resolver.
    let stages = if let Some(resolver) = config.resolver.clone() {
        let apply_config = ApplyTemplateConfig::new().with_resolver(resolver);
        build_html_pipeline_stages_with_apply_config(Some(apply_config))
    } else {
        build_html_pipeline_stages()
    };

    let (output, diagnostics) = run_pipeline(content, source_name, ctx, runtime, stages).await?;
    // Extract the rendered output
    let rendered = output.into_rendered_output().ok_or_else(|| {
        crate::error::QuartoError::Other("Pipeline did not produce RenderedOutput".to_string())
    })?;

    // Create source context for the output
    let mut source_context = SourceContext::new();
    let content_str = String::from_utf8_lossy(content).to_string();
    source_context.add_file(source_name.to_string(), Some(content_str));

    Ok(RenderOutput {
        html: rendered.content,
        diagnostics,
        source_context,
    })
}

/// Build the standard transform pipeline.
///
/// The transforms are applied in this order:
///
/// ## Normalization Phase
/// 1. `CalloutTransform` - Convert callout Divs to CustomNodes
/// 2. `CalloutResolveTransform` - Resolve CustomNodes to structured Divs
/// 3. `ShortcodeResolveTransform` - Resolve shortcodes (e.g., `{{< meta title >}}`)
/// 4. `MetadataNormalizeTransform` - Add derived metadata (pagetitle, etc.)
/// 4a. `WebsiteTitlePrefixTransform` - Combine `website.title` with the page's title
///     into the rendered `<title>` (Phase 7)
/// 4b. `WebsiteFaviconTransform` - Append `<link rel="icon">` for `website.favicon`
///     to the page's `header-includes` (Phase 7)
/// 4b'. `WebsiteBootstrapIconsTransform` - For website projects, ship the
///     vendored `bootstrap-icons.{css,woff}` to `_site/site_libs/bootstrap/`
///     and append a `<link rel="stylesheet">` so `bi-*` icons render (bd-bsut)
/// 4c. `WebsiteCanonicalUrlTransform` - Set `canonical-url` from
///     `website.site-url + output_href` (Phase 7)
/// 5. `TitleBlockTransform` - Add title header from metadata if not present
/// 6. `SectionizeTransform` - Wrap headers in section Divs (for HTML semantic structure)
/// 7. `FootnotesTransform` - Extract footnotes and create footnotes section
/// 8. `FloatRefTargetSugarTransform` - Wrap float crossref Divs / Figures in canonical CustomNode
///
/// ## Navigation Phase
///
/// All `Generate` transforms run first so that by the time any renderer sees
/// `ast.meta.navigation.*`, every structured subtree is populated. This keeps
/// the door open for user filters (between generate and render) or future
/// non-HTML pipelines (slideshows, dashboards) that need the structured data
/// but emit different HTML.
///
/// 9. `TocGenerateTransform` - Generate TOC from headers (if `toc: true`)
/// 10. `NavbarGenerateTransform` - Resolve `navbar:` YAML into `navigation.navbar`
/// 11. `SidebarGenerateTransform` - Resolve `website.sidebar:` into `navigation.sidebar`
/// 12. `FooterGenerateTransform` - Resolve `page-footer:` YAML into `navigation.footer`
/// 13. `TocRenderTransform` - Render TOC to HTML for template insertion
/// 14. `NavbarRenderTransform` - Render navbar to HTML for template insertion
/// 15. `SidebarRenderTransform` - Render sidebar to HTML (w/ .qmd→.html rewrite)
/// 16. `FooterRenderTransform` - Render page footer to HTML for template insertion
///
/// ## Finalization Phase
/// 17. `LinkRewriteTransform` - Rewrite body-content `.qmd` links to relative output URLs (Phase 6)
/// 18. `AppendixStructureTransform` - Consolidate appendix content into container
/// 19. `CrossrefRenderTransform` - Resolve crossref custom nodes to final HTML structure
/// 20. `ResourceCollectorTransform` - Collect image dependencies
pub fn build_transform_pipeline(
    shortcode_paths: Vec<std::path::PathBuf>,
    extensions: Vec<crate::extension::types::Extension>,
    runtime: std::sync::Arc<dyn quarto_system_runtime::SystemRuntime>,
    target_format: String,
) -> TransformPipeline {
    let mut pipeline: TransformPipeline = TransformPipeline::new();

    // === NORMALIZATION PHASE ===
    pipeline.push(Box::new(CalloutTransform::new()));
    pipeline.push(Box::new(CalloutResolveTransform::new()));
    pipeline.push(Box::new(ShortcodeResolveTransform::with_lua_support(
        shortcode_paths,
        extensions,
        runtime,
        target_format,
    )));
    pipeline.push(Box::new(MetadataNormalizeTransform::new()));
    // Website per-page metadata transforms (Phase 7 of the
    // website-projects epic). Each is a no-op outside a website
    // project. Order: title-prefix runs before favicon/canonical
    // because the latter two read fields the former might modify
    // in the future; today they're independent.
    // See `claude-notes/plans/2026-04-27-websites-phase-7.md`
    // §Decision 3.
    pipeline.push(Box::new(WebsiteTitlePrefixTransform::new()));
    pipeline.push(Box::new(WebsiteFaviconTransform::new()));
    pipeline.push(Box::new(WebsiteBootstrapIconsTransform::new()));
    pipeline.push(Box::new(WebsiteCanonicalUrlTransform::new()));
    pipeline.push(Box::new(TitleBlockTransform::new()));
    pipeline.push(Box::new(SectionizeTransform::new()));
    pipeline.push(Box::new(FootnotesTransform::new()));
    // TheoremSugarTransform / ProofSugarTransform run before
    // FloatRefTargetSugarTransform so `Div(#thm-foo .theorem)` and
    // `Div(.proof)` become Theorem / Proof custom nodes first; the
    // float-target classifier only sees plain `Div` blocks.
    pipeline.push(Box::new(TheoremSugarTransform::new()));
    pipeline.push(Box::new(ProofSugarTransform::new()));
    pipeline.push(Box::new(FloatRefTargetSugarTransform::new()));
    pipeline.push(Box::new(EquationLabelTransform::new()));

    // === CROSSREF PHASE ===
    pipeline.push(Box::new(CrossrefIndexTransform::new()));
    pipeline.push(Box::new(CrossrefResolveTransform::new()));

    // === NAVIGATION PHASE ===
    // All generates run before any renders so a future user filter or
    // non-HTML pipeline sees a complete navigation.* subtree before rendering.
    // TocGenerate must run after SectionizeTransform so section IDs are
    // available; navbar/footer generates only read top-level metadata.
    pipeline.push(Box::new(TocGenerateTransform::new()));
    pipeline.push(Box::new(NavbarGenerateTransform::new()));
    pipeline.push(Box::new(SidebarGenerateTransform::new()));
    // PageNavGenerate must run after SidebarGenerate so it reads the
    // resolved `navigation.sidebar` for the current page.
    pipeline.push(Box::new(PageNavGenerateTransform::new()));
    pipeline.push(Box::new(FooterGenerateTransform::new()));
    pipeline.push(Box::new(TocRenderTransform::new()));
    pipeline.push(Box::new(NavbarRenderTransform::new()));
    pipeline.push(Box::new(SidebarRenderTransform::new()));
    pipeline.push(Box::new(PageNavRenderTransform::new()));
    pipeline.push(Box::new(FooterRenderTransform::new()));

    // === FINALIZATION PHASE ===
    // LinkRewriteTransform runs first in the Finalization Phase
    // (Phase 6 of the website-projects epic). It walks every
    // `Inline::Link` in the body and rewrites internal `.qmd`
    // hrefs to their page-relative output URLs via the
    // `ProjectIndex` and `ResourceResolverContext`. Standalone
    // renders without a `ProjectIndex` are a no-op. See
    // `claude-notes/plans/2026-04-24-websites-phase-6.md`.
    pipeline.push(Box::new(LinkRewriteTransform::new()));
    pipeline.push(Box::new(AppendixStructureTransform::new()));
    pipeline.push(Box::new(CrossrefRenderTransform::new()));
    pipeline.push(Box::new(ResourceCollectorTransform::new()));

    pipeline
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectContext};
    use crate::render::BinaryDependencies;
    use std::path::PathBuf;

    fn make_test_project() -> ProjectContext {
        ProjectContext {
            dir: PathBuf::from("/project"),
            config: crate::project::ProjectConfig::default(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/test.qmd")],
            output_dir: PathBuf::from("/project"),
        }
    }

    fn make_test_runtime() -> Arc<dyn quarto_system_runtime::SystemRuntime> {
        Arc::new(quarto_system_runtime::NativeRuntime::new())
    }

    #[test]
    fn test_render_simple_document() {
        let content = b"---\ntitle: Test\n---\n\nHello, world!";

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let config = HtmlRenderConfig::default();
        let runtime = make_test_runtime();
        let output = pollster::block_on(render_qmd_to_html(
            content, "test.qmd", &mut ctx, &config, runtime,
        ))
        .unwrap();

        assert!(output.html.contains("Hello, world!"));
        assert!(output.html.contains("<!DOCTYPE html>"));
        assert!(output.html.contains("<title>Test</title>"));
    }

    #[test]
    fn test_render_with_callout() {
        let content =
            b"---\ntitle: Test\n---\n\n::: {.callout-warning}\n## Watch Out\nBe careful!\n:::";

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let config = HtmlRenderConfig::default();
        let runtime = make_test_runtime();
        let output = pollster::block_on(render_qmd_to_html(
            content, "test.qmd", &mut ctx, &config, runtime,
        ))
        .unwrap();

        // Verify callout was transformed
        assert!(output.html.contains("callout"));
        assert!(output.html.contains("callout-warning"));
        assert!(output.html.contains("Watch Out"));
        assert!(output.html.contains("Be careful!"));
    }

    #[test]
    fn test_render_code_block_is_syntax_highlighted() {
        // Full-pipeline end-to-end: a Python code block should be
        // annotated by `CodeHighlightStage` and rendered with nested
        // `<span class="hl-*">` tags by the HTML writer.
        let content =
            b"---\ntitle: Test\n---\n\n```python\ndef greet(name):\n    print(name)\n```\n";

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let config = HtmlRenderConfig::default();
        let runtime = make_test_runtime();
        let output = pollster::block_on(render_qmd_to_html(
            content, "test.qmd", &mut ctx, &config, runtime,
        ))
        .unwrap();

        // The annotation stage should have run and the HTML writer
        // should have consumed `data-hl-spans` into nested spans.
        assert!(
            output
                .html
                .contains("<span class=\"hl-keyword\">def</span>"),
            "expected hl-keyword span around `def`; got:\n{}",
            &output.html,
        );
        assert!(
            output
                .html
                .contains("<span class=\"hl-function-builtin\">print</span>"),
            "expected hl-function-builtin span around `print`; got:\n{}",
            &output.html,
        );

        // The raw `data-hl-spans` attribute must not leak to the container.
        assert!(
            !output.html.contains("data-hl-spans="),
            "container should not carry the raw data-hl-spans attr; got:\n{}",
            &output.html,
        );

        // The `.sourceCode` marker should be present so default themes
        // + user themes can key off it.
        assert!(
            output.html.contains("sourceCode"),
            "pre/code container should carry the `sourceCode` class",
        );
    }

    #[test]
    fn test_render_with_meta_shortcode() {
        let content = b"---\ntitle: My Document Title\nauthor: Jane Doe\n---\n\nThe title is {{< meta title >}} by {{< meta author >}}.";

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let config = HtmlRenderConfig::default();
        let runtime = make_test_runtime();
        let output = pollster::block_on(render_qmd_to_html(
            content, "test.qmd", &mut ctx, &config, runtime,
        ))
        .unwrap();

        // Verify shortcodes were resolved
        assert!(output.html.contains("My Document Title"));
        assert!(output.html.contains("Jane Doe"));
        // Shortcode syntax should not appear in output
        assert!(!output.html.contains("{{<"));
        assert!(!output.html.contains(">}}"));
    }

    #[test]
    fn test_render_with_nested_meta_shortcode() {
        // Use simple text without @ symbols to avoid citation parsing
        let content = b"---\ntitle: Test\nauthor:\n  name: John Smith\n  location: New York\n---\n\nContact: {{< meta author.name >}} in {{< meta author.location >}}.";

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let config = HtmlRenderConfig::default();
        let runtime = make_test_runtime();
        let output = pollster::block_on(render_qmd_to_html(
            content, "test.qmd", &mut ctx, &config, runtime,
        ))
        .unwrap();

        // Verify nested metadata was resolved
        assert!(output.html.contains("John Smith"));
        assert!(output.html.contains("New York"));
    }

    #[test]
    fn test_render_with_missing_meta_key() {
        let content = b"---\ntitle: Test\n---\n\nMissing: {{< meta nonexistent >}}.";

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let config = HtmlRenderConfig::default();
        let runtime = make_test_runtime();
        let output = pollster::block_on(render_qmd_to_html(
            content, "test.qmd", &mut ctx, &config, runtime,
        ))
        .unwrap();

        // Verify error output is visible (TS Quarto style: "?meta:key" in bold)
        assert!(output.html.contains("?meta:nonexistent"));
        // Should have a diagnostic
        assert!(!output.diagnostics.is_empty());
    }

    #[test]
    fn test_render_with_escaped_shortcode() {
        let content = b"---\ntitle: Test\n---\n\nShow literal: {{{< meta title >}}}.";

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let config = HtmlRenderConfig::default();
        let runtime = make_test_runtime();
        let output = pollster::block_on(render_qmd_to_html(
            content, "test.qmd", &mut ctx, &config, runtime,
        ))
        .unwrap();

        // Escaped shortcode should render as literal text (without the extra braces)
        assert!(output.html.contains("{{&lt; meta title &gt;}}"));
    }

    #[test]
    fn test_render_emits_theme_css_link_via_resolver() {
        // Phase 5: the theme CSS comes from the
        // `css:theme:<fingerprint>` artifact stored by
        // `CompileThemeCssStage`. With a `single_doc` resolver
        // attached, its URL appears as
        // `<output_stem>_files/styles.css` in the rendered HTML.
        let content = b"---\ntitle: Test\n---\n\nContent";

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let resolver = crate::resource_resolver::ResourceResolverContext::single_doc(
            "/project/test.html",
            "test",
        );
        let config = HtmlRenderConfig::with_resolver(resolver);
        let runtime = make_test_runtime();
        let output = pollster::block_on(render_qmd_to_html(
            content, "test.qmd", &mut ctx, &config, runtime,
        ))
        .unwrap();

        assert!(
            output.html.contains("test_files/styles.css"),
            "expected `<link href=\"test_files/styles.css\">` from the resolver; got:\n{}",
            &output.html,
        );
    }

    #[test]
    fn test_render_code_block_is_syntax_highlighted_via_resolver() {
        // Regression test for the CLI render path: `render_document_to_file`
        // routes through the "resolver-attached ApplyTemplateStage"
        // branch of `render_qmd_to_html`. A previous version of that
        // branch inlined its own stage list and silently omitted
        // `CodeHighlightStage`, so `quarto render` emitted
        // un-highlighted HTML even though the default-config test
        // passed. Phase 5 keeps both branches sharing the same stage
        // list via `build_html_pipeline_stages_with_apply_config`;
        // this test pins the highlighting still works under the
        // resolver-attached config.
        let content =
            b"---\ntitle: Test\n---\n\n```python\ndef greet(name):\n    print(name)\n```\n";

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let resolver = crate::resource_resolver::ResourceResolverContext::single_doc(
            "/project/test.html",
            "test",
        );
        let config = HtmlRenderConfig::with_resolver(resolver);
        let runtime = make_test_runtime();
        let output = pollster::block_on(render_qmd_to_html(
            content, "test.qmd", &mut ctx, &config, runtime,
        ))
        .unwrap();

        assert!(
            output
                .html
                .contains("<span class=\"hl-keyword\">def</span>"),
            "expected hl-keyword span around `def` on the CLI path; got:\n{}",
            &output.html,
        );
        assert!(
            output
                .html
                .contains("<span class=\"hl-function-builtin\">print</span>"),
            "expected hl-function-builtin span around `print` on the CLI path; got:\n{}",
            &output.html,
        );
        assert!(
            !output.html.contains("data-hl-spans="),
            "container should not carry the raw data-hl-spans attr; got:\n{}",
            &output.html,
        );
    }

    #[test]
    #[ignore = "pampa parser is too forgiving - need to find input that produces parse error"]
    fn test_parse_error_has_structured_diagnostics() {
        // NOTE: This test is ignored because pampa's parser is very forgiving
        // and doesn't produce parse errors for most malformed inputs.
        // The YAML parser panics on malformed YAML instead of returning errors.
        // TODO: Find a way to test parse error propagation
        let content = b"---\ntitle: Test\n---\n\nSome content";

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/about.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let config = HtmlRenderConfig::default();
        let runtime = make_test_runtime();
        let result = pollster::block_on(render_qmd_to_html(
            content,
            "about.qmd",
            &mut ctx,
            &config,
            runtime,
        ));

        // Should fail with a parse error
        assert!(result.is_err());

        // The error should be a Parse error with diagnostics
        if let Err(crate::error::QuartoError::Parse(parse_error)) = result {
            // Should have at least one diagnostic
            assert!(
                !parse_error.diagnostics.is_empty(),
                "Parse error should contain diagnostics"
            );
        } else {
            panic!("Expected QuartoError::Parse, got {:?}", result);
        }
    }

    // === Pipeline builder tests ===

    #[test]
    fn test_build_html_pipeline_stages() {
        let stages = build_html_pipeline_stages();
        assert_eq!(stages.len(), 15);
        assert_eq!(stages[0].name(), "parse-document");
        assert_eq!(stages[1].name(), "metadata-merge");
        // Include expansion runs before the profile checkpoint (bd-xfwx)
        // so profiles reflect content spliced in via `{{< include ... >}}`.
        assert_eq!(stages[2].name(), "include-expansion");
        // Profile checkpoint (Phase 0 website epic, bd-f3jc).
        assert_eq!(stages[3].name(), "document-profile");
        // Cross-doc body-link resolution (Phase 8 sub-phase 8.0d).
        assert_eq!(stages[4].name(), "link-resolution");
        assert_eq!(stages[5].name(), "unwrap-profile");
        assert_eq!(stages[6].name(), "pre-engine-sugaring");
        assert_eq!(stages[7].name(), "engine-execution");
        assert_eq!(stages[8].name(), "compile-theme-css");
        assert_eq!(stages[9].name(), "user-filters-pre");
        assert_eq!(stages[10].name(), "ast-transforms");
        assert_eq!(stages[11].name(), "user-filters-post");
        assert_eq!(stages[12].name(), "code-highlight");
        assert_eq!(stages[13].name(), "render-html-body");
        assert_eq!(stages[14].name(), "apply-template");
    }

    #[test]
    fn test_build_html_pipeline() {
        let pipeline = build_html_pipeline();
        assert_eq!(pipeline.len(), 15);
    }

    #[test]
    fn test_build_wasm_html_pipeline() {
        let pipeline = build_wasm_html_pipeline();
        // WASM pipeline now has 14 stages: same as the native HTML
        // pipeline (include-expansion, profile checkpoint, link-resolution,
        // code-highlight, …) minus `engine-execution`.
        assert_eq!(pipeline.len(), 14);
    }

    #[test]
    fn test_build_analysis_pipeline() {
        use crate::stage::PipelineDataKind;

        let pipeline = build_analysis_pipeline();
        // Parse + MetadataMerge + IncludeExpansion + PreEngineSugaring + AstTransforms(analysis subset)
        assert_eq!(pipeline.len(), 5);
        assert_eq!(pipeline.expected_input(), PipelineDataKind::LoadedSource);
        assert_eq!(pipeline.expected_output(), PipelineDataKind::DocumentAst);
    }

    #[test]
    fn test_build_analysis_transform_pipeline_ordering() {
        // Lock in the order: sugaring before crossref indexing. The indexer
        // relies on sugared CustomNodes carrying plain_data.{ref_type, kind,
        // identifier} — if any sugar transform moves past the indexer the
        // outline will lose numbers for that ref type.
        let pipeline = build_analysis_transform_pipeline();
        let names: Vec<&str> = pipeline.iter().map(|t| t.name()).collect();

        let index_pos = names
            .iter()
            .position(|&n| n == "crossref-index")
            .expect("crossref-index must be in analysis pipeline");
        let theorem_pos = names
            .iter()
            .position(|&n| n == "theorem-sugar")
            .expect("theorem-sugar must be in analysis pipeline");
        let float_pos = names
            .iter()
            .position(|&n| n == "float-ref-target-sugar")
            .expect("float-ref-target-sugar must be in analysis pipeline");
        let equation_pos = names
            .iter()
            .position(|&n| n == "equation-label")
            .expect("equation-label must be in analysis pipeline");

        assert!(theorem_pos < index_pos);
        assert!(float_pos < index_pos);
        assert!(equation_pos < index_pos);

        // CrossrefRenderTransform must NOT be in the analysis pipeline — it
        // replaces crossref custom nodes with render-visible shapes, which
        // would make the outline walker's job impossible.
        assert!(!names.contains(&"crossref-render"));
    }

    #[test]
    fn test_build_html_pipeline_with_stages() {
        use crate::stage::PipelineDataKind;

        let stages: Vec<Box<dyn PipelineStage>> = vec![
            Box::new(ParseDocumentStage::new()),
            Box::new(AstTransformsStage::new()),
            Box::new(RenderHtmlBodyStage::new()),
        ];

        let result = build_html_pipeline_with_stages(stages);
        assert!(result.is_ok());

        let pipeline = result.unwrap();
        assert_eq!(pipeline.len(), 3);
        assert_eq!(pipeline.expected_input(), PipelineDataKind::LoadedSource);
        assert_eq!(pipeline.expected_output(), PipelineDataKind::RenderedOutput);
    }

    #[test]
    fn test_build_html_pipeline_with_stages_invalid() {
        // Try to create a pipeline with incompatible consecutive stages
        // ParseDocumentStage outputs DocumentAst, but ApplyTemplateStage expects RenderedOutput
        let stages: Vec<Box<dyn PipelineStage>> = vec![
            Box::new(ParseDocumentStage::new()),
            Box::new(ApplyTemplateStage::new()), // Expects RenderedOutput, not DocumentAst
        ];

        let result = build_html_pipeline_with_stages(stages);
        assert!(result.is_err());
    }

    // === Theme CSS integration tests ===

    use crate::project::ProjectConfig;
    use crate::resources::DEFAULT_CSS;
    use quarto_pandoc_types::{ConfigMapEntry, ConfigValue, ConfigValueKind};
    use quarto_source_map::SourceInfo;
    use yaml_rust2::Yaml;

    fn project_with_theme(theme: &str) -> ProjectContext {
        let theme_value = ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::String(theme.to_string())),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };
        let entry = ConfigMapEntry {
            key: "theme".to_string(),
            key_source: SourceInfo::default(),
            value: theme_value,
        };
        let metadata = ConfigValue {
            value: ConfigValueKind::Map(vec![entry]),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };
        ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::with_metadata(metadata),
            is_single_file: false,
            files: vec![DocumentInfo::from_path("/project/test.qmd")],
            output_dir: PathBuf::from("/project"),
        }
    }

    fn get_css_artifact(ctx: &crate::render::RenderContext) -> String {
        // Phase 5: theme CSS is now keyed `css:theme:<fingerprint>`
        // (one entry per distinct compiled theme).
        let entries: Vec<_> = ctx.artifacts.get_by_prefix("css:theme:");
        assert_eq!(
            entries.len(),
            1,
            "expected exactly one css:theme:* artifact, found {}",
            entries.len()
        );
        String::from_utf8(entries[0].1.content.clone()).expect("CSS should be valid UTF-8")
    }

    #[test]
    fn test_render_pipeline_theme_from_project() {
        let content = b"---\ntitle: Test\n---\n\nContent.";

        let project = project_with_theme("darkly");
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let config = HtmlRenderConfig::default();
        let runtime = make_test_runtime();
        let _output = pollster::block_on(render_qmd_to_html(
            content, "test.qmd", &mut ctx, &config, runtime,
        ))
        .unwrap();

        let css = get_css_artifact(&ctx);
        assert_ne!(css, DEFAULT_CSS, "should not be default CSS");
        assert!(
            css.contains("#375a7f"),
            "darkly theme should contain primary color #375a7f"
        );
    }

    #[test]
    fn test_render_pipeline_theme_from_document_overrides_project() {
        // Project has darkly, document has flatly — document should win
        let content = b"---\ntitle: Test\ntheme: flatly\n---\n\nContent.";

        let project = project_with_theme("darkly");
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let config = HtmlRenderConfig::default();
        let runtime = make_test_runtime();
        let _output = pollster::block_on(render_qmd_to_html(
            content, "test.qmd", &mut ctx, &config, runtime,
        ))
        .unwrap();

        let css = get_css_artifact(&ctx);
        assert!(
            css.contains("#2c3e50"),
            "flatly theme should contain primary color #2c3e50"
        );
        assert!(
            !css.contains("#375a7f"),
            "darkly primary color should not be present"
        );
    }

    #[test]
    fn test_render_pipeline_no_theme_compiles_default_bootstrap() {
        // Q1 parity: missing `theme:` compiles the default Bootstrap +
        // Quarto customization layer so navbar / footer / TOC CSS classes
        // are available out of the box. The old static-DEFAULT_CSS path is
        // now reached only via an explicit `theme: none` opt-out.
        let content = b"---\ntitle: Test\n---\n\nContent.";

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let config = HtmlRenderConfig::default();
        let runtime = make_test_runtime();
        let _output = pollster::block_on(render_qmd_to_html(
            content, "test.qmd", &mut ctx, &config, runtime,
        ))
        .unwrap();

        let css = get_css_artifact(&ctx);
        assert_ne!(
            css, DEFAULT_CSS,
            "no theme should compile Bootstrap, not ship static DEFAULT_CSS"
        );
        assert!(
            css.contains(".navbar"),
            "compiled default CSS should contain Bootstrap .navbar"
        );
    }

    #[test]
    fn test_render_pipeline_theme_none_opts_out_of_bootstrap() {
        let content = b"---\ntitle: Test\ntheme: none\n---\n\nContent.";

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let config = HtmlRenderConfig::default();
        let runtime = make_test_runtime();
        let _output = pollster::block_on(render_qmd_to_html(
            content, "test.qmd", &mut ctx, &config, runtime,
        ))
        .unwrap();

        let css = get_css_artifact(&ctx);
        assert_eq!(
            css, DEFAULT_CSS,
            "`theme: none` must ship the static DEFAULT_CSS (no Bootstrap)"
        );
    }
}
