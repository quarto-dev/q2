use anyhow::Result;
use std::path::Path;

use crate::rule::{CheckResult, ConvertResult, Rule};
use crate::utils::parse_probe::probe_file;

pub struct ParseChecker {}

impl ParseChecker {
    pub fn new() -> Result<Self> {
        Ok(Self {})
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
        match probe_file(file_path)? {
            None => Ok(vec![]),
            Some(failure) => {
                let message = if failure.error_count == 1 {
                    "File failed to parse (1 error)".to_string()
                } else {
                    format!("File failed to parse ({} errors)", failure.error_count)
                };

                Ok(vec![CheckResult {
                    rule_name: self.name().to_string(),
                    file_path: file_path.to_string_lossy().to_string(),
                    has_issue: true,
                    issue_count: failure.error_count,
                    message: Some(message),
                    location: None, // Parse errors don't have a single location
                    error_code: failure.error_codes.first().cloned(),
                    error_codes: if failure.error_codes.is_empty() {
                        None
                    } else {
                        Some(failure.error_codes)
                    },
                    ..Default::default()
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
