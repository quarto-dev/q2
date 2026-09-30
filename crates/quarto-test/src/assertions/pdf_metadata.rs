/*
 * quarto-test/src/assertions/pdf_metadata.rs
 *
 * PDF document metadata assertion.
 */

//! `ensurePdfMetadata` assertion implementation.

use anyhow::{Context, Result, bail};
use pdf_extract::{Document, Object};

use super::{Assertion, VerifyContext};

/// Expected PDF document metadata (Info dictionary) fields to check.
#[derive(Debug, Default, Clone)]
pub struct PdfMetadataExpectation {
    pub title: Option<String>,
    pub author: Option<String>,
    pub keywords: Vec<String>,
    pub creator: Option<String>,
}

/// Assertion that verifies a rendered PDF's document metadata
/// (Title/Author/Keywords/Creator, from the PDF Info dictionary) against
/// expected values. Matches Q1's `ensurePdfMetadata` semantics: string
/// fields are case-insensitive substring matches; `keywords` entries must
/// each appear as a case-insensitive substring of the PDF's Keywords field.
#[derive(Debug)]
pub struct EnsurePdfMetadata {
    expected: PdfMetadataExpectation,
}

impl EnsurePdfMetadata {
    pub fn new(expected: PdfMetadataExpectation) -> Self {
        Self { expected }
    }
}

/// PDFDocEncoding matches ASCII for the printable range PDF metadata
/// typically uses; only the UTF-16BE (BOM-prefixed) case needs real decoding.
fn decode_pdf_string(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xfe && bytes[1] == 0xff {
        let utf16: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&utf16)
    } else {
        bytes.iter().map(|&b| b as char).collect()
    }
}

fn info_string(doc: &Document, key: &[u8]) -> Option<String> {
    let info_ref = doc.trailer.get(b"Info").ok()?;
    let info_dict = match info_ref {
        Object::Reference(id) => doc.get_object(*id).ok()?.as_dict().ok()?,
        Object::Dictionary(dict) => dict,
        _ => return None,
    };
    match info_dict.get(key).ok()? {
        Object::String(bytes, _) => Some(decode_pdf_string(bytes)),
        _ => None,
    }
}

fn check_contains(field: &str, actual: Option<&str>, expected: &str, errors: &mut Vec<String>) {
    let actual = actual.unwrap_or("");
    if !actual.to_lowercase().contains(&expected.to_lowercase()) {
        errors.push(format!(
            "{field}: expected to contain \"{expected}\", got \"{actual}\""
        ));
    }
}

impl Assertion for EnsurePdfMetadata {
    fn name(&self) -> &str {
        "ensurePdfMetadata"
    }

    fn verify(&self, context: &VerifyContext) -> Result<()> {
        if let Some(err) = &context.render_error {
            bail!("Cannot check PDF metadata: rendering failed with: {err}");
        }

        let doc = Document::load(&context.output_path).with_context(|| {
            format!(
                "failed to load PDF output file: {}",
                context.output_path.display()
            )
        })?;

        let title = info_string(&doc, b"Title");
        let author = info_string(&doc, b"Author");
        let keywords = info_string(&doc, b"Keywords");
        let creator = info_string(&doc, b"Creator");

        let mut errors = Vec::new();

        if let Some(expected) = &self.expected.title {
            check_contains("title", title.as_deref(), expected, &mut errors);
        }
        if let Some(expected) = &self.expected.author {
            check_contains("author", author.as_deref(), expected, &mut errors);
        }
        for expected in &self.expected.keywords {
            check_contains("keywords", keywords.as_deref(), expected, &mut errors);
        }
        if let Some(expected) = &self.expected.creator {
            check_contains("creator", creator.as_deref(), expected, &mut errors);
        }

        if !errors.is_empty() {
            bail!(
                "PDF metadata assertions failed in {}:\n{}",
                context.output_path.display(),
                errors
                    .iter()
                    .enumerate()
                    .map(|(i, e)| format!("  {}. {e}", i + 1))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdf_extract::{Dictionary, Stream, dictionary};

    fn create_pdf_with_metadata(path: &std::path::Path, info: &[(&str, &str)]) {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let content = pdf_extract::content::Content {
            operations: vec![
                pdf_extract::content::Operation::new("BT", vec![]),
                pdf_extract::content::Operation::new("ET", vec![]),
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
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);

        let mut info_dict = Dictionary::new();
        for (key, value) in info {
            info_dict.set(*key, Object::string_literal(*value));
        }
        let info_id = doc.add_object(Object::Dictionary(info_dict));
        doc.trailer.set("Info", info_id);

        doc.save(path).unwrap();
    }

    fn context_for(pdf_path: std::path::PathBuf, input_path: std::path::PathBuf) -> VerifyContext {
        VerifyContext {
            output_path: pdf_path,
            input_path,
            format: "typst".to_string(),
            render_error: None,
            messages: vec![],
        }
    }

    #[test]
    fn matches_title_author_keywords_creator() {
        let dir = tempfile::TempDir::new().unwrap();
        let pdf_path = dir.path().join("output.pdf");
        create_pdf_with_metadata(
            &pdf_path,
            &[
                ("Title", "Test Document"),
                ("Author", "Alice Smith"),
                ("Keywords", "quarto, typst, testing"),
                ("Creator", "Typst"),
            ],
        );
        let input_path = dir.path().join("input.qmd");
        std::fs::write(&input_path, "# Test\n").unwrap();

        let assertion = EnsurePdfMetadata::new(PdfMetadataExpectation {
            title: Some("Test Document".to_string()),
            author: Some("Alice Smith".to_string()),
            keywords: vec![
                "quarto".to_string(),
                "typst".to_string(),
                "testing".to_string(),
            ],
            creator: Some("Typst".to_string()),
        });

        assert!(assertion.verify(&context_for(pdf_path, input_path)).is_ok());
    }

    #[test]
    fn reports_mismatched_field() {
        let dir = tempfile::TempDir::new().unwrap();
        let pdf_path = dir.path().join("output.pdf");
        create_pdf_with_metadata(&pdf_path, &[("Title", "Wrong Title")]);
        let input_path = dir.path().join("input.qmd");
        std::fs::write(&input_path, "# Test\n").unwrap();

        let assertion = EnsurePdfMetadata::new(PdfMetadataExpectation {
            title: Some("Test Document".to_string()),
            ..Default::default()
        });

        let err = assertion
            .verify(&context_for(pdf_path, input_path))
            .unwrap_err();
        assert!(err.to_string().contains("title"));
        assert!(err.to_string().contains("Wrong Title"));
    }

    #[test]
    fn missing_field_reports_empty_actual() {
        let dir = tempfile::TempDir::new().unwrap();
        let pdf_path = dir.path().join("output.pdf");
        create_pdf_with_metadata(&pdf_path, &[]);
        let input_path = dir.path().join("input.qmd");
        std::fs::write(&input_path, "# Test\n").unwrap();

        let assertion = EnsurePdfMetadata::new(PdfMetadataExpectation {
            author: Some("Someone".to_string()),
            ..Default::default()
        });

        let err = assertion
            .verify(&context_for(pdf_path, input_path))
            .unwrap_err();
        assert!(err.to_string().contains("got \"\""));
    }
}
