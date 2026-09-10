// Q-2-52: Shortcode delimiter not separated by a space
//
// A shortcode's delimiters take a space on the inside: `{{< fa plus >}}`,
// not `{{<fa plus>}}`. Quarto 1 matched shortcodes with a regular
// expression and tolerated the tight spelling, so documents ported from
// it arrive full of them.
//
// Error catalog entry: crates/quarto-error-catalog/error_catalog.json
// Error code: Q-2-52
//
// Example:
//   Input:  Click the {{<fa plus>}} icon.
//   Output: Click the {{< fa plus >}} icon.
//
// Both delimiters matter. Spacing only the opening one leaves
// `{{< fa plus>}}`, which is still a parse error — so this rule keeps
// going until the file has no Q-2-52 left rather than fixing one side.

use anyhow::{Context, Result};
use std::path::Path;

use crate::rule::{CheckResult, ConvertResult, Rule, SourceLocation};
use crate::utils::file_io::read_file;

pub struct Q252Converter {}

#[derive(Debug, Clone)]
struct Q252Violation {
    /// Offset in the *original* content where a space is missing.
    offset: usize,
    location: SourceLocation,
}

/// A missing separator leaves the parser desynchronised, so Quarto reports
/// one Q-2-52 per parse and stops — see `desynchronizes` in the error
/// corpus. A file with seven tight shortcodes therefore needs seven passes
/// to enumerate them, which is what `analyze` does. The bound only exists
/// so that a diagnostic which somehow survives its own fix cannot spin.
///
/// One pass is one full parse, so the cost is quadratic in the number of
/// violations: roughly 0.12s for ten of them in a debug build, 1.4s for
/// eighty. That is the right trade for a one-shot migration tool, but it
/// is worth knowing before pointing it at something enormous.
const MAX_PASSES: usize = 10_000;

impl Q252Converter {
    pub fn new() -> Result<Self> {
        Ok(Self {})
    }

    /// Find every missing separator, and return the corrected content.
    ///
    /// Both spellings are fixed by the same edit: Quarto blames the
    /// character where the space belongs — the one right after `{{<`, or
    /// the `>` of `>}}` — so inserting a single space at the reported
    /// offset is the whole repair. Re-parsing after each insertion is
    /// what keeps a code span such as `` `{{<fa plus>}}` `` untouched:
    /// its contents are not parsed as a shortcode, so they never produce
    /// a diagnostic to act on.
    fn analyze(&self, content: &str, filename: &str) -> Result<(Vec<Q252Violation>, String)> {
        let mut violations: Vec<Q252Violation> = Vec::new();
        let mut working = content.to_string();

        for _ in 0..MAX_PASSES {
            let Some(offset) = first_missing_separator(&working, filename) else {
                break;
            };
            // Inserting at, or slicing on, a byte that is not a character
            // boundary would panic. Tree-sitter reports boundaries, so
            // this is a guard rather than an expected path.
            debug_assert!(
                working.is_char_boundary(offset),
                "tree-sitter reported an offset inside a character: {offset}"
            );
            if !working.is_char_boundary(offset) {
                break;
            }

            // Offsets are monotone across passes: the earliest remaining
            // Q-2-52 is reported each time, and fixing one never creates
            // an earlier one. That is what makes `offset - inserted` the
            // position in the original content. Bail rather than record a
            // wrong location if the assumption is ever violated.
            let inserted = violations.len();
            let Some(original_offset) = offset.checked_sub(inserted) else {
                break;
            };
            debug_assert!(
                content.is_char_boundary(original_offset),
                "mapped offset {original_offset} is inside a character"
            );
            if !content.is_char_boundary(original_offset) {
                break;
            }
            debug_assert!(
                violations
                    .last()
                    .is_none_or(|previous| original_offset >= previous.offset),
                "offsets went backwards at {original_offset}"
            );
            if violations
                .last()
                .is_some_and(|previous| original_offset < previous.offset)
            {
                break;
            }

            violations.push(Q252Violation {
                offset: original_offset,
                location: SourceLocation {
                    row: row_of(content, original_offset),
                    column: column_of(content, original_offset),
                },
            });
            working.insert(offset, ' ');
        }

        Ok((violations, working))
    }

    fn violations_for(&self, file_path: &Path) -> Result<(Vec<Q252Violation>, String)> {
        let content = read_file(file_path)?;
        self.analyze(&content, &file_path.to_string_lossy())
            .with_context(|| format!("Failed to analyze {}", file_path.display()))
    }
}

/// The offset of the first missing shortcode separator in `content` that
/// this rule is willing to repair, if any.
fn first_missing_separator(content: &str, filename: &str) -> Option<usize> {
    let fenced = fenced_code_ranges(content);
    let result = pampa::readers::qmd::read(
        content.as_bytes(),
        false, // not loose mode
        filename,
        &mut std::io::sink(),
        // Don't prune. Pruning keeps one diagnostic per ERROR node, so an
        // unrelated error earlier in the file hides every Q-2-52 after it,
        // and the rule reports a document with real violations as clean.
        // `q-2-28` makes the same call for the same reason.
        false,
        None,
    );

    let diagnostics = match result {
        Ok(_) => return None,
        Err(diagnostics) => diagnostics,
    };

    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code.as_deref() == Some("Q-2-52"))
        .filter_map(|diagnostic| diagnostic.location.as_ref()?.resolve_byte_range())
        // The file id is discarded: this rule parses exactly one file with
        // no parent context, so any resolved span is necessarily in it and
        // the offset applies to `content` directly.
        .map(|(_file_id, start_offset, _end_offset)| start_offset)
        .filter(|offset| {
            !fenced
                .iter()
                .any(|(start, end)| offset >= start && offset < end)
        })
        .min()
}

/// Byte ranges covered by fenced code blocks, found by scanning lines
/// rather than by asking the parser.
///
/// A tight shortcode inside a fence is someone showing the syntax, and it
/// must survive the conversion. In a document whose only fault is the
/// missing separator the parser already protects it — fence content is
/// not parsed as inline, so it raises no diagnostic. But one unrelated
/// error anywhere in the file (a literal brace run, a space in a link
/// target) desynchronises the parser far enough that the fence stops
/// being recognised as a fence, and its contents begin reporting Q-2-52.
/// Editing them would silently rewrite a documented example. A line scan
/// still sees the fence in that case, which is the point of not going
/// through the parser here.
///
/// The scan is deliberately generous — any line whose first non-space run
/// is three or more backticks or tildes toggles the state — because
/// over-including means the rule declines to fix something, while
/// under-including means it rewrites something it should not.
fn fenced_code_ranges(content: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut open: Option<(char, usize, usize)> = None; // fence char, run length, start offset
    let mut offset = 0;

    for line in content.split_inclusive('\n') {
        let trimmed = line.trim_start_matches(' ');
        let indent = line.len() - trimmed.len();
        let fence = if indent <= 3 {
            trimmed
                .chars()
                .next()
                .filter(|c| *c == '`' || *c == '~')
                .map(|c| (c, trimmed.chars().take_while(|d| *d == c).count()))
        } else {
            None
        };

        match (&open, fence) {
            (None, Some((marker, run))) if run >= 3 => {
                open = Some((marker, run, offset));
            }
            (Some((marker, run, start)), Some((closing, closing_run)))
                if *marker == closing && closing_run >= *run =>
            {
                ranges.push((*start, offset + line.len()));
                open = None;
            }
            _ => {}
        }
        offset += line.len();
    }

    // An unclosed fence runs to the end of the document.
    if let Some((_, _, start)) = open {
        ranges.push((start, content.len()));
    }

    ranges
}

/// Row of `offset`, 0-indexed.
fn row_of(content: &str, offset: usize) -> usize {
    content[..offset].matches('\n').count()
}

/// Column of `offset`, 0-indexed, in bytes from the start of its line.
fn column_of(content: &str, offset: usize) -> usize {
    let line_start = content[..offset].rfind('\n').map_or(0, |pos| pos + 1);
    offset - line_start
}

impl Rule for Q252Converter {
    fn name(&self) -> &str {
        "q-2-52"
    }

    fn description(&self) -> &str {
        "Fix Q-2-52: Put a space inside both shortcode delimiters ({{< name args >}})"
    }

    fn check(&self, file_path: &Path, _verbose: bool) -> Result<Vec<CheckResult>> {
        let (violations, _fixed) = self.violations_for(file_path)?;

        Ok(violations
            .into_iter()
            .map(|violation| CheckResult {
                rule_name: self.name().to_string(),
                file_path: file_path.to_string_lossy().to_string(),
                has_issue: true,
                issue_count: 1,
                message: Some(format!(
                    "Q-2-52 shortcode delimiter missing its space at line {}, column {}",
                    violation.location.row + 1,
                    violation.location.column + 1
                )),
                location: Some(violation.location),
                error_code: Some("Q-2-52".to_string()),
                error_codes: None,
                ..Default::default()
            })
            .collect())
    }

    fn convert(
        &self,
        file_path: &Path,
        in_place: bool,
        check_mode: bool,
        _verbose: bool,
    ) -> Result<ConvertResult> {
        let (violations, fixed_content) = self.violations_for(file_path)?;
        let name = self.name().to_string();
        let path = file_path.to_string_lossy().to_string();

        if violations.is_empty() {
            return Ok(ConvertResult {
                rule_name: name,
                file_path: path,
                fixes_applied: 0,
                message: Some("No Q-2-52 shortcode delimiter issues found".to_string()),
            });
        }

        if check_mode {
            return Ok(ConvertResult {
                rule_name: name,
                file_path: path,
                fixes_applied: violations.len(),
                message: Some(format!(
                    "Would fix {} Q-2-52 shortcode delimiter violation(s)",
                    violations.len()
                )),
            });
        }

        if in_place {
            crate::utils::file_io::write_file(file_path, &fixed_content)?;
            Ok(ConvertResult {
                rule_name: name,
                file_path: path,
                fixes_applied: violations.len(),
                message: Some(format!(
                    "Fixed {} Q-2-52 shortcode delimiter violation(s)",
                    violations.len()
                )),
            })
        } else {
            Ok(ConvertResult {
                rule_name: name,
                file_path: path,
                fixes_applied: violations.len(),
                message: Some(fixed_content),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fenced_code_ranges;

    fn spans(content: &str) -> Vec<&str> {
        fenced_code_ranges(content)
            .into_iter()
            .map(|(start, end)| &content[start..end])
            .collect()
    }

    #[test]
    fn finds_a_backtick_fence() {
        assert_eq!(spans("a\n```\nx\n```\nb\n"), vec!["```\nx\n```\n"]);
    }

    #[test]
    fn finds_a_tilde_fence() {
        assert_eq!(spans("a\n~~~\nx\n~~~\nb\n"), vec!["~~~\nx\n~~~\n"]);
    }

    /// A fence closes only on its own marker, so a run of the other
    /// character inside it is content.
    #[test]
    fn a_tilde_run_does_not_close_a_backtick_fence() {
        assert_eq!(spans("```\n~~~\n```\n"), vec!["```\n~~~\n```\n"]);
    }

    /// A shorter run does not close a longer fence, which is what makes a
    /// fence containing a fence work.
    #[test]
    fn a_shorter_run_does_not_close_a_longer_fence() {
        assert_eq!(
            spans("````\n```\nx\n```\n````\n"),
            vec!["````\n```\nx\n```\n````\n"]
        );
    }

    /// An unclosed fence swallows the rest of the document. Declining to
    /// edit there is the safe reading.
    #[test]
    fn an_unclosed_fence_runs_to_the_end() {
        assert_eq!(spans("a\n```\nx\ny\n"), vec!["```\nx\ny\n"]);
    }

    #[test]
    fn up_to_three_spaces_of_indent_still_opens_a_fence() {
        assert_eq!(spans("   ```\nx\n   ```\n"), vec!["   ```\nx\n   ```\n"]);
    }

    /// Four spaces is an indented code block, not a fence. Its contents
    /// are not parsed as inline either way, so nothing is at risk.
    #[test]
    fn four_spaces_of_indent_is_not_a_fence() {
        assert!(spans("    ```\nx\n    ```\n").is_empty());
    }

    #[test]
    fn a_document_without_fences_has_no_ranges() {
        assert!(spans("Click the {{<fa plus>}} icon.\n").is_empty());
    }
}
