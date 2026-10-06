/*
 * tests/integration/pandoc_typst_template_partials.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Typst `template-partials` staging in PandocWriteStage: a document (or
 * extension) that declares `template-partials` without a whole `template:`
 * must have those partials staged over the same-named vendored partials in
 * the directory Pandoc's `--template` resolves `$partial.typ()$` calls
 * against. Mirrors ApplyTemplateStage's existing HTML handling
 * (apply_template.rs:179-183). Surfaced by the book-projects P2 item-79
 * probe: orange-book ships `typst-show.typ` (the only place Typst's
 * `part`/`chapter` functions are imported) as a template-partial, and the
 * Typst leg silently compiled the vendored default instead — every
 * `#part[...]` RawBlock the extension's own Lua filter emitted failed with
 * `unknown variable: part`.
 */

use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::render_to_file::{RenderToFileOptions, render_document_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn write(path: &std::path::Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// A document-level `template-partials` entry shadows the vendored
/// same-named partial: the kept `.typ` intermediate must contain the
/// custom partial's marker and none of the vendored `typst-show.typ`'s
/// `#show: doc => article(` rule.
#[test]
fn typst_template_partials_shadow_vendored_partials() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("f.qmd");
    write(
        &input_path,
        "---\ntitle: Partials\nkeep-typ: true\ntemplate-partials:\n  - partials/typst-show.typ\n  - partials/numbering.typ\n---\n\n# Heading\n\nBody text.\n",
    );
    write(
        &project_dir.join("partials/typst-show.typ"),
        "// Q2-CUSTOM-SHOW-PARTIAL-MARKER\n#show: doc => doc\n",
    );
    write(
        &project_dir.join("partials/numbering.typ"),
        "// Q2-CUSTOM-NUMBERING-PARTIAL-MARKER\n",
    );

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let options = RenderToFileOptions::default();

    let result = render_document_to_file(
        &input_path,
        "typst",
        &options,
        None,
        runtime,
        None,
        None,
        None,
    )
    .expect("typst render with template-partials should succeed");

    let typ_path = result.output_path.with_extension("typ");
    let typ_text = std::fs::read_to_string(&typ_path)
        .unwrap_or_else(|e| panic!("expected kept intermediate {}: {e}", typ_path.display()));
    assert!(
        typ_text.contains("Q2-CUSTOM-SHOW-PARTIAL-MARKER"),
        "expected custom typst-show.typ partial in the .typ output, got:\n{typ_text}"
    );
    assert!(
        typ_text.contains("Q2-CUSTOM-NUMBERING-PARTIAL-MARKER"),
        "expected custom numbering.typ partial in the .typ output, got:\n{typ_text}"
    );
    assert!(
        !typ_text.contains("#show: doc => article("),
        "vendored typst-show.typ should have been shadowed, got:\n{typ_text}"
    );
}
