/*
 * tests/integration/typst_html_table_css.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Raw HTML tables (gt, pandas, ...) carry their styling in a `<style>`
//! block. For Typst output the filter chain inlines those rules onto the
//! cells before Pandoc converts the table (Q1's juice step, here the hidden
//! `q2 call inline-css` subcommand). Driven through the real `q2` binary because
//! the filter finds it via the `quarto-cli-path` filter param.

use std::path::Path;
use std::process::{Command, Stdio};

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

const TABLE_DOC: &str = "---\nformat:\n  typst:\n    keep-typ: true\n---\n\n```{=html}\n\
<style>\n.r { text-align: right; color: #ff0000 }\n</style>\n\
<table><tr><td class=\"r\">CSSCELL</td><td>plain</td></tr></table>\n```\n";

fn render_typ(dir: &Path, name: &str, doc: &str) -> String {
    std::fs::write(dir.join(name), doc).unwrap();
    let out = Command::new(Q2_BIN)
        .current_dir(dir)
        .args(["render", name, "--to", "typst"])
        .output()
        .expect("spawn q2");
    assert!(
        out.status.success(),
        "render failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let typ = dir.join(name).with_extension("typ");
    std::fs::read_to_string(&typ).unwrap_or_else(|e| panic!("reading {}: {e}", typ.display()))
}

#[test]
fn style_block_rules_reach_typst_table_cells() {
    let dir = TempDir::new().unwrap();
    let typ = render_typ(dir.path(), "t.qmd", TABLE_DOC);
    // The class rule lives only in the <style> block; without inlining the
    // cell would be a bare `[CSSCELL]` with no fill/alignment/colour.
    let before = &typ[..typ.find("CSSCELL").expect("cell text in output")];
    let cell = &before[before.rfind("table.cell").expect("styled cell")..];
    assert!(
        cell.contains("right") && cell.contains("#ff0000"),
        "expected inlined alignment and colour on the cell, got: {cell}"
    );
}

#[test]
fn inline_css_subcommand_filters_stdin_to_stdout() {
    use std::io::Write;
    let mut child = Command::new(Q2_BIN)
        .args(["call", "inline-css"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"<style>td{color:red}</style><table><tr><td>x</td></tr></table>")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let s = String::from_utf8(out.stdout).unwrap();
    assert!(s.contains("color"), "{s}");
    assert!(!s.contains("<style"), "{s}");
}
