/*
 * project/discovery.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Multi-file project file-list expansion.
 */

//! Expand the set of source files a project renders.
//!
//! Phase-1 scope: `.qmd` only. Support for `.md`, `.ipynb`, and other
//! renderable extensions is deferred to a follow-up — see
//! `claude-notes/plans/2026-04-23-websites-phase-1.md` §"File-list
//! expansion".
//!
//! Discovery rules:
//!
//! 1. If `render_patterns` is non-empty, treat each entry as a glob
//!    relative to the project directory. Keep only matches with a
//!    `.qmd` extension.
//! 2. Otherwise, recursively walk the project directory for `.qmd`
//!    files.
//! 3. Exclude in either mode:
//!    - the output directory (e.g. `_site/`)
//!    - `.quarto/`, `.git/`, `node_modules/`
//!    - any path whose component starts with `_` (partials /
//!      `_metadata.yml` / `_quarto*.yml`)
//!    - any path whose component starts with `.` (hidden)
//!    - any file whose stem is `README` (case-insensitive)
//!
//! The project config file itself (`_quarto.yml` / `.yaml`) is
//! naturally excluded because of the `_` prefix rule.

use std::path::{Component, Path, PathBuf};

use quarto_source_map::{By, SourceInfo};
use quarto_system_runtime::SystemRuntime;

use crate::error::{QuartoError, Result};
use crate::glob::{BaseDirContext, GlobOptions, RawGlob, resolve_patterns};

/// Glob semantics for `project.render`.
///
/// `directory_rule: false` preserves the pre-bd-mt7a6uc4 behavior
/// where a bare directory entry matches nothing; Phase 2 of that
/// strand turns it on (decision D4) together with the diagnostic
/// that explains what a pattern matched.
const RENDER_GLOB_OPTIONS: GlobOptions = GlobOptions {
    directory_rule: false,
    default_positive: None,
};

/// Input describing what to discover.
#[derive(Debug, Clone)]
pub struct DiscoveryConfig<'a> {
    /// Project root, absolute and canonical.
    pub project_dir: &'a Path,
    /// Resolved project output directory (e.g. `project_dir/_site`).
    pub output_dir: &'a Path,
    /// `project.render` globs from `_quarto.yml`. Empty = walk the
    /// whole project directory.
    pub render_patterns: &'a [String],
}

/// Discover the list of `.qmd` files for a project.
///
/// Returned paths are **absolute** (joined with `project_dir`) and
/// de-duplicated. Order is stable: render-patterns preserve their
/// listed order (within a single pattern: lexicographic by path);
/// the walk path uses directory-first lexicographic order.
pub fn discover_project_files(
    config: &DiscoveryConfig<'_>,
    runtime: &dyn SystemRuntime,
) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let candidates: Vec<PathBuf> = if config.render_patterns.is_empty() {
        walk_qmd(config.project_dir, runtime)?
    } else {
        expand_patterns(config.project_dir, config.render_patterns, runtime)?
    };

    for candidate in candidates {
        if !is_renderable_qmd(&candidate, config) {
            continue;
        }
        if seen.insert(candidate.clone()) {
            files.push(candidate);
        }
    }

    Ok(files)
}

/// True if a candidate path should be rendered under Phase-1 rules.
fn is_renderable_qmd(candidate: &Path, config: &DiscoveryConfig<'_>) -> bool {
    if !has_qmd_extension(candidate) {
        return false;
    }
    let Ok(relative) = candidate.strip_prefix(config.project_dir) else {
        return false;
    };
    // Default projects emit output beside the source (no `_site/` dir),
    // so `output_dir == project_dir`. In that case the exclusion check
    // below would reject every candidate (every file starts with the
    // project root). Skip it when the output dir isn't a distinct
    // subdirectory.
    if config.output_dir != config.project_dir && starts_with(candidate, config.output_dir) {
        return false;
    }
    for component in relative.components() {
        let Component::Normal(os) = component else {
            // Reject `.` / `..` / root anchors — they can't occur in a
            // well-formed candidate but we don't want to guess.
            return false;
        };
        let s = match os.to_str() {
            Some(s) => s,
            None => return false,
        };
        if is_excluded_component(s) {
            return false;
        }
    }
    // README check (stem case-insensitive; applies to files only).
    if let Some(stem) = candidate.file_stem().and_then(|s| s.to_str())
        && stem.eq_ignore_ascii_case("README")
    {
        return false;
    }
    true
}

fn has_qmd_extension(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some("qmd")
}

fn is_excluded_component(name: &str) -> bool {
    if name.starts_with('_') || name.starts_with('.') {
        return true;
    }
    matches!(name, "node_modules")
}

fn starts_with(path: &Path, prefix: &Path) -> bool {
    let prefix = prefix.components().collect::<Vec<_>>();
    if prefix.is_empty() {
        return false;
    }
    let mut p = path.components();
    for want in prefix {
        match p.next() {
            Some(got) if got == want => {}
            _ => return false,
        }
    }
    true
}

/// Expand `project.render` glob patterns relative to `project_dir`.
///
/// Patterns are project-root-anchored (`project.render` can only be
/// written in `_quarto.yml`, whose directory *is* the project root)
/// and matched against the walked `.qmd` set with the shared matcher
/// in [`crate::glob`] — the same one listings use, so a pattern means
/// the same thing in both places.
fn expand_patterns(
    project_dir: &Path,
    patterns: &[String],
    runtime: &dyn SystemRuntime,
) -> Result<Vec<PathBuf>> {
    let walked = walk_qmd(project_dir, runtime)?;
    let mut matches = Vec::new();
    for pattern in patterns {
        // Resolve against the project root, then compile. A pattern
        // that escapes the root or fails to compile matches nothing;
        // bd-mt7a6uc4 Phase 2 gives both a diagnostic (D7).
        let resolution = resolve_patterns(
            [RawGlob::new(pattern.clone(), SourceInfo::generated(By::programmatic_config()))],
            &BaseDirContext {
                source_context: None,
                project_dir,
                fallback_dir: "",
            },
            &RENDER_GLOB_OPTIONS,
        );
        let Ok(compiled) = resolution.compile(&RENDER_GLOB_OPTIONS) else {
            continue;
        };
        for candidate in &walked {
            let Ok(relative) = candidate.strip_prefix(project_dir) else {
                continue;
            };
            if compiled.matches_path(relative) {
                matches.push(candidate.clone());
            }
        }
    }
    Ok(matches)
}

/// Recursively walk `project_dir` collecting `.qmd` files, already
/// filtered against the cheap excludes (hidden, underscore, output
/// dir). Paths are absolute.
fn walk_qmd(project_dir: &Path, runtime: &dyn SystemRuntime) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    walk_rec(project_dir, project_dir, runtime, &mut out)?;
    out.sort();
    Ok(out)
}

fn walk_rec(
    root: &Path,
    dir: &Path,
    runtime: &dyn SystemRuntime,
    out: &mut Vec<PathBuf>,
) -> Result<()> {
    let entries = runtime.dir_list(dir).map_err(|e| {
        QuartoError::Other(format!("Failed to list directory {}: {}", dir.display(), e))
    })?;
    for entry in entries {
        let name = entry
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        if is_excluded_component(name) {
            continue;
        }
        let is_dir = runtime.is_dir(&entry).unwrap_or(false);
        if is_dir {
            if entry.strip_prefix(root).is_ok() {
                walk_rec(root, &entry, runtime, out)?;
            }
            continue;
        }
        if has_qmd_extension(&entry) {
            out.push(entry);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use quarto_system_runtime::NativeRuntime;
    use std::fs;
    use tempfile::TempDir;

    fn native() -> NativeRuntime {
        NativeRuntime::new()
    }

    fn canonical(path: &Path) -> PathBuf {
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    }

    fn write_file(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn discovery_walks_directory() {
        let temp = TempDir::new().unwrap();
        let project_dir = canonical(temp.path());

        write_file(&project_dir.join("a.qmd"), "# a\n");
        write_file(&project_dir.join("sub/b.qmd"), "# b\n");
        write_file(&project_dir.join("_partial.qmd"), "# p\n");
        write_file(&project_dir.join(".hidden.qmd"), "# h\n");
        write_file(&project_dir.join("README.md"), "readme\n");
        write_file(&project_dir.join("README.qmd"), "readme\n");
        write_file(&project_dir.join("notes.md"), "notes\n");
        write_file(&project_dir.join("notebook.ipynb"), "{}\n");
        // Also ensure a _dir under the project is skipped.
        write_file(&project_dir.join("_drafts/in_progress.qmd"), "draft\n");

        let output_dir = project_dir.join("_site");
        let config = DiscoveryConfig {
            project_dir: &project_dir,
            output_dir: &output_dir,
            render_patterns: &[],
        };

        let files = discover_project_files(&config, &native()).unwrap();
        let rels: Vec<_> = files
            .iter()
            .map(|p| {
                p.strip_prefix(&project_dir)
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            })
            .collect();

        // Only `a.qmd` and `sub/b.qmd` survive. All other paths are
        // excluded: dot/underscore components, README names, non-.qmd.
        let mut expected = vec!["a.qmd".to_string(), "sub/b.qmd".to_string()];
        expected.sort();
        let mut got: Vec<String> = rels
            .into_iter()
            .map(|s| s.replace(std::path::MAIN_SEPARATOR, "/"))
            .collect();
        got.sort();
        assert_eq!(got, expected);
    }

    #[test]
    fn discovery_honors_render_patterns() {
        let temp = TempDir::new().unwrap();
        let project_dir = canonical(temp.path());
        write_file(&project_dir.join("index.qmd"), "# x\n");
        write_file(&project_dir.join("about.qmd"), "# a\n");
        write_file(&project_dir.join("docs/api.qmd"), "# api\n");
        write_file(&project_dir.join("docs/sub/nested.qmd"), "# n\n");

        let patterns = vec!["index.qmd".to_string(), "docs/**/*.qmd".to_string()];
        let output_dir = project_dir.join("_site");
        let config = DiscoveryConfig {
            project_dir: &project_dir,
            output_dir: &output_dir,
            render_patterns: &patterns,
        };
        let files = discover_project_files(&config, &native()).unwrap();
        let mut rels: Vec<String> = files
            .iter()
            .map(|p| {
                p.strip_prefix(&project_dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/")
            })
            .collect();
        rels.sort();

        assert_eq!(
            rels,
            vec![
                "docs/api.qmd".to_string(),
                "docs/sub/nested.qmd".to_string(),
                "index.qmd".to_string(),
            ]
        );
    }

    #[test]
    fn discovery_excludes_output_dir() {
        let temp = TempDir::new().unwrap();
        let project_dir = canonical(temp.path());
        write_file(&project_dir.join("index.qmd"), "# x\n");
        write_file(&project_dir.join("_site/stale.qmd"), "# stale\n");

        let output_dir = project_dir.join("_site");
        let config = DiscoveryConfig {
            project_dir: &project_dir,
            output_dir: &output_dir,
            render_patterns: &[],
        };
        let files = discover_project_files(&config, &native()).unwrap();
        let rels: Vec<_> = files
            .iter()
            .map(|p| {
                p.strip_prefix(&project_dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/")
            })
            .collect();
        assert_eq!(rels, vec!["index.qmd".to_string()]);
    }

    #[test]
    fn discovery_excludes_quarto_scratch_and_vcs() {
        let temp = TempDir::new().unwrap();
        let project_dir = canonical(temp.path());
        write_file(&project_dir.join("index.qmd"), "# x\n");
        write_file(&project_dir.join(".quarto/cache/.cached.qmd"), "# c\n");
        write_file(&project_dir.join(".git/hooks/pre-commit.qmd"), "# g\n");
        write_file(&project_dir.join("node_modules/lib/readme.qmd"), "# n\n");

        let output_dir = project_dir.join("_site");
        let config = DiscoveryConfig {
            project_dir: &project_dir,
            output_dir: &output_dir,
            render_patterns: &[],
        };
        let files = discover_project_files(&config, &native()).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("index.qmd"));
    }

    #[test]
    fn discovery_unicode_and_spaces() {
        let temp = TempDir::new().unwrap();
        let project_dir = canonical(temp.path());
        write_file(&project_dir.join("cómo estás.qmd"), "# hola\n");
        write_file(&project_dir.join("with spaces.qmd"), "# w\n");

        let output_dir = project_dir.join("_site");
        let config = DiscoveryConfig {
            project_dir: &project_dir,
            output_dir: &output_dir,
            render_patterns: &[],
        };
        let files = discover_project_files(&config, &native()).unwrap();
        let names: Vec<_> = files
            .iter()
            .filter_map(|p| p.file_name().and_then(|s| s.to_str()).map(str::to_string))
            .collect();
        assert!(names.contains(&"cómo estás.qmd".to_string()));
        assert!(names.contains(&"with spaces.qmd".to_string()));
    }

    #[test]
    fn discovery_default_project_walks_when_output_dir_equals_root() {
        // Regression for the post-websites-merge bug: a default project
        // has `output_dir == project_dir` (no `_site`), so the
        // output_dir-exclusion check used to reject every candidate.
        // After the fix, the walker should return the `.qmd` files just
        // like a website project does.
        let temp = TempDir::new().unwrap();
        let project_dir = canonical(temp.path());
        write_file(&project_dir.join("index.qmd"), "# x\n");
        write_file(&project_dir.join("about.qmd"), "# a\n");

        let config = DiscoveryConfig {
            project_dir: &project_dir,
            output_dir: &project_dir,
            render_patterns: &[],
        };
        let files = discover_project_files(&config, &native()).unwrap();
        let mut rels: Vec<String> = files
            .iter()
            .map(|p| {
                p.strip_prefix(&project_dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/")
            })
            .collect();
        rels.sort();
        assert_eq!(rels, vec!["about.qmd".to_string(), "index.qmd".to_string()],);
    }

    #[test]
    fn discovery_excludes_real_output_dir() {
        // Regression guard for the website case: when output_dir is a
        // distinct subdirectory, files inside it are excluded.
        let temp = TempDir::new().unwrap();
        let project_dir = canonical(temp.path());
        write_file(&project_dir.join("index.qmd"), "# x\n");
        write_file(&project_dir.join("_site/old.qmd"), "# stale\n");

        let output_dir = project_dir.join("_site");
        let config = DiscoveryConfig {
            project_dir: &project_dir,
            output_dir: &output_dir,
            render_patterns: &[],
        };
        let files = discover_project_files(&config, &native()).unwrap();
        let rels: Vec<String> = files
            .iter()
            .map(|p| {
                p.strip_prefix(&project_dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/")
            })
            .collect();
        assert_eq!(rels, vec!["index.qmd".to_string()]);
    }
}
