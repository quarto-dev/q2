/*
 * quarto-test/src/assertions/css_regex.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * CSS regex matching assertion.
 */

//! `ensureCssRegexMatches` assertion implementation.
//!
//! Parses the rendered HTML for `<link rel="stylesheet">` tags, reads
//! each linked CSS file from the output directory, concatenates the
//! content, and runs regex matches/no-matches against the combined CSS.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use regex::Regex;

use super::{Assertion, VerifyContext};

/// Assertion that verifies linked CSS content matches (or doesn't match) regex patterns.
///
/// This corresponds to Quarto 1's `ensureCssRegexMatches` verification function.
#[derive(Debug)]
pub struct EnsureCssRegexMatches {
    /// Patterns that must match in the combined CSS.
    pub matches: Vec<Regex>,
    /// Patterns that must NOT match in the combined CSS.
    pub no_matches: Vec<Regex>,
    /// Original pattern strings for error messages.
    match_patterns: Vec<String>,
    no_match_patterns: Vec<String>,
}

impl EnsureCssRegexMatches {
    /// Create a new assertion from pattern strings.
    ///
    /// Patterns are compiled as multiline regexes (so `^` and `$` match line boundaries).
    pub fn new(matches: Vec<String>, no_matches: Vec<String>) -> Result<Self> {
        let compiled_matches: Result<Vec<Regex>> = matches
            .iter()
            .map(|p| {
                Regex::new(&format!("(?m){}", p))
                    .with_context(|| format!("invalid regex pattern: {}", p))
            })
            .collect();

        let compiled_no_matches: Result<Vec<Regex>> = no_matches
            .iter()
            .map(|p| {
                Regex::new(&format!("(?m){}", p))
                    .with_context(|| format!("invalid regex pattern: {}", p))
            })
            .collect();

        Ok(Self {
            matches: compiled_matches?,
            no_matches: compiled_no_matches?,
            match_patterns: matches,
            no_match_patterns: no_matches,
        })
    }
}

/// Extract local stylesheet hrefs from HTML content.
///
/// Finds `<link ... rel="stylesheet" ... href="...">` tags and returns
/// the href values, skipping external URLs (http://, https://, //).
fn extract_stylesheet_hrefs(html: &str) -> Vec<String> {
    // Match <link> tags with rel="stylesheet" and extract href
    let link_re = Regex::new(r#"<link\b[^>]*\brel=["']stylesheet["'][^>]*>"#).expect("valid regex");
    let href_re = Regex::new(r#"\bhref=["']([^"']+)["']"#).expect("valid regex");

    let mut hrefs = Vec::new();
    for link_match in link_re.find_iter(html) {
        let link_tag = link_match.as_str();
        if let Some(href_cap) = href_re.captures(link_tag) {
            let href = &href_cap[1];
            // Skip external URLs
            if href.starts_with("http://") || href.starts_with("https://") || href.starts_with("//")
            {
                continue;
            }
            hrefs.push(href.to_string());
        }
    }

    // Also match when href comes before rel
    let link_re2 =
        Regex::new(r#"<link\b[^>]*\bhref=["']([^"']+)["'][^>]*\brel=["']stylesheet["'][^>]*>"#)
            .expect("valid regex");
    for cap in link_re2.captures_iter(html) {
        let href = &cap[1];
        if href.starts_with("http://") || href.starts_with("https://") || href.starts_with("//") {
            continue;
        }
        // Avoid duplicates
        let href_str = href.to_string();
        if !hrefs.contains(&href_str) {
            hrefs.push(href_str);
        }
    }

    hrefs
}

/// Read and concatenate CSS files linked from the HTML output.
fn read_linked_css(output_path: &Path) -> Result<String> {
    let html = fs::read_to_string(output_path)
        .with_context(|| format!("failed to read output file: {}", output_path.display()))?;

    let output_dir = output_path
        .parent()
        .context("output path has no parent directory")?;

    let hrefs = extract_stylesheet_hrefs(&html);

    let mut combined_css = String::new();
    for href in &hrefs {
        let css_path = output_dir.join(href);
        match fs::read_to_string(&css_path) {
            Ok(css) => {
                combined_css.push_str(&css);
                combined_css.push('\n');
            }
            Err(e) => {
                // Warn but don't fail — the file might not exist yet
                // (e.g., before B1 migration wires up CSS artifacts)
                eprintln!(
                    "ensureCssRegexMatches: warning: could not read {}: {}",
                    css_path.display(),
                    e
                );
            }
        }
    }

    Ok(combined_css)
}

impl Assertion for EnsureCssRegexMatches {
    fn name(&self) -> &str {
        "ensureCssRegexMatches"
    }

    fn verify(&self, context: &VerifyContext) -> Result<()> {
        if let Some(err) = &context.render_error {
            bail!("Cannot check CSS patterns: rendering failed with: {}", err);
        }

        let css = read_linked_css(&context.output_path)?;

        if css.is_empty() {
            bail!(
                "ensureCssRegexMatches: no CSS content found (no local stylesheets linked in {})",
                context.output_path.display()
            );
        }

        let mut failures: Vec<String> = Vec::new();

        for (i, regex) in self.matches.iter().enumerate() {
            if !regex.is_match(&css) {
                failures.push(format!(
                    "Required CSS pattern not found: {}",
                    self.match_patterns[i]
                ));
            }
        }

        for (i, regex) in self.no_matches.iter().enumerate() {
            if regex.is_match(&css) {
                failures.push(format!(
                    "Illegal CSS pattern found: {}",
                    self.no_match_patterns[i]
                ));
            }
        }

        if failures.is_empty() {
            Ok(())
        } else {
            bail!(
                "{} CSS regex mismatch(es) in {}:\n  - {}",
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

    #[test]
    fn test_extract_stylesheet_hrefs_basic() {
        let html = r#"
            <link rel="stylesheet" href="styles.css">
            <link rel="stylesheet" href="theme.css">
        "#;
        let hrefs = extract_stylesheet_hrefs(html);
        assert_eq!(hrefs, vec!["styles.css", "theme.css"]);
    }

    #[test]
    fn test_extract_stylesheet_hrefs_skips_external() {
        let html = r#"
            <link rel="stylesheet" href="https://cdn.example.com/style.css">
            <link rel="stylesheet" href="local.css">
            <link rel="stylesheet" href="//cdn.example.com/other.css">
        "#;
        let hrefs = extract_stylesheet_hrefs(html);
        assert_eq!(hrefs, vec!["local.css"]);
    }

    #[test]
    fn test_extract_stylesheet_hrefs_href_before_rel() {
        let html = r#"<link href="styles.css" rel="stylesheet">"#;
        let hrefs = extract_stylesheet_hrefs(html);
        assert_eq!(hrefs, vec!["styles.css"]);
    }

    #[test]
    fn test_extract_stylesheet_hrefs_with_subdir() {
        let html = r#"<link rel="stylesheet" href="doc_files/styles.css">"#;
        let hrefs = extract_stylesheet_hrefs(html);
        assert_eq!(hrefs, vec!["doc_files/styles.css"]);
    }

    #[test]
    fn test_extract_stylesheet_hrefs_no_links() {
        let html = r#"<html><body>No stylesheets here</body></html>"#;
        let hrefs = extract_stylesheet_hrefs(html);
        assert!(hrefs.is_empty());
    }

    #[test]
    fn test_css_regex_matches() {
        use std::io::Write;
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();

        // Write CSS file
        let css_path = dir.path().join("styles.css");
        let mut css_file = fs::File::create(&css_path).unwrap();
        write!(css_file, "--bs-primary: #375a7f;\n--bs-body-bg: #222;").unwrap();

        // Write HTML that links to CSS
        let html_path = dir.path().join("output.html");
        let mut html_file = fs::File::create(&html_path).unwrap();
        write!(
            html_file,
            r#"<html><head><link rel="stylesheet" href="styles.css"></head></html>"#
        )
        .unwrap();

        let assertion = EnsureCssRegexMatches::new(
            vec!["--bs-primary:.*#375a7f".to_string()],
            vec!["#2c3e50".to_string()],
        )
        .unwrap();

        let context = VerifyContext {
            output_path: html_path,
            input_path: dir.path().join("test.qmd"),
            format: "html".to_string(),
            render_error: None,
            messages: vec![],
        };

        assert!(assertion.verify(&context).is_ok());
    }

    #[test]
    fn test_css_regex_match_fails() {
        use std::io::Write;
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();

        let css_path = dir.path().join("styles.css");
        let mut css_file = fs::File::create(&css_path).unwrap();
        write!(css_file, "--bs-primary: #2c3e50;").unwrap();

        let html_path = dir.path().join("output.html");
        let mut html_file = fs::File::create(&html_path).unwrap();
        write!(
            html_file,
            r#"<html><head><link rel="stylesheet" href="styles.css"></head></html>"#
        )
        .unwrap();

        let assertion =
            EnsureCssRegexMatches::new(vec!["--bs-primary:.*#375a7f".to_string()], vec![]).unwrap();

        let context = VerifyContext {
            output_path: html_path,
            input_path: dir.path().join("test.qmd"),
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
                .contains("Required CSS pattern not found")
        );
    }
}
