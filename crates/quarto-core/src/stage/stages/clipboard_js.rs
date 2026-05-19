/*
 * stage/stages/clipboard_js.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Inject the vendored clipboard.js library as a Project-scoped
 * artifact when the document has code-copy enabled.
 */

//! Inject the vendored clipboard.js library when code-copy is enabled.
//!
//! Twin of [`BootstrapJsStage`](super::BootstrapJsStage) — same predicate
//! → register Project-scoped `js:*` artifact shape, different payload and
//! gating predicate. Reads the document-level `code-copy` metadata
//! through the shared
//! [`resolve_default_copy_mode`](crate::transforms::resolve_default_copy_mode)
//! helper that
//! [`CodeBlockGenerateTransform`](crate::transforms::CodeBlockGenerateTransform)
//! also uses, so the JS shipping decision can never diverge from the
//! decoration decision: if Generate emitted any copy scaffolding, this
//! stage ships the library that makes it functional, and vice versa.
//!
//! The companion init handler (the small script that wires up
//! `ClipboardJS('.code-copy-button')` and the "Copied!" tooltip) is
//! injected as a sibling `js:code-copy-init` artifact. Without it, the
//! library is shipped but inert; the button is visible (after the
//! Phase 2 SCSS port) but clicking doesn't copy.
//!
//! ## Script ordering
//!
//! [`ApplyTemplateStage`](super::ApplyTemplateStage) emits scripts in
//! sorted-key order. The keys land as:
//!
//! - `js:bootstrap` (provides the Tooltip popover the init handler
//!   uses for "Copied!")
//! - `js:clipboard` (the library)
//! - `js:code-copy-init` (the init handler — depends on both above)
//!
//! Alphabetic order happens to match the dependency order. Same caveat
//! as `bootstrap_js.rs`: a future key that breaks this ordering will
//! need a dedicated reorder stage.
//!
//! ## WASM exclusion
//!
//! Gated `#[cfg(not(target_arch = "wasm32"))]` for the same reasons as
//! [`BootstrapJsStage`]. The hub-client preview reinitializes its
//! iframe on every render tick, so no in-iframe JS state survives;
//! shipping clipboard.js would bloat the WASM bundle by 9KB for no
//! functional benefit. The copy *button* still appears in the iframe
//! (it's an AST-level construct emitted by
//! [`CodeBlockRenderTransform`](crate::transforms::CodeBlockRenderTransform))
//! — it just isn't wired to a click handler.

use std::path::PathBuf;

use async_trait::async_trait;

use crate::artifact::{Artifact, ArtifactScope};
use crate::format::is_minimal_html;
use crate::stage::{
    EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};
use crate::trace_event;
use crate::transforms::{CopyMode, resolve_default_copy_mode};

/// Vendored clipboard.js library, embedded at compile time.
///
/// Source: a copy of `clipboard/clipboard.min.js` from the Quarto 1
/// distribution (currently v2.x). Update both this file and the Q1
/// reference together if the upstream library ever changes shape.
pub(crate) const CLIPBOARD_JS: &[u8] =
    include_bytes!("../../../../../resources/js/clipboard/clipboard.min.js");

/// Init handler that wires `ClipboardJS` to every `.code-copy-button`
/// and shows the "Copied!" Bootstrap-Tooltip on success.
///
/// Ported from Q1's `quarto-html-after-body.ejs` (`if (copyCode) { … }`
/// block). Q1 inlined this in the page template; Q2 ships it as a
/// separate file so the script loads from the artifact pipeline like
/// every other JS asset.
pub(crate) const CODE_COPY_INIT_JS: &[u8] =
    include_bytes!("../../../../../resources/js/clipboard/code-copy-init.js");

/// Filename used for the on-disk JS asset and as the leaf of the
/// artifact path.
const CLIPBOARD_JS_FILENAME: &str = "clipboard.min.js";
const CODE_COPY_INIT_FILENAME: &str = "code-copy-init.js";

/// Artifact keys. See module docs for the load-order convention —
/// alphabetic order across these keys matches the dependency order
/// (bootstrap → clipboard → init).
const CLIPBOARD_JS_KEY: &str = "js:clipboard";
const CODE_COPY_INIT_KEY: &str = "js:code-copy-init";

/// Inject the clipboard.js library when code-copy is enabled.
///
/// Predicate is the conjunction of:
///
/// - `!is_minimal_html(meta)` — the minimal-HTML template has no
///   Bootstrap-aware `<head>` to inject scripts into; matches
///   [`BootstrapJsStage`](super::BootstrapJsStage)'s gate.
/// - `resolve_default_copy_mode(meta) != CopyMode::Off` — explicit
///   `code-copy: false` opts out of the entire copy machinery. The
///   *default* (unset metadata) is `Hover`, so the library ships by
///   default — matching Q1's behavior (`format-html.ts:710-712`).
///
/// On success the stage stores a single Project-scoped `js:clipboard`
/// artifact. Path is `clipboard.min.js` for single-doc renders and
/// `quarto/clipboard.min.js` for multi-doc / website renders,
/// mirroring [`BootstrapJsStage`](super::BootstrapJsStage)'s layout.
pub struct ClipboardJsStage;

impl ClipboardJsStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ClipboardJsStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for ClipboardJsStage {
    fn name(&self) -> &str {
        "clipboard-js"
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

        // Two skip cases, both matching Quarto 1:
        // - minimal-html template has nowhere to inject the script.
        // - `code-copy: false` explicitly opts out.
        if is_minimal_html(&doc.ast.meta) {
            trace_event!(
                ctx,
                EventLevel::Debug,
                "minimal HTML format, skipping clipboard.js"
            );
            return Ok(PipelineData::DocumentAst(doc));
        }
        if matches!(resolve_default_copy_mode(&doc.ast.meta), CopyMode::Off) {
            trace_event!(
                ctx,
                EventLevel::Debug,
                "code-copy: false, skipping clipboard.js"
            );
            return Ok(PipelineData::DocumentAst(doc));
        }

        let asset_path = |filename: &str| {
            if ctx.project.is_single_file {
                PathBuf::from(filename)
            } else {
                PathBuf::from(format!("quarto/{}", filename))
            }
        };

        ctx.artifacts.store(
            CLIPBOARD_JS_KEY,
            Artifact::from_bytes(CLIPBOARD_JS.to_vec(), "text/javascript")
                .with_path(asset_path(CLIPBOARD_JS_FILENAME))
                .with_scope(ArtifactScope::Project),
        );
        ctx.artifacts.store(
            CODE_COPY_INIT_KEY,
            Artifact::from_bytes(CODE_COPY_INIT_JS.to_vec(), "text/javascript")
                .with_path(asset_path(CODE_COPY_INIT_FILENAME))
                .with_scope(ArtifactScope::Project),
        );

        trace_event!(
            ctx,
            EventLevel::Debug,
            "stored clipboard.js ({} bytes) + code-copy-init.js ({} bytes)",
            CLIPBOARD_JS.len(),
            CODE_COPY_INIT_JS.len(),
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
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        }
    }

    fn meta_with_entry(key: &str, value: ConfigValueKind) -> ConfigValue {
        ConfigValue {
            value: ConfigValueKind::Map(vec![ConfigMapEntry {
                key: key.to_string(),
                key_source: SourceInfo::default(),
                value: ConfigValue {
                    value,
                    source_info: SourceInfo::default(),
                    merge_op: quarto_pandoc_types::MergeOp::Concat,
                },
            }]),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        }
    }

    /// Minimal mock runtime so we can construct a `StageContext`.
    /// Mirrors the mock used by `bootstrap_js.rs` — the trait surface
    /// is wide enough that duplicating the no-op impl is cheaper than
    /// extracting a shared test helper for now.
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

    #[tokio::test]
    async fn clipboard_js_stage_stores_both_artifacts_under_default() {
        // Empty meta → default copy mode is Hover → ship the library
        // AND the init handler.
        let mut ctx = make_stage_context(Arc::new(MockRuntime), true);
        let result = ClipboardJsStage::new()
            .run(make_doc_ast(empty_meta()), &mut ctx)
            .await
            .unwrap();
        assert!(matches!(result, PipelineData::DocumentAst(_)));
        assert!(
            ctx.artifacts.get(CLIPBOARD_JS_KEY).is_some(),
            "default meta should ship js:clipboard; got keys {:?}",
            ctx.artifacts.keys().collect::<Vec<_>>(),
        );
        assert!(
            ctx.artifacts.get(CODE_COPY_INIT_KEY).is_some(),
            "default meta should ship js:code-copy-init; got keys {:?}",
            ctx.artifacts.keys().collect::<Vec<_>>(),
        );
    }

    #[tokio::test]
    async fn clipboard_js_stage_skips_both_when_code_copy_false() {
        let meta = meta_with_entry("code-copy", ConfigValueKind::Scalar(Yaml::Boolean(false)));
        let mut ctx = make_stage_context(Arc::new(MockRuntime), true);
        ClipboardJsStage::new()
            .run(make_doc_ast(meta), &mut ctx)
            .await
            .unwrap();
        assert!(
            ctx.artifacts.get(CLIPBOARD_JS_KEY).is_none(),
            "code-copy: false must skip js:clipboard",
        );
        assert!(
            ctx.artifacts.get(CODE_COPY_INIT_KEY).is_none(),
            "code-copy: false must skip js:code-copy-init",
        );
    }

    #[tokio::test]
    async fn clipboard_js_stage_skips_both_when_minimal_html() {
        // `minimal: true` opts the document into the minimal-HTML
        // template, which has no script-injection slot. Matches the
        // gate used by BootstrapJsStage.
        let meta = meta_with_entry("minimal", ConfigValueKind::Scalar(Yaml::Boolean(true)));
        let mut ctx = make_stage_context(Arc::new(MockRuntime), true);
        ClipboardJsStage::new()
            .run(make_doc_ast(meta), &mut ctx)
            .await
            .unwrap();
        assert!(
            ctx.artifacts.get(CLIPBOARD_JS_KEY).is_none(),
            "minimal HTML must skip js:clipboard",
        );
        assert!(
            ctx.artifacts.get(CODE_COPY_INIT_KEY).is_none(),
            "minimal HTML must skip js:code-copy-init",
        );
    }

    #[tokio::test]
    async fn clipboard_js_stage_uses_quarto_path_in_multidoc() {
        let mut ctx = make_stage_context(Arc::new(MockRuntime), /* single = */ false);
        ClipboardJsStage::new()
            .run(make_doc_ast(empty_meta()), &mut ctx)
            .await
            .unwrap();
        let clipboard = ctx
            .artifacts
            .get(CLIPBOARD_JS_KEY)
            .expect("js:clipboard stored");
        assert_eq!(
            clipboard.path,
            Some(PathBuf::from("quarto/clipboard.min.js")),
            "multi-doc render must scope the library under quarto/",
        );
        let init = ctx
            .artifacts
            .get(CODE_COPY_INIT_KEY)
            .expect("js:code-copy-init stored");
        assert_eq!(
            init.path,
            Some(PathBuf::from("quarto/code-copy-init.js")),
            "multi-doc render must scope the init handler under quarto/",
        );
    }
}
