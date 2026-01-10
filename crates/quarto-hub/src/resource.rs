//! Binary resource support for quarto-hub
//!
//! This module provides utilities for creating and managing binary file documents
//! (images, PDFs, etc.) in automerge. Binary documents are self-describing:
//! they have a `content` field (Bytes) instead of a `text` field (Text).
//!
//! ## Document Schema
//!
//! **Text documents** (existing):
//! ```text
//! ROOT
//! └── text: Text  // automerge Text type
//! ```
//!
//! **Binary documents** (new):
//! ```text
//! ROOT
//! ├── content: Bytes     // Uint8Array with file contents
//! ├── mimeType: String   // MIME type (e.g., "image/png")
//! └── hash: String       // SHA-256 hash of content (hex-encoded)
//! ```

use automerge::{Automerge, ROOT, transaction::Transactable};
use sha2::{Digest, Sha256};

use crate::error::Result;

/// Known binary file extensions and their MIME types.
///
/// Used as a fallback when `infer` cannot detect the type from magic bytes.
const BINARY_EXTENSIONS: &[(&str, &str)] = &[
    // Images
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("svg", "image/svg+xml"),
    ("ico", "image/x-icon"),
    ("bmp", "image/bmp"),
    ("tiff", "image/tiff"),
    ("tif", "image/tiff"),
    // Documents
    ("pdf", "application/pdf"),
    // Fonts
    ("woff", "font/woff"),
    ("woff2", "font/woff2"),
    ("ttf", "font/ttf"),
    ("otf", "font/otf"),
    ("eot", "application/vnd.ms-fontobject"),
    // Audio/Video
    ("mp3", "audio/mpeg"),
    ("mp4", "video/mp4"),
    ("webm", "video/webm"),
    ("ogg", "audio/ogg"),
    ("wav", "audio/wav"),
];

/// Check if a file extension indicates a binary file.
pub fn is_binary_extension(ext: &str) -> bool {
    let ext_lower = ext.to_lowercase();
    BINARY_EXTENSIONS.iter().any(|(e, _)| *e == ext_lower)
}

/// Get MIME type from file extension.
///
/// Returns `None` if the extension is not recognized.
pub fn mime_type_from_extension(ext: &str) -> Option<&'static str> {
    let ext_lower = ext.to_lowercase();
    BINARY_EXTENSIONS
        .iter()
        .find(|(e, _)| *e == ext_lower)
        .map(|(_, mime)| *mime)
}

/// Detect MIME type from file content using magic bytes.
///
/// Falls back to extension-based detection if magic bytes don't match.
pub fn detect_mime_type(content: &[u8], filename: Option<&str>) -> String {
    // Try to detect from content first (magic bytes)
    if let Some(kind) = infer::get(content) {
        return kind.mime_type().to_string();
    }

    // Fall back to extension-based detection
    if let Some(name) = filename {
        if let Some(ext) = std::path::Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
        {
            if let Some(mime) = mime_type_from_extension(ext) {
                return mime.to_string();
            }
        }
    }

    // Default fallback
    "application/octet-stream".to_string()
}

/// Compute SHA-256 hash of content and return as hex string.
pub fn compute_hash(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    let result = hasher.finalize();
    hex::encode(result)
}

/// Create a new automerge document for binary content.
///
/// The document has the schema:
/// ```text
/// ROOT
/// ├── content: Bytes     // raw binary data
/// ├── mimeType: String   // detected or provided MIME type
/// └── hash: String       // SHA-256 hash (hex-encoded)
/// ```
pub fn create_binary_document(content: &[u8], mime_type: &str) -> Result<Automerge> {
    let hash = compute_hash(content);

    let mut doc = Automerge::new();
    doc.transact::<_, _, automerge::AutomergeError>(|tx| {
        // Store binary content
        tx.put(ROOT, "content", content.to_vec())?;

        // Store MIME type
        tx.put(ROOT, "mimeType", mime_type)?;

        // Store content hash
        tx.put(ROOT, "hash", hash)?;

        Ok(())
    })
    .map_err(|e| {
        crate::error::Error::IndexDocument(format!("failed to create binary document: {:?}", e))
    })?;

    Ok(doc)
}

/// Document type enumeration for distinguishing text and binary documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentType {
    /// Text document with `text` field
    Text,
    /// Binary document with `content` field
    Binary,
    /// Invalid or empty document
    Invalid,
}

/// Detect document type by checking which fields are present.
///
/// - If `text` field exists: Text document
/// - If `content` field exists: Binary document
/// - Otherwise: Invalid
pub fn detect_document_type(doc: &Automerge) -> DocumentType {
    use automerge::ReadDoc;

    let has_text = doc.get(ROOT, "text").ok().flatten().is_some();
    let has_content = doc.get(ROOT, "content").ok().flatten().is_some();

    match (has_text, has_content) {
        (true, false) => DocumentType::Text,
        (false, true) => DocumentType::Binary,
        (true, true) => {
            // Both fields present - prefer text for backwards compatibility
            tracing::warn!("Document has both 'text' and 'content' fields, treating as text");
            DocumentType::Text
        }
        (false, false) => DocumentType::Invalid,
    }
}

/// Read binary content from a document.
///
/// Returns `None` if the document is not a binary document or if the content
/// field is missing/invalid.
pub fn read_binary_content(doc: &Automerge) -> Option<Vec<u8>> {
    use automerge::ReadDoc;

    let (value, _) = doc.get(ROOT, "content").ok()??;

    // Content is stored as bytes (scalar value)
    if let automerge::Value::Scalar(scalar) = value {
        if let automerge::ScalarValue::Bytes(bytes) = scalar.as_ref() {
            return Some(bytes.clone());
        }
    }

    None
}

/// Read MIME type from a binary document.
pub fn read_mime_type(doc: &Automerge) -> Option<String> {
    use automerge::ReadDoc;

    let (value, _) = doc.get(ROOT, "mimeType").ok()??;

    if let automerge::Value::Scalar(scalar) = value {
        if let automerge::ScalarValue::Str(s) = scalar.as_ref() {
            return Some(s.to_string());
        }
    }

    None
}

/// Read content hash from a binary document.
pub fn read_content_hash(doc: &Automerge) -> Option<String> {
    use automerge::ReadDoc;

    let (value, _) = doc.get(ROOT, "hash").ok()??;

    if let automerge::Value::Scalar(scalar) = value {
        if let automerge::ScalarValue::Str(s) = scalar.as_ref() {
            return Some(s.to_string());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_binary_extension() {
        assert!(is_binary_extension("png"));
        assert!(is_binary_extension("PNG"));
        assert!(is_binary_extension("jpg"));
        assert!(is_binary_extension("pdf"));
        assert!(!is_binary_extension("qmd"));
        assert!(!is_binary_extension("yml"));
        assert!(!is_binary_extension("txt"));
    }

    #[test]
    fn test_mime_type_from_extension() {
        assert_eq!(mime_type_from_extension("png"), Some("image/png"));
        assert_eq!(mime_type_from_extension("PNG"), Some("image/png"));
        assert_eq!(mime_type_from_extension("jpg"), Some("image/jpeg"));
        assert_eq!(mime_type_from_extension("jpeg"), Some("image/jpeg"));
        assert_eq!(mime_type_from_extension("pdf"), Some("application/pdf"));
        assert_eq!(mime_type_from_extension("svg"), Some("image/svg+xml"));
        assert_eq!(mime_type_from_extension("unknown"), None);
    }

    #[test]
    fn test_compute_hash() {
        let content = b"Hello, world!";
        let hash = compute_hash(content);

        // SHA-256 of "Hello, world!" is known
        assert_eq!(
            hash,
            "315f5bdb76d078c43b8ac0064e4a0164612b1fce77c869345bfc94c75894edd3"
        );

        // Different content should have different hash
        let hash2 = compute_hash(b"Different content");
        assert_ne!(hash, hash2);
    }

    #[test]
    fn test_detect_mime_type_from_magic_bytes() {
        // PNG magic bytes: 89 50 4E 47 0D 0A 1A 0A
        let png_content = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        assert_eq!(detect_mime_type(&png_content, None), "image/png");

        // JPEG magic bytes: FF D8 FF
        let jpeg_content = [0xFF, 0xD8, 0xFF, 0xE0];
        assert_eq!(detect_mime_type(&jpeg_content, None), "image/jpeg");
    }

    #[test]
    fn test_detect_mime_type_from_extension() {
        // Unknown magic bytes, but known extension
        let unknown_content = [0x00, 0x01, 0x02, 0x03];
        assert_eq!(
            detect_mime_type(&unknown_content, Some("image.svg")),
            "image/svg+xml"
        );
    }

    #[test]
    fn test_detect_mime_type_fallback() {
        // Unknown magic bytes and unknown extension
        let unknown_content = [0x00, 0x01, 0x02, 0x03];
        assert_eq!(
            detect_mime_type(&unknown_content, Some("file.unknown")),
            "application/octet-stream"
        );
    }

    #[test]
    fn test_create_binary_document() {
        let content = b"Binary content here";
        let mime_type = "application/octet-stream";

        let doc = create_binary_document(content, mime_type).unwrap();

        // Check document type
        assert_eq!(detect_document_type(&doc), DocumentType::Binary);

        // Check content
        let read_content = read_binary_content(&doc).unwrap();
        assert_eq!(read_content, content);

        // Check MIME type
        let read_mime = read_mime_type(&doc).unwrap();
        assert_eq!(read_mime, mime_type);

        // Check hash
        let read_hash = read_content_hash(&doc).unwrap();
        assert_eq!(read_hash, compute_hash(content));
    }

    #[test]
    fn test_detect_document_type() {
        use automerge::ObjType;

        // Text document
        let mut text_doc = Automerge::new();
        text_doc
            .transact::<_, _, automerge::AutomergeError>(|tx| {
                let text_obj = tx.put_object(ROOT, "text", ObjType::Text)?;
                tx.update_text(&text_obj, "Hello")?;
                Ok(())
            })
            .unwrap();
        assert_eq!(detect_document_type(&text_doc), DocumentType::Text);

        // Binary document
        let binary_doc = create_binary_document(b"content", "image/png").unwrap();
        assert_eq!(detect_document_type(&binary_doc), DocumentType::Binary);

        // Empty document
        let empty_doc = Automerge::new();
        assert_eq!(detect_document_type(&empty_doc), DocumentType::Invalid);
    }
}
