//! Shared "does this file parse?" probe (bd-syntax-helper-parse-masking-w88mhedp).
//!
//! Rules that walk the parsed AST (`Rule::requires_parse`) can only report
//! "no findings" when they actually saw an AST. The check/convert drivers use
//! this probe to decide, per file, whether those rules can run at all; the
//! `parse` rule reuses it so the two agree on what "fails to parse" means.

use anyhow::{Context, Result};
use std::fmt;
use std::path::Path;

/// A failed parse probe: the file could not be parsed into an AST.
#[derive(Debug, Clone)]
pub struct ParseFailure {
    /// Number of parse diagnostics reported.
    pub error_count: usize,
    /// Deduplicated diagnostic codes (e.g. `Q-2-10`), in first-occurrence
    /// order. May be empty when the diagnostics carry no codes.
    pub error_codes: Vec<String>,
}

impl ParseFailure {
    pub fn from_diagnostics(diags: &[quarto_error_reporting::DiagnosticMessage]) -> Self {
        let mut error_codes: Vec<String> = Vec::new();
        for diag in diags {
            if let Some(code) = &diag.code
                && !error_codes.contains(code)
            {
                error_codes.push(code.clone());
            }
        }
        Self {
            error_count: diags.len(),
            error_codes,
        }
    }

    /// The codes joined for display — `Q-2-10, Q-2-5` — falling back to a
    /// bare count when the diagnostics carry no codes.
    pub fn codes_summary(&self) -> String {
        if self.error_codes.is_empty() {
            format!("{} error(s)", self.error_count)
        } else {
            self.error_codes.join(", ")
        }
    }
}

impl fmt::Display for ParseFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "file does not parse ({})", self.codes_summary())
    }
}

impl std::error::Error for ParseFailure {}

/// Parse `content`, reporting `Some(ParseFailure)` when it fails.
pub fn probe_content(content: &str, filename: &str) -> Option<ParseFailure> {
    let mut sink = std::io::sink();
    match pampa::readers::qmd::read(content.as_bytes(), false, filename, &mut sink, true, None) {
        Ok(_) => None,
        Err(diags) => Some(ParseFailure::from_diagnostics(&diags)),
    }
}

/// Read and probe a file on disk. `Err` means the file could not be read at
/// all; `Ok(Some(_))` means it was read but does not parse.
pub fn probe_file(path: &Path) -> Result<Option<ParseFailure>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read file: {}", path.display()))?;
    Ok(probe_content(&content, &path.to_string_lossy()))
}
