use anyhow::{Context, Result};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub file: PathBuf,
    pub success: bool,
    pub error_message: Option<String>,
}

pub struct SyntaxChecker {
    pub results: Vec<CheckResult>,
}

impl SyntaxChecker {
    pub fn new() -> Self {
        Self {
            results: Vec::new(),
        }
    }

    /// Check a single file by attempting to parse it
    pub fn check_file(&mut self, file_path: &Path, verbose: bool) -> Result<()> {
        if verbose {
            print!("Checking: {} ... ", file_path.display());
        }

        let result = self.parse_file(file_path);

        match &result {
            Ok(_) => {
                if verbose {
                    println!("{}", "✓".green());
                }
                self.results.push(CheckResult {
                    file: file_path.to_path_buf(),
                    success: true,
                    error_message: None,
                });
            }
            Err(e) => {
                if verbose {
                    println!("{}", "✗".red());
                    println!("  Error: {}", e);
                }
                self.results.push(CheckResult {
                    file: file_path.to_path_buf(),
                    success: false,
                    error_message: Some(e.to_string()),
                });
            }
        }

        Ok(())
    }

    /// Parse a file using pampa
    fn parse_file(&self, file_path: &Path) -> Result<()> {
        let content = fs::read_to_string(file_path)
            .with_context(|| format!("Failed to read file: {}", file_path.display()))?;

        // Use the pampa library to parse
        let mut sink = std::io::sink();
        let filename = file_path.to_string_lossy();

        let result = pampa::readers::qmd::read(
            content.as_bytes(),
            false, // not loose mode
            &filename,
            &mut sink,
            Some(
                pampa::readers::qmd_error_messages::produce_json_error_messages
                    as fn(
                        &[u8],
                        &pampa::utils::tree_sitter_log_observer::TreeSitterLogObserver,
                        &str,
                    ) -> Vec<String>,
            ), // Use JSON error formatter for machine-readable errors
        );

        match result {
            Ok(_) => Ok(()),
            Err(errors) => {
                // Join error messages
                let error_msg = errors.join("\n");
                Err(anyhow::anyhow!("{}", error_msg))
            }
        }
    }

    /// Print a summary of the results
    pub fn print_summary(&self) {
        let total = self.results.len();
        let successes = self.results.iter().filter(|r| r.success).count();
        let failures = total - successes;

        println!("\n{}", "=== Summary ===".bold());
        println!("Total files:    {}", total);
        println!("Successful:     {} {}", successes, "✓".green());
        println!(
            "Failed:         {} {}",
            failures,
            if failures > 0 {
                "✗".red()
            } else {
                "✓".green()
            }
        );

        if failures > 0 {
            let success_rate = (successes as f64 / total as f64) * 100.0;
            println!("Success rate:   {:.1}%", success_rate);
        }
    }

    /// Get a list of failed files
    pub fn failed_files(&self) -> Vec<&CheckResult> {
        self.results.iter().filter(|r| !r.success).collect()
    }

    /// Export results as JSONL
    pub fn export_jsonl(&self, output_path: &Path) -> Result<()> {
        let mut output = String::new();
        for result in &self.results {
            let json = serde_json::to_string(result)?;
            output.push_str(&json);
            output.push('\n');
        }

        fs::write(output_path, output)
            .with_context(|| format!("Failed to write to: {}", output_path.display()))?;

        Ok(())
    }
}
