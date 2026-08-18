// literal-brackets: escape bracketed text that has no matching definition.
//
// This is the *destructive* arm of bd-reference-links-unsupported-ddc4skac.
//
//   Input:  Requires Posit Connect [Version TBD] or later.
//   Output: Requires Posit Connect \[Version TBD\] or later.
//
// Without the escape q2 deletes the brackets silently, which in the Posit
// Connect docs changed documented values rather than just formatting — the
// default mail subject prefix really is `[Posit Connect]`, and the CSRF
// explanation keys its prose to a numbered diagram via `[1]` and `[2]`.
//
// It is a separate rule from `reference-links` because the risk profile is
// different, not because the syntax is: an escape is a source edit that
// cannot afterwards be distinguished from an author's intent, and
// `convert -r all` must never make that edit unasked. Run
// `qmd-syntax-helper check -r literal-brackets` first — it enumerates every
// bracket this rule would escape, with locations.
//
// The escaped form is safe in both engines: q2 and Quarto 1 both render
// `\[...\]` as literal brackets, and q2 produces no `Span`/`Image` node for
// it at all, which is what makes repeated `convert` passes idempotent.

use anyhow::Result;
use std::path::Path;

use crate::conversions::bracket_analysis::{Finding, PartKind, analyze};
use crate::rule::{CheckResult, ConvertResult, Rule};
use crate::utils::file_io::{read_file, write_file};

use super::bracket_edit::{Edit, apply_edits, source_location};

pub struct LiteralBracketsConverter {}

impl LiteralBracketsConverter {
    pub fn new() -> Result<Self> {
        Ok(Self {})
    }
}

/// Escape the opening and closing bracket of one group.
///
/// The group's interior is left completely alone — it may contain arbitrary
/// inline markup (`` [`noexec`] ``), and only the two bracket characters are
/// at risk of being eaten.
fn escape_edits(source: &str, start: usize, end: usize, kind: PartKind) -> Vec<Edit> {
    // `[label]` opens at `start`; `![alt]` opens one byte later, after the `!`.
    let open = match kind {
        PartKind::Span => start,
        PartKind::Image => start + 1,
    };
    let close = end - 1;

    debug_assert_eq!(source.as_bytes().get(open), Some(&b'['));
    debug_assert_eq!(source.as_bytes().get(close), Some(&b']'));

    vec![
        Edit::replace(open, open + 1, "\\[".to_string()),
        Edit::replace(close, close + 1, "\\]".to_string()),
    ]
}

impl Rule for LiteralBracketsConverter {
    fn name(&self) -> &str {
        "literal-brackets"
    }

    fn description(&self) -> &str {
        "Escape bracketed text that would otherwise be silently deleted"
    }

    /// Escaping rewrites the author's source in a way that cannot later be
    /// told apart from something they wrote deliberately, so this rule is
    /// never applied by a bulk `convert -r all`. Run `check -r
    /// literal-brackets` to enumerate the edits, then opt in explicitly.
    fn opt_in_only(&self) -> bool {
        true
    }

    /// Findings come from walking the parsed AST; without one there is
    /// nothing to say, and "no findings" must not read as clean.
    fn requires_parse(&self) -> bool {
        true
    }

    fn check(&self, file_path: &Path, _verbose: bool) -> Result<Vec<CheckResult>> {
        let content = read_file(file_path)?;
        let analysis = analyze(&content, &file_path.to_string_lossy())?;
        let path = file_path.to_string_lossy().to_string();

        let mut results = Vec::new();

        for finding in &analysis.findings {
            let text = content
                .get(finding.start()..finding.end())
                .unwrap_or_default();

            let message = match finding {
                Finding::Literal { kind, .. } => match kind {
                    PartKind::Span => {
                        format!("Brackets in `{text}` will be silently deleted; they need escaping")
                    }
                    PartKind::Image => format!(
                        "`{text}` has no matching definition and renders as an empty image; \
                         it needs escaping"
                    ),
                },
                Finding::Ambiguous { count, .. } => format!(
                    "`{text}` has {count} adjacent bracketed groups and is ambiguous; \
                     left unchanged for human review"
                ),
                // Resolvable references belong to `reference-links`.
                Finding::Reference { .. } => continue,
            };

            results.push(CheckResult {
                rule_name: self.name().to_string(),
                file_path: path.clone(),
                has_issue: true,
                issue_count: 1,
                message: Some(message),
                location: Some(source_location(&content, finding.start())),
                error_code: None,
                error_codes: None,
                ..Default::default()
            });
        }

        Ok(results)
    }

    fn convert(
        &self,
        file_path: &Path,
        in_place: bool,
        check_mode: bool,
        _verbose: bool,
    ) -> Result<ConvertResult> {
        let content = read_file(file_path)?;
        let analysis = analyze(&content, &file_path.to_string_lossy())?;

        let mut edits = Vec::new();
        let mut fixes = 0;

        for finding in &analysis.findings {
            if let Finding::Literal { start, end, kind } = finding {
                edits.extend(escape_edits(&content, *start, *end, *kind));
                fixes += 1;
            }
        }

        let name = self.name().to_string();
        let path = file_path.to_string_lossy().to_string();

        if fixes == 0 {
            return Ok(ConvertResult {
                rule_name: name,
                file_path: path,
                fixes_applied: 0,
                message: Some(if in_place || check_mode {
                    "No unescaped literal brackets found".to_string()
                } else {
                    content
                }),
            });
        }

        let escaped = apply_edits(&content, edits);

        if check_mode {
            return Ok(ConvertResult {
                rule_name: name,
                file_path: path,
                fixes_applied: fixes,
                message: Some(format!("Would escape {fixes} bracketed group(s)")),
            });
        }

        if in_place {
            write_file(file_path, &escaped)?;
            return Ok(ConvertResult {
                rule_name: name,
                file_path: path,
                fixes_applied: fixes,
                message: Some(format!("Escaped {fixes} bracketed group(s)")),
            });
        }

        Ok(ConvertResult {
            rule_name: name,
            file_path: path,
            fixes_applied: fixes,
            message: Some(escaped),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_both_brackets_of_a_span() {
        let source = "A [x] B";
        let edits = escape_edits(source, 2, 5, PartKind::Span);
        assert_eq!(apply_edits(source, edits), "A \\[x\\] B");
    }

    #[test]
    fn escapes_the_brackets_but_not_the_bang_of_an_image() {
        let source = "A ![x] B";
        let edits = escape_edits(source, 2, 6, PartKind::Image);
        assert_eq!(apply_edits(source, edits), "A !\\[x\\] B");
    }
}
