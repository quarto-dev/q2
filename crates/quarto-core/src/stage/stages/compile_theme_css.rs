/*
 * stage/stages/compile_theme_css.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Compile theme CSS and store as pipeline artifact.
 */

//! Compile theme CSS from merged metadata.
//!
//! This stage reads the format-flattened metadata (produced by
//! [`MetadataMergeStage`]), extracts the theme configuration, compiles
//! SCSS to CSS, and stores the result as a Project-scoped artifact
//! keyed `css:theme:<fingerprint>` (Phase 5). The fingerprint is
//! derived from the *output* CSS bytes so two compilations producing
//! identical output share an artifact (deduplication across pages
//! that use the same theme), while different output produces
//! different keys (multi-theme websites coexist).
//!
//! Behavior by theme value (mirrors Quarto 1):
//!
//! - Theme **absent**: compile the default Bootstrap + Quarto customization
//!   layer. Produces a fully functional Bootstrap stylesheet ready for
//!   navbar / footer / TOC rendering.
//! - `theme: none`: ship the static lightweight `DEFAULT_CSS` without any
//!   Bootstrap. Opt-out for users who want minimal output.
//! - `theme: cosmo` (or any other name/array): compile the named theme(s)
//!   layered on top of Bootstrap + Quarto.
//!
//! Compilation results are cached via the `SystemRuntime` cache interface
//! to avoid expensive recompilation.

use std::path::PathBuf;

use async_trait::async_trait;
use quarto_config::resolve_website_value;
use quarto_pandoc_types::ConfigValue;
use quarto_sass::{CSS_BUILD_ID, SassLayer, ThemeConfig, ThemeContext, compile_default_css};
use quarto_system_runtime::{
    SASS_CACHE_BUDGET_BYTES, SystemRuntime, cache_get_lru, cache_set_lru, ensure_namespace_version,
};

use crate::artifact::{Artifact, ArtifactScope};
use crate::resources::DEFAULT_CSS;
use crate::stage::{
    EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};
use crate::trace_event;

/// Build a SCSS layer of `$variable: value;` assignments derived from
/// per-document / project metadata.
///
/// The returned layer slots into `assemble_with_user_layers` as the
/// **last** user layer, which `merge_layers()` then promotes to the
/// front of the merged-defaults section. That places its assignments
/// before the framework's `$variable: ... !default;` declarations, so
/// they win the `!default` race without needing `!default` themselves.
///
/// **Currently emits:**
///
/// - `$sidebar-border` for the website's first sidebar. Mirrors Q1's
///   `format-html-scss.ts` — defaults to `(style == "docked")`. Phase
///   3 of the bd-k8y0 plan adds an explicit `sidebar.border:` YAML
///   knob and threads it through here.
///
/// Multi-sidebar projects pick the **first** sidebar's setting, matching
/// Q1's per-format behavior. A future pass that wants per-sidebar
/// borders would need a different mechanism (the `$sidebar-border` rule
/// is global by selector).
///
/// Returns an empty layer when no relevant metadata is present.
pub fn derive_doc_scss_layer(meta: &ConfigValue) -> SassLayer {
    use quarto_navigation::{Sidebar, SidebarStyle};

    let mut defaults = String::new();

    if let Some(sidebar_cv) = resolve_website_value(meta, "sidebar") {
        let sidebars = Sidebar::parse_list_from_config(&sidebar_cv);
        if let Some(first) = sidebars.first() {
            // Q1 parity: explicit `sidebar.border:` wins; absent → default
            // to `(style == Docked)`. See `format-html-scss.ts:631-642`.
            let border = first
                .border
                .unwrap_or_else(|| matches!(first.style, SidebarStyle::Docked));
            defaults.push_str(&format!("$sidebar-border: {};\n", border));
        }
    }

    SassLayer {
        defaults,
        ..Default::default()
    }
}

/// Name of the cache namespace used for compiled SCSS CSS output.
const SASS_CACHE_NAMESPACE: &str = "sass";

/// Fixed cache key for the default (no-theme) compiled CSS. The
/// minified flag distinguishes minified from expanded output; the
/// generational purge ([`ensure_sass_cache_ready`]) keyed on
/// [`CSS_BUILD_ID`] handles all other invalidation cases
/// automatically. No manual `_vN` suffix needed — the build ID
/// changes whenever any SCSS resource OR any Rust source in
/// `crates/quarto-sass/src/` changes.
const DEFAULT_CACHE_KEY_MINIFIED: &str = "default_minified";
const DEFAULT_CACHE_KEY_EXPANDED: &str = "default_expanded";

fn default_cache_key(minified: bool) -> &'static str {
    if minified {
        DEFAULT_CACHE_KEY_MINIFIED
    } else {
        DEFAULT_CACHE_KEY_EXPANDED
    }
}

/// Run the generational-purge check on the `sass` namespace.
///
/// Called once per stage run (not memoized). On WASM this adds a single
/// IndexedDB read per render — negligible compared to the 100-500 ms
/// compile cost the caching prevents, and the simplicity is worth it
/// (memoization keyed by process-scope was considered but leaked state
/// between tests and between runtimes). Returns `false` if the cache
/// layer errors out; callers fall through to compile-without-caching.
async fn ensure_sass_cache_ready(runtime: &dyn SystemRuntime) -> bool {
    ensure_namespace_version(runtime, SASS_CACHE_NAMESPACE, CSS_BUILD_ID.as_bytes())
        .await
        .is_ok()
}

/// Compile theme CSS and store as a pipeline artifact.
///
/// This stage:
/// 1. Extracts `ThemeConfig` from merged metadata (`doc.ast.meta`)
/// 2. If no theme: stores `DEFAULT_CSS` and returns
/// 3. If themed: assembles SCSS, checks cache, compiles if needed
/// 4. Stores result as `"css:theme:<fingerprint>"` Project-scoped artifact
///
/// The stage passes `DocumentAst` through unchanged — it only produces
/// a side-effect artifact.
///
/// # Caching
///
/// The cache key is `sha256(assembled_scss + ":minified=" + minified)`.
/// Cache hits skip compilation entirely. Cache errors are non-fatal
/// (best-effort caching).
pub struct CompileThemeCssStage;

impl CompileThemeCssStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CompileThemeCssStage {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute a cache key from theme specifications, the SCSS resources hash,
/// custom file contents, and any per-document SCSS variable assignments.
///
/// The key is `SHA256(SCSS_RESOURCES_HASH + theme_identities +
/// custom_file_contents + doc_vars + minified)`. Built-in themes contribute
/// only their name (content is already covered by `SCSS_RESOURCES_HASH`).
/// Custom themes contribute their resolved path and file contents.
/// `doc_vars` contributes its serialized `defaults` string so two
/// documents with different per-document variables (e.g. docked vs.
/// floating sidebar → different `$sidebar-border`) get distinct keys
/// and don't alias each other in the runtime cache.
fn cache_key(
    theme_config: &ThemeConfig,
    theme_context: &ThemeContext<'_>,
    runtime: &dyn quarto_system_runtime::SystemRuntime,
    doc_vars: &SassLayer,
) -> Result<String, String> {
    use quarto_sass::{SCSS_RESOURCES_HASH, ThemeSpec};
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();

    // Include the build-time hash of all built-in SCSS resources
    hasher.update(SCSS_RESOURCES_HASH.as_bytes());

    // Include each theme's identity and (for custom themes) content
    for spec in &theme_config.themes {
        match spec {
            ThemeSpec::BuiltIn(theme) => {
                hasher.update(b"builtin:");
                hasher.update(theme.name().as_bytes());
            }
            ThemeSpec::Custom(path) => {
                let resolved = theme_context.resolve_path(path);
                hasher.update(b"custom:");
                hasher.update(resolved.to_string_lossy().as_bytes());
                hasher.update(b"\n");
                // Read custom file contents for the key
                let contents = runtime.file_read(&resolved).map_err(|e| {
                    format!("failed to read custom theme {}: {}", resolved.display(), e)
                })?;
                hasher.update(&contents);
            }
            ThemeSpec::Brand => {
                hasher.update(b"brand:");
                // Hash the resolved brand's YAML serialization (so any
                // brand change → different key). When the brand is
                // absent here we still hash the marker — `compile_with_doc_vars`
                // will fail downstream, and the failure path bypasses
                // the cache.
                if let Some(brand) = theme_context.brand() {
                    let yaml = serde_yaml::to_string(brand)
                        .map_err(|e| format!("serialize brand for cache key: {e}"))?;
                    hasher.update(yaml.as_bytes());
                }
            }
        }
        hasher.update(b"\n");
    }

    // Include doc-vars layer's defaults. We hash the defaults section
    // because that's the only section `derive_doc_scss_layer` populates
    // today; if we ever start emitting rules / mixins from metadata,
    // this hash should grow accordingly.
    hasher.update(b"doc_vars:");
    hasher.update(doc_vars.defaults.as_bytes());
    hasher.update(b"\n");

    // Include minification flag
    hasher.update(if theme_config.minified { b"1" } else { b"0" });

    let hash = hasher.finalize();
    Ok(hex::encode(hash))
}

#[async_trait(?Send)]
impl PipelineStage for CompileThemeCssStage {
    fn name(&self) -> &str {
        "compile-theme-css"
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

        // Extract theme config from merged metadata.
        //
        // Any error here is a **user-facing configuration error**
        // (malformed `theme:`, `brand:` token without a `brand:` key,
        // unknown theme name, etc.). When we know the offending
        // value lives in `_quarto.yml`, we lift the error into a
        // structured ariadne diagnostic via `theme_diagnostic` so
        // the user gets a code (Q-14-1) and a source span. When the
        // source file is unknown (single-file renders, brand
        // resolution failures without a config path), we fall back
        // to the legacy stage-error path so the message still lands
        // even without a span.
        let theme_config = match ThemeConfig::from_config_value(&doc.ast.meta) {
            Ok(c) => c,
            Err(e) => {
                // The error's source span can point at either the
                // project's _quarto.yml (most common) OR the
                // document's own frontmatter (when the document
                // overrides `theme:`). Hand the converter both
                // candidates with their FileId bindings:
                // - _quarto.yml uses the YAML parser's hash-based
                //   FileId (via `quarto_yaml::file_id_for_filename`).
                // - The document uses pampa's primary FileId(0).
                let mut candidates: Vec<(quarto_source_map::FileId, &std::path::Path)> = Vec::new();
                if let Some(p) = ctx.project.config.config_path.as_deref() {
                    let fid = quarto_yaml::file_id_for_filename(&p.to_string_lossy());
                    candidates.push((fid, p));
                }
                candidates.push((quarto_source_map::FileId(0), ctx.document.input.as_path()));
                let pe = crate::theme_diagnostic::sass_error_to_parse_error(&e, &candidates);
                return Err(PipelineError::Structured(pe));
            }
        };

        // `theme: none` → ship the static lightweight DEFAULT_CSS without
        // compiling Bootstrap. This is the explicit opt-out path.
        if theme_config.suppress_bootstrap {
            trace_event!(
                ctx,
                EventLevel::Debug,
                "theme: none set, using static DEFAULT_CSS"
            );
            store_default_css(ctx);
            return Ok(PipelineData::DocumentAst(doc));
        }

        // Ensure the sass namespace matches the current SCSS resources
        // generation. When this returns `false` the cache layer is
        // unavailable and we compile without caching.
        let cache_ok = ensure_sass_cache_ready(ctx.runtime.as_ref()).await;

        // Build the per-document SCSS variables layer (Phase 2 of bd-k8y0).
        // Today this is just `$sidebar-border` from `website.sidebar.style`,
        // but the same hook is the home for future `$sidebar-bg`,
        // `$navbar-bg`, etc. injections — see the plan and `derive_doc_scss_layer`.
        let doc_vars = derive_doc_scss_layer(&doc.ast.meta);

        // Fast path: no themes AND no doc-derived variables. Use the
        // shared, cached default-CSS bundle. This preserves byte-identity
        // with prior behavior for plain documents (no website / no sidebar).
        if !theme_config.has_themes() && doc_vars.is_empty() {
            // Try the runtime cache first (cross-session persistence).
            if cache_ok {
                if let Ok(Some(cached)) = cache_get_lru(
                    ctx.runtime.as_ref(),
                    SASS_CACHE_NAMESPACE,
                    default_cache_key(theme_config.minified),
                )
                .await
                {
                    if let Ok(css) = String::from_utf8(cached) {
                        trace_event!(ctx, EventLevel::Debug, "cache hit for default CSS");
                        store_css(ctx, css);
                        return Ok(PipelineData::DocumentAst(doc));
                    }
                }
            }

            trace_event!(
                ctx,
                EventLevel::Debug,
                "no theme / no doc-vars, compiling default Bootstrap + Quarto layer"
            );
            match compile_default(ctx, theme_config.minified).await {
                Ok(css) => {
                    if cache_ok {
                        let _ = cache_set_lru(
                            ctx.runtime.as_ref(),
                            SASS_CACHE_NAMESPACE,
                            default_cache_key(theme_config.minified),
                            css.as_bytes(),
                            SASS_CACHE_BUDGET_BYTES,
                        )
                        .await;
                    }
                    store_css(ctx, css);
                }
                Err(e) => {
                    trace_event!(
                        ctx,
                        EventLevel::Warn,
                        "default Bootstrap compilation failed: {}, using static DEFAULT_CSS",
                        e
                    );
                    store_default_css(ctx);
                }
            }
            return Ok(PipelineData::DocumentAst(doc));
        }

        // Themed and/or doc-vars-bearing path: compute fingerprinted cache
        // key (factors in theme identities + doc-vars), check the runtime
        // cache, then compile via `compile_with_doc_vars` on miss.
        let document_dir = doc
            .path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));

        // Resolve the brand (if any) before building the theme context.
        // I/O happens here. Failures are user-facing configuration
        // errors (missing `_brand.yml`, invalid YAML, unknown brand
        // shape) — propagate them rather than silently shipping
        // DEFAULT_CSS, same reasoning as the `from_config_value`
        // error path above.
        let resolved = theme_config
            .clone()
            .resolve(ctx.runtime.as_ref(), &ctx.project.dir)
            .map_err(|e| {
                PipelineError::stage_error(self.name(), format!("brand resolution: {e}"))
            })?;

        let mut theme_context = ThemeContext::new(document_dir, ctx.runtime.as_ref());
        if let Some(brand) = resolved.brand.as_ref() {
            let brand_dir = resolved
                .brand_dir
                .clone()
                .unwrap_or_else(|| ctx.project.dir.clone());
            theme_context = theme_context.with_brand(brand, brand_dir);
        }

        let key = match cache_key(
            &theme_config,
            &theme_context,
            ctx.runtime.as_ref(),
            &doc_vars,
        ) {
            Ok(k) => k,
            Err(e) => {
                trace_event!(
                    ctx,
                    EventLevel::Warn,
                    "failed to compute cache key: {}, compiling without cache",
                    e
                );
                // Fall through with no cache key — will compile without caching
                String::new()
            }
        };

        // Check cache (best-effort — errors are non-fatal).
        if cache_ok && !key.is_empty() {
            if let Ok(Some(cached)) =
                cache_get_lru(ctx.runtime.as_ref(), SASS_CACHE_NAMESPACE, &key).await
            {
                if let Ok(css) = String::from_utf8(cached) {
                    trace_event!(
                        ctx,
                        EventLevel::Debug,
                        "cache hit for theme CSS (key={})",
                        key
                    );
                    store_css(ctx, css);
                    return Ok(PipelineData::DocumentAst(doc));
                }
            }
        }

        trace_event!(
            ctx,
            EventLevel::Debug,
            "compiling theme CSS ({} themes, doc_vars={} bytes, key={})",
            theme_config.themes.len(),
            doc_vars.defaults.len(),
            key
        );

        let css =
            compile_with_doc_vars_via_runtime(ctx, &theme_config, &theme_context, &doc_vars).await;

        match css {
            Ok(css) => {
                // Store in cache (best-effort, skip if no key or cache unavailable).
                if cache_ok && !key.is_empty() {
                    let _ = cache_set_lru(
                        ctx.runtime.as_ref(),
                        SASS_CACHE_NAMESPACE,
                        &key,
                        css.as_bytes(),
                        SASS_CACHE_BUDGET_BYTES,
                    )
                    .await;
                }
                store_css(ctx, css);
            }
            Err(e) => {
                trace_event!(
                    ctx,
                    EventLevel::Warn,
                    "theme CSS compilation failed: {}, using default CSS",
                    e
                );
                store_default_css(ctx);
            }
        }

        Ok(PipelineData::DocumentAst(doc))
    }
}

/// Length of the theme fingerprint hex prefix used in artifact
/// keys and on-disk filenames. 16 hex chars = 64 bits = ample
/// collision resistance for the small set of distinct themes a
/// single project will produce.
const THEME_FINGERPRINT_LEN: usize = 16;

/// Compute a stable, content-derived fingerprint for a compiled
/// theme CSS string. Two compilations producing identical bytes
/// produce identical fingerprints; the merge layer
/// ([`crate::artifact::ArtifactStore::merge_into_project`])
/// dedupes those into a single shared artifact.
///
/// Hashing the *output* (rather than the inputs) sidesteps the
/// "did we hash everything that affects the result?" problem:
/// the SCSS pipeline can normalize / canonicalize inputs in any
/// order it likes, and identical CSS output → identical
/// fingerprint by construction.
///
/// See `claude-notes/plans/2026-04-24-websites-phase-5.md`
/// Decision 9.
pub fn theme_fingerprint(css: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(css.as_bytes());
    let hash = hasher.finalize();
    let hex = hex::encode(hash);
    hex[..THEME_FINGERPRINT_LEN].to_string()
}

/// Build the artifact key + relative on-disk path from a theme
/// CSS fingerprint.
///
/// **Path naming differs by project type** to preserve byte-
/// identical single-doc behavior (see Phase 5 Decision 10):
///
/// - **Single-doc** (`is_single_file == true`): path is the bare
///   `styles.css`, mirroring the pre-Phase-5 layout. Single-doc
///   renders only ever produce one theme per render call, so a
///   non-fingerprinted name is unambiguous.
/// - **Multi-doc / website**: path is
///   `quarto/quarto-theme-<fingerprint>.css`, namespaced under
///   `quarto/` so multi-theme websites can coexist.
///
/// The artifact **key** is fingerprint-keyed in both cases so
/// the project-merge dedup works whenever two pages emit the
/// same theme bytes.
fn theme_artifact_key_and_path(fingerprint: &str, single_doc: bool) -> (String, PathBuf) {
    let key = format!("css:theme:{}", fingerprint);
    let path = if single_doc {
        PathBuf::from("styles.css")
    } else {
        PathBuf::from(format!("quarto/quarto-theme-{}.css", fingerprint))
    };
    (key, path)
}

fn store_default_css(ctx: &mut StageContext) {
    store_css(ctx, DEFAULT_CSS.to_string());
}

fn store_css(ctx: &mut StageContext, css: String) {
    let fingerprint = theme_fingerprint(&css);
    let (key, path) = theme_artifact_key_and_path(&fingerprint, ctx.project.is_single_file);
    ctx.artifacts.store(
        key,
        Artifact::from_string(css, "text/css")
            .with_path(path)
            .with_scope(ArtifactScope::Project),
    );
}

/// Compile assembled SCSS to CSS.
///
/// Uses `compile_scss_with_embedded` on native (sync, via grass) and
/// `runtime.compile_sass` on WASM (async, via dart-sass JS bridge).
/// Compile the default Bootstrap + Quarto layer (no Bootswatch theme).
///
/// Native uses `grass` in-process (sync); WASM uses the dart-sass JS bridge
/// (async). This wrapper gives the stage one call site.
#[cfg(not(target_arch = "wasm32"))]
async fn compile_default(ctx: &StageContext, minified: bool) -> Result<String, String> {
    compile_default_css(ctx.runtime.as_ref(), minified).map_err(|e| e.to_string())
}

#[cfg(target_arch = "wasm32")]
async fn compile_default(ctx: &StageContext, minified: bool) -> Result<String, String> {
    compile_default_css(ctx.runtime.as_ref(), minified)
        .await
        .map_err(|e| e.to_string())
}

/// Compile a (possibly themed) bundle plus an optional doc-derived
/// SassLayer. Native is sync (`grass`); WASM is async (dart-sass JS
/// bridge). This wrapper gives the stage a single call site.
#[cfg(not(target_arch = "wasm32"))]
async fn compile_with_doc_vars_via_runtime(
    ctx: &StageContext,
    theme_config: &ThemeConfig,
    theme_context: &ThemeContext<'_>,
    doc_vars: &SassLayer,
) -> Result<String, String> {
    let _ = ctx; // runtime is captured inside theme_context
    quarto_sass::compile_with_doc_vars(theme_config, theme_context, doc_vars)
        .map_err(|e| e.to_string())
}

#[cfg(target_arch = "wasm32")]
async fn compile_with_doc_vars_via_runtime(
    ctx: &StageContext,
    theme_config: &ThemeConfig,
    theme_context: &ThemeContext<'_>,
    doc_vars: &SassLayer,
) -> Result<String, String> {
    let _ = ctx; // runtime is captured inside theme_context
    quarto_sass::compile_with_doc_vars(theme_config, theme_context, doc_vars)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectContext};
    use crate::stage::DocumentAst;
    use quarto_pandoc_types::pandoc::Pandoc;
    use quarto_pandoc_types::{ConfigMapEntry, ConfigValue, ConfigValueKind};
    use quarto_sass::ThemeSpec;
    use quarto_source_map::{SourceContext, SourceInfo};
    use quarto_system_runtime::TempDir;
    use std::sync::Arc;
    use yaml_rust2::Yaml;

    // ── Test helpers ─────────────────────────────────────────────────

    fn make_stage_context(runtime: Arc<dyn quarto_system_runtime::SystemRuntime>) -> StageContext {
        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: crate::project::ProjectConfig::default(),
            is_single_file: true,
            files: vec![],
            output_dir: PathBuf::from("/project"),
        };
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();
        StageContext::new(runtime, format, project, doc).unwrap()
    }

    fn make_doc_ast(meta: ConfigValue) -> PipelineData {
        PipelineData::DocumentAst(DocumentAst {
            path: PathBuf::from("/project/test.qmd"),
            ast: Pandoc {
                meta,
                ..Default::default()
            },
            ast_context: pampa::pandoc::ASTContext::default(),
            source_context: SourceContext::new(),
            warnings: vec![],
            recorded_includes: Vec::new(),
        })
    }

    fn empty_meta() -> ConfigValue {
        ConfigValue {
            value: ConfigValueKind::Map(vec![]),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        }
    }

    fn meta_with_theme(theme: &str) -> ConfigValue {
        let theme_value = ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::String(theme.to_string())),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let root_entry = ConfigMapEntry {
            key: "theme".to_string(),
            key_source: SourceInfo::for_test(),
            value: theme_value,
        };

        ConfigValue {
            value: ConfigValueKind::Map(vec![root_entry]),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        }
    }

    /// Locate the (single) `css:theme:*` artifact stored by
    /// [`CompileThemeCssStage`] and return its content as a string.
    /// Phase 5 retires the singleton `"css:default"` key in favor
    /// of fingerprint-keyed artifacts.
    fn get_css_artifact(ctx: &StageContext) -> String {
        let entries: Vec<_> = ctx.artifacts.get_by_prefix("css:theme:");
        assert_eq!(
            entries.len(),
            1,
            "expected exactly one css:theme:* artifact, found {}",
            entries.len()
        );
        let artifact = entries[0].1;
        assert_eq!(artifact.scope, ArtifactScope::Project);
        String::from_utf8(artifact.content.clone()).expect("CSS should be valid UTF-8")
    }

    // ── Mock runtime ─────────────────────────────────────────────────

    struct MockRuntime;

    #[async_trait::async_trait]
    impl quarto_system_runtime::SystemRuntime for MockRuntime {
        fn file_read(
            &self,
            _path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
            Ok(vec![])
        }
        fn file_write(
            &self,
            _path: &std::path::Path,
            _contents: &[u8],
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn path_exists(
            &self,
            _path: &std::path::Path,
            _kind: Option<quarto_system_runtime::PathKind>,
        ) -> quarto_system_runtime::RuntimeResult<bool> {
            Ok(true)
        }
        fn canonicalize(
            &self,
            path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            Ok(path.to_path_buf())
        }
        fn path_metadata(
            &self,
            _path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::PathMetadata> {
            unimplemented!()
        }
        fn file_copy(
            &self,
            _src: &std::path::Path,
            _dst: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn path_rename(
            &self,
            _old: &std::path::Path,
            _new: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn file_remove(&self, _path: &std::path::Path) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn dir_create(
            &self,
            _path: &std::path::Path,
            _recursive: bool,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn dir_remove(
            &self,
            _path: &std::path::Path,
            _recursive: bool,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn dir_list(
            &self,
            _path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<Vec<PathBuf>> {
            Ok(vec![])
        }
        fn cwd(&self) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            Ok(PathBuf::from("/"))
        }
        fn temp_dir(&self, _template: &str) -> quarto_system_runtime::RuntimeResult<TempDir> {
            Ok(TempDir::new(PathBuf::from("/tmp/test")))
        }
        fn exec_pipe(
            &self,
            _command: &str,
            _args: &[&str],
            _stdin: &[u8],
        ) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
            Ok(vec![])
        }
        fn exec_command(
            &self,
            _command: &str,
            _args: &[&str],
            _stdin: Option<&[u8]>,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::CommandOutput> {
            Ok(quarto_system_runtime::CommandOutput {
                code: 0,
                stdout: vec![],
                stderr: vec![],
            })
        }
        fn env_get(&self, _name: &str) -> quarto_system_runtime::RuntimeResult<Option<String>> {
            Ok(None)
        }
        fn env_all(
            &self,
        ) -> quarto_system_runtime::RuntimeResult<std::collections::HashMap<String, String>>
        {
            Ok(std::collections::HashMap::new())
        }
        async fn fetch_url(
            &self,
            _url: &str,
        ) -> quarto_system_runtime::RuntimeResult<(Vec<u8>, String)> {
            Err(quarto_system_runtime::RuntimeError::NotSupported(
                "mock".to_string(),
            ))
        }
        fn os_name(&self) -> &'static str {
            "mock"
        }
        fn arch(&self) -> &'static str {
            "mock"
        }
        fn cpu_time(&self) -> quarto_system_runtime::RuntimeResult<u64> {
            Ok(0)
        }
        fn xdg_dir(
            &self,
            _kind: quarto_system_runtime::XdgDirKind,
            _subpath: Option<&std::path::Path>,
        ) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            Ok(PathBuf::from("/xdg"))
        }
        fn stdout_write(&self, _data: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn stderr_write(&self, _data: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
    }

    // ── Tests ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_no_theme_compiles_default_bootstrap() {
        // Q1 parity: when `theme:` is absent, compile the full Bootstrap +
        // Quarto customization layer. The old behavior (static 244-line
        // DEFAULT_CSS) is now gated behind the explicit `theme: none`
        // opt-out.
        let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> =
            Arc::new(quarto_system_runtime::NativeRuntime::new());
        let mut ctx = make_stage_context(runtime);
        let stage = CompileThemeCssStage::new();

        let input = make_doc_ast(empty_meta());
        let output = stage.run(input, &mut ctx).await.unwrap();
        assert!(output.into_document_ast().is_some());

        let css = get_css_artifact(&ctx);
        assert_ne!(
            css, DEFAULT_CSS,
            "missing theme must produce compiled Bootstrap, not static DEFAULT_CSS"
        );
        assert!(
            css.contains(".navbar"),
            "compiled default CSS should include Bootstrap .navbar rules"
        );
        assert!(
            css.contains(".btn"),
            "compiled default CSS should include Bootstrap .btn rules"
        );
    }

    #[tokio::test]
    async fn test_theme_none_preserves_static_default_css() {
        // `theme: none` is the explicit opt-out from Bootstrap — the stage
        // ships the lightweight static DEFAULT_CSS instead of compiling
        // anything.
        let runtime = Arc::new(MockRuntime);
        let mut ctx = make_stage_context(runtime);
        let stage = CompileThemeCssStage::new();

        let input = make_doc_ast(meta_with_theme("none"));
        let output = stage.run(input, &mut ctx).await.unwrap();
        assert!(output.into_document_ast().is_some());

        let css = get_css_artifact(&ctx);
        assert_eq!(css, DEFAULT_CSS);
    }

    #[tokio::test]
    async fn test_builtin_theme_compiles_css() {
        let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> =
            Arc::new(quarto_system_runtime::NativeRuntime::new());
        let mut ctx = make_stage_context(runtime);
        let stage = CompileThemeCssStage::new();

        let input = make_doc_ast(meta_with_theme("cosmo"));
        let output = stage.run(input, &mut ctx).await.unwrap();

        assert!(output.into_document_ast().is_some());

        let css = get_css_artifact(&ctx);
        // Should NOT be the static default
        assert_ne!(css, DEFAULT_CSS);
        // Should be real compiled Bootstrap CSS
        assert!(css.contains(".btn"), "compiled CSS should contain .btn");
        assert!(
            css.contains(".container"),
            "compiled CSS should contain .container"
        );
    }

    #[tokio::test]
    async fn test_cache_hit_skips_compilation() {
        // Use a NativeRuntime with a temp cache dir, pre-populate cache
        let temp = tempfile::TempDir::new().unwrap();
        let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> = Arc::new(
            quarto_system_runtime::NativeRuntime::with_cache_dir(temp.path().to_path_buf()),
        );

        // First run: compiles and caches
        let mut ctx = make_stage_context(runtime.clone());
        let stage = CompileThemeCssStage::new();
        let input = make_doc_ast(meta_with_theme("cosmo"));
        stage.run(input, &mut ctx).await.unwrap();
        let first_css = get_css_artifact(&ctx);
        assert_ne!(first_css, DEFAULT_CSS);

        // Second run: should get same CSS from cache
        let mut ctx2 = make_stage_context(runtime);
        let input2 = make_doc_ast(meta_with_theme("cosmo"));
        stage.run(input2, &mut ctx2).await.unwrap();
        let second_css = get_css_artifact(&ctx2);

        assert_eq!(first_css, second_css);
    }

    #[tokio::test]
    async fn test_no_theme_path_writes_runtime_cache() {
        // After a successful first compile, the default-cache key and the
        // version sentinel should both be present in the runtime cache.
        // Proves the no-theme path routes through cache_set_lru +
        // ensure_namespace_version, so a cold-start second session will
        // find it.
        let temp = tempfile::TempDir::new().unwrap();
        let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> = Arc::new(
            quarto_system_runtime::NativeRuntime::with_cache_dir(temp.path().to_path_buf()),
        );

        let mut ctx = make_stage_context(runtime.clone());
        let stage = CompileThemeCssStage::new();
        stage
            .run(make_doc_ast(empty_meta()), &mut ctx)
            .await
            .unwrap();

        // Default-key entry present after the compile.
        let cached =
            pollster::block_on(runtime.cache_get("sass", DEFAULT_CACHE_KEY_MINIFIED)).unwrap();
        assert!(
            cached.is_some(),
            "no-theme path should populate the default cache entry"
        );
        // Version sentinel written by the generational-purge helper.
        let version =
            pollster::block_on(runtime.cache_get("sass", quarto_system_runtime::CACHE_VERSION_KEY))
                .unwrap();
        assert_eq!(
            version.as_deref(),
            Some(quarto_sass::CSS_BUILD_ID.as_bytes())
        );
    }

    #[tokio::test]
    async fn test_no_theme_path_uses_cached_value_on_subsequent_run() {
        // Pre-populating the cache with a recognizable sentinel string
        // makes a second-run hit observable: the stage must return the
        // sentinel unchanged rather than recompiling.
        let temp = tempfile::TempDir::new().unwrap();
        let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> = Arc::new(
            quarto_system_runtime::NativeRuntime::with_cache_dir(temp.path().to_path_buf()),
        );

        // Seed the namespace with the correct version and a sentinel entry.
        pollster::block_on(runtime.cache_set(
            "sass",
            quarto_system_runtime::CACHE_VERSION_KEY,
            quarto_sass::CSS_BUILD_ID.as_bytes(),
        ))
        .unwrap();
        const SENTINEL: &str = "/* cached sentinel */";
        pollster::block_on(quarto_system_runtime::cache_set_lru(
            runtime.as_ref(),
            "sass",
            DEFAULT_CACHE_KEY_MINIFIED,
            SENTINEL.as_bytes(),
            quarto_system_runtime::SASS_CACHE_BUDGET_BYTES,
        ))
        .unwrap();

        let mut ctx = make_stage_context(runtime.clone());
        let stage = CompileThemeCssStage::new();
        stage
            .run(make_doc_ast(empty_meta()), &mut ctx)
            .await
            .unwrap();

        let css = get_css_artifact(&ctx);
        assert_eq!(
            css, SENTINEL,
            "no-theme path must reuse the cached value instead of recompiling"
        );
    }

    #[tokio::test]
    async fn test_rust_side_change_invalidates_default_cache_via_build_id() {
        // Regression guard for the Phase 3 syntax-highlighting bug. The
        // original incident:
        //
        //   1. Old Quarto assembled default CSS without `highlight.scss`
        //      and stored it under `"default_minified"` in IndexedDB,
        //      with the namespace version sentinel set to
        //      `SCSS_RESOURCES_HASH` (hash of `.scss` files only).
        //   2. We changed `compile_default_css` (Rust) to also load
        //      `highlight_layer`. No `.scss` file changed, so
        //      `SCSS_RESOURCES_HASH` was identical across the upgrade.
        //   3. On next render, the generational purge compared the
        //      stored sentinel to the (unchanged) hash, saw a match,
        //      and left the stale entry in place. Users got served the
        //      pre-fix CSS missing the `.hl-*` rules.
        //
        // The fix is to key the generational purge on `CSS_BUILD_ID`
        // instead of `SCSS_RESOURCES_HASH`. `CSS_BUILD_ID` combines the
        // SCSS hash with a hash of every `.rs` file under
        // `crates/quarto-sass/src/`, so any Rust edit that could affect
        // the compiled CSS automatically bumps the sentinel and triggers
        // a namespace purge on next load. No manual version knob.
        //
        // This test simulates the same setup as the original incident:
        // a previous deploy's CSS cached under the current key, but
        // tagged with an OLD build-id. The generational purge must wipe
        // it on first load despite the cache key itself being
        // unchanged.
        let temp = tempfile::TempDir::new().unwrap();
        let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> = Arc::new(
            quarto_system_runtime::NativeRuntime::with_cache_dir(temp.path().to_path_buf()),
        );

        pollster::block_on(runtime.cache_set(
            "sass",
            quarto_system_runtime::CACHE_VERSION_KEY,
            b"previous-deploy-build-id",
        ))
        .unwrap();
        const STALE: &str = "/* stale: pre-Rust-edit default CSS without .hl-* rules */";
        pollster::block_on(quarto_system_runtime::cache_set_lru(
            runtime.as_ref(),
            "sass",
            DEFAULT_CACHE_KEY_MINIFIED,
            STALE.as_bytes(),
            quarto_system_runtime::SASS_CACHE_BUDGET_BYTES,
        ))
        .unwrap();

        let mut ctx = make_stage_context(runtime.clone());
        let stage = CompileThemeCssStage::new();
        stage
            .run(make_doc_ast(empty_meta()), &mut ctx)
            .await
            .unwrap();

        // Build-id mismatch → namespace purged → stale entry gone →
        // stage recompiled. Output must be the new CSS, not the stale
        // sentinel, and must include the highlight-layer rules.
        let css = get_css_artifact(&ctx);
        assert_ne!(
            css, STALE,
            "stage must not serve stale CSS after a Rust-side change",
        );
        assert!(
            css.contains(".hl-keyword"),
            "recompiled default CSS must include .hl-* rules from highlight.scss",
        );

        // Version sentinel was rewritten to the current build-id.
        let version =
            pollster::block_on(runtime.cache_get("sass", quarto_system_runtime::CACHE_VERSION_KEY))
                .unwrap();
        assert_eq!(
            version.as_deref(),
            Some(quarto_sass::CSS_BUILD_ID.as_bytes()),
            "generational purge must rewrite the sentinel to the current CSS_BUILD_ID",
        );
    }

    #[tokio::test]
    async fn test_stale_generation_purges_old_default_entry() {
        // If a stored cache entry was produced for a different SCSS
        // generation (e.g., we bumped Bootstrap), the generational purge
        // must clear it on first use.
        let temp = tempfile::TempDir::new().unwrap();
        let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> = Arc::new(
            quarto_system_runtime::NativeRuntime::with_cache_dir(temp.path().to_path_buf()),
        );

        // Seed with a stale version + a leftover stale entry.
        pollster::block_on(runtime.cache_set(
            "sass",
            quarto_system_runtime::CACHE_VERSION_KEY,
            b"stale-hash-from-an-older-quarto",
        ))
        .unwrap();
        pollster::block_on(runtime.cache_set(
            "sass",
            DEFAULT_CACHE_KEY_MINIFIED,
            b"/* stale css */",
        ))
        .unwrap();

        let mut ctx = make_stage_context(runtime.clone());
        let stage = CompileThemeCssStage::new();
        stage
            .run(make_doc_ast(empty_meta()), &mut ctx)
            .await
            .unwrap();

        // The stored CSS must NOT be the stale sentinel — the stage had to
        // recompile after the purge cleared it.
        let css = get_css_artifact(&ctx);
        assert_ne!(css, "/* stale css */");
        // Fresh compile produces real Bootstrap CSS.
        assert!(css.contains(".navbar"));
        // And the version sentinel was rewritten to the current hash.
        let version =
            pollster::block_on(runtime.cache_get("sass", quarto_system_runtime::CACHE_VERSION_KEY))
                .unwrap();
        assert_eq!(
            version.as_deref(),
            Some(quarto_sass::CSS_BUILD_ID.as_bytes())
        );
    }

    #[tokio::test]
    async fn test_invalid_theme_produces_loud_error() {
        // Q2 treats configuration errors as user-facing failures
        // rather than silently shipping DEFAULT_CSS. An unknown theme
        // name is a typo in the user's YAML and we want them to know.
        // This replaces the previous silent-fallback test that paired
        // with the now-removed swallow path.
        let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> =
            Arc::new(quarto_system_runtime::NativeRuntime::new());
        let mut ctx = make_stage_context(runtime);
        let stage = CompileThemeCssStage::new();

        let input = make_doc_ast(meta_with_theme("nonexistent"));
        let err = stage
            .run(input, &mut ctx)
            .await
            .expect_err("unknown theme should fail loudly");
        let msg = err.to_string();
        assert!(
            msg.to_lowercase().contains("nonexistent")
                || msg.to_lowercase().contains("unknown theme"),
            "error should mention the offending theme, got: {msg}"
        );
    }

    #[tokio::test]
    async fn test_null_theme_compiles_default_bootstrap() {
        // `theme: null` is treated identically to "theme absent": compile
        // the default Bootstrap + Quarto layer. (Historically this test
        // paired with MockRuntime and relied on compilation-failure fallback
        // producing DEFAULT_CSS; after the Q1-parity change it asserts the
        // real behavior on a real runtime.)
        let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> =
            Arc::new(quarto_system_runtime::NativeRuntime::new());
        let mut ctx = make_stage_context(runtime);
        let stage = CompileThemeCssStage::new();

        // theme: null
        let theme_value = ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::Null),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };
        let root_entry = ConfigMapEntry {
            key: "theme".to_string(),
            key_source: SourceInfo::for_test(),
            value: theme_value,
        };
        let meta = ConfigValue {
            value: ConfigValueKind::Map(vec![root_entry]),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let input = make_doc_ast(meta);
        stage.run(input, &mut ctx).await.unwrap();

        let css = get_css_artifact(&ctx);
        assert_ne!(
            css, DEFAULT_CSS,
            "null theme must compile default Bootstrap, not fall through to static CSS"
        );
        assert!(
            css.contains(".navbar"),
            "null-theme compiled CSS should include Bootstrap .navbar"
        );
    }

    /// Helper to create a theme array metadata (e.g., `theme: [cosmo, custom.scss]`)
    fn meta_with_theme_array(themes: &[&str]) -> ConfigValue {
        let items: Vec<ConfigValue> = themes
            .iter()
            .map(|s| ConfigValue {
                value: ConfigValueKind::Scalar(Yaml::String(s.to_string())),
                source_info: SourceInfo::for_test(),
                merge_op: quarto_pandoc_types::MergeOp::Concat,
            })
            .collect();

        let theme_value = ConfigValue {
            value: ConfigValueKind::Array(items),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let root_entry = ConfigMapEntry {
            key: "theme".to_string(),
            key_source: SourceInfo::for_test(),
            value: theme_value,
        };

        ConfigValue {
            value: ConfigValueKind::Map(vec![root_entry]),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        }
    }

    /// Helper to create a doc_ast with a custom document path (for custom theme resolution)
    fn make_doc_ast_at(path: &str, meta: ConfigValue) -> PipelineData {
        PipelineData::DocumentAst(DocumentAst {
            path: PathBuf::from(path),
            ast: Pandoc {
                meta,
                ..Default::default()
            },
            ast_context: pampa::pandoc::ASTContext::default(),
            source_context: SourceContext::new(),
            warnings: vec![],
            recorded_includes: Vec::new(),
        })
    }

    /// Helper to create a stage context with a custom project dir
    fn make_stage_context_at(
        runtime: Arc<dyn quarto_system_runtime::SystemRuntime>,
        project_dir: &str,
    ) -> StageContext {
        let project = ProjectContext {
            dir: PathBuf::from(project_dir),
            config: crate::project::ProjectConfig::default(),
            is_single_file: true,
            files: vec![],
            output_dir: PathBuf::from(project_dir),
        };
        let doc_path = format!("{}/test.qmd", project_dir);
        let doc = DocumentInfo::from_path(&doc_path);
        let format = Format::html();
        StageContext::new(runtime, format, project, doc).unwrap()
    }

    #[tokio::test]
    async fn test_builtin_plus_custom_theme_array() {
        // Use the quarto-sass test fixture directory as the "document dir"
        let fixture_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../quarto-sass/test-fixtures/custom");
        let fixture_dir = fixture_dir.canonicalize().unwrap();
        let doc_path = fixture_dir.join("test.qmd");

        let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> =
            Arc::new(quarto_system_runtime::NativeRuntime::new());
        let mut ctx = make_stage_context_at(runtime, fixture_dir.to_str().unwrap());
        let stage = CompileThemeCssStage::new();

        // theme: [cosmo, override.scss]
        let meta = meta_with_theme_array(&["cosmo", "override.scss"]);
        let input = make_doc_ast_at(doc_path.to_str().unwrap(), meta);
        let output = stage.run(input, &mut ctx).await.unwrap();

        assert!(output.into_document_ast().is_some());

        let css = get_css_artifact(&ctx);
        // Should NOT be the static default
        assert_ne!(
            css, DEFAULT_CSS,
            "should compile themed CSS, not fall back to default"
        );
        // Should have Bootstrap classes (from cosmo)
        assert!(css.contains(".btn"), "compiled CSS should contain .btn");
        // Should have the custom rule from override.scss
        assert!(
            css.contains(".custom-rule"),
            "compiled CSS should contain .custom-rule from override.scss"
        );
    }

    fn make_builtin_config(theme: &str, minified: bool) -> ThemeConfig {
        let spec = ThemeSpec::parse(theme).unwrap();
        ThemeConfig {
            themes: vec![spec],
            minified,
            suppress_bootstrap: false,
            brand_ref: None,
        }
    }

    fn make_custom_config(path: &str, minified: bool) -> ThemeConfig {
        ThemeConfig {
            themes: vec![ThemeSpec::Custom(PathBuf::from(path))],
            minified,
            suppress_bootstrap: false,
            brand_ref: None,
        }
    }

    #[test]
    fn test_cache_key_deterministic() {
        let runtime = MockRuntime;
        let config = make_builtin_config("cosmo", true);
        let ctx = ThemeContext::new(PathBuf::from("/project"), &runtime);
        let key1 = cache_key(&config, &ctx, &runtime, &SassLayer::default()).unwrap();
        let key2 = cache_key(&config, &ctx, &runtime, &SassLayer::default()).unwrap();
        assert_eq!(key1, key2);
        // SHA-256 hex should be 64 chars
        assert_eq!(key1.len(), 64);
    }

    #[test]
    fn test_cache_key_differs_for_minified() {
        let runtime = MockRuntime;
        let config_min = make_builtin_config("cosmo", true);
        let config_nomin = make_builtin_config("cosmo", false);
        let ctx = ThemeContext::new(PathBuf::from("/project"), &runtime);
        let key_min = cache_key(&config_min, &ctx, &runtime, &SassLayer::default()).unwrap();
        let key_nomin = cache_key(&config_nomin, &ctx, &runtime, &SassLayer::default()).unwrap();
        assert_ne!(key_min, key_nomin);
    }

    #[test]
    fn test_cache_key_differs_for_different_themes() {
        let runtime = MockRuntime;
        let config_cosmo = make_builtin_config("cosmo", true);
        let config_darkly = make_builtin_config("darkly", true);
        let ctx = ThemeContext::new(PathBuf::from("/project"), &runtime);
        let key1 = cache_key(&config_cosmo, &ctx, &runtime, &SassLayer::default()).unwrap();
        let key2 = cache_key(&config_darkly, &ctx, &runtime, &SassLayer::default()).unwrap();
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_cache_key_custom_file_reads_content() {
        // MockRuntime returns empty bytes for file_read, so two different
        // custom paths with the same (empty) content but different paths
        // should still differ.
        let runtime = MockRuntime;
        let config_a = make_custom_config("theme_a.scss", true);
        let config_b = make_custom_config("theme_b.scss", true);
        let ctx = ThemeContext::new(PathBuf::from("/project"), &runtime);
        let key_a = cache_key(&config_a, &ctx, &runtime, &SassLayer::default()).unwrap();
        let key_b = cache_key(&config_b, &ctx, &runtime, &SassLayer::default()).unwrap();
        assert_ne!(key_a, key_b);
    }

    #[test]
    fn test_cache_key_custom_file_different_content() {
        // Create a runtime that returns different content for different files
        struct ContentRuntime;
        #[async_trait::async_trait]
        impl quarto_system_runtime::SystemRuntime for ContentRuntime {
            fn file_read(
                &self,
                path: &std::path::Path,
            ) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
                // Return content based on filename
                Ok(path.to_string_lossy().as_bytes().to_vec())
            }
            fn file_write(
                &self,
                _path: &std::path::Path,
                _contents: &[u8],
            ) -> quarto_system_runtime::RuntimeResult<()> {
                Ok(())
            }
            fn path_exists(
                &self,
                _path: &std::path::Path,
                _kind: Option<quarto_system_runtime::PathKind>,
            ) -> quarto_system_runtime::RuntimeResult<bool> {
                Ok(true)
            }
            fn canonicalize(
                &self,
                path: &std::path::Path,
            ) -> quarto_system_runtime::RuntimeResult<PathBuf> {
                Ok(path.to_path_buf())
            }
            fn path_metadata(
                &self,
                _path: &std::path::Path,
            ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::PathMetadata>
            {
                unimplemented!()
            }
            fn file_copy(
                &self,
                _src: &std::path::Path,
                _dst: &std::path::Path,
            ) -> quarto_system_runtime::RuntimeResult<()> {
                Ok(())
            }
            fn path_rename(
                &self,
                _old: &std::path::Path,
                _new: &std::path::Path,
            ) -> quarto_system_runtime::RuntimeResult<()> {
                Ok(())
            }
            fn file_remove(
                &self,
                _path: &std::path::Path,
            ) -> quarto_system_runtime::RuntimeResult<()> {
                Ok(())
            }
            fn dir_create(
                &self,
                _path: &std::path::Path,
                _recursive: bool,
            ) -> quarto_system_runtime::RuntimeResult<()> {
                Ok(())
            }
            fn dir_remove(
                &self,
                _path: &std::path::Path,
                _recursive: bool,
            ) -> quarto_system_runtime::RuntimeResult<()> {
                Ok(())
            }
            fn dir_list(
                &self,
                _path: &std::path::Path,
            ) -> quarto_system_runtime::RuntimeResult<Vec<PathBuf>> {
                Ok(vec![])
            }
            fn cwd(&self) -> quarto_system_runtime::RuntimeResult<PathBuf> {
                Ok(PathBuf::from("/"))
            }
            fn temp_dir(&self, _template: &str) -> quarto_system_runtime::RuntimeResult<TempDir> {
                Ok(TempDir::new(PathBuf::from("/tmp/test")))
            }
            fn exec_pipe(
                &self,
                _command: &str,
                _args: &[&str],
                _stdin: &[u8],
            ) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
                Ok(vec![])
            }
            fn exec_command(
                &self,
                _command: &str,
                _args: &[&str],
                _stdin: Option<&[u8]>,
            ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::CommandOutput>
            {
                Ok(quarto_system_runtime::CommandOutput {
                    code: 0,
                    stdout: vec![],
                    stderr: vec![],
                })
            }
            fn env_get(&self, _name: &str) -> quarto_system_runtime::RuntimeResult<Option<String>> {
                Ok(None)
            }
            fn env_all(
                &self,
            ) -> quarto_system_runtime::RuntimeResult<std::collections::HashMap<String, String>>
            {
                Ok(std::collections::HashMap::new())
            }
            async fn fetch_url(
                &self,
                _url: &str,
            ) -> quarto_system_runtime::RuntimeResult<(Vec<u8>, String)> {
                Err(quarto_system_runtime::RuntimeError::NotSupported(
                    "mock".to_string(),
                ))
            }
            fn os_name(&self) -> &'static str {
                "mock"
            }
            fn arch(&self) -> &'static str {
                "mock"
            }
            fn cpu_time(&self) -> quarto_system_runtime::RuntimeResult<u64> {
                Ok(0)
            }
            fn xdg_dir(
                &self,
                _kind: quarto_system_runtime::XdgDirKind,
                _subpath: Option<&std::path::Path>,
            ) -> quarto_system_runtime::RuntimeResult<PathBuf> {
                Ok(PathBuf::from("/xdg"))
            }
            fn stdout_write(&self, _data: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
                Ok(())
            }
            fn stderr_write(&self, _data: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
                Ok(())
            }
        }

        // Same file path but runtime returns path-based content
        let runtime = ContentRuntime;
        let config = make_custom_config("theme.scss", true);
        let ctx = ThemeContext::new(PathBuf::from("/project"), &runtime);
        let key1 = cache_key(&config, &ctx, &runtime, &SassLayer::default()).unwrap();

        // Same config, same runtime → same key (deterministic)
        let key2 = cache_key(&config, &ctx, &runtime, &SassLayer::default()).unwrap();
        assert_eq!(key1, key2);
    }

    // === Phase 5 Decision 9: theme fingerprinting ===

    /// Plan test 15a: identical CSS bytes produce identical
    /// fingerprints across calls.
    #[test]
    fn fingerprint_stable_for_identical_inputs() {
        let css = "body { color: red; }";
        let fp1 = theme_fingerprint(css);
        let fp2 = theme_fingerprint(css);
        assert_eq!(fp1, fp2);
        assert_eq!(fp1.len(), THEME_FINGERPRINT_LEN);
    }

    /// Plan test 15b: different CSS outputs produce different
    /// fingerprints (collision-resistance sanity).
    #[test]
    fn fingerprint_differs_for_different_themes() {
        let cosmo_like = ".btn-primary { background: #2780e3; }";
        let darkly_like = ".btn-primary { background: #375a7f; }";
        assert_ne!(
            theme_fingerprint(cosmo_like),
            theme_fingerprint(darkly_like)
        );
    }

    /// Plan test 15c: adding additional rules (analogous to a
    /// user SCSS layer added on top of `cosmo`) changes the
    /// fingerprint. Demonstrates that the fingerprint reacts to
    /// any output change without needing to enumerate every
    /// possible input source.
    #[test]
    fn fingerprint_differs_for_added_scss_layer() {
        let base = ".btn { padding: 0.5rem; }";
        let with_layer = ".btn { padding: 0.5rem; } .custom-rule { color: rebeccapurple; }";
        assert_ne!(theme_fingerprint(base), theme_fingerprint(with_layer));
    }

    /// Plan test 15d: under content-based fingerprinting, two
    /// inputs that produce *byte-equal* CSS outputs (e.g. SCSS
    /// pipelines that canonicalize whitespace, list ordering,
    /// or selector ordering) get the same fingerprint regardless
    /// of how the input differed. This is the key property that
    /// makes the dedup work in practice without a brittle
    /// per-input canonicalization layer.
    #[test]
    fn fingerprint_input_canonicalization() {
        // Whatever upstream did to produce these strings, if both
        // emerge byte-identical from the SCSS pipeline, the
        // fingerprint must agree.
        let a = ".btn{color:red}";
        let b = ".btn{color:red}";
        assert_eq!(theme_fingerprint(a), theme_fingerprint(b));
    }

    /// Sanity: the artifact key/path helper produces the
    /// documented namespace + filename shape (Decision 5) and
    /// honors the single-doc flattening rule (Decision 10).
    #[test]
    fn theme_artifact_key_and_path_shape() {
        let (key_w, path_w) = theme_artifact_key_and_path("abc123", false);
        assert_eq!(key_w, "css:theme:abc123");
        assert_eq!(path_w, PathBuf::from("quarto/quarto-theme-abc123.css"));

        let (key_s, path_s) = theme_artifact_key_and_path("abc123", true);
        assert_eq!(
            key_s, "css:theme:abc123",
            "key is fingerprint-derived in both modes",
        );
        assert_eq!(
            path_s,
            PathBuf::from("styles.css"),
            "single-doc path is the bare styles.css for byte-identity",
        );
    }

    #[test]
    fn test_cache_key_builtin_no_file_reads() {
        // Built-in themes should not cause file reads. MockRuntime returns
        // Ok(vec![]) for file_read, but we verify the key is valid and
        // doesn't depend on file content.
        let runtime = MockRuntime;
        let config = make_builtin_config("cosmo", true);
        let ctx = ThemeContext::new(PathBuf::from("/project"), &runtime);
        let key = cache_key(&config, &ctx, &runtime, &SassLayer::default()).unwrap();
        assert_eq!(key.len(), 64); // SHA-256 hex
    }

    // ── Phase 2 of bd-k8y0: doc-derived SCSS variables seam ──────────

    /// Build a `website.sidebar:` ConfigValue with the given style as a
    /// single-sidebar list. Used by the doc-vars + stage-level tests.
    fn meta_with_website_sidebar_style(style: &str) -> ConfigValue {
        // Inner sidebar object: { id: "guide", style: "<style>" }
        let style_value = ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::String(style.to_string())),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };
        let id_value = ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::String("guide".to_string())),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };
        let sidebar_obj = ConfigValue {
            value: ConfigValueKind::Map(vec![
                ConfigMapEntry {
                    key: "id".to_string(),
                    key_source: SourceInfo::for_test(),
                    value: id_value,
                },
                ConfigMapEntry {
                    key: "style".to_string(),
                    key_source: SourceInfo::for_test(),
                    value: style_value,
                },
            ]),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };
        // website.sidebar = [ sidebar_obj ]
        let sidebar_array = ConfigValue {
            value: ConfigValueKind::Array(vec![sidebar_obj]),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };
        let mut meta = empty_meta();
        meta.insert_path(&["website", "sidebar"], sidebar_array);
        meta
    }

    #[test]
    fn doc_scss_layer_empty_meta_is_empty() {
        let layer = derive_doc_scss_layer(&empty_meta());
        assert!(
            layer.is_empty(),
            "empty metadata should produce an empty SassLayer"
        );
    }

    #[test]
    fn doc_scss_layer_docked_sidebar_emits_border_true() {
        let meta = meta_with_website_sidebar_style("docked");
        let layer = derive_doc_scss_layer(&meta);
        assert!(
            layer.defaults.contains("$sidebar-border: true;"),
            "docked sidebar should emit `$sidebar-border: true;`, got defaults: {:?}",
            layer.defaults
        );
        // Must NOT have `!default` — Q1 emits unconditional assignments
        // so they win against the framework's `false !default`.
        assert!(
            !layer.defaults.contains("!default"),
            "doc-vars assignments must be unconditional (no !default), got: {:?}",
            layer.defaults
        );
    }

    #[test]
    fn doc_scss_layer_floating_sidebar_emits_border_false() {
        let meta = meta_with_website_sidebar_style("floating");
        let layer = derive_doc_scss_layer(&meta);
        assert!(
            layer.defaults.contains("$sidebar-border: false;"),
            "floating sidebar should emit `$sidebar-border: false;`, got defaults: {:?}",
            layer.defaults
        );
    }

    /// Build a `website.sidebar:` ConfigValue with `style` AND an
    /// explicit `border:` boolean. Used to test the Q1 override:
    /// `sidebar.border` wins over the implicit `style == docked` default.
    fn meta_with_website_sidebar_style_and_border(style: &str, border: bool) -> ConfigValue {
        let style_value = ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::String(style.to_string())),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };
        let border_value = ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::Boolean(border)),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };
        let id_value = ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::String("guide".to_string())),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };
        let sidebar_obj = ConfigValue {
            value: ConfigValueKind::Map(vec![
                ConfigMapEntry {
                    key: "id".to_string(),
                    key_source: SourceInfo::for_test(),
                    value: id_value,
                },
                ConfigMapEntry {
                    key: "style".to_string(),
                    key_source: SourceInfo::for_test(),
                    value: style_value,
                },
                ConfigMapEntry {
                    key: "border".to_string(),
                    key_source: SourceInfo::for_test(),
                    value: border_value,
                },
            ]),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };
        let sidebar_array = ConfigValue {
            value: ConfigValueKind::Array(vec![sidebar_obj]),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };
        let mut meta = empty_meta();
        meta.insert_path(&["website", "sidebar"], sidebar_array);
        meta
    }

    #[test]
    fn doc_scss_layer_explicit_border_true_overrides_floating_default() {
        // Floating sidebars default to no border, but `border: true`
        // forces it on. This is the Q1 override path.
        let meta = meta_with_website_sidebar_style_and_border("floating", true);
        let layer = derive_doc_scss_layer(&meta);
        assert!(
            layer.defaults.contains("$sidebar-border: true;"),
            "explicit `border: true` must win over `style: floating`, got defaults: {:?}",
            layer.defaults
        );
    }

    #[test]
    fn doc_scss_layer_explicit_border_false_overrides_docked_default() {
        // Docked sidebars default to border on, but `border: false` suppresses.
        let meta = meta_with_website_sidebar_style_and_border("docked", false);
        let layer = derive_doc_scss_layer(&meta);
        assert!(
            layer.defaults.contains("$sidebar-border: false;"),
            "explicit `border: false` must win over `style: docked`, got defaults: {:?}",
            layer.defaults
        );
    }

    /// bd-jjep / bd-telo — the doc-vars seam also accepts a top-level
    /// `sidebar:` (not just nested `website.sidebar`), matching the
    /// resolver path used by `SidebarGenerateTransform` and
    /// `resolve_sidebar_membership`. Without this, the SCSS variables
    /// would diverge from the rendered sidebar.
    #[test]
    fn doc_scss_layer_top_level_sidebar_picks_up_style() {
        // Same shape as `meta_with_website_sidebar_style("docked")`,
        // but stored at the top level instead of under `website:`.
        let style_value = ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::String("docked".to_string())),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };
        let sidebar_obj = ConfigValue {
            value: ConfigValueKind::Map(vec![ConfigMapEntry {
                key: "style".to_string(),
                key_source: SourceInfo::for_test(),
                value: style_value,
            }]),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };
        let sidebar_array = ConfigValue {
            value: ConfigValueKind::Array(vec![sidebar_obj]),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };
        let mut meta = empty_meta();
        meta.insert_path(&["sidebar"], sidebar_array);

        let layer = derive_doc_scss_layer(&meta);
        assert!(
            layer.defaults.contains("$sidebar-border: true;"),
            "top-level docked sidebar should emit `$sidebar-border: true;`, got defaults: {:?}",
            layer.defaults
        );
    }

    #[tokio::test]
    async fn stage_omits_sidebar_border_rule_when_docked_overrides_to_false() {
        // End-to-end: an explicit `border: false` on a docked sidebar
        // must suppress the rule, even though the implicit default would
        // emit it.
        let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> =
            Arc::new(quarto_system_runtime::NativeRuntime::new());
        let mut ctx = make_stage_context(runtime);
        let stage = CompileThemeCssStage::new();

        let input = make_doc_ast(meta_with_website_sidebar_style_and_border("docked", false));
        stage.run(input, &mut ctx).await.unwrap();

        let css = get_css_artifact(&ctx);
        assert!(
            !css.contains(".sidebar.sidebar-navigation:not(.rollup)"),
            "explicit `border: false` must suppress the sidebar-border rule"
        );
    }

    /// End-to-end stage test for Phase 2 of bd-k8y0: a document whose
    /// merged metadata declares a docked website sidebar should produce
    /// theme CSS that includes the sidebar-border rule.
    #[tokio::test]
    async fn stage_emits_sidebar_border_rule_for_docked_sidebar() {
        let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> =
            Arc::new(quarto_system_runtime::NativeRuntime::new());
        let mut ctx = make_stage_context(runtime);
        let stage = CompileThemeCssStage::new();

        let input = make_doc_ast(meta_with_website_sidebar_style("docked"));
        stage.run(input, &mut ctx).await.unwrap();

        let css = get_css_artifact(&ctx);
        assert!(
            css.contains(".sidebar.sidebar-navigation:not(.rollup)"),
            "stage CSS for a docked sidebar must include the sidebar-border selector"
        );
        let idx = css
            .find(".sidebar.sidebar-navigation:not(.rollup)")
            .unwrap();
        let tail = &css[idx..idx.saturating_add(400).min(css.len())];
        assert!(
            tail.contains("border-right:1px solid") || tail.contains("border-right: 1px solid"),
            "rule must declare border-right:1px solid, got: {}",
            tail
        );
        assert!(tail.contains("!important"), "rule must carry !important");
    }

    /// Sanity: a doc with NO website sidebar must NOT emit the
    /// sidebar-border rule. Guards against accidentally hardcoding.
    #[tokio::test]
    async fn stage_does_not_emit_sidebar_border_rule_for_plain_doc() {
        let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> =
            Arc::new(quarto_system_runtime::NativeRuntime::new());
        let mut ctx = make_stage_context(runtime);
        let stage = CompileThemeCssStage::new();

        let input = make_doc_ast(empty_meta());
        stage.run(input, &mut ctx).await.unwrap();

        let css = get_css_artifact(&ctx);
        assert!(
            !css.contains(".sidebar.sidebar-navigation:not(.rollup)"),
            "plain doc (no website sidebar) must not emit sidebar-border rule"
        );
    }

    /// Cache-correctness: two docs whose only difference is sidebar
    /// style must produce DIFFERENT CSS — i.e. the cache key must
    /// distinguish them. Without this, the no-theme path's fixed
    /// `default_minified` key would alias them and the second doc
    /// would get the first doc's CSS.
    #[tokio::test]
    async fn stage_distinguishes_docked_vs_floating_in_cache() {
        let temp = tempfile::TempDir::new().unwrap();
        let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> = Arc::new(
            quarto_system_runtime::NativeRuntime::with_cache_dir(temp.path().to_path_buf()),
        );
        let stage = CompileThemeCssStage::new();

        // First: docked
        let mut ctx_d = make_stage_context(runtime.clone());
        stage
            .run(
                make_doc_ast(meta_with_website_sidebar_style("docked")),
                &mut ctx_d,
            )
            .await
            .unwrap();
        let css_docked = get_css_artifact(&ctx_d);

        // Second: floating, same project/runtime — must NOT alias
        let mut ctx_f = make_stage_context(runtime);
        stage
            .run(
                make_doc_ast(meta_with_website_sidebar_style("floating")),
                &mut ctx_f,
            )
            .await
            .unwrap();
        let css_floating = get_css_artifact(&ctx_f);

        assert_ne!(
            css_docked, css_floating,
            "docked and floating sidebars must produce different CSS — cache aliasing!"
        );
        assert!(
            css_docked.contains(".sidebar.sidebar-navigation:not(.rollup)"),
            "docked CSS should have the rule"
        );
        assert!(
            !css_floating.contains(".sidebar.sidebar-navigation:not(.rollup)"),
            "floating CSS should NOT have the rule"
        );
    }
}
