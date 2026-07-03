/*
 * project/mod.rs
 * Copyright (c) 2025-2026 Posit, PBC
 *
 * Project context and orchestration for Quarto rendering.
 */

//! Project context and orchestration.
//!
//! A project context represents either:
//! - A Quarto project (with `_quarto.yml`)
//! - A single-file "pseudo-project" (no configuration file)
//!
//! The project context provides:
//! - Project root directory
//! - Parsed configuration
//! - List of input files
//! - Output directory resolution
//!
//! Submodules:
//! - [`index`]: cross-document index built from Pass-1 profiles.
//! - [`orchestrator`]: the [`orchestrator::ProjectType`] trait and the
//!   [`orchestrator::ProjectPipeline`] two-pass driver.
//! - [`discovery`]: multi-file project file-list expansion.

pub mod cache_key;
pub mod dependency_graph;
pub mod discovery;
pub mod index;
pub mod listing;
pub mod orchestrator;
pub mod pass2_renderer;
pub mod profile_cache;
pub mod sidebar_membership;
pub mod website_config;
// Phase 9 sub-phase 9.2 lifted the cfg gate so `flush_site_libs`
// can run cross-platform (its destination is now resolver-driven).
// Other hooks inside this module remain `#[cfg(not(wasm32))]` per
// function.
pub mod website_post_render;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::config_value::ConfigValueKind;
use quarto_system_runtime::SystemRuntime;

use crate::engine::EngineRegistry;
use crate::error::{QuartoError, Result};
use crate::extension::Extension;
use crate::render::BinaryDependencies;

/// Default output directory for a project when `project.output-dir`
/// is unset.
///
/// - **Website** projects default to `dir/_site` — this matches Q1
///   and means file discovery naturally excludes previously-rendered
///   output (see discovery rule §"excludes the output directory").
/// - All other projects (default / book / manuscript) emit beside
///   the project root. Phase-1 book / manuscript land in default
///   since they fall back to `DefaultProjectType` (see
///   `crate::project::orchestrator::project_type_for`). Their real
///   defaults will be set when those project kinds are implemented.
fn default_output_dir(dir: &Path, config: Option<&ProjectConfig>) -> PathBuf {
    match config.map(|c| c.project_kind) {
        Some(ProjectKind::Website) => dir.join("_site"),
        _ => dir.to_path_buf(),
    }
}

/// Find and parse all `_metadata.yml` files between project root and document directory.
///
/// Walks the directory hierarchy from project root to the document's parent directory,
/// looking for `_metadata.yml` or `_metadata.yaml` files. Each found file is parsed
/// and returned as a ConfigValue layer.
///
/// # Arguments
///
/// * `project` - The project context (provides project root directory)
/// * `document_path` - Path to the document being rendered
///
/// # Returns
///
/// A vector of `ConfigValue` layers, ordered from project root to document directory.
/// Each layer contains the parsed metadata from that directory's `_metadata.yml` file.
/// Directories without `_metadata.yml` are skipped.
///
/// # Behavior
///
/// - Walks directories between project root and document's parent directory
/// - Does NOT include the project root directory itself (matches TS Quarto behavior)
/// - Returns empty vec for single-file projects (no project config)
/// - Returns empty vec if document is directly in project root
///
/// # Errors
///
/// Returns an error if:
/// - A `_metadata.yml` file contains invalid YAML syntax
/// - File I/O errors occur
///
/// # Example
///
/// Given project structure:
/// ```text
/// project/
///   _quarto.yml
///   _metadata.yml          # NOT included (project root)
///   chapters/
///     _metadata.yml        # Layer 0: { toc: true }
///     intro/
///       _metadata.yml      # Layer 1: { toc-depth: 2 }
///       chapter1.qmd       # Document being rendered
/// ```
///
/// Returns: [layer0, layer1] - deeper directories later in vec
pub fn directory_metadata_for_document(
    project: &ProjectContext,
    document_path: &Path,
    runtime: &dyn SystemRuntime,
) -> Result<Vec<ConfigValue>> {
    use pampa::pandoc::yaml_to_config_value;
    use pampa::utils::diagnostic_collector::DiagnosticCollector;
    use quarto_config::InterpretationContext;

    // Single-file projects don't have directory metadata
    if project.is_single_file {
        return Ok(Vec::new());
    }

    // Canonicalize the document path so strip_prefix works reliably.
    // project.dir is always canonical (from ProjectContext::discover), but
    // callers may pass relative paths (e.g., WASM render_qmd with VFS paths).
    let document_path = runtime
        .canonicalize(document_path)
        .unwrap_or_else(|_| document_path.to_path_buf());

    let project_dir = &project.dir;
    let document_dir = document_path
        .parent()
        .ok_or_else(|| QuartoError::Other("Document has no parent directory".into()))?;

    // Get relative path from project root to document directory
    let relative_path = match document_dir.strip_prefix(project_dir) {
        Ok(rel) => rel,
        Err(_) => {
            // Document is not under project directory
            return Ok(Vec::new());
        }
    };

    // Split into directory components
    let components: Vec<_> = relative_path.components().collect();
    if components.is_empty() {
        // Document is in project root, no directories to walk
        return Ok(Vec::new());
    }

    let mut layers = Vec::new();
    let mut current_dir = project_dir.clone();

    // Walk through each directory from project root toward document
    // (but not including project root itself - we start from first subdir)
    for component in components {
        current_dir = current_dir.join(component);

        // Look for _metadata.yml or _metadata.yaml
        let metadata_path = find_metadata_file(&current_dir, runtime);

        if let Some(path) = metadata_path {
            // Parse the metadata file
            let content = runtime.file_read_string(&path).map_err(|e| {
                QuartoError::Other(format!("Failed to read {}: {}", path.display(), e))
            })?;

            let filename = path.to_string_lossy().to_string();
            let yaml = quarto_yaml::parse_file(&content, &filename).map_err(|e| {
                QuartoError::Other(format!(
                    "Directory metadata validation failed for {}: {}",
                    path.display(),
                    e
                ))
            })?;

            // Convert to ConfigValue with ProjectConfig interpretation context
            let mut diagnostics = DiagnosticCollector::new();
            let mut metadata =
                yaml_to_config_value(yaml, InterpretationContext::ProjectConfig, &mut diagnostics);

            // Adjust !path values to be relative to document directory
            adjust_paths_to_document_dir(&mut metadata, &current_dir, document_dir);

            layers.push(metadata);
        }
    }

    Ok(layers)
}

/// Find `_metadata.yml` or `_metadata.yaml` in a directory.
///
/// Returns the path to the metadata file if found, preferring `.yml` over `.yaml`.
fn find_metadata_file(dir: &Path, runtime: &dyn SystemRuntime) -> Option<PathBuf> {
    let yml_path = dir.join("_metadata.yml");
    if runtime.is_file(&yml_path).unwrap_or(false) {
        return Some(yml_path);
    }

    let yaml_path = dir.join("_metadata.yaml");
    if runtime.is_file(&yaml_path).unwrap_or(false) {
        return Some(yaml_path);
    }

    None
}

/// Adjust `!path` values in metadata to be relative to document directory.
///
/// Walks the ConfigValue tree and for each `ConfigValueKind::Path`:
/// - Computes absolute path relative to metadata_dir
/// - Recomputes relative path from document_dir
///
/// Leaves other values (strings, globs, etc.) unchanged.
pub(crate) fn adjust_paths_to_document_dir(
    metadata: &mut ConfigValue,
    metadata_dir: &Path,
    document_dir: &Path,
) {
    adjust_paths_recursive(metadata, metadata_dir, document_dir);
}

/// Recursively walk ConfigValue, adjusting Path variants.
fn adjust_paths_recursive(value: &mut ConfigValue, metadata_dir: &Path, document_dir: &Path) {
    match &mut value.value {
        ConfigValueKind::Path(path_str) => {
            let path = PathBuf::from(&*path_str);
            // Only adjust relative paths (not absolute, not URLs). Use
            // `is_rooted` (has_root), not `Path::is_relative`: on Windows a
            // POSIX-absolute path like `/usr/share/base.css` is not
            // `is_absolute` (no drive prefix) and would be wrongly rebased.
            if !quarto_util::is_rooted(&path)
                && !path_str.starts_with("http://")
                && !path_str.starts_with("https://")
            {
                let abs_path = metadata_dir.join(&path);
                if let Some(adjusted) = pathdiff::diff_paths(&abs_path, document_dir) {
                    // The adjusted value is used verbatim in HTML hrefs (e.g. a
                    // `css: !path` <link>), so it must use forward slashes on
                    // every platform; pathdiff yields native separators.
                    *path_str = quarto_util::to_forward_slashes(&adjusted);
                }
            }
        }
        ConfigValueKind::Array(items) => {
            for item in items {
                adjust_paths_recursive(item, metadata_dir, document_dir);
            }
        }
        ConfigValueKind::Map(entries) => {
            for entry in entries {
                adjust_paths_recursive(&mut entry.value, metadata_dir, document_dir);
            }
        }
        // All other kinds (Scalar, PandocInlines, Glob, Expr, etc.) - no adjustment
        _ => {}
    }
}

/// Project kind enumeration.
///
/// This is the *tag* a `_quarto.yml` selects via `project.type:`. It is
/// deliberately narrow — it carries no behavior. The orchestration hook-set
/// is implemented by the [`ProjectType`](crate::project::orchestrator::ProjectType)
/// trait, which dispatches on this tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProjectKind {
    /// Default project (individual documents)
    #[default]
    Default,
    /// Website project
    Website,
    /// Book project
    Book,
    /// Manuscript project
    Manuscript,
}

impl ProjectKind {
    /// Get the project kind name as it appears in `_quarto.yml`.
    pub fn as_str(&self) -> &'static str {
        match self {
            ProjectKind::Default => "default",
            ProjectKind::Website => "website",
            ProjectKind::Book => "book",
            ProjectKind::Manuscript => "manuscript",
        }
    }
}

impl TryFrom<&str> for ProjectKind {
    type Error = String;

    fn try_from(s: &str) -> std::result::Result<Self, Self::Error> {
        match s.to_lowercase().as_str() {
            "default" => Ok(ProjectKind::Default),
            "website" => Ok(ProjectKind::Website),
            "book" => Ok(ProjectKind::Book),
            "manuscript" => Ok(ProjectKind::Manuscript),
            _ => Err(format!("Unknown project type: {}", s)),
        }
    }
}

/// Parsed project configuration from `_quarto.yml`
#[derive(Debug, Clone, Default)]
pub struct ProjectConfig {
    /// Project kind (the tag selected by `project.type:`).
    pub project_kind: ProjectKind,

    /// Output directory (relative to project root)
    pub output_dir: Option<PathBuf>,

    /// Input file patterns (glob patterns)
    pub render_patterns: Vec<String>,

    /// Project-level `project.resources:` patterns (`bd-o8pr`).
    ///
    /// Raw patterns from `_quarto.yml` plus the YAML source location
    /// each one came from (`bd-c1et2`); glob expansion happens at
    /// project-render time. See [`crate::project_resources`] for the
    /// resolution helpers. The per-entry `source_info` is forwarded
    /// into [`crate::project_resources::ResourceError`] variants so a
    /// failed pattern can be rendered as a tidyverse-style diagnostic
    /// pointing at the offending YAML scalar.
    /// Empty when `project.resources` is absent.
    pub resources: Vec<crate::project_resources::RawResourcePattern>,

    /// Full project metadata as ConfigValue with source tracking.
    ///
    /// This is the entire `_quarto.yml` parsed with `InterpretationContext::ProjectConfig`,
    /// meaning strings are kept literal by default (no markdown parsing).
    ///
    /// Used by the render pipeline to merge project-level settings with document metadata.
    /// Format-specific settings (e.g., `format.html.toc`) are extracted using
    /// `quarto_config::resolve_format_config()` before merging.
    pub metadata: Option<ConfigValue>,

    /// Absolute path of the `_quarto.yml` (or `_quarto.yaml`) file
    /// this config was parsed from (`bd-c1et2`).
    ///
    /// `None` for default-constructed configs (single-file renders,
    /// tests). The path is used by the resource-error diagnostic
    /// path to load the YAML content into a [`SourceContext`] so
    /// Ariadne can render a source snippet for the offending scalar.
    pub config_path: Option<PathBuf>,
}

impl ProjectConfig {
    /// Create a ProjectConfig with metadata.
    ///
    /// This is useful for programmatically creating a project config
    /// (e.g., in WASM) with specific settings.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Create project config with format settings
    /// let metadata = ConfigValue::from_path(&["format", "html", "source-location"], "full");
    /// let config = ProjectConfig::with_metadata(metadata);
    /// ```
    pub fn with_metadata(metadata: ConfigValue) -> Self {
        Self {
            metadata: Some(metadata),
            ..Default::default()
        }
    }
}

/// Information about a document to be rendered
#[derive(Debug, Clone)]
pub struct DocumentInfo {
    /// Input file path (absolute)
    pub input: PathBuf,

    /// Output file path (absolute, determined by format)
    pub output: Option<PathBuf>,

    /// Document title (from front matter, if available)
    pub title: Option<String>,

    /// Document ID (for cross-references)
    pub id: Option<String>,
}

impl DocumentInfo {
    /// Create document info from an input path
    pub fn from_path(input: impl Into<PathBuf>) -> Self {
        Self {
            input: input.into(),
            output: None,
            title: None,
            id: None,
        }
    }

    /// Set the output path
    pub fn with_output(mut self, output: impl Into<PathBuf>) -> Self {
        self.output = Some(output.into());
        self
    }

    /// Get the file name without extension
    pub fn stem(&self) -> Option<&str> {
        self.input.file_stem().and_then(|s| s.to_str())
    }
}

// ── Engine registry construction ──────────────────────────────────────────────

/// Returns `true` if any extension contributes an `External` engine (i.e.
/// requires a `TsEngineHost` to be constructed).
///
/// A `Reorder`-only or empty extension list returns `false`, which means
/// `build_engine_registry` can skip the IO-creating `quarto_runtime_dir()` /
/// `quarto_data_dir()` calls entirely — restoring the pre-Task-7b behavior for
/// the common single-file / no-extension case.
#[cfg(not(target_arch = "wasm32"))]
fn any_external_engine(extensions: &[Extension]) -> bool {
    use crate::extension::types::EngineContribution;
    extensions.iter().any(|e| {
        e.contributes
            .engines
            .iter()
            .any(|c| matches!(c, EngineContribution::External { .. }))
    })
}

/// Lower a project-config [`ConfigValue`] subtree to its wire
/// [`TsMetadataValue`](crate::engine::ts_protocol::TsMetadataValue) mirror
/// (a plain JSON value).
///
/// - maps/arrays recurse (map keys stringified),
/// - bool / integer / float / null scalars map directly,
/// - every string-like value (`Scalar(String)`, `Path`, `Glob`, `Expr`, and
///   `PandocInlines`) goes through [`ConfigValue::as_plain_text`] — the
///   metadata-as-str lint lesson: a bare front-matter string can be stored as
///   `PandocInlines`, for which `as_str()` returns `None`.
///
/// Source info is dropped. Invoked on the `engines` subtree by
/// [`build_engine_config_map`] (DQ-5) and over the full merged document
/// metadata by [`document_metadata_to_ts_map`] (Plan 1c.2 P1.1b).
#[cfg(not(target_arch = "wasm32"))]
fn config_value_to_ts_metadata(cv: &ConfigValue) -> crate::engine::ts_protocol::TsMetadataValue {
    use crate::engine::ts_protocol::TsMetadataValue;
    use yaml_rust2::Yaml;

    if let Some(entries) = cv.as_map_entries() {
        let mut map = std::collections::HashMap::new();
        for entry in entries {
            map.insert(entry.key.clone(), config_value_to_ts_metadata(&entry.value));
        }
        return TsMetadataValue::Map(map);
    }
    if let Some(items) = cv.as_array() {
        return TsMetadataValue::Array(items.iter().map(config_value_to_ts_metadata).collect());
    }
    match cv.as_yaml() {
        Some(Yaml::Boolean(b)) => return TsMetadataValue::Bool(*b),
        Some(Yaml::Integer(i)) => return TsMetadataValue::Number(*i as f64),
        Some(Yaml::Real(r)) => {
            if let Ok(f) = r.parse::<f64>() {
                return TsMetadataValue::Number(f);
            }
        }
        Some(Yaml::Null) => return TsMetadataValue::Null,
        _ => {}
    }
    // String-like: Scalar(String), Path, Glob, Expr, PandocInlines.
    match cv.as_plain_text() {
        Some(s) => TsMetadataValue::String(s),
        None => TsMetadataValue::Null,
    }
}

/// Flatten a document's merged metadata (e.g. `doc_ast.ast.meta` — always a
/// top-level mapping) into the wire's flat `Record<string, TsMetadataValue>`
/// shape used by `TsFormatInfo.metadata` (P1.1b — the execute-options half of
/// DQ-5's metadata threading; distinct from [`build_engine_config_map`],
/// which populates the launch-time `config`).
///
/// Reuses [`config_value_to_ts_metadata`] — the same recursive lowering P1.1
/// built for the `engines` subtree — over the FULL merged map, and unwraps
/// its top-level `Map` variant; non-mapping input (shouldn't occur for
/// document metadata) degrades to an empty map rather than panicking.
///
/// Callable from any target: `ExecutionContext.metadata` (the field this
/// feeds) is not itself wasm-gated — only the TS-engine *consumer*
/// (`TsEngine::build_execute_options`) is native-only. On wasm32 there is no
/// TS engine to consume the value, so this returns an empty map without
/// invoking the native-only converter.
pub(crate) fn document_metadata_to_ts_map(
    meta: &ConfigValue,
) -> std::collections::HashMap<String, crate::engine::ts_protocol::TsMetadataValue> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        match config_value_to_ts_metadata(meta) {
            crate::engine::ts_protocol::TsMetadataValue::Map(m) => m,
            _ => std::collections::HashMap::new(),
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = meta;
        std::collections::HashMap::new()
    }
}

/// Build the wire `config` map carried on `LaunchEngine.project` (DQ-5).
///
/// Returns `None` when the project has no parsed metadata (single-file renders
/// use `ProjectConfig::default()`, matching Q1's `context.config ? … :
/// undefined`). Otherwise builds `{ "engines": Array(…), "output-dir":
/// String(…) }`, **omitting** a key when its source is absent:
/// - `engines` ← `metadata.get("engines")` lowered via
///   [`config_value_to_ts_metadata`] (the only ConfigValue lowering),
/// - `output-dir` ← the RAW relative typed `ProjectConfig.output_dir`.
///
/// # Wire shape note (top-level `output-dir`, not nested under `project`)
///
/// The engine-host-deno harness's `reconstructRichProject` reads a
/// **top-level** `config["output-dir"]` (or `config["outputDir"]`) and *bridges*
/// it into the rich Q1 `EngineProjectContext.config.project.outputDir` the
/// engine actually sees. So the wire carries `output-dir` flat; the host
/// produces the nested `project.outputDir` on the engine side. (This differs
/// from Plan 1c.2's "Shape: `project: { output-dir }`" prose and the wire-spec
/// interface annotation, which describe the *rich* nesting; the shipped host is
/// the authoritative consumer. See the P1.1 report for the discrepancy.)
#[cfg(not(target_arch = "wasm32"))]
fn build_engine_config_map(
    config: &ProjectConfig,
) -> Option<std::collections::HashMap<String, crate::engine::ts_protocol::TsMetadataValue>> {
    use crate::engine::ts_protocol::TsMetadataValue;

    let metadata = config.metadata.as_ref()?;
    let mut map = std::collections::HashMap::new();

    if let Some(engines) = metadata.get("engines") {
        map.insert("engines".to_string(), config_value_to_ts_metadata(engines));
    }

    if let Some(output_dir) = &config.output_dir {
        map.insert(
            "output-dir".to_string(),
            TsMetadataValue::String(output_dir.to_string_lossy().into_owned()),
        );
    }

    Some(map)
}

/// Build the project's engine registry: built-ins + every extension-contributed
/// engine, plus the shared `TsEngineHost` (constructed lazily — only when at
/// least one `External` engine contribution is present).
///
/// Errors on: missing .js bundle, name collision, or a `Reorder` hint naming an
/// unregistered engine.
///
/// Native-only: `TsEngine` / `TsEngineHost` are not available on wasm32.
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
fn build_engine_registry(
    extensions: &[Extension],
    project_dir: Option<&Path>,
    is_single_file: bool,
    output_dir: &Path,
    config: Option<&ProjectConfig>,
    binary_dependencies: &BinaryDependencies,
    runtime: &dyn SystemRuntime,
) -> Result<Arc<EngineRegistry>> {
    use std::collections::{HashMap, HashSet};

    use crate::engine::TsEngine;
    use crate::engine::ts_process::TsEngineHost;
    use crate::engine::ts_protocol::{EngineProjectContext, HostGlobalConfig};
    use crate::extension::BUILTIN_EXTENSIONS;
    use crate::extension::types::{EngineContribution, engine_contribution_missing_fields_warning};

    // ── Per-render project context (P1.1 / DQ-5) ─────────────────────────────
    // Built once from the ProjectContext fields the caller resolved and set on
    // every TsEngine at construction (the concrete type is erased at
    // registry.register, so the TsEngine::new site is the dominating write
    // point). First-write-wins: launch caches make any later write inert.
    let engine_project_context = EngineProjectContext {
        project_dir: project_dir.map(|p| p.to_string_lossy().into_owned()),
        is_single_file,
        output_dir: Some(output_dir.to_string_lossy().into_owned()),
        config: config.and_then(build_engine_config_map),
    };

    // ── Needs-host check ──────────────────────────────────────────────────────
    // Skip HostGlobalConfig / TsEngineHost construction — and the directory-
    // creating quarto_runtime_dir() / quarto_data_dir() calls they imply — when
    // no extension contributes an External engine. A Reorder hint alone only
    // references an existing built-in and is validated in step 6 without a host.
    let needs_host = any_external_engine(extensions);

    // ── Step 1: Built-ins registry (markdown / knitr / jupyter) ──────────────
    let mut registry = EngineRegistry::new();

    // Track key → contributor label for collision detection.
    let mut key_to_contributor: HashMap<String, String> = {
        let mut m = HashMap::new();
        m.insert("markdown".to_string(), "built-in".to_string());
        m.insert("knitr".to_string(), "built-in".to_string());
        m.insert("jupyter".to_string(), "built-in".to_string());
        m
    };

    let mut order: Vec<String> = Vec::new();

    if needs_host {
        // ── Step 2: Build the process-stable HostGlobalConfig ────────────────
        let resource_dir = BUILTIN_EXTENSIONS
            .path()
            .ok()
            .map(|p| p.display().to_string())
            .unwrap_or_default();

        let runtime_dir = quarto_util::quarto_runtime_dir()
            .map_err(|e| QuartoError::Other(format!("failed to locate Quarto runtime dir: {}", e)))?
            .display()
            .to_string();

        let data_dir = quarto_util::quarto_data_dir()
            .map_err(|e| QuartoError::Other(format!("failed to locate Quarto data dir: {}", e)))?
            .display()
            .to_string();

        let pandoc_path = binary_dependencies
            .pandoc
            .as_ref()
            .map(|p| p.display().to_string());

        let global = HostGlobalConfig {
            resource_dir,
            runtime_dir,
            data_dir,
            pandoc_path,
            is_interactive_session: runtime.is_interactive(),
            running_in_ci: runtime.running_in_ci(),
            quarto_version: crate::version().to_string(),
        };

        // ── Step 3: Build the shared host (NOT spawned; first round-trip spawns) ──
        let host = Arc::new(TsEngineHost::new(global));

        // ── Step 4: Process each extension's engine contributions ─────────────────
        for ext in extensions {
            let ext_label = ext.id.to_string();

            for contribution in &ext.contributes.engines {
                match contribution {
                    // Reorder hint: record in order list, do NOT register
                    EngineContribution::Reorder { name } => {
                        order.push(name.clone());
                    }

                    EngineContribution::External {
                        path,
                        name,
                        claims,
                        file_extensions,
                        claims_files,
                    } => {
                        // Step 4a: Bundle-exists check
                        if !path.exists() {
                            return Err(QuartoError::Other(format!(
                                "Engine extension '{}' has no bundled .js file at {}. \
                                Run 'q2 build-ts-extension' in {} to build it.",
                                ext_label,
                                path.display(),
                                ext.path.display()
                            )));
                        }

                        // Step 4b: Registry key and name_declared flag
                        let key = name.clone().unwrap_or_else(|| ext.id.to_string());
                        let name_declared = name.is_some();

                        // Step 4c: Collision check (P1-4)
                        if registry.has_engine(&key) {
                            let prev = key_to_contributor
                                .get(&key)
                                .map_or("unknown", |s| s.as_str());
                            return Err(QuartoError::Other(format!(
                                "Engine name collision: both '{}' and '{}' register engine '{}'. \
                                Engine names must be unique.",
                                prev, ext_label, key
                            )));
                        }

                        // Step 4d: Construct TsEngine and register
                        let engine = TsEngine::new(
                            key.clone(),
                            name_declared,
                            path.clone(),
                            Arc::clone(&host),
                            claims.clone(),
                            file_extensions.clone(),
                            claims_files.clone(),
                            ext.id.clone(),
                            registry.aliases.clone(),
                            registry.diagnostics.clone(),
                        );
                        // P1.1: thread the per-render project context in before the
                        // engine is launched. Construction time is the dominating
                        // write point — the registry erases the concrete type at
                        // `register`, and launch caches make later writes inert.
                        engine.set_project(engine_project_context.clone());
                        registry.register(Arc::new(engine));
                        key_to_contributor.insert(key.clone(), ext_label.clone());

                        // External engines push their name into the user-specified order.
                        order.push(key.clone());

                        // Step 4e: Missing-static-fields warning
                        if let Some(w) =
                            engine_contribution_missing_fields_warning(&ext_label, contribution)
                        {
                            registry.diagnostics.lock().unwrap().push(w);
                        }
                    }
                }
            }
        }
    } else {
        // No External contributions. Collect only Reorder hints into the order
        // vec. No host construction, no subprocess, no quarto_runtime_dir() /
        // quarto_data_dir() IO — zero additional disk operations vs. pre-Task-7b.
        for ext in extensions {
            for contribution in &ext.contributes.engines {
                if let EngineContribution::Reorder { name } = contribution {
                    order.push(name.clone());
                }
            }
        }
    }

    // ── Step 5: Set contribution_order (dedup, first-occurrence wins) ─────────
    let mut seen = HashSet::new();
    registry.contribution_order = order
        .into_iter()
        .filter(|n| seen.insert(n.clone()))
        .collect();
    // Task 9: splice _quarto.yml engines: list here for full resolution ordering.

    // ── Step 6: Validate every name in contribution_order is registered (P1-3) ─
    for name in &registry.contribution_order {
        if !registry.has_engine(name) {
            let mut available = registry.engine_names();
            available.sort_unstable();
            return Err(QuartoError::Other(format!(
                "'{}' was specified in the list of engines but it is not a valid engine. \
                Available engines are: {}",
                name,
                available.join(", ")
            )));
        }
    }

    // ── Step 7: Return ────────────────────────────────────────────────────────
    // Task: drain registry.diagnostics at orchestrator (plan step 10)
    Ok(Arc::new(registry))
}

/// Discover extensions -- the cheap, host-free parse half of the old
/// `discover_extensions_and_build_registry` (Corollary-0 split, plan 1c.2
/// task 2b). Split out so `ProjectContext::discover`'s walk can consult
/// contributed engine claims-files (via `RenderableExtensions::new`) BEFORE
/// walking, instead of after.
fn discover_extensions_only(
    discovery_anchor: &Path,
    project_dir: Option<&Path>,
    runtime: &dyn SystemRuntime,
) -> Vec<Extension> {
    // Builtin extensions dir — present on native, irrelevant on WASM (VFS path).
    #[cfg(not(target_arch = "wasm32"))]
    let builtin_dir = crate::extension::BUILTIN_EXTENSIONS
        .path()
        .ok()
        .map(|p| p.to_path_buf());
    #[cfg(target_arch = "wasm32")]
    let builtin_dir: Option<PathBuf> = None;

    crate::extension::discover_extensions(
        discovery_anchor,
        project_dir,
        builtin_dir.as_deref(),
        runtime,
    )
}

/// Build the engine registry from already-discovered extensions -- the
/// host-touching finalize half of the old `discover_extensions_and_build_registry`
/// (Corollary-0 split, plan 1c.2 task 2b). Internally unchanged: still calls
/// `build_engine_registry`, preserving P1.1's `set_project` threading
/// (`build_engine_registry` -> `EngineProjectContext` -> `TsEngine::set_project`).
#[allow(clippy::too_many_arguments)]
fn build_registry(
    extensions: &[Extension],
    project_dir: Option<&Path>,
    is_single_file: bool,
    output_dir: &Path,
    config: Option<&ProjectConfig>,
    binary_dependencies: &BinaryDependencies,
    runtime: &dyn SystemRuntime,
) -> Result<Arc<EngineRegistry>> {
    #[cfg(not(target_arch = "wasm32"))]
    let registry = build_engine_registry(
        extensions,
        project_dir,
        is_single_file,
        output_dir,
        config,
        binary_dependencies,
        runtime,
    )?;
    #[cfg(target_arch = "wasm32")]
    let registry = {
        // WASM has no subprocess engine host, so binary_dependencies (used only
        // to build the host's global config on native) is unused here — and the
        // per-render project-context fields (plus the parsed `extensions`, which
        // only `build_engine_registry` consumes on native) are inert here too.
        let _ = (
            extensions,
            project_dir,
            is_single_file,
            output_dir,
            config,
            binary_dependencies,
            runtime,
        );
        Arc::new(EngineRegistry::new())
    };

    Ok(registry)
}

/// Discover extensions and build the engine registry in one step.
///
/// Thin wrapper over [`discover_extensions_only`] + [`build_registry`],
/// kept for `ProjectContext::single_file` -- which has no walk to interleave
/// between the two halves, so the pre-2b one-step shape stays behaviorally
/// identical. `ProjectContext::discover` calls the two halves directly
/// (task 2b) so its walk can run between them.
#[allow(clippy::too_many_arguments)]
fn discover_extensions_and_build_registry(
    discovery_anchor: &Path,
    project_dir: Option<&Path>,
    is_single_file: bool,
    output_dir: &Path,
    config: Option<&ProjectConfig>,
    binary_dependencies: &BinaryDependencies,
    runtime: &dyn SystemRuntime,
) -> Result<(Vec<Extension>, Arc<EngineRegistry>)> {
    let extensions = discover_extensions_only(discovery_anchor, project_dir, runtime);
    let registry = build_registry(
        &extensions,
        project_dir,
        is_single_file,
        output_dir,
        config,
        binary_dependencies,
        runtime,
    )?;
    Ok((extensions, registry))
}

// ── Project context ────────────────────────────────────────────────────────────

/// Project context for rendering
#[derive(Debug, Clone, Default)]
pub struct ProjectContext {
    /// Project root directory (directory containing `_quarto.yml`, or input file directory)
    pub dir: PathBuf,

    /// Parsed project configuration.
    ///
    /// Always present: real projects get their parsed `_quarto.yml`,
    /// single-file renders get `ProjectConfig::default()`.
    pub config: ProjectConfig,

    /// Is this a single-file pseudo-project?
    pub is_single_file: bool,

    /// List of input files to render
    pub files: Vec<DocumentInfo>,

    /// Output directory (resolved, absolute path)
    pub output_dir: PathBuf,

    /// Engine registry built once at project construction (built-ins now; Task 7b adds
    /// extension engines). Shared via `Arc` so per-file `StageContext` clones are cheap.
    pub registry: Arc<EngineRegistry>,

    /// Extensions discovered at the project level (Task 7b uses these to build the registry;
    /// hoisted from per-document discovery in a later task).
    pub extensions: Vec<Extension>,

    /// Discovered binary dependencies (pandoc/dart-sass/esbuild). Task 7b reads `pandoc` for the
    /// engine host's global config.
    pub binary_dependencies: BinaryDependencies,
}

impl ProjectContext {
    /// Discover project context from a path.
    ///
    /// If the path is a file, looks for `_quarto.yml` in parent directories.
    /// If the path is a directory, looks for `_quarto.yml` in that directory and parents.
    ///
    /// If no `_quarto.yml` is found, creates a single-file pseudo-project.
    pub fn discover(path: impl AsRef<Path>, runtime: &dyn SystemRuntime) -> Result<Self> {
        let path = path.as_ref();

        // Canonicalize the path
        let path = runtime
            .canonicalize(path)
            .map_err(|e| QuartoError::Other(format!("Failed to canonicalize path: {}", e)))?;

        // Determine if this is a file or directory
        let is_file = runtime
            .is_file(&path)
            .map_err(|e| QuartoError::Other(format!("Failed to check path type: {}", e)))?;
        let is_dir = runtime
            .is_dir(&path)
            .map_err(|e| QuartoError::Other(format!("Failed to check path type: {}", e)))?;

        let (search_dir, input_file) = if is_file {
            (
                path.parent()
                    .ok_or_else(|| QuartoError::Other("Input file has no parent directory".into()))?
                    .to_path_buf(),
                Some(path.clone()),
            )
        } else if is_dir {
            (path.clone(), None)
        } else {
            return Err(QuartoError::Other(format!(
                "Path does not exist: {}",
                path.display()
            )));
        };

        // Search for _quarto.yml
        let (project_dir, config) = Self::find_project_config(&search_dir, runtime)?;

        // Determine if this is a single-file project
        let is_single_file = config.is_none() && input_file.is_some();

        // Use project dir if found, otherwise use search dir
        let dir = project_dir.unwrap_or(search_dir);

        // Determine output directory
        let output_dir = config
            .as_ref()
            .and_then(|c| c.output_dir.as_ref())
            .map_or_else(
                || default_output_dir(&dir, config.as_ref()),
                |o| dir.join(o),
            );

        // Capture single-file input path for extension discovery before it's consumed below.
        let single_file_input = input_file.clone();

        // Anchor for `discover_extensions`:
        // - single-file: use the actual input file so start_dir = its parent.
        // - project: use `dir/_quarto.yml` so start_dir = dir (searches dir/_extensions).
        let discovery_anchor: PathBuf = if is_single_file {
            single_file_input.unwrap_or_else(|| dir.clone())
        } else {
            dir.join("_quarto.yml")
        };
        let project_dir_for_extensions: Option<&Path> =
            if is_single_file { None } else { Some(&dir) };

        // 2b (Corollary-0 split): parse extensions BEFORE the walk, so the
        // walk can consult contributed engine claims-files via
        // `RenderableExtensions`. The registry (host-touching half) still
        // builds AFTER the walk, unchanged in shape from pre-2b.
        let extensions =
            discover_extensions_only(&discovery_anchor, project_dir_for_extensions, runtime);

        // Build file list
        let files = if let Some(input) = input_file {
            vec![DocumentInfo::from_path(input)]
        } else {
            // Multi-file project: walk the project directory, honoring
            // `project.render` globs if provided.
            let render_patterns = config
                .as_ref()
                .map(|c| c.render_patterns.clone())
                .unwrap_or_default();
            // 2b: real FIXED_RENDERABLE union of engine claims-files
            // extensions, computed from the extensions parsed above.
            // Target-agnostic by construction: on WASM, `discover_extensions`
            // naturally yields fewer/no External engines (no subprocess
            // engine host there), so `claimed_file_extensions` returns `[]`
            // for those and this degrades to `{qmd}` without a wasm #[cfg]
            // branch of its own.
            let renderable_extensions = discovery::RenderableExtensions::new(
                extensions
                    .iter()
                    .flat_map(|e| &e.contributes.engines)
                    .flat_map(crate::extension::types::claimed_file_extensions),
            );
            let discovery_cfg = discovery::DiscoveryConfig {
                project_dir: &dir,
                output_dir: &output_dir,
                render_patterns: &render_patterns,
                renderable_extensions: &renderable_extensions,
            };
            let paths = discovery::discover_project_files(&discovery_cfg, runtime)?;
            paths.into_iter().map(DocumentInfo::from_path).collect()
        };

        let binary_dependencies = BinaryDependencies::discover(runtime);

        let registry = build_registry(
            &extensions,
            project_dir_for_extensions,
            is_single_file,
            &output_dir,
            config.as_ref(),
            &binary_dependencies,
            runtime,
        )?;

        Ok(Self {
            dir,
            config: config.unwrap_or_default(),
            is_single_file,
            files,
            output_dir,
            registry,
            extensions,
            binary_dependencies,
        })
    }

    /// Create a single-file project context directly
    pub fn single_file(input: impl AsRef<Path>, runtime: &dyn SystemRuntime) -> Result<Self> {
        let input = input.as_ref();

        let input = runtime
            .canonicalize(input)
            .map_err(|e| QuartoError::Other(format!("Failed to canonicalize path: {}", e)))?;

        let dir = input
            .parent()
            .ok_or_else(|| QuartoError::Other("Input file has no parent directory".into()))?
            .to_path_buf();

        let binary_dependencies = BinaryDependencies::discover(runtime);

        // Single-file: no project_dir, search only input's directory.
        // (No production callers — grep 2026-07-02 — but keep the per-render
        // context consistent with `discover`'s single-file branch: no project
        // dir, single-file true, output dir = the input's directory, no config.)
        let (extensions, registry) = discover_extensions_and_build_registry(
            &input,
            None,
            true,
            &dir,
            None,
            &binary_dependencies,
            runtime,
        )?;

        Ok(Self {
            dir: dir.clone(),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path(input)],
            output_dir: dir,
            registry,
            extensions,
            binary_dependencies,
        })
    }

    /// Search for `_quarto.yml` in directory and parents
    fn find_project_config(
        start_dir: &Path,
        runtime: &dyn SystemRuntime,
    ) -> Result<(Option<PathBuf>, Option<ProjectConfig>)> {
        let mut current = start_dir.to_path_buf();

        loop {
            let config_path = current.join("_quarto.yml");
            let exists = runtime
                .path_exists(&config_path, None)
                .map_err(|e| QuartoError::Other(format!("Failed to check config path: {}", e)))?;
            if exists {
                // Found config file - parse it
                let config = Self::parse_config(&config_path, runtime)?;
                return Ok((Some(current), Some(config)));
            }

            // Also check for _quarto.yaml (alternate extension)
            let config_path_yaml = current.join("_quarto.yaml");
            let exists_yaml = runtime
                .path_exists(&config_path_yaml, None)
                .map_err(|e| QuartoError::Other(format!("Failed to check config path: {}", e)))?;
            if exists_yaml {
                let config = Self::parse_config(&config_path_yaml, runtime)?;
                return Ok((Some(current), Some(config)));
            }

            // Move to parent directory
            if let Some(parent) = current.parent() {
                current = parent.to_path_buf();
            } else {
                // Reached root, no config found
                return Ok((None, None));
            }
        }
    }

    /// Parse a `_quarto.yml` file
    fn parse_config(path: &Path, runtime: &dyn SystemRuntime) -> Result<ProjectConfig> {
        use pampa::pandoc::yaml_to_config_value;
        use pampa::utils::diagnostic_collector::DiagnosticCollector;
        use quarto_config::InterpretationContext;

        let content = runtime
            .file_read_string(path)
            .map_err(|e| QuartoError::Other(format!("Failed to read config file: {}", e)))?;

        let filename = path.to_string_lossy().to_string();

        // Parse YAML with source tracking
        let yaml = quarto_yaml::parse_file(&content, &filename).map_err(|e| {
            QuartoError::Other(format!("Failed to parse {}: {}", path.display(), e))
        })?;

        // Convert to ConfigValue with ProjectConfig interpretation context
        // (strings are kept literal, not parsed as markdown)
        let mut diagnostics = DiagnosticCollector::new();
        let metadata =
            yaml_to_config_value(yaml, InterpretationContext::ProjectConfig, &mut diagnostics);

        // Extract project-specific settings from metadata
        let project_kind = metadata
            .get("project")
            .and_then(|p| p.get("type"))
            .and_then(|t| t.as_str())
            .and_then(|s| ProjectKind::try_from(s).ok())
            .unwrap_or_default();

        let output_dir = metadata
            .get("project")
            .and_then(|p| p.get("output-dir"))
            .and_then(|o| o.as_str())
            .map(PathBuf::from);

        let render_patterns = metadata
            .get("project")
            .and_then(|p| p.get("render"))
            .and_then(|r| r.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let resources = crate::project_resources::extract_resource_patterns(
            &metadata,
            &["project", "resources"],
        );

        Ok(ProjectConfig {
            project_kind,
            output_dir,
            render_patterns,
            resources,
            metadata: Some(metadata),
            config_path: Some(path.to_path_buf()),
        })
    }

    /// Get the project kind.
    pub fn project_kind(&self) -> ProjectKind {
        self.config.project_kind
    }

    /// Check if this is a multi-document project
    pub fn is_multi_document(&self) -> bool {
        !self.is_single_file
            && matches!(
                self.project_kind(),
                ProjectKind::Website | ProjectKind::Book | ProjectKind::Manuscript
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === ProjectKind tests ===

    #[test]
    fn test_project_kind_from_string() {
        assert_eq!(
            ProjectKind::try_from("website").unwrap(),
            ProjectKind::Website
        );
        assert_eq!(ProjectKind::try_from("book").unwrap(), ProjectKind::Book);
        assert_eq!(
            ProjectKind::try_from("default").unwrap(),
            ProjectKind::Default
        );
        assert!(ProjectKind::try_from("unknown").is_err());
    }

    #[test]
    fn test_project_kind_from_string_manuscript() {
        assert_eq!(
            ProjectKind::try_from("manuscript").unwrap(),
            ProjectKind::Manuscript
        );
    }

    #[test]
    fn test_project_kind_from_string_case_insensitive() {
        // Test uppercase
        assert_eq!(
            ProjectKind::try_from("WEBSITE").unwrap(),
            ProjectKind::Website
        );
        assert_eq!(ProjectKind::try_from("BOOK").unwrap(), ProjectKind::Book);
        assert_eq!(
            ProjectKind::try_from("DEFAULT").unwrap(),
            ProjectKind::Default
        );
        assert_eq!(
            ProjectKind::try_from("MANUSCRIPT").unwrap(),
            ProjectKind::Manuscript
        );

        // Test mixed case
        assert_eq!(
            ProjectKind::try_from("WebSite").unwrap(),
            ProjectKind::Website
        );
        assert_eq!(ProjectKind::try_from("Book").unwrap(), ProjectKind::Book);
    }

    #[test]
    fn test_project_kind_from_string_error_message() {
        let result = ProjectKind::try_from("invalid");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Unknown project type"));
        assert!(err.contains("invalid"));
    }

    #[test]
    fn test_project_kind_as_str() {
        assert_eq!(ProjectKind::Default.as_str(), "default");
        assert_eq!(ProjectKind::Website.as_str(), "website");
        assert_eq!(ProjectKind::Book.as_str(), "book");
        assert_eq!(ProjectKind::Manuscript.as_str(), "manuscript");
    }

    #[test]
    fn test_project_kind_default() {
        let default_type: ProjectKind = Default::default();
        assert_eq!(default_type, ProjectKind::Default);
    }

    #[test]
    fn test_project_kind_clone_and_copy() {
        let original = ProjectKind::Website;
        let cloned = original;
        let copied = original; // Copy trait
        assert_eq!(original, cloned);
        assert_eq!(original, copied);
    }

    #[test]
    fn test_project_kind_eq() {
        assert_eq!(ProjectKind::Website, ProjectKind::Website);
        assert_ne!(ProjectKind::Website, ProjectKind::Book);
    }

    // === ProjectConfig tests ===

    #[test]
    fn test_project_config_default() {
        let config = ProjectConfig::default();
        assert_eq!(config.project_kind, ProjectKind::Default);
        assert!(config.output_dir.is_none());
        assert!(config.render_patterns.is_empty());
        assert!(config.metadata.is_none());
    }

    #[test]
    fn test_project_config_with_metadata() {
        use quarto_pandoc_types::ConfigValue;
        use quarto_source_map::SourceInfo;

        let metadata = ConfigValue::new_string("test", SourceInfo::for_test());
        let config = ProjectConfig::with_metadata(metadata.clone());

        assert_eq!(config.project_kind, ProjectKind::Default);
        assert!(config.output_dir.is_none());
        assert!(config.render_patterns.is_empty());
        assert!(config.metadata.is_some());
    }

    // === DocumentInfo tests ===

    #[test]
    fn test_document_info() {
        let doc = DocumentInfo::from_path("/path/to/doc.qmd").with_output("/path/to/doc.html");

        assert_eq!(doc.input, PathBuf::from("/path/to/doc.qmd"));
        assert_eq!(doc.output, Some(PathBuf::from("/path/to/doc.html")));
        assert_eq!(doc.stem(), Some("doc"));
    }

    #[test]
    fn test_document_info_from_path_only() {
        let doc = DocumentInfo::from_path("/path/to/file.qmd");

        assert_eq!(doc.input, PathBuf::from("/path/to/file.qmd"));
        assert!(doc.output.is_none());
        assert!(doc.title.is_none());
        assert!(doc.id.is_none());
    }

    #[test]
    fn test_document_info_stem_no_extension() {
        let doc = DocumentInfo::from_path("/path/to/README");
        assert_eq!(doc.stem(), Some("README"));
    }

    #[test]
    fn test_document_info_stem_hidden_file() {
        let doc = DocumentInfo::from_path("/path/to/.gitignore");
        assert_eq!(doc.stem(), Some(".gitignore"));
    }

    #[test]
    fn test_document_info_stem_multiple_dots() {
        let doc = DocumentInfo::from_path("/path/to/file.test.qmd");
        assert_eq!(doc.stem(), Some("file.test"));
    }

    #[test]
    fn test_document_info_with_output_chaining() {
        let doc = DocumentInfo::from_path("/input.qmd").with_output("/output.html");

        assert_eq!(doc.input, PathBuf::from("/input.qmd"));
        assert_eq!(doc.output, Some(PathBuf::from("/output.html")));
    }

    #[test]
    fn test_document_info_clone() {
        let doc = DocumentInfo::from_path("/path/to/doc.qmd").with_output("/path/to/doc.html");
        let cloned = doc.clone();

        assert_eq!(doc.input, cloned.input);
        assert_eq!(doc.output, cloned.output);
    }

    // === ProjectContext tests (unit tests for methods that don't need runtime) ===

    #[test]
    fn test_project_context_project_kind_with_config() {
        let context = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig {
                project_kind: ProjectKind::Website,
                ..Default::default()
            },
            is_single_file: false,
            files: vec![],
            output_dir: PathBuf::from("/project/_site"),

            ..Default::default()
        };

        assert_eq!(context.project_kind(), ProjectKind::Website);
    }

    #[test]
    fn test_project_context_project_kind_without_config() {
        let context = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![],
            output_dir: PathBuf::from("/project"),

            ..Default::default()
        };

        assert_eq!(context.project_kind(), ProjectKind::Default);
    }

    #[test]
    fn test_project_context_is_multi_document_website() {
        let context = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig {
                project_kind: ProjectKind::Website,
                ..Default::default()
            },
            is_single_file: false,
            files: vec![],
            output_dir: PathBuf::from("/project/_site"),

            ..Default::default()
        };

        assert!(context.is_multi_document());
    }

    #[test]
    fn test_project_context_is_multi_document_book() {
        let context = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig {
                project_kind: ProjectKind::Book,
                ..Default::default()
            },
            is_single_file: false,
            files: vec![],
            output_dir: PathBuf::from("/project/_book"),

            ..Default::default()
        };

        assert!(context.is_multi_document());
    }

    #[test]
    fn test_project_context_is_multi_document_manuscript() {
        let context = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig {
                project_kind: ProjectKind::Manuscript,
                ..Default::default()
            },
            is_single_file: false,
            files: vec![],
            output_dir: PathBuf::from("/project/_manuscript"),

            ..Default::default()
        };

        assert!(context.is_multi_document());
    }

    #[test]
    fn test_project_context_is_multi_document_default_type() {
        // Default project type is NOT multi-document
        let context = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig {
                project_kind: ProjectKind::Default,
                ..Default::default()
            },
            is_single_file: false,
            files: vec![],
            output_dir: PathBuf::from("/project"),

            ..Default::default()
        };

        assert!(!context.is_multi_document());
    }

    #[test]
    fn test_project_context_is_multi_document_single_file() {
        // Single file projects are never multi-document, even if type is Website
        let context = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig {
                project_kind: ProjectKind::Website,
                ..Default::default()
            },
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/index.qmd")],
            output_dir: PathBuf::from("/project"),

            ..Default::default()
        };

        assert!(!context.is_multi_document());
    }

    #[test]
    fn test_project_context_is_multi_document_no_config() {
        // No config means single-file pseudo-project, not multi-document
        let context = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![],
            output_dir: PathBuf::from("/project"),

            ..Default::default()
        };

        assert!(!context.is_multi_document());
    }

    // === any_external_engine predicate tests ===
    //
    // These unit tests bind the needs_host predicate used in build_engine_registry
    // to gate the IO-creating quarto_runtime_dir()/quarto_data_dir() calls.
    // The predicate is false when no extension contributes an External engine, so
    // a no-extension or Reorder-only project does zero extra IO.

    #[cfg(not(target_arch = "wasm32"))]
    mod needs_host_tests {
        use std::collections::HashMap;
        use std::path::PathBuf;

        use crate::extension::Extension;
        use crate::extension::types::{Contributes, EngineContribution, ExtensionId};

        use super::any_external_engine;

        fn make_ext(contributions: Vec<EngineContribution>) -> Extension {
            Extension {
                id: ExtensionId::new("test-ext"),
                title: "Test".to_string(),
                author: "Test".to_string(),
                version: None,
                quarto_required: None,
                path: PathBuf::from("/test"),
                contributes: Contributes {
                    engines: contributions,
                    ..Default::default()
                },
            }
        }

        #[test]
        fn needs_host_false_for_no_extensions() {
            // Empty extension list → no host IO needed.
            assert!(!any_external_engine(&[]));
        }

        #[test]
        fn needs_host_false_for_reorder_only() {
            // A Reorder hint is just a priority annotation — no subprocess or
            // dir-creation is needed to process it.
            let ext = make_ext(vec![EngineContribution::Reorder {
                name: "knitr".to_string(),
            }]);
            assert!(!any_external_engine(&[ext]));
        }

        #[test]
        fn needs_host_true_for_external_engine() {
            // An External contribution requires TsEngineHost → needs_host = true.
            let ext = make_ext(vec![EngineContribution::External {
                path: PathBuf::from("/test/engine.js"),
                name: Some("test-engine".to_string()),
                claims: Some(HashMap::new()),
                file_extensions: Some(vec![]),
                claims_files: Some(vec![]),
            }]);
            assert!(any_external_engine(&[ext]));
        }

        #[test]
        fn needs_host_true_when_external_mixed_with_reorder() {
            // Even one External among multiple Reorders → true.
            let ext = make_ext(vec![
                EngineContribution::Reorder {
                    name: "knitr".to_string(),
                },
                EngineContribution::External {
                    path: PathBuf::from("/test/engine.js"),
                    name: None,
                    claims: None,
                    file_extensions: None,
                    claims_files: None,
                },
            ]);
            assert!(any_external_engine(&[ext]));
        }
    }

    // === ProjectContext::discover and ::single_file tests ===

    mod discover_tests {
        use super::*;
        use quarto_system_runtime::NativeRuntime;
        use std::fs;
        use tempfile::TempDir;

        #[test]
        fn test_discover_without_quarto_yml_has_default_config() {
            // A path with no _quarto.yml should get a default config, not None
            let temp = TempDir::new().unwrap();
            let qmd_path = temp.path().join("doc.qmd");
            fs::write(&qmd_path, "# Hello\n").unwrap();

            let runtime = NativeRuntime::new();
            let ctx = ProjectContext::discover(&qmd_path, &runtime).unwrap();

            assert!(ctx.is_single_file);
            // Config should be default with no metadata
            assert_eq!(ctx.config.project_kind, ProjectKind::Default);
            assert!(ctx.config.metadata.is_none());
        }

        #[test]
        fn test_single_file_has_default_config() {
            let temp = TempDir::new().unwrap();
            let qmd_path = temp.path().join("doc.qmd");
            fs::write(&qmd_path, "# Hello\n").unwrap();

            let runtime = NativeRuntime::new();
            let ctx = ProjectContext::single_file(&qmd_path, &runtime).unwrap();

            assert!(ctx.is_single_file);
            // Config should be default with no metadata
            assert_eq!(ctx.config.project_kind, ProjectKind::Default);
            assert!(ctx.config.metadata.is_none());
        }
    }

    // === Directory Metadata tests ===

    mod directory_metadata_tests {
        use super::*;
        use quarto_system_runtime::NativeRuntime;
        use std::fs;
        use tempfile::TempDir;

        /// Helper to create a project context for testing.
        /// Canonicalizes the dir to match what ProjectContext::discover does,
        /// ensuring strip_prefix works correctly (e.g., on macOS where
        /// /tmp symlinks to /private/tmp).
        fn test_project_context(dir: &Path) -> ProjectContext {
            let canonical = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
            ProjectContext {
                dir: canonical.clone(),
                config: ProjectConfig::default(),
                is_single_file: false,
                files: vec![],
                output_dir: canonical,

                ..Default::default()
            }
        }

        fn native_runtime() -> NativeRuntime {
            NativeRuntime::new()
        }

        #[test]
        fn test_directory_metadata_empty() {
            // Project with no _metadata.yml files returns empty vec
            let temp = TempDir::new().unwrap();
            let project = test_project_context(temp.path());
            let doc_path = temp.path().join("doc.qmd");

            let result =
                directory_metadata_for_document(&project, &doc_path, &native_runtime()).unwrap();

            assert!(result.is_empty());
        }

        #[test]
        fn test_directory_metadata_single_file_in_subdir() {
            // project/
            //   chapters/
            //     _metadata.yml  { toc: true }
            //     doc.qmd
            // Returns: [{ toc: true }]
            let temp = TempDir::new().unwrap();
            let chapters = temp.path().join("chapters");
            fs::create_dir(&chapters).unwrap();
            fs::write(chapters.join("_metadata.yml"), "toc: true\n").unwrap();
            fs::write(chapters.join("doc.qmd"), "# Test\n").unwrap();

            let project = test_project_context(temp.path());
            let doc_path = chapters.join("doc.qmd");

            let result =
                directory_metadata_for_document(&project, &doc_path, &native_runtime()).unwrap();

            assert_eq!(result.len(), 1);
            assert_eq!(result[0].get("toc").unwrap().as_bool(), Some(true));
        }

        #[test]
        fn test_directory_metadata_hierarchy() {
            // project/
            //   _metadata.yml     { theme: "cosmo" }
            //   chapters/
            //     _metadata.yml   { toc: true }
            //     intro/
            //       _metadata.yml { toc-depth: 2 }
            //       doc.qmd
            // Returns: [{ theme }, { toc }, { toc-depth }] in order
            let temp = TempDir::new().unwrap();

            // Root _metadata.yml - NOTE: TS Quarto walks from first subdir, not root
            // But we should include root if document is in subdir
            fs::write(temp.path().join("_metadata.yml"), "theme: cosmo\n").unwrap();

            // chapters/_metadata.yml
            let chapters = temp.path().join("chapters");
            fs::create_dir(&chapters).unwrap();
            fs::write(chapters.join("_metadata.yml"), "toc: true\n").unwrap();

            // chapters/intro/_metadata.yml
            let intro = chapters.join("intro");
            fs::create_dir(&intro).unwrap();
            fs::write(intro.join("_metadata.yml"), "toc-depth: 2\n").unwrap();
            fs::write(intro.join("doc.qmd"), "# Test\n").unwrap();

            let project = test_project_context(temp.path());
            let doc_path = intro.join("doc.qmd");

            let result =
                directory_metadata_for_document(&project, &doc_path, &native_runtime()).unwrap();

            // Should have 3 layers (root is NOT included based on TS behavior)
            // Actually, re-reading TS code: it walks from projectDir to inputDir
            // using relativePath.split(SEP_PATTERN), so if doc is in chapters/intro,
            // relativePath is "chapters/intro", split gives ["chapters", "intro"]
            // and it joins from projectDir: project/chapters, project/chapters/intro
            // So root is NOT included. Let me verify this...
            //
            // Wait, the TS code starts with currentDir = projectDir, then does:
            //   currentDir = join(currentDir, dir) for each dir in dirs
            // So if dirs = ["chapters", "intro"], it processes:
            //   project/chapters, project/chapters/intro
            // Root (project/) is NOT processed.
            //
            // So our test should expect 2 layers, not 3.
            assert_eq!(result.len(), 2);
            assert_eq!(result[0].get("toc").unwrap().as_bool(), Some(true));
            assert_eq!(result[1].get("toc-depth").unwrap().as_int(), Some(2));
        }

        #[test]
        fn test_directory_metadata_skips_missing() {
            // project/
            //   _metadata.yml     { theme: "cosmo" } -- not included (root)
            //   chapters/
            //     intro/          # No _metadata.yml here
            //       deep/
            //         _metadata.yml { toc: true }
            //         doc.qmd
            // Returns: [{ toc }] - skips chapters/ and intro/
            let temp = TempDir::new().unwrap();

            fs::write(temp.path().join("_metadata.yml"), "theme: cosmo\n").unwrap();

            let chapters = temp.path().join("chapters");
            fs::create_dir(&chapters).unwrap();
            // No _metadata.yml in chapters/

            let intro = chapters.join("intro");
            fs::create_dir(&intro).unwrap();
            // No _metadata.yml in intro/

            let deep = intro.join("deep");
            fs::create_dir(&deep).unwrap();
            fs::write(deep.join("_metadata.yml"), "toc: true\n").unwrap();
            fs::write(deep.join("doc.qmd"), "# Test\n").unwrap();

            let project = test_project_context(temp.path());
            let doc_path = deep.join("doc.qmd");

            let result =
                directory_metadata_for_document(&project, &doc_path, &native_runtime()).unwrap();

            // Only the deep/_metadata.yml should be found
            assert_eq!(result.len(), 1);
            assert_eq!(result[0].get("toc").unwrap().as_bool(), Some(true));
        }

        #[test]
        fn test_directory_metadata_yaml_extension() {
            // Test that _metadata.yaml (not just .yml) is recognized
            let temp = TempDir::new().unwrap();
            let chapters = temp.path().join("chapters");
            fs::create_dir(&chapters).unwrap();
            fs::write(chapters.join("_metadata.yaml"), "toc: true\n").unwrap();
            fs::write(chapters.join("doc.qmd"), "# Test\n").unwrap();

            let project = test_project_context(temp.path());
            let doc_path = chapters.join("doc.qmd");

            let result =
                directory_metadata_for_document(&project, &doc_path, &native_runtime()).unwrap();

            assert_eq!(result.len(), 1);
            assert_eq!(result[0].get("toc").unwrap().as_bool(), Some(true));
        }

        #[test]
        fn test_directory_metadata_invalid_yaml_fails() {
            // _metadata.yml with YAML syntax error should fail
            let temp = TempDir::new().unwrap();
            let chapters = temp.path().join("chapters");
            fs::create_dir(&chapters).unwrap();
            fs::write(chapters.join("_metadata.yml"), "invalid: yaml: : syntax\n").unwrap();
            fs::write(chapters.join("doc.qmd"), "# Test\n").unwrap();

            let project = test_project_context(temp.path());
            let doc_path = chapters.join("doc.qmd");

            let result = directory_metadata_for_document(&project, &doc_path, &native_runtime());

            assert!(result.is_err());
            let err = result.unwrap_err().to_string();
            assert!(
                err.contains("metadata") || err.contains("parse") || err.contains("yaml"),
                "Error should mention metadata/parse/yaml: {}",
                err
            );
        }

        #[test]
        fn test_directory_metadata_document_at_root() {
            // Document directly in project root should return empty vec
            // (no directories to walk)
            let temp = TempDir::new().unwrap();
            fs::write(temp.path().join("_metadata.yml"), "toc: true\n").unwrap();
            fs::write(temp.path().join("doc.qmd"), "# Test\n").unwrap();

            let project = test_project_context(temp.path());
            let doc_path = temp.path().join("doc.qmd");

            let result =
                directory_metadata_for_document(&project, &doc_path, &native_runtime()).unwrap();

            // Document at root means relativePath is "", dirs is empty or [""]
            // TS behavior: no directories to process, returns empty config
            assert!(result.is_empty());
        }

        #[test]
        fn test_directory_metadata_single_file_project() {
            // Single-file project (default config) should return empty vec
            let temp = TempDir::new().unwrap();
            let chapters = temp.path().join("chapters");
            fs::create_dir(&chapters).unwrap();
            fs::write(chapters.join("_metadata.yml"), "toc: true\n").unwrap();
            fs::write(chapters.join("doc.qmd"), "# Test\n").unwrap();

            // Single-file project has default config
            let project = ProjectContext {
                dir: temp.path().to_path_buf(),
                config: ProjectConfig::default(),
                is_single_file: true,
                files: vec![],
                output_dir: temp.path().to_path_buf(),

                ..Default::default()
            };
            let doc_path = chapters.join("doc.qmd");

            let result =
                directory_metadata_for_document(&project, &doc_path, &native_runtime()).unwrap();

            // Per TS behavior: directory metadata requires project context
            assert!(result.is_empty());
        }

        // === Path adjustment tests ===
        //
        // These tests verify that `!path` values in _metadata.yml are adjusted
        // to be relative to the document directory, not the metadata file directory.

        #[test]
        fn test_path_adjusted_for_subdirectory() {
            // project/
            //   shared/
            //     styles.css        # The actual file (not required to exist)
            //   chapters/
            //     _metadata.yml     # css: !path ../shared/styles.css
            //     intro/
            //       doc.qmd
            //
            // When rendering doc.qmd, css should become "../../shared/styles.css"
            let temp = TempDir::new().unwrap();

            // Create shared directory (file doesn't need to exist)
            let shared = temp.path().join("shared");
            fs::create_dir(&shared).unwrap();

            // Create chapters/_metadata.yml with a !path value
            let chapters = temp.path().join("chapters");
            fs::create_dir(&chapters).unwrap();
            fs::write(
                chapters.join("_metadata.yml"),
                "css: !path ../shared/styles.css\n",
            )
            .unwrap();

            // Create chapters/intro/doc.qmd
            let intro = chapters.join("intro");
            fs::create_dir(&intro).unwrap();
            fs::write(intro.join("doc.qmd"), "# Test\n").unwrap();

            let project = test_project_context(temp.path());
            let doc_path = intro.join("doc.qmd");

            let result =
                directory_metadata_for_document(&project, &doc_path, &native_runtime()).unwrap();

            assert_eq!(result.len(), 1);
            let css_value = result[0].get("css").expect("should have css key");

            // The path should be adjusted from ../shared/styles.css to ../../shared/styles.css
            // because we went one directory deeper (chapters/intro instead of chapters/)
            assert_eq!(
                css_value.as_str(),
                Some("../../shared/styles.css"),
                "Path should be adjusted relative to document directory"
            );
        }

        #[test]
        fn test_path_same_directory_unchanged() {
            // project/
            //   chapters/
            //     _metadata.yml     # css: !path ./local.css
            //     doc.qmd           # Same directory
            //
            // Path stays "./local.css" (or normalized equivalent)
            let temp = TempDir::new().unwrap();

            let chapters = temp.path().join("chapters");
            fs::create_dir(&chapters).unwrap();
            fs::write(chapters.join("_metadata.yml"), "css: !path ./local.css\n").unwrap();
            fs::write(chapters.join("doc.qmd"), "# Test\n").unwrap();

            let project = test_project_context(temp.path());
            let doc_path = chapters.join("doc.qmd");

            let result =
                directory_metadata_for_document(&project, &doc_path, &native_runtime()).unwrap();

            assert_eq!(result.len(), 1);
            let css_value = result[0].get("css").expect("should have css key");

            // Path should remain equivalent (pathdiff may normalize ./local.css to local.css)
            let path_str = css_value.as_str().expect("should be a string path");
            assert!(
                path_str == "./local.css" || path_str == "local.css",
                "Path should stay relative to same directory: got '{}'",
                path_str
            );
        }

        #[test]
        fn test_plain_string_not_adjusted() {
            // project/
            //   chapters/
            //     _metadata.yml     # theme: cosmo (plain string, not !path)
            //     intro/
            //       doc.qmd
            //
            // "cosmo" must NOT be changed to "../cosmo" or anything else
            let temp = TempDir::new().unwrap();

            let chapters = temp.path().join("chapters");
            fs::create_dir(&chapters).unwrap();
            fs::write(chapters.join("_metadata.yml"), "theme: cosmo\n").unwrap();

            let intro = chapters.join("intro");
            fs::create_dir(&intro).unwrap();
            fs::write(intro.join("doc.qmd"), "# Test\n").unwrap();

            let project = test_project_context(temp.path());
            let doc_path = intro.join("doc.qmd");

            let result =
                directory_metadata_for_document(&project, &doc_path, &native_runtime()).unwrap();

            assert_eq!(result.len(), 1);
            let theme_value = result[0].get("theme").expect("should have theme key");

            // Plain string should NOT be adjusted
            assert_eq!(
                theme_value.as_str(),
                Some("cosmo"),
                "Plain strings should not be adjusted"
            );
        }

        #[test]
        fn test_absolute_path_unchanged() {
            // css: !path /usr/share/styles/base.css
            // Should pass through unchanged
            let temp = TempDir::new().unwrap();

            let chapters = temp.path().join("chapters");
            fs::create_dir(&chapters).unwrap();
            fs::write(
                chapters.join("_metadata.yml"),
                "css: !path /usr/share/styles/base.css\n",
            )
            .unwrap();

            let intro = chapters.join("intro");
            fs::create_dir(&intro).unwrap();
            fs::write(intro.join("doc.qmd"), "# Test\n").unwrap();

            let project = test_project_context(temp.path());
            let doc_path = intro.join("doc.qmd");

            let result =
                directory_metadata_for_document(&project, &doc_path, &native_runtime()).unwrap();

            assert_eq!(result.len(), 1);
            let css_value = result[0].get("css").expect("should have css key");

            // Absolute path should be unchanged
            assert_eq!(
                css_value.as_str(),
                Some("/usr/share/styles/base.css"),
                "Absolute paths should not be adjusted"
            );
        }

        #[test]
        fn test_array_of_paths_all_adjusted() {
            // css:
            //   - !path ../shared/a.css
            //   - !path ../shared/b.css
            // Both should be adjusted
            let temp = TempDir::new().unwrap();

            let chapters = temp.path().join("chapters");
            fs::create_dir(&chapters).unwrap();
            fs::write(
                chapters.join("_metadata.yml"),
                "css:\n  - !path ../shared/a.css\n  - !path ../shared/b.css\n",
            )
            .unwrap();

            let intro = chapters.join("intro");
            fs::create_dir(&intro).unwrap();
            fs::write(intro.join("doc.qmd"), "# Test\n").unwrap();

            let project = test_project_context(temp.path());
            let doc_path = intro.join("doc.qmd");

            let result =
                directory_metadata_for_document(&project, &doc_path, &native_runtime()).unwrap();

            assert_eq!(result.len(), 1);
            let css_array = result[0]
                .get("css")
                .expect("should have css key")
                .as_array()
                .expect("css should be an array");

            assert_eq!(css_array.len(), 2);
            assert_eq!(
                css_array[0].as_str(),
                Some("../../shared/a.css"),
                "First path should be adjusted"
            );
            assert_eq!(
                css_array[1].as_str(),
                Some("../../shared/b.css"),
                "Second path should be adjusted"
            );
        }

        #[test]
        fn test_glob_not_adjusted() {
            // resources: !glob ../images/*.png
            // Globs are patterns, not paths - should NOT be adjusted
            let temp = TempDir::new().unwrap();

            let chapters = temp.path().join("chapters");
            fs::create_dir(&chapters).unwrap();
            fs::write(
                chapters.join("_metadata.yml"),
                "resources: !glob ../images/*.png\n",
            )
            .unwrap();

            let intro = chapters.join("intro");
            fs::create_dir(&intro).unwrap();
            fs::write(intro.join("doc.qmd"), "# Test\n").unwrap();

            let project = test_project_context(temp.path());
            let doc_path = intro.join("doc.qmd");

            let result =
                directory_metadata_for_document(&project, &doc_path, &native_runtime()).unwrap();

            assert_eq!(result.len(), 1);
            let resources = result[0]
                .get("resources")
                .expect("should have resources key");

            // Glob should NOT be adjusted (globs need separate handling)
            assert_eq!(
                resources.as_str(),
                Some("../images/*.png"),
                "Globs should not be adjusted"
            );
        }

        #[test]
        fn test_nested_map_path_adjusted() {
            // Test that paths nested in maps are also adjusted
            // format:
            //   html:
            //     css: !path ../shared/styles.css
            let temp = TempDir::new().unwrap();

            let chapters = temp.path().join("chapters");
            fs::create_dir(&chapters).unwrap();
            fs::write(
                chapters.join("_metadata.yml"),
                "format:\n  html:\n    css: !path ../shared/styles.css\n",
            )
            .unwrap();

            let intro = chapters.join("intro");
            fs::create_dir(&intro).unwrap();
            fs::write(intro.join("doc.qmd"), "# Test\n").unwrap();

            let project = test_project_context(temp.path());
            let doc_path = intro.join("doc.qmd");

            let result =
                directory_metadata_for_document(&project, &doc_path, &native_runtime()).unwrap();

            assert_eq!(result.len(), 1);
            let css_value = result[0]
                .get("format")
                .and_then(|f| f.get("html"))
                .and_then(|h| h.get("css"))
                .expect("should have format.html.css");

            assert_eq!(
                css_value.as_str(),
                Some("../../shared/styles.css"),
                "Nested path should be adjusted"
            );
        }
    }

    // === P1.1 T3: ConfigValue → TsMetadataValue helper + config-map builder ===
    //
    // Native-only: the helper + builder are gated cfg(not(wasm32)) (they build the
    // per-render EngineProjectContext for TsEngine, which is native-only).
    #[cfg(not(target_arch = "wasm32"))]
    mod engine_context_lowering {
        use quarto_source_map::{By, SourceInfo};

        use super::super::{
            build_engine_config_map, config_value_to_ts_metadata, document_metadata_to_ts_map,
        };
        use crate::engine::ts_protocol::TsMetadataValue;
        use crate::project::{ProjectConfig, ProjectKind};
        use quarto_pandoc_types::ConfigValue;
        use quarto_pandoc_types::config_value::ConfigMapEntry;
        use std::path::PathBuf;

        fn si() -> SourceInfo {
            SourceInfo::generated(By::unknown())
        }

        /// `engines: ["knitr", {path: "x.js"}]` as a ConfigValue array.
        fn engines_value() -> ConfigValue {
            ConfigValue::new_array(
                vec![
                    ConfigValue::new_string("knitr", si()),
                    ConfigValue::new_map(
                        vec![ConfigMapEntry {
                            key: "path".to_string(),
                            key_source: si(),
                            value: ConfigValue::new_string("x.js", si()),
                        }],
                        si(),
                    ),
                ],
                si(),
            )
        }

        /// Metadata map `{ engines: [...] }`.
        fn metadata_with_engines() -> ConfigValue {
            ConfigValue::new_map(
                vec![ConfigMapEntry {
                    key: "engines".to_string(),
                    key_source: si(),
                    value: engines_value(),
                }],
                si(),
            )
        }

        // ── helper: string scalar arm (as_plain_text) + Mapping arm ──────────────
        //
        // Named revert (as_plain_text arm): `"knitr"` drops to Null → RED.
        // Named revert (Map arm): the `{path: "x.js"}` map drops/misshapes → RED.
        #[test]
        fn config_value_to_ts_metadata_lowers_array_of_string_and_map() {
            let lowered = config_value_to_ts_metadata(&engines_value());

            let mut path_map = std::collections::HashMap::new();
            path_map.insert(
                "path".to_string(),
                TsMetadataValue::String("x.js".to_string()),
            );

            assert_eq!(
                lowered,
                TsMetadataValue::Array(vec![
                    TsMetadataValue::String("knitr".to_string()),
                    TsMetadataValue::Map(path_map),
                ]),
                "string scalar must lower via as_plain_text; the {{path}} entry must lower to a Map"
            );
        }

        // ── builder: two-key config map from a real project config ───────────────
        #[test]
        fn build_engine_config_map_builds_two_key_map() {
            let config = ProjectConfig {
                project_kind: ProjectKind::Default,
                output_dir: Some(PathBuf::from("out")),
                render_patterns: Vec::new(),
                resources: Vec::new(),
                metadata: Some(metadata_with_engines()),
                config_path: None,
            };

            let map = build_engine_config_map(&config).expect("config map must be Some");

            // engines key present, lowered from metadata.get("engines").
            let mut path_map = std::collections::HashMap::new();
            path_map.insert(
                "path".to_string(),
                TsMetadataValue::String("x.js".to_string()),
            );
            assert_eq!(
                map.get("engines"),
                Some(&TsMetadataValue::Array(vec![
                    TsMetadataValue::String("knitr".to_string()),
                    TsMetadataValue::Map(path_map),
                ])),
                "engines key must be lowered from metadata"
            );

            // output-dir carries the RAW relative typed field value, flat at the
            // top of the wire config map (the host bridges it to the rich
            // project.outputDir the engine sees).
            assert_eq!(
                map.get("output-dir"),
                Some(&TsMetadataValue::String("out".to_string())),
                "output-dir must carry the raw relative ProjectConfig.output_dir"
            );
        }

        // ── builder: absent engines ⇒ key omitted ────────────────────────────────
        #[test]
        fn build_engine_config_map_omits_absent_engines() {
            let config = ProjectConfig {
                output_dir: Some(PathBuf::from("out")),
                // Metadata present but has NO `engines` key.
                metadata: Some(ConfigValue::new_map(Vec::new(), si())),
                ..ProjectConfig::default()
            };

            let map = build_engine_config_map(&config).expect("config map must be Some");
            assert!(
                !map.contains_key("engines"),
                "absent engines ⇒ engines key omitted"
            );
            assert!(map.contains_key("output-dir"), "output-dir still present");
        }

        // ── builder: no metadata ⇒ config: None ──────────────────────────────────
        //
        // Named revert ("builder returns Some(map) unconditionally"): this REDs.
        #[test]
        fn build_engine_config_map_none_when_no_metadata() {
            let config = ProjectConfig::default();
            assert_eq!(
                build_engine_config_map(&config),
                None,
                "a config with no metadata (single-file default) ⇒ config: None"
            );
        }

        // === P1.1b: document_metadata_to_ts_map — flattens the top-level Map ====
        //
        // Unit-level coverage of the flatten/unwrap step T14 (e2e) exercises
        // end-to-end. Not itself a frozen seam row (T14 is e2e-only in the
        // spec) — additive defense-in-depth isolating the unwrap logic.
        //
        // Named revert: return `HashMap::new()` unconditionally (as if the
        // unwrap always hit the `_ => HashMap::new()` fallback arm) → this
        // goes RED (the "engines" key would be missing).
        #[test]
        fn document_metadata_to_ts_map_flattens_top_level_mapping() {
            let map = document_metadata_to_ts_map(&metadata_with_engines());

            let mut path_map = std::collections::HashMap::new();
            path_map.insert(
                "path".to_string(),
                TsMetadataValue::String("x.js".to_string()),
            );
            assert_eq!(
                map.get("engines"),
                Some(&TsMetadataValue::Array(vec![
                    TsMetadataValue::String("knitr".to_string()),
                    TsMetadataValue::Map(path_map),
                ])),
                "top-level 'engines' key must be flattened directly into the \
                 returned map, not left wrapped in an outer TsMetadataValue::Map"
            );
        }

        // ── non-mapping input degrades to empty (not a panic) ────────────────────
        #[test]
        fn document_metadata_to_ts_map_non_mapping_input_is_empty() {
            let map = document_metadata_to_ts_map(&ConfigValue::new_string("not-a-map", si()));
            assert!(
                map.is_empty(),
                "non-mapping document metadata must degrade to an empty map, not panic"
            );
        }
    }
}
