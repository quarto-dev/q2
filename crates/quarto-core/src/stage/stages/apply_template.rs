/*
 * stage/stages/apply_template.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Apply HTML template to rendered body.
 */

//! Apply HTML template to rendered body.
//!
//! This stage wraps the rendered HTML body with a complete HTML document
//! using the template engine.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use quarto_doctemplate::{ChainedResolver, MemoryResolver, Template};

use crate::artifact::Artifact;
use crate::pipeline::DEFAULT_CSS_ARTIFACT_PATH;
use crate::resources::DEFAULT_CSS;
use crate::stage::{
    EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};
use crate::template;
use crate::template::RuntimeResolver;
use crate::trace_event;

/// Configuration for the ApplyTemplateStage.
#[derive(Default)]
pub struct ApplyTemplateConfig {
    /// CSS paths to include in the document (relative to the output HTML).
    pub css_paths: Vec<String>,
}

impl ApplyTemplateConfig {
    /// Create a new default configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set custom CSS paths.
    pub fn with_css_paths(mut self, paths: Vec<String>) -> Self {
        self.css_paths = paths;
        self
    }
}

/// Apply HTML template to rendered body.
///
/// This stage:
/// 1. Takes a RenderedOutput with HTML body content
/// 2. Applies the HTML template with metadata
/// 3. Stores the default CSS as an artifact
/// 4. Returns a RenderedOutput with the complete HTML document
///
/// # Configuration
///
/// - `css_paths`: CSS paths to include in the document
///
/// # Input
///
/// - `RenderedOutput` - HTML body content with format metadata
///
/// # Output
///
/// - `RenderedOutput` - Complete HTML document
///
/// # Artifacts
///
/// This stage stores the default CSS at `DEFAULT_CSS_ARTIFACT_PATH`
/// for WASM consumption.
pub struct ApplyTemplateStage {
    config: ApplyTemplateConfig,
}

impl ApplyTemplateStage {
    /// Create a new ApplyTemplateStage with default configuration.
    pub fn new() -> Self {
        Self {
            config: ApplyTemplateConfig::default(),
        }
    }

    /// Create with custom configuration.
    pub fn with_config(config: ApplyTemplateConfig) -> Self {
        Self { config }
    }
}

impl Default for ApplyTemplateStage {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl PipelineStage for ApplyTemplateStage {
    fn name(&self) -> &str {
        "apply-template"
    }

    fn input_kind(&self) -> PipelineDataKind {
        PipelineDataKind::RenderedOutput
    }

    fn output_kind(&self) -> PipelineDataKind {
        PipelineDataKind::RenderedOutput
    }

    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let PipelineData::RenderedOutput(mut rendered) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };

        trace_event!(
            ctx,
            EventLevel::Debug,
            "applying template to {} bytes of body",
            rendered.content.len()
        );

        // Store CSS artifact for WASM consumption (only if not already set
        // by CompileThemeCssStage, which produces themed CSS)
        if ctx.artifacts.get("css:default").is_none() {
            ctx.artifacts.store(
                "css:default",
                Artifact::from_string(DEFAULT_CSS, "text/css")
                    .with_path(PathBuf::from(DEFAULT_CSS_ARTIFACT_PATH)),
            );
        }

        // Get metadata from the rendered output
        let metadata = rendered.metadata.clone();

        // CSS paths for the template context
        let css_paths: Vec<String> = if self.config.css_paths.is_empty() {
            vec![DEFAULT_CSS_ARTIFACT_PATH.to_string()]
        } else {
            self.config.css_paths.clone()
        };

        // Extract custom template/partials from merged metadata
        let custom_template_path = metadata.get("template").and_then(|v| v.as_str());
        let partial_paths: Vec<String> = metadata
            .get("template-partials")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        // Apply template: metadata-driven selection
        let document_dir = rendered
            .input_path
            .parent()
            .unwrap_or_else(|| Path::new("."));

        let html = match custom_template_path {
            Some(template_path) => {
                // Custom template from extension or document metadata
                let abs_path = document_dir.join(template_path);
                let template_content = ctx.runtime.file_read_string(&abs_path).map_err(|e| {
                    PipelineError::stage_error(
                        self.name(),
                        format!("failed to read template '{}': {}", abs_path.display(), e),
                    )
                })?;

                let compiled = if partial_paths.is_empty() {
                    // Custom template, no explicit partials: use RuntimeResolver
                    let resolver = RuntimeResolver::new(ctx.runtime.as_ref());
                    Template::compile_with_resolver(&template_content, &abs_path, &resolver, 0)
                        .map_err(|e| {
                            PipelineError::stage_error(
                                self.name(),
                                format!(
                                    "failed to compile template '{}': {}",
                                    abs_path.display(),
                                    e
                                ),
                            )
                        })?
                } else {
                    // Custom template + explicit partials: chain MemoryResolver → RuntimeResolver
                    let memory = build_partial_resolver(
                        &partial_paths,
                        document_dir,
                        ctx.runtime.as_ref(),
                        self.name(),
                    )?;
                    let runtime = RuntimeResolver::new(ctx.runtime.as_ref());
                    let chained = ChainedResolver::new(memory, runtime);
                    Template::compile_with_resolver(&template_content, &abs_path, &chained, 0)
                        .map_err(|e| {
                            PipelineError::stage_error(
                                self.name(),
                                format!(
                                    "failed to compile template '{}': {}",
                                    abs_path.display(),
                                    e
                                ),
                            )
                        })?
                };

                template::render_with_compiled_template(
                    &compiled,
                    &rendered.content,
                    &metadata,
                    &css_paths,
                )
                .map_err(|e| PipelineError::stage_error(self.name(), e.to_string()))?
            }
            None if !partial_paths.is_empty() => {
                // No custom template, but explicit partials: compile built-in with partials
                let memory = build_partial_resolver(
                    &partial_paths,
                    document_dir,
                    ctx.runtime.as_ref(),
                    self.name(),
                )?;
                let compiled = template::compile_builtin_template_with_partials(&metadata, &memory)
                    .map_err(|e| PipelineError::stage_error(self.name(), e.to_string()))?;

                template::render_with_compiled_template(
                    &compiled,
                    &rendered.content,
                    &metadata,
                    &css_paths,
                )
                .map_err(|e| PipelineError::stage_error(self.name(), e.to_string()))?
            }
            None => {
                // No custom template, no partials: existing behavior
                template::render_with_format(
                    &rendered.content,
                    &metadata,
                    &rendered.format,
                    &css_paths,
                )
                .map_err(|e| PipelineError::stage_error(self.name(), e.to_string()))?
            }
        };

        trace_event!(
            ctx,
            EventLevel::Debug,
            "template applied, {} bytes of HTML",
            html.len()
        );

        // Update content with full HTML document
        rendered.content = html;

        Ok(PipelineData::RenderedOutput(rendered))
    }
}

/// Build a `MemoryResolver` from explicit partial paths, reading content via runtime.
///
/// Partials are keyed by file stem (e.g., `title-block.html` → `"title-block"`).
fn build_partial_resolver(
    partial_paths: &[String],
    document_dir: &Path,
    runtime: &dyn quarto_system_runtime::SystemRuntime,
    stage_name: &str,
) -> Result<MemoryResolver, PipelineError> {
    let mut resolver = MemoryResolver::new();
    for path_str in partial_paths {
        let path = Path::new(path_str);
        let abs_path = document_dir.join(path);
        let content = runtime.file_read_string(&abs_path).map_err(|e| {
            PipelineError::stage_error(
                stage_name,
                format!(
                    "failed to read template partial '{}': {}",
                    abs_path.display(),
                    e
                ),
            )
        })?;
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(path_str);
        resolver.add(name, content);
    }
    Ok(resolver)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectContext};
    use crate::stage::RenderedOutput;
    use quarto_system_runtime::TempDir;
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

    #[tokio::test]
    async fn test_apply_template_basic() {
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

        let mut ctx = StageContext::new(runtime, format.clone(), project, doc).unwrap();

        let stage = ApplyTemplateStage::new();

        let rendered = RenderedOutput {
            input_path: PathBuf::from("/project/test.qmd"),
            output_path: PathBuf::from("/project/test.html"),
            format,
            content: "<p>Hello, world!</p>".to_string(),
            is_intermediate: false,
            supporting_files: vec![],
            metadata: quarto_pandoc_types::ConfigValue::null(
                quarto_source_map::SourceInfo::default(),
            ),
        };

        let input = PipelineData::RenderedOutput(rendered);
        let output = stage.run(input, &mut ctx).await.unwrap();

        let result = output
            .into_rendered_output()
            .expect("Should be RenderedOutput");
        assert!(result.content.contains("<!DOCTYPE html>"));
        assert!(result.content.contains("<p>Hello, world!</p>"));
        // Should have the default CSS artifact stored
        assert!(ctx.artifacts.get("css:default").is_some());
    }

    fn make_rendered_output_with_metadata(
        input_path: PathBuf,
        metadata: quarto_pandoc_types::ConfigValue,
    ) -> RenderedOutput {
        RenderedOutput {
            input_path: input_path.clone(),
            output_path: input_path.with_extension("html"),
            format: Format::html(),
            content: "<p>Hello</p>".to_string(),
            is_intermediate: false,
            supporting_files: vec![],
            metadata,
        }
    }

    fn meta_with_template(template_path: &str) -> quarto_pandoc_types::ConfigValue {
        use quarto_pandoc_types::ConfigMapEntry;
        let si = quarto_source_map::SourceInfo::default();
        quarto_pandoc_types::ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "template".to_string(),
                key_source: si.clone(),
                value: quarto_pandoc_types::ConfigValue::new_path(template_path.to_string(), si),
            }],
            quarto_source_map::SourceInfo::default(),
        )
    }

    fn meta_with_template_and_partials(
        template_path: &str,
        partial_paths: &[&str],
    ) -> quarto_pandoc_types::ConfigValue {
        use quarto_pandoc_types::ConfigMapEntry;
        let si = quarto_source_map::SourceInfo::default();
        let partials_array: Vec<quarto_pandoc_types::ConfigValue> = partial_paths
            .iter()
            .map(|p| quarto_pandoc_types::ConfigValue::new_path(p.to_string(), si.clone()))
            .collect();

        quarto_pandoc_types::ConfigValue::new_map(
            vec![
                ConfigMapEntry {
                    key: "template".to_string(),
                    key_source: si.clone(),
                    value: quarto_pandoc_types::ConfigValue::new_path(
                        template_path.to_string(),
                        si.clone(),
                    ),
                },
                ConfigMapEntry {
                    key: "template-partials".to_string(),
                    key_source: si.clone(),
                    value: quarto_pandoc_types::ConfigValue::new_array(partials_array, si),
                },
            ],
            quarto_source_map::SourceInfo::default(),
        )
    }

    #[tokio::test]
    async fn test_custom_template_from_metadata() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project_dir = tmp.path().to_path_buf();

        // Write a custom template
        let template_content = "<!DOCTYPE html><html><body>CUSTOM: $body$</body></html>";
        std::fs::write(project_dir.join("custom.html"), template_content).unwrap();

        // Write a qmd file (just need the path)
        let qmd_path = project_dir.join("test.qmd");
        std::fs::write(&qmd_path, "").unwrap();

        let runtime = Arc::new(quarto_system_runtime::NativeRuntime::new());
        let project = ProjectContext {
            dir: project_dir.clone(),
            config: crate::project::ProjectConfig::default(),
            is_single_file: true,
            files: vec![],
            output_dir: project_dir.clone(),
        };
        let doc = DocumentInfo::from_path(&qmd_path);
        let format = Format::html();
        let mut ctx = StageContext::new(runtime, format.clone(), project, doc).unwrap();

        let stage = ApplyTemplateStage::new();
        let metadata = meta_with_template("custom.html");
        let rendered = make_rendered_output_with_metadata(qmd_path, metadata);

        let input = PipelineData::RenderedOutput(rendered);
        let output = stage.run(input, &mut ctx).await.unwrap();
        let result = output.into_rendered_output().unwrap();

        assert!(
            result.content.contains("CUSTOM: <p>Hello</p>"),
            "expected custom template output, got: {}",
            result.content
        );
    }

    #[tokio::test]
    async fn test_custom_template_with_partials() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project_dir = tmp.path().to_path_buf();

        // Write a custom template that uses a partial
        let template_content = "<!DOCTYPE html><html><body>$header()$\n$body$</body></html>";
        std::fs::write(project_dir.join("custom.html"), template_content).unwrap();

        // Write the partial
        std::fs::write(
            project_dir.join("header.html"),
            "<header>MY HEADER</header>",
        )
        .unwrap();

        let qmd_path = project_dir.join("test.qmd");
        std::fs::write(&qmd_path, "").unwrap();

        let runtime = Arc::new(quarto_system_runtime::NativeRuntime::new());
        let project = ProjectContext {
            dir: project_dir.clone(),
            config: crate::project::ProjectConfig::default(),
            is_single_file: true,
            files: vec![],
            output_dir: project_dir.clone(),
        };
        let doc = DocumentInfo::from_path(&qmd_path);
        let format = Format::html();
        let mut ctx = StageContext::new(runtime, format.clone(), project, doc).unwrap();

        let stage = ApplyTemplateStage::new();
        let metadata = meta_with_template_and_partials("custom.html", &["header.html"]);
        let rendered = make_rendered_output_with_metadata(qmd_path, metadata);

        let input = PipelineData::RenderedOutput(rendered);
        let output = stage.run(input, &mut ctx).await.unwrap();
        let result = output.into_rendered_output().unwrap();

        assert!(
            result.content.contains("<header>MY HEADER</header>"),
            "expected partial content in output, got: {}",
            result.content
        );
        assert!(result.content.contains("<p>Hello</p>"));
    }

    #[tokio::test]
    async fn test_no_template_no_partials_existing_behavior() {
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
        let mut ctx = StageContext::new(runtime, format.clone(), project, doc).unwrap();

        let stage = ApplyTemplateStage::new();
        let rendered = RenderedOutput {
            input_path: PathBuf::from("/project/test.qmd"),
            output_path: PathBuf::from("/project/test.html"),
            format,
            content: "<p>Hello, world!</p>".to_string(),
            is_intermediate: false,
            supporting_files: vec![],
            metadata: quarto_pandoc_types::ConfigValue::null(
                quarto_source_map::SourceInfo::default(),
            ),
        };

        let input = PipelineData::RenderedOutput(rendered);
        let output = stage.run(input, &mut ctx).await.unwrap();
        let result = output.into_rendered_output().unwrap();

        // Should use built-in template
        assert!(result.content.contains("<!DOCTYPE html>"));
        assert!(result.content.contains("<p>Hello, world!</p>"));
    }

    #[tokio::test]
    async fn test_template_key_not_in_output() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project_dir = tmp.path().to_path_buf();

        // Custom template that would show $template$ if it leaked through
        let template_content = "<!DOCTYPE html><html><body>TMPL=[$template$] $body$</body></html>";
        std::fs::write(project_dir.join("custom.html"), template_content).unwrap();

        let qmd_path = project_dir.join("test.qmd");
        std::fs::write(&qmd_path, "").unwrap();

        let runtime = Arc::new(quarto_system_runtime::NativeRuntime::new());
        let project = ProjectContext {
            dir: project_dir.clone(),
            config: crate::project::ProjectConfig::default(),
            is_single_file: true,
            files: vec![],
            output_dir: project_dir.clone(),
        };
        let doc = DocumentInfo::from_path(&qmd_path);
        let format = Format::html();
        let mut ctx = StageContext::new(runtime, format.clone(), project, doc).unwrap();

        let stage = ApplyTemplateStage::new();
        let metadata = meta_with_template("custom.html");
        let rendered = make_rendered_output_with_metadata(qmd_path, metadata);

        let input = PipelineData::RenderedOutput(rendered);
        let output = stage.run(input, &mut ctx).await.unwrap();
        let result = output.into_rendered_output().unwrap();

        // $template$ should resolve to empty (stripped from context), not "custom.html"
        assert!(
            !result.content.contains("custom.html"),
            "template path leaked into output: {}",
            result.content
        );
        assert!(result.content.contains("TMPL=[]"));
    }

    #[tokio::test]
    async fn test_missing_template_file_errors() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project_dir = tmp.path().to_path_buf();

        let qmd_path = project_dir.join("test.qmd");
        std::fs::write(&qmd_path, "").unwrap();

        let runtime = Arc::new(quarto_system_runtime::NativeRuntime::new());
        let project = ProjectContext {
            dir: project_dir.clone(),
            config: crate::project::ProjectConfig::default(),
            is_single_file: true,
            files: vec![],
            output_dir: project_dir.clone(),
        };
        let doc = DocumentInfo::from_path(&qmd_path);
        let format = Format::html();
        let mut ctx = StageContext::new(runtime, format.clone(), project, doc).unwrap();

        let stage = ApplyTemplateStage::new();
        let metadata = meta_with_template("nonexistent.html");
        let rendered = make_rendered_output_with_metadata(qmd_path, metadata);

        let input = PipelineData::RenderedOutput(rendered);
        let err = stage.run(input, &mut ctx).await.unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("nonexistent.html"),
            "error should mention the missing file: {}",
            msg
        );
    }

    #[tokio::test]
    async fn test_document_template_overrides_extension() {
        // When both document and extension provide template, the document-level
        // value wins because it's higher in the merge order. After merge, only
        // one template path exists in metadata. This test verifies the stage
        // uses whatever metadata says.
        let tmp = tempfile::TempDir::new().unwrap();
        let project_dir = tmp.path().to_path_buf();

        let template_content = "<!DOCTYPE html><html><body>DOC-TMPL: $body$</body></html>";
        std::fs::write(project_dir.join("doc-template.html"), template_content).unwrap();

        let qmd_path = project_dir.join("test.qmd");
        std::fs::write(&qmd_path, "").unwrap();

        let runtime = Arc::new(quarto_system_runtime::NativeRuntime::new());
        let project = ProjectContext {
            dir: project_dir.clone(),
            config: crate::project::ProjectConfig::default(),
            is_single_file: true,
            files: vec![],
            output_dir: project_dir.clone(),
        };
        let doc = DocumentInfo::from_path(&qmd_path);
        let format = Format::html();
        let mut ctx = StageContext::new(runtime, format.clone(), project, doc).unwrap();

        let stage = ApplyTemplateStage::new();
        let metadata = meta_with_template("doc-template.html");
        let rendered = make_rendered_output_with_metadata(qmd_path, metadata);

        let input = PipelineData::RenderedOutput(rendered);
        let output = stage.run(input, &mut ctx).await.unwrap();
        let result = output.into_rendered_output().unwrap();

        assert!(result.content.contains("DOC-TMPL:"));
    }
}
