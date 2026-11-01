/*
 * engine/jupyter/inline.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * EXR-3: how a span's code is sent to a kernel and how its result is read.
 */

//! Run an inline expression through a kernel's ordinary execute path and read its
//! value back (EXR-3 design decisions 1 and 2, divergences 1 and 2).
//!
//! A span runs as a one-line cell, so a kernel without `user_expressions` works. One
//! thing cell mode cannot say is *why* a silent execution was silent: IPython sends no
//! `execute_result` both for the statement `x = 1` and for an expression whose value is
//! `None`, and Q1 treats the two differently (statement: error; `None`: the text
//! `None`). The decision, pinned by the tests below:
//!
//! - **Python kernels** (kernelspec language `python`) get a wrapper with Q1's
//!   semantics, [`wrap_inline_code`]: the source is `eval`ed (a statement raises
//!   `SyntaxError`, which is fatal like Q1) and a `None` value is turned into the text
//!   `None`. Multi-statement bodies (`a; b`) therefore fail as they do in Q1.
//! - **Every other kernel** runs the span as written. A silent execution is
//!   [`InlineOutcome::Silent`], which the caller reports as "produced no value", fatal
//!   (divergence 1). On such a kernel `None`-like values that print nothing cannot be
//!   told from a statement; kernels that print `NULL` or similar are unaffected.
//!
//! `user_expressions` was considered and not used: Ark does not implement it and
//! Positron's supervisor hard-codes `{}`.

use crate::presenter::output::{CellOutput, CellResult, ExecuteStatus, MimeBundle};

/// The text mime types a span's value can use.
const TEXT_MIMES: [&str; 4] = ["text/plain", "text/html", "text/latex", "text/markdown"];

/// The code to send for a span `expr` on a kernel whose language is `language`.
pub(crate) fn wrap_inline_code(language: &str, expr: &str) -> String {
    if language.eq_ignore_ascii_case("python") {
        // JSON string escapes are valid Python string-literal escapes, and
        // serde_json leaves non-ASCII text alone (no surrogate pairs).
        let literal = serde_json::to_string(expr).expect("a str serializes");
        // The inner `eval` runs in the cell's globals, so the span sees earlier cells.
        // `'None'` displays as `'None'`, which the quoted-value rule turns into `None`.
        format!(
            "(lambda __quarto_v: 'None' if __quarto_v is None else __quarto_v)\
             (eval(compile({literal}, '<inline expression>', 'eval')))"
        )
    } else {
        expr.to_string()
    }
}

/// Undo the YAML double-quote escapes the qmd writer applied to a front matter value
/// (`\\`, `\"`, `\n`), so the kernel gets the expression as the author wrote it
/// (EXR-3 divergence 10). Any other backslash sequence is left alone.
pub(crate) fn unescape_yaml_double(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.clone().next() {
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('n') => out.push('\n'),
            _ => {
                out.push('\\');
                continue;
            }
        }
        chars.next();
    }
    out
}

/// Escape `s` for the inside of a YAML double-quoted scalar. A newline is not handled:
/// the caller rejects it first (it would fold to a space).
pub(crate) fn escape_yaml_double(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// The front matter key on the line holding byte `offset` of `text` (`title` for
/// `title: "a `{python} x`"`), or `None` when the line has no `key:` prefix.
pub(crate) fn front_matter_key_at(text: &str, offset: usize) -> Option<String> {
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

/// Whether the jupyter engine runs a span in `lang`: the language is in the engine's
/// owned set (the stage computed it from the document), or it is the language of the
/// kernel the document selects (so a span-only document with `jupyter: python3` owns its
/// `{python}` spans). Case-insensitive.
pub(crate) fn span_is_owned(
    lang: &str,
    owned_languages: &[String],
    kernel_language: Option<&str>,
) -> bool {
    owned_languages.iter().any(|o| o.eq_ignore_ascii_case(lang))
        || kernel_language.is_some_and(|k| k.eq_ignore_ascii_case(lang))
}

/// What a span's execution amounted to.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum InlineOutcome {
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
pub(crate) fn classify_inline_result(result: &CellResult) -> InlineOutcome {
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

    #[test]
    fn yaml_unescape_reverses_the_writer_escapes() {
        assert_eq!(unescape_yaml_double(r#"f(\"a\")"#), r#"f("a")"#);
        assert_eq!(unescape_yaml_double(r"a\\b"), r"a\b");
        assert_eq!(unescape_yaml_double(r"a\nb"), "a\nb");
        // Unknown escapes and a trailing backslash are kept.
        assert_eq!(unescape_yaml_double(r"a\*b\"), r"a\*b\");
    }

    #[test]
    fn yaml_escape_round_trips_with_unescape() {
        for s in [r#"a"b"#, r"a\b", r"a\*b", r#"\""#, "plain"] {
            assert_eq!(unescape_yaml_double(&escape_yaml_double(s)), s);
        }
        assert_eq!(escape_yaml_double(r#"a\*"b"#), r#"a\\*\"b"#);
    }

    #[test]
    fn front_matter_key_is_read_from_the_span_line() {
        let text = "---\ntitle: \"T `{python} 1` x\"\nsub:\n  - a: \"`{python} 2`\"\n---\n";
        let at = text.find("`{python} 1`").unwrap();
        assert_eq!(front_matter_key_at(text, at).as_deref(), Some("title"));
        let at = text.find("`{python} 2`").unwrap();
        assert_eq!(front_matter_key_at(text, at).as_deref(), Some("a"));
        assert_eq!(front_matter_key_at("no colon `x`", 9), None);
    }

    #[test]
    fn span_ownership_is_by_owned_set_or_kernel_language() {
        let owned = vec!["r".to_string()];
        assert!(span_is_owned("R", &owned, None));
        assert!(span_is_owned("Python", &[], Some("python")));
        assert!(!span_is_owned("r", &[], Some("python")));
        assert!(!span_is_owned("python", &[], None));
    }
    use serde_json::json;

    fn bundle(pairs: &[(&str, serde_json::Value)]) -> MimeBundle {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    fn result(outputs: Vec<CellOutput>) -> CellResult {
        CellResult {
            status: ExecuteStatus::Ok,
            outputs,
            execution_count: None,
        }
    }

    fn exec_result(b: MimeBundle) -> CellOutput {
        CellOutput::ExecuteResult {
            execution_count: 1,
            data: b,
            metadata: json!({}),
        }
    }

    fn display(b: MimeBundle) -> CellOutput {
        CellOutput::DisplayData {
            data: b,
            metadata: json!({}),
        }
    }

    fn stdout(text: &str) -> CellOutput {
        CellOutput::Stream {
            name: "stdout".into(),
            text: text.into(),
        }
    }

    // ── wrap_inline_code ─────────────────────────────────────────────────

    #[test]
    fn python_span_is_wrapped_in_an_eval_with_the_source_as_a_literal() {
        let code = wrap_inline_code("python", "f(\"a\") + 1");
        assert!(code.contains("eval(compile(\"f(\\\"a\\\") + 1\""), "{code}");
        assert!(code.contains("'None' if __quarto_v is None"), "{code}");
    }

    #[test]
    fn python_language_match_ignores_case() {
        assert_ne!(wrap_inline_code("Python", "1"), "1");
    }

    #[test]
    fn other_languages_are_sent_as_written() {
        assert_eq!(wrap_inline_code("R", "1 + 1"), "1 + 1");
        assert_eq!(wrap_inline_code("julia", "x"), "x");
    }

    #[test]
    fn newlines_and_unicode_survive_the_literal() {
        let code = wrap_inline_code("python", "(1,\n 2)  # é");
        assert!(code.contains("\\n"), "{code}");
        assert!(code.contains('é'), "{code}");
    }

    // ── classify_inline_result ───────────────────────────────────────────

    #[test]
    fn the_execute_result_is_the_value() {
        let b = bundle(&[("text/plain", json!("2"))]);
        let r = result(vec![exec_result(b.clone())]);
        assert_eq!(classify_inline_result(&r), InlineOutcome::Value(b));
    }

    #[test]
    fn streams_are_dropped_and_the_value_kept() {
        let b = bundle(&[("text/plain", json!("2"))]);
        let r = result(vec![stdout("noise\n"), exec_result(b.clone())]);
        assert_eq!(classify_inline_result(&r), InlineOutcome::Value(b));
    }

    #[test]
    fn the_last_text_output_wins_and_display_data_counts() {
        let first = bundle(&[("text/plain", json!("1"))]);
        let last = bundle(&[("text/markdown", json!("**2**"))]);
        let r = result(vec![exec_result(first), display(last.clone())]);
        assert_eq!(classify_inline_result(&r), InlineOutcome::Value(last));
    }

    #[test]
    fn an_error_wins_over_a_value() {
        let r = result(vec![
            exec_result(bundle(&[("text/plain", json!("2"))])),
            CellOutput::Error {
                ename: "NameError".into(),
                evalue: "x".into(),
                traceback: vec![],
            },
        ]);
        assert_eq!(
            classify_inline_result(&r),
            InlineOutcome::Raised {
                ename: "NameError".into(),
                evalue: "x".into()
            }
        );
    }

    #[test]
    fn an_error_status_alone_is_raised() {
        let r = CellResult {
            status: ExecuteStatus::Error {
                ename: "E".into(),
                evalue: "v".into(),
                traceback: vec![],
            },
            outputs: vec![],
            execution_count: None,
        };
        assert!(matches!(
            classify_inline_result(&r),
            InlineOutcome::Raised { .. }
        ));
    }

    #[test]
    fn no_outputs_at_all_is_silent() {
        assert_eq!(
            classify_inline_result(&result(vec![])),
            InlineOutcome::Silent
        );
    }

    #[test]
    fn a_stream_only_execution_is_not_silent_but_has_no_text_value() {
        let r = result(vec![stdout("printed\n")]);
        assert_eq!(classify_inline_result(&r), InlineOutcome::NoTextValue);
    }

    #[test]
    fn an_image_only_value_has_no_text_value() {
        let r = result(vec![exec_result(bundle(&[("image/png", json!("AAAA"))]))]);
        assert_eq!(classify_inline_result(&r), InlineOutcome::NoTextValue);
    }
}
