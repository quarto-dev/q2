/*
 * tests/integration/unit_fixtures.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * EXR-2 Phase 3: scanner conformance fixtures.
 */

//! Every `tests/fixtures/units/<name>.qmd` has a `<name>.units.json` listing,
//! in order, the units `scan_units` must find. The format is documented in
//! `tests/fixtures/units/README.md`.
//!
//! Regenerate the JSON after a deliberate scanner change with
//! `UPDATE_UNIT_FIXTURES=1 cargo nextest run -p quarto-core -E
//! 'test(unit_fixtures::)'`, then review the diff by hand: the files are the
//! contract, not a snapshot of whatever the scanner does today.

use std::path::{Path, PathBuf};

use pampa::readers::qmd;
use quarto_core::engine::cell_lang::engine_cell_lang;
use quarto_core::engine::units::{Unit, scan_units};
use quarto_pandoc_types::Block;
use serde_json::{Value, json};

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/units")
}

fn qmd_fixtures() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(fixture_dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "qmd"))
        .collect();
    v.sort();
    v
}

/// 1-based line of a byte offset.
fn line_of(text: &str, offset: usize) -> usize {
    text[..offset].bytes().filter(|&b| b == b'\n').count() + 1
}

fn units_json(text: &str) -> Value {
    Value::Array(
        scan_units(text)
            .iter()
            .map(|u| match u {
                Unit::Cell(c) => json!({
                    "kind": "cell",
                    "lang": c.lang,
                    "index": c.index,
                    "line": line_of(text, c.start),
                    "code": c.code,
                }),
                Unit::Inline(i) => json!({
                    "kind": "span",
                    "lang": i.lang,
                    "span_index": i.span_index,
                    "line": line_of(text, i.start),
                    "code": i.code,
                }),
            })
            .collect(),
    )
}

fn json_path(qmd: &Path) -> PathBuf {
    qmd.with_extension("units.json")
}

#[test]
fn every_fixture_matches_its_units_json() {
    let update = std::env::var_os("UPDATE_UNIT_FIXTURES").is_some();
    let fixtures = qmd_fixtures();
    assert!(
        fixtures.len() >= 20,
        "corpus went missing: {}",
        fixtures.len()
    );
    for qmd_path in fixtures {
        let text = std::fs::read_to_string(&qmd_path).unwrap();
        let got = units_json(&text);
        let jp = json_path(&qmd_path);
        if update {
            let mut s = serde_json::to_string_pretty(&got).unwrap();
            s.push('\n');
            std::fs::write(&jp, s).unwrap();
            continue;
        }
        let want: Value = serde_json::from_str(
            &std::fs::read_to_string(&jp)
                .unwrap_or_else(|_| panic!("missing {}; see the README", jp.display())),
        )
        .unwrap();
        assert_eq!(got, want, "{}", qmd_path.display());
    }
}

fn collect_cell_langs(blocks: &[Block], out: &mut Vec<String>) {
    for b in blocks {
        match b {
            Block::CodeBlock(_) => {
                if let Some(l) = engine_cell_lang(b) {
                    out.push(l.to_string());
                }
            }
            Block::BlockQuote(x) => collect_cell_langs(&x.content, out),
            Block::Div(x) => collect_cell_langs(&x.content, out),
            Block::BulletList(x) => x.content.iter().for_each(|i| collect_cell_langs(i, out)),
            Block::OrderedList(x) => x.content.iter().for_each(|i| collect_cell_langs(i, out)),
            _ => {}
        }
    }
}

/// Expected differences between the AST walker (`engine_cell_lang`) and the
/// text scanner, each with a reason. Everything else must agree exactly.
const EXPECTED_DIVERGENCES: &[(&str, &str)] = &[(
    "cell_unclosed",
    "CommonMark (pampa) runs an unclosed fence to the end of the document, so the AST has a \
     cell; the scanner yields no cell for a fence that never closes, as the legacy executor \
     did not match it either",
)];

#[test]
fn ast_walker_and_text_scanner_agree_on_cell_languages() {
    for qmd_path in qmd_fixtures() {
        let name = qmd_path.file_stem().unwrap().to_string_lossy().to_string();
        let text = std::fs::read_to_string(&qmd_path).unwrap();
        let mut sink = Vec::<u8>::new();
        let (ast, _ctx, _w) =
            qmd::read(text.as_bytes(), false, "fixture.qmd", &mut sink, true, None)
                .unwrap_or_else(|_| panic!("{name}: fixture must parse"));
        let mut walker = Vec::new();
        collect_cell_langs(&ast.blocks, &mut walker);
        let scanner: Vec<String> = scan_units(&text)
            .into_iter()
            .filter_map(|u| match u {
                Unit::Cell(c) => Some(c.lang),
                Unit::Inline(_) => None,
            })
            .collect();
        if EXPECTED_DIVERGENCES.iter().any(|(n, _)| *n == name) {
            continue;
        }
        assert_eq!(walker, scanner, "{name}: walker vs scanner");
    }
}
