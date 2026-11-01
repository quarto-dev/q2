/*
 * engine/knitr/pieces_map.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * KNR-1 S2 mapper: a chunk's captured knitr pieces to the presenter's inputs.
 */

//! Maps one chunk's captured [`KnitrPiece`]s to the presenter's [`CellResult`]
//! (the S2 table in `claude-notes/plans/2026-10-30-knr-01-capture-spike-and-harness.md`).
//!
//! There is no record writer here (that is KNR-3); the output types are the
//! presenter's, so the baseline can call `render_cell` directly.

use base64::Engine as _;
use serde_json::{Value, json};

use crate::presenter::output::{CellOutput, CellResult, ExecuteStatus, MimeBundle};

use super::types::{KnitrCapturedChunk, KnitrPiece};

/// The fence q2's `htmlPreserve` puts around a widget's HTML (S2 table).
const WIDGET_FENCE_OPEN: &str = "\n```{=html}\n";
const WIDGET_FENCE_CLOSE: &str = "\n```\n";

/// Map a chunk to its source and presenter result.
///
/// `read_file` returns the bytes of a figure path as knitr addresses it.
/// Returns `None` when the chunk must become a fallback unit: an `Other`
/// piece, a figure that cannot be read, or an image type the presenter does
/// not write (`pdf`).
pub fn pieces_to_cell(
    chunk: &KnitrCapturedChunk,
    read_file: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Option<(String, CellResult)> {
    let mut outputs = Vec::new();
    let mut sources = String::new();
    for piece in &chunk.pieces {
        match piece {
            KnitrPiece::Source { text } => sources.push_str(text),
            KnitrPiece::Output { text } => outputs.push(stream("stdout", text)),
            KnitrPiece::Message { text } | KnitrPiece::Warning { text, .. } => {
                outputs.push(stream("stderr", text));
            }
            KnitrPiece::Error {
                text,
                ename,
                evalue,
                ..
            } => outputs.push(CellOutput::Error {
                ename: ename.clone(),
                evalue: evalue.clone(),
                traceback: vec![text.clone()],
            }),
            KnitrPiece::Plot { files } => {
                for file in files {
                    outputs.push(image(file, read_file)?);
                }
            }
            KnitrPiece::ImagePaths { paths } => {
                for path in paths {
                    outputs.push(image(path, read_file)?);
                }
            }
            KnitrPiece::Asis {
                text,
                mime,
                widget,
                knit_meta,
            } => outputs.push(asis(text, mime, *widget, knit_meta.as_deref())),
            KnitrPiece::Other { .. } => return None,
        }
    }
    let code = if chunk.code.is_empty() {
        sources
    } else {
        chunk.code.clone()
    };
    Some((
        code,
        CellResult {
            status: ExecuteStatus::Ok,
            outputs,
            execution_count: None,
        },
    ))
}

fn stream(name: &str, text: &str) -> CellOutput {
    CellOutput::Stream {
        name: name.to_string(),
        text: text.to_string(),
    }
}

/// An image piece as `display_data`; `metadata.quarto_knitr.file` keeps knitr's own path.
fn image(path: &str, read_file: &dyn Fn(&str) -> Option<Vec<u8>>) -> Option<CellOutput> {
    let ext = path.rsplit('.').next()?.to_ascii_lowercase();
    let bytes = read_file(path)?;
    let (mime, value) = match ext.as_str() {
        "png" => ("image/png", Value::String(b64(&bytes))),
        "jpg" | "jpeg" => ("image/jpeg", Value::String(b64(&bytes))),
        "svg" => (
            "image/svg+xml",
            Value::String(String::from_utf8(bytes).ok()?),
        ),
        _ => return None,
    };
    let mut data = MimeBundle::new();
    data.insert(mime.to_string(), value);
    Some(CellOutput::DisplayData {
        data,
        metadata: json!({"quarto_knitr": {"file": path}}),
    })
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn asis(text: &str, mime: &str, widget: bool, knit_meta: Option<&str>) -> CellOutput {
    let body = if widget {
        text.strip_prefix(WIDGET_FENCE_OPEN)
            .and_then(|t| t.strip_suffix(WIDGET_FENCE_CLOSE))
            .unwrap_or(text)
    } else {
        text
    };
    let mut data = MimeBundle::new();
    data.insert(mime.to_string(), Value::String(body.to_string()));
    let metadata = match knit_meta {
        Some(meta) => json!({"quarto_knitr": {"knit_meta": meta}}),
        None => json!({}),
    };
    CellOutput::DisplayData { data, metadata }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(pieces: Value) -> KnitrCapturedChunk {
        serde_json::from_value(json!({"ordinal": 1, "label": "t", "pieces": pieces})).unwrap()
    }

    fn no_files(_: &str) -> Option<Vec<u8>> {
        None
    }

    fn map(pieces: Value) -> (String, Vec<CellOutput>) {
        let (code, result) = pieces_to_cell(&chunk(pieces), &no_files).unwrap();
        (code, result.outputs)
    }

    #[test]
    fn sources_concatenate_into_the_code() {
        let (code, outputs) = map(json!([
            {"class": "source", "text": "x <- 1\n"},
            {"class": "source", "text": "x\n"},
            {"class": "output", "text": "[1] 1\n"},
        ]));
        assert_eq!(code, "x <- 1\nx\n");
        assert_eq!(outputs.len(), 1);
    }

    #[test]
    fn code_field_wins_over_concatenated_sources() {
        let mut c = chunk(json!([{"class": "source", "text": "a\n"}]));
        c.code = "a\nb\n".to_string();
        let (code, _) = pieces_to_cell(&c, &no_files).unwrap();
        assert_eq!(code, "a\nb\n");
    }

    #[test]
    fn output_is_stdout_message_and_warning_are_stderr() {
        let (_, outputs) = map(json!([
            {"class": "output", "text": "o\n"},
            {"class": "message", "text": "m\n"},
            {"class": "warning", "text": "Warning: w", "call": "f()"},
        ]));
        let names: Vec<_> = outputs
            .iter()
            .map(|o| match o {
                CellOutput::Stream { name, text } => (name.as_str(), text.as_str()),
                other => panic!("not a stream: {other:?}"),
            })
            .collect();
        assert_eq!(
            names,
            [
                ("stdout", "o\n"),
                ("stderr", "m\n"),
                ("stderr", "Warning: w")
            ]
        );
    }

    #[test]
    fn error_has_name_value_and_the_printed_text_as_traceback() {
        let (_, outputs) = map(json!([
            {"class": "error", "text": "Error in f(): boom\n", "ename": "simpleError",
             "evalue": "boom", "call": "f()"},
        ]));
        match &outputs[0] {
            CellOutput::Error {
                ename,
                evalue,
                traceback,
            } => {
                assert_eq!(ename, "simpleError");
                assert_eq!(evalue, "boom");
                assert_eq!(traceback, &vec!["Error in f(): boom\n".to_string()]);
            }
            other => panic!("not an error: {other:?}"),
        }
    }

    #[test]
    fn plot_becomes_display_data_with_the_file_in_metadata() {
        let c = chunk(json!([{"class": "plot", "files": ["d/p-1.png", "d/p-2.svg"]}]));
        let read = |p: &str| Some(format!("bytes of {p}").into_bytes());
        let (_, result) = pieces_to_cell(&c, &read).unwrap();
        assert_eq!(result.outputs.len(), 2);
        match &result.outputs[0] {
            CellOutput::DisplayData { data, metadata } => {
                assert_eq!(
                    data["image/png"],
                    json!(b64(b"bytes of d/p-1.png")),
                    "png is base64"
                );
                assert_eq!(metadata["quarto_knitr"]["file"], "d/p-1.png");
            }
            other => panic!("not display_data: {other:?}"),
        }
        match &result.outputs[1] {
            CellOutput::DisplayData { data, .. } => {
                assert_eq!(data["image/svg+xml"], json!("bytes of d/p-2.svg"));
            }
            other => panic!("not display_data: {other:?}"),
        }
    }

    #[test]
    fn image_paths_map_like_plots() {
        let c = chunk(json!([{"class": "image_paths", "paths": ["made.png"]}]));
        let read = |_: &str| Some(vec![1, 2, 3]);
        let (_, result) = pieces_to_cell(&c, &read).unwrap();
        assert!(matches!(
            &result.outputs[0],
            CellOutput::DisplayData { data, .. } if data.contains_key("image/png")
        ));
    }

    #[test]
    fn unreadable_or_unsupported_figure_is_a_fallback() {
        let c = chunk(json!([{"class": "plot", "files": ["d/p-1.png"]}]));
        assert!(pieces_to_cell(&c, &no_files).is_none());
        let pdf = chunk(json!([{"class": "plot", "files": ["d/p-1.pdf"]}]));
        assert!(pieces_to_cell(&pdf, &|_| Some(vec![0])).is_none());
    }

    #[test]
    fn other_piece_makes_the_chunk_a_fallback() {
        let c = chunk(json!([
            {"class": "output", "text": "o\n"},
            {"class": "other", "classes": ["weird"]},
        ]));
        assert!(pieces_to_cell(&c, &no_files).is_none());
    }

    #[test]
    fn asis_keeps_its_mime_and_text() {
        let (_, outputs) = map(json!([
            {"class": "asis", "text": "<div>x</div>", "mime": "text/html"},
            {"class": "asis", "text": "| a |\n", "mime": "text/markdown"},
        ]));
        match (&outputs[0], &outputs[1]) {
            (CellOutput::DisplayData { data: h, .. }, CellOutput::DisplayData { data: m, .. }) => {
                assert_eq!(h["text/html"], json!("<div>x</div>"));
                assert_eq!(m["text/markdown"], json!("| a |\n"));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn widget_loses_q2s_html_fence_and_carries_knit_meta() {
        let (_, outputs) = map(json!([
            {"class": "asis", "text": "\n```{=html}\n<div id=\"w\"></div>\n```\n",
             "mime": "text/html", "widget": true, "knit_meta": "[1,2]"},
        ]));
        match &outputs[0] {
            CellOutput::DisplayData { data, metadata } => {
                assert_eq!(data["text/html"], json!("<div id=\"w\"></div>"));
                assert_eq!(metadata["quarto_knitr"]["knit_meta"], "[1,2]");
            }
            other => panic!("not display_data: {other:?}"),
        }
    }
}
