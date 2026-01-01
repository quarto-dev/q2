use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::rule::{CheckResult, ConvertResult, Rule, SourceLocation};
use crate::utils::file_io::read_file;

pub struct ApostropheQuotesConverter {}

#[derive(Debug, Clone)]
struct ApostropheViolation {
    offset: usize,                          // Offset of the apostrophe character
    error_location: Option<SourceLocation>, // For reporting
}

impl ApostropheQuotesConverter {
    pub fn new() -> Result<Self> {
        Ok(Self {})
    }

    /// Get parse errors and extract Q-2-10 apostrophe violations
    fn get_apostrophe_violations(&self, file_path: &Path) -> Result<Vec<ApostropheViolation>> {
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
            true,
            None,
        );

        let diagnostics = match result {
            Ok(_) => return Ok(Vec::new()), // No errors
            Err(diagnostics) => diagnostics,
        };

        let mut violations = Vec::new();

        for diagnostic in diagnostics {
            // Check if this is a Q-2-10 error
            if diagnostic.code.as_deref() != Some("Q-2-10") {
                continue;
            }

            // Extract location
            let location = diagnostic.location.as_ref();
            if location.is_none() {
                continue;
            }

            let offset = location.as_ref().unwrap().start_offset();

            violations.push(ApostropheViolation {
                offset,
                error_location: Some(SourceLocation {
                    row: self.offset_to_row(&content, offset),
                    column: self.offset_to_column(&content, offset),
                }),
            });
        }

        Ok(violations)
    }

    /// Apply fixes to the content by inserting backslashes before apostrophes
    fn apply_fixes(
        &self,
        content: &str,
        mut violations: Vec<ApostropheViolation>,
    ) -> Result<String> {
        if violations.is_empty() {
            return Ok(content.to_string());
        }

        // Sort violations in reverse order to avoid offset invalidation
        violations.sort_by_key(|v| std::cmp::Reverse(v.offset));

        let mut result = content.to_string();

        for violation in violations {
            // The offset points to the space after the apostrophe,
            // so we need to insert the backslash at offset-1 (before the apostrophe)
            result.insert(violation.offset - 1, '\\');
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

impl Rule for ApostropheQuotesConverter {
    fn name(&self) -> &str {
        "apostrophe-quotes"
    }

    fn description(&self) -> &str {
        "Fix Q-2-10: Escape apostrophes misinterpreted as quote closes"
    }

    fn check(&self, file_path: &Path, _verbose: bool) -> Result<Vec<CheckResult>> {
        let violations = self.get_apostrophe_violations(file_path)?;

        let results: Vec<CheckResult> = violations
            .into_iter()
            .map(|v| CheckResult {
                rule_name: self.name().to_string(),
                file_path: file_path.to_string_lossy().to_string(),
                has_issue: true,
                issue_count: 1,
                message: Some(format!(
                    "Q-2-10 apostrophe violation at offset {}",
                    v.offset
                )),
                location: v.error_location,
                error_code: Some("Q-2-10".to_string()),
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
        let violations = self.get_apostrophe_violations(file_path)?;

        if violations.is_empty() {
            return Ok(ConvertResult {
                rule_name: self.name().to_string(),
                file_path: file_path.to_string_lossy().to_string(),
                fixes_applied: 0,
                message: Some("No Q-2-10 apostrophe issues found".to_string()),
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
                    "Would fix {} Q-2-10 apostrophe violation(s)",
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
                    "Fixed {} Q-2-10 apostrophe violation(s)",
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
