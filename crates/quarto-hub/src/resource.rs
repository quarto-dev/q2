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
    if let Some(name) = filename
        && let Some(ext) = std::path::Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
        && let Some(mime) = mime_type_from_extension(ext)
    {
        return mime.to_string();
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

/// MIME type stamped on engine-capture binary docs. Single source of
/// truth (bd-eiku4ymo); `quarto-preview` and `quarto-hub-provider`
/// re-export/consume this. The TS consumers hold the same literal.
pub const CAPTURE_MIME_TYPE: &str = "application/x-engine-capture+gzip";

/// Uncompressed provenance stamped on capture binary docs
/// (bd-eiku4ymo). Everything else about a capture lives inside the
/// gzipped `content` payload, invisible to sync-server audits; these
/// fields are written as a top-level automerge `meta` map so an
/// auditor (`hub admin scan`) can read them without decompression.
///
/// Written once at creation, never mutated — no CRDT merge concerns.
#[derive(Debug, Clone)]
pub struct CaptureDocMeta {
    /// Project-relative path of the source document, forward slashes.
    pub source_path: String,
    /// Engines that produced the capture sequence, in execution order.
    pub engines: Vec<String>,
}

/// Schema version of the capture-doc `meta` map.
const CAPTURE_META_SCHEMA_VERSION: i64 = 1;

/// Create an engine-capture binary document: the standard binary-doc
/// schema (`content`/`mimeType`/`hash`, MIME = [`CAPTURE_MIME_TYPE`])
/// plus the uncompressed `meta` audit map:
///
/// ```text
/// ROOT
/// ├── content: Bytes
/// ├── mimeType: String
/// ├── hash: String
/// └── meta: Map
///     ├── kind: "engine-capture"
///     ├── schemaVersion: 1
///     ├── createdAt: String   // RFC 3339 UTC
///     ├── sourcePath: String
///     └── engines: List<String>
/// ```
///
/// Docs created before this envelope (no `meta`) remain valid; readers
/// of `content` are unaffected either way.
pub fn create_capture_document(gzipped: &[u8], meta: &CaptureDocMeta) -> Result<Automerge> {
    create_capture_document_at(gzipped, meta, &chrono::Utc::now().to_rfc3339())
}

/// Test seam: like [`create_capture_document`] with an explicit
/// `createdAt` so age-gate tests can fabricate old captures.
pub fn create_capture_document_at(
    gzipped: &[u8],
    meta: &CaptureDocMeta,
    created_at: &str,
) -> Result<Automerge> {
    use automerge::ObjType;
    let mut doc = create_binary_document(gzipped, CAPTURE_MIME_TYPE)?;
    doc.transact::<_, _, automerge::AutomergeError>(|tx| {
        let meta_obj = tx.put_object(ROOT, "meta", ObjType::Map)?;
        tx.put(&meta_obj, "kind", "engine-capture")?;
        tx.put(&meta_obj, "schemaVersion", CAPTURE_META_SCHEMA_VERSION)?;
        tx.put(&meta_obj, "createdAt", created_at)?;
        tx.put(&meta_obj, "sourcePath", meta.source_path.as_str())?;
        let engines_obj = tx.put_object(&meta_obj, "engines", ObjType::List)?;
        for (i, engine) in meta.engines.iter().enumerate() {
            tx.insert(&engines_obj, i, engine.as_str())?;
        }
        Ok(())
    })
    .map_err(|e| {
        crate::error::Error::IndexDocument(format!("failed to stamp capture meta: {:?}", e))
    })?;
    Ok(doc)
}

/// Read the `meta` audit map back from a capture doc. `None` when the
/// doc predates the envelope (or isn't a capture doc). Used by
/// `hub admin scan`'s age gate and inventory.
pub fn read_capture_meta(doc: &Automerge) -> Option<CaptureDocMetaRead> {
    use automerge::ReadDoc;
    let (_, meta_obj) = doc.get(ROOT, "meta").ok().flatten()?;
    let get_str = |key: &str| -> Option<String> {
        doc.get(&meta_obj, key)
            .ok()
            .flatten()
            .and_then(|(value, _)| value.to_str().map(str::to_string))
    };
    let engines = match doc.get(&meta_obj, "engines").ok().flatten() {
        Some((_, engines_obj)) => (0..doc.length(&engines_obj))
            .filter_map(|i| {
                doc.get(&engines_obj, i)
                    .ok()
                    .flatten()
                    .and_then(|(value, _)| value.to_str().map(str::to_string))
            })
            .collect(),
        None => Vec::new(),
    };
    Some(CaptureDocMetaRead {
        kind: get_str("kind"),
        created_at: get_str("createdAt"),
        source_path: get_str("sourcePath"),
        engines,
    })
}

/// The `meta` map as read back from a doc — every field optional
/// because a capture written by a future (or buggy) writer must
/// still be inspectable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureDocMetaRead {
    pub kind: Option<String>,
    pub created_at: Option<String>,
    pub source_path: Option<String>,
    pub engines: Vec<String>,
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
    if let automerge::Value::Scalar(scalar) = value
        && let automerge::ScalarValue::Bytes(bytes) = scalar.as_ref()
    {
        return Some(bytes.clone());
    }

    None
}

/// Read MIME type from a binary document.
pub fn read_mime_type(doc: &Automerge) -> Option<String> {
    use automerge::ReadDoc;

    let (value, _) = doc.get(ROOT, "mimeType").ok()??;

    if let automerge::Value::Scalar(scalar) = value
        && let automerge::ScalarValue::Str(s) = scalar.as_ref()
    {
        return Some(s.to_string());
    }

    None
}

/// Read content hash from a binary document.
pub fn read_content_hash(doc: &Automerge) -> Option<String> {
    use automerge::ReadDoc;

    let (value, _) = doc.get(ROOT, "hash").ok()??;

    if let automerge::Value::Scalar(scalar) = value
        && let automerge::ScalarValue::Str(s) = scalar.as_ref()
    {
        return Some(s.to_string());
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

    // ── bd-eiku4ymo: capture-doc meta envelope ─────────────────────

    fn sample_meta() -> CaptureDocMeta {
        CaptureDocMeta {
            source_path: "posts/analysis.qmd".to_string(),
            engines: vec!["knitr".to_string(), "jupyter".to_string()],
        }
    }

    #[test]
    fn capture_document_has_binary_schema_and_capture_mime() {
        use automerge::ReadDoc;
        let doc = create_capture_document(b"gzipped-bytes", &sample_meta()).unwrap();
        // Standard binary-doc fields intact.
        assert_eq!(detect_document_type(&doc), DocumentType::Binary);
        let (mime, _) = doc.get(ROOT, "mimeType").unwrap().unwrap();
        assert_eq!(mime.to_str(), Some(CAPTURE_MIME_TYPE));
        let (hash, _) = doc.get(ROOT, "hash").unwrap().unwrap();
        assert_eq!(hash.to_str(), Some(compute_hash(b"gzipped-bytes").as_str()));
    }

    #[test]
    fn capture_document_meta_roundtrips() {
        let doc = create_capture_document(b"gz", &sample_meta()).unwrap();
        let meta = read_capture_meta(&doc).expect("meta map present");
        assert_eq!(meta.kind.as_deref(), Some("engine-capture"));
        assert_eq!(meta.source_path.as_deref(), Some("posts/analysis.qmd"));
        assert_eq!(meta.engines, vec!["knitr", "jupyter"]);
        // createdAt parses as RFC 3339 — the scan age gate depends on it.
        let created_at = meta.created_at.expect("createdAt present");
        chrono::DateTime::parse_from_rfc3339(&created_at)
            .unwrap_or_else(|e| panic!("createdAt must be RFC 3339, got {created_at}: {e}"));
    }

    #[test]
    fn capture_document_at_uses_explicit_timestamp() {
        let doc =
            create_capture_document_at(b"gz", &sample_meta(), "2020-01-02T03:04:05+00:00").unwrap();
        let meta = read_capture_meta(&doc).unwrap();
        assert_eq!(
            meta.created_at.as_deref(),
            Some("2020-01-02T03:04:05+00:00")
        );
    }

    #[test]
    fn legacy_capture_doc_without_meta_reads_none() {
        // Pre-envelope captures (plain binary docs with the capture
        // MIME) must read back as "no meta", not error.
        let doc = create_binary_document(b"gz", CAPTURE_MIME_TYPE).unwrap();
        assert!(read_capture_meta(&doc).is_none());
    }
}
