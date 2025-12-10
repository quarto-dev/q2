// Q-2-5: Unclosed Underscore Emphasis
//
// This conversion rule fixes Q-2-5 errors by adding closing '_' marks
// where they are missing at the end of blocks.
//
// Error catalog entry: crates/quarto-error-reporting/error_catalog.json
// Error code: Q-2-5
// Title: "Unclosed Underscore Emphasis"
// Message: "I reached the end of the block before finding a closing '_' for the emphasis."
//
// Example:
//   Input:  _This is an unclosed emphasis
//   Output: _This is an unclosed emphasis_
//

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::rule::{CheckResult, ConvertResult, Rule, SourceLocation};
use crate::utils::file_io::read_file;

pub struct Q25Converter {}

#[derive(Debug, Clone)]
struct Q25Violation {
    offset: usize,
    error_location: Option<SourceLocation>,
}

impl Q25Converter {
    pub fn new() -> Result<Self> {
        Ok(Self {})
    }

    fn get_violations(&self, file_path: &Path) -> Result<Vec<Q25Violation>> {
        let content = fs::read_to_string(file_path)
            .with_context(|| format!("Failed to read file: {}", file_path.display()))?;

        let mut sink = std::io::sink();
        let filename = file_path.to_string_lossy();

        let result =
            pampa::readers::qmd::read(content.as_bytes(), false, &filename, &mut sink, true, None);

        let diagnostics = match result {
            Ok(_) => return Ok(Vec::new()),
            Err(diagnostics) => diagnostics,
        };

        let mut violations = Vec::new();

        for diagnostic in diagnostics {
            if diagnostic.code.as_deref() != Some("Q-2-5") {
                continue;
            }

            let location = diagnostic.location.as_ref();
            if location.is_none() {
                continue;
            }

            let offset = location.as_ref().unwrap().start_offset();

            violations.push(Q25Violation {
                offset,
                error_location: Some(SourceLocation {
                    row: self.offset_to_row(&content, offset),
                    column: self.offset_to_column(&content, offset),
                }),
            });
        }

        Ok(violations)
    }

    fn apply_fixes(&self, content: &str, mut violations: Vec<Q25Violation>) -> Result<String> {
        if violations.is_empty() {
            return Ok(content.to_string());
        }

        violations.sort_by_key(|v| std::cmp::Reverse(v.offset));

        let mut result = content.to_string();

        for violation in violations {
            result.insert(violation.offset, '_');
        }

        Ok(result)
    }

    fn offset_to_row(&self, content: &str, offset: usize) -> usize {
        content[..offset].matches('\n').count()
    }

    fn offset_to_column(&self, content: &str, offset: usize) -> usize {
        let line_start = content[..offset]
            .rfind('\n')
            .map(|pos| pos + 1)
            .unwrap_or(0);
        offset - line_start
    }
}

impl Rule for Q25Converter {
    fn name(&self) -> &str {
        "q-2-5"
    }

    fn description(&self) -> &str {
        "Fix Q-2-5: Add closing '_' for unclosed underscore emphasis"
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
                    "Q-2-5 unclosed underscore emphasis at offset {}",
                    v.offset
                )),
                location: v.error_location,
                error_code: Some("Q-2-5".to_string()),
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
                message: Some("No Q-2-5 unclosed underscore emphasis issues found".to_string()),
            });
        }

        let fixed_content = self.apply_fixes(&content, violations.clone())?;

        if check_mode {
            return Ok(ConvertResult {
                rule_name: self.name().to_string(),
                file_path: file_path.to_string_lossy().to_string(),
                fixes_applied: violations.len(),
                message: Some(format!(
                    "Would fix {} Q-2-5 unclosed underscore emphasis violation(s)",
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
                    "Fixed {} Q-2-5 unclosed underscore emphasis violation(s)",
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
