/*
 * engine/ts_presentation.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * OPR-7f Part B: the attributes and file names the Julia engine route stamps.
 */

//! What QuartoNotebookRunner's notebook lets core say about how the plain route would have
//! written a cell: the wrapper id (`cell-<i>` or the label), the `execution_count`, the figure
//! file names and the figure output class (OPR-7f).
//!
//! The plain route (`ts-packages/quarto-api/src/jupyter/to-markdown.ts`, `mdFromCodeCell`) uses
//! **two counters** over the notebook the engine returned:
//!
//! - the wrapper id is `label ? asHtmlId(label) : "cell-" + i`, with `i` the raw 0-based index
//!   over **every** notebook cell (markdown, raw and code_prefix cells included);
//! - a figure file is `<stem>_files/figure-html/<base>-output-<k>.<ext>`, with `base` the label
//!   (already normalized by `asHtmlId`) or `"cell-" + (c + 1)` where `c` is the 1-based count
//!   of **code** cells (so the first code cell\'s figure is `cell-2`), and `k` the 1-based
//!   position among the outputs the plain
//!   route keeps (stderr dropped under `warning: false`; a discardable `execute_result` dropped
//!   when an image is present), counting streams and text outputs too.
//!
//! The result is written under `quarto_attrs` and `quarto_file_path`, the channels the
//! presenter reads without interpretation. Nothing here changes the unit hash.

use serde_json::{Map, Value, json};

/// What to write for one notebook code cell.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct CellPresentation {
    /// The value for the record cell\'s `quarto_attrs`.
    pub cell_attrs: Value,
    /// The cell\'s outputs with `quarto_file_path` and `quarto_attrs` added to the metadata of
    /// those that need them.
    pub outputs: Vec<Value>,
}

/// Compute the presentation of one code cell. `index` is the cell\'s raw 0-based position in the
/// notebook, `code_ordinal` its 1-based position among code cells, `stem` the document\'s file
/// stem.
pub(super) fn presentation_for(
    index: usize,
    code_ordinal: usize,
    source: &str,
    execution_count: Option<u64>,
    outputs: &[Value],
    stem: &str,
    format: &str,
) -> CellPresentation {
    let format = format.split('+').next().unwrap_or(format);
    let label = cell_label(source).map(|l| auto_identifier(&l, false));
    let label = label.filter(|l| !l.is_empty());
    let id = label.clone().unwrap_or_else(|| format!("cell-{index}"));
    let base = auto_identifier(
        &label
            .clone()
            .unwrap_or_else(|| format!("cell-{}", code_ordinal + 1)),
        true,
    );

    let mut attrs = json!({"id": id});
    if let Some(n) = execution_count {
        attrs["attributes"] = json!({"execution_count": n.to_string()});
    }

    let have_image = outputs.iter().any(|o| image_mime(o).is_some());
    let warnings_shown = !source_option_is_false(source, "warning");
    let mut kept = 0usize;
    let outputs = outputs
        .iter()
        .map(|output| {
            let dropped =
                (is_stderr(output) && !warnings_shown) || discardable_value(output, have_image);
            if dropped {
                return output.clone();
            }
            kept += 1;
            let mut output = output.clone();
            let mut out_attrs = Map::new();
            let mut path = None;
            if let Some((mime, ext)) = image_mime(&output) {
                let _ = mime;
                path = Some(format!(
                    "{stem}_files/figure-{format}/{base}-output-{kept}.{ext}"
                ));
                out_attrs.insert("classes".into(), json!(["cell-output"]));
            }
            if output["output_type"] == "execute_result"
                && let Some(n) = output["execution_count"].as_u64()
            {
                out_attrs.insert(
                    "attributes".into(),
                    json!({"execution_count": n.to_string()}),
                );
            }
            let keeps_metadata = matches!(
                output["output_type"].as_str(),
                Some("display_data" | "execute_result")
            );
            if keeps_metadata && (path.is_some() || !out_attrs.is_empty()) {
                if !output["metadata"].is_object() {
                    output["metadata"] = json!({});
                }
                if let Some(path) = path {
                    output["metadata"]["quarto_file_path"] = json!(path);
                }
                if !out_attrs.is_empty() {
                    output["metadata"]["quarto_attrs"] = Value::Object(out_attrs);
                }
            }
            output
        })
        .collect();
    CellPresentation {
        cell_attrs: attrs,
        outputs,
    }
}

/// The image mime type and file extension of an output with `data`, in the order the presenter
/// picks (svg, png, jpeg), or `None`.
fn image_mime(output: &Value) -> Option<(&'static str, &'static str)> {
    let data = output.get("data")?.as_object()?;
    [
        ("image/svg+xml", "svg"),
        ("image/png", "png"),
        ("image/jpeg", "jpeg"),
    ]
    .into_iter()
    .find(|(m, _)| data.contains_key(*m))
}

fn is_stderr(output: &Value) -> bool {
    output["output_type"] == "stream" && output["name"] == "stderr"
}

/// `isDiscardableTextExecuteResult` of the plain route: an `execute_result` whose only payload
/// is `text/plain` that is a bracketed repr next to an image, or a known plotting-library repr.
fn discardable_value(output: &Value, have_image: bool) -> bool {
    if output["output_type"] != "execute_result" {
        return false;
    }
    let Some(data) = output["data"].as_object() else {
        return false;
    };
    if data.len() != 1 {
        return false;
    }
    let lines: Vec<&str> = match data.get("text/plain") {
        Some(Value::String(s)) => vec![s.as_str()],
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect(),
        _ => return false,
    };
    let Some(first) = lines.first() else {
        return false;
    };
    if have_image && lines.len() == 1 {
        let t = first.trim();
        let open = t.starts_with(['<', '(', '[']);
        let close = t.ends_with(['>', ')', ']']);
        return open && close;
    }
    ["[<matplotlib", "<matplotlib", "<seaborn.", "<ggplot:"]
        .iter()
        .any(|p| first.starts_with(p))
}

/// The value of the `#| label:` line of a cell\'s source, if any.
fn cell_label(source: &str) -> Option<String> {
    source
        .lines()
        .map_while(|l| l.strip_prefix("#|"))
        .find_map(|l| l.trim().strip_prefix("label:"))
        .map(|v| v.trim().trim_matches(['"', '\'']).to_string())
}

/// Whether an `#| key: false` line is in the cell\'s option block.
fn source_option_is_false(source: &str, key: &str) -> bool {
    source
        .lines()
        .map_while(|l| l.strip_prefix("#|"))
        .filter_map(|l| l.trim().strip_prefix(key)?.strip_prefix(':'))
        .any(|v| v.trim() == "false")
}

/// `pandocAutoIdentifier` (`pandoc-id.ts`): remove the listed punctuation, spaces to hyphens,
/// lower case, then everything up to the first ASCII letter. With `asciify`, non-ASCII
/// characters are dropped (the plain route transliterates accented letters; an id that still
/// has a non-ASCII character is refused by the presenter with a note).
fn auto_identifier(text: &str, asciify: bool) -> String {
    const PUNCT: &str = "!\"#$%&')*+,/:;<=>?@[\\]^`{|}~";
    let s: String = text
        .chars()
        .filter(|c| !(asciify && !c.is_ascii()))
        .filter(|c| !PUNCT.contains(*c))
        .map(|c| if c.is_whitespace() { '-' } else { c })
        .flat_map(char::to_lowercase)
        .collect();
    s.trim_start_matches(|c: char| !c.is_ascii_alphabetic())
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png() -> Value {
        json!({"output_type": "display_data", "data": {"image/png": "AAAA", "text/plain": "<Plot>"}, "metadata": {}})
    }
    fn stdout(text: &str) -> Value {
        json!({"output_type": "stream", "name": "stdout", "text": [text]})
    }
    fn stderr(text: &str) -> Value {
        json!({"output_type": "stream", "name": "stderr", "text": [text]})
    }
    fn value(text: &str, count: u64) -> Value {
        json!({"output_type": "execute_result", "execution_count": count,
               "data": {"text/plain": text}, "metadata": {}})
    }
    fn path_of(output: &Value) -> Option<&str> {
        output["metadata"]["quarto_file_path"].as_str()
    }

    /// `[code]` (the fixture `cell-classes`): wrapper `#cell-0`, figure `cell-2-output-1`.
    #[test]
    fn the_only_code_cell_is_cell_0_with_a_cell_2_figure() {
        let p = presentation_for(0, 1, "plot_png()", Some(1), &[png()], "notebook", "html");
        assert_eq!(p.cell_attrs["id"], "cell-0");
        assert_eq!(p.cell_attrs["attributes"]["execution_count"], "1");
        assert_eq!(
            path_of(&p.outputs[0]),
            Some("notebook_files/figure-html/cell-2-output-1.png")
        );
        assert_eq!(
            p.outputs[0]["metadata"]["quarto_attrs"]["classes"],
            json!(["cell-output"])
        );
    }

    /// `[md, code]` (the fixture `hidden-then-plot`): wrapper `#cell-1`, figure `cell-3`?
    /// No: the figure uses the code ordinal. One code cell after one markdown cell is
    /// `#cell-1` with `cell-2`. (The fixture\'s `cell-3` has two code cells before the plot.)
    #[test]
    fn the_two_counters_are_independent() {
        let p = presentation_for(1, 1, "plot_png()", Some(2), &[png()], "notebook", "html");
        assert_eq!(p.cell_attrs["id"], "cell-1");
        assert_eq!(
            path_of(&p.outputs[0]),
            Some("notebook_files/figure-html/cell-2-output-1.png")
        );
        // `[md, code, code]`: the second code cell has id `cell-2` and figure `cell-3`.
        let p = presentation_for(2, 2, "plot_png()", Some(3), &[png()], "notebook", "html");
        assert_eq!(p.cell_attrs["id"], "cell-2");
        assert_eq!(
            path_of(&p.outputs[0]),
            Some("notebook_files/figure-html/cell-3-output-1.png")
        );
        // `[md, md, code]`: `cell-2` and `cell-2`.
        let p = presentation_for(2, 1, "plot_png()", Some(1), &[png()], "notebook", "html");
        assert_eq!(p.cell_attrs["id"], "cell-2");
        assert_eq!(
            path_of(&p.outputs[0]),
            Some("notebook_files/figure-html/cell-2-output-1.png")
        );
    }

    #[test]
    fn the_figure_path_is_named_after_the_format() {
        // The sink prefers a producer path over its own name, so this directory is the one
        // Julia figures land in (OPR-9b).
        for (format, dir) in [
            ("gfm", "figure-gfm"),
            ("typst", "figure-typst"),
            ("html", "figure-html"),
        ] {
            let p = presentation_for(0, 1, "plot_png()", Some(1), &[png()], "notebook", format);
            assert_eq!(
                path_of(&p.outputs[0]),
                Some(format!("notebook_files/{dir}/cell-2-output-1.png").as_str()),
                "{format}"
            );
        }
        let p = presentation_for(
            0,
            1,
            "plot_png()",
            Some(1),
            &[png()],
            "notebook",
            "docx+smart",
        );
        assert_eq!(
            path_of(&p.outputs[0]),
            Some("notebook_files/figure-docx/cell-2-output-1.png")
        );
    }

    #[test]
    fn a_cell_that_prints_then_plots_gets_output_2() {
        let p = presentation_for(
            0,
            1,
            "print(1)\nplot()",
            Some(1),
            &[stdout("1\n"), png()],
            "n",
            "html",
        );
        assert_eq!(path_of(&p.outputs[0]), None);
        assert_eq!(
            path_of(&p.outputs[1]),
            Some("n_files/figure-html/cell-2-output-2.png")
        );
    }

    #[test]
    fn two_figures_are_numbered_in_order_with_their_own_extension() {
        let svg = json!({"output_type": "display_data", "data": {"image/svg+xml": "<svg/>"}, "metadata": {}});
        let p = presentation_for(0, 1, "x", Some(1), &[png(), svg], "n", "html");
        assert_eq!(
            path_of(&p.outputs[0]),
            Some("n_files/figure-html/cell-2-output-1.png")
        );
        assert_eq!(
            path_of(&p.outputs[1]),
            Some("n_files/figure-html/cell-2-output-2.svg")
        );
    }

    #[test]
    fn a_label_replaces_the_base_and_the_id() {
        let p = presentation_for(
            4,
            3,
            "#| label: fig-plot\nplot()",
            Some(1),
            &[png()],
            "n",
            "html",
        );
        assert_eq!(p.cell_attrs["id"], "fig-plot");
        assert_eq!(
            path_of(&p.outputs[0]),
            Some("n_files/figure-html/fig-plot-output-1.png")
        );
        // A label is normalized as the plain route does (`asHtmlId`): punctuation such as `:`
        // is removed, spaces become hyphens, letters are lower-cased; the file base is the
        // same text, because the plain route derives it from the normalized label.
        let p = presentation_for(
            0,
            1,
            "#| label: Fig:My Plot\nx",
            Some(1),
            &[png()],
            "n",
            "html",
        );
        assert_eq!(p.cell_attrs["id"], "figmy-plot");
        assert_eq!(
            path_of(&p.outputs[0]),
            Some("n_files/figure-html/figmy-plot-output-1.png")
        );
    }

    #[test]
    fn stderr_is_not_counted_when_warnings_are_hidden_and_is_when_shown() {
        let outs = [stderr("w\n"), png()];
        let shown = presentation_for(0, 1, "plot()", Some(1), &outs, "n", "html");
        assert_eq!(
            path_of(&shown.outputs[1]),
            Some("n_files/figure-html/cell-2-output-2.png")
        );
        let hidden = presentation_for(
            0,
            1,
            "#| warning: false\nplot()",
            Some(1),
            &outs,
            "n",
            "html",
        );
        assert_eq!(
            path_of(&hidden.outputs[1]),
            Some("n_files/figure-html/cell-2-output-1.png")
        );
    }

    #[test]
    fn a_discardable_value_next_to_an_image_is_not_counted() {
        let outs = [value("<Plot>", 1), png()];
        let p = presentation_for(0, 1, "plot()", Some(1), &outs, "n", "html");
        assert_eq!(
            path_of(&p.outputs[1]),
            Some("n_files/figure-html/cell-2-output-1.png")
        );
        // With no image it is kept, so a following output is position 2.
        let p = presentation_for(
            0,
            1,
            "x",
            Some(1),
            &[value("<Plot>", 1), stdout("a\n")],
            "n",
            "html",
        );
        assert_eq!(p.outputs.len(), 2);
    }

    #[test]
    fn execute_results_carry_their_count_and_displays_do_not() {
        let p = presentation_for(0, 1, "x", Some(7), &[value("42", 7), png()], "n", "html");
        assert_eq!(p.cell_attrs["attributes"]["execution_count"], "7");
        assert_eq!(
            p.outputs[0]["metadata"]["quarto_attrs"]["attributes"]["execution_count"],
            "7"
        );
        assert!(p.outputs[1]["metadata"]["quarto_attrs"]["attributes"].is_null());
    }

    #[test]
    fn a_cell_with_no_figure_gets_no_file_path_and_no_count_means_no_attribute() {
        let p = presentation_for(3, 2, "x", None, &[stdout("a\n")], "n", "html");
        assert_eq!(p.cell_attrs["id"], "cell-3");
        assert!(p.cell_attrs.get("attributes").is_none());
        assert!(path_of(&p.outputs[0]).is_none());
        assert_eq!(p.outputs[0], stdout("a\n"));
    }
}
