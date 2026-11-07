//! KNR-1 Phase 1C: the baseline presenter run. Given a document and the pieces knitr's capture
//! returned for it, produce the markdown the unmodified presenter would write: one `render_cell`
//! call per chunk (S2), spliced into the source where the chunk was. The differences against
//! knitr's own markdown are what KNR-2 has to close.
//!
//! No R here: the input is plain data, so the splice and the visibility rules are tested with
//! hand-built chunks.

use std::path::{Path, PathBuf};

use serde_json::Value;

use quarto_core::engine::capture_files::collect_capture_files;
use quarto_core::engine::units::{CellUnit, Unit, scan_units};
use quarto_core::engine::{
    KnitrCapturedChunk, expression_starts, knitr_visibility, output_kinds, pieces_to_cell_anchored,
};
use quarto_core::presenter::cell::{HoldMode, apply_holds, render_cell_interleaved};
use quarto_core::presenter::figures::FsFigureSink;
use quarto_core::presenter::options::CellOptions;
use quarto_core::presenter::visibility::CellVisibility;
use quarto_system_runtime::NativeRuntime;

/// What the baseline made of a document.
#[derive(Debug, PartialEq, Eq)]
pub enum Baseline {
    Candidate(String),
    /// The document cannot be presented chunk by chunk; `reason` names why.
    Fallback(String),
}

/// The chunk\'s info string as written (`r c1`), for `fenced.echo`.
fn original_params(cell: &CellUnit) -> String {
    format!("{} {}", cell.lang, cell.attr_tail)
        .trim()
        .to_string()
}

/// The leading `#|` option lines of the chunk as written, for `fenced.echo`.
fn option_lines(cell: &CellUnit) -> String {
    cell.code
        .lines()
        .take_while(|l| l.starts_with("#|"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Build the baseline for `source` (written at `doc_path`, whose directory holds the figure
/// files the chunks name).
pub fn baseline(source: &str, doc_path: &Path, chunks: &[KnitrCapturedChunk]) -> Baseline {
    let cells: Vec<CellUnit> = scan_units(source)
        .into_iter()
        .filter_map(|u| match u {
            Unit::Cell(c) => Some(c),
            Unit::Inline(_) => None,
        })
        .collect();
    if cells.len() != chunks.len() {
        return Baseline::Fallback(format!(
            "{} cells in the source, {} chunks from knitr",
            cells.len(),
            chunks.len()
        ));
    }
    let doc_dir = doc_path.parent().unwrap_or(Path::new("."));
    let runtime = NativeRuntime::new();
    let mut fig = FsFigureSink::new(doc_path);
    let mut rendered = Vec::new();
    for (cell, chunk) in cells.iter().zip(chunks) {
        let files: Vec<PathBuf> = chunk.files.iter().map(PathBuf::from).collect();
        let captured = collect_capture_files(&runtime, doc_dir, &files);
        let read = |path: &str| -> Option<Vec<u8>> {
            use base64::Engine as _;
            let f = captured.iter().find(|f| f.path == path)?;
            base64::engine::general_purpose::STANDARD
                .decode(&f.contents_base64)
                .ok()
        };
        let Some((code, mut result, anchors)) = pieces_to_cell_anchored(chunk, &read) else {
            return Baseline::Fallback(format!("chunk {} is a fallback unit", chunk.label));
        };
        // Holds run at present time over the capture-order anchors (epic decision 21).
        let hold = HoldMode {
            plots: chunk.options.get("fig.show").and_then(Value::as_str) == Some("hold"),
            text: chunk.options.get("results").and_then(Value::as_str) == Some("hold"),
        };
        let anchors = match anchors {
            Some(a) if hold.plots || hold.text => {
                let (outputs, a) = apply_holds(
                    std::mem::take(&mut result.outputs),
                    &a,
                    code.lines().count(),
                    hold,
                );
                result.outputs = outputs;
                Some(a)
            }
            other => other,
        };
        let text = render_cell_interleaved(
            &cell.lang,
            &code,
            &result,
            anchors.as_deref(),
            &expression_starts(chunk).unwrap_or_default(),
            &CellOptions::from_knitr(&chunk.options)
                .with_output_kinds(output_kinds(chunk))
                .with_original_chunk(&original_params(cell), &option_lines(cell)),
            &mut fig,
            visibility(&chunk.options),
        );
        rendered.push(indent_after_first_line(&text, cell));
    }
    // Splice from the back so earlier offsets stay valid.
    let mut out = source.to_string();
    for (cell, text) in cells.iter().zip(&rendered).rev() {
        out.replace_range(cell.start..cell.end, text);
    }
    Baseline::Candidate(out)
}

/// The presenter's visibility from a chunk's recorded options (see `knitr_visibility`).
pub fn visibility(options: &Value) -> CellVisibility {
    knitr_visibility(options)
}

/// The text starts where the opener's backticks were, so only the following lines carry the
/// container prefix (a blockquote's `> `) and the indent. The rendered cell ends with the
/// newline the source's closer line had.
fn indent_after_first_line(text: &str, cell: &CellUnit) -> String {
    if text.is_empty() {
        return String::new();
    }
    let prefix = format!("{}{}", cell.container_prefix, " ".repeat(cell.indent));
    let blank = cell.container_prefix.trim_end();
    let body = text.trim_end_matches('\n');
    let mut lines = body.split('\n');
    let mut out = String::from(lines.next().unwrap_or(""));
    for line in lines {
        out.push('\n');
        if line.is_empty() {
            out.push_str(blank);
        } else {
            out.push_str(&prefix);
            out.push_str(line);
        }
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn chunk(options: Value, pieces: Value) -> KnitrCapturedChunk {
        serde_json::from_value(json!({
            "ordinal": 1, "label": "c1", "code": "1 + 1", "options": options, "pieces": pieces
        }))
        .unwrap()
    }

    fn one_chunk(options: Value) -> KnitrCapturedChunk {
        chunk(
            options,
            json!([
                {"class": "source", "text": "1 + 1\n"},
                {"class": "output", "text": "[1] 2\n"}
            ]),
        )
    }

    fn run(source: &str, c: KnitrCapturedChunk) -> Baseline {
        baseline(source, Path::new("/nonexistent/doc.qmd"), &[c])
    }

    #[test]
    fn splices_a_cell_in_place_and_keeps_the_prose() {
        let b = run(
            "Before.\n\n```{r c1}\n1 + 1\n```\n\nAfter.\n",
            one_chunk(json!({})),
        );
        let Baseline::Candidate(md) = b else {
            panic!("{b:?}")
        };
        assert!(md.starts_with("Before.\n\n::: {.cell}"), "{md}");
        assert!(md.contains("[1] 2"), "{md}");
        assert!(md.ends_with(":::\n\nAfter.\n"), "{md}");
    }

    #[test]
    fn include_false_drops_the_cell() {
        let b = run(
            "A\n\n```{r c1}\n1 + 1\n```\n\nB\n",
            one_chunk(json!({"include": false})),
        );
        assert_eq!(b, Baseline::Candidate("A\n\n\nB\n".into()));
    }

    #[test]
    fn echo_false_keeps_only_the_output() {
        let b = run("```{r c1}\n1 + 1\n```\n", one_chunk(json!({"echo": false})));
        let Baseline::Candidate(md) = b else { panic!() };
        assert!(!md.contains("1 + 1"), "{md}");
        assert!(md.contains("[1] 2"), "{md}");
    }

    #[test]
    fn container_prefix_and_indent_follow_the_opener() {
        let quoted = run(
            "> q\n>\n> ```{r c1}\n> 1 + 1\n> ```\n",
            one_chunk(json!({})),
        );
        let Baseline::Candidate(md) = quoted else {
            panic!()
        };
        assert!(
            md.lines().all(|l| l.starts_with('>') || l.is_empty()),
            "{md}"
        );
        let listed = run(
            "- item\n\n  ```{r c1}\n  1 + 1\n  ```\n",
            one_chunk(json!({})),
        );
        let Baseline::Candidate(md) = listed else {
            panic!()
        };
        let tail: Vec<&str> = md.lines().skip(2).collect();
        assert!(
            tail.iter().all(|l| l.is_empty() || l.starts_with("  ")),
            "{md}"
        );
    }

    #[test]
    fn count_mismatch_and_fallback_chunks_are_fallbacks() {
        let none = baseline(
            "```{r c1}\n1\n```\n```{r c2}\n2\n```\n",
            Path::new("/x/doc.qmd"),
            &[one_chunk(json!({}))],
        );
        assert!(matches!(none, Baseline::Fallback(r) if r.contains("2 cells")));
        let other = chunk(json!({}), json!([{"class": "other", "classes": ["weird"]}]));
        assert!(
            matches!(run("```{r c1}\n1\n```\n", other), Baseline::Fallback(r) if r.contains("c1"))
        );
    }
}
