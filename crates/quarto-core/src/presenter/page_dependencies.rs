/*
 * presenter/page_dependencies.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * OPR-7i: page dependencies for Jupyter-style outputs on the recorded route.
 */

//! The Quarto 1 notebook scan (`widgetDependencies` and `widgetDependencyIncludes`,
//! `src/core/jupyter/widgets.ts`), run in core over a record's cells.
//!
//! An engine declares what kind of dependency data its outputs carry
//! ([`EngineEntry::dependency_data`]); core runs the strategy the label names. The one kind is
//! [`JUPYTER_OUTPUTS`]. The result is derived at present time on every route and never stored.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use super::cell_step::engine_entry_for;
use super::inline::InlineTarget;
use super::knitr_inputs::{OUTPUT_ANCHORS_KEY, OUTPUT_KINDS_KEY};
use crate::record::{EngineEntry, RecordCell};

/// The one label value: the cell outputs are nbformat outputs.
pub const JUPYTER_OUTPUTS: &str = "jupyter-outputs";

/// What the scan found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageDependencies {
    /// The derived head text, one block per labeled engine, already deduplicated.
    pub header_includes: Vec<String>,
    /// Outputs moved into the head, by the cell's nbformat `id` and the output indexes.
    pub hoisted: BTreeMap<String, BTreeSet<usize>>,
}

/// Scan `cells` (all of a record's cells, stale and metadata ones included, as Quarto 1 scans the
/// whole notebook) for the engines whose entry carries a label.
///
/// A cell belongs to the entry its `quarto_engine` names; a cell with none belongs to the only
/// entry when the record has exactly one, and to none when it has several (it is never scanned
/// or hoisted). An entry with no label contributes nothing, so knitr, marimo, an older engine
/// and native Jupyter (no entry) are unchanged. Only for HTML output formats (Quarto 1:
/// `toHtml && !toIpynb`); `prefer-html` markdown is not recognized here.
pub fn page_dependencies(
    cells: &[RecordCell],
    engines: &[EngineEntry],
    format: &str,
) -> PageDependencies {
    let mut out = PageDependencies::default();
    if !InlineTarget::from_format(format).to_html {
        return out;
    }
    for entry in engines {
        // A record whose engine presented itself carries no label (the label is set only with the
        // presentation acknowledgement), so what that engine's `toMarkdown` stored in the entry's
        // includes is never added to here.
        if entry.dependency_data.as_deref() != Some(JUPYTER_OUTPUTS) {
            continue;
        }
        let mine: Vec<&RecordCell> = cells
            .iter()
            .filter(|c| engine_entry_for(engines, c).is_some_and(|e| std::ptr::eq(e, entry)))
            .collect();
        let found = scan_jupyter_outputs(&mine);
        for (id, indexes) in found.hoisted {
            out.hoisted.entry(id).or_default().extend(indexes);
        }
        if let Some(head) = found.head
            && !out.header_includes.contains(&head)
        {
            out.header_includes.push(head);
        }
    }
    out
}

/// `cell` without the outputs at `hoisted`. The positional metadata (`quarto_output_kinds` and
/// `quarto_output_anchors`) loses the same entries, so the presenter, which reads them by
/// position, still lines them up with the outputs that remain.
pub fn without_outputs(cell: &RecordCell, hoisted: &BTreeSet<usize>) -> RecordCell {
    let mut out = cell.clone();
    out.outputs = keep_unhoisted(&cell.outputs, hoisted);
    for key in [OUTPUT_KINDS_KEY, OUTPUT_ANCHORS_KEY] {
        if let Some(Value::Array(items)) = out.metadata.extra.get(key)
            && items.len() == cell.outputs.len()
        {
            let kept = keep_unhoisted(items, hoisted);
            out.metadata
                .extra
                .insert(key.to_string(), Value::Array(kept));
        }
    }
    out
}

fn keep_unhoisted(items: &[Value], hoisted: &BTreeSet<usize>) -> Vec<Value> {
    items
        .iter()
        .enumerate()
        .filter(|(i, _)| !hoisted.contains(i))
        .map(|(_, v)| v.clone())
        .collect()
}

struct Scan {
    head: Option<String>,
    hoisted: BTreeMap<String, BTreeSet<usize>>,
}

const REQUIREJS: &str = r#"<script src="https://cdn.jsdelivr.net/npm/requirejs@2.3.6/require.min.js" integrity="sha384-c9c+LnTbwQ3aujuU7ULEPVvgLs+Fn6fJUvIGTsuu1ZcCf11fiEubah0ttpca4ntM sha384-6V1/AdqZRWk1KAlWbKBlGhN7VG4iE/yAZcO6NZPMF8od0vukrvr0tg4qY6NSrItx" crossorigin="anonymous"></script>"#;
const JQUERY: &str = r#"<script src="https://cdn.jsdelivr.net/npm/jquery@3.5.1/dist/jquery.min.js" integrity="sha384-ZvpUoO/+PpLXR1lu4jmpXWu80pZlYUAfxl5NsBMWOEPSjUn/6Z/hRTt8+pR6L4N2" crossorigin="anonymous" data-relocate-top="true"></script>"#;
const JQUERY_SHIM: &str = r#"<script type="application/javascript">define('jquery', [],function() {return window.jQuery;})</script>"#;
// Quarto 1's unpinned `@*`, kept.
const HTML_MANAGER: &str = r#"<script src="https://cdn.jsdelivr.net/npm/@jupyter-widgets/html-manager@*/dist/embed-amd.js" crossorigin="anonymous"></script>"#;

const TEXT_HTML: &str = "text/html";
const APPLICATION_JAVASCRIPT: &str = "application/javascript";
const WIDGET_VIEW: &str = "application/vnd.jupyter.widget-view+json";

/// `widgetDependencies` and `widgetDependencyIncludes` over one engine's cells. There is no
/// widget state (records carry no `metadata.widgets`), so the state script is not produced.
fn scan_jupyter_outputs(cells: &[&RecordCell]) -> Scan {
    let (mut js_widgets, mut jupyter_widgets) = (false, false);
    let mut libraries: Vec<String> = Vec::new();
    let mut hoisted: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    for cell in cells {
        for (oi, output) in cell.outputs.iter().enumerate() {
            // Quarto 1's `isDisplayData`: only these two output types are looked at.
            if !matches!(
                output.get("output_type").and_then(Value::as_str),
                Some("display_data" | "execute_result")
            ) {
                continue;
            }
            let Some(data) = output.get("data").and_then(Value::as_object) else {
                continue;
            };
            // The flags are set before the strip, so a hoisted library also turns on requirejs.
            js_widgets |= data.contains_key(TEXT_HTML) || data.contains_key(APPLICATION_JAVASCRIPT);
            jupyter_widgets |= data.contains_key(WIDGET_VIEW);
            let Some(text) = data.get(TEXT_HTML).and_then(html_text) else {
                continue;
            };
            if !text.is_empty() && is_plotly_library(&text) {
                let lib = html_libraries_text(&text);
                if !libraries.contains(&lib) {
                    libraries.push(lib);
                }
                hoisted.entry(cell.id.clone()).or_default().insert(oi);
            }
        }
    }
    let mut head: Vec<&str> = Vec::new();
    if js_widgets || jupyter_widgets {
        head.extend([REQUIREJS, JQUERY, JQUERY_SHIM]);
    }
    head.extend(libraries.iter().map(String::as_str));
    if jupyter_widgets {
        head.push(HTML_MANAGER);
    }
    Scan {
        head: (!head.is_empty()).then(|| format!("{}\n", head.join("\n"))),
        hoisted,
    }
}

/// `text/html` as a string, or an array of strings joined with `""`.
fn html_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Array(parts) => Some(parts.iter().filter_map(Value::as_str).collect()),
        _ => None,
    }
}

static REQUIRE_SCRIPT: LazyLock<(Regex, Regex, Regex)> = LazyLock::new(|| {
    (
        Regex::new(r#"^\s*<script type="text/javascript">"#).unwrap(),
        Regex::new(r#"require\.undef\(["']plotly["']\)"#).unwrap(),
        Regex::new(r"define\('plotly'").unwrap(),
    )
});
// JavaScript `.` excludes `\n`, `\r`, U+2028 and U+2029; Rust's excludes only `\n`.
static MODULE_SCRIPT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<script type="module">import [^\n\r\u{2028}\u{2029}]*plotly[^\n\r\u{2028}\u{2029}]*</script>"#)
        .unwrap()
});
// `\d` is ASCII in JavaScript and Unicode in Rust: spell it out.
static EMBED_SCRIPT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\* plotly\.js v[0-9]+\.[0-9]+\.[0-9]+\s*\n\s*\* Copyright [0-9]{4}-[0-9]{4}, Plotly, Inc\.\s*\n\s*\* All rights reserved\.\s*\n\s*\* Licensed under the MIT license",
    )
    .unwrap()
});

/// Quarto 1's `isPlotlyLibrary` (the one widget include today).
fn is_plotly_library(html: &str) -> bool {
    let (start, undef, define) = &*REQUIRE_SCRIPT;
    (start.is_match(html) && (undef.is_match(html) || define.is_match(html)))
        || MODULE_SCRIPT.is_match(html)
        || EMBED_SCRIPT.is_match(html)
}

/// JavaScript's whitespace set (`\s`, `trim`): not Rust's (it has U+FEFF, lacks U+0085).
fn js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// Quarto 1's `htmlLibrariesText`: strip the common leading space (`line.replace(" " x n, "")`
/// removes the first match anywhere in the line, a Quarto 1 quirk kept for parity); blank lines
/// become empty; lines split on `\r\n?|\n` and are joined with `\n`.
fn html_libraries_text(html: &str) -> String {
    let lines = split_lines(html);
    let leading = lines
        .iter()
        .filter_map(|l| l.chars().position(|c| !js_whitespace(c)))
        .min();
    let Some(n) = leading else {
        return html.to_string();
    };
    let spaces = " ".repeat(n);
    lines
        .iter()
        .map(|l| {
            if l.chars().all(js_whitespace) {
                String::new()
            } else {
                l.replacen(&spaces, "", 1)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn split_lines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut start, bytes) = (0, text.as_bytes());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                out.push(&text[start..i]);
                start = i + 1;
            }
            b'\r' => {
                out.push(&text[start..i]);
                if bytes.get(i + 1) == Some(&b'\n') {
                    i += 1;
                }
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(&text[start..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{
        AuthorTexts, CellMetadata, CellType, EngineEntryInput, HashDefinition, RecordBuilder,
        UnitKind,
    };
    use serde_json::{Value, json};
    use std::collections::BTreeMap;

    fn entry(name: &str, label: Option<&str>) -> EngineEntry {
        let def = HashDefinition::conservative(name);
        let mut e = RecordBuilder::new(name, &def, &[], &AuthorTexts::default())
            .unwrap()
            .finish(EngineEntryInput::default())
            .unwrap()
            .engine;
        e.dependency_data = label.map(str::to_string);
        e
    }

    fn cell(id: &str, engine: Option<&str>, outputs: Vec<Value>) -> RecordCell {
        RecordCell {
            cell_type: CellType::Code,
            execution_count: None,
            id: id.into(),
            metadata: CellMetadata {
                quarto_cell_id: format!("{id}-cid"),
                quarto_content_hash: id.into(),
                quarto_label: None,
                quarto_kind: UnitKind::Cell,
                quarto_lang: "julia".into(),
                quarto_engine: engine.map(str::to_string),
                quarto_kernel: None,
                quarto_location: None,
                quarto_exec_hash: "h".into(),
                quarto_hash_rule: None,
                extra: BTreeMap::new(),
            },
            outputs,
            source: vec!["x".into()],
        }
    }

    fn julia() -> Vec<EngineEntry> {
        vec![entry("julia", Some(JUPYTER_OUTPUTS))]
    }

    fn fixture(name: &str) -> Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/page_dependencies")
            .join(format!("{name}.json"));
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
    }

    fn fixture_names() -> Vec<String> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/page_dependencies");
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| {
                let p = e.unwrap().path();
                (p.extension()? == "json")
                    .then(|| p.file_stem().unwrap().to_string_lossy().into_owned())
            })
            .collect();
        names.sort();
        names
    }

    /// The cells of an oracle fixture, tagged for the one labeled engine.
    fn fixture_cells(f: &Value) -> Vec<RecordCell> {
        f["cells"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(i, outs)| {
                cell(
                    &format!("c{i}"),
                    Some("julia"),
                    outs.as_array().unwrap().clone(),
                )
            })
            .collect()
    }

    fn expected_head(f: &Value) -> Vec<String> {
        f["expected"]["head"]
            .as_str()
            .map(|h| vec![h.to_string()])
            .unwrap_or_default()
    }

    fn expected_hoisted(f: &Value) -> BTreeMap<String, BTreeSet<usize>> {
        let mut m: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
        for pair in f["expected"]["hoisted"].as_array().unwrap() {
            m.entry(format!("c{}", pair[0].as_u64().unwrap()))
                .or_default()
                .insert(pair[1].as_u64().unwrap() as usize);
        }
        m
    }

    #[test]
    fn every_oracle_fixture_matches_the_typescript_scan() {
        let names = fixture_names();
        assert!(names.len() >= 19, "{names:?}");
        for name in names {
            let f = fixture(&name);
            let got = page_dependencies(&fixture_cells(&f), &julia(), "html");
            assert_eq!(got.header_includes, expected_head(&f), "head of {name}");
            assert_eq!(got.hoisted, expected_hoisted(&f), "hoisted of {name}");
        }
    }

    #[test]
    fn no_label_gives_nothing() {
        let f = fixture("plots-data-uri");
        let engines = vec![entry("julia", None)];
        assert_eq!(
            page_dependencies(&fixture_cells(&f), &engines, "html"),
            PageDependencies::default()
        );
        assert_eq!(
            page_dependencies(&fixture_cells(&f), &[], "html"),
            PageDependencies::default()
        );
    }

    #[test]
    fn a_non_html_format_and_ipynb_give_nothing() {
        let f = fixture("plotly-require-indented");
        for format in ["pdf", "typst", "docx", "ipynb", "gfm"] {
            assert_eq!(
                page_dependencies(&fixture_cells(&f), &julia(), format),
                PageDependencies::default(),
                "{format}"
            );
        }
        for format in ["html", "revealjs", "html+smart"] {
            assert!(
                !page_dependencies(&fixture_cells(&f), &julia(), format)
                    .header_includes
                    .is_empty(),
                "{format}"
            );
        }
    }

    #[test]
    fn a_cell_with_no_engine_belongs_to_the_only_entry_but_to_none_of_several() {
        let outs = fixture("plotly-require-indented")["cells"][0]
            .as_array()
            .unwrap()
            .clone();
        let one = page_dependencies(&[cell("a", None, outs.clone())], &julia(), "html");
        assert_eq!(one.header_includes.len(), 1);
        assert!(one.hoisted.contains_key("a"));
        let two = vec![
            entry("julia", Some(JUPYTER_OUTPUTS)),
            entry("other", Some(JUPYTER_OUTPUTS)),
        ];
        assert_eq!(
            page_dependencies(&[cell("a", None, outs)], &two, "html"),
            PageDependencies::default()
        );
    }

    #[test]
    fn only_labeled_engines_cells_are_scanned() {
        let outs = fixture("plots-data-uri")["cells"][0]
            .as_array()
            .unwrap()
            .clone();
        let engines = vec![entry("julia", Some(JUPYTER_OUTPUTS)), entry("knitr", None)];
        let got = page_dependencies(&[cell("a", Some("knitr"), outs)], &engines, "html");
        assert_eq!(got, PageDependencies::default());
    }

    #[test]
    fn two_labeled_records_give_one_block() {
        let outs = fixture("plots-data-uri")["cells"][0]
            .as_array()
            .unwrap()
            .clone();
        let engines = vec![
            entry("julia", Some(JUPYTER_OUTPUTS)),
            entry("julia2", Some(JUPYTER_OUTPUTS)),
        ];
        let cells = [
            cell("a", Some("julia"), outs.clone()),
            cell("b", Some("julia2"), outs),
        ];
        let got = page_dependencies(&cells, &engines, "html");
        assert_eq!(got.header_includes.len(), 1, "{:?}", got.header_includes);
    }

    #[test]
    fn hidden_outputs_still_count() {
        // The scan reads the record, not the presented output: `output: false` changes nothing.
        let mut c = cell(
            "a",
            Some("julia"),
            fixture("plots-data-uri")["cells"][0]
                .as_array()
                .unwrap()
                .clone(),
        );
        c.metadata.extra.insert("output".into(), json!(false));
        assert_eq!(
            page_dependencies(&[c], &julia(), "html")
                .header_includes
                .len(),
            1
        );
    }
}
