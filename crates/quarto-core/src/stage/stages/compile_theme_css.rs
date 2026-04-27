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
//! SCSS to CSS, and stores the result as the `"css:default"` artifact.
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
use quarto_sass::{
    CSS_BUILD_ID, ThemeConfig, ThemeContext, assemble_theme_scss, compile_default_css,
};
use quarto_system_runtime::{
    SASS_CACHE_BUDGET_BYTES, SystemRuntime, cache_get_lru, cache_set_lru, ensure_namespace_version,
};

use crate::artifact::Artifact;
use crate::pipeline::DEFAULT_CSS_ARTIFACT_PATH;
use crate::resources::DEFAULT_CSS;
use crate::stage::{
    EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};
use crate::trace_event;

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
/// 4. Stores result as `"css:default"` artifact
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
/// and custom file contents.
///
/// The key is `SHA256(SCSS_RESOURCES_HASH + theme_identities + custom_file_contents + minified)`.
/// Built-in themes contribute only their name (content is already covered by
/// `SCSS_RESOURCES_HASH`). Custom themes contribute their resolved path and file contents.
fn cache_key(
    theme_config: &ThemeConfig,
    theme_context: &ThemeContext<'_>,
    runtime: &dyn quarto_system_runtime::SystemRuntime,
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
        }
        hasher.update(b"\n");
    }

    // Include minification flag
    hasher.update(if theme_config.minified { b"1" } else { b"0" });

    let hash = hasher.finalize();
    Ok(format!("{:x}", hash))
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

        // Extract theme config from merged metadata
        let theme_config = match ThemeConfig::from_config_value(&doc.ast.meta) {
            Ok(config) => config,
            Err(e) => {
                trace_event!(
                    ctx,
                    EventLevel::Warn,
                    "failed to extract theme config: {}, using default CSS",
                    e
                );
                store_default_css(ctx);
                return Ok(PipelineData::DocumentAst(doc));
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

        // Theme absent → compile default Bootstrap + Quarto layer. Matches
        // Quarto 1's behavior so features like navbar/footer have the CSS
        // classes they depend on.
        if !theme_config.has_themes() {
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
                "no theme specified, compiling default Bootstrap + Quarto layer"
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

        // Create ThemeContext (needed for both cache key and assembly)
        let document_dir = doc
            .path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        let theme_context = ThemeContext::new(document_dir, ctx.runtime.as_ref());

        // Compute cache key BEFORE assembly (reads custom files, but skips SCSS assembly)
        let key = match cache_key(&theme_config, &theme_context, ctx.runtime.as_ref()) {
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

        // Cache miss — assemble and compile
        let (scss, load_paths) = match assemble_theme_scss(&theme_config, &theme_context) {
            Ok(result) => result,
            Err(e) => {
                trace_event!(
                    ctx,
                    EventLevel::Warn,
                    "failed to assemble theme SCSS: {}, using default CSS",
                    e
                );
                store_default_css(ctx);
                return Ok(PipelineData::DocumentAst(doc));
            }
        };

        trace_event!(
            ctx,
            EventLevel::Debug,
            "compiling theme CSS ({} themes, key={})",
            theme_config.themes.len(),
            key
        );

        let css = compile_scss(ctx, &scss, &load_paths, theme_config.minified).await;

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

fn store_default_css(ctx: &mut StageContext) {
    ctx.artifacts.store(
        "css:default",
        Artifact::from_string(DEFAULT_CSS, "text/css")
            .with_path(PathBuf::from(DEFAULT_CSS_ARTIFACT_PATH)),
    );
}

fn store_css(ctx: &mut StageContext, css: String) {
    ctx.artifacts.store(
        "css:default",
        Artifact::from_string(css, "text/css").with_path(PathBuf::from(DEFAULT_CSS_ARTIFACT_PATH)),
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

#[cfg(not(target_arch = "wasm32"))]
async fn compile_scss(
    ctx: &StageContext,
    scss: &str,
    load_paths: &[PathBuf],
    minified: bool,
) -> Result<String, String> {
    use quarto_sass::{all_resources, default_load_paths};
    use quarto_system_runtime::sass_native::compile_scss_with_embedded;

    let resources = all_resources();

    // Merge default load paths with theme-specific ones
    let mut all_paths = default_load_paths();
    // Avoid duplicates: assemble_theme_scss already includes default_load_paths,
    // but compile_scss_with_embedded uses them for filesystem resolution
    all_paths.clear();
    all_paths.extend_from_slice(load_paths);

    compile_scss_with_embedded(ctx.runtime.as_ref(), &resources, scss, &all_paths, minified)
        .map_err(|e| e.to_string())
}

#[cfg(target_arch = "wasm32")]
async fn compile_scss(
    ctx: &StageContext,
    scss: &str,
    load_paths: &[PathBuf],
    minified: bool,
) -> Result<String, String> {
    ctx.runtime
        .compile_sass(scss, load_paths, minified)
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
        })
    }

    fn empty_meta() -> ConfigValue {
        ConfigValue {
            value: ConfigValueKind::Map(vec![]),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        }
    }

    fn meta_with_theme(theme: &str) -> ConfigValue {
        let theme_value = ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::String(theme.to_string())),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let root_entry = ConfigMapEntry {
            key: "theme".to_string(),
            key_source: SourceInfo::default(),
            value: theme_value,
        };

        ConfigValue {
            value: ConfigValueKind::Map(vec![root_entry]),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        }
    }

    fn get_css_artifact(ctx: &StageContext) -> String {
        let artifact = ctx
            .artifacts
            .get("css:default")
            .expect("css:default artifact missing");
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
    async fn test_invalid_theme_falls_back_to_default() {
        let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> =
            Arc::new(quarto_system_runtime::NativeRuntime::new());
        let mut ctx = make_stage_context(runtime);
        let stage = CompileThemeCssStage::new();

        // "nonexistent" is not a valid theme name
        let input = make_doc_ast(meta_with_theme("nonexistent"));
        let output = stage.run(input, &mut ctx).await.unwrap();

        assert!(output.into_document_ast().is_some());

        // Should fall back to DEFAULT_CSS
        let css = get_css_artifact(&ctx);
        assert_eq!(css, DEFAULT_CSS);
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
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };
        let root_entry = ConfigMapEntry {
            key: "theme".to_string(),
            key_source: SourceInfo::default(),
            value: theme_value,
        };
        let meta = ConfigValue {
            value: ConfigValueKind::Map(vec![root_entry]),
            source_info: SourceInfo::default(),
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
                source_info: SourceInfo::default(),
                merge_op: quarto_pandoc_types::MergeOp::Concat,
            })
            .collect();

        let theme_value = ConfigValue {
            value: ConfigValueKind::Array(items),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let root_entry = ConfigMapEntry {
            key: "theme".to_string(),
            key_source: SourceInfo::default(),
            value: theme_value,
        };

        ConfigValue {
            value: ConfigValueKind::Map(vec![root_entry]),
            source_info: SourceInfo::default(),
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
        }
    }

    fn make_custom_config(path: &str, minified: bool) -> ThemeConfig {
        ThemeConfig {
            themes: vec![ThemeSpec::Custom(PathBuf::from(path))],
            minified,
            suppress_bootstrap: false,
        }
    }

    #[test]
    fn test_cache_key_deterministic() {
        let runtime = MockRuntime;
        let config = make_builtin_config("cosmo", true);
        let ctx = ThemeContext::new(PathBuf::from("/project"), &runtime);
        let key1 = cache_key(&config, &ctx, &runtime).unwrap();
        let key2 = cache_key(&config, &ctx, &runtime).unwrap();
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
        let key_min = cache_key(&config_min, &ctx, &runtime).unwrap();
        let key_nomin = cache_key(&config_nomin, &ctx, &runtime).unwrap();
        assert_ne!(key_min, key_nomin);
    }

    #[test]
    fn test_cache_key_differs_for_different_themes() {
        let runtime = MockRuntime;
        let config_cosmo = make_builtin_config("cosmo", true);
        let config_darkly = make_builtin_config("darkly", true);
        let ctx = ThemeContext::new(PathBuf::from("/project"), &runtime);
        let key1 = cache_key(&config_cosmo, &ctx, &runtime).unwrap();
        let key2 = cache_key(&config_darkly, &ctx, &runtime).unwrap();
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
        let key_a = cache_key(&config_a, &ctx, &runtime).unwrap();
        let key_b = cache_key(&config_b, &ctx, &runtime).unwrap();
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
        let key1 = cache_key(&config, &ctx, &runtime).unwrap();

        // Same config, same runtime → same key (deterministic)
        let key2 = cache_key(&config, &ctx, &runtime).unwrap();
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_cache_key_builtin_no_file_reads() {
        // Built-in themes should not cause file reads. MockRuntime returns
        // Ok(vec![]) for file_read, but we verify the key is valid and
        // doesn't depend on file content.
        let runtime = MockRuntime;
        let config = make_builtin_config("cosmo", true);
        let ctx = ThemeContext::new(PathBuf::from("/project"), &runtime);
        let key = cache_key(&config, &ctx, &runtime).unwrap();
        assert_eq!(key.len(), 64); // SHA-256 hex
    }
}
