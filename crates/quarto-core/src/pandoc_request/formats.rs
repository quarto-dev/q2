//! The supported-format table and the project-aware format resolver
//! (design: D8). Rust owns both; the host only asks.
//!
//! Three classes (D8.4): the preview can render the format
//! ([`FormatClass::Preview`]), pandoc.wasm can produce it
//! ([`FormatClass::Download`]), or neither ([`FormatClass::Neither`], e.g.
//! `revealjs` is a preview, `latex`/`pdf` are neither).

use std::path::Path;

use quarto_pandoc_types::ConfigValue;
use quarto_system_runtime::SystemRuntime;
use serde::Serialize;

use crate::format::{Format, FormatIdentifier};
use crate::project::ProjectContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatClass {
    /// The preview renders it (html, revealjs and their extension variants).
    Preview,
    /// pandoc.wasm can produce it (the rows of [`PANDOC_FORMATS`]).
    Download,
    /// Neither; the host disables the control and names the format.
    Neither,
}

/// One row of the "Download as" table, in menu order.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct PandocFormatInfo {
    /// The key in a document's `format:` map, and what `render_pandoc_request`
    /// takes.
    pub key: &'static str,
    pub label: &'static str,
    /// The downloaded file's extension (no dot). Typst is source only.
    pub extension: &'static str,
    pub mime: &'static str,
    /// Whether `render_pandoc_request` accepts it yet.
    pub available: bool,
    /// Accepted by `render_pandoc_request` but not offered: the host leaves
    /// it out of the menu, and the resolver classes it [`FormatClass::Neither`],
    /// until the work that finishes it lands. No row is hidden today (`pdf`
    /// was, until host H8).
    pub hidden: bool,
}

pub const PANDOC_FORMATS: &[PandocFormatInfo] = &[
    PandocFormatInfo {
        key: "docx",
        label: "Word (.docx)",
        extension: "docx",
        mime: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        available: true,
        hidden: false,
    },
    PandocFormatInfo {
        key: "odt",
        label: "OpenDocument (.odt)",
        extension: "odt",
        mime: "application/vnd.oasis.opendocument.text",
        available: true,
        hidden: false,
    },
    PandocFormatInfo {
        key: "pptx",
        label: "PowerPoint (.pptx)",
        extension: "pptx",
        mime: "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        available: true,
        hidden: false,
    },
    PandocFormatInfo {
        key: "epub",
        label: "EPUB (.epub)",
        extension: "epub",
        mime: "application/epub+zip",
        available: true,
        hidden: false,
    },
    PandocFormatInfo {
        key: "typst",
        label: "Typst source (.typ)",
        extension: "typ",
        mime: "text/plain",
        available: true,
        hidden: false,
    },
    // The typst request plus `post: compile_typst` (R4); the host compiles
    // the `.typ` to a PDF (H8).
    PandocFormatInfo {
        key: TYPST_PDF_KEY,
        label: "PDF (.pdf)",
        extension: "pdf",
        mime: "application/pdf",
        available: true,
        hidden: false,
    },
];

/// The internal key of the "typst compiled to PDF" artifact. Distinct from a document's `format: pdf` (LaTeX).
pub const TYPST_PDF_KEY: &str = "typst-pdf";

pub fn pandoc_format(key: &str) -> Option<&'static PandocFormatInfo> {
    PANDOC_FORMATS.iter().find(|f| f.key == key)
}

/// Which class a `format:` key belongs to.
pub fn format_class(key: &str) -> FormatClass {
    if pandoc_format(key).is_some_and(|f| !f.hidden) {
        return FormatClass::Download;
    }
    match Format::from_format_string(key) {
        Ok(f)
            if matches!(
                f.identifier,
                FormatIdentifier::Html | FormatIdentifier::Revealjs
            ) =>
        {
            FormatClass::Preview
        }
        _ => FormatClass::Neither,
    }
}

/// Where a resolved format list came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatSource {
    Document,
    /// The document names none; the `_quarto.yml` `format:` applies.
    Project,
    /// Neither names one: `html`.
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedFormat {
    pub key: String,
    pub class: FormatClass,
    /// The literal file extension a `typst` source download takes when the
    /// document's `output-ext` is not `pdf` (Q1: `output-ext: typst` →
    /// `doc.typst`). `None` means the table row's extension applies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedFormats {
    pub source: FormatSource,
    /// In declaration order; the first is the document's own format.
    pub formats: Vec<ResolvedFormat>,
    /// `None` outside a book project (serialized as `null`).
    pub book: Option<BookResolution>,
}

/// What the menu needs to know about a book before the first click (R9): the
/// host fetches the capture blobs for `chapters`, and offers "Download book"
/// when `chapter` is true.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BookResolution {
    /// The file-bearing chapters in book order, as sidecar keys
    /// ([`super::captures::sidecar_key`]: `/`-normalized, relative to the VFS
    /// project root; the keys of `capturesByPath`).
    pub chapters: Vec<String>,
    /// The document is one of `chapters`.
    pub chapter: bool,
}

/// The keys of a `format:` value: a string, or the keys of a map in order.
fn keys_of_format_value(value: &ConfigValue) -> Vec<String> {
    if let Some(entries) = value.as_map_entries() {
        return entries.iter().map(|e| e.key.clone()).collect();
    }
    value.as_plain_text().into_iter().collect()
}

/// The `format:` keys of a QMD document's YAML front matter, in order, or
/// `None` when the document names none (no front matter, no `format` key, an
/// unparsable block).
pub fn frontmatter_format_keys(content: &str) -> Option<Vec<String>> {
    let trimmed = content.trim_start();
    let after_first = trimmed.strip_prefix("---")?;
    let end = after_first.find("\n---")?;
    let docs = yaml_rust2::YamlLoader::load_from_str(&after_first[..end]).ok()?;
    let doc = docs.first()?;
    let keys: Vec<String> = match &doc["format"] {
        yaml_rust2::Yaml::String(s) => vec![s.clone()],
        yaml_rust2::Yaml::Hash(map) => map
            .keys()
            .filter_map(|k| k.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    };
    (!keys.is_empty()).then_some(keys)
}

/// Pure resolution: the document's own `format:` wins; otherwise the
/// project's; otherwise `html`.
///
/// `project_metadata` is the whole project metadata map (its `format:` is
/// the project's formats; `output-ext` is read from it too).
///
/// The `typst-pdf` key is the internal "typst compiled to PDF" artifact, never
/// something a document may name: a `typst` document resolves to it (the
/// default `output-ext`) or to the `typst`-source key with its literal
/// extension (any other `output-ext`, as in Q1). A declared `pdf` is LaTeX,
/// which is not in the table ([`FormatClass::Neither`]).
pub fn resolve_formats(content: &str, project_metadata: Option<&ConfigValue>) -> ResolvedFormats {
    let project_format = project_metadata.and_then(|m| m.get("format"));
    let (source, keys) = if let Some(keys) = frontmatter_format_keys(content) {
        (FormatSource::Document, keys)
    } else if let Some(keys) = project_format
        .map(keys_of_format_value)
        .filter(|k| !k.is_empty())
    {
        (FormatSource::Project, keys)
    } else {
        (FormatSource::Default, vec!["html".to_string()])
    };
    ResolvedFormats {
        source,
        formats: keys
            .into_iter()
            .map(|declared| resolve_declared(&declared, content, project_metadata))
            .collect(),
        book: None,
    }
}

fn resolve_declared(
    declared: &str,
    content: &str,
    project_metadata: Option<&ConfigValue>,
) -> ResolvedFormat {
    // The artifact key is never a document's own format, and a declared `pdf` (LaTeX) is not in the table.
    if declared == TYPST_PDF_KEY {
        return ResolvedFormat {
            key: declared.to_string(),
            class: FormatClass::Neither,
            extension: None,
        };
    }
    if declared == "typst" {
        let ext = crate::output_ext::resolve_output_ext(content, project_metadata, "typst");
        return if crate::output_ext::is_typst_compile_ext(ext.as_deref()) {
            ResolvedFormat {
                key: TYPST_PDF_KEY.to_string(),
                class: FormatClass::Download,
                extension: None,
            }
        } else {
            ResolvedFormat {
                key: "typst".to_string(),
                class: FormatClass::Download,
                extension: ext,
            }
        };
    }
    ResolvedFormat {
        key: declared.to_string(),
        class: format_class(declared),
        extension: None,
    }
}

/// [`resolve_formats`] for a document at `path`, with the surrounding
/// project (`_quarto.yml`) found through [`ProjectContext::discover`].
pub fn resolve_document_formats(
    path: &Path,
    runtime: &dyn SystemRuntime,
) -> Result<ResolvedFormats, String> {
    let content = runtime
        .file_read_string(path)
        .map_err(|e| format!("Failed to read file: {e}"))?;
    let project = ProjectContext::discover(path, runtime)
        .map_err(|e| format!("Failed to discover project context: {e}"))?;
    let mut resolved = resolve_formats(&content, project.config.metadata.as_ref());
    if project.project_kind() == crate::project::ProjectKind::Book {
        resolved.book = Some(book_resolution(&project, path, runtime));
    }
    Ok(resolved)
}

/// Computed without Pass 1 from the book's file list alone. A broken list
/// (`Q-5-34`, `Q-5-35`, `Q-5-36`) degrades to "not a chapter" rather than
/// failing the resolver; the export reports that error when a book download
/// is asked for.
fn book_resolution(
    project: &ProjectContext,
    path: &Path,
    runtime: &dyn SystemRuntime,
) -> BookResolution {
    let Ok(files) = crate::project::book::book_chapter_files(project, runtime) else {
        return BookResolution {
            chapters: Vec::new(),
            chapter: false,
        };
    };
    let chapters: Vec<String> = files
        .iter()
        .map(|f| super::captures::sidecar_key(runtime, &project.dir.join(f)))
        .collect();
    let active = super::captures::sidecar_key(runtime, path);
    BookResolution {
        chapter: chapters.contains(&active),
        chapters,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_has_the_download_formats_in_menu_order_and_none_hidden() {
        let keys: Vec<_> = PANDOC_FORMATS.iter().map(|f| f.key).collect();
        assert_eq!(keys, ["docx", "odt", "pptx", "epub", "typst", "typst-pdf"]);
        let hidden: Vec<_> = PANDOC_FORMATS
            .iter()
            .filter(|f| f.hidden)
            .map(|f| f.key)
            .collect();
        assert!(hidden.is_empty(), "{hidden:?}");
        assert!(PANDOC_FORMATS.iter().all(|f| !f.mime.is_empty()));
    }

    #[test]
    fn classes_follow_d8() {
        for key in ["docx", "odt", "pptx", "epub", "typst", "typst-pdf"] {
            assert_eq!(format_class(key), FormatClass::Download, "{key}");
        }
        for key in ["html", "revealjs", "acm-html", "q2-preview"] {
            assert_eq!(format_class(key), FormatClass::Preview, "{key}");
        }
        for key in ["latex", "opendocument", "no-such-format", "my-docx"] {
            assert_eq!(format_class(key), FormatClass::Neither, "{key}");
        }
    }

    fn keys(r: &ResolvedFormats) -> Vec<&str> {
        r.formats.iter().map(|f| f.key.as_str()).collect()
    }

    #[test]
    fn the_first_key_of_a_format_map_comes_first() {
        let r = resolve_formats(
            "---\nformat:\n  docx: default\n  html: default\n---\n",
            None,
        );
        assert_eq!(r.source, FormatSource::Document);
        assert_eq!(keys(&r), ["docx", "html"]);
        assert_eq!(r.formats[0].class, FormatClass::Download);
    }

    #[test]
    fn a_string_format_and_no_format() {
        let r = resolve_formats("---\nformat: pptx\n---\n", None);
        assert_eq!(keys(&r), ["pptx"]);
        let r = resolve_formats("# no front matter\n", None);
        assert_eq!(r.source, FormatSource::Default);
        assert_eq!(keys(&r), ["html"]);
    }

    #[test]
    fn an_unknown_format_is_neither() {
        let r = resolve_formats("---\nformat: nonsense\n---\n", None);
        assert_eq!(r.formats[0].class, FormatClass::Neither);
    }
}

#[cfg(test)]
mod output_ext_tests {
    use super::*;

    fn first(content: &str) -> ResolvedFormat {
        resolve_formats(content, None).formats.remove(0)
    }

    #[test]
    fn typst_defaults_to_the_pdf_chain() {
        for doc in [
            "---\nformat: typst\n---\n",
            "---\nformat: typst\noutput-ext: pdf\n---\n",
        ] {
            let f = first(doc);
            assert_eq!(
                (f.key.as_str(), f.class),
                ("typst-pdf", FormatClass::Download)
            );
            assert_eq!(f.extension, None);
        }
    }

    #[test]
    fn typst_with_another_output_ext_is_the_source_download_with_that_extension() {
        for (doc, ext) in [
            ("---\nformat: typst\noutput-ext: typ\n---\n", "typ"),
            (
                "---\nformat:\n  typst:\n    output-ext: typst\n---\n",
                "typst",
            ),
        ] {
            let f = first(doc);
            assert_eq!((f.key.as_str(), f.class), ("typst", FormatClass::Download));
            assert_eq!(f.extension.as_deref(), Some(ext));
        }
    }

    #[test]
    fn a_documents_own_pdf_format_is_neither() {
        let f = first("---\nformat: pdf\n---\n");
        assert_eq!((f.key.as_str(), f.class), ("pdf", FormatClass::Neither));
    }

    #[test]
    fn a_document_cannot_name_the_artifact_key() {
        let f = first("---\nformat: typst-pdf\n---\n");
        assert_eq!(
            (f.key.as_str(), f.class),
            ("typst-pdf", FormatClass::Neither)
        );
    }

    #[test]
    fn the_extension_is_not_serialized_when_absent() {
        let json = serde_json::to_string(&first("---\nformat: docx\n---\n")).unwrap();
        assert!(!json.contains("extension"), "{json}");
        let json =
            serde_json::to_string(&first("---\nformat: typst\noutput-ext: foo\n---\n")).unwrap();
        assert!(json.contains(r#""extension":"foo""#), "{json}");
    }
}
