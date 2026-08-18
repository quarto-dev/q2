/*
 * stage/stages/bootstrap_js.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Inject Bootstrap's JS runtime as a Project-scoped artifact when
 * the document uses a Bootstrap-backed theme.
 */

//! Inject the Bootstrap 5 JS runtime when a Bootstrap-backed theme is active.
//!
//! ## Pattern this stage demonstrates
//!
//! This is the prototypical "predicate → register Project-scoped `js:*`
//! artifact" stage. It checks one condition (`!is_minimal_html(meta)`)
//! and, when true, stores a single static `js:bootstrap` artifact whose
//! `path` points into the project lib dir. Downstream
//! [`ApplyTemplateStage`](super::ApplyTemplateStage) collects the artifact
//! by `js:` prefix and emits a `<script src="…">` tag in the document
//! `<head>`. There is no separate raw-HTML injection step — the artifact
//! pipeline does both jobs.
//!
//! Future feature stages that need to ship a static JS payload
//! conditionally (autoloader, KaTeX/MathJax, etc.) should follow the same
//! shape unless they need *more* than one knob (inline configuration
//! script, version selection, multiple files), at which point a small
//! shared `JsFeature` helper might pay for itself. Today, with one
//! consumer, that abstraction would be premature.
//!
//! ## Script ordering caveat
//!
//! [`ApplyTemplateStage`](super::ApplyTemplateStage) emits scripts in
//! sorted-key order. `js:bootstrap` sorts before typical `js:libs:*` /
//! `js:quarto-*` keys, which is the order we want today (Bootstrap loads
//! first, components depending on it load later). If we ever introduce a
//! key that needs to load *before* Bootstrap (e.g. a polyfill `js:autoloader`),
//! alphabetic ordering will silently put it after Bootstrap and the user
//! gets a confusing runtime error.
//!
//! Quarto 1 has the same problem and no real solution — building a proper
//! script-dependency-ordering system would degenerate into a small SAT
//! solver, which is not worth the payoff. **The plan is to accept this**
//! and add a small dedicated reorder stage between artifact registration
//! and `ApplyTemplateStage` *if and when* a real conflict shows up.
//! Until then, contributors adding new `js:*` artifacts must pick keys
//! whose alphabetic order matches their desired load order.
//!
//! ## WASM exclusion
//!
//! This module is gated `#[cfg(not(target_arch = "wasm32"))]`. The hub-
//! client preview reinitializes its iframe on every render tick, which
//! blows away any state held by Bootstrap components (open modals,
//! expanded collapses, active tabs). Until the hub-client has a non-iframe
//! renderer, shipping Bootstrap JS to the browser would be at best
//! useless and at worst confusing. The cfg gate also keeps the 80KB
//! payload out of the WASM bundle.

use std::path::PathBuf;

use async_trait::async_trait;
use quarto_sass::ThemeConfig;

use crate::artifact::{Artifact, ArtifactScope};
use crate::format::is_minimal_html;
use crate::stage::{
    EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};
use crate::trace_event;

/// The Bootstrap 5 bundled JS runtime, embedded at compile time.
///
/// "Bundle" means Popper is included — popovers, tooltips, and
/// auto-positioned dropdowns all work without an extra script.
///
/// **Version contract:** must match the SCSS-side Bootstrap version
/// (see `resources/scss/README.md`). Bump the two together.
pub(crate) const BOOTSTRAP_JS: &[u8] =
    include_bytes!("../../../../../resources/js/bootstrap/bootstrap.bundle.min.js");

/// Filename used for the on-disk JS asset and as the leaf of the
/// artifact path.
const BOOTSTRAP_JS_FILENAME: &str = "bootstrap.bundle.min.js";

/// Artifact key. Sort-order matters because `ApplyTemplateStage`
/// emits `<script>` tags in sorted-key order — see module docs.
const BOOTSTRAP_JS_KEY: &str = "js:bootstrap";

/// Inject Bootstrap's JS runtime when the document uses a Bootstrap-backed theme.
///
/// Predicate is the negation of [`is_minimal_html`]: this matches the
/// exact same condition that [`super::CompileThemeCssStage`] uses to
/// compile Bootstrap CSS, so we never ship JS without matching CSS or
/// vice versa.
///
/// On success the stage stores a single Project-scoped `js:bootstrap`
/// artifact. Path is `bootstrap.bundle.min.js` for single-doc renders
/// and `quarto/bootstrap.bundle.min.js` for multi-doc / website renders,
/// mirroring how `CompileThemeCssStage` lays out theme CSS.
pub struct BootstrapJsStage;

impl BootstrapJsStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BootstrapJsStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for BootstrapJsStage {
    fn name(&self) -> &str {
        "bootstrap-js"
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

        // Skip when the document opted out of Bootstrap. We must agree
        // with CompileThemeCssStage's decision exactly — if that stage
        // didn't ship Bootstrap CSS, we mustn't ship Bootstrap JS.
        //
        // Two skip cases, matching Quarto 1's `formatHasBootstrap`:
        // - `theme: none` / `theme: pandoc` → checked via
        //   `ThemeConfig::suppress_bootstrap`. This is the predicate
        //   `CompileThemeCssStage` uses, and it correctly handles
        //   format-nested `format.html.theme: none` (which the
        //   metadata merger does not flatten to root).
        // - `minimal: true` → checked via `is_minimal_html`, which
        //   `ApplyTemplateStage` uses to pick the minimal template
        //   that has no Bootstrap-aware `<head>` to inject into.
        let suppress_by_theme = match ThemeConfig::from_config_value(&doc.ast.meta) {
            // Per-variant since bd-0pic6 A2: `{light: none, dark:
            // darkly}` still compiles a Bootstrap-based dark variant,
            // so JS ships iff ANY variant ships Bootstrap.
            Ok(c) => !c.ships_bootstrap(),
            Err(e) => {
                trace_event!(
                    ctx,
                    EventLevel::Warn,
                    "failed to parse theme config: {}, skipping Bootstrap JS",
                    e
                );
                true
            }
        };
        if suppress_by_theme || is_minimal_html(&doc.ast.meta) {
            trace_event!(ctx, EventLevel::Debug, "Bootstrap not in use, skipping JS");
            return Ok(PipelineData::DocumentAst(doc));
        }

        let path = if ctx.project.is_single_file {
            PathBuf::from(BOOTSTRAP_JS_FILENAME)
        } else {
            PathBuf::from(format!("quarto/{}", BOOTSTRAP_JS_FILENAME))
        };

        ctx.artifacts.store(
            BOOTSTRAP_JS_KEY,
            Artifact::from_bytes(BOOTSTRAP_JS.to_vec(), "text/javascript")
                .with_path(path)
                .with_scope(ArtifactScope::Project),
        );

        trace_event!(
            ctx,
            EventLevel::Debug,
            "stored Bootstrap JS artifact ({} bytes)",
            BOOTSTRAP_JS.len()
        );

        Ok(PipelineData::DocumentAst(doc))
    }
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

    // ── Test helpers (mirroring compile_theme_css.rs) ─────────────────

    fn make_stage_context(
        runtime: Arc<dyn quarto_system_runtime::SystemRuntime>,
        is_single_file: bool,
    ) -> StageContext {
        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: crate::project::ProjectConfig::default(),
            is_single_file,
            files: vec![],
            output_dir: PathBuf::from("/project"),

            ..Default::default()
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
        ConfigValue {
            value: ConfigValueKind::Map(vec![ConfigMapEntry {
                key: "theme".to_string(),
                key_source: SourceInfo::for_test(),
                value: theme_value,
            }]),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        }
    }

    /// Metadata with the Q1 light/dark map form:
    /// `theme: {light: <light>, dark: <dark>}` (bd-o76p01wb).
    fn meta_with_light_dark_theme(light: &str, dark: &str) -> ConfigValue {
        let scalar = |s: &str| ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::String(s.to_string())),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };
        let entry = |key: &str, value: ConfigValue| ConfigMapEntry {
            key: key.to_string(),
            key_source: SourceInfo::for_test(),
            value,
        };
        let theme_value = ConfigValue {
            value: ConfigValueKind::Map(vec![
                entry("light", scalar(light)),
                entry("dark", scalar(dark)),
            ]),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };
        ConfigValue {
            value: ConfigValueKind::Map(vec![entry("theme", theme_value)]),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        }
    }

    fn meta_with_minimal(value: bool) -> ConfigValue {
        let v = ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::Boolean(value)),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };
        ConfigValue {
            value: ConfigValueKind::Map(vec![ConfigMapEntry {
                key: "minimal".to_string(),
                key_source: SourceInfo::for_test(),
                value: v,
            }]),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        }
    }

    // ── Mock runtime (unused-but-required by StageContext) ────────────

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

    // ── Tests ─────────────────────────────────────────────────────────

    /// Empty meta (no `theme:` key) → Bootstrap-backed default theme is
    /// in use, so JS gets registered.
    #[tokio::test]
    async fn empty_meta_registers_bootstrap_js() {
        let runtime = Arc::new(MockRuntime);
        let mut ctx = make_stage_context(runtime, /* single_doc */ true);

        let stage = BootstrapJsStage::new();
        stage
            .run(make_doc_ast(empty_meta()), &mut ctx)
            .await
            .unwrap();

        let entries = ctx.artifacts.get_by_prefix("js:");
        assert_eq!(entries.len(), 1, "expected one js: artifact");
        let (key, artifact) = entries[0];
        assert_eq!(key, "js:bootstrap");
        assert_eq!(artifact.scope, ArtifactScope::Project);
        assert_eq!(artifact.content_type, "text/javascript");
        assert!(
            !artifact.content.is_empty(),
            "Bootstrap JS payload must not be empty"
        );
        assert_eq!(
            artifact.path,
            Some(PathBuf::from("bootstrap.bundle.min.js")),
            "single-doc path must be bare filename"
        );
    }

    /// `theme: cosmo` → Bootstrap theme is in use, JS gets registered.
    #[tokio::test]
    async fn themed_doc_registers_bootstrap_js() {
        let runtime = Arc::new(MockRuntime);
        let mut ctx = make_stage_context(runtime, true);

        let stage = BootstrapJsStage::new();
        stage
            .run(make_doc_ast(meta_with_theme("cosmo")), &mut ctx)
            .await
            .unwrap();

        assert!(ctx.artifacts.contains("js:bootstrap"));
    }

    /// `theme: none` → user opted out of Bootstrap; no JS.
    #[tokio::test]
    async fn theme_none_skips_bootstrap_js() {
        let runtime = Arc::new(MockRuntime);
        let mut ctx = make_stage_context(runtime, true);

        let stage = BootstrapJsStage::new();
        stage
            .run(make_doc_ast(meta_with_theme("none")), &mut ctx)
            .await
            .unwrap();

        assert!(
            !ctx.artifacts.contains("js:bootstrap"),
            "theme: none must not register Bootstrap JS"
        );
        assert!(ctx.artifacts.get_by_prefix("js:").is_empty());
    }

    /// `theme: {light: cosmo, dark: darkly}` → Bootstrap (light half)
    /// is in use, so JS must be registered. Guards against the
    /// pre-bd-o76p01wb behavior where the map form failed theme
    /// parsing and this stage silently treated the failure as
    /// `suppress_bootstrap`, shipping themed CSS without its JS.
    #[tokio::test]
    async fn light_dark_theme_map_registers_bootstrap_js() {
        let runtime = Arc::new(MockRuntime);
        let mut ctx = make_stage_context(runtime, true);

        let stage = BootstrapJsStage::new();
        stage
            .run(
                make_doc_ast(meta_with_light_dark_theme("cosmo", "darkly")),
                &mut ctx,
            )
            .await
            .unwrap();

        assert!(
            ctx.artifacts.contains("js:bootstrap"),
            "light/dark map form must not suppress Bootstrap JS"
        );
    }

    /// `theme: {light: none, dark: darkly}` → the dark variant uses
    /// Bootstrap even though the light variant opted out, so JS must
    /// ship (bd-0pic6 A2: suppression is per-variant; JS ships iff
    /// any variant ships Bootstrap).
    #[tokio::test]
    async fn light_none_dark_theme_still_registers_bootstrap_js() {
        let runtime = Arc::new(MockRuntime);
        let mut ctx = make_stage_context(runtime, true);

        let stage = BootstrapJsStage::new();
        stage
            .run(
                make_doc_ast(meta_with_light_dark_theme("none", "darkly")),
                &mut ctx,
            )
            .await
            .unwrap();

        assert!(
            ctx.artifacts.contains("js:bootstrap"),
            "a Bootstrap-using dark variant must ship Bootstrap JS even when light: none"
        );
    }

    /// `theme: {light: none, dark: none}` → no variant uses Bootstrap.
    #[tokio::test]
    async fn both_variants_none_skip_bootstrap_js() {
        let runtime = Arc::new(MockRuntime);
        let mut ctx = make_stage_context(runtime, true);

        let stage = BootstrapJsStage::new();
        stage
            .run(
                make_doc_ast(meta_with_light_dark_theme("none", "none")),
                &mut ctx,
            )
            .await
            .unwrap();

        assert!(!ctx.artifacts.contains("js:bootstrap"));
    }

    /// `theme: pandoc` → user wants raw Pandoc HTML; no Bootstrap JS.
    #[tokio::test]
    async fn theme_pandoc_skips_bootstrap_js() {
        let runtime = Arc::new(MockRuntime);
        let mut ctx = make_stage_context(runtime, true);

        let stage = BootstrapJsStage::new();
        stage
            .run(make_doc_ast(meta_with_theme("pandoc")), &mut ctx)
            .await
            .unwrap();

        assert!(!ctx.artifacts.contains("js:bootstrap"));
    }

    /// `minimal: true` → explicit minimal-output opt-out; no JS.
    #[tokio::test]
    async fn minimal_true_skips_bootstrap_js() {
        let runtime = Arc::new(MockRuntime);
        let mut ctx = make_stage_context(runtime, true);

        let stage = BootstrapJsStage::new();
        stage
            .run(make_doc_ast(meta_with_minimal(true)), &mut ctx)
            .await
            .unwrap();

        assert!(!ctx.artifacts.contains("js:bootstrap"));
    }

    /// Multi-doc context → path is namespaced under `quarto/` so the
    /// project lib dir layout matches `CompileThemeCssStage`.
    #[tokio::test]
    async fn multi_doc_uses_namespaced_path() {
        let runtime = Arc::new(MockRuntime);
        let mut ctx = make_stage_context(runtime, /* single_doc */ false);

        let stage = BootstrapJsStage::new();
        stage
            .run(make_doc_ast(empty_meta()), &mut ctx)
            .await
            .unwrap();

        let artifact = ctx.artifacts.get("js:bootstrap").unwrap();
        assert_eq!(
            artifact.path,
            Some(PathBuf::from("quarto/bootstrap.bundle.min.js"))
        );
    }

    /// Re-running the stage with the same input is byte-identical and
    /// idempotent — second run does not duplicate or alter the artifact.
    #[tokio::test]
    async fn rerun_is_idempotent() {
        let runtime = Arc::new(MockRuntime);
        let mut ctx = make_stage_context(runtime, true);

        let stage = BootstrapJsStage::new();
        stage
            .run(make_doc_ast(empty_meta()), &mut ctx)
            .await
            .unwrap();
        let first = ctx.artifacts.get("js:bootstrap").unwrap().content.clone();

        stage
            .run(make_doc_ast(empty_meta()), &mut ctx)
            .await
            .unwrap();
        let second = ctx.artifacts.get("js:bootstrap").unwrap().content.clone();

        assert_eq!(first, second);
        assert_eq!(ctx.artifacts.get_by_prefix("js:").len(), 1);
    }

    /// The embedded payload is the bundled Bootstrap build (Popper
    /// inlined). Without Popper, tooltips/popovers/auto-positioned
    /// dropdowns silently break — verifying this catches an accidental
    /// downgrade to the non-bundle file.
    #[test]
    fn bundled_js_includes_popper() {
        let s = std::str::from_utf8(BOOTSTRAP_JS).expect("Bootstrap JS must be valid UTF-8");
        assert!(
            s.to_ascii_lowercase().contains("popper"),
            "Bootstrap JS must be the bundled build that includes Popper"
        );
    }

    /// bd-4b7f1hr7: enforce the "bump the two together" version
    /// contract between the vendored Bootstrap JS bundle and the
    /// vendored Bootstrap SCSS distribution.
    ///
    /// The SCSS dist carries no machine-readable version string, so
    /// the strongest available SCSS-side marker is the version
    /// `resources/scss/README.md` documents (the README is part of
    /// the documented bump procedure for `resources/scss/`). The JS
    /// side is the `Bootstrap vX.Y.Z` banner in the bundle itself.
    /// Bumping either artifact without updating the README — or the
    /// README without the other artifact — fails this test.
    #[test]
    fn bootstrap_js_version_matches_scss_readme() {
        fn version_after(text: &str, marker: &str, source: &str) -> String {
            let at = text
                .find(marker)
                .unwrap_or_else(|| panic!("{source}: no `{marker}` version marker found"));
            let rest = &text[at + marker.len()..];
            let version: String = rest
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            assert!(
                version.split('.').count() == 3 && version.split('.').all(|p| !p.is_empty()),
                "{source}: malformed version `{version}` after `{marker}`"
            );
            version
        }

        let js = std::str::from_utf8(BOOTSTRAP_JS).expect("Bootstrap JS must be valid UTF-8");
        let js_version = version_after(js, "Bootstrap v", "bootstrap.bundle.min.js banner");

        let readme_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../resources/scss/README.md"
        );
        let readme = std::fs::read_to_string(readme_path)
            .unwrap_or_else(|e| panic!("reading {readme_path}: {e}"));
        let scss_version = version_after(&readme, "Bootstrap ", "resources/scss/README.md");

        assert_eq!(
            js_version, scss_version,
            "vendored Bootstrap JS bundle (v{js_version}) and the SCSS \
             distribution documented in resources/scss/README.md \
             (v{scss_version}) must be the same version — bump the two \
             together (and update the README)"
        );
    }
}
