/*
 * lib.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Which files in a Quarto project directory are sources, config,
//! extension files and binary resources.
//!
//! The sync hub (`quarto-hub`, and through it `q2 preview`) and the language
//! server both need to walk a project directory and decide which files belong
//! to it. They must agree, or the LSP would analyse a different file set than
//! the one a preview syncs. This crate is the one place that decision lives
//! (epic bd-3f6ijnw9, decision D3; moved out of `quarto-hub` in bd-n0uth2zt).
//! It is deliberately light — `walkdir` and `tracing` only — so the language
//! server can use it without pulling in axum or samod.
//!
//! - [`ProjectFiles`] walks a project directory and classifies what it finds.
//! - [`is_binary_extension`] / [`mime_type_from_extension`] answer from the
//!   binary-extension table, which `quarto-hub` also uses to decide whether a
//!   file becomes a text or a binary automerge document.

pub mod discovery;

pub use discovery::{FileClass, ProjectFiles, classify};

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
    // Import sources (document import I21): stored as-is, and EMF/WMF when
    // conversion fails. Mirror of `BINARY_EXTENSIONS` in
    // ts-packages/quarto-automerge-schema.
    (
        "docx",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    ),
    ("odt", "application/vnd.oasis.opendocument.text"),
    ("rtf", "application/rtf"),
    ("epub", "application/epub+zip"),
    (
        "pptx",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    ),
    ("emf", "image/emf"),
    ("wmf", "image/wmf"),
    // hephaestus plot documents (bd-sxiv2tio). Not an IANA type; the
    // vendor-tree name keeps the binary document self-describing. Mirror
    // of `BINARY_EXTENSIONS` in ts-packages/quarto-automerge-schema.
    ("hep", "application/vnd.hephaestus.plot"),
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
