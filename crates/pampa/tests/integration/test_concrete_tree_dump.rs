/*
 * test_concrete_tree_dump.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * `pampa -v` prints the tree-sitter concrete syntax tree to stderr, which is
 * the main tool for debugging the grammar. That dump used to be produced
 * inside `readers::qmd::read` on every call, into whatever verbose stream
 * the caller passed — including `io::sink()` and throwaway `Vec`s in
 * `quarto-core` — costing a full extra tree walk plus formatting per parse
 * (bd-khect2gq). The dump is now an explicit, opt-in call.
 */

use pampa::readers;
use std::process::Command;

/// The dump of `x\n`: one line per node, two spaces of indentation per level.
const DUMP_OF_X: &str = "\
document: {Node document (0, 0) - (1, 0)}
  section: {Node section (0, 0) - (1, 0)}
    pandoc_paragraph: {Node pandoc_paragraph (0, 0) - (1, 0)}
      pandoc_str: {Node pandoc_str (0, 0) - (0, 1)}
";

#[test]
fn read_writes_nothing_to_the_verbose_stream_for_a_well_formed_document() {
    let input = "# Title\n\nSome *emphasis* and [a span]{.c}.\n\n- a\n- b\n";
    let mut verbose = Vec::new();
    readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut verbose,
        true,
        None,
    )
    .unwrap_or_else(|errs| panic!("read failed: {errs:#?}"));
    assert_eq!(String::from_utf8_lossy(&verbose), "");
}

#[test]
fn dump_concrete_tree_prints_every_node_indented_by_depth() {
    let mut out = Vec::new();
    readers::qmd::dump_concrete_tree(b"x\n", &mut out);
    assert_eq!(String::from_utf8(out).unwrap(), DUMP_OF_X);
}

#[test]
fn pampa_verbose_prints_the_concrete_tree_to_stderr() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = tmp.path().join("x.qmd");
    std::fs::write(&file, "x\n").expect("write qmd");

    let output = Command::new(env!("CARGO_BIN_EXE_pampa"))
        .arg("-v")
        .arg(&file)
        .output()
        .expect("run pampa -v");

    assert!(output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(DUMP_OF_X),
        "expected the concrete tree dump on stderr, got:\n{stderr}"
    );
}

#[test]
fn pampa_without_verbose_prints_no_tree() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = tmp.path().join("x.qmd");
    std::fs::write(&file, "x\n").expect("write qmd");

    let output = Command::new(env!("CARGO_BIN_EXE_pampa"))
        .arg(&file)
        .output()
        .expect("run pampa");

    assert!(output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("document: {Node"),
        "unexpected tree dump:\n{stderr}"
    );
}
