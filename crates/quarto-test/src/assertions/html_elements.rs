/*
 * quarto-test/src/assertions/html_elements.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * HTML element assertion using CSS selectors.
 */

//! `ensureHtmlElements` assertion implementation.

use std::fs;

use anyhow::{Context, Result, bail};
use scraper::{Html, Selector};

use super::{Assertion, VerifyContext};

/// Assertion that verifies HTML elements exist (or don't) via CSS selectors.
///
/// This corresponds to Quarto 1's `ensureHtmlElements` verification function.
#[derive(Debug)]
pub struct EnsureHtmlElements {
    /// CSS selectors that must match at least one element.
    pub selectors: Vec<String>,
    /// CSS selectors that must NOT match any element.
    pub no_match_selectors: Vec<String>,
}

impl EnsureHtmlElements {
    pub fn new(selectors: Vec<String>, no_match_selectors: Vec<String>) -> Result<Self> {
        // Validate all selectors at construction time so we fail early.
        for s in selectors.iter().chain(no_match_selectors.iter()) {
            Selector::parse(s)
                .map_err(|e| anyhow::anyhow!("invalid CSS selector '{}': {:?}", s, e))?;
        }
        Ok(Self {
            selectors,
            no_match_selectors,
        })
    }
}

impl Assertion for EnsureHtmlElements {
    fn name(&self) -> &str {
        "ensureHtmlElements"
    }

    fn verify(&self, context: &VerifyContext) -> Result<()> {
        if let Some(err) = &context.render_error {
            bail!("Cannot check HTML elements: rendering failed with: {}", err);
        }

        let content = fs::read_to_string(&context.output_path).with_context(|| {
            format!(
                "failed to read output file: {}",
                context.output_path.display()
            )
        })?;

        let document = Html::parse_document(&content);
        let mut failures: Vec<String> = Vec::new();

        for css in &self.selectors {
            // Safe to unwrap: validated in new()
            let sel = Selector::parse(css).unwrap();
            if document.select(&sel).next().is_none() {
                failures.push(format!("Expected selector to match: {}", css));
            }
        }

        for css in &self.no_match_selectors {
            let sel = Selector::parse(css).unwrap();
            if document.select(&sel).next().is_some() {
                failures.push(format!("Expected selector NOT to match: {}", css));
            }
        }

        if failures.is_empty() {
            Ok(())
        } else {
            bail!(
                "{} HTML element check(s) failed in {}:\n  - {}",
                failures.len(),
                context.output_path.display(),
                failures.join("\n  - ")
            )
        }
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

    fn make_context(path: &std::path::Path) -> VerifyContext {
        VerifyContext {
            output_path: path.to_path_buf(),
            input_path: path.to_path_buf(),
            format: "html".to_string(),
            render_error: None,
            messages: vec![],
        }
    }

    #[test]
    fn test_selector_matches() {
        let file = create_temp_file(
            r#"<!DOCTYPE html><html><body><nav id="TOC">contents</nav></body></html>"#,
        );
        let assertion = EnsureHtmlElements::new(vec!["nav#TOC".to_string()], vec![]).unwrap();
        assert!(assertion.verify(&make_context(file.path())).is_ok());
    }

    #[test]
    fn test_selector_does_not_match() {
        let file =
            create_temp_file(r#"<!DOCTYPE html><html><body><div>no nav here</div></body></html>"#);
        let assertion = EnsureHtmlElements::new(vec!["nav#TOC".to_string()], vec![]).unwrap();
        let result = assertion.verify(&make_context(file.path()));
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Expected selector to match")
        );
    }

    #[test]
    fn test_no_match_selector_passes() {
        let file =
            create_temp_file(r#"<!DOCTYPE html><html><body><div>content</div></body></html>"#);
        let assertion = EnsureHtmlElements::new(vec![], vec!["nav#TOC".to_string()]).unwrap();
        assert!(assertion.verify(&make_context(file.path())).is_ok());
    }

    #[test]
    fn test_no_match_selector_fails() {
        let file =
            create_temp_file(r#"<!DOCTYPE html><html><body><nav id="TOC">toc</nav></body></html>"#);
        let assertion = EnsureHtmlElements::new(vec![], vec!["nav#TOC".to_string()]).unwrap();
        let result = assertion.verify(&make_context(file.path()));
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Expected selector NOT to match")
        );
    }

    #[test]
    fn test_attribute_selector() {
        let file = create_temp_file(
            r#"<!DOCTYPE html><html><head><link href="../../shared/styles.css" rel="stylesheet"></head><body></body></html>"#,
        );
        let assertion = EnsureHtmlElements::new(
            vec![r#"link[href="../../shared/styles.css"]"#.to_string()],
            vec![],
        )
        .unwrap();
        assert!(assertion.verify(&make_context(file.path())).is_ok());
    }

    #[test]
    fn test_descendant_combinator() {
        let file = create_temp_file(
            r#"<!DOCTYPE html><html><body><div class="cell-output-display"><ul><li>item</li></ul></div></body></html>"#,
        );
        let assertion = EnsureHtmlElements::new(
            vec!["div.cell-output-display > ul > li".to_string()],
            vec![],
        )
        .unwrap();
        assert!(assertion.verify(&make_context(file.path())).is_ok());
    }

    #[test]
    fn test_nth_child_pseudo() {
        let file = create_temp_file(
            r#"<!DOCTYPE html><html><body><main><p>first</p><p>second</p><p>third</p></main></body></html>"#,
        );
        let assertion =
            EnsureHtmlElements::new(vec!["main p:nth-child(3)".to_string()], vec![]).unwrap();
        assert!(assertion.verify(&make_context(file.path())).is_ok());
    }

    #[test]
    fn test_invalid_selector_rejected() {
        let result = EnsureHtmlElements::new(vec!["[[[invalid".to_string()], vec![]);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("invalid CSS selector")
        );
    }

    #[test]
    fn test_render_error_reported() {
        let file = create_temp_file("");
        let assertion = EnsureHtmlElements::new(vec!["div".to_string()], vec![]).unwrap();
        let context = VerifyContext {
            output_path: file.path().to_path_buf(),
            input_path: file.path().to_path_buf(),
            format: "html".to_string(),
            render_error: Some("render exploded".to_string()),
            messages: vec![],
        };
        let result = assertion.verify(&context);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("rendering failed"));
    }
}
