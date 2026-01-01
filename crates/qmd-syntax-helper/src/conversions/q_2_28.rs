// Q-2-28: Line Break Before Escaped Shortcode Close
//
// This conversion rule fixes Q-2-28 errors by removing line breaks
// immediately before the escaped shortcode closing delimiter >}}}
//
// Error catalog entry: crates/quarto-error-reporting/error_catalog.json
// Error code: Q-2-28
// Title: "Line Break Before Escaped Shortcode Close"
// Message: "Line breaks are not allowed immediately before the escaped shortcode closing delimiter `>}}}`."
//
// Example:
//   Input:  {{{< include file.qmd
//           >}}}
//   Output: {{{< include file.qmd >}}}
//

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::rule::{CheckResult, ConvertResult, Rule, SourceLocation};
use crate::utils::file_io::read_file;

pub struct Q228Converter {}

#[derive(Debug, Clone)]
struct Q228Violation {
    // We'll store the offset of the newline that needs to be removed
    newline_start: usize,
    // And the offset where >}}} starts (after whitespace)
    close_delimiter_start: usize,
    error_location: Option<SourceLocation>,
}

impl Q228Converter {
    pub fn new() -> Result<Self> {
        Ok(Self {})
    }

    /// Get parse errors and extract Q-2-28 line break violations
    fn get_violations(&self, file_path: &Path) -> Result<Vec<Q228Violation>> {
        let content = fs::read_to_string(file_path)
            .with_context(|| format!("Failed to read file: {}", file_path.display()))?;

        // Parse with pampa to get diagnostics
        let mut sink = std::io::sink();
        let filename = file_path.to_string_lossy();

        let result = pampa::readers::qmd::read(
            content.as_bytes(),
            false, // not loose mode
            &filename,
            &mut sink,
            false, // don't prune errors - we need them!
            None,
        );

        // Get diagnostics from either Ok or Err variant
        let diagnostics = match result {
            Ok((_pandoc, _context, diags)) => diags,
            Err(diags) => diags,
        };

        let mut violations = Vec::new();

        for diagnostic in diagnostics {
            // Check if this is a Q-2-28 error
            if diagnostic.code.as_deref() != Some("Q-2-28") {
                continue;
            }

            // Extract location - this points to where the error occurs
            let location = diagnostic.location.as_ref();
            if location.is_none() {
                continue;
            }

            // The error location can span multiple tokens. Use end_offset to ensure
            // we're after any tokens that might be part of the error
            let error_offset = location.as_ref().unwrap().end_offset();

            // Now we need to find the newline before >}}} and the start of >}}}
            // We'll scan backwards from error_offset to find the newline,
            // then scan forward to find >}}}

            if let Some(violation) = self.find_violation_offsets(&content, error_offset) {
                violations.push(violation);
            }
        }

        Ok(violations)
    }

    /// Find the exact offsets to fix for a Q-2-28 violation
    fn find_violation_offsets(&self, content: &str, error_offset: usize) -> Option<Q228Violation> {
        // Scan backwards from error_offset to find a newline
        // Include error_offset itself in case it points to the newline
        let mut newline_pos = None;
        for i in (0..=error_offset).rev() {
            if i < content.len() && content.as_bytes()[i] == b'\n' {
                newline_pos = Some(i);
                break;
            }
        }

        let newline_start = newline_pos?;

        // Now scan forward from newline to find where >}}} starts (skip whitespace)
        let mut close_delimiter_start = newline_start + 1;
        while close_delimiter_start < content.len() {
            let ch = content.as_bytes()[close_delimiter_start];
            if ch != b' ' && ch != b'\t' {
                break;
            }
            close_delimiter_start += 1;
        }

        // Verify that we're actually at >}}}
        if close_delimiter_start + 4 <= content.len() {
            let slice = &content[close_delimiter_start..close_delimiter_start + 4];
            if slice == ">}}}" {
                return Some(Q228Violation {
                    newline_start,
                    close_delimiter_start,
                    error_location: Some(SourceLocation {
                        row: self.offset_to_row(content, newline_start),
                        column: self.offset_to_column(content, newline_start),
                    }),
                });
            }
        }

        None
    }

    /// Apply fixes to the content by removing line breaks before >}}}
    fn apply_fixes(&self, content: &str, mut violations: Vec<Q228Violation>) -> Result<String> {
        if violations.is_empty() {
            return Ok(content.to_string());
        }

        // Sort violations in reverse order to avoid offset invalidation
        violations.sort_by_key(|v| std::cmp::Reverse(v.newline_start));

        let mut result = content.to_string();

        for violation in violations {
            // Remove everything from the newline to just before >}}}
            // This removes the \n and any leading whitespace
            let remove_start = violation.newline_start;
            let remove_end = violation.close_delimiter_start;

            // Replace with a single space to keep >}}} separated from content
            result.replace_range(remove_start..remove_end, " ");
        }

        Ok(result)
    }

    /// Convert byte offset to row number (0-indexed)
    fn offset_to_row(&self, content: &str, offset: usize) -> usize {
        content[..offset].matches('\n').count()
    }

    /// Convert byte offset to column number (0-indexed)
    fn offset_to_column(&self, content: &str, offset: usize) -> usize {
        let line_start = content[..offset]
            .rfind('\n')
            .map_or(0, |pos| pos + 1);
        offset - line_start
    }
}

impl Rule for Q228Converter {
    fn name(&self) -> &str {
        "q-2-28"
    }

    fn description(&self) -> &str {
        "Fix Q-2-28: Remove line breaks before escaped shortcode closing delimiter >}}}"
    }

    fn check(&self, file_path: &Path, _verbose: bool) -> Result<Vec<CheckResult>> {
        let violations = self.get_violations(file_path)?;

        let results: Vec<CheckResult> = violations
            .into_iter()
            .map(|v| CheckResult {
                rule_name: self.name().to_string(),
                file_path: file_path.to_string_lossy().to_string(),
                has_issue: true,
                issue_count: 1,
                message: Some(format!(
                    "Q-2-28 line break before escaped shortcode close at line {}",
                    v.error_location.as_ref().map_or(0, |l| l.row + 1)
                )),
                location: v.error_location,
                error_code: Some("Q-2-28".to_string()),
                error_codes: None,
            })
            .collect();

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
        let violations = self.get_violations(file_path)?;

        if violations.is_empty() {
            return Ok(ConvertResult {
                rule_name: self.name().to_string(),
                file_path: file_path.to_string_lossy().to_string(),
                fixes_applied: 0,
                message: Some("No Q-2-28 line break issues found".to_string()),
            });
        }

        let fixed_content = self.apply_fixes(&content, violations.clone())?;

        if check_mode {
            // Just report what would be done
            return Ok(ConvertResult {
                rule_name: self.name().to_string(),
                file_path: file_path.to_string_lossy().to_string(),
                fixes_applied: violations.len(),
                message: Some(format!(
                    "Would fix {} Q-2-28 line break violation(s)",
                    violations.len()
                )),
            });
        }

        if in_place {
            // Write back to file
            crate::utils::file_io::write_file(file_path, &fixed_content)?;
            Ok(ConvertResult {
                rule_name: self.name().to_string(),
                file_path: file_path.to_string_lossy().to_string(),
                fixes_applied: violations.len(),
                message: Some(format!(
                    "Fixed {} Q-2-28 line break violation(s)",
                    violations.len()
                )),
            })
        } else {
            // Return the converted content in message
            Ok(ConvertResult {
                rule_name: self.name().to_string(),
                file_path: file_path.to_string_lossy().to_string(),
                fixes_applied: violations.len(),
                message: Some(fixed_content),
            })
        }
    }
}
