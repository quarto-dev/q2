use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::rule::{CheckResult, ConvertResult, Rule};

pub struct ParseChecker {}

impl ParseChecker {
    pub fn new() -> Result<Self> {
        Ok(Self {})
    }

    /// Check if a file parses successfully and return diagnostic messages if it fails
    fn check_parse(
        &self,
        file_path: &Path,
    ) -> Result<Option<Vec<quarto_error_reporting::DiagnosticMessage>>> {
        let content = fs::read_to_string(file_path)
            .with_context(|| format!("Failed to read file: {}", file_path.display()))?;

        let mut sink = std::io::sink();
        let filename = file_path.to_string_lossy();

        let result = quarto_markdown_pandoc::readers::qmd::read(
            content.as_bytes(),
            false,
            &filename,
            &mut sink,
            true,
            None,
        );

        match result {
            Ok(_) => Ok(None),
            Err(diagnostics) => Ok(Some(diagnostics)),
        }
    }
}

impl Rule for ParseChecker {
    fn name(&self) -> &str {
        "parse"
    }

    fn description(&self) -> &str {
        "Check if file parses successfully"
    }

    fn check(&self, file_path: &Path, _verbose: bool) -> Result<Vec<CheckResult>> {
        let diagnostics = self.check_parse(file_path)?;

        match diagnostics {
            None => Ok(vec![]),
            Some(diags) => {
                // Extract error codes from all diagnostics
                let error_codes: Vec<String> =
                    diags.iter().filter_map(|d| d.code.clone()).collect();

                // Create a message that includes information about all errors
                let message = if diags.len() == 1 {
                    "File failed to parse (1 error)".to_string()
                } else {
                    format!("File failed to parse ({} errors)", diags.len())
                };

                // Use the first error code as the primary one, or None if no codes
                let primary_error_code = error_codes.first().cloned();

                Ok(vec![CheckResult {
                    rule_name: self.name().to_string(),
                    file_path: file_path.to_string_lossy().to_string(),
                    has_issue: true,
                    issue_count: diags.len(),
                    message: Some(message),
                    location: None, // Parse errors don't have a single location
                    error_code: primary_error_code,
                    error_codes: if error_codes.is_empty() {
                        None
                    } else {
                        Some(error_codes)
                    },
                }])
            }
        }
    }

    fn convert(
        &self,
        file_path: &Path,
        _in_place: bool,
        _check_mode: bool,
        _verbose: bool,
    ) -> Result<ConvertResult> {
        // Parse errors can't be auto-fixed
        Ok(ConvertResult {
            rule_name: self.name().to_string(),
            file_path: file_path.to_string_lossy().to_string(),
            fixes_applied: 0,
            message: Some("Parse errors cannot be automatically fixed".to_string()),
        })
    }
}
