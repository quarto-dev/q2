/*
 * integration/ipynb_content_processor.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Plan 7c Phase 1: the ipynb content processor's source-mapping flagship.
 *
 * The *mapping* half of the flagship test lives here; the *rendering* half
 * (the diagnostic renders labeled `foo.ipynb[cell N]` with a correct
 * in-cell snippet) arrived with the upstream quarto-error-reporting
 * cross-piece fix (plan § "Phase 1 (upstream)"; posit-dev/qer PR #7,
 * 0.3.1).
 */

use std::io::sink;
use std::path::Path;

use quarto_core::engine::content_processors::{Converted, ORIGINAL_FILE_ID, ipynb};
use quarto_source_map::{FileId, SourceContext};

const FLAGSHIP_NOTEBOOK: &str = include_str!("fixtures/ipynb/flagship-malformed-cell.ipynb");
const FRONT_MATTER_NOTEBOOK: &str = include_str!("fixtures/ipynb/front-matter.ipynb");
const CROSS_CELL_FENCE_NOTEBOOK: &str = include_str!("fixtures/ipynb/cross-cell-fence.ipynb");

/// Register the original + per-cell virtual files exactly as the
/// production `SourceContext` rebuild sites must (plan decision 6): the
/// original `.ipynb` at `ORIGINAL_FILE_ID`, each cell file contiguous
/// from `FileId(2)`, in `Converted.files` order. Contiguity is
/// load-bearing — a gap would make the next sequential `add_file`
/// silently misbind.
fn register_cells(ctx: &mut SourceContext, converted: &Converted, notebook: &str) {
    ctx.add_file_with_id(
        ORIGINAL_FILE_ID,
        "notebook.ipynb".to_string(),
        Some(notebook.to_string()),
    );
    for (i, (label, text)) in converted.files.iter().enumerate() {
        ctx.add_file_with_id(
            FileId(ORIGINAL_FILE_ID.0 + 1 + i),
            label.clone(),
            Some(text.clone()),
        );
    }
}

/// THE flagship mapping half (plan Phase 1, TDD item): malformed markdown
/// in cell N > 1 flows through `read()` with the converter's multi-piece
/// `Concat` as `parent_source_info`; every Err-path diagnostic must map —
/// via `map_offset`, NOT `resolve_byte_range` (`Substring`-over-`Concat`
/// returns `None` there by design) — into the owning cell's virtual file
/// at in-cell coordinates. This is also the seam-1 confirmation harness:
/// `reroot_diagnostics_into_parent` composes for any parent shape, and
/// this proves the per-cell `Concat` case end-to-end.
#[test]
fn flagship_malformed_cell_maps_diagnostics_into_owning_cell() {
    let converted = ipynb::convert_notebook(Path::new("notebook.ipynb"), FLAGSHIP_NOTEBOOK)
        .expect("flagship notebook must convert");

    let mut ctx = SourceContext::new();
    register_cells(&mut ctx, &converted, FLAGSHIP_NOTEBOOK);

    let mut sink = sink();
    let err = pampa::readers::qmd::read(
        converted.markdown.as_bytes(),
        false,
        "notebook.ipynb",
        &mut sink,
        true,
        Some(converted.source_info.clone()),
    )
    .expect_err("kv-before-class in cell 2 must fail to parse");
    assert!(
        !err.is_empty(),
        "pruned diagnostics must still contain the parse failure"
    );

    // Cell 2's virtual file: original at FileId(1), cell 1 at FileId(2),
    // so the malformed cell owns FileId(3).
    let malformed_cell_file = FileId(3);

    for diag in &err {
        let loc = diag
            .location
            .as_ref()
            .expect("Err-path diagnostic must carry a location");
        let mapped = loc
            .map_offset(0, &ctx)
            .expect("location must map into the Concat parent");
        assert_eq!(
            mapped.file_id, malformed_cell_file,
            "diagnostic must map into the owning cell's file, got {diag:#?}"
        );
        assert_eq!(
            mapped.location.row, 0,
            "the malformed markdown is the first (0-indexed) line of its cell"
        );
        for det in &diag.details {
            if let Some(l) = &det.location {
                let mapped = l
                    .map_offset(0, &ctx)
                    .expect("detail location must map into the Concat parent");
                assert_eq!(
                    mapped.file_id, malformed_cell_file,
                    "detail spans must map into the owning cell too, got {diag:#?}"
                );
            }
        }
    }
}

/// THE flagship rendering half (plan § "Phase 1 (upstream)"): rendering the
/// Err-path diagnostics through the harness `SourceContext` must label the
/// owning cell's virtual file and show its in-cell snippet. Against
/// quarto-error-reporting 0.3.0 this failed: the renderer took the report's
/// file from `root_file_id()` — cell 1 — and drew cell 1's label and content
/// at cell-2 offsets. Fixed upstream in 0.3.1 (report file from the mapped
/// start). This test pins the integration through q2's real dependency.
#[test]
fn flagship_rendering_half_labels_owning_cell_with_snippet() {
    let converted = ipynb::convert_notebook(Path::new("notebook.ipynb"), FLAGSHIP_NOTEBOOK)
        .expect("flagship notebook must convert");

    let mut ctx = SourceContext::new();
    register_cells(&mut ctx, &converted, FLAGSHIP_NOTEBOOK);

    let mut sink = sink();
    let err = pampa::readers::qmd::read(
        converted.markdown.as_bytes(),
        false,
        "notebook.ipynb",
        &mut sink,
        true,
        Some(converted.source_info.clone()),
    )
    .expect_err("kv-before-class in cell 2 must fail to parse");

    let options = quarto_error_reporting::TextRenderOptions {
        enable_hyperlinks: false,
    };
    // `enable_hyperlinks: false` disables OSC-8 only; the ariadne renderer
    // still emits SGR color codes (nextest strips them when displaying, so
    // assert on the stripped text).
    let rendered: String = err
        .iter()
        .map(|d| d.to_text_with_renderer(Some(&ctx), &options, None))
        .collect::<Vec<_>>()
        .join("\n");
    let rendered = strip_ansi(&rendered);

    // The owning cell's virtual file, at in-cell (1-based) coordinates.
    assert!(
        rendered.contains("notebook.ipynb[cell 2, markdown]:1:"),
        "diagnostic must label the owning cell's virtual file; got:\n{rendered}"
    );
    // Its in-cell snippet: the malformed attribute line, not cell 1's text.
    assert!(
        rendered.contains("![logo](images/logo.svg)"),
        "diagnostic must show cell 2's in-cell snippet; got:\n{rendered}"
    );
    // The 0.3.0 bug's wrong label and wrong snippet must stay gone.
    assert!(
        !rendered.contains("notebook.ipynb[cell 1, markdown]"),
        "diagnostic must not fall back to cell 1's file label; got:\n{rendered}"
    );
    assert!(
        !rendered.contains("Some intro text"),
        "diagnostic must not render cell 1's content as the snippet; got:\n{rendered}"
    );
}

/// Ok-path: a genuine YAML front-matter cell parses with its metadata's
/// source rooting into the front-matter cell's virtual file (cell 1 →
/// `FileId(2)`), through the real `parent_source_info` threading.
#[test]
fn front_matter_ok_path_roots_metadata_into_front_matter_cell() {
    let converted = ipynb::convert_notebook(Path::new("notebook.ipynb"), FRONT_MATTER_NOTEBOOK)
        .expect("front-matter notebook must convert");

    let mut ctx = SourceContext::new();
    register_cells(&mut ctx, &converted, FRONT_MATTER_NOTEBOOK);
    let _ = &mut ctx;

    let mut sink = sink();
    let (pandoc, _ast_ctx, warnings) = pampa::readers::qmd::read(
        converted.markdown.as_bytes(),
        false,
        "notebook.ipynb",
        &mut sink,
        true,
        Some(converted.source_info.clone()),
    )
    .expect("front matter must parse");
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:#?}");

    assert_eq!(
        pandoc.meta.source_info.root_file_id(),
        Some(FileId(2)),
        "front-matter metadata must root to cell 1's virtual file, got {:#?}",
        pandoc.meta.source_info
    );
}

/// Decision 2 end-to-end: a fence/div opened in one markdown cell and
/// closed in a later one must parse cleanly through the *concatenated*
/// document — the dropped per-cell standalone-parse gate would have
/// hard-errored exactly this shape.
#[test]
fn cross_cell_div_parses_through_concatenated_document() {
    let converted = ipynb::convert_notebook(Path::new("notebook.ipynb"), CROSS_CELL_FENCE_NOTEBOOK)
        .expect("cross-cell notebook must convert");

    let mut sink = sink();
    let (_pandoc, _ast_ctx, warnings) = pampa::readers::qmd::read(
        converted.markdown.as_bytes(),
        false,
        "notebook.ipynb",
        &mut sink,
        true,
        Some(converted.source_info.clone()),
    )
    .expect("cross-cell div must parse");
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:#?}");
}

/// The converted cells must be exactly one virtual file per notebook cell,
/// with logical (JSON-unescaped) content — the fixture's second cell
/// carries `\t`/`\n` escapes and non-ASCII text that serde_json unescapes.
#[test]
fn converted_files_carry_logical_cell_text_in_notebook_order() {
    let notebook = include_str!("fixtures/ipynb/escaped-text.ipynb");
    let converted = ipynb::convert_notebook(Path::new("notebook.ipynb"), notebook)
        .expect("escaped-text notebook must convert");

    assert_eq!(converted.files.len(), 2);
    assert_eq!(converted.files[0].0, "notebook.ipynb[cell 1, markdown]");
    assert_eq!(converted.files[0].1, "first\n");
    assert_eq!(converted.files[1].0, "notebook.ipynb[cell 2, markdown]");
    assert_eq!(converted.files[1].1, "tab\there\nnew\nline\ncafé ✓\n");
    // And the assembled markdown embeds the unescaped text verbatim.
    assert!(converted.markdown.contains("tab\there\n"));
}

/// Strip ANSI escape sequences (same helper as `llms_txt.rs` / `website_aliases.rs`).
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for d in chars.by_ref() {
                if d.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

// ═══════════════════════════════════════════════════════════════════
// Plan 7c Phase 2 seam 2: the production `SourceContext` plumbing. The
// flagship halves above register the per-cell files by hand (the
// harness); the tests below pin the REAL pipeline sites that must do
// it: `run_pipeline`'s StageError rebuild (pipeline.rs:896) and the
// q2-preview output context (pipeline.rs:1108).
// ═══════════════════════════════════════════════════════════════════

// ═══════════════════════════════════════════════════════════════════
// Plan 7c Phase 2: cell-options composition. A code cell's `#|` option
// lines flow through the existing cell-options machinery
// (`partition_cell_options`), whose option-line Concat is built from
// `Substring` pieces over the caller's `body_source` — which, for a
// converted notebook, is itself a `Substring` over the converter's
// document `Concat`. The composition must resolve a YAML-option
// failure into the owning code cell's virtual file.
// ═══════════════════════════════════════════════════════════════════

const BAD_OPTIONS_NOTEBOOK: &str = include_str!("fixtures/ipynb/code-cell-bad-options.ipynb");

/// A `#|` YAML error inside a code cell must anchor in the owning cell.
/// The anchor is computed exactly as the production execute path
/// computes it (`text_execute.rs`): the body's provenance is
/// `SourceInfo::substring(document_source_info, code_start, …)`, the
/// error's own location is preferred, and — because quarto-yaml's
/// `From<ScanError>` carries `location: None` for scan-level failures —
/// the body source itself is the fallback. Both routes must resolve
/// into the code cell's virtual file, never cell 1 or the raw JSON.
#[test]
fn cell_option_yaml_error_lands_in_owning_cell() {
    let converted = ipynb::convert_notebook(Path::new("notebook.ipynb"), BAD_OPTIONS_NOTEBOOK)
        .expect("bad-options notebook must convert");

    let mut ctx = SourceContext::new();
    register_cells(&mut ctx, &converted, BAD_OPTIONS_NOTEBOOK);

    // Two cells: original at FileId(1), cell 1 at FileId(2), so the
    // code cell owns FileId(3).
    let code_cell_file = FileId(3);
    let code = "#| error: [unclosed\nprint(1)\n";
    let code_start = converted
        .markdown
        .find(code)
        .expect("converted markdown must embed the code cell's body verbatim");

    // Production anchor shape (text_execute.rs execute path).
    let body_source = quarto_source_map::SourceInfo::substring(
        converted.source_info.clone(),
        code_start,
        code_start + code.len(),
    );
    let err =
        quarto_core::cell_options::partition_cell_options("python", code, body_source.clone())
            .expect_err("unclosed flow sequence must fail YAML parsing");

    let mapped = err
        .location()
        .and_then(|loc| loc.map_offset(0, &ctx))
        .or_else(|| body_source.map_offset(0, &ctx))
        .expect("yaml error anchor must map through the composition");
    assert_eq!(
        mapped.file_id, code_cell_file,
        "anchor must resolve into the owning code cell's virtual file, got {mapped:?}"
    );
    assert_eq!(
        mapped.location.row, 0,
        "the option line is row 0 of the cell body"
    );

    // Rendering half: a diagnostic anchored where production anchors
    // must label the owning cell and show its in-cell option line.
    let anchor = err.location().cloned().unwrap_or(body_source);
    let diagnostic =
        quarto_error_reporting::DiagnosticMessageBuilder::error("cell options are not valid YAML")
            .with_location(anchor)
            .build();
    let options = quarto_error_reporting::TextRenderOptions {
        enable_hyperlinks: false,
    };
    let rendered = strip_ansi(&diagnostic.to_text_with_renderer(Some(&ctx), &options, None));
    assert!(
        rendered.contains("notebook.ipynb[cell 2, code]:1:"),
        "diagnostic must label the owning code cell; got:\n{rendered}"
    );
    assert!(
        rendered.contains("#| error: [unclosed"),
        "diagnostic must show the cell's option line; got:\n{rendered}"
    );
    assert!(
        !rendered.contains("[cell 1"),
        "diagnostic must not fall back to cell 1; got:\n{rendered}"
    );
}

mod seam2 {
    use std::path::Path;
    use std::sync::Arc;

    use quarto_core::error::QuartoError;
    use quarto_core::format::Format;
    use quarto_core::pipeline::{render_qmd_to_preview_ast, run_pipeline};
    use quarto_core::project::{DocumentInfo, ProjectContext};
    use quarto_core::render::{BinaryDependencies, RenderContext};
    use quarto_core::stage::stages::{ParseDocumentStage, SourceConversionStage};
    use quarto_error_reporting::TextRenderOptions;
    use quarto_source_map::FileId;
    use quarto_system_runtime::{NativeRuntime, SystemRuntime};

    use super::{
        CROSS_CELL_FENCE_NOTEBOOK, FLAGSHIP_NOTEBOOK, ORIGINAL_FILE_ID, ipynb, strip_ansi,
    };

    /// Assert the seam-2 registration contract on `sc` (plan decision 6):
    /// FileId(0) under a "(converted by jupyter)" synthetic name with the
    /// converted markdown, the original notebook at ORIGINAL_FILE_ID, and
    /// the per-cell virtual files contiguous from FileId(2) in
    /// `Converted.files` order with logical content.
    fn assert_seam2_registration(
        sc: &quarto_source_map::SourceContext,
        notebook: &str,
        name: &str,
    ) {
        let converted = ipynb::convert_notebook(Path::new("notebook.ipynb"), notebook)
            .expect("reference conversion must succeed");

        let f0 = sc
            .get_file(FileId(0))
            .unwrap_or_else(|| panic!("{name}: FileId(0) must be registered"));
        assert!(
            f0.path.contains("(converted by jupyter)"),
            "{name}: FileId(0) must be the converted buffer under its synthetic name, got {:?}",
            f0.path
        );
        assert_eq!(
            f0.content.as_deref(),
            Some(converted.markdown.as_str()),
            "{name}: FileId(0) must hold the converted markdown"
        );

        let orig = sc
            .get_file(ORIGINAL_FILE_ID)
            .unwrap_or_else(|| panic!("{name}: original missing at ORIGINAL_FILE_ID"));
        assert_eq!(
            orig.content.as_deref(),
            Some(notebook),
            "{name}: ORIGINAL_FILE_ID must hold the raw notebook bytes"
        );

        for (i, (label, text)) in converted.files.iter().enumerate() {
            let id = FileId(ORIGINAL_FILE_ID.0 + 1 + i);
            let f = sc
                .get_file(id)
                .unwrap_or_else(|| panic!("{name}: cell {i} missing at {id:?}"));
            assert!(
                f.path.ends_with(label),
                "{name}: cell {i} label must end with {label:?}, got {:?}",
                f.path
            );
            assert_eq!(
                f.content.as_deref(),
                Some(text.as_str()),
                "{name}: cell {i} content"
            );
        }
    }

    /// Seam 2, error path (site pipeline.rs:896): a parse failure in a
    /// converted notebook must produce a `QuartoError::Parse` whose
    /// `source_context` holds the converted buffer at FileId(0), the
    /// original notebook at ORIGINAL_FILE_ID, and the per-cell virtual
    /// files — otherwise every diagnostic (a `Substring` over the
    /// converter's `Concat`) resolves against the wrong bytes. This arm
    /// registers only the raw JSON at FileId(0) today, which is also the
    /// live 7b-era percent/spin latent gap → RED until fixed.
    #[tokio::test]
    async fn stage_error_context_registers_converted_original_and_cells() {
        let temp = tempfile::TempDir::new().unwrap();
        let nb_path = temp.path().join("notebook.ipynb");
        std::fs::write(&nb_path, FLAGSHIP_NOTEBOOK).unwrap();
        let nb_path = nb_path.canonicalize().unwrap();

        let project = ProjectContext {
            dir: temp.path().to_path_buf(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path(&nb_path)],
            output_dir: temp.path().to_path_buf(),
            ..Default::default()
        };
        let doc = DocumentInfo::from_path(&nb_path);
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());

        let stages: Vec<Box<dyn quarto_core::stage::PipelineStage>> = vec![
            Box::new(SourceConversionStage::new()),
            Box::new(ParseDocumentStage::new()),
        ];
        let result = run_pipeline(
            FLAGSHIP_NOTEBOOK.as_bytes(),
            &nb_path.display().to_string(),
            &mut ctx,
            runtime,
            stages,
        )
        .await;
        let Err(err) = result else {
            panic!("kv-before-class in cell 2 must fail the parse stage");
        };
        let QuartoError::Parse(pe) = err else {
            panic!("expected QuartoError::Parse, got {err:#?}");
        };

        assert_seam2_registration(&pe.source_context, FLAGSHIP_NOTEBOOK, "stage-error");

        // The error-path flagship, end to end: rendering the REBUILT
        // context must label the owning cell and show its in-cell snippet.
        let options = TextRenderOptions {
            enable_hyperlinks: false,
        };
        let rendered: String = pe
            .diagnostics
            .iter()
            .map(|d| d.to_text_with_renderer(Some(&pe.source_context), &options, None))
            .collect::<Vec<_>>()
            .join("\n");
        let rendered = strip_ansi(&rendered);
        assert!(
            rendered.contains("notebook.ipynb[cell 2, markdown]:1:"),
            "error-path diagnostic must label the owning cell; got:\n{rendered}"
        );
        assert!(
            rendered.contains("![logo](images/logo.svg)"),
            "error-path diagnostic must show cell 2's in-cell snippet; got:\n{rendered}"
        );
    }

    /// Seam 2, preview output (site pipeline.rs:1108): the q2-preview
    /// JSON envelope's `source_context` must be the parse stage's honest
    /// one — converted buffer at FileId(0), original + per-cell files
    /// registered — not a fresh context holding only the raw notebook
    /// JSON. Today the stage-populated context is dropped on the floor
    /// → RED until fixed.
    #[tokio::test]
    async fn preview_ast_output_context_carries_converted_original_and_cells() {
        let temp = tempfile::TempDir::new().unwrap();
        let nb_path = temp.path().join("notebook.ipynb");
        std::fs::write(&nb_path, CROSS_CELL_FENCE_NOTEBOOK).unwrap();
        let nb_path = nb_path.canonicalize().unwrap();

        let project = ProjectContext {
            dir: temp.path().to_path_buf(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path(&nb_path)],
            output_dir: temp.path().to_path_buf(),
            ..Default::default()
        };
        let doc = DocumentInfo::from_path(&nb_path);
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        // This test exercises SourceContext plumbing, not execution; the
        // notebook declares a kernelspec, and jupyter isn't installed in
        // CI/dev shells, so pass execution through inert (ExecutionPolicy
        // gate returns before the engine's availability check) rather
        // than fail on a missing runtime.
        ctx.execution_policy = quarto_core::engine::ExecutionPolicy::None;
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());

        let out = render_qmd_to_preview_ast(
            CROSS_CELL_FENCE_NOTEBOOK.as_bytes(),
            &nb_path.display().to_string(),
            &mut ctx,
            runtime,
            None,
            Vec::new(),
        )
        .await
        .expect("valid cross-cell notebook must render through the preview pipeline");

        assert_seam2_registration(
            &out.source_context,
            CROSS_CELL_FENCE_NOTEBOOK,
            "preview-ast",
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
// Phase 2, Pass-1 admission + launch-free proof: a multi-notebook
// project is admitted through production discovery (explicit render
// list — `.ipynb` is never auto-discovered, a deliberate 2026-08-18
// decision) and every admitted notebook converts natively with zero
// engine launches.
// ═══════════════════════════════════════════════════════════════════

mod discovery_pass1 {
    use std::sync::Arc;

    use quarto_core::engine::ExecutionPolicy;
    use quarto_core::engine::content_processors::ORIGINAL_FILE_ID;
    use quarto_core::engine::jupyter::find_jupyter_call_count;
    use quarto_core::format::Format;
    use quarto_core::pipeline::render_qmd_to_preview_ast;
    use quarto_core::project::ProjectContext;
    use quarto_core::render::{BinaryDependencies, RenderContext};
    use quarto_system_runtime::{NativeRuntime, SystemRuntime};

    const NB1: &str = r##"{"cells":[{"cell_type":"markdown","metadata":{},"source":["# Notebook one\n"]}],"metadata":{"kernelspec":{"name":"python3"}},"nbformat":4,"nbformat_minor":5}"##;
    const NB2: &str = r##"{"cells":[{"cell_type":"markdown","metadata":{},"source":["Intro\n"]},{"cell_type":"code","metadata":{},"source":["print(1)\n"]}],"metadata":{"kernelspec":{"name":"python3"}},"nbformat":4,"nbformat_minor":5}"##;
    const NB3: &str = r##"{"cells":[{"cell_type":"markdown","metadata":{},"source":["# Notebook three\n\nText.\n"]}],"metadata":{"kernelspec":{"name":"python3"}},"nbformat":4,"nbformat_minor":5}"##;

    /// Pass-1 (plan Phase 2): a project of N notebooks is admitted through
    /// production discovery — `ProjectContext::discover` builds
    /// `RenderableExtensions` from `builtin_file_claims()` (`.ipynb`
    /// included) and honors the explicit `project.render` allowlist — and
    /// every admitted notebook converts through the native ipynb content
    /// processor with **zero engine launches**: the jupyter PATH-lookup
    /// counter reads exactly 1 afterward, the single memoized lookup every
    /// render's registry construction pays (`EngineRegistry::new()` →
    /// `JupyterEngine::new()`, OnceLock-capped for the process lifetime).
    /// A count above 1 would mean Pass-1 probed per notebook (the
    /// bd-c5u2g cache regression); a missing admission fails the file-list
    /// assertion; a wire-path conversion fails the ORIGINAL_FILE_ID
    /// registration assertion (the wire path can only produce
    /// `Generated(By::unknown())` placeholders).
    #[tokio::test]
    async fn project_of_notebooks_admitted_and_converted_launch_free() {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            temp.path().join("_quarto.yml"),
            "project:\n  render:\n    - \"**/*.qmd\"\n    - \"**/*.ipynb\"\n",
        )
        .unwrap();
        std::fs::write(temp.path().join("doc.qmd"), "# Hello\n").unwrap();
        std::fs::write(temp.path().join("nb1.ipynb"), NB1).unwrap();
        std::fs::write(temp.path().join("nb2.ipynb"), NB2).unwrap();
        std::fs::write(temp.path().join("nb3.ipynb"), NB3).unwrap();

        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());

        let project = ProjectContext::discover(temp.path(), runtime.as_ref())
            .expect("project with an explicit notebook render list must discover");

        // Admission: all three notebooks in the walk output, alongside the
        // qmd — and nothing else (`project.render` is an allowlist).
        let mut names: Vec<&str> = project
            .files
            .iter()
            .filter_map(|d| d.input.file_name().and_then(|f| f.to_str()))
            .collect();
        names.sort_unstable();
        assert_eq!(
            names,
            vec!["doc.qmd", "nb1.ipynb", "nb2.ipynb", "nb3.ipynb"],
            "the explicit render list must admit every notebook; got {names:?}"
        );

        // Pass-1 conversion, per admitted notebook, through the preview
        // pipeline the real render drives (SourceConversionStage is its
        // first stage). ExecutionPolicy::None: Pass-1 is conversion+parse;
        // the policy gate returns before the engine availability check, and
        // jupyter is not installed in dev shells/CI.
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let notebooks: Vec<_> = project
            .files
            .iter()
            .filter(|d| d.input.extension().is_some_and(|e| e == "ipynb"))
            .collect();
        assert_eq!(notebooks.len(), 3, "all three notebooks must be admitted");
        for doc in notebooks {
            let bytes = std::fs::read(&doc.input).unwrap();
            let name = doc.input.display().to_string();
            let mut ctx = RenderContext::new(&project, doc, &format, &binaries);
            ctx.execution_policy = ExecutionPolicy::None;
            let out = render_qmd_to_preview_ast(
                &bytes,
                &name,
                &mut ctx,
                runtime.clone(),
                None,
                Vec::new(),
            )
            .await
            .unwrap_or_else(|e| panic!("{name} must convert+render in Pass-1: {e}"));

            // Native routing: the preview context registers the raw notebook
            // bytes at ORIGINAL_FILE_ID — the wire path's placeholder can
            // never do this (see `processor_conversion_carries_ephemeral_files`).
            let orig = out
                .source_context
                .get_file(ORIGINAL_FILE_ID)
                .unwrap_or_else(|| panic!("{name}: original missing at ORIGINAL_FILE_ID"));
            assert_eq!(
                orig.content.as_deref(),
                Some(std::str::from_utf8(&bytes).unwrap()),
                "{name}: ORIGINAL_FILE_ID must hold the raw notebook bytes"
            );
        }

        // Launch-free: exactly one jupyter PATH lookup for the whole pass —
        // the one every render already pays at registry construction. The
        // absolute (not delta) form is stable under both nextest (fresh
        // process per test) and plain `cargo test` (shared process, cache
        // possibly pre-warmed): the OnceLock cap means the counter can never
        // legitimately exceed 1, so equality pins Pass-1 to zero additional
        // probes regardless of who warmed it.
        assert_eq!(
            find_jupyter_call_count(),
            1,
            "Pass-1 over 3 notebooks must leave find_jupyter at its process cap of 1; \
             more would mean a per-notebook PATH probe (bd-c5u2g regression)"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
// Phase 2 scaling gate (plan open question 1): one virtual file per
// cell means N files in `SourceContext`, and `Concat`::map_offset`
// walks pieces linearly — quantify both before wiring (measured
// 2026-09-25; numbers recorded in the plan).
// ═══════════════════════════════════════════════════════════════════

/// An N-cell markdown notebook with realistic cell size (heading + two
/// short paragraphs, ~150 bytes/cell).
fn build_notebook(cells: usize) -> String {
    let mut json = String::from("{\"cells\":[");
    for i in 0..cells {
        if i > 0 {
            json.push(',');
        }
        let body = format!(
            "## Section {i}\n\nParagraph one of section {i} with some prose to fill the line.\n\nParagraph two mentions [a link](./target-{i}.html) and more text.\n"
        );
        let escaped = body.replace('\n', "\\n");
        json.push_str(&format!(
            "{{\"cell_type\":\"markdown\",\"metadata\":{{}},\"source\":[\"{escaped}\"]}}"
        ));
    }
    json.push_str("],\"metadata\":{\"kernelspec\":{\"name\":\"python3\"}},\"nbformat\":4,\"nbformat_minor\":5}");
    json
}

#[test]
#[ignore = "scaling gate: run explicitly (cargo nextest run -p quarto-core --test integration scaling_gate --run-ignored ignored-only --nocapture)"]
fn scaling_gate_per_cell_registration_and_render() {
    use std::time::Instant;

    for cells in [125usize, 250, 500, 1000] {
        let notebook = build_notebook(cells);

        let t = Instant::now();
        let converted = ipynb::convert_notebook(Path::new("notebook.ipynb"), &notebook)
            .expect("generated notebook must convert");
        let convert = t.elapsed();

        let t = Instant::now();
        let mut ctx = SourceContext::new();
        register_cells(&mut ctx, &converted, &notebook);
        let register = t.elapsed();

        // map_offset spread across all pieces (N calls, one per cell start).
        let t = Instant::now();
        let mut hits = 0usize;
        for i in 0..cells {
            let offset = converted.source_info.length() * i / cells;
            hits += converted.source_info.map_offset(offset, &ctx).is_some() as usize;
        }
        let map_n = t.elapsed();

        // Diagnostic render rooted in the LAST cell (worst-case Concat walk),
        // shaped exactly like a rerooted Err-path diagnostic: a Substring
        // over the converted Concat.
        let end = converted.source_info.length();
        let location = quarto_source_map::SourceInfo::substring(
            converted.source_info.clone(),
            end - 3,
            end - 2,
        );
        let diagnostic = quarto_error_reporting::DiagnosticMessageBuilder::error("synthetic")
            .with_location(location)
            .build();
        let options = quarto_error_reporting::TextRenderOptions {
            enable_hyperlinks: false,
        };
        let t = Instant::now();
        let rendered = diagnostic.to_text_with_renderer(Some(&ctx), &options, None);
        let render = t.elapsed();
        assert!(!rendered.is_empty());

        eprintln!(
            "scaling gate cells={cells}: convert={convert:?} register={register:?} \
             map{cells}={map_n:?} (hits={hits}) render_last_cell={render:?} rendered={}",
            rendered.len()
        );
    }
}
