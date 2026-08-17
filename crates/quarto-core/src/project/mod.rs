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

pub mod aliases;
pub mod cache_key;
pub mod dependency_graph;
pub mod discovery;
pub mod environment;
pub mod index;
pub mod listing;
pub mod orchestrator;
pub mod pass2_renderer;
pub mod profile_cache;
pub mod project_profile;
pub mod render_scripts;
pub mod sidebar_membership;
pub mod website_config;
// Every hook in this module is native-only (`#[cfg(not(wasm32))]`
// per function) — each writes into the on-disk output dir. The one
// cross-platform member, the Phase 5 project-artifact flush, moved
// to `crate::artifact_flush` in bd-v8gx.
pub mod llms_post_render;
pub mod website_post_render;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use quarto_brand::ResolvedBrand;
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
/// Returns: [layer0, layer1] - deeper directories later in vec.
/// Each layer carries the path of the `_metadata.yml` it was parsed
/// from — the exact `PathBuf` whose string form was hashed into the
/// layer's `FileId`s by `quarto_yaml::parse_file`, so callers
/// (`MetadataMergeStage`) can register the file in the document's
/// `SourceContext` under the matching id.
pub fn directory_metadata_for_document(
    project: &ProjectContext,
    document_path: &Path,
    runtime: &dyn SystemRuntime,
) -> Result<Vec<(PathBuf, ConfigValue)>> {
    use pampa::pandoc::yaml_to_config_value;
    use pampa::utils::diagnostic_collector::DiagnosticCollector;
    use quarto_config::InterpretationContext;

    // Single-file projects don't have directory metadata
    if project.is_single_file {
        return Ok(Vec::new());
    }

    let document_path = runtime
        .canonicalize(document_path)
        .unwrap_or_else(|_| document_path.to_path_buf());
    let document_dir = document_path
        .parent()
        .ok_or_else(|| QuartoError::Other("Document has no parent directory".into()))?;

    let mut layers = Vec::new();
    for path in directory_metadata_paths_for_document(project, &document_path, runtime) {
        // Parse the metadata file
        let content = runtime
            .file_read_string(&path)
            .map_err(|e| QuartoError::Other(format!("Failed to read {}: {}", path.display(), e)))?;

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
        let layer_dir = path.parent().expect("metadata file has a directory");
        adjust_paths_to_document_dir(&mut metadata, layer_dir, document_dir);

        layers.push((path, metadata));
    }

    Ok(layers)
}

/// Enumerate the `_metadata.yml` / `_metadata.yaml` layer files that
/// apply to `document_path`, outermost directory first — the same
/// walk, and crucially the **same path spelling**, as
/// [`directory_metadata_for_document`] (the spelling is what
/// `quarto_yaml::parse_file` hashed into each layer's FileId, so
/// diagnostic assembly re-derives layer ids from these exact paths;
/// bd-x113wg9v). Returns an empty list for single-file projects and
/// for documents outside the project directory.
///
/// Callers that already canonicalized `document_path` (as
/// [`directory_metadata_for_document`] does) pass it through; the
/// function canonicalizes again defensively, which is idempotent.
pub fn directory_metadata_paths_for_document(
    project: &ProjectContext,
    document_path: &Path,
    runtime: &dyn SystemRuntime,
) -> Vec<PathBuf> {
    if project.is_single_file {
        return Vec::new();
    }
    // Canonicalize so strip_prefix works reliably. project.dir is always
    // canonical (from ProjectContext::discover), but callers may pass
    // relative paths (e.g., WASM render_qmd with VFS paths).
    let document_path = runtime
        .canonicalize(document_path)
        .unwrap_or_else(|_| document_path.to_path_buf());
    let project_dir = &project.dir;
    let Some(document_dir) = document_path.parent() else {
        return Vec::new();
    };
    let Ok(relative_path) = document_dir.strip_prefix(project_dir) else {
        // Document is not under project directory
        return Vec::new();
    };

    let mut paths = Vec::new();
    let mut current_dir = project_dir.clone();
    // Walk through each directory from project root toward document
    // (but not including project root itself - we start from first subdir)
    for component in relative_path.components() {
        current_dir = current_dir.join(component);
        if let Some(path) = find_metadata_file(&current_dir, runtime) {
            paths.push(path);
        }
    }
    paths
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

/// Advisory diagnostics about the parsed project kind (bd-ad7i1pc6).
///
/// `book` and `manuscript` parse successfully but currently render
/// with default-project behavior (`project_type_for` maps them to
/// `DefaultProjectType`). That surprise used to be silent; this
/// returns the **Q-5-18** warning CLI drivers print next to the
/// `underscore_typo_diagnostics` check. Span-less, mirroring Q-5-11:
/// the config file is named in the problem text instead.
pub fn project_kind_diagnostics(
    config: &ProjectConfig,
) -> Vec<quarto_error_reporting::DiagnosticMessage> {
    use quarto_error_reporting::DiagnosticMessageBuilder;

    let kind = config.project_kind;
    if !matches!(kind, ProjectKind::Book | ProjectKind::Manuscript) {
        return Vec::new();
    }
    let config_name = config
        .config_path
        .as_ref()
        .map_or_else(|| "_quarto.yml".to_string(), |p| p.display().to_string());
    vec![
        DiagnosticMessageBuilder::warning(format!(
            "`{}` projects are not yet implemented",
            kind.as_str()
        ))
        .with_code("Q-5-18")
        .problem(format!(
            "{config_name} sets `project.type: {}`, but Quarto 2 does not implement \
             {} projects yet. The project renders with default-project behavior: \
             documents are rendered individually, without {}-specific structure.",
            kind.as_str(),
            kind.as_str(),
            kind.as_str(),
        ))
        .build(),
    ]
}

/// Result of resolving `project.type` (bd-ad7i1pc6).
struct ResolvedProjectType {
    kind: ProjectKind,
    custom: Option<CustomProjectType>,
    /// Non-fatal resolution diagnostics (Q-16-8 ambiguity, Q-16-9
    /// missing base type), destined for
    /// [`ProjectConfig::config_diagnostics`].
    diagnostics: Vec<quarto_error_reporting::DiagnosticMessage>,
}

impl ResolvedProjectType {
    fn builtin(kind: ProjectKind) -> Self {
        Self {
            kind,
            custom: None,
            diagnostics: Vec::new(),
        }
    }
}

/// Resolve `project.type` from parsed `_quarto.yml` metadata
/// (bd-ad7i1pc6).
///
/// Built-in names parse directly. A custom name resolves against
/// extensions discovered at project scope
/// ([`crate::extension::discover_project_extensions`]): the selected
/// extension's `contributes.project` fragment is merged **under** the
/// user's config — mutating `metadata` in place, before
/// `parse_config` extracts any field from it — and `project.type` is
/// rewritten to the extension's built-in base type, so every
/// downstream consumer sees an ordinary built-in project.
///
/// An unknown or non-string `project.type` is a hard **Q-5-17** error:
/// the previous `.ok().unwrap_or_default()` fallback silently rendered
/// e.g. a `type: posit-docs` website as a bare default project.
///
fn resolve_project_type(
    metadata: &mut ConfigValue,
    config_path: &Path,
    extensions: &[crate::extension::Extension],
    load_diagnostics: &[quarto_error_reporting::DiagnosticMessage],
    runtime: &dyn SystemRuntime,
) -> Result<ResolvedProjectType> {
    let Some(type_value) = metadata.get("project").and_then(|p| p.get("type")) else {
        return Ok(ResolvedProjectType::builtin(ProjectKind::Default));
    };
    let type_source = type_value.source_info.clone();

    let Some(type_str) = type_value.as_str().map(str::to_owned) else {
        return Err(project_type_error(
            "`project.type` must be a string.".to_string(),
            &type_source,
            config_path,
            extensions,
            Vec::new(),
            Vec::new(),
        ));
    };

    if let Ok(kind) = ProjectKind::try_from(type_str.as_str()) {
        return Ok(ResolvedProjectType::builtin(kind));
    }

    resolve_custom_project_type(
        metadata,
        &type_str,
        &type_source,
        config_path,
        extensions,
        load_diagnostics,
        runtime,
    )
}

/// Resolve a non-built-in `project.type` against project-scoped
/// extensions and merge the winner's `contributes.project` fragment
/// under the user's config.
///
/// Merge semantics are Quarto 2's standard rules, uniformly — no
/// Q1-style special cases: user wins scalar conflicts, arrays concat
/// (extension entries first), and a user value tagged `!prefer`
/// replaces the extension's outright. The only fragment surgery is
/// stripping `project.detect` (bootstrap-only, unsupported — future
/// strand) before the merge.
#[allow(clippy::too_many_arguments)]
fn resolve_custom_project_type(
    metadata: &mut ConfigValue,
    type_str: &str,
    type_source: &quarto_source_map::SourceInfo,
    config_path: &Path,
    extensions: &[crate::extension::Extension],
    load_diagnostics: &[quarto_error_reporting::DiagnosticMessage],
    runtime: &dyn SystemRuntime,
) -> Result<ResolvedProjectType> {
    use quarto_config::MergedConfig;

    let project_dir = config_path.parent().unwrap_or(Path::new("."));

    let candidates: Vec<&crate::extension::Extension> = extensions
        .iter()
        .filter(|e| e.contributes.project.is_some())
        .collect();

    let (selected, mut diagnostics) =
        select_project_type_extension(type_str, &candidates, config_path);
    let Some(ext) = selected else {
        let mut hints = Vec::new();
        if candidates.is_empty() {
            hints.push(
                "No extension under `_extensions/` contributes a project type \
                 (`contributes: project:` in `_extension.yml`)."
                    .to_string(),
            );
        } else {
            let available: Vec<String> = candidates.iter().map(|e| format!("`{}`", e.id)).collect();
            hints.push(format!(
                "Extensions contributing project types found: {}.",
                available.join(", ")
            ));
        }
        // A broken manifest (Q-16-1) may be exactly why the type failed
        // to resolve — attach those diagnostics so the cause is visible.
        return Err(project_type_error(
            format!("`{type_str}` is not a recognized project type."),
            type_source,
            config_path,
            extensions,
            hints,
            load_diagnostics.to_vec(),
        ));
    };

    // `select_project_type_extension` only returns candidates, and
    // candidates are filtered on `contributes.project.is_some()`.
    let fragment_src = ext
        .contributes
        .project
        .as_ref()
        .expect("selected extension must contribute a project fragment");
    if !fragment_src.is_map() {
        return Err(project_contribution_error(
            ext,
            format!(
                "The `contributes.project` entry in `{}` must be a mapping \
                 (a `_quarto.yml` fragment).",
                ext.path.join("_extension.yml").display()
            ),
        ));
    }

    // Base kind: the built-in type this custom type renders as.
    let base_kind = match fragment_src
        .get("project")
        .and_then(|p| p.get("type"))
        .map(|t| t.as_str().map(str::to_owned))
    {
        None => {
            diagnostics.push(missing_base_type_warning(ext));
            ProjectKind::Default
        }
        Some(None) => {
            return Err(project_contribution_error(
                ext,
                "The `project.type` inside `contributes.project` must be a string.".to_string(),
            ));
        }
        Some(Some(base)) => match ProjectKind::try_from(base.as_str()) {
            Ok(kind @ (ProjectKind::Book | ProjectKind::Manuscript)) => {
                return Err(project_contribution_error(
                    ext,
                    format!(
                        "Extension `{}` declares base project type `{}`, which \
                         Quarto 2 does not implement yet. Custom project types \
                         with a `{}` base are not yet supported.",
                        ext.id,
                        kind.as_str(),
                        kind.as_str()
                    ),
                ));
            }
            Ok(kind) => kind,
            Err(_) => {
                return Err(project_contribution_error(
                    ext,
                    format!(
                        "Extension `{}` declares base project type `{base}`, which \
                         is not a built-in project type. A custom project type must \
                         name a built-in base (`default` or `website`); chaining \
                         custom types is not supported.",
                        ext.id
                    ),
                ));
            }
        },
    };

    // Strip `project.detect` (bootstrap-only) from a working copy of
    // the fragment before merging.
    let mut fragment = fragment_src.clone();
    if let Some(project_entry) = fragment.get_mut("project")
        && let ConfigValueKind::Map(entries) = &mut project_entry.value
    {
        entries.retain(|e| e.key != "detect");
    }

    // Rebase extension-bundled file references (theme SCSS, includes,
    // favicon, …) from extension-dir-relative to project-root-relative
    // so the merged config behaves as if the user had written the
    // paths in `_quarto.yml`.
    rebase_fragment_paths(&mut fragment, &ext.path, project_dir, runtime);

    // Merge: extension fragment is the lower-priority layer; the
    // user's `_quarto.yml` wins conflicts under standard Q2 semantics.
    let merged = MergedConfig::new(vec![&fragment, metadata])
        .materialize()
        .map_err(|e| {
            QuartoError::Other(format!(
                "Failed to merge project config contributed by extension `{}`: {}",
                ext.id, e
            ))
        })?;
    *metadata = merged;

    // Rewrite `project.type` to the base type so downstream consumers
    // see an ordinary built-in project. The value keeps the provenance
    // of the user's original `type:` scalar.
    metadata.insert_path(
        &["project", "type"],
        ConfigValue::new_string(base_kind.as_str(), type_source.clone()),
    );

    Ok(ResolvedProjectType {
        kind: base_kind,
        custom: Some(CustomProjectType {
            name: type_str.to_string(),
            extension_id: ext.id.to_string(),
            extension_dir: ext.path.clone(),
        }),
        diagnostics,
    })
}

/// Merge every discovered extension's `contributes.metadata.project`
/// into the project config (bd-ad7i1pc6 Phase 5, absorbing
/// bd-zb2tod5f).
///
/// Unlike `contributes.project` (opt-in via `project.type`), this
/// applies from **all** discovered extensions, unconditionally — the
/// Q1 mechanism quarto-openapi uses to inject its `pre-render` script.
/// Layering (low → high): extension contributions in discovery order,
/// then the user's `_quarto.yml` (already fragment-merged for custom
/// types) — so the **user wins**, deliberately diverging from Q1's
/// `mergeExtensionMetadata`, which lets the extension override the
/// user (an accident of implementation order, not a design).
///
/// Bundled file paths (scripts, resources) rebase ext-dir →
/// project-root via the same existence-driven machinery as
/// `contributes.project` fragments.
///
/// Non-`project` keys of `contributes.metadata` are a *document-level*
/// concern, handled as a metadata layer in `MetadataMergeStage` — they
/// must not merge into project config here.
fn apply_metadata_project_contributions(
    metadata: &mut ConfigValue,
    extensions: &[crate::extension::Extension],
    project_dir: &Path,
    runtime: &dyn SystemRuntime,
) {
    use quarto_config::MergedConfig;
    use quarto_pandoc_types::config_value::ConfigMapEntry;

    let mut contribution_layers: Vec<ConfigValue> = Vec::new();
    for ext in extensions {
        let Some(meta) = ext.contributes.metadata.as_ref() else {
            continue;
        };
        let Some(project_part) = meta.get("project") else {
            continue;
        };
        // Wrap as `{project: …}` so the fragment merges at top level,
        // then rebase bundled paths against this extension's dir.
        let mut fragment = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "project".to_string(),
                key_source: project_part.source_info.clone(),
                value: project_part.clone(),
            }],
            meta.source_info.clone(),
        );
        rebase_fragment_paths(&mut fragment, &ext.path, project_dir, runtime);
        contribution_layers.push(fragment);
    }
    if contribution_layers.is_empty() {
        return;
    }

    let mut layers: Vec<&ConfigValue> = contribution_layers.iter().collect();
    layers.push(metadata);
    // A materialize failure (pathological nesting) keeps the user's
    // config untouched — same tolerant posture as `MetadataMergeStage`.
    if let Ok(merged) = MergedConfig::new(layers).materialize() {
        *metadata = merged;
    }
}

/// Key paths within a `contributes.project` fragment whose string
/// values may name files bundled with the extension (bd-ad7i1pc6 D4).
///
/// `*` matches any map key; arrays are transparent (a pattern position
/// applies to every item). When a pattern is exhausted at a node,
/// every string leaf underneath is a rebase candidate — this is what
/// makes the `theme: {light: […], dark: […]}` and plain-list forms
/// work from one `["format", "*", "theme"]` entry.
///
/// The table narrows *where* rebasing may happen; the existence check
/// in [`rebase_candidate`] decides *whether* an individual string is a
/// bundled file (builtin theme names like `cosmo`, command lines, and
/// project-relative references simply don't exist under the extension
/// dir and pass through verbatim).
const FRAGMENT_PATH_PATTERNS: &[&[&str]] = &[
    &["format", "*", "theme"],
    &["format", "*", "css"],
    &["format", "*", "include-in-header"],
    &["format", "*", "include-before-body"],
    &["format", "*", "include-after-body"],
    &["format", "*", "format-resources"],
    &["format", "*", "template"],
    &["format", "*", "template-partials"],
    &["website", "favicon"],
    &["website", "navbar", "logo"],
    &["website", "sidebar", "logo"],
    &["project", "pre-render"],
    &["project", "post-render"],
    &["project", "resources"],
    &["brand"],
];

/// Rebase extension-bundled file references in a `contributes.project`
/// fragment from extension-dir-relative to project-root-relative.
///
/// Rebased values become [`ConfigValueKind::Path`] so the per-document
/// metadata merge keeps adjusting them (project root → document dir)
/// for documents in subdirectories.
///
/// The pattern walk and the bundled-file existence check are shared
/// with the `contributes.formats` marking (bd-of20unsb) via
/// [`crate::extension::paths`].
fn rebase_fragment_paths(
    fragment: &mut ConfigValue,
    ext_dir: &Path,
    project_dir: &Path,
    runtime: &dyn SystemRuntime,
) {
    crate::extension::paths::walk_pattern_leaves(fragment, FRAGMENT_PATH_PATTERNS, &mut |leaf| {
        let (ConfigValueKind::Scalar(yaml_rust2::Yaml::String(s)) | ConfigValueKind::Path(s)) =
            &leaf.value
        else {
            return;
        };
        if let Some(rebased) = rebase_candidate(s, ext_dir, project_dir, runtime) {
            leaf.value = ConfigValueKind::Path(rebased);
        }
    });
}

/// Decide whether `s` names a file bundled with the extension, and if
/// so return its rebased (forward-slash) form.
///
/// Rooted paths and URLs pass through. Otherwise the string is a
/// bundled file exactly when it exists under the extension dir; the
/// rebased form is project-root-relative when the extension lives
/// inside the project (`_extensions/…`), absolute otherwise (embedded
/// built-in extensions extracted to a temp dir).
fn rebase_candidate(
    s: &str,
    ext_dir: &Path,
    project_dir: &Path,
    runtime: &dyn SystemRuntime,
) -> Option<String> {
    if !crate::extension::paths::bundled_file_exists(s, ext_dir, runtime) {
        return None;
    }
    let abs = ext_dir.join(s);
    let rebased = match pathdiff::diff_paths(&abs, project_dir) {
        Some(rel)
            if !matches!(
                rel.components().next(),
                Some(std::path::Component::ParentDir)
            ) =>
        {
            quarto_util::to_forward_slashes(&rel)
        }
        _ => quarto_util::to_forward_slashes(&abs),
    };
    Some(rebased)
}

/// Select the extension backing a custom project type name.
///
/// Rules (Q1-compatible, made deterministic):
/// - An extension with the same full id appearing more than once
///   (built-in shadowed by a user copy) collapses to the **last**
///   occurrence — user extensions are discovered after built-ins.
/// - `org/name` form: exact id match only.
/// - bare `name`: all candidates with that name; if several distinct
///   ids match, the org-less one wins, then the lexicographically
///   first organization — with a **Q-16-8** warning naming the choice
///   and the shadowed candidates.
fn select_project_type_extension<'a>(
    name: &str,
    candidates: &[&'a crate::extension::Extension],
    config_path: &Path,
) -> (
    Option<&'a crate::extension::Extension>,
    Vec<quarto_error_reporting::DiagnosticMessage>,
) {
    use hashlink::LinkedHashMap;

    // Dedup by full id, keeping the last occurrence (user shadows
    // built-in). LinkedHashMap keeps discovery order for determinism.
    let mut by_id: LinkedHashMap<String, &crate::extension::Extension> = LinkedHashMap::new();
    for ext in candidates {
        by_id.replace(ext.id.to_string(), ext);
    }

    if let Some((org, ext_name)) = name.split_once('/') {
        let found = by_id
            .values()
            .find(|e| e.id.name == ext_name && e.id.organization.as_deref() == Some(org))
            .copied();
        return (found, Vec::new());
    }

    let mut matches: Vec<&crate::extension::Extension> = by_id
        .values()
        .filter(|e| e.id.name == name)
        .copied()
        .collect();
    match matches.len() {
        0 => (None, Vec::new()),
        1 => (Some(matches[0]), Vec::new()),
        _ => {
            // Deterministic preference: org-less first, then by org.
            matches.sort_by_key(|e| e.id.organization.clone());
            let chosen = matches
                .iter()
                .find(|e| e.id.organization.is_none())
                .copied()
                .unwrap_or(matches[0]);
            let others: Vec<String> = matches
                .iter()
                .filter(|e| e.id != chosen.id)
                .map(|e| format!("`{}`", e.id))
                .collect();
            let config_name = config_path.display();
            let warning = quarto_error_reporting::DiagnosticMessageBuilder::warning(
                "Ambiguous project type extension",
            )
            .with_code("Q-16-8")
            .problem(format!(
                "`project.type: {name}` in {config_name} matches more than one \
                 extension; `{}` was chosen over {}.",
                chosen.id,
                others.join(", ")
            ))
            .add_hint(format!(
                "Use the full `organization/name` form (e.g. `type: {}`) to \
                 disambiguate.",
                chosen.id
            ))
            .build();
            (Some(chosen), vec![warning])
        }
    }
}

/// Build the **Q-16-9** warning for a `contributes.project` fragment
/// with no `project.type`: the base defaults to `default`, which is
/// almost certainly an authoring mistake in the extension.
fn missing_base_type_warning(
    ext: &crate::extension::Extension,
) -> quarto_error_reporting::DiagnosticMessage {
    quarto_error_reporting::DiagnosticMessageBuilder::warning(
        "Project type contribution has no base type",
    )
    .with_code("Q-16-9")
    .problem(format!(
        "Extension `{}` contributes a project type but its \
         `contributes.project` fragment does not declare `project.type`. \
         The project renders with the `default` base type.",
        ext.id
    ))
    .add_hint(format!(
        "Add `project: type: website` (or `default`) to `{}`.",
        ext.path.join("_extension.yml").display()
    ))
    .build()
}

/// Build the structured **Q-16-7** error for an invalid
/// `contributes.project` fragment (non-mapping, bad or unsupported
/// base type). Span-less: the fault is in the extension manifest, not
/// the user's `_quarto.yml`; the problem text names the extension.
fn project_contribution_error(ext: &crate::extension::Extension, problem: String) -> QuartoError {
    use quarto_error_reporting::DiagnosticMessageBuilder;
    use quarto_source_map::SourceContext;

    let diagnostic = DiagnosticMessageBuilder::error("Invalid project type contribution")
        .with_code("Q-16-7")
        .problem(problem)
        .add_info(format!(
            "The extension's manifest is `{}`.",
            ext.path.join("_extension.yml").display()
        ))
        .build();

    QuartoError::Parse(crate::error::ParseError::new(
        vec![diagnostic],
        SourceContext::new(),
    ))
}

/// Build the structured **Q-5-17** error for a `project.type` that
/// could not be resolved, anchored at the `type:` value's span in the
/// project config file. `extra_hints` (e.g. the list of available
/// project-contributing extensions) follow the built-ins hint;
/// `extra_diagnostics` (e.g. Q-16-1 manifest-load failures that may be
/// the root cause) are attached after the main diagnostic.
fn project_type_error(
    problem: String,
    type_source: &quarto_source_map::SourceInfo,
    config_path: &Path,
    extensions: &[crate::extension::Extension],
    extra_hints: Vec<String>,
    extra_diagnostics: Vec<quarto_error_reporting::DiagnosticMessage>,
) -> QuartoError {
    use quarto_error_reporting::DiagnosticMessageBuilder;
    use quarto_source_map::SourceContext;

    // Candidate-matched binding (bd-h5rfw3ao): today `type_source`
    // always originates in `config_path` (both call sites run before
    // any extension-fragment merge), but that was enforced only by
    // call ordering. Matching by re-derived FileId keeps the anchor
    // correct even if a merged (extension-contributed) `project.type`
    // ever reaches this path — and degrades span-less, never
    // wrong-file, for anything else.
    let manifest_paths: Vec<std::path::PathBuf> = extensions
        .iter()
        .map(|e| e.path.join("_extension.yml"))
        .collect();
    let mut source_context = SourceContext::new();
    let matched = crate::config_sources::bind_config_source(
        &mut source_context,
        type_source,
        std::iter::once(config_path).chain(manifest_paths.iter().map(std::path::PathBuf::as_path)),
    );

    let mut builder = DiagnosticMessageBuilder::error("Unknown project type")
        .with_code("Q-5-17")
        .with_location(type_source.clone())
        .problem(problem);
    if let Some(path) = matched
        && path != config_path
    {
        builder = builder.add_info(format!(
            "The `project.type` value is contributed by the extension manifest `{}`.",
            path.display()
        ));
    }
    builder = builder
        .add_hint("Built-in project types are `default`, `website`, `book`, and `manuscript`.");
    for hint in extra_hints {
        builder = builder.add_hint(hint);
    }

    let mut diagnostics = vec![builder.build()];
    diagnostics.extend(extra_diagnostics);

    QuartoError::Parse(crate::error::ParseError::new(diagnostics, source_context))
}

/// Record of an extension-contributed custom project type
/// (bd-ad7i1pc6).
///
/// Present on [`ProjectConfig`] when `project.type` named a custom
/// type that an extension's `contributes.project` resolved. The custom
/// type always resolves to a built-in **base** kind before anything
/// dispatches on it — [`ProjectConfig::project_kind`] holds that base
/// kind, and nothing downstream behaves differently for a custom type;
/// this record exists for diagnostics (`q2 render`'s
/// `type: posit-docs (website)` banner) and provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomProjectType {
    /// The type name as written in `_quarto.yml` (e.g. `posit-docs`).
    pub name: String,
    /// Resolved extension id (`org/name` or bare `name`).
    pub extension_id: String,
    /// Absolute path of the extension's directory (holds
    /// `_extension.yml`); the base for rebasing contributed paths.
    pub extension_dir: PathBuf,
}

/// Parsed project configuration from `_quarto.yml`
#[derive(Debug, Clone, Default)]
pub struct ProjectConfig {
    /// Project kind (the tag selected by `project.type:`).
    ///
    /// For extension-contributed custom types this is the resolved
    /// **base** kind; see [`Self::custom_project_type`].
    pub project_kind: ProjectKind,

    /// Set when `project.type` named an extension-contributed custom
    /// type (bd-ad7i1pc6); `None` for built-in types.
    pub custom_project_type: Option<CustomProjectType>,

    /// Non-fatal diagnostics produced while parsing the project
    /// config (e.g. Q-16-8 ambiguous project-type extension, Q-16-9
    /// missing base type). CLI drivers print these once per run,
    /// next to [`project_kind_diagnostics`] — they are *not* folded
    /// into per-document render diagnostics, which would repeat them
    /// for every file.
    pub config_diagnostics: Vec<quarto_error_reporting::DiagnosticMessage>,

    /// Output directory (relative to project root)
    pub output_dir: Option<PathBuf>,

    /// Input file patterns (`project.render`), each carrying the
    /// provenance of the YAML scalar it was written as so discovery
    /// diagnostics can point at it (bd-mt7a6uc4 D8).
    pub render_patterns: Vec<crate::glob::RawGlob>,

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

    /// `project.pre-render` script entries (bd-w348iu63): command
    /// lines run before a project render, in declaration order, with
    /// each entry's YAML source location. A bare string normalizes to
    /// a one-element list. Empty when the key is absent (including
    /// single-file pseudo-projects, which never run scripts). See
    /// [`render_scripts`] for the execution contract.
    pub pre_render_scripts: Vec<render_scripts::RenderScript>,

    /// `project.post-render` script entries — run after a successful
    /// project render. Same shape as
    /// [`pre_render_scripts`](Self::pre_render_scripts).
    pub post_render_scripts: Vec<render_scripts::RenderScript>,

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

    /// Manifest paths (`…/_extension.yml`) of every extension
    /// discovered at config-parse time, whose
    /// `contributes.metadata.project` / `contributes.project`
    /// fragments may have merged entries into this config
    /// (bd-m6wmztln).
    ///
    /// Together with [`config_path`](Self::config_path) these are the
    /// candidate source files for
    /// [`crate::config_sources::bind_config_source`]: a merged
    /// value's `SourceInfo` keeps the filename-hash FileId of the
    /// file it was written in, so config-anchored diagnostics must
    /// pick the matching file instead of assuming `_quarto.yml`.
    ///
    /// Reconstructed as `ext.path.join("_extension.yml")`, which is
    /// exact because extension discovery only accepts that manifest
    /// name and `ext.path` is the manifest's parent directory.
    /// bd-xh1v98d9 tracks storing the actual manifest path on
    /// [`crate::extension::Extension`] instead.
    pub extension_manifest_paths: Vec<PathBuf>,

    /// The **project-level** brand named by `_quarto.yml`'s `brand:`
    /// key, resolved once at config-parse time (`bd-97yc`).
    ///
    /// `None` when no `brand:` key is present — Q2 deliberately has
    /// no `_brand.yml` auto-discovery, unlike Q1 — and also when the
    /// brand could not be read or parsed. Failure is silent *here* on
    /// purpose: `CompileThemeCssStage` resolves brand again from the
    /// merged document metadata and raises the user-facing
    /// diagnostic, with a source location this early parse doesn't
    /// have. Raising from both sites would report one mistake twice.
    ///
    /// The double resolution is not redundant — the two answer
    /// different questions. The theme stage asks "what brand applies
    /// to *this document*", which a document can override in its own
    /// frontmatter; this field is the **site-level** brand, which is
    /// the right scope for site-wide artifacts like the favicon.
    /// Quarto 1 draws the same distinction (`project.resolveBrand()`
    /// vs. the per-file variant in `project-shared.ts`).
    ///
    /// Consumers: the brand-aware favicon fallback
    /// ([`crate::project::website_config::website_favicon`]); the
    /// navbar brand image is expected to join it (bd-hp3tx).
    pub brand: Option<ResolvedBrand>,

    /// The resolved **project-profile** activation set
    /// (bd-fu16z22k), in activation order, with per-profile
    /// provenance for the `-v` echo.
    ///
    /// ⚠️ Not related to [`crate::document_profile::DocumentProfile`]
    /// (the pass-1 summary) — this is the Quarto 1 "project profiles"
    /// feature (`--profile` / `QUARTO_PROFILE` / `_quarto-<name>.yml`).
    /// Empty when no profiles are active (including single-file
    /// pseudo-projects and default-constructed configs).
    pub active_config_profiles: Vec<project_profile::ActiveProfile>,

    /// Paths of the profile overlay files (`_quarto-<name>.yml`) and
    /// the local override (`_quarto.yml.local`) that were actually
    /// read and merged into [`metadata`](Self::metadata), in merge
    /// order (lowest priority first).
    ///
    /// Candidate source files for
    /// [`crate::config_sources::bind_config_source`], alongside
    /// [`config_path`](Self::config_path) and
    /// [`extension_manifest_paths`](Self::extension_manifest_paths):
    /// merged values keep the filename-hash FileId of the overlay
    /// they were written in. Also a cache-key input (Phase 2).
    pub profile_config_paths: Vec<PathBuf>,
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

    // Q1 types `engines` as `string[]`. Map-form entries (Plan 6's claim
    // tables) are legal Rust-side now, but the wire only needs the ORDERING
    // name — lower to names only via the shared `engine_entry_name`: strings
    // pass through, a single-key map contributes its key, `path:`-maps are
    // skipped (no name known Rust-side until Plan 4b's external-engine work).
    if let Some(engines) = metadata.get("engines").and_then(|v| v.as_array()) {
        let names: Vec<TsMetadataValue> = engines
            .iter()
            .filter_map(engine_entry_name)
            .map(TsMetadataValue::String)
            .collect();
        map.insert("engines".to_string(), TsMetadataValue::Array(names));
    }

    if let Some(output_dir) = &config.output_dir {
        map.insert(
            "output-dir".to_string(),
            TsMetadataValue::String(output_dir.to_string_lossy().into_owned()),
        );
    }

    Some(map)
}

/// Extract the ordering name (if any) from one project `_quarto.yml`
/// `engines:` list entry (Plan 6 design contract `engine-and-engines-keys.md`).
///
/// The plural, project-level `engines:` key is Q1-compatible: entries come
/// in THREE forms, only two of which contribute an ordering entry:
///
/// - **string** (`knitr`) — a bare ordering entry → `Some(name)`.
/// - **`{path: …}` map** — Q1's external-engine loader. RESERVED / SKIPPED by
///   q2 (engines arrive via `_extensions/` discovery instead) — contributes
///   NO ordering entry and must never error → `None`.
/// - **`{<name>: {claims: …}}` single-key map** — Plan 6's claim-table entry.
///   Its KEY is the engine name, so it ALSO contributes an ordering entry;
///   the payload is Plan 6's and is ignored here → `Some(name)`.
///
/// Shared by the project `engines:` ordering splice
/// ([`build_engine_registry`]'s Task-9 site, native-only) and Plan 6's
/// claim-table reader (`engine::resolution`, WASM-compatible) — keep this the
/// single source of entry-name parsing so Plan 6 does not need a second one.
/// `pub(crate)` and NOT wasm-gated: the body has no native dependencies (pure
/// `ConfigValue` traversal), and `engine::resolution` — which is documented
/// WASM-compatible — calls it unconditionally.
pub(crate) fn engine_entry_name(entry: &ConfigValue) -> Option<String> {
    // String form: `- knitr`.
    if let Some(name) = entry.as_plain_text() {
        return Some(name);
    }
    // Map forms: distinguish `{path: …}` (reserved/skipped) from
    // `{<name>: {claims: …}}` (single-key map; key IS the name).
    if let Some(entries) = entry.as_map_entries() {
        if entries.iter().any(|e| e.key == "path") {
            return None;
        }
        if let [only] = entries {
            return Some(only.key.clone());
        }
    }
    None
}

/// Compute the set of project `engines:` entry names that carry a `claims`
/// table (Plan 6 Phase 3/5) — the single-key-map entries whose payload has a
/// `claims` key, via the shared [`engine_entry_name`] parser. Empty when the
/// project has no `engines:` key, or none of its entries table anything.
/// Stashed on [`ProjectContext::tabled_engines`] for Phase 5's warning, which
/// needs to tell "untabled, claims-less engine" apart from "deliberately
/// tabled" when deciding whether a Pass-1 fall-through is actionable.
fn tabled_engine_names(config: Option<&ProjectConfig>) -> std::collections::HashSet<String> {
    let Some(engines) = config
        .and_then(|c| c.metadata.as_ref())
        .and_then(|m| m.get("engines"))
        .and_then(|e| e.as_array())
    else {
        return std::collections::HashSet::new();
    };
    engines
        .iter()
        .filter_map(|entry| {
            let name = engine_entry_name(entry)?;
            entry.get(&name)?.get("claims")?;
            Some(name)
        })
        .collect()
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
                        extension_yml_path,
                        name,
                        claims,
                        file_extensions,
                        claims_files,
                    } => {
                        // Step 4a: Bundle-exists check
                        if !path.exists() {
                            return Err(QuartoError::Other(format!(
                                "Engine extension '{}' has no bundled .js file at {}. \
                                Run 'q2 call build-ts-extension' in {} to build it.",
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
                        // Plan 6 Phase 5: record the contributing extension's
                        // `_extension.yml` path for the fall-through warning
                        // and the Pass-1 cache key.
                        engine.set_extension_yml_path(extension_yml_path.clone());
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

    // ── Task 9 / Plan 4b-C: splice project `_quarto.yml` `engines:` list ─────
    // Q1 ordering semantics: user-declared project entries win the tiebreak —
    // prepend their (deduped, in listed order) names ahead of the
    // extension-contributed order already in `registry.contribution_order`,
    // so they take effect BEFORE extension auto-promotion. All three grammar
    // forms are parsed via `engine_entry_name` (Plan 6 reuses it). The
    // prepended names re-enter the SAME step-6 validation below as any other
    // contribution_order entry — the single validation site Plan 6 relies on,
    // no second check needed here.
    if let Some(project_engines) = config
        .and_then(|c| c.metadata.as_ref())
        .and_then(|m| m.get("engines"))
        .and_then(|e| e.as_array())
    {
        let project_names: Vec<String> = project_engines
            .iter()
            .filter_map(engine_entry_name)
            .collect();

        let mut combined: Vec<String> = project_names;
        combined.extend(registry.contribution_order.iter().cloned());
        let mut seen = HashSet::new();
        registry.contribution_order = combined
            .into_iter()
            .filter(|n| seen.insert(n.clone()))
            .collect();
    }

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
///
/// Returns the intake diagnostics alongside the extensions (main's
/// `discover_extensions` gained them with the Q1-compat intake relaxation,
/// bd-8b0af414). The caller folds them into `ProjectConfig::config_diagnostics`
/// so a malformed `_extension.yml` is still reported even though this half of
/// the split runs before the config is finalized.
fn discover_extensions_only(
    discovery_anchor: &Path,
    project_dir: Option<&Path>,
    runtime: &dyn SystemRuntime,
) -> (
    Vec<Extension>,
    Vec<quarto_error_reporting::DiagnosticMessage>,
) {
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
    // This wrapper has no diagnostic sink of its own; `ProjectContext::discover`
    // is the path that surfaces intake diagnostics (see the call there).
    let (extensions, _intake_diagnostics) =
        discover_extensions_only(discovery_anchor, project_dir, runtime);
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

    /// Names of project-level `engines:` entries that carry a `claims` table
    /// (Plan 6 Phase 3). Consumed by Phase 5's index-pass warning to tell
    /// "this engine is untabled and claims-less" apart from "this engine was
    /// deliberately given a table" when deciding whether a fall-through is
    /// actionable. Empty when the project has no `engines:` key (default).
    pub tabled_engines: std::collections::HashSet<String>,
}

/// What [`ProjectContext::apply_project_profiles`] resolved: the
/// activation set and the overlay/local file paths actually merged.
struct ProfileApplyOutcome {
    active: Vec<project_profile::ActiveProfile>,
    paths: Vec<PathBuf>,
}

/// The file name of a config path, for diagnostics (`_quarto.yml`,
/// `_quarto-prod.yml`, …). Falls back to the full display string for
/// pathological paths.
fn file_label(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .map_or_else(|| path.display().to_string(), str::to_string)
}

/// Read + parse one profile config layer (`_quarto-<name>.yml` or
/// `_quarto.yml.local`) into a source-tracked [`ConfigValue`], with
/// the same interpretation context as the base `_quarto.yml`.
/// Parse failures abort loudly (Q1 parity: a malformed profile file
/// stops the render).
fn read_config_layer(path: &Path, runtime: &dyn SystemRuntime) -> Result<ConfigValue> {
    use pampa::pandoc::yaml_to_config_value;
    use pampa::utils::diagnostic_collector::DiagnosticCollector;
    use quarto_config::InterpretationContext;

    let content = runtime
        .file_read_string(path)
        .map_err(|e| QuartoError::Other(format!("Failed to read {}: {}", path.display(), e)))?;
    let filename = path.to_string_lossy().to_string();
    let yaml = quarto_yaml::parse_file(&content, &filename)
        .map_err(|e| QuartoError::Other(format!("Failed to parse {}: {}", path.display(), e)))?;
    // Like the base-config parse above, tag diagnostics from the
    // collector are not surfaced here; MetadataMergeStage re-collects
    // them per document.
    let mut collector = DiagnosticCollector::new();
    Ok(yaml_to_config_value(
        yaml,
        InterpretationContext::ProjectConfig,
        &mut collector,
    ))
}

impl ProjectContext {
    /// Discover project context from a path.
    ///
    /// If the path is a file, looks for `_quarto.yml` in parent directories.
    /// If the path is a directory, looks for `_quarto.yml` in that directory and parents.
    ///
    /// If no `_quarto.yml` is found, creates a single-file pseudo-project.
    ///
    /// Project-profile activation (bd-fu16z22k) reads the
    /// `QUARTO_PROFILE` environment variable through the runtime; to
    /// pass an explicit `--profile` selection instead, use
    /// [`Self::discover_with_profile`].
    pub fn discover(path: impl AsRef<Path>, runtime: &dyn SystemRuntime) -> Result<Self> {
        Self::discover_with_profile(path, runtime, None)
    }

    /// [`Self::discover`] with an explicit project-profile selection.
    ///
    /// `cli_selection` carries `--profile` values (each may itself be
    /// a comma/space-separated list). `Some` **replaces** the
    /// `QUARTO_PROFILE` environment variable entirely (Q1 parity);
    /// `None` falls back to the environment variable and the config
    /// defaults. See [`project_profile`] for the precedence chain.
    pub fn discover_with_profile(
        path: impl AsRef<Path>,
        runtime: &dyn SystemRuntime,
        cli_selection: Option<&[String]>,
    ) -> Result<Self> {
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
        let (project_dir, config) = Self::find_project_config(&search_dir, runtime, cli_selection)?;

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
        let (extensions, extension_intake_diagnostics) =
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

        // Project-less discovery (no `_quarto.yml` found): still
        // resolve profile activation from the explicit selection /
        // `QUARTO_PROFILE`, so `--profile bad/name` errors here too
        // and conditional content (`when-profile`) sees the active
        // set. There are no overlays, env files, or declarations to
        // match, so the Q-5-19 unknown-profile warning never fires.
        let config = match config {
            Some(config) => config,
            None => {
                let mut diagnostics = Vec::new();
                let env_profile = runtime
                    .env_get(project_profile::QUARTO_PROFILE_VAR)
                    .ok()
                    .flatten();
                let active = project_profile::resolve_active_profiles(
                    &project_profile::ProfileResolutionInputs {
                        cli: cli_selection,
                        env_var: env_profile.as_deref(),
                        ..Default::default()
                    },
                    &mut diagnostics,
                );
                if diagnostics
                    .iter()
                    .any(|d| d.kind == quarto_error_reporting::DiagnosticKind::Error)
                {
                    return Err(QuartoError::Parse(crate::error::ParseError::new(
                        diagnostics,
                        quarto_source_map::SourceContext::new(),
                    )));
                }
                ProjectConfig {
                    active_config_profiles: active,
                    config_diagnostics: diagnostics,
                    ..Default::default()
                }
            }
        };

        // Surface the extension-intake diagnostics produced by the pre-walk
        // half of the Corollary-0 split. `discover_extensions` gained these
        // with the Q1-compat intake relaxation (bd-8b0af414); on main the
        // only caller was `StageContext`, which folds them into its startup
        // diagnostics. This path runs earlier (before the config exists), so
        // they are appended here instead — dropping them would silently lose
        // every malformed-`_extension.yml` report under a project render.
        let mut config = config;
        config
            .config_diagnostics
            .extend(extension_intake_diagnostics);

        let binary_dependencies = BinaryDependencies::discover(runtime);
        // `config` is no longer an `Option` at this point (main rebound it
        // just above for project-less profile resolution). A default
        // `ProjectConfig` yields no tabled engines, exactly as the old
        // `None` did for single-file renders.
        let tabled_engines = tabled_engine_names(Some(&config));

        let registry = build_registry(
            &extensions,
            project_dir_for_extensions,
            is_single_file,
            &output_dir,
            Some(&config),
            &binary_dependencies,
            runtime,
        )?;

        Ok(Self {
            dir,
            config,
            is_single_file,
            files,
            output_dir,
            registry,
            extensions,
            binary_dependencies,
            tabled_engines,
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
            // No project config for a single-file pseudo-project → no
            // `engines:` key to read.
            tabled_engines: std::collections::HashSet::new(),
        })
    }

    /// Search for `_quarto.yml` in directory and parents
    fn find_project_config(
        start_dir: &Path,
        runtime: &dyn SystemRuntime,
        cli_selection: Option<&[String]>,
    ) -> Result<(Option<PathBuf>, Option<ProjectConfig>)> {
        let mut current = start_dir.to_path_buf();

        loop {
            let config_path = current.join("_quarto.yml");
            let exists = runtime
                .path_exists(&config_path, None)
                .map_err(|e| QuartoError::Other(format!("Failed to check config path: {}", e)))?;
            if exists {
                // Found config file - parse it
                let config = Self::parse_config(&config_path, runtime, cli_selection)?;
                return Ok((Some(current), Some(config)));
            }

            // Also check for _quarto.yaml (alternate extension)
            let config_path_yaml = current.join("_quarto.yaml");
            let exists_yaml = runtime
                .path_exists(&config_path_yaml, None)
                .map_err(|e| QuartoError::Other(format!("Failed to check config path: {}", e)))?;
            if exists_yaml {
                let config = Self::parse_config(&config_path_yaml, runtime, cli_selection)?;
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
    fn parse_config(
        path: &Path,
        runtime: &dyn SystemRuntime,
        cli_selection: Option<&[String]>,
    ) -> Result<ProjectConfig> {
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
        let mut metadata =
            yaml_to_config_value(yaml, InterpretationContext::ProjectConfig, &mut diagnostics);

        // ── Project profiles (bd-fu16z22k) ──────────────────────────
        // Resolve the activation set and merge `_quarto-<name>.yml`
        // overlays plus `_quarto.yml.local` into `metadata` *before*
        // any field below is extracted, so `project.type`,
        // `output-dir`, `render`, `resources`, render scripts, and
        // `brand` are all profile-aware. ("Profiles" here = project
        // profiles, not DocumentProfile.)
        let mut profile_diagnostics: Vec<quarto_error_reporting::DiagnosticMessage> = Vec::new();
        let profile_outcome = Self::apply_project_profiles(
            &mut metadata,
            path,
            runtime,
            cli_selection,
            &mut profile_diagnostics,
        )?;
        let active_config_profiles = profile_outcome.active;
        let profile_config_paths = profile_outcome.paths;

        // Discover project-scoped extensions once (bd-ad7i1pc6): both
        // custom-type resolution and `contributes.metadata.project`
        // merging consume the same list. Manifest-load failures
        // (Q-16-1) are attached to the Q-5-17 error when type
        // resolution fails; otherwise they are dropped here — the
        // per-document discovery in `StageContext::new` re-reports
        // them on every render.
        let config_dir = path.parent().unwrap_or(Path::new("."));
        let builtin_dir = crate::extension::builtin_extensions_path(runtime);
        let (extensions, load_diagnostics) = crate::extension::discover_project_extensions(
            config_dir,
            builtin_dir.as_deref(),
            runtime,
        );

        // Resolve `project.type` (bd-ad7i1pc6). Built-in names parse
        // directly; a custom name resolves against the discovered
        // extensions, whose `contributes.project` fragment is merged
        // *under* the user's config (mutating `metadata`) before any
        // field below is extracted. An unresolvable `project.type` is
        // a hard error (Q-5-17), not a silent fall-back to the
        // default kind.
        let resolved =
            resolve_project_type(&mut metadata, path, &extensions, &load_diagnostics, runtime)?;
        let project_kind = resolved.kind;

        // Merge `contributes.metadata.project` from every discovered
        // extension (user wins; bd-zb2tod5f semantics) — before the
        // field extraction below, so contributed `pre-render` /
        // `resources` / `output-dir` entries take effect.
        apply_metadata_project_contributions(&mut metadata, &extensions, config_dir, runtime);

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
                    .filter_map(|v| {
                        v.as_str()
                            .map(|s| crate::glob::RawGlob::new(s, v.source_info.clone()))
                    })
                    .collect()
            })
            .unwrap_or_default();

        let resources = crate::project_resources::extract_resource_patterns(
            &metadata,
            &["project", "resources"],
        );

        let pre_render_scripts = render_scripts::extract_render_scripts(&metadata, "pre-render");
        let post_render_scripts = render_scripts::extract_render_scripts(&metadata, "post-render");

        // Resolve the project-level `brand:` once, here, so every
        // site-wide consumer reads the same answer. Relative brand
        // paths are written against the directory holding this
        // `_quarto.yml`, which is the project root.
        //
        // Errors are swallowed deliberately — see the `brand` field's
        // docs on `ProjectConfig`. `CompileThemeCssStage` re-resolves
        // from merged metadata and owns the user-facing diagnostic.
        let project_dir = path.parent().unwrap_or(Path::new("."));
        let brand = quarto_sass::resolve_brand(&metadata, runtime, project_dir)
            .ok()
            .flatten();

        // See the field docs: candidate source files for
        // config-anchored diagnostics (bd-m6wmztln).
        let extension_manifest_paths = extensions
            .iter()
            .map(|ext| ext.path.join("_extension.yml"))
            .collect();

        // Profile warnings first (they arose first), then
        // type-resolution warnings.
        let mut config_diagnostics = profile_diagnostics;
        config_diagnostics.extend(resolved.diagnostics);

        Ok(ProjectConfig {
            project_kind,
            custom_project_type: resolved.custom,
            config_diagnostics,
            output_dir,
            render_patterns,
            resources,
            pre_render_scripts,
            post_render_scripts,
            metadata: Some(metadata),
            config_path: Some(path.to_path_buf()),
            extension_manifest_paths,
            brand,
            active_config_profiles,
            profile_config_paths,
        })
    }

    /// Resolve project-profile activation and merge the overlay
    /// layers into `metadata` (bd-fu16z22k, Phase 1).
    ///
    /// Steps, in order:
    /// 1. extract + strip `profile:` from the base config;
    /// 2. parse `_quarto.yml.local` (if present) — its
    ///    `profile.default` feeds activation, its remaining content
    ///    becomes the highest-priority merge layer;
    /// 3. resolve the activation set (explicit `cli_selection`
    ///    replaces `QUARTO_PROFILE` from the runtime environment,
    ///    which beats the config defaults; then group expansion);
    /// 4. read `_quarto-<name>.yml` overlays in reverse activation
    ///    order, so the **first-listed** profile merges last and wins
    ///    conflicts (Q1 parity);
    /// 5. warn (Q-5-19) about active profiles that match nothing;
    /// 6. abort on any error-severity profile diagnostic, with the
    ///    config files registered so spans render;
    /// 7. merge `[base, overlays…, local]` and replace `metadata`.
    ///
    /// Warnings are left in `diagnostics` for the caller to surface
    /// via `config_diagnostics`.
    fn apply_project_profiles(
        metadata: &mut ConfigValue,
        path: &Path,
        runtime: &dyn SystemRuntime,
        cli_selection: Option<&[String]>,
        diagnostics: &mut Vec<quarto_error_reporting::DiagnosticMessage>,
    ) -> Result<ProfileApplyOutcome> {
        use project_profile::{
            ProfileKeySite, ProfileResolutionInputs, ProfileSource, extract_profile_config,
            resolve_active_profiles,
        };

        let config_dir = path.parent().unwrap_or(Path::new("."));
        let base_label = file_label(path);

        let profile_config = extract_profile_config(
            metadata,
            ProfileKeySite::BaseConfig,
            &base_label,
            diagnostics,
        );

        // `_quarto.yml.local` / `_quarto.yaml.local` (Q1's extension
        // order: `_quarto` + `.yml` + `.local`; `.yml` preferred).
        let local_path = ["_quarto.yml.local", "_quarto.yaml.local"]
            .iter()
            .map(|name| config_dir.join(name))
            .find(|p| matches!(runtime.path_exists(p, None), Ok(true)));
        let mut local_default: Vec<String> = Vec::new();
        let local_layer: Option<(PathBuf, ConfigValue)> = match &local_path {
            Some(local) => {
                let mut value = read_config_layer(local, runtime)?;
                let local_profile = extract_profile_config(
                    &mut value,
                    ProfileKeySite::LocalConfig,
                    &file_label(local),
                    diagnostics,
                );
                local_default = local_profile.default;
                Some((local.clone(), value))
            }
            None => None,
        };

        // Explicit CLI selection replaces the environment variable
        // entirely (Q1 parity); the env var is read through the
        // runtime so WASM/hub runtimes simply report it unset.
        let env_profile = runtime
            .env_get(project_profile::QUARTO_PROFILE_VAR)
            .ok()
            .flatten();
        let env_file_profile = environment::dotenv_quarto_profile(runtime, config_dir);
        let active = resolve_active_profiles(
            &ProfileResolutionInputs {
                cli: cli_selection,
                env_var: env_profile.as_deref(),
                env_file: env_file_profile.as_deref(),
                local_default: &local_default,
                config: &profile_config,
            },
            diagnostics,
        );

        // Overlays in reverse activation order: the first-listed
        // profile is merged last, so it wins conflicts.
        let mut overlay_layers: Vec<(PathBuf, ConfigValue)> = Vec::new();
        let mut matched_names: Vec<&str> = Vec::new();
        for profile in active.iter().rev() {
            let overlay = ["yml", "yaml"]
                .iter()
                .map(|ext| config_dir.join(format!("_quarto-{}.{ext}", profile.name)))
                .find(|p| matches!(runtime.path_exists(p, None), Ok(true)));
            let Some(overlay) = overlay else {
                continue;
            };
            let mut value = read_config_layer(&overlay, runtime)?;
            // A `profile:` key inside an overlay is inert: Q-5-22.
            extract_profile_config(
                &mut value,
                ProfileKeySite::Overlay,
                &file_label(&overlay),
                diagnostics,
            );
            overlay_layers.push((overlay, value));
            matched_names.push(profile.name.as_str());
        }

        // Q-5-19: an explicitly-selected profile that matches nothing
        // is probably a typo. Config-sourced profiles are declared by
        // construction; an `_environment-<name>` file or a
        // declaration under `profile:` also makes a name known.
        for profile in &active {
            let explicit = matches!(
                profile.source,
                ProfileSource::CliArg | ProfileSource::EnvVar | ProfileSource::EnvironmentFile
            );
            if !explicit || matched_names.contains(&profile.name.as_str()) {
                continue;
            }
            let declared = profile_config.default.iter().any(|n| n == &profile.name)
                || local_default.iter().any(|n| n == &profile.name)
                || profile_config
                    .groups
                    .iter()
                    .any(|g| g.iter().any(|n| n == &profile.name));
            let has_env_file = matches!(
                runtime.path_exists(
                    &config_dir.join(format!("_environment-{}", profile.name)),
                    None
                ),
                Ok(true)
            );
            if declared || has_env_file {
                continue;
            }
            diagnostics.push(
                quarto_error_reporting::DiagnosticMessageBuilder::warning(format!(
                    "Project profile `{}` matches nothing in this project",
                    profile.name
                ))
                .with_code("Q-5-19")
                .problem(format!(
                    "`{}` (from {}) selects no configuration: there is no \
                     `_quarto-{}.yml`, no `_environment-{}`, and the name is not \
                     declared under `profile:` in `{base_label}`. This is usually \
                     a typo.",
                    profile.name,
                    profile.source.describe(),
                    profile.name,
                    profile.name,
                ))
                .add_hint(format!(
                    "If the profile exists only for conditional content, declare \
                     it under `profile.group` or `profile.default` in \
                     `{base_label}` to silence this warning.",
                ))
                .build(),
            );
        }

        // Error-severity diagnostics abort discovery, with every
        // config file registered so spans render against the right
        // text (bd-m6wmztln discipline).
        if diagnostics
            .iter()
            .any(|d| d.kind == quarto_error_reporting::DiagnosticKind::Error)
        {
            let mut source_context = quarto_source_map::SourceContext::new();
            crate::config_sources::register_config_source(&mut source_context, path);
            if let Some(local) = &local_path {
                crate::config_sources::register_config_source(&mut source_context, local);
            }
            for (overlay, _) in &overlay_layers {
                crate::config_sources::register_config_source(&mut source_context, overlay);
            }
            return Err(QuartoError::Parse(crate::error::ParseError::new(
                std::mem::take(diagnostics),
                source_context,
            )));
        }

        // Merge: base (lowest), overlays (reverse activation order),
        // `_quarto.yml.local` (highest).
        let mut paths: Vec<PathBuf> = overlay_layers.iter().map(|(p, _)| p.clone()).collect();
        if let Some((local, _)) = &local_layer {
            paths.push(local.clone());
        }
        if !overlay_layers.is_empty() || local_layer.is_some() {
            let mut layers: Vec<&ConfigValue> = vec![metadata];
            layers.extend(overlay_layers.iter().map(|(_, v)| v));
            if let Some((_, v)) = &local_layer {
                layers.push(v);
            }
            let merged = quarto_config::merge_with_diagnostics(layers, diagnostics)
                .map(|m| m.materialize())
                .and_then(|r| match r {
                    Ok(v) => Some(v),
                    Err(e) => {
                        diagnostics.push(
                            quarto_error_reporting::DiagnosticMessageBuilder::error(
                                "Failed to merge project profile configuration",
                            )
                            .with_code("Q-1-23")
                            .problem(format!(
                                "Merging the profile configuration layers failed: {e}"
                            ))
                            .build(),
                        );
                        None
                    }
                });
            match merged {
                Some(merged) => *metadata = merged,
                None => {
                    return Err(QuartoError::Parse(crate::error::ParseError::new(
                        std::mem::take(diagnostics),
                        quarto_source_map::SourceContext::new(),
                    )));
                }
            }
        }

        Ok(ProfileApplyOutcome { active, paths })
    }

    /// Get the project kind.
    pub fn project_kind(&self) -> ProjectKind {
        self.config.project_kind
    }

    /// Human-readable project-type label for status output.
    ///
    /// Built-in types print their name (`website`); custom types print
    /// the name the user wrote plus the resolved base kind:
    /// `posit-docs (website)`.
    pub fn project_type_label(&self) -> String {
        match &self.config.custom_project_type {
            Some(custom) => format!("{} ({})", custom.name, self.project_kind().as_str()),
            None => self.project_kind().as_str().to_string(),
        }
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
                title: Some("Test".to_string()),
                author: Some("Test".to_string()),
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
                extension_yml_path: PathBuf::from("/test/_extension.yml"),
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
                    extension_yml_path: PathBuf::from("/test/_extension.yml"),
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

    // === project.type config parsing tests (bd-sekn481x) ===

    mod project_type_config_tests {
        use super::*;
        use quarto_system_runtime::NativeRuntime;
        use std::fs;
        use tempfile::TempDir;

        fn discover_with_config(config: &str) -> Result<ProjectContext> {
            let temp = TempDir::new().unwrap();
            fs::write(temp.path().join("_quarto.yml"), config).unwrap();
            fs::write(temp.path().join("index.qmd"), "---\ntitle: x\n---\n\nhi\n").unwrap();
            let runtime = NativeRuntime::new();
            ProjectContext::discover(temp.path(), &runtime)
        }

        /// An unrecognized `project.type` (e.g. a Quarto 1 extension
        /// project type) must be a hard error, not a silent fallback
        /// to `default` — the fallback strews per-document lib copies
        /// into the source tree of what the user meant to be a
        /// website (bd-sekn481x).
        #[test]
        fn unknown_project_type_is_hard_error() {
            let err = discover_with_config("project:\n  type: posit-docs\n")
                .expect_err("unknown project.type must fail discovery");
            let QuartoError::Parse(parse_err) = err else {
                panic!("expected QuartoError::Parse, got: {err:?}");
            };
            assert_eq!(parse_err.diagnostics.len(), 1);
            let d = &parse_err.diagnostics[0];
            assert_eq!(d.code.as_deref(), Some("Q-5-17"));
            assert!(
                d.location.is_some(),
                "diagnostic must point at the `type:` scalar in _quarto.yml"
            );
            let rendered = parse_err.render();
            assert!(
                rendered.contains("posit-docs"),
                "diagnostic must name the offending type; got:\n{rendered}"
            );
            for valid in ["default", "website", "book", "manuscript"] {
                assert!(
                    rendered.contains(valid),
                    "diagnostic must list valid type `{valid}`; got:\n{rendered}"
                );
            }
            assert!(
                rendered.contains("_quarto.yml"),
                "diagnostic must render a source snippet naming the file; got:\n{rendered}"
            );
        }

        /// A non-string `project.type` gets the same hard error
        /// rather than silently becoming `default` via `as_str() ->
        /// None`.
        #[test]
        fn non_string_project_type_is_hard_error() {
            let err = discover_with_config("project:\n  type:\n    nested: map\n")
                .expect_err("non-string project.type must fail discovery");
            let QuartoError::Parse(parse_err) = err else {
                panic!("expected QuartoError::Parse, got: {err:?}");
            };
            assert_eq!(parse_err.diagnostics.len(), 1);
            let d = &parse_err.diagnostics[0];
            assert_eq!(d.code.as_deref(), Some("Q-5-17"));
            let rendered = parse_err.render();
            assert!(
                rendered.contains("string"),
                "diagnostic must say the value should be a string; got:\n{rendered}"
            );
        }

        /// Absent `project.type` keeps defaulting to `default` — the
        /// documented behavior for bare projects.
        #[test]
        fn absent_project_type_still_defaults() {
            let ctx = discover_with_config("project:\n  render:\n    - \"*.qmd\"\n").unwrap();
            assert_eq!(ctx.config.project_kind, ProjectKind::Default);
        }

        /// Recognized types (including case-insensitive spellings)
        /// keep parsing.
        #[test]
        fn valid_project_types_still_parse() {
            for (spelling, kind) in [
                ("default", ProjectKind::Default),
                ("website", ProjectKind::Website),
                ("book", ProjectKind::Book),
                ("manuscript", ProjectKind::Manuscript),
                ("WEBSITE", ProjectKind::Website),
            ] {
                let ctx = discover_with_config(&format!("project:\n  type: {spelling}\n")).unwrap();
                assert_eq!(ctx.config.project_kind, kind, "spelling: {spelling}");
            }
        }

        /// Belt-and-braces: the code this module emits must exist in
        /// the shared catalog, under the 'project' subsystem (mirrors
        /// `theme_diagnostic_code_is_registered_in_catalog`).
        #[test]
        fn unknown_project_type_code_is_registered_in_catalog() {
            let info = quarto_error_catalog::ERROR_CATALOG.get("Q-5-17");
            assert!(
                info.is_some(),
                "Q-5-17 is not registered in error_catalog.json"
            );
            let info = info.unwrap();
            assert_eq!(info.subsystem, "project");
            assert!(
                info.docs_url
                    .as_deref()
                    .is_some_and(|u| u.ends_with("Q-5-17")),
                "Q-5-17 docs_url must end with the code; got: {:?}",
                info.docs_url
            );
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
            assert_eq!(result[0].1.get("toc").unwrap().as_bool(), Some(true));
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
            assert_eq!(result[0].1.get("toc").unwrap().as_bool(), Some(true));
            assert_eq!(result[1].1.get("toc-depth").unwrap().as_int(), Some(2));
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
            assert_eq!(result[0].1.get("toc").unwrap().as_bool(), Some(true));
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
            assert_eq!(result[0].1.get("toc").unwrap().as_bool(), Some(true));
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
            let css_value = result[0].1.get("css").expect("should have css key");

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
            let css_value = result[0].1.get("css").expect("should have css key");

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
            let theme_value = result[0].1.get("theme").expect("should have theme key");

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
            let css_value = result[0].1.get("css").expect("should have css key");

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
                .1
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
                .1
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
                .1
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
            tabled_engine_names,
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
                ..Default::default()
            };

            let map = build_engine_config_map(&config).expect("config map must be Some");

            // engines key present, lowered to NAMES ONLY (Plan 6 Phase 3 item
            // E): `knitr` (string) passes through; `{path: "x.js"}` names no
            // engine Rust-side and is skipped — the wire types `engines` as
            // `string[]`, so a `{path: ...}` entry lowering to a `Map` (the
            // pre-Phase-3 behavior) was never a valid wire value.
            assert_eq!(
                map.get("engines"),
                Some(&TsMetadataValue::Array(vec![TsMetadataValue::String(
                    "knitr".to_string()
                ),])),
                "engines key must be lowered to names only, dropping the {{path:}} entry"
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

        // ── builder: single-key claim-table map entry lowers to its name ────────
        //
        // Plan 6 Phase 3 item E: `engines:` map-form entries (claim tables)
        // are legal Rust-side now; the wire only needs the ordering NAME.
        //
        // Named revert: reverting the `engine_entry_name` filter_map back to
        // `config_value_to_ts_metadata` verbatim-lowering makes this go RED
        // (the claim-table entry would lower to a `Map`, and the `{path:}`
        // entry would NOT be dropped).
        #[test]
        fn build_engine_config_map_lowers_claim_table_entry_to_name() {
            let engines = ConfigValue::new_array(
                vec![
                    ConfigValue::new_string("knitr", si()),
                    ConfigValue::new_map(
                        vec![ConfigMapEntry {
                            key: "legacy".to_string(),
                            key_source: si(),
                            value: ConfigValue::new_map(
                                vec![ConfigMapEntry {
                                    key: "claims".to_string(),
                                    key_source: si(),
                                    value: ConfigValue::new_array(
                                        vec![ConfigValue::new_string("python", si())],
                                        si(),
                                    ),
                                }],
                                si(),
                            ),
                        }],
                        si(),
                    ),
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
            );
            let config = ProjectConfig {
                metadata: Some(ConfigValue::new_map(
                    vec![ConfigMapEntry {
                        key: "engines".to_string(),
                        key_source: si(),
                        value: engines,
                    }],
                    si(),
                )),
                ..ProjectConfig::default()
            };

            let map = build_engine_config_map(&config).expect("config map must be Some");

            assert_eq!(
                map.get("engines"),
                Some(&TsMetadataValue::Array(vec![
                    TsMetadataValue::String("knitr".to_string()),
                    TsMetadataValue::String("legacy".to_string()),
                ])),
                "the claim-table entry's KEY (legacy) lowers to a name; the \
                 {{path:}} entry is dropped (no name known Rust-side)"
            );
        }

        // ── tabled_engine_names: stash for Phase 5's warning ─────────────────────
        //
        // Named revert: dropping the `entry.get(&name)?.get("claims")?` guard
        // (treating every named entry as tabled) makes `knitr` (a bare
        // ordering entry, no table) appear in the set → RED.
        #[test]
        fn tabled_engine_names_collects_claims_bearing_entries() {
            let engines = ConfigValue::new_array(
                vec![
                    ConfigValue::new_string("knitr", si()), // ordering only, no table
                    ConfigValue::new_map(
                        vec![ConfigMapEntry {
                            key: "legacy".to_string(),
                            key_source: si(),
                            value: ConfigValue::new_map(
                                vec![ConfigMapEntry {
                                    key: "claims".to_string(),
                                    key_source: si(),
                                    value: ConfigValue::new_array(
                                        vec![ConfigValue::new_string("python", si())],
                                        si(),
                                    ),
                                }],
                                si(),
                            ),
                        }],
                        si(),
                    ),
                ],
                si(),
            );
            let config = ProjectConfig {
                metadata: Some(ConfigValue::new_map(
                    vec![ConfigMapEntry {
                        key: "engines".to_string(),
                        key_source: si(),
                        value: engines,
                    }],
                    si(),
                )),
                ..ProjectConfig::default()
            };

            let tabled = tabled_engine_names(Some(&config));

            assert!(
                tabled.contains("legacy"),
                "legacy carries a `claims` table → must be in the set"
            );
            assert!(
                !tabled.contains("knitr"),
                "knitr is a bare ordering entry (no table) → must NOT be in the set"
            );
        }

        #[test]
        fn tabled_engine_names_empty_when_no_project_engines_key() {
            assert!(
                tabled_engine_names(None).is_empty(),
                "no project config ⇒ empty set"
            );
            let config = ProjectConfig::default();
            assert!(
                tabled_engine_names(Some(&config)).is_empty(),
                "no `engines:` key ⇒ empty set"
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

    /// Project-level brand resolution at config-parse time (bd-97yc).
    ///
    /// These pin `ProjectConfig::brand` — the site-level brand that
    /// site-wide consumers (favicon fallback today, navbar logo in
    /// bd-hp3tx) read. They are deliberately about *resolution*, not
    /// about any particular consumer.
    mod project_brand {
        use super::*;
        use quarto_system_runtime::NativeRuntime;
        use std::fs;
        use tempfile::TempDir;

        fn discover(dir: &Path) -> ProjectContext {
            ProjectContext::discover(dir, &NativeRuntime::new()).expect("discover")
        }

        #[test]
        fn no_brand_key_resolves_to_none() {
            let temp = TempDir::new().unwrap();
            fs::write(
                temp.path().join("_quarto.yml"),
                "project:\n  type: website\n",
            )
            .unwrap();
            // Present on disk but unreferenced: Q2 has no auto-discovery.
            fs::write(temp.path().join("_brand.yml"), "logo:\n  small: logo.png\n").unwrap();

            assert!(
                discover(temp.path()).config.brand.is_none(),
                "an unreferenced _brand.yml must not be picked up"
            );
        }

        #[test]
        fn brand_path_resolves_with_project_root_as_dir() {
            let temp = TempDir::new().unwrap();
            let root = temp.path().canonicalize().unwrap();
            fs::write(
                root.join("_quarto.yml"),
                "project:\n  type: website\nbrand: _brand.yml\n",
            )
            .unwrap();
            fs::write(root.join("_brand.yml"), "logo:\n  small: logo.png\n").unwrap();

            let project = discover(&root);
            let resolved = project.config.brand.expect("brand should resolve");
            assert_eq!(resolved.brand.favicon(), Some("logo.png"));
            assert_eq!(
                resolved.dir.as_deref(),
                Some(root.as_path()),
                "a root _brand.yml's dir is the project root"
            );
        }

        #[test]
        fn brand_in_subdirectory_records_that_subdirectory() {
            // The case that distinguishes a correct `dir` from a
            // hardcoded project root.
            let temp = TempDir::new().unwrap();
            let root = temp.path().canonicalize().unwrap();
            fs::write(
                root.join("_quarto.yml"),
                "project:\n  type: website\nbrand: _brand/_brand.yml\n",
            )
            .unwrap();
            fs::create_dir(root.join("_brand")).unwrap();
            fs::write(root.join("_brand/_brand.yml"), "logo:\n  small: logo.png\n").unwrap();

            let project = discover(&root);
            let resolved = project.config.brand.expect("brand should resolve");
            // The brand itself still reports the raw, brand-relative path.
            assert_eq!(resolved.brand.favicon(), Some("logo.png"));
            assert_eq!(
                resolved.dir.as_deref(),
                Some(root.join("_brand").as_path()),
                "dir must be the brand file's own directory, not the project root"
            );
        }

        #[test]
        fn inline_brand_block_resolves_with_no_dir() {
            let temp = TempDir::new().unwrap();
            fs::write(
                temp.path().join("_quarto.yml"),
                "project:\n  type: website\nbrand:\n  logo:\n    small: logo.png\n",
            )
            .unwrap();

            let project = discover(temp.path());
            let resolved = project.config.brand.expect("inline brand should resolve");
            assert_eq!(resolved.brand.favicon(), Some("logo.png"));
            assert!(
                resolved.dir.is_none(),
                "an inline block has no file, so no directory of its own"
            );
        }

        #[test]
        fn unresolvable_brand_is_silent_here() {
            // The theme stage owns this diagnostic (it has a source
            // location); reporting from both sites would show one
            // mistake twice. `discover` must not fail.
            let temp = TempDir::new().unwrap();
            fs::write(
                temp.path().join("_quarto.yml"),
                "project:\n  type: website\nbrand: does-not-exist.yml\n",
            )
            .unwrap();

            let project = ProjectContext::discover(temp.path(), &NativeRuntime::new())
                .expect("discover must not fail on an unresolvable brand");
            assert!(project.config.brand.is_none());
        }
    }
}
