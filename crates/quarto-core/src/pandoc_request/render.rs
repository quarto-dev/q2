//! The logic behind the browser's `render_pandoc_request` export: run the
//! pandoc-format pipeline with `PandocPrepareStage` as the tail and hand back
//! the request. The wasm wrapper (`wasm-quarto-hub-client`) only converts the
//! outcome to a JS object; everything testable lives here, so native tests
//! drive it with `NativeRuntime` over a directory.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use quarto_error_reporting::{DiagnosticKind, DiagnosticMessage};
use quarto_source_map::SourceContext;
use quarto_system_runtime::SystemRuntime;

use super::formats::pandoc_format;
use super::{PandocRequest, PrepareOptions, RequestPost, constants};
use crate::error::QuartoError;
use crate::format::Format;
use crate::pipeline::{build_pandoc_request_stages_fetching, run_pipeline};
use crate::project::{DocumentInfo, ProjectContext};
use crate::render::{BinaryDependencies, RenderContext, RenderOptions};
use crate::resource_resolver::ResourceResolverContext;

pub struct PandocRequestInput<'a> {
    pub path: &'a Path,
    pub content: &'a [u8],
    /// A `format:` key (`docx`, ...). Must be a downloadable format.
    pub format: &'a str,
    pub project: &'a ProjectContext,
    /// Seconds. Set as `SOURCE_DATE_EPOCH`; `None` leaves it unset.
    pub source_date_epoch: Option<i64>,
    pub captures: Vec<quarto_trace::EngineCapture>,
    /// Echoed into the request (`None` until the PDF work).
    pub typst_available_fonts: Option<Vec<String>>,
    /// The hub prelude installs the vfs-root resolver for every render;
    /// `None` leaves the pipeline's default.
    pub resolver: Option<ResourceResolverContext>,
}

#[derive(Debug)]
pub struct PandocRequestOutcome {
    /// Absent when the document has errors or the request could not be built.
    pub request: Option<PandocRequest>,
    /// Warnings, and the errors that kept `request` from being built.
    pub diagnostics: Vec<DiagnosticMessage>,
    pub source_context: SourceContext,
    /// Why there is no request, when there is none.
    pub error: Option<String>,
    /// Code cells with no cached result, which the document shows as source.
    pub unexecuted_cells: usize,
}

impl PandocRequestOutcome {
    fn failed(error: impl Into<String>) -> Self {
        Self {
            request: None,
            diagnostics: Vec::new(),
            source_context: SourceContext::default(),
            error: Some(error.into()),
            unexecuted_cells: 0,
        }
    }
}

pub async fn render_pandoc_request(
    input: PandocRequestInput<'_>,
    runtime: Arc<dyn SystemRuntime>,
) -> PandocRequestOutcome {
    let PandocRequestInput {
        path,
        content,
        format: format_key,
        project,
        source_date_epoch,
        captures,
        typst_available_fonts,
        resolver,
    } = input;

    // D8: only the table's formats. `pdf` is the typst request plus a
    // `compile_typst` post step (the host compiles the `.typ`), so it is
    // built as typst.
    let Some(info) = pandoc_format(format_key) else {
        return PandocRequestOutcome::failed(format!(
            "{format_key} cannot be rendered by pandoc in the browser"
        ));
    };
    if !info.available {
        return PandocRequestOutcome::failed(format!(
            "{format_key} output is not available in the browser yet"
        ));
    }
    let compile_typst = format_key == "pdf";
    let format = match Format::from_format_string(if compile_typst { "typst" } else { format_key })
    {
        Ok(f) => f,
        Err(e) => return PandocRequestOutcome::failed(e),
    };

    let prepare_options = PrepareOptions {
        temp_root: PathBuf::from(&constants().share_root),
        source_date_epoch,
        collect_resources: true,
        typst_available_fonts: typst_available_fonts.clone(),
        post: if compile_typst {
            RequestPost::CompileTypst
        } else {
            RequestPost::None
        },
    };

    // A document inside a `_quarto.yml` project renders as the active page of
    // the project (R7), through the orchestrator's pass 1 (the index) and a
    // pandoc Pass2Renderer.
    if !project.is_single_file {
        return render_project_request(
            path,
            format,
            if compile_typst { "typst" } else { format_key },
            project,
            prepare_options,
            captures,
            runtime,
        )
        .await;
    }

    let doc = DocumentInfo::from_path(path);
    let binaries = BinaryDependencies::new();
    let mut ctx =
        RenderContext::new(project, &doc, &format, &binaries).with_options(RenderOptions {
            verbose: false,
            execute: false,
            use_freeze: false,
            output_path: None,
        });
    ctx.resource_resolver = resolver;
    ctx.prepare_options = Some(prepare_options);
    let source_name = path.to_string_lossy();
    build_request_in_context(
        &mut ctx,
        content,
        &source_name,
        runtime,
        captures,
        typst_available_fonts,
    )
    .await
}

/// Run the pandoc-request pipeline over `content` in a prepared context and
/// collect the outcome. `ctx.prepare_options` must be set. Shared by the
/// single-document path and [`RenderToPandocRequestRenderer`], so a project
/// document goes through exactly the stages a single document does.
///
/// [`RenderToPandocRequestRenderer`]: crate::project::pass2_renderer::RenderToPandocRequestRenderer
pub(crate) async fn build_request_in_context(
    ctx: &mut RenderContext<'_>,
    content: &[u8],
    source_name: &str,
    runtime: Arc<dyn SystemRuntime>,
    captures: Vec<quarto_trace::EngineCapture>,
    typst_available_fonts: Option<Vec<String>>,
) -> PandocRequestOutcome {
    // The browser has no engines: cells without a capture pass through
    // inert and silently (the response counts them), whatever the native
    // registry would have done with them.
    ctx.execution_policy = crate::engine::ExecutionPolicy::None;

    let (output, diagnostics) = match run_pipeline(
        content,
        source_name,
        ctx,
        runtime,
        build_pandoc_request_stages_fetching(captures),
    )
    .await
    {
        Ok(ok) => ok,
        Err(QuartoError::Parse(parse_error)) => {
            return PandocRequestOutcome {
                request: None,
                diagnostics: parse_error.diagnostics.clone(),
                source_context: parse_error.source_context.clone(),
                error: Some(QuartoError::Parse(parse_error).to_string()),
                unexecuted_cells: 0,
            };
        }
        Err(e) => return PandocRequestOutcome::failed(e.to_string()),
    };
    let source_context = output
        .into_rendered_output()
        .map(|r| r.source_context)
        .unwrap_or_default();

    // A document with errors does not produce a request (as natively).
    let first_error = diagnostics
        .iter()
        .find(|d| d.kind == DiagnosticKind::Error)
        .map(|d| d.title.clone());
    if let Some(title) = first_error {
        return PandocRequestOutcome {
            request: None,
            diagnostics,
            source_context,
            error: Some(title),
            unexecuted_cells: ctx.unexecuted_cells,
        };
    }

    let Some(mut request) = ctx.pandoc_request.take() else {
        return PandocRequestOutcome::failed("the pipeline produced no pandoc request");
    };
    if typst_available_fonts.is_some() {
        request.typst_available_fonts = typst_available_fonts;
    }
    request.job_id = request.compute_job_id();
    PandocRequestOutcome {
        request: Some(request),
        diagnostics,
        source_context,
        error: None,
        unexecuted_cells: ctx.unexecuted_cells,
    }
}

/// A document inside a project: the project's pass 1 builds the index (so
/// cross-document links, the project's `format:` layers and its metadata all
/// resolve as they do natively), then pass 2 renders only the active page,
/// with a renderer that builds the request instead of writing a file.
async fn render_project_request(
    path: &Path,
    format: Format,
    format_str: &str,
    project: &ProjectContext,
    prepare_options: PrepareOptions,
    captures: Vec<quarto_trace::EngineCapture>,
    runtime: Arc<dyn SystemRuntime>,
) -> PandocRequestOutcome {
    use crate::project::orchestrator::{ProjectPipeline, RenderMode, project_type_for};
    use crate::project::pass2_renderer::RenderToPandocRequestRenderer;

    // The pipeline fills in per-render state (the book's render list) and
    // takes the project by `&mut`; the caller's copy stays as it was.
    let mut project = project.clone();
    let project_type = project_type_for(&project);
    // `project.files` holds canonical paths and the active-page filter
    // compares by equality.
    let active = runtime
        .canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf());
    let renderer = RenderToPandocRequestRenderer::new(prepare_options, captures);
    let mut pipeline = ProjectPipeline::with_renderer(
        &mut project,
        project_type,
        format,
        format_str,
        runtime,
        renderer,
    )
    .with_mode(RenderMode::ActivePage(active.clone()));
    let summary = match pipeline.run().await {
        Ok(summary) => summary,
        Err(QuartoError::Parse(parse_error)) => {
            return PandocRequestOutcome {
                request: None,
                diagnostics: parse_error.diagnostics.clone(),
                source_context: parse_error.source_context.clone(),
                error: Some(QuartoError::Parse(parse_error).to_string()),
                unexecuted_cells: 0,
            };
        }
        Err(e) => return PandocRequestOutcome::failed(e.to_string()),
    };

    let Some(page) = summary.outputs.into_iter().next() else {
        // Pass 1 drops a page that fails to parse, and pass 2 never sees it:
        // report the page's own diagnostics (as the preview does).
        let failure = summary
            .pass1_failures
            .into_iter()
            .find(|f| f.input == active)
            .or_else(|| summary.pass2_failures.into_iter().next());
        return match failure {
            Some(failure) => PandocRequestOutcome {
                request: None,
                diagnostics: failure.diagnostics,
                source_context: failure.source_context.unwrap_or_default(),
                error: Some(format!(
                    "Rendering {} failed: {}",
                    failure.input.display(),
                    failure.error
                )),
                unexecuted_cells: 0,
            },
            None => PandocRequestOutcome::failed(
                "The project render produced no output for the active page",
            ),
        };
    };

    let mut outcome = page.outcome;
    outcome.diagnostics.extend(summary.project_diagnostics);
    outcome
}
