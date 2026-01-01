use anyhow::{Context, Result, anyhow};
use regex::Regex;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::rule::{CheckResult, ConvertResult, Rule, SourceLocation};
use crate::utils::file_io::read_file;

pub struct AttributeOrderingConverter {
    // Regex for extracting normalized attributes from Pandoc output
    pandoc_output_regex: Regex,
}

#[derive(Debug, Clone)]
struct AttributeOrderingViolation {
    start_offset: usize,                    // Offset of '{'
    end_offset: usize,                      // Offset of '}' + 1
    original: String,                       // Original attrs including braces
    error_location: Option<SourceLocation>, // For reporting
}

impl AttributeOrderingConverter {
    pub fn new() -> Result<Self> {
        let pandoc_output_regex =
            Regex::new(r"^\[\]\{(.+)\}\s*$").context("Failed to compile pandoc output regex")?;

        Ok(Self {
            pandoc_output_regex,
        })
    }

    /// Get parse errors and extract attribute ordering violations
    fn get_attribute_ordering_errors(
        &self,
        file_path: &Path,
    ) -> Result<Vec<AttributeOrderingViolation>> {
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
            // Check if this is an attribute ordering error
            if diagnostic.title != "Key-value Pair Before Class Specifier in Attribute" {
                continue;
            }

            // Extract location
            let location = diagnostic.location.as_ref();
            if location.is_none() {
                continue;
            }

            let start_offset = location.as_ref().unwrap().start_offset();

            // Find the full attribute block
            match self.find_attribute_block(&content, start_offset) {
                Ok((block_start, block_end)) => {
                    let original = content[block_start..block_end].to_string();

                    violations.push(AttributeOrderingViolation {
                        start_offset: block_start,
                        end_offset: block_end,
                        original,
                        error_location: Some(SourceLocation {
                            row: self.offset_to_row(&content, start_offset),
                            column: self.offset_to_column(&content, start_offset),
                        }),
                    });
                }
                Err(e) => {
                    eprintln!("Warning: Could not locate attribute block: {}", e);
                }
            }
        }

        Ok(violations)
    }

    /// Find the full attribute block given an error location
    fn find_attribute_block(&self, content: &str, error_offset: usize) -> Result<(usize, usize)> {
        let bytes = content.as_bytes();

        if error_offset >= bytes.len() {
            return Err(anyhow!(
                "Error offset {} is beyond content length {}",
                error_offset,
                bytes.len()
            ));
        }

        // Search backward for '{'
        let mut start = error_offset;
        while start > 0 && bytes[start] != b'{' {
            start -= 1;
        }
        if bytes[start] != b'{' {
            return Err(anyhow!(
                "Could not find opening brace before offset {}",
                error_offset
            ));
        }

        // Search forward for '}'
        let mut end = error_offset;
        while end < bytes.len() && bytes[end] != b'}' {
            end += 1;
        }
        if end >= bytes.len() || bytes[end] != b'}' {
            return Err(anyhow!(
                "Could not find closing brace after offset {}",
                error_offset
            ));
        }

        Ok((start, end + 1)) // +1 to include the '}'
    }

    /// Normalize attributes using Pandoc
    fn normalize_with_pandoc(&self, attrs: &str) -> Result<String> {
        // Create input: []{ + attrs_content + }
        // attrs is already "{...}" so wrap with []
        let input = format!("[]{}", attrs);

        // Run pandoc
        let mut child = Command::new("pandoc")
            .arg("-t")
            .arg("markdown")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to spawn pandoc. Is pandoc installed?")?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(input.as_bytes())
                .context("Failed to write to pandoc stdin")?;
        }

        let output = child
            .wait_with_output()
            .context("Failed to wait for pandoc")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("Pandoc failed: {}", stderr));
        }

        let stdout =
            String::from_utf8(output.stdout).context("Pandoc output is not valid UTF-8")?;

        // Extract normalized attrs from "[]{...}"
        if let Some(caps) = self.pandoc_output_regex.captures(stdout.trim()) {
            Ok(format!("{{{}}}", &caps[1]))
        } else {
            Err(anyhow!("Unexpected pandoc output: {}", stdout))
        }
    }

    /// Apply fixes to the content
    fn apply_fixes(
        &self,
        content: &str,
        mut violations: Vec<AttributeOrderingViolation>,
    ) -> Result<String> {
        if violations.is_empty() {
            return Ok(content.to_string());
        }

        // Sort violations in reverse order to avoid offset invalidation
        violations.sort_by_key(|v| std::cmp::Reverse(v.start_offset));

        let mut result = content.to_string();

        for violation in violations {
            let normalized = self
                .normalize_with_pandoc(&violation.original)
                .with_context(|| {
                    format!("Failed to normalize attributes: {}", violation.original)
                })?;

            // Replace original with normalized
            result.replace_range(violation.start_offset..violation.end_offset, &normalized);
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

impl Rule for AttributeOrderingConverter {
    fn name(&self) -> &str {
        "attribute-ordering"
    }

    fn description(&self) -> &str {
        "Fix attribute ordering: reorder {key=value .class #id} to {#id .class key=\"value\"}"
    }

    fn check(&self, file_path: &Path, _verbose: bool) -> Result<Vec<CheckResult>> {
        let violations = self.get_attribute_ordering_errors(file_path)?;

        let results: Vec<CheckResult> = violations
            .into_iter()
            .map(|v| CheckResult {
                rule_name: self.name().to_string(),
                file_path: file_path.to_string_lossy().to_string(),
                has_issue: true,
                issue_count: 1,
                message: Some(format!("Attribute ordering violation: {}", v.original)),
                location: v.error_location,
                error_code: None,
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
        let violations = self.get_attribute_ordering_errors(file_path)?;

        if violations.is_empty() {
            return Ok(ConvertResult {
                rule_name: self.name().to_string(),
                file_path: file_path.to_string_lossy().to_string(),
                fixes_applied: 0,
                message: Some("No attribute ordering issues found".to_string()),
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
                    "Would fix {} attribute ordering violation(s)",
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
                    "Fixed {} attribute ordering violation(s)",
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
