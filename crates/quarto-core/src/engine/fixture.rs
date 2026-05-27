/*
 * engine/fixture.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * File-backed test engine for exercising multi-engine sequencing.
 */

//! File-backed test engine (test-registry-only).
//!
//! [`FixtureEngine`] is a deterministic, dependency-free
//! [`ExecutionEngine`] used to exercise **sequential multi-engine
//! execution** (bd-5yff4) without R, Python, or Jupyter. It "executes"
//! code cells by splicing pre-recorded results in document order:
//!
//! 1. It scans the input QMD for executable code cells whose language
//!    token matches its [`name`](FixtureEngine::name) — i.e. fences of
//!    the form ```` ```{<name>} ````. (pampa keeps the braces inside the
//!    code block's class name, so an executable cell serializes back to
//!    this exact form; a plain ```` ```<name> ```` display block does
//!    *not* match.)
//! 2. For the i-th such cell (in document order) it replaces the entire
//!    fenced block with the i-th entry of its results list.
//!
//! Results come from either an in-memory list ([`with_results`], a
//! convenience for filesystem-free unit tests) or a JSON file named by
//! the engine config — `engine: { <name>: { results: <path> } }`, a JSON
//! array of strings, one per cell, in order ([`new`]). The file-backed
//! form is the design the multi-engine plan calls for; the in-memory form
//! keeps unit tests simple.
//!
//! Because a spliced result may itself contain a ```` ```{<other>} ````
//! cell, a `FixtureEngine` named `a` can hand work to a `FixtureEngine`
//! named `b` that runs later in the sequence — exactly the "engine N
//! produces cells for engine N+1" property multi-engine execution must
//! support.
//!
//! # Not for production
//!
//! This engine is **never** registered in the default
//! [`EngineRegistry`](super::EngineRegistry); tests register it
//! explicitly via [`EngineRegistry::register`]. Quarto 2's freeze feature
//! will reuse the execution *trace* (`engine: replay`), not this engine,
//! so it carries no production responsibility. It is gated to non-WASM
//! targets since it is a native test utility.
//!
//! [`new`]: FixtureEngine::new
//! [`with_results`]: FixtureEngine::with_results

use super::context::{ExecuteResult, ExecutionContext};
use super::error::ExecutionError;
use super::traits::ExecutionEngine;

/// A deterministic, file-backed test engine. See module docs.
#[derive(Debug, Clone)]
pub struct FixtureEngine {
    /// Engine name. Matches both the `engine:` declaration and the cell
    /// language token (```` ```{<name>} ````).
    name: String,
    /// In-memory results, used when `Some`. When `None`, results are read
    /// from the JSON file named by the engine config `results` key.
    results: Option<Vec<String>>,
}

impl FixtureEngine {
    /// Create a file-backed engine. Results are read at `execute` time
    /// from the JSON file named by the engine config `results` key,
    /// resolved relative to the execution `cwd`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            results: None,
        }
    }

    /// Create an engine with in-memory results (filesystem-free; for unit
    /// tests). Each entry replaces one matching cell, in document order.
    pub fn with_results(name: impl Into<String>, results: Vec<String>) -> Self {
        Self {
            name: name.into(),
            results: Some(results),
        }
    }

    /// Resolve the ordered results list: in-memory if present, else from
    /// the JSON file named by the engine config `results` key. Absent
    /// config (or no `results` key) yields an empty list, which the
    /// cell/result count check in [`splice_cells`] treats as "no cells
    /// expected".
    fn resolve_results(&self, ctx: &ExecutionContext) -> Result<Vec<String>, ExecutionError> {
        if let Some(results) = &self.results {
            return Ok(results.clone());
        }
        let Some(config) = &ctx.engine_config else {
            return Ok(Vec::new());
        };
        let Some(results_val) = config.get("results") else {
            return Ok(Vec::new());
        };
        let Some(path_str) = results_val.as_str() else {
            return Err(ExecutionError::execution_failed(
                &self.name,
                "engine config `results` must be a string path to a JSON results file",
            ));
        };
        let path = ctx.cwd.join(path_str);
        let bytes = std::fs::read(&path).map_err(|e| {
            ExecutionError::execution_failed(
                &self.name,
                format!("failed to read results file {}: {e}", path.display()),
            )
        })?;
        let parsed: Vec<String> = serde_json::from_slice(&bytes).map_err(|e| {
            ExecutionError::execution_failed(
                &self.name,
                format!(
                    "results file {} must be a JSON array of strings: {e}",
                    path.display()
                ),
            )
        })?;
        Ok(parsed)
    }
}

impl ExecutionEngine for FixtureEngine {
    fn name(&self) -> &str {
        &self.name
    }

    fn execute(
        &self,
        input: &str,
        ctx: &ExecutionContext,
    ) -> Result<ExecuteResult, ExecutionError> {
        let results = self.resolve_results(ctx)?;
        let output = splice_cells(input, &self.name, &results)
            .map_err(|msg| ExecutionError::execution_failed(&self.name, msg))?;
        Ok(ExecuteResult::new(output))
    }

    fn is_available(&self) -> bool {
        true
    }
}

/// Replace each ```` ```{name} ```` executable cell in `input` with the
/// corresponding entry of `results`, in document order.
///
/// Errors (returned as a message, wrapped into `ExecutionError` by the
/// caller) on:
/// - an unterminated `{name}` cell,
/// - a count mismatch between matching cells and results (missing or
///   surplus results).
///
/// Non-matching fenced blocks (other engines' cells, plain display
/// blocks, fenced divs) are passed through untouched, and their contents
/// are *not* scanned for false `{name}` matches — the scanner skips over
/// every fenced block, splicing only the ones whose info string is
/// exactly `{name}`.
fn splice_cells(input: &str, name: &str, results: &[String]) -> Result<String, String> {
    // `split('\n')` is the exact inverse of `join("\n")`, so the trailing
    // newline (and any blank trailing line) round-trips precisely.
    let lines: Vec<&str> = input.split('\n').collect();
    let want_info = format!("{{{name}}}");

    // Pass 1: locate every matching cell as a (start, end_inclusive) line
    // range, skipping the content of all fenced blocks so a `{name}`
    // string inside another block is not mistaken for a cell.
    let mut cells: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let Some((fence_len, info)) = parse_opening_fence(lines[i]) else {
            i += 1;
            continue;
        };
        let is_match = info.trim() == want_info;
        // Find the closing fence (>= fence_len backticks, nothing else).
        let mut j = i + 1;
        let mut closed = false;
        while j < lines.len() {
            if is_closing_fence(lines[j], fence_len) {
                closed = true;
                break;
            }
            j += 1;
        }
        if !closed {
            if is_match {
                return Err(format!(
                    "fixture engine '{name}': unterminated code cell opened at line {}",
                    i + 1
                ));
            }
            // A non-matching unterminated fence is malformed input that is
            // not ours to police; treat the remainder as block content.
            break;
        }
        if is_match {
            cells.push((i, j));
        }
        i = j + 1;
    }

    if cells.len() != results.len() {
        return Err(format!(
            "fixture engine '{name}': document has {} executable cell(s) but {} result(s) were provided",
            cells.len(),
            results.len()
        ));
    }

    // Pass 2: rebuild, replacing each cell's line range with its result.
    let mut out: Vec<&str> = Vec::with_capacity(lines.len());
    let mut prev_end = 0; // first not-yet-emitted line index
    let mut result_lines_storage: Vec<Vec<&str>> = Vec::with_capacity(cells.len());
    for result in results {
        result_lines_storage.push(result.split('\n').collect());
    }
    for (k, (start, end)) in cells.iter().enumerate() {
        out.extend_from_slice(&lines[prev_end..*start]);
        out.extend_from_slice(&result_lines_storage[k]);
        prev_end = end + 1;
    }
    out.extend_from_slice(&lines[prev_end..]);
    Ok(out.join("\n"))
}

/// If `line` opens a backtick code fence (3+ leading backticks), return
/// the fence length and the info string following the backticks.
fn parse_opening_fence(line: &str) -> Option<(usize, &str)> {
    let n = line.bytes().take_while(|&b| b == b'`').count();
    if n < 3 {
        return None;
    }
    Some((n, &line[n..]))
}

/// True if `line` is a closing fence for an opening fence of `fence_len`
/// backticks: at least `fence_len` backticks and nothing else (trailing
/// whitespace allowed; leading whitespace is not, since a space is not a
/// backtick).
fn is_closing_fence(line: &str, fence_len: usize) -> bool {
    let trimmed = line.trim_end();
    trimmed.len() >= fence_len && !trimmed.is_empty() && trimmed.bytes().all(|b| b == b'`')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn ctx() -> ExecutionContext {
        ExecutionContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/project"),
            PathBuf::from("/project/doc.qmd"),
            "html",
        )
    }

    #[test]
    fn name_is_configurable() {
        assert_eq!(FixtureEngine::new("fixture-a").name(), "fixture-a");
        assert_eq!(
            FixtureEngine::with_results("fixture-b", vec![]).name(),
            "fixture-b"
        );
    }

    #[test]
    fn always_available() {
        assert!(FixtureEngine::new("fixture-a").is_available());
    }

    #[test]
    fn splices_single_cell() {
        let engine = FixtureEngine::with_results("fixture-a", vec!["**Result:** 2".to_string()]);
        let input = "Intro.\n\n```{fixture-a}\n1 + 1\n```\n\nOutro.\n";
        let out = engine.execute(input, &ctx()).unwrap().markdown;
        assert_eq!(out, "Intro.\n\n**Result:** 2\n\nOutro.\n");
    }

    #[test]
    fn splices_multiple_cells_in_order() {
        let engine =
            FixtureEngine::with_results("fixture-a", vec!["one".to_string(), "two".to_string()]);
        let input = "```{fixture-a}\na\n```\n\nmid\n\n```{fixture-a}\nb\n```\n";
        let out = engine.execute(input, &ctx()).unwrap().markdown;
        assert_eq!(out, "one\n\nmid\n\ntwo\n");
    }

    #[test]
    fn ignores_other_engine_cells_and_display_blocks() {
        // Only `{fixture-a}` cells are matched. `{fixture-b}` and the
        // plain display block `fixture-a` (no braces) are untouched.
        let engine = FixtureEngine::with_results("fixture-a", vec!["A!".to_string()]);
        let input = "```{fixture-a}\nx\n```\n\n```{fixture-b}\ny\n```\n\n```fixture-a\nz\n```\n";
        let out = engine.execute(input, &ctx()).unwrap().markdown;
        assert_eq!(
            out,
            "A!\n\n```{fixture-b}\ny\n```\n\n```fixture-a\nz\n```\n"
        );
    }

    #[test]
    fn does_not_match_inside_other_fenced_blocks() {
        // A `{fixture-a}` line that is the *content* of a plain ``` block
        // must not be treated as an opening cell fence.
        let engine = FixtureEngine::with_results("fixture-a", vec![]);
        let input = "```\n```{fixture-a}\n```\n";
        // Zero matching cells, zero results → passthrough unchanged.
        let out = engine.execute(input, &ctx()).unwrap().markdown;
        assert_eq!(out, input);
    }

    #[test]
    fn handoff_result_can_introduce_next_engines_cell() {
        // fixture-a's result contains a `{fixture-b}` cell — proves an
        // engine can emit a cell for a later engine in the sequence.
        let engine = FixtureEngine::with_results(
            "fixture-a",
            vec!["```{fixture-b}\nfrom-a\n```".to_string()],
        );
        let input = "```{fixture-a}\nseed\n```\n";
        let out = engine.execute(input, &ctx()).unwrap().markdown;
        assert_eq!(out, "```{fixture-b}\nfrom-a\n```\n");
    }

    #[test]
    fn errors_on_too_few_results() {
        let engine = FixtureEngine::with_results("fixture-a", vec!["only-one".to_string()]);
        let input = "```{fixture-a}\na\n```\n\n```{fixture-a}\nb\n```\n";
        let err = engine.execute(input, &ctx()).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("2 executable cell(s) but 1 result(s)"),
            "got: {msg}"
        );
    }

    #[test]
    fn errors_on_surplus_results() {
        let engine =
            FixtureEngine::with_results("fixture-a", vec!["one".to_string(), "two".to_string()]);
        let input = "```{fixture-a}\na\n```\n";
        let err = engine.execute(input, &ctx()).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("1 executable cell(s) but 2 result(s)"),
            "got: {msg}"
        );
    }

    #[test]
    fn errors_on_unterminated_cell() {
        let engine = FixtureEngine::with_results("fixture-a", vec!["x".to_string()]);
        let input = "```{fixture-a}\nnever closed\n";
        let err = engine.execute(input, &ctx()).unwrap_err();
        assert!(format!("{err}").contains("unterminated"));
    }

    #[test]
    fn longer_fences_round_trip() {
        // A 4-backtick cell closes only on >= 4 backticks; an interior
        // 3-backtick line is content, not a close.
        let engine = FixtureEngine::with_results("fixture-a", vec!["spliced".to_string()]);
        let input = "````{fixture-a}\n```\ninner\n```\n````\n";
        let out = engine.execute(input, &ctx()).unwrap().markdown;
        assert_eq!(out, "spliced\n");
    }

    #[test]
    fn no_cells_no_results_is_passthrough() {
        let engine = FixtureEngine::with_results("fixture-a", vec![]);
        let input = "# Just prose\n\nNo cells here.\n";
        let out = engine.execute(input, &ctx()).unwrap().markdown;
        assert_eq!(out, input);
    }

    #[test]
    fn file_backed_reads_results_from_engine_config() {
        use quarto_pandoc_types::ConfigValue;
        use quarto_pandoc_types::config_value::ConfigMapEntry;
        use quarto_source_map::SourceInfo;

        let dir = tempfile::tempdir().unwrap();
        let results_path = dir.path().join("results.json");
        std::fs::write(&results_path, r#"["**from file**"]"#).unwrap();

        let engine_config = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "results".to_string(),
                key_source: SourceInfo::default(),
                value: ConfigValue::new_string("results.json", SourceInfo::default()),
            }],
            SourceInfo::default(),
        );

        let exec_ctx = ExecutionContext::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            dir.path().join("doc.qmd"),
            "html",
        )
        .with_engine_config(Some(engine_config));

        let engine = FixtureEngine::new("fixture-a");
        let input = "```{fixture-a}\nseed\n```\n";
        let out = engine.execute(input, &exec_ctx).unwrap().markdown;
        assert_eq!(out, "**from file**\n");
    }

    #[test]
    fn file_backed_reports_unreadable_file() {
        use quarto_pandoc_types::ConfigValue;
        use quarto_pandoc_types::config_value::ConfigMapEntry;
        use quarto_source_map::SourceInfo;

        let engine_config = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "results".to_string(),
                key_source: SourceInfo::default(),
                value: ConfigValue::new_string("does-not-exist.json", SourceInfo::default()),
            }],
            SourceInfo::default(),
        );
        let exec_ctx = ExecutionContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/nonexistent-dir-xyz"),
            PathBuf::from("/tmp/doc.qmd"),
            "html",
        )
        .with_engine_config(Some(engine_config));

        let engine = FixtureEngine::new("fixture-a");
        let err = engine
            .execute("```{fixture-a}\nx\n```\n", &exec_ctx)
            .unwrap_err();
        assert!(format!("{err}").contains("failed to read results file"));
    }

    #[test]
    fn is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<FixtureEngine>();
    }
}
