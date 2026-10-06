/*
 * stage/stages/pandoc_write.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * The Pandoc-hybrid leg's writer stage: serializes the wire-format AST,
 * shells out to a real `pandoc` subprocess (with the vendored Q1 Lua
 * filters), and writes its own output file.
 */

//! `PandocWriteStage` — the docx/pptx tail of the Pandoc-hybrid render leg.
//!
//! Native-only: this stage shells out to a real `pandoc` binary via
//! `std::process::Command` and materializes the vendored filter tree via
//! `crate::pandoc_filters::bundle::extract_share_tree`, into the per-render
//! `ctx.temp_dir()` (no real filesystem on `wasm32-unknown-unknown`).
//! Mirrors the same gate as `crate::pandoc_filters::{bundle, harness}`.
//!
//! Unlike [`super::render_html::RenderHtmlBodyStage`], this stage's
//! `RenderedOutput.content` is always empty — no binary bytes travel
//! through `PipelineData`. The pandoc subprocess writes the output file
//! directly at `ctx.output_path()`, and downstream consumers (`FinalOutput`
//! relocation, etc.) key off `output_path`, not `content`.
//!
//! See `claude-notes/plans/2026-09-18-pandoc-hybrid-P4-implementation.md`
//! Task 9.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use async_trait::async_trait;
use quarto_error_reporting::DiagnosticMessage;

use crate::format::FormatIdentifier;
use crate::language::LanguageTerms;
use crate::pandoc_filters::bundle::{extract_formats_tree, extract_share_tree};
use crate::pandoc_filters::diagnostics::{classify_pandoc_stderr, nonzero_exit_error};
use crate::pandoc_filters::format_defaults::build_forwarded_args;
use crate::pandoc_filters::params::{
    BookSingleFileContributor, DocxCalloutIconsContributor, EntryPointFilter, FilterParamsBuilder,
    QuartoFilterEntryPointsContributor,
};
use crate::pandoc_filters::params_codec::encode_params_blob;
use crate::pandoc_filters::version;
use crate::stage::{
    PipelineData, PipelineDataKind, PipelineError, PipelineStage, RenderedOutput, StageContext,
};

/// Decides how to handle a completed pandoc invocation's exit status and
/// captured stderr — the corrected policy (commit `0b295831e`): capture
/// stderr **unconditionally**, not only on failure.
///
/// On success, classifies `[WARNING]`-shaped stderr lines as `Q-11-1`
/// diagnostics via [`classify_pandoc_stderr`] (may return an empty `Vec`).
/// On failure, builds the `Q-20-3` error via [`nonzero_exit_error`],
/// wrapping stderr verbatim and naming `json_path`.
///
/// `success`/`status_desc` are taken separately rather than as a single
/// `std::process::ExitStatus` so this function stays platform-neutral and
/// directly unit-testable with injected values (see
/// `crates/quarto-core/tests/integration/pandoc_transport.rs` T10.1-T10.5).
pub fn classify_pandoc_completion(
    stage_name: &str,
    success: bool,
    status_desc: &str,
    stderr: &str,
    json_path: &Path,
) -> Result<Vec<DiagnosticMessage>, PipelineError> {
    if success {
        Ok(classify_pandoc_stderr(stderr))
    } else {
        Err(nonzero_exit_error(
            stage_name,
            status_desc,
            stderr,
            json_path,
        ))
    }
}

/// Removes the temp JSON input on a successful render; leaves it on disk
/// on failure, where a future debugging session may want to inspect the
/// exact input pandoc choked on.
///
/// A missing/already-removed file on the success path is not an error —
/// `remove_file`'s result is deliberately discarded.
pub fn retain_temp_json_unless_success(success: bool, json_path: &Path) {
    if success {
        let _ = std::fs::remove_file(json_path);
    }
}

/// Resolves the `pandoc` binary via `runtime.find_binary` (honouring
/// `QUARTO_PANDOC`, per `render.rs`'s `BinaryDependencies::discover`) and
/// runs [`version::gate`] against it, surfacing `Q-20-1`/`Q-20-2` as a
/// `PipelineError` before any real invocation is attempted.
///
/// Returns the resolved binary path (or the literal `"pandoc"` when
/// resolution fails — a state `gate` above already turned into an `Err`,
/// so this fallback value is never actually reached in a caller that
/// propagates the `?`).
fn resolve_and_gate_pandoc(
    stage_name: &str,
    runtime: &dyn quarto_system_runtime::SystemRuntime,
) -> Result<std::path::PathBuf, PipelineError> {
    let pandoc_path = runtime.find_binary("pandoc", "QUARTO_PANDOC");

    let version_output = pandoc_path
        .as_ref()
        .and_then(|p| Command::new(p).arg("--version").output().ok());
    let version_str = version_output.map(|o| String::from_utf8_lossy(&o.stdout).into_owned());

    version::gate(version_str.as_deref())
        .map_err(|diag| PipelineError::stage_error_with_diagnostics(stage_name, vec![diag]))?;

    Ok(pandoc_path.unwrap_or_else(|| std::path::PathBuf::from("pandoc")))
}

/// Format-specific extra pandoc CLI args, appended before `-o <output>`.
///
/// Epub only, for now (Q1's `createEbookFormat`,
/// `claude-notes/plans/2026-09-18-pandoc-hybrid-epub.md` Phase 1):
/// `--default-image-extension=png`, `--math-method=mathml` (epub3's own
/// default already produces MathML — set explicitly to match Q1's intent
/// rather than rely on an unstated Pandoc default), two
/// `--include-in-header` CSS files extracted from the embedded
/// `FORMATS_DIR` tree, and (when the document sets it) `--split-level`
/// from the `epub-chapter-level` metadata key — Q1's name for what Pandoc
/// itself calls `--split-level` (`--epub-chapter-level` is Pandoc's own
/// deprecated synonym for the same flag). Deliberately **not** merged into
/// one file (Q1's `merge-includes: false`) — passing two separate
/// `--include-in-header` flags already keeps them from colliding, with no
/// merge step to disable.
/// Collects the string-bearing leaves of a scalar-or-array metadata value
/// (mirrors `project::format_paths::for_each_entry`, which is private to
/// that module).
fn entry_strings(value: &quarto_pandoc_types::ConfigValue) -> Vec<String> {
    match value.as_array() {
        Some(items) => items.iter().filter_map(|v| v.as_plain_text()).collect(),
        None => value.as_plain_text().into_iter().collect(),
    }
}

/// Resolves a `mark_format_path_values`-normalized path value (already
/// document-relative — see `project::format_paths`) against the
/// document's own directory, producing an absolute path pandoc's
/// subprocess can open regardless of its own cwd.
fn resolve_doc_relative(doc_dir: &Path, declared: &str) -> PathBuf {
    doc_dir.join(declared)
}

fn epub_extra_args(
    temp_dir: &Path,
    doc_path: &Path,
    meta: &quarto_pandoc_types::ConfigValue,
) -> Result<Vec<OsString>, PipelineError> {
    let doc_dir = doc_path.parent().unwrap_or_else(|| Path::new("."));
    let formats_root = temp_dir.join("pandoc-formats");
    std::fs::create_dir_all(&formats_root).map_err(|e| {
        PipelineError::stage_error(
            "pandoc-write",
            format!("failed to create formats directory: {e}"),
        )
    })?;
    extract_formats_tree(&formats_root).map_err(|e| {
        PipelineError::stage_error(
            "pandoc-write",
            format!("failed to materialize vendored format resources: {e}"),
        )
    })?;
    let formats_dest = formats_root.join("formats");
    let mut args = vec![
        OsString::from("--default-image-extension=png"),
        OsString::from("--math-method=mathml"),
        {
            let mut arg = OsString::from("--include-in-header=");
            arg.push(formats_dest.join("html").join("styles-callout.html"));
            arg
        },
        {
            let mut arg = OsString::from("--include-in-header=");
            arg.push(formats_dest.join("epub").join("styles.html"));
            arg
        },
    ];
    if let Some(level) = meta
        .get("epub-chapter-level")
        .and_then(|v| v.as_int_lenient())
    {
        args.push(OsString::from(format!("--split-level={level}")));
    }

    // Single-valued path keys: mark_format_path_values normalized these
    // to a document-relative `Path` value at merge time (see
    // `project::format_paths::FORMAT_PATH_KEYS`); resolve against the
    // document's own directory to hand pandoc an absolute path.
    for (key, flag) in [
        ("epub-cover-image", "--epub-cover-image="),
        ("epub-metadata", "--epub-metadata="),
    ] {
        if let Some(declared) = meta.get(key).and_then(|v| v.as_plain_text()) {
            let mut arg = OsString::from(flag);
            arg.push(resolve_doc_relative(doc_dir, &declared));
            args.push(arg);
        }
    }

    // Repeatable path keys: pandoc accepts multiple --epub-embed-font /
    // --css flags, one per file.
    for (key, flag) in [("epub-embed-font", "--epub-embed-font="), ("css", "--css=")] {
        if let Some(value) = meta.get(key) {
            for declared in entry_strings(value) {
                let mut arg = OsString::from(flag);
                arg.push(resolve_doc_relative(doc_dir, &declared));
                args.push(arg);
            }
        }
    }

    // Not a path — an internal directory *name* inside the epub
    // container, passed through verbatim.
    if let Some(subdir) = meta
        .get("epub-subdirectory")
        .and_then(|v| v.as_plain_text())
    {
        args.push(OsString::from(format!("--epub-subdirectory={subdir}")));
    }

    Ok(args)
}

/// Resolves the `brand`, `logo`, and `brand-mode` filter params for a
/// typst render (Phase 1's `extractTypstFilterParams` bullet, extended
/// by bd-67i2z57f to resolve both halves of the brand and wire the
/// document's own `brand-mode`). `meta` is the document's
/// already-merged metadata (`doc.ast.meta`, a `ConfigValue`) — the same
/// config shape every other brand consumer reads a brand out of, but
/// unlike the single-variant consumers (`quarto_sass::resolve_brand`'s
/// doc comment: favicon fallback, reveal) this uses
/// [`quarto_sass::resolve_brand_variants`] because Typst picks its
/// brand mode per document (`brand-mode:` / `format.typst.brand-mode:`)
/// rather than compiling a light/dark CSS pair the browser toggles
/// between. Returns `Ok((None, None, None))` for a brand-less,
/// logo-less document, which is the common case and not an error.
///
/// The `brand`/`logo` params are resolved together because
/// [`typst_brand::build_logo_param`] reads the brand image path back
/// out of the already-built `brand` JSON rather than re-resolving the
/// brand a second time.
///
/// Errors ([`quarto_sass::SassError`] — invalid `_brand.yml` shape, a bad
/// font weight, a missing brand file) are real user-facing configuration
/// problems, mapped through the same `sass_error_to_parse_error` bridge
/// `compile_theme_css` uses. The candidate-source list here is smaller
/// than that stage's (no profile overlays/extension manifests) because a
/// `brand:` value reaching `PandocWriteStage` came from either the
/// project config or the document itself — good enough for the span
/// binding to land on the right file in the common case.
#[allow(clippy::type_complexity)]
fn resolve_typst_brand(
    meta: &quarto_pandoc_types::ConfigValue,
    ctx: &StageContext,
) -> Result<
    (
        Option<quarto_brand::ResolvedBrand>,
        Option<quarto_brand::ResolvedBrand>,
    ),
    PipelineError,
> {
    quarto_sass::resolve_brand_variants(meta, ctx.runtime.as_ref(), &ctx.project.dir).map_err(|e| {
        let mut candidates: Vec<(quarto_source_map::FileId, std::path::PathBuf)> = Vec::new();
        if let Some(p) = ctx.project.config.config_path.as_deref() {
            candidates.push((
                quarto_yaml::file_id_for_filename(&p.to_string_lossy()),
                p.to_path_buf(),
            ));
        }
        candidates.push((quarto_source_map::FileId(0), ctx.document.input.clone()));
        PipelineError::Structured(crate::theme_diagnostic::sass_error_to_parse_error(
            &e,
            &candidates,
        ))
    })
}

/// Builds the `brand`, `logo` and `brand-mode` filter params from
/// already-resolved brand variants (see [`resolve_typst_brand`]).
fn resolve_typst_brand_param(
    meta: &quarto_pandoc_types::ConfigValue,
    light: Option<&quarto_brand::ResolvedBrand>,
    dark: Option<&quarto_brand::ResolvedBrand>,
    ctx: &StageContext,
) -> (
    Option<serde_json::Value>,
    Option<serde_json::Value>,
    Option<String>,
) {
    // The compiled `.typ` file (and, from it, the final PDF) is written
    // at `ctx.output_path()`'s directory — for a single document that's
    // ordinarily the project root, but for a book render it's the
    // project's output directory (e.g. `_book/`). Typst resolves a
    // relative logo path against *that* directory (the directory of the
    // file doing the `image(...)` call), not the project root, so the
    // brand's own path-rewriting must use the same base or a book
    // render's brand logo comes out one level too shallow (P8
    // orange-book: `_book/Test-Typst-Book.typ` importing `logo.svg`
    // failed to find it at `_book/logo.svg` — the real file is one
    // level up, at the project root).
    let output_dir = ctx
        .output_path()
        .parent()
        .map_or_else(|| ctx.project.dir.clone(), std::path::Path::to_path_buf);
    let brand_param =
        crate::pandoc_filters::typst_brand::build_brand_param(light, dark, &output_dir);
    let logo_param =
        crate::pandoc_filters::typst_brand::build_logo_param(meta, brand_param.as_ref());
    // `typst-brand-yaml.lua`'s `param('brand-mode') or 'light'` already
    // supplies the default, so the key is only emitted when the
    // document (or its `format.typst.brand-mode`, flattened into `meta`
    // by `MetadataMergeStage`) sets one explicitly.
    let brand_mode = meta.get("brand-mode").and_then(|v| v.as_plain_text());
    (brand_param, logo_param, brand_mode)
}

/// Download the brand's `source: google` fonts into the project's font
/// cache so `typst fonts` and `typst compile` can see them (see
/// [`crate::typst_google_fonts`]). Failures only warn: the font then falls
/// back, as Typst did before this existed.
fn stage_typst_brand_fonts(brand: Option<&quarto_brand::ResolvedBrand>, ctx: &mut StageContext) {
    let Some(brand) = brand else {
        return;
    };
    let runtime = ctx.runtime.clone();
    let fetch = |url: &str| {
        pollster::block_on(runtime.fetch_url(url))
            .map(|(bytes, _mime)| bytes)
            .map_err(|e| e.to_string())
    };
    let diagnostics = crate::typst_google_fonts::stage_brand_fonts(
        brand,
        &crate::typst_google_fonts::font_cache_dir(&ctx.project.dir),
        &fetch,
    );
    ctx.add_diagnostics(diagnostics);
}

/// Resolves the `typst-available-fonts` filter param (pandoc-hybrid-typst
/// Phase 2's `getAvailableTypstFonts` bullet, deferred by Phase 1). Stages
/// the same vendored packages/fonts tree `TypstCompileStage` will stage
/// again later in this render (idempotent — both extract into the same
/// `ctx.temp_dir()`-scoped directory) so `typst fonts` is asked about
/// exactly the font-path Typst will actually compile against.
///
/// Returns `Ok(None)` — not an error — when the `typst` binary can't be
/// found or `typst fonts` fails; that failure mode belongs to
/// `TypstCompileStage`'s later, clearer `Q-19-*` diagnostic, and the Lua
/// consumer already treats an absent param as fully permissive.
///
/// Resolves the document's own `font-paths` metadata the same way
/// `TypstCompileStage` will (see `typst_compile::resolve_font_paths`), so
/// `typst fonts` is asked about the exact same font-path set the real
/// compile uses — otherwise the CSS font-fallback filter list could
/// silently disagree with what Typst actually finds.
fn resolve_typst_available_fonts(
    stage_name: &str,
    meta: &quarto_pandoc_types::ConfigValue,
    brand: Option<&quarto_brand::ResolvedBrand>,
    input_path: &Path,
    ctx: &mut StageContext,
) -> Result<Option<Vec<String>>, PipelineError> {
    let temp_dir = ctx.temp_dir()?.to_path_buf();
    let packages_dir = temp_dir.join("typst-packages");
    std::fs::create_dir_all(&packages_dir).map_err(|e| {
        PipelineError::stage_error(
            stage_name,
            format!("failed to create typst packages directory: {e}"),
        )
    })?;
    crate::pandoc_filters::bundle::extract_typst_packages(&packages_dir).map_err(|e| {
        PipelineError::stage_error(
            stage_name,
            format!("failed to materialize vendored typst packages: {e}"),
        )
    })?;
    let typst_path = ctx.runtime.find_binary("typst", "QUARTO_TYPST");
    let input_dir = input_path
        .parent()
        .map_or_else(|| ctx.project.dir.clone(), std::path::Path::to_path_buf);
    let extra_font_paths = super::typst_compile::with_google_font_cache(
        super::typst_compile::with_brand_file_fonts(
            super::typst_compile::resolve_font_paths(
                &super::typst_compile::string_array(meta.get("font-paths")),
                &ctx.project.dir,
                &input_dir,
            ),
            brand,
            &ctx.project.dir,
        ),
        &ctx.project.dir,
    );
    let font_args = super::typst_compile::font_path_args(&packages_dir, &extra_font_paths);
    Ok(super::typst_compile::discover_available_typst_fonts(
        typst_path.as_deref(),
        &font_args,
    ))
}

pub struct PandocWriteStage;

impl PandocWriteStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PandocWriteStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for PandocWriteStage {
    fn name(&self) -> &str {
        "pandoc-write"
    }

    fn input_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    fn output_kind(&self) -> PipelineDataKind {
        PipelineDataKind::RenderedOutput
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

        // P7 Task 6: `title`/`subtitle` reaching Pandoc `Meta` as
        // `MetaBlocks` is not what the docx/pptx writers read for
        // `title` — coerce to `MetaInlines` before serialization.
        crate::pandoc_filters::meta_coerce::coerce_meta_blocks_to_inlines(&mut doc.ast.meta);

        // pandoc-hybrid-typst Phase 1: `section-numbering`/
        // `shift-heading-level-by` (`format-typst.ts:82-100`) are
        // typst-only and must be resolved right here, immediately before
        // the wire-format cut — `section-numbering` mutates the metadata
        // that's about to be serialized below; `shift-heading-level-by`
        // is computed from the same fully resolved `Block` list, not
        // from `DocumentProfile.outline` (see the two functions' doc
        // comments for why).
        let shift_heading_level_by =
            if ctx.format.identifier == crate::format::FormatIdentifier::Typst {
                insert_typst_section_numbering(&mut doc.ast.meta);
                shift_heading_level_by_for(&doc.ast.blocks, &doc.ast.meta)
            } else {
                None
            };

        // Findings 3/4 (final review): resolve the binary through the
        // runtime (honouring `QUARTO_PANDOC`) and enforce the version
        // floor before spawning anything.
        let pandoc_bin = resolve_and_gate_pandoc(self.name(), ctx.runtime.as_ref())?;

        // `LanguageResolveStage` (earlier in the shared prefix) populates
        // `quarto.language`; the `unwrap_or_else` fallback only matters for
        // a caller that assembles a bare stage list without it.
        let language = LanguageTerms::from_meta(&doc.ast.meta)
            .unwrap_or_else(|| crate::language::resolve_language("en", &[]));

        // T9.5: the serialized JSON lives inside the per-render temp
        // directory, not beside the output file or the process cwd.
        let temp_dir = ctx.temp_dir()?.to_path_buf();
        let results_file = temp_dir.join("pandoc-results.json");

        // Finding 5 (final review): extracted into the per-render temp
        // dir rather than a process-global cache, so cleanup rides on
        // `ctx.temp_dir()`'s existing lifecycle instead of leaking.
        // Computed here (before the actual extraction, below) because
        // Task 4's docx callout-icon params need the path to embed in
        // `QUARTO_FILTER_PARAMS` — the files only need to exist on disk by
        // the time pandoc actually runs, not when this string is built.
        let share = temp_dir.join("pandoc-share");

        // pandoc-hybrid-typst Phase 1: `extractTypstFilterParams` — the
        // `brand`/`logo`/`brand-mode` keys, typst-only (see `typst_params`'s
        // module docs for why `brand` isn't a core, format-independent key
        // yet). Resolved from the document's own merged metadata via
        // `quarto_sass::resolve_brand_variants` (bd-67i2z57f): both halves
        // of the brand are resolved (unlike the single-variant consumers —
        // favicon fallback, reveal — which only ever need `light`), and
        // `brand-mode` is read straight off `meta` so the vendored Lua
        // filter's `param('brand-mode') or 'light'` picks the document's
        // own mode instead of always falling back to light.
        let (
            (typst_brand_param, typst_logo_param, typst_brand_mode),
            typst_available_fonts,
            typst_citation_location,
            typst_reference_location,
            typst_cite_method,
            typst_code_block_bg,
        ) = if ctx.format.identifier == crate::format::FormatIdentifier::Typst {
            let (light, dark) = resolve_typst_brand(&doc.ast.meta, ctx)?;
            // Before `typst fonts` runs: the available-fonts list must
            // include the fonts we are about to make available.
            stage_typst_brand_fonts(light.as_ref(), ctx);
            stage_typst_brand_fonts(dark.as_ref(), ctx);
            // The brand's `monospace-block` background replaces the
            // highlight palette's code-block background.
            let code_block_bg = super::typst_compile::brand_for_mode(
                light.as_ref(),
                dark.as_ref(),
                doc.ast
                    .meta
                    .get("brand-mode")
                    .and_then(|v| v.as_plain_text())
                    .as_deref(),
            )
            .and_then(|resolved| {
                let name = resolved
                    .brand
                    .effective_monospace_block()?
                    .background_color?;
                Some(resolved.brand.resolve_color_quiet(&name))
            });
            (
                resolve_typst_brand_param(&doc.ast.meta, light.as_ref(), dark.as_ref(), ctx),
                resolve_typst_available_fonts(
                    self.name(),
                    &doc.ast.meta,
                    super::typst_compile::brand_for_mode(
                        light.as_ref(),
                        dark.as_ref(),
                        doc.ast
                            .meta
                            .get("brand-mode")
                            .and_then(|v| v.as_plain_text())
                            .as_deref(),
                    ),
                    &doc.path,
                    ctx,
                )?,
                doc.ast
                    .meta
                    .get("citation-location")
                    .and_then(|value| value.as_plain_text()),
                doc.ast
                    .meta
                    .get("reference-location")
                    .and_then(|value| value.as_plain_text()),
                // `quarto.doc.cite_method()` (`init.lua:939-940`) drives
                // `quarto-post/typst.lua`'s margin-citation `Cite` handler:
                // whether to use pre-rendered citeproc bibliography entries
                // in the margin note, or emit a bare native
                // `#cite(<id>, form: "full")`. Unlike the LaTeX-only
                // `cite-method` consumers in `bibliography.lua`/`meta.lua`
                // (which default to `'citeproc'` when unset, since Pandoc's
                // own citeproc pass is the ordinary default there), margin
                // citations default to *native* Typst rendering — confirmed
                // by `citation-margin-basic.qmd` (no `citeproc` key, asserts
                // native `#cite(..., form: "full")` output) vs.
                // `citation-margin-citeproc.qmd` (`citeproc: true`, asserts
                // citeproc-rendered text). So only emit this key when the
                // doc opts in explicitly; leaving it unset preserves the
                // existing native-by-default margin behavior.
                doc.ast
                    .meta
                    .get("citeproc")
                    .and_then(|value| value.as_bool())
                    .and_then(|is_citeproc| is_citeproc.then(|| "citeproc".to_string())),
                code_block_bg,
            )
        } else {
            ((None, None, None), None, None, None, None, None)
        };

        // `mediabag-dir`: `<output-dir>/<stem>_files/mediabag`, mirroring
        // Q1's `render.ts:119-120` unconditional assignment. Computed from
        // the *final* output path (not the `output_path` local below, which
        // for typst is the intermediate `.typ` — same directory and stem,
        // just a different extension, so either would do; this one is
        // available before `params_blob` needs it). See `insert_mediabag_dir`
        // in `pandoc_filters::params` for why every format gets this key.
        let mediabag_dir = {
            let out = ctx.output_path();
            let parent = out.parent().unwrap_or_else(|| Path::new("."));
            let stem = out
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("quarto-output");
            parent
                .join(crate::resources::resource_dir_name(stem))
                .join("mediabag")
        };

        let mut builder = FilterParamsBuilder::new(
            &ctx.format,
            &ctx.project,
            ctx.ref_type_registry.as_ref(),
            &language,
            results_file,
            mediabag_dir,
        );
        // P7 Task 4: the 5 docx callout-icon params
        // (`docxCalloutImage`/`param("icon-" .. type, nil)`,
        // `resources/pandoc-filters/filters/modules/callouts.lua`) — docx
        // only; pptx has no callout-icon consumer in the vendored filters.
        if ctx.format.output_extension == "docx" {
            builder = builder.with_contributor(Box::new(DocxCalloutIconsContributor {
                share_dir: share.clone(),
            }));
        }
        let typst_root_dir = if ctx.format.identifier == crate::format::FormatIdentifier::Typst {
            Some(ctx.project.dir.clone())
        } else {
            None
        };
        let typst_code_line_numbers =
            if ctx.format.identifier == crate::format::FormatIdentifier::Typst {
                doc.ast
                    .meta
                    .get("code-line-numbers")
                    .and_then(|v| v.as_bool())
            } else {
                None
            };
        let typst_css_property_processing =
            if ctx.format.identifier == crate::format::FormatIdentifier::Typst {
                doc.ast
                    .meta
                    .get("css-property-processing")
                    .and_then(|v| v.as_plain_text())
            } else {
                None
            };
        if typst_brand_param.is_some()
            || typst_css_property_processing.is_some()
            || typst_logo_param.is_some()
            || typst_brand_mode.is_some()
            || typst_available_fonts.is_some()
            || typst_citation_location.is_some()
            || typst_reference_location.is_some()
            || typst_root_dir.is_some()
            || typst_cite_method.is_some()
            || typst_code_line_numbers.is_some()
        {
            builder = builder.with_contributor(Box::new(
                crate::pandoc_filters::typst_params::TypstFilterParamsContributor {
                    brand: typst_brand_param,
                    logo: typst_logo_param,
                    brand_mode: typst_brand_mode.clone(),
                    css_property_processing: typst_css_property_processing,
                    available_fonts: typst_available_fonts,
                    citation_location: typst_citation_location,
                    reference_location: typst_reference_location,
                    root_dir: typst_root_dir,
                    cite_method: typst_cite_method,
                    code_line_numbers: typst_code_line_numbers,
                },
            ));
        }
        // book-projects P2: the merge step is the first thing that knows
        // this render is a book single-file merge, and it hands that
        // knowledge down as a plain metadata flag on the merged
        // document — exactly like `top-level-division` above — rather
        // than a new call-site parameter. Gates `book-cleanup.lua`'s
        // part handling, `book-numbering.lua`'s counter-reset
        // generation, and (moot once links are already `#<id>`-shaped)
        // `book-links.lua`.
        if doc
            .ast
            .meta
            .get("single-file-book")
            .and_then(|v| v.as_bool())
            == Some(true)
        {
            builder = builder.with_contributor(Box::new(BookSingleFileContributor));
        }

        // book-projects P2b: `Position::Post` user filters are forwarded
        // into `main.lua`'s own entry-point mechanism here instead of
        // running through pampa — `UserFiltersStage::post()` skips them
        // for this (Pandoc-hybrid) leg, since pampa's Lua engine never
        // implemented the Q1-ported pure-Lua helpers
        // (`quarto.utils.file_metadata_filter` etc.) some extension
        // filters rely on. Re-resolves from `doc.ast.meta["filters"]`,
        // which `UserFiltersStage::post()` leaves untouched precisely so
        // this re-resolution sees the same `resolved.post` it would have.
        let document_dir = ctx
            .document
            .input
            .parent()
            .unwrap_or(std::path::Path::new("."));
        let resolved_filters = crate::filter_resolve::resolve_filters(
            &doc.ast.meta,
            document_dir,
            &ctx.extensions,
            ctx.runtime.as_ref(),
        );
        // Whether Q2's citeproc filter resolved this document's citations
        // upstream of pandoc (read before `.post` is consumed below).
        // Margin-citation mode is excluded: there the Typst margin handler
        // owns citation rendering and the `Cite` nodes stay native.
        let citeproc_resolved = !crate::filter_resolve::margin_citations(&doc.ast.meta)
            && resolved_filters
                .pre
                .iter()
                .chain(resolved_filters.post.iter())
                .any(|f| *f == pampa::unified_filter::FilterSpec::Citeproc);
        let entry_points: Vec<EntryPointFilter> = resolved_filters
            .post
            .into_iter()
            .zip(resolved_filters.post_entry_points)
            .filter_map(|(spec, at)| match spec {
                pampa::unified_filter::FilterSpec::Lua(path) => Some(EntryPointFilter {
                    at,
                    path,
                    filter_type: "lua",
                }),
                pampa::unified_filter::FilterSpec::Json(path) => Some(EntryPointFilter {
                    at,
                    path,
                    filter_type: "json",
                }),
                pampa::unified_filter::FilterSpec::Citeproc => {
                    // Q2's own Rust citeproc filter, not a `main.lua`
                    // entry point. `UserFiltersStage::post()` already ran
                    // it on the AST (Q2 never passes `--citeproc` to
                    // pandoc) — nothing to forward.
                    None
                }
            })
            .collect();
        if !entry_points.is_empty() {
            builder = builder.with_contributor(Box::new(QuartoFilterEntryPointsContributor {
                entry_points,
            }));
        }

        // The crossref-index Lua filter (`crossref/index.lua`) writes
        // straight to `crossref-index-file` (`<project>/.quarto/crossref-index.json`,
        // set in `insert_project_keys`) with no directory creation of its
        // own — Lua's `io.open(path, "w")` never creates missing parent
        // directories, and doing the `mkdir -p` there would need a
        // shell-out that isn't portable to Windows. On a project whose
        // `.quarto/` has never been created (a fresh checkout, or a fresh
        // test fixture dir), that `io.open` returns `nil` and the filter
        // only warns "Error attempting to write crossref index" — no path,
        // no errno. Every book chapter's own Pandoc invocation writes to
        // this same file, so ensure the directory exists before any of
        // them run, rather than depending on some other chapter (or a
        // leftover `.quarto/` from a previous run) having created it first.
        if !ctx.project.is_single_file {
            let quarto_dir = ctx.project.dir.join(".quarto");
            std::fs::create_dir_all(&quarto_dir).map_err(|e| {
                PipelineError::stage_error(
                    self.name(),
                    format!(
                        "failed to create {} for the crossref index: {e}",
                        quarto_dir.display()
                    ),
                )
            })?;
        }

        let params_blob = builder.build().to_string();

        // T9.1: the Pandoc-superset shape (`raw: false`), never pampa's
        // native `raw-json` envelope.
        let mut json_buf = Vec::new();
        pampa::writers::json::write_with_config(
            &doc.ast,
            &doc.ast_context,
            &mut json_buf,
            &pampa::writers::json::JsonConfig {
                raw: false,
                ..Default::default()
            },
        )
        .map_err(|diags| {
            PipelineError::stage_error(
                self.name(),
                format!(
                    "failed to serialize AST to Pandoc JSON ({} diagnostics)",
                    diags.len()
                ),
            )
        })?;

        let json_path = temp_dir.join("pandoc-input.json");
        std::fs::write(&json_path, &json_buf).map_err(|e| {
            PipelineError::stage_error(self.name(), format!("failed to write temp JSON: {e}"))
        })?;

        std::fs::create_dir_all(&share).map_err(|e| {
            PipelineError::stage_error(
                self.name(),
                format!("failed to create pandoc share directory: {e}"),
            )
        })?;
        extract_share_tree(&share).map_err(|e| {
            PipelineError::stage_error(
                self.name(),
                format!("failed to materialize vendored pandoc filter tree: {e}"),
            )
        })?;

        // pandoc-hybrid-typst Phase 1: the 8-partial typst doctemplate,
        // typst-only. Staged independent of `share` above (verified: no
        // positional relationship between `--template` and
        // `--data-dir`/`-L` — see `bundle::extract_typst_template`'s doc
        // comment).
        let typst_template_path = if ctx.format.identifier == crate::format::FormatIdentifier::Typst
        {
            let template_dir = temp_dir.join("pandoc-typst-template");
            std::fs::create_dir_all(&template_dir).map_err(|e| {
                PipelineError::stage_error(
                    self.name(),
                    format!("failed to create typst template directory: {e}"),
                )
            })?;
            crate::pandoc_filters::bundle::extract_typst_template(&template_dir).map_err(|e| {
                PipelineError::stage_error(
                    self.name(),
                    format!("failed to materialize vendored typst template: {e}"),
                )
            })?;
            let vendored_template = template_dir.join("template.typ");

            // pandoc-hybrid-typst Phase 1's "Pandoc-defaults forwarding
            // allow-list" bullet, `template` entry: a user-configured
            // `format.typst.template` replaces the vendored template file,
            // mirroring Q1's `userTemplate`
            // (`command/render/pandoc.ts:784-810`). The vendored partials
            // stay staged alongside it unchanged, so a custom template can
            // still reference them (`$numbering.typ()$` etc).
            let doc_dir = doc
                .path
                .parent()
                .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
            if let Some(user_template) = resolve_user_template_path(&doc.ast.meta, &doc_dir) {
                std::fs::copy(&user_template, &vendored_template).map_err(|e| {
                    PipelineError::stage_error(
                        self.name(),
                        format!(
                            "failed to stage user-configured typst template {}: {e}",
                            user_template.display()
                        ),
                    )
                })?;
            }
            stage_typst_template_partials(&doc.ast.meta, &doc_dir, &template_dir)
                .map_err(|msg| PipelineError::stage_error(self.name(), msg))?;
            Some(vendored_template)
        } else {
            None
        };

        // pandoc-hybrid-typst Phase 2: for typst, `ctx.output_path()` is
        // the *final* PDF path (`output_extension` is `"pdf"`) — but this
        // stage only ever produces pandoc's `.typ` source. Writing that
        // text directly to a file named `.pdf` would be a misleading
        // artifact; `TypstCompileStage` (appended after this stage only
        // for typst, see `pipeline::build_pandoc_pipeline_stages`) compiles
        // this intermediate into the real PDF at `ctx.output_path()`.
        let output_path = if ctx.format.identifier == crate::format::FormatIdentifier::Typst {
            ctx.output_path().with_extension("typ")
        } else {
            ctx.output_path()
        };
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                PipelineError::stage_error(
                    self.name(),
                    format!(
                        "failed to create output directory {}: {e}",
                        parent.display()
                    ),
                )
            })?;
        }

        // `init.lua`'s dependenciesFile() only needs the path to exist and
        // be readable/writable, not carry pre-existing content -- but it
        // does need to exist: `QUARTO_FILTER_DEPENDENCY_FILE` names a file
        // for the Lua side to open, not to create.
        let deps_file = temp_dir.join("pandoc-filter-deps.txt");
        std::fs::write(&deps_file, "").map_err(|e| {
            PipelineError::stage_error(
                self.name(),
                format!("failed to create filter dependency file: {e}"),
            )
        })?;
        let mut to_format = ctx.format.pandoc_writer_name();
        if citeproc_resolved && ctx.format.identifier == FormatIdentifier::Typst {
            // Q1's `typstResolveFormat` equivalent: with citations already
            // resolved, turn off the writer's `citations` extension so it
            // prints the resolved `Cite.content` instead of re-deriving
            // native `@key`/`#cite()` syntax, and so the `$citations$`
            // template variable `biblio.typ` gates `#bibliography()` on is
            // unset (no second, redundant bibliography).
            to_format.push_str("-citations");
        }

        let format_extra_args = if ctx.format.identifier == FormatIdentifier::Epub {
            epub_extra_args(&temp_dir, &doc.path, &doc.ast.meta)?
        } else {
            Vec::new()
        };
        // P7 Task 4: the per-format `--default-image-extension` default
        // plus the pandoc-defaults forwarding allow-list
        // (`reference-doc`/`template`/`highlight-style`/`toc`/`toc-depth`/
        // `reference-location`/`shift-heading-level-by`/`slide-level`).
        // `doc.path`'s parent is the document's own directory — the base a
        // `reference-doc`/`template` entry's `FORMAT_PATH_KEYS`-resolved,
        // document-relative `Path` value is rebased against, since this
        // `Command` inherits the process cwd rather than setting its own.
        let doc_dir = doc.path.parent().unwrap_or_else(|| Path::new("."));
        let mut forwarded_args =
            build_forwarded_args(self.name(), doc_dir, &doc.ast.meta, ctx.format.identifier)?;

        if ctx.format.identifier == FormatIdentifier::Typst {
            let args = crate::pandoc_filters::typst_highlight::typst_highlight_args(
                doc_dir,
                &doc.ast.meta,
                typst_brand_mode.as_deref(),
                typst_code_block_bg.as_deref(),
            )
            .map_err(|e| PipelineError::stage_error(self.name(), e.to_string()))?;
            forwarded_args.extend(args);
        }

        // `build_forwarded_args` deliberately skips a bare `--toc-depth` CLI
        // flag for Typst (pandoc's CLI hard-validates it to 1-6, which
        // Typst's own uncapped `#outline(depth: ...)` doesn't need) — but
        // the vendored `typst.lua` filter reads the real value back out of
        // `PANDOC_WRITER_OPTIONS.toc_depth`, which only reflects the
        // document's metadata when threaded through a `--defaults` file
        // (see `build_typst_toc_defaults_yaml`'s doc comment). Write that
        // file here, alongside this stage's other temp-dir artifacts.
        if ctx.format.identifier == FormatIdentifier::Typst
            && let Some(yaml) =
                crate::pandoc_filters::format_defaults::build_typst_toc_defaults_yaml(&doc.ast.meta)
        {
            let defaults_path = temp_dir.join("pandoc-typst-toc-defaults.yaml");
            std::fs::write(&defaults_path, yaml).map_err(|e| {
                PipelineError::stage_error(
                    self.name(),
                    format!("failed to write typst toc-depth defaults file: {e}"),
                )
            })?;
            forwarded_args.push(OsString::from("--defaults"));
            forwarded_args.push(defaults_path.into_os_string());
        }

        // Body-content `Image`/`Link` targets (e.g. `img/thinker.jpg`)
        // reach pandoc as literal, unrebased strings from the AST — unlike
        // the `FORMAT_PATH_KEYS` config keys `build_forwarded_args` already
        // rebases above, nothing upstream of this stage rewrites them for
        // filesystem resolution (the sibling `link-rewrite` B3 transform
        // only rewrites for browser/HTML consumption, and only when a
        // `ResourceResolverContext` is attached). Pandoc's own docx/pptx
        // writers read the referenced file's bytes directly to embed it,
        // resolving a relative target against pandoc's cwd — which this
        // `Command` never sets, so it inherits whatever cwd the host
        // process happens to have. `--resource-path` tells pandoc to also
        // check `doc_dir`, matching every other resolution in this stage.
        // Without it, every docx/pptx render referencing an image by a
        // relative path silently drops the image (measured: `cargo run
        // --bin q2 -- render <fixture with a relative image> --to docx`
        // printed `Warning [Q-11-1]: Could not fetch resource
        // img/thinker.jpg: replacing image with description`).
        //
        // T9.6: `-f json -t <to_format> --data-dir <share>/pandoc/datadir
        // -L <share>/filters/main.lua --resource-path <doc_dir> -o <output>`,
        // plus any format-specific extra flags (pandoc-hybrid-typst Phase 1's
        // invocation builder — typst needs `--standalone --wrap none
        // --default-image-extension svg`; see `Format::pandoc_invocation_args`).
        let output = Command::new(&pandoc_bin)
            .arg("-f")
            .arg("json")
            .arg("-t")
            .arg(&to_format)
            .arg("--data-dir")
            .arg(share.join("pandoc").join("datadir"))
            .arg("-L")
            .arg(share.join("filters").join("main.lua"))
            .args(&format_extra_args)
            .args(ctx.format.pandoc_invocation_args())
            .arg("--resource-path")
            .arg(doc_dir)
            .args(
                shift_heading_level_by
                    .map(|n| vec!["--shift-heading-level-by".to_string(), n.to_string()])
                    .unwrap_or_default(),
            )
            .args(
                typst_template_path
                    .as_ref()
                    .map(|p| vec!["--template".to_string(), p.to_string_lossy().into_owned()])
                    .unwrap_or_default(),
            )
            .arg("-o")
            .arg(&output_path)
            .args(&forwarded_args)
            .arg(&json_path)
            .env("QUARTO_SHARE_PATH", share)
            .env("QUARTO_FILTER_PARAMS", encode_params_blob(&params_blob))
            .env("QUARTO_FILTER_DEPENDENCY_FILE", &deps_file)
            .output()
            .map_err(|e| {
                PipelineError::stage_error(self.name(), format!("failed to execute pandoc: {e}"))
            })?;

        // T10.1/T10.5: stderr is classified unconditionally, regardless of
        // exit status — not only on failure. T10.3: the temp JSON is
        // retained on failure (for debugging) and removed on success.
        let stderr = String::from_utf8_lossy(&output.stderr);
        let status_desc = output.status.to_string();
        let warnings = classify_pandoc_completion(
            self.name(),
            output.status.success(),
            &status_desc,
            &stderr,
            &json_path,
        )?;
        retain_temp_json_unless_success(output.status.success(), &json_path);
        ctx.add_diagnostics(warnings);

        // Q1's shortcode-unescape postprocessor (`format-markdown.ts:21`,
        // wired for every `isMarkdownOutput` flavor at `pandoc.ts:968-973`):
        // pandoc's markdown-family writers re-escape the braces of a
        // literal `{{< … >}}` Str to `{{\< … \>}}`, so an escaped shortcode
        // from the source (`{{{< … >}}}`) would round-trip double-escaped.
        // Rewrite the output file in place. Only runs after a successful
        // pandoc invocation, and only touches files that contain the
        // writer-escaped delimiters.
        if output.status.success() && ctx.format.identifier.is_markdown_output() {
            unescape_shortcodes_in_output(&output_path)?;
        }

        // T9.2: no binary bytes travel through `PipelineData` — pandoc
        // already wrote `output_path` directly. For typst, `output_path`
        // is the intermediate `.typ` file `TypstCompileStage` compiles
        // next — `is_intermediate` mirrors that (see the output_path
        // computation above).
        let is_intermediate = ctx.format.identifier == crate::format::FormatIdentifier::Typst;
        Ok(PipelineData::RenderedOutput(RenderedOutput {
            input_path: doc.path,
            output_path,
            format: ctx.format.clone(),
            content: String::new(),
            is_intermediate,
            supporting_files: vec![],
            metadata: doc.ast.meta,
            source_context: doc.source_context,
        }))
    }
}

/// Whether `blocks` contains a level-1 `Header` anywhere in the final
/// document, recursing into every block container Pandoc's own AST walk
/// would visit — not just top-level blocks. This matters because a
/// heading can arrive from executed code (`results: asis` printing `#
/// Section`), which may land nested inside a wrapper `Div`/`BlockQuote`/
/// list/figure rather than at the top level.
///
/// pandoc-hybrid-typst Phase 1: feeds the `shift-heading-level-by: -1`
/// decision (`format-typst.ts:92-100`) — computed here, immediately
/// before the wire-format cut, over the fully resolved `Block` list, per
/// the document-profile contract's invariant against consuming the
/// earlier-captured `DocumentProfile.outline` for this purpose (see
/// `claude-notes/designs/document-profile-contract.md`).
fn has_level_one_heading(blocks: &[quarto_pandoc_types::Block]) -> bool {
    use quarto_pandoc_types::Block;
    blocks.iter().any(|block| match block {
        Block::Header(h) => h.level == 1,
        Block::BlockQuote(bq) => has_level_one_heading(&bq.content),
        Block::OrderedList(ol) => ol.content.iter().any(|item| has_level_one_heading(item)),
        Block::BulletList(bl) => bl.content.iter().any(|item| has_level_one_heading(item)),
        Block::DefinitionList(dl) => dl
            .content
            .iter()
            .any(|(_, defs)| defs.iter().any(|def| has_level_one_heading(def))),
        Block::Div(d) => has_level_one_heading(&d.content),
        Block::Figure(f) => has_level_one_heading(&f.content),
        Block::Table(t) => {
            t.head
                .rows
                .iter()
                .chain(t.foot.rows.iter())
                .any(|row| row.cells.iter().any(|c| has_level_one_heading(&c.content)))
                || t.bodies.iter().any(|body| {
                    body.body
                        .iter()
                        .any(|row| row.cells.iter().any(|c| has_level_one_heading(&c.content)))
                })
        }
        Block::Custom(c) => c.slots.iter().any(|(_, slot)| match slot {
            quarto_pandoc_types::Slot::Block(b) => has_level_one_heading(std::slice::from_ref(b)),
            quarto_pandoc_types::Slot::Blocks(bs) => has_level_one_heading(bs),
            quarto_pandoc_types::Slot::Inline(_) | quarto_pandoc_types::Slot::Inlines(_) => false,
        }),
        Block::Plain(_)
        | Block::Paragraph(_)
        | Block::LineBlock(_)
        | Block::CodeBlock(_)
        | Block::RawBlock(_)
        | Block::HorizontalRule(_)
        | Block::BlockMetadata(_)
        | Block::NoteDefinitionPara(_)
        | Block::NoteDefinitionFencedBlock(_)
        | Block::CaptionBlock(_) => false,
    })
}

/// `format-typst.ts:92-105`'s decision, re-derived: `Some(-1)` when the
/// document has no level-1 heading anywhere AND the user hasn't set
/// `shift-heading-level-by` explicitly, `None` (no shift) otherwise. Q1
/// checks this key's absence (`flags`/`format.pandoc`) before applying its
/// own default; folded into a single Rust implementation (bd-pandoc-hybrid
/// fold), an explicit `shift-heading-level-by:` must win over this
/// heuristic the same way — otherwise `build_forwarded_args`'s allow-listed
/// forwarding of the same key (P7 Task 4) and this auto value would both
/// emit `--shift-heading-level-by` for the same render.
fn shift_heading_level_by_for(
    blocks: &[quarto_pandoc_types::Block],
    meta: &quarto_pandoc_types::ConfigValue,
) -> Option<i64> {
    if has_level_one_heading(blocks) || meta.get("shift-heading-level-by").is_some() {
        None
    } else {
        Some(-1)
    }
}

/// pandoc-hybrid-typst Phase 1's "Pandoc-defaults forwarding allow-list"
/// bullet, `template` entry: resolves a user-configured `template:` value
/// (already flattened from `format: typst: template: ...` into a plain
/// top-level `template` key by `resolve_format_config`, and marked
/// `ConfigValueKind::Path` — document-relative — by `FORMAT_PATH_KEYS` at
/// merge time) into an absolute filesystem path, joined against the
/// document's own directory. Mirrors Q1's `userTemplate`
/// (`command/render/pandoc.ts:784-810`). Only the `Path` variant is
/// handled: `MarkPolicy::Always` guarantees any string entry is marked, so
/// an unmarked value here means no `template:` key was set at all.
fn resolve_user_template_path(
    meta: &quarto_pandoc_types::ConfigValue,
    doc_dir: &Path,
) -> Option<PathBuf> {
    let value = meta.get("template")?;
    match &value.value {
        quarto_pandoc_types::config_value::ConfigValueKind::Path(s) => Some(doc_dir.join(s)),
        _ => None,
    }
}

/// Typst-leg counterpart of ApplyTemplateStage's HTML `template-partials`
/// handling (`apply_template.rs:179-183`): copies each declared partial over
/// the same-named vendored partial in `template_dir` — the directory
/// Pandoc's `--template` resolves `$partial.typ()$` calls against — so an
/// extension like orange-book that ships a `typst-show.typ` partial without
/// a whole `template:` actually reaches the compiled output (its
/// `#part[...]` RawBlocks otherwise fail with `unknown variable: part`;
/// book-projects P2 item 79). Entries arrive as `ConfigValueKind::Path`
/// (extension contributions, rebased document-relative by
/// `adjust_paths_to_document_dir` at metadata-merge time) or as plain
/// scalars/inlines (document front matter); both resolve against `doc_dir`.
/// Shadowing is by file name, matching pandoc's partial resolution and Q1.
fn stage_typst_template_partials(
    meta: &quarto_pandoc_types::ConfigValue,
    doc_dir: &Path,
    template_dir: &Path,
) -> Result<(), String> {
    let Some(partials) = meta.get("template-partials").and_then(|v| v.as_array()) else {
        return Ok(());
    };
    for partial in partials {
        let rel = match &partial.value {
            quarto_pandoc_types::config_value::ConfigValueKind::Path(s) => s.clone(),
            _ => match partial.as_plain_text() {
                Some(s) => s,
                None => continue,
            },
        };
        let Some(file_name) = Path::new(&rel).file_name() else {
            continue;
        };
        let src = doc_dir.join(&rel);
        std::fs::copy(&src, template_dir.join(file_name)).map_err(|e| {
            format!(
                "failed to stage typst template partial {}: {e}",
                src.display()
            )
        })?;
    }
    Ok(())
}

/// `format-typst.ts:82-90`'s `section-numbering: "1.1.a"`, inserted into
/// the document metadata when `number-sections` is on. A pure
/// metadata-flag check with no AST dependency, so — unlike
/// `shift_heading_level_by_for` — it's safe to compute from any snapshot
/// of the resolved metadata, including right here at the wire-format
/// cut.
fn insert_typst_section_numbering(meta: &mut quarto_pandoc_types::ConfigValue) {
    if meta.get("number-sections").and_then(|v| v.as_bool()) == Some(true) {
        meta.insert_path(
            &["section-numbering"],
            quarto_pandoc_types::ConfigValue::new_string(
                "1.1.a",
                quarto_source_map::SourceInfo::generated(
                    quarto_source_map::By::programmatic_config(),
                ),
            ),
        );
    }
}

/// Q1's `shortcodeUnescapePostprocessor` (`format-markdown.ts:21-22`):
/// replace pandoc's writer-escaped shortcode delimiters `{{\<` / `\>}}`
/// with the literal `{{<` / `>}}` the source's escaped shortcode
/// (`{{{< … >}}}`) is supposed to round-trip to. Skips files containing
/// neither delimiter (the common case — no rewrite, no mtime churn);
/// a read or write failure fails the stage, since leaving the output
/// double-escaped would be silently wrong.
fn unescape_shortcodes_in_output(output_path: &Path) -> Result<(), PipelineError> {
    let content = std::fs::read_to_string(output_path).map_err(|e| {
        PipelineError::stage_error(
            PandocWriteStage.name(),
            format!("failed to read output for shortcode unescaping: {e}"),
        )
    })?;
    if !content.contains("{{\\<") && !content.contains("\\>}}") {
        return Ok(());
    }
    let unescaped = content.replace("{{\\<", "{{<").replace("\\>}}", ">}}");
    std::fs::write(output_path, unescaped).map_err(|e| {
        PipelineError::stage_error(
            PandocWriteStage.name(),
            format!("failed to write unescaped output: {e}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_error_reporting::DiagnosticKind;
    use quarto_pandoc_types::{Block, BlockQuote, ConfigValue, Div, Header};
    use quarto_source_map::SourceInfo;
    use yaml_rust2::Yaml;

    fn header(level: usize) -> Block {
        Block::Header(Header {
            level,
            attr: quarto_pandoc_types::empty_attr(),
            content: vec![],
            source_info: SourceInfo::for_test(),
            attr_source: quarto_pandoc_types::AttrSourceInfo::empty(),
        })
    }

    fn div(content: Vec<Block>) -> Block {
        Block::Div(Div {
            attr: quarto_pandoc_types::empty_attr(),
            content,
            source_info: SourceInfo::for_test(),
            attr_source: quarto_pandoc_types::AttrSourceInfo::empty(),
        })
    }

    /// pandoc-hybrid-typst Phase 1: a top-level level-1 heading means no
    /// shift is needed.
    ///
    /// Revert hunk: flipping `shift_heading_level_by_for`'s branches
    /// makes this RED (would return `Some(-1)` despite the level-1
    /// heading being present).
    #[test]
    fn test_shift_absent_when_top_level_h1_present() {
        let blocks = vec![header(1), header(2)];
        assert_eq!(
            shift_heading_level_by_for(&blocks, &meta_with_number_sections(None)),
            None
        );
    }

    /// No heading at all in the document → shift by -1, same as "no
    /// level-1 heading".
    #[test]
    fn test_shift_applied_when_no_headings_at_all() {
        let blocks = vec![Block::Paragraph(quarto_pandoc_types::Paragraph {
            content: vec![],
            source_info: SourceInfo::for_test(),
        })];
        assert_eq!(
            shift_heading_level_by_for(&blocks, &meta_with_number_sections(None)),
            Some(-1)
        );
    }

    /// Only a level-2 heading at top level → shift by -1.
    ///
    /// Revert hunk: removing the `Block::Header` arm's `level == 1`
    /// check (treating any header as level-1) makes this RED.
    #[test]
    fn test_shift_applied_when_only_h2_present() {
        let blocks = vec![header(2), header(3)];
        assert_eq!(
            shift_heading_level_by_for(&blocks, &meta_with_number_sections(None)),
            Some(-1)
        );
    }

    /// A level-1 heading emitted by executed code can land nested
    /// inside a wrapper `Div` rather than at the top level — the
    /// motivating case from the plan's `results: asis` example. The
    /// scan must still find it.
    ///
    /// Revert hunk: removing the `Block::Div` recursion arm (treating
    /// `Div` as opaque) makes this RED.
    #[test]
    fn test_shift_absent_when_h1_nested_inside_div() {
        let blocks = vec![div(vec![header(1)])];
        assert_eq!(
            shift_heading_level_by_for(&blocks, &meta_with_number_sections(None)),
            None
        );
    }

    /// Nesting two levels deep (`Div` inside `BlockQuote`) must still be
    /// found — confirms the recursion, not just one level of it.
    #[test]
    fn test_shift_absent_when_h1_nested_two_levels_deep() {
        let blocks = vec![Block::BlockQuote(BlockQuote {
            content: vec![div(vec![header(1)])],
            source_info: SourceInfo::for_test(),
        })];
        assert_eq!(
            shift_heading_level_by_for(&blocks, &meta_with_number_sections(None)),
            None
        );
    }

    /// bd-pandoc-hybrid fold (p7 + typst): a document that lacks an H1
    /// would normally get the auto `-1` shift, but an explicit
    /// `shift-heading-level-by:` in front matter must win — matching Q1's
    /// `format-typst.ts:99-105`, which checks the key's absence before
    /// applying its own default. Without the `meta.get(...).is_some()`
    /// guard, this would return `Some(-1)` and `build_forwarded_args`'s
    /// separate, generic forwarding of the user's explicit value would
    /// emit `--shift-heading-level-by` twice for the same pandoc
    /// invocation.
    #[test]
    fn test_shift_absent_when_explicitly_set_even_without_h1() {
        let blocks = vec![header(2)];
        let mut meta = ConfigValue::new_map(vec![], SourceInfo::for_test());
        meta.insert_path(
            &["shift-heading-level-by"],
            ConfigValue::new_scalar(Yaml::Integer(2), SourceInfo::for_test()),
        );
        assert_eq!(shift_heading_level_by_for(&blocks, &meta), None);
    }

    fn meta_with_number_sections(value: Option<bool>) -> ConfigValue {
        let mut meta = ConfigValue::new_map(vec![], SourceInfo::for_test());
        if let Some(v) = value {
            meta.insert_path(
                &["number-sections"],
                ConfigValue::new_bool(v, SourceInfo::for_test()),
            );
        }
        meta
    }

    /// `number-sections: true` gets `section-numbering: "1.1.a"` inserted.
    ///
    /// Revert hunk: emptying `insert_typst_section_numbering`'s body
    /// makes this RED (key stays absent).
    #[test]
    fn test_section_numbering_inserted_when_number_sections_true() {
        let mut meta = meta_with_number_sections(Some(true));
        insert_typst_section_numbering(&mut meta);
        assert_eq!(
            meta.get("section-numbering")
                .and_then(|v| v.as_plain_text()),
            Some("1.1.a".to_string())
        );
    }

    /// `number-sections: false` (or absent) leaves the key out entirely
    /// — both polarities, so this is a discriminator, not a presence
    /// check.
    #[test]
    fn test_section_numbering_absent_when_number_sections_false_or_unset() {
        for value in [Some(false), None] {
            let mut meta = meta_with_number_sections(value);
            insert_typst_section_numbering(&mut meta);
            assert!(
                meta.get("section-numbering").is_none(),
                "expected no section-numbering key for number-sections={value:?}"
            );
        }
    }

    /// T10.1: a zero-exit render whose stderr carries a `[WARNING]` line
    /// surfaces a corresponding diagnostic — the success-case
    /// discriminator this whole task exists for (commit `0b295831e`
    /// changed the policy from stderr-on-nonzero-exit-only to
    /// unconditional; a test asserting only "stderr appears on failure"
    /// would pass under both the old and new behaviour).
    ///
    /// Revert hunk: reverting `classify_pandoc_completion` to
    /// `if !success { classify_pandoc_stderr(stderr) } else { Ok(vec![]) }`
    /// (i.e. only classifying on failure) makes this RED: `success` is
    /// `true` here, so the reverted branch would return `Ok(vec![])`
    /// instead of the one warning diagnostic.
    #[test]
    fn test_warning_on_successful_render_is_surfaced() {
        let result = classify_pandoc_completion(
            "pandoc-write",
            true,
            "exit status: 0 (success)",
            "[WARNING] Could not fetch resource img.png: replacing image with description\n",
            Path::new("/tmp/does-not-matter.json"),
        );

        let diags = result.expect("a successful render must not produce an Err");
        assert_eq!(diags.len(), 1, "expected exactly one diagnostic");
        assert_eq!(diags[0].kind, DiagnosticKind::Warning);
        assert!(
            diags[0].title.contains("Could not fetch resource"),
            "expected the warning line verbatim, got: {}",
            diags[0].title
        );
    }

    /// A failing render surfaces the `Q-20-3` error instead of any
    /// diagnostics — `classify_pandoc_completion`'s other branch.
    #[test]
    fn test_failing_render_surfaces_error_not_diagnostics() {
        let result = classify_pandoc_completion(
            "pandoc-write",
            false,
            "exit status: 83",
            "lua: boom\n",
            Path::new("/tmp/does-not-matter.json"),
        );
        let err = result.expect_err("a failing render must produce an Err");
        assert!(err.to_string().contains("boom"));
    }

    /// T10.3: the temp-JSON retention policy — retained (still exists) on
    /// failure, removed on success.
    ///
    /// Revert hunk: replacing the `if success { remove_file(...) }` guard
    /// in `retain_temp_json_unless_success` with an unconditional
    /// `remove_file` call makes the failure half of this test RED (the
    /// file would be gone after a failed render, when it should be kept
    /// for debugging).
    #[test]
    fn test_temp_json_retained_on_failure_removed_on_success() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");

        let failure_json = dir.path().join("failure.json");
        std::fs::write(&failure_json, b"{}").expect("failed to write fixture JSON");
        retain_temp_json_unless_success(false, &failure_json);
        assert!(
            failure_json.exists(),
            "temp JSON should be retained after a failed render"
        );

        let success_json = dir.path().join("success.json");
        std::fs::write(&success_json, b"{}").expect("failed to write fixture JSON");
        retain_temp_json_unless_success(true, &success_json);
        assert!(
            !success_json.exists(),
            "temp JSON should be removed after a successful render"
        );

        // The `Q-20-3` error itself also names the retained path, so a
        // caller reading only the error text knows where to look.
        let err = classify_pandoc_completion(
            "pandoc-write",
            false,
            "exit status: 83",
            "lua: boom\n",
            &failure_json,
        )
        .expect_err("a failing render must produce an Err");
        assert!(
            err.to_string()
                .contains(&failure_json.display().to_string()),
            "expected the error to name the retained JSON path, got: {err}"
        );
    }

    /// Finding 4 (final review): `version::gate` had a well-tested unit but
    /// no production caller — `Q-20-1`/`Q-20-2` could never actually be
    /// emitted. Mirrors `xtask::verify`'s
    /// `pandoc_preflight_is_wired_into_run` wiring-reachability pattern: a
    /// source-grep that fails if the call site disappears, since a passing
    /// unit test for `gate` alone cannot detect that regression.
    #[test]
    fn test_version_gate_is_wired_into_run() {
        let source = include_str!("pandoc_write.rs");
        assert!(
            source.contains("version::gate(version_str.as_deref())"),
            "`resolve_and_gate_pandoc` no longer calls `version::gate` — the pandoc \
             version floor is untested by any wiring-reachability check if this call \
             is removed"
        );
        assert!(
            source.contains("resolve_and_gate_pandoc(self.name(), ctx.runtime.as_ref())?"),
            "`PandocWriteStage::run` no longer calls `resolve_and_gate_pandoc` — the \
             pandoc binary would be resolved via a bare `Command::new(\"pandoc\")` again, \
             silently ignoring `QUARTO_PANDOC` and the version floor"
        );
    }
}
