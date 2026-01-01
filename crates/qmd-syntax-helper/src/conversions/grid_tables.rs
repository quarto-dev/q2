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

pub struct GridTableConverter {
    grid_start_regex: Regex,
    table_line_regex: Regex,
    caption_regex: Regex,
    resources: ResourceManager,
}

#[derive(Debug)]
pub struct GridTable {
    pub text: String,
    pub start_line: usize,
    pub end_line: usize,
}

impl GridTableConverter {
    pub fn new() -> Result<Self> {
        Ok(Self {
            // Matches lines that start with + and contain - or =
            grid_start_regex: Regex::new(r"^\+[-=+]+\+").unwrap(),
            // Matches table content lines (start with + or |)
            table_line_regex: Regex::new(r"^[+|]").unwrap(),
            // Matches caption lines (start with :)
            caption_regex: Regex::new(r"^:").unwrap(),
            resources: ResourceManager::new()?,
        })
    }

    /// Find all grid tables in the content
    pub fn find_grid_tables(&self, content: &str) -> Vec<GridTable> {
        let lines: Vec<&str> = content.lines().collect();
        let mut tables = Vec::new();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i];

            // Check if this line starts a grid table
            if self.grid_start_regex.is_match(line) {
                let start_idx = i;
                let mut table_lines = vec![line];
                i += 1;

                // Collect all lines that are part of the table
                while i < lines.len() {
                    let line = lines[i];

                    // Table content lines start with + or |
                    if self.table_line_regex.is_match(line) {
                        table_lines.push(line);
                        i += 1;
                    }
                    // Caption line starts with : and must immediately follow table
                    else if self.caption_regex.is_match(line)
                        && i == start_idx + table_lines.len()
                    {
                        table_lines.push(line);
                        i += 1;
                        break;
                    } else {
                        break;
                    }
                }

                // Found a complete table
                let table_text = table_lines.join("\n");
                tables.push(GridTable {
                    text: table_text,
                    start_line: start_idx,
                    end_line: i - 1,
                });
            } else {
                i += 1;
            }
        }

        tables
    }

    /// Convert a single grid table by:
    /// 1. Running pandoc with the Lua filter to convert to JSON
    /// 2. Running pampa to convert JSON to markdown
    pub fn convert_table(&self, table_text: &str) -> Result<String> {
        use std::io::Write;

        // Get the Lua filter path from resources
        let filter_path = self
            .resources
            .get_resource("filters/grid-table-to-list-table.lua")?;

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
            stdin.write_all(table_text.as_bytes())?;
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

impl Rule for GridTableConverter {
    fn name(&self) -> &str {
        "grid-tables"
    }

    fn description(&self) -> &str {
        "Convert grid tables to list-table format"
    }

    fn check(&self, file_path: &Path, verbose: bool) -> Result<Vec<CheckResult>> {
        let content = read_file(file_path)?;
        let tables = self.find_grid_tables(&content);

        if verbose {
            if tables.is_empty() {
                println!("  No grid tables found");
            } else {
                println!("  Found {} grid table(s)", tables.len());
            }
        }

        let mut results = Vec::new();
        for table in tables {
            results.push(CheckResult {
                rule_name: self.name().to_string(),
                file_path: file_path.to_string_lossy().to_string(),
                has_issue: true,
                issue_count: 1,
                message: Some("Grid table found".to_string()),
                location: Some(crate::rule::SourceLocation {
                    row: table.start_line + 1, // Convert 0-indexed to 1-indexed
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
        let tables = self.find_grid_tables(&content);

        if tables.is_empty() {
            return Ok(ConvertResult {
                rule_name: self.name().to_string(),
                file_path: file_path.to_string_lossy().to_string(),
                fixes_applied: 0,
                message: None,
            });
        }

        // Convert each table and build new content
        let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
        let mut offset: isize = 0;

        for (idx, table) in tables.iter().enumerate() {
            if verbose {
                println!("  Converting table {}...", idx + 1);
            }

            let converted = self.convert_table(&table.text)?;
            let start = (table.start_line as isize + offset) as usize;
            let end = (table.end_line as isize + offset) as usize;

            if check_mode && verbose {
                println!(
                    "  Table {} at lines {}-{}:",
                    idx + 1,
                    table.start_line,
                    table.end_line
                );
                println!(
                    "    {} {} lines -> {} {} lines",
                    "Original:".red(),
                    table.end_line - table.start_line + 1,
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

        if !check_mode
            && in_place {
                write_file(file_path, &new_content)?;
            }

        Ok(ConvertResult {
            rule_name: self.name().to_string(),
            file_path: file_path.to_string_lossy().to_string(),
            fixes_applied: tables.len(),
            message: if in_place {
                Some(format!("Converted {} table(s)", tables.len()))
            } else {
                Some(new_content)
            },
        })
    }
}
