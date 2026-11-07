/*
 * presenter/inline.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * EXR-3 Phase 0: present the value of an inline expression (pure, no kernel).
 */

//! Present the value of an inline expression (a brace-spelling span such as
//! `` `{python} 1+1` ``) as markdown.
//!
//! Ports two Q1 functions: `displayDataMimeType`
//! (`external-sources/quarto-cli/src/core/jupyter/display-data.ts`) and the
//! `resolveUserExpressions` / `asEscapedMarkdown` pair
//! (`external-sources/quarto-cli/src/core/jupyter/jupyter-inline.ts`).
//!
//! [`display_data_mime_type`] is a separate function from the cell path's
//! `format_outputs` precedence; it does not change it. The TypeScript port
//! (`ts-packages/quarto-api/src/jupyter/display-data.ts`) is checked against the same
//! fixture, `display-data-precedence.json`, so the two cannot drift silently.

use super::output::{CellOutput, CellResult, ExecuteStatus, MimeBundle, extract_text_content};

const TEXT_HTML: &str = "text/html";
const TEXT_LATEX: &str = "text/latex";
const TEXT_MARKDOWN: &str = "text/markdown";
const TEXT_PLAIN: &str = "text/plain";
const IMAGE_SVG: &str = "image/svg+xml";
const IMAGE_PNG: &str = "image/png";
const IMAGE_JPEG: &str = "image/jpeg";
const APPLICATION_PDF: &str = "application/pdf";
const WIDGET_STATE: &str = "application/vnd.jupyter.widget-state+json";
const WIDGET_VIEW: &str = "application/vnd.jupyter.widget-view+json";
const APPLICATION_JAVASCRIPT: &str = "application/javascript";

/// The three output-format flags Q1's `displayDataMimeType` reads
/// (`toHtml`, `toLatex`, `toMarkdown`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InlineTarget {
    pub to_html: bool,
    pub to_latex: bool,
    pub to_markdown: bool,
}

impl InlineTarget {
    /// Classify a format identifier such as `html`, `revealjs`, `pdf`, `gfm`
    /// or `typst`. Extensions (`html+smart`) are ignored. A format that is none of
    /// the three classes (docx, typst, ...) gets no flags.
    pub fn from_format(format: &str) -> Self {
        let base = format.split('+').next().unwrap_or(format);
        const HTML: &[&str] = &[
            "html",
            "html4",
            "html5",
            "revealjs",
            "slidy",
            "slideous",
            "s5",
            "dzslides",
            "epub",
            "epub2",
            "epub3",
            "dashboard",
        ];
        const LATEX: &[&str] = &["pdf", "latex", "beamer"];
        const MARKDOWN: &[&str] = &[
            "markdown",
            "gfm",
            "commonmark",
            "commonmark_x",
            "markdown_strict",
            "markdown_mmd",
            "markdown_phpextra",
            "markua",
        ];
        Self {
            to_html: HTML.contains(&base),
            to_latex: LATEX.contains(&base),
            to_markdown: MARKDOWN.contains(&base),
        }
    }
}

/// Whether the bundle's `text/html` holds both `<table` and `</table`
/// (Q1 `displayDataHasHtmlTable`).
fn has_html_table(bundle: &MimeBundle) -> bool {
    let Some(value) = bundle.get(TEXT_HTML) else {
        return false;
    };
    let lines: Vec<String> = match value {
        serde_json::Value::String(s) => vec![s.to_lowercase()],
        serde_json::Value::Array(a) => a
            .iter()
            .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_lowercase))
            .collect(),
        _ => return false,
    };
    lines.iter().any(|l| l.contains("<table")) && lines.iter().any(|l| l.contains("</table"))
}

/// Select the mime type to present from `bundle`, or `None` if no candidate is
/// present. Port of Q1 `displayDataMimeType`, including its quirk that for an html
/// target the widget/javascript/html cluster always ranks first, whether or not
/// the target is also markdown.
pub fn display_data_mime_type(bundle: &MimeBundle, target: InlineTarget) -> Option<&'static str> {
    let mut priority: Vec<&'static str> = vec![TEXT_MARKDOWN, IMAGE_SVG, IMAGE_PNG, IMAGE_JPEG];
    if target.to_html {
        priority.splice(
            0..0,
            [WIDGET_STATE, WIDGET_VIEW, APPLICATION_JAVASCRIPT, TEXT_HTML],
        );
    } else if target.to_latex {
        priority.extend([TEXT_LATEX, APPLICATION_PDF]);
    } else if target.to_markdown {
        priority.push(TEXT_HTML);
    }
    if has_html_table(bundle) && !priority.contains(&TEXT_HTML) {
        priority.push(TEXT_HTML);
    }
    priority.push(TEXT_PLAIN);
    priority.into_iter().find(|m| bundle.contains_key(*m))
}

/// A backtick run for an inline code span, longer than any run in `text`.
/// Q1 uses a fixed single backtick, which a value containing one would break.
fn inline_ticks(text: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        if c == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    "`".repeat(longest + 1)
}

/// A raw inline span: `` `data`{=format} ``, with the ticks sized to the content and a
/// space of padding when the content starts or ends with a backtick.
fn raw_inline(data: &str, format: &str) -> String {
    let ticks = inline_ticks(data);
    let pad = if data.starts_with('`') || data.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{ticks}{pad}{data}{pad}{ticks}{{={format}}}")
}

/// Q1 `asEscapedMarkdown`: only a value that starts and ends with the same kind of
/// quote is changed. The quotes are stripped, backslashes doubled, and every
/// character of ``\ ` * _ { } [ ] ( ) > # + , - . !`` backslash-escaped (the regex
/// class `+-.` is a range that covers the comma).
fn escape_quoted(s: &str) -> String {
    let quoted =
        (s.starts_with('\'') && s.ends_with('\'')) || (s.starts_with('"') && s.ends_with('"'));
    if !quoted {
        return s.to_string();
    }
    // JavaScript `slice(1, -1)`: a lone quote leaves the empty string.
    let inner = if s.len() >= 2 { &s[1..s.len() - 1] } else { "" };
    let doubled = inner.replace('\\', "\\\\");
    let mut out = String::with_capacity(doubled.len());
    for c in doubled.chars() {
        if "\\`*_{}[]()>#+,-.!".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Present a span's value as markdown, or `None` when the bundle has no usable text
/// type (the caller warns and leaves the span source in place).
///
/// Only the text types q2 can splice into prose are candidates: an image or widget
/// value would reach Q1's plain-text branch as raw base64 or JSON. Raw html and
/// latex become raw inline spans, markdown is inserted as is, and plain text goes
/// through the quote-aware escape. The result may be the empty string (a quoted
/// `''`); the caller treats it like Q1: leave the span source in place.
pub fn render_inline(bundle: &MimeBundle, target: InlineTarget) -> Option<String> {
    let text_only: MimeBundle = bundle
        .iter()
        .filter(|(k, _)| {
            matches!(
                k.as_str(),
                TEXT_HTML | TEXT_LATEX | TEXT_MARKDOWN | TEXT_PLAIN
            )
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let mime = display_data_mime_type(&text_only, target)?;
    let data = extract_text_content(&text_only[mime]);
    Some(match mime {
        TEXT_HTML => raw_inline(&data, "html"),
        TEXT_LATEX => raw_inline(&data, "tex"),
        TEXT_MARKDOWN => data,
        _ => escape_quoted(&data),
    })
}

// ── Inline values (moved from `engine::jupyter::inline` in EXR-6 Phase 1, so the
// document driver can read a span's value from a record on every target) ────────

/// The text mime types a span's value can use.
const TEXT_MIMES: [&str; 4] = ["text/plain", "text/html", "text/latex", "text/markdown"];

/// Escape `s` for the inside of a YAML double-quoted scalar. A newline is not handled:
/// the caller rejects it first (it would fold to a space).
pub fn escape_yaml_double(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// The front matter key on the line holding byte `offset` of `text` (`title` for
/// `title: "a `{python} x`"`), or `None` when the line has no `key:` prefix.
pub fn front_matter_key_at(text: &str, offset: usize) -> Option<String> {
    let line_start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[offset..].find('\n').map_or(text.len(), |i| offset + i);
    let line = &text[line_start..line_end];
    let (key, _) = line.split_once(':')?;
    let key = key
        .trim()
        .trim_start_matches("- ")
        .trim_matches(['"', '\'']);
    (!key.is_empty()).then(|| key.to_string())
}

/// What a span's execution amounted to.
#[derive(Debug, Clone, PartialEq)]
pub enum InlineOutcome {
    /// The last text-bearing `execute_result` or `display_data`.
    Value(MimeBundle),
    /// The kernel raised.
    Raised { ename: String, evalue: String },
    /// The kernel finished without any output: a statement, or a value that prints
    /// nothing (see the module docs).
    Silent,
    /// Outputs arrived, but none carried a text mime type (for example only an image).
    NoTextValue,
}

fn has_text_mime(bundle: &MimeBundle) -> bool {
    TEXT_MIMES.iter().any(|m| bundle.contains_key(*m))
}

/// Read a span's value from its execution result (design decision 2): the last output
/// that carries a text mime type, whether `execute_result` or `display_data`. Streams
/// and other displays are dropped; an error wins over everything.
pub fn classify_inline_result(result: &CellResult) -> InlineOutcome {
    for output in &result.outputs {
        if let CellOutput::Error { ename, evalue, .. } = output {
            return InlineOutcome::Raised {
                ename: ename.clone(),
                evalue: evalue.clone(),
            };
        }
    }
    if let ExecuteStatus::Error { ename, evalue, .. } = &result.status {
        return InlineOutcome::Raised {
            ename: ename.clone(),
            evalue: evalue.clone(),
        };
    }

    let mut value = None;
    for output in &result.outputs {
        match output {
            CellOutput::ExecuteResult { data, .. } | CellOutput::DisplayData { data, .. }
                if has_text_mime(data) =>
            {
                value = Some(data);
            }
            other => tracing::debug!(output = ?other, "inline expression: dropping output"),
        }
    }
    match value {
        Some(bundle) => InlineOutcome::Value(bundle.clone()),
        None if result.outputs.is_empty() => InlineOutcome::Silent,
        None => InlineOutcome::NoTextValue,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    const HTML: InlineTarget = InlineTarget {
        to_html: true,
        to_latex: false,
        to_markdown: false,
    };
    const LATEX: InlineTarget = InlineTarget {
        to_html: false,
        to_latex: true,
        to_markdown: false,
    };
    const OTHER: InlineTarget = InlineTarget {
        to_html: false,
        to_latex: false,
        to_markdown: false,
    };

    fn bundle(pairs: &[(&str, Value)]) -> MimeBundle {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    fn read_fixture(name: &str) -> Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../ts-packages/quarto-api/src/jupyter/__fixtures__")
            .join(name);
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
    }

    #[test]
    fn precedence_matches_shared_fixture() {
        let fixture = read_fixture("display-data-precedence.json");
        let cases = fixture["cases"].as_array().unwrap();
        assert!(cases.len() >= 20, "fixture shrank");
        for case in cases {
            let flag = |k: &str| case["flags"][k].as_bool().unwrap_or(false);
            let target = InlineTarget {
                to_html: flag("toHtml"),
                to_latex: flag("toLatex"),
                to_markdown: flag("toMarkdown"),
            };
            let data: MimeBundle = case["data"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            assert_eq!(
                display_data_mime_type(&data, target),
                case["expected"].as_str(),
                "case: {}",
                case["name"]
            );
        }
    }

    #[test]
    fn escape_matches_q1_regex_table() {
        let table = read_fixture("inline-escape.json");
        for row in table.as_array().unwrap() {
            let input = row["input"].as_str().unwrap();
            assert_eq!(
                escape_quoted(input),
                row["expected"].as_str().unwrap(),
                "input: {input}"
            );
        }
    }

    #[test]
    fn target_from_format() {
        assert!(InlineTarget::from_format("html").to_html);
        assert!(InlineTarget::from_format("revealjs").to_html);
        assert!(InlineTarget::from_format("html+smart").to_html);
        assert!(InlineTarget::from_format("pdf").to_latex);
        assert!(InlineTarget::from_format("gfm").to_markdown);
        assert_eq!(InlineTarget::from_format("typst"), OTHER);
        assert_eq!(InlineTarget::from_format("docx"), OTHER);
    }

    #[test]
    fn markdown_is_inserted_as_is() {
        let b = bundle(&[
            ("text/markdown", json!("**bold** _x_")),
            ("text/plain", json!("'ignored'")),
        ]);
        assert_eq!(render_inline(&b, HTML).unwrap(), "**bold** _x_");
    }

    #[test]
    fn html_becomes_raw_span() {
        let b = bundle(&[("text/html", json!("<b>x</b>")), ("text/plain", json!("x"))]);
        assert_eq!(render_inline(&b, HTML).unwrap(), "`<b>x</b>`{=html}");
    }

    #[test]
    fn html_array_value_is_joined() {
        let b = bundle(&[("text/html", json!(["<b>", "x</b>"]))]);
        assert_eq!(render_inline(&b, HTML).unwrap(), "`<b>x</b>`{=html}");
    }

    #[test]
    fn latex_becomes_raw_tex_span() {
        let b = bundle(&[("text/latex", json!("$x^2$"))]);
        assert_eq!(render_inline(&b, LATEX).unwrap(), "`$x^2$`{=tex}");
    }

    #[test]
    fn raw_span_with_backticks_cannot_break_out() {
        let b = bundle(&[("text/html", json!("a`b"))]);
        assert_eq!(render_inline(&b, HTML).unwrap(), "``a`b``{=html}");
        let b = bundle(&[("text/html", json!("`x"))]);
        assert_eq!(render_inline(&b, HTML).unwrap(), "`` `x ``{=html}");
        let b = bundle(&[("text/html", json!("a``b`"))]);
        assert_eq!(render_inline(&b, HTML).unwrap(), "``` a``b` ```{=html}");
    }

    #[test]
    fn quoted_plain_text_is_stripped_and_escaped() {
        let b = bundle(&[("text/plain", json!("'hello_world'"))]);
        assert_eq!(render_inline(&b, OTHER).unwrap(), "hello\\_world");
    }

    #[test]
    fn quoted_backslash_is_doubled_then_escaped() {
        // Q1: `'a\\b'` (one backslash in the value) becomes four backslashes.
        let b = bundle(&[("text/plain", json!("'a\\b'"))]);
        assert_eq!(render_inline(&b, OTHER).unwrap(), "a\\\\\\\\b");
    }

    #[test]
    fn unquoted_numbers_are_unchanged() {
        for n in ["2", "3.14", "None", "-1"] {
            let b = bundle(&[("text/plain", json!(n))]);
            assert_eq!(render_inline(&b, OTHER).unwrap(), n);
        }
    }

    #[test]
    fn text_with_backticks_in_a_quoted_value_is_escaped() {
        let b = bundle(&[("text/plain", json!("'a`b'"))]);
        assert_eq!(render_inline(&b, OTHER).unwrap(), "a\\`b");
    }

    #[test]
    fn empty_bundle_has_no_value() {
        assert_eq!(render_inline(&MimeBundle::new(), HTML), None);
    }

    #[test]
    fn image_only_bundle_has_no_value() {
        let b = bundle(&[("image/png", json!("AAAA"))]);
        assert_eq!(render_inline(&b, HTML), None);
    }

    #[test]
    fn image_with_plain_fallback_uses_plain() {
        let b = bundle(&[
            ("image/png", json!("AAAA")),
            ("text/plain", json!("<Figure size 640x480>")),
        ]);
        assert_eq!(render_inline(&b, OTHER).unwrap(), "<Figure size 640x480>");
    }

    #[test]
    fn quoted_empty_string_is_an_empty_result() {
        let b = bundle(&[("text/plain", json!("''"))]);
        assert_eq!(render_inline(&b, OTHER), Some(String::new()));
    }
}
