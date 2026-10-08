/*
 * yaml_syntax_error.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Reporting YAML that `quarto-yaml` could not parse.
//!
//! Since quarto-yaml 0.4.0 a syntax error carries the scanner's message
//! with no position text, plus a `location` at the character where the
//! scanner gave up, already mapped through the parse's parent `SourceInfo`.
//! Anything that only stringifies the error therefore loses the position;
//! use these helpers instead (bd-x30aq7ae).

use quarto_error_reporting::DiagnosticMessageBuilder;
use quarto_source_map::SourceInfo;

/// Split a YAML parse error into its message and, when quarto-yaml could
/// locate it, the position where parsing failed.
pub fn yaml_error_parts(error: quarto_yaml::Error) -> (String, Option<SourceInfo>) {
    match error {
        quarto_yaml::Error::ParseError { message, location }
        | quarto_yaml::Error::InvalidStructure { message, location } => (message, location),
        quarto_yaml::Error::UnexpectedEof { location } => {
            ("unexpected end of input".to_string(), location)
        }
    }
}

/// A Q-1-1 "YAML Syntax Error" located where parsing failed, or at
/// `fallback` (typically the whole YAML text) when the error carries no
/// location. Returns the builder so callers can add a context hint.
pub fn yaml_syntax_error(
    error: quarto_yaml::Error,
    fallback: &SourceInfo,
) -> DiagnosticMessageBuilder {
    let (problem, location) = yaml_error_parts(error);
    DiagnosticMessageBuilder::error("YAML Syntax Error")
        .with_code("Q-1-1")
        .with_location(location.unwrap_or_else(|| fallback.clone()))
        .problem(problem)
}

/// The error as prose with its position, for `quarto_yaml::parse_file(content, ..)`
/// failures reported where no span can be rendered: `"<message> (line L, column C)"`,
/// or just the message when the error has no location.
pub fn describe_yaml_error(content: &str, error: quarto_yaml::Error) -> String {
    let (message, location) = yaml_error_parts(error);
    match location.and_then(|l| l.resolve_byte_range()) {
        Some((_file, start, _end)) => {
            let (line, column) = line_column(content, start);
            format!("{message} (line {line}, column {column})")
        }
        None => message,
    }
}

/// 1-based `(line, column)` of a byte `offset` in `content`, column counted
/// in characters. For prose-only error paths that cannot carry a span.
pub fn line_column(content: &str, offset: usize) -> (usize, usize) {
    let mut offset = offset.min(content.len());
    while !content.is_char_boundary(offset) {
        offset -= 1;
    }
    let before = &content[..offset];
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    (
        before.matches('\n').count() + 1,
        before[line_start..].chars().count() + 1,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_source_map::FileId;

    #[test]
    fn located_error_keeps_its_location() {
        let at = SourceInfo::original(FileId(7), 12, 13);
        let error = quarto_yaml::Error::ParseError {
            message: "bad".to_string(),
            location: Some(at.clone()),
        };
        let diag = yaml_syntax_error(error, &SourceInfo::original(FileId(7), 0, 40)).build();
        assert_eq!(diag.code.as_deref(), Some("Q-1-1"));
        assert_eq!(diag.title, "YAML Syntax Error");
        assert_eq!(diag.location, Some(at));
        assert_eq!(diag.problem.as_ref().map(|p| p.as_str()), Some("bad"));
    }

    #[test]
    fn unlocated_error_falls_back() {
        let fallback = SourceInfo::original(FileId(7), 0, 40);
        let diag = yaml_syntax_error(
            quarto_yaml::Error::UnexpectedEof { location: None },
            &fallback,
        )
        .build();
        assert_eq!(diag.location, Some(fallback));
    }

    #[test]
    fn describe_resolves_the_position_in_the_parsed_file() {
        let content = "a: 1\nb: [x\nc: 2\n";
        let error = quarto_yaml::parse_file(content, "f.yml").unwrap_err();
        let text = describe_yaml_error(content, error);
        assert!(text.ends_with("(line 3, column 2)"), "{text}");
    }

    #[test]
    fn line_column_counts_chars_and_clamps() {
        let s = "a: 1\nhé: [x\n";
        assert_eq!(line_column(s, 0), (1, 1));
        let colon = s.find(": [").unwrap();
        assert_eq!(line_column(s, colon), (2, 3));
        assert_eq!(line_column(s, colon - 1), (2, 2)); // inside 'é': floors
        assert_eq!(line_column(s, 999), (3, 1));
    }
}
