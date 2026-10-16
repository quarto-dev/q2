/*
 * format_resolution.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * The one resolver for "which format, and which book scope was requested".
 */

//! Format selection and the requested book scope.
//!
//! The resolver reads the same layers the merge stage reads
//! ([`crate::metadata_layers`]), only the top-level `format` and `book-scope`
//! keys, and answers two questions:
//!
//! - **Which format?** [`select_formats`] is the pure rule (Quarto 1's
//!   key-list rule, epic FR1, FR2 and FR4); [`resolve_document_format`] and
//!   [`resolve_project_format`] feed it from the layers.
//! - **Which book scope was requested?** [`resolve_render_scope`] reads the
//!   top-level `book-scope` key (FR5) and falls back to what the render's
//!   target implies (FR3). [`requested_book_scope`] is the one function that
//!   interprets a `book-scope` value.
//!
//! It compiles for wasm: it needs only a [`ProjectContext`] and a runtime.

use std::path::Path;

use quarto_error_reporting::{DiagnosticKind, DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::ConfigValue;
use quarto_system_runtime::SystemRuntime;

use crate::metadata_layers::{
    DocumentLayer, LayerInputs, LayerKind, MetadataLayer, collect_metadata_layers,
    json_to_config_value,
};
use crate::pandoc_request::formats::TYPST_PDF_KEY;
use crate::project::book::BookScope;
use crate::project::orchestrator::RenderMode;
use crate::project::{ProjectContext, ProjectKind};

/// The format used when no layer declares one and none is requested (as in
/// Quarto 1).
const FALLBACK_FORMAT: &str = "html";

const BOOK_SCOPE_KEY: &str = "book-scope";

// ---------------------------------------------------------------------------
// Format selection
// ---------------------------------------------------------------------------

/// What [`select_formats`] chooses from.
#[derive(Debug, Clone, Copy, Default)]
pub struct FormatInputs<'a> {
    pub project_keys: &'a [String],
    /// Keys of the **merged** `_metadata.yml` chain (not unioned per layer).
    pub directory_keys: &'a [String],
    pub document_keys: &'a [String],
    /// A Book project rendering a whole book: only the project's keys select.
    pub project_formats_only: bool,
    /// `--to`.
    pub cli_to: Option<&'a str>,
    /// A scalar `format` in the runtime layer (`-M format:typst`).
    pub runtime_format: Option<&'a str>,
}

/// The outcome of [`select_formats`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatSelection {
    /// The declared list (the requested format may not be in it), so the
    /// Q-20-8 warning can say what was skipped.
    pub keys: Vec<String>,
    pub selected: String,
    /// The layer that supplied `keys`; `None` when nothing declared one.
    pub source: Option<LayerKind>,
    /// `selected` came from `--to` or a runtime scalar, not from `keys[0]`.
    pub requested: bool,
}

/// Quarto 1's key-list rule (epic FR1, FR2, FR4).
///
/// The list is the project's keys when `project_formats_only`; otherwise the
/// document's keys followed by the directory keys if that pool is non-empty,
/// else the project's keys. The selected key is the requested format (`--to`,
/// then a runtime scalar; it may be undeclared), else the first key, else
/// `html`.
pub fn select_formats(inputs: FormatInputs<'_>) -> FormatSelection {
    let (keys, source) = if inputs.project_formats_only {
        (inputs.project_keys.to_vec(), LayerKind::Project)
    } else {
        let mut pool: Vec<String> = inputs.document_keys.to_vec();
        for k in inputs.directory_keys {
            if !pool.contains(k) {
                pool.push(k.clone());
            }
        }
        if pool.is_empty() {
            (inputs.project_keys.to_vec(), LayerKind::Project)
        } else if inputs.document_keys.is_empty() {
            (pool, LayerKind::Directory)
        } else {
            (pool, LayerKind::Document)
        }
    };
    let source = (!keys.is_empty()).then_some(source);
    let requested = inputs.cli_to.or(inputs.runtime_format);
    let selected = match (requested, keys.first()) {
        (Some(r), _) => r.to_string(),
        (None, Some(first)) => first.clone(),
        (None, None) => FALLBACK_FORMAT.to_string(),
    };
    FormatSelection {
        keys,
        selected,
        source,
        requested: requested.is_some(),
    }
}

/// The keys a `format:` value declares: a scalar string, or a map's keys in
/// order. Entries whose value is `null` or `false` (and a scalar that is) are
/// skipped.
pub fn format_keys_of(value: &ConfigValue) -> Vec<String> {
    if let Some(entries) = value.as_map_entries() {
        return entries
            .iter()
            .filter(|e| !is_disabled(&e.value))
            .map(|e| e.key.clone())
            .collect();
    }
    if is_disabled(value) {
        return Vec::new();
    }
    value.as_plain_text().into_iter().collect()
}

fn is_disabled(value: &ConfigValue) -> bool {
    value.is_null() || value.as_bool() == Some(false)
}

/// Fold the `_metadata.yml` chain's `format:` values, root to leaf, as the
/// chain is merged: a scalar replaces whatever is below, a map merges into a
/// map key by key (a `null`/`false` entry disables the key), and a map over a
/// scalar replaces it. Returns the keys that remain.
fn merged_directory_format_keys(layers: &[&MetadataLayer]) -> Vec<String> {
    // `None` = no `format` seen yet; entries are (key, enabled).
    let mut acc: Option<Vec<(String, bool)>> = None;
    for layer in layers {
        let Some(value) = layer.config.get("format") else {
            continue;
        };
        if let Some(entries) = value.as_map_entries() {
            let acc = acc.get_or_insert_with(Vec::new);
            for e in entries {
                let enabled = !is_disabled(&e.value);
                match acc.iter_mut().find(|(k, _)| *k == e.key) {
                    Some(slot) => slot.1 = enabled,
                    None => acc.push((e.key.clone(), enabled)),
                }
            }
        } else {
            acc = Some(
                format_keys_of(value)
                    .into_iter()
                    .map(|k| (k, true))
                    .collect(),
            );
        }
    }
    acc.unwrap_or_default()
        .into_iter()
        .filter_map(|(k, on)| on.then_some(k))
        .collect()
}

/// The runtime layer's `format`, when it is a scalar (a map only configures).
fn runtime_scalar_format(layers: &[MetadataLayer]) -> Option<String> {
    let layer = layers.iter().rev().find(|l| l.kind == LayerKind::Runtime)?;
    let value = layer.config.get("format")?;
    if value.is_map() || is_disabled(value) {
        return None;
    }
    value.as_plain_text().filter(|s| !s.is_empty())
}

/// A requested key is a request key (`typst-pdf` is typst compiled to a PDF);
/// the format is `typst`.
fn normalise_requested(key: &str) -> String {
    if key == TYPST_PDF_KEY {
        "typst".to_string()
    } else {
        key.to_string()
    }
}

/// What the resolver decided for a document or a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatResolution {
    /// The selected format key (`html` when nothing declares or requests one;
    /// see [`Self::key_or`] for a caller with its own default).
    pub key: String,
    /// Every key the winning layer declared, for the Q-20-8 notice.
    pub all_keys: Vec<String>,
    pub source: Option<LayerKind>,
    pub requested: bool,
}

impl FormatResolution {
    /// The selected key, or `fallback` when no layer declared one and none
    /// was requested (a caller whose default is not `html`, such as a
    /// preview pseudo-format).
    pub fn key_or<'a>(&'a self, fallback: &'a str) -> &'a str {
        if self.source.is_none() && !self.requested {
            fallback
        } else {
            &self.key
        }
    }
}

impl From<FormatSelection> for FormatResolution {
    fn from(s: FormatSelection) -> Self {
        FormatResolution {
            key: s.selected,
            all_keys: s.keys,
            source: s.source,
            requested: s.requested,
        }
    }
}

fn keys_in_layer(layers: &[MetadataLayer], kind: LayerKind) -> Vec<String> {
    layers
        .iter()
        .rev()
        .find(|l| l.kind == kind)
        .and_then(|l| l.config.get("format"))
        .map(format_keys_of)
        .unwrap_or_default()
}

fn select_from_layers(
    layers: &[MetadataLayer],
    project_formats_only: bool,
    requested_format: Option<&str>,
) -> FormatResolution {
    let project_keys = keys_in_layer(layers, LayerKind::Project);
    let directory: Vec<&MetadataLayer> = layers
        .iter()
        .filter(|l| l.kind == LayerKind::Directory)
        .collect();
    let directory_keys = merged_directory_format_keys(&directory);
    let document_keys = keys_in_layer(layers, LayerKind::Document);
    let cli_to = requested_format.map(normalise_requested);
    let runtime_format = runtime_scalar_format(layers).map(|k| normalise_requested(&k));
    select_formats(FormatInputs {
        project_keys: &project_keys,
        directory_keys: &directory_keys,
        document_keys: &document_keys,
        project_formats_only,
        cli_to: cli_to.as_deref(),
        runtime_format: runtime_format.as_deref(),
    })
    .into()
}

/// Whether a request that resolves to `request` makes only the project's keys
/// select (FR4): a Book project rendering the whole book.
fn project_formats_only(project: &ProjectContext, request: &ScopeRequest) -> bool {
    project.project_kind() == ProjectKind::Book && request.value == BookScope::Book
}

/// The format of one document.
///
/// `content_override` is the text to read the document layer from (an editor
/// buffer, or the bytes a render is about to use); `None` reads the file
/// through `runtime`. Only the front matter is parsed, never the markdown, so
/// this is cheap enough per keystroke.
pub fn resolve_document_format(
    project: &ProjectContext,
    path: &Path,
    content_override: Option<&str>,
    request: &ScopeRequest,
    requested_format: Option<&str>,
    runtime: &dyn SystemRuntime,
) -> FormatResolution {
    let read;
    let content = match content_override {
        Some(c) => c,
        None => {
            read = runtime.file_read_string(path).unwrap_or_default();
            &read
        }
    };
    let layers = collect_metadata_layers(&LayerInputs {
        project,
        document_path: path,
        document_dir: path.parent().map(Path::to_path_buf).unwrap_or_default(),
        document: DocumentLayer::Content(content),
        extensions: None,
        target_format: None,
        is_book: false,
        runtime,
    });
    select_from_layers(
        &layers,
        project_formats_only(project, request),
        requested_format,
    )
}

/// The project's format, for callers with no single document (the
/// dispatch's format, Pass 1, `post_render` gating; FR11): the project's keys
/// plus the requested format.
pub fn resolve_project_format(
    project: &ProjectContext,
    _request: &ScopeRequest,
    requested_format: Option<&str>,
    runtime: &dyn SystemRuntime,
) -> FormatResolution {
    let layers = project_and_runtime_layers(project, runtime);
    select_from_layers(&layers, true, requested_format)
}

fn project_and_runtime_layers(
    project: &ProjectContext,
    runtime: &dyn SystemRuntime,
) -> Vec<MetadataLayer> {
    let mut layers = Vec::new();
    if let Some(metadata) = project.config.metadata.as_ref() {
        layers.push(MetadataLayer {
            kind: LayerKind::Project,
            source: project.config.config_path.clone(),
            config: metadata.clone(),
            base_dir: project.dir.clone(),
        });
    }
    if let Some(json) = runtime.runtime_metadata() {
        layers.push(MetadataLayer {
            kind: LayerKind::Runtime,
            source: None,
            config: json_to_config_value(&json),
            base_dir: project.dir.clone(),
        });
    }
    layers
}

// ---------------------------------------------------------------------------
// Requested book scope
// ---------------------------------------------------------------------------

/// A `book-scope` value that is not `book` or `chapter`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidBookScope {
    /// The offending value as written (or a description of a non-string).
    pub value: String,
}

impl InvalidBookScope {
    pub fn message(&self) -> String {
        if self.value == "none" {
            "`none` is not a `book-scope` value: it only describes the output of a render \
             that is not in book mode. Use `book` or `chapter`."
                .to_string()
        } else {
            format!(
                "`{}` is not a `book-scope` value. Use `book` or `chapter`.",
                self.value
            )
        }
    }
}

/// The one function that interprets a `book-scope` value: `None` is absent,
/// `book` and `chapter` are the two scopes, anything else is invalid
/// (including `none`, which is output only).
pub fn requested_book_scope(value: Option<&str>) -> Result<Option<BookScope>, InvalidBookScope> {
    match value {
        None => Ok(None),
        Some("book") => Ok(Some(BookScope::Book)),
        Some("chapter") => Ok(Some(BookScope::Chapter)),
        Some(other) => Err(InvalidBookScope {
            value: other.to_string(),
        }),
    }
}

/// [`requested_book_scope`] over the top-level `book-scope` key of a
/// `ConfigValue` map (never under `format.<x>:`). A non-string value is
/// invalid.
pub fn book_scope_in_config(config: &ConfigValue) -> Result<Option<BookScope>, InvalidBookScope> {
    let Some(value) = config.get(BOOK_SCOPE_KEY) else {
        return Ok(None);
    };
    match value.as_plain_text() {
        Some(s) => requested_book_scope(Some(&s)),
        None => Err(InvalidBookScope {
            value: "a non-string value".to_string(),
        }),
    }
}

/// [`requested_book_scope`] over the top-level `book-scope` key of a JSON
/// object (the routes' request metadata).
pub fn book_scope_in_json(
    metadata: &serde_json::Value,
) -> Result<Option<BookScope>, InvalidBookScope> {
    match metadata.get(BOOK_SCOPE_KEY) {
        None => Ok(None),
        Some(serde_json::Value::String(s)) => requested_book_scope(Some(s)),
        Some(_) => Err(InvalidBookScope {
            value: "a non-string value".to_string(),
        }),
    }
}

/// The requested book scope of one render (FR3, FR5).
#[derive(Debug, Clone)]
pub struct ScopeRequest {
    pub value: BookScope,
    /// The layer whose `book-scope` decided; `None` when the target's
    /// default did.
    pub explicit: Option<LayerKind>,
    /// Non-error notices, and the FR6 error when there is one.
    pub diagnostics: Vec<DiagnosticMessage>,
}

impl ScopeRequest {
    /// A request for one chapter that no layer asked for: what a
    /// per-document caller that was not told the render's intent assumes.
    pub fn chapter() -> Self {
        ScopeRequest {
            value: BookScope::Chapter,
            explicit: None,
            diagnostics: Vec::new(),
        }
    }

    /// The FR6 error (`book-scope: chapter` with nothing to render), if the
    /// request carries it.
    pub fn error(&self) -> Option<&DiagnosticMessage> {
        self.diagnostics
            .iter()
            .find(|d| d.kind == DiagnosticKind::Error)
    }
}

fn layer_name(kind: LayerKind) -> &'static str {
    match kind {
        LayerKind::Runtime => "the command line or request (`-M`)",
        LayerKind::Document => "the document",
        LayerKind::Directory => "a `_metadata.yml`",
        LayerKind::Project => "the project configuration",
        LayerKind::ExtensionContribution | LayerKind::ExtensionFormat => "an extension",
    }
}

/// The single active document of a target, if it has exactly one.
fn active_document(target: &RenderMode) -> Option<&Path> {
    match target {
        RenderMode::ActivePage(p) => Some(p),
        RenderMode::Subset(set) if set.len() == 1 => set.iter().next().map(|p| p.as_path()),
        _ => None,
    }
}

/// Which book scope this render requests (FR3, FR5, FR6).
///
/// Reads the top-level `book-scope` from, highest first: the runtime layer,
/// the document, the `_metadata.yml` chain, the project. The document and
/// `_metadata.yml` layers count only when exactly one file is the active
/// document. Absent, the target decides: `Full` is `book`, anything else
/// `chapter`.
pub fn resolve_render_scope(
    project: &ProjectContext,
    target: &RenderMode,
    runtime: &dyn SystemRuntime,
) -> ScopeRequest {
    let active = active_document(target);
    let full = matches!(target, RenderMode::Full);
    let is_book = project.project_kind() == ProjectKind::Book;
    let layers = match active {
        Some(path) => {
            let content = runtime.file_read_string(path).unwrap_or_default();
            collect_metadata_layers(&LayerInputs {
                project,
                document_path: path,
                document_dir: path.parent().map(Path::to_path_buf).unwrap_or_default(),
                document: DocumentLayer::Content(&content),
                extensions: None,
                target_format: None,
                is_book: false,
                runtime,
            })
        }
        None => project_and_runtime_layers(project, runtime),
    };

    let mut diagnostics = Vec::new();
    let mut winner: Option<(BookScope, LayerKind)> = None;
    for layer in layers.iter().rev() {
        if !matches!(
            layer.kind,
            LayerKind::Runtime | LayerKind::Document | LayerKind::Directory | LayerKind::Project
        ) {
            continue;
        }
        if layer.config.get("format").is_some_and(|f| {
            f.as_map_entries().is_some_and(|entries| {
                entries
                    .iter()
                    .any(|e| e.value.get(BOOK_SCOPE_KEY).is_some())
            })
        }) {
            diagnostics.push(scope_under_format_notice(layer.kind));
        }
        match book_scope_in_config(&layer.config) {
            Err(invalid) => diagnostics.push(invalid_scope_notice(layer.kind, &invalid)),
            Ok(None) => {}
            Ok(Some(BookScope::Chapter)) if full && is_book && layer.kind != LayerKind::Runtime => {
                diagnostics.push(chapter_ignored_notice(layer.kind));
            }
            Ok(Some(value)) => {
                if winner.is_none() {
                    winner = Some((value, layer.kind));
                }
            }
        }
    }

    let default = if full {
        BookScope::Book
    } else {
        BookScope::Chapter
    };
    let (value, explicit) = match winner {
        Some((v, k)) => (v, Some(k)),
        None => (default, None),
    };
    if let Some((_, kind)) = winner {
        if !is_book {
            diagnostics.push(not_a_book_notice(kind));
        } else if full && value == BookScope::Chapter {
            // Only the runtime layer gets here: a request for one chapter
            // with no chapter named.
            diagnostics.push(chapter_needs_a_file_error());
        }
    }
    ScopeRequest {
        value,
        explicit,
        diagnostics,
    }
}

fn invalid_scope_notice(layer: LayerKind, invalid: &InvalidBookScope) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning("Invalid `book-scope` value")
        .with_code("Q-5-43")
        .problem(format!(
            "`book-scope` in {} was ignored. {}",
            layer_name(layer),
            invalid.message()
        ))
        .build()
}

fn scope_under_format_notice(layer: LayerKind) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning("`book-scope` is not a format option")
        .with_code("Q-5-44")
        .problem(format!(
            "`book-scope` under `format:` in {} was ignored.",
            layer_name(layer)
        ))
        .add_hint(
            "Set `book-scope` at the top level: which format a render uses depends on the \
             scope, so the scope cannot depend on the format.",
        )
        .build()
}

fn chapter_ignored_notice(layer: LayerKind) -> DiagnosticMessage {
    DiagnosticMessageBuilder::info("`book-scope: chapter` ignored for a whole-book render")
        .with_code("Q-5-45")
        .problem(format!(
            "`book-scope: chapter` in {} describes how to render one chapter, so it was \
             ignored in a render of the whole book.",
            layer_name(layer)
        ))
        .build()
}

fn not_a_book_notice(layer: LayerKind) -> DiagnosticMessage {
    DiagnosticMessageBuilder::info("`book-scope` only applies to book projects")
        .with_code("Q-5-46")
        .problem(format!(
            "`book-scope` in {} has no effect: this project is not a book.",
            layer_name(layer)
        ))
        .build()
}

fn chapter_needs_a_file_error() -> DiagnosticMessage {
    DiagnosticMessageBuilder::error("`book-scope: chapter` needs a file to render")
        .with_code("Q-5-47")
        .problem("`book-scope: chapter` needs a file to render: name the chapter(s).")
        .add_hint(
            "Name the chapter to render (`q2 render ch1.qmd`), or drop `-M book-scope:chapter`.",
        )
        .build()
}

#[cfg(test)]
mod tests;
