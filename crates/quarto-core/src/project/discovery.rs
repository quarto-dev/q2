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

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_system_runtime::SystemRuntime;

use crate::error::{QuartoError, Result};
use crate::glob::{
    BaseDirContext, GlobOptions, GlobResolution, PatternSet, RawGlob, resolve_patterns,
};
use crate::project::DocumentInfo;

/// Glob semantics for `project.render` — see
/// [`GlobOptions::RENDER`], where every consumer's option set lives
/// so they can be compared at a glance.
const RENDER_GLOB_OPTIONS: GlobOptions = GlobOptions::RENDER;

/// Input describing what to discover.
#[derive(Debug, Clone)]
pub struct DiscoveryConfig<'a> {
    /// Project root, absolute and canonical.
    pub project_dir: &'a Path,
    /// Resolved project output directory (e.g. `project_dir/_site`).
    pub output_dir: &'a Path,
    /// `project.render` globs from `_quarto.yml`, each with the
    /// provenance of the YAML scalar it came from so diagnostics can
    /// point at it. Empty = walk the whole project directory.
    pub render_patterns: &'a [RawGlob],
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
    patterns: &[RawGlob],
    runtime: &dyn SystemRuntime,
) -> Result<Vec<PathBuf>> {
    let walked = walk_qmd(project_dir, runtime)?;
    let resolution = resolve_render_patterns(project_dir, patterns);
    let Ok(compiled) = resolution.compile(&RENDER_GLOB_OPTIONS) else {
        // Unreachable: resolution only emits patterns it compiled.
        return Ok(Vec::new());
    };

    // Iterate the *positive* patterns in their listed order so the
    // documented render order survives (within one pattern, walk
    // order is lexicographic). Exclusions are global: a `!` entry
    // subtracts from every positive pattern regardless of where the
    // author wrote it.
    let mut matches = Vec::new();
    for glob in resolution.globs.iter().filter(|g| !g.negated) {
        let Ok(single) = PatternSet::compile(std::slice::from_ref(glob), &RENDER_GLOB_OPTIONS)
        else {
            continue;
        };
        for candidate in &walked {
            let Ok(relative) = candidate.strip_prefix(project_dir) else {
                continue;
            };
            if single.matches_path(relative) && !compiled.excluded_path(relative) {
                matches.push(candidate.clone());
            }
        }
    }
    Ok(matches)
}

/// Resolve `project.render` entries against the project root.
///
/// Shared by [`expand_patterns`] and
/// [`render_pattern_diagnostics`] so the two agree by construction:
/// what the diagnostic reports about a pattern is what discovery
/// actually did with it.
fn resolve_render_patterns(project_dir: &Path, patterns: &[RawGlob]) -> GlobResolution {
    resolve_patterns(
        patterns.iter().cloned(),
        &BaseDirContext {
            source_context: None,
            project_dir,
            fallback_dir: "",
        },
        &RENDER_GLOB_OPTIONS,
    )
}

/// Report `project.render` patterns that contributed nothing.
///
/// Three cases, each pointed at the YAML scalar the author wrote
/// (bd-mt7a6uc4 D7):
///
/// - `Q-5-14` — the pattern escapes the project root;
/// - `Q-5-15` — the glob engine rejects it;
/// - `Q-5-13` — it compiled but no **renderable** file matched.
///
/// "Renderable" is deliberate: `selected` is the post-exclusion file
/// list, so `render: ["README.qmd"]` reports honestly rather than
/// claiming a match discovery then dropped.
///
/// Negation entries are never reported — subtracting nothing is not
/// a mistake, and a `!` pattern that matches nothing is the normal
/// state of a defensive exclusion.
///
/// Pure: no filesystem access, so the orchestrator can call it after
/// the fact instead of threading a diagnostics channel through
/// discovery.
pub fn render_pattern_diagnostics(
    project_dir: &Path,
    patterns: &[RawGlob],
    selected: &[DocumentInfo],
) -> Vec<DiagnosticMessage> {
    if patterns.is_empty() {
        return Vec::new();
    }
    let resolution = resolve_render_patterns(project_dir, patterns);
    let mut out = Vec::new();

    for escaped in &resolution.escaped {
        out.push(
            DiagnosticMessageBuilder::warning(format!(
                "`project.render` pattern `{}` points outside the project directory",
                escaped.raw
            ))
            .with_code("Q-5-14")
            .with_location(escaped.source.clone())
            .problem(
                "The pattern's `..` segments climb above the project root, \
                 so it matches nothing.",
            )
            .add_info(
                "A project renders only files inside its own directory. \
                 Adjust the pattern so it stays within the project.",
            )
            .build(),
        );
    }

    for invalid in &resolution.invalid {
        out.push(
            DiagnosticMessageBuilder::warning(format!(
                "`project.render` pattern `{}` is not a valid glob",
                invalid.raw
            ))
            .with_code("Q-5-15")
            .with_location(invalid.source.clone())
            .problem(invalid.message.clone())
            .add_info(
                "`**` must be a whole path segment (`docs/**/*.qmd`), and \
                 `[...]` character classes must be closed.",
            )
            .build(),
        );
    }

    // Compiled-but-matched-nothing. Re-pair each resolved positive
    // pattern with the raw entry it came from, so the diagnostic can
    // quote what the author wrote and point at its span.
    let positives: Vec<_> = resolution.globs.iter().filter(|g| !g.negated).collect();
    let raw_positives: Vec<&RawGlob> = patterns
        .iter()
        .filter(|r| !r.raw.starts_with('!'))
        .filter(|r| {
            !resolution.escaped.iter().any(|e| e.raw == r.raw)
                && !resolution.invalid.iter().any(|i| i.raw == r.raw)
        })
        .collect();

    for (glob, raw) in positives.iter().zip(raw_positives.iter()) {
        let Ok(single) = PatternSet::compile(std::slice::from_ref(*glob), &RENDER_GLOB_OPTIONS)
        else {
            continue;
        };
        let matched = selected.iter().any(|doc| {
            doc.input
                .strip_prefix(project_dir)
                .is_ok_and(|rel| single.matches_path(rel))
        });
        if !matched {
            out.push(
                DiagnosticMessageBuilder::warning(format!(
                    "`project.render` pattern `{}` matched no renderable files",
                    raw.raw
                ))
                .with_code("Q-5-13")
                .with_location(raw.source.clone())
                .problem(
                    "No `.qmd` file in the project matches this pattern, so it \
                     contributes nothing to the render list.",
                )
                .add_info(
                    "In Quarto 2, `*` matches within one directory level — write \
                     `**/` to search subdirectories (`posts/**/*.qmd`). Files whose \
                     name or directory starts with `_` or `.`, and `README` files, \
                     are never rendered.",
                )
                .build(),
            );
        }
    }

    out
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

    use quarto_source_map::{By, SourceInfo};
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

    /// Helper: run discovery over `patterns`, returning the
    /// project-relative matches (sorted) plus any diagnostics.
    fn discover_with(
        project_dir: &Path,
        patterns: &[&str],
    ) -> (Vec<String>, Vec<quarto_error_reporting::DiagnosticMessage>) {
        let patterns: Vec<RawGlob> = patterns
            .iter()
            .map(|p| RawGlob::new(*p, SourceInfo::generated(By::programmatic_config())))
            .collect();
        let output_dir = project_dir.join("_site");
        let config = DiscoveryConfig {
            project_dir,
            output_dir: &output_dir,
            render_patterns: &patterns,
        };
        let files = discover_project_files(&config, &native()).unwrap();
        // Diagnostics are computed from the post-exclusion file list,
        // exactly as the orchestrator does it.
        let selected: Vec<DocumentInfo> =
            files.iter().cloned().map(DocumentInfo::from_path).collect();
        let diagnostics = render_pattern_diagnostics(project_dir, &patterns, &selected);
        let mut rels: Vec<String> = files
            .iter()
            .map(|p| {
                p.strip_prefix(project_dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/")
            })
            .collect();
        rels.sort();
        (rels, diagnostics)
    }

    fn render_fixture() -> (TempDir, PathBuf) {
        let temp = TempDir::new().unwrap();
        let project_dir = canonical(temp.path());
        write_file(&project_dir.join("index.qmd"), "# x\n");
        write_file(&project_dir.join("draft.qmd"), "# d\n");
        write_file(&project_dir.join("posts/a.qmd"), "# a\n");
        write_file(&project_dir.join("posts/b.qmd"), "# b\n");
        write_file(&project_dir.join("posts/deep/c.qmd"), "# c\n");
        (temp, project_dir)
    }

    /// D3: `!` excludes. Before bd-mt7a6uc4 the entry was matched
    /// literally, never matched anything, and the file rendered
    /// anyway with no diagnostic (Phase 0 fixture f2).
    #[test]
    fn render_patterns_honor_negation() {
        let (_t, dir) = render_fixture();
        let (rels, diags) = discover_with(&dir, &["*.qmd", "!draft.qmd"]);
        assert_eq!(rels, vec!["index.qmd".to_string()]);
        assert!(
            diags.is_empty(),
            "a negation that excludes something is not a problem: {diags:?}"
        );
    }

    #[test]
    fn render_patterns_negation_is_order_independent() {
        let (_t, dir) = render_fixture();
        let (rels, _) = discover_with(&dir, &["!draft.qmd", "*.qmd"]);
        assert_eq!(rels, vec!["index.qmd".to_string()]);
    }

    /// D4: a bare directory means everything beneath it. Before, it
    /// matched nothing and the render set was silently short
    /// (Phase 0 fixture f3).
    #[test]
    fn render_patterns_expand_a_bare_directory() {
        let (_t, dir) = render_fixture();
        let (rels, diags) = discover_with(&dir, &["index.qmd", "posts"]);
        assert_eq!(
            rels,
            vec![
                "index.qmd".to_string(),
                "posts/a.qmd".to_string(),
                "posts/b.qmd".to_string(),
                "posts/deep/c.qmd".to_string(),
            ]
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    /// D2: a leading `/` anchors at the project root (which, for
    /// `project.render`, is also its base — so this is a no-op that
    /// must not break).
    #[test]
    fn render_patterns_accept_a_leading_slash() {
        let (_t, dir) = render_fixture();
        let (rels, _) = discover_with(&dir, &["/index.qmd", "/posts/*.qmd"]);
        assert_eq!(
            rels,
            vec![
                "index.qmd".to_string(),
                "posts/a.qmd".to_string(),
                "posts/b.qmd".to_string(),
            ]
        );
    }

    /// D7: a pattern that matches nothing is reported. This is the
    /// migration aid for the D5 divergence from Q1 — a Q1 project
    /// whose `*.qmd` meant "everywhere" now gets told.
    #[test]
    fn render_pattern_matching_nothing_is_diagnosed() {
        let (_t, dir) = render_fixture();
        let (rels, diags) = discover_with(&dir, &["index.qmd", "postz/*.qmd"]);
        assert_eq!(rels, vec!["index.qmd".to_string()]);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code.as_deref(), Some("Q-5-13"));
        let text = format!("{:?}", diags[0]);
        assert!(
            text.contains("postz/*.qmd"),
            "should name the pattern: {text}"
        );
    }

    /// A pattern escaping the project root matches nothing and says so.
    #[test]
    fn render_pattern_escaping_the_project_is_diagnosed() {
        let (_t, dir) = render_fixture();
        let (rels, diags) = discover_with(&dir, &["../outside.qmd"]);
        assert!(rels.is_empty());
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code.as_deref(), Some("Q-5-14"));
    }

    /// An uncompilable pattern is reported rather than silently
    /// matching nothing.
    #[test]
    fn render_pattern_with_invalid_syntax_is_diagnosed() {
        let (_t, dir) = render_fixture();
        let (rels, diags) = discover_with(&dir, &["a**b.qmd"]);
        assert!(rels.is_empty());
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code.as_deref(), Some("Q-5-15"));
    }

    #[test]
    fn render_glob_diagnostic_codes_are_registered_in_catalog() {
        for code in ["Q-5-13", "Q-5-14", "Q-5-15"] {
            assert!(
                quarto_error_catalog::ERROR_CATALOG.get(code).is_some(),
                "{code} must be registered in the quarto-error-catalog"
            );
        }
    }

    /// Character classes reach `project.render` too (D1).
    #[test]
    fn render_patterns_support_character_classes() {
        let temp = TempDir::new().unwrap();
        let dir = canonical(temp.path());
        write_file(&dir.join("ch-1.qmd"), "# 1\n");
        write_file(&dir.join("ch-x.qmd"), "# x\n");
        let (rels, _) = discover_with(&dir, &["ch-[0-9].qmd"]);
        assert_eq!(rels, vec!["ch-1.qmd".to_string()]);
    }

    /// Discovery exclusions are the enumerator's policy, not glob
    /// semantics: an explicit pattern still cannot pull in an
    /// underscore/hidden/README path.
    #[test]
    fn render_patterns_do_not_defeat_discovery_exclusions() {
        let temp = TempDir::new().unwrap();
        let dir = canonical(temp.path());
        write_file(&dir.join("index.qmd"), "# x\n");
        write_file(&dir.join("_partial.qmd"), "# p\n");
        write_file(&dir.join("README.qmd"), "# r\n");
        let (rels, _) = discover_with(&dir, &["*.qmd"]);
        assert_eq!(rels, vec!["index.qmd".to_string()]);
    }

    #[test]
    fn discovery_honors_render_patterns() {
        let temp = TempDir::new().unwrap();
        let project_dir = canonical(temp.path());
        write_file(&project_dir.join("index.qmd"), "# x\n");
        write_file(&project_dir.join("about.qmd"), "# a\n");
        write_file(&project_dir.join("docs/api.qmd"), "# api\n");
        write_file(&project_dir.join("docs/sub/nested.qmd"), "# n\n");

        let patterns: Vec<RawGlob> = ["index.qmd", "docs/**/*.qmd"]
            .iter()
            .map(|p| RawGlob::new(*p, SourceInfo::generated(By::programmatic_config())))
            .collect();
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
