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
//! The stage is split in two (pandoc-wasm R1): [`PandocWriteStage::prepare`]
//! builds a [`PandocRequest`] (argv, env, file bytes, dirs: everything one
//! pandoc run needs, as data) and [`PandocWriteStage::execute`] runs it
//! natively with `std::process::Command`, writing the request's files and
//! extracting the vendored filter tree
//! (`crate::pandoc_filters::bundle::extract_share_tree`) under the request's
//! share root. The wasm host runs the same request in a worker. `prepare()`
//! is ungated and reads through the runtime only: the typst template, its
//! partials, the `.theme` highlight file and the toc-depth defaults file all
//! travel as request files or argv text (R4).
//!
//! Unlike [`super::render_html::RenderHtmlBodyStage`], this stage's
//! `RenderedOutput.content` is always empty — no binary bytes travel
//! through `PipelineData`. The pandoc subprocess writes the output file
//! directly at `ctx.output_path()`, and downstream consumers (`FinalOutput`
//! relocation, etc.) key off `output_path`, not `content`.
//!
//! See `claude-notes/plans/2026-09-18-pandoc-hybrid-P4-implementation.md`
//! Task 9 and `claude-notes/plans/2026-10-01-pandoc-request-R1-request-seam.md`.

use std::path::{Path, PathBuf};
#[cfg(not(target_arch = "wasm32"))]
use std::process::Command;

use async_trait::async_trait;
use quarto_error_reporting::DiagnosticMessage;

use crate::format::FormatIdentifier;
use crate::language::LanguageTerms;
#[cfg(not(target_arch = "wasm32"))]
use crate::pandoc_filters::bundle::extract_share_tree;
#[cfg(not(target_arch = "wasm32"))]
use crate::pandoc_filters::diagnostics::classify_pandoc_completion;
use crate::pandoc_filters::format_defaults::build_forwarded_args;
use crate::pandoc_filters::params::{
    BookSingleFileContributor, DocxCalloutIconsContributor, EntryPointFilter, FilterParamsBuilder,
    QuartoFilterEntryPointsContributor,
};
use crate::pandoc_filters::params_codec::encode_params_blob;
#[cfg(not(target_arch = "wasm32"))]
use crate::pandoc_filters::version;
use crate::pandoc_request::args::is_absolute_request_path;
#[cfg(not(target_arch = "wasm32"))]
use crate::pandoc_request::args::to_native_windows_arg;
use crate::pandoc_request::{
    PandocArg, PandocRequest, PrepareOptions, REQUEST_SCHEMA_VERSION, RequestFile, RequestPost,
    ResourceCollector, constants, normalize_request_path, share_tree_version, validate_mounts,
};
#[cfg(not(target_arch = "wasm32"))]
use crate::stage::RenderedOutput;
use crate::stage::{
    DocumentAst, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};

/// Removes the temp JSON input on a successful render; leaves it on disk
/// on failure, where a future debugging session may want to inspect the
/// exact input pandoc choked on.
///
/// A missing/already-removed file on the success path is not an error —
/// `remove_file`'s result is deliberately discarded.
#[cfg(not(target_arch = "wasm32"))]
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
#[cfg(not(target_arch = "wasm32"))]
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
    temp_root: &Path,
    doc_dir: &Path,
    meta: &quarto_pandoc_types::ConfigValue,
    request_path: impl Fn(&Path) -> Result<String, PipelineError>,
) -> Result<(Vec<PandocArg>, Vec<RequestFile>), PipelineError> {
    // The two `--include-in-header` files are the only part of the embedded
    // `formats/` tree pandoc reads; they travel as request `files` at the
    // paths the flags name rather than extracting the whole tree.
    let formats_dest = temp_root.join("pandoc-formats").join("formats");
    let mut files = Vec::new();
    let mut header_arg = |rel: &str| -> Result<PandocArg, PipelineError> {
        let embedded = crate::pandoc_filters::FORMATS_DIR
            .get_file(rel)
            .ok_or_else(|| {
                PipelineError::stage_error(
                    "pandoc-write",
                    format!("vendored format resource {rel} is missing"),
                )
            })?;
        let dest = formats_dest.join(rel);
        files.push(RequestFile {
            path: request_path(&dest)?,
            bytes: embedded.contents().to_vec(),
        });
        Ok(PandocArg::FlagPath {
            flag: "--include-in-header=".to_string(),
            path: dest,
        })
    };
    let mut args = vec![
        PandocArg::text("--default-image-extension=png"),
        PandocArg::text("--math-method=mathml"),
        header_arg("html/styles-callout.html")?,
        header_arg("epub/styles.html")?,
    ];
    if let Some(level) = meta
        .get("epub-chapter-level")
        .and_then(|v| v.as_int_lenient())
    {
        args.push(PandocArg::text(format!("--split-level={level}")));
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
            args.push(PandocArg::FlagPath {
                flag: flag.to_string(),
                path: resolve_doc_relative(doc_dir, &declared),
            });
        }
    }

    // Repeatable path keys: pandoc accepts multiple --epub-embed-font /
    // --css flags, one per file.
    for (key, flag) in [("epub-embed-font", "--epub-embed-font="), ("css", "--css=")] {
        if let Some(value) = meta.get(key) {
            for declared in entry_strings(value) {
                args.push(PandocArg::FlagPath {
                    flag: flag.to_string(),
                    path: resolve_doc_relative(doc_dir, &declared),
                });
            }
        }
    }

    // Not a path — an internal directory *name* inside the epub
    // container, passed through verbatim.
    if let Some(subdir) = meta
        .get("epub-subdirectory")
        .and_then(|v| v.as_plain_text())
    {
        args.push(PandocArg::text(format!("--epub-subdirectory={subdir}")));
    }

    Ok((args, files))
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

/// The brand variant Typst renders with: the dark half when the
/// document's `brand-mode` is `dark` and a dark half exists, otherwise
/// light (the default, matching the `brand-mode` filter param).
pub(crate) fn brand_for_mode<'a>(
    light: Option<&'a quarto_brand::ResolvedBrand>,
    dark: Option<&'a quarto_brand::ResolvedBrand>,
    brand_mode: Option<&str>,
) -> Option<&'a quarto_brand::ResolvedBrand> {
    match brand_mode {
        Some("dark") => dark.or(light),
        _ => light,
    }
}

/// Download the brand's `source: google` fonts into the project's font
/// cache so `typst fonts` and `typst compile` can see them (see
/// [`crate::typst_google_fonts`]). Failures only warn: the font then falls
/// back, as Typst did before this existed.
#[cfg(not(target_arch = "wasm32"))]
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
#[cfg(not(target_arch = "wasm32"))]
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

/// What the native typst pre-step computes and `prepare()` takes as input.
///
/// Brand-font staging (`stage_typst_brand_fonts`, which fetches and writes
/// into the project) and `typst fonts` discovery are effects, not part of
/// `prepare()`: their results feed the params blob, so they run first, on
/// the native path only. In the browser Google and URL brand fonts and the
/// font cache are skipped (design D8.6: the `.typ` names the font, the
/// compile or the reader's machine finds it), and the host supplies
/// `available_fonts` (`PrepareOptions::typst_available_fonts`).
#[derive(Debug, Default, Clone)]
pub struct TypstPrepInputs {
    pub brand: Option<serde_json::Value>,
    pub logo: Option<serde_json::Value>,
    pub brand_mode: Option<String>,
    pub available_fonts: Option<Vec<String>>,
    pub citation_location: Option<String>,
    pub reference_location: Option<String>,
    pub cite_method: Option<String>,
    pub code_block_bg: Option<String>,
    /// Brand font and logo files a PDF compile reads (see
    /// [`PrepareOptions::post`]); not part of the params blob.
    pub asset_files: Vec<PathBuf>,
}

/// What `prepare()` returns: the request plus what the native caller needs
/// to finish the stage.
#[derive(Debug)]
pub struct PreparedPandoc {
    pub request: PandocRequest,
    pub diagnostics: Vec<DiagnosticMessage>,
    /// Where pandoc writes (for typst, the intermediate `.typ`).
    pub output_path: PathBuf,
    pub is_intermediate: bool,
}

/// A request path: UTF-8, absolute (relative paths resolve against the
/// project directory), `/`-normalized.
fn request_path(stage: &str, project_dir: &Path, path: &Path) -> Result<String, PipelineError> {
    let absolute = if path.has_root() || path.is_absolute() {
        path.to_path_buf()
    } else {
        project_dir.join(path)
    };
    if absolute.to_str().is_none() {
        return Err(PipelineError::stage_error(
            stage,
            format!("path is not valid UTF-8: {}", absolute.display()),
        ));
    }
    let normalized = normalize_request_path(&absolute);
    if !is_absolute_request_path(&normalized) {
        return Err(PipelineError::stage_error(
            stage,
            format!("path is not absolute: {}", absolute.display()),
        ));
    }
    Ok(normalized)
}

/// The typst doctemplate as request data: the vendored partials under
/// `<temp_root>/pandoc-typst-template/`, with a user `template:` replacing
/// `template.typ` and each `template-partials:` entry replacing the
/// same-named partial. Returns the path to hand `--template` and the
/// `(path, bytes)` files to write; user files are read through `runtime`
/// (the VFS in the browser), so nothing here touches `std::fs`.
fn typst_template_files(
    name: &str,
    temp_root: &Path,
    doc_dir: &Path,
    meta: &quarto_pandoc_types::ConfigValue,
    runtime: &dyn quarto_system_runtime::SystemRuntime,
) -> Result<(PathBuf, Vec<(PathBuf, Vec<u8>)>), PipelineError> {
    use std::collections::BTreeMap;

    fn collect(dir: &include_dir::Dir<'static>, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in dir.entries() {
            match entry {
                include_dir::DirEntry::Dir(d) => collect(d, out),
                include_dir::DirEntry::File(f) => {
                    out.insert(
                        f.path().to_string_lossy().replace('\\', "/"),
                        f.contents().to_vec(),
                    );
                }
            }
        }
    }

    let template_dir = temp_root.join("pandoc-typst-template");
    let mut entries: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    collect(&crate::pandoc_filters::TYPST_TEMPLATE_DIR, &mut entries);
    let vendored_template = template_dir.join("template.typ");

    // pandoc-hybrid-typst Phase 1's "Pandoc-defaults forwarding
    // allow-list" bullet, `template` entry: a user-configured
    // `format.typst.template` replaces the vendored template file,
    // mirroring Q1's `userTemplate`
    // (`command/render/pandoc.ts:784-810`). The vendored partials
    // stay staged alongside it unchanged, so a custom template can
    // still reference them (`$numbering.typ()$` etc).
    if let Some(user_template) = resolve_user_template_path(meta, doc_dir) {
        let bytes = runtime.file_read(&user_template).map_err(|e| {
            PipelineError::stage_error(
                name,
                format!(
                    "failed to stage user-configured typst template {}: {e}",
                    user_template.display()
                ),
            )
        })?;
        entries.insert("template.typ".to_string(), bytes);
    }
    // Typst-leg counterpart of ApplyTemplateStage's HTML `template-partials`
    // handling (`apply_template.rs:179-183`): each declared partial replaces
    // the same-named vendored partial, the directory Pandoc's `--template`
    // resolves `$partial.typ()$` calls against — so an extension like
    // orange-book that ships a `typst-show.typ` partial without a whole
    // `template:` reaches the output (its `#part[...]` RawBlocks otherwise
    // fail with `unknown variable: part`; book-projects P2 item 79).
    // Entries arrive as `ConfigValueKind::Path` (extension contributions,
    // rebased document-relative at metadata-merge time) or as plain
    // scalars/inlines (front matter); both resolve against `doc_dir`.
    // Shadowing is by file name, matching pandoc's partial resolution and Q1.
    if let Some(partials) = meta.get("template-partials").and_then(|v| v.as_array()) {
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
            let bytes = runtime.file_read(&src).map_err(|e| {
                PipelineError::stage_error(
                    name,
                    format!(
                        "failed to stage typst template partial {}: {e}",
                        src.display()
                    ),
                )
            })?;
            entries.insert(file_name.to_string_lossy().into_owned(), bytes);
        }
    }
    let files = entries
        .into_iter()
        .map(|(rel, bytes)| (template_dir.join(rel), bytes))
        .collect();
    Ok((vendored_template, files))
}

pub struct PandocWriteStage;

impl PandocWriteStage {
    pub fn new() -> Self {
        Self
    }

    /// The typst pre-step: brand, logo, brand-mode and the
    /// citation/reference params. Empty for every other format.
    ///
    /// `native_effects` adds what only the native executor can do: brand
    /// font staging (downloads into the project's font cache) and `typst
    /// fonts` discovery. The request path passes `false` (and the wasm
    /// build has no such code), leaving `available_fonts` to the host.
    pub fn typst_prestep(
        &self,
        doc: &DocumentAst,
        ctx: &mut StageContext,
        native_effects: bool,
    ) -> Result<TypstPrepInputs, PipelineError> {
        if ctx.format.identifier != FormatIdentifier::Typst {
            return Ok(TypstPrepInputs::default());
        }
        let (light, dark) = resolve_typst_brand(&doc.ast.meta, ctx)?;
        let brand_mode_text = doc
            .ast
            .meta
            .get("brand-mode")
            .and_then(|v| v.as_plain_text());
        let active_brand =
            brand_for_mode(light.as_ref(), dark.as_ref(), brand_mode_text.as_deref());
        // The brand's `monospace-block` background replaces the highlight
        // palette's code-block background.
        let code_block_bg = active_brand.and_then(|resolved| {
            let name = resolved
                .brand
                .effective_monospace_block()?
                .background_color?;
            Some(resolved.brand.resolve_color_quiet(&name))
        });
        let (brand, logo, brand_mode) =
            resolve_typst_brand_param(&doc.ast.meta, light.as_ref(), dark.as_ref(), ctx);
        let available_fonts = self.typst_native_font_effects(
            native_effects,
            doc,
            ctx,
            light.as_ref(),
            dark.as_ref(),
            active_brand,
        )?;
        Ok(TypstPrepInputs {
            brand,
            logo,
            brand_mode,
            available_fonts,
            citation_location: doc
                .ast
                .meta
                .get("citation-location")
                .and_then(|value| value.as_plain_text()),
            reference_location: doc
                .ast
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
            cite_method: doc
                .ast
                .meta
                .get("citeproc")
                .and_then(|value| value.as_bool())
                .and_then(|is_citeproc| is_citeproc.then(|| "citeproc".to_string())),
            code_block_bg,
            asset_files: [light.as_ref(), dark.as_ref()]
                .into_iter()
                .flatten()
                .flat_map(|b| {
                    crate::pandoc_filters::typst_brand::brand_asset_files(b, &ctx.project.dir)
                })
                .collect(),
        })
    }

    /// Brand-font staging and `typst fonts` discovery (native only).
    #[cfg(not(target_arch = "wasm32"))]
    fn typst_native_font_effects(
        &self,
        native_effects: bool,
        doc: &DocumentAst,
        ctx: &mut StageContext,
        light: Option<&quarto_brand::ResolvedBrand>,
        dark: Option<&quarto_brand::ResolvedBrand>,
        active_brand: Option<&quarto_brand::ResolvedBrand>,
    ) -> Result<Option<Vec<String>>, PipelineError> {
        if !native_effects {
            return Ok(None);
        }
        // Before `typst fonts` runs: the available-fonts list must include
        // the fonts we are about to make available.
        stage_typst_brand_fonts(light, ctx);
        stage_typst_brand_fonts(dark, ctx);
        resolve_typst_available_fonts(self.name(), &doc.ast.meta, active_brand, &doc.path, ctx)
    }

    /// The wasm build stages no fonts and runs no `typst fonts`: Google
    /// and URL brand fonts and the font cache are skipped (design D8.6),
    /// and the host supplies the available-font list (`render_pandoc_request`).
    #[cfg(target_arch = "wasm32")]
    fn typst_native_font_effects(
        &self,
        _native_effects: bool,
        _doc: &DocumentAst,
        _ctx: &mut StageContext,
        _light: Option<&quarto_brand::ResolvedBrand>,
        _dark: Option<&quarto_brand::ResolvedBrand>,
        _active_brand: Option<&quarto_brand::ResolvedBrand>,
    ) -> Result<Option<Vec<String>>, PipelineError> {
        Ok(None)
    }

    /// Build the [`PandocRequest`] for `doc`: everything one pandoc run
    /// needs, as data. Pure with respect to the filesystem for the formats
    /// the seam covers (docx): reads go through the runtime, nothing is
    /// written. The typst-only sites (template dir, partials, the
    /// highlight-theme read, the toc defaults file) stay direct `std::fs`
    /// calls here until R4 converts them.
    ///
    /// Idempotent on `doc` (the metadata coercions it applies before
    /// serializing are), so a second call on the same document returns the
    /// same request.
    pub fn prepare(
        &self,
        doc: &mut DocumentAst,
        ctx: &StageContext,
        opts: &PrepareOptions,
        typst: &TypstPrepInputs,
    ) -> Result<PreparedPandoc, PipelineError> {
        let name = self.name();
        let project_dir = ctx.project.dir.clone();
        let is_typst = ctx.format.identifier == FormatIdentifier::Typst;
        let rp = |p: &Path| request_path(name, &project_dir, p);

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
        let shift_heading_level_by = if is_typst {
            insert_typst_section_numbering(&mut doc.ast.meta);
            shift_heading_level_by_for(&doc.ast.blocks, &doc.ast.meta)
        } else {
            None
        };

        // `LanguageResolveStage` (earlier in the shared prefix) populates
        // `quarto.language`; the `unwrap_or_else` fallback only matters for
        // a caller that assembles a bare stage list without it.
        let language = LanguageTerms::from_meta(&doc.ast.meta)
            .unwrap_or_else(|| crate::language::resolve_language("en", &[]));

        // T9.5: the serialized JSON lives inside the per-render temp root,
        // not beside the output file or the process cwd. The share tree
        // sits at `<temp_root>/pandoc-share`; the callout icon paths in
        // `QUARTO_FILTER_PARAMS` point into it, so it is derived from the
        // root rather than listed in `files`.
        let temp_root = opts.temp_root.clone();
        let results_file = temp_root.join("pandoc-results.json");
        let share = temp_root.join("pandoc-share");

        let typst_brand_param = typst.brand.clone();
        let typst_logo_param = typst.logo.clone();
        let typst_brand_mode = typst.brand_mode.clone();
        let typst_available_fonts = typst.available_fonts.clone();
        let typst_citation_location = typst.citation_location.clone();
        let typst_reference_location = typst.reference_location.clone();
        let typst_cite_method = typst.cite_method.clone();

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
        let typst_root_dir = if is_typst {
            Some(ctx.project.dir.clone())
        } else {
            None
        };
        let typst_code_line_numbers = if is_typst {
            doc.ast
                .meta
                .get("code-line-numbers")
                .and_then(|v| v.as_bool())
        } else {
            None
        };
        let typst_css_property_processing = if is_typst {
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
                    available_fonts: typst_available_fonts.clone(),
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
        let filter_refs: Vec<(PathBuf, bool)> = resolved_filters
            .post
            .iter()
            .filter_map(|spec| match spec {
                pampa::unified_filter::FilterSpec::Lua(path) => Some((path.clone(), false)),
                pampa::unified_filter::FilterSpec::Json(path) => Some((path.clone(), true)),
                pampa::unified_filter::FilterSpec::Citeproc => None,
            })
            .collect();
        // In a request the filter path must name the mounted file: absolute and
        // normalized (a built-in extension's path can reach us as
        // `<project>/../__quarto_resources__/...`).
        let entry_path = |path: PathBuf| -> Result<PathBuf, PipelineError> {
            if opts.collect_resources {
                Ok(PathBuf::from(rp(&path)?))
            } else {
                Ok(path)
            }
        };
        let entry_points: Vec<EntryPointFilter> = resolved_filters
            .post
            .into_iter()
            .zip(resolved_filters.post_entry_points)
            .map(|(spec, at)| match spec {
                pampa::unified_filter::FilterSpec::Lua(path) => Ok(Some(EntryPointFilter {
                    at,
                    path: entry_path(path)?,
                    filter_type: "lua",
                })),
                pampa::unified_filter::FilterSpec::Json(path) => Ok(Some(EntryPointFilter {
                    at,
                    path: entry_path(path)?,
                    filter_type: "json",
                })),
                pampa::unified_filter::FilterSpec::Citeproc => {
                    // Q2's own Rust citeproc filter, not a `main.lua`
                    // entry point. `UserFiltersStage::post()` already ran
                    // it on the AST (Q2 never passes `--citeproc` to
                    // pandoc) — nothing to forward.
                    Ok(None)
                }
            })
            .collect::<Result<Vec<_>, PipelineError>>()?
            .into_iter()
            .flatten()
            .collect();
        if !entry_points.is_empty() {
            builder = builder.with_contributor(Box::new(QuartoFilterEntryPointsContributor {
                entry_points,
            }));
        }

        let params_blob = builder.build().to_string();

        // T9.1: the Pandoc-superset shape (`raw: false`), never pampa's
        // native `raw-json` envelope.
        //
        // Epub's `css` travels as absolute `--css=` flags (`epub_extra_args`).
        // Left in the metadata too, pandoc would read the document-relative
        // value against its own cwd (`/` in the browser, the q2 process cwd
        // natively) and embed every stylesheet twice, so it is left out of
        // the serialized AST only.
        let epub_css = if ctx.format.identifier == FormatIdentifier::Epub {
            let css = doc.ast.meta.get("css").cloned();
            doc.ast.meta.remove("css");
            css
        } else {
            None
        };
        let mut json_buf = Vec::new();
        let json_result = pampa::writers::json::write_with_config(
            &doc.ast,
            &doc.ast_context,
            &mut json_buf,
            &pampa::writers::json::JsonConfig {
                raw: false,
                ..Default::default()
            },
        );
        if let Some(css) = epub_css {
            doc.ast.meta.insert_path(&["css"], css);
        }
        json_result.map_err(|diags| {
            PipelineError::stage_error(
                name,
                format!(
                    "failed to serialize AST to Pandoc JSON ({} diagnostics)",
                    diags.len()
                ),
            )
        })?;

        let json_path = temp_root.join("pandoc-input.json");
        let mut files = vec![RequestFile {
            path: rp(&json_path)?,
            bytes: json_buf,
        }];

        // pandoc-hybrid-typst Phase 2: for typst, `ctx.output_path()` is
        // the *final* PDF path (`output_extension` is `"pdf"`) — but this
        // stage only ever produces pandoc's `.typ` source. Writing that
        // text directly to a file named `.pdf` would be a misleading
        // artifact; `TypstCompileStage` (appended after this stage only
        // for typst, see `pipeline::build_pandoc_pipeline_stages`) compiles
        // this intermediate into the real PDF at `ctx.output_path()`.
        let output_path = if is_typst {
            ctx.output_path().with_extension("typ")
        } else {
            ctx.output_path()
        };
        let mut dirs: Vec<String> = Vec::new();
        if let Some(parent) = output_path.parent() {
            dirs.push(rp(parent)?);
        }
        // The crossref-index Lua filter (`crossref/index.lua`) writes
        // straight to `crossref-index-file` (`<project>/.quarto/crossref-index.json`,
        // set in `insert_project_keys`) with no directory creation of its
        // own — Lua's `io.open(path, "w")` never creates missing parent
        // directories. On a project whose `.quarto/` has never been created
        // that `io.open` returns `nil` and the filter only warns "Error
        // attempting to write crossref index". Every book chapter's own
        // Pandoc invocation writes to this same file, so the directory must
        // exist before any of them run.
        if !ctx.project.is_single_file {
            dirs.push(rp(&ctx.project.dir.join(".quarto"))?);
        }
        // `pandoc.system.with_temporary_directory` without `/tmp` is an
        // uncatchable fatal error under wasm (design: Contracts).
        if opts.collect_resources {
            dirs.push("/tmp".to_string());
        }

        // `init.lua`'s dependenciesFile() only needs the path to exist and
        // be readable/writable, not carry pre-existing content -- but it
        // does need to exist: `QUARTO_FILTER_DEPENDENCY_FILE` names a file
        // for the Lua side to open, not to create.
        let deps_file = temp_root.join("pandoc-filter-deps.txt");
        files.push(RequestFile {
            path: rp(&deps_file)?,
            bytes: Vec::new(),
        });

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

        // Every user-facing path must be absolute: the wasm working
        // directory is `/`, whereas native's is the q2 process cwd.
        let doc_dir_abs = {
            let dir = doc.path.parent().unwrap_or_else(|| Path::new("."));
            if dir.has_root() || dir.is_absolute() {
                dir.to_path_buf()
            } else {
                project_dir.join(dir)
            }
        };

        let (format_extra_args, extra_files) = if ctx.format.identifier == FormatIdentifier::Epub {
            epub_extra_args(&temp_root, &doc_dir_abs, &doc.ast.meta, |p| rp(p))?
        } else {
            (Vec::new(), Vec::new())
        };
        files.extend(extra_files);

        // pandoc-hybrid-typst Phase 1: the 8-partial typst doctemplate,
        // typst-only, as request files. Independent of `share` above
        // (verified: no positional relationship between `--template` and
        // `--data-dir`/`-L` — see `bundle::extract_typst_template`'s doc
        // comment).
        let typst_template_path = if is_typst {
            let (template, template_files) = typst_template_files(
                name,
                &temp_root,
                &doc_dir_abs,
                &doc.ast.meta,
                ctx.runtime.as_ref(),
            )?;
            for (path, bytes) in template_files {
                files.push(RequestFile {
                    path: rp(&path)?,
                    bytes,
                });
            }
            Some(template)
        } else {
            None
        };

        // P7 Task 4: the per-format `--default-image-extension` default
        // plus the pandoc-defaults forwarding allow-list
        // (`reference-doc`/`template`/`highlight-style`/`toc`/`toc-depth`/
        // `reference-location`/`shift-heading-level-by`/`slide-level`).
        // The document's own directory is the base a
        // `reference-doc`/`template` entry's `FORMAT_PATH_KEYS`-resolved,
        // document-relative `Path` value is rebased against.
        let mut forwarded_args =
            build_forwarded_args(name, &doc_dir_abs, &doc.ast.meta, ctx.format.identifier)?;

        if is_typst {
            let args = crate::pandoc_filters::typst_highlight::typst_highlight_args(
                &doc_dir_abs,
                &doc.ast.meta,
                typst_brand_mode.as_deref(),
                typst.code_block_bg.as_deref(),
                ctx.runtime.as_ref(),
            )
            .map_err(|e| PipelineError::stage_error(name, e.to_string()))?;
            forwarded_args.extend(
                args.into_iter()
                    .map(|a| PandocArg::text(a.to_string_lossy())),
            );
        }

        // `build_forwarded_args` deliberately skips a bare `--toc-depth` CLI
        // flag for Typst (pandoc's CLI hard-validates it to 1-6, which
        // Typst's own uncapped `#outline(depth: ...)` doesn't need) — but
        // the vendored `typst.lua` filter reads the real value back out of
        // `PANDOC_WRITER_OPTIONS.toc_depth`, which only reflects the
        // document's metadata when threaded through a `--defaults` file
        // (see `build_typst_toc_defaults_yaml`'s doc comment). Write that
        // file here, alongside this stage's other temp-root artifacts
        // (a request file).
        if is_typst
            && let Some(yaml) =
                crate::pandoc_filters::format_defaults::build_typst_toc_defaults_yaml(&doc.ast.meta)
        {
            let defaults_path = temp_root.join("pandoc-typst-toc-defaults.yaml");
            files.push(RequestFile {
                path: rp(&defaults_path)?,
                bytes: yaml.into_bytes(),
            });
            forwarded_args.push(PandocArg::text("--defaults"));
            forwarded_args.push(PandocArg::Path(defaults_path));
        }

        // Body-content `Image`/`Link` targets (e.g. `img/thinker.jpg`)
        // reach pandoc as literal, unrebased strings from the AST — unlike
        // the `FORMAT_PATH_KEYS` config keys `build_forwarded_args` already
        // rebases above, nothing upstream of this stage rewrites them for
        // filesystem resolution. Pandoc's own docx/pptx writers read the
        // referenced file's bytes directly to embed it, resolving a
        // relative target against pandoc's cwd. `--resource-path` tells
        // pandoc to also check `doc_dir`, matching every other resolution
        // in this stage. Without it, every docx/pptx render referencing an
        // image by a relative path silently drops the image (`Warning
        // [Q-11-1]: Could not fetch resource img/thinker.jpg: replacing
        // image with description`).
        //
        // T9.6: `-f json -t <to_format> --data-dir <share>/pandoc/datadir
        // -L <share>/filters/main.lua --resource-path <doc_dir> -o <output>`,
        // plus any format-specific extra flags (pandoc-hybrid-typst Phase 1's
        // invocation builder — typst needs `--standalone --wrap none
        // --default-image-extension svg`; see `Format::pandoc_invocation_args`).
        // D3: copy the bytes of the document's own files into the request
        // (wasm only; natively pandoc reads them where they already are).
        let mut diagnostics = Vec::new();
        let resource_refs = if opts.collect_resources {
            let invocation_args: Vec<PandocArg> = ctx
                .format
                .pandoc_invocation_args()
                .into_iter()
                .map(PandocArg::Text)
                .collect();
            let default_ext =
                crate::pandoc_request::resources::default_image_extension(&format_extra_args)
                    .or_else(|| {
                        crate::pandoc_request::resources::default_image_extension(&invocation_args)
                    });
            let base_total: u64 = crate::pandoc_request::share::share_tree_total_bytes()
                + files.iter().map(|f| f.bytes.len() as u64).sum::<u64>();
            let mut collector = ResourceCollector::new(
                ctx.runtime.as_ref(),
                Path::new(&rp(&project_dir)?),
                &temp_root,
                base_total,
            )
            .with_extension_roots(&crate::extension::all_builtin_extension_roots(
                ctx.runtime.as_ref(),
            ));
            // Pandoc's typst writer only names an image in the `.typ`; it never
            // reads the file. So a `.typ` download leaves images and brand
            // assets out (they would count against the size limits for
            // nothing), and the PDF request, whose compile reads them,
            // carries them.
            let compile_reads_files = !is_typst || opts.post == RequestPost::CompileTypst;
            if compile_reads_files {
                let mut image_targets = Vec::new();
                crate::ast_walk::for_each_inline_mut(&mut doc.ast.blocks, &mut |inline| {
                    if let quarto_pandoc_types::Inline::Image(img) = inline {
                        image_targets.push(img.target.0.clone());
                    }
                });
                for target in &image_targets {
                    collector.add_image(&doc_dir_abs, target, default_ext.as_deref());
                }
            }
            if is_typst && opts.post == RequestPost::CompileTypst {
                for path in &typst.asset_files {
                    collector.add_file(path, crate::pandoc_request::ResourceKind::Other);
                }
            }
            collector.add_args(&format_extra_args);
            collector.add_args(&forwarded_args);
            for (path, is_json) in &filter_refs {
                let absolute = if path.has_root() || path.is_absolute() {
                    path.clone()
                } else {
                    project_dir.join(path)
                };
                if *is_json {
                    collector.reject_json_filter(&absolute);
                } else {
                    collector.add_lua_filter(&absolute);
                }
            }
            let (refs, extension_files, notes) = collector.finish();
            diagnostics = notes;
            // A built-in extension's filter directory travels as `files`,
            // not `resource_refs`: the host validator admits `resource_refs`
            // only under the project root.
            files.extend(extension_files);
            refs
        } else {
            Vec::new()
        };

        let mut args: Vec<PandocArg> = vec![
            PandocArg::text("-f"),
            PandocArg::text("json"),
            PandocArg::text("-t"),
            PandocArg::text(to_format.clone()),
            PandocArg::text("--data-dir"),
            PandocArg::Path(share.join("pandoc").join("datadir")),
            PandocArg::text("-L"),
            PandocArg::Path(share.join("filters").join("main.lua")),
        ];
        args.extend(format_extra_args);
        args.extend(
            ctx.format
                .pandoc_invocation_args()
                .into_iter()
                .map(PandocArg::Text),
        );
        args.push(PandocArg::text("--resource-path"));
        args.push(PandocArg::Path(doc_dir_abs.clone()));
        if let Some(n) = shift_heading_level_by {
            args.push(PandocArg::text("--shift-heading-level-by"));
            args.push(PandocArg::text(n.to_string()));
        }
        if let Some(template) = &typst_template_path {
            args.push(PandocArg::text("--template"));
            args.push(PandocArg::Path(template.clone()));
        }
        args.push(PandocArg::text("-o"));
        args.push(PandocArg::Path(
            if output_path.is_absolute() || output_path.has_root() {
                output_path.clone()
            } else {
                project_dir.join(&output_path)
            },
        ));
        args.extend(forwarded_args);
        args.push(PandocArg::Path(json_path.clone()));

        let mut argv = vec!["pandoc".to_string()];
        for arg in &args {
            argv.push(
                arg.to_request_string()
                    .map_err(|msg| PipelineError::stage_error(name, msg))?,
            );
        }

        // The env is an allowlist: never `LANG`/`LC_*` (non-ASCII file
        // names fail under `LANG=C`; leaving it unset is fine).
        let mut env = std::collections::BTreeMap::new();
        env.insert("QUARTO_SHARE_PATH".to_string(), rp(&share)?);
        env.insert(
            "QUARTO_FILTER_PARAMS".to_string(),
            encode_params_blob(&params_blob),
        );
        env.insert("QUARTO_FILTER_DEPENDENCY_FILE".to_string(), rp(&deps_file)?);
        if let Some(epoch) = opts.source_date_epoch {
            env.insert("SOURCE_DATE_EPOCH".to_string(), epoch.to_string());
        }

        let mut request = PandocRequest {
            schema_version: REQUEST_SCHEMA_VERSION,
            kind: Default::default(),
            job_id: String::new(),
            writer: to_format,
            argv,
            env,
            files,
            dirs,
            resource_refs,
            share_root: rp(&temp_root)?,
            share_tree_path: rp(&share)?,
            doc_dir: rp(&doc_dir_abs)?,
            project_root: rp(&project_dir)?,
            output_path: rp(&output_path)?,
            stage_name: name.to_string(),
            json_path: rp(&json_path)?,
            post: opts.post,
            expected_pandoc_wasm_sha256: constants().wasm_sha256.clone(),
            share_tree_version: share_tree_version().to_string(),
            typst_available_fonts,
        };
        validate_mounts(&mut request, opts.collect_resources)
            .map_err(|msg| PipelineError::stage_error(name, msg))?;
        request.job_id = request.compute_job_id();

        // T9.2: no binary bytes travel through `PipelineData` — pandoc
        // writes `output_path` directly. For typst, `output_path` is the
        // intermediate `.typ` file `TypstCompileStage` compiles next.
        Ok(PreparedPandoc {
            request,
            diagnostics,
            output_path,
            is_intermediate: is_typst,
        })
    }

    /// Run a request natively: create `dirs`, write `files`, extract the
    /// share tree at `share_tree_path`, then spawn `pandoc_bin` with the
    /// request's argv (argv[0] replaced by the resolved binary) and env
    /// (applied over the inherited environment). Returns the classified
    /// pandoc warnings.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn execute(
        &self,
        request: &PandocRequest,
        pandoc_bin: &Path,
    ) -> Result<Vec<DiagnosticMessage>, PipelineError> {
        let name = self.name();
        for dir in &request.dirs {
            std::fs::create_dir_all(dir).map_err(|e| {
                PipelineError::stage_error(name, format!("failed to create directory {dir}: {e}"))
            })?;
        }
        for file in &request.files {
            let path = Path::new(&file.path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    PipelineError::stage_error(
                        name,
                        format!("failed to create directory {}: {e}", parent.display()),
                    )
                })?;
            }
            std::fs::write(path, &file.bytes).map_err(|e| {
                PipelineError::stage_error(name, format!("failed to write {}: {e}", file.path))
            })?;
        }

        // Finding 5 (final review): extracted into the per-render temp
        // root rather than a process-global cache, so cleanup rides on
        // `ctx.temp_dir()`'s existing lifecycle instead of leaking.
        let share = Path::new(&request.share_tree_path);
        std::fs::create_dir_all(share).map_err(|e| {
            PipelineError::stage_error(
                name,
                format!("failed to create pandoc share directory: {e}"),
            )
        })?;
        extract_share_tree(share).map_err(|e| {
            PipelineError::stage_error(
                name,
                format!("failed to materialize vendored pandoc filter tree: {e}"),
            )
        })?;

        // Native Windows gets native separators back (see
        // `to_native_windows_arg`); the request itself stays `/`-normalized.
        // The params blob is not touched: its builders emitted native paths.
        let native = |s: &str| {
            if cfg!(windows) {
                to_native_windows_arg(s)
            } else {
                s.to_string()
            }
        };
        let env = request.env.iter().map(|(k, v)| {
            let v = match k.as_str() {
                "QUARTO_SHARE_PATH" | "QUARTO_FILTER_DEPENDENCY_FILE" => native(v),
                _ => v.clone(),
            };
            (k.clone(), v)
        });
        let output = Command::new(pandoc_bin)
            .args(request.argv.iter().skip(1).map(|a| native(a)))
            .envs(env)
            .output()
            .map_err(|e| {
                PipelineError::stage_error(name, format!("failed to execute pandoc: {e}"))
            })?;

        // T10.1/T10.5: stderr is classified unconditionally, regardless of
        // exit status — not only on failure. T10.3: the temp JSON is
        // retained on failure (for debugging) and removed on success.
        let stderr = String::from_utf8_lossy(&output.stderr);
        let status_desc = output.status.to_string();
        let json_path = Path::new(&request.json_path);
        let warnings = classify_pandoc_completion(
            &request.stage_name,
            output.status.success(),
            &status_desc,
            &stderr,
            json_path,
        )?;
        retain_temp_json_unless_success(output.status.success(), json_path);
        Ok(warnings)
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

    #[cfg(not(target_arch = "wasm32"))]
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

        // Findings 3/4 (final review): resolve the binary through the
        // runtime (honouring `QUARTO_PANDOC`) and enforce the version
        // floor before spawning anything.
        let pandoc_bin = resolve_and_gate_pandoc(self.name(), ctx.runtime.as_ref())?;

        let typst = self.typst_prestep(&doc, ctx, true)?;
        let opts = match &ctx.prepare_options {
            Some(opts) => opts.clone(),
            None => PrepareOptions::native(ctx.temp_dir()?.to_path_buf()),
        };
        let prepared = self.prepare(&mut doc, ctx, &opts, &typst)?;
        ctx.add_diagnostics(prepared.diagnostics);

        let warnings = self.execute(&prepared.request, &pandoc_bin)?;
        ctx.add_diagnostics(warnings);

        // Q1's shortcode-unescape postprocessor (`format-markdown.ts:21`,
        // wired for every `isMarkdownOutput` flavor at `pandoc.ts:968-973`):
        // pandoc's markdown-family writers re-escape the braces of a
        // literal `{{< … >}}` Str to `{{\< … \>}}`, so an escaped shortcode
        // from the source (`{{{< … >}}}`) would round-trip double-escaped.
        // Rewrite the output file in place. Only runs after a successful
        // pandoc invocation (`execute` returns `Err` otherwise), and only
        // touches files that contain the writer-escaped delimiters.
        if ctx.format.identifier.is_markdown_output() {
            unescape_shortcodes_in_output(&prepared.output_path)?;
        }

        Ok(PipelineData::RenderedOutput(RenderedOutput {
            input_path: doc.path,
            output_path: prepared.output_path,
            format: ctx.format.clone(),
            content: String::new(),
            is_intermediate: prepared.is_intermediate,
            supporting_files: vec![],
            metadata: doc.ast.meta,
            source_context: doc.source_context,
        }))
    }

    /// There is no process to run on wasm: the request is returned to the
    /// host instead (`PandocPrepareStage`).
    #[cfg(target_arch = "wasm32")]
    async fn run(
        &self,
        _input: PipelineData,
        _ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        Err(PipelineError::other(
            "pandoc-write runs natively only; the wasm pipeline uses pandoc-prepare",
        ))
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
#[cfg(not(target_arch = "wasm32"))]
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
}
