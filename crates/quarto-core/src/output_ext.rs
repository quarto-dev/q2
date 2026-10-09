/*
 * output_ext.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * The `output-ext` lookup shared by the native and wasm runtimes.
 */

//! `output-ext` resolution for a format key.
//!
//! Q1 treats `output-ext` as a free string. For `typst` the default (`pdf`)
//! means "pandoc, then `typst compile`"; any other value is taken literally
//! and means "stop after pandoc, write `<stem>.<value>`".
//!
//! The lookup reads the metadata layers of [`crate::metadata_layers`] (the
//! same collector the merge stage uses, without extensions), so every layer
//! the merge sees is seen here. Precedence, highest first:
//!
//! 1. runtime metadata (`-M`, request metadata)
//! 2. document `format.<key>.output-ext`, then document top-level `output-ext`
//! 3. the `_metadata.yml` chain, deepest first, each `format.<key>` then top level
//! 4. project `format.<key>.output-ext`, then project top-level `output-ext`
//!
//! The first layer with a non-empty string value wins; a non-string value
//! falls through to the next layer.
//!
//! Known gap: included metadata (`metadata-files`) is not seen here.

use std::path::Path;

use quarto_pandoc_types::ConfigValue;
use quarto_system_runtime::SystemRuntime;

use crate::metadata_layers::{
    DocumentLayer, LayerInputs, MetadataLayer, collect_metadata_layers, document_layer_from_content,
};
use crate::project::ProjectContext;

/// The value of `output-ext` that selects the compile recipe for typst.
pub const TYPST_COMPILE_EXT: &str = "pdf";

/// A string `output-ext` at `config`'s top level, if any. Non-string and
/// empty values are ignored, so a lower layer can still apply.
fn string_ext(config: &ConfigValue) -> Option<String> {
    config
        .get("output-ext")
        .and_then(ConfigValue::as_plain_text)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// One layer's `output-ext`: `format.<key>.output-ext`, then top-level.
fn layer_ext(config: &ConfigValue, format_key: &str) -> Option<String> {
    config
        .get_path(&["format", format_key])
        .and_then(string_ext)
        .or_else(|| string_ext(config))
}

fn ext_from_layers(layers: &[MetadataLayer], format_key: &str) -> Option<String> {
    layers
        .iter()
        .rev()
        .find_map(|layer| layer_ext(&layer.config, format_key))
}

/// Where a document lives: the project that surrounds it and the runtime its
/// layers are read through.
#[derive(Clone, Copy)]
pub struct DocumentLocation<'a> {
    pub project: &'a ProjectContext,
    pub runtime: &'a dyn SystemRuntime,
    pub path: &'a Path,
}

/// The `output-ext` configured for `format_key`, or `None` when no layer sets one.
///
/// `content` is the document source (its leading YAML front matter is the
/// document layer, whatever is on disk).
pub fn resolve_output_ext(
    location: DocumentLocation<'_>,
    content: &str,
    format_key: &str,
) -> Option<String> {
    let document_dir = location
        .path
        .parent()
        .map_or_else(|| location.project.dir.clone(), Path::to_path_buf);
    let layers = collect_metadata_layers(&LayerInputs {
        project: location.project,
        document_path: location.path,
        document_dir,
        document: DocumentLayer::Content(content),
        extensions: None,
        target_format: None,
        is_book: false,
        runtime: location.runtime,
    });
    ext_from_layers(&layers, format_key)
}

/// [`resolve_output_ext`] with no project: the document's front matter only.
pub fn resolve_document_output_ext(content: &str, format_key: &str) -> Option<String> {
    layer_ext(&document_layer_from_content(content), format_key)
}

/// Whether `ext` selects the typst compile recipe. `None` is the default (`pdf`).
pub fn is_typst_compile_ext(ext: Option<&str>) -> bool {
    ext.is_none_or(|e| e == TYPST_COMPILE_EXT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata_layers::test_runtime::MetaRuntime;
    use serde_json::json;
    use tempfile::TempDir;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    /// Resolve for `doc` (a document at `<root>/sub/doc.qmd` with `content`
    /// as its source) in the project that `files` write under a temp root.
    fn resolve(
        files: &[(&str, &str)],
        content: &str,
        runtime_metadata: Option<serde_json::Value>,
    ) -> Option<String> {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        for (name, body) in files {
            write(&root.join(name), body);
        }
        let doc = root.join("sub/doc.qmd");
        write(&doc, content);
        let runtime = MetaRuntime::new(runtime_metadata);
        let project = ProjectContext::discover_with_profile(&root, &runtime, None).unwrap();
        resolve_output_ext(
            DocumentLocation {
                project: &project,
                runtime: &runtime,
                path: &doc,
            },
            content,
            "typst",
        )
    }

    const PROJECT: &str = "project:\n  type: default\n";

    #[test]
    fn none_when_unset() {
        assert_eq!(
            resolve(
                &[("_quarto.yml", PROJECT)],
                "---\nformat: typst\n---\n",
                None
            ),
            None
        );
        assert_eq!(
            resolve(&[("_quarto.yml", PROJECT)], "no front matter", None),
            None
        );
    }

    #[test]
    fn document_per_format_beats_document_top_level() {
        let doc = "---\noutput-ext: a\nformat:\n  typst:\n    output-ext: b\n---\n";
        assert_eq!(
            resolve(&[("_quarto.yml", PROJECT)], doc, None).as_deref(),
            Some("b")
        );
    }

    #[test]
    fn document_top_level_applies() {
        let doc = "---\noutput-ext: typ\nformat: typst\n---\n";
        assert_eq!(
            resolve(&[("_quarto.yml", PROJECT)], doc, None).as_deref(),
            Some("typ")
        );
    }

    #[test]
    fn document_beats_project() {
        let project = "project:\n  type: default\noutput-ext: proj-top\nformat:\n  typst:\n    output-ext: proj-fmt\n";
        let doc = "---\noutput-ext: doc\n---\n";
        assert_eq!(
            resolve(&[("_quarto.yml", project)], doc, None).as_deref(),
            Some("doc")
        );
    }

    #[test]
    fn project_per_format_beats_project_top_level() {
        let project = "project:\n  type: default\noutput-ext: proj-top\nformat:\n  typst:\n    output-ext: proj-fmt\n";
        assert_eq!(
            resolve(&[("_quarto.yml", project)], "", None).as_deref(),
            Some("proj-fmt")
        );
    }

    #[test]
    fn project_top_level_applies_to_any_format() {
        let project = "project:\n  type: default\noutput-ext: proj-top\n";
        assert_eq!(
            resolve(&[("_quarto.yml", project)], "", None).as_deref(),
            Some("proj-top")
        );
    }

    #[test]
    fn project_per_format_ignored_for_other_formats() {
        let project = "project:\n  type: default\nformat:\n  html:\n    output-ext: proj-fmt\n";
        assert_eq!(resolve(&[("_quarto.yml", project)], "", None), None);
    }

    #[test]
    fn non_string_values_fall_through_to_lower_layers() {
        let project = "project:\n  type: default\noutput-ext: proj-top\n";
        let doc = "---\noutput-ext: 5\nformat:\n  typst:\n    output-ext: [a]\n---\n";
        assert_eq!(
            resolve(&[("_quarto.yml", project)], doc, None).as_deref(),
            Some("proj-top")
        );
        assert_eq!(resolve(&[("_quarto.yml", PROJECT)], doc, None), None);
    }

    #[test]
    fn empty_string_is_ignored() {
        assert_eq!(
            resolve(
                &[("_quarto.yml", PROJECT)],
                "---\noutput-ext: ''\n---\n",
                None
            ),
            None
        );
    }

    #[test]
    fn directory_metadata_is_seen_between_project_and_document() {
        let project = "project:\n  type: default\noutput-ext: proj\n";
        let files = [
            ("_quarto.yml", project),
            ("sub/_metadata.yml", "output-ext: dir\n"),
        ];
        assert_eq!(resolve(&files, "", None).as_deref(), Some("dir"));
        assert_eq!(
            resolve(&files, "---\noutput-ext: doc\n---\n", None).as_deref(),
            Some("doc")
        );
    }

    #[test]
    fn deeper_directory_beats_shallower() {
        let files = [
            ("_quarto.yml", PROJECT),
            ("_metadata.yml", "output-ext: shallow\n"),
            ("sub/_metadata.yml", "output-ext: deep\n"),
        ];
        assert_eq!(resolve(&files, "", None).as_deref(), Some("deep"));
    }

    #[test]
    fn runtime_metadata_beats_everything() {
        let files = [
            ("_quarto.yml", PROJECT),
            ("sub/_metadata.yml", "output-ext: dir\n"),
        ];
        let doc = "---\noutput-ext: doc\n---\n";
        assert_eq!(
            resolve(&files, doc, Some(json!({"output-ext": "rt"}))).as_deref(),
            Some("rt")
        );
        assert_eq!(
            resolve(
                &files,
                doc,
                Some(json!({"format": {"typst": {"output-ext": "rt-fmt"}}}))
            )
            .as_deref(),
            Some("rt-fmt")
        );
    }

    #[test]
    fn document_only_lookup_reads_front_matter() {
        assert_eq!(
            resolve_document_output_ext(
                "---\nformat:\n  typst:\n    output-ext: b\n---\n",
                "typst"
            )
            .as_deref(),
            Some("b")
        );
        assert_eq!(
            resolve_document_output_ext("# no front matter", "typst"),
            None
        );
    }

    #[test]
    fn compile_ext_is_pdf_or_unset() {
        assert!(is_typst_compile_ext(None));
        assert!(is_typst_compile_ext(Some("pdf")));
        assert!(!is_typst_compile_ext(Some("typ")));
        assert!(!is_typst_compile_ext(Some("foo")));
    }
}
