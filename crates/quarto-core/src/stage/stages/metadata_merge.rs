/*
 * stage/stages/metadata_merge.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Merge project, directory, document, and runtime metadata.
 */

//! Merge metadata layers into a single flattened config.
//!
//! This stage resolves the full metadata hierarchy for the target format:
//!
//! 1. Project top-level settings (`_quarto.yml`)
//! 2. Project format-specific settings (`format.{target}.*`)
//! 3. Directory `_metadata.yml` layers (root → leaf, deeper wins)
//! 4. Document top-level settings (YAML front matter)
//! 5. Document format-specific settings (`format.{target}.*`)
//! 6. Runtime metadata (e.g., `--metadata` flags, WASM preview settings)
//!
//! After this stage, `doc.ast.meta` contains the fully merged and
//! format-flattened config. Downstream stages (AST transforms, rendering)
//! can read metadata without knowing about the layering.

use async_trait::async_trait;
use quarto_config::{MergedConfig, resolve_format_config};
use quarto_pandoc_types::{ConfigMapEntry, ConfigValue, ConfigValueKind, MergeOp};
use quarto_source_map::SourceInfo;

use crate::project::{adjust_paths_to_document_dir, directory_metadata_for_document};
use crate::stage::{
    EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};
use crate::trace_event;

/// Convert a `serde_json::Value` to a `ConfigValue`.
///
/// Used for converting runtime metadata (which uses `serde_json::Value` to avoid
/// coupling `quarto-system-runtime` to `quarto-pandoc-types`) into the `ConfigValue`
/// type needed by the merge pipeline.
fn json_to_config_value(value: &serde_json::Value) -> ConfigValue {
    use yaml_rust2::Yaml;

    let source_info = SourceInfo::default();
    let kind = match value {
        serde_json::Value::Null => ConfigValueKind::Scalar(Yaml::Null),
        serde_json::Value::Bool(b) => ConfigValueKind::Scalar(Yaml::Boolean(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                ConfigValueKind::Scalar(Yaml::Integer(i))
            } else if let Some(f) = n.as_f64() {
                ConfigValueKind::Scalar(Yaml::Real(f.to_string()))
            } else {
                ConfigValueKind::Scalar(Yaml::String(n.to_string()))
            }
        }
        serde_json::Value::String(s) => ConfigValueKind::Scalar(Yaml::String(s.clone())),
        serde_json::Value::Array(arr) => {
            let items: Vec<ConfigValue> = arr.iter().map(json_to_config_value).collect();
            ConfigValueKind::Array(items)
        }
        serde_json::Value::Object(obj) => {
            let entries: Vec<ConfigMapEntry> = obj
                .iter()
                .map(|(k, v)| ConfigMapEntry {
                    key: k.clone(),
                    key_source: SourceInfo::default(),
                    value: json_to_config_value(v),
                })
                .collect();
            ConfigValueKind::Map(entries)
        }
    };
    ConfigValue {
        value: kind,
        source_info,
        merge_op: MergeOp::default(),
    }
}

/// Merge project, directory, document, and runtime metadata.
///
/// This stage takes a `DocumentAst` and replaces `doc.ast.meta` with the
/// fully merged and format-flattened config. It does not modify the AST
/// structure — only the metadata map.
///
/// # Input
///
/// - `DocumentAst` - Parsed Pandoc AST with source context
///
/// # Output
///
/// - `DocumentAst` - Same AST with merged metadata in `doc.ast.meta`
pub struct MetadataMergeStage;

impl MetadataMergeStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MetadataMergeStage {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl PipelineStage for MetadataMergeStage {
    fn name(&self) -> &str {
        "metadata-merge"
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

        // Merge project config, directory metadata, document metadata, and
        // runtime metadata. All metadata layers are flattened for the target
        // format before merging. This extracts format-specific settings
        // (e.g., format.html.*) and merges them with top-level settings.
        //
        // Precedence (lowest to highest):
        // 1. Project top-level settings
        // 2. Project format-specific settings (format.{target}.*)
        // 3. Directory _metadata.yml layers (root → leaf, deeper wins)
        // 4. Document top-level settings
        // 5. Document format-specific settings (format.{target}.*)
        // 6. Runtime metadata (e.g., --metadata flags, WASM preview settings)
        let runtime_meta_json = ctx.runtime.runtime_metadata();
        let target_format = ctx.format.identifier.as_str();

        // Layer 1: Project metadata (flattened for format)
        // Adjust !path values to be relative to document directory
        // (project config paths are relative to project root)
        let document_dir = doc
            .path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| ctx.project.dir.clone());
        let project_layer = ctx.project.config.metadata.as_ref().map(|m| {
            let mut flattened = resolve_format_config(m, target_format);
            adjust_paths_to_document_dir(&mut flattened, &ctx.project.dir, &document_dir);
            flattened
        });

        // Layer 2: Directory metadata layers (each flattened for format)
        let dir_layers: Vec<_> = if !ctx.project.is_single_file {
            directory_metadata_for_document(&ctx.project, &ctx.document.input, ctx.runtime.as_ref())
                .unwrap_or_default()
                .into_iter()
                .map(|m| resolve_format_config(&m, target_format))
                .collect()
        } else {
            vec![]
        };

        // Layer 3: Document metadata (flattened for format)
        let doc_layer = resolve_format_config(&doc.ast.meta, target_format);

        // Layer 4: Runtime metadata (flattened for format)
        let runtime_layer = runtime_meta_json
            .as_ref()
            .map(|json| resolve_format_config(&json_to_config_value(json), target_format));

        // Build merge layers: project → dir[0] → dir[1] → ... → document → runtime
        let mut layers: Vec<&ConfigValue> = Vec::new();
        if let Some(ref proj) = project_layer {
            layers.push(proj);
        }
        for dir_meta in &dir_layers {
            layers.push(dir_meta);
        }
        layers.push(&doc_layer);
        if let Some(ref rt) = runtime_layer {
            layers.push(rt);
        }

        // Merge all layers
        let layer_count = layers.len();
        let merged = MergedConfig::new(layers);
        if let Ok(materialized) = merged.materialize() {
            let has_runtime = if runtime_layer.is_some() {
                " + runtime"
            } else {
                ""
            };
            trace_event!(
                ctx,
                EventLevel::Debug,
                "merged {} metadata layers for format '{}' (project + {} dir + doc{})",
                layer_count,
                target_format,
                dir_layers.len(),
                has_runtime
            );
            doc.ast.meta = materialized;
        }
        // Note: If materialization fails (shouldn't happen with well-formed configs),
        // we silently continue with the original document metadata.

        Ok(PipelineData::DocumentAst(doc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::stage::DocumentAst;
    use quarto_pandoc_types::pandoc::Pandoc;
    use quarto_source_map::SourceContext;
    use quarto_system_runtime::TempDir;
    use std::path::PathBuf;
    use std::sync::Arc;

    // Mock runtime for testing
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

    /// Helper to create a ConfigValue map from key-value pairs
    fn config_map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        use quarto_pandoc_types::ConfigMapEntry;
        let map_entries: Vec<ConfigMapEntry> = entries
            .into_iter()
            .map(|(k, v)| ConfigMapEntry {
                key: k.to_string(),
                key_source: SourceInfo::default(),
                value: v,
            })
            .collect();
        ConfigValue::new_map(map_entries, SourceInfo::default())
    }

    /// Helper to create a scalar string ConfigValue
    fn config_str(s: &str) -> ConfigValue {
        ConfigValue::new_string(s, SourceInfo::default())
    }

    /// Helper to create a scalar bool ConfigValue
    fn config_bool(b: bool) -> ConfigValue {
        ConfigValue::new_bool(b, SourceInfo::default())
    }

    // ============================================================================
    // Project Metadata Merging Tests
    // ============================================================================

    #[tokio::test]
    async fn test_project_metadata_merging_basic() {
        // Project has title, document has author
        // Result should have both
        let runtime = Arc::new(MockRuntime);

        let project_metadata = config_map(vec![("title", config_str("Project Title"))]);

        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::with_metadata(project_metadata),
            is_single_file: false,
            files: vec![],
            output_dir: PathBuf::from("/project"),
        };
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();

        let mut ctx = StageContext::new(runtime, format, project, doc).unwrap();
        let stage = MetadataMergeStage::new();

        // Document has author metadata
        let doc_metadata = config_map(vec![("author", config_str("John Doe"))]);
        let doc_ast = DocumentAst {
            path: PathBuf::from("/project/test.qmd"),
            ast: Pandoc {
                meta: doc_metadata,
                ..Default::default()
            },
            ast_context: pampa::pandoc::ASTContext::default(),
            source_context: SourceContext::new(),
            warnings: vec![],
        };

        let input = PipelineData::DocumentAst(doc_ast);
        let output = stage.run(input, &mut ctx).await.unwrap();
        let result = output.into_document_ast().unwrap();

        // Both title from project and author from document should be present
        assert!(result.ast.meta.get("title").is_some());
        assert!(result.ast.meta.get("author").is_some());
        assert_eq!(
            result.ast.meta.get("title").unwrap().as_str(),
            Some("Project Title")
        );
        assert_eq!(
            result.ast.meta.get("author").unwrap().as_str(),
            Some("John Doe")
        );
    }

    #[tokio::test]
    async fn test_project_metadata_document_overrides_project() {
        // Both project and document have title
        // Document title should win
        let runtime = Arc::new(MockRuntime);

        let project_metadata = config_map(vec![("title", config_str("Project Title"))]);

        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::with_metadata(project_metadata),
            is_single_file: false,
            files: vec![],
            output_dir: PathBuf::from("/project"),
        };
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();

        let mut ctx = StageContext::new(runtime, format, project, doc).unwrap();
        let stage = MetadataMergeStage::new();

        // Document also has title
        let doc_metadata = config_map(vec![("title", config_str("Document Title"))]);
        let doc_ast = DocumentAst {
            path: PathBuf::from("/project/test.qmd"),
            ast: Pandoc {
                meta: doc_metadata,
                ..Default::default()
            },
            ast_context: pampa::pandoc::ASTContext::default(),
            source_context: SourceContext::new(),
            warnings: vec![],
        };

        let input = PipelineData::DocumentAst(doc_ast);
        let output = stage.run(input, &mut ctx).await.unwrap();
        let result = output.into_document_ast().unwrap();

        // Document title should override project title
        assert_eq!(
            result.ast.meta.get("title").unwrap().as_str(),
            Some("Document Title")
        );
    }

    #[tokio::test]
    async fn test_project_format_specific_settings_inherited() {
        // Project has format.html.toc: true
        // Document should inherit toc setting when rendering to HTML
        let runtime = Arc::new(MockRuntime);

        let project_metadata = config_map(vec![(
            "format",
            config_map(vec![("html", config_map(vec![("toc", config_bool(true))]))]),
        )]);

        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::with_metadata(project_metadata),
            is_single_file: false,
            files: vec![],
            output_dir: PathBuf::from("/project"),
        };
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();

        let mut ctx = StageContext::new(runtime, format, project, doc).unwrap();
        let stage = MetadataMergeStage::new();

        // Document has no metadata
        let doc_ast = DocumentAst {
            path: PathBuf::from("/project/test.qmd"),
            ast: Pandoc::default(),
            ast_context: pampa::pandoc::ASTContext::default(),
            source_context: SourceContext::new(),
            warnings: vec![],
        };

        let input = PipelineData::DocumentAst(doc_ast);
        let output = stage.run(input, &mut ctx).await.unwrap();
        let result = output.into_document_ast().unwrap();

        // toc should be inherited from project's format.html settings
        assert_eq!(result.ast.meta.get("toc").unwrap().as_bool(), Some(true));
        // format key should be removed (flattened)
        assert!(result.ast.meta.get("format").is_none());
    }

    #[tokio::test]
    async fn test_document_format_specific_overrides_project() {
        // Project has format.html.toc: true
        // Document has format.html.toc: false
        // Document setting should win
        let runtime = Arc::new(MockRuntime);

        let project_metadata = config_map(vec![(
            "format",
            config_map(vec![("html", config_map(vec![("toc", config_bool(true))]))]),
        )]);

        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::with_metadata(project_metadata),
            is_single_file: false,
            files: vec![],
            output_dir: PathBuf::from("/project"),
        };
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();

        let mut ctx = StageContext::new(runtime, format, project, doc).unwrap();
        let stage = MetadataMergeStage::new();

        // Document has format.html.toc: false
        let doc_metadata = config_map(vec![(
            "format",
            config_map(vec![(
                "html",
                config_map(vec![("toc", config_bool(false))]),
            )]),
        )]);
        let doc_ast = DocumentAst {
            path: PathBuf::from("/project/test.qmd"),
            ast: Pandoc {
                meta: doc_metadata,
                ..Default::default()
            },
            ast_context: pampa::pandoc::ASTContext::default(),
            source_context: SourceContext::new(),
            warnings: vec![],
        };

        let input = PipelineData::DocumentAst(doc_ast);
        let output = stage.run(input, &mut ctx).await.unwrap();
        let result = output.into_document_ast().unwrap();

        // Document's toc: false should override project's toc: true
        assert_eq!(result.ast.meta.get("toc").unwrap().as_bool(), Some(false));
    }

    #[tokio::test]
    async fn test_non_target_format_settings_ignored() {
        // Project has format.pdf.documentclass
        // Should be ignored when rendering to HTML
        let runtime = Arc::new(MockRuntime);

        let project_metadata = config_map(vec![
            ("title", config_str("My Doc")),
            (
                "format",
                config_map(vec![(
                    "pdf",
                    config_map(vec![("documentclass", config_str("article"))]),
                )]),
            ),
        ]);

        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::with_metadata(project_metadata),
            is_single_file: false,
            files: vec![],
            output_dir: PathBuf::from("/project"),
        };
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html(); // Rendering to HTML, not PDF

        let mut ctx = StageContext::new(runtime, format, project, doc).unwrap();
        let stage = MetadataMergeStage::new();

        let doc_ast = DocumentAst {
            path: PathBuf::from("/project/test.qmd"),
            ast: Pandoc::default(),
            ast_context: pampa::pandoc::ASTContext::default(),
            source_context: SourceContext::new(),
            warnings: vec![],
        };

        let input = PipelineData::DocumentAst(doc_ast);
        let output = stage.run(input, &mut ctx).await.unwrap();
        let result = output.into_document_ast().unwrap();

        // title should be present
        assert_eq!(
            result.ast.meta.get("title").unwrap().as_str(),
            Some("My Doc")
        );
        // documentclass from pdf format should NOT be present
        assert!(result.ast.meta.get("documentclass").is_none());
        // format key should be removed
        assert!(result.ast.meta.get("format").is_none());
    }

    #[tokio::test]
    async fn test_top_level_overridden_by_format_specific() {
        // Project has top-level toc: true and format.html.toc: false
        // format.html.toc should win
        let runtime = Arc::new(MockRuntime);

        let project_metadata = config_map(vec![
            ("toc", config_bool(true)),
            (
                "format",
                config_map(vec![(
                    "html",
                    config_map(vec![("toc", config_bool(false))]),
                )]),
            ),
        ]);

        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::with_metadata(project_metadata),
            is_single_file: false,
            files: vec![],
            output_dir: PathBuf::from("/project"),
        };
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();

        let mut ctx = StageContext::new(runtime, format, project, doc).unwrap();
        let stage = MetadataMergeStage::new();

        let doc_ast = DocumentAst {
            path: PathBuf::from("/project/test.qmd"),
            ast: Pandoc::default(),
            ast_context: pampa::pandoc::ASTContext::default(),
            source_context: SourceContext::new(),
            warnings: vec![],
        };

        let input = PipelineData::DocumentAst(doc_ast);
        let output = stage.run(input, &mut ctx).await.unwrap();
        let result = output.into_document_ast().unwrap();

        // format.html.toc: false should override top-level toc: true
        assert_eq!(result.ast.meta.get("toc").unwrap().as_bool(), Some(false));
    }

    // ============================================================================
    // Runtime Metadata Tests
    // ============================================================================

    /// Mock runtime that returns configurable runtime metadata
    struct MockRuntimeWithMetadata {
        metadata: Option<serde_json::Value>,
    }

    impl MockRuntimeWithMetadata {
        fn new(metadata: serde_json::Value) -> Self {
            Self {
                metadata: Some(metadata),
            }
        }
    }

    impl quarto_system_runtime::SystemRuntime for MockRuntimeWithMetadata {
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
        fn runtime_metadata(&self) -> Option<serde_json::Value> {
            self.metadata.clone()
        }
    }

    #[tokio::test]
    async fn test_runtime_metadata_applied() {
        // Runtime provides source-location, document has title
        // Both should appear in merged result
        let runtime = Arc::new(MockRuntimeWithMetadata::new(serde_json::json!({
            "source-location": "full"
        })));

        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::with_metadata(config_map(vec![])),
            is_single_file: false,
            files: vec![],
            output_dir: PathBuf::from("/project"),
        };
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();

        let mut ctx = StageContext::new(runtime, format, project, doc).unwrap();
        let stage = MetadataMergeStage::new();

        let doc_metadata = config_map(vec![("title", config_str("Hello"))]);
        let doc_ast = DocumentAst {
            path: PathBuf::from("/project/test.qmd"),
            ast: Pandoc {
                meta: doc_metadata,
                ..Default::default()
            },
            ast_context: pampa::pandoc::ASTContext::default(),
            source_context: SourceContext::new(),
            warnings: vec![],
        };

        let input = PipelineData::DocumentAst(doc_ast);
        let output = stage.run(input, &mut ctx).await.unwrap();
        let result = output.into_document_ast().unwrap();

        assert_eq!(
            result.ast.meta.get("title").unwrap().as_str(),
            Some("Hello")
        );
        assert_eq!(
            result.ast.meta.get("source-location").unwrap().as_str(),
            Some("full")
        );
    }

    #[tokio::test]
    async fn test_runtime_metadata_overrides_document() {
        // Runtime sets toc: false, document sets toc: true
        // Runtime should win (highest precedence)
        let runtime = Arc::new(MockRuntimeWithMetadata::new(serde_json::json!({
            "toc": false
        })));

        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::with_metadata(config_map(vec![])),
            is_single_file: false,
            files: vec![],
            output_dir: PathBuf::from("/project"),
        };
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();

        let mut ctx = StageContext::new(runtime, format, project, doc).unwrap();
        let stage = MetadataMergeStage::new();

        let doc_metadata = config_map(vec![("toc", config_bool(true))]);
        let doc_ast = DocumentAst {
            path: PathBuf::from("/project/test.qmd"),
            ast: Pandoc {
                meta: doc_metadata,
                ..Default::default()
            },
            ast_context: pampa::pandoc::ASTContext::default(),
            source_context: SourceContext::new(),
            warnings: vec![],
        };

        let input = PipelineData::DocumentAst(doc_ast);
        let output = stage.run(input, &mut ctx).await.unwrap();
        let result = output.into_document_ast().unwrap();

        assert_eq!(result.ast.meta.get("toc").unwrap().as_bool(), Some(false));
    }

    #[tokio::test]
    async fn test_runtime_metadata_overrides_project() {
        // Project sets toc: true, runtime sets toc: false
        // Runtime should win
        let runtime = Arc::new(MockRuntimeWithMetadata::new(serde_json::json!({
            "toc": false
        })));

        let project_metadata = config_map(vec![("toc", config_bool(true))]);
        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::with_metadata(project_metadata),
            is_single_file: false,
            files: vec![],
            output_dir: PathBuf::from("/project"),
        };
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();

        let mut ctx = StageContext::new(runtime, format, project, doc).unwrap();
        let stage = MetadataMergeStage::new();

        let doc_ast = DocumentAst {
            path: PathBuf::from("/project/test.qmd"),
            ast: Pandoc::default(),
            ast_context: pampa::pandoc::ASTContext::default(),
            source_context: SourceContext::new(),
            warnings: vec![],
        };

        let input = PipelineData::DocumentAst(doc_ast);
        let output = stage.run(input, &mut ctx).await.unwrap();
        let result = output.into_document_ast().unwrap();

        assert_eq!(result.ast.meta.get("toc").unwrap().as_bool(), Some(false));
    }

    #[tokio::test]
    async fn test_runtime_metadata_format_specific() {
        // Runtime provides format.html.source-location: full
        // Should be flattened to source-location: full in merged result
        let runtime = Arc::new(MockRuntimeWithMetadata::new(serde_json::json!({
            "format": {
                "html": {
                    "source-location": "full"
                }
            }
        })));

        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::with_metadata(config_map(vec![])),
            is_single_file: false,
            files: vec![],
            output_dir: PathBuf::from("/project"),
        };
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();

        let mut ctx = StageContext::new(runtime, format, project, doc).unwrap();
        let stage = MetadataMergeStage::new();

        let doc_ast = DocumentAst {
            path: PathBuf::from("/project/test.qmd"),
            ast: Pandoc::default(),
            ast_context: pampa::pandoc::ASTContext::default(),
            source_context: SourceContext::new(),
            warnings: vec![],
        };

        let input = PipelineData::DocumentAst(doc_ast);
        let output = stage.run(input, &mut ctx).await.unwrap();
        let result = output.into_document_ast().unwrap();

        assert_eq!(
            result.ast.meta.get("source-location").unwrap().as_str(),
            Some("full")
        );
        // format key should be removed (flattened)
        assert!(result.ast.meta.get("format").is_none());
    }

    #[tokio::test]
    async fn test_runtime_metadata_none_no_change() {
        // Runtime returns None — should behave exactly like existing tests
        let runtime = Arc::new(MockRuntime); // MockRuntime has default None

        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::with_metadata(config_map(vec![])),
            is_single_file: false,
            files: vec![],
            output_dir: PathBuf::from("/project"),
        };
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();

        let mut ctx = StageContext::new(runtime, format, project, doc).unwrap();
        let stage = MetadataMergeStage::new();

        let doc_metadata = config_map(vec![("title", config_str("Hello"))]);
        let doc_ast = DocumentAst {
            path: PathBuf::from("/project/test.qmd"),
            ast: Pandoc {
                meta: doc_metadata,
                ..Default::default()
            },
            ast_context: pampa::pandoc::ASTContext::default(),
            source_context: SourceContext::new(),
            warnings: vec![],
        };

        let input = PipelineData::DocumentAst(doc_ast);
        let output = stage.run(input, &mut ctx).await.unwrap();
        let result = output.into_document_ast().unwrap();

        assert_eq!(
            result.ast.meta.get("title").unwrap().as_str(),
            Some("Hello")
        );
    }

    #[tokio::test]
    async fn test_runtime_metadata_without_project_config() {
        // No project config (config: None), but runtime provides metadata
        // Runtime metadata should still be merged into document metadata
        let runtime = Arc::new(MockRuntimeWithMetadata::new(serde_json::json!({
            "source-location": "full"
        })));

        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(), // No project config
            is_single_file: true,
            files: vec![],
            output_dir: PathBuf::from("/project"),
        };
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();

        let mut ctx = StageContext::new(runtime, format, project, doc).unwrap();
        let stage = MetadataMergeStage::new();

        let doc_metadata = config_map(vec![("title", config_str("Hello"))]);
        let doc_ast = DocumentAst {
            path: PathBuf::from("/project/test.qmd"),
            ast: Pandoc {
                meta: doc_metadata,
                ..Default::default()
            },
            ast_context: pampa::pandoc::ASTContext::default(),
            source_context: SourceContext::new(),
            warnings: vec![],
        };

        let input = PipelineData::DocumentAst(doc_ast);
        let output = stage.run(input, &mut ctx).await.unwrap();
        let result = output.into_document_ast().unwrap();

        assert_eq!(
            result.ast.meta.get("title").unwrap().as_str(),
            Some("Hello")
        );
        assert_eq!(
            result.ast.meta.get("source-location").unwrap().as_str(),
            Some("full")
        );
    }

    // ============================================================================
    // Single-file format flattening bug
    // ============================================================================

    #[tokio::test]
    async fn test_single_file_format_flattening() {
        // BUG: Single-file renders (config: None, no runtime metadata) skip
        // the merge gate entirely, so format.html.toc stays nested instead
        // of being flattened to top-level toc.
        let runtime = Arc::new(MockRuntime);

        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(), // Single-file render, no project config
            is_single_file: true,
            files: vec![],
            output_dir: PathBuf::from("/project"),
        };
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();

        let mut ctx = StageContext::new(runtime, format, project, doc).unwrap();
        let stage = MetadataMergeStage::new();

        // Document has format.html.toc: true in frontmatter
        let doc_metadata = config_map(vec![(
            "format",
            config_map(vec![("html", config_map(vec![("toc", config_bool(true))]))]),
        )]);
        let doc_ast = DocumentAst {
            path: PathBuf::from("/project/test.qmd"),
            ast: Pandoc {
                meta: doc_metadata,
                ..Default::default()
            },
            ast_context: pampa::pandoc::ASTContext::default(),
            source_context: SourceContext::new(),
            warnings: vec![],
        };

        let input = PipelineData::DocumentAst(doc_ast);
        let output = stage.run(input, &mut ctx).await.unwrap();
        let result = output.into_document_ast().unwrap();

        // After merge, toc should be flattened to top level
        assert_eq!(
            result.ast.meta.get("toc").unwrap().as_bool(),
            Some(true),
            "format.html.toc should be flattened to top-level toc for single-file renders"
        );
        // format key should be removed
        assert!(
            result.ast.meta.get("format").is_none(),
            "format key should be removed after flattening"
        );
    }
}
