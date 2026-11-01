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

use crate::presenter::options::OutputKind;
use crate::presenter::output::{CellOutput, CellResult, ExecuteStatus, MimeBundle};
#[cfg(test)]
use crate::presenter::visibility::CellVisibility;

use super::types::{KnitrCapturedChunk, KnitrPiece};

/// The fence q2's `htmlPreserve` puts around a widget's HTML (S2 table).
const WIDGET_FENCE_OPEN: &str = "\n```{=html}\n";
const WIDGET_FENCE_CLOSE: &str = "\n```\n";

/// The line index in the chunk\'s code where each source piece (one top-level expression)
/// starts, for knitr\'s `prompt` (KNR-2 Phase 2D). `None` when the pieces\' lines do not add up
/// to the chunk\'s `code` (the indices would point at the wrong lines).
pub fn expression_starts(chunk: &KnitrCapturedChunk) -> Option<Vec<usize>> {
    let mut starts = Vec::new();
    let mut lines = 0usize;
    for piece in &chunk.pieces {
        if let KnitrPiece::Source { text } = piece {
            starts.push(lines);
            lines += text.lines().count();
        }
    }
    let total = if chunk.code.is_empty() {
        lines
    } else {
        chunk.code.lines().count()
    };
    (total == lines).then_some(starts)
}

pub use crate::presenter::visibility::knitr_visibility;

/// Which of a chunk\'s outputs is a message or a warning, parallel to the outputs
/// [`pieces_to_cell_anchored`] returns (the record\'s `quarto_output_kinds`): both become stderr
/// streams there, so only this tells `class.message` from `class.warning`.
pub fn output_kinds(chunk: &KnitrCapturedChunk) -> Vec<Option<OutputKind>> {
    chunk
        .pieces
        .iter()
        .flat_map(|piece| match piece {
            KnitrPiece::Message { .. } => vec![Some(OutputKind::Message)],
            KnitrPiece::Warning { .. } => vec![Some(OutputKind::Warning)],
            KnitrPiece::Plot { files } => vec![None; files.len()],
            KnitrPiece::ImagePaths { paths, .. } => vec![None; paths.len()],
            KnitrPiece::Source { .. } | KnitrPiece::Other { .. } => vec![],
            KnitrPiece::Output { .. } | KnitrPiece::Error { .. } | KnitrPiece::Asis { .. } => {
                vec![None]
            }
        })
        .collect()
}

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
    pieces_to_cell_anchored(chunk, read_file).map(|(code, result, _)| (code, result))
}

/// [`pieces_to_cell`] plus the interleaving anchors for `render_cell_interleaved` (KNR-2, epic
/// decision 21): one entry per output, counting the source lines that precede it, in capture
/// order. The anchors are `None` when the source pieces' lines do not add up to the chunk's
/// `code` (they would point at the wrong lines). Holds, `results` and numeric `echo` are applied
/// at present time (`apply_holds`), not here.
pub fn pieces_to_cell_anchored(
    chunk: &KnitrCapturedChunk,
    read_file: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Option<(String, CellResult, Option<Vec<usize>>)> {
    let mut outputs: Vec<CellOutput> = Vec::new();
    let mut anchors: Vec<usize> = Vec::new();
    let mut source_lines = 0usize;
    let mut sources = String::new();
    for piece in &chunk.pieces {
        match piece {
            KnitrPiece::Source { text } => {
                source_lines += text.lines().count();
                sources.push_str(text);
            }
            KnitrPiece::Output { text } => outputs.push(stream("stdout", text)),
            KnitrPiece::Message { text } | KnitrPiece::Warning { text, .. } => {
                outputs.push(stream("stderr", text));
            }
            // knitr prints the whole message, so the record carries it as the traceback alone;
            // a `name: value` header would repeat it (Q1 writes `Error: boom`).
            KnitrPiece::Error { text, .. } => outputs.push(CellOutput::Error {
                ename: String::new(),
                evalue: String::new(),
                traceback: vec![text.clone()],
            }),
            KnitrPiece::Plot { files } => {
                for file in files {
                    outputs.push(image(file, read_file)?);
                }
            }
            KnitrPiece::ImagePaths { paths, widths } => {
                for (i, path) in paths.iter().enumerate() {
                    let mut out = image(path, read_file)?;
                    // The user\'s own file: the presenter references it where it is.
                    if let CellOutput::DisplayData { metadata, .. } = &mut out {
                        let k = &mut metadata["quarto_knitr"];
                        k["include_graphics"] = json!(true);
                        if let Some(Some(w)) = widths.get(i) {
                            k["width"] = json!(w);
                        }
                    }
                    outputs.push(out);
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
        anchors.resize(outputs.len(), source_lines);
    }
    let code = if chunk.code.is_empty() {
        sources
    } else {
        chunk.code.clone()
    };
    let anchors = (code.lines().count() == source_lines).then_some(anchors);
    Some((
        code,
        CellResult {
            status: ExecuteStatus::Ok,
            outputs,
            execution_count: None,
        },
        anchors,
    ))
}

fn stream(name: &str, text: &str) -> CellOutput {
    CellOutput::Stream {
        name: name.to_string(),
        text: text.to_string(),
    }
}

/// An image piece as `display_data`; `metadata.quarto_file_path` keeps knitr's own path
/// (epic decision 25: the engine-neutral key the figure sink writes the bytes under).
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
        metadata: json!({"quarto_file_path": path}),
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
    // `quarto_knitr` holds what only knitr cells carry: `widget` (an htmlwidget, which
    // `results = "hide"` keeps, unlike other `knit_asis` text) and `knit_meta`.
    let mut knitr = serde_json::Map::new();
    if widget {
        knitr.insert("widget".to_string(), json!(true));
    }
    if let Some(meta) = knit_meta {
        knitr.insert("knit_meta".to_string(), json!(meta));
    }
    let metadata = if knitr.is_empty() {
        json!({"quarto_markdown_passthrough": true})
    } else {
        json!({"quarto_knitr": knitr, "quarto_markdown_passthrough": true})
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
    fn error_is_the_printed_text_alone_with_no_header() {
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
                // knitr prints the whole message; a header would repeat it (Q1: `Error: boom`).
                assert_eq!(ename, "");
                assert_eq!(evalue, "");
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
                // Epic decision 25: the engine-neutral key, not a `quarto_knitr` one.
                assert_eq!(metadata["quarto_file_path"], "d/p-1.png");
                assert!(
                    metadata.pointer("/quarto_knitr/file").is_none(),
                    "{metadata}"
                );
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
    fn image_paths_are_marked_and_carry_their_width() {
        let c = chunk(
            json!([{"class": "image_paths", "paths": ["a.png", "b.png"], "widths": [200.0, null]}]),
        );
        let read = |_: &str| Some(vec![1, 2, 3]);
        let (_, result) = pieces_to_cell(&c, &read).unwrap();
        let meta = |i: usize| match &result.outputs[i] {
            CellOutput::DisplayData { metadata, .. } => metadata.clone(),
            o => panic!("{o:?}"),
        };
        assert_eq!(meta(0)["quarto_knitr"]["include_graphics"], true);
        assert_eq!(meta(0)["quarto_knitr"]["width"], 200.0);
        assert!(meta(1)["quarto_knitr"].get("width").is_none());
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

    #[test]
    fn only_a_widget_is_marked_as_one() {
        let (_, outputs) = map(json!([
            {"class": "asis", "text": "<b>w</b>", "mime": "text/html", "widget": true},
            {"class": "asis", "text": "<b>k</b>", "mime": "text/html"},
        ]));
        let meta = |i: usize| match &outputs[i] {
            CellOutput::DisplayData { metadata, .. } => metadata.clone(),
            other => panic!("not display_data: {other:?}"),
        };
        assert_eq!(meta(0)["quarto_knitr"]["widget"], true);
        assert!(meta(1).get("quarto_knitr").is_none(), "{}", meta(1));
    }

    fn anchors(pieces: Value) -> Option<Vec<usize>> {
        pieces_to_cell_anchored(&chunk(pieces), &no_files)
            .unwrap()
            .2
    }

    #[test]
    fn anchors_count_source_lines_before_each_output() {
        let a = anchors(json!([
            {"class": "source", "text": "a\nb\n"},
            {"class": "output", "text": "1\n"},
            {"class": "message", "text": "m\n"},
            {"class": "source", "text": "c\n"},
            {"class": "output", "text": "2\n"},
        ]));
        assert_eq!(a, Some(vec![2, 2, 3]));
    }

    #[test]
    fn anchors_stay_in_capture_order_under_holds() {
        // fig.show = "hold" and results = "hold" are applied at present time, so the
        // mapper keeps capture order and ignores the options.
        let mut c = chunk(json!([
            {"class": "source", "text": "a\n"},
            {"class": "plot", "files": ["p.png"]},
            {"class": "source", "text": "b\n"},
            {"class": "output", "text": "o\n"},
        ]));
        c.options["fig.show"] = json!("hold");
        let (_, _, a) = pieces_to_cell_anchored(&c, &|_| Some(vec![1])).unwrap();
        assert_eq!(a, Some(vec![1, 2]));
    }

    #[test]
    fn anchors_are_none_when_source_lines_disagree_with_code() {
        let mut c = chunk(json!([
            {"class": "source", "text": "a\n"},
            {"class": "output", "text": "o\n"},
        ]));
        c.code = "a\nb\n".to_string();
        let (_, _, a) = pieces_to_cell_anchored(&c, &no_files).unwrap();
        assert_eq!(a, None);
    }

    #[test]
    fn expression_starts_are_the_line_of_each_source_piece() {
        let c = chunk(json!([
            {"class": "source", "text": "a\n"},
            {"class": "output", "text": "1\n"},
            {"class": "source", "text": "f(\n  1)\n"},
            {"class": "source", "text": "z\n"},
        ]));
        assert_eq!(expression_starts(&c), Some(vec![0, 1, 3]));
    }

    #[test]
    fn expression_starts_are_none_when_source_lines_disagree_with_code() {
        let mut c = chunk(json!([{"class": "source", "text": "a\n"}]));
        c.code = "a\nb\n".to_string();
        assert_eq!(expression_starts(&c), None);
    }

    #[test]
    fn anchored_wrapper_matches_pieces_to_cell() {
        let c = chunk(json!([
            {"class": "source", "text": "a\n"},
            {"class": "output", "text": "o\n"},
        ]));
        let (code, result) = pieces_to_cell(&c, &no_files).unwrap();
        let (code2, result2, _) = pieces_to_cell_anchored(&c, &no_files).unwrap();
        assert_eq!(code, code2);
        assert_eq!(result.outputs.len(), result2.outputs.len());
    }

    #[test]
    fn asis_pieces_carry_the_markdown_passthrough_marker() {
        let (_, outputs) = map(json!([
            {"class": "asis", "text": "| a |\n", "mime": "text/markdown"},
            {"class": "asis", "text": "<b>x</b>", "mime": "text/html", "knit_meta": "[1]"},
        ]));
        for o in &outputs {
            match o {
                CellOutput::DisplayData { metadata, .. } => {
                    assert_eq!(metadata["quarto_markdown_passthrough"], json!(true));
                    assert!(metadata.pointer("/quarto_knitr/raw").is_none());
                }
                other => panic!("not display_data: {other:?}"),
            }
        }
    }

    fn vis(options: Value) -> CellVisibility {
        knitr_visibility(&options)
    }

    #[test]
    fn visibility_defaults_to_everything() {
        assert_eq!(vis(json!({})), CellVisibility::VISIBLE);
    }

    #[test]
    fn include_false_hides_the_cell() {
        assert_eq!(vis(json!({"include": false})), CellVisibility::HIDDEN);
    }

    #[test]
    fn echo_false_and_output_false_are_the_presenters() {
        let v = vis(json!({"echo": false}));
        assert!(!v.echo && v.output && v.warning);
        let v = vis(json!({"output": false}));
        assert!(v.echo && !v.output && !v.warning);
    }

    #[test]
    fn logical_warning_does_not_make_the_presenter_drop_stderr() {
        // R filtered the warnings at capture; the messages that remain must still show.
        assert!(vis(json!({"warning": false})).warning);
        assert!(vis(json!({"message": false})).warning);
        assert!(vis(json!({"warning": null})).warning); // warning = NA
    }

    #[test]
    fn numeric_echo_shows_the_source_for_the_presenter_to_select() {
        assert!(vis(json!({"echo": [1, 3]})).echo);
        assert!(vis(json!({"echo": -1})).echo);
    }

    #[test]
    fn output_kinds_run_parallel_to_the_mapped_outputs() {
        let c = chunk(json!([
            {"class": "source", "text": "x\n"},
            {"class": "output", "text": "o\n"},
            {"class": "message", "text": "m\n"},
            {"class": "warning", "text": "Warning: w", "call": "f()"},
            {"class": "plot", "files": ["a.png", "b.png"]},
            {"class": "error", "text": "Error: e", "ename": "simpleError", "evalue": "e"},
        ]));
        let read = |_: &str| Some(vec![1u8]);
        let (_, result) = pieces_to_cell(&c, &read).unwrap();
        let kinds = output_kinds(&c);
        assert_eq!(kinds.len(), result.outputs.len());
        assert_eq!(
            kinds,
            [
                None,
                Some(OutputKind::Message),
                Some(OutputKind::Warning),
                None,
                None,
                None
            ]
        );
    }
}
