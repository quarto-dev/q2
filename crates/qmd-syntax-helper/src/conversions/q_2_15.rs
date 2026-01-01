// Q-2-15: Unclosed Strong Underscore Emphasis
//
// Error catalog entry: crates/quarto-error-reporting/error_catalog.json
// Error code: Q-2-15
// Title: "Unclosed Strong Underscore Emphasis"
// Message: "I reached the end of the block before finding a closing '__' for the strong emphasis."

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::rule::{CheckResult, ConvertResult, Rule, SourceLocation};
use crate::utils::file_io::read_file;

pub struct Q215Converter {}

#[derive(Debug, Clone)]
struct Q215Violation {
    offset: usize,
    error_location: Option<SourceLocation>,
}

impl Q215Converter {
    pub fn new() -> Result<Self> {
        Ok(Self {})
    }

    fn get_violations(&self, file_path: &Path) -> Result<Vec<Q215Violation>> {
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
            if diagnostic.code.as_deref() != Some("Q-2-15") {
                continue;
            }

            let location = diagnostic.location.as_ref();
            if location.is_none() {
                continue;
            }

            let offset = location.as_ref().unwrap().start_offset();

            violations.push(Q215Violation {
                offset,
                error_location: Some(SourceLocation {
                    row: self.offset_to_row(&content, offset),
                    column: self.offset_to_column(&content, offset),
                }),
            });
        }

        Ok(violations)
    }

    fn apply_fixes(&self, content: &str, mut violations: Vec<Q215Violation>) -> Result<String> {
        if violations.is_empty() {
            return Ok(content.to_string());
        }

        violations.sort_by_key(|v| std::cmp::Reverse(v.offset));

        let mut result = content.to_string();

        for violation in violations {
            result.insert_str(violation.offset, "__");
        }

        Ok(result)
    }

    fn offset_to_row(&self, content: &str, offset: usize) -> usize {
        content[..offset].matches('\n').count()
    }

    fn offset_to_column(&self, content: &str, offset: usize) -> usize {
        let line_start = content[..offset].rfind('\n').map_or(0, |pos| pos + 1);
        offset - line_start
    }
}

impl Rule for Q215Converter {
    fn name(&self) -> &str {
        "q-2-15"
    }

    fn description(&self) -> &str {
        "Fix Q-2-15: Add closing '__' for unclosed strong underscore emphasis"
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
                    "Q-2-15 unclosed strong underscore emphasis at offset {}",
                    v.offset
                )),
                location: v.error_location,
                error_code: Some("Q-2-15".to_string()),
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
                message: Some(
                    "No Q-2-15 unclosed strong underscore emphasis issues found".to_string(),
                ),
            });
        }

        let fixed_content = self.apply_fixes(&content, violations.clone())?;

        if check_mode {
            return Ok(ConvertResult {
                rule_name: self.name().to_string(),
                file_path: file_path.to_string_lossy().to_string(),
                fixes_applied: violations.len(),
                message: Some(format!(
                    "Would fix {} Q-2-15 unclosed strong underscore emphasis violation(s)",
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
                    "Fixed {} Q-2-15 unclosed strong underscore emphasis violation(s)",
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
