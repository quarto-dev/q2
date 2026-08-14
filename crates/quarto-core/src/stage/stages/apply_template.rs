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

use std::path::Path;

use async_trait::async_trait;
use quarto_doctemplate::{ChainedResolver, MemoryResolver, Template};

use crate::resource_resolver::ResourceResolverContext;
use crate::stage::{
    EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};
use crate::template;
use crate::template::RuntimeResolver;
use crate::trace_event;

/// Configuration for the ApplyTemplateStage.
///
/// Phase 5: replaced `css_paths` + `resource_prefix` with a
/// scope-aware [`ResourceResolverContext`]. When `resolver` is
/// provided (CLI render, project pipeline), every CSS / JS
/// artifact in the store gets its `<link>` / `<script>` URL
/// computed by the resolver. When `resolver` is absent
/// (in-memory tests, hub-client legacy path), each artifact's
/// bare `path` is used verbatim.
#[derive(Default)]
pub struct ApplyTemplateConfig {
    /// Scope-aware resolver. When `Some`, drives URL computation
    /// for every artifact emitted into `<head>`. When `None`,
    /// artifacts' relative `path`s are used as-is (legacy
    /// in-memory behavior; preserved for tests and the WASM
    /// hub-client until that gets a resolver of its own —
    /// tracked separately as the Phase 5 WASM-audit task).
    pub resolver: Option<ResourceResolverContext>,
}

impl ApplyTemplateConfig {
    /// Create a new default configuration (no resolver).
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the [`ResourceResolverContext`] that translates
    /// per-artifact `(scope, path)` tuples into HTML URLs.
    pub fn with_resolver(mut self, resolver: ResourceResolverContext) -> Self {
        self.resolver = Some(resolver);
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

#[async_trait(?Send)]
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

        // Get metadata from the rendered output. Drain any post-resolve
        // additions to `ctx.includes` (shortcode resolution, Lua filters
        // via `quarto.doc.include_text()`) into the canonical
        // `rendered.includes.*` arrays so the template helper sees
        // everything in one place. Engine includes were already drained
        // by `IncludeResolveStage`; this catches the late path.
        let mut metadata = rendered.metadata.clone();
        let late = std::mem::take(&mut ctx.includes);
        super::include_resolve::append_pandoc_includes(&mut metadata, &late);

        // Build CSS / JS URL lists from the artifact store.
        //
        // Phase 5: every CSS / JS artifact carries an
        // `ArtifactScope` tag (Page or Project). The resolver
        // translates the artifact's `(scope, path)` into a URL
        // suitable for the rendered HTML, accounting for the
        // page's depth below the site root and the project's
        // shared lib dir.
        //
        // Iteration order is deterministic (sorted by key) so the
        // emitted `<link>` / `<script>` order does not depend on
        // HashMap layout. Theme CSS (key prefix `css:theme:`)
        // sorts ahead of extension CSS (`css:libs:*` /
        // `css:<other>:*`), preserving today's "theme first" order.
        let css_paths = collect_artifact_urls(ctx, "css:", self.config.resolver.as_ref());
        let script_paths = collect_artifact_urls(ctx, "js:", self.config.resolver.as_ref());

        // Extract custom template/partials from merged metadata.
        //
        // bd-xdnk note: YAML scalars like `template: custom.html` are parsed
        // as `ConfigValueKind::PandocInlines` by the document loader
        // (consistent with how Pandoc treats inline-content metadata
        // values). `as_str()` only matches `String` / `Path` and would
        // silently miss the inlines case — that's why `template:` from a
        // qmd front-matter never reached the renderer before this fix.
        // Use `as_plain_text()`, which also extracts text from inlines.
        let custom_template_path = metadata.get("template").and_then(|v| v.as_plain_text());
        let partial_paths: Vec<String> = metadata
            .get("template-partials")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_plain_text()).collect())
            .unwrap_or_default();

        // Apply template: metadata-driven selection
        let document_dir = rendered
            .input_path
            .parent()
            .unwrap_or_else(|| Path::new("."));

        // bd-xdnk: thread the document's `SourceContext` through template
        // compilation so doctemplate diagnostics (Q-10-2 Undefined variable
        // etc.) carry FileIds the renderer can resolve back to source for
        // ariadne carets. The context is owned by `rendered.source_context`
        // (forwarded here by `RenderHtmlStage`).
        let (html, template_diags) = match custom_template_path {
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
                    // Custom template, no explicit partials: RuntimeResolver
                    // (template-adjacent files), falling back to the built-in
                    // partials so Q1-ported templates can call
                    // `$title-block.html()$` without shipping a copy.
                    let runtime = RuntimeResolver::new(ctx.runtime.as_ref());
                    let resolver = ChainedResolver::new(runtime, template::builtin_html_partials());
                    Template::compile_with_resolver_and_context(
                        &template_content,
                        &abs_path,
                        &resolver,
                        0,
                        &mut rendered.source_context,
                    )
                    .map_err(|e| {
                        PipelineError::stage_error(
                            self.name(),
                            format!("failed to compile template '{}': {}", abs_path.display(), e),
                        )
                    })?
                } else {
                    // Custom template + explicit partials: chain
                    // MemoryResolver → RuntimeResolver → built-in partials.
                    let memory = build_partial_resolver(
                        &partial_paths,
                        document_dir,
                        ctx.runtime.as_ref(),
                        self.name(),
                    )?;
                    let runtime = RuntimeResolver::new(ctx.runtime.as_ref());
                    let chained = ChainedResolver::new(
                        memory,
                        ChainedResolver::new(runtime, template::builtin_html_partials()),
                    );
                    Template::compile_with_resolver_and_context(
                        &template_content,
                        &abs_path,
                        &chained,
                        0,
                        &mut rendered.source_context,
                    )
                    .map_err(|e| {
                        PipelineError::stage_error(
                            self.name(),
                            format!("failed to compile template '{}': {}", abs_path.display(), e),
                        )
                    })?
                };

                template::render_with_compiled_template(
                    &compiled,
                    &rendered.content,
                    &metadata,
                    &css_paths,
                    &script_paths,
                )
                .map_err(|e| PipelineError::stage_error(self.name(), e.to_string()))?
            }
            None if !partial_paths.is_empty() => {
                // No custom template, but explicit partials: compile built-in with partials.
                // Built-in templates have no unguarded `$var$` references
                // (verified pre-flight, bd-xdnk plan §Phase 0); we still
                // thread the SourceContext so any future tweak that does
                // emit diagnostics attributes correctly.
                let memory = build_partial_resolver(
                    &partial_paths,
                    document_dir,
                    ctx.runtime.as_ref(),
                    self.name(),
                )?;
                let compiled = template::compile_builtin_template_with_partials(
                    &metadata,
                    &memory,
                    &mut rendered.source_context,
                )
                .map_err(|e| PipelineError::stage_error(self.name(), e.to_string()))?;

                template::render_with_compiled_template(
                    &compiled,
                    &rendered.content,
                    &metadata,
                    &css_paths,
                    &script_paths,
                )
                .map_err(|e| PipelineError::stage_error(self.name(), e.to_string()))?
            }
            None if ctx.format.identifier == crate::format::FormatIdentifier::Revealjs => {
                // revealjs uses its own scaffold (reveal.js + Reveal.initialize),
                // bypassing the Bootstrap HTML templates. The body is already a
                // sequence of `<section>` slides from RevealSlidesTransform.
                // bd-jij5gge2: the deck's vendored assets are LINKED, not
                // inlined — `RevealAssetsStage` registered them as
                // `css:revealjs:*` / `js:revealjs:*` artifacts; we collect just
                // those (a reveal deck never wants the Bootstrap `css:theme:*`)
                // and the resolver gives the right per-context URLs.
                let reveal_css: Vec<String> =
                    collect_artifact_urls(ctx, "css:revealjs:", self.config.resolver.as_ref())
                        .into_iter()
                        .map(|r| r.url)
                        .collect();
                let reveal_js: Vec<String> =
                    collect_artifact_urls(ctx, "js:revealjs:", self.config.resolver.as_ref())
                        .into_iter()
                        .map(|r| r.url)
                        .collect();
                let html = crate::revealjs::render_revealjs_document(
                    &rendered.content,
                    &metadata,
                    &reveal_css,
                    &reveal_js,
                );
                (html, Vec::new())
            }
            None => {
                // No custom template, no partials: select built-in template based on format
                let minimal = crate::format::is_minimal_html(&metadata);
                let compiled = template::select_template(minimal)
                    .map_err(|e| PipelineError::stage_error(self.name(), e.to_string()))?;
                template::render_with_compiled_template(
                    &compiled,
                    &rendered.content,
                    &metadata,
                    &css_paths,
                    &script_paths,
                )
                .map_err(|e| PipelineError::stage_error(self.name(), e.to_string()))?
            }
        };

        trace_event!(
            ctx,
            EventLevel::Debug,
            "template applied, {} bytes of HTML, {} diagnostics",
            html.len(),
            template_diags.len()
        );

        // Surface template diagnostics (e.g. Q-10-2 Undefined variable) to
        // the pipeline's diagnostic stream so the CLI / hub-client can
        // render them. bd-xdnk.
        ctx.diagnostics.extend(template_diags);

        // Update content with full HTML document
        rendered.content = html;

        Ok(PipelineData::RenderedOutput(rendered))
    }
}

/// Collect HTML URLs for every artifact under a given key
/// prefix.
///
/// Iterates artifacts in sorted-key order so the emitted
/// `<link>` / `<script>` block is deterministic across runs.
/// Skips artifacts without a `path`.
///
/// - `Some(resolver)`: each artifact's URL is computed by the
///   resolver from its `(scope, path)` tuple.
/// - `None`: each artifact's URL is its `path` rendered as a
///   forward-slash relative URL. This preserves legacy
///   in-memory test behavior; the WASM hub-client (which today
///   relies on a synthetic `DEFAULT_CSS_ARTIFACT_PATH`) will be
///   migrated to a resolver in the WASM-audit follow-up.
fn collect_artifact_urls(
    ctx: &StageContext,
    prefix: &str,
    resolver: Option<&ResourceResolverContext>,
) -> Vec<crate::template::LinkedResource> {
    let mut entries: Vec<(&str, &crate::artifact::Artifact)> = ctx.artifacts.get_by_prefix(prefix);
    // Sort by (link_order, key): all artifacts default to order 0, so
    // the pre-existing lexicographic-key order is preserved exactly;
    // the light/dark theme sheets use positive orders to pin their
    // FOUC-safe light → dark → default-copy sequence (bd-0pic6 A3).
    entries.sort_by(|a, b| (a.1.link_order, a.0).cmp(&(b.1.link_order, b.0)));

    let mut urls = Vec::with_capacity(entries.len());
    for (_, artifact) in entries {
        let Some(path) = &artifact.path else { continue };
        let url = match resolver {
            Some(r) => r.html_url_for(artifact.scope, path),
            None => path.to_string_lossy().replace('\\', "/"),
        };
        urls.push(crate::template::LinkedResource {
            url,
            attribs: artifact.link_attribs.clone(),
        });
    }
    urls
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
    use std::path::PathBuf;
    use std::sync::Arc;

    // Mock runtime for testing
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
                quarto_source_map::SourceInfo::for_test(),
            ),
            source_context: quarto_source_map::SourceContext::new(),
        };

        let input = PipelineData::RenderedOutput(rendered);
        let output = stage.run(input, &mut ctx).await.unwrap();

        let result = output
            .into_rendered_output()
            .expect("Should be RenderedOutput");
        assert!(result.content.contains("<!DOCTYPE html>"));
        assert!(result.content.contains("<p>Hello, world!</p>"));
        // Phase 5: ApplyTemplateStage no longer stores its own
        // theme CSS artifact. CompileThemeCssStage is now the
        // sole producer (key prefix `css:theme:*`); when
        // ApplyTemplateStage runs in isolation (e.g. this test
        // sets up RenderedOutput directly without running the
        // earlier stages), no theme CSS artifact is expected.
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
            source_context: quarto_source_map::SourceContext::new(),
        }
    }

    fn meta_with_template(template_path: &str) -> quarto_pandoc_types::ConfigValue {
        use quarto_pandoc_types::ConfigMapEntry;
        let si = quarto_source_map::SourceInfo::for_test();
        quarto_pandoc_types::ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "template".to_string(),
                key_source: si.clone(),
                value: quarto_pandoc_types::ConfigValue::new_path(template_path.to_string(), si),
            }],
            quarto_source_map::SourceInfo::for_test(),
        )
    }

    /// bd-xdnk: real YAML front-matter parses `template: custom.html` as
    /// `PandocInlines`, not as a `Path` scalar. This helper mirrors the
    /// shape the qmd parser actually produces, so regression tests can
    /// catch the "as_str() returns None for inlines" bug that hid
    /// custom templates from `quarto render`.
    fn meta_with_template_as_inlines(template_path: &str) -> quarto_pandoc_types::ConfigValue {
        use quarto_pandoc_types::{ConfigMapEntry, Inline, Str};
        let si = quarto_source_map::SourceInfo::for_test();
        let inlines: quarto_pandoc_types::Inlines = vec![Inline::Str(Str {
            text: template_path.to_string(),
            source_info: si.clone(),
        })];
        quarto_pandoc_types::ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "template".to_string(),
                key_source: si.clone(),
                value: quarto_pandoc_types::ConfigValue::new_inlines(inlines, si),
            }],
            quarto_source_map::SourceInfo::for_test(),
        )
    }

    fn meta_with_template_and_partials(
        template_path: &str,
        partial_paths: &[&str],
    ) -> quarto_pandoc_types::ConfigValue {
        use quarto_pandoc_types::ConfigMapEntry;
        let si = quarto_source_map::SourceInfo::for_test();
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
            quarto_source_map::SourceInfo::for_test(),
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

    /// bd-xdnk regression: real qmd front-matter parses
    /// `template: custom.html` as `PandocInlines`. The previous
    /// `metadata.get("template").as_str()` lookup returned `None` for
    /// inlines, so the custom template was silently ignored under
    /// `quarto render`. This test mirrors the inlines shape to lock
    /// in `as_plain_text()` as the correct lookup.
    #[tokio::test]
    async fn test_custom_template_path_from_pandoc_inlines() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project_dir = tmp.path().to_path_buf();

        let template_content = "<!DOCTYPE html><html><body>FROM-INLINES: $body$</body></html>";
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
        let metadata = meta_with_template_as_inlines("custom.html");
        let rendered = make_rendered_output_with_metadata(qmd_path, metadata);

        let input = PipelineData::RenderedOutput(rendered);
        let output = stage.run(input, &mut ctx).await.unwrap();
        let result = output.into_rendered_output().unwrap();

        assert!(
            result.content.contains("FROM-INLINES: <p>Hello</p>"),
            "PandocInlines `template:` value should select the custom \
             template; got:\n{}",
            result.content
        );
    }

    /// bd-xdnk: an undefined variable in a custom template must produce a
    /// `Q-10-2` warning in `ctx.diagnostics`, with a source location that
    /// points back into the template file.
    #[tokio::test]
    async fn test_custom_template_undefined_variable_emits_diagnostic() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project_dir = tmp.path().to_path_buf();

        // Custom template references a variable that the document never
        // defines. Render must succeed (warning, not error) and the
        // stage must publish the warning into ctx.diagnostics.
        let template_content =
            "<!DOCTYPE html><html><body><header>by $author-greeting$</header>$body$</body></html>";
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
        let output = stage
            .run(input, &mut ctx)
            .await
            .expect("stage should succeed (warning, not error)");

        let result = output.into_rendered_output().unwrap();
        assert!(
            result.content.contains("<p>Hello</p>"),
            "body should still render, got: {}",
            result.content
        );

        let undef = ctx.diagnostics.iter().find(|d| {
            d.code.as_deref() == Some("Q-10-2")
                && d.kind == quarto_error_reporting::DiagnosticKind::Warning
        });
        assert!(
            undef.is_some(),
            "expected Q-10-2 warning for undefined variable in ctx.diagnostics, got: {:?}",
            ctx.diagnostics
        );

        // The diagnostic's location should resolve to the template file
        // via the shared SourceContext (so ariadne can slice source for
        // the caret). Walk to the root SourceInfo and look up the FileId
        // in `result.source_context`.
        let diag = undef.unwrap();
        let location = diag
            .details
            .iter()
            .find_map(|d| d.location.as_ref())
            .or(diag.location.as_ref())
            .expect("diagnostic should carry a SourceInfo location");

        let file_id = location
            .root_file_id()
            .expect("diagnostic location should have a resolvable FileId");
        let file = result
            .source_context
            .get_file(file_id)
            .expect("template file should be registered in shared SourceContext");
        assert!(
            file.path.contains("custom.html"),
            "diagnostic should attribute to custom.html, got path = {}",
            file.path
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
                quarto_source_map::SourceInfo::for_test(),
            ),
            source_context: quarto_source_map::SourceContext::new(),
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
