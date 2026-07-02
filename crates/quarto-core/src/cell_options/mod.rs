/*
 * cell_options/mod.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Shared cell-options facility: identify and extract the YAML options
 * block at the head of an executable code cell (bd-ohvl879u).
 */

//! Cell-option partitioning for executable code cells.
//!
//! Quarto cells carry per-cell options as YAML written in the cell
//! language's own comment syntax, one option-marker per line at the
//! top of the cell: `#|` for python/R, `--|` for lua/sql, `//|` for
//! js/rust, `%|` for matlab, and block-comment forms like
//! `/*| … */` for C. This module is q2's **single** implementation of
//! detecting that block, splitting a cell body into options + code,
//! and parsing the options with real source attribution — Quarto 1
//! grew several divergent copies of this logic (partition-cell-options.ts,
//! jupyter.ts, notebook.py, constants.lua), and q2 itself had two
//! partial ad-hoc ones before this module (the crossref code-block
//! shorthand's string matcher and the LSP's highlight-only
//! `directive_tokens`); consumers should migrate here.
//!
//! ## Shape of the options block
//!
//! Mirroring Q1's `partition-cell-options.ts`:
//! - An option line is `<prefix><spaces/tabs>|<one optional space><content>`,
//!   anchored at column 0 (indented markers are *not* option lines).
//! - For block-comment languages the line must also end with the
//!   suffix (after trailing whitespace), which is stripped.
//! - Only the **leading run** of option lines counts; the first
//!   non-matching line starts the code.
//!
//! ## Source attribution
//!
//! The option lines' YAML content is non-contiguous in the cell body
//! (markers elided), so the reassembled YAML document's provenance is
//! a [`SourceInfo::concat`] of per-line substrings of the caller's
//! `body_source` — for prefix-only languages every byte of the
//! reassembled string (including the newlines) is a real source byte,
//! so `map_offset` on any parsed node resolves exactly. The YAML is
//! parsed with [`quarto_yaml::parse_with_parent`], so every node's
//! span composes back through the concat automatically.
//!
//! The facility is config-agnostic: it returns [`YamlWithSourceInfo`].
//! Consumers that want document-scoped semantics convert via
//! [`options_to_config`] and merge with [`merge_cell_over_scope`]
//! (cell options over a document-level scope, cell wins) — the
//! beginning of scoped resolution of document metadata in cell
//! options, which Q1 never had.

use quarto_config::MergedConfig;
use quarto_pandoc_types::config_value::{ConfigValue, InterpretationContext};
use quarto_source_map::SourceInfo;
use quarto_yaml::YamlWithSourceInfo;

/// Comment syntax for a language's cell-option lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommentSyntax {
    /// Line-comment (or block-comment opener) characters, e.g. `#`,
    /// `//`, `--`, `/*`.
    pub prefix: &'static str,
    /// Block-comment closer for languages with no line comments,
    /// e.g. `*/` for C/CSS, `*)` for OCaml, `;` for SAS.
    pub suffix: Option<&'static str>,
}

/// Look up the comment syntax for `language` (case-insensitive).
/// Unknown languages default to `#`, matching Q1.
///
/// Ported from Q1's `kLangCommentChars`
/// (`quarto-cli/src/core/lib/partition-cell-options.ts`). When q2
/// grows user-extensible engines this becomes a registration surface
/// (Q1's `addLanguageComment`); today it is a static table.
pub fn comment_syntax_for(language: &str) -> CommentSyntax {
    let lang = language.to_lowercase();
    let (prefix, suffix): (&'static str, Option<&'static str>) = match lang.as_str() {
        // Q1 table order preserved for diffability against
        // kLangCommentChars; the default arm covers r/python/julia/
        // powershell/bash/stan/octave/awk/gawk/sed/perl/prql/ruby/
        // coffee and every unknown language.
        "scala" | "csharp" | "fsharp" | "cpp" | "cc" | "java" | "groovy" | "kotlin" | "js"
        | "d3" | "node" | "sass" | "scss" | "go" | "asy" | "dot" | "ojs" | "rust" => ("//", None),
        "matlab" | "tikz" => ("%", None),
        "c" | "css" => ("/*", Some("*/")),
        "sas" => ("*", Some(";")),
        "sql" | "mysql" | "psql" | "lua" | "haskell" => ("--", None),
        "fortran" | "fortran95" => ("!", None),
        "stata" => ("*", None),
        "apl" => ("⍝", None),
        "ocaml" => ("(*", Some("*)")),
        "q" => ("/", None),
        _ => ("#", None),
    };
    CommentSyntax { prefix, suffix }
}

/// Result of partitioning a cell body into options + code.
#[derive(Debug)]
pub struct PartitionedCell {
    /// Parsed options. `None` when the cell has no option lines (or
    /// the option lines are blank).
    pub options: Option<YamlWithSourceInfo>,
    /// The cell body with the option lines removed — what should be
    /// executed and echoed.
    pub code: String,
    /// Provenance of `code`: a substring of the caller's `body_source`.
    pub code_source: SourceInfo,
    /// Byte length of the option-line run at the head of the body
    /// (0 when there are no option lines).
    pub options_len: usize,
}

/// Failure to interpret a cell's option lines.
#[derive(Debug)]
pub enum CellOptionsError {
    /// The option lines were detected but are not valid YAML. The
    /// wrapped error's location (when present) maps into the caller's
    /// `body_source` via the concat parent.
    InvalidYaml(quarto_yaml::Error),
}

impl std::fmt::Display for CellOptionsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CellOptionsError::InvalidYaml(e) => {
                write!(f, "cell options are not valid YAML: {}", e)
            }
        }
    }
}

impl std::error::Error for CellOptionsError {}

impl CellOptionsError {
    /// The most specific source location the failure carries (maps
    /// into the caller's `body_source` via the concat parent).
    pub fn location(&self) -> Option<&SourceInfo> {
        match self {
            CellOptionsError::InvalidYaml(e) => match e {
                quarto_yaml::Error::ParseError { location, .. }
                | quarto_yaml::Error::UnexpectedEof { location }
                | quarto_yaml::Error::InvalidStructure { location, .. } => location.as_ref(),
            },
        }
    }
}

/// Partition `body` (a code cell's text) into its leading option
/// lines and the remaining code, per `language`'s comment syntax.
///
/// `body_source` is the provenance of `body`; the parsed options'
/// node spans and `code_source` are derived from it. Callers with no
/// better anchor can register `body` as an ephemeral file in a
/// [`quarto_source_map::SourceContext`] and pass an `Original` span.
pub fn partition_cell_options(
    language: &str,
    body: &str,
    body_source: SourceInfo,
) -> Result<PartitionedCell, CellOptionsError> {
    let syntax = comment_syntax_for(language);

    // Scan the leading run of option lines, collecting the byte
    // ranges (within `body`) of each line's YAML content. For
    // prefix-only languages a range runs to the end of the line
    // *including* its newline, so the reassembled YAML document is a
    // pure concatenation of real source bytes. For suffix languages
    // the suffix is elided, so the line's newline becomes its own
    // piece (still a real source byte).
    let mut pieces: Vec<(usize, usize)> = Vec::new();
    let mut run_end = 0usize;
    let mut offset = 0usize;
    while offset < body.len() {
        let line_end = body[offset..]
            .find('\n')
            .map_or(body.len(), |i| offset + i + 1);
        let line = &body[offset..line_end];
        let Some(ranges) = option_content_ranges(line, &syntax) else {
            break;
        };
        for r in ranges {
            pieces.push((offset + r.start, offset + r.end));
        }
        run_end = line_end;
        offset = line_end;
    }

    let code = body[run_end..].to_string();
    let code_source = SourceInfo::substring(body_source.clone(), run_end, body.len());

    if pieces.is_empty() {
        return Ok(PartitionedCell {
            options: None,
            code,
            code_source,
            options_len: 0,
        });
    }

    let mut yaml_text = String::new();
    let mut concat_pieces: Vec<(SourceInfo, usize)> = Vec::new();
    for (start, end) in &pieces {
        yaml_text.push_str(&body[*start..*end]);
        concat_pieces.push((
            SourceInfo::substring(body_source.clone(), *start, *end),
            end - start,
        ));
    }

    // A run of blank option lines (`#|` with no content) is not an
    // options document.
    if yaml_text.trim().is_empty() {
        return Ok(PartitionedCell {
            options: None,
            code,
            code_source,
            options_len: run_end,
        });
    }

    let yaml_parent = SourceInfo::concat(concat_pieces);
    let options = quarto_yaml::parse_with_parent(&yaml_text, yaml_parent)
        .map_err(CellOptionsError::InvalidYaml)?;

    Ok(PartitionedCell {
        options: Some(options),
        code,
        code_source,
        options_len: run_end,
    })
}

/// Match one line against the option-line shape
/// `<prefix><spaces/tabs>|<one optional space><content>` (anchored at
/// column 0), returning the line-relative byte ranges of the YAML
/// content — one range for prefix-only languages (content through the
/// line's newline), or content + newline ranges for suffix languages
/// (whose suffix must terminate the line and is elided). `None` means
/// the line is not an option line.
fn option_content_ranges(
    line: &str,
    syntax: &CommentSyntax,
) -> Option<Vec<std::ops::Range<usize>>> {
    let rest = line.strip_prefix(syntax.prefix)?;
    let after_ws = rest.trim_start_matches([' ', '\t']);
    let ws_len = rest.len() - after_ws.len();
    let after_pipe = after_ws.strip_prefix('|')?;
    let content_start = syntax.prefix.len() + ws_len + 1 + usize::from(after_pipe.starts_with(' '));

    let mut ranges = Vec::with_capacity(2);
    match syntax.suffix {
        None => {
            ranges.push(content_start..line.len());
            Some(ranges)
        }
        Some(suffix) => {
            // The line (sans newline and trailing whitespace) must end
            // with the suffix; content is what precedes it, trimmed.
            let no_newline = line.trim_end_matches(['\n', '\r']);
            let before_suffix = no_newline.trim_end().strip_suffix(suffix)?;
            let content_end = before_suffix.trim_end().len().max(content_start);

            ranges.push(content_start..content_end);
            // The elided suffix means the line's newline is not
            // adjacent to the content — carry it as its own piece so
            // option lines stay separate YAML lines and every
            // reassembled byte remains a real source byte.
            let newline_len = line.len() - no_newline.len();
            if newline_len > 0 {
                ranges.push(no_newline.len()..line.len());
            }
            Some(ranges)
        }
    }
}

/// Convert parsed cell options to a [`ConfigValue`] using
/// document-metadata interpretation (markdown-bearing strings become
/// `PandocInlines`, exactly like front matter). Conversion
/// diagnostics (e.g. unknown tags) are returned alongside.
pub fn options_to_config(
    options: YamlWithSourceInfo,
) -> (ConfigValue, Vec<quarto_error_reporting::DiagnosticMessage>) {
    let mut collector = pampa::utils::diagnostic_collector::DiagnosticCollector::new();
    let config = pampa::pandoc::meta::yaml_to_config_value(
        options,
        InterpretationContext::DocumentMetadata,
        &mut collector,
    );
    (config, collector.into_diagnostics())
}

/// Merge cell-level options over a document-level scope: the scope
/// (e.g. the document's merged `execute` map) is the lower layer,
/// the cell options the higher — per-cell values win, scope values
/// fill the gaps. Returns `None` when both inputs are `None`.
pub fn merge_cell_over_scope(
    scope: Option<&ConfigValue>,
    cell: Option<&ConfigValue>,
) -> Option<ConfigValue> {
    match (scope, cell) {
        (None, None) => None,
        (Some(s), None) => Some(s.clone()),
        (None, Some(c)) => Some(c.clone()),
        (Some(s), Some(c)) => {
            // In-tree precedent (metadata_merge.rs): later layer wins.
            let merged = MergedConfig::new(vec![s, c]);
            Some(merged.materialize().unwrap_or_else(|_| c.clone()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_source_map::{FileId, SourceContext};

    // ── Registry ────────────────────────────────────────────────────

    #[test]
    fn registry_line_comment_languages() {
        for (lang, prefix) in [
            ("python", "#"),
            ("r", "#"),
            ("julia", "#"),
            ("bash", "#"),
            ("lua", "--"),
            ("sql", "--"),
            ("haskell", "--"),
            ("js", "//"),
            ("rust", "//"),
            ("cpp", "//"),
            ("java", "//"),
            ("matlab", "%"),
            ("tikz", "%"),
            ("fortran", "!"),
            ("apl", "⍝"),
            ("stata", "*"),
        ] {
            let syn = comment_syntax_for(lang);
            assert_eq!(syn.prefix, prefix, "prefix for {lang}");
            assert_eq!(syn.suffix, None, "suffix for {lang}");
        }
    }

    #[test]
    fn registry_block_comment_languages() {
        assert_eq!(
            comment_syntax_for("c"),
            CommentSyntax {
                prefix: "/*",
                suffix: Some("*/")
            }
        );
        assert_eq!(
            comment_syntax_for("css"),
            CommentSyntax {
                prefix: "/*",
                suffix: Some("*/")
            }
        );
        assert_eq!(
            comment_syntax_for("ocaml"),
            CommentSyntax {
                prefix: "(*",
                suffix: Some("*)")
            }
        );
        assert_eq!(
            comment_syntax_for("sas"),
            CommentSyntax {
                prefix: "*",
                suffix: Some(";")
            }
        );
    }

    #[test]
    fn registry_unknown_language_defaults_to_hash() {
        assert_eq!(
            comment_syntax_for("some-new-kernel"),
            CommentSyntax {
                prefix: "#",
                suffix: None
            }
        );
    }

    #[test]
    fn registry_lookup_is_case_insensitive() {
        assert_eq!(comment_syntax_for("Python").prefix, "#");
        assert_eq!(comment_syntax_for("LUA").prefix, "--");
    }

    // ── Partition: test scaffolding ─────────────────────────────────

    /// Register `body` as an ephemeral file and return a matching
    /// `Original` SourceInfo plus the context for mapping assertions.
    fn body_fixture(body: &str) -> (SourceInfo, SourceContext, FileId) {
        let mut ctx = SourceContext::new();
        let file_id = ctx.add_file("cell-body.txt".to_string(), Some(body.to_string()));
        let info = SourceInfo::original(file_id, 0, body.len());
        (info, ctx, file_id)
    }

    fn partition(language: &str, body: &str) -> PartitionedCell {
        let (info, _ctx, _) = body_fixture(body);
        partition_cell_options(language, body, info).expect("partition should succeed")
    }

    // ── Partition: structure ────────────────────────────────────────

    #[test]
    fn partition_no_option_lines_returns_body_unchanged() {
        let body = "x = 1\nprint(x)\n";
        let cell = partition("python", body);
        assert!(cell.options.is_none());
        assert_eq!(cell.code, body);
        assert_eq!(cell.options_len, 0);
    }

    #[test]
    fn partition_extracts_leading_options_and_strips_them_from_code() {
        let body = "#| error: true\n#| echo: false\nx = 1\n";
        let cell = partition("python", body);
        let options = cell.options.expect("options parsed");
        assert!(options.is_hash());
        assert_eq!(
            options
                .get_hash_value("error")
                .and_then(|v| v.yaml.as_bool()),
            Some(true)
        );
        assert_eq!(
            options
                .get_hash_value("echo")
                .and_then(|v| v.yaml.as_bool()),
            Some(false)
        );
        assert_eq!(cell.code, "x = 1\n");
        assert_eq!(cell.options_len, "#| error: true\n#| echo: false\n".len());
    }

    #[test]
    fn partition_stops_at_first_non_option_line() {
        // The `#|` after a code line is NOT an option (leading run only).
        let body = "#| error: true\nx = 1\n#| echo: false\n";
        let cell = partition("python", body);
        let options = cell.options.expect("options parsed");
        assert!(options.get_hash_value("error").is_some());
        assert!(options.get_hash_value("echo").is_none());
        assert_eq!(cell.code, "x = 1\n#| echo: false\n");
    }

    #[test]
    fn partition_indented_marker_is_not_an_option_line() {
        // Q1 anchors the option pattern at column 0.
        let body = "  #| error: true\nx = 1\n";
        let cell = partition("python", body);
        assert!(cell.options.is_none());
        assert_eq!(cell.code, body);
    }

    #[test]
    fn partition_marker_allows_space_before_pipe_and_one_after() {
        // `# | error: true` (spaces between prefix and pipe) and
        // exactly one space consumed after the pipe.
        let body = "#  | error: true\nx = 1\n";
        let cell = partition("python", body);
        let options = cell.options.expect("options parsed");
        assert_eq!(
            options
                .get_hash_value("error")
                .and_then(|v| v.yaml.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn partition_options_only_cell_has_empty_code() {
        let body = "#| error: true\n";
        let cell = partition("python", body);
        assert!(cell.options.is_some());
        assert_eq!(cell.code, "");
        assert_eq!(cell.options_len, body.len());
    }

    #[test]
    fn partition_blank_option_lines_yield_no_options() {
        // A bare `#|` run with no content is not an options map.
        let body = "#|\nx = 1\n";
        let cell = partition("python", body);
        assert!(cell.options.is_none());
        assert_eq!(cell.code, "x = 1\n");
        // The blank option line is still consumed from the code.
        assert_eq!(cell.options_len, "#|\n".len());
    }

    #[test]
    fn partition_multiline_yaml_value_reassembles() {
        // Block-scalar option value spanning several option lines.
        let body = "#| fig-cap: |\n#|   A caption\n#|   over two lines\nplot()\n";
        let cell = partition("python", body);
        let options = cell.options.expect("options parsed");
        let cap = options
            .get_hash_value("fig-cap")
            .and_then(|v| v.yaml.as_str().map(str::to_string))
            .expect("fig-cap parsed");
        assert_eq!(cap, "A caption\nover two lines\n");
        assert_eq!(cell.code, "plot()\n");
    }

    #[test]
    fn partition_lua_style_marker() {
        let body = "--| error: true\nprint(1)\n";
        let cell = partition("lua", body);
        let options = cell.options.expect("options parsed");
        assert_eq!(
            options
                .get_hash_value("error")
                .and_then(|v| v.yaml.as_bool()),
            Some(true)
        );
        assert_eq!(cell.code, "print(1)\n");
    }

    #[test]
    fn partition_js_style_marker() {
        let body = "//| error: true\nconsole.log(1)\n";
        let cell = partition("js", body);
        assert!(cell.options.is_some());
        assert_eq!(cell.code, "console.log(1)\n");
    }

    #[test]
    fn partition_block_comment_language_requires_and_strips_suffix() {
        let body = "/*| error: true */\nint x;\n";
        let cell = partition("c", body);
        let options = cell.options.expect("options parsed");
        assert_eq!(
            options
                .get_hash_value("error")
                .and_then(|v| v.yaml.as_bool()),
            Some(true)
        );
        assert_eq!(cell.code, "int x;\n");
    }

    #[test]
    fn partition_block_comment_line_without_suffix_is_not_an_option() {
        let body = "/*| error: true\nint x;\n";
        let cell = partition("c", body);
        assert!(cell.options.is_none());
        assert_eq!(cell.code, body);
    }

    #[test]
    fn partition_hash_marker_does_not_match_other_languages() {
        // A `#|` line in a js cell is not an option line (js is `//|`).
        let body = "#| error: true\nconsole.log(1)\n";
        let cell = partition("js", body);
        assert!(cell.options.is_none());
        assert_eq!(cell.code, body);
    }

    #[test]
    fn partition_malformed_yaml_is_an_error() {
        let body = "#| error: [unclosed\nx = 1\n";
        let (info, _ctx, _) = body_fixture(body);
        let result = partition_cell_options("python", body, info);
        assert!(
            matches!(result, Err(CellOptionsError::InvalidYaml(_))),
            "expected InvalidYaml, got {result:?}"
        );
    }

    // ── Partition: source attribution ───────────────────────────────

    #[test]
    fn partition_option_value_maps_back_to_body_offset() {
        let body = "#| error: true\nx = 1\n";
        let (info, ctx, file_id) = body_fixture(body);
        let cell = partition_cell_options("python", body, info).expect("partition");
        let options = cell.options.expect("options parsed");
        let value = options.get_hash_value("error").expect("error key");
        let mapped = value
            .source_info
            .map_offset(0, &ctx)
            .expect("value offset maps through the concat");
        assert_eq!(mapped.file_id, file_id);
        // `true` starts at byte 10 of the body ("#| error: " is 10 bytes).
        assert_eq!(mapped.location.offset, body.find("true").unwrap());
    }

    #[test]
    fn partition_second_line_option_maps_back_to_body_offset() {
        let body = "#| error: true\n#| echo: false\nx = 1\n";
        let (info, ctx, file_id) = body_fixture(body);
        let cell = partition_cell_options("python", body, info).expect("partition");
        let options = cell.options.expect("options parsed");
        let value = options.get_hash_value("echo").expect("echo key");
        let mapped = value
            .source_info
            .map_offset(0, &ctx)
            .expect("second-line value maps");
        assert_eq!(mapped.file_id, file_id);
        assert_eq!(mapped.location.offset, body.find("false").unwrap());
    }

    #[test]
    fn partition_code_source_is_substring_after_options() {
        let body = "#| error: true\nx = 1\n";
        let (info, ctx, file_id) = body_fixture(body);
        let cell = partition_cell_options("python", body, info).expect("partition");
        let mapped = cell
            .code_source
            .map_offset(0, &ctx)
            .expect("code start maps");
        assert_eq!(mapped.file_id, file_id);
        assert_eq!(mapped.location.offset, body.find("x = 1").unwrap());
    }

    // ── Scoped resolution (decision 3) ──────────────────────────────

    /// Parse a YAML string into a ConfigValue for scope-merge tests.
    fn config_of(yaml: &str) -> ConfigValue {
        let parsed = quarto_yaml::parse(yaml).expect("test yaml parses");
        let (config, diags) = options_to_config(parsed);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        config
    }

    fn allow_errors(merged: Option<&ConfigValue>) -> bool {
        merged
            .and_then(|c| c.get("error"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    #[test]
    fn scoped_cell_error_true_alone_allows() {
        let cell = config_of("error: true");
        let merged = merge_cell_over_scope(None, Some(&cell));
        assert!(allow_errors(merged.as_ref()));
    }

    #[test]
    fn scoped_doc_execute_error_true_alone_allows() {
        let scope = config_of("error: true");
        let merged = merge_cell_over_scope(Some(&scope), None);
        assert!(allow_errors(merged.as_ref()));
    }

    #[test]
    fn scoped_cell_false_overrides_doc_true() {
        let scope = config_of("error: true");
        let cell = config_of("error: false");
        let merged = merge_cell_over_scope(Some(&scope), Some(&cell));
        assert!(!allow_errors(merged.as_ref()));
    }

    #[test]
    fn scoped_cell_true_overrides_doc_false() {
        let scope = config_of("error: false");
        let cell = config_of("error: true");
        let merged = merge_cell_over_scope(Some(&scope), Some(&cell));
        assert!(allow_errors(merged.as_ref()));
    }

    #[test]
    fn scoped_absent_everywhere_disallows() {
        assert!(!allow_errors(merge_cell_over_scope(None, None).as_ref()));
        // Unrelated keys on both sides don't grant permission.
        let scope = config_of("echo: true");
        let cell = config_of("warning: false");
        let merged = merge_cell_over_scope(Some(&scope), Some(&cell));
        assert!(!allow_errors(merged.as_ref()));
    }

    #[test]
    fn scoped_merge_preserves_scope_keys_cell_does_not_set() {
        let scope = config_of("error: true\ntimeout: 30");
        let cell = config_of("echo: false");
        let merged = merge_cell_over_scope(Some(&scope), Some(&cell)).expect("merged");
        assert_eq!(merged.get("timeout").and_then(|v| v.as_int()), Some(30));
        assert!(allow_errors(Some(&merged)));
        assert_eq!(merged.get("echo").and_then(|v| v.as_bool()), Some(false));
    }
}
