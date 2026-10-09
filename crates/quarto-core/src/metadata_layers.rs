/*
 * metadata_layers.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * The ordered metadata layers, with provenance, shared by the merge stage
 * and the format resolver.
 */

//! One collector for the metadata layers.
//!
//! [`collect_metadata_layers`] returns the layers that make up a document's
//! metadata, **lowest to highest precedence**, **unflattened**, each tagged
//! with its [`LayerKind`]:
//!
//! 1. [`LayerKind::ExtensionContribution`]: extension `contributes.metadata`
//!    (one per extension, `project` key stripped). Only when extensions are
//!    passed.
//! 2. [`LayerKind::Project`]: `_quarto.yml` (already including profiles,
//!    `_quarto-<name>.yml` and `_quarto.yml.local`, merged by
//!    `ProjectContext::parse_config`).
//! 3. [`LayerKind::ExtensionFormat`]: the extension's format metadata for the
//!    target format. Only when extensions *and* a target format are passed.
//! 4. [`LayerKind::Directory`]: the `_metadata.yml` chain, root to leaf.
//!    Absent for a single-file project.
//! 5. [`LayerKind::Document`]: the document's front matter.
//! 6. [`LayerKind::Runtime`]: the runtime metadata (`-M`, preview settings).
//!
//! `MetadataMergeStage` flattens each layer for its base format, adjusts
//! paths and merges them. The format resolver reads only the top-level
//! `format` and `book-scope` keys, from the project, directory, document and
//! runtime layers; it passes no extensions and no target format, because it
//! never discovers extensions.

use std::path::{Path, PathBuf};

use quarto_pandoc_types::{ConfigMapEntry, ConfigValue, ConfigValueKind, MergeOp};
use quarto_source_map::{By, SourceInfo};
use quarto_system_runtime::SystemRuntime;

#[cfg(test)]
pub(crate) mod test_runtime;

use crate::extension::Extension;
use crate::extension::discover::{find_extension, parse_format_descriptor};
use crate::project::{ProjectContext, directory_metadata_for_document};

/// Which layer a [`MetadataLayer`] is. Lowest precedence first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LayerKind {
    ExtensionContribution,
    Project,
    ExtensionFormat,
    Directory,
    Document,
    Runtime,
}

/// One unflattened metadata layer with its provenance.
#[derive(Debug, Clone)]
pub struct MetadataLayer {
    pub kind: LayerKind,
    /// The file the layer was read from, where there is one (the config
    /// file, a `_metadata.yml`, an extension directory, the document).
    pub source: Option<PathBuf>,
    /// The layer's value, not flattened for any format.
    pub config: ConfigValue,
    /// The directory path-shaped values in this layer are relative to.
    pub base_dir: PathBuf,
}

/// Where the document layer comes from.
#[derive(Debug, Clone, Copy)]
pub enum DocumentLayer<'a> {
    /// The already-parsed front matter (`doc.ast.meta`).
    Parsed(&'a ConfigValue),
    /// The document's source text (an editor buffer or file contents); its
    /// front matter is read with [`document_layer_from_content`], not the
    /// copy on disk.
    Content(&'a str),
}

/// Everything [`collect_metadata_layers`] needs.
pub struct LayerInputs<'a> {
    pub project: &'a ProjectContext,
    /// The document path the `_metadata.yml` chain is computed for.
    pub document_path: &'a Path,
    /// The directory path-shaped document and runtime values are relative to.
    pub document_dir: PathBuf,
    pub document: DocumentLayer<'a>,
    /// `None` for the resolver: it never discovers extensions.
    pub extensions: Option<&'a [Extension]>,
    /// The full target format (`acm-html`). `None` skips the extension-format
    /// layer, the case for format selection.
    pub target_format: Option<&'a str>,
    pub is_book: bool,
    pub runtime: &'a dyn SystemRuntime,
}

/// Collect the layers, lowest to highest precedence.
///
/// A malformed `_metadata.yml` yields no directory layers (as the merge stage
/// has always behaved); callers that must report it call
/// `directory_metadata_for_document` themselves.
pub fn collect_metadata_layers(inputs: &LayerInputs<'_>) -> Vec<MetadataLayer> {
    let project = inputs.project;
    let mut layers = Vec::new();

    if let Some(extensions) = inputs.extensions {
        layers.extend(extension_contribution_layers(extensions));
    }

    if let Some(metadata) = project.config.metadata.as_ref() {
        layers.push(MetadataLayer {
            kind: LayerKind::Project,
            source: project.config.config_path.clone(),
            config: metadata.clone(),
            base_dir: project.dir.clone(),
        });
    }

    if let (Some(extensions), Some(target_format)) = (inputs.extensions, inputs.target_format)
        && let Some((config, ext_dir)) =
            extension_format_layer(extensions, target_format, inputs.is_book)
    {
        layers.push(MetadataLayer {
            kind: LayerKind::ExtensionFormat,
            source: Some(ext_dir.clone()),
            config,
            base_dir: ext_dir,
        });
    }

    if !project.is_single_file {
        let entries =
            directory_metadata_for_document(project, inputs.document_path, inputs.runtime)
                .unwrap_or_default();
        for (path, config) in entries {
            let base_dir = path
                .parent()
                .map_or_else(|| project.dir.clone(), Path::to_path_buf);
            layers.push(MetadataLayer {
                kind: LayerKind::Directory,
                source: Some(path),
                config,
                base_dir,
            });
        }
    }

    let document_config = match inputs.document {
        DocumentLayer::Parsed(meta) => meta.clone(),
        DocumentLayer::Content(content) => document_layer_from_content(content),
    };
    layers.push(MetadataLayer {
        kind: LayerKind::Document,
        source: Some(inputs.document_path.to_path_buf()),
        config: document_config,
        base_dir: inputs.document_dir.clone(),
    });

    if let Some(json) = inputs.runtime.runtime_metadata() {
        layers.push(MetadataLayer {
            kind: LayerKind::Runtime,
            source: None,
            config: json_to_config_value(&json),
            base_dir: inputs.document_dir.clone(),
        });
    }

    layers
}

/// The document layer read from source text: its leading YAML front matter
/// as a `ConfigValue`, or an empty map when there is none or it does not
/// parse.
///
/// This is the same YAML-to-config path `directory_metadata_for_document`
/// uses for `_metadata.yml`, in the document-metadata interpretation, and it
/// skips the markdown parse, so it is cheap enough for per-keystroke calls.
/// Read string values with `as_plain_text()` (document strings are parsed as
/// markdown).
///
/// The front matter is located as the qmd reader does: `---` must be the
/// first thing in the text after an optional BOM and blank lines, and the
/// block ends at the first line that is exactly `---`. A `...`
/// terminator is not recognised (the reader does not recognise it either, so
/// the stage would see no front matter), which is why this does not use
/// `format::extract_yaml_frontmatter`.
pub fn document_layer_from_content(content: &str) -> ConfigValue {
    use pampa::pandoc::yaml_to_config_value;
    use pampa::utils::diagnostic_collector::DiagnosticCollector;
    use quarto_config::InterpretationContext;

    let empty =
        || ConfigValue::new_map(Vec::new(), SourceInfo::generated(By::programmatic_config()));
    let Some(yaml_text) = frontmatter_block(content) else {
        return empty();
    };
    let Ok(yaml) = quarto_yaml::parse(yaml_text) else {
        return empty();
    };
    let mut diagnostics = DiagnosticCollector::new();
    yaml_to_config_value(
        yaml,
        InterpretationContext::DocumentMetadata,
        &mut diagnostics,
    )
}

/// The text between the opening `---` line and the next line that is
/// exactly `---` (trailing whitespace allowed; `----` is not a terminator).
fn frontmatter_block(content: &str) -> Option<&str> {
    let s = content.strip_prefix('\u{feff}').unwrap_or(content);
    // The reader skips whitespace-only lines before the opening `---`.
    let s = s.trim_start();
    let after_open = s
        .strip_prefix("---\n")
        .or_else(|| s.strip_prefix("---\r\n"))?;
    let mut offset = 0;
    for line in after_open.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return Some(&after_open[..offset]);
        }
        offset += line.len();
    }
    None
}

/// Convert a `serde_json::Value` to a `ConfigValue`.
///
/// Used for converting runtime metadata (which uses `serde_json::Value` to avoid
/// coupling `quarto-system-runtime` to `quarto-pandoc-types`) into the `ConfigValue`
/// type needed by the merge pipeline. YAML tags are lost.
pub(crate) fn json_to_config_value(value: &serde_json::Value) -> ConfigValue {
    use yaml_rust2::Yaml;

    let source_info = SourceInfo::generated(By::programmatic_config());
    let kind = match value {
        serde_json::Value::Null => ConfigValueKind::scalar(Yaml::Null),
        serde_json::Value::Bool(b) => ConfigValueKind::scalar(Yaml::Boolean(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                ConfigValueKind::scalar(Yaml::Integer(i))
            } else if let Some(f) = n.as_f64() {
                ConfigValueKind::scalar(Yaml::Real(f.to_string()))
            } else {
                ConfigValueKind::scalar(Yaml::String(n.to_string()))
            }
        }
        serde_json::Value::String(s) => ConfigValueKind::scalar(Yaml::String(s.clone())),
        serde_json::Value::Array(arr) => {
            let items: Vec<ConfigValue> = arr.iter().map(json_to_config_value).collect();
            ConfigValueKind::Array(items)
        }
        serde_json::Value::Object(obj) => {
            let entries: Vec<ConfigMapEntry> = obj
                .iter()
                .map(|(k, v)| ConfigMapEntry {
                    key: k.clone(),
                    key_source: SourceInfo::generated(By::programmatic_config()),
                    value: json_to_config_value(v),
                })
                .collect();
            ConfigValueKind::Map(entries)
        }
    };
    ConfigValue {
        value: kind,
        source_info,
        merge_op: MergeOp::default(),
    }
}

/// The extension format metadata for the target format.
///
/// Parses the target format name to extract an extension reference (e.g.,
/// "acm-html" → extension "acm", base format "html"), finds the matching
/// extension, and returns its format-specific metadata (already specific to
/// the format, not to be flattened again) with the extension's directory.
///
/// `is_book` feeds the Typst-book default (book-projects P2): a book
/// project targeting bare `typst` resolves to the bundled `orange-book`
/// extension with no explicit extension named (Q1 parity; see
/// [`crate::extension::discover::with_typst_book_default`]).
pub(crate) fn extension_format_layer(
    extensions: &[Extension],
    target_format: &str,
    is_book: bool,
) -> Option<(ConfigValue, PathBuf)> {
    let desc = crate::extension::discover::with_typst_book_default(
        parse_format_descriptor(target_format),
        is_book,
    );

    let ext_name = desc.extension_name.as_deref()?;
    let ext = find_extension(ext_name, extensions)?;

    // Look up format metadata: try base format first, then exact match
    let base_meta = ext.contributes.formats.get(&desc.base_format);

    // Also check for exact format name match (e.g., "acm-html" as a key)
    let exact_meta = ext.contributes.formats.get(target_format);

    let config = match (base_meta, exact_meta) {
        (Some(base), Some(exact)) => {
            // Merge: base is lower priority, exact match is higher
            let layers: Vec<&ConfigValue> = vec![base, exact];
            let merged = quarto_config::MergedConfig::new(layers);
            Some(merged.materialize().unwrap_or_else(|_| exact.clone()))
        }
        (Some(base), None) => Some(base.clone()),
        (None, Some(exact)) => Some(exact.clone()),
        (None, None) => None,
    };

    config.map(|cv| (cv, ext.path.clone()))
}

/// The layers from extension `contributes.metadata` (bd-ad7i1pc6 Phase 5,
/// absorbing bd-zb2tod5f).
///
/// One layer per extension declaring `contributes.metadata`, in discovery
/// order. The `project` key is stripped (it is a project-config concern,
/// merged into the project config by `ProjectContext::parse_config`). These
/// layers sit at the **bottom** of the merge stack: extension metadata fills
/// gaps, and everything the user writes wins over it. Their paths are
/// relative to the extension directory.
pub(crate) fn extension_contribution_layers(extensions: &[Extension]) -> Vec<MetadataLayer> {
    extensions
        .iter()
        .filter_map(|ext| {
            let meta = ext.contributes.metadata.as_ref()?;
            let entries: Vec<ConfigMapEntry> = meta
                .as_map_entries()?
                .iter()
                .filter(|e| e.key != "project")
                .cloned()
                .collect();
            if entries.is_empty() {
                return None;
            }
            Some(MetadataLayer {
                kind: LayerKind::ExtensionContribution,
                source: Some(ext.path.clone()),
                config: ConfigValue::new_map(entries, meta.source_info.clone()),
                base_dir: ext.path.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::test_runtime::MetaRuntime;
    use super::*;
    use crate::extension::types::{Contributes, ExtensionId};
    use serde_json::json;
    use tempfile::TempDir;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn string(v: &str) -> ConfigValue {
        ConfigValue::new_string(v, SourceInfo::for_test())
    }

    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        ConfigValue::new_map(
            entries
                .into_iter()
                .map(|(k, v)| ConfigMapEntry {
                    key: k.to_string(),
                    key_source: SourceInfo::for_test(),
                    value: v,
                })
                .collect(),
            SourceInfo::for_test(),
        )
    }

    /// An extension `acme` contributing `probe-ext-meta` (plus a `project`
    /// key that must be stripped) and, for `html`, `probe-ext-format`.
    fn acme() -> Extension {
        let mut formats = std::collections::HashMap::new();
        formats.insert(
            "html".to_string(),
            map(vec![("probe-ext-format", string("f"))]),
        );
        Extension {
            id: ExtensionId::new("acme"),
            title: Some("Acme".to_string()),
            author: None,
            version: None,
            quarto_required: None,
            path: PathBuf::from("/ext/acme"),
            contributes: Contributes {
                metadata: Some(map(vec![
                    ("probe-ext-meta", string("e")),
                    ("project", map(vec![("pre-render", string("x.ts"))])),
                ])),
                formats,
                ..Default::default()
            },
        }
    }

    struct Fixture {
        _tmp: TempDir,
        root: PathBuf,
        doc: PathBuf,
        project: ProjectContext,
    }

    /// A project with a `_quarto.yml`, two nested `_metadata.yml` and a
    /// document at `<root>/a/b/doc.qmd`.
    fn fixture(single_file: bool) -> Fixture {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        write(
            &root.join("_quarto.yml"),
            "project:\n  type: default\nprobe: project\n",
        );
        write(&root.join("a/_metadata.yml"), "probe: dir-a\n");
        write(&root.join("a/b/_metadata.yml"), "probe: dir-b\n");
        let doc = root.join("a/b/doc.qmd");
        write(&doc, "---\nprobe: doc\n---\n");
        let runtime = MetaRuntime::new(None);
        let mut project = ProjectContext::discover_with_profile(&root, &runtime, None).unwrap();
        project.is_single_file = single_file;
        Fixture {
            _tmp: tmp,
            root,
            doc,
            project,
        }
    }

    fn collect(
        f: &Fixture,
        content: &str,
        extensions: Option<&[Extension]>,
        target_format: Option<&str>,
        runtime: &MetaRuntime,
    ) -> Vec<MetadataLayer> {
        collect_metadata_layers(&LayerInputs {
            project: &f.project,
            document_path: &f.doc,
            document_dir: f.doc.parent().unwrap().to_path_buf(),
            document: DocumentLayer::Content(content),
            extensions,
            target_format,
            is_book: false,
            runtime,
        })
    }

    fn kinds(layers: &[MetadataLayer]) -> Vec<LayerKind> {
        layers.iter().map(|l| l.kind).collect()
    }

    #[test]
    fn every_kind_in_order_lowest_to_highest() {
        let f = fixture(false);
        let runtime = MetaRuntime::new(Some(json!({"probe": "runtime"})));
        let layers = collect(
            &f,
            "---\nprobe: doc\n---\n",
            Some(&[acme()]),
            Some("acme-html"),
            &runtime,
        );
        assert_eq!(
            kinds(&layers),
            [
                LayerKind::ExtensionContribution,
                LayerKind::Project,
                LayerKind::ExtensionFormat,
                LayerKind::Directory,
                LayerKind::Directory,
                LayerKind::Document,
                LayerKind::Runtime,
            ]
        );
        // Sorted by precedence, so the derived `Ord` agrees with the order.
        assert!(kinds(&layers).windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn extension_layers_are_absent_without_extensions() {
        let f = fixture(false);
        let layers = collect(&f, "", None, Some("acme-html"), &MetaRuntime::new(None));
        assert_eq!(
            kinds(&layers),
            [
                LayerKind::Project,
                LayerKind::Directory,
                LayerKind::Directory,
                LayerKind::Document
            ]
        );
    }

    #[test]
    fn extension_format_layer_is_absent_without_a_target_format() {
        let f = fixture(false);
        let layers = collect(&f, "", Some(&[acme()]), None, &MetaRuntime::new(None));
        assert_eq!(layers[0].kind, LayerKind::ExtensionContribution);
        assert!(!kinds(&layers).contains(&LayerKind::ExtensionFormat));
    }

    #[test]
    fn extension_contribution_drops_the_project_key() {
        let f = fixture(false);
        let layers = collect(&f, "", Some(&[acme()]), None, &MetaRuntime::new(None));
        let contribution = &layers[0].config;
        assert!(contribution.get("probe-ext-meta").is_some());
        assert!(contribution.get("project").is_none());
        assert_eq!(layers[0].base_dir, PathBuf::from("/ext/acme"));
    }

    #[test]
    fn directory_chain_is_absent_for_a_single_file_project() {
        let f = fixture(true);
        let layers = collect(&f, "", None, None, &MetaRuntime::new(None));
        assert!(!kinds(&layers).contains(&LayerKind::Directory));
    }

    #[test]
    fn deepest_directory_layer_comes_last_and_carries_its_file() {
        let f = fixture(false);
        let layers = collect(&f, "", None, None, &MetaRuntime::new(None));
        let dirs: Vec<_> = layers
            .iter()
            .filter(|l| l.kind == LayerKind::Directory)
            .collect();
        assert_eq!(dirs.len(), 2);
        assert_eq!(
            dirs[0].source.as_deref(),
            Some(f.root.join("a/_metadata.yml").as_path())
        );
        assert_eq!(
            dirs[1].source.as_deref(),
            Some(f.root.join("a/b/_metadata.yml").as_path())
        );
        assert_eq!(dirs[1].base_dir, f.root.join("a/b"));
    }

    #[test]
    fn runtime_layer_is_last_and_absent_without_runtime_metadata() {
        let f = fixture(false);
        let with = collect(
            &f,
            "",
            None,
            None,
            &MetaRuntime::new(Some(json!({"k": "v"}))),
        );
        assert_eq!(with.last().unwrap().kind, LayerKind::Runtime);
        assert_eq!(
            with.last()
                .unwrap()
                .config
                .get("k")
                .and_then(ConfigValue::as_plain_text)
                .as_deref(),
            Some("v")
        );
        let without = collect(&f, "", None, None, &MetaRuntime::new(None));
        assert_eq!(without.last().unwrap().kind, LayerKind::Document);
    }

    #[test]
    fn project_layer_carries_the_config_file() {
        let f = fixture(false);
        let layers = collect(&f, "", None, None, &MetaRuntime::new(None));
        let project = layers
            .iter()
            .find(|l| l.kind == LayerKind::Project)
            .unwrap();
        assert_eq!(
            project.source.as_deref(),
            Some(f.root.join("_quarto.yml").as_path())
        );
        assert_eq!(project.base_dir, f.root);
    }

    #[test]
    fn content_override_never_reads_the_copy_on_disk() {
        let f = fixture(false);
        // The file on disk says `probe: doc`; the buffer says otherwise.
        let layers = collect(
            &f,
            "---\nprobe: buffer\n---\n",
            None,
            None,
            &MetaRuntime::new(None),
        );
        let document = layers
            .iter()
            .find(|l| l.kind == LayerKind::Document)
            .unwrap();
        assert_eq!(
            document
                .config
                .get("probe")
                .and_then(ConfigValue::as_plain_text)
                .as_deref(),
            Some("buffer")
        );
    }

    #[test]
    fn parsed_document_layer_is_taken_as_given() {
        let f = fixture(false);
        let meta = map(vec![("probe", string("parsed"))]);
        let layers = collect_metadata_layers(&LayerInputs {
            project: &f.project,
            document_path: &f.doc,
            document_dir: f.doc.parent().unwrap().to_path_buf(),
            document: DocumentLayer::Parsed(&meta),
            extensions: None,
            target_format: None,
            is_book: false,
            runtime: &MetaRuntime::new(None),
        });
        let document = layers
            .iter()
            .find(|l| l.kind == LayerKind::Document)
            .unwrap();
        assert_eq!(
            document
                .config
                .get("probe")
                .and_then(ConfigValue::as_plain_text)
                .as_deref(),
            Some("parsed")
        );
    }

    /// An extension's `contributes.metadata` that carries `format:` is merged
    /// by the stage (which passes the extensions) and is invisible to a
    /// collector call without them: the documented exception where the
    /// early one-key resolver and the late full merge can differ (FR14).
    #[test]
    fn extension_metadata_format_key_is_seen_only_with_extensions() {
        let mut ext = acme();
        ext.contributes.metadata = Some(map(vec![("format", string("typst"))]));
        let f = fixture(false);
        let runtime = MetaRuntime::new(None);
        let format_in = |layers: &[MetadataLayer]| {
            layers.iter().any(|l| {
                l.kind == LayerKind::ExtensionContribution && l.config.get("format").is_some()
            })
        };
        assert!(format_in(&collect(
            &f,
            "",
            Some(&[ext.clone()]),
            None,
            &runtime
        )));
        assert!(!format_in(&collect(&f, "", None, None, &runtime)));
    }

    // ── document_layer_from_content agrees with the parse stage ─────────

    /// The `format` keys (null and false entries skipped) and `book-scope`
    /// of a document's metadata, however it was obtained.
    fn probe(meta: &ConfigValue) -> (Vec<String>, Option<String>) {
        let keys = match meta.get("format") {
            Some(f) => match f.as_map_entries() {
                Some(entries) => entries
                    .iter()
                    .filter(|e| {
                        !matches!(e.value.as_bool(), Some(false))
                            && !matches!(
                                &e.value.value,
                                ConfigValueKind::Scalar {
                                    yaml: yaml_rust2::Yaml::Null,
                                    ..
                                }
                            )
                    })
                    .map(|e| e.key.clone())
                    .collect(),
                None => f.as_plain_text().into_iter().collect(),
            },
            None => Vec::new(),
        };
        (
            keys,
            meta.get("book-scope").and_then(ConfigValue::as_plain_text),
        )
    }

    /// The document's metadata as the pipeline's parse stage reads it.
    fn parsed_meta(content: &str) -> ConfigValue {
        use crate::format::Format;
        use crate::pipeline::run_pipeline;
        use crate::project::DocumentInfo;
        use crate::render::{BinaryDependencies, RenderContext};
        use crate::stage::{ParseDocumentStage, PipelineStage};
        use std::sync::Arc;

        let project = ProjectContext {
            dir: PathBuf::from("/"),
            is_single_file: true,
            ..Default::default()
        };
        let doc = DocumentInfo::from_path("/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        let stages: Vec<Box<dyn PipelineStage>> = vec![Box::new(ParseDocumentStage::new())];
        let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> =
            Arc::new(MetaRuntime::new(None));
        let (output, _) = pollster::block_on(run_pipeline(
            content.as_bytes(),
            "/doc.qmd",
            &mut ctx,
            runtime,
            stages,
        ))
        .unwrap();
        output.into_document_ast().unwrap().ast.meta
    }

    #[test]
    fn content_layer_agrees_with_the_parse_stage_on_format_and_book_scope() {
        let corpus = [
            "---\nformat: typst\n---\n\nBody\n",
            "---\nformat:\n  html: default\n  typst:\n    toc: true\n---\n",
            "---\nformat:\n  html: default\n  docx: null\n  pdf: false\n---\n",
            "---\nformat: typst\nbook-scope: chapter\n---\n",
            "\u{feff}---\nformat: typst\n---\n",
            "---\r\nformat: typst\r\n---\r\n",
            "---\nformat: typst\n...\n\nBody\n",
            "---\nformat: typst\n...\nbook-scope: chapter\n---\n",
            "\n\n---\nformat: typst\n---\n",
            "\r\n---\nformat: typst\n---\n",
            "  \n---\nformat: typst\n---\n",
            "---\nformat: typst\n----\n",
            "---\nformat: typst\n---   \nBody\n",
            "---\nformat: typst\n--- x\n---\n",
            "---\ntitle: T\n---\n",
            "no front matter\n",
            "---\nformat: [unclosed\n---\n",
        ];
        for content in corpus {
            assert_eq!(
                probe(&document_layer_from_content(content)),
                probe(&parsed_meta(content)),
                "disagreement on {content:?}"
            );
        }
    }

    /// Known difference: the qmd reader also takes a `---` block that comes
    /// later in the document (after a blank line, as Pandoc does) as metadata,
    /// so the stage sees its keys; the helper reads only the block at the top,
    /// which is where Quarto 1 reads a document's `format:` from.
    #[test]
    fn a_metadata_block_after_other_content_is_not_front_matter_for_the_helper() {
        let content = "# Title\n\n---\nformat: typst\n---\n";
        assert_eq!(probe(&document_layer_from_content(content)), (vec![], None));
        assert_eq!(probe(&parsed_meta(content)).0, ["typst"]);
    }

    /// A `...` terminator is not one: the qmd reader (and so the stage)
    /// sees no front matter in a document that has only `...`, and the block
    /// of a document with both runs to the first `\n---`, whose YAML ends at
    /// the `...` document marker.
    #[test]
    fn dot_terminator_is_not_a_terminator() {
        let doc = document_layer_from_content("---\nformat: typst\n...\nBody\n");
        assert_eq!(probe(&doc), (vec![], None));
        let doc =
            document_layer_from_content("---\nformat: typst\n...\nbook-scope: chapter\n---\n");
        assert_eq!(probe(&doc), (vec!["typst".to_string()], None));
    }
}
