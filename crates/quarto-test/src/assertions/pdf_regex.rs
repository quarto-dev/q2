/*
 * quarto-test/src/assertions/pdf_regex.rs
 *
 * PDF text regex matching assertion.
 */

//! `ensurePdfRegexMatches` assertion implementation.

use anyhow::{Context, Result, bail};
use regex::Regex;

use super::regex_patterns::{compile_patterns, normalize_pdf_text, verify_patterns};
use super::{Assertion, VerifyContext};

/// Assertion that verifies extracted PDF text against regex patterns.
#[derive(Debug)]
pub struct EnsurePdfRegexMatches {
    /// Patterns that must match in the PDF text.
    pub matches: Vec<Regex>,
    /// Patterns that must NOT match in the PDF text.
    pub no_matches: Vec<Regex>,
    match_patterns: Vec<String>,
    no_match_patterns: Vec<String>,
}

impl EnsurePdfRegexMatches {
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

impl Assertion for EnsurePdfRegexMatches {
    fn name(&self) -> &str {
        "ensurePdfRegexMatches"
    }

    fn verify(&self, context: &VerifyContext) -> Result<()> {
        if let Some(err) = &context.render_error {
            bail!("Cannot check PDF patterns: rendering failed with: {err}");
        }

        let text = pdf_extract::extract_text(&context.output_path).with_context(|| {
            format!(
                "failed to extract text from PDF output file: {}",
                context.output_path.display()
            )
        })?;
        let text = normalize_pdf_text(&text);

        verify_patterns(
            &text,
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
    use pdf_extract::content::{Content, Operation};
    use pdf_extract::{Document, Object, Stream, dictionary};
    use tempfile::TempDir;

    fn create_pdf(path: &std::path::Path, text: &str) {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Courier",
        });
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let content = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 12.into()]),
                Operation::new("Td", vec![100.into(), 700.into()]),
                Operation::new("Tj", vec![Object::string_literal(text)]),
                Operation::new("ET", vec![]),
            ],
        };
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
                "Resources" => resources_id,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);
        doc.save(path).unwrap();
    }

    #[test]
    fn test_pdf_regex_matches_and_rejects_forbidden_text() {
        let dir = TempDir::new().unwrap();
        let pdf_path = dir.path().join("output.pdf");
        create_pdf(&pdf_path, "Chapter One");

        let assertion = EnsurePdfRegexMatches::new(
            vec!["Chapter One".to_string()],
            vec!["Forbidden".to_string()],
        )
        .unwrap();
        let input_path = dir.path().join("input.qmd");
        std::fs::write(&input_path, "# Chapter One\n").unwrap();
        let context = VerifyContext {
            output_path: pdf_path,
            input_path,
            format: "typst".to_string(),
            render_error: None,
            messages: vec![],
        };

        assert!(assertion.verify(&context).is_ok());
    }

    #[test]
    fn test_pdf_regex_rejects_forbidden_text() {
        let dir = TempDir::new().unwrap();
        let pdf_path = dir.path().join("output.pdf");
        create_pdf(&pdf_path, "Chapter One Forbidden");

        let assertion = EnsurePdfRegexMatches::new(vec![], vec!["Forbidden".to_string()]).unwrap();
        let input_path = dir.path().join("input.qmd");
        std::fs::write(&input_path, "# Chapter One\n").unwrap();
        let context = VerifyContext {
            output_path: pdf_path,
            input_path,
            format: "typst".to_string(),
            render_error: None,
            messages: vec![],
        };

        let error = assertion.verify(&context).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Illegal pattern found: Forbidden")
        );
    }
}
