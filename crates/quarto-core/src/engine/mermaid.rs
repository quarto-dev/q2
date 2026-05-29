/*
 * engine/mermaid.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Mermaid diagram engine: rewrites `{mermaid}` code cells into the
 * `<pre class="mermaid">` markup that the mermaid.js browser runtime
 * picks up at page load.
 */

//! Mermaid diagram engine — direct HTML emission (B1).
//!
//! [`MermaidEngine`] is a pure-Rust, in-process [`ExecutionEngine`]
//! that rewrites Quarto's `{mermaid}` code cells into raw HTML the
//! browser-side `mermaid.js` runtime can pick up. It does **no** server-
//! side diagram rendering: no SVG generation, no PNG export, no
//! subprocess. The actual diagram drawing happens in the browser when
//! `mermaid.initialize({ startOnLoad: true })` runs.
//!
//! # What it does
//!
//! 1. Scans the input QMD for executable code cells whose info string
//!    is exactly `{mermaid}` — i.e. fences of the form
//!    ```` ```{mermaid} ````. (pampa keeps the braces in the class
//!    name, matching the `{r}` / `{python}` / `{fixture-…}` shape.)
//! 2. Rewrites each matched cell as a raw HTML block
//!    `<pre class="mermaid">…HTML-escaped source…</pre>`. The browser
//!    then reads the `textContent` of the element (entity-decoding the
//!    escapes) and hands it to `mermaid.render`.
//! 3. If at least one mermaid cell was found, appends a single
//!    `<script type="module">` block at the end of the output that
//!    imports `mermaid@11` from jsdelivr and calls `initialize({
//!    startOnLoad: true })`. The include is emitted in-band (inside
//!    `ExecuteResult.markdown`) rather than via `ExecuteResult.includes`
//!    on purpose — the in-band form survives the q2-preview capture-
//!    splice path verbatim (see `claude-notes/plans/2026-05-28-
//!    mermaidjs-engine-design.md`, Q-C → C1).
//!
//! Non-mermaid fenced blocks pass through untouched, and their interiors
//! are *not* scanned for nested `{mermaid}` strings — the scanner skips
//! over every fenced block, transforming only the ones whose info string
//! matches exactly.
//!
//! # Why text-level, not AST-level
//!
//! [`fixture::FixtureEngine`](super::FixtureEngine) is the established
//! precedent for pure-Rust in-process engines on the multi-engine branch:
//! a hand-rolled fence scanner over the QMD text. AST-level work would
//! require pulling pampa into the engine's dependency graph and
//! publicising `serialize_ast_to_qmd`; neither is needed here. The
//! mermaid engine returns QMD text containing raw HTML blocks, which
//! pampa's QMD reader parses as `RawBlock(HTML, …)` on the next stage's
//! reparse — exactly the round-trip [`EngineExecutionStage`] already
//! expects for multi-engine pipelines.
//!
//! # B1 is the format-locked first ship
//!
//! Emitting HTML directly from an engine couples mermaid to one output
//! format. The plan's Q-B → B1 decision accepts this on the explicit
//! understanding that:
//!
//! - Quarto 2 only renders HTML today, so the coupling is hypothetical
//!   cost against a future PDF/docx leg.
//! - The proper fix is bd-mqk49 ("engine -> stage extension"), which
//!   would let the mermaid engine declare a per-format AST pass on its
//!   output instead of emitting HTML inline.
//!
//! When bd-mqk49 lands, refactor: emit a marker `Div.mermaid` wrapping
//! the source as a code block, and declare an HTML-conditional pass that
//! turns the marker into the `<pre class="mermaid">` form below. See the
//! comment marker further down in this file at the HTML-emission site.

use super::context::{ExecuteResult, ExecutionContext};
use super::error::ExecutionError;
use super::traits::ExecutionEngine;

/// The once-per-document `<script type="module">` block that loads
/// mermaid from jsdelivr and triggers diagram rendering on page load.
///
/// Emitted at the end of the engine's output if (and only if) at least
/// one `{mermaid}` cell was matched. A document containing no mermaid
/// cells passes through without this include.
///
/// # Two structural quirks that this block deliberately handles
///
/// **1. Pandoc raw-HTML block wrapping (`` ```{=html} ``).** pampa's
/// QMD reader treats *bare* `<tag>` markup at block position as a
/// sequence of `RawInline` nodes and tries to parse the interior as
/// Markdown — which breaks on `startOnLoad: true` because the `:`
/// reads as a definition-list-like construct. The explicit
/// `` ```{=html} `` fence form tells the reader the contents are
/// opaque raw HTML to leave alone. The fence closes with matching
/// `` ``` `` on its own line.
///
/// **2. Explicit `mermaid.run()` instead of relying on
/// `startOnLoad: true`.** The auto-run-on-load path only fires when
/// `mermaid.initialize` is called *before* `DOMContentLoaded`. In
/// static `q2 render`, the script ships in the initial document and
/// that condition holds. But in `q2 preview` the script reaches the
/// iframe long after the document has loaded — the preview's React
/// renderer recreates the script element via
/// `document.createElement('script')` so it executes; see
/// [`RawBlock.tsx`](../../../../../ts-packages/preview-renderer/src/q2-preview/blocks/RawBlock.tsx)
/// — and `startOnLoad` silently no-ops at that point. Calling
/// `mermaid.run()` explicitly works regardless of when the script
/// executes and is idempotent on already-processed elements (via
/// mermaid's `data-processed` attribute), so it is safe in both
/// paths.
const MERMAID_SCRIPT_BLOCK: &str = "\
```{=html}
<script type=\"module\">
import mermaid from 'https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.esm.min.mjs';
mermaid.initialize({ startOnLoad: false });
mermaid.run({ querySelector: 'pre.mermaid' });
</script>
```";

/// Mermaid diagram engine — emits browser-runtime markup for `{mermaid}`
/// code cells. See module docs.
///
/// # Characteristics
///
/// - Always available (no external dependencies).
/// - Available in both native and WASM builds (no subprocess work).
/// - Does not support freeze/thaw (output is a deterministic function
///   of input — caching the trace via `engine: replay` would be silly).
/// - Produces no intermediate files.
#[derive(Debug, Clone, Default)]
pub struct MermaidEngine;

impl MermaidEngine {
    /// Create a new mermaid engine instance.
    pub fn new() -> Self {
        Self
    }
}

impl ExecutionEngine for MermaidEngine {
    fn name(&self) -> &str {
        "mermaidjs"
    }

    fn execute(
        &self,
        input: &str,
        _ctx: &ExecutionContext,
    ) -> Result<ExecuteResult, ExecutionError> {
        let output = render_mermaid_cells(input)
            .map_err(|msg| ExecutionError::execution_failed(self.name(), msg))?;
        Ok(ExecuteResult::new(output))
    }

    fn is_available(&self) -> bool {
        true
    }
}

/// Rewrite each ```` ```{mermaid} ```` executable cell in `input` as a
/// raw HTML `<pre class="mermaid">…</pre>` block. If any cell was
/// matched, append the once-per-doc jsdelivr `<script>` include at the
/// end of the output.
///
/// Errors (returned as a message, wrapped into `ExecutionError` by the
/// caller) on an unterminated `{mermaid}` cell. Non-matching fenced
/// blocks are passed through untouched, and their contents are *not*
/// scanned for nested `{mermaid}` strings — the scanner skips over every
/// fenced block, transforming only the ones whose info string is
/// exactly `{mermaid}`.
fn render_mermaid_cells(input: &str) -> Result<String, String> {
    // `split('\n')` is the exact inverse of `join("\n")`, so trailing
    // newlines round-trip precisely. Matches the FixtureEngine
    // convention.
    let lines: Vec<&str> = input.split('\n').collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut any_matched = false;

    let mut i = 0;
    while i < lines.len() {
        // Not a fence? Just emit the line.
        let Some((fence_len, info)) = parse_opening_fence(lines[i]) else {
            out.push(lines[i].to_string());
            i += 1;
            continue;
        };

        // Find the closing fence so we know the cell's bounds. We need
        // this for both matching cells (to splice over the whole range)
        // and non-matching cells (to skip over content that may contain
        // false `{mermaid}` lines).
        let mut j = i + 1;
        let mut closed = false;
        while j < lines.len() {
            if is_closing_fence(lines[j], fence_len) {
                closed = true;
                break;
            }
            j += 1;
        }

        let is_match = info.trim() == "{mermaid}";

        if !closed {
            if is_match {
                return Err(format!(
                    "mermaid engine: unterminated `{{mermaid}}` code cell opened at line {}",
                    i + 1
                ));
            }
            // A non-matching unterminated fence is malformed input that
            // is not ours to police; pass the remainder through verbatim
            // and stop.
            for line in &lines[i..] {
                out.push((*line).to_string());
            }
            break;
        }

        if !is_match {
            // Pass the whole fenced block (opening, content, closing)
            // through unchanged.
            for line in &lines[i..=j] {
                out.push((*line).to_string());
            }
            i = j + 1;
            continue;
        }

        // Matched. The source is the content between the fences.
        any_matched = true;
        let source = lines[i + 1..j].join("\n");
        // bd-mqk49: when engines can declare per-format AST passes,
        // route this through a format-conditional transform instead of
        // emitting HTML inline. Today Quarto 2 only renders HTML so the
        // format-locked emission is acceptable.
        //
        // The wrapping ```` ```{=html} ```` fence is required so pampa's
        // QMD reader treats the inner `<pre>` as a block-level raw HTML
        // element — bare `<pre>` at block position is converted to
        // RawInline and the interior is parsed as Markdown (which breaks
        // on `&gt;` and any other surprises). See MERMAID_SCRIPT_BLOCK
        // for the matching rationale on the script include.
        out.push(format!(
            "```{{=html}}\n<pre class=\"mermaid\">\n{}\n</pre>\n```",
            html_escape(&source)
        ));
        i = j + 1;
    }

    let mut result = out.join("\n");
    if any_matched {
        // Emit the once-per-doc script include as a top-level raw HTML
        // block. A blank-line separator ensures pampa parses it as a
        // raw block rather than appending it to a trailing paragraph.
        if !result.ends_with('\n') {
            result.push('\n');
        }
        result.push('\n');
        result.push_str(MERMAID_SCRIPT_BLOCK);
        result.push('\n');
    }
    Ok(result)
}

/// If `line` opens a backtick code fence (3+ leading backticks), return
/// the fence length and the info string following the backticks. Mirrors
/// [`fixture::parse_opening_fence`](super::fixture) — duplicated here so
/// the mermaid engine has no implicit dependency on the test-only
/// fixture engine.
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

/// HTML-escape the three characters that change the structure of the
/// emitted `<pre class="mermaid">…</pre>` block when present in source
/// text. The browser entity-decodes these when reading `textContent`,
/// so mermaid sees the original characters.
fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
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
    fn name_is_mermaidjs() {
        assert_eq!(MermaidEngine::new().name(), "mermaidjs");
    }

    #[test]
    fn always_available() {
        assert!(MermaidEngine::new().is_available());
    }

    #[test]
    fn single_cell_emits_pre_and_script() {
        let engine = MermaidEngine::new();
        let input = "Intro.\n\n```{mermaid}\ngraph TD\nA --> B\n```\n\nOutro.\n";
        let out = engine.execute(input, &ctx()).unwrap().markdown;
        assert!(
            out.contains("<pre class=\"mermaid\">\ngraph TD\nA --&gt; B\n</pre>"),
            "missing pre wrapper. got:\n{out}"
        );
        assert!(
            out.contains("mermaid.esm.min.mjs"),
            "missing script include. got:\n{out}"
        );
        // Prose around the cell survives intact.
        assert!(out.starts_with("Intro.\n\n"));
        assert!(out.contains("\n\nOutro.\n"));
    }

    #[test]
    fn multiple_cells_share_one_script() {
        let engine = MermaidEngine::new();
        let input = "```{mermaid}\nA\n```\n\nmid\n\n```{mermaid}\nB\n```\n";
        let out = engine.execute(input, &ctx()).unwrap().markdown;
        assert_eq!(
            out.matches("<pre class=\"mermaid\">").count(),
            2,
            "expected two pre wrappers"
        );
        assert_eq!(
            out.matches("mermaid.esm.min.mjs").count(),
            1,
            "expected exactly one script include (once-per-doc invariant)"
        );
    }

    #[test]
    fn no_cells_means_no_script() {
        let engine = MermaidEngine::new();
        let input = "# Just prose\n\nNo cells here.\n";
        let out = engine.execute(input, &ctx()).unwrap().markdown;
        assert_eq!(out, input);
        assert!(!out.contains("mermaid.esm.min.mjs"));
        assert!(!out.contains("<pre class=\"mermaid\">"));
    }

    #[test]
    fn other_engine_cells_pass_through() {
        // `{r}`, `{python}`, and plain display blocks are untouched.
        let engine = MermaidEngine::new();
        let input = "\
```{r}
x <- 1
```

```{python}
y = 2
```

```text
plain
```

```{mermaid}
graph TD
A --> B
```
";
        let out = engine.execute(input, &ctx()).unwrap().markdown;
        assert!(
            out.contains("```{r}\nx <- 1\n```"),
            "r cell mutated. got:\n{out}"
        );
        assert!(
            out.contains("```{python}\ny = 2\n```"),
            "python cell mutated. got:\n{out}"
        );
        assert!(
            out.contains("```text\nplain\n```"),
            "plain display block mutated. got:\n{out}"
        );
        assert!(out.contains("<pre class=\"mermaid\">"));
    }

    #[test]
    fn does_not_match_inside_other_fenced_blocks() {
        // A `{mermaid}` line that is the *content* of an outer fence
        // must not be treated as an opening cell fence.
        let engine = MermaidEngine::new();
        let input = "````\n```{mermaid}\ninside\n```\n````\n";
        let out = engine.execute(input, &ctx()).unwrap().markdown;
        // Zero matches → output unchanged and no script include.
        assert_eq!(out, input);
        assert!(!out.contains("<pre class=\"mermaid\">"));
        assert!(!out.contains("mermaid.esm.min.mjs"));
    }

    #[test]
    fn html_escapes_lt_gt_amp_in_source() {
        // Mermaid syntax rarely produces literal `<`, `>`, `&` but
        // defensive escaping keeps the wrapper structurally well-formed
        // regardless. The browser entity-decodes textContent so mermaid
        // sees the original characters.
        let engine = MermaidEngine::new();
        let input = "```{mermaid}\nA & B < C > D\n```\n";
        let out = engine.execute(input, &ctx()).unwrap().markdown;
        assert!(
            out.contains("A &amp; B &lt; C &gt; D"),
            "expected HTML-escaped source. got:\n{out}"
        );
        assert!(!out.contains("A & B < C > D"));
    }

    #[test]
    fn errors_on_unterminated_mermaid_cell() {
        let engine = MermaidEngine::new();
        let input = "```{mermaid}\ngraph TD\nA --> B\n";
        let err = engine.execute(input, &ctx()).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("unterminated"), "got: {msg}");
    }

    #[test]
    fn unterminated_non_mermaid_fence_is_passthrough() {
        // A non-matching unterminated fence is not ours to police —
        // we pass it through and stop scanning.
        let engine = MermaidEngine::new();
        let input = "```r\nnever closed\n";
        let out = engine.execute(input, &ctx()).unwrap().markdown;
        assert_eq!(out, input);
    }

    #[test]
    fn longer_fences_round_trip() {
        // A 4-backtick cell closes only on >= 4 backticks; an interior
        // 3-backtick line is content, not a close.
        let engine = MermaidEngine::new();
        let input = "````{mermaid}\n```\ngraph TD\n```\n````\n";
        let out = engine.execute(input, &ctx()).unwrap().markdown;
        // The interior 3-backtick lines must be part of the wrapped
        // source, not treated as closing the outer cell.
        assert!(
            out.contains("<pre class=\"mermaid\">\n```\ngraph TD\n```\n</pre>"),
            "longer fence not handled. got:\n{out}"
        );
        assert!(out.contains("mermaid.esm.min.mjs"));
    }

    #[test]
    fn script_block_is_pampa_raw_html_block() {
        // The structural property the engine relies on for pampa's
        // QMD reader to treat the script include as block-level raw
        // HTML: the script's ```{=html} opening fence appears on its
        // own line, preceded by a blank line that separates it from
        // the prior cell's content. Bare `<script>` at block position
        // gets converted to RawInline and pampa tries to parse the
        // interior as Markdown (which fails on `startOnLoad: true`).
        let engine = MermaidEngine::new();
        let input = "```{mermaid}\nA\n```\n";
        let out = engine.execute(input, &ctx()).unwrap().markdown;
        assert!(
            out.contains("\n\n```{=html}\n<script type=\"module\">"),
            "script not wrapped in a blank-line-preceded ```{{=html}} \
             raw-HTML fence. got:\n{out}"
        );
    }

    #[test]
    fn cell_is_pampa_raw_html_block() {
        // Same structural property for each mermaid cell: bare `<pre>`
        // at block position is converted to RawInline and the inside
        // is parsed as Markdown. Wrap in ```{=html} so the reader
        // treats the whole `<pre class="mermaid">…</pre>` as opaque
        // raw HTML.
        let engine = MermaidEngine::new();
        let input = "```{mermaid}\nA\n```\n";
        let out = engine.execute(input, &ctx()).unwrap().markdown;
        assert!(
            out.contains("```{=html}\n<pre class=\"mermaid\">"),
            "cell not wrapped in a ```{{=html}} raw-HTML fence. got:\n{out}"
        );
    }
}
