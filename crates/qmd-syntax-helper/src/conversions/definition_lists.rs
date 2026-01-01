use anyhow::{Context, Result};
use colored::Colorize;
use regex::Regex;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::rule::{CheckResult, ConvertResult, Rule};
use crate::utils::file_io::{read_file, write_file};
use crate::utils::resources::ResourceManager;
use pampa::readers::json;
use pampa::writers::qmd;

pub struct DefinitionListConverter {
    def_item_regex: Regex,
    resources: ResourceManager,
}

#[derive(Debug)]
pub struct DefinitionList {
    pub text: String,
    pub start_line: usize,
    pub end_line: usize,
}

impl DefinitionListConverter {
    pub fn new() -> Result<Self> {
        Ok(Self {
            // Matches definition list items that start with `:` followed by spaces
            def_item_regex: Regex::new(r"^:\s+").unwrap(),
            resources: ResourceManager::new()?,
        })
    }

    /// Find all definition lists in the content
    pub fn find_definition_lists(&self, content: &str) -> Vec<DefinitionList> {
        let lines: Vec<&str> = content.lines().collect();
        let mut lists = Vec::new();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i];

            // Look for a definition item (line starting with `:   `)
            // But not div fences (`::`or `:::`)
            if self.def_item_regex.is_match(line) && !line.starts_with("::") {
                // Found a definition item, now scan backwards to find the term
                let mut start_idx = i;

                // Skip back over any blank lines
                while start_idx > 0 && lines[start_idx - 1].trim().is_empty() {
                    start_idx -= 1;
                }

                // The line before the blank lines should be the term
                start_idx = start_idx.saturating_sub(1);

                // Check if the "term" is actually a table row or grid table border
                // Table rows contain multiple pipe characters (e.g., | cell | cell |)
                // Grid table borders start with + and contain multiple + characters
                // This helps distinguish table captions from definition lists
                let has_pipes = lines[start_idx].matches('|').count() >= 2;
                let is_grid_border =
                    lines[start_idx].starts_with('+') && lines[start_idx].matches('+').count() >= 2;

                if has_pipes || is_grid_border {
                    // This is likely a table caption, not a definition list
                    i += 1;
                    continue;
                }

                // Now scan forward to collect all terms and definitions in this list
                let mut end_idx = i;
                i += 1;

                loop {
                    // Continue through continuation lines and blank lines
                    while i < lines.len() {
                        let line = lines[i];
                        if line.starts_with("    ") || line.trim().is_empty() {
                            end_idx = i;
                            i += 1;
                        } else {
                            break;
                        }
                    }

                    // Check if the next item is part of this definition list
                    // It should be: optional non-blank line (term), then blank lines, then `:   `
                    if i < lines.len() {
                        let potential_term = lines[i];

                        // Not a definition line, might be next term
                        if !self.def_item_regex.is_match(potential_term)
                            || potential_term.starts_with("::")
                        {
                            // Look ahead for a definition line
                            let mut j = i + 1;
                            while j < lines.len() && lines[j].trim().is_empty() {
                                j += 1;
                            }

                            if j < lines.len()
                                && self.def_item_regex.is_match(lines[j])
                                && !lines[j].starts_with("::")
                            {
                                // Found another term-definition pair
                                end_idx = j;
                                i = j + 1;
                            } else {
                                // No more definition items
                                break;
                            }
                        } else {
                            // This IS a definition line (continuation of same term)
                            end_idx = i;
                            i += 1;
                        }
                    } else {
                        break;
                    }
                }

                // Extract the definition list text
                let list_lines = &lines[start_idx..=end_idx];
                let list_text = list_lines.join("\n");

                lists.push(DefinitionList {
                    text: list_text,
                    start_line: start_idx,
                    end_line: end_idx,
                });
            } else {
                i += 1;
            }
        }

        lists
    }

    /// Convert a single definition list by:
    /// 1. Running pandoc with the Lua filter to convert to JSON
    /// 2. Using pampa library to convert JSON to markdown
    pub fn convert_list(&self, list_text: &str) -> Result<String> {
        use std::io::Write;

        // Get the Lua filter path from resources
        let filter_path = self
            .resources
            .get_resource("filters/definition-list-to-div.lua")?;

        // Step 1: pandoc -f markdown -t json -L filter.lua
        let mut pandoc = Command::new("pandoc")
            .args(["-f", "markdown", "-t", "json"])
            .arg("-L")
            .arg(&filter_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to spawn pandoc")?;

        {
            let stdin = pandoc
                .stdin
                .as_mut()
                .context("Failed to get pandoc stdin")?;
            stdin.write_all(list_text.as_bytes())?;
        }

        let pandoc_output = pandoc.wait_with_output()?;

        if !pandoc_output.status.success() {
            anyhow::bail!(
                "pandoc failed: {}",
                String::from_utf8_lossy(&pandoc_output.stderr)
            );
        }

        // Step 2: Use library to convert JSON to markdown
        let mut json_reader = std::io::Cursor::new(&pandoc_output.stdout);
        let (pandoc_ast, _ctx) =
            json::read(&mut json_reader).context("Failed to parse JSON output from pandoc")?;

        let mut output = Vec::new();
        qmd::write(&pandoc_ast, &mut output).map_err(|diagnostics| {
            anyhow::anyhow!("Failed to write markdown output: {:?}", diagnostics)
        })?;

        let result = String::from_utf8(output)
            .context("Failed to parse output as UTF-8")?
            .trim_end()
            .to_string();

        Ok(result)
    }
}

impl Rule for DefinitionListConverter {
    fn name(&self) -> &str {
        "definition-lists"
    }

    fn description(&self) -> &str {
        "Convert definition lists to div-based format"
    }

    fn check(&self, file_path: &Path, verbose: bool) -> Result<Vec<CheckResult>> {
        let content = read_file(file_path)?;
        let lists = self.find_definition_lists(&content);

        if verbose {
            if lists.is_empty() {
                println!("  No definition lists found");
            } else {
                println!("  Found {} definition list(s)", lists.len());
            }
        }

        let mut results = Vec::new();
        for list in lists {
            results.push(CheckResult {
                rule_name: self.name().to_string(),
                file_path: file_path.to_string_lossy().to_string(),
                has_issue: true,
                issue_count: 1,
                message: Some("Definition list found".to_string()),
                location: Some(crate::rule::SourceLocation {
                    row: list.start_line + 1, // Convert 0-indexed to 1-indexed
                    column: 1,
                }),
                error_code: None,
                error_codes: None,
            });
        }

        Ok(results)
    }

    fn convert(
        &self,
        file_path: &Path,
        in_place: bool,
        check_mode: bool,
        verbose: bool,
    ) -> Result<ConvertResult> {
        let content = read_file(file_path)?;
        let lists = self.find_definition_lists(&content);

        if lists.is_empty() {
            return Ok(ConvertResult {
                rule_name: self.name().to_string(),
                file_path: file_path.to_string_lossy().to_string(),
                fixes_applied: 0,
                message: None,
            });
        }

        // Convert each list and build new content
        let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
        let mut offset: isize = 0;

        for (idx, list) in lists.iter().enumerate() {
            if verbose {
                println!("  Converting list {}...", idx + 1);
            }

            let converted = self.convert_list(&list.text)?;
            let start = (list.start_line as isize + offset) as usize;
            let end = (list.end_line as isize + offset) as usize;

            if check_mode && verbose {
                println!(
                    "  List {} at lines {}-{}:",
                    idx + 1,
                    list.start_line,
                    list.end_line
                );
                println!(
                    "    {} {} lines -> {} {} lines",
                    "Original:".red(),
                    list.end_line - list.start_line + 1,
                    "Converted:".green(),
                    converted.lines().count()
                );
            }

            let converted_lines: Vec<String> = converted.lines().map(|s| s.to_string()).collect();
            let new_len = converted_lines.len();
            let old_len = end - start + 1;

            lines.splice(start..=end, converted_lines);
            offset += new_len as isize - old_len as isize;
        }

        let new_content = lines.join("\n") + "\n";

        if !check_mode && in_place {
            write_file(file_path, &new_content)?;
        }

        Ok(ConvertResult {
            rule_name: self.name().to_string(),
            file_path: file_path.to_string_lossy().to_string(),
            fixes_applied: lists.len(),
            message: if in_place {
                Some(format!("Converted {} list(s)", lists.len()))
            } else {
                Some(new_content)
            },
        })
    }
}
