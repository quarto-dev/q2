/*
 * quarto-test/src/assertions/file_regex.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * File regex matching assertion.
 */

//! `ensureFileRegexMatches` assertion implementation.

use std::fs;

use anyhow::{Context, Result, bail};
use regex::Regex;

use super::regex_patterns::{compile_patterns, verify_patterns};
use super::{Assertion, VerifyContext};

/// Assertion that verifies file content matches (or doesn't match) regex patterns.
///
/// This corresponds to Quarto 1's `ensureFileRegexMatches` verification function.
#[derive(Debug)]
pub struct EnsureFileRegexMatches {
    /// Patterns that must match in the output file.
    pub matches: Vec<Regex>,
    /// Patterns that must NOT match in the output file.
    pub no_matches: Vec<Regex>,
    /// Original pattern strings for error messages.
    match_patterns: Vec<String>,
    no_match_patterns: Vec<String>,
}

impl EnsureFileRegexMatches {
    /// Create a new assertion from pattern strings.
    ///
    /// Patterns are compiled as multiline regexes (so `^` and `$` match line boundaries).
    pub fn new(matches: Vec<String>, no_matches: Vec<String>) -> Result<Self> {
        let (compiled_matches, compiled_no_matches) = compile_patterns(&matches, &no_matches)?;

        Ok(Self {
            matches: compiled_matches,
            no_matches: compiled_no_matches,
            match_patterns: matches,
            no_match_patterns: no_matches,
        })
    }
}

impl Assertion for EnsureFileRegexMatches {
    fn name(&self) -> &str {
        "ensureFileRegexMatches"
    }

    fn verify(&self, context: &VerifyContext) -> Result<()> {
        // If rendering failed, we can't check file content
        if let Some(err) = &context.render_error {
            bail!("Cannot check file patterns: rendering failed with: {}", err);
        }

        let content = fs::read_to_string(&context.output_path).with_context(|| {
            format!(
                "failed to read output file: {}",
                context.output_path.display()
            )
        })?;

        verify_patterns(
            &content,
            &self.matches,
            &self.no_matches,
            &self.match_patterns,
            &self.no_match_patterns,
            &context.output_path.display().to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_temp_file(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "{}", content).unwrap();
        file
    }

    #[test]
    fn test_matches_pass() {
        let file = create_temp_file("<!DOCTYPE html>\n<title>Test</title>");

        let assertion =
            EnsureFileRegexMatches::new(vec!["<!DOCTYPE html>".to_string()], vec![]).unwrap();

        let context = VerifyContext {
            output_path: file.path().to_path_buf(),
            input_path: file.path().to_path_buf(),
            format: "html".to_string(),
            render_error: None,
            messages: vec![],
        };

        assert!(assertion.verify(&context).is_ok());
    }

    #[test]
    fn test_matches_fail() {
        let file = create_temp_file("<!DOCTYPE html>\n<title>Test</title>");

        let assertion =
            EnsureFileRegexMatches::new(vec!["MISSING_PATTERN".to_string()], vec![]).unwrap();

        let context = VerifyContext {
            output_path: file.path().to_path_buf(),
            input_path: file.path().to_path_buf(),
            format: "html".to_string(),
            render_error: None,
            messages: vec![],
        };

        let result = assertion.verify(&context);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Required pattern not found")
        );
    }

    #[test]
    fn test_no_matches_pass() {
        let file = create_temp_file("<!DOCTYPE html>\n<title>Test</title>");

        let assertion = EnsureFileRegexMatches::new(vec![], vec!["ERROR".to_string()]).unwrap();

        let context = VerifyContext {
            output_path: file.path().to_path_buf(),
            input_path: file.path().to_path_buf(),
            format: "html".to_string(),
            render_error: None,
            messages: vec![],
        };

        assert!(assertion.verify(&context).is_ok());
    }

    #[test]
    fn test_no_matches_fail() {
        let file = create_temp_file("<!DOCTYPE html>\nERROR: something went wrong");

        let assertion = EnsureFileRegexMatches::new(vec![], vec!["ERROR".to_string()]).unwrap();

        let context = VerifyContext {
            output_path: file.path().to_path_buf(),
            input_path: file.path().to_path_buf(),
            format: "html".to_string(),
            render_error: None,
            messages: vec![],
        };

        let result = assertion.verify(&context);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Illegal pattern found")
        );
    }

    #[test]
    fn test_multiline_regex() {
        let file = create_temp_file("line 1\nline 2\nline 3");

        let assertion = EnsureFileRegexMatches::new(vec!["^line 2$".to_string()], vec![]).unwrap();

        let context = VerifyContext {
            output_path: file.path().to_path_buf(),
            input_path: file.path().to_path_buf(),
            format: "html".to_string(),
            render_error: None,
            messages: vec![],
        };

        assert!(assertion.verify(&context).is_ok());
    }
}
