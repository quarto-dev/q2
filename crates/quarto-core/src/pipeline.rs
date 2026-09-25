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
#[cfg(not(target_arch = "wasm32"))]
use crate::stage::stages::ClipboardJsStage;
#[cfg(not(target_arch = "wasm32"))]
use crate::stage::stages::PandocWriteStage;
#[cfg(not(target_arch = "wasm32"))]
use crate::stage::stages::TypstCompileStage;
use crate::stage::{
    ApplyTemplateStage, AstTransformsStage, AttributionGenerateStage, CompileThemeCssStage,
    DocumentProfileStage, EngineExecutionStage, EquationNumberStage, IncludeExpansionStage,
    IncludeResolveStage, LanguageResolveStage, LinkResolutionStage, ListingItemInfoStage,
    LoadedSource, MathJsStage, MathMlStage, MetadataMergeStage, ParseDocumentStage, Pipeline,
    PipelineData, PipelineStage, PreEngineSugaringStage, RenderHtmlBodyStage, ResourceReportStage,
    SourceConversionStage, StageContext, UnwrapProfileStage, UserFiltersStage,
};
use crate::transform::TransformPipeline;
use crate::transforms::{
    AppendixStructureTransform, AttributionRenderTransform, AttributionViewerTransform,
    AuthorsNormalizeTransform, BreadcrumbsRenderTransform, CalloutResolveTransform,
    CalloutTransform, CategoriesSidebarTransform, CodeBlockGenerateTransform,
    CodeBlockRenderTransform, ConditionalContentTransform, CrossrefIndexTransform,
    CrossrefRenderTransform, CrossrefResolveTransform, DateNormalizeTransform, DraftAlertTransform,
    EquationLabelTransform, ExampleEmbedRenderTransform, ExampleEmbedTransform,
    FloatRefTargetSugarTransform, FooterGenerateTransform, FooterRenderTransform,
    FootnotesResolveTransform, FootnotesTransform, LinkRewriteTransform, ListingGenerateTransform,
    ListingRenderTransform, MermaidRenderTransform, MetadataNormalizeTransform,
    NavbarGenerateTransform, NavbarRenderTransform, PageNavGenerateTransform,
    PageNavRenderTransform, ProofSugarTransform, ReferenceLinkDiagnosticsTransform,
    RepoActionsRenderTransform, ResourceCollectorTransform, ResponsiveImageTransform,
    SectionizeTransform, ShortcodeResolveTransform, SidebarGenerateTransform,
    SidebarRenderTransform, TableBootstrapClassTransform, TheoremSugarTransform,
    TitleBannerTransform, TitleBlockTransform, TocGenerateTransform, TocLocationTransform,
    TocRenderTransform, WebsiteBootstrapIconsTransform, WebsiteCanonicalUrlTransform,
    WebsiteFaviconTransform, WebsiteTitlePrefixTransform,
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
    pub engine_registry: Option<std::sync::Arc<crate::engine::EngineRegistry>>,
    /// Which documents may execute code (bd-sl79jjiq). Threaded onto
    /// `RenderContext` next to `engine_registry`.
    pub execution_policy: crate::engine::ExecutionPolicy,

    /// Server-recorded engine captures to splice into the HTML render
    /// (bd-uy4uygha). When non-empty, a [`crate::stage::CaptureSpliceStage`]
    /// is inserted before [`EngineExecutionStage`] so recorded engine output
    /// appears in the rendered HTML without re-running the engine — this is how
    /// hub-client's default `format: html` preview shows the output of a
    /// document executed by a connected `q2 provide-hub`.
    ///
    /// Empty (the default) renders code cells as source, byte-identical to the
    /// pre-bd-uy4uygha behavior for every existing caller (`q2 render` runs the
    /// real engine natively instead).
    pub captures: Vec<quarto_trace::EngineCapture>,
}

impl HtmlRenderConfig {
    /// Create a new configuration with a resolver attached.
    pub fn with_resolver(resolver: crate::resource_resolver::ResourceResolverContext) -> Self {
        Self {
            resolver: Some(resolver),
            engine_registry: None,
            execution_policy: crate::engine::ExecutionPolicy::default(),
            captures: Vec::new(),
        }
    }

    /// Attach an engine registry override (bd-45yw replay activation).
    pub fn with_engine_registry(
        mut self,
        registry: std::sync::Arc<crate::engine::EngineRegistry>,
    ) -> Self {
        self.engine_registry = Some(registry);
        self
    }

    /// Attach server-recorded engine captures to splice into the HTML render
    /// (bd-uy4uygha). See the [`captures`](Self::captures) field.
    pub fn with_captures(mut self, captures: Vec<quarto_trace::EngineCapture>) -> Self {
        self.captures = captures;
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
    /// True when the document resolved to a code-executing engine but
    /// the render's `ExecutionPolicy` excluded it, so its cells were
    /// passed through inert (bd-sl79jjiq). Never true for a document
    /// with nothing to execute.
    pub execution_skipped: bool,
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
    /// The **untransformed** Pandoc AST — the `qmd_to_pandoc` output
    /// captured immediately after `ParseDocumentStage`, before
    /// `AstTransformsStage`. Serialized with the same JSON config as
    /// `ast_json`. Round-tripped to the frontend and used as the
    /// baseline in `apply_node_edit` (Phase 1 of the target-incremental-
    /// writes plan).
    pub untransformed_ast_json: Option<String>,
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
    build_html_pipeline_stages_with_options(apply_config)
}

/// Like [`build_html_pipeline_stages_with_apply_config`], but also
/// accepts an optional `ApplyTemplateConfig`.  The engine registry is
/// now carried on [`crate::stage::StageContext`] (Task 8 of the
/// ts-engine-extensions plan) — callers that need a non-default
/// registry set `ctx.engine_registry_override` (or equivalently
/// `HtmlRenderConfig.engine_registry`) before calling `run_pipeline`,
/// which applies it after constructing the `StageContext`.
pub fn build_html_pipeline_stages_with_options(
    apply_config: Option<ApplyTemplateConfig>,
) -> Vec<Box<dyn PipelineStage>> {
    let engine_stage = EngineExecutionStage::new();
    let mut stages: Vec<Box<dyn PipelineStage>> = vec![
        // Convert non-QMD files (e.g. .echo, .jl, .ipynb) to QMD before parse.
        // First engine in deterministic order that claims the file wins.
        // .qmd / .md files pass through unchanged; unclaimed non-QMD files error.
        Box::new(SourceConversionStage::new()),
        Box::new(ParseDocumentStage::new()),
        Box::new(MetadataMergeStage::new()),
        // Resolve localized terms (`lang` + `language:` → `quarto.language`
        // metadata) right after the merge so every downstream consumer —
        // profile extraction, transforms, templates — sees the table
        // (bd-llhlzd7p).
        Box::new(LanguageResolveStage::new()),
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
    // Inject clipboard.js as a Project-scoped artifact when
    // `code-copy` isn't explicitly disabled (Phase 2 of bd-1tl09).
    // Sits next to BootstrapJsStage because the two share the
    // minimal-HTML gate and the WASM-exclusion reasoning. The
    // companion init handler is added in Phase 2 Commit 3.
    #[cfg(not(target_arch = "wasm32"))]
    stages.push(Box::new(ClipboardJsStage::new()));
    // Inject the grouped-tabset sync module alongside Bootstrap JS
    // (bd-toc-tabset-titles-zq93gjvf, design decision 4: ships
    // whenever Bootstrap does — inert on pages without grouped
    // tabsets). Same WASM-exclusion reasoning as the two above.
    #[cfg(not(target_arch = "wasm32"))]
    stages.push(Box::new(crate::stage::stages::TabsetsJsStage::new()));
    // Attribution-generate runs *before* user filters so the
    // `quarto.attribution.*` Lua host binding sees a populated
    // sidecar in both `pre` and `post` filter passes. No-op when
    // no provider is installed (`ctx.attribution_provider` is None).
    stages.push(Box::new(AttributionGenerateStage::new()));
    stages.push(Box::new(UserFiltersStage::pre()));
    stages.push(Box::new(AstTransformsStage::new()));
    stages.push(Box::new(UserFiltersStage::post()));
    // bd-o8pr Phase 3: finalize the per-doc resource report
    // (defends against filters that mutate `meta.resources`).
    stages.push(Box::new(ResourceReportStage::new()));
    // Equation-number encoding (bd-vlhi2zkj). Crossref-render left each
    // numbered equation's number on the reserved `quarto-eq-number`
    // attribute; this stage turns it into `\tag{N}` (MathJax/KaTeX),
    // ` \qquad(N)` (no engine reads `\tag`) or a sibling label (MathML)
    // and removes the attribute. It sits *after* `UserFiltersStage::post`
    // on purpose: a Lua post filter may rewrite or delete the attribute,
    // and this stage honours the result. A no-op in q2-preview, where
    // crossref-render is excluded and `Equation.tsx` numbers client-side.
    stages.push(Box::new(EquationNumberStage::new()));
    // Native MathML (bd-3evfzwal): under `html-math-method: mathml`,
    // convert every `Inline::Math` to `<math>` with quarto-math. Runs
    // after equation-number (the number is already a sibling label, so
    // the TeX is the author's) and before math-js, which loads MathJax
    // only for the expressions this stage had to leave as TeX.
    stages.push(Box::new(MathMlStage::new()));
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

/// Render QMD content through the Pandoc-hybrid leg (docx/pptx today).
///
/// Sibling of [`render_qmd_to_html`], using [`build_pandoc_pipeline_stages`]
/// instead of the HTML stage list. The pandoc subprocess writes the output
/// file directly; the returned [`RenderedOutput::content`] is always empty
/// (Finding 3's explicit decision — no binary bytes travel through
/// `PipelineData`).
///
/// Returns the pipeline's diagnostics alongside the rendered output —
/// P7-foundation Task 3's first real caller (`render_document_to_file`)
/// needs `PandocWriteStage`'s classified pandoc-stderr warnings (e.g.
/// `Q-11-1` "Could not fetch resource") to actually reach the CLI's
/// printed diagnostics, not be dropped on the floor.
///
/// Native-only: the Pandoc-hybrid leg shells out to a real `pandoc`
/// binary, which has no WASM equivalent.
///
/// # Errors
///
/// Returns an error if parsing, transforms, or the `pandoc` subprocess
/// fail.
#[cfg(not(target_arch = "wasm32"))]
pub async fn render_qmd_to_pandoc(
    content: &[u8],
    source_name: &str,
    ctx: &mut RenderContext<'_>,
    runtime: Arc<dyn quarto_system_runtime::SystemRuntime>,
) -> Result<(crate::stage::RenderedOutput, Vec<DiagnosticMessage>)> {
    let stages = build_pandoc_pipeline_stages(ctx.format.identifier);
    let (output, diagnostics) = run_pipeline(content, source_name, ctx, runtime, stages).await?;
    let rendered = output.into_rendered_output().ok_or_else(|| {
        crate::error::QuartoError::Other(
            "Pandoc pipeline did not produce RenderedOutput".to_string(),
        )
    })?;
    Ok((rendered, diagnostics))
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
/// bd-nxslt: `CodeHighlightStage` is **included** in q2-preview
/// (it's AST-level — annotates `data-hl-spans` on the existing
/// `CodeBlock` / inline `Code` nodes; the React renderer in
/// `ts-packages/preview-renderer/src/q2-preview/blocks/CodeBlock.tsx`
/// reads the attribute and emits the highlighted `<span>` markup).
///
/// The unknown-name validator
/// (`q2_preview_stage_excluded_names_exist_in_html_pipeline`)
/// fails the test suite if any name here is not an actual stage in
/// the full HTML pipeline (typo / rename guard).
const Q2_PREVIEW_STAGE_EXCLUDED: &[&str] =
    &["math-ml", "math-js", "render-html-body", "apply-template"];

/// Build the q2-preview pipeline stages (Plan 1).
///
/// Constructed as [`build_html_pipeline_stages_with_options`] with
/// the names in [`Q2_PREVIEW_STAGE_EXCLUDED`] removed. Order is
/// preserved.
///
/// `AstTransformsStage` runs in both pipelines; it dispatches at
/// run-time on the [`crate::format::PipelineProfile`] derived from
/// `ctx.format.target_format` between `build_transform_pipeline` (render)
/// and `build_q2_preview_transform_pipeline` (preview).
///
/// **bd-lucp:** an optional `capture` slot inserts a
/// [`CaptureSpliceStage`](crate::stage::CaptureSpliceStage) between
/// `PreEngineSugaringStage` and `EngineExecutionStage`. When a
/// capture is supplied, the splice replaces engine code cells in
/// the AST with the server-recorded post-engine output blocks (keyed
/// by `(structural_hash, occurrence_index)`); `EngineExecutionStage`
/// then sees an AST with no engine cells left and the WASM
/// fallback-to-markdown path is a clean no-op. When `capture` is
/// `None`, the stage is still inserted but runs as a pass-through.
/// See `crates/quarto-core/src/engine/capture_splice.rs` and
/// `claude-notes/plans/2026-05-18-q2-preview-project-replay-engine.md`.
pub fn build_q2_preview_pipeline_stages(
    captures: Vec<quarto_trace::EngineCapture>,
) -> Vec<Box<dyn PipelineStage>> {
    // Build the base list; the engine registry comes from ctx.registry
    // (set by run_pipeline from project.registry or the override seam).
    // We still reconstruct the engine-execution stage ourselves so it can
    // carry the spliced-engine set (bd-sauc9iiq).
    let mut stages = build_html_pipeline_stages_with_options(None);
    stages.retain(|s| !Q2_PREVIEW_STAGE_EXCLUDED.contains(&s.name()));
    insert_capture_splice_stage(&mut stages, captures);
    stages
}

/// Names of stages in [`build_html_pipeline_stages_with_options`] that a
/// Pandoc-writer output format (`PipelineProfile::Pandoc(_)` — docx, pptx,
/// …) drops. This is a **stage**-level exclude-list — a separate mechanism
/// from [`PANDOC_TRANSFORM_EXCLUDED`], which excludes individual
/// `AstTransform`s. Stages are the coarser, macro-level render steps
/// (theme CSS compilation, JS artifact injection, syntax highlighting,
/// HTML body rendering, template application); none of these has a
/// Pandoc-writer analog, since Pandoc's own writer produces the final
/// document directly from the AST with no HTML chrome to decorate.
///
/// **Jointly owned with the P4 plan.** This list is P1's half of the
/// contract — the const plus its two validators
/// (`pandoc_stage_excluded_names_exist_in_html_pipeline`,
/// `t6_2_pandoc_stage_list_produces_exact_surviving_name_list`). P4 owns
/// inserting `PandocWriteStage` and deciding the final stage composition
/// for a real Pandoc-writer render; this list is not necessarily final.
///
/// The unknown-name validator
/// (`pandoc_stage_excluded_names_exist_in_html_pipeline`) fails the test
/// suite if any name here is not an actual stage in the full HTML pipeline
/// (typo / rename guard) — same rationale as [`Q2_PREVIEW_STAGE_EXCLUDED`]'s
/// validator.
const PANDOC_STAGE_EXCLUDED: &[&str] = &[
    "compile-theme-css",
    "bootstrap-js",
    "clipboard-js",
    "tabsets-js",
    "code-highlight",
    "math-js",
    // `html-math-method` is an HTML option; MathMlStage is a no-op for
    // every other format (see its module docs) and is excluded here so
    // the Pandoc leg never hands a converted RawInline to pandoc.
    "math-ml",
    "render-html-body",
    "apply-template",
];

/// Build the stage list for a `PipelineProfile::Pandoc(_)` render (docx,
/// pptx, typst, …): [`build_html_pipeline_stages_with_options`] with the
/// names in [`PANDOC_STAGE_EXCLUDED`] removed, plus [`PandocWriteStage`]
/// appended as the tail (P4 Task 9) — the stage that serializes the
/// wire-format AST and shells out to a real `pandoc` subprocess. Order is
/// preserved for the retained prefix.
///
/// `format_identifier` decides whether a further tail stage is needed:
/// typst is the one Pandoc-hybrid format where `PandocWriteStage`'s output
/// (`.typ` source) is not the final artifact — pandoc-hybrid-typst Phase 2
/// appends [`TypstCompileStage`] after it, which compiles that intermediate
/// file to the real PDF. docx/pptx get no further stage: pandoc's own
/// output there already is the final artifact.
///
/// The AST-transform exclude-list *within* `AstTransformsStage` (which
/// individual transforms should not run for a `Pandoc(fmt)` profile) is a
/// separate mechanism from this stage-level list and is `seam deferred
/// until P1's PipelineProfile work` — this function owns only which
/// **stages** run.
#[cfg(not(target_arch = "wasm32"))]
pub fn build_pandoc_pipeline_stages(
    format_identifier: crate::format::FormatIdentifier,
) -> Vec<Box<dyn PipelineStage>> {
    let mut stages = build_html_pipeline_stages_with_options(None);
    stages.retain(|s| !PANDOC_STAGE_EXCLUDED.contains(&s.name()));
    stages.push(Box::new(PandocWriteStage::new()));
    if format_identifier == crate::format::FormatIdentifier::Typst {
        stages.push(Box::new(TypstCompileStage::new()));
    }
    stages
}

/// Insert a [`crate::stage::CaptureSpliceStage`] immediately *before*
/// `EngineExecutionStage`, rebuilding that stage with `engine_registry` plus the
/// captured engine names.
///
/// bd-lucp / bd-5yff4: the splice folds an ordered capture sequence (one per
/// engine) into the AST, replacing engine code cells with their recorded
/// output. bd-sauc9iiq: rebuilding the engine stage with
/// `.with_spliced_engines(...)` suppresses the misleading "(no execution)"
/// warning for exactly the engines the splice already served (the WASM preview
/// registry has no knitr/jupyter).
///
/// Shared by the q2-preview pipeline and the HTML capture pipeline
/// ([`build_html_pipeline_stages_with_captures`], bd-uy4uygha) so the two stay
/// cell-aligned with `build_capture_pipeline_stages`. With empty `captures` the
/// inserted splice is a pass-through and the engine stage is still (re)built
/// with the registry, so callers may invoke this unconditionally.
fn insert_capture_splice_stage(
    stages: &mut Vec<Box<dyn PipelineStage>>,
    captures: Vec<quarto_trace::EngineCapture>,
) {
    let spliced_engine_names: std::collections::HashSet<String> =
        captures.iter().map(|c| c.engine_name.clone()).collect();

    // Reconstruct the engine-execution stage with the spliced-engine set.
    // The registry is no longer a stage-level concern — it is read from
    // ctx.registry at run time (Task 8 of ts-engine-extensions).
    let engine_stage = EngineExecutionStage::new().with_spliced_engines(spliced_engine_names);
    let engine_idx = stages
        .iter()
        .position(|s| s.name() == "engine-execution")
        .expect("engine-execution stage must exist in the pipeline");
    stages[engine_idx] = Box::new(engine_stage);
    let splice_stage: Box<dyn PipelineStage> =
        Box::new(crate::stage::CaptureSpliceStage::new().with_captures(captures));
    stages.insert(engine_idx, splice_stage);
}

/// Like [`build_html_pipeline_stages_with_options`] but splices server-recorded
/// engine captures into the HTML render (bd-uy4uygha): hub-client's default
/// `format: html` preview shows the output of a document executed by a connected
/// `q2 provide-hub`, without re-running the engine in the browser.
///
/// With empty `captures` the result is behaviorally identical to
/// `build_html_pipeline_stages_with_options` (a pass-through splice + the same
/// engine stage). Cell alignment with the recorded capture is guaranteed because
/// captures are recorded from this same stage list truncated at engine-execution
/// (`build_capture_pipeline_stages`).
pub fn build_html_pipeline_stages_with_captures(
    apply_config: Option<ApplyTemplateConfig>,
    captures: Vec<quarto_trace::EngineCapture>,
) -> Vec<Box<dyn PipelineStage>> {
    // The engine registry is not a builder concern — it flows via
    // `ctx.registry` at run time (Task 8 of ts-engine-extensions). This helper
    // rebuilds the engine stage only to carry the spliced-engine set.
    let mut stages = build_html_pipeline_stages_with_options(apply_config);
    insert_capture_splice_stage(&mut stages, captures);
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
///   `FootnotesTransform`, `FootnotesResolveTransform` — render-shape
///   transforms that don't affect the outline.
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
        // Localized-term resolution — keeps analysis-path transforms in
        // sync with the render pipelines once they consume terms
        // (bd-llhlzd7p).
        Box::new(LanguageResolveStage::new()),
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
    // Attribution: forward the opt-in provider from the outer ctx
    // so `AttributionGenerateTransform` (inside `AstTransformsStage`)
    // sees it. `None` is the default and means "attribution off".
    stage_ctx.attribution_provider = ctx.attribution_provider.clone();
    // bd-o8pr Phase 2: transfer the per-doc resource report into
    // the stage context so engine + filter stages can append to it.
    stage_ctx.resource_report = std::mem::take(&mut ctx.resource_report);
    // bd-cfl67: same shape for resource-copy intents (image / asset
    // copies collected by AST transforms). The outer renderer drains
    // these into the sink after the pipeline returns.
    stage_ctx.resource_copies = std::mem::take(&mut ctx.resource_copies);
    // Apply the engine registry override when the caller has supplied one
    // (test seam / replay path).  Mirrors how `project_index` and
    // `resource_resolver` are threaded at the lines above — the default
    // project registry is already in `stage_ctx.registry` from
    // `StageContext::new()`; this replaces it only when an override is set.
    if let Some(override_reg) = &ctx.engine_registry_override {
        stage_ctx.registry = override_reg.clone();
    }
    stage_ctx.execution_policy = ctx.execution_policy.clone();

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
    // bd-cfl67: bridge transform-collected copy intents back to the
    // outer renderer for the sink-flush step.
    ctx.resource_copies = stage_ctx.resource_copies;
    // Transfer writer-side `format_options` populated by transforms
    // running inside the pipeline (e.g. `AttributionRenderTransform`
    // writes `attribution_by_node` / `attribution_actors` here). The
    // q2-preview JSON writer runs *outside* `AstTransformsStage`,
    // so it reads the populated data from the outer ctx after the
    // pipeline returns. Pre-pipeline callers don't write
    // `ctx.format_options`, so the overwrite is safe.
    ctx.format_options = stage_ctx.format_options;
    // Bridge the document profile stashed by `UnwrapProfileStage`
    // (bd-0rsk07il) so response builders can read it after a full
    // render. `None` for pipelines that stop before the unwrap stage.
    ctx.document_profile = stage_ctx.document_profile;
    ctx.execution_skipped = stage_ctx.execution_skipped;

    // Apply the `diagnostics:` suppression policy resolved by
    // `MetadataMergeStage`. This is deliberately the *only* place
    // suppression happens: every per-document diagnostic — from stages,
    // transforms, pampa, or Lua filters — leaves the pipeline through
    // here, and every frontend (`quarto render`, `q2 preview`,
    // hub-client) reads it from here. Doing it inside the render also
    // puts it strictly before `--strict`'s promotion at the CLI summary
    // boundary, so a suppressed warning stays suppressed rather than
    // reappearing as an error (bd-lone-bracket-diagnostic-mxu41qbt).
    //
    // Both exits pass through it. A failing stage can carry warnings
    // collected before it out through its error — an unexpandable
    // include does exactly that
    // (bd-include-parse-failure-dropped-u4rdjxru) — and a suppressed
    // warning must not reappear just because the document later failed.
    // Errors are never suppressible, so the failure itself always
    // survives.
    let policy = stage_ctx.diagnostic_policy;

    // Plan 7c seam 2: the conversion provenance ParseDocumentStage stashed on
    // the stage context, consumed by the StageError arm below to rebuild a
    // SourceContext that matches what a successful parse would have produced.
    let conversion_stash = stage_ctx.conversion_stash.take();

    result
        .map_err({
            let policy = policy.clone();
            move |e| match e {
                // Already-structured errors (built by stages that know
                // their span lives outside the document — see
                // `theme_diagnostic`). Pass through with the stage's own
                // SourceContext so the ariadne renderer can resolve
                // cross-file references.
                crate::stage::PipelineError::Structured(mut pe) => {
                    policy.apply(&mut pe.diagnostics);
                    crate::error::QuartoError::Parse(pe)
                }
                crate::stage::PipelineError::StageError {
                    mut diagnostics, ..
                } if !diagnostics.is_empty() => {
                    policy.apply(&mut diagnostics);
                    // Create a SourceContext for the parse error
                    let mut source_context = SourceContext::new();
                    let content_str = String::from_utf8_lossy(content).to_string();
                    match conversion_stash {
                        // The parse ran on engine-converted text, so FileId(0)
                        // must be the converted buffer under the same
                        // synthetic name ParseDocumentStage uses — otherwise
                        // diagnostics pointing into the converted text are
                        // labeled with the original file's name. When the
                        // converter also produced a faithful mapping,
                        // registering converted → original → cells *in that
                        // order* lands each file on the exact FileId the
                        // Concat pieces point at (ORIGINAL_FILE_ID, then the
                        // contiguous cells), because `add_file` assigns ids
                        // sequentially.
                        Some(stash) => {
                            source_context.add_file(
                                format!("<{source_name} (converted by {})>", stash.engine),
                                Some(stash.converted),
                            );
                            if stash.source_info.is_some() {
                                source_context.add_file(source_name.to_string(), Some(content_str));
                                for (i, cell_file) in stash.files.into_iter().enumerate() {
                                    let file_id = source_context
                                        .add_file(cell_file.label, Some(cell_file.text));
                                    // Mirror ParseDocumentStage's success-path
                                    // registration (plan 7c Phase 4): the
                                    // rebuilt context must match what a
                                    // successful parse would have produced.
                                    if let Some(f) = source_context.get_file_mut(file_id) {
                                        f.metadata.origin =
                                            Some(quarto_source_map::FileOrigin::NotebookCell {
                                                notebook_path: source_name.to_string(),
                                                cell_index: i + 1,
                                                cell_id: cell_file.cell_id,
                                                cell_type: cell_file.cell_type,
                                            });
                                    }
                                }
                            }
                        }
                        None => {
                            source_context.add_file(source_name.to_string(), Some(content_str));
                        }
                    }
                    crate::error::QuartoError::Parse(crate::error::ParseError::new(
                        diagnostics,
                        source_context,
                    ))
                }
                other => crate::error::QuartoError::Other(other.to_string()),
            }
        })
        .map(|d| {
            let mut diagnostics = stage_ctx.diagnostics;
            policy.apply(&mut diagnostics);
            (d, diagnostics)
        })
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

    // Plan 7c seam 2: carry the stage-populated SourceContext — for a
    // converted document it includes the converted buffer, the original
    // file, and the per-cell virtual files. Byte-identical to the previous
    // single-file context for plain .qmd.
    let source_context = ast.source_context;

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
    // Thread the optional engine registry override from `HtmlRenderConfig`
    // onto `RenderContext` so `run_pipeline` can apply it to `StageContext`
    // after `StageContext::new()` populates the default project registry.
    // Cloning the Arc is cheap; `HtmlRenderConfig` is borrowed `&`.
    ctx.engine_registry_override = config.engine_registry.clone();
    ctx.execution_policy = config.execution_policy.clone();
    let apply_config = config
        .resolver
        .clone()
        .map(|r| ApplyTemplateConfig::new().with_resolver(r));
    // bd-uy4uygha: when the caller supplies server-recorded captures (hub-client
    // executing via a connected `q2 provide-hub`), splice them into the HTML.
    // Empty captures take the unchanged builder — byte-identical for every
    // existing caller (`q2 render`, which runs the real engine natively).
    // The engine registry is NOT threaded through the builders — it flows via
    // `ctx.engine_registry_override` (set above) into `ctx.registry` at run time
    // (Task 8 of ts-engine-extensions).
    let stages = if config.captures.is_empty() {
        build_html_pipeline_stages_with_options(apply_config)
    } else {
        build_html_pipeline_stages_with_captures(apply_config, config.captures.clone())
    };

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
        execution_skipped: ctx.execution_skipped,
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
///   set so `LinkRewriteTransform` rewrites link and image URLs to
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
    engine_registry: Option<std::sync::Arc<crate::engine::EngineRegistry>>,
    captures: Vec<quarto_trace::EngineCapture>,
) -> Result<PreviewAstOutput> {
    // Capture the untransformed AST before any pipeline stage runs.
    // This is the baseline `incremental_write` reconciles against (plan:
    // 2026-06-04-target-incremental-writes.md, Phase 1).  We parse the
    // content a second time rather than intercepting the pipeline's own
    // parse so that the returned JSON is wholly self-contained (its own
    // source-info pool) and independent of the main pipeline's run.
    let untransformed_ast_json = capture_untransformed_ast_json(content, source_name);

    // Thread the optional engine registry override onto `RenderContext` so
    // `run_pipeline` can apply it to `StageContext` after populating the
    // default project registry (same seam as `HtmlRenderConfig.engine_registry`
    // in `render_qmd_to_html`).  Production callers leave it `None`.
    ctx.engine_registry_override = engine_registry;

    // The q2-preview stage list excludes `CodeHighlightStage` /
    // `RenderHtmlBodyStage` / `ApplyTemplateStage`, so the
    // pipeline returns `DocumentAst`, not `RenderedOutput`.
    //
    // bd-lucp: `captures` is the preview-time consumer. When non-empty,
    // [`CaptureSpliceStage`] inside the pipeline splices the recorded
    // engine output into the live AST before `EngineExecutionStage` runs
    // (which then no-ops via the WASM markdown fallback). See
    // `claude-notes/plans/2026-05-18-q2-preview-project-replay-engine.md`.
    let stages = build_q2_preview_pipeline_stages(captures);

    let (output, diagnostics) = run_pipeline(content, source_name, ctx, runtime, stages).await?;
    let ast = output.into_document_ast().ok_or_else(|| {
        crate::error::QuartoError::Other(
            "q2-preview pipeline did not produce DocumentAst".to_string(),
        )
    })?;

    // Plan 7c seam 2: carry the stage-populated SourceContext — for a
    // converted document it includes the converted buffer, the original
    // file, and the per-cell virtual files, so preview diagnostics resolve
    // into per-cell locations. Byte-identical to the previous single-file
    // context for plain .qmd.
    let source_context = ast.source_context;

    // Build an `ASTContext` from the source context — the JSON
    // writer needs this to emit `[file_id, start, end]` source-
    // location triples for inlines (`include_inline_locations:
    // true`). This shape is lifted verbatim from the q2-debug
    // entry point (`wasm-quarto-hub-client/src/lib.rs:914-916`)
    // so q2-preview's JSON envelope matches q2-debug's at the
    // wire level. (`filenames` stays the single original name: the
    // writer interns Substring parent chains from the AST nodes' own
    // embedded SourceInfo Arcs, not from this list.)
    let ast_context = pampa::pandoc::ASTContext {
        filenames: vec![source_name.to_string()],
        example_list_counter: std::cell::Cell::new(1),
        source_context: source_context.clone(),
        parent_source_info: None,
    };
    // When `AttributionRenderTransform` ran (i.e. a provider was
    // installed on `ctx.attribution_provider`), forward
    // `ctx.format_options.json` into `JsonConfig` so the writer emits
    // `astContext.attribution` and `astContext.attributionActors`.
    // Off-path (provider absent), both fields stay `None` and the
    // JSON output is byte-identical to today's.
    let (attribution_by_node, attribution_actors) =
        crate::attribution::json_attribution_fields(&ctx.format_options.json);
    let json_config = pampa::writers::json::JsonConfig {
        include_inline_locations: true,
        attribution_by_node,
        attribution_actors,
        ..Default::default()
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
        untransformed_ast_json,
        diagnostics,
        source_context,
    })
}

/// Parse `content` with `qmd_to_pandoc` and serialize the result to JSON.
///
/// Returns `Some(json)` on success, `None` if parsing or serialization fails
/// (the main pipeline will surface those errors through its own path; we
/// silently degrade to `None` here so a parse error does not prevent the
/// transformed AST from being returned).
///
/// The resulting JSON is independent of the main pipeline's source-info pool —
/// it has its own pool with the same values, so `source_info` equality holds
/// by value (which is all the lookup in `apply_node_edit` requires).
fn capture_untransformed_ast_json(content: &[u8], source_name: &str) -> Option<String> {
    let (ast, context) = pampa::wasm_entry_points::qmd_to_pandoc(content).ok()?;

    let ast_context = pampa::pandoc::ASTContext {
        filenames: vec![source_name.to_string()],
        example_list_counter: std::cell::Cell::new(1),
        source_context: context.source_context.clone(),
        parent_source_info: None,
    };
    let json_config = pampa::writers::json::JsonConfig {
        include_inline_locations: true,
        ..Default::default()
    };
    let mut buf = Vec::new();
    pampa::writers::json::write_with_config(&ast, &ast_context, &mut buf, &json_config).ok()?;
    String::from_utf8(buf).ok()
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
/// 4d. `DraftAlertTransform` - For `draft: true` pages, set the localized
///     `rendered.draft-alert-text` the template's `#quarto-draft-alert`
///     banner gates on, and append a `quarto:status` meta tag
///     (bd-draft-banner-missing-hgx1gkqm)
/// 5. `TitleBlockTransform` - Add title header from metadata if not present
/// 6. `SectionizeTransform` - Wrap headers in section Divs (for HTML semantic structure)
/// 7. `FootnotesTransform` - Resolve footnote refs/defs into native `Inline::Note`s
/// 7a. `FootnotesResolveTransform` - Resolve `Inline::Note`s into HTML footnote chrome
///     (excluded for `Pandoc(_)` profiles — pandoc's own writer numbers/places `Note`s)
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
/// 13a. `RepoActionsRenderTransform` - Render repository action links
///     (source/edit/issue) for the TOC and footer slots
///     (bd-repo-actions-missing-99ezd2fe)
/// 14. `NavbarRenderTransform` - Render navbar to HTML for template insertion
/// 15. `SidebarRenderTransform` - Render sidebar to HTML (w/ .qmd→.html rewrite)
/// 15a. `BreadcrumbsRenderTransform` - Derive the page's breadcrumb trail from
///     `navigation.sidebar` into `rendered.navigation.breadcrumbs`
/// 16. `FooterRenderTransform` - Render page footer to HTML for template insertion
/// 16a. `AttributionGenerateTransform` - Tail-of-phase: call the installed
///     `AttributionSourceProvider` (if any) and merge identities into the
///     `RenderContext` sidecar for the Render-side transform to read
///
/// ## Finalization Phase
/// 17. `LinkRewriteTransform` - Rewrite body-content `.qmd` links to relative output URLs (Phase 6)
/// 18. `AppendixStructureTransform` - Consolidate appendix content into container
/// 19. `CrossrefRenderTransform` - Resolve crossref custom nodes to final HTML structure
/// 20. `ResourceCollectorTransform` - Collect image dependencies
/// Select the format-specific footer-render stage.
///
/// Footer *generation* is format-agnostic (`FooterGenerateTransform` →
/// `navigation.footer`); footer *rendering* is not. `format: html` emits
/// page-footer chrome into `rendered.navigation.footer`; `format: revealjs`
/// emits a deck-level `.footer`/`.slide-logo` into `rendered.reveal.*` (and so
/// must *not* run the html render, whose "skip if slot populated" hook keys a
/// different slot). This is the one place that maps format → render stage; see
/// the call site for why it's the seam a future format-driven composition layer
/// would grow from.
fn footer_render_stage(is_revealjs: bool) -> Box<dyn crate::transform::AstTransform> {
    if is_revealjs {
        Box::new(crate::revealjs::RevealFooterLogoTransform::new())
    } else {
        Box::new(FooterRenderTransform::new())
    }
}

/// The format-specific *presentation* transforms that run in the Finalization
/// phase, after the crossref custom nodes have been rendered to writer-visible
/// shapes (`CrossrefRenderTransform`).
///
/// This is the sibling of [`footer_render_stage`] for the *presentation* slot:
/// the one named place that maps format → which late, semantics-consuming
/// transforms run (and lets the call site keep its `is_revealjs` checks from
/// scattering). Today the only member is revealjs auto-stretch, which must see
/// the final `Figure` produced by crossref-render so a single-image crossref
/// figure is numbered/resolved *before* it is hoisted to `section > img.r-stretch`
/// (bd-w0c6d38k). A new format that needs late, float-aware reshaping (e.g.
/// `dashboard`, `typst`) adds its transforms here rather than inline.
///
/// These are `TransformPhase::Finalization` transforms; the phase-ordering
/// invariant (`test_build_transform_pipeline_phase_ordering`) keeps them after
/// the `Crossref` phase.
fn reveal_finalization_transforms(
    is_revealjs: bool,
) -> Vec<Box<dyn crate::transform::AstTransform>> {
    if is_revealjs {
        vec![Box::new(crate::revealjs::RevealAutoStretchTransform::new())]
    } else {
        Vec::new()
    }
}

pub fn build_transform_pipeline(
    shortcode_paths: Vec<std::path::PathBuf>,
    extensions: Vec<crate::extension::types::Extension>,
    runtime: std::sync::Arc<dyn quarto_system_runtime::SystemRuntime>,
    target_format: String,
    pipeline_profile: crate::format::PipelineProfile,
    variables: Option<quarto_pandoc_types::ConfigValue>,
    project_env: hashlink::LinkedHashMap<String, String>,
    quarto_profile: Option<String>,
) -> TransformPipeline {
    let mut pipeline: TransformPipeline = TransformPipeline::new();

    // Family axis, from the single derivation point (`PipelineProfile`):
    // true for `RevealjsRender` (native render) and `RevealjsPreview`
    // (`q2-slides`).
    let is_revealjs = matches!(
        pipeline_profile,
        crate::format::PipelineProfile::RevealjsRender
            | crate::format::PipelineProfile::RevealjsPreview
    );

    // The Lua engines (shortcodes, user filters) see the *canonical* Pandoc
    // format as their `FORMAT` global, not q2's preview pseudo-format. Under
    // preview, `target_format` is `q2-preview` / `q2-slides`, which Lua's
    // `is_format("html:js")` / `is_format("revealjs")` don't recognize — so
    // format-gated shortcodes degrade (the `{{< video >}}` → plain-link bug,
    // bd-5b21rbaq). Normalizing here makes preview Lua behave like render.
    let lua_format = crate::format::lua_format_for(&target_format).to_string();

    // === NORMALIZATION PHASE ===
    // Conditional content runs FIRST: hidden content must disappear
    // before callouts assemble, shortcodes resolve (no spurious
    // warnings from deliberately-excluded content), and long before
    // crossref numbering (bd-fu16z22k Phase 4).
    pipeline.push(Box::new(ConditionalContentTransform::new()));
    // Reference-link diagnostics (bd-reference-links-unsupported-ddc4skac):
    // warn about `[label][ref]` and `[ref]: url` lines, which qmd does not
    // support and which were previously silent. Read-only. Runs immediately
    // after conditional content — for the same reason shortcodes do, so
    // deliberately-excluded content cannot raise spurious warnings — but
    // before any sugaring rewrites spans, so it still sees the document
    // essentially as the author wrote it.
    pipeline.push(Box::new(ReferenceLinkDiagnosticsTransform::new()));
    pipeline.push(Box::new(CalloutTransform::new()));
    pipeline.push(Box::new(CalloutResolveTransform::new()));
    // Panel-tabset pair (bd-toc-tabset-titles-zq93gjvf), mirroring the
    // callout pair above. Position is load-bearing: the parse half
    // consumes the tab-title Headers *before* SectionizeTransform and
    // TocGenerateTransform run, which is what keeps tab titles out of
    // sections and the TOC (Q1 does the same by filter ordering).
    // Both self-gate to non-reveal, non-minimal HTML.
    pipeline.push(Box::new(crate::transforms::PanelTabsetTransform::new()));
    pipeline.push(Box::new(
        crate::transforms::PanelTabsetResolveTransform::new(),
    ));
    // Markdown-parse blessed website presentation config strings
    // (website.title, page-footer regions, …) so the shortcode
    // transform's metadata walk — registered immediately after — sees
    // live Shortcode/RawInline nodes instead of literal scalars
    // (bd-shortcodes-in-metadata-bp06aub8).
    pipeline.push(Box::new(crate::transforms::ConfigMarkdownTransform::new()));
    pipeline.push(Box::new(ShortcodeResolveTransform::with_lua_support(
        shortcode_paths,
        extensions,
        runtime.clone(),
        lua_format,
        variables,
        project_env,
        quarto_profile,
    )));
    pipeline.push(Box::new(MetadataNormalizeTransform::new()));
    // Date normalization (bd-gx9cic8z P4): resolves today/now/
    // last-modified, writes ISO `date-meta`/`date-modified-meta` for
    // machine slots, and replaces `date`/`date-modified` with the
    // formatted string (Q1's pre-Pandoc rewrite + forced `long` for
    // the styled HTML title block). Runs before AuthorsNormalize so
    // every downstream consumer sees formatted dates.
    pipeline.push(Box::new(DateNormalizeTransform::new(runtime.clone())));
    // Author/label normalization (bd-gx9cic8z P1): derives `by-author`,
    // `labels`, and `rendered.has-title-block` from raw metadata for
    // the title-block template partial AND the q2-preview React title
    // block (which reads the same metadata keys). Runs right after
    // metadata-normalize; format-agnostic like Q1's authors.lua pass.
    pipeline.push(Box::new(AuthorsNormalizeTransform::new()));
    // Title-block banner mode (bd-gx9cic8z P5): derives
    // `rendered.title-block-banner` (the template's banner gate) and,
    // for explicit banner colors/images, pushes the generated
    // include-in-header <style> + image ResourceCopyIntent. HTML-only
    // (self-gated on `ctx.format.is_html_based()`).
    pipeline.push(Box::new(TitleBannerTransform::new(runtime.clone())));
    // bd-1tl09 Phase 0: code-block decoration Generate runs after
    // metadata-normalize so document-level defaults (e.g.
    // `code-copy: true`) are visible when computing per-block
    // decorations. The matching Render half lives in the
    // Finalization Phase below. Phase 0 implementation is a no-op
    // walker; Phases 1–3 fill in filename / copy / fold.
    pipeline.push(Box::new(CodeBlockGenerateTransform::new()));
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
    // User-declared stylesheets (bd-format-css-not-copied-crn3bjdz):
    // copy `css:` files into the output tree and rewrite the entries
    // to per-page hrefs. Not website-scoped — default projects (and
    // books, which ride the same dispatch) and single-doc renders get
    // the same treatment. Self-gates to HTML-family formats.
    pipeline.push(Box::new(crate::transforms::FormatCssTransform::new()));
    // Draft marking (bd-draft-banner-missing-hgx1gkqm). Not website-scoped
    // — a standalone `draft: true` document gets the banner too — but it
    // belongs with the metadata producers above: it only writes
    // `rendered.draft-alert-text` and a `quarto:status` header include,
    // both consumed later (by the template and `IncludeResolveStage`
    // respectively). Self-gates to non-reveal HTML.
    pipeline.push(Box::new(DraftAlertTransform::new()));
    // Slide construction for `format: revealjs` replaces the generic
    // title-block + sectionize pair: reveal needs an exactly-two-level slide
    // tree built from `slide-level` (Pandoc keeps reveal slide-construction
    // separate from its `--section-divs` machinery; so do we). See
    // claude-notes/plans/2026-06-08-revealjs-presentations.md.
    if is_revealjs {
        // Columns: rewrite `.column width=X` → `flex-basis` before slide
        // construction (the column Divs are still flat at this point).
        pipeline.push(Box::new(crate::revealjs::RevealColumnsTransform::new()));
        pipeline.push(Box::new(crate::revealjs::RevealSlidesTransform::new()));
        // Alias Quarto-1's reveal `footer:` → `page-footer:` so it flows
        // through the format-agnostic `FooterGenerateTransform` below. Must run
        // before that generate (and it only touches metadata, so its position
        // relative to slide construction is irrelevant).
        pipeline.push(Box::new(crate::revealjs::RevealFooterAliasTransform::new()));
    } else {
        pipeline.push(Box::new(TitleBlockTransform::new()));
        pipeline.push(Box::new(SectionizeTransform::new()));
    }
    pipeline.push(Box::new(FootnotesTransform::new()));
    // Immediately after: the HTML-chrome half consuming the `Inline::Note`s
    // this produces. Excluded for `Pandoc(_)` profiles below (a Pandoc
    // writer numbers/places `Note`s itself) — `FootnotesTransform` is not,
    // so its `Inline::Note`s survive to pandoc's own writer.
    pipeline.push(Box::new(FootnotesResolveTransform::new()));
    if is_revealjs {
        // Per-slide footnote/aside coalescing consumes FootnotesResolveTransform's
        // resolved output (refs = `Span#fnrefN`, defs in the trailing
        // `Div#footnotes`), so it must run *after* it. Pure AST → benefits
        // render and preview alike. See `revealjs::footnotes`. This is a
        // `Normalization`-phase transform: it builds slide scaffolding and does
        // not consume crossref semantics.
        pipeline.push(Box::new(crate::revealjs::RevealFootnotesTransform::new()));
        // NOTE: revealjs auto-stretch is NOT here. It is a `Finalization`-phase
        // transform — it consumes the *rendered* `Figure` shape that
        // `CrossrefRenderTransform` produces, so running it this early would
        // hoist a crossref figure to a bare `<img>` before the float is ever
        // numbered (bd-w0c6d38k). It is spliced in after `CrossrefRenderTransform`
        // via `reveal_finalization_transforms` below.
    }
    // Example-iframe embeds (bd-z1smhvuo / bd-t3cert81). Sugars
    // `Div.embed-example-iframe[file=…]` into a `CustomNode("ExampleEmbed")`.
    // Runs *before* the theorem/float sugar so a `#demo-…` example div is
    // consumed here and never claimed as a generic float (`demo` is a
    // registered ref-type, so `FloatRefTargetSugarTransform` would otherwise
    // grab it). When the id is `demo-…` the node carries the crossref triple,
    // so the CROSSREF PHASE below numbers it; the matching render step runs in
    // the Finalization Phase, after `CrossrefRenderTransform`. See
    // `claude-notes/plans/2026-06-09-crossreferenceable-examples.md`.
    pipeline.push(Box::new(ExampleEmbedTransform::new()));
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
    // Placement decision for the rendered TOC (bd-e2kpwy7n): must run
    // after TocRenderTransform (it gates on `rendered.navigation.toc`)
    // and before SidebarRenderTransform (which consumes the
    // `toc-in-sidebar` directive to merge the TOC into
    // `nav#quarto-sidebar` for website pages).
    pipeline.push(Box::new(TocLocationTransform::new()));
    // Repository action links (bd-repo-actions-missing-99ezd2fe).
    // Ordering is load-bearing in both directions.
    //
    // AFTER `TocRenderTransform` (:1358): the TOC copy is emitted only
    // when `rendered.navigation.toc` is non-empty, and Q-13-13 is
    // gated on the same flag.
    //
    // BEFORE `SidebarRenderTransform` (:1366): for the website-left
    // placement the TOC's `<nav>` is built in Rust, by `toc_block_html`
    // — called from `SidebarRenderTransform`, its only caller. Running
    // after it would leave that one placement with no actions while the
    // three template-emitted placements worked, a failure no test using
    // the default `toc-location: right` can see.
    //
    // BEFORE `footer_render_stage` (:1396): `FooterRenderTransform`
    // consumes `rendered.navigation.footer-actions`.
    //
    // Not gated on format: revealjs's template includes neither the
    // `toc-block` partial nor the html footer stage, so both slots are
    // inert there.
    pipeline.push(Box::new(RepoActionsRenderTransform::new()));
    pipeline.push(Box::new(NavbarRenderTransform::new()));
    // MUST come after NavbarRenderTransform: the sidebar-title gate
    // (bd-sidebar-title-with-navbar-82wxow6m) suppresses the sidebar's
    // own title when the page has a navbar, and its predicate reads the
    // `rendered.navigation.navbar` key NavbarRenderTransform writes —
    // the same signal the template's header gate uses. Registered
    // unconditionally on both native and WASM: the preview renders the
    // sidebar fragment too, so gating it on target would make the
    // preview disagree with `render` about the title.
    pipeline.push(Box::new(SidebarRenderTransform::new()));
    // Breadcrumbs derive from the resolved `navigation.sidebar`
    // (bd-breadcrumbs-missing-1vpuqh34); the title-block partial
    // consumes `rendered.navigation.breadcrumbs`.
    pipeline.push(Box::new(BreadcrumbsRenderTransform::new()));
    // The narrow-viewport secondary nav (bd-26bf3j1y) derives its own
    // trail from the same sidebar — Q1 gates the two breadcrumb
    // instances differently, so they don't share a result. See the
    // comparison table in `transforms/secondary_nav_render.rs`.
    //
    // Registered on BOTH native and WASM since bd-ersobfbt. It was
    // native-only at introduction (bd-26bf3j1y decision 3), on the
    // premise that the hub-client preview reinitialized its iframe
    // every render tick and shipped no Bootstrap JS — but the preview
    // iframe has been persistent since Phase F.1 (bd-kw93.14), which
    // injects Bootstrap's bundle at `entry.tsx` module top, so the
    // toggle works there. `PreviewDocument.tsx` renders the bar via
    // `SecondaryNavSlot` inside the `#quarto-header` wrapper.
    pipeline.push(Box::new(
        crate::transforms::SecondaryNavRenderTransform::new(),
    ));
    // Fixed-header JS (quarto-nav.js + headroom.min.js, bd-ersobfbt).
    // MUST come after NavbarRenderTransform and SecondaryNavRenderTransform:
    // its predicate reads the `rendered.navigation.{navbar,secondary-nav}`
    // keys those transforms write — the same signal the template's header
    // gate uses. Native-only: the preview excludes ApplyTemplateStage (no
    // `<script>` emission) and instead injects the same vendored files at
    // entry.tsx module top (Phase F.1 pattern).
    #[cfg(not(target_arch = "wasm32"))]
    pipeline.push(Box::new(crate::transforms::QuartoNavJsTransform::new()));
    pipeline.push(Box::new(PageNavRenderTransform::new()));
    // Footer *generation* (above) is format-agnostic; footer *rendering* is
    // format-specific — html emits page-footer chrome, revealjs emits a
    // deck-level `.footer`/`.slide-logo` into `rendered.reveal.*`. Selecting the
    // render stage by format here (rather than scattering `is_revealjs` checks)
    // is a deliberately small first step toward format-driven pipeline
    // composition: as more "non-`html` HTML formats" appear, this is the seam
    // where a format → stage-sequence mapping would grow.
    pipeline.push(footer_render_stage(is_revealjs));

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
    // Example-embed render (bd-t3cert81). Runs right after
    // `CrossrefRenderTransform` so the per-`demo` `order` the index assigned
    // is available: turns each `CustomNode("ExampleEmbed")` into the final
    // container — the `<iframe>` (page-relative src), a "Demo N: …" caption
    // when numbered, and the source link. Unknown to crossref-render (which
    // dispatches on FloatRefTarget/Theorem/Proof), so the node survives to
    // here untouched.
    pipeline.push(Box::new(ExampleEmbedRenderTransform::new()));
    // Format-specific presentation that consumes rendered crossref shapes:
    // revealjs auto-stretch. Runs *after* `CrossrefRenderTransform` (so a
    // single-image `![cap]{#fig-…}` figure is numbered/resolved/rendered to a
    // real `Figure` first) and *before* `ResourceCollectorTransform` (so the
    // hoisted `<img>` is still visible to resource collection). See
    // `reveal_finalization_transforms` and bd-w0c6d38k.
    pipeline.extend(reveal_finalization_transforms(is_revealjs));
    // bd-5m4ga0s1: replace ```mermaid code blocks with
    // `<pre class="mermaid">` RawBlocks + the after-body CDN script.
    // HTML-family self-gated (html + revealjs). Must precede
    // `code-block-render` so diagram blocks never grow copy-button /
    // filename chrome. Excluded from q2-preview via
    // `Q2_PREVIEW_TRANSFORM_EXCLUDED` — the React built-in mermaid
    // component consumes the raw CodeBlock there.
    pipeline.push(Box::new(MermaidRenderTransform::new()));
    // bd-1tl09 Phase 0: code-block decoration Render. Consumes the
    // typed payload produced by `code-block-generate` in the
    // Normalization Phase and emits the outer wrapping markup
    // (filename header, copy scaffold, <details> fold) around the
    // existing `CodeBlock`. Phase 0 is a no-op; Phases 1–3 fill it in.
    // Must run after any transform that creates or mutates code
    // blocks (shortcode expansion is upstream; resource-collector
    // does not touch code blocks).
    pipeline.push(Box::new(CodeBlockRenderTransform::new()));
    pipeline.push(Box::new(ResourceCollectorTransform::new()));
    // bd-3qych45b: render hephaestus plot documents (`![](plot.hep)`)
    // to SVG artifacts under `figure-html/` and point the image at
    // them. After `resource-collector` on purpose: the collector still
    // copies the `.hep` beside the page (a future live-reflow layer
    // wants it there) and never goes looking for the generated SVG in
    // the source tree. Before `responsive-image` so the rewritten
    // `<img>` is tagged `img-fluid` like any other. HTML-family
    // self-gated; native-only (the preview renders `.hep` with the npm
    // `hephaestus-svg-wasm` client in React) and listed in
    // `Q2_PREVIEW_TRANSFORM_EXCLUDED`.
    #[cfg(not(target_arch = "wasm32"))]
    pipeline.push(Box::new(crate::transforms::HephaestusRenderTransform::new(
        runtime.clone(),
    )));

    // bd-2c8rg: tag every <table> with Bootstrap's `caption-top` and
    // `table` classes so the rendered HTML picks up the project's
    // Bootstrap stylesheet (matches Quarto 1's
    // `quarto-bootstrap-table.lua`). Runs late: by this point every
    // upstream transform has finished mutating tables, and any future
    // user-filter slot inserted before AttributionRender still sees the
    // un-enriched class list. Idempotent.
    pipeline.push(Box::new(TableBootstrapClassTransform::new()));

    // bd-images-no-max-width-e5ywgnma: tag body images with Bootstrap's
    // `img-fluid` so an image the author never sized is capped at its
    // container's width instead of laying out at intrinsic pixel width
    // (matches Quarto 1's `quarto-post/responsive.lua`). Sits next to
    // the table pass above for the same reason it runs late: every
    // upstream transform has finished producing images by now, so
    // crossref-rendered figures are tagged too. Chrome emitted as raw
    // HTML (listing thumbnails, navbar logos) is not in `ast.blocks`
    // and stays untagged, as in Quarto 1. Idempotent, and self-gated
    // to HTML formats excluding revealjs and `minimal: true`.
    pipeline.push(Box::new(ResponsiveImageTransform::new()));

    // llms markdown capture (bd-llms-txt-unimplemented-oih6z6j7).
    // Runs after every content-mutating transform — crossref-render
    // has resolved numbers, link-rewrite has produced output hrefs,
    // code-block-render has finished — so the captured clone is the
    // final semantic content. Self-gated on `llms_view_active`
    // (website + `llms-txt: true` + html target); also the sole
    // consumer of the `.quarto-llms-{keep,omit}` marker classes
    // `conditional-content` plants under the same predicate, so it
    // must run whenever that transform does.
    pipeline.push(Box::new(crate::transforms::LlmsCaptureTransform::new()));

    // Very last transform: bake the per-node attribution lookup and
    // the pruned actors table onto `ctx.format_options`. No-op when
    // `ctx.attribution_data` is None (i.e. no provider was installed,
    // or generate skipped). Placing this at the very end means any
    // future finalization stage that mutates `SourceInfo` is
    // automatically covered without having to remember to insert it
    // before attribution-render.
    pipeline.push(Box::new(AttributionRenderTransform::new()));

    // After attribution-render: auto-inject the default viewer
    // CSS+JS pair into `rendered.includes.{header,after-body}` so
    // `--attribution=git` produces a visible default rather than
    // inert `data-attr-*` attributes. Internally gated on
    // `attribution_by_node.is_some()` AND
    // `attribution_viewer_enabled`, so the off-path is a no-op.
    // CLI-only: q2-preview omits this transform via
    // `Q2_PREVIEW_TRANSFORM_EXCLUDED` (hub-client ignores
    // `rendered.includes.*` and binds hover via React props).
    pipeline.push(Box::new(AttributionViewerTransform::new()));

    // Format-profile exclude-list application: the same `retain_excluding`
    // seam used by `build_q2_preview_transform_pipeline`, applied here so
    // `build_transform_pipeline` itself is total over `PipelineProfile` —
    // `HtmlRender`/`RevealjsRender` (native renders) keep every transform;
    // `HtmlPreview`/`RevealjsPreview` drop `Q2_PREVIEW_TRANSFORM_EXCLUDED`;
    // `Pandoc(_)` (docx, pptx, gfm, …) drops `PANDOC_TRANSFORM_EXCLUDED`,
    // since a Pandoc writer has no HTML chrome/decoration to consume those
    // transforms' output.
    match &pipeline_profile {
        crate::format::PipelineProfile::Pandoc(_) => {
            pipeline.retain_excluding(PANDOC_TRANSFORM_EXCLUDED);
        }
        crate::format::PipelineProfile::HtmlPreview
        | crate::format::PipelineProfile::RevealjsPreview => {
            pipeline.retain_excluding(Q2_PREVIEW_TRANSFORM_EXCLUDED);
        }
        crate::format::PipelineProfile::HtmlRender
        | crate::format::PipelineProfile::RevealjsRender => {}
    }

    pipeline
}

/// Names of transforms in [`build_transform_pipeline`] that the
/// q2-preview pipeline drops. The remaining excludes are:
///
/// 1. **Preserve CustomNodes for React** — `callout-resolve`,
///    `crossref-render`. Wrappers stay so React's type-specific
///    components (Plan 2) can render Callout / Theorem / Proof /
///    FloatRefTarget / Equation / CrossrefResolvedRef.
/// 2. **Synthesize-with-no-preimage** — `title-block`. Constructs
///    a container with no source backing; deferred to a future plan
///    with wrapper-CustomNode round-trip support. (`footnotes` and
///    `appendix-structure` are included — see Plan 2B notes below.)
///
/// Phase F.1 (bd-kw93.14) included `link-rewrite` so cross-page
/// body links emit `.html` hrefs the iframe link-handler can
/// intercept.
///
/// Phase F.2 (bd-kw93.15) included the chrome-render transforms
/// (`navbar-render`, `sidebar-render`, `page-nav-render`,
/// `toc-render`, `footer-render`, `website-favicon`). These
/// populate `meta.rendered.navigation.*` and
/// `meta.rendered.includes.header` with HTML strings that React's
/// `PreviewDocument` injects via `dangerouslySetInnerHTML` slots.
/// Tracked: bd-d8fo replaces the HTML-injection approach with
/// proper React components when chrome state-preservation becomes
/// a real complaint.
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
    // `hephaestus-render` swaps a `.hep` image for a rendered SVG
    // artifact. In q2-preview the raw `Image` must reach React, where
    // the `hephaestus-svg-wasm`-backed component renders it live
    // (bd-3qych45b, phase 2). Native-only at the cargo level too.
    "hephaestus-render",
    // `attribution-viewer` injects raw <style>/<script> tags into
    // `rendered.includes.{header,after-body}`, which the HTML
    // template wires into the final HTML. q2-preview's React leaves
    // ignore those slots entirely — the hub-client's own
    // `framework/attribution.tsx` carries the visual presentation
    // (badge classes, hover wiring) and would double-mount if this
    // transform ran here. CLI-only by design.
    "attribution-viewer",
    "title-block",
    // Other transforms previously listed here that are now INCLUDED:
    //   - "footnotes" (Plan 2B) — emits Pandoc primitives, rendered
    //     natively by q2-preview's leaves.
    //   - "appendix-structure" (Plan 2B) — pure Pandoc primitives.
    //   - "link-rewrite" (Phase F.1, bd-kw93.14) — body link
    //     rewriting; the SPA's iframe link-handler intercepts the
    //     resulting artifact-rooted `.html` hrefs.
    //   - "navbar-render", "sidebar-render", "page-nav-render",
    //     "toc-render", "footer-render", "website-favicon"
    //     (Phase F.2, bd-kw93.15) — populate
    //     `meta.rendered.navigation.*` and
    //     `meta.rendered.includes.header`; PreviewDocument injects
    //     each via `dangerouslySetInnerHTML`. bd-d8fo tracks
    //     replacing the HTML-injection approach with React
    //     components.
    "crossref-render",
    // `mermaid-render` replaces the diagram CodeBlock with a RawBlock
    // + after-body CDN script for `q2 render`. In preview the raw
    // CodeBlock must survive to the React layer, where the built-in
    // mermaid component (ts-packages/preview-renderer) renders the
    // diagram live for both q2-preview and q2-slides (bd-5m4ga0s1).
    "mermaid-render",
    // The tabset pair (bd-toc-tabset-titles-zq93gjvf) builds its nav
    // as *split* RawInlines (`<ul…>`, `<li…><a…>`, title inlines,
    // `</a></li>`, `</ul>`) — correct for the string-concatenating
    // HTML writer, but q2-preview's React `RawInline` component
    // renders each fragment via its own `dangerouslySetInnerHTML`
    // span, where unbalanced fragments get auto-closed and the tab
    // structure collapses. Excluding BOTH halves keeps the preview at
    // the passthrough (stacked headings) it showed before tabsets
    // existed. A React Tabset component consuming the CustomNode is
    // the proper preview story — tracked as a follow-up strand.
    "panel-tabset",
    "panel-tabset-resolve",
];

/// Names of transforms in [`build_transform_pipeline`] that a Pandoc-writer
/// output format (`PipelineProfile::Pandoc(_)` — docx, pptx, gfm, …) drops.
///
/// Pandoc's own writer produces the final document from the wire format's
/// custom nodes and Pandoc primitives; the excluded transforms here are the
/// ones whose job is HTML-specific *presentation* (chrome rendering,
/// HTML-only decoration) rather than format-agnostic semantic structure.
/// Unlike [`Q2_PREVIEW_TRANSFORM_EXCLUDED`] (a small, deliberate carve-out
/// from an otherwise-included-by-default HTML pipeline), this list drops the
/// entire Navigation phase plus every HTML-render-only presentation
/// transform, since a Pandoc writer has no navbar/sidebar/TOC/footer chrome
/// and no HTML markup to decorate.
///
/// `panel-tabset` (the sugar transform, as opposed to
/// `panel-tabset-resolve`) is deliberately **not** here — Pandoc's own
/// writers can render nested Divs, and P5's Route-R Tabset path relies on
/// the sugared structure surviving to a later stage. Adding it here would
/// pass every absence-shaped test while breaking that path; see the
/// "exclude-list superset trap" note on the exact-list validator below.
///
/// The unknown-name validator
/// (`pandoc_transform_excluded_names_exist_in_html_pipeline`) fails the test
/// suite if any name here is not an actual transform in the full HTML
/// pipeline (typo / rename guard) — same rationale as
/// [`Q2_PREVIEW_TRANSFORM_EXCLUDED`]'s validator.
const PANDOC_TRANSFORM_EXCLUDED: &[&str] = &[
    // B4: format-specific presentation that consumes rendered/semantic
    // shapes — HTML-only rendering of custom nodes and HTML decoration.
    "crossref-render",
    "mermaid-render",
    "code-block-render",
    "table-bootstrap-class",
    // B2: HTML scaffolding / website chrome producers with no Pandoc-writer
    // analog.
    "title-block",
    "sectionize",
    "title-banner",
    "website-title-prefix",
    "website-favicon",
    "website-bootstrap-icons",
    "website-canonical-url",
    // Navigation phase, twenty: chrome generate + render (TOC, navbar,
    // sidebar, page-nav, footer, listings) has no Pandoc-writer output slot.
    "navbar-generate",
    "navbar-render",
    "sidebar-generate",
    "sidebar-render",
    "secondary-nav-render",
    "breadcrumbs-render",
    "quarto-nav-js",
    "page-nav-generate",
    "page-nav-render",
    "toc-generate",
    "toc-render",
    "toc-location",
    "footer-generate",
    "footer-render",
    "listing-generate",
    "listing-render",
    "listing-feed-stage",
    "listing-feed-link",
    "categories-sidebar",
    "repo-actions-render",
    // B4-adjacent / previously unclassified: HTML-only presentation and
    // decoration with no Pandoc-writer analog.
    "attribution-viewer",
    "attribution-render",
    "draft-alert",
    "format-css",
    "responsive-image",
    // Swaps a `.hep` plot-document reference for a rendered SVG artifact;
    // no Pandoc-writer analog exists yet (bd-3qych45b covers HTML only).
    "hephaestus-render",
    // Design-doc-table corrections: resolve-halves whose job is producing
    // the writer-visible HTML shape.
    "callout-resolve",
    "panel-tabset-resolve",
    // Task 5: the HTML-chrome half of the footnotes split (Span#fnrefN +
    // Div#footnotes). `"footnotes"` (the semantic half, resolving refs/defs
    // into native `Inline::Note`) is deliberately NOT here — it must keep
    // running so pandoc's own writers get `Note`s to number and place.
    "footnotes-resolve",
];

/// The four-bucket taxonomy every transform in [`build_transform_pipeline`]
/// is classified into, per §6 of
/// `claude-notes/designs/pandoc-hybrid-architecture.md` (Decision 1, frozen).
///
/// **Litmus:** a transform is format-specific ([`Bucket::B2`] / [`Bucket::B4`])
/// if it bakes one format's *presentation* of semantics captured neutrally
/// elsewhere, re-expressed per-format downstream by the Pandoc
/// template/writer.
///
/// The pandoc-hybrid invariant follows directly: `B1 | B3` survive to the
/// Pandoc cut, `B2 | B4` must not. See
/// [`BUCKETS`] and `neutral_core_invariant_no_b2_b4_survives_the_pandoc_cut`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bucket {
    /// **B1 — format-neutral semantic core.** Pre-cut, shared by every
    /// format; this is the wire-format content itself. Survives the cut.
    B1,
    /// **B2 — format-family scaffolding.** Pre-cut, per-family (HTML,
    /// revealjs); must not touch semantics. Dropped at the cut.
    B2,
    /// **B3 — shared post-core service.** Crosses the cut and runs for
    /// Pandoc too (resource staging, link rewriting, appendix structure).
    B3,
    /// **B4 — format presentation.** The renderer tail: the HTML writer's
    /// own copy of the renderer trinity, plus website/navigation chrome.
    /// Dropped at the cut; Pandoc's writer (and the vendored Q1 Lua
    /// filters) re-express the same semantics for their own format.
    B4,
}

impl Bucket {
    /// Whether a transform in this bucket must still be running when the
    /// AST reaches the Pandoc cut. True for `B1`/`B3`, false for `B2`/`B4`.
    pub fn survives_pandoc_cut(self) -> bool {
        matches!(self, Bucket::B1 | Bucket::B3)
    }
}

/// The **authoritative** bucket classification of every transform registered
/// in [`build_transform_pipeline`], per the frozen §6 taxonomy (see
/// [`Bucket`]). The design doc's §6 table is the prose mirror of this const;
/// where the two disagree, this const is correct and the table is stale.
///
/// Two tests keep it honest, both built from the *real* pipeline rather than
/// from a hand-copied name list:
///
/// - `bucket_classification_is_total_over_the_html_pipeline` — every member of
///   `build_transform_pipeline(HtmlRender)` appears here exactly once, and
///   nothing here names a transform that is no longer registered.
/// - `neutral_core_invariant_no_b2_b4_survives_the_pandoc_cut` — every
///   transform surviving `build_transform_pipeline(Pandoc(_))` is `B1` or `B3`.
///
/// The totality test exists because hand-classification failed three
/// consecutive review rounds: `breadcrumbs-render`, `quarto-nav-js`,
/// `repo-actions-render`, `attribution-viewer` were missed in the first;
/// `secondary-nav-render`, `listing-feed-stage`, `listing-feed-link` in the
/// second; `config-markdown`, `reference-link-diagnostics`, `draft-alert`,
/// `format-css`, `responsive-image`, `llms-capture` in the third. **When you
/// add a transform to `build_transform_pipeline`, add its bucket here in the
/// same commit.**
///
/// Scope: the `HtmlRender` pipeline. The reveal-family scaffolding transforms
/// (`reveal-slides`, `reveal-columns`, …) are `B2`/`B4` by §6 but are not
/// registered for `HtmlRender`, so they are deliberately absent — the totality
/// test's "no stale entries" half would otherwise reject them.
pub const BUCKETS: &[(&str, Bucket)] = &[
    // --- B1: format-neutral semantic core ---------------------------------
    ("conditional-content", Bucket::B1),
    // Read-only diagnostic over the document as authored; no format in its
    // predicate, so a docx render warns about `[label][ref]` exactly as an
    // HTML one does.
    ("reference-link-diagnostics", Bucket::B1),
    // Sugar halves only. Each `*-resolve` sibling is B4 (see below).
    ("callout", Bucket::B1),
    ("panel-tabset", Bucket::B1),
    // Markdown-parses blessed config scalars into inlines so the shortcode
    // walk sees live nodes. A metadata *parse*, not presentation — the keys
    // it targets happen to be website ones, but nothing about the rewrite is
    // HTML-shaped.
    ("config-markdown", Bucket::B1),
    // Format-*parameterized*, not format-specific: `lua_format_for()` hands
    // Lua the canonical Pandoc format string.
    ("shortcode-resolve", Bucket::B1),
    ("metadata-normalize", Bucket::B1),
    ("date-normalize", Bucket::B1),
    ("authors-normalize", Bucket::B1),
    // Output is a `RenderContext` sideband; it does not cross the cut, but
    // the transform itself is format-neutral and must run.
    ("code-block-generate", Bucket::B1),
    // The semantic half of the Task 5 split: refs/defs → native
    // `Inline::Note`, which Pandoc's own writers number and place.
    ("footnotes", Bucket::B1),
    ("example-embed", Bucket::B1),
    ("theorem-sugar", Bucket::B1),
    ("proof-sugar", Bucket::B1),
    ("float-ref-target-sugar", Bucket::B1),
    ("equation-label", Bucket::B1),
    ("crossref-index", Bucket::B1),
    ("crossref-resolve", Bucket::B1),
    // Reclassified from B4 on 2026-09-17: format-parameterized like
    // `shortcode-resolve`. The iframe is emitted only for iframe-capable
    // profiles; a Pandoc profile gets the snippet + numbered caption.
    ("example-embed-render", Bucket::B1),
    // --- B2: format-family scaffolding ------------------------------------
    ("title-banner", Bucket::B2),
    ("website-title-prefix", Bucket::B2),
    ("website-favicon", Bucket::B2),
    ("website-bootstrap-icons", Bucket::B2),
    ("website-canonical-url", Bucket::B2),
    // Copies `css:` files into the output tree and rewrites the entries to
    // per-page hrefs — a stylesheet plumbing step with no Pandoc analog.
    ("format-css", Bucket::B2),
    // Writes `rendered.draft-alert-text` + a `quarto:status` header include
    // for the HTML template; sibling of `title-banner`.
    ("draft-alert", Bucket::B2),
    // Synthesizes an HTML title-block container from Meta; Pandoc's own
    // templates build the equivalent from the same Meta.
    ("title-block", Bucket::B2),
    // `--section-divs` is an HTML-writer concern.
    ("sectionize", Bucket::B2),
    // --- B3: shared post-core services ------------------------------------
    ("link-rewrite", Bucket::B3),
    ("appendix-structure", Bucket::B3),
    ("resource-collector", Bucket::B3),
    // Captures the markdown companion for llms.txt. A service, not
    // presentation of the current format; self-gates on `llms_view_active`
    // (website + `llms-txt: true` + an `html` target), so it is inert for a
    // Pandoc profile. It runs for Pandoc anyway because it is the only thing
    // that clears the marker classes `conditional-content` plants under the
    // identical predicate — the two must stay on the same side of the cut.
    ("llms-capture", Bucket::B3),
    // --- B4: format presentation (the renderer tail) -----------------------
    // Resolve halves: each destroys a CustomNode into Bootstrap-specific DOM.
    ("callout-resolve", Bucket::B4),
    ("panel-tabset-resolve", Bucket::B4),
    // HTML chrome half of the Task 5 footnotes split: `Span#fnrefN` refs and
    // a trailing `Div#footnotes` with backlinks.
    ("footnotes-resolve", Bucket::B4),
    // Navigation phase, in registration order: website chrome with no
    // Pandoc-writer output slot. T2.2 derives this set from `phase()`, so a
    // 21st Navigation transform cannot land unexcluded — but it can still
    // land unclassified here, which is what the totality test catches.
    ("toc-generate", Bucket::B4),
    ("navbar-generate", Bucket::B4),
    ("sidebar-generate", Bucket::B4),
    ("page-nav-generate", Bucket::B4),
    ("footer-generate", Bucket::B4),
    ("listing-generate", Bucket::B4),
    ("listing-render", Bucket::B4),
    ("categories-sidebar", Bucket::B4),
    ("listing-feed-stage", Bucket::B4),
    ("listing-feed-link", Bucket::B4),
    ("toc-render", Bucket::B4),
    ("toc-location", Bucket::B4),
    ("repo-actions-render", Bucket::B4),
    ("navbar-render", Bucket::B4),
    ("sidebar-render", Bucket::B4),
    ("breadcrumbs-render", Bucket::B4),
    ("secondary-nav-render", Bucket::B4),
    ("quarto-nav-js", Bucket::B4),
    ("page-nav-render", Bucket::B4),
    ("footer-render", Bucket::B4),
    // The HTML writer's copy of the renderer trinity. `crossref-render` is
    // the load-bearing member: it destroys every numbered `CustomNode` into
    // raw HTML `Figure`/`Div` structure, so if it ran before the cut P5's
    // shim would have nothing left to route.
    ("crossref-render", Bucket::B4),
    ("mermaid-render", Bucket::B4),
    ("code-block-render", Bucket::B4),
    ("table-bootstrap-class", Bucket::B4),
    // Swaps a `.hep` plot-document reference for a rendered SVG artifact —
    // an HTML-only presentation decision (sibling of `responsive-image`,
    // which it runs immediately before). No Pandoc-writer analog exists yet.
    ("hephaestus-render", Bucket::B4),
    ("responsive-image", Bucket::B4),
    // Both bake per-node attribution onto `ctx.format_options` fields that
    // only the HTML/JSON writers read.
    ("attribution-render", Bucket::B4),
    ("attribution-viewer", Bucket::B4),
];

/// Build the q2-preview transform pipeline (Plan 1).
///
/// Constructed as [`build_transform_pipeline`] with the names in
/// [`Q2_PREVIEW_TRANSFORM_EXCLUDED`] removed. Order is preserved.
/// Constructor args (notably `shortcode_paths`, `extensions`,
/// `runtime`, `target_format`, `pipeline_profile`) are forwarded
/// verbatim so shortcode-and-Lua semantics match the HTML pipeline.
///
/// `AstTransformsStage::run()` dispatches between this and
/// `build_transform_pipeline` based on the [`crate::format::PipelineProfile`]
/// derived for the render.
pub fn build_q2_preview_transform_pipeline(
    shortcode_paths: Vec<std::path::PathBuf>,
    extensions: Vec<crate::extension::types::Extension>,
    runtime: std::sync::Arc<dyn quarto_system_runtime::SystemRuntime>,
    target_format: String,
    pipeline_profile: crate::format::PipelineProfile,
    variables: Option<quarto_pandoc_types::ConfigValue>,
    project_env: hashlink::LinkedHashMap<String, String>,
    quarto_profile: Option<String>,
) -> TransformPipeline {
    let mut pipeline = build_transform_pipeline(
        shortcode_paths,
        extensions,
        runtime,
        target_format,
        pipeline_profile,
        variables,
        project_env,
        quarto_profile,
    );
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

            ..Default::default()
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

    /// bd-uy4uygha: `render_qmd_to_html` must splice server-recorded captures
    /// into the HTML (hub-client's default `format: html` preview), not just the
    /// q2-preview AST path. Mirrors `captureSplice.wasm.test.ts`: one engine cell
    /// + a hand-built capture whose result markdown is a `.cell` wrapper carrying
    /// a marker that appears ONLY in the capture, never in the source.
    #[tokio::test]
    async fn render_qmd_to_html_splices_captures() {
        use quarto_trace::EngineCapture;

        // The doc renders as html (no `format:` key). Use a fictitious engine
        // name no platform registers, so EngineExecutionStage takes the
        // markdown-fallback branch (no subprocess) and the splice — which runs
        // before it — is the only thing that can produce output.
        let qmd = "---\ntitle: T\nengine: markerlang\n---\n\n```{markerlang}\n1 + 1\n```\n";
        let capture = EngineCapture {
            engine_name: "markerlang".into(),
            // Same `{markerlang}` cell as the doc, so its content-hash matches.
            input_qmd: "```{markerlang}\n1 + 1\n```\n".into(),
            // Post-engine markdown: a `.cell` wrapper whose stdout is the marker.
            result: serde_json::json!({
                "markdown": "::: {.cell}\n```{.markerlang .cell-code}\n1 + 1\n```\n\n::: {.cell-output .cell-output-stdout}\n```\nSPLICEMARKER_ZX9\n```\n:::\n:::\n"
            }),
            files: Vec::new(),
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let runtime = make_test_runtime();

        // With the capture, the marker (which is only in the capture) appears.
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        let config = HtmlRenderConfig::default().with_captures(vec![capture]);
        let out = render_qmd_to_html(
            qmd.as_bytes(),
            "test.qmd",
            &mut ctx,
            &config,
            runtime.clone(),
        )
        .await
        .unwrap();
        assert!(
            out.html.contains("SPLICEMARKER_ZX9"),
            "spliced engine output must appear in the HTML; got:\n{}",
            out.html
        );

        // No capture => source-only render (byte-compatible default path).
        let mut ctx2 = RenderContext::new(&project, &doc, &format, &binaries);
        let out2 = render_qmd_to_html(
            qmd.as_bytes(),
            "test.qmd",
            &mut ctx2,
            &HtmlRenderConfig::default(),
            runtime,
        )
        .await
        .unwrap();
        assert!(
            !out2.html.contains("SPLICEMARKER_ZX9"),
            "no capture => source-only render"
        );
    }

    /// bd-qbhp2cvv: a capture carrying embedded supporting-file bytes
    /// must have them materialized next to the document when the
    /// splice runs — that is how engine-generated figures become
    /// readable by the preview's VFS-based image resolvers (and, in
    /// this native test, appear on disk under the doc's directory).
    #[tokio::test]
    async fn capture_splice_materializes_embedded_files_next_to_doc() {
        use base64::Engine as _;
        use quarto_trace::{CaptureFile, EngineCapture};

        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().canonicalize().unwrap();
        let doc_path = dir.join("test.qmd");

        let qmd = "---\ntitle: T\nengine: markerlang\n---\n\n```{markerlang}\nplot(1)\n```\n";
        let capture = EngineCapture {
            engine_name: "markerlang".into(),
            input_qmd: "```{markerlang}\nplot(1)\n```\n".into(),
            result: serde_json::json!({
                "markdown": "::: {.cell}\n![](test_files/figure-html/fig.png)\n:::\n"
            }),
            files: vec![CaptureFile {
                path: "test_files/figure-html/fig.png".into(),
                contents_base64: base64::engine::general_purpose::STANDARD.encode(b"FAKE-PNG"),
            }],
        };

        let project = ProjectContext {
            dir: dir.clone(),
            config: crate::project::ProjectConfig::default(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path(&doc_path)],
            output_dir: dir.clone(),
            ..Default::default()
        };
        let doc = DocumentInfo::from_path(&doc_path);
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        let config = HtmlRenderConfig::default().with_captures(vec![capture]);
        let out = render_qmd_to_html(
            qmd.as_bytes(),
            &doc_path.to_string_lossy(),
            &mut ctx,
            &config,
            make_test_runtime(),
        )
        .await
        .unwrap();

        // The spliced output references the figure...
        assert!(
            out.html.contains("test_files/figure-html/fig.png"),
            "spliced image ref must appear in the HTML; got:\n{}",
            out.html
        );
        // ...and the splice materialized its bytes next to the doc.
        let materialized = dir.join("test_files/figure-html/fig.png");
        assert_eq!(
            std::fs::read(&materialized).expect("figure file materialized next to the doc"),
            b"FAKE-PNG"
        );
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
        // Merged pipeline: SourceConversionStage at [0] (branch) plus two
        // stages main added — LanguageResolveStage after metadata-merge
        // (bd-llhlzd7p) and TabsetsJsStage in the JS block
        // (bd-toc-tabset-titles-zq93gjvf) — plus EquationNumberStage and
        // MathMlStage after the post filters (bd-vlhi2zkj, bd-3evfzwal): 27.
        assert_eq!(stages.len(), 27);
        // Pre-parse file-claim/convert (Task 10).
        assert_eq!(stages[0].name(), "source-conversion");
        assert_eq!(stages[1].name(), "parse-document");
        assert_eq!(stages[2].name(), "metadata-merge");
        // Localized-term resolution (bd-llhlzd7p) directly follows the
        // metadata merge so `quarto.language` is present for every
        // downstream consumer, including the profile checkpoint.
        assert_eq!(stages[3].name(), "language-resolve");
        // Include expansion runs before the profile checkpoint (bd-xfwx)
        // so profiles reflect content spliced in via `{{< include ... >}}`.
        assert_eq!(stages[4].name(), "include-expansion");
        // include-resolve (bd-8kp3) sits between include-expansion and
        // the profile checkpoint so file-slot include dependencies are
        // recorded into `profile.includes` for cache invalidation.
        assert_eq!(stages[5].name(), "include-resolve");
        // Listings auto-fill (bd-izqh, L1) sits between include-resolve
        // and the profile checkpoint so `meta.listing-item.*` enrichment
        // is visible to `DocumentProfile.listing_item`.
        assert_eq!(stages[6].name(), "listing-item-info");
        // Profile checkpoint (Phase 0 website epic, bd-f3jc).
        assert_eq!(stages[7].name(), "document-profile");
        // Cross-doc body-link resolution (Phase 8 sub-phase 8.0d).
        assert_eq!(stages[8].name(), "link-resolution");
        assert_eq!(stages[9].name(), "unwrap-profile");
        assert_eq!(stages[10].name(), "pre-engine-sugaring");
        assert_eq!(stages[11].name(), "engine-execution");
        assert_eq!(stages[12].name(), "compile-theme-css");
        // Bootstrap JS (bd-4eyf) sits immediately after CompileThemeCssStage
        // so the same theme predicate gates JS and CSS together.
        assert_eq!(stages[13].name(), "bootstrap-js");
        // ClipboardJsStage (Phase 2 of bd-1tl09) sits next to
        // bootstrap-js because both ship a Project-scoped JS payload
        // gated on minimal-HTML. clipboard-js additionally gates on
        // `code-copy != false`.
        assert_eq!(stages[14].name(), "clipboard-js");
        // TabsetsJsStage (bd-toc-tabset-titles-zq93gjvf) ships the
        // grouped-tabset sync module whenever Bootstrap does — same
        // gate as bootstrap-js, so it sits in the same JS block.
        assert_eq!(stages[15].name(), "tabsets-js");
        // Attribution-generate runs before user filters so the
        // `quarto.attribution.*` Lua host binding sees a populated
        // sidecar (bd-0fd0). No-op when no provider is installed.
        assert_eq!(stages[16].name(), "attribution-generate");
        assert_eq!(stages[17].name(), "user-filters-pre");
        assert_eq!(stages[18].name(), "ast-transforms");
        assert_eq!(stages[19].name(), "user-filters-post");
        // bd-o8pr Phase 3: finalize per-doc resource report.
        assert_eq!(stages[20].name(), "resource-report");
        // Equation-number encoding (bd-vlhi2zkj) must follow
        // user-filters-post: a Lua post filter may rewrite or delete the
        // reserved `quarto-eq-number` attribute the stage consumes.
        assert_eq!(stages[21].name(), "equation-number");
        // Native MathML (bd-3evfzwal) follows equation-number (the TeX it
        // converts carries no `\tag`) and precedes math-js (which loads
        // MathJax only for what this stage left as TeX).
        assert_eq!(stages[22].name(), "math-ml");
        assert_eq!(stages[23].name(), "code-highlight");
        // Math-mode (bd-w5ov) walks the post-transform AST and
        // populates meta.math when math is present. Sits just before
        // render-html-body so any late-introduced math (sugar, user
        // filters, the `\tag{N}` equation-number encoding) is visible.
        assert_eq!(stages[24].name(), "math-js");
        assert_eq!(stages[25].name(), "render-html-body");
        assert_eq!(stages[26].name(), "apply-template");
    }

    #[test]
    fn test_build_html_pipeline() {
        let pipeline = build_html_pipeline();
        // Merged pipeline carries both SourceConversionStage (Task 10, branch)
        // LanguageResolveStage and TabsetsJsStage (main), plus
        // EquationNumberStage and MathMlStage → 27 stages.
        assert_eq!(pipeline.len(), 27);
    }

    #[test]
    fn test_build_analysis_pipeline() {
        use crate::stage::PipelineDataKind;

        let pipeline = build_analysis_pipeline();
        // Parse + MetadataMerge + LanguageResolve + IncludeExpansion +
        // PreEngineSugaring + AstTransforms(analysis subset)
        assert_eq!(pipeline.len(), 6);
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
            value: ConfigValueKind::scalar(Yaml::String(theme.to_string())),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };
        let entry = ConfigMapEntry {
            key: "theme".to_string(),
            key_source: SourceInfo::for_test(),
            value: theme_value,
        };
        let metadata = ConfigValue {
            value: ConfigValueKind::Map(vec![entry]),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };
        ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::with_metadata(metadata),
            is_single_file: false,
            files: vec![DocumentInfo::from_path("/project/test.qmd")],
            output_dir: PathBuf::from("/project"),

            ..Default::default()
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
        let content = b"---\nengine: replay-only-engine\n---\n\n# Original Heading\n\nOriginal body.\n\n```{replay-only-engine}\ncode\n```\n";

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
            fn claims_language(
                &self,
                language: &str,
                _first_class: Option<&str>,
            ) -> crate::engine::LanguageClaim {
                if language == "replay-only-engine" {
                    crate::engine::LanguageClaim::Primary(1)
                } else {
                    crate::engine::LanguageClaim::None
                }
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
                engine_registry: Some(Arc::new(probe_registry)),
                ..Default::default()
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
            files: Vec::new(),
        };

        let replay_registry = EngineRegistry::with_replay(capture);

        // Real run, this time through the replay-substituted registry.
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        let config = HtmlRenderConfig {
            resolver: None,
            engine_registry: Some(Arc::new(replay_registry)),
            ..Default::default()
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

    /// bd-sauc9iiq: when a capture is supplied for an engine the WASM
    /// preview registry doesn't implement, `build_q2_preview_pipeline_stages`
    /// must thread that engine's name into `EngineExecutionStage` so the
    /// "(no execution)" fallback warning is suppressed — the user *did* see
    /// real (server-spliced) output. Uses a fictitious engine name no
    /// platform registers, so the unregistered-fallback branch fires
    /// deterministically on every OS (unlike `knitr`, whose availability
    /// depends on whether R is installed).
    #[test]
    fn q2_preview_capture_suppresses_engine_unavailable_warning() {
        use quarto_trace::EngineCapture;

        let content = b"---\ntitle: Test\nengine: replay-only-engine\n---\n\n# Heading\n\nBody.\n";

        let capture = EngineCapture {
            engine_name: "replay-only-engine".into(),
            input_qmd: String::new(),
            result: serde_json::json!({ "markdown": "" }),
            files: Vec::new(),
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::from_format_string("q2-preview").unwrap();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        let runtime = make_test_runtime();

        let output = pollster::block_on(render_qmd_to_preview_ast(
            content,
            "test.qmd",
            &mut ctx,
            runtime,
            None,
            vec![capture],
        ))
        .expect("q2-preview render");

        assert!(
            !output
                .diagnostics
                .iter()
                .any(|d| d.title.contains("not available")),
            "a spliced capture must suppress the engine-unavailable warning; got: {:?}",
            output
                .diagnostics
                .iter()
                .map(|d| d.title.clone())
                .collect::<Vec<_>>()
        );
    }

    /// Companion to the test above: with *no* capture for the engine, the
    /// render must **fail loudly** (P2-12). Guarding against the suppression
    /// over-firing — a registered-but-unavailable engine with no capture means
    /// the document was never executed, and a silent markdown fallback would
    /// silently produce wrong output. The render must surface an error the user
    /// can act on.
    #[test]
    fn q2_preview_without_capture_errors_unavailable_engine() {
        use crate::engine::EngineRegistry;

        // A fake engine that is always unavailable, claims its own language as
        // Primary. Provides a deterministic "engine unavailable" warning without
        // depending on whether R/Python runtimes are installed (unlike
        // knitr/jupyter). Under Task 9, an engine only warns when it appears in
        // the resolution sequence — so the document must have a cell for it.
        struct AlwaysUnavailableEngine;
        impl crate::engine::ExecutionEngine for AlwaysUnavailableEngine {
            fn name(&self) -> &str {
                "always-unavailable"
            }
            fn execute(
                &self,
                _input: &str,
                _ctx: &crate::engine::ExecutionContext,
            ) -> std::result::Result<crate::engine::ExecuteResult, crate::engine::ExecutionError>
            {
                unreachable!("always-unavailable is never invoked")
            }
            fn is_available(&self) -> bool {
                false
            }
            fn claims_language(
                &self,
                language: &str,
                _first_class: Option<&str>,
            ) -> crate::engine::LanguageClaim {
                if language == "always-unavailable" {
                    crate::engine::LanguageClaim::Primary(1)
                } else {
                    crate::engine::LanguageClaim::None
                }
            }
        }

        let content = b"---\ntitle: Test\nengine: always-unavailable\n---\n\n# Heading\n\n```{always-unavailable}\nx\n```\n";

        let mut registry = EngineRegistry::new();
        registry.register(Arc::new(AlwaysUnavailableEngine));

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::from_format_string("q2-preview").unwrap();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        let runtime = make_test_runtime();

        let result = pollster::block_on(render_qmd_to_preview_ast(
            content,
            "test.qmd",
            &mut ctx,
            runtime,
            Some(Arc::new(registry)),
            Vec::new(),
        ));

        // P2-12: a registered engine with no available runtime and no spliced
        // capture must fail loudly so the user knows their document was not
        // executed. A silent markdown fallback would silently produce wrong output.
        assert!(
            result.is_err(),
            "P2-12: without a spliced capture, a registered-but-unavailable engine \
             must loud-error; got Ok (old silent-fallback behaviour)"
        );
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("always-unavailable"),
            "error message must name the engine; got: {err_msg}"
        );
    }

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
        let html = build_transform_pipeline(
            vec![],
            vec![],
            runtime,
            "html".to_string(),
            crate::format::PipelineProfile::HtmlRender,
            None,
            Default::default(),
            None,
        );
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
            content,
            "test.qmd",
            &mut ctx,
            runtime,
            None,
            Vec::new(),
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

    /// bd-mermaid-cell-options-9wo3crl0: mermaid `%%|` cell options are
    /// processed by `PreEngineSugaringStage`, which is a *stage* — while
    /// `Q2_PREVIEW_TRANSFORM_EXCLUDED` only filters *transforms*. So the
    /// preview AST must carry the same structure `q2 render` emits: a
    /// Figure wrapping the diagram, the options gone from the diagram
    /// source, and `fig-alt` folded into mermaid's `accDescr:`.
    ///
    /// The `mermaid-render` transform stays excluded here (the raw
    /// CodeBlock has to reach `MermaidCodeBlock.tsx`), so the diagram
    /// arrives as a CodeBlock rather than a `<pre>` RawBlock — that is
    /// the intended difference, and this test pins it so a future change
    /// to either list cannot silently diverge the two surfaces.
    #[test]
    fn render_qmd_to_preview_ast_processes_mermaid_cell_options() {
        let content = "---\ntitle: Test\nformat: q2-preview\n---\n\n\
                        ```mermaid\n\
                        %%| fig-cap: A tiny flowchart.\n\
                        %%| fig-alt: Two nodes connected by an arrow.\n\
                        flowchart LR\n  A --> B\n\
                        ```\n"
            .as_bytes();

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::from_format_string("q2-preview").unwrap();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let runtime = make_test_runtime();
        let output = pollster::block_on(render_qmd_to_preview_ast(
            content,
            "test.qmd",
            &mut ctx,
            runtime,
            None,
            Vec::new(),
        ))
        .expect("q2-preview render");

        let json = &output.ast_json;
        assert!(
            json.contains("\"Figure\""),
            "preview AST must carry the Figure wrapper; got:\n{json}"
        );
        // The caption is markdown, so it arrives as separate Str/Space
        // inlines rather than one contiguous string.
        for word in ["\"tiny\"", "\"flowchart.\""] {
            assert!(
                json.contains(word),
                "preview AST must carry the caption word {word}; got:\n{json}"
            );
        }
        assert!(
            json.contains("accDescr: Two nodes connected by an arrow."),
            "preview AST must carry the injected accDescr; got:\n{json}"
        );
        assert!(
            !json.contains("%%|"),
            "consumed option lines must not reach the preview; got:\n{json}"
        );
        assert!(
            json.contains("\"CodeBlock\""),
            "the raw CodeBlock must survive for MermaidCodeBlock.tsx; got:\n{json}"
        );
    }

    /// Phase 1P: the `q2-slides` preview pseudo-format must run the reveal
    /// slide construction (`RevealSlidesTransform`) and return the
    /// section-structured AST — the shared contract the SPA renders with a
    /// reveal shell. Confirms the preview AST path produces the same slide
    /// structure as the native `revealjs` render.
    #[test]
    fn render_qmd_to_preview_ast_builds_reveal_slides_for_q2_slides() {
        let content = b"---\ntitle: Slides Test\nformat: revealjs\n---\n\n\
                        ## First\n\n- a\n- b\n\n## Second\n\nBody.\n";

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::from_format_string("q2-slides").unwrap();
        assert_eq!(format.pipeline_kind, Some("preview"));
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let runtime = make_test_runtime();
        let output = pollster::block_on(render_qmd_to_preview_ast(
            content,
            "test.qmd",
            &mut ctx,
            runtime,
            None,
            Vec::new(),
        ))
        .expect("q2-slides render");

        let snippet = || &output.ast_json[..output.ast_json.len().min(1200)];
        // The synthesized title slide and section-class Divs prove
        // RevealSlidesTransform ran (vs. the generic sectionize/title-block).
        assert!(
            output.ast_json.contains("title-slide"),
            "expected a title-slide section in q2-slides AST; got:\n{}",
            snippet()
        );
        assert!(
            output.ast_json.contains("\"section\""),
            "expected section-class Divs (reveal slides); got:\n{}",
            snippet()
        );
    }

    /// bd-y259zb57 (L2.1): the q2-preview pipeline — the exact path
    /// `q2 preview` drives in WASM — must compile the deck's reveal theme and
    /// expose it through the standard `css:theme:<fp>` artifact, so the SPA's
    /// existing `theme_fingerprint` + styles.css transport delivers it. Before
    /// the fix the `q2-slides` pseudo-format (identifier `Html`) took the
    /// Bootstrap branch, so the preview shipped Bootstrap and reveal decks fell
    /// back to stock `white.css` (centered, uppercase) instead of the Quarto
    /// reveal theme (left-aligned, non-uppercase).
    #[test]
    fn q2_preview_pipeline_compiles_reveal_theme_for_slides() {
        let content = b"---\ntitle: Slides Test\nformat: revealjs\n---\n\n\
                        ## First\n\n- a\n- b\n";

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::from_format_string("q2-slides").unwrap();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let runtime = make_test_runtime();
        pollster::block_on(render_qmd_to_preview_ast(
            content,
            "test.qmd",
            &mut ctx,
            runtime,
            None,
            Vec::new(),
        ))
        .expect("q2-slides render");

        // The compiled reveal theme is delivered via `css:theme:<fp>` (the same
        // artifact key + styles.css path the SPA already reads), NOT the
        // linkable `css:revealjs:*` set (that's the render path's site_libs
        // delivery — the SPA bundles reset/reveal/quarto-reveal itself).
        let theme_entries = ctx.artifacts.get_by_prefix("css:theme:");
        assert_eq!(
            theme_entries.len(),
            1,
            "q2-slides preview should produce exactly one css:theme artifact"
        );
        let css = theme_entries[0].1.as_str().expect("theme CSS is UTF-8");
        assert!(
            css.contains(".reveal"),
            "preview must compile the reveal theme (scoped under .reveal), not Bootstrap"
        );
        assert!(
            !css.contains(".navbar"),
            "preview reveal theme must not be Bootstrap (.navbar present)"
        );
        assert!(
            ctx.artifacts.get_by_prefix("css:revealjs:").is_empty(),
            "preview should not register linkable css:revealjs:* assets"
        );
    }

    /// bd-y259zb57 (L2.1): a *named* reveal theme nested under
    /// `format.revealjs.theme` must reach the preview's compiled theme. This
    /// guards the metadata-flattening half of the fix: the `q2-slides`
    /// pseudo-format has identifier `Html`, so `MetadataMergeStage` would
    /// flatten `format.html.*` (burying `theme:`) unless it maps the reveal
    /// preview back to the `revealjs` base format. Before that fix the preview
    /// silently compiled the *default* theme for every named theme/brand.
    #[test]
    fn q2_preview_pipeline_compiles_named_reveal_theme_for_slides() {
        // `theme: dark` lives under `format.revealjs.theme`, exactly as a real
        // deck authors it.
        let content = b"---\ntitle: Dark\nformat:\n  revealjs:\n    theme: dark\n---\n\n\
                        ## First\n\nBody.\n";

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::from_format_string("q2-slides").unwrap();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let runtime = make_test_runtime();
        pollster::block_on(render_qmd_to_preview_ast(
            content,
            "test.qmd",
            &mut ctx,
            runtime,
            None,
            Vec::new(),
        ))
        .expect("q2-slides render");

        let theme_entries = ctx.artifacts.get_by_prefix("css:theme:");
        assert_eq!(theme_entries.len(), 1, "expected one css:theme artifact");
        let css = theme_entries[0].1.as_str().expect("theme CSS is UTF-8");
        // The reveal `dark` theme sets a dark background; the default theme is
        // white (`#fff`). Asserting the dark background proves the named theme
        // was resolved (not silently defaulted).
        assert!(
            css.contains("#191919"),
            "named theme `dark` must reach the compiled preview theme \
             (expected dark background #191919); got default theme?"
        );
    }

    /// bd-y259zb57 (L2.1): a `_brand.yml` reveal deck previewed as `q2-slides`
    /// must fold the brand into the compiled theme — same as `q2 render`. This
    /// exercises the reveal branch's brand resolution (`resolve_brand_layers`
    /// against `ctx.project.dir`) through the preview pipeline, plus the
    /// `format.revealjs.brand` metadata flattening. (E2E `q2 preview` of a brand
    /// deck is additionally gated on the preview server syncing `_brand.yml`
    /// into the VFS — a separate, pre-existing infra gap that affects HTML brand
    /// previews identically; tracked in its own strand.)
    #[test]
    fn q2_preview_pipeline_compiles_brand_reveal_theme_for_slides() {
        // Real tempdir so the NativeRuntime can read `_brand.yml` from disk,
        // exactly as the reveal branch resolves it against the project dir.
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::write(
            root.join("_brand.yml"),
            "color:\n  palette:\n    purple: \"#6f42c1\"\n  primary: purple\n  \
             background: \"#fdf6ff\"\n  foreground: \"#2a1a3a\"\n",
        )
        .unwrap();

        let content = b"---\ntitle: Brand\nformat:\n  revealjs:\n    brand: _brand.yml\n---\n\n\
                        ## First\n\nBody.\n";

        let project = ProjectContext {
            dir: root.clone(),
            config: crate::project::ProjectConfig::default(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path(root.join("deck.qmd"))],
            output_dir: root.clone(),

            ..Default::default()
        };
        let doc = DocumentInfo::from_path(root.join("deck.qmd"));
        let format = Format::from_format_string("q2-slides").unwrap();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let runtime = make_test_runtime();
        pollster::block_on(render_qmd_to_preview_ast(
            content,
            root.join("deck.qmd").to_str().unwrap(),
            &mut ctx,
            runtime,
            None,
            Vec::new(),
        ))
        .expect("q2-slides brand render");

        let theme_entries = ctx.artifacts.get_by_prefix("css:theme:");
        assert_eq!(theme_entries.len(), 1, "expected one css:theme artifact");
        let css = theme_entries[0].1.as_str().expect("theme CSS is UTF-8");
        // Brand background colour folded into the reveal theme proves the brand
        // reached the compiled output (default reveal background is `#fff`).
        assert!(
            css.contains("#fdf6ff"),
            "brand background must reach the compiled preview theme; got default?"
        );
    }

    /// Phase 1 (target-incremental-writes): `render_qmd_to_preview_ast` must
    /// return *both* the transformed AST (`ast_json`) and the untransformed
    /// AST (`untransformed_ast_json`).  An unchanged paragraph must have
    /// byte-identical `source_info` values in both trees.
    #[test]
    fn render_qmd_to_preview_ast_returns_dual_ast() {
        let content = b"---\nformat: q2-preview\n---\n\nHello world.\n";

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::from_format_string("q2-preview").unwrap();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let runtime = make_test_runtime();
        let output = pollster::block_on(render_qmd_to_preview_ast(
            content,
            "/project/test.qmd",
            &mut ctx,
            runtime,
            None,
            Vec::new(),
        ))
        .expect("q2-preview render");

        // Phase 1: untransformed_ast_json must be present.
        let untransformed_json = output
            .untransformed_ast_json
            .expect("render_qmd_to_preview_ast must return untransformed_ast_json");

        // Deserialize both ASTs.
        let transformed_ast = {
            let mut cursor = std::io::Cursor::new(output.ast_json.as_bytes());
            pampa::readers::json::read(&mut cursor)
                .expect("parse transformed AST JSON")
                .0
        };
        let untransformed_ast = {
            let mut cursor = std::io::Cursor::new(untransformed_json.as_bytes());
            pampa::readers::json::read(&mut cursor)
                .expect("parse untransformed AST JSON")
                .0
        };

        // Both trees must contain a paragraph.
        let t_para = transformed_ast
            .blocks
            .iter()
            .find(|b| matches!(b, pampa::pandoc::Block::Paragraph(_)))
            .expect("transformed AST must contain a paragraph block");
        let u_para = untransformed_ast
            .blocks
            .iter()
            .find(|b| matches!(b, pampa::pandoc::Block::Paragraph(_)))
            .expect("untransformed AST must contain a paragraph block");

        // An unchanged paragraph preserves its source_info through transforms.
        assert_eq!(
            u_para.source_info(),
            t_para.source_info(),
            "unchanged paragraph must have byte-identical source_info in both ASTs"
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
            content,
            "test.qmd",
            &mut ctx,
            runtime,
            None,
            Vec::new(),
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

    /// **T1 (Plan 3, Phase 1) — invariant pin, not a regression test.**
    ///
    /// Pins the invariant that makes the incremental writer's *byte-copy*
    /// arms safe: **every `SourceInfo` referenced by a body node's `s` key in
    /// the captured baseline pool is an `Original` rooted at the document's
    /// own `FileId`.**
    ///
    /// # Why this matters
    ///
    /// `SourceInfo::preimage_in` returns a hull for a contiguous `Concat`,
    /// and that hull is an **offset claim, not a byte-identity claim**
    /// (`quarto-source-map-0.1.3/src/source_info.rs:410-425`). Three sites in
    /// the incremental writer nevertheless slice `original_qmd` at a
    /// `preimage_in` range and emit those bytes as the node's own text:
    ///
    /// - `pampa/src/writers/incremental.rs:205` — `BlockAlignment::KeepBefore`
    ///   → `CoarsenedEntry::Verbatim`;
    /// - `pampa/src/writers/incremental.rs:816` — `InlineAlignment::KeepBefore`
    ///   in `assemble_inline_content`;
    /// - `pampa/src/writers/incremental.rs:868` — `assemble_recursed_container`
    ///   with no nested plan or no children.
    ///
    /// Their `.get()` guards check **bounds, not identity**. What actually
    /// keeps them correct is the shape of the baseline they consume: a body
    /// whose provenance is all `Original` in its own file has no `Concat`,
    /// hence no fold, hence no gap between "the bytes at this range" and
    /// "this node's content". The safety is incidental to the writer and
    /// structural to the capture, which is why the guard lives here rather
    /// than at the copy sites.
    ///
    /// # What this pin does and does not cover
    ///
    /// It pins **one entry point**: the artifact
    /// `capture_untransformed_ast_json` produces, as reached through
    /// `render_qmd_to_preview_ast`. `apply_node_edit`
    /// (`pampa/src/apply_node_edit.rs:120`) deserializes that same artifact,
    /// so it **inherits** this guard.
    ///
    /// `incremental_write_qmd`
    /// (`crates/wasm-quarto-hub-client/src/lib.rs:2952`) does **not**. It
    /// reaches its own `qmd_to_pandoc(original_qmd.as_bytes())` on raw bytes.
    /// The invariant there is analogous — a fresh, parent-less parse of the
    /// very text being sliced — but **no test asserts it**, and all three copy
    /// sites sit on that path too. Do not read this test as covering it.
    ///
    /// # Why the body, and not the whole pool
    ///
    /// The plan specced this as "every `astContext.p` entry". Measured, that
    /// is false and would have made the test unpassable: the pool also holds
    /// **front-matter metadata** provenance, which is legitimately
    /// `Substring` (wire-code `1`) over the front-matter `Original` — for
    /// this fixture, entries `13..=26` chaining to entry `12` (`[0,39]`, the
    /// `---` block), against `0..=11` for the body (`[40,73]`).
    ///
    /// Restricting to the body is not a weakening; it is the set that
    /// load-bears. `coarsen` and `assemble` walk `original_ast.blocks` and
    /// their inlines and copy bytes from those spans alone — metadata is
    /// never byte-copied. A folded scalar in front matter would put a genuine
    /// `Concat` in the pool and harm nothing.
    ///
    /// # What "body-reachable" means here, exactly
    ///
    /// The `s` key, and only that. **Attr and link/image target provenance
    /// are also body-reachable pool refs and are deliberately out of scope**:
    /// `write_attr_source` (`pampa/src/writers/json.rs:694-720`) and
    /// `write_target_source` emit them through `to_json_ref` (`json.rs:430-433`)
    /// as **bare integers** with no `s` key, so `collect_pool_ids` does not
    /// see them. Do not "fix" that by collecting bare numbers under `blocks`:
    /// header levels, alignment indices and column widths are bare numbers
    /// too, and blind collection would assert against them. Both failure modes
    /// below still redden this test through the `s` ids, so the pin holds —
    /// its reach is simply narrower than "everything the body touches".
    ///
    /// # Why one assertion covers both failure modes
    ///
    /// - Thread a parent into the baseline parse and every body node becomes
    ///   `Substring` (wire-code `1`) — `pampa/src/pandoc/location.rs:214`
    ///   consumes `parent_source_info` at parse time.
    /// - Move the capture after the transform stages and transform-injected
    ///   nodes appear in the body carrying `Generated` (wire-code `4`) —
    ///   `footnotes.rs`, `appendix.rs`, `title_block.rs`,
    ///   `shortcode_resolve.rs:1175` — or a foreign `file_id`.
    ///
    /// So "every `s`-referenced body entry is `t: 0` with `d: 0`" catches
    /// both, and it is a claim about a *value* rather than about the order of
    /// statements in a function body.
    ///
    /// # Fixture
    ///
    /// One inline footnote. `FootnotesTransform` runs in the q2-preview
    /// pipeline while `title-block` does not, and no project config is
    /// needed — so a capture that drifted downstream of the transforms would
    /// demonstrably pick up a `Generated` section here.
    ///
    /// # No line-level revert hunk
    ///
    /// There is no single production line whose flip reddens this test. The
    /// body is all-`Original` because `capture_untransformed_ast_json`
    /// re-parses the raw bytes through `qmd_to_pandoc` with a fresh,
    /// parent-less reader context (`pipeline.rs:1007`) — *not* because of the
    /// `parent_source_info: None` at `:1013`, which is built after the parse
    /// and read only by the JSON writer. The honest hunk is the rewrite the
    /// comment at `:914-919` invites ("derive the baseline from the
    /// pipeline's own parse"), under which the footnote transform's
    /// `Generated { by: footnotes() }` section enters the body. This test is
    /// therefore an invariant pin: it is green on arrival, and its job is to
    /// go red the day someone performs that rewrite.
    ///
    /// # Ownership
    ///
    /// The invariant load-bears for provenance correctness in `pampa`'s
    /// writer, but it is a property of a `quarto-core` function, and neither
    /// `quarto-source-map` nor `quarto-yaml` owns it. It is pinned here
    /// because here is where it can be broken.
    #[test]
    fn preview_untransformed_baseline_body_pool_is_all_original_own_file() {
        let content =
            b"---\ntitle: Test\nformat: q2-preview\n---\n\nA paragraph^[the footnote body].\n";

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::from_format_string("q2-preview").unwrap();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let runtime = make_test_runtime();
        let output = pollster::block_on(render_qmd_to_preview_ast(
            content,
            "test.qmd",
            &mut ctx,
            runtime,
            None,
            Vec::new(),
        ))
        .expect("q2-preview render");

        let baseline = output
            .untransformed_ast_json
            .expect("capture_untransformed_ast_json must succeed for a well-formed document");
        let baseline: serde_json::Value =
            serde_json::from_str(&baseline).expect("untransformed AST JSON must parse");
        let pool = baseline["astContext"]["p"]
            .as_array()
            .expect("untransformed AST must carry a source-info pool at astContext.p");

        // Every wire node carries its pool id under `"s"`.
        use std::collections::BTreeSet;
        fn collect_pool_ids(v: &serde_json::Value, out: &mut BTreeSet<usize>) {
            match v {
                serde_json::Value::Object(map) => {
                    for (key, child) in map {
                        match (key.as_str(), child.as_u64()) {
                            ("s", Some(id)) => {
                                out.insert(id as usize);
                            }
                            _ => collect_pool_ids(child, out),
                        }
                    }
                }
                serde_json::Value::Array(items) => {
                    for item in items {
                        collect_pool_ids(item, out);
                    }
                }
                _ => {}
            }
        }

        let mut body_ids = BTreeSet::new();
        collect_pool_ids(&baseline["blocks"], &mut body_ids);
        assert!(
            !body_ids.is_empty(),
            "no source-info ids reachable from the baseline body — the assertion \
             below would pass vacuously; the fixture or the wire format changed"
        );

        // The capture's filename table holds exactly one entry — the document
        // itself (`pipeline.rs:1010`) — so `FileId(0)` is this document and
        // any other value is foreign provenance.
        const OWN_FILE_ID: u64 = 0;
        let offenders: Vec<String> = body_ids
            .iter()
            .map(|&id| (id, &pool[id]))
            .filter(|(_, e)| e["t"].as_u64() != Some(0) || e["d"].as_u64() != Some(OWN_FILE_ID))
            .map(|(id, e)| format!("  [{id}] {e}"))
            .collect();
        assert!(
            offenders.is_empty(),
            "Every source-info referenced by a body node's `s` key must be an \
             `Original` (wire-code 0) rooted at the document's own \
             FileId({OWN_FILE_ID}); {} of {} are not:\n{}\n\n\
             Two changes cause this, and both make the incremental writer's \
             byte-copy arms unsafe. Those arms slice `original_qmd` at a \
             `preimage_in` range and emit those bytes as the node's text, \
             guarded for bounds and never for byte identity — in \
             `pampa/src/writers/incremental.rs`:\n\
             \x20 - `coarsen`, the `BlockAlignment::KeepBefore` arm (~:205);\n\
             \x20 - `assemble_inline_content`, the `InlineAlignment::KeepBefore` \
             arm (~:816);\n\
             \x20 - `assemble_recursed_container`, the verbatim early returns \
             (~:868).\n\
             The causes:\n\
             \x20 (1) a parent was threaded into the baseline parse, making body \
             nodes `Substring` (wire-code 1) over a possibly-`Concat` parent; or\n\
             \x20 (2) the capture moved downstream of the transform stages, so \
             transform-injected nodes (`Generated`, wire-code 4) or nodes from \
             another file entered the body.\n\
             Restore the invariant — do not relax this assertion. If the baseline \
             body must genuinely change shape, the three copy sites need a \
             byte-identity check first. Note that front-matter *metadata* entries \
             are legitimately `Substring`, and that attr/target refs are bare \
             integers this check does not reach — both deliberately out of \
             scope; see this test's doc comment.",
            offenders.len(),
            body_ids.len(),
            offenders.join("\n"),
        );
    }

    /// PR #214 follow-up probe: verify the q2-preview pipeline
    /// emits the `quarto-appendix` wrapper Div around the footnotes
    /// section. The smoke-all `q2-preview/multi-element-doc.qmd`
    /// fixture expects `div#quarto-appendix > div#footnotes` in the
    /// rendered iframe DOM; this regression test pins the Rust-side
    /// contract so we catch the wrapper drop before E2E does.
    #[test]
    fn render_qmd_to_preview_ast_emits_appendix_wrapper_for_footnotes() {
        let content =
            b"---\ntitle: Test\nformat: q2-preview\n---\n\nA paragraph^[the footnote body].\n";

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::from_format_string("q2-preview").unwrap();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let runtime = make_test_runtime();
        let output = pollster::block_on(render_qmd_to_preview_ast(
            content,
            "test.qmd",
            &mut ctx,
            runtime,
            None,
            Vec::new(),
        ))
        .expect("q2-preview render");

        // The appendix-structure transform must produce a Div with
        // id="quarto-appendix" wrapping the inner footnotes Div.
        assert!(
            output.ast_json.contains("quarto-appendix"),
            "expected `quarto-appendix` wrapper in q2-preview output; full output:\n{}",
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
        let pipeline = build_q2_preview_transform_pipeline(
            vec![],
            vec![],
            runtime,
            "q2-preview".to_string(),
            crate::format::PipelineProfile::HtmlPreview,
            None,
            Default::default(),
            None,
        );
        let names: Vec<&str> = pipeline.iter().map(|t| t.name()).collect();
        assert!(
            names.contains(&"link-rewrite"),
            "link-rewrite must be present in the q2-preview pipeline; got: {names:?}",
        );
    }

    /// Phase F.2 (bd-kw93.15): the chrome-rendering transforms run
    /// in the q2-preview pipeline so React's `PreviewDocument` can
    /// inject the produced HTML into the iframe via
    /// `dangerouslySetInnerHTML` slots. If any of these regress out,
    /// the SPA loses navbar/sidebar/page-nav/TOC/footer/favicon —
    /// the user-visible "looks like q2 render" promise of Phase F.
    #[test]
    fn q2_preview_pipeline_includes_chrome_transforms() {
        let runtime = make_test_runtime();
        let pipeline = build_q2_preview_transform_pipeline(
            vec![],
            vec![],
            runtime,
            "q2-preview".to_string(),
            crate::format::PipelineProfile::HtmlPreview,
            None,
            Default::default(),
            None,
        );
        let names: Vec<&str> = pipeline.iter().map(|t| t.name()).collect();
        for required in [
            "navbar-render",
            "sidebar-render",
            "page-nav-render",
            "toc-render",
            "footer-render",
            "website-favicon",
        ] {
            assert!(
                names.contains(&required),
                "{required} must be present in the q2-preview pipeline; got: {names:?}",
            );
        }
    }

    /// bd-nxslt: the q2-preview pipeline must run `CodeHighlightStage`
    /// so that code blocks reach the React renderer with `data-hl-spans`
    /// annotations and render highlighted (matching `q2 render`'s
    /// `<span class="hl-...">` markup). The stage is AST-level (it only
    /// adds an attribute to the existing `CodeBlock` node) so its
    /// inclusion in q2-preview is safe — the React `CodeBlock`
    /// component reads the attribute and emits the spans on the JS side.
    /// If this regresses out, `q2 preview` shows plain `<code>` for R /
    /// Python / etc. cells; `q2 render` keeps highlighting.
    #[test]
    fn q2_preview_pipeline_includes_code_highlight() {
        let stages = build_q2_preview_pipeline_stages(Vec::new());
        let names: Vec<&str> = stages.iter().map(|s| s.name()).collect();
        assert!(
            names.contains(&"code-highlight"),
            "code-highlight must be present in the q2-preview pipeline; got: {names:?}",
        );
    }

    /// Phase 0 of bd-1tl09 (code-block decorations epic). The
    /// `code-block-generate` / `code-block-render` pair is the
    /// architectural scaffolding for filename / copy / fold / etc.
    /// (Phases 1-3) and must be present in both the HTML pipeline and
    /// the q2-preview pipeline so the two render paths stay in sync.
    /// Phase 0 implementations are empty walkers; the assertions here
    /// only check presence and ordering relative to anchors.
    #[test]
    fn html_pipeline_includes_code_block_decoration_transforms() {
        let runtime = make_test_runtime();
        let pipeline = build_transform_pipeline(
            vec![],
            vec![],
            runtime,
            "html".to_string(),
            crate::format::PipelineProfile::HtmlRender,
            None,
            Default::default(),
            None,
        );
        let names: Vec<&str> = pipeline.iter().map(|t| t.name()).collect();

        let gen_pos = names.iter().position(|&n| n == "code-block-generate");
        let render_pos = names.iter().position(|&n| n == "code-block-render");
        assert!(
            gen_pos.is_some(),
            "code-block-generate must be in build_transform_pipeline; got: {names:?}",
        );
        assert!(
            render_pos.is_some(),
            "code-block-render must be in build_transform_pipeline; got: {names:?}",
        );

        // Generate must come before Render — sideband data flows in
        // that direction.
        assert!(
            gen_pos.unwrap() < render_pos.unwrap(),
            "code-block-generate must precede code-block-render; got positions \
             gen={:?}, render={:?} in {names:?}",
            gen_pos,
            render_pos,
        );

        // Generate runs in the Normalization Phase, after metadata is
        // resolved (so doc-level defaults like `code-copy: true` are
        // visible).
        let metadata_pos = names
            .iter()
            .position(|&n| n == "metadata-normalize")
            .expect("metadata-normalize anchor missing");
        assert!(
            gen_pos.unwrap() > metadata_pos,
            "code-block-generate must run after metadata-normalize; got positions \
             metadata={metadata_pos}, gen={:?} in {names:?}",
            gen_pos,
        );
    }

    /// bd-26bf3j1y: the secondary nav must be registered in the
    /// Navigation phase (bd-ersobfbt lifted the original native-only
    /// `cfg`, so the registration is now unconditional). Without this
    /// pin a refactor could silently drop the bar from every website
    /// and only the integration tests would notice.
    #[test]
    fn test_secondary_nav_registered_in_navigation_phase() {
        use crate::transform::TransformPhase;

        let runtime = make_test_runtime();
        let pipeline = build_transform_pipeline(
            vec![],
            vec![],
            runtime,
            "html".to_string(),
            crate::format::PipelineProfile::HtmlRender,
            None,
            Default::default(),
            None,
        );
        let found = pipeline
            .iter()
            .find(|t| t.name() == "secondary-nav-render")
            .map(|t| t.phase());

        assert_eq!(
            found,
            Some(TransformPhase::Navigation),
            "secondary-nav-render must be registered in the Navigation phase; \
             pipeline was: {:?}",
            pipeline.iter().map(|t| t.name()).collect::<Vec<_>>()
        );
    }

    /// bd-ersobfbt: quarto-nav-js must be registered in the Navigation
    /// phase AFTER navbar-render and secondary-nav-render — its predicate
    /// reads the `rendered.navigation.*` keys they write. Same pin
    /// rationale as `test_secondary_nav_registered_in_navigation_phase`.
    #[test]
    fn test_quarto_nav_js_registered_after_nav_renders() {
        use crate::transform::TransformPhase;

        let runtime = make_test_runtime();
        let pipeline = build_transform_pipeline(
            vec![],
            vec![],
            runtime,
            "html".to_string(),
            crate::format::PipelineProfile::HtmlRender,
            None,
            Default::default(),
            None,
        );
        let names: Vec<&str> = pipeline.iter().map(|t| t.name()).collect();
        let pos = |n: &str| names.iter().position(|x| *x == n);

        let nav_js = pos("quarto-nav-js").expect("quarto-nav-js registered");
        let nav_js_phase = pipeline
            .iter()
            .find(|t| t.name() == "quarto-nav-js")
            .map(|t| t.phase());
        assert_eq!(
            nav_js_phase,
            Some(TransformPhase::Navigation),
            "quarto-nav-js must be in the Navigation phase; pipeline: {names:?}"
        );
        let navbar = pos("navbar-render").expect("navbar-render registered");
        let secondary = pos("secondary-nav-render").expect("secondary-nav-render registered");
        assert!(
            nav_js > navbar && nav_js > secondary,
            "quarto-nav-js must run after navbar-render ({navbar}) and \
             secondary-nav-render ({secondary}), got {nav_js}; pipeline: {names:?}"
        );
    }

    /// bd-sidebar-title-with-navbar-82wxow6m: sidebar-render must be
    /// registered in the Navigation phase AFTER navbar-render — its
    /// sidebar-title gate reads the `rendered.navigation.navbar` key
    /// navbar-render writes. Same pin rationale as
    /// `test_quarto_nav_js_registered_after_nav_renders`.
    #[test]
    fn test_sidebar_render_registered_after_navbar_render() {
        use crate::transform::TransformPhase;

        let runtime = make_test_runtime();
        let pipeline = build_transform_pipeline(
            vec![],
            vec![],
            runtime,
            "html".to_string(),
            crate::format::PipelineProfile::HtmlRender,
            None,
            Default::default(),
            None,
        );
        let names: Vec<&str> = pipeline.iter().map(|t| t.name()).collect();
        let pos = |n: &str| names.iter().position(|x| *x == n);

        let sidebar = pos("sidebar-render").expect("sidebar-render registered");
        let sidebar_phase = pipeline
            .iter()
            .find(|t| t.name() == "sidebar-render")
            .map(|t| t.phase());
        assert_eq!(
            sidebar_phase,
            Some(TransformPhase::Navigation),
            "sidebar-render must be in the Navigation phase; pipeline: {names:?}"
        );
        let navbar = pos("navbar-render").expect("navbar-render registered");
        assert!(
            sidebar > navbar,
            "sidebar-render must run after navbar-render ({navbar}), got \
             {sidebar}; pipeline: {names:?}"
        );
    }

    /// Format-neutral pipeline phase-ordering invariant (bd-w0c6d38k).
    ///
    /// Every transform in `build_transform_pipeline` must (1) declare a real
    /// phase — not the `Unclassified` default — and (2) appear in non-decreasing
    /// phase-rank order. Together these forbid a format-specific *presentation*
    /// transform (e.g. revealjs auto-stretch, a `Finalization` transform) from
    /// running before the format-agnostic *semantic* structure it consumes (the
    /// `Crossref` phase) is established.
    ///
    /// The test loops over every render format string — there is deliberately
    /// **no `is_revealjs` branch** — so a new output format (`dashboard`,
    /// `typst`, `pdf`, …) is covered the moment its transforms are classified,
    /// without editing this test.
    ///
    /// # What the Pandoc/preview profiles add here — and what they do not
    ///
    /// Task 7 (T7.3) widened the loop from `["html", "revealjs"]` to a format
    /// string per [`crate::format::PipelineProfile`] variant. **Only
    /// assertion (1), exhaustiveness, gains discriminating power from that.**
    /// It is genuinely load-bearing: if P4/P7 ever splices a Pandoc-only
    /// transform into the pipeline without a `phase()` override, this is what
    /// catches it.
    ///
    /// Assertion (2), monotonicity, is **vacuous for every non-`*Render`
    /// profile, by construction.** Those pipelines are the `HtmlRender` /
    /// `RevealjsRender` pipeline with names removed (`retain_excluding`), and
    /// a subsequence of a non-decreasing sequence is always non-decreasing —
    /// so no regression the Pandoc work could introduce can make it fail.
    /// Do not read a green run here as evidence that the Pandoc profile's
    /// transform set is correct. The real discriminators for that are T2.3's
    /// exact surviving-name list
    /// (`t2_3_pandoc_docx_profile_produces_exact_surviving_name_list`) and
    /// T7.1's bucket check
    /// (`neutral_core_invariant_no_b2_b4_survives_the_pandoc_cut`).
    ///
    /// See `claude-notes/designs/transform-pipeline-phases.md`.
    #[test]
    fn test_build_transform_pipeline_phase_ordering() {
        use crate::transform::TransformPhase;

        // One format string per `PipelineProfile` variant: HtmlRender,
        // RevealjsRender, HtmlPreview, RevealjsPreview, Pandoc(_). Add new
        // format strings here as they land; the invariant then covers them
        // automatically.
        for format in ["html", "revealjs", "q2-preview", "q2-slides", "docx"] {
            let runtime = make_test_runtime();
            let pipeline = build_transform_pipeline(
                vec![],
                vec![],
                runtime,
                format.to_string(),
                crate::format::PipelineProfile::from_format(format),
                None,
                Default::default(),
                None,
            );
            let steps: Vec<(&str, TransformPhase)> =
                pipeline.iter().map(|t| (t.name(), t.phase())).collect();

            // (1) Exhaustiveness: every pipeline member must be classified.
            let unclassified: Vec<&str> = steps
                .iter()
                .filter(|(_, p)| *p == TransformPhase::Unclassified)
                .map(|(n, _)| *n)
                .collect();
            assert!(
                unclassified.is_empty(),
                "[{format}] these pipeline transforms have no phase() override \
                 (still TransformPhase::Unclassified) — classify them per \
                 claude-notes/designs/transform-pipeline-phases.md: {unclassified:?}",
            );

            // (2) Monotonicity: phase ranks must not decrease by position.
            for win in steps.windows(2) {
                let (prev_name, prev_phase) = win[0];
                let (next_name, next_phase) = win[1];
                assert!(
                    prev_phase <= next_phase,
                    "[{format}] phase ordering inversion: `{prev_name}` ({prev_phase:?}) \
                     runs before `{next_name}` ({next_phase:?}), but {prev_phase:?} \
                     ranks after {next_phase:?}. A transform that consumes semantic \
                     structure must not precede the phase that produces it. \
                     See claude-notes/designs/transform-pipeline-phases.md.\n\
                     Full order: {:?}",
                    steps
                        .iter()
                        .map(|(n, p)| format!("{n}:{p:?}"))
                        .collect::<Vec<_>>(),
                );
            }
        }
    }

    #[test]
    fn repo_actions_render_sits_between_its_producers_and_consumers() {
        let pipeline = build_transform_pipeline(
            vec![],
            vec![],
            make_test_runtime(),
            "html".to_string(),
            crate::format::PipelineProfile::HtmlRender,
            None,
            Default::default(),
            None,
        );
        let names: Vec<&str> = pipeline.iter().map(|t| t.name()).collect();
        let pos = |want: &str| {
            names
                .iter()
                .position(|n| *n == want)
                .unwrap_or_else(|| panic!("`{want}` must be in the html pipeline; got {names:?}"))
        };
        let actions = pos("repo-actions-render");

        assert!(
            pos("toc-render") < actions,
            "repo actions read rendered.navigation.toc to decide placement"
        );
        // The one that is easy to get wrong: `toc_block_html` runs inside
        // SidebarRenderTransform, so the website-left placement reads
        // `toc-actions` during that transform, not at template time.
        assert!(
            actions < pos("sidebar-render"),
            "sidebar-render builds the website-left TOC nav in Rust and must see toc-actions"
        );
        assert!(
            actions < pos("footer-render"),
            "footer-render consumes rendered.navigation.footer-actions"
        );
    }

    #[test]
    fn q2_preview_pipeline_includes_code_block_decoration_transforms() {
        let runtime = make_test_runtime();
        let pipeline = build_q2_preview_transform_pipeline(
            vec![],
            vec![],
            runtime,
            "q2-preview".to_string(),
            crate::format::PipelineProfile::HtmlPreview,
            None,
            Default::default(),
            None,
        );
        let names: Vec<&str> = pipeline.iter().map(|t| t.name()).collect();
        for required in ["code-block-generate", "code-block-render"] {
            assert!(
                names.contains(&required),
                "{required} must be present in the q2-preview pipeline so preview's React \
                 renderer sees the same decorated code blocks as `q2 render`; got: {names:?}",
            );
        }
    }

    /// bd-5m4ga0s1: `mermaid-render` must run for both HTML-family
    /// render formats, and must precede `code-block-render` so a
    /// diagram block is already a `RawBlock` before code-block chrome
    /// (copy button, filename header) would attach to it.
    #[test]
    fn mermaid_render_present_before_code_block_render() {
        for format in ["html", "revealjs"] {
            let runtime = make_test_runtime();
            let pipeline = build_transform_pipeline(
                vec![],
                vec![],
                runtime,
                format.to_string(),
                crate::format::PipelineProfile::from_format(format),
                None,
                Default::default(),
                None,
            );
            let names: Vec<&str> = pipeline.iter().map(|t| t.name()).collect();

            let mermaid_pos = names.iter().position(|&n| n == "mermaid-render");
            let cbr_pos = names.iter().position(|&n| n == "code-block-render");
            assert!(
                mermaid_pos.is_some(),
                "[{format}] mermaid-render must be in build_transform_pipeline; got: {names:?}",
            );
            assert!(
                mermaid_pos.unwrap() < cbr_pos.expect("code-block-render anchor missing"),
                "[{format}] mermaid-render must precede code-block-render; got positions \
                 mermaid={mermaid_pos:?}, code-block-render={cbr_pos:?} in {names:?}",
            );
        }
    }

    /// Task 5 (T5.6): the two footnotes-split halves must be registered
    /// **immediately adjacent**, `"footnotes"` then `"footnotes-resolve"`, in
    /// `HtmlRender` — not just both present somewhere. If another transform
    /// could be spliced between them it could consume/rewrite the
    /// `Inline::Note`s `"footnotes"` produces before `"footnotes-resolve"`
    /// ever saw them, and none of the footnotes snapshot tests would catch
    /// that (verified in Task 7's vacuity note). Also asserts the
    /// `Pandoc("docx")` half of the split: `"footnotes"` survives (pandoc's
    /// own writer needs native `Note`s) but `"footnotes-resolve"` does not
    /// (no HTML chrome to build for a Pandoc writer).
    #[test]
    fn t5_6_footnotes_halves_are_adjacent_and_split_correctly_by_profile() {
        let html_pipeline = build_transform_pipeline(
            vec![],
            vec![],
            make_test_runtime(),
            "html".to_string(),
            crate::format::PipelineProfile::HtmlRender,
            None,
            Default::default(),
            None,
        );
        let html_names: Vec<&str> = html_pipeline.iter().map(|t| t.name()).collect();
        let footnotes_pos = html_names.iter().position(|&n| n == "footnotes");
        let resolve_pos = html_names.iter().position(|&n| n == "footnotes-resolve");
        assert_eq!(
            footnotes_pos.map(|p| p + 1),
            resolve_pos,
            "[HtmlRender] \"footnotes-resolve\" must be registered immediately after \
             \"footnotes\"; got: {html_names:?}",
        );

        let docx_pipeline = build_transform_pipeline(
            vec![],
            vec![],
            make_test_runtime(),
            "docx".to_string(),
            crate::format::PipelineProfile::Pandoc("docx".to_string()),
            None,
            Default::default(),
            None,
        );
        let docx_names: Vec<&str> = docx_pipeline.iter().map(|t| t.name()).collect();
        assert!(
            docx_names.contains(&"footnotes"),
            "[Pandoc(\"docx\")] \"footnotes\" must survive so pandoc's own writer gets native \
             Note inlines; got: {docx_names:?}",
        );
        assert!(
            !docx_names.contains(&"footnotes-resolve"),
            "[Pandoc(\"docx\")] \"footnotes-resolve\" must NOT survive — a Pandoc writer has no \
             HTML chrome to build; got: {docx_names:?}",
        );
    }

    /// T4.1 (P7-foundation Task 4): the two B3 shared post-core services —
    /// `resource-collector` (mediabag/resource staging) and `link-rewrite`
    /// (body link/image rewriting) — are present in the `Pandoc("docx")`
    /// transform list, i.e. neither is on `PANDOC_TRANSFORM_EXCLUDED`. A
    /// presence check alone is not a substitute for exercising the real
    /// path end-to-end (see the E-tier `pandoc_b3_services` tests, which
    /// drive a real `q2 render --to docx` and inspect `word/media/`) — this
    /// row only guards the exclude-list itself.
    #[test]
    fn t4_1_pandoc_docx_pipeline_includes_b3_shared_services() {
        let docx_pipeline = build_transform_pipeline(
            vec![],
            vec![],
            make_test_runtime(),
            "docx".to_string(),
            crate::format::PipelineProfile::Pandoc("docx".to_string()),
            None,
            Default::default(),
            None,
        );
        let docx_names: Vec<&str> = docx_pipeline.iter().map(|t| t.name()).collect();
        assert!(
            docx_names.contains(&"resource-collector"),
            "[Pandoc(\"docx\")] \"resource-collector\" must survive — B3 shared service; \
             got: {docx_names:?}",
        );
        assert!(
            docx_names.contains(&"link-rewrite"),
            "[Pandoc(\"docx\")] \"link-rewrite\" must survive — B3 shared service; \
             got: {docx_names:?}",
        );
    }

    /// bd-5m4ga0s1: in `q2 preview` / hub-client the raw `CodeBlock`
    /// with class `mermaid` must survive to the React layer (the
    /// built-in mermaid component in ts-packages/preview-renderer owns
    /// rendering there, for both `q2-preview` and `q2-slides`). The
    /// transform is therefore on `Q2_PREVIEW_TRANSFORM_EXCLUDED`.
    #[test]
    fn q2_preview_pipeline_excludes_mermaid_render() {
        for format in ["q2-preview", "q2-slides"] {
            let runtime = make_test_runtime();
            let pipeline = build_q2_preview_transform_pipeline(
                vec![],
                vec![],
                runtime,
                format.to_string(),
                crate::format::PipelineProfile::from_format(format),
                None,
                Default::default(),
                None,
            );
            let names: Vec<&str> = pipeline.iter().map(|t| t.name()).collect();
            assert!(
                !names.contains(&"mermaid-render"),
                "[{format}] mermaid-render must NOT run in the preview pipeline — the React \
                 mermaid component consumes the raw CodeBlock; got: {names:?}",
            );
        }
    }

    /// Task 1 seam (T1.4): `build_transform_pipeline(HtmlRender)` must
    /// produce a byte-identical ordered transform-name list to today's
    /// `"html"` pipeline — this is the no-regression bar for replacing the
    /// inline `is_revealjs` family check with a `PipelineProfile` match.
    #[test]
    fn t1_4_html_render_profile_produces_todays_exact_name_list() {
        let pipeline = build_transform_pipeline(
            vec![],
            vec![],
            make_test_runtime(),
            "html".to_string(),
            crate::format::PipelineProfile::HtmlRender,
            None,
            Default::default(),
            None,
        );
        let names: Vec<&str> = pipeline.iter().map(|t| t.name()).collect();
        assert_eq!(
            names,
            [
                "conditional-content",
                "reference-link-diagnostics",
                "callout",
                "callout-resolve",
                "panel-tabset",
                "panel-tabset-resolve",
                "config-markdown",
                "shortcode-resolve",
                "metadata-normalize",
                "date-normalize",
                "authors-normalize",
                "title-banner",
                "code-block-generate",
                "website-title-prefix",
                "website-favicon",
                "website-bootstrap-icons",
                "website-canonical-url",
                "format-css",
                "draft-alert",
                "title-block",
                "sectionize",
                "footnotes",
                "footnotes-resolve",
                "example-embed",
                "theorem-sugar",
                "proof-sugar",
                "float-ref-target-sugar",
                "equation-label",
                "crossref-index",
                "crossref-resolve",
                "toc-generate",
                "navbar-generate",
                "sidebar-generate",
                "page-nav-generate",
                "footer-generate",
                "listing-generate",
                "listing-render",
                "categories-sidebar",
                "listing-feed-stage",
                "listing-feed-link",
                "toc-render",
                "toc-location",
                "repo-actions-render",
                "navbar-render",
                "sidebar-render",
                "breadcrumbs-render",
                "secondary-nav-render",
                "quarto-nav-js",
                "page-nav-render",
                "footer-render",
                "link-rewrite",
                "appendix-structure",
                "crossref-render",
                "example-embed-render",
                "mermaid-render",
                "code-block-render",
                "resource-collector",
                "hephaestus-render",
                "table-bootstrap-class",
                "responsive-image",
                "llms-capture",
                "attribution-render",
                "attribution-viewer",
            ]
        );
    }

    /// Task 1 seam (T1.5): `build_transform_pipeline(RevealjsRender)` must
    /// produce a byte-identical ordered transform-name list to today's
    /// `"revealjs"` pipeline — the reveal-family sibling of T1.4.
    #[test]
    fn t1_5_revealjs_render_profile_produces_todays_exact_name_list() {
        let pipeline = build_transform_pipeline(
            vec![],
            vec![],
            make_test_runtime(),
            "revealjs".to_string(),
            crate::format::PipelineProfile::RevealjsRender,
            None,
            Default::default(),
            None,
        );
        let names: Vec<&str> = pipeline.iter().map(|t| t.name()).collect();
        assert_eq!(
            names,
            [
                "conditional-content",
                "reference-link-diagnostics",
                "callout",
                "callout-resolve",
                "panel-tabset",
                "panel-tabset-resolve",
                "config-markdown",
                "shortcode-resolve",
                "metadata-normalize",
                "date-normalize",
                "authors-normalize",
                "title-banner",
                "code-block-generate",
                "website-title-prefix",
                "website-favicon",
                "website-bootstrap-icons",
                "website-canonical-url",
                "format-css",
                "draft-alert",
                "reveal-columns",
                "reveal-slides",
                "reveal-footer-alias",
                "footnotes",
                "footnotes-resolve",
                "reveal-footnotes",
                "example-embed",
                "theorem-sugar",
                "proof-sugar",
                "float-ref-target-sugar",
                "equation-label",
                "crossref-index",
                "crossref-resolve",
                "toc-generate",
                "navbar-generate",
                "sidebar-generate",
                "page-nav-generate",
                "footer-generate",
                "listing-generate",
                "listing-render",
                "categories-sidebar",
                "listing-feed-stage",
                "listing-feed-link",
                "toc-render",
                "toc-location",
                "repo-actions-render",
                "navbar-render",
                "sidebar-render",
                "breadcrumbs-render",
                "secondary-nav-render",
                "quarto-nav-js",
                "page-nav-render",
                "reveal-footer-logo",
                "link-rewrite",
                "appendix-structure",
                "crossref-render",
                "example-embed-render",
                "reveal-auto-stretch",
                "mermaid-render",
                "code-block-render",
                "resource-collector",
                "hephaestus-render",
                "table-bootstrap-class",
                "responsive-image",
                "llms-capture",
                "attribution-render",
                "attribution-viewer",
            ]
        );

        // Discriminator (T1.5's Test Seam Spec wording): the reveal-family
        // arm must actually replace the HTML-family one, not just append to
        // it — the HTML-only scaffolding transforms must be absent.
        assert!(!names.contains(&"title-block"));
        assert!(!names.contains(&"sectionize"));
    }

    /// Task 2 (T2.1): verify every name in [`PANDOC_TRANSFORM_EXCLUDED`] is
    /// an actual transform in the full HTML pipeline. Same drift-mode guard
    /// as `q2_preview_transform_excluded_names_exist_in_html_pipeline`: a
    /// renamed/typo'd transform silently no-ops in `retain_excluding`
    /// (`TransformPipeline::retain_excluding` has no unknown-name
    /// diagnostic), which would leak the "excluded" transform straight into
    /// Pandoc-writer output.
    #[test]
    fn pandoc_transform_excluded_names_exist_in_html_pipeline() {
        let runtime = make_test_runtime();
        let html = build_transform_pipeline(
            vec![],
            vec![],
            runtime,
            "html".to_string(),
            crate::format::PipelineProfile::HtmlRender,
            None,
            Default::default(),
            None,
        );
        let html_names: Vec<&str> = html.iter().map(|t| t.name()).collect();

        let unknown: Vec<&&str> = PANDOC_TRANSFORM_EXCLUDED
            .iter()
            .filter(|n| !html_names.contains(n))
            .collect();
        assert!(
            unknown.is_empty(),
            "PANDOC_TRANSFORM_EXCLUDED contains names not in build_transform_pipeline: \
             {unknown:?}. Likely a typo or a rename — update the const in pipeline.rs. \
             Full HTML transform list: {html_names:?}",
        );
    }

    /// Task 2 (T2.2): `PANDOC_TRANSFORM_EXCLUDED` must contain **every**
    /// transform in the `HtmlRender` pipeline whose `phase()` is
    /// `TransformPhase::Navigation` — derived by querying `phase()` rather
    /// than hand-enumerating, so a 21st Navigation transform can never land
    /// silently omitted from the Pandoc exclude-list (the exact drift mode
    /// that recurred three consecutive review rounds: 18 → 20 Navigation
    /// members). The `20` here is a tripwire on *pipeline growth*: both
    /// sides are derived from `build_transform_pipeline` + `phase()`, never
    /// from a hand-list, so it cannot go vacuous the way a
    /// hand-list-vs-hand-list comparison would.
    #[test]
    fn pandoc_transform_excluded_contains_every_navigation_phase_transform() {
        use crate::transform::TransformPhase;

        let runtime = make_test_runtime();
        let html = build_transform_pipeline(
            vec![],
            vec![],
            runtime,
            "html".to_string(),
            crate::format::PipelineProfile::HtmlRender,
            None,
            Default::default(),
            None,
        );
        let nav_names: Vec<&str> = html
            .iter()
            .filter(|t| t.phase() == TransformPhase::Navigation)
            .map(|t| t.name())
            .collect();

        assert_eq!(
            nav_names.len(),
            20,
            "expected exactly 20 Navigation-phase transforms in the HTML pipeline; got: \
             {nav_names:?}",
        );
        assert!(
            nav_names
                .iter()
                .all(|n| PANDOC_TRANSFORM_EXCLUDED.contains(n)),
            "PANDOC_TRANSFORM_EXCLUDED is missing a Navigation-phase transform: \
             Navigation names = {nav_names:?}, exclude-list = {PANDOC_TRANSFORM_EXCLUDED:?}",
        );
    }

    /// Task 2 (T2.3): `build_transform_pipeline(Pandoc("docx"))`'s surviving
    /// ordered name list, pinned exactly. This is the counterweight to the
    /// T2.2 subset check: T2.2 catches *omissions* from the exclude-list;
    /// this catches *over-exclusion* (e.g. wrongly adding `"panel-tabset"`,
    /// which would break P5's Route-R Tabset path while every
    /// absence-shaped assertion kept passing) and *under-application* (the
    /// `retain_excluding` call itself being missing from the `Pandoc(_)`
    /// arm). An exact, ordered `assert_eq!` is the only shape that pins
    /// both directions in one assertion.
    #[test]
    fn t2_3_pandoc_docx_profile_produces_exact_surviving_name_list() {
        let pipeline = build_transform_pipeline(
            vec![],
            vec![],
            make_test_runtime(),
            "docx".to_string(),
            crate::format::PipelineProfile::Pandoc("docx".to_string()),
            None,
            Default::default(),
            None,
        );
        let names: Vec<&str> = pipeline.iter().map(|t| t.name()).collect();
        assert_eq!(
            names,
            [
                "conditional-content",
                "reference-link-diagnostics",
                "callout",
                "panel-tabset",
                "config-markdown",
                "shortcode-resolve",
                "metadata-normalize",
                "date-normalize",
                "authors-normalize",
                "code-block-generate",
                "footnotes",
                "example-embed",
                "theorem-sugar",
                "proof-sugar",
                "float-ref-target-sugar",
                "equation-label",
                "crossref-index",
                "crossref-resolve",
                "link-rewrite",
                "appendix-structure",
                "example-embed-render",
                "resource-collector",
                "llms-capture",
            ]
        );

        for excluded in PANDOC_TRANSFORM_EXCLUDED {
            assert!(
                !names.contains(excluded),
                "`{excluded}` is on PANDOC_TRANSFORM_EXCLUDED but survived in the Pandoc(docx) \
                 pipeline; got: {names:?}",
            );
        }
    }

    /// Task 7 (T7.2): [`BUCKETS`] must classify **every** member of
    /// `build_transform_pipeline(HtmlRender)` exactly once, and must name
    /// nothing that is not a member.
    ///
    /// Both sides are derived from the real pipeline, so this cannot go
    /// vacuous the way a hand-list-vs-hand-list comparison would. It is the
    /// test whose absence let thirteen transforms go unclassified across
    /// three consecutive review rounds — see the doc comment on [`BUCKETS`]
    /// for the roll-call. Declaring `BUCKETS` in the module proper rather
    /// than in this test module is what keeps the check non-circular: a
    /// `#[cfg(test)]` const would make this assert a test fixture against the
    /// pipeline while the classification the design doc mirrors drifted.
    #[test]
    fn bucket_classification_is_total_over_the_html_pipeline() {
        let pipeline = build_transform_pipeline(
            vec![],
            vec![],
            make_test_runtime(),
            "html".to_string(),
            crate::format::PipelineProfile::HtmlRender,
            None,
            Default::default(),
            None,
        );
        let names: Vec<&str> = pipeline.iter().map(|t| t.name()).collect();

        let unbucketed: Vec<&&str> = names
            .iter()
            .filter(|n| !BUCKETS.iter().any(|(b, _)| b == *n))
            .collect();
        assert!(
            unbucketed.is_empty(),
            "these transforms are registered in build_transform_pipeline but have no \
             entry in BUCKETS: {unbucketed:?}. Classify each per §6 of \
             claude-notes/designs/pandoc-hybrid-architecture.md and add it to the const \
             in pipeline.rs.",
        );

        let stale: Vec<&&str> = BUCKETS
            .iter()
            .map(|(n, _)| n)
            .filter(|n| !names.contains(n))
            .collect();
        assert!(
            stale.is_empty(),
            "BUCKETS names transforms that are not members of \
             build_transform_pipeline(HtmlRender): {stale:?}. Likely a rename or a \
             removed transform — drop the stale entries. Full HTML transform list: \
             {names:?}",
        );

        let mut sorted: Vec<&str> = BUCKETS.iter().map(|(n, _)| *n).collect();
        sorted.sort_unstable();
        let duplicated: Vec<&str> = sorted
            .windows(2)
            .filter(|pair| pair[0] == pair[1])
            .map(|pair| pair[0])
            .collect();
        assert!(
            duplicated.is_empty(),
            "BUCKETS has duplicate entries (a transform must have exactly one bucket): \
             {duplicated:?}",
        );
    }

    /// Task 7 (T7.1): the epic's central correctness invariant — **no `B2` or
    /// `B4` transform survives to the Pandoc cut.**
    ///
    /// `crossref-render` is the load-bearing case: it destroys every numbered
    /// `CustomNode` into raw HTML `Figure`/`Div` structure, so if it ran
    /// before the cut, P5's shim would have nothing left to route. Dropping it
    /// from [`PANDOC_TRANSFORM_EXCLUDED`] reddens this test.
    ///
    /// Complements T2.3's exact surviving-name list: that pins *which* names
    /// survive, this pins *why* they are allowed to.
    #[test]
    fn neutral_core_invariant_no_b2_b4_survives_the_pandoc_cut() {
        let pipeline = build_transform_pipeline(
            vec![],
            vec![],
            make_test_runtime(),
            "docx".to_string(),
            crate::format::PipelineProfile::Pandoc("docx".to_string()),
            None,
            Default::default(),
            None,
        );

        let mut bad: Vec<(&str, Bucket)> = Vec::new();
        let mut unbucketed: Vec<&str> = Vec::new();
        for name in pipeline.iter().map(|t| t.name()) {
            match BUCKETS.iter().find(|(n, _)| *n == name) {
                Some((_, bucket)) if !bucket.survives_pandoc_cut() => bad.push((name, *bucket)),
                Some(_) => {}
                None => unbucketed.push(name),
            }
        }

        assert!(
            unbucketed.is_empty(),
            "these Pandoc(docx) survivors have no BUCKETS entry, so the invariant below \
             cannot be checked for them: {unbucketed:?}",
        );
        assert!(
            bad.is_empty(),
            "B2/B4 transforms survive to the Pandoc cut: {bad:?}. Only B1 (neutral \
             semantic core) and B3 (shared services) may reach a Pandoc writer — a B2/B4 \
             transform bakes HTML presentation over semantics the Pandoc writer needs \
             intact. Either add the name to PANDOC_TRANSFORM_EXCLUDED or, if the \
             classification is wrong, correct BUCKETS and §6 of \
             claude-notes/designs/pandoc-hybrid-architecture.md together.",
        );
    }

    /// Task 2 (T2.5): `build_transform_pipeline(HtmlPreview)`'s surviving
    /// name list must equal `build_q2_preview_transform_pipeline`'s output,
    /// captured here as a literal pre-refactor — the preview-parity gate for
    /// "one mechanism (the `PipelineProfile` match in
    /// `build_transform_pipeline`) serves both Preview and Pandoc". Calling
    /// `build_transform_pipeline` directly with `HtmlPreview` must already
    /// apply `Q2_PREVIEW_TRANSFORM_EXCLUDED`, not just the
    /// `build_q2_preview_transform_pipeline` wrapper.
    #[test]
    fn t2_5_html_preview_profile_produces_todays_preview_exact_name_list() {
        let pipeline = build_transform_pipeline(
            vec![],
            vec![],
            make_test_runtime(),
            "q2-preview".to_string(),
            crate::format::PipelineProfile::HtmlPreview,
            None,
            Default::default(),
            None,
        );
        let names: Vec<&str> = pipeline.iter().map(|t| t.name()).collect();

        let preview_pipeline = build_q2_preview_transform_pipeline(
            vec![],
            vec![],
            make_test_runtime(),
            "q2-preview".to_string(),
            crate::format::PipelineProfile::HtmlPreview,
            None,
            Default::default(),
            None,
        );
        let preview_names: Vec<&str> = preview_pipeline.iter().map(|t| t.name()).collect();

        assert_eq!(
            names, preview_names,
            "build_transform_pipeline(HtmlPreview) must match \
             build_q2_preview_transform_pipeline's surviving name list exactly",
        );

        assert_eq!(
            names,
            [
                "conditional-content",
                "reference-link-diagnostics",
                "callout",
                "config-markdown",
                "shortcode-resolve",
                "metadata-normalize",
                "date-normalize",
                "authors-normalize",
                "title-banner",
                "code-block-generate",
                "website-title-prefix",
                "website-favicon",
                "website-bootstrap-icons",
                "website-canonical-url",
                "format-css",
                "draft-alert",
                "sectionize",
                "footnotes",
                "footnotes-resolve",
                "example-embed",
                "theorem-sugar",
                "proof-sugar",
                "float-ref-target-sugar",
                "equation-label",
                "crossref-index",
                "crossref-resolve",
                "toc-generate",
                "navbar-generate",
                "sidebar-generate",
                "page-nav-generate",
                "footer-generate",
                "listing-generate",
                "listing-render",
                "categories-sidebar",
                "listing-feed-stage",
                "listing-feed-link",
                "toc-render",
                "toc-location",
                "repo-actions-render",
                "navbar-render",
                "sidebar-render",
                "breadcrumbs-render",
                "secondary-nav-render",
                "quarto-nav-js",
                "page-nav-render",
                "footer-render",
                "link-rewrite",
                "appendix-structure",
                "example-embed-render",
                "code-block-render",
                "resource-collector",
                "table-bootstrap-class",
                "responsive-image",
                "llms-capture",
                "attribution-render",
            ]
        );
    }

    /// Verify every name in [`Q2_PREVIEW_STAGE_EXCLUDED`] is an
    /// actual stage in the full HTML pipeline. Same drift-mode
    /// guard as
    /// `q2_preview_transform_excluded_names_exist_in_html_pipeline`,
    /// but at the stage level.
    #[test]
    fn q2_preview_stage_excluded_names_exist_in_html_pipeline() {
        let html_stages = build_html_pipeline_stages_with_options(None);
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

    /// Task 6 (T6.1): verify every name in [`PANDOC_STAGE_EXCLUDED`] is an
    /// actual stage in the full HTML pipeline. Same drift-mode guard as
    /// `q2_preview_stage_excluded_names_exist_in_html_pipeline`: a
    /// renamed/typo'd stage silently no-ops in `retain`, which would leak
    /// the "excluded" stage straight into a Pandoc-writer render.
    #[test]
    fn pandoc_stage_excluded_names_exist_in_html_pipeline() {
        let html_stages = build_html_pipeline_stages_with_options(None);
        let html_names: Vec<&str> = html_stages.iter().map(|s| s.name()).collect();

        let unknown: Vec<&&str> = PANDOC_STAGE_EXCLUDED
            .iter()
            .filter(|n| !html_names.contains(n))
            .collect();
        assert!(
            unknown.is_empty(),
            "PANDOC_STAGE_EXCLUDED contains names not in build_html_pipeline_stages: \
             {unknown:?}. Likely a typo or a rename — update the const in pipeline.rs. \
             Full HTML stage list: {html_names:?}",
        );
    }

    /// Task 6 (T6.2): `build_pandoc_pipeline_stages()`'s surviving ordered
    /// stage-name list, pinned exactly. Built via the real profile-selecting
    /// entry point (not a hand-filtered literal in the test body), so this
    /// both catches under-application (the `retain` call missing or
    /// misapplied) and over-exclusion (e.g. wrongly dropping
    /// `engine-execution`, which would silently skip code execution for a
    /// docx render — see the plan's 2026-04-20 `CodeHighlightStage`
    /// incident).
    #[test]
    fn t6_2_pandoc_stage_list_produces_exact_surviving_name_list() {
        let stages = build_pandoc_pipeline_stages(crate::format::FormatIdentifier::Docx);
        let names: Vec<&str> = stages.iter().map(|s| s.name()).collect();

        assert_eq!(
            names,
            [
                "source-conversion",
                "parse-document",
                "metadata-merge",
                "language-resolve",
                "include-expansion",
                "include-resolve",
                "listing-item-info",
                "document-profile",
                "link-resolution",
                "unwrap-profile",
                "pre-engine-sugaring",
                "engine-execution",
                "attribution-generate",
                "user-filters-pre",
                "ast-transforms",
                "user-filters-post",
                "resource-report",
                // Survives on purpose: NumberEncoding::Writer is a no-op for
                // every non-HTML format, and the stage still strips the
                // quarto-eq-number attribute before pandoc sees it.
                "equation-number",
                "pandoc-write",
            ]
        );

        for excluded in PANDOC_STAGE_EXCLUDED {
            assert!(
                !names.contains(excluded),
                "`{excluded}` is on PANDOC_STAGE_EXCLUDED but survived in the Pandoc stage \
                 list; got: {names:?}",
            );
        }
    }

    /// pandoc-hybrid-typst Phase 2: typst's stage list appends
    /// `typst-compile` after `pandoc-write` — pandoc's own `.typ` output
    /// is not the final artifact for typst the way it is for docx/pptx.
    ///
    /// Revert hunk: removing the `if format_identifier == ... Typst`
    /// branch in `build_pandoc_pipeline_stages` makes this RED (the tail
    /// would just be `"pandoc-write"`).
    #[test]
    fn typst_stage_list_appends_typst_compile_after_pandoc_write() {
        let stages = build_pandoc_pipeline_stages(crate::format::FormatIdentifier::Typst);
        let names: Vec<&str> = stages.iter().map(|s| s.name()).collect();
        assert_eq!(
            &names[names.len() - 2..],
            ["pandoc-write", "typst-compile"],
            "got: {names:?}"
        );
    }

    /// Task 6 (T6.3): `"attribution-generate"` is a real stage name but is
    /// **not** a member of `build_transform_pipeline(HtmlRender)`. This
    /// pins the structural fact behind the plan's round-4 correction (2):
    /// `AttributionGenerateStage` and `AttributionGenerateTransform` share a
    /// `name()`, but only the stage is a pipeline member — the transform is
    /// never pushed into `build_transform_pipeline` (it runs from inside
    /// `AstTransformsStage`). It has no revert hunk of its own: it asserts
    /// an existing structural fact rather than new behavior, and guards
    /// against `"attribution-generate"` ever being wrongly added to
    /// [`PANDOC_TRANSFORM_EXCLUDED`] (which would make T2.1's
    /// `unknown.is_empty()` go red for that reason, with this test staying
    /// green and naming why).
    #[test]
    fn t6_3_attribution_generate_is_stage_only_not_a_transform_pipeline_member() {
        let html_stages = build_html_pipeline_stages_with_options(None);
        let stage_names: Vec<&str> = html_stages.iter().map(|s| s.name()).collect();
        assert!(
            stage_names.contains(&"attribution-generate"),
            "expected `attribution-generate` to be a real stage name; got: {stage_names:?}",
        );

        let runtime = make_test_runtime();
        let html_transforms = build_transform_pipeline(
            vec![],
            vec![],
            runtime,
            "html".to_string(),
            crate::format::PipelineProfile::HtmlRender,
            None,
            Default::default(),
            None,
        );
        let transform_names: Vec<&str> = html_transforms.iter().map(|t| t.name()).collect();
        assert!(
            !transform_names.contains(&"attribution-generate"),
            "expected `attribution-generate` to NOT be a member of \
             build_transform_pipeline(HtmlRender) — AttributionGenerateTransform is never \
             pushed into the transform pipeline; got: {transform_names:?}",
        );
    }

    /// Phase 0 test #1 from `2026-05-13-q2-preview-attribution.md`.
    ///
    /// With a `PreBuiltAttributionProvider` installed on the
    /// `RenderContext`, `render_qmd_to_preview_ast` must surface
    /// `astContext.attribution` and `astContext.attributionActors` in
    /// the emitted JSON. Without a provider, those keys are absent
    /// — the byte-identicality regression guard for unflagged
    /// q2-preview renders.
    #[test]
    fn render_qmd_to_preview_ast_surfaces_attribution_when_provider_installed() {
        let content = b"---\ntitle: Test\nformat: q2-preview\n---\n\nHello world!\n".as_slice();

        // Run #1: no provider — keys must be absent.
        let baseline = {
            let project = make_test_project();
            let doc = DocumentInfo::from_path("/project/test.qmd");
            let format = Format::from_format_string("q2-preview").unwrap();
            let binaries = BinaryDependencies::new();
            let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

            let runtime = make_test_runtime();
            pollster::block_on(render_qmd_to_preview_ast(
                content,
                "test.qmd",
                &mut ctx,
                runtime,
                None,
                Vec::new(),
            ))
            .expect("baseline q2-preview render")
        };
        assert!(
            !baseline.ast_json.contains("\"attribution\""),
            "no-provider baseline must omit `attribution` key; got:\n{}",
            baseline.ast_json
        );
        assert!(
            !baseline.ast_json.contains("\"attributionActors\""),
            "no-provider baseline must omit `attributionActors` key; got:\n{}",
            baseline.ast_json
        );

        // Run #2: provider installed — keys must be present, with
        // the expected actor + identity surfaced.
        let attribution_json = serde_json::json!({
            "runs": [
                { "start": 0, "end": 10_000, "actor": "alice", "time": 42 }
            ],
            "identities": {
                "alice": { "name": "Alice", "color": "#ff0000" }
            }
        })
        .to_string();

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::from_format_string("q2-preview").unwrap();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        ctx.attribution_provider = Some(Arc::new(
            crate::attribution::PreBuiltAttributionProvider::new(attribution_json),
        ));

        let runtime = make_test_runtime();
        let output = pollster::block_on(render_qmd_to_preview_ast(
            content,
            "test.qmd",
            &mut ctx,
            runtime,
            None,
            Vec::new(),
        ))
        .expect("attributed q2-preview render");

        assert!(
            output.ast_json.contains("\"attribution\""),
            "expected `attribution` key in attributed q2-preview output; got:\n{}",
            output.ast_json
        );
        assert!(
            output.ast_json.contains("\"attributionActors\""),
            "expected `attributionActors` key in attributed q2-preview output; got:\n{}",
            output.ast_json
        );
        assert!(
            output.ast_json.contains("\"actor\":\"alice\""),
            "expected at least one record naming alice; got:\n{}",
            output.ast_json
        );
        assert!(
            output.ast_json.contains("\"name\":\"Alice\""),
            "expected alice's identity entry with display name; got:\n{}",
            output.ast_json
        );
        assert!(
            output.ast_json.contains("\"color\":\"#ff0000\""),
            "expected alice's identity entry with color; got:\n{}",
            output.ast_json
        );
    }

    // === bd-lone-bracket-diagnostic-mxu41qbt: `diagnostics:` suppression ===
    //
    // These exercise the *wiring*, not the policy parser (which has its own
    // unit tests in `diagnostic_policy.rs`): the policy must be resolved by
    // `MetadataMergeStage` from merged metadata and applied by
    // `run_pipeline` on the way out. `Q-2-45` is used as the specimen
    // because it is a per-document, coded warning that a two-line fixture
    // reliably triggers.

    /// `[label][ref]` reliably produces `Q-2-45`. This is the baseline the
    /// suppression tests below are measured against — without it, a
    /// suppression test could pass simply because the warning never fired.
    #[test]
    fn reference_link_warning_fires_without_suppression() {
        let content = b"---\ntitle: Test\n---\n\nSee [label][ref].\n";
        let diagnostics = render_and_collect_diagnostics(content);
        assert!(
            diagnostics.iter().any(|c| c == "Q-2-45"),
            "expected Q-2-45 in {diagnostics:?}"
        );
    }

    /// Front-matter suppression.
    #[test]
    fn document_metadata_suppresses_a_diagnostic() {
        let content = b"---\ntitle: Test\ndiagnostics:\n  Q-2-45: off\n---\n\nSee [label][ref].\n";
        let diagnostics = render_and_collect_diagnostics(content);
        assert!(
            !diagnostics.iter().any(|c| c == "Q-2-45"),
            "Q-2-45 should have been suppressed; got {diagnostics:?}"
        );
    }

    /// Suppressing one code must not silence the document wholesale.
    #[test]
    fn suppression_is_scoped_to_the_named_code() {
        let content = b"---\ntitle: Test\ndiagnostics:\n  Q-2-46: off\n---\n\nSee [label][ref].\n";
        let diagnostics = render_and_collect_diagnostics(content);
        assert!(
            diagnostics.iter().any(|c| c == "Q-2-45"),
            "suppressing Q-2-46 must leave Q-2-45 alone; got {diagnostics:?}"
        );
    }

    /// The long form, with a reason, behaves identically to the short form.
    #[test]
    fn long_form_suppression_works_end_to_end() {
        let content = b"---\ntitle: Test\ndiagnostics:\n  Q-2-45:\n    level: off\n    reason: legacy corpus\n---\n\nSee [label][ref].\n";
        let diagnostics = render_and_collect_diagnostics(content);
        assert!(
            !diagnostics.iter().any(|c| c == "Q-2-45"),
            "Q-2-45 should have been suppressed; got {diagnostics:?}"
        );
    }

    /// A malformed entry is reported (Q-5-27) rather than silently
    /// ignored, and does not suppress anything.
    #[test]
    fn malformed_suppression_entry_is_reported() {
        let content =
            b"---\ntitle: Test\ndiagnostics:\n  Q-2-45: shout\n---\n\nSee [label][ref].\n";
        let diagnostics = render_and_collect_diagnostics(content);
        assert!(
            diagnostics.iter().any(|c| c == "Q-5-27"),
            "expected the invalid-entry diagnostic; got {diagnostics:?}"
        );
        assert!(
            diagnostics.iter().any(|c| c == "Q-2-45"),
            "a malformed entry must not suppress; got {diagnostics:?}"
        );
    }

    /// Decision 3: suppression applies in the q2-preview pipeline too, not
    /// only under `quarto render`. Preview is where authors actually live,
    /// so a project that has opted out must not be nagged there.
    #[test]
    fn suppression_applies_in_the_preview_pipeline() {
        let content = b"---\ntitle: Test\ndiagnostics:\n  Q-2-45: off\n---\n\nSee [label][ref].\n";

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::from_format_string("q2-preview").unwrap();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        let runtime = make_test_runtime();

        let output = pollster::block_on(render_qmd_to_preview_ast(
            content,
            "test.qmd",
            &mut ctx,
            runtime,
            None,
            Vec::new(),
        ))
        .expect("q2-preview render");

        let codes: Vec<String> = output
            .diagnostics
            .iter()
            .filter_map(|d| d.code.clone())
            .collect();
        assert!(
            !codes.iter().any(|c| c == "Q-2-45"),
            "preview must honor suppression; got {codes:?}"
        );
    }

    /// Render `content` as HTML and return the codes of every diagnostic
    /// that survived the pipeline.
    fn render_and_collect_diagnostics(content: &[u8]) -> Vec<String> {
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
        .expect("render must succeed");

        output
            .diagnostics
            .iter()
            .filter_map(|d| d.code.clone())
            .collect()
    }
}
