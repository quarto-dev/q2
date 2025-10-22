use anyhow::{Context, Result};
use colored::Colorize;
use std::fs;
use std::path::Path;

use crate::rule::{CheckResult, ConvertResult, Rule};
use crate::utils::file_io::{read_file, write_file};

pub struct DivWhitespaceConverter {}

impl DivWhitespaceConverter {
    pub fn new() -> Result<Self> {
        Ok(Self {})
    }

    /// Parse a file and get diagnostic messages
    fn get_parse_errors(
        &self,
        file_path: &Path,
    ) -> Result<Vec<quarto_error_reporting::DiagnosticMessage>> {
        let content = fs::read_to_string(file_path)
            .with_context(|| format!("Failed to read file: {}", file_path.display()))?;

        // Use the quarto-markdown-pandoc library to parse with JSON error formatter
        let mut sink = std::io::sink();
        let filename = file_path.to_string_lossy();

        let result = quarto_markdown_pandoc::readers::qmd::read(
            content.as_bytes(),
            false, // not loose mode
            &filename,
            &mut sink,
        );

        match result {
            Ok(_) => Ok(Vec::new()), // No errors
            Err(diagnostics) => {
                // Return diagnostic messages directly
                Ok(diagnostics)
            }
        }
    }

    /// Find div fence errors that need whitespace fixes
    fn find_div_whitespace_errors(
        &self,
        content: &str,
        errors: &[quarto_error_reporting::DiagnosticMessage],
    ) -> Vec<usize> {
        let mut fix_positions = Vec::new();
        let lines: Vec<&str> = content.lines().collect();

        // Pre-compute line start offsets for O(1) lookup instead of O(N) per error
        let mut line_starts = Vec::with_capacity(lines.len());
        let mut offset = 0;
        for line in &lines {
            line_starts.push(offset);
            offset += line.len() + 1; // +1 for newline
        }

        for error in errors {
            // Skip errors that are not about div fences
            // We're looking for "Missing Space After Div Fence" or errors on lines with :::
            let is_div_error = error.title.contains("Div Fence") || error.title == "Parse error";

            if !is_div_error {
                continue;
            }

            // Extract row from location (if available)
            // SourceInfo uses 0-indexed rows, div_whitespace uses them too
            let error_row = error
                .location
                .as_ref()
                .map(|loc| loc.range.start.row)
                .unwrap_or(0);

            // The error might be on the line itself or the line before (for div fences)
            // Check both the current line and the previous line
            let lines_to_check = if error_row > 0 {
                vec![error_row - 1, error_row]
            } else {
                vec![error_row]
            };

            for &line_idx in &lines_to_check {
                if line_idx >= lines.len() {
                    continue;
                }

                let line = lines[line_idx];

                // Check if this line starts with ::: followed immediately by {
                let trimmed = line.trim_start();
                if let Some(after_colon) = trimmed.strip_prefix(":::") {
                    if after_colon.starts_with('{') {
                        // Calculate the position right after :::
                        // We need byte offset, not char offset
                        // Use pre-computed offset for O(1) lookup
                        let line_start = line_starts[line_idx];

                        let indent_bytes = line.len() - trimmed.len();
                        let fix_pos = line_start + indent_bytes + 3; // +3 for ":::"

                        fix_positions.push(fix_pos);
                        break; // Found it, no need to check other lines for this error
                    }
                }
            }
        }

        // Remove duplicates and sort
        fix_positions.sort_unstable();
        fix_positions.dedup();

        fix_positions
    }

    /// Convert byte offset to row/column (1-indexed)
    fn byte_offset_to_location(
        &self,
        content: &str,
        byte_offset: usize,
    ) -> crate::rule::SourceLocation {
        let mut row = 1;
        let mut column = 1;
        let mut current_offset = 0;

        for ch in content.chars() {
            if current_offset >= byte_offset {
                break;
            }
            current_offset += ch.len_utf8();

            if ch == '\n' {
                row += 1;
                column = 1;
            } else {
                column += 1;
            }
        }

        crate::rule::SourceLocation { row, column }
    }

    /// Apply fixes to content by inserting spaces at specified positions
    fn apply_fixes(&self, content: &str, fix_positions: &[usize]) -> String {
        let mut result = String::with_capacity(content.len() + fix_positions.len());
        let mut last_pos = 0;

        for &pos in fix_positions {
            // Copy content up to this position
            result.push_str(&content[last_pos..pos]);
            // Insert a space
            result.push(' ');
            last_pos = pos;
        }

        // Copy remaining content
        result.push_str(&content[last_pos..]);

        result
    }

    /// Process a single file
    #[allow(dead_code)]
    pub fn process_file(
        &self,
        file_path: &Path,
        in_place: bool,
        check: bool,
        verbose: bool,
    ) -> Result<()> {
        let content = read_file(file_path)?;

        // Get parse errors
        let errors = self.get_parse_errors(file_path)?;

        if errors.is_empty() {
            if verbose {
                println!("  No div whitespace issues found");
            }
            return Ok(());
        }

        // Find positions that need fixes
        let fix_positions = self.find_div_whitespace_errors(&content, &errors);

        if fix_positions.is_empty() {
            if verbose {
                println!("  No div whitespace issues found");
            }
            return Ok(());
        }

        if verbose || check {
            println!(
                "  Found {} div fence(s) needing whitespace fixes",
                fix_positions.len().to_string().yellow()
            );
        }

        if check {
            println!("  {} No changes written (--check mode)", "✓".green());
            return Ok(());
        }

        // Apply fixes
        let new_content = self.apply_fixes(&content, &fix_positions);

        if in_place {
            write_file(file_path, &new_content)?;
            println!(
                "  {} Fixed {} div fence(s)",
                "✓".green(),
                fix_positions.len()
            );
        } else {
            // Output to stdout
            print!("{}", new_content);
        }

        Ok(())
    }
}

impl Rule for DivWhitespaceConverter {
    fn name(&self) -> &str {
        "div-whitespace"
    }

    fn description(&self) -> &str {
        "Fix div fences missing whitespace (:::{ -> ::: {)"
    }

    fn check(&self, file_path: &Path, verbose: bool) -> Result<Vec<CheckResult>> {
        let content = read_file(file_path)?;
        let errors = self.get_parse_errors(file_path)?;
        let fix_positions = self.find_div_whitespace_errors(&content, &errors);

        if verbose {
            if fix_positions.is_empty() {
                println!("  No div whitespace issues found");
            } else {
                println!(
                    "  Found {} div fence(s) needing whitespace fixes",
                    fix_positions.len()
                );
            }
        }

        let mut results = Vec::new();
        for &pos in &fix_positions {
            let location = self.byte_offset_to_location(&content, pos);
            results.push(CheckResult {
                rule_name: self.name().to_string(),
                file_path: file_path.to_string_lossy().to_string(),
                has_issue: true,
                issue_count: 1,
                message: Some("Div fence missing whitespace (:::{ should be ::: {)".to_string()),
                location: Some(location),
            });
        }

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
        let errors = self.get_parse_errors(file_path)?;
        let fix_positions = self.find_div_whitespace_errors(&content, &errors);

        if fix_positions.is_empty() {
            return Ok(ConvertResult {
                rule_name: self.name().to_string(),
                file_path: file_path.to_string_lossy().to_string(),
                fixes_applied: 0,
                message: None,
            });
        }

        let new_content = self.apply_fixes(&content, &fix_positions);

        if !check_mode {
            if in_place {
                write_file(file_path, &new_content)?;
            }
        }

        Ok(ConvertResult {
            rule_name: self.name().to_string(),
            file_path: file_path.to_string_lossy().to_string(),
            fixes_applied: fix_positions.len(),
            message: if in_place {
                Some(format!("Fixed {} div fence(s)", fix_positions.len()))
            } else {
                Some(new_content)
            },
        })
    }
}
