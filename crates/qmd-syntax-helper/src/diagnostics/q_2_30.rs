// Q-2-30: Multi-Paragraph Footnote Indentation
//
// Detects when a paragraph immediately follows a NoteDefinitionPara
// and starts with indentation, suggesting an attempted multi-paragraph
// footnote using Pandoc's indentation syntax.
//
// This is a LINTING diagnostic - the document parses successfully but
// likely has a semantic error (the indented paragraph is NOT part of the footnote).
//
// Error catalog entry: crates/quarto-error-reporting/error_catalog.json
// Error code: Q-2-30
// Title: "Multi-Paragraph Footnote Indentation Not Supported"
//
// Example:
//   [^1]: First paragraph
//
//       Second paragraph (indented - user thinks it's part of footnote, but it's not)
//
// Correct qmd syntax:
//   ::: ^1
//
//   First paragraph
//
//   Second paragraph
//
//   :::

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::rule::{CheckResult, ConvertResult, Rule, SourceLocation};
use pampa::pandoc::Block;

pub struct Q230Checker {}

#[derive(Debug, Clone)]
struct Q230Violation {
    note_id: String,
    row: usize,
    column: usize,
}

impl Q230Checker {
    pub fn new() -> Result<Self> {
        Ok(Self {})
    }

    /// Parse document and detect multi-paragraph footnote pattern
    fn get_violations(&self, file_path: &Path) -> Result<Vec<Q230Violation>> {
        let content = fs::read_to_string(file_path)
            .with_context(|| format!("Failed to read file: {}", file_path.display()))?;

        // Parse with pampa to get AST
        let mut sink = std::io::sink();
        let filename = file_path.to_string_lossy();

        let parse_result = pampa::readers::qmd::read(
            content.as_bytes(),
            false, // not loose mode
            &filename,
            &mut sink,
            true, // prune errors
            None,
        );

        // If parse fails, return empty violations (let parse rule handle it)
        let (pandoc_doc, _ast_context, _diagnostics) = match parse_result {
            Ok(result) => result,
            Err(_) => return Ok(Vec::new()),
        };

        let mut violations = Vec::new();
        let blocks = &pandoc_doc.blocks;

        // Walk through consecutive block pairs
        for i in 0..blocks.len().saturating_sub(1) {
            let current = &blocks[i];
            let next = &blocks[i + 1];

            // Check if current is NoteDefinitionPara
            if let Block::NoteDefinitionPara(note_def_para) = current {
                // Check if next is Paragraph
                if let Block::Paragraph(para) = next {
                    // Check if Para's source starts with whitespace
                    if self.para_starts_with_indent(&content, para)? {
                        let offset = para.source_info.start_offset();
                        violations.push(Q230Violation {
                            note_id: note_def_para.id.clone(),
                            row: self.offset_to_row(&content, offset),
                            column: self.offset_to_column(&content, offset),
                        });
                    }
                }
            }
        }

        Ok(violations)
    }

    /// Check if a Para block's source text starts with whitespace
    ///
    /// Note: The SourceInfo for Paragraph points to the content, not the full line.
    /// We need to look back to the start of the line to check for indentation.
    fn para_starts_with_indent(
        &self,
        content: &str,
        para: &pampa::pandoc::Paragraph,
    ) -> Result<bool> {
        let para_start = para.source_info.start_offset();

        if para_start >= content.len() {
            return Ok(false);
        }

        // Find the start of the line containing the paragraph
        let line_start = content[..para_start].rfind('\n').map_or(0, |pos| pos + 1);

        // Get the text from line start to paragraph start
        let leading_text = &content[line_start..para_start];

        // Check if there's whitespace before the paragraph content on its line
        Ok(leading_text.contains(' ') || leading_text.contains('\t'))
    }

    /// Convert byte offset to row number (0-indexed)
    fn offset_to_row(&self, content: &str, offset: usize) -> usize {
        content[..offset].matches('\n').count()
    }

    /// Convert byte offset to column number (0-indexed)
    fn offset_to_column(&self, content: &str, offset: usize) -> usize {
        let line_start = content[..offset].rfind('\n').map_or(0, |pos| pos + 1);
        offset - line_start
    }
}

impl Rule for Q230Checker {
    fn name(&self) -> &str {
        "q-2-30"
    }

    fn description(&self) -> &str {
        "Detect multi-paragraph footnotes using Pandoc indentation syntax"
    }

    fn check(&self, file_path: &Path, _verbose: bool) -> Result<Vec<CheckResult>> {
        // If file doesn't parse, return empty (let parse rule handle it)
        let violations = match self.get_violations(file_path) {
            Ok(v) => v,
            Err(_) => return Ok(vec![]),
        };

        let results: Vec<CheckResult> = violations
            .into_iter()
            .map(|v| CheckResult {
                rule_name: self.name().to_string(),
                file_path: file_path.to_string_lossy().to_string(),
                has_issue: true,
                issue_count: 1,
                message: Some(format!(
                    "Q-2-30: Indented paragraph after footnote [^{}] suggests multi-paragraph footnote",
                    v.note_id
                )),
                location: Some(SourceLocation {
                    row: v.row,
                    column: v.column,
                }),
                error_code: Some("Q-2-30".to_string()),
                error_codes: None,
            })
            .collect();

        Ok(results)
    }

    fn convert(
        &self,
        file_path: &Path,
        _in_place: bool,
        _check_mode: bool,
        _verbose: bool,
    ) -> Result<ConvertResult> {
        let violations = match self.get_violations(file_path) {
            Ok(v) => v,
            Err(_) => {
                return Ok(ConvertResult {
                    rule_name: self.name().to_string(),
                    file_path: file_path.to_string_lossy().to_string(),
                    fixes_applied: 0,
                    message: Some("File does not parse - cannot check for Q-2-30".to_string()),
                });
            }
        };

        // This is a linting diagnostic - no auto-fix available
        // Requires manual conversion to div syntax
        Ok(ConvertResult {
            rule_name: self.name().to_string(),
            file_path: file_path.to_string_lossy().to_string(),
            fixes_applied: 0,
            message: Some(if violations.is_empty() {
                "No Q-2-30 violations found".to_string()
            } else {
                format!(
                    "Found {} Q-2-30 violation(s). Manual conversion to div syntax required: ::: ^ref ... :::",
                    violations.len()
                )
            }),
        })
    }
}
