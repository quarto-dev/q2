/*
 * tests/integration/book_cross_chapter_links.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * book-projects P2 item 75: an explicit cross-chapter link resolves to
 * the correct in-document anchor in the compiled output, via the new
 * Rust transform (`resolve_cross_chapter_links`, `project/book/links.rs`)
 * — not just asserted from the unit tests on the merged AST. See
 * `claude-notes/plans/2026-09-21-book-projects-P2-single-file-merge.md`.
 */

//! Renders a real 2-chapter Typst book (bare `--to typst`, resolving to
//! the vendored `orange-book` default) with `keep-typ: true` so the
//! intermediate `.typ` pandoc-Typst source survives the compile, and
//! inspects it directly: the target chapter's heading must carry a
//! Typst `<label>` at its explicit identifier, and the linking chapter's
//! link must resolve to `#link(<that-label>)` — not a `ch1.qmd`-shaped
//! raw file target, which would mean the Rust transform never ran and
//! the link survived unresolved into the compiled source.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::render_to_file::RenderToFileOptions;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

const BOOK_QUARTO_YML: &str = r#"project:
  type: book

keep-typ: true

book:
  title: "Cross Chapter Links Book"
  author: "Test Author"
  chapters:
    - index.qmd
    - ch1.qmd
    - ch2.qmd
"#;

fn write_fixture(project_dir: &Path) {
    write(&project_dir.join("_quarto.yml"), BOOK_QUARTO_YML);
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nWelcome to the book.\n",
    );
    write(
        &project_dir.join("ch1.qmd"),
        "# Chapter One\n\n## Foo Section {#sec-foo}\n\nSome content in chapter one.\n",
    );
    write(
        &project_dir.join("ch2.qmd"),
        "# Chapter Two\n\nSee [chapter one's foo section](ch1.qmd#sec-foo) for details.\n",
    );
}

/// The primary claim of book-projects P2 item 75: a real, compiled Typst
/// book resolves an explicit cross-chapter link
/// (`[..](ch1.qmd#sec-foo)`) to the correct in-document Typst anchor —
/// the target heading's own `<sec-foo>` label, not a raw `ch1.qmd`-shaped
/// file target (which would be broken in a single-file merge, since
/// there is no `ch1.pdf`/`ch1.typ` to point at).
#[test]
fn cross_chapter_link_resolves_to_target_heading_label() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_fixture(&project_dir);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();

    let project_type = project_type_for(&project);
    let format = Format::from_format_string("typst").unwrap();
    let options = RenderToFileOptions::default();
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        format,
        "typst",
        &options,
        runtime,
    );
    let summary = pollster::block_on(pipeline.run_with_book_support())
        .unwrap_or_else(|e| panic!("run_with_book_support failed: {e}"));

    assert_eq!(summary.outputs.len(), 1, "{summary:?}");
    let output_path = &summary.outputs[0].output_path;
    let bytes = std::fs::read(output_path)
        .unwrap_or_else(|e| panic!("expected {} to exist: {e}", output_path.display()));
    assert!(bytes.starts_with(b"%PDF-"), "expected a real compiled PDF");

    let typ_path = output_path.with_extension("typ");
    let typ_text = std::fs::read_to_string(&typ_path).unwrap_or_else(|e| {
        panic!(
            "expected {} (keep-typ: true) to exist: {e}",
            typ_path.display()
        )
    });

    assert!(
        typ_text.contains("<sec-foo>"),
        "expected the target heading's own Typst label to survive the merge: {typ_text}"
    );
    assert!(
        typ_text.contains("#link(<sec-foo>)"),
        "expected the cross-chapter link to resolve to a same-document Typst link \
         targeting the <sec-foo> label: {typ_text}"
    );
    assert!(
        !typ_text.contains("ch1.qmd"),
        "the raw chapter-file link target must not survive into the compiled Typst \
         source — book-links.lua had nothing left to do, and the compiled fixture \
         proves the Rust transform (not the Lua) is what resolved it: {typ_text}"
    );
}
