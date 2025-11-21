// Q-2-7: Unclosed Single Quote
//
// This conversion rule fixes Q-2-7 errors by escaping straight apostrophes
// that are misinterpreted as opening quotes.
//
// The parser misinterprets straight apostrophes when they appear before
// Markdown syntax (e.g., `d'`code``, `qu'**emphasis**`).
//
// Fix strategy: Escape the apostrophe with a backslash `'` → `\'`
//
// Error catalog entry: crates/quarto-markdown-pandoc/resources/error-corpus/Q-2-7.json
// Error code: Q-2-7
// Title: "Unclosed Single Quote"
//
// Example:
//   Input:  d'`Arrow`
//   Output: d\'`Arrow`

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::rule::{CheckResult, ConvertResult, Rule, SourceLocation};
use crate::utils::file_io::read_file;

pub struct Q27Converter {}

#[derive(Debug, Clone)]
struct Q27Violation {
    offset: usize,                          // Offset of the apostrophe to escape
    error_location: Option<SourceLocation>, // For reporting
}

impl Q27Converter {
    pub fn new() -> Result<Self> {
        Ok(Self {})
    }

    /// Get parse errors and extract Q-2-7 unclosed single quote violations
    fn get_violations(&self, file_path: &Path) -> Result<Vec<Q27Violation>> {
        let content = fs::read_to_string(file_path)
            .with_context(|| format!("Failed to read file: {}", file_path.display()))?;

        // Parse with quarto-markdown-pandoc to get diagnostics
        let mut sink = std::io::sink();
        let filename = file_path.to_string_lossy();

        let result = quarto_markdown_pandoc::readers::qmd::read(
            content.as_bytes(),
            false, // not loose mode
            &filename,
            &mut sink,
            true,
            None,
        );

        let diagnostics = match result {
            Ok(_) => return Ok(Vec::new()), // No errors
            Err(diagnostics) => diagnostics,
        };

        let mut violations = Vec::new();

        for diagnostic in diagnostics {
            // Check if this is a Q-2-7 error
            if diagnostic.code.as_deref() != Some("Q-2-7") {
                continue;
            }

            // CRITICAL: For Q-2-7, we need the apostrophe location from details[0]
            // NOT the main diagnostic location (which points to end of block)
            if diagnostic.details.is_empty() {
                continue;
            }

            let detail_location = diagnostic.details[0].location.as_ref();
            if detail_location.is_none() {
                continue;
            }

            let offset = detail_location.unwrap().start_offset();

            violations.push(Q27Violation {
                offset,
                error_location: Some(SourceLocation {
                    row: self.offset_to_row(&content, offset),
                    column: self.offset_to_column(&content, offset),
                }),
            });
        }

        Ok(violations)
    }

    /// Apply fixes by inserting backslashes before apostrophes
    fn apply_fixes(&self, content: &str, mut violations: Vec<Q27Violation>) -> Result<String> {
        if violations.is_empty() {
            return Ok(content.to_string());
        }

        // Sort violations in reverse order to avoid offset invalidation
        violations.sort_by_key(|v| std::cmp::Reverse(v.offset));

        let mut result = content.to_string();

        for violation in violations {
            // Insert backslash before the apostrophe
            // The offset points to the apostrophe itself
            result.insert(violation.offset, '\\');
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
            .map(|pos| pos + 1)
            .unwrap_or(0);
        offset - line_start
    }
}

impl Rule for Q27Converter {
    fn name(&self) -> &str {
        "q-2-7"
    }

    fn description(&self) -> &str {
        "Fix Q-2-7: Escape apostrophes misinterpreted as opening quotes"
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
                    "Q-2-7 unclosed single quote at offset {}",
                    v.offset
                )),
                location: v.error_location,
                error_code: Some("Q-2-7".to_string()),
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
                message: Some("No Q-2-7 unclosed single quote issues found".to_string()),
            });
        }

        let fixed_content = self.apply_fixes(&content, violations.clone())?;

        if check_mode {
            return Ok(ConvertResult {
                rule_name: self.name().to_string(),
                file_path: file_path.to_string_lossy().to_string(),
                fixes_applied: violations.len(),
                message: Some(format!(
                    "Would fix {} Q-2-7 unclosed single quote violation(s)",
                    violations.len()
                )),
            });
        }

        if in_place {
            crate::utils::file_io::write_file(file_path, &fixed_content)?;
            Ok(ConvertResult {
                rule_name: self.name().to_string(),
                file_path: file_path.to_string_lossy().to_string(),
                fixes_applied: violations.len(),
                message: Some(format!(
                    "Fixed {} Q-2-7 unclosed single quote violation(s)",
                    violations.len()
                )),
            })
        } else {
            Ok(ConvertResult {
                rule_name: self.name().to_string(),
                file_path: file_path.to_string_lossy().to_string(),
                fixes_applied: violations.len(),
                message: Some(fixed_content),
            })
        }
    }
}
