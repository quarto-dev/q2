/*
 * quarto-test/src/assertions/typst_file_regex.rs
 *
 * Typst source regex matching assertion.
 */

//! `ensureTypstFileRegexMatches` assertion implementation.

use std::fs;

use anyhow::{Context, Result, bail};
use regex::Regex;

use super::regex_patterns::{compile_patterns, verify_patterns};
use super::{Assertion, VerifyContext};

/// Assertion that verifies the kept Typst intermediate against regex patterns.
#[derive(Debug)]
pub struct EnsureTypstFileRegexMatches {
    /// Patterns that must match in the Typst source.
    pub matches: Vec<Regex>,
    /// Patterns that must NOT match in the Typst source.
    pub no_matches: Vec<Regex>,
    match_patterns: Vec<String>,
    no_match_patterns: Vec<String>,
}

impl EnsureTypstFileRegexMatches {
    /// Create an assertion from required and forbidden pattern strings.
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

impl Assertion for EnsureTypstFileRegexMatches {
    fn name(&self) -> &str {
        "ensureTypstFileRegexMatches"
    }

    fn verify(&self, context: &VerifyContext) -> Result<()> {
        if let Some(err) = &context.render_error {
            bail!("Cannot check Typst source patterns: rendering failed with: {err}");
        }

        let typst_path = context.output_path.with_extension("typ");
        if !typst_path.is_file() {
            bail!(
                "Cannot check Typst source patterns: expected intermediate file {} is missing; set keep-typ: true in the document metadata",
                typst_path.display()
            );
        }
        let content = fs::read_to_string(&typst_path).with_context(|| {
            format!(
                "failed to read Typst intermediate file: {}",
                typst_path.display()
            )
        })?;

        verify_patterns(
            &content,
            &self.matches,
            &self.no_matches,
            &self.match_patterns,
            &self.no_match_patterns,
            &typst_path.display().to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_typst_fixture(typst: &str) -> (TempDir, VerifyContext) {
        let dir = TempDir::new().unwrap();
        let input_path = dir.path().join("input.qmd");
        fs::write(&input_path, "---\nkeep-typ: true\n---\n\n# Chapter One\n").unwrap();
        fs::write(dir.path().join("output.typ"), typst).unwrap();
        let context = VerifyContext {
            output_path: dir.path().join("output.pdf"),
            input_path,
            format: "typst".to_string(),
            render_error: None,
            messages: vec![],
        };
        (dir, context)
    }

    #[test]
    fn test_typst_file_regex_matches_and_excludes_patterns() {
        let (_dir, context) = create_typst_fixture("#figure(\"Chapter One\")\n");
        let assertion = EnsureTypstFileRegexMatches::new(
            vec!["#figure".to_string()],
            vec!["#set page".to_string()],
        )
        .unwrap();

        assert!(assertion.verify(&context).is_ok());
    }

    #[test]
    fn test_typst_file_regex_rejects_forbidden_text() {
        let (_dir, context) = create_typst_fixture("#figure(\"Chapter One\")\n#set page\n");
        let assertion = EnsureTypstFileRegexMatches::new(
            vec!["#figure".to_string()],
            vec!["#set page".to_string()],
        )
        .unwrap();

        let error = assertion.verify(&context).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Illegal pattern found: #set page")
        );
    }

    #[test]
    fn test_typst_file_regex_requires_keep_typ_intermediate() {
        let dir = TempDir::new().unwrap();
        let assertion =
            EnsureTypstFileRegexMatches::new(vec!["#figure".to_string()], vec![]).unwrap();
        let context = VerifyContext {
            output_path: dir.path().join("output.pdf"),
            input_path: dir.path().join("input.qmd"),
            format: "typst".to_string(),
            render_error: None,
            messages: vec![],
        };

        let error = assertion.verify(&context).unwrap_err();
        assert!(error.to_string().contains("keep-typ: true"));
    }
}
