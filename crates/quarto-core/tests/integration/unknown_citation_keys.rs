//! A citation key no reference defines (GH #1111).
//!
//! Pandoc's citeproc warns `citation X not found` and renders the item as
//! bold `X?`; the render goes on. Q2 used to fail the HTML render with an
//! uncoded "Reference 'X' not found" and no location, and on Typst let
//! `typst compile` fail on the generated `.typ`. Both now warn with
//! `Q-23-2` at the key in the `.qmd` and render `**X?**`.

use std::path::Path;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::render_to_file::{RenderToFileOptions, render_document_to_file};
use quarto_error_reporting::{DiagnosticKind, DiagnosticMessage};
use quarto_source_map::SourceContext;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

const REFS_BIB: &str = "@article{doe2020,\n  author = {Doe, Jane},\n  title = {A title},\n  journal = {A Journal},\n  year = {2020}\n}\n";

/// The body line holding the citations; it is line 6 (row 5) of the file.
const BODY: &str = "See [@doe2020; @missing2020] and @alsomissing, then [@missing2020].";

struct Rendered {
    output: String,
    diagnostics: Vec<DiagnosticMessage>,
    source_context: SourceContext,
}

/// Render the fixture to `format`, writing `index.<extension>`.
fn render(format: &str, extension: &str) -> Rendered {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("index.qmd");
    std::fs::write(
        &input_path,
        format!("---\nformat: {format}\nbibliography: refs.bib\n---\n\n{BODY}\n"),
    )
    .unwrap();
    std::fs::write(project_dir.join("refs.bib"), REFS_BIB).unwrap();
    let output_path = project_dir.join(format!("index.{extension}"));
    let options = RenderToFileOptions {
        output_path: Some(output_path.clone()),
        ..Default::default()
    };
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_document_to_file(
        &input_path,
        format,
        &options,
        None,
        runtime,
        None,
        None,
        None,
    )
    .unwrap_or_else(|e| panic!("an unknown citation key must not fail the {format} render: {e}"));
    Rendered {
        output: std::fs::read_to_string(&output_path).unwrap(),
        diagnostics: result.render_output.diagnostics,
        source_context: result.render_output.source_context,
    }
}

/// `(file name, row, column)` of a diagnostic's location, all 0-based.
fn location(d: &DiagnosticMessage, ctx: &SourceContext) -> (String, usize, usize) {
    let loc = d
        .location
        .as_ref()
        .expect("the warning must carry a location");
    let mapped = loc
        .map_offset(0, ctx)
        .expect("the location must map to a file");
    let file = ctx.get_file(mapped.file_id).expect("a known file");
    let name = Path::new(&file.path)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    (name, mapped.location.row, mapped.location.column)
}

/// One `Q-23-2` warning per unknown key, at the key's first citation.
fn assert_unknown_key_warnings(rendered: &Rendered) {
    let warnings: Vec<&DiagnosticMessage> = rendered
        .diagnostics
        .iter()
        .filter(|d| d.code.as_deref() == Some("Q-23-2"))
        .collect();
    assert_eq!(warnings.len(), 2, "{:#?}", rendered.diagnostics);
    for (warning, key) in warnings.iter().zip(["missing2020", "alsomissing"]) {
        assert_eq!(warning.kind, DiagnosticKind::Warning);
        let text = warning.to_text(None);
        assert!(text.contains(key), "{text}");
        // The span starts at the key, right after its `@`.
        let column = BODY.find(&format!("@{key}")).unwrap() + 1;
        assert_eq!(
            location(warning, &rendered.source_context),
            ("index.qmd".to_string(), 5, column),
            "{key}"
        );
    }
}

#[test]
fn html_unknown_key_warns_at_the_key_and_renders_a_bold_marker() {
    let rendered = render("html", "html");
    assert_unknown_key_warnings(&rendered);
    let html = &rendered.output;
    assert!(html.contains("<strong>missing2020?</strong>"), "{html}");
    assert!(html.contains("<strong>alsomissing?</strong>"), "{html}");
    assert!(
        html.contains("Doe 2020"),
        "the known key must still resolve:\n{html}"
    );
}

#[test]
fn typst_unknown_key_warns_at_the_key_and_keeps_known_keys_for_typst() {
    let rendered = render("typst", "typ");
    assert_unknown_key_warnings(&rendered);
    let typ = &rendered.output;
    assert!(typ.contains("#strong[missing2020?]"), "{typ}");
    assert!(typ.contains("#strong[alsomissing?]"), "{typ}");
    assert!(
        typ.contains("@doe2020"),
        "the known key must stay a Typst citation:\n{typ}"
    );
    assert!(!typ.contains("@missing2020"), "{typ}");
    assert!(!typ.contains("@alsomissing"), "{typ}");
}
