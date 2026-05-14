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
#[cfg(not(target_arch = "wasm32"))]
use crate::stage::stages::BootstrapJsStage;
use crate::stage::{
    ApplyTemplateStage, AstTransformsStage, CompileThemeCssStage, DocumentProfileStage,
    EngineExecutionStage, IncludeExpansionStage, IncludeResolveStage, LinkResolutionStage,
    ListingItemInfoStage, LoadedSource, MathJsStage, MetadataMergeStage, ParseDocumentStage,
    Pipeline, PipelineData, PipelineStage, PreEngineSugaringStage, RenderHtmlBodyStage,
    ResourceReportStage, StageContext, UnwrapProfileStage, UserFiltersStage,
};
use crate::transform::TransformPipeline;
use crate::transforms::{
    AppendixStructureTransform, CalloutResolveTransform, CalloutTransform,
    CategoriesSidebarTransform, CrossrefIndexTransform, CrossrefRenderTransform,
    CrossrefResolveTransform, EquationLabelTransform, FloatRefTargetSugarTransform,
    FooterGenerateTransform, FooterRenderTransform, FootnotesTransform, LinkRewriteTransform,
    ListingGenerateTransform, ListingRenderTransform, MetadataNormalizeTransform,
    NavbarGenerateTransform, NavbarRenderTransform, PageNavGenerateTransform,
    PageNavRenderTransform, ProofSugarTransform, ResourceCollectorTransform, SectionizeTransform,
    ShortcodeResolveTransform, SidebarGenerateTransform, SidebarRenderTransform,
    TheoremSugarTransform, TitleBlockTransform, TocGenerateTransform, TocRenderTransform,
    WebsiteBootstrapIconsTransform, WebsiteCanonicalUrlTransform, WebsiteFaviconTransform,
    WebsiteTitlePrefixTransform,
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

    /// Engine registry override for the pipeline's
    /// [`EngineExecutionStage`] (bd-45yw, replay activation).
    ///
    /// `None` (the default) means: use the standard registry that the
    /// stage builds via `EngineRegistry::new()` — markdown plus, on
    /// native, knitr and jupyter.
    ///
    /// `Some(registry)` substitutes the supplied registry. The
    /// orchestrator/CLI's replay path constructs this via
    /// [`crate::engine::EngineRegistry::with_replay`] from a
    /// [`quarto_trace::EngineCapture`] loaded from a trace file.
    pub engine_registry: Option<crate::engine::EngineRegistry>,
}

impl HtmlRenderConfig {
    /// Create a new configuration with a resolver attached.
    pub fn with_resolver(resolver: crate::resource_resolver::ResourceResolverContext) -> Self {
        Self {
            resolver: Some(resolver),
            engine_registry: None,
        }
    }

    /// Attach an engine registry override (bd-45yw replay activation).
    pub fn with_engine_registry(mut self, registry: crate::engine::EngineRegistry) -> Self {
        self.engine_registry = Some(registry);
        self
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

/// Output of [`render_qmd_to_preview_ast`] — the q2-preview entry-point
/// sibling of [`render_qmd_to_html`].
///
/// Carries the **already-serialized** AST JSON (not the typed
/// `Pandoc`) so the renderer can plumb it straight into
/// `Pass2Payload::AstJson` without re-running the JSON writer.
/// Compared to [`AstOutput`], `PreviewAstOutput` skips the typed
/// `Pandoc` field — the q2-preview pipeline runs the full transform
/// pipeline, which mutates the AST extensively, so the typed value
/// is no longer interesting to callers; only the serialized form is.
#[derive(Debug)]
pub struct PreviewAstOutput {
    /// The transformed Pandoc AST, serialized as JSON via
    /// `pampa::writers::json::write_with_config` with
    /// `include_inline_locations: true`. Ready to ship to the
    /// React iframe.
    pub ast_json: String,
    /// Diagnostics emitted by the head pipeline plus every Pass-2
    /// stage that ran. Pipe to `RenderResponse.warnings` after
    /// translation via `diagnostics_to_json`.
    pub diagnostics: Vec<DiagnosticMessage>,
    /// Source-context handle for translating diagnostic offsets
    /// into line/column positions on the JS side. Same shape as
    /// [`RenderOutput::source_context`].
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
/// 4. `IncludeResolveStage` - Resolve `include-in-header` etc. authored keys
/// 5. `ListingItemInfoStage` - Auto-fill `meta.listing-item.*` (L1, `bd-izqh`)
/// 6. `DocumentProfileStage` - Extract the static profile at the checkpoint
/// 7. `LinkResolutionStage` - Walk AST for cross-doc body-link targets (Phase 8)
/// 8. `UnwrapProfileStage` - Hand the AST back to downstream stages
/// 9. `PreEngineSugaringStage` - Seed crossref registry / desugar shorthand
/// 10. `EngineExecutionStage` - Execute code cells (jupyter, knitr, or markdown passthrough)
/// 11. `CompileThemeCssStage` - Compile theme CSS from merged metadata
/// 12. `UserFiltersStage::pre()` - Apply user filters before Quarto transforms
/// 13. `AstTransformsStage` - Run Quarto transforms (callouts, metadata, etc.)
/// 14. `UserFiltersStage::post()` - Apply user filters after Quarto transforms
/// 15. `CodeHighlightStage` - Annotate CodeBlock/Code with `data-hl-spans`
/// 16. `RenderHtmlBodyStage` - Render AST to HTML body
/// 17. `ApplyTemplateStage` - Apply HTML template
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
    build_html_pipeline_stages_with_options(apply_config, None)
}

/// Like [`build_html_pipeline_stages_with_apply_config`], but also
/// accepts an optional [`crate::engine::EngineRegistry`] override for
/// the [`EngineExecutionStage`].
///
/// When `engine_registry` is `Some`, the stage is constructed via
/// [`EngineExecutionStage::with_registry`] using the caller's
/// registry — this is the seam the orchestrator/CLI replay path uses
/// to substitute a [`crate::engine::ReplayEngine`] without touching
/// the rest of the pipeline (bd-45yw).
///
/// When `engine_registry` is `None`, the stage builds its own default
/// registry (markdown + native engines), preserving pre-bd-45yw
/// behavior for every existing call site.
pub fn build_html_pipeline_stages_with_options(
    apply_config: Option<ApplyTemplateConfig>,
    engine_registry: Option<crate::engine::EngineRegistry>,
) -> Vec<Box<dyn PipelineStage>> {
    let engine_stage = match engine_registry {
        Some(reg) => EngineExecutionStage::with_registry(reg),
        None => EngineExecutionStage::new(),
    };
    let mut stages: Vec<Box<dyn PipelineStage>> = vec![
        Box::new(ParseDocumentStage::new()),
        Box::new(MetadataMergeStage::new()),
        // Include-shortcode expansion runs before the profile
        // checkpoint so content spliced in via `{{< include … >}}`
        // (headings, code blocks, crossref targets) is visible to
        // DocumentProfile — see bd-xfwx and
        // `claude-notes/plans/2026-04-24-include-expansion-merge.md`.
        Box::new(IncludeExpansionStage::new()),
        // Resolve include-in-header / include-before-body /
        // include-after-body authored keys (plus the legacy inline
        // `header-includes` / `include-before` / `include-after`
        // keys) into the canonical `rendered.includes.{header,
        // before-body, after-body}` location. Runs *before* the
        // profile checkpoint so file-slot dependencies are
        // recorded into `profile.includes` for `bd-r82e` cache
        // invalidation. Engine-contributed PandocIncludes are
        // folded later by ApplyTemplateStage's late-drain.
        // Plan: claude-notes/plans/2026-05-04-includes-feature.md.
        Box::new(IncludeResolveStage::new()),
        // Auto-fill `meta.listing-item.*` (description, image, word
        // count, reading time, mtime) when the author hasn't supplied
        // them. Runs pre-checkpoint so the values land in
        // `DocumentProfile.listing_item` for the listings feature
        // (epic `bd-61cd`, L1 = `bd-izqh`). Author values always win.
        // See `claude-notes/plans/2026-05-05-listings-L1-autofill-stage.md`.
        Box::new(ListingItemInfoStage::new()),
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
        Box::new(engine_stage),
        Box::new(CompileThemeCssStage::new()),
    ];
    // Inject Bootstrap JS as a Project-scoped artifact when a
    // Bootstrap-backed theme is active. Predicate matches
    // CompileThemeCssStage so JS and CSS travel together.
    // Native-only: hub-client's iframe-per-render preview blows
    // away stateful Bootstrap components, so the WASM pipeline
    // omits this stage. See bootstrap_js.rs for full rationale.
    #[cfg(not(target_arch = "wasm32"))]
    stages.push(Box::new(BootstrapJsStage::new()));
    stages.push(Box::new(UserFiltersStage::pre()));
    stages.push(Box::new(AstTransformsStage::new()));
    stages.push(Box::new(UserFiltersStage::post()));
    // bd-o8pr Phase 3: finalize the per-doc resource report
    // (defends against filters that mutate `meta.resources`).
    stages.push(Box::new(ResourceReportStage::new()));
    stages.push(Box::new(CodeHighlightStage::new()));
    // Math-mode (bd-w5ov): walk the post-transform AST and, when math
    // is present, populate `meta.math` with the engine's config + loader
    // markup. Sits right before render-html-body so any late-introduced
    // math (engine output, sugar transforms, crossref `\tag{N}`
    // injection) is visible to the walk. Included on both native and
    // WASM pipelines — see math_js.rs module docs for the rationale.
    stages.push(Box::new(MathJsStage::new()));
    stages.push(Box::new(RenderHtmlBodyStage::new()));
    let apply_stage = match apply_config {
        Some(cfg) => ApplyTemplateStage::with_config(cfg),
        None => ApplyTemplateStage::new(),
    };
    stages.push(Box::new(apply_stage));
    stages
}

/// Names of stages in [`build_html_pipeline_stages_with_options`]
/// that the q2-preview pipeline drops. All three turn the AST into
/// an HTML string (or wrap one); q2-preview returns the AST itself
/// to the React iframe.
///
/// New stages added to the HTML pipeline are **included by default**
/// — q2-preview opts a stage out only when its output is HTML-only.
/// `CompileThemeCssStage` is included so the compiled theme CSS
/// lands in VFS at `/.quarto/project-artifacts/styles.css` after a
/// q2-preview render (Plan 1 §"Multi-plan contract: theme CSS
/// artifact"); Plan 2A's iframe entry reads it.
///
/// The unknown-name validator
/// (`q2_preview_stage_excluded_names_exist_in_html_pipeline`)
/// fails the test suite if any name here is not an actual stage in
/// the full HTML pipeline (typo / rename guard).
const Q2_PREVIEW_STAGE_EXCLUDED: &[&str] = &[
    "code-highlight",
    "math-js",
    "render-html-body",
    "apply-template",
];

/// Build the q2-preview pipeline stages (Plan 1).
///
/// Constructed as [`build_html_pipeline_stages_with_options`] with
/// the names in [`Q2_PREVIEW_STAGE_EXCLUDED`] removed. Order is
/// preserved.
///
/// `AstTransformsStage` runs in both pipelines; it dispatches at
/// run-time on `ctx.format.pipeline_kind` between
/// `build_transform_pipeline` (HTML) and
/// `build_q2_preview_transform_pipeline` (q2-preview).
pub fn build_q2_preview_pipeline_stages(
    engine_registry: Option<crate::engine::EngineRegistry>,
) -> Vec<Box<dyn PipelineStage>> {
    let mut stages = build_html_pipeline_stages_with_options(None, engine_registry);
    stages.retain(|s| !Q2_PREVIEW_STAGE_EXCLUDED.contains(&s.name()));
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
/// 4. `IncludeResolveStage` - Resolve `include-in-header` etc. authored keys
/// 5. `ListingItemInfoStage` - Auto-fill `meta.listing-item.*` (L1, `bd-izqh`)
/// 6. `DocumentProfileStage` - Extract the static profile at the checkpoint
/// 7. `LinkResolutionStage` - Walk AST for cross-doc body-link targets (Phase 8)
/// 8. `UnwrapProfileStage` - Hand the AST back to downstream stages
/// 9. `CompileThemeCssStage` - Compile theme CSS from merged metadata
/// 10. `UserFiltersStage::pre()` - Apply user filters before Quarto transforms
/// 11. `AstTransformsStage` - Run Quarto transforms (callouts, metadata, TOC, etc.)
/// 12. `UserFiltersStage::post()` - Apply user filters after Quarto transforms
/// 13. `RenderHtmlBodyStage` - Render AST to HTML body
/// 14. `ApplyTemplateStage` - Apply HTML template
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
        // Resolve include-in-header / before-body / after-body
        // before the profile checkpoint so file-slot dependencies
        // land in `profile.includes` for cache invalidation
        // (bd-r82e). See `claude-notes/plans/2026-05-04-includes-feature.md`.
        Box::new(IncludeResolveStage::new()),
        // Auto-fill `meta.listing-item.*` for the listings feature
        // (L1, `bd-izqh`). Same position and contract as the native
        // pipeline. mtime via `SystemRuntime::path_metadata`; the
        // WASM impl currently returns `modified: None`, so
        // hub-client renders skip `date_modified` until `bd-a3we`
        // teaches the Automerge VFS to surface change-history time.
        Box::new(ListingItemInfoStage::new()),
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
        Box::new(ResourceReportStage::new()),
    ];
    stages.push(Box::new(CodeHighlightStage::new()));
    // Math-mode (bd-w5ov): unlike Bootstrap (which we omit from WASM
    // because iframe reinit blows away stateful components), math
    // display is safe under iframe reinit — each load gets a fresh DOM
    // and the engine typesets once. Hub-client preview should typeset
    // math live, so include the stage here too.
    stages.push(Box::new(MathJsStage::new()));
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
    // Cloning the `Rc` is cheap and keeps the provider shared across
    // every page the renderer touches (bd-izfv: the project-render path
    // calls `run_pipeline` once per page through a single
    // `RenderToHtmlRenderer`).
    stage_ctx.user_grammar_provider = ctx.user_grammar_provider.clone();
    // Transfer the project index (set by ProjectPipeline::pass_two).
    // Cloning the `Arc` is cheap and keeps the RenderContext usable
    // after the stage context is built.
    stage_ctx.project_index = ctx.project_index.clone();
    // Phase 6: thread the per-page resource resolver through to the
    // stage so that `AstTransformsStage` can re-bridge it back into
    // the inner `RenderContext` consumed by AST transforms (notably
    // `LinkRewriteTransform`).
    stage_ctx.resource_resolver = ctx.resource_resolver.clone();
    // bd-o8pr Phase 2: transfer the per-doc resource report into
    // the stage context so engine + filter stages can append to it.
    stage_ctx.resource_report = std::mem::take(&mut ctx.resource_report);

    // Create input from content
    let input = PipelineData::LoadedSource(LoadedSource::new(
        PathBuf::from(source_name),
        content.to_vec(),
    ));

    let pipeline = Pipeline::new(stages).expect("Pipeline stages should be compatible");

    let result = pipeline.run(input, &mut stage_ctx).await;

    // Transfer artifacts back to RenderContext
    ctx.artifacts = stage_ctx.artifacts;
    // bd-o8pr Phase 2: transfer engine/filter-collected resources
    // back to the caller (`render_document_to_file` reads this).
    ctx.resource_report = stage_ctx.resource_report;

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
    // stage list (via `build_html_pipeline_stages_with_options`); the
    // only differences are whether the final `ApplyTemplateStage`
    // carries a scope-aware resolver and whether
    // `EngineExecutionStage` runs against a replay-substituted
    // registry (bd-45yw).
    //
    // Note: the engine_registry override is consumed (via Option::take
    // on a clone path) — `HtmlRenderConfig` is borrowed `&`, so we
    // clone the registry. EngineRegistry itself stores Arc<dyn
    // ExecutionEngine>, so cloning is cheap.
    let apply_config = config
        .resolver
        .clone()
        .map(|r| ApplyTemplateConfig::new().with_resolver(r));
    let engine_registry = config.engine_registry.clone();
    let stages = build_html_pipeline_stages_with_options(apply_config, engine_registry);

    let (output, diagnostics) = run_pipeline(content, source_name, ctx, runtime, stages).await?;
    // Extract the rendered output
    let rendered = output.into_rendered_output().ok_or_else(|| {
        crate::error::QuartoError::Other("Pipeline did not produce RenderedOutput".to_string())
    })?;

    // bd-xdnk: forward the document's SourceContext (populated by
    // ParseDocumentStage, IncludeExpansionStage, ApplyTemplateStage)
    // so cross-file diagnostics — template warnings, includes — can
    // resolve back to the right source slice when ariadne renders.
    Ok(RenderOutput {
        html: rendered.content,
        diagnostics,
        source_context: rendered.source_context,
    })
}

/// Render QMD content to AST JSON for the q2-preview format (Plan 1).
///
/// Sibling of [`render_qmd_to_html`]. Drives the q2-preview stage
/// list (everything through `ResourceReportStage`, no HTML
/// rendering) and serializes the resulting Pandoc AST to JSON via
/// `pampa::writers::json::write_with_config` so the React iframe
/// can render it directly.
///
/// # Arguments
///
/// * `content` - The QMD source content as bytes
/// * `source_name` - Name of the source file (for error messages
///   and the `ASTContext::filenames` slot the JSON writer reads)
/// * `ctx` - Render context. Should have a `resource_resolver`
///   set so `ResourceCollectorTransform` rewrites image URLs to
///   the same path the consumer (e.g. `RenderToPreviewAstRenderer`)
///   uses when flushing artifacts to VFS.
/// * `runtime` - System runtime for filesystem operations
///
/// # Returns
///
/// A [`PreviewAstOutput`] carrying the serialized AST plus
/// diagnostics and source context.
///
/// # Errors
///
/// Returns an error if parsing fails, transforms fail, or the JSON
/// serialization fails (e.g. due to a non-UTF-8 byte sequence in
/// the writer output, which would indicate a writer bug).
pub async fn render_qmd_to_preview_ast(
    content: &[u8],
    source_name: &str,
    ctx: &mut RenderContext<'_>,
    runtime: Arc<dyn quarto_system_runtime::SystemRuntime>,
    engine_registry: Option<crate::engine::EngineRegistry>,
) -> Result<PreviewAstOutput> {
    // The q2-preview stage list excludes `CodeHighlightStage` /
    // `RenderHtmlBodyStage` / `ApplyTemplateStage`, so the
    // pipeline returns `DocumentAst`, not `RenderedOutput`.
    //
    // Phase C.4 (bd-kw93.3): an `engine_registry` override is now
    // threaded through so callers (the WASM `render_page_for_preview`
    // entry point) can substitute a `ReplayEngine` constructed from
    // an `EngineCapture` recorded server-side (Phase C.1, bd-kw93.2)
    // without re-running the real engine in the browser.
    let stages = build_q2_preview_pipeline_stages(engine_registry);

    let (output, diagnostics) = run_pipeline(content, source_name, ctx, runtime, stages).await?;
    let ast = output.into_document_ast().ok_or_else(|| {
        crate::error::QuartoError::Other(
            "q2-preview pipeline did not produce DocumentAst".to_string(),
        )
    })?;

    // Source context for translating diagnostic offsets into
    // line/column on the JS side.
    let mut source_context = SourceContext::new();
    let content_str = String::from_utf8_lossy(content).to_string();
    source_context.add_file(source_name.to_string(), Some(content_str));

    // Build an `ASTContext` from the source context — the JSON
    // writer needs this to emit `[file_id, start, end]` source-
    // location triples for inlines (`include_inline_locations:
    // true`). This shape is lifted verbatim from the q2-debug
    // entry point (`wasm-quarto-hub-client/src/lib.rs:914-916`)
    // so q2-preview's JSON envelope matches q2-debug's at the
    // wire level.
    let ast_context = pampa::pandoc::ASTContext {
        filenames: vec![source_name.to_string()],
        example_list_counter: std::cell::Cell::new(1),
        source_context: source_context.clone(),
        parent_source_info: None,
    };
    let json_config = pampa::writers::json::JsonConfig {
        include_inline_locations: true,
    };
    let mut buf = Vec::new();
    pampa::writers::json::write_with_config(&ast.ast, &ast_context, &mut buf, &json_config)
        .map_err(|e| {
            crate::error::QuartoError::Other(format!("q2-preview JSON serialization failed: {e:?}"))
        })?;
    let ast_json = String::from_utf8(buf).map_err(|e| {
        crate::error::QuartoError::Other(format!("q2-preview JSON output was not valid UTF-8: {e}"))
    })?;

    Ok(PreviewAstOutput {
        ast_json,
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
    // TODO(bd-0fd0): there is no Lua-filter slot between this Generate
    // sub-phase and the Render sub-phase below — `UserFiltersStage::pre`
    // and `::post` bracket the whole `AstTransformsStage`. The L3 plan's
    // D2 ("resolved data lives at meta.listings.<id>") was revised in
    // light of this for listings (data flows via a typed RenderContext
    // field instead). The same forward-compat note applies to the
    // navbar/sidebar/footer generates here.
    //
    // TocGenerate must run after SectionizeTransform so section IDs are
    // available; navbar/footer generates only read top-level metadata.
    pipeline.push(Box::new(TocGenerateTransform::new()));
    pipeline.push(Box::new(NavbarGenerateTransform::new()));
    pipeline.push(Box::new(SidebarGenerateTransform::new()));
    // PageNavGenerate must run after SidebarGenerate so it reads the
    // resolved `navigation.sidebar` for the current page.
    pipeline.push(Box::new(PageNavGenerateTransform::new()));
    pipeline.push(Box::new(FooterGenerateTransform::new()));
    // ListingGenerateTransform runs after the navigation generates
    // because a future Lua-filter slot (bd-0fd0) should see the full
    // generated set in one place. ListingRenderTransform runs *before*
    // the navigation renders so listing markup gets a stable place in
    // ast.blocks before any rendered-HTML emission for templates.
    pipeline.push(Box::new(ListingGenerateTransform::new()));
    pipeline.push(Box::new(ListingRenderTransform::new()));
    // CategoriesSidebarTransform runs after ListingRenderTransform
    // so it reads `RenderContext::resolved_listings` (which the
    // render transform restores after consumption) and aggregates
    // categories across all listings on the host page. It must run
    // before TocRenderTransform so both `rendered.navigation.*`
    // keys land before ApplyTemplate reads them.
    //
    // TODO(bd-0fd0): same Lua-filter slot caveat as the listing
    // generate/render transforms above — when the slot lands the
    // resolved-listing data path becomes user-mutable.
    pipeline.push(Box::new(CategoriesSidebarTransform::new()));
    // L9 (bd-o90m): emit one staged feed file per feed-configured
    // listing on the host page. Reads `ctx.resolved_listings` and
    // writes `<output_dir>/<dir>/<stem>.feed-{type}-staged` to disk
    // synchronously. Native-only — the entire feed staging module
    // is gated to `cfg(not(target_arch = "wasm32"))` (it depends on
    // `imagesize` and synchronous `std::fs::write`, neither of
    // which makes sense in the in-browser VFS). The
    // `ListingFeedLinkTransform` registered just below DOES run on
    // both targets so the rendered HTML's head metadata stays
    // byte-for-byte identical between the CLI and hub-client preview.
    #[cfg(not(target_arch = "wasm32"))]
    pipeline.push(Box::new(
        crate::project::listing::feed::ListingFeedStageTransform::new(),
    ));
    // L9 (bd-o90m): inject `<link rel="alternate" type="application/rss+xml">`
    // into `rendered.includes.header` for every feed-configured
    // listing. Runs on both native AND WASM (registered in both
    // pipeline builders) so the rendered HTML matches between the
    // CLI render and the hub-client preview. The link points at
    // a feed file the hub-client doesn't write — clicking it 404s
    // in preview, which is acceptable v1 behavior (documented in
    // the L11 listings reference page).
    pipeline.push(Box::new(
        crate::project::listing::feed::ListingFeedLinkTransform::new(),
    ));
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

/// Names of transforms in [`build_transform_pipeline`] that the
/// q2-preview pipeline drops. Three categories:
///
/// 1. **Preserve CustomNodes for React** — `callout-resolve`,
///    `crossref-render`. Wrappers stay so React's type-specific
///    components (Plan 2) can render Callout / Theorem / Proof /
///    FloatRefTarget / Equation / CrossrefResolvedRef.
/// 2. **Synthesize-with-no-preimage** — `title-block`, `footnotes`,
///    `appendix-structure`. These construct containers with no
///    source backing; deferred to a future plan with
///    wrapper-CustomNode round-trip support.
/// 3. **HTML-pipeline-specific outputs** — `toc-render`,
///    `navbar-render`, `sidebar-render`, `page-nav-render`,
///    `footer-render`, `website-favicon`. These produce HTML strings
///    that React consumes from structured metadata directly.
///
/// Phase F.1 (bd-kw93.14) note: `link-rewrite` was previously here
/// but is now included so cross-page body links emit `.html` hrefs
/// the iframe link-handler can intercept.
///
/// New transforms added to [`build_transform_pipeline`] are
/// **included by default** — q2-preview opts a transform out
/// only when there's a concrete reason (one of the three
/// categories above). This is the deliberate inversion of the
/// original Plan 1 explicit-list framing: see commit message of
/// the deny-list flip for the empirical motivation.
///
/// The unknown-name validator
/// (`q2_preview_transform_excluded_names_exist_in_html_pipeline`)
/// fails the test suite if any name here is not an actual transform
/// in the full HTML pipeline (typo / rename guard).
const Q2_PREVIEW_TRANSFORM_EXCLUDED: &[&str] = &[
    "callout-resolve",
    "website-favicon",
    "title-block",
    // "footnotes" — included in q2-preview's pipeline (Plan 2B):
    // produces Pandoc primitives (Span/Sup/Link/Div/OrderedList) that
    // q2-preview's leaves render natively. Note marker numbering and
    // the document-end footnote section both come from this transform.
    // bd-1kly tracks the upstream gap for `reference-location: block`
    // and `section`; until that lands, q2-preview's `Note.tsx`
    // tooltip-body fallback handles those configs.
    "toc-render",
    "navbar-render",
    "sidebar-render",
    "page-nav-render",
    "footer-render",
    // "link-rewrite" — included in q2-preview's pipeline (Phase F.1,
    // bd-kw93.14): rewrites cross-page body links to artifact-rooted
    // `.html` hrefs the SPA's iframe link-handler intercepts and
    // routes through `onNavigateToDocument`.
    // "appendix-structure" — included in q2-preview's pipeline (Plan 2B):
    // pure Pandoc primitives, structurally identical to the HTML
    // pipeline. Folds footnotes section, license/copyright/citation
    // metadata into <div id="quarto-appendix">. Bibliography branch
    // is inert until Citeproc lands.
    "crossref-render",
];

/// Build the q2-preview transform pipeline (Plan 1).
///
/// Constructed as [`build_transform_pipeline`] with the names in
/// [`Q2_PREVIEW_TRANSFORM_EXCLUDED`] removed. Order is preserved.
/// Constructor args (notably `shortcode_paths`, `extensions`,
/// `runtime`, `target_format`) are forwarded verbatim so
/// shortcode-and-Lua semantics match the HTML pipeline.
///
/// `AstTransformsStage::run()` dispatches between this and
/// `build_transform_pipeline` based on `ctx.format.pipeline_kind`.
pub fn build_q2_preview_transform_pipeline(
    shortcode_paths: Vec<std::path::PathBuf>,
    extensions: Vec<crate::extension::types::Extension>,
    runtime: std::sync::Arc<dyn quarto_system_runtime::SystemRuntime>,
    target_format: String,
) -> TransformPipeline {
    let mut pipeline =
        build_transform_pipeline(shortcode_paths, extensions, runtime, target_format);
    pipeline.retain_excluding(Q2_PREVIEW_TRANSFORM_EXCLUDED);
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
        assert_eq!(stages.len(), 20);
        assert_eq!(stages[0].name(), "parse-document");
        assert_eq!(stages[1].name(), "metadata-merge");
        // Include expansion runs before the profile checkpoint (bd-xfwx)
        // so profiles reflect content spliced in via `{{< include ... >}}`.
        assert_eq!(stages[2].name(), "include-expansion");
        // include-resolve (bd-8kp3) sits between include-expansion and
        // the profile checkpoint so file-slot include dependencies are
        // recorded into `profile.includes` for cache invalidation.
        assert_eq!(stages[3].name(), "include-resolve");
        // Listings auto-fill (bd-izqh, L1) sits between include-resolve
        // and the profile checkpoint so `meta.listing-item.*` enrichment
        // is visible to `DocumentProfile.listing_item`.
        assert_eq!(stages[4].name(), "listing-item-info");
        // Profile checkpoint (Phase 0 website epic, bd-f3jc).
        assert_eq!(stages[5].name(), "document-profile");
        // Cross-doc body-link resolution (Phase 8 sub-phase 8.0d).
        assert_eq!(stages[6].name(), "link-resolution");
        assert_eq!(stages[7].name(), "unwrap-profile");
        assert_eq!(stages[8].name(), "pre-engine-sugaring");
        assert_eq!(stages[9].name(), "engine-execution");
        assert_eq!(stages[10].name(), "compile-theme-css");
        // Bootstrap JS (bd-4eyf) sits immediately after CompileThemeCssStage
        // so the same theme predicate gates JS and CSS together.
        assert_eq!(stages[11].name(), "bootstrap-js");
        assert_eq!(stages[12].name(), "user-filters-pre");
        assert_eq!(stages[13].name(), "ast-transforms");
        assert_eq!(stages[14].name(), "user-filters-post");
        // bd-o8pr Phase 3: finalize per-doc resource report.
        assert_eq!(stages[15].name(), "resource-report");
        assert_eq!(stages[16].name(), "code-highlight");
        // Math-mode (bd-w5ov) walks the post-transform AST and
        // populates meta.math when math is present. Sits just before
        // render-html-body so any late-introduced math (sugar, user
        // filters, crossref `\tag{N}`) is visible.
        assert_eq!(stages[17].name(), "math-js");
        assert_eq!(stages[18].name(), "render-html-body");
        assert_eq!(stages[19].name(), "apply-template");
    }

    #[test]
    fn test_build_html_pipeline() {
        let pipeline = build_html_pipeline();
        assert_eq!(pipeline.len(), 20);
    }

    #[test]
    fn test_build_wasm_html_pipeline() {
        let pipeline = build_wasm_html_pipeline();
        // WASM pipeline matches the native HTML pipeline minus
        // `engine-execution` and `bootstrap-js`. Includes the
        // `include-resolve` stage (bd-8kp3) so the same
        // `rendered.includes.*` contract holds in the browser.
        // Includes `listing-item-info` (bd-izqh) so listing-item
        // metadata is auto-filled symmetrically.
        // Includes `math-js` (bd-w5ov) — math display is safe under
        // hub-client iframe reinit and we want live math in preview.
        assert_eq!(pipeline.len(), 18);
        let names = pipeline.stage_names();
        // bd-4eyf: hub-client iframe reinit blows away stateful
        // Bootstrap components, so we deliberately omit `bootstrap-js`
        // from the WASM pipeline. This assertion locks the omission in.
        assert!(
            !names.contains(&"bootstrap-js"),
            "wasm pipeline must not include bootstrap-js (hub-client iframe reinit)"
        );
        // bd-w5ov: math display IS safe under iframe reinit (each load
        // gets a fresh DOM and the engine typesets once). The hub-client
        // preview should typeset math live, so `math-js` is included.
        assert!(
            names.contains(&"math-js"),
            "wasm pipeline must include math-js (live math in hub-client preview)"
        );
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

    /// Plan 2A item 11: the artifact key produced by
    /// `CompileThemeCssStage` is `css:theme:<fingerprint>`, where
    /// `<fingerprint>` matches `theme_fingerprint(css)` byte-for-byte.
    /// The WASM bridge recovers `RenderResponse.theme_fingerprint` from
    /// this suffix without re-hashing CSS bytes; the contract this test
    /// locks is that the suffix and the CSS-derived fingerprint stay in
    /// sync.
    #[test]
    fn test_theme_fingerprint_recoverable_from_artifact_key() {
        use crate::stage::stages::theme_fingerprint;

        // Render twice with the same theme — fingerprints must match.
        let content_a = b"---\ntitle: Test\ntheme: flatly\n---\n\nA.";
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();

        let mut ctx_a1 = RenderContext::new(&project, &doc, &format, &binaries);
        let mut ctx_a2 = RenderContext::new(&project, &doc, &format, &binaries);
        let config = HtmlRenderConfig::default();
        let _ = pollster::block_on(render_qmd_to_html(
            content_a,
            "test.qmd",
            &mut ctx_a1,
            &config,
            make_test_runtime(),
        ))
        .unwrap();
        let _ = pollster::block_on(render_qmd_to_html(
            content_a,
            "test.qmd",
            &mut ctx_a2,
            &config,
            make_test_runtime(),
        ))
        .unwrap();

        let key_a1 = ctx_a1
            .artifacts
            .get_by_prefix("css:theme:")
            .first()
            .map(|(k, _)| k.to_string())
            .expect("expected one css:theme:* artifact");
        let key_a2 = ctx_a2
            .artifacts
            .get_by_prefix("css:theme:")
            .first()
            .map(|(k, _)| k.to_string())
            .expect("expected one css:theme:* artifact");
        assert_eq!(
            key_a1, key_a2,
            "same theme renders must produce byte-identical fingerprint keys"
        );

        let suffix_a = key_a1
            .strip_prefix("css:theme:")
            .expect("key should start with css:theme:");
        let css_a = get_css_artifact(&ctx_a1);
        assert_eq!(
            suffix_a,
            theme_fingerprint(&css_a),
            "key suffix must match theme_fingerprint(css) byte-for-byte"
        );

        // Render with a different theme — fingerprint must differ.
        let content_b = b"---\ntitle: Test\ntheme: cosmo\n---\n\nB.";
        let mut ctx_b = RenderContext::new(&project, &doc, &format, &binaries);
        let _ = pollster::block_on(render_qmd_to_html(
            content_b,
            "test.qmd",
            &mut ctx_b,
            &config,
            make_test_runtime(),
        ))
        .unwrap();
        let key_b = ctx_b
            .artifacts
            .get_by_prefix("css:theme:")
            .first()
            .map(|(k, _)| k.to_string())
            .expect("expected one css:theme:* artifact");
        assert_ne!(
            key_a1, key_b,
            "different themes must produce different fingerprint keys"
        );
    }

    /// bd-45yw Phase 4a: `HtmlRenderConfig.engine_registry` overrides
    /// the engine registry that `EngineExecutionStage` uses, so a
    /// caller (orchestrator/CLI replay path) can substitute a
    /// `ReplayEngine` without touching the rest of the pipeline. This
    /// test renders a document declaring an engine that *no real
    /// engine implements* — the only way the render can succeed is
    /// through the replay-substituted registry.
    #[test]
    fn test_render_qmd_to_html_uses_replay_registry_from_config() {
        use crate::engine::EngineRegistry;
        use quarto_trace::EngineCapture;

        // A document that declares an engine name no real engine
        // covers — without replay substitution, the stage falls back
        // to markdown with a warning, which would yield different
        // output than the recorded one.
        let content =
            b"---\nengine: replay-only-engine\n---\n\n# Original Heading\n\nOriginal body.\n";

        // The recorded ExecuteResult deliberately replaces the body
        // with a distinct marker. Asserting the marker reaches the
        // rendered HTML proves the replay engine ran.
        let recorded_markdown = "---\nengine: replay-only-engine\n---\n\n# Replayed Heading\n\nReplayed body marker XYZ.\n";

        // Determine the QMD that the stage will pass to execute().
        // The recorded `input_qmd` must match this byte-for-byte.
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();

        // Two-pass: first compute the serialized QMD by running with
        // a probe engine, then build the real capture and rerun. We
        // can't easily compute it without running the parse + serialize
        // path, so we use a probe engine that records its own input.
        use std::sync::Mutex;
        struct ProbeEngine {
            captured_input: Arc<Mutex<Option<String>>>,
        }
        impl crate::engine::ExecutionEngine for ProbeEngine {
            fn name(&self) -> &str {
                "replay-only-engine"
            }
            fn execute(
                &self,
                input: &str,
                _ctx: &crate::engine::ExecutionContext,
            ) -> std::result::Result<crate::engine::ExecuteResult, crate::engine::ExecutionError>
            {
                *self.captured_input.lock().unwrap() = Some(input.to_string());
                // Return passthrough so the probe completes successfully.
                Ok(crate::engine::ExecuteResult::passthrough(input))
            }
            fn is_available(&self) -> bool {
                true
            }
        }

        let captured = Arc::new(Mutex::new(None::<String>));
        let probe = Arc::new(ProbeEngine {
            captured_input: captured.clone(),
        });
        let mut probe_registry = EngineRegistry::new();
        probe_registry.register(probe);

        // Probe run.
        {
            let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
            let probe_config = HtmlRenderConfig {
                resolver: None,
                engine_registry: Some(probe_registry),
            };
            let runtime = make_test_runtime();
            let _ = pollster::block_on(render_qmd_to_html(
                content,
                "test.qmd",
                &mut ctx,
                &probe_config,
                runtime,
            ))
            .unwrap();
        }

        let recorded_input = captured
            .lock()
            .unwrap()
            .clone()
            .expect("probe must have captured the engine's input");

        // Now build the replay capture against that input and the
        // distinct recorded markdown.
        let capture = EngineCapture {
            engine_name: "replay-only-engine".into(),
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

        let replay_registry = EngineRegistry::with_replay(capture);

        // Real run, this time through the replay-substituted registry.
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        let config = HtmlRenderConfig {
            resolver: None,
            engine_registry: Some(replay_registry),
        };
        let runtime = make_test_runtime();
        let output = pollster::block_on(render_qmd_to_html(
            content, "test.qmd", &mut ctx, &config, runtime,
        ))
        .unwrap();

        assert!(
            output.html.contains("Replayed Heading"),
            "rendered HTML must contain the replay engine's heading; got:\n{}",
            &output.html,
        );
        assert!(
            output.html.contains("Replayed body marker XYZ"),
            "rendered HTML must contain the replay marker; got:\n{}",
            &output.html,
        );
        assert!(
            !output.html.contains("Original body"),
            "rendered HTML must not contain the original body — replay should override; got:\n{}",
            &output.html,
        );
    }

    // ─── q2-preview pipeline (Plan 1) ────────────────────────────

    /// Drift-protection helper for subset transform pipelines.
    ///
    /// Asserts that `subset` is exactly `full` filtered by
    /// `expected_excluded`, preserving order. Catches every drift
    /// mode in one shot: a transform added to `full`, renamed,
    /// reordered on either side, or removed from `subset`.
    /// Verify every name in [`Q2_PREVIEW_TRANSFORM_EXCLUDED`] is an
    /// actual transform in the full HTML pipeline. Catches the one
    /// drift mode the deny-list construction *can't* catch on its
    /// own: a transform gets renamed and the exclusion list silently
    /// no-ops on the old name (so the renamed transform leaks into
    /// q2-preview).
    ///
    /// New transforms added to `build_transform_pipeline` are
    /// included in q2-preview by default — that's the whole point
    /// of the deny-list flip — so this test does NOT fail on
    /// HTML-pipeline additions.
    #[test]
    fn q2_preview_transform_excluded_names_exist_in_html_pipeline() {
        let runtime = make_test_runtime();
        let html = build_transform_pipeline(vec![], vec![], runtime, "html".to_string());
        let html_names: Vec<&str> = html.iter().map(|t| t.name()).collect();

        let unknown: Vec<&&str> = Q2_PREVIEW_TRANSFORM_EXCLUDED
            .iter()
            .filter(|n| !html_names.contains(n))
            .collect();
        assert!(
            unknown.is_empty(),
            "Q2_PREVIEW_TRANSFORM_EXCLUDED contains names not in build_transform_pipeline: \
             {unknown:?}. Likely a typo or a rename — update the const in pipeline.rs. \
             Full HTML transform list: {html_names:?}",
        );
    }

    /// `render_qmd_to_preview_ast` runs the q2-preview pipeline
    /// (CalloutTransform sugar, no CalloutResolveTransform) so a
    /// callout survives as a `__quarto_custom_node` wrapper Div in
    /// the serialized JSON. This is the contract Plan 2 (React
    /// CustomNode components) consumes.
    #[test]
    fn render_qmd_to_preview_ast_preserves_callout_custom_node() {
        let content = b"---\ntitle: Test\nformat: q2-preview\n---\n\n\
                        ::: {.callout-warning}\n## Watch Out\n\nBe careful!\n:::\n";

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::from_format_string("q2-preview").unwrap();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let runtime = make_test_runtime();
        let output = pollster::block_on(render_qmd_to_preview_ast(
            content, "test.qmd", &mut ctx, runtime, None,
        ))
        .expect("q2-preview render");

        let snippet = || &output.ast_json[..output.ast_json.len().min(800)];

        // JSON should contain the wrapper Div class + the
        // type-name attribute pampa emits for CustomNodes.
        assert!(
            output.ast_json.contains("__quarto_custom_node"),
            "expected wrapper class in q2-preview JSON; got:\n{}",
            snippet()
        );
        assert!(
            output.ast_json.contains("data-custom-type"),
            "expected data-custom-type attribute; got:\n{}",
            snippet()
        );
        assert!(
            output.ast_json.contains("Callout"),
            "expected Callout type-name in JSON; got:\n{}",
            snippet()
        );
    }

    /// Plan 2B: with `FootnotesTransform` no longer in the
    /// q2-preview deny-list, inline-footnote rendering (`^[body]`
    /// syntax — produces `Inline::Note` directly) must emit the
    /// standard `Span(Sup(Link))` reference and a `Div.footnotes`
    /// body section. Catches regressions if the transform is
    /// accidentally re-excluded.
    ///
    /// **Reference-style footnotes** (`[^1]: body` with `[^1]` in
    /// prose) are NOT covered by this test: pampa's postprocess at
    /// `crates/pampa/src/pandoc/treesitter_utils/postprocess.rs:1134-1146`
    /// converts `Inline::NoteReference` to a `Span(class="quarto-note-reference")`
    /// with empty content during parsing, before any quarto-core
    /// transform runs. Nothing downstream resolves those Spans (the
    /// HTML pipeline drops them too, verified manually). That's a
    /// pre-existing gap, not a Plan 2B regression. bd-1kly tracks
    /// the related upstream work.
    #[test]
    fn render_qmd_to_preview_ast_emits_inline_footnote_section() {
        let content =
            b"---\ntitle: Test\nformat: q2-preview\n---\n\nA paragraph^[the footnote body].\n";

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::from_format_string("q2-preview").unwrap();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let runtime = make_test_runtime();
        let output = pollster::block_on(render_qmd_to_preview_ast(
            content, "test.qmd", &mut ctx, runtime, None,
        ))
        .expect("q2-preview render");

        // The transform replaces the inline Note with a Span carrying
        // the footnote-ref class.
        assert!(
            output.ast_json.contains("footnote-ref"),
            "expected footnote-ref class in q2-preview output; full output:\n{}",
            output.ast_json
        );
        // The section at end is a Div with class="footnotes".
        assert!(
            output.ast_json.contains("\"footnotes\""),
            "expected footnotes class on section Div; full output:\n{}",
            output.ast_json
        );
    }

    /// Phase F.1 (bd-kw93.14): `LinkRewriteTransform` runs in the
    /// q2-preview pipeline so cross-page body links emit `.html`
    /// hrefs that the iframe link-handler can intercept and route
    /// through `onNavigateToDocument`. If this regresses, the SPA's
    /// cross-page navigation breaks (clicks fall through to the
    /// browser's default `.qmd` request, which 404s the iframe).
    #[test]
    fn q2_preview_pipeline_includes_link_rewrite() {
        let runtime = make_test_runtime();
        let pipeline =
            build_q2_preview_transform_pipeline(vec![], vec![], runtime, "q2-preview".to_string());
        let names: Vec<&str> = pipeline.iter().map(|t| t.name()).collect();
        assert!(
            names.contains(&"link-rewrite"),
            "link-rewrite must be present in the q2-preview pipeline; got: {names:?}",
        );
    }

    /// Verify every name in [`Q2_PREVIEW_STAGE_EXCLUDED`] is an
    /// actual stage in the full HTML pipeline. Same drift-mode
    /// guard as
    /// `q2_preview_transform_excluded_names_exist_in_html_pipeline`,
    /// but at the stage level.
    #[test]
    fn q2_preview_stage_excluded_names_exist_in_html_pipeline() {
        let html_stages = build_html_pipeline_stages_with_options(None, None);
        let html_names: Vec<&str> = html_stages.iter().map(|s| s.name()).collect();

        let unknown: Vec<&&str> = Q2_PREVIEW_STAGE_EXCLUDED
            .iter()
            .filter(|n| !html_names.contains(n))
            .collect();
        assert!(
            unknown.is_empty(),
            "Q2_PREVIEW_STAGE_EXCLUDED contains names not in build_html_pipeline_stages: \
             {unknown:?}. Likely a typo or a rename — update the const in pipeline.rs. \
             Full HTML stage list: {html_names:?}",
        );
    }
}
