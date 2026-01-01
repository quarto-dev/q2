// Q-2-17: Unclosed Subscript
//
// This conversion rule fixes Q-2-17 errors by adding closing subscript marks
// where they are missing at the end of blocks.
//
// Error catalog entry: crates/quarto-error-reporting/error_catalog.json
// Error code: Q-2-17
// Title: "Unclosed Subscript"
// Message: "I reached the end of the block before finding a closing '~' for the subscript."
//
// Example:
//   Input:  H~2O and CO~2
//   Output: H~2~O and CO~2~
//

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::rule::{CheckResult, ConvertResult, Rule, SourceLocation};
use crate::utils::file_io::read_file;

pub struct Q217Converter {}

#[derive(Debug, Clone)]
struct Q217Violation {
    offset: usize,                          // Offset where closing ~ should be added
    error_location: Option<SourceLocation>, // For reporting
}

impl Q217Converter {
    pub fn new() -> Result<Self> {
        Ok(Self {})
    }

    /// Get parse errors and extract Q-2-17 unclosed subscript violations
    fn get_violations(&self, file_path: &Path) -> Result<Vec<Q217Violation>> {
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
            // Check if this is a Q-2-17 error
            if diagnostic.code.as_deref() != Some("Q-2-17") {
                continue;
            }

            // Extract location - this points to the end of the block
            let location = diagnostic.location.as_ref();
            if location.is_none() {
                continue;
            }

            let offset = location.as_ref().unwrap().start_offset();

            violations.push(Q217Violation {
                offset,
                error_location: Some(SourceLocation {
                    row: self.offset_to_row(&content, offset),
                    column: self.offset_to_column(&content, offset),
                }),
            });
        }

        Ok(violations)
    }

    /// Apply fixes to the content by adding closing subscript marks
    fn apply_fixes(&self, content: &str, mut violations: Vec<Q217Violation>) -> Result<String> {
        if violations.is_empty() {
            return Ok(content.to_string());
        }

        // Sort violations in reverse order to avoid offset invalidation
        violations.sort_by_key(|v| std::cmp::Reverse(v.offset));

        let mut result = content.to_string();

        for violation in violations {
            // Insert closing ~ at the error location (end of block)
            result.insert(violation.offset, '~');
        }

        Ok(result)
    }

    /// Convert byte offset to row number (0-indexed)
    fn offset_to_row(&self, content: &str, offset: usize) -> usize {
        content[..offset].matches('\n').count()
    }

    /// Convert byte offset to column number (0-indexed)
    fn offset_to_column(&self, content: &str, offset: usize) -> usize {
        let line_start = content[..offset].rfind('\n').map_or(0, |pos| pos + 1);
        offset - line_start
    }
}

impl Rule for Q217Converter {
    fn name(&self) -> &str {
        "q-2-17"
    }

    fn description(&self) -> &str {
        "Fix Q-2-17: Add closing subscript marks for unclosed subscripts"
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
                message: Some(format!("Q-2-17 unclosed subscript at offset {}", v.offset)),
                location: v.error_location,
                error_code: Some("Q-2-17".to_string()),
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
                message: Some("No Q-2-17 unclosed subscript issues found".to_string()),
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
                    "Would fix {} Q-2-17 unclosed subscript violation(s)",
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
                    "Fixed {} Q-2-17 unclosed subscript violation(s)",
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
