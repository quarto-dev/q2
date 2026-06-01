/*
 * project_resources.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * User- and engine-declared project resources (bd-o8pr).
 */

//! User-declared additional files for a project render (`bd-o8pr`).
//!
//! Three declaration channels (see
//! `claude-notes/plans/2026-05-03-project-resources.md`):
//!
//! 1. **Project metadata** — `project.resources:` in `_quarto.yml`,
//!    parsed into [`crate::project::ProjectConfig::resources`].
//! 2. **Document metadata** — `resources:` in document YAML
//!    frontmatter, captured into
//!    [`crate::document_profile::DocumentProfile::resources`] at
//!    profile freeze time. Frozen.
//! 3. **Engine and Lua filter** (Phase 2 / Phase 3) — accumulated
//!    into a `DocumentResourceReport` by a late-pipeline collector
//!    stage.
//!
//! This module owns the type definitions and the glob/path helpers.
//! The wiring into the render pipeline lives in
//! [`crate::project::orchestrator`] and the per-project-type
//! `post_render` hooks.

use std::path::{Path, PathBuf};

use quarto_source_map::SourceInfo;
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ─────────────────────────────────────────────────────────────────────
// Types
// ─────────────────────────────────────────────────────────────────────

/// Where a resource declaration originated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ResourceOrigin {
    /// `project.resources:` in `_quarto.yml`.
    ProjectMetadata,
    /// `resources:` in a document's YAML frontmatter.
    DocumentMetadata { source: PathBuf },
    /// Returned by an engine via `ExecuteResult.supporting_files`.
    Engine { engine: String, source: PathBuf },
    /// Added by a Lua filter via `quarto.doc.add_resource(path)`.
    LuaFilter { source: PathBuf },
    // Reserved for future built-in walkers (Image src, OJS
    // FileAttachment, includes). See plan §"Internal use".
    // AutoDiscovery { kind: AutoDiscoveryKind, source: PathBuf },
}

/// Where the resource's output path is anchored.
///
/// - `Project`: anchored at `output_dir` root. Author declarations in
///   `_quarto.yml` use this scope.
/// - `Page { source }`: anchored at the document's output dir. Doc
///   YAML, engine, and Lua-filter declarations use this scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "lowercase")]
pub enum ResourceScope {
    Project,
    Page { source: PathBuf },
}

/// One resource entry resolved to an absolute on-disk source path
/// and a project-relative output path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedResource {
    /// Absolute path of the source file on disk.
    pub source: PathBuf,
    /// Output path relative to `output_dir`, forward-slash separated.
    pub output_relative: String,
    /// Where this declaration came from.
    pub origin: ResourceOrigin,
    /// Where the output path is anchored.
    pub scope: ResourceScope,
}

// ─────────────────────────────────────────────────────────────────────
// Raw YAML pattern + source info (bd-c1et2)
// ─────────────────────────────────────────────────────────────────────

/// A user-declared resource pattern with the YAML source location it
/// came from. The `source_info` is preserved through `expand_patterns`
/// into [`ResourceError`] variants so an error can be rendered as a
/// tidyverse-style diagnostic with an Ariadne span pointing at the
/// offending scalar in `_quarto.yml` or in a document header.
///
/// Carries the same `SourceInfo` value that the originating
/// [`quarto_pandoc_types::ConfigValue`] scalar held, with no
/// reinterpretation — line/column resolution happens at render time
/// against a [`quarto_source_map::SourceContext`] built from the file
/// on disk (see [`resource_error_to_diagnostic`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawResourcePattern {
    /// The raw pattern as written by the user (e.g.
    /// `"/docs/foo.json"`, `"data/*.csv"`, `"../escape"`).
    pub pattern: String,
    /// Source location of the YAML scalar that supplied
    /// [`pattern`](Self::pattern). [`SourceInfo::default`] is allowed
    /// for synthetic patterns (engine-generated, tests); diagnostics
    /// just degrade to a span-less message in that case.
    pub source_info: SourceInfo,
}

impl RawResourcePattern {
    /// Construct a pattern with the given source info.
    pub fn new(pattern: impl Into<String>, source_info: SourceInfo) -> Self {
        Self {
            pattern: pattern.into(),
            source_info,
        }
    }

    /// Construct a pattern with no source info. Use only in tests or
    /// for synthetic patterns that have no on-disk origin.
    pub fn without_source(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            source_info: SourceInfo::default(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────
// Errors
// ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum ResourceError {
    #[error(
        "resource path '{pattern}' resolves outside the project root '{project_root}'. \
         Project resources must live within the project directory."
    )]
    OutOfProject {
        pattern: String,
        project_root: PathBuf,
        /// Where this pattern appeared in YAML; preserved so the
        /// orchestrator can render an Ariadne-spanned diagnostic.
        /// [`SourceInfo::default`] when the pattern was synthetic.
        source_info: SourceInfo,
    },

    #[error("invalid glob pattern '{pattern}': {source}")]
    InvalidGlob {
        pattern: String,
        #[source]
        source: glob::PatternError,
        source_info: SourceInfo,
    },

    #[error("error walking glob matches for '{pattern}': {source}")]
    GlobWalk {
        pattern: String,
        #[source]
        source: glob::GlobError,
        source_info: SourceInfo,
    },
}

impl ResourceError {
    /// The source location of the YAML scalar that produced this
    /// error, for diagnostic rendering. Returns
    /// [`SourceInfo::default`] when the pattern was synthetic.
    pub fn source_info(&self) -> &SourceInfo {
        match self {
            ResourceError::OutOfProject { source_info, .. }
            | ResourceError::InvalidGlob { source_info, .. }
            | ResourceError::GlobWalk { source_info, .. } => source_info,
        }
    }

    /// The raw pattern string as the user wrote it.
    pub fn pattern(&self) -> &str {
        match self {
            ResourceError::OutOfProject { pattern, .. }
            | ResourceError::InvalidGlob { pattern, .. }
            | ResourceError::GlobWalk { pattern, .. } => pattern,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────
// YAML extraction
// ─────────────────────────────────────────────────────────────────────

/// Read a `resources:` field from a `ConfigValue`, accepting either a
/// list of strings or a single scalar (Q1 parity). Returns
/// [`RawResourcePattern`]s carrying each scalar's source location;
/// glob/path expansion happens later via [`expand_patterns`].
///
/// The per-entry `source_info` is the scalar's own [`SourceInfo`] —
/// for an array form the spans point at each item, for the
/// shorthand-scalar form the single span covers the lone string.
pub fn extract_resource_patterns(
    meta: &quarto_pandoc_types::ConfigValue,
    key_path: &[&str],
) -> Vec<RawResourcePattern> {
    let mut cur = meta;
    for key in key_path {
        match cur.get(key) {
            Some(v) => cur = v,
            None => return Vec::new(),
        }
    }
    if let Some(arr) = cur.as_array() {
        arr.iter()
            .filter_map(|v| {
                v.as_plain_text().map(|s| RawResourcePattern {
                    pattern: s,
                    source_info: v.source_info.clone(),
                })
            })
            .collect()
    } else if let Some(s) = cur.as_plain_text() {
        vec![RawResourcePattern {
            pattern: s,
            source_info: cur.source_info.clone(),
        }]
    } else {
        Vec::new()
    }
}

// ─────────────────────────────────────────────────────────────────────
// Glob expansion + path validation
// ─────────────────────────────────────────────────────────────────────

const GLOB_CHARS: &[char] = &['*', '?', '['];

fn looks_like_glob(s: &str) -> bool {
    s.contains(|c| GLOB_CHARS.contains(&c))
}

/// Expand a list of patterns into resolved resources.
///
/// - `project_root`: canonical project root; every resolved source
///   must be inside this directory or [`ResourceError::OutOfProject`]
///   is returned.
/// - `anchor`: directory the patterns are relative to. For project-
///   level patterns this equals `project_root`. For doc-level
///   patterns this is the document's parent directory.
/// - `patterns`: raw patterns from YAML.
/// - `make_origin`: builds the `ResourceOrigin` for each entry given
///   the pattern that produced it. Same origin for every match of
///   a single pattern.
/// - `scope`: reused across every entry produced.
///
/// **Leading-`/` semantics (Quarto YAML convention, TS Quarto parity).**
/// A pattern beginning with `/` is project-root-relative — e.g.
/// `"/docs/foo.json"` means `<project_root>/docs/foo.json`, not the
/// filesystem path `/docs/foo.json`. This applies to YAML
/// `resources:` declarations in both `_quarto.yml` and document
/// headers. It does NOT apply to engine/Lua-filter contributions,
/// which arrive through [`resolve_reported_resources`] and use
/// real filesystem semantics for absolute paths.
///
/// **Directory expansion (TS Quarto parity, bd-47w7o).** A literal
/// pattern (no glob characters) that resolves to an existing
/// directory is equivalent to the recursive glob `<dir>/**/*` —
/// every file under the directory becomes its own resource entry.
/// Trailing-slash form (`"data/"`) and bare-directory form
/// (`"data"`) behave identically. Subdirectories themselves are
/// not emitted; only their files.
pub fn expand_patterns(
    project_root: &Path,
    anchor: &Path,
    patterns: &[RawResourcePattern],
    mut make_origin: impl FnMut() -> ResourceOrigin,
    scope: ResourceScope,
) -> Result<Vec<ResolvedResource>, ResourceError> {
    let mut out = Vec::new();
    for raw in patterns {
        let matched = expand_one(project_root, anchor, &raw.pattern, &raw.source_info)?;
        for source in matched {
            let rel = source
                .strip_prefix(project_root)
                .expect("expand_one verified the path is within project_root")
                .to_string_lossy()
                .replace('\\', "/");
            out.push(ResolvedResource {
                source,
                output_relative: rel,
                origin: make_origin(),
                scope: scope.clone(),
            });
        }
    }
    Ok(out)
}

fn expand_one(
    project_root: &Path,
    anchor: &Path,
    pattern: &str,
    source_info: &SourceInfo,
) -> Result<Vec<PathBuf>, ResourceError> {
    // YAML convention (TS Quarto parity, bd-wlza2): a leading `/`
    // anchors the pattern at the project root, NOT the filesystem
    // root. Strip exactly one `/` and rebase from `project_root` so
    // the `join` below treats the remainder as relative. The
    // original `pattern` string is preserved for use in any error
    // message so the user sees what they wrote.
    //
    // Engine/Lua-filter channels do NOT go through `expand_one`;
    // they enter via `resolve_reported_resources` and keep
    // absolute-path semantics intact (engines really do return
    // filesystem-absolute paths to on-disk supporting files).
    let (base, pat) = match pattern.strip_prefix('/') {
        Some(rest) => (project_root, rest),
        None => (anchor, pattern),
    };
    if looks_like_glob(pat) {
        let combined = base.join(pat);
        return expand_glob_files(
            project_root,
            &combined.to_string_lossy(),
            pattern,
            source_info,
        );
    }

    // Literal path. Resolve and project-containment-check first; then
    // decide whether it's a directory (TS Quarto parity, bd-47w7o:
    // a literal directory is equivalent to the recursive glob
    // `<dir>/**/*`) or a single file/missing path.
    let absolute = base.join(pat);
    let canonical = canonicalize_within_project(project_root, &absolute, pattern, source_info)?;
    if canonical.is_dir() {
        let dir_glob = format!("{}/**/*", canonical.display());
        expand_glob_files(project_root, &dir_glob, pattern, source_info)
    } else {
        Ok(vec![canonical])
    }
}

/// Run `glob::glob(glob_pattern)`, drop directory entries, and
/// project-containment-check every remaining file. Shared by the
/// glob branch of `expand_one` and the literal-directory branch
/// (where we synthesize `<dir>/**/*`). `original_pattern` is the
/// user-supplied YAML string, preserved for any error message;
/// `source_info` is the YAML scalar's source location, propagated
/// into any [`ResourceError`] this returns.
fn expand_glob_files(
    project_root: &Path,
    glob_pattern: &str,
    original_pattern: &str,
    source_info: &SourceInfo,
) -> Result<Vec<PathBuf>, ResourceError> {
    let entries = glob::glob(glob_pattern).map_err(|e| ResourceError::InvalidGlob {
        pattern: original_pattern.to_string(),
        source: e,
        source_info: source_info.clone(),
    })?;
    let mut matched = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| ResourceError::GlobWalk {
            pattern: original_pattern.to_string(),
            source: e,
            source_info: source_info.clone(),
        })?;
        // Skip directories — only files become published resources.
        // The recursive-copy intent is expressed elsewhere: either by
        // the user writing `dir/**/*`, or by the literal-directory
        // branch above synthesising that pattern for a bare `dir`.
        if path.is_dir() {
            continue;
        }
        let canonical =
            canonicalize_within_project(project_root, &path, original_pattern, source_info)?;
        matched.push(canonical);
    }
    Ok(matched)
}

fn canonicalize_within_project(
    project_root: &Path,
    path: &Path,
    pattern: &str,
    source_info: &SourceInfo,
) -> Result<PathBuf, ResourceError> {
    // Best-effort canonicalization: if the file doesn't exist yet
    // (literal-path case for a file the user just declared but hasn't
    // created), fall back to lexical normalization. Either way the
    // out-of-project check uses the same prefix comparison.
    let canonical = path
        .canonicalize()
        .unwrap_or_else(|_| lexical_normalize(path));
    if !canonical.starts_with(project_root) {
        return Err(ResourceError::OutOfProject {
            pattern: pattern.to_string(),
            project_root: project_root.to_path_buf(),
            source_info: source_info.clone(),
        });
    }
    Ok(canonical)
}

/// Lexical (no-I/O) normalization that resolves `.` and `..`
/// components without consulting the filesystem. Used as a fallback
/// for paths that don't yet exist on disk.
fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        use std::path::Component;
        match component {
            Component::Prefix(p) => out.push(p.as_os_str()),
            Component::RootDir => out.push("/"),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(seg) => out.push(seg),
        }
    }
    out
}

// ─────────────────────────────────────────────────────────────────────
// Per-document resource report (engine + Lua-filter channels)
// ─────────────────────────────────────────────────────────────────────

/// One entry contributed to a document's [`DocumentResourceReport`]
/// by an engine or a Lua filter.
///
/// Stays raw (not yet resolved against the project root) so that the
/// resolution + out-of-project check happen in one place
/// ([`resolve_reported_resources`]). The `origin` already records who
/// added the entry, which is preserved in the resolved
/// [`ResolvedResource`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportedResource {
    /// Path the engine or filter handed us. Absolute or relative —
    /// the resolver anchors relatives at the document's parent
    /// directory.
    pub raw_path: PathBuf,
    /// Where this entry came from. Carries the document source path
    /// so engine/filter contributions are still attributable after
    /// being merged into the project-wide list.
    pub origin: ResourceOrigin,
}

/// Per-document accumulator drained by the orchestrator after each
/// Pass-2 render.
///
/// Engines push to this from [`crate::stage::stages::EngineExecutionStage`]
/// after [`crate::engine::ExecuteResult::supporting_files`] is
/// returned. Lua filters (Phase 3) push from
/// `quarto.doc.add_resource(path)` via the standard sidecar drain.
///
/// The orchestrator resolves entries against the project root and
/// the document's parent directory, then merges with the static-
/// channel list before the copy step.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentResourceReport {
    pub entries: Vec<ReportedResource>,
}

impl DocumentResourceReport {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append every supporting-file path produced by an engine,
    /// tagged with that engine's name.
    pub fn add_engine_files(
        &mut self,
        engine_name: &str,
        doc_source: &Path,
        files: impl IntoIterator<Item = PathBuf>,
    ) {
        for file in files {
            self.entries.push(ReportedResource {
                raw_path: file,
                origin: ResourceOrigin::Engine {
                    engine: engine_name.to_string(),
                    source: doc_source.to_path_buf(),
                },
            });
        }
    }

    /// Append every path supplied by a Lua filter (Phase 3).
    pub fn add_lua_filter_files(
        &mut self,
        doc_source: &Path,
        files: impl IntoIterator<Item = PathBuf>,
    ) {
        for file in files {
            self.entries.push(ReportedResource {
                raw_path: file,
                origin: ResourceOrigin::LuaFilter {
                    source: doc_source.to_path_buf(),
                },
            });
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Resolve a [`DocumentResourceReport`] against the project root.
/// Each entry becomes a [`ResolvedResource`] anchored at the
/// document's parent dir (for relative paths) or used as-is (for
/// absolute paths), validated for project-root containment.
///
/// The doc source is pulled from each entry's `origin`, so a single
/// call can resolve a report containing entries from multiple
/// originating documents (although in practice each report is per-doc).
pub fn resolve_reported_resources(
    project_root: &Path,
    report: &DocumentResourceReport,
) -> Result<Vec<ResolvedResource>, ResourceError> {
    let mut out = Vec::with_capacity(report.entries.len());
    for entry in &report.entries {
        let doc_source = match &entry.origin {
            ResourceOrigin::Engine { source, .. }
            | ResourceOrigin::LuaFilter { source }
            | ResourceOrigin::DocumentMetadata { source } => source.clone(),
            ResourceOrigin::ProjectMetadata => project_root.to_path_buf(),
        };
        let doc_dir = doc_source
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| project_root.to_path_buf());

        let raw_str = entry.raw_path.to_string_lossy();
        let absolute = if entry.raw_path.is_absolute() {
            entry.raw_path.clone()
        } else {
            doc_dir.join(&entry.raw_path)
        };
        // Engine/Lua-filter entries don't have a YAML source location;
        // diagnostics degrade to a span-less message.
        let canonical =
            canonicalize_within_project(project_root, &absolute, &raw_str, &SourceInfo::default())?;
        let rel = canonical
            .strip_prefix(project_root)
            .expect("canonicalize_within_project verified containment")
            .to_string_lossy()
            .replace('\\', "/");
        out.push(ResolvedResource {
            source: canonical,
            output_relative: rel,
            origin: entry.origin.clone(),
            scope: ResourceScope::Page { source: doc_source },
        });
    }
    Ok(out)
}

// ─────────────────────────────────────────────────────────────────────
// High-level collection (static channel)
// ─────────────────────────────────────────────────────────────────────

/// Collect every static-channel resource declared by the project and
/// its documents (`bd-o8pr`, Phase 1).
///
/// "Static channel" means: declarations frozen by the time the
/// pipeline reaches the collector — i.e. project YAML
/// (`project.resources:`) and document YAML (`resources:`).
/// Engine and Lua-filter channels (Phases 2 and 3) merge their
/// contributions into the same vector via the
/// `DocumentResourceReport` mechanism.
///
/// Errors out if any pattern resolves outside the project root —
/// that's a design choice for v1; see plan §"Out-of-project
/// resources".
pub fn collect_static_resources(
    project: &crate::project::ProjectContext,
    index: &crate::project::index::ProjectIndex,
) -> Result<Vec<ResolvedResource>, ResourceError> {
    let project_root = &project.dir;
    let mut out = Vec::new();

    // Project-level: anchor = project root, scope = Project.
    out.extend(expand_patterns(
        project_root,
        project_root,
        &project.config.resources,
        || ResourceOrigin::ProjectMetadata,
        ResourceScope::Project,
    )?);

    // Document-level: anchor = doc's parent dir, scope = Page.
    for profile in index.profiles() {
        if profile.resources.is_empty() {
            continue;
        }
        let doc_source_abs = if profile.source_path.is_absolute() {
            profile.source_path.clone()
        } else {
            project_root.join(&profile.source_path)
        };
        let doc_dir = doc_source_abs
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| project_root.clone());

        out.extend(expand_patterns(
            project_root,
            &doc_dir,
            &profile.resources,
            || ResourceOrigin::DocumentMetadata {
                source: doc_source_abs.clone(),
            },
            ResourceScope::Page {
                source: doc_source_abs.clone(),
            },
        )?);
    }

    Ok(out)
}

// ─────────────────────────────────────────────────────────────────────
// Diagnostic-aware variants (bd-c1et2)
// ─────────────────────────────────────────────────────────────────────

/// Build a [`crate::error::ParseError`] from a [`ResourceError`],
/// loading `source_file` so the resulting diagnostic can render an
/// Ariadne snippet pointing at the offending YAML scalar.
///
/// The [`SourceInfo`] inside `err` already carries a [`FileId`] —
/// the same `hash(filename)` that `quarto_yaml::parse_file` computes
/// when it produces source-tracked YAML. We register `source_file`
/// in a fresh [`SourceContext`] under that exact FileId so the
/// renderer can resolve offsets back to line/column in the file's
/// content.
///
/// If `source_file` cannot be read (rare; the YAML *was* read once
/// already during parse) or the [`SourceInfo`] has no resolvable
/// FileId (Concat / FilterProvenance), the diagnostic degrades to a
/// span-less message — still tidyverse-shaped, still better than
/// `Error: …` plain text.
pub fn resource_error_to_parse_error(
    err: ResourceError,
    source_file: &Path,
) -> crate::error::ParseError {
    use quarto_error_reporting::DiagnosticMessageBuilder;
    use quarto_source_map::{FileId, SourceContext};

    let mut source_context = SourceContext::new();
    if let Some((fid_usize, _, _)) = err.source_info().resolve_byte_range() {
        let content = std::fs::read_to_string(source_file).ok();
        source_context.add_file_with_id(
            FileId(fid_usize),
            source_file.to_string_lossy().into_owned(),
            content,
        );
    }

    let diagnostic = match &err {
        ResourceError::OutOfProject {
            pattern,
            project_root,
            source_info,
        } => DiagnosticMessageBuilder::error(
            "Resource path resolves outside the project root",
        )
        .with_code("Q-5-1")
        .with_location(source_info.clone())
        .problem(format!(
            "Pattern `{}` resolves outside `{}`. Project resources must live within the project directory.",
            pattern,
            project_root.display()
        ))
        .add_info(
            "A leading `/` is project-root-relative — e.g. `/docs/foo.json` means `<project>/docs/foo.json`. \
             To reference files outside the project, copy them in or use `copy:` (Q1: not yet supported).",
        )
        .build(),

        ResourceError::InvalidGlob {
            pattern,
            source,
            source_info,
        } => DiagnosticMessageBuilder::error("Invalid glob pattern in `resources:`")
            .with_code("Q-5-2")
            .with_location(source_info.clone())
            .problem(format!("`{}` is not a valid glob: {}", pattern, source))
            .build(),

        ResourceError::GlobWalk {
            pattern,
            source,
            source_info,
        } => DiagnosticMessageBuilder::error("Failed walking glob matches for `resources:`")
            .with_code("Q-5-3")
            .with_location(source_info.clone())
            .problem(format!("Walking `{}` failed: {}", pattern, source))
            .build(),
    };

    crate::error::ParseError::new(vec![diagnostic], source_context)
}

/// Diagnostic-aware variant of [`collect_static_resources`]. Same
/// result on success; on error returns a fully-rendered
/// [`crate::error::ParseError`] with the YAML span attached.
///
/// Splits the project-level and per-document calls so each error can
/// be attributed to the right source file (`_quarto.yml` vs. the
/// declaring `.qmd`).
pub fn collect_static_resources_with_diagnostics(
    project: &crate::project::ProjectContext,
    index: &crate::project::index::ProjectIndex,
) -> Result<Vec<ResolvedResource>, crate::error::ParseError> {
    let project_root = &project.dir;
    let mut out = Vec::new();

    // Project-level. Errors point into `_quarto.yml` (or `.yaml`).
    let project_yaml = project
        .config
        .config_path
        .clone()
        .unwrap_or_else(|| project_root.join("_quarto.yml"));

    if let Err(e) = (|| -> Result<(), ResourceError> {
        out.extend(expand_patterns(
            project_root,
            project_root,
            &project.config.resources,
            || ResourceOrigin::ProjectMetadata,
            ResourceScope::Project,
        )?);
        Ok(())
    })() {
        return Err(resource_error_to_parse_error(e, &project_yaml));
    }

    // Document-level. Errors point into the doc that declared the
    // bad pattern.
    for profile in index.profiles() {
        if profile.resources.is_empty() {
            continue;
        }
        let doc_source_abs = if profile.source_path.is_absolute() {
            profile.source_path.clone()
        } else {
            project_root.join(&profile.source_path)
        };
        let doc_dir = doc_source_abs
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| project_root.clone());

        if let Err(e) = (|| -> Result<(), ResourceError> {
            out.extend(expand_patterns(
                project_root,
                &doc_dir,
                &profile.resources,
                || ResourceOrigin::DocumentMetadata {
                    source: doc_source_abs.clone(),
                },
                ResourceScope::Page {
                    source: doc_source_abs.clone(),
                },
            )?);
            Ok(())
        })() {
            return Err(resource_error_to_parse_error(e, &doc_source_abs));
        }
    }

    Ok(out)
}

/// Copy resolved resources into `output_dir`, preserving each entry's
/// project-relative output path. Creates parent directories as
/// needed. Skips entries whose source equals their destination
/// (degenerate case for `output_dir` inside the project).
#[cfg(not(target_arch = "wasm32"))]
pub fn copy_resources_to_output_dir(
    resources: &[ResolvedResource],
    output_dir: &Path,
    runtime: &dyn quarto_system_runtime::SystemRuntime,
) -> crate::Result<()> {
    for entry in resources {
        let dst = output_dir.join(&entry.output_relative);
        // Source missing? Convert to a clear error before file_copy
        // produces a less-friendly one.
        let exists = runtime.path_exists(&entry.source, None).map_err(|e| {
            crate::error::QuartoError::other(format!(
                "Failed to probe resource '{}': {}",
                entry.source.display(),
                e
            ))
        })?;
        if !exists {
            return Err(crate::error::QuartoError::other(format!(
                "Declared resource '{}' does not exist on disk",
                entry.source.display()
            )));
        }
        if same_canonical_path(&entry.source, &dst) {
            continue;
        }
        if let Some(parent) = dst.parent() {
            runtime.dir_create(parent, true).map_err(|e| {
                crate::error::QuartoError::other(format!(
                    "Failed to create resource output directory '{}': {}",
                    parent.display(),
                    e
                ))
            })?;
        }
        runtime.file_copy(&entry.source, &dst).map_err(|e| {
            crate::error::QuartoError::other(format!(
                "Failed to copy resource {} → {}: {}",
                entry.source.display(),
                dst.display(),
                e
            ))
        })?;
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn same_canonical_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => a == b,
    }
}

// ─────────────────────────────────────────────────────────────────────
// Render manifest (Phase 4)
// ─────────────────────────────────────────────────────────────────────

/// One entry in the manifest's `resources` array. Mirrors
/// [`ResolvedResource`] in serializable form.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestResource {
    /// Project-relative source path, forward-slash separated.
    pub source: String,
    /// Project-relative output path inside `output_dir`,
    /// forward-slash separated.
    pub output: String,
    /// Where this entry came from. Same enum as
    /// [`ResourceOrigin`], preserved for diagnostics.
    pub origin: ResourceOrigin,
}

/// The shape written to `.quarto/render-manifest.json` after every
/// project render. The canonical input to `quarto publish`.
///
/// Schema is intentionally permissive: extra fields are ignored by
/// consumers, so we can add fields without breaking older `quarto
/// publish` versions. `version` is the schema version, bumped only
/// for breaking changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderManifest {
    pub version: u32,
    /// Project-relative paths of every primary rendered output
    /// (e.g. `index.html`, `posts/foo.html`).
    pub rendered_files: Vec<String>,
    /// Project-relative paths of every resource published with the
    /// site, plus the origin metadata.
    pub resources: Vec<ManifestResource>,
}

impl RenderManifest {
    pub const VERSION: u32 = 1;
    pub const FILENAME: &'static str = ".quarto/render-manifest.json";

    pub fn new(
        project_root: &Path,
        rendered_files: Vec<String>,
        resources: &[ResolvedResource],
    ) -> Self {
        let resources = resources
            .iter()
            .map(|r| ManifestResource {
                source: project_relative_str(&r.source, Some(project_root))
                    .unwrap_or_else(|| r.source.to_string_lossy().replace('\\', "/")),
                output: r.output_relative.clone(),
                origin: r.origin.clone(),
            })
            .collect();
        Self {
            version: Self::VERSION,
            rendered_files,
            resources,
        }
    }

    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

/// Compute a project-relative path string. If the path can be made
/// relative to `project_root` (and a root was supplied), return that.
/// Otherwise return the absolute string for diagnostics. Always
/// forward-slash separated.
fn project_relative_str(path: &Path, project_root: Option<&Path>) -> Option<String> {
    project_root
        .and_then(|root| path.strip_prefix(root).ok())
        .map(|p| p.to_string_lossy().replace('\\', "/"))
}

/// Write the manifest to `<project_dir>/.quarto/render-manifest.json`.
/// Native-only (the in-browser renderer doesn't have a project dir).
#[cfg(not(target_arch = "wasm32"))]
pub fn write_render_manifest(
    project_dir: &Path,
    manifest: &RenderManifest,
    runtime: &dyn quarto_system_runtime::SystemRuntime,
) -> crate::Result<()> {
    let path = project_dir.join(RenderManifest::FILENAME);
    if let Some(parent) = path.parent() {
        runtime.dir_create(parent, true).map_err(|e| {
            crate::error::QuartoError::other(format!(
                "Failed to create .quarto directory '{}': {}",
                parent.display(),
                e
            ))
        })?;
    }
    let json = manifest.to_json_pretty().map_err(|e| {
        crate::error::QuartoError::other(format!("Failed to serialize render manifest: {}", e))
    })?;
    runtime.file_write(&path, json.as_bytes()).map_err(|e| {
        crate::error::QuartoError::other(format!(
            "Failed to write render manifest '{}': {}",
            path.display(),
            e
        ))
    })?;
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn touch(path: &Path) {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).unwrap();
        }
        std::fs::write(path, b"x").unwrap();
    }

    /// Test helper: build a `RawResourcePattern` with no source info.
    /// Tests that exercise [`expand_patterns`] don't depend on source
    /// info; tests that *do* care construct `RawResourcePattern`
    /// directly with the relevant [`SourceInfo`].
    fn raw(pattern: &str) -> RawResourcePattern {
        RawResourcePattern::without_source(pattern)
    }

    /// Test helper: collect pattern strings out of an extract result,
    /// dropping source info for value-only assertions.
    fn just_patterns(v: &[RawResourcePattern]) -> Vec<String> {
        v.iter().map(|r| r.pattern.clone()).collect()
    }

    #[test]
    fn looks_like_glob_basic() {
        assert!(looks_like_glob("data/*.csv"));
        assert!(looks_like_glob("img/?.png"));
        assert!(looks_like_glob("[ab].txt"));
        assert!(!looks_like_glob("plain.txt"));
        assert!(!looks_like_glob("data/file.csv"));
    }

    #[test]
    fn expand_literal_path() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        touch(&root.join("a.txt"));

        let resolved = expand_patterns(
            &root,
            &root,
            &[raw("a.txt")],
            || ResourceOrigin::ProjectMetadata,
            ResourceScope::Project,
        )
        .unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].output_relative, "a.txt");
        assert_eq!(
            resolved[0].source,
            root.join("a.txt").canonicalize().unwrap()
        );
    }

    #[test]
    fn expand_glob() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        touch(&root.join("data/a.csv"));
        touch(&root.join("data/b.csv"));
        touch(&root.join("data/skip.txt"));

        let mut resolved = expand_patterns(
            &root,
            &root,
            &[raw("data/*.csv")],
            || ResourceOrigin::ProjectMetadata,
            ResourceScope::Project,
        )
        .unwrap();
        resolved.sort_by(|a, b| a.output_relative.cmp(&b.output_relative));
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0].output_relative, "data/a.csv");
        assert_eq!(resolved[1].output_relative, "data/b.csv");
    }

    #[test]
    fn glob_skips_directories() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("data/sub")).unwrap();
        touch(&root.join("data/file.txt"));

        let resolved = expand_patterns(
            &root,
            &root,
            &[raw("data/*")],
            || ResourceOrigin::ProjectMetadata,
            ResourceScope::Project,
        )
        .unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].output_relative, "data/file.txt");
    }

    #[test]
    fn out_of_project_literal_is_error() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();

        let err = expand_patterns(
            &root,
            &root,
            &[raw("../outside.csv")],
            || ResourceOrigin::ProjectMetadata,
            ResourceScope::Project,
        )
        .unwrap_err();
        assert!(matches!(err, ResourceError::OutOfProject { .. }));
    }

    // === Leading-slash patterns are project-root-relative ===
    //
    // Quarto YAML convention (matching TS Quarto): a `resources:`
    // entry beginning with `/` is anchored at the project root,
    // *not* the filesystem root. See bd-wlza2.

    #[test]
    fn expand_leading_slash_literal_is_project_relative() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        touch(&root.join("data/a.txt"));

        let resolved = expand_patterns(
            &root,
            &root,
            &[raw("/data/a.txt")],
            || ResourceOrigin::ProjectMetadata,
            ResourceScope::Project,
        )
        .unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].output_relative, "data/a.txt");
        assert_eq!(
            resolved[0].source,
            root.join("data/a.txt").canonicalize().unwrap()
        );
    }

    #[test]
    fn expand_leading_slash_glob_is_project_relative() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        touch(&root.join("data/a.csv"));
        touch(&root.join("data/b.csv"));

        let mut resolved = expand_patterns(
            &root,
            &root,
            &[raw("/data/*.csv")],
            || ResourceOrigin::ProjectMetadata,
            ResourceScope::Project,
        )
        .unwrap();
        resolved.sort_by(|a, b| a.output_relative.cmp(&b.output_relative));
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0].output_relative, "data/a.csv");
        assert_eq!(resolved[1].output_relative, "data/b.csv");
    }

    #[test]
    fn expand_leading_slash_doc_pattern_anchors_to_project_root_not_doc_dir() {
        // A doc under <root>/posts/ declares "/shared.js" — that's the
        // project-root-relative `<root>/shared.js`, not `<root>/posts/shared.js`.
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let doc_dir = root.join("posts");
        std::fs::create_dir_all(&doc_dir).unwrap();
        touch(&root.join("shared.js"));

        let doc_source = doc_dir.join("foo.qmd");
        let resolved = expand_patterns(
            &root,
            &doc_dir,
            &[raw("/shared.js")],
            || ResourceOrigin::DocumentMetadata {
                source: doc_source.clone(),
            },
            ResourceScope::Page {
                source: doc_source.clone(),
            },
        )
        .unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].output_relative, "shared.js");
        assert_eq!(
            resolved[0].source,
            root.join("shared.js").canonicalize().unwrap()
        );
    }

    #[test]
    fn engine_report_absolute_path_keeps_filesystem_absolute_semantics() {
        // Regression guard: the leading-`/` normalization is YAML-only.
        // Engine and Lua-filter channels still pass real on-disk
        // absolute paths and must NOT be stripped/reinterpreted.
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let doc = root.join("posts/foo.qmd");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        let supporting = root.join("posts/foo_files/data.png");
        touch(&supporting);

        // `supporting` is filesystem-absolute (e.g. /tmp/.../posts/foo_files/data.png)
        // — the engine channel uses it as-is.
        let mut report = DocumentResourceReport::new();
        report.add_engine_files("stub", &doc, [supporting.clone()]);

        let resolved = resolve_reported_resources(&root, &report).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].source, supporting);
        assert_eq!(resolved[0].output_relative, "posts/foo_files/data.png");
    }

    // === Directory resources expand recursively ===
    //
    // TS Quarto parity (see external-sources/quarto-cli/src/core/path.ts:269-278):
    // a literal `resources:` entry that resolves to an existing
    // directory is equivalent to the recursive glob `dir/**/*`.
    // Trailing-slash and bare-directory forms are equivalent. See
    // bd-47w7o.

    fn touch_many(root: &Path, rel_paths: &[&str]) {
        for rel in rel_paths {
            touch(&root.join(rel));
        }
    }

    fn sorted_output_relatives(resolved: &[ResolvedResource]) -> Vec<String> {
        let mut v: Vec<String> = resolved.iter().map(|r| r.output_relative.clone()).collect();
        v.sort();
        v
    }

    #[test]
    fn expand_literal_directory_recursively_enumerates_files() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        touch_many(&root, &["demo/a.html", "demo/sub/b.css", "demo/sub/c.png"]);

        let resolved = expand_patterns(
            &root,
            &root,
            &[raw("demo")],
            || ResourceOrigin::ProjectMetadata,
            ResourceScope::Project,
        )
        .unwrap();
        assert_eq!(
            sorted_output_relatives(&resolved),
            vec![
                "demo/a.html".to_string(),
                "demo/sub/b.css".to_string(),
                "demo/sub/c.png".to_string(),
            ]
        );
    }

    #[test]
    fn expand_literal_directory_with_trailing_slash_works() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        touch_many(&root, &["demo/a.html", "demo/sub/b.css", "demo/sub/c.png"]);

        let resolved = expand_patterns(
            &root,
            &root,
            &[raw("demo/")],
            || ResourceOrigin::ProjectMetadata,
            ResourceScope::Project,
        )
        .unwrap();
        assert_eq!(
            sorted_output_relatives(&resolved),
            vec![
                "demo/a.html".to_string(),
                "demo/sub/b.css".to_string(),
                "demo/sub/c.png".to_string(),
            ]
        );
    }

    #[test]
    fn expand_leading_slash_directory_recursively_enumerates_files() {
        // Composes the bd-wlza2 leading-`/` rule with the bd-47w7o
        // directory-expansion rule. Mirrors the quarto-web case:
        // a project-level `"/demo"` entry resolves to a directory,
        // expanded to every file inside.
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        touch_many(&root, &["demo/a.html", "demo/sub/b.css"]);

        let resolved = expand_patterns(
            &root,
            &root,
            &[raw("/demo")],
            || ResourceOrigin::ProjectMetadata,
            ResourceScope::Project,
        )
        .unwrap();
        assert_eq!(
            sorted_output_relatives(&resolved),
            vec!["demo/a.html".to_string(), "demo/sub/b.css".to_string()]
        );
    }

    #[test]
    fn expand_literal_directory_with_only_empty_subdir_yields_no_resources() {
        // The directory itself and any nested subdirs must NOT appear
        // as resources; only files do. An empty `sub/` produces zero
        // entries.
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("demo/sub")).unwrap();

        let resolved = expand_patterns(
            &root,
            &root,
            &[raw("demo")],
            || ResourceOrigin::ProjectMetadata,
            ResourceScope::Project,
        )
        .unwrap();
        assert!(
            resolved.is_empty(),
            "expected no entries for a directory containing only an empty subdir; got: {:?}",
            sorted_output_relatives(&resolved)
        );
    }

    #[test]
    fn expand_literal_nonexistent_path_returns_single_entry() {
        // Regression: a missing file is NOT silently dropped. Today
        // we hand back the unresolved (lexically-normalized) path; the
        // downstream copy step then surfaces the
        // "does not exist on disk" error to the user.
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();

        let resolved = expand_patterns(
            &root,
            &root,
            &[raw("missing.txt")],
            || ResourceOrigin::ProjectMetadata,
            ResourceScope::Project,
        )
        .unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].output_relative, "missing.txt");
    }

    #[test]
    fn expand_literal_file_unchanged_after_directory_support() {
        // Regression for the file case under the same code path that
        // now also handles directories. Mirrors `expand_literal_path`
        // but is placed here so a directory-detect regression doesn't
        // silently break the file case.
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        touch(&root.join("a.txt"));

        let resolved = expand_patterns(
            &root,
            &root,
            &[raw("a.txt")],
            || ResourceOrigin::ProjectMetadata,
            ResourceScope::Project,
        )
        .unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].output_relative, "a.txt");
        assert_eq!(
            resolved[0].source,
            root.join("a.txt").canonicalize().unwrap()
        );
    }

    #[test]
    fn doc_anchor_resolves_relative_to_doc_dir() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let doc_dir = root.join("posts");
        std::fs::create_dir_all(&doc_dir).unwrap();
        touch(&doc_dir.join("data/extra.html"));

        let doc_source = doc_dir.join("foo.qmd");
        let resolved = expand_patterns(
            &root,
            &doc_dir,
            &[raw("data/extra.html")],
            || ResourceOrigin::DocumentMetadata {
                source: doc_source.clone(),
            },
            ResourceScope::Page {
                source: doc_source.clone(),
            },
        )
        .unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].output_relative, "posts/data/extra.html");
    }

    #[test]
    fn extract_patterns_scalar() {
        use quarto_pandoc_types::ConfigValue;
        let scalar = ConfigValue::from_path(&["resources"], "x.txt");
        assert_eq!(
            just_patterns(&extract_resource_patterns(&scalar, &["resources"])),
            vec!["x.txt".to_string()]
        );
    }

    #[test]
    fn extract_patterns_nested_path() {
        use quarto_pandoc_types::ConfigValue;
        let cv = ConfigValue::from_path(&["project", "resources"], "x.txt");
        assert_eq!(
            just_patterns(&extract_resource_patterns(&cv, &["project", "resources"])),
            vec!["x.txt".to_string()]
        );
        // missing
        assert!(extract_resource_patterns(&cv, &["project", "missing"]).is_empty());
    }

    // === bd-c1et2: source_info preservation ===

    fn synthetic_source_info(start: usize, end: usize) -> SourceInfo {
        SourceInfo::Original {
            file_id: quarto_source_map::FileId(42),
            start_offset: start,
            end_offset: end,
        }
    }

    #[test]
    fn extract_resource_patterns_preserves_source_info_per_array_item() {
        // Given a `resources:` array of two scalars with distinct
        // SourceInfo, extract_resource_patterns must keep each
        // scalar's source location alongside its pattern.
        use quarto_pandoc_types::ConfigValue;
        use quarto_pandoc_types::config_value::ConfigMapEntry;

        let si_first = synthetic_source_info(100, 110);
        let si_second = synthetic_source_info(200, 215);
        let array = ConfigValue::new_array(
            vec![
                ConfigValue::new_string("first.txt", si_first.clone()),
                ConfigValue::new_string("second.txt", si_second.clone()),
            ],
            SourceInfo::for_test(),
        );
        let outer = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "resources".into(),
                key_source: SourceInfo::for_test(),
                value: array,
            }],
            SourceInfo::for_test(),
        );

        let extracted = extract_resource_patterns(&outer, &["resources"]);
        assert_eq!(extracted.len(), 2);
        assert_eq!(extracted[0].pattern, "first.txt");
        assert_eq!(extracted[0].source_info, si_first);
        assert_eq!(extracted[1].pattern, "second.txt");
        assert_eq!(extracted[1].source_info, si_second);
    }

    #[test]
    fn extract_resource_patterns_preserves_source_info_for_scalar_shorthand() {
        // Single-scalar shorthand: `resources: x.txt` — the scalar's
        // own SourceInfo should be on the lone returned entry.
        use quarto_pandoc_types::ConfigValue;
        use quarto_pandoc_types::config_value::ConfigMapEntry;

        let si = synthetic_source_info(50, 56);
        let scalar = ConfigValue::new_string("x.txt", si.clone());
        let outer = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "resources".into(),
                key_source: SourceInfo::for_test(),
                value: scalar,
            }],
            SourceInfo::for_test(),
        );

        let extracted = extract_resource_patterns(&outer, &["resources"]);
        assert_eq!(extracted.len(), 1);
        assert_eq!(extracted[0].pattern, "x.txt");
        assert_eq!(extracted[0].source_info, si);
    }

    #[test]
    fn resource_error_codes_are_registered_in_catalog() {
        // Belt-and-braces: every code emitted by
        // `resource_error_to_parse_error` should be findable in the
        // shared error catalog. If a future change adds a new variant
        // and forgets to register a code, this test fails loudly.
        for code in ["Q-5-1", "Q-5-2", "Q-5-3"] {
            assert!(
                quarto_error_reporting::catalog::get_error_info(code).is_some(),
                "code {} is not registered in error_catalog.json",
                code
            );
            assert_eq!(
                quarto_error_reporting::catalog::get_subsystem(code),
                Some("project"),
                "code {} should be under the 'project' subsystem",
                code
            );
        }
    }

    #[test]
    fn out_of_project_error_renders_as_diagnostic_with_yaml_span() {
        // T3: end-to-end through the diagnostic helper. Given an
        // out-of-project pattern with a real YAML file on disk:
        // - the resulting ParseError carries the SourceInfo
        // - rendered text contains the title, the Q-RSC-1 code, the
        //   leading-`/` hint, *and* an Ariadne span showing the
        //   exact YAML line.
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let yaml_path = root.join("_quarto.yml");
        // Hand-crafted contents so we can compute the byte offsets
        // pointed to by the SourceInfo. The pattern lives on line 2,
        // and spans the quoted string excluding the dash/space prefix.
        let contents = "resources:\n  - \"../escape.csv\"\n";
        std::fs::write(&yaml_path, contents).unwrap();

        // Compute a SourceInfo for the pattern scalar. Hash the YAML
        // filename the same way `quarto_yaml::parse_file` does so the
        // diagnostic helper can look it up by FileId.
        let filename = yaml_path.to_string_lossy().to_string();
        let file_id = {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            filename.hash(&mut hasher);
            quarto_source_map::FileId(hasher.finish() as usize)
        };
        let pattern_start = contents.find("\"../escape.csv\"").unwrap();
        let pattern_end = pattern_start + "\"../escape.csv\"".len();
        let si = SourceInfo::Original {
            file_id,
            start_offset: pattern_start,
            end_offset: pattern_end,
        };

        let err = expand_patterns(
            &root,
            &root,
            &[RawResourcePattern::new("../escape.csv", si.clone())],
            || ResourceOrigin::ProjectMetadata,
            ResourceScope::Project,
        )
        .unwrap_err();

        let parse_err = resource_error_to_parse_error(err, &yaml_path);
        assert_eq!(parse_err.diagnostics.len(), 1);
        let d = &parse_err.diagnostics[0];
        assert_eq!(d.code.as_deref(), Some("Q-5-1"));
        assert!(
            d.title.as_str().contains("outside the project root"),
            "title was: {}",
            d.title.as_str()
        );
        assert_eq!(d.location.as_ref(), Some(&si));

        // Render to text WITHOUT hyperlinks so the snapshot is
        // path-independent. The Ariadne snippet should at least
        // include the pattern text and the leading-`/` info hint.
        let opts = quarto_error_reporting::TextRenderOptions {
            enable_hyperlinks: false,
        };
        let rendered = d.to_text_with_options(Some(&parse_err.source_context), &opts);
        assert!(
            rendered.contains("Q-5-1"),
            "rendered output missing code Q-5-1:\n{}",
            rendered
        );
        assert!(
            rendered.contains("../escape.csv"),
            "rendered output missing pattern:\n{}",
            rendered
        );
        assert!(
            rendered.contains("project-root-relative"),
            "rendered output missing leading-/ info hint:\n{}",
            rendered
        );
    }

    #[test]
    fn expand_patterns_out_of_project_error_carries_source_info() {
        // T2: when expand_patterns rejects a pattern that escapes the
        // project root, the resulting ResourceError carries the
        // SourceInfo we passed in via RawResourcePattern. The
        // orchestrator's diagnostic-rendering path reads this back.
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let si = synthetic_source_info(300, 314);

        let err = expand_patterns(
            &root,
            &root,
            &[RawResourcePattern::new("../escape.csv", si.clone())],
            || ResourceOrigin::ProjectMetadata,
            ResourceScope::Project,
        )
        .unwrap_err();

        match &err {
            ResourceError::OutOfProject {
                pattern,
                source_info,
                ..
            } => {
                assert_eq!(pattern, "../escape.csv");
                assert_eq!(source_info, &si);
            }
            other => panic!("expected OutOfProject, got {:?}", other),
        }
        // Accessor method also surfaces the same SourceInfo.
        assert_eq!(err.source_info(), &si);
        assert_eq!(err.pattern(), "../escape.csv");
    }

    // === DocumentResourceReport / resolve_reported_resources ===

    #[test]
    fn resolve_engine_report_absolute_paths() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let doc = root.join("posts/foo.qmd");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        let supporting = root.join("posts/foo_files/figure-html/cell-1.png");
        touch(&supporting);

        let mut report = DocumentResourceReport::new();
        report.add_engine_files("knitr", &doc, [supporting.clone()]);

        let resolved = resolve_reported_resources(&root, &report).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].source, supporting);
        assert_eq!(
            resolved[0].output_relative,
            "posts/foo_files/figure-html/cell-1.png"
        );
        assert!(matches!(
            resolved[0].origin,
            ResourceOrigin::Engine { ref engine, .. } if engine == "knitr"
        ));
    }

    #[test]
    fn resolve_engine_report_relative_paths_anchored_at_doc_dir() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let doc = root.join("posts/foo.qmd");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        let supporting = root.join("posts/extras/data.csv");
        touch(&supporting);

        let mut report = DocumentResourceReport::new();
        report.add_engine_files(
            "stub",
            &doc,
            [PathBuf::from("extras/data.csv")], // relative
        );

        let resolved = resolve_reported_resources(&root, &report).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].output_relative, "posts/extras/data.csv");
    }

    #[test]
    fn resolve_lua_filter_report_carries_origin() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let doc = root.join("a.qmd");
        let supporting = root.join("from-filter.txt");
        touch(&supporting);

        let mut report = DocumentResourceReport::new();
        report.add_lua_filter_files(&doc, [supporting.clone()]);

        let resolved = resolve_reported_resources(&root, &report).unwrap();
        assert_eq!(resolved.len(), 1);
        assert!(matches!(
            resolved[0].origin,
            ResourceOrigin::LuaFilter { ref source } if source == &doc
        ));
    }

    #[test]
    fn resolve_engine_report_out_of_project_errors() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let doc = root.join("a.qmd");

        let mut report = DocumentResourceReport::new();
        report.add_engine_files("stub", &doc, [PathBuf::from("../escape.csv")]);

        let err = resolve_reported_resources(&root, &report).unwrap_err();
        assert!(matches!(err, ResourceError::OutOfProject { .. }));
    }
}
