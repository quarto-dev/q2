//! The supported-format table and the project-aware format resolver
//! (design: D8). Rust owns both; the host only asks.
//!
//! Three classes (D8.4): the preview can render the format
//! ([`FormatClass::Preview`]), pandoc.wasm can produce it
//! ([`FormatClass::Download`]), or neither ([`FormatClass::Neither`], e.g.
//! `revealjs` is a preview, `latex`/`odt`/`pdf` are neither).

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
    /// Whether `render_pandoc_request` accepts it yet. Typst waits for R4.
    pub available: bool,
}

pub const PANDOC_FORMATS: &[PandocFormatInfo] = &[
    PandocFormatInfo {
        key: "docx",
        label: "Word (.docx)",
        extension: "docx",
        mime: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        available: true,
    },
    PandocFormatInfo {
        key: "pptx",
        label: "PowerPoint (.pptx)",
        extension: "pptx",
        mime: "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        available: true,
    },
    PandocFormatInfo {
        key: "epub",
        label: "EPUB (.epub)",
        extension: "epub",
        mime: "application/epub+zip",
        available: true,
    },
    PandocFormatInfo {
        key: "typst",
        label: "Typst source (.typ)",
        extension: "typ",
        mime: "text/plain",
        available: false,
    },
];

pub fn pandoc_format(key: &str) -> Option<&'static PandocFormatInfo> {
    PANDOC_FORMATS.iter().find(|f| f.key == key)
}

/// Which class a `format:` key belongs to.
pub fn format_class(key: &str) -> FormatClass {
    if pandoc_format(key).is_some() {
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedFormats {
    pub source: FormatSource,
    /// In declaration order; the first is the document's own format.
    pub formats: Vec<ResolvedFormat>,
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
pub fn resolve_formats(content: &str, project_format: Option<&ConfigValue>) -> ResolvedFormats {
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
            .map(|key| {
                let class = format_class(&key);
                ResolvedFormat { key, class }
            })
            .collect(),
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
    let project_format = project
        .config
        .metadata
        .as_ref()
        .and_then(|meta| meta.get("format"));
    Ok(resolve_formats(&content, project_format))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_has_the_four_download_formats_in_menu_order() {
        let keys: Vec<_> = PANDOC_FORMATS.iter().map(|f| f.key).collect();
        assert_eq!(keys, ["docx", "pptx", "epub", "typst"]);
        assert!(PANDOC_FORMATS.iter().all(|f| !f.mime.is_empty()));
    }

    #[test]
    fn classes_follow_d8() {
        for key in ["docx", "pptx", "epub", "typst"] {
            assert_eq!(format_class(key), FormatClass::Download, "{key}");
        }
        for key in ["html", "revealjs", "acm-html", "q2-preview"] {
            assert_eq!(format_class(key), FormatClass::Preview, "{key}");
        }
        for key in ["pdf", "latex", "odt", "no-such-format", "my-docx"] {
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
