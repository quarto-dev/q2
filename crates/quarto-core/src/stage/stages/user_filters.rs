/*
 * stage/stages/user_filters.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Apply user-specified filters (Lua, JSON, citeproc) to the document.
 */

use async_trait::async_trait;

use crate::filter_resolve::{ResolvedFilters, resolve_filters};
use crate::stage::{
    EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};
use crate::trace_event;

/// Pipeline position for user filters.
#[derive(Debug, Clone, Copy)]
enum FilterPosition {
    /// Runs before `AstTransformsStage`
    Pre,
    /// Runs after `AstTransformsStage`
    Post,
}

/// Apply user-specified filters from the `filters` metadata key.
///
/// This stage reads the `filters` key from merged document metadata,
/// resolves filter paths, and applies them via pampa's filter engine.
///
/// Two instances are used in the pipeline:
/// - `UserFiltersStage::pre()` — runs before `AstTransformsStage`
/// - `UserFiltersStage::post()` — runs after `AstTransformsStage`
///
/// The `quarto` sentinel in the filters list controls which filters
/// run at each position. Filters can also use the `at` field to
/// specify an explicit entry point.
///
/// This stage is a no-op when no filters are configured for its position.
pub struct UserFiltersStage {
    position: FilterPosition,
}

impl UserFiltersStage {
    /// Create a stage that runs user filters before AST transforms.
    pub fn pre() -> Self {
        Self {
            position: FilterPosition::Pre,
        }
    }

    /// Create a stage that runs user filters after AST transforms.
    pub fn post() -> Self {
        Self {
            position: FilterPosition::Post,
        }
    }

    fn select_filters<'a>(
        &self,
        resolved: &'a ResolvedFilters,
    ) -> &'a [pampa::unified_filter::FilterSpec] {
        match self.position {
            FilterPosition::Pre => &resolved.pre,
            FilterPosition::Post => &resolved.post,
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl PipelineStage for UserFiltersStage {
    fn name(&self) -> &str {
        match self.position {
            FilterPosition::Pre => "user-filters-pre",
            FilterPosition::Post => "user-filters-post",
        }
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
        let PipelineData::DocumentAst(mut doc) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };

        // Resolve filters from merged metadata
        let document_dir = ctx
            .document
            .input
            .parent()
            .unwrap_or(std::path::Path::new("."));

        let resolved = resolve_filters(&doc.ast.meta, document_dir);

        let filters = self.select_filters(&resolved);
        if filters.is_empty() {
            return Ok(PipelineData::DocumentAst(doc));
        }

        trace_event!(
            ctx,
            EventLevel::Debug,
            "applying {} user {} filter(s): {}",
            filters.len(),
            match self.position {
                FilterPosition::Pre => "pre",
                FilterPosition::Post => "post",
            },
            filters
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );

        let target_format = ctx.format.identifier.as_str();

        let (new_ast, new_context, diagnostics) = pampa::unified_filter::apply_filters(
            doc.ast,
            doc.ast_context,
            filters,
            target_format,
            ctx.runtime.clone(),
        )
        .map_err(|e| PipelineError::stage_error(self.name(), e.to_string()))?;

        doc.ast = new_ast;
        doc.ast_context = new_context;
        ctx.diagnostics.extend(diagnostics);

        trace_event!(ctx, EventLevel::Debug, "user filters complete");

        Ok(PipelineData::DocumentAst(doc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectContext};
    use crate::stage::DocumentAst;
    use quarto_pandoc_types::ConfigValue;
    use quarto_pandoc_types::config_value::ConfigMapEntry;
    use quarto_pandoc_types::pandoc::Pandoc;
    use quarto_source_map::{SourceContext, SourceInfo};
    use quarto_system_runtime::TempDir;
    use std::path::PathBuf;
    use std::sync::Arc;

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

    fn make_ctx() -> StageContext {
        let runtime = Arc::new(MockRuntime);
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

    fn make_doc_ast(meta: ConfigValue) -> DocumentAst {
        DocumentAst {
            path: PathBuf::from("/project/test.qmd"),
            ast: Pandoc {
                meta,
                blocks: vec![].into(),
            },
            ast_context: pampa::pandoc::ASTContext::default(),
            source_context: SourceContext::new(),
            warnings: vec![],
        }
    }

    fn cv_str(s: &str) -> ConfigValue {
        ConfigValue::new_string(s, SourceInfo::default())
    }

    fn cv_array(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, SourceInfo::default())
    }

    fn cv_map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        ConfigValue::new_map(
            entries
                .into_iter()
                .map(|(k, v)| ConfigMapEntry {
                    key: k.to_string(),
                    key_source: SourceInfo::default(),
                    value: v,
                })
                .collect(),
            SourceInfo::default(),
        )
    }

    #[tokio::test]
    async fn pre_stage_no_filters_is_passthrough() {
        let mut ctx = make_ctx();
        let stage = UserFiltersStage::pre();
        let doc = make_doc_ast(cv_map(vec![]));
        let input = PipelineData::DocumentAst(doc);
        let output = stage.run(input, &mut ctx).await.unwrap();
        assert!(output.into_document_ast().is_some());
    }

    #[tokio::test]
    async fn post_stage_no_filters_is_passthrough() {
        let mut ctx = make_ctx();
        let stage = UserFiltersStage::post();
        let doc = make_doc_ast(cv_map(vec![]));
        let input = PipelineData::DocumentAst(doc);
        let output = stage.run(input, &mut ctx).await.unwrap();
        assert!(output.into_document_ast().is_some());
    }

    #[tokio::test]
    async fn pre_stage_with_filters_key_but_empty_is_passthrough() {
        let mut ctx = make_ctx();
        let stage = UserFiltersStage::pre();
        let meta = cv_map(vec![("filters", cv_array(vec![]))]);
        let doc = make_doc_ast(meta);
        let input = PipelineData::DocumentAst(doc);
        let output = stage.run(input, &mut ctx).await.unwrap();
        assert!(output.into_document_ast().is_some());
    }

    #[tokio::test]
    async fn post_stage_ignores_pre_only_filters() {
        // Filters without sentinel all go to Pre, so Post stage should be a no-op
        let mut ctx = make_ctx();
        let stage = UserFiltersStage::post();
        let meta = cv_map(vec![("filters", cv_array(vec![cv_str("test.lua")]))]);
        let doc = make_doc_ast(meta);
        let input = PipelineData::DocumentAst(doc);
        let output = stage.run(input, &mut ctx).await.unwrap();
        assert!(output.into_document_ast().is_some());
    }

    #[test]
    fn stage_names_are_distinct() {
        let pre = UserFiltersStage::pre();
        let post = UserFiltersStage::post();
        assert_eq!(pre.name(), "user-filters-pre");
        assert_eq!(post.name(), "user-filters-post");
        assert_ne!(pre.name(), post.name());
    }

    #[test]
    fn stage_kinds_are_document_ast() {
        let stage = UserFiltersStage::pre();
        assert_eq!(stage.input_kind(), PipelineDataKind::DocumentAst);
        assert_eq!(stage.output_kind(), PipelineDataKind::DocumentAst);
    }
}
