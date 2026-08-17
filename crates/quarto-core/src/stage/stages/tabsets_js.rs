/*
 * tabsets_js.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Pipeline stage that injects the grouped-tabset sync module.
 */

//! Tabsets sync JS injection stage.
//!
//! Twin of [`BootstrapJsStage`](super::BootstrapJsStage) — same
//! predicate, same artifact plumbing — for the grouped-tabset sync
//! module (`resources/js/tabsets/tabsets.js`, ported from Quarto 1's
//! `site_libs/quarto-html/tabsets/tabsets.js`).
//!
//! Basic tab *switching* needs no JS beyond Bootstrap's own bundle
//! (`data-bs-toggle="tab"`); this module adds the grouped behavior: all
//! tabsets sharing a `data-group` sync when one is clicked, and the
//! choice persists in localStorage across pages and reloads
//! (bd-toc-tabset-titles-zq93gjvf).
//!
//! Ships unconditionally alongside Bootstrap JS (design decision 4 of
//! `claude-notes/plans/2026-08-17-tabset-panel-tabset.md`): the module
//! is ~100 lines, inert on pages without grouped tabsets, and shipping
//! it always keeps cross-page group persistence working without
//! content-sniffing here. Q1 likewise ships it on every page.

use std::path::PathBuf;

use async_trait::async_trait;
use quarto_sass::ThemeConfig;

use crate::artifact::{Artifact, ArtifactScope};
use crate::format::is_minimal_html;
use crate::stage::{
    EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};
use crate::trace_event;

/// The grouped-tabset sync module, embedded at compile time.
pub(crate) const TABSETS_JS: &[u8] =
    include_bytes!("../../../../../resources/js/tabsets/tabsets.js");

/// Filename used for the on-disk JS asset and as the leaf of the
/// artifact path.
const TABSETS_JS_FILENAME: &str = "tabsets.js";

/// Artifact key. Sorts after `js:bootstrap` (see `ApplyTemplateStage`'s
/// sorted-key `<script>` emission); load order is irrelevant here since
/// the module only touches the DOM on `pageshow`.
const TABSETS_JS_KEY: &str = "js:tabsets";

/// Inject the grouped-tabset sync module when Bootstrap is active.
///
/// Predicate matches [`BootstrapJsStage`](super::BootstrapJsStage)
/// exactly: tab markup only exists on Bootstrap-themed pages (the
/// `PanelTabsetTransform` gates on the same condition), so the module
/// travels with the Bootstrap bundle.
///
/// On success the stage stores a single Project-scoped `js:tabsets`
/// artifact. Path is `tabsets.js` for single-doc renders and
/// `quarto/tabsets.js` for multi-doc / website renders, mirroring
/// [`BootstrapJsStage`](super::BootstrapJsStage)'s layout.
pub struct TabsetsJsStage;

impl TabsetsJsStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TabsetsJsStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for TabsetsJsStage {
    fn name(&self) -> &str {
        "tabsets-js"
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

        // Same two skip cases as BootstrapJsStage — no Bootstrap CSS,
        // no tab markup, no sync module.
        let suppress_by_theme = match ThemeConfig::from_config_value(&doc.ast.meta) {
            Ok(c) => !c.ships_bootstrap(),
            Err(e) => {
                trace_event!(
                    ctx,
                    EventLevel::Warn,
                    "failed to parse theme config: {}, skipping tabsets JS",
                    e
                );
                true
            }
        };
        if suppress_by_theme || is_minimal_html(&doc.ast.meta) {
            trace_event!(
                ctx,
                EventLevel::Debug,
                "Bootstrap not in use, skipping tabsets JS"
            );
            return Ok(PipelineData::DocumentAst(doc));
        }

        let path = if ctx.project.is_single_file {
            PathBuf::from(TABSETS_JS_FILENAME)
        } else {
            PathBuf::from(format!("quarto/{}", TABSETS_JS_FILENAME))
        };

        ctx.artifacts.store(
            TABSETS_JS_KEY,
            Artifact::from_bytes(TABSETS_JS.to_vec(), "text/javascript")
                .with_path(path)
                .with_scope(ArtifactScope::Project),
        );

        trace_event!(
            ctx,
            EventLevel::Debug,
            "stored tabsets JS artifact ({} bytes)",
            TABSETS_JS.len()
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
        let document = DocumentInfo::from_path("/project/test.qmd");
        StageContext::new(runtime, Format::html(), project, document).unwrap()
    }

    fn make_doc_with_meta(meta: ConfigValue) -> DocumentAst {
        DocumentAst {
            path: PathBuf::from("/project/test.qmd"),
            ast: Pandoc {
                meta,
                ..Default::default()
            },
            ast_context: pampa::pandoc::ASTContext::default(),
            source_context: SourceContext::new(),
            warnings: vec![],
            recorded_includes: Vec::new(),
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

    fn empty_meta() -> ConfigValue {
        ConfigValue {
            value: ConfigValueKind::Map(vec![]),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        }
    }

    fn run_stage(meta: ConfigValue, is_single_file: bool) -> StageContext {
        let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> =
            Arc::new(quarto_system_runtime::NativeRuntime::new());
        let mut ctx = make_stage_context(runtime, is_single_file);
        let stage = TabsetsJsStage::new();
        let input = PipelineData::DocumentAst(make_doc_with_meta(meta));
        pollster::block_on(stage.run(input, &mut ctx)).expect("stage run");
        ctx
    }

    /// Default (Bootstrap) theme → the artifact is stored, single-doc
    /// path has no `quarto/` prefix.
    #[test]
    fn default_theme_stores_artifact_single_doc() {
        let ctx = run_stage(empty_meta(), true);
        let artifact = ctx
            .artifacts
            .get(TABSETS_JS_KEY)
            .expect("js:tabsets stored");
        assert_eq!(
            artifact.path.as_deref(),
            Some(std::path::Path::new("tabsets.js"))
        );
        assert_eq!(artifact.scope, ArtifactScope::Project);
    }

    /// Multi-doc renders place the file under `quarto/`.
    #[test]
    fn default_theme_stores_artifact_multi_doc() {
        let ctx = run_stage(empty_meta(), false);
        let artifact = ctx
            .artifacts
            .get(TABSETS_JS_KEY)
            .expect("js:tabsets stored");
        assert_eq!(
            artifact.path.as_deref(),
            Some(std::path::Path::new("quarto/tabsets.js"))
        );
    }

    /// `theme: none` suppresses the artifact (same gate as Bootstrap JS).
    #[test]
    fn theme_none_skips_artifact() {
        let ctx = run_stage(meta_with_theme("none"), true);
        assert!(ctx.artifacts.get(TABSETS_JS_KEY).is_none());
    }

    /// The embedded bytes are the sync module (sanity: localStorage key
    /// present, and it is not the ES-module form — no `export`).
    #[test]
    fn embedded_js_is_self_initializing_sync_module() {
        let js = std::str::from_utf8(TABSETS_JS).expect("utf8");
        assert!(js.contains("quarto-persistent-tabsets-data"));
        assert!(
            !js.contains("export function"),
            "the port must self-initialize, not export an init()"
        );
        assert!(js.contains("addEventListener(\"pageshow\""));
    }
}
