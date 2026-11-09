//! RIU-3a: the Positron cache as a record source, native end to end.
#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use quarto_core::engine::preview_record::{SourceInputsOptions, compute_source_inputs_with};
use quarto_core::project::ProjectContext;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

const PY_AND_JULIA: &str = "```{python}\nx = 1\n```\n\n```{julia}\ny = 2\n```\n";

/// The real engine registry (the project's discovered one) resolves a Python cell to `jupyter`
/// and a Julia cell to `julia`, which is what the language gate needs to admit both.
#[tokio::test]
async fn the_real_registry_owns_python_with_jupyter_and_julia_with_julia() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("doc.qmd");
    std::fs::write(&path, PY_AND_JULIA).unwrap();
    let path = path.canonicalize().unwrap();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&path, runtime.as_ref()).unwrap();

    let inputs = compute_source_inputs_with(
        &path,
        &project,
        runtime.clone(),
        SourceInputsOptions {
            ownership: true,
            registry: None,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(inputs.owner_of("python"), "jupyter");
    assert_eq!(inputs.owner_of("julia"), "julia");
    assert_eq!(inputs.owner_of("Python"), "jupyter", "case-insensitive");
    assert_eq!(
        inputs.owner_of("ojs"),
        "",
        "nobody owns it: no gate admits it"
    );

    // Not asked for: not computed, nothing paid.
    let plain = compute_source_inputs_with(&path, &project, runtime, Default::default())
        .await
        .unwrap();
    assert!(plain.ownership.is_none());
}

// ── The durable fixtures, end to end through the real preview pipeline ────────────────────────

use std::path::Path;

use quarto_core::engine::{EngineRegistry, MarkdownEngine};
use quarto_core::format::Format;
use quarto_core::pandoc_request::captures::CapturePayload;
use quarto_core::pipeline::render_qmd_to_preview_ast;
use quarto_core::project::DocumentInfo;
use quarto_core::record::{
    Record, UNCLAIMED_KEY, content_hash, import_positron_cache, import_record_source,
};
use quarto_core::render::{BinaryDependencies, RenderContext};

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/positron-cache")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Import a fixture cache for a fixture document through the real source inputs (the real
/// registry's ownership), exactly as the walk does, and return the record plus the dir and path.
async fn import_fixture(stem: &str) -> (Record, tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("doc.qmd");
    std::fs::write(&path, fixture(&format!("{stem}.qmd"))).unwrap();
    let path = path.canonicalize().unwrap();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&path, runtime.as_ref()).unwrap();
    let inputs = compute_source_inputs_with(
        &path,
        &project,
        runtime,
        SourceInputsOptions {
            ownership: true,
            registry: None,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let import = import_positron_cache(fixture(&format!("{stem}.cache.ipynb")).as_bytes()).unwrap();
    let record = import_record_source(&import, &inputs, Some("doc.qmd".into()));
    (record, dir, path)
}

/// Preview-render the fixture document with `record` and an empty registry: no engine can run,
/// so every output in the AST came from the record.
async fn preview_ast(path: &Path, record: &Record) -> String {
    try_preview_ast(path, record).await.unwrap()
}

/// As [`preview_ast`], but a render error (for example a parse error) is returned, not unwrapped.
async fn try_preview_ast(path: &Path, record: &Record) -> Result<String, String> {
    let source = std::fs::read_to_string(path).unwrap();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(path, runtime.as_ref()).unwrap();
    let doc_info = DocumentInfo::from_path(path);
    let format = Format::from_format_string("q2-preview").unwrap();
    let bins = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc_info, &format, &bins);
    let mut registry = EngineRegistry::empty();
    registry.register(Arc::new(MarkdownEngine::new()));
    render_qmd_to_preview_ast(
        source.as_bytes(),
        &path.display().to_string(),
        &mut ctx,
        runtime,
        Some(Arc::new(registry)),
        CapturePayload::Record(Arc::new(record.clone())),
    )
    .await
    .map(|out| out.ast_json)
    .map_err(|e| e.to_string())
}

fn cached_hashes(stem: &str) -> Vec<String> {
    let v: serde_json::Value =
        serde_json::from_str(&fixture(&format!("{stem}.cache.ipynb"))).unwrap();
    v["cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            c["metadata"]["quarto_content_hash"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect()
}

/// The hash Positron wrote equals the one q2 computes through the splice path (the sugared
/// snapshot): every cached cell is claimed (nothing unclaimed) and every cell is in the record.
/// The R fixture's first cell is labeled (`#| label: fig-cars`), the case where a raw scan and
/// the sugared text differ.
#[tokio::test]
async fn every_cached_hash_equals_the_one_q2_computes_through_the_splice_path() {
    for (stem, cells) in [("python", 2), ("r", 3), ("julia", 3)] {
        let (record, _dir, _) = import_fixture(stem).await;
        assert_eq!(
            record.cells.len(),
            cells,
            "{stem}: every cached cell paired with a unit"
        );
        assert_eq!(record.metadata.extra[UNCLAIMED_KEY], 0, "{stem}");
        let got: Vec<_> = record
            .cells
            .iter()
            .map(|c| c.metadata.quarto_content_hash.clone())
            .collect();
        assert_eq!(got, cached_hashes(stem), "{stem}");
    }
    // The labeled R cell kept its `#|` line in the source the hash was taken from.
    let (r, _d, _) = import_fixture("r").await;
    assert!(r.cells[0].source.concat().starts_with("#| label: fig-cars"));
    assert_eq!(
        r.cells[0].metadata.quarto_label.as_deref(),
        Some("fig-cars")
    );
    assert_eq!(
        r.cells[0].metadata.quarto_content_hash,
        content_hash(&r.cells[0].source.concat())
    );
}

#[tokio::test]
async fn the_python_fixture_shows_its_outputs_with_no_kernel() {
    let (record, _dir, path) = import_fixture("python").await;
    let ast = preview_ast(&path, &record).await;
    assert!(ast.contains("sum: PUSHED-FROM-FILE"), "the stream shows");
    assert!(ast.contains("[1, 4, 9, 16]"), "the result shows");
}

/// The Julia owner is `julia` in the real registry, and the gate admits it: the hand-built
/// fixture's outputs show.
#[tokio::test]
async fn the_hand_built_julia_fixture_shows_its_outputs_with_no_kernel() {
    let (record, _dir, path) = import_fixture("julia").await;
    let ast = preview_ast(&path, &record).await;
    assert!(ast.contains("hello from julia"));
    assert!(ast.contains("a string result"));
    assert!(
        ast.contains("figure-html/cell-3-output-1.png"),
        "the multi-MIME plot bundle becomes a figure reference"
    );
}

/// R is owned by `knitr`, which the gate admits for the exact token `r` (KNR-5), so the R
/// fixture imports and shows through the real registry. R is not an acceptance case of this
/// epic; this pins that the conversion calls the gate with the right owner and language.
#[tokio::test]
async fn the_r_fixture_converts_through_the_knitr_owner() {
    let (record, _dir, path) = import_fixture("r").await;
    assert_eq!(record.cells.len(), 3);
    let ast = preview_ast(&path, &record).await;
    assert!(ast.contains("speed"), "the printed data frame shows");
}

/// A document edited after import: only the edited cell loses its output and the page is stale.
#[tokio::test]
async fn an_edit_after_import_keeps_the_other_outputs_and_flags_the_page() {
    let (record, dir, path) = import_fixture("python").await;
    let edited = fixture("python.qmd").replace("[v * v for v in x]", "[v + 1 for v in x]");
    std::fs::write(&path, &edited).unwrap();
    let ast = preview_ast(&path, &record).await;
    assert!(
        ast.contains("sum: PUSHED-FROM-FILE"),
        "the untouched cell keeps its output"
    );
    assert!(
        !ast.contains("[1, 4, 9, 16]"),
        "the edited cell's output is not shown"
    );
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&path, runtime.as_ref()).unwrap();
    let inputs = compute_source_inputs_with(&path, &project, runtime, Default::default())
        .await
        .unwrap();
    assert!(quarto_core::record::imported_record_is_stale(
        &record,
        &String::from_utf8_lossy(&inputs.input_qmd),
        &inputs.sugared_blocks,
    ));
    drop(dir);
}

// ── EXR-16: the 21 conformance cases through import, convert and splice ──────────────────────

fn conformance_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/positron-conformance")
}

/// What Positron would write for a conformance document: one code cell per cell Positron's
/// parser found (`positron-expected.json`), carrying Positron\'s own id and hash, and one
/// stream output `MARK-<case>-<n>` so the page shows which cells received outputs.
fn positron_cache_for(case: &str, expected: &serde_json::Value) -> Vec<u8> {
    let cells: Vec<_> = expected[case]["cells"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(n, c)| {
            serde_json::json!({
                "cell_type": "code", "execution_count": null,
                "metadata": {
                    "quarto_cell_id": c["id"],
                    "quarto_content_hash": c["content_hash"],
                },
                "source": ["x"],
                "outputs": [{
                    "output_type": "stream", "name": "stdout",
                    "text": [format!("MARK-{case}-{n}\n")],
                }],
            })
        })
        .collect();
    serde_json::to_vec(&serde_json::json!({
        "nbformat": 4, "nbformat_minor": 5,
        "metadata": {"quarto_source": "file:///d/doc.qmd", "quarto_cache_version": 1},
        "cells": cells,
    }))
    .unwrap()
}

/// Which of Positron's cached cells (by position in Positron\'s list) show on the page after
/// import, convert and splice, or the render error.
async fn cells_shown_for_case(
    case: &str,
    expected: &serde_json::Value,
) -> Result<Vec<usize>, String> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("doc.qmd");
    // Bytes exactly as the fixture has them (`crlf` matters).
    std::fs::write(
        &path,
        std::fs::read(conformance_dir().join(format!("{case}.qmd"))).unwrap(),
    )
    .unwrap();
    let path = path.canonicalize().unwrap();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&path, runtime.as_ref()).unwrap();
    let inputs = compute_source_inputs_with(
        &path,
        &project,
        runtime,
        SourceInputsOptions {
            ownership: true,
            registry: None,
            format: None,
        },
    )
    .await
    .map_err(|e| e.to_string())?;
    let import = import_positron_cache(&positron_cache_for(case, expected)).unwrap();
    let record = import_record_source(&import, &inputs, Some("doc.qmd".into()));
    let ast = try_preview_ast(&path, &record).await?;
    let n_cells = expected[case]["cells"].as_array().unwrap().len();
    Ok((0..n_cells)
        .filter(|n| ast.contains(&format!("MARK-{case}-{n}")))
        .collect())
}

/// `(case, cells of Positron's list that show, why)`. A cell Positron lists that q2 does not
/// treat as one never shows; a cell of an engine the language gate refuses never shows; a
/// document q2 rejects does not render. `Err` rows are the parse-error cases.
const SWEEP: &[(&str, Result<&[usize], ()>, &str)] = &[
    ("plain", Ok(&[0]), "agree"),
    (
        "label_comment",
        Ok(&[0]),
        "hashes agree although the ids differ",
    ),
    (
        "r_label_comment",
        Ok(&[0]),
        "R through the knitr owner; ids differ, hashes agree",
    ),
    (
        "brace_label_r",
        Err(()),
        "q2 rejects `{r, label=foo}` with Q-2-36",
    ),
    (
        "brace_label_bare",
        Err(()),
        "q2 rejects `{python, fig-x}` with Q-2-36",
    ),
    (
        "brace_id",
        Ok(&[]),
        "FINDING 1: the masked document rewrites `{python #bar}` to `{#bar python}`, which the unit scan does not see; the cached cell is unclaimed and the page reads stale",
    ),
    ("dotted_fence", Ok(&[]), "neither parser finds a cell"),
    (
        "marimo_spaced",
        Ok(&[0]),
        "FINDING 2: `{python .marimo}` is language python owned by jupyter, so the import fills it",
    ),
    (
        "marimo_dotted",
        Ok(&[]),
        "marimo owns `python.marimo`; the gate refuses",
    ),
    ("lang_case", Ok(&[0]), "agree"),
    ("blockquote", Ok(&[]), "Positron sees no cell"),
    ("list", Ok(&[]), "Positron sees no cell"),
    (
        "four_backtick",
        Ok(&[]),
        "Positron's phantom cell has no q2 unit",
    ),
    (
        "display_fence",
        Ok(&[]),
        "Positron's phantom cell has no q2 unit",
    ),
    (
        "tilde_fence",
        Err(()),
        "q2 rejects `~~~{python}` (braces are reserved); Positron finds no cell",
    ),
    ("crlf", Ok(&[0]), "agree"),
    ("raw_html", Ok(&[]), "neither parser finds a cell"),
    (
        "two_cells",
        Ok(&[1]),
        "an r cell makes knitr own the document, so the python cell is gated out (owner knitr, language python)",
    ),
    (
        "duplicate_pair",
        Ok(&[0, 1]),
        "same hash twice: paired by occurrence",
    ),
    (
        "span_before_cell",
        Ok(&[0]),
        "the span does not shift the cell",
    ),
    ("unicode_cell", Ok(&[0]), "ids differ, hashes agree"),
];

#[tokio::test]
async fn the_sweep_table_covers_exactly_the_conformance_cases() {
    let on_disk: std::collections::BTreeSet<String> = std::fs::read_dir(conformance_dir())
        .unwrap()
        .filter_map(|e| {
            let n = e.unwrap().file_name().to_string_lossy().to_string();
            n.strip_suffix(".qmd").map(str::to_string)
        })
        .collect();
    let table: std::collections::BTreeSet<String> =
        SWEEP.iter().map(|(n, ..)| n.to_string()).collect();
    assert_eq!(SWEEP.len(), 21);
    assert_eq!(table, on_disk);
}

#[tokio::test]
async fn each_conformance_case_shows_outputs_on_exactly_the_expected_units() {
    let expected: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(conformance_dir().join("positron-expected.json")).unwrap(),
    )
    .unwrap();
    let mut wrong = Vec::new();
    for (case, want, why) in SWEEP {
        let got = cells_shown_for_case(case, &expected).await;
        let ok = match (&got, want) {
            (Ok(g), Ok(w)) => g.as_slice() == *w,
            (Err(_), Err(())) => true,
            _ => false,
        };
        if !ok {
            wrong.push(format!("{case} ({why}): want {want:?}, got {got:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

// ── A labeled Python cell with `fig-cap`, through import, convert and splice ─────────────────

/// A 1x1 PNG.
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNgYGD4DwABBAEAwS2OUAAAAABJRU5ErkJggg==";

#[tokio::test]
async fn a_labeled_python_cell_with_fig_cap_shows_its_figure_and_caption() {
    let body = "#| label: fig-x\n#| fig-cap: A squares plot\nplot_it()";
    let doc = format!("Intro.\n\n```{{python}}\n{body}\n```\n");
    let cache = serde_json::to_vec(&serde_json::json!({
        "nbformat": 4, "nbformat_minor": 5,
        "metadata": {"quarto_source": "file:///d/doc.qmd", "quarto_cache_version": 1},
        "cells": [{
            "cell_type": "code", "execution_count": null,
            "metadata": {
                "quarto_cell_id": "0-unused-unlabeled",
                "quarto_content_hash": content_hash(body),
            },
            "source": ["plot_it()"],
            "outputs": [{
                "output_type": "display_data",
                "data": {"image/png": PNG, "text/plain": "<Figure>"},
                "metadata": {},
            }],
        }],
    }))
    .unwrap();

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("doc.qmd");
    std::fs::write(&path, &doc).unwrap();
    let path = path.canonicalize().unwrap();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&path, runtime.as_ref()).unwrap();
    let inputs = compute_source_inputs_with(
        &path,
        &project,
        runtime,
        SourceInputsOptions {
            ownership: true,
            registry: None,
            format: None,
        },
    )
    .await
    .unwrap();
    let import = import_positron_cache(&cache).unwrap();
    let record = import_record_source(&import, &inputs, Some("doc.qmd".into()));
    assert_eq!(record.cells.len(), 1, "the labeled cell is claimed");
    assert_eq!(record.metadata.extra[UNCLAIMED_KEY], 0);
    assert_eq!(
        record.cells[0].metadata.quarto_label.as_deref(),
        Some("fig-x")
    );

    let ast = try_preview_ast(&path, &record).await.unwrap();
    // The labeled cell became a figure float with its identifier, the image the record's bundle
    // produced, and the `fig-cap` text as the caption (once in the figure, once in the float slot).
    assert!(
        ast.contains(r#"\"identifier\":\"fig-x\""#),
        "float target fig-x"
    );
    assert!(ast.contains(r#""t":"Image""#), "the figure image shows");
    assert!(
        ast.contains("figure-html/cell-1-output-1.png"),
        "the image is a figure file"
    );
    assert!(
        ast.contains(r#""c":"squares""#) && ast.contains(r#""c":"A""#),
        "the fig-cap text shows"
    );
    assert!(
        ast.contains("caption_long"),
        "the caption is carried by the float"
    );
}
