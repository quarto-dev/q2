//! YAML tag parsing for configuration merging.
//!
//! This module handles parsing of underscore-separated YAML tags like `!prefer_md`
//! into merge operations and interpretation hints.
//!
//! ## Tag Syntax
//!
//! Tags use underscore as a separator: `!prefer_md`, `!concat_path`.
//! This syntax is fully supported by standard YAML parsers.
//!
//! Single-component tags are also supported: `!prefer`, `!md`, `!path`.

use crate::types::{Interpretation, MergeOp};
use quarto_error_reporting::DiagnosticMessage;
use quarto_source_map::SourceInfo;

/// Result of parsing a YAML tag.
///
/// Contains the extracted merge operation and interpretation hint,
/// plus a flag indicating whether any errors occurred (not just warnings).
#[derive(Debug, Clone, Default)]
pub struct ParsedTag {
    /// Merge operation (None means use default)
    pub merge_op: Option<MergeOp>,

    /// Interpretation hint (None means use context-dependent default)
    pub interpretation: Option<Interpretation>,

    /// True if any errors occurred during parsing (not just warnings)
    pub had_errors: bool,
}

impl ParsedTag {
    /// Get the merge operation, using the provided default if not explicitly set.
    pub fn merge_op_or(&self, default: MergeOp) -> MergeOp {
        self.merge_op.unwrap_or(default)
    }
}

/// Parse a YAML tag suffix with underscore-separated components.
///
/// # Examples
///
/// - `"prefer"` → MergeOp::Prefer
/// - `"md"` → Interpretation::Markdown
/// - `"prefer_md"` → MergeOp::Prefer + Interpretation::Markdown
/// - `"concat_path"` → MergeOp::Concat + Interpretation::Path
///
/// # Error Handling
///
/// - Unknown components emit warnings (Q-1-21)
/// - Empty components emit errors (Q-1-24)
/// - Invalid characters emit errors (Q-1-26)
/// - Conflicting merge operations emit errors (Q-1-28)
///
/// # Arguments
///
/// * `tag_str` - The tag suffix (without the `!` prefix)
/// * `tag_source` - Source location of the tag for error reporting
/// * `diagnostics` - Collector for errors and warnings
///
/// # Returns
///
/// A `ParsedTag` containing the parsed merge operation and interpretation.
/// Check `had_errors` to determine if the tag should be rejected.
pub fn parse_tag(
    tag_str: &str,
    tag_source: &SourceInfo,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> ParsedTag {
    let mut result = ParsedTag::default();

    // Check for invalid characters (only alphanumeric and underscore allowed)
    if tag_str.contains(|c: char| !c.is_alphanumeric() && c != '_') {
        diagnostics.push(
            quarto_error_reporting::DiagnosticMessageBuilder::error(
                "Invalid character in tag",
            )
            .with_code("Q-1-26")
            .problem(format!(
                "Tag '!{}' contains invalid characters (only letters, numbers, and underscores are allowed)",
                tag_str
            ))
            .with_location(tag_source.clone())
            .build(),
        );
        result.had_errors = true;
        return result;
    }

    for component in tag_str.split('_') {
        // Empty component check (Q-1-24)
        if component.is_empty() {
            diagnostics.push(
                quarto_error_reporting::DiagnosticMessageBuilder::error(
                    "Empty tag component",
                )
                .with_code("Q-1-24")
                .problem(format!(
                    "Tag '!{}' has an empty component (check for leading, trailing, or consecutive underscores)",
                    tag_str
                ))
                .with_location(tag_source.clone())
                .build(),
            );
            result.had_errors = true;
            return result;
        }

        // Whitespace check (Q-1-25)
        // Note: This shouldn't normally happen since YAML parsers handle whitespace,
        // but we check anyway for robustness
        if component != component.trim() {
            diagnostics.push(
                quarto_error_reporting::DiagnosticMessageBuilder::error(
                    "Whitespace in tag component",
                )
                .with_code("Q-1-25")
                .problem(format!(
                    "Tag '!{}' has whitespace around a component",
                    tag_str
                ))
                .with_location(tag_source.clone())
                .build(),
            );
            result.had_errors = true;
            return result;
        }

        match component {
            // Merge operations
            "prefer" => {
                if result.merge_op.is_some() {
                    diagnostics.push(
                        quarto_error_reporting::DiagnosticMessageBuilder::error(
                            "Conflicting merge operations",
                        )
                        .with_code("Q-1-28")
                        .problem(format!(
                            "Tag '!{}' specifies multiple merge operations (only one of 'prefer' or 'concat' allowed)",
                            tag_str
                        ))
                        .with_location(tag_source.clone())
                        .build(),
                    );
                    result.had_errors = true;
                    return result;
                }
                result.merge_op = Some(MergeOp::Prefer);
            }
            "concat" => {
                if result.merge_op.is_some() {
                    diagnostics.push(
                        quarto_error_reporting::DiagnosticMessageBuilder::error(
                            "Conflicting merge operations",
                        )
                        .with_code("Q-1-28")
                        .problem(format!(
                            "Tag '!{}' specifies multiple merge operations (only one of 'prefer' or 'concat' allowed)",
                            tag_str
                        ))
                        .with_location(tag_source.clone())
                        .build(),
                    );
                    result.had_errors = true;
                    return result;
                }
                result.merge_op = Some(MergeOp::Concat);
            }

            // Interpretation hints
            "md" => result.interpretation = Some(Interpretation::Markdown),
            "str" => result.interpretation = Some(Interpretation::PlainString),
            "path" => result.interpretation = Some(Interpretation::Path),
            "glob" => result.interpretation = Some(Interpretation::Glob),
            "expr" => result.interpretation = Some(Interpretation::Expr),

            // Unknown components emit warnings (Q-1-21), not errors
            unknown => {
                // Check for common typos and provide suggestions
                let suggestion = match unknown {
                    "prefre" | "perfer" | "pref" => Some("prefer"),
                    "concate" | "conact" | "cat" => Some("concat"),
                    "markdown" | "mark" => Some("md"),
                    "string" | "text" => Some("str"),
                    "file" | "filepath" => Some("path"),
                    "pattern" | "wildcard" => Some("glob"),
                    "expression" | "eval" => Some("expr"),
                    _ => None,
                };

                let mut builder = quarto_error_reporting::DiagnosticMessageBuilder::warning(
                    "Unknown tag component",
                )
                .with_code("Q-1-21")
                .problem(format!(
                    "Unknown component '{}' in tag '!{}'",
                    unknown, tag_str
                ))
                .with_location(tag_source.clone());

                if let Some(did_you_mean) = suggestion {
                    builder = builder.add_hint(format!("Did you mean '{}'?", did_you_mean));
                } else {
                    builder = builder.add_hint(
                        "Valid components are: prefer, concat, md, str, path, glob, expr",
                    );
                }

                diagnostics.push(builder.build());
                // Note: warnings don't set had_errors
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_prefer() {
        let mut diagnostics = Vec::new();
        let result = parse_tag("prefer", &SourceInfo::default(), &mut diagnostics);

        assert!(!result.had_errors);
        assert!(diagnostics.is_empty());
        assert_eq!(result.merge_op, Some(MergeOp::Prefer));
        assert_eq!(result.interpretation, None);
    }

    #[test]
    fn test_parse_concat() {
        let mut diagnostics = Vec::new();
        let result = parse_tag("concat", &SourceInfo::default(), &mut diagnostics);

        assert!(!result.had_errors);
        assert!(diagnostics.is_empty());
        assert_eq!(result.merge_op, Some(MergeOp::Concat));
    }

    #[test]
    fn test_parse_md() {
        let mut diagnostics = Vec::new();
        let result = parse_tag("md", &SourceInfo::default(), &mut diagnostics);

        assert!(!result.had_errors);
        assert!(diagnostics.is_empty());
        assert_eq!(result.merge_op, None);
        assert_eq!(result.interpretation, Some(Interpretation::Markdown));
    }

    #[test]
    fn test_parse_prefer_md_combined() {
        let mut diagnostics = Vec::new();
        let result = parse_tag("prefer_md", &SourceInfo::default(), &mut diagnostics);

        assert!(!result.had_errors);
        assert!(diagnostics.is_empty());
        assert_eq!(result.merge_op, Some(MergeOp::Prefer));
        assert_eq!(result.interpretation, Some(Interpretation::Markdown));
    }

    #[test]
    fn test_parse_concat_path() {
        let mut diagnostics = Vec::new();
        let result = parse_tag("concat_path", &SourceInfo::default(), &mut diagnostics);

        assert!(!result.had_errors);
        assert!(diagnostics.is_empty());
        assert_eq!(result.merge_op, Some(MergeOp::Concat));
        assert_eq!(result.interpretation, Some(Interpretation::Path));
    }

    #[test]
    fn test_parse_all_interpretations() {
        for (tag, expected) in [
            ("md", Interpretation::Markdown),
            ("str", Interpretation::PlainString),
            ("path", Interpretation::Path),
            ("glob", Interpretation::Glob),
            ("expr", Interpretation::Expr),
        ] {
            let mut diagnostics = Vec::new();
            let result = parse_tag(tag, &SourceInfo::default(), &mut diagnostics);

            assert!(!result.had_errors, "Failed for tag: {}", tag);
            assert!(
                diagnostics.is_empty(),
                "Unexpected diagnostics for tag: {}",
                tag
            );
            assert_eq!(
                result.interpretation,
                Some(expected),
                "Wrong interpretation for tag: {}",
                tag
            );
        }
    }

    #[test]
    fn test_unknown_component_warning() {
        let mut diagnostics = Vec::new();
        let result = parse_tag("prefre", &SourceInfo::default(), &mut diagnostics);

        assert!(!result.had_errors); // Warnings don't set had_errors
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].code.as_deref() == Some("Q-1-21"));
    }

    #[test]
    fn test_empty_component_error() {
        let mut diagnostics = Vec::new();
        let result = parse_tag("prefer_", &SourceInfo::default(), &mut diagnostics);

        assert!(result.had_errors);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].code.as_deref() == Some("Q-1-24"));
    }

    #[test]
    fn test_leading_underscore_error() {
        let mut diagnostics = Vec::new();
        let result = parse_tag("_md", &SourceInfo::default(), &mut diagnostics);

        assert!(result.had_errors);
        assert!(diagnostics[0].code.as_deref() == Some("Q-1-24"));
    }

    #[test]
    fn test_invalid_character_error() {
        let mut diagnostics = Vec::new();
        let result = parse_tag("prefer@md", &SourceInfo::default(), &mut diagnostics);

        assert!(result.had_errors);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].code.as_deref() == Some("Q-1-26"));
    }

    #[test]
    fn test_conflicting_merge_ops_error() {
        let mut diagnostics = Vec::new();
        let result = parse_tag("prefer_concat", &SourceInfo::default(), &mut diagnostics);

        assert!(result.had_errors);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].code.as_deref() == Some("Q-1-28"));
    }

    #[test]
    fn test_merge_op_or_default() {
        let tag = ParsedTag::default();
        assert_eq!(tag.merge_op_or(MergeOp::Prefer), MergeOp::Prefer);
        assert_eq!(tag.merge_op_or(MergeOp::Concat), MergeOp::Concat);

        let tag_with_prefer = ParsedTag {
            merge_op: Some(MergeOp::Prefer),
            ..Default::default()
        };
        assert_eq!(
            tag_with_prefer.merge_op_or(MergeOp::Concat),
            MergeOp::Prefer
        );
    }
}
