//! Cross-engine output-parity suite (bd-gthycd33, decision 4).
//!
//! The engine API is text-in/text-out (and will stay that way for
//! future user-extensible engines), so uniformity of the post-engine
//! markdown *structure* cannot be enforced at the API level. These
//! tests enforce it empirically: equivalent minimal inputs run
//! through knitr and jupyter must produce post-engine markdown that
//! parses to the same block structure. Structural divergence between
//! engines is exactly the class of bug bd-gthycd33 was — the preview
//! capture splice, CSS selectors (`.cell .cell-output-* pre code`),
//! and cell-level transforms all key on one shared shape.
//!
//! "Same structure" is deliberately *shape* parity, not byte parity:
//! we compare block kinds recursively plus the semantic classes the
//! downstream consumers rely on (`cell`, `cell-code`, `cell-output`),
//! ignoring content bytes, language classes (`r` vs `python`), and
//! output-subtype classes. The subtype tolerance is intentional and
//! reviewed: the same logical "expression evaluates to a value" is
//! `.cell-output-stdout` under knitr (R autoprints to stdout) but
//! `.cell-output-display` under jupyter (an execute_result) — a
//! Q1-inherited semantic difference with identical block structure.
//! See claude-notes/plans/2026-07-01-bd-gthycd33-jupyter-cell-wrapper.md.
//!
//! The suite needs BOTH engines installed (R + knitr, jupyter +
//! ipykernel); each test skips with a note when either is missing.

use std::path::PathBuf;
use std::sync::Arc;

use quarto_core::engine::EngineRegistry;
use quarto_core::engine::preview_record::record_capture;
use quarto_core::project::ProjectContext;
use quarto_pandoc_types::{Block, Pandoc};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn both_engines_available() -> bool {
    let registry = EngineRegistry::default();
    ["knitr", "jupyter"]
        .iter()
        .all(|name| registry.get(name).is_some_and(|e| e.is_available()))
}

fn fixture(
    content: &str,
) -> (
    tempfile::TempDir,
    PathBuf,
    ProjectContext,
    Arc<dyn SystemRuntime>,
) {
    let dir = tempfile::tempdir().unwrap();
    let qmd_path = dir.path().join("doc.qmd");
    std::fs::write(&qmd_path, content).unwrap();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&qmd_path, runtime.as_ref()).unwrap();
    (dir, qmd_path, project, runtime)
}

/// Run the real engine over `content` (via `record_capture`, the same
/// producer path `q2 preview` / `q2 provide-hub` use — see
/// capture_splice_engines.rs for why pollster and not #[tokio::test])
/// and parse the capture's post-engine markdown.
fn post_engine_ast(content: &str, engine: &str) -> Pandoc {
    let (_tmp, path, project, runtime) = fixture(content);
    let captures =
        pollster::block_on(record_capture(&path, &project, runtime, None)).expect("record_capture");
    assert_eq!(captures.len(), 1, "expected one capture for {engine}");
    let capture = &captures[0];
    assert_eq!(capture.engine_name, engine);
    let result_markdown = capture
        .result
        .get("markdown")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let (pandoc, _, _) = pampa::readers::qmd::read(
        result_markdown.as_bytes(),
        false,
        "capture-result.md",
        &mut std::io::sink(),
        false,
        None,
    )
    .expect("parse post-engine markdown");
    pandoc
}

/// The classes downstream consumers key on. Language classes and
/// output-subtype classes are deliberately not compared (see module
/// docs).
fn semantic_classes(classes: &[String]) -> Vec<String> {
    classes
        .iter()
        .filter(|c| matches!(c.as_str(), "cell" | "cell-code" | "cell-output"))
        .cloned()
        .collect()
}

fn block_kind(b: &Block) -> &'static str {
    match b {
        Block::Paragraph(_) => "Paragraph",
        Block::Plain(_) => "Plain",
        Block::CodeBlock(_) => "CodeBlock",
        Block::Div(_) => "Div",
        Block::RawBlock(_) => "RawBlock",
        Block::Header(_) => "Header",
        _ => "Other",
    }
}

/// Recursively assert the two block sequences have the same shape:
/// same length, same kinds, same semantic classes on Divs and
/// CodeBlocks, recursing into Div content. `path` locates a mismatch
/// in the failure message.
fn assert_blocks_parity(knitr: &[Block], jupyter: &[Block], path: &str) {
    assert_eq!(
        knitr.len(),
        jupyter.len(),
        "block count mismatch at '{path}': knitr {:?} vs jupyter {:?}",
        knitr.iter().map(block_kind).collect::<Vec<_>>(),
        jupyter.iter().map(block_kind).collect::<Vec<_>>()
    );
    for (i, (k, j)) in knitr.iter().zip(jupyter.iter()).enumerate() {
        let p = format!("{path}[{i}]");
        match (k, j) {
            (Block::Div(dk), Block::Div(dj)) => {
                assert_eq!(
                    semantic_classes(&dk.attr.1),
                    semantic_classes(&dj.attr.1),
                    "Div semantic classes differ at '{p}' (knitr {:?} vs jupyter {:?})",
                    dk.attr.1,
                    dj.attr.1
                );
                assert_blocks_parity(&dk.content, &dj.content, &p);
            }
            (Block::CodeBlock(ck), Block::CodeBlock(cj)) => {
                assert_eq!(
                    semantic_classes(&ck.attr.1),
                    semantic_classes(&cj.attr.1),
                    "CodeBlock semantic classes differ at '{p}' (knitr {:?} vs jupyter {:?})",
                    ck.attr.1,
                    cj.attr.1
                );
            }
            (a, b) => {
                assert_eq!(
                    std::mem::discriminant(a),
                    std::mem::discriminant(b),
                    "block kind mismatch at '{p}': knitr={} jupyter={}",
                    block_kind(a),
                    block_kind(b)
                );
            }
        }
    }
}

/// Equivalent single-cell documents for the two engines. Identical
/// prose so the whole-document shape comparison is meaningful; only
/// the cell language and body differ.
fn knitr_doc(cell_body: &str) -> String {
    format!(
        "---\ntitle: Parity\nengine: knitr\n---\n\nBefore.\n\n```{{r}}\n{cell_body}\n```\n\nAfter.\n"
    )
}

fn jupyter_doc(cell_body: &str) -> String {
    format!(
        "---\ntitle: Parity\nengine: jupyter\n---\n\nBefore.\n\n```{{python}}\n{cell_body}\n```\n\nAfter.\n"
    )
}

fn assert_engine_parity(knitr_cell: &str, jupyter_cell: &str) {
    if !both_engines_available() {
        eprintln!("Skipping test: parity suite needs both knitr and jupyter installed");
        return;
    }
    let k = post_engine_ast(&knitr_doc(knitr_cell), "knitr");
    let j = post_engine_ast(&jupyter_doc(jupyter_cell), "jupyter");
    assert_blocks_parity(&k.blocks, &j.blocks, "blocks");
}

#[test]
fn parity_stream_output() {
    assert_engine_parity("cat(\"hi\\n\")", "print(\"hi\")");
}

#[test]
fn parity_expression_value() {
    assert_engine_parity("1 + 1", "2 + 3");
}

/// Error *output shape* parity, under the sanctioned "show errors in
/// the output" mode (`#| error: true`). Without it, both engines fail
/// the whole pipeline on a cell error (Q1's default `error: false`
/// policy — see `parity_error_policy_behavior` below and
/// engine_error_policy.rs, bd-ohvl879u) and produce no capture, so
/// there would be no shape to compare.
#[test]
fn parity_error_output() {
    assert_engine_parity(
        "#| error: true\nstop(\"boom\")",
        "#| error: true\nraise Exception(\"boom\")",
    );
}

/// Error-policy *behavior* parity (bd-ohvl879u): an un-annotated cell
/// error must fail `record_capture` for BOTH engines — no capture is
/// produced. (Shape parity above covers the `error: true` mode;
/// this pins the default-deny policy itself.)
#[test]
fn parity_error_policy_behavior() {
    if !both_engines_available() {
        eprintln!("Skipping test: parity suite needs both knitr and jupyter installed");
        return;
    }
    let knitr = try_record(&knitr_doc("stop(\"boom\")"));
    let jupyter = try_record(&jupyter_doc("raise Exception(\"boom\")"));
    assert!(
        knitr.is_err(),
        "knitr must fail the render for an un-annotated cell error"
    );
    assert!(
        jupyter.is_err(),
        "jupyter must fail the render for an un-annotated cell error (bd-ohvl879u)"
    );
}

/// `record_capture` without unwrapping — for behavior (not shape) cases.
fn try_record(content: &str) -> Result<Vec<quarto_trace::EngineCapture>, String> {
    let (_tmp, path, project, runtime) = fixture(content);
    pollster::block_on(record_capture(&path, &project, runtime, None)).map_err(|e| format!("{e:?}"))
}

#[test]
fn parity_source_only_cell() {
    assert_engine_parity("x <- 1", "x = 1");
}

/// Whether the `python3` on PATH can import matplotlib. Approximation:
/// the kernel the daemon launches resolves `python3` the same way in
/// practice, and a false negative just skips the test. Keeps the
/// figure-parity case from failing on machines that have jupyter but
/// not matplotlib.
fn matplotlib_available() -> bool {
    std::process::Command::new("python3")
        .args(["-c", "import matplotlib"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Figure output parity (bd-5t6wvu7m): a plotting cell must produce
/// the same shape from both engines — a `.cell` wrapper with the
/// echoed source fence plus a figure display div containing an image
/// whose target lives under `<stem>_files/figure-html/`, and a
/// capture that reports non-empty `supporting_files` (the transport
/// contract bd-o8pr / bd-qbhp2cvv ride on).
///
/// The jupyter cell ends with `;` to suppress the execute_result text
/// (`[<matplotlib.lines.Line2D …>]`) — matplotlib both returns a
/// value and displays the figure, while knitr's `plot()` only
/// displays; the semicolon makes the two cells semantically
/// equivalent (one figure, no textual value).
#[test]
fn parity_plot_output() {
    if !both_engines_available() {
        eprintln!("Skipping test: parity suite needs both knitr and jupyter installed");
        return;
    }
    if !matplotlib_available() {
        eprintln!("Skipping test: figure parity needs matplotlib importable from python3");
        return;
    }

    let knitr = record_one(&knitr_doc("plot(1)"), "knitr");
    let jupyter = record_one(
        &jupyter_doc("import matplotlib.pyplot as plt\nplt.plot([1,2,3]);"),
        "jupyter",
    );

    let k_ast = parse_capture_markdown(&knitr);
    let j_ast = parse_capture_markdown(&jupyter);
    assert_blocks_parity(&k_ast.blocks, &j_ast.blocks, "blocks");

    for (engine, ast) in [("knitr", &k_ast), ("jupyter", &j_ast)] {
        let images = collect_image_targets(&ast.blocks);
        assert_eq!(
            images.len(),
            1,
            "{engine}: expected exactly one figure image; got {images:?}"
        );
        assert!(
            images[0].starts_with("doc_files/figure-html/"),
            "{engine}: figure must live under doc_files/figure-html/; got {}",
            images[0]
        );
    }

    for (engine, capture) in [("knitr", &knitr), ("jupyter", &jupyter)] {
        let supporting = capture
            .result
            .get("supporting_files")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        assert!(
            !supporting.is_empty(),
            "{engine}: a figure-producing run must report supporting_files"
        );
    }
}

/// Run one engine over `content` and return its single capture.
fn record_one(content: &str, engine: &str) -> quarto_trace::EngineCapture {
    let (_tmp, path, project, runtime) = fixture(content);
    let captures =
        pollster::block_on(record_capture(&path, &project, runtime, None)).expect("record_capture");
    assert_eq!(captures.len(), 1, "expected one capture for {engine}");
    assert_eq!(captures[0].engine_name, engine);
    captures[0].clone()
}

fn parse_capture_markdown(capture: &quarto_trace::EngineCapture) -> Pandoc {
    let result_markdown = capture
        .result
        .get("markdown")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let (pandoc, _, _) = pampa::readers::qmd::read(
        result_markdown.as_bytes(),
        false,
        "capture-result.md",
        &mut std::io::sink(),
        false,
        None,
    )
    .expect("parse post-engine markdown");
    pandoc
}

/// Collect every `Image` target in the block tree (top-level Divs and
/// Paragraphs are the only containers the cell shape produces).
fn collect_image_targets(blocks: &[Block]) -> Vec<String> {
    use quarto_pandoc_types::Inline;
    fn walk_inlines(inlines: &[Inline], out: &mut Vec<String>) {
        for inline in inlines {
            if let Inline::Image(img) = inline {
                out.push(img.target.0.clone());
            }
        }
    }
    fn walk(blocks: &[Block], out: &mut Vec<String>) {
        for block in blocks {
            match block {
                Block::Div(d) => walk(&d.content, out),
                Block::Paragraph(p) => walk_inlines(&p.content, out),
                Block::Plain(p) => walk_inlines(&p.content, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(blocks, &mut out);
    out
}

/// Two cells where the second depends on state from the first — pins
/// both kernel-state persistence within a document (through the
/// production text path) and the multi-cell output shape.
#[test]
fn parity_dependent_cells() {
    if !both_engines_available() {
        eprintln!("Skipping test: parity suite needs both knitr and jupyter installed");
        return;
    }
    let knitr = post_engine_ast(
        &two_cell_doc("knitr", "r", "x <- 40", "cat(x + 2, \"\\n\")"),
        "knitr",
    );
    let jupyter = post_engine_ast(
        &two_cell_doc("jupyter", "python", "x = 40", "print(x + 2)"),
        "jupyter",
    );
    assert_blocks_parity(&knitr.blocks, &jupyter.blocks, "blocks");
}

fn two_cell_doc(engine: &str, lang: &str, cell1: &str, cell2: &str) -> String {
    format!(
        "---\ntitle: Parity\nengine: {engine}\n---\n\nBefore.\n\n```{{{lang}}}\n{cell1}\n```\n\nBetween.\n\n```{{{lang}}}\n{cell2}\n```\n\nAfter.\n"
    )
}
