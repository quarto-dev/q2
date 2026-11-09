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
    let path = quarto_system_runtime::canonicalize(&path).unwrap();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&path, runtime.as_ref()).unwrap();

    let inputs = compute_source_inputs_with(
        &path,
        &project,
        runtime.clone(),
        SourceInputsOptions {
            ownership: true,
            registry: None,
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
    let path = quarto_system_runtime::canonicalize(&path).unwrap();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&path, runtime.as_ref()).unwrap();
    let inputs = compute_source_inputs_with(
        &path,
        &project,
        runtime,
        SourceInputsOptions {
            ownership: true,
            registry: None,
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
    .unwrap()
    .ast_json
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
