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

use std::path::PathBuf;

use async_trait::async_trait;
use quarto_doctemplate::Template;

use crate::artifact::Artifact;
use crate::pipeline::DEFAULT_CSS_ARTIFACT_PATH;
use crate::resources::DEFAULT_CSS;
use crate::stage::{
    EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};
use crate::template;
use crate::trace_event;

/// Configuration for the ApplyTemplateStage.
#[derive(Default)]
pub struct ApplyTemplateConfig {
    /// CSS paths to include in the document (relative to the output HTML).
    pub css_paths: Vec<String>,
    /// Custom template to use instead of the built-in default.
    pub template: Option<Template>,
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

    /// Set a custom template.
    pub fn with_template(mut self, template: Template) -> Self {
        self.template = Some(template);
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
/// - `template`: Custom template (defaults to built-in HTML5 template)
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

#[async_trait]
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

        // Store CSS artifact for WASM consumption
        ctx.artifacts.store(
            "css:default",
            Artifact::from_string(DEFAULT_CSS, "text/css")
                .with_path(PathBuf::from(DEFAULT_CSS_ARTIFACT_PATH)),
        );

        // Get metadata from the rendered output
        let metadata = rendered.metadata.clone();

        // Apply template
        let html = match &self.config.template {
            Some(template) => {
                template::render_with_custom_template(template, &rendered.content, &metadata)
                    .map_err(|e| PipelineError::stage_error(self.name(), e.to_string()))?
            }
            None => {
                // When no CSS paths are provided, use the default CSS artifact path
                let css_paths: Vec<String> = if self.config.css_paths.is_empty() {
                    vec![DEFAULT_CSS_ARTIFACT_PATH.to_string()]
                } else {
                    self.config.css_paths.clone()
                };

                template::render_with_resources(&rendered.content, &metadata, &css_paths)
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
            config: None,
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
}
