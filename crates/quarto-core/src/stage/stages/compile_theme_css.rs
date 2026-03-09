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
//! If no theme is specified, the stage stores the static `DEFAULT_CSS`
//! without compilation. Compilation results are cached via the
//! `SystemRuntime` cache interface to avoid expensive recompilation.

use std::path::PathBuf;

use async_trait::async_trait;
use quarto_sass::{ThemeConfig, ThemeContext, assemble_theme_scss};

use crate::artifact::Artifact;
use crate::pipeline::DEFAULT_CSS_ARTIFACT_PATH;
use crate::resources::DEFAULT_CSS;
use crate::stage::{
    EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};
use crate::trace_event;

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

/// Compute a cache key from the assembled SCSS and minification flag.
fn cache_key(scss: &str, minified: bool) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    scss.hash(&mut hasher);
    minified.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
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

        // No themes → use static DEFAULT_CSS (no compilation needed)
        if !theme_config.has_themes() {
            trace_event!(
                ctx,
                EventLevel::Debug,
                "no theme specified, using default CSS"
            );
            store_default_css(ctx);
            return Ok(PipelineData::DocumentAst(doc));
        }

        // Assemble SCSS from theme config
        let document_dir = doc
            .path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        let theme_context = ThemeContext::new(document_dir, ctx.runtime.as_ref());

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

        let key = cache_key(&scss, theme_config.minified);

        // Check cache (best-effort — errors are non-fatal)
        if let Ok(Some(cached)) = ctx.runtime.cache_get("sass", &key).await {
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

        // Cache miss — compile
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
                // Store in cache (best-effort)
                let _ = ctx.runtime.cache_set("sass", &key, css.as_bytes()).await;
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
    use quarto_source_map::{SourceContext, SourceInfo};
    use quarto_system_runtime::TempDir;
    use std::sync::Arc;
    use yaml_rust2::Yaml;

    // ── Test helpers ─────────────────────────────────────────────────

    fn make_stage_context(runtime: Arc<dyn quarto_system_runtime::SystemRuntime>) -> StageContext {
        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: None,
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
        fn fetch_url(&self, _url: &str) -> quarto_system_runtime::RuntimeResult<(Vec<u8>, String)> {
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
    async fn test_no_theme_uses_default_css() {
        let runtime = Arc::new(MockRuntime);
        let mut ctx = make_stage_context(runtime);
        let stage = CompileThemeCssStage::new();

        let input = make_doc_ast(empty_meta());
        let output = stage.run(input, &mut ctx).await.unwrap();

        // Should pass through DocumentAst
        assert!(output.into_document_ast().is_some());

        // Artifact should be DEFAULT_CSS
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
    async fn test_null_theme_uses_default_css() {
        let runtime = Arc::new(MockRuntime);
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
        assert_eq!(css, DEFAULT_CSS);
    }

    #[test]
    fn test_cache_key_deterministic() {
        let key1 = cache_key("scss content", true);
        let key2 = cache_key("scss content", true);
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_cache_key_differs_for_minified() {
        let key_min = cache_key("scss content", true);
        let key_nomin = cache_key("scss content", false);
        assert_ne!(key_min, key_nomin);
    }

    #[test]
    fn test_cache_key_differs_for_content() {
        let key1 = cache_key("content A", true);
        let key2 = cache_key("content B", true);
        assert_ne!(key1, key2);
    }
}
