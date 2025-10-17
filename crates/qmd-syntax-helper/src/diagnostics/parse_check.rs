use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::rule::{CheckResult, ConvertResult, Rule};

pub struct ParseChecker {}

impl ParseChecker {
    pub fn new() -> Result<Self> {
        Ok(Self {})
    }

    /// Check if a file parses successfully
    fn check_parse(&self, file_path: &Path) -> Result<bool> {
        let content = fs::read_to_string(file_path)
            .with_context(|| format!("Failed to read file: {}", file_path.display()))?;

        let mut sink = std::io::sink();
        let filename = file_path.to_string_lossy();

        let result = quarto_markdown_pandoc::readers::qmd::read(
            content.as_bytes(),
            false,
            &filename,
            &mut sink,
            Some(
                quarto_markdown_pandoc::readers::qmd_error_messages::produce_json_error_messages
                    as fn(
                        &[u8],
                        &quarto_markdown_pandoc::utils::tree_sitter_log_observer::TreeSitterLogObserver,
                        &str,
                    ) -> Vec<String>,
            ),
        );

        Ok(result.is_ok())
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
        let parses = self.check_parse(file_path)?;

        if parses {
            Ok(vec![])
        } else {
            Ok(vec![CheckResult {
                rule_name: self.name().to_string(),
                file_path: file_path.to_string_lossy().to_string(),
                has_issue: true,
                issue_count: 1,
                message: Some("File failed to parse".to_string()),
                location: None, // Parse errors don't have specific locations
            }])
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
