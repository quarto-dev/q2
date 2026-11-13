/*
 * engine/knitr/span_records.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! EXR-12: the Rust half of knitr's inline-span records.
//!
//! - [`SpanPlan`] takes the ids of the brace-spelling `r` spans knitr owns from the stage's
//!   shared unit list and hands them to the inline rewrite
//!   ([`resolve_inline_r_expressions_with_ids`]), which embeds each in its span.
//! - [`span_output`] is the record's one output for a span: the markdown knitr inserted.
//! - [`span_location`] names a span's place in the author's file for a diagnostic.
//!
//! Only the brace spelling is a unit (EXR-2 decision 2); a classic `` `r expr` `` span is
//! rewritten as before, carries no id and has no record. Ids come from the shared list and
//! are never generated here, and the rewrite never numbers spans: it asks for the id of the
//! span at a byte offset, and the plan answers by line (the one position that survives the
//! edits knitr's input has been through), in order within a line.

use std::collections::{BTreeMap, VecDeque};

use serde_json::{Value, json};

use crate::engine::context::ExecutionContext;
use crate::engine::units::{Unit, line_at, locate_units};
use crate::record::{QUARTO_MARKDOWN_MIME, VERBATIM_KEY};

use super::error_lines::engine_lines_to_document;
use super::pieces_map::span_piece_output;
use super::preprocess::{resolve_inline_r_expressions, resolve_inline_r_expressions_with_ids};
#[cfg(test)]
use super::types::KnitrPiece;
use super::types::KnitrSpanRecord;

/// The only language the knitr pass evaluates inline: `preprocess.rs`'s pattern matches
/// `{r}` and nothing else. A span in a language knitr owns through interop (`python`,
/// `sql`, ...) stays literal, so it is no unit of this engine's and not counted.
const SPAN_LANGUAGE: &str = "r";

/// The ids to embed, by the line of the span they belong to, plus every id in document
/// order (what the engine must hand back).
#[derive(Debug, Default)]
pub(super) struct SpanPlan {
    by_line: BTreeMap<usize, VecDeque<String>>,
    pub(super) expected: Vec<String>,
}

impl SpanPlan {
    /// The owned brace `r` spans of `ctx.units`, relocated into `text` (the text the
    /// rewrite will run over). Empty when no record is wanted or the context carries no
    /// units, which keeps the live path byte-identical.
    pub(super) fn new(text: &str, ctx: &ExecutionContext) -> Self {
        let mut plan = Self::default();
        let owned = ctx
            .owned_languages
            .iter()
            .any(|o| o.eq_ignore_ascii_case(SPAN_LANGUAGE));
        if !ctx.builds_record() || ctx.units.is_empty() || !owned {
            return plan;
        }
        for unit in locate_units(&ctx.units, text) {
            let Unit::Inline(span) = &unit else { continue };
            let Some(id) = span.id.as_ref() else { continue };
            if span.lang != SPAN_LANGUAGE {
                continue;
            }
            plan.by_line
                .entry(line_at(text, span.start))
                .or_default()
                .push_back(id.0.clone());
            plan.expected.push(id.0.clone());
        }
        plan
    }

    /// Rewrite the inline expressions of `text`, embedding an id in each brace span the
    /// plan has one for.
    pub(super) fn resolve(mut self, text: &str) -> String {
        if self.expected.is_empty() {
            // The live path: no ids, the original rewrite.
            return resolve_inline_r_expressions(text);
        }
        // The rewrite asks in document order, so the line is counted incrementally.
        let (mut at, mut line) = (0, 1);
        resolve_inline_r_expressions_with_ids(text, &mut |offset| {
            line += text[at..offset].matches('\n').count();
            at = offset;
            self.by_line.get_mut(&line)?.pop_front()
        })
    }
}

/// The record's one output for a span, as `display_data`, tagged with the output format it was
/// rendered for. Its `text/x-quarto-markdown` is the markdown knitr inserted, which the
/// presenter inserts verbatim. When knitr read the value as `knit_asis` or an image path, the
/// same bundle also holds the value as knitr produced it (`text/html`, an image) with the
/// metadata a chunk output takes (`quarto_knitr.knit_meta`, `quarto_file_path`), so replay and
/// the widget adapters need no R (KNR-4 decision 6).
pub(super) fn span_output(
    span: &KnitrSpanRecord,
    format: &str,
    read_file: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Value {
    let rich = span
        .piece
        .as_ref()
        .and_then(|piece| span_piece_output(piece, read_file));
    let mut out = match rich {
        Some(output) => output.to_nbformat(),
        None => json!({"output_type": "display_data", "data": {}, "metadata": {}}),
    };
    out["data"][QUARTO_MARKDOWN_MIME] = json!(span.markdown);
    out["metadata"]["quarto_format"] = json!(format);
    // knitr inserted this text as it is, so the presenter must too.
    out["metadata"][VERBATIM_KEY] = json!(true);
    out
}

/// `file:line` of the span with unit id `id`, in the author's file where the stage's
/// source map can say and in the text knitr was given otherwise. `engine_text` is the text
/// the stage scanned (before this engine's edits).
pub(super) fn span_location(ctx: &ExecutionContext, engine_text: &str, id: &str) -> String {
    let file = ctx.source_path.display();
    let Some(span) = locate_units(&ctx.units, engine_text)
        .into_iter()
        .find_map(|u| match u {
            Unit::Inline(i) if i.id.as_ref().is_some_and(|i| i.0 == id) => Some(i),
            _ => None,
        })
    else {
        return file.to_string();
    };
    let line = line_at(engine_text, span.start);
    let mapped =
        engine_lines_to_document(ctx, engine_text, line..=line).map_or(line, |r| *r.start());
    format!("{file}:{mapped}")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::engine::units::scan_units;
    use crate::record::{AuthorTexts, fill_ids};

    fn ctx(doc: &str, owned: &[&str], want_record: bool) -> ExecutionContext {
        let mut units = scan_units(doc);
        let texts = AuthorTexts::from_scanned(&units);
        fill_ids(&mut units, &texts).unwrap();
        ExecutionContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/proj"),
            PathBuf::from("/proj/doc.qmd"),
            "html",
        )
        .with_owned_languages(owned.iter().map(|s| s.to_string()).collect())
        .with_units(units)
        .with_author_texts(texts)
        .with_want_record(want_record)
    }

    /// The ids embedded in `rewritten`, in order.
    fn embedded_ids(rewritten: &str) -> Vec<String> {
        rewritten
            .split(".QuartoInlineId(\"")
            .skip(1)
            .map(|rest| rest[..rest.find('"').unwrap()].to_string())
            .collect()
    }

    const MIXED: &str = "\
Prose `{r} a` and classic `r b`, then `{r} c`.

```{r}
x <- 1
```

Python `{python} p`, other `{R} q`, and `{r} d` here.

::: {#h data-v=\"`{r} e`\"}
:::

> quoted `{r} f`
";

    #[test]
    fn ids_come_from_the_shared_unit_list() {
        let c = ctx(MIXED, &["r", "python"], true);
        let plan = SpanPlan::new(MIXED, &c);
        let shared: Vec<String> = c
            .units
            .iter()
            .filter_map(|u| match u {
                Unit::Inline(i) if i.lang == "r" => Some(i.id.as_ref().unwrap().0.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(shared.len(), 5, "a, c, d, e, f");
        assert_eq!(plan.expected, shared);
        let out = plan.resolve(MIXED);
        // The rewrite embeds exactly the shared ids, in order. A classic span, another
        // language's span and a `{R}` span are skipped without shifting anyone's id.
        assert_eq!(embedded_ids(&out), shared);
        assert!(out.contains("`r b`"), "classic span untouched: {out}");
        assert!(
            out.contains("`{python} p`") && out.contains("`{R} q`"),
            "{out}"
        );
    }

    #[test]
    fn two_spans_on_one_line_take_their_ids_in_order() {
        let doc = "`{r} a` then `{r} b` then `r z` then `{r} c`\n";
        let c = ctx(doc, &["r"], true);
        let out = SpanPlan::new(doc, &c).resolve(doc);
        let ids = embedded_ids(&out);
        assert_eq!(ids.len(), 3);
        for (id, expr) in ids.iter().zip(["a", "b", "c"]) {
            assert!(
                out.contains(&format!(
                    ".QuartoInlineId(\"{id}\"); .QuartoInlineRender({expr})"
                )),
                "{id} belongs to {expr}: {out}"
            );
        }
    }

    #[test]
    fn nothing_is_embedded_unless_a_record_is_wanted_and_r_is_owned() {
        for c in [
            ctx(MIXED, &["r"], false),
            ctx(MIXED, &["python"], true),
            ExecutionContext::new(
                PathBuf::from("/tmp"),
                PathBuf::from("/proj"),
                PathBuf::from("/proj/doc.qmd"),
                "html",
            )
            .with_owned_languages(vec!["r".into()])
            .with_want_record(true),
        ] {
            let plan = SpanPlan::new(MIXED, &c);
            assert!(plan.expected.is_empty());
            assert_eq!(plan.resolve(MIXED), resolve_inline_r_expressions(MIXED));
        }
    }

    /// The guard against phantom units: for every fixture, the brace spans the rewrite
    /// evaluates are exactly the scanner's `r` units, so each unit's id lands on a span
    /// knitr will evaluate and no evaluated span lacks a unit. Classic spans are excluded
    /// from both sides by design. Fixtures are stage text: a span inside a display fence
    /// is masked upstream (`nested_cell_mask`), so no fixture carries one.
    #[test]
    fn resolver_and_scanner_agree_on_the_corpus() {
        let corpus = [
            MIXED,
            "Prose: `{r} version`.\n\n::: {#hero data-version=\"`{r} version`\"}\nHero.\n:::\n\n[l](https://e.com \"`{r} version`\")\n",
            "Adjacent `{r} a``{r} b` end.\n",
            "A `{r} sum(\n  c(1, 2)\n)` multi-line.\n",
            "`{r}\ttab` and `{r}   spaced  `.\n",
            "---\ntitle: \"`{r} t`\"\n---\n\nBody `{r} u`.\n",
            "- item `{r} a`\n- item `r b`\n\n| h |\n|---|\n| `{r} c` |\n",
            "```{r}\n#| echo: false\nv <- 1\n```\n\nAfter `{r} v`.\n",
            "```{python}\nprint(1)\n```\n\n`{python} 1` and `{r} 2`.\n",
            "A `{r}` empty and `r` empty.\n",
        ];
        for doc in corpus {
            let c = ctx(doc, &["r"], true);
            let mut rewrite_offsets = Vec::new();
            resolve_inline_r_expressions_with_ids(doc, &mut |o| {
                rewrite_offsets.push(o);
                None
            });
            let scanner_offsets: Vec<usize> = c
                .units
                .iter()
                .filter_map(|u| match u {
                    Unit::Inline(i) if i.lang == "r" => Some(i.start),
                    _ => None,
                })
                .collect();
            assert_eq!(rewrite_offsets, scanner_offsets, "{doc:?}");
            // And the ids really land: one per unit, in order.
            let plan = SpanPlan::new(doc, &c);
            let expected = plan.expected.clone();
            assert_eq!(embedded_ids(&plan.resolve(doc)), expected, "{doc:?}");
        }
    }

    /// A known by-design difference (S2): the scanner skips HTML comments, the rewrite
    /// does not. The span gets no id, so it is evaluated by knitr and unrecorded; the
    /// other spans keep theirs.
    #[test]
    fn a_span_in_an_html_comment_is_evaluated_but_not_a_unit() {
        let doc = "<!-- `{r} hidden` -->\n\nShown `{r} x`.\n";
        let c = ctx(doc, &["r"], true);
        let plan = SpanPlan::new(doc, &c);
        assert_eq!(plan.expected.len(), 1);
        let out = plan.resolve(doc);
        assert_eq!(embedded_ids(&out), plan_ids(doc, &c));
        assert!(out.contains("`r .QuartoInlineRender(hidden)`"), "{out}");
    }

    fn plan_ids(doc: &str, c: &ExecutionContext) -> Vec<String> {
        SpanPlan::new(doc, c).expected
    }

    #[test]
    fn an_edit_between_scan_and_rewrite_does_not_move_an_id() {
        // knitr's input has had option lines added above the span; the ids follow the
        // span, not its old line.
        let doc = "```{r}\n1\n```\n\nSpan `{r} x`.\n";
        let c = ctx(doc, &["r"], true);
        let edited = "```{r}\n#| quarto-unit-id: \"c\"\n1\n```\n\nSpan `{r} x`.\n";
        let plan = SpanPlan::new(edited, &c);
        let expected = plan.expected.clone();
        assert_eq!(expected.len(), 1);
        assert_eq!(embedded_ids(&plan.resolve(edited)), expected);
    }

    #[test]
    fn span_output_is_rendered_markdown_tagged_with_the_format() {
        let out = span_output(
            &KnitrSpanRecord {
                id: "1-aaaaaaaa-inline".into(),
                markdown: "x\\*y".into(),
                piece: None,
            },
            "html",
            &|_| None,
        );
        assert_eq!(out["output_type"], "display_data");
        assert_eq!(out["data"][QUARTO_MARKDOWN_MIME], "x\\*y");
        assert_eq!(out["metadata"]["quarto_format"], "html");
        assert_eq!(out["metadata"][VERBATIM_KEY], true);
    }

    #[test]
    fn a_span_location_names_the_file_and_line() {
        let doc = "Intro\n\nSpan `{r} x`.\n";
        let c = ctx(doc, &["r"], true);
        let id = plan_ids(doc, &c).remove(0);
        let file = c.source_path.display().to_string();
        assert_eq!(span_location(&c, doc, &id), format!("{file}:3"));
        // An id the text does not hold names the file alone.
        assert_eq!(span_location(&c, doc, "nope"), file);
    }

    fn piece_span(markdown: &str, piece: serde_json::Value) -> KnitrSpanRecord {
        KnitrSpanRecord {
            id: "1-aaaaaaaa-inline".into(),
            markdown: markdown.into(),
            piece: Some(serde_json::from_value(piece).unwrap()),
        }
    }

    #[test]
    fn a_span_record_deserializes_with_and_without_a_piece() {
        let plain: KnitrSpanRecord =
            serde_json::from_str(r#"{"id": "1-a-inline", "markdown": "2"}"#).unwrap();
        assert!(plain.piece.is_none());
        let rich: KnitrSpanRecord = serde_json::from_str(
            r#"{"id": "1-a-inline", "markdown": "<b>x</b>",
                "piece": {"class": "asis", "text": "<b>x</b>", "mime": "text/html"}}"#,
        )
        .unwrap();
        assert!(matches!(rich.piece, Some(KnitrPiece::Asis { .. })));
    }

    #[test]
    fn an_html_span_keeps_its_mime_beside_the_inserted_markdown() {
        let span = piece_span(
            "<b>x</b>",
            json!({"class": "asis", "text": "<b>x</b>", "mime": "text/html"}),
        );
        let out = span_output(&span, "html", &|_| None);
        // The presenter still reads the markdown the live path inserted ...
        assert_eq!(out["data"][QUARTO_MARKDOWN_MIME], "<b>x</b>");
        // ... and the record also holds the value as knitr produced it.
        assert_eq!(out["data"]["text/html"], "<b>x</b>");
        assert_eq!(out["metadata"]["quarto_format"], "html");
    }

    #[test]
    fn a_widget_span_carries_its_dependencies_in_knit_meta() {
        let span = piece_span(
            "\n```{=html}\n<div id=\"w\"></div>\n```\n",
            json!({"class": "asis", "mime": "text/html", "widget": true, "knit_meta": "[1,2]",
                   "text": "\n```{=html}\n<div id=\"w\"></div>\n```\n"}),
        );
        let out = span_output(&span, "html", &|_| None);
        assert_eq!(out["data"]["text/html"], "<div id=\"w\"></div>");
        assert_eq!(out["metadata"]["quarto_knitr"]["knit_meta"], "[1,2]");
        assert_eq!(
            out["data"][QUARTO_MARKDOWN_MIME],
            "\n```{=html}\n<div id=\"w\"></div>\n```\n"
        );
    }

    #[test]
    fn an_include_graphics_span_keeps_the_image_and_its_path() {
        let span = piece_span(
            "![](img.png)",
            json!({"class": "image_paths", "paths": ["img.png"]}),
        );
        let out = span_output(&span, "html", &|p| {
            (p == "img.png").then(|| vec![0x89, b'P'])
        });
        assert!(out["data"]["image/png"].is_string());
        assert_eq!(out["metadata"]["quarto_file_path"], "img.png");
        assert_eq!(out["data"][QUARTO_MARKDOWN_MIME], "![](img.png)");
    }

    #[test]
    fn a_span_whose_image_cannot_be_read_keeps_only_the_markdown() {
        let span = piece_span(
            "![](gone.png)",
            json!({"class": "image_paths", "paths": ["gone.png"]}),
        );
        let out = span_output(&span, "html", &|_| None);
        assert_eq!(out["data"].as_object().unwrap().len(), 1);
        assert_eq!(out["data"][QUARTO_MARKDOWN_MIME], "![](gone.png)");
    }

    #[test]
    fn several_image_paths_keep_only_the_markdown() {
        let span = piece_span(
            "![](a.png)![](b.png)",
            json!({"class": "image_paths", "paths": ["a.png", "b.png"]}),
        );
        let out = span_output(&span, "html", &|_| Some(vec![1]));
        assert_eq!(out["data"].as_object().unwrap().len(), 1);
    }
}
