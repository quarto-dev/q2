/*
 * format.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Output format types and resolution.
 */

//! Output format specification and resolution.
//!
//! Formats determine how documents are rendered. The format includes:
//! - The format identifier (html, pdf, docx, etc.)
//! - Whether to use the native Rust pipeline or Pandoc
//! - Format-specific options

use std::path::PathBuf;

/// Format identifier enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FormatIdentifier {
    /// HTML output (native Rust pipeline)
    Html,
    /// PDF output (requires Pandoc + LaTeX)
    Pdf,
    /// Word document (requires Pandoc)
    Docx,
    /// EPUB (requires Pandoc)
    Epub,
    /// Typst (requires typst binary)
    Typst,
    /// RevealJS slides (native Rust pipeline)
    Revealjs,
    /// GitHub-flavored Markdown
    Gfm,
    /// CommonMark
    CommonMark,
    /// Custom/unknown format
    Custom(u32), // Using u32 to keep Copy
}

impl FormatIdentifier {
    /// Get the format name as a string
    pub fn as_str(&self) -> &'static str {
        match self {
            FormatIdentifier::Html => "html",
            FormatIdentifier::Pdf => "pdf",
            FormatIdentifier::Docx => "docx",
            FormatIdentifier::Epub => "epub",
            FormatIdentifier::Typst => "typst",
            FormatIdentifier::Revealjs => "revealjs",
            FormatIdentifier::Gfm => "gfm",
            FormatIdentifier::CommonMark => "commonmark",
            FormatIdentifier::Custom(_) => "custom",
        }
    }

    /// Check if this format uses the native Rust pipeline
    pub fn is_native(&self) -> bool {
        matches!(self, FormatIdentifier::Html | FormatIdentifier::Revealjs)
    }

    /// Check if this is an HTML-based format
    pub fn is_html_based(&self) -> bool {
        matches!(self, FormatIdentifier::Html | FormatIdentifier::Revealjs)
    }

    /// Check if this format produces multiple output files (e.g., HTML website chapters)
    pub fn is_multi_file(&self) -> bool {
        // HTML is multi-file in project context (each chapter gets a file)
        // PDF, DOCX, EPUB are single-file
        matches!(self, FormatIdentifier::Html | FormatIdentifier::Revealjs)
    }
}

impl std::fmt::Display for FormatIdentifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl TryFrom<&str> for FormatIdentifier {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s.to_lowercase().as_str() {
            "html" => Ok(FormatIdentifier::Html),
            "pdf" => Ok(FormatIdentifier::Pdf),
            "docx" => Ok(FormatIdentifier::Docx),
            "epub" => Ok(FormatIdentifier::Epub),
            "typst" => Ok(FormatIdentifier::Typst),
            "revealjs" => Ok(FormatIdentifier::Revealjs),
            "gfm" => Ok(FormatIdentifier::Gfm),
            "commonmark" => Ok(FormatIdentifier::CommonMark),
            _ => Err(format!("Unknown format: {}", s)),
        }
    }
}

/// A complete format specification
#[derive(Debug, Clone)]
pub struct Format {
    /// Format identifier
    pub identifier: FormatIdentifier,

    /// Output file extension (without leading dot)
    pub output_extension: String,

    /// Whether this format uses the native Rust pipeline
    pub native_pipeline: bool,

    /// Format-specific metadata (merged from config and document)
    pub metadata: serde_json::Value,
}

impl Format {
    /// Create an HTML format
    pub fn html() -> Self {
        Self {
            identifier: FormatIdentifier::Html,
            output_extension: "html".to_string(),
            native_pipeline: true,
            metadata: serde_json::Value::Null,
        }
    }

    /// Create a PDF format
    pub fn pdf() -> Self {
        Self {
            identifier: FormatIdentifier::Pdf,
            output_extension: "pdf".to_string(),
            native_pipeline: false,
            metadata: serde_json::Value::Null,
        }
    }

    /// Create a DOCX format
    pub fn docx() -> Self {
        Self {
            identifier: FormatIdentifier::Docx,
            output_extension: "docx".to_string(),
            native_pipeline: false,
            metadata: serde_json::Value::Null,
        }
    }

    /// Set format metadata
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }

    /// Check if this format is HTML-based
    pub fn is_html(&self) -> bool {
        self.identifier.is_html_based()
    }

    /// Check if this format produces multiple files
    pub fn is_multi_file(&self) -> bool {
        self.identifier.is_multi_file()
    }

    /// Get the output file path for an input file
    pub fn output_path(&self, input: &std::path::Path) -> PathBuf {
        let mut output = input.to_path_buf();
        output.set_extension(&self.output_extension);
        output
    }
}

impl Default for Format {
    fn default() -> Self {
        Self::html()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // === FormatIdentifier tests ===

    #[test]
    fn test_format_identifier_from_string() {
        assert_eq!(
            FormatIdentifier::try_from("html").unwrap(),
            FormatIdentifier::Html
        );
        assert_eq!(
            FormatIdentifier::try_from("HTML").unwrap(),
            FormatIdentifier::Html
        );
        assert_eq!(
            FormatIdentifier::try_from("pdf").unwrap(),
            FormatIdentifier::Pdf
        );
        assert!(FormatIdentifier::try_from("unknown").is_err());
    }

    #[test]
    fn test_format_identifier_from_string_all_formats() {
        assert_eq!(
            FormatIdentifier::try_from("html").unwrap(),
            FormatIdentifier::Html
        );
        assert_eq!(
            FormatIdentifier::try_from("pdf").unwrap(),
            FormatIdentifier::Pdf
        );
        assert_eq!(
            FormatIdentifier::try_from("docx").unwrap(),
            FormatIdentifier::Docx
        );
        assert_eq!(
            FormatIdentifier::try_from("epub").unwrap(),
            FormatIdentifier::Epub
        );
        assert_eq!(
            FormatIdentifier::try_from("typst").unwrap(),
            FormatIdentifier::Typst
        );
        assert_eq!(
            FormatIdentifier::try_from("revealjs").unwrap(),
            FormatIdentifier::Revealjs
        );
        assert_eq!(
            FormatIdentifier::try_from("gfm").unwrap(),
            FormatIdentifier::Gfm
        );
        assert_eq!(
            FormatIdentifier::try_from("commonmark").unwrap(),
            FormatIdentifier::CommonMark
        );
    }

    #[test]
    fn test_format_identifier_from_string_case_insensitive() {
        assert_eq!(
            FormatIdentifier::try_from("DOCX").unwrap(),
            FormatIdentifier::Docx
        );
        assert_eq!(
            FormatIdentifier::try_from("Epub").unwrap(),
            FormatIdentifier::Epub
        );
        assert_eq!(
            FormatIdentifier::try_from("TYPST").unwrap(),
            FormatIdentifier::Typst
        );
        assert_eq!(
            FormatIdentifier::try_from("RevealJS").unwrap(),
            FormatIdentifier::Revealjs
        );
        assert_eq!(
            FormatIdentifier::try_from("GFM").unwrap(),
            FormatIdentifier::Gfm
        );
        assert_eq!(
            FormatIdentifier::try_from("CommonMark").unwrap(),
            FormatIdentifier::CommonMark
        );
    }

    #[test]
    fn test_format_identifier_from_string_error() {
        let result = FormatIdentifier::try_from("invalid_format");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Unknown format"));
        assert!(err.contains("invalid_format"));
    }

    #[test]
    fn test_format_identifier_as_str() {
        assert_eq!(FormatIdentifier::Html.as_str(), "html");
        assert_eq!(FormatIdentifier::Pdf.as_str(), "pdf");
        assert_eq!(FormatIdentifier::Docx.as_str(), "docx");
        assert_eq!(FormatIdentifier::Epub.as_str(), "epub");
        assert_eq!(FormatIdentifier::Typst.as_str(), "typst");
        assert_eq!(FormatIdentifier::Revealjs.as_str(), "revealjs");
        assert_eq!(FormatIdentifier::Gfm.as_str(), "gfm");
        assert_eq!(FormatIdentifier::CommonMark.as_str(), "commonmark");
        assert_eq!(FormatIdentifier::Custom(42).as_str(), "custom");
    }

    #[test]
    fn test_format_identifier_properties() {
        assert!(FormatIdentifier::Html.is_native());
        assert!(!FormatIdentifier::Pdf.is_native());

        assert!(FormatIdentifier::Html.is_html_based());
        assert!(FormatIdentifier::Revealjs.is_html_based());
        assert!(!FormatIdentifier::Pdf.is_html_based());
    }

    #[test]
    fn test_format_identifier_is_native_all() {
        // Native formats
        assert!(FormatIdentifier::Html.is_native());
        assert!(FormatIdentifier::Revealjs.is_native());

        // Non-native formats
        assert!(!FormatIdentifier::Pdf.is_native());
        assert!(!FormatIdentifier::Docx.is_native());
        assert!(!FormatIdentifier::Epub.is_native());
        assert!(!FormatIdentifier::Typst.is_native());
        assert!(!FormatIdentifier::Gfm.is_native());
        assert!(!FormatIdentifier::CommonMark.is_native());
        assert!(!FormatIdentifier::Custom(0).is_native());
    }

    #[test]
    fn test_format_identifier_is_html_based_all() {
        // HTML-based formats
        assert!(FormatIdentifier::Html.is_html_based());
        assert!(FormatIdentifier::Revealjs.is_html_based());

        // Non-HTML formats
        assert!(!FormatIdentifier::Pdf.is_html_based());
        assert!(!FormatIdentifier::Docx.is_html_based());
        assert!(!FormatIdentifier::Epub.is_html_based());
        assert!(!FormatIdentifier::Typst.is_html_based());
        assert!(!FormatIdentifier::Gfm.is_html_based());
        assert!(!FormatIdentifier::CommonMark.is_html_based());
        assert!(!FormatIdentifier::Custom(0).is_html_based());
    }

    #[test]
    fn test_format_identifier_is_multi_file() {
        // Multi-file formats
        assert!(FormatIdentifier::Html.is_multi_file());
        assert!(FormatIdentifier::Revealjs.is_multi_file());

        // Single-file formats
        assert!(!FormatIdentifier::Pdf.is_multi_file());
        assert!(!FormatIdentifier::Docx.is_multi_file());
        assert!(!FormatIdentifier::Epub.is_multi_file());
        assert!(!FormatIdentifier::Typst.is_multi_file());
        assert!(!FormatIdentifier::Gfm.is_multi_file());
        assert!(!FormatIdentifier::CommonMark.is_multi_file());
        assert!(!FormatIdentifier::Custom(0).is_multi_file());
    }

    #[test]
    fn test_format_identifier_display() {
        assert_eq!(format!("{}", FormatIdentifier::Html), "html");
        assert_eq!(format!("{}", FormatIdentifier::Pdf), "pdf");
        assert_eq!(format!("{}", FormatIdentifier::Custom(123)), "custom");
    }

    #[test]
    fn test_format_identifier_custom() {
        let custom1 = FormatIdentifier::Custom(1);
        let custom2 = FormatIdentifier::Custom(2);
        let custom1_copy = FormatIdentifier::Custom(1);

        assert_ne!(custom1, custom2);
        assert_eq!(custom1, custom1_copy);
        assert_eq!(custom1.as_str(), "custom");
    }

    #[test]
    fn test_format_identifier_clone_copy() {
        let original = FormatIdentifier::Html;
        let cloned = original.clone();
        let copied = original; // Copy trait

        assert_eq!(original, cloned);
        assert_eq!(original, copied);
    }

    #[test]
    fn test_format_identifier_hash() {
        let mut set = HashSet::new();
        set.insert(FormatIdentifier::Html);
        set.insert(FormatIdentifier::Pdf);
        set.insert(FormatIdentifier::Html); // Duplicate

        assert_eq!(set.len(), 2);
        assert!(set.contains(&FormatIdentifier::Html));
        assert!(set.contains(&FormatIdentifier::Pdf));
    }

    // === Format tests ===

    #[test]
    fn test_format_html() {
        let format = Format::html();

        assert_eq!(format.identifier, FormatIdentifier::Html);
        assert_eq!(format.output_extension, "html");
        assert!(format.native_pipeline);
        assert_eq!(format.metadata, serde_json::Value::Null);
    }

    #[test]
    fn test_format_pdf() {
        let format = Format::pdf();

        assert_eq!(format.identifier, FormatIdentifier::Pdf);
        assert_eq!(format.output_extension, "pdf");
        assert!(!format.native_pipeline);
        assert_eq!(format.metadata, serde_json::Value::Null);
    }

    #[test]
    fn test_format_docx() {
        let format = Format::docx();

        assert_eq!(format.identifier, FormatIdentifier::Docx);
        assert_eq!(format.output_extension, "docx");
        assert!(!format.native_pipeline);
        assert_eq!(format.metadata, serde_json::Value::Null);
    }

    #[test]
    fn test_format_with_metadata() {
        let metadata = serde_json::json!({
            "toc": true,
            "number-sections": true
        });

        let format = Format::html().with_metadata(metadata.clone());

        assert_eq!(format.identifier, FormatIdentifier::Html);
        assert_eq!(format.metadata, metadata);
    }

    #[test]
    fn test_format_is_html() {
        assert!(Format::html().is_html());
        assert!(!Format::pdf().is_html());
        assert!(!Format::docx().is_html());
    }

    #[test]
    fn test_format_is_multi_file() {
        assert!(Format::html().is_multi_file());
        assert!(!Format::pdf().is_multi_file());
        assert!(!Format::docx().is_multi_file());
    }

    #[test]
    fn test_format_output_path() {
        let format = Format::html();
        let input = std::path::Path::new("/path/to/document.qmd");
        let output = format.output_path(input);
        assert_eq!(output, std::path::PathBuf::from("/path/to/document.html"));
    }

    #[test]
    fn test_format_output_path_pdf() {
        let format = Format::pdf();
        let input = std::path::Path::new("/path/to/document.qmd");
        let output = format.output_path(input);
        assert_eq!(output, std::path::PathBuf::from("/path/to/document.pdf"));
    }

    #[test]
    fn test_format_output_path_docx() {
        let format = Format::docx();
        let input = std::path::Path::new("/path/to/report.qmd");
        let output = format.output_path(input);
        assert_eq!(output, std::path::PathBuf::from("/path/to/report.docx"));
    }

    #[test]
    fn test_format_output_path_no_extension() {
        let format = Format::html();
        let input = std::path::Path::new("/path/to/README");
        let output = format.output_path(input);
        assert_eq!(output, std::path::PathBuf::from("/path/to/README.html"));
    }

    #[test]
    fn test_format_default() {
        let format = Format::default();

        assert_eq!(format.identifier, FormatIdentifier::Html);
        assert_eq!(format.output_extension, "html");
        assert!(format.native_pipeline);
    }

    #[test]
    fn test_format_clone() {
        let original = Format::html().with_metadata(serde_json::json!({"key": "value"}));
        let cloned = original.clone();

        assert_eq!(original.identifier, cloned.identifier);
        assert_eq!(original.output_extension, cloned.output_extension);
        assert_eq!(original.native_pipeline, cloned.native_pipeline);
        assert_eq!(original.metadata, cloned.metadata);
    }
}
