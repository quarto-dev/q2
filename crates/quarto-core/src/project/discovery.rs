/*
 * project/discovery.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Multi-file project file-list expansion.
 */

//! Expand the set of source files a project renders.
//!
//! Two gates, and they answer different questions.
//!
//! **Gate 1 — may this extension ever be an input?** `.qmd` and `.md`
//! always ([`FIXED_RENDERABLE`]), plus any extension an installed engine
//! statically claims (see [`RenderableExtensions`]). `.ipynb` / `.Rmd` are
//! not special-cased: they pass gate 1 exactly when an engine claims them —
//! see bd-xxul, bd-19nc56ao, and
//! `claude-notes/plans/2026-07-20-ipynb-surface-syntax-design.md`.
//!
//! **Gate 2 — is this file in the render list?** The `project.render`
//! patterns, whose default is `**/*.qmd` and nothing else.
//!
//! Passing gate 1 does not imply passing gate 2. An engine claim makes
//! `.echo` renderable *when listed*; it does not put `.echo` files into the
//! render list.
//!
//! Discovery rules (plans:
//! `claude-notes/plans/2026-08-07-md-render-support.md`; the
//! engine-claimed widening in D1 of
//! `claude-notes/plans/2026-08-13-ts-engine-extensions-merge-main.md` is
//! **superseded** — see [`effective_render_patterns`]):
//!
//! 1. Every candidate comes from one recursive walk of the project
//!    directory collecting files whose extension is in the resolved
//!    [`RenderableExtensions`] set.
//! 2. Candidates are selected by matching them against the
//!    `project.render` patterns. When the author wrote no *positive*
//!    pattern — no `render:` key at all, or only `!` negations —
//!    discovery supplies the default positive [`DEFAULT_RENDER_PATTERN`]
//!    (`**/*.qmd`). The invariant users can be told verbatim: **omitting
//!    `project.render` is exactly equivalent to writing
//!    `render: ["**/*.qmd"]`**. A negation-only list subtracts from that
//!    same set.
//!
//!    Quarto 2 auto-discovers `**/*.qmd` and nothing else. `.md`,
//!    `.ipynb`, percent/spin scripts and engine-contributed extensions all
//!    render only when an explicitly written pattern matches them. This is
//!    a deliberate divergence from Quarto 1, which walked the whole tree
//!    and asked each engine to claim what it found — content-sniffing
//!    every `.py` and `.R` on the way.
//! 3. Exclude in either mode:
//!    - the output directory (e.g. `_site/`)
//!    - `.quarto/`, `.git/`, `node_modules/`
//!    - any path whose component starts with `_` (partials /
//!      `_metadata.yml` / `_quarto*.yml`)
//!    - any path whose component starts with `.` (hidden)
//!    - any file whose stem is `README` (case-insensitive)
//!    - agent-instruction markdown (`CLAUDE.md`, `CLAUDE.local.md`,
//!      `AGENTS.md`, `AGENTS.local.md`, `*.llms.md`; case-insensitive)
//!
//! These exclusions are hard filters: an explicit pattern naming an
//! excluded file does not override them (the author gets `Q-5-13`
//! instead). The project config file itself (`_quarto.yml` / `.yaml`)
//! is naturally excluded because of the `_` prefix rule.

use std::path::{Component, Path, PathBuf};

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_source_map::{By, SourceInfo};
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

/// The always-renderable extensions, independent of engines. Undotted,
/// lowercase.
///
/// This must stay in sync with [`has_renderable_extension`]'s native arm:
/// `.md` is a member (bd-6d2wj4zp made `.md` a first-class input), so
/// dropping it here would silently remove `.md` from discovery.
pub const FIXED_RENDERABLE: &[&str] = &["qmd", "md"];

/// Extensions q2 owns natively and no engine may claim: the two markdown
/// kinds plus the extension-less case (treated as qmd). Used both by the
/// discovery widening below (a native member never becomes a synthetic
/// default pattern) and by the conversion stage's claim refusal (`Q-2-51`).
pub const NATIVE_EXTENSIONS: &[&str] = &["", "qmd", "md", "markdown"];

/// A resolved, normalized set of renderable file extensions (undotted
/// lowercase): [`FIXED_RENDERABLE`] union the engine claims-files
/// extensions. Discovery consults this instead of a hardcoded "qmd".
/// Never carries the engine registry — just strings.
///
/// This is **gate 1** only: it answers "may this extension ever be an
/// input", not "is this file discovered by default". Gate 2 — the render
/// patterns — is where that is decided, and its default is `**/*.qmd`
/// alone. An engine claim therefore makes `.echo` renderable *when listed*;
/// it does not put `.echo` files into the render list.
///
/// The set deliberately does NOT record which members came from an engine.
/// It used to, because the superseded D1 widened the default pattern set
/// per engine-claimed extension and needed to tell them apart. Nothing
/// widens now, so the distinction has no consumer.
#[derive(Debug, Clone)]
pub struct RenderableExtensions {
    all: std::collections::HashSet<String>,
}

impl RenderableExtensions {
    /// Build from [`FIXED_RENDERABLE`] unioned with `engine_exts`. Each
    /// declared extension must already be undotted lowercase (P4 guarantees
    /// this on the declared side) — `debug_assert`ed here; the newtype only
    /// normalizes *candidates* at comparison time (see `ext_in_set`), never
    /// declared members.
    pub fn new(engine_exts: impl IntoIterator<Item = String>) -> Self {
        let mut all: std::collections::HashSet<String> =
            FIXED_RENDERABLE.iter().map(|s| s.to_string()).collect();
        for ext in engine_exts {
            debug_assert!(
                !ext.starts_with('.') && ext == ext.to_ascii_lowercase(),
                "engine-declared extension must be undotted lowercase: {ext:?}"
            );
            all.insert(ext);
        }
        Self { all }
    }

    /// [`FIXED_RENDERABLE`] only — no engine extensions. Equivalent to
    /// `new(std::iter::empty())`.
    pub fn fixed() -> Self {
        Self::new(std::iter::empty())
    }

    /// Membership predicate. `candidate` is expected pre-normalized
    /// (undotted lowercase); see `ext_in_set` for the path-extraction +
    /// normalization wrapper callers should use instead of calling this
    /// directly.
    pub(crate) fn contains(&self, candidate: &str) -> bool {
        self.all.contains(candidate)
    }
}

/// Membership test for a candidate *path*: extracts the extension and
/// normalizes it (undotted lowercase) before consulting the set.
fn ext_in_set(path: &Path, set: &RenderableExtensions) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => set.contains(&ext.to_ascii_lowercase()),
        None => false,
    }
}

/// Input describing what to discover.
#[derive(Debug, Clone)]
pub struct DiscoveryConfig<'a> {
    /// Project root, absolute and canonical.
    pub project_dir: &'a Path,
    /// Resolved project output directory (e.g. `project_dir/_site`).
    pub output_dir: &'a Path,
    /// `project.render` globs from `_quarto.yml`, each with the
    /// provenance of the YAML scalar it came from so diagnostics can
    /// point at it. When no positive pattern is present (empty, or
    /// negations only), discovery matches against
    /// [`DEFAULT_RENDER_PATTERN`] plus one `**/*.<ext>` per
    /// engine-claimed extension instead.
    pub render_patterns: &'a [RawGlob],
    /// Resolved renderable-extension set: [`FIXED_RENDERABLE`] union engine
    /// claims-files extensions. Discovery stays a pure path/string module —
    /// this is never the engine registry.
    pub renderable_extensions: &'a RenderableExtensions,
}

/// The positive pattern discovery supplies when the author wrote
/// none: no `render:` key, or a list of only `!` negations. This is
/// what makes the S2′ invariant literal — omitting `project.render`
/// ≡ `render: ["**/*.qmd"]`.
pub const DEFAULT_RENDER_PATTERN: &str = "**/*.qmd";

/// Discover the render list for a project.
///
/// Returned paths are **absolute** (joined with `project_dir`) and
/// de-duplicated. Order is stable: patterns preserve their listed
/// order (within a single pattern: lexicographic by path); the
/// default pattern therefore yields lexicographic order.
pub fn discover_project_files(
    config: &DiscoveryConfig<'_>,
    runtime: &dyn SystemRuntime,
) -> Result<Vec<PathBuf>> {
    let walked = walk_sources(config.project_dir, runtime, config.renderable_extensions)?;
    Ok(select_from_walk(&walked, config))
}

/// `.md` files an author may have expected to render: they survive
/// every built-in exclusion but no render pattern selected them
/// (with no `render:` key, that is all renderable `.md`). Pure apart
/// from the walk; the orchestrator calls this only when explaining
/// an empty render set (`Q-PROJECT-EMPTY`), so discovery's own
/// signature stays lean.
pub fn unmatched_md_files(
    config: &DiscoveryConfig<'_>,
    runtime: &dyn SystemRuntime,
) -> Result<Vec<PathBuf>> {
    let walked = walk_sources(config.project_dir, runtime, config.renderable_extensions)?;
    let selected: std::collections::HashSet<PathBuf> =
        select_from_walk(&walked, config).into_iter().collect();
    Ok(walked
        .into_iter()
        .filter(|p| has_md_extension(p) && is_renderable_source(p, config))
        .filter(|p| !selected.contains(p))
        .collect())
}

/// The render patterns discovery actually matches against: the
/// author's, plus [`DEFAULT_RENDER_PATTERN`] prepended when no
/// positive pattern is present. Policy lives here in the enumerator
/// (not in `GlobOptions::RENDER.default_positive`) per
/// `claude-notes/designs/glob-semantics.md` — which files exist to be
/// matched is discovery's business, and this way a broken positive
/// pattern (`Q-5-14`/`Q-5-15`) never silently falls back to
/// rendering everything.
///
/// # The default render set is `**/*.qmd`, and only that
///
/// Omitting `project.render` is exactly equivalent to writing
/// `render: ["**/*.qmd"]`. Every other input type — `.md`, `.ipynb`,
/// percent/spin scripts, engine-contributed extensions — renders only
/// when a pattern the author wrote matches it.
///
/// **This supersedes D1** of the ts-engine-extensions merge runbook, which
/// widened the default set with one `**/*.<ext>` per engine-claimed
/// extension. Deciding whether a `.py` in the tree is a percent script, or
/// a `.echo` a document rather than a fixture, requires opening it — which
/// is what Quarto 1 did, and what lets installing an extension silently
/// change which files a project renders. One line of `render:` cannot
/// surprise anyone.
///
/// A **negation-only** list still subtracts from the default:
/// `render: ["!drafts/**"]` means every `.qmd` except those. Any
/// *positive* pattern replaces the default outright — including its
/// `**/*.qmd` half, which is why advice to add `render:` must always say
/// to keep `**/*.qmd` as well.
///
fn effective_render_patterns(user: &[RawGlob]) -> Vec<RawGlob> {
    if user.iter().any(|r| !r.raw.starts_with('!')) {
        return user.to_vec();
    }
    let mut patterns = vec![RawGlob::new(
        DEFAULT_RENDER_PATTERN,
        SourceInfo::generated(By::programmatic_config()),
    )];
    patterns.extend(user.iter().cloned());
    patterns
}

/// Match walked candidates against the effective render patterns,
/// applying the built-in exclusions. Iterates the *positive* patterns
/// in their listed order so the documented render order survives
/// (within one pattern: walk order). Exclusions are global: a `!`
/// entry subtracts from every positive pattern regardless of where
/// the author wrote it.
fn select_from_walk(walked: &[PathBuf], config: &DiscoveryConfig<'_>) -> Vec<PathBuf> {
    let patterns = effective_render_patterns(config.render_patterns);
    let resolution = resolve_render_patterns(config.project_dir, &patterns);
    let Ok(compiled) = resolution.compile(&RENDER_GLOB_OPTIONS) else {
        // Unreachable: resolution only emits patterns it compiled.
        return Vec::new();
    };

    let mut files = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for glob in resolution.globs.iter().filter(|g| !g.negated) {
        let Ok(single) = PatternSet::compile(std::slice::from_ref(glob), &RENDER_GLOB_OPTIONS)
        else {
            continue;
        };
        for candidate in walked {
            let Ok(relative) = candidate.strip_prefix(config.project_dir) else {
                continue;
            };
            if single.matches_path(relative)
                && !compiled.excluded_path(relative)
                && is_renderable_source(candidate, config)
                && seen.insert(candidate.clone())
            {
                files.push(candidate.clone());
            }
        }
    }
    files
}

/// True if a candidate path may appear in a render list at all:
/// renderable extension, and none of the built-in exclusions apply.
fn is_renderable_source(candidate: &Path, config: &DiscoveryConfig<'_>) -> bool {
    if !has_renderable_extension(candidate, config.renderable_extensions) {
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
    // Agent-instruction markdown never renders (D4) — under
    // `render: ["**/*.md"]` these would otherwise publish agent
    // instructions into the site. Case-insensitive, like README.
    if let Some(name) = candidate.file_name().and_then(|s| s.to_str())
        && is_agent_instruction_md(name)
    {
        return false;
    }
    true
}

/// File names on the agent-instruction ignore list (matched
/// case-insensitively): `CLAUDE.md`, `CLAUDE.local.md`, `AGENTS.md`,
/// `AGENTS.local.md`, and any `*.llms.md` companion file. Mirrors
/// Quarto 1's `projectHiddenIgnoreGlob`.
fn is_agent_instruction_md(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "claude.md" | "claude.local.md" | "agents.md" | "agents.local.md"
    ) || lower.ends_with(".llms.md")
}

/// True if `path` carries an extension discovery will consider.
///
/// The native arm (`qmd`/`md`) is written out explicitly *and* the resolved
/// set is consulted, so the predicate is correct even if a caller hands us a
/// set that somehow lacks a fixed member. [`FIXED_RENDERABLE`] contains both
/// native extensions, so the second arm alone would suffice today; keeping
/// the first is what guarantees `.md` can never silently drop out of
/// discovery and revert bd-6d2wj4zp.
fn has_renderable_extension(path: &Path, set: &RenderableExtensions) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("qmd" | "md")
    ) || ext_in_set(path, set)
}

fn has_md_extension(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some("md")
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

/// Resolve `project.render` entries against the project root.
///
/// Shared by [`select_from_walk`] and
/// [`render_pattern_diagnostics`] so the two agree by construction:
/// what the diagnostic reports about a pattern is what discovery
/// actually did with it. (The diagnostics side resolves the *user's*
/// patterns, not the effective ones — the synthesized
/// [`DEFAULT_RENDER_PATTERN`] has no YAML behind it and is never
/// reported.)
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
                    "No renderable source file in the project matches this pattern, \
                     so it contributes nothing to the render list. Renderable means \
                     `.qmd`, `.md`, or an extension claimed by an installed engine \
                     extension.",
                )
                .add_info(
                    "In Quarto 2, `*` matches within one directory level — write \
                     `**/` to search subdirectories (`posts/**/*.qmd`). Files whose \
                     name or directory starts with `_` or `.`, `README` files, and \
                     agent-instruction files (`CLAUDE.md`, `AGENTS.md`, `*.llms.md`) \
                     are never rendered.",
                )
                .build(),
            );
        }
    }

    out
}

/// Recursively walk `project_dir` collecting candidate source files
/// (`.qmd` and `.md`), already filtered against the cheap excludes
/// (hidden, underscore components). Paths are absolute and sorted.
fn walk_sources(
    project_dir: &Path,
    runtime: &dyn SystemRuntime,
    set: &RenderableExtensions,
) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    walk_rec(project_dir, project_dir, runtime, set, &mut out)?;
    out.sort();
    Ok(out)
}

fn walk_rec(
    root: &Path,
    dir: &Path,
    runtime: &dyn SystemRuntime,
    set: &RenderableExtensions,
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
                walk_rec(root, &entry, runtime, set, out)?;
            }
            continue;
        }
        if has_renderable_extension(&entry, set) {
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
        let exts = RenderableExtensions::fixed();
        let config = DiscoveryConfig {
            project_dir: &project_dir,
            output_dir: &output_dir,
            render_patterns: &[],
            renderable_extensions: &exts,
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
        let exts = RenderableExtensions::fixed();
        let config = DiscoveryConfig {
            project_dir,
            output_dir: &output_dir,
            render_patterns: &patterns,
            renderable_extensions: &exts,
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
        let exts = RenderableExtensions::fixed();
        let config = DiscoveryConfig {
            project_dir: &project_dir,
            output_dir: &output_dir,
            render_patterns: &patterns,
            renderable_extensions: &exts,
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
        let exts = RenderableExtensions::fixed();
        let config = DiscoveryConfig {
            project_dir: &project_dir,
            output_dir: &output_dir,
            render_patterns: &[],
            renderable_extensions: &exts,
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
        let exts = RenderableExtensions::fixed();
        let config = DiscoveryConfig {
            project_dir: &project_dir,
            output_dir: &output_dir,
            render_patterns: &[],
            renderable_extensions: &exts,
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
        let exts = RenderableExtensions::fixed();
        let config = DiscoveryConfig {
            project_dir: &project_dir,
            output_dir: &output_dir,
            render_patterns: &[],
            renderable_extensions: &exts,
        };
        let files = discover_project_files(&config, &native()).unwrap();
        let names: Vec<_> = files
            .iter()
            .filter_map(|p| p.file_name().and_then(|s| s.to_str()).map(str::to_string))
            .collect();
        assert!(names.contains(&"cómo estás.qmd".to_string()));
        assert!(names.contains(&"with spaces.qmd".to_string()));
    }

    // ── .md render support (bd-6d2wj4zp) ─────────────────────────
    //
    // Semantics under test (plan: 2026-08-07-md-render-support.md):
    // `.md` files render only when matched by an explicitly written
    // `project.render` pattern. Omitting `project.render` is exactly
    // equivalent to writing `render: ["**/*.qmd"]` (S2′), so `.md` is
    // invisible to default discovery; once matched, built-in
    // exclusions still apply, including the agent-file ignore list
    // (D4).

    /// Fixture with a representative mix of `.qmd`, opt-in `.md`, and
    /// `.md` files that must never render (D4 + built-in exclusions).
    fn md_fixture() -> (TempDir, PathBuf) {
        let temp = TempDir::new().unwrap();
        let dir = canonical(temp.path());
        write_file(&dir.join("index.qmd"), "# i\n");
        write_file(&dir.join("posts/a.qmd"), "# a\n");
        write_file(&dir.join("notes.md"), "# n\n");
        write_file(&dir.join("posts/note.md"), "# pn\n");
        write_file(&dir.join("news/NEWS.md"), "# news\n");
        (temp, dir)
    }

    /// `.md` files are invisible when `project.render` is absent.
    /// (Deliberate divergence from Quarto 1, which walks them in.)
    #[test]
    fn md_is_not_discovered_without_render_patterns() {
        let (_t, dir) = md_fixture();
        let (rels, _) = discover_with(&dir, &[]);
        assert_eq!(
            rels,
            vec!["index.qmd".to_string(), "posts/a.qmd".to_string()]
        );
    }

    /// S2′: omitting `project.render` ≡ `render: ["**/*.qmd"]` —
    /// same files, same order, including root-level files (`**`
    /// matches zero segments).
    #[test]
    fn default_discovery_equals_explicit_qmd_globstar() {
        let (_t, dir) = md_fixture();
        let ordered = |patterns: &[&str]| -> Vec<PathBuf> {
            let patterns: Vec<RawGlob> = patterns
                .iter()
                .map(|p| RawGlob::new(*p, SourceInfo::generated(By::programmatic_config())))
                .collect();
            let output_dir = dir.join("_site");
            let exts = RenderableExtensions::fixed();
            let config = DiscoveryConfig {
                project_dir: &dir,
                output_dir: &output_dir,
                render_patterns: &patterns,
                renderable_extensions: &exts,
            };
            discover_project_files(&config, &native()).unwrap()
        };
        let by_default = ordered(&[]);
        let by_pattern = ordered(&["**/*.qmd"]);
        assert!(!by_default.is_empty());
        assert_eq!(by_default, by_pattern, "same files in the same order");
    }

    /// D2(a): any positive pattern match opts a `.md` file in — the
    /// pattern does not need to name the extension.
    #[test]
    fn md_is_included_when_matched_by_render_patterns() {
        let (_t, dir) = md_fixture();

        // Extension glob, the Connect-docs shape.
        let (rels, diags) = discover_with(&dir, &["**/*.qmd", "**/*.md"]);
        assert_eq!(
            rels,
            vec![
                "index.qmd".to_string(),
                "news/NEWS.md".to_string(),
                "notes.md".to_string(),
                "posts/a.qmd".to_string(),
                "posts/note.md".to_string(),
            ]
        );
        assert!(diags.is_empty(), "{diags:?}");

        // Literal path.
        let (rels, _) = discover_with(&dir, &["notes.md"]);
        assert_eq!(rels, vec!["notes.md".to_string()]);

        // Bare directory: everything renderable beneath it, `.md`
        // included (the pattern is the explicit opt-in, not the
        // extension it happens to spell).
        let (rels, _) = discover_with(&dir, &["posts"]);
        assert_eq!(
            rels,
            vec!["posts/a.qmd".to_string(), "posts/note.md".to_string()]
        );
    }

    /// Render patterns replace the default entirely: an `.md`-only
    /// list renders no `.qmd`.
    #[test]
    fn md_only_pattern_does_not_include_qmd() {
        let (_t, dir) = md_fixture();
        let (rels, _) = discover_with(&dir, &["**/*.md"]);
        assert_eq!(
            rels,
            vec![
                "news/NEWS.md".to_string(),
                "notes.md".to_string(),
                "posts/note.md".to_string(),
            ]
        );
    }

    /// Negations subtract `.md` matches like any other (the
    /// Connect-docs `!news/NEWS.md` shape).
    #[test]
    fn negation_excludes_md_matches() {
        let (_t, dir) = md_fixture();
        let (rels, diags) = discover_with(&dir, &["**/*.md", "!news/NEWS.md"]);
        assert_eq!(
            rels,
            vec!["notes.md".to_string(), "posts/note.md".to_string()]
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    /// S2′: a negation-only render list means "the default
    /// `**/*.qmd`, minus these" — the documented semantics, which
    /// previously produced an empty render set.
    #[test]
    fn negation_only_render_list_subtracts_from_the_default() {
        let (_t, dir) = md_fixture();
        let (rels, diags) = discover_with(&dir, &["!posts/a.qmd"]);
        assert_eq!(
            rels,
            vec!["index.qmd".to_string()],
            "all default-discovered `.qmd` minus the negation, no `.md`"
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    /// Built-in exclusions (underscore/dot components, README,
    /// output dir) apply to `.md` exactly as to `.qmd`, even under a
    /// pattern that matches them.
    #[test]
    fn md_respects_builtin_exclusions() {
        let temp = TempDir::new().unwrap();
        let dir = canonical(temp.path());
        write_file(&dir.join("docs/guide.md"), "# g\n");
        write_file(&dir.join("_partials/frag.md"), "# f\n");
        write_file(&dir.join("_draft.md"), "# d\n");
        write_file(&dir.join(".hidden.md"), "# h\n");
        write_file(&dir.join("README.md"), "# r\n");
        write_file(&dir.join("readme.md"), "# r2\n");
        write_file(&dir.join("_site/stale.md"), "# s\n");
        let (rels, _) = discover_with(&dir, &["**/*.md"]);
        assert_eq!(rels, vec!["docs/guide.md".to_string()]);
    }

    /// D4: agent-instruction files never render, even when a pattern
    /// matches them. Case-insensitive, like the README rule.
    #[test]
    fn agent_instruction_md_files_never_render() {
        let temp = TempDir::new().unwrap();
        let dir = canonical(temp.path());
        write_file(&dir.join("notes.md"), "# n\n");
        write_file(&dir.join("CLAUDE.md"), "# c\n");
        write_file(&dir.join("CLAUDE.local.md"), "# cl\n");
        write_file(&dir.join("AGENTS.md"), "# a\n");
        write_file(&dir.join("agents.local.md"), "# al\n");
        write_file(&dir.join("docs/AGENTS.md"), "# da\n");
        write_file(&dir.join("api.llms.md"), "# llms\n");
        let (rels, _) = discover_with(&dir, &["**/*.md"]);
        assert_eq!(rels, vec!["notes.md".to_string()]);
    }

    /// D4′: built-in exclusions are hard filters — a literal render
    /// entry naming an excluded file does not override them, and the
    /// author is told the pattern contributed nothing (Q-5-13).
    #[test]
    fn literal_pattern_naming_an_excluded_file_is_diagnosed() {
        let temp = TempDir::new().unwrap();
        let dir = canonical(temp.path());
        write_file(&dir.join("CLAUDE.md"), "# c\n");
        write_file(&dir.join("index.qmd"), "# i\n");
        let (rels, diags) = discover_with(&dir, &["index.qmd", "CLAUDE.md"]);
        assert_eq!(rels, vec!["index.qmd".to_string()]);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code.as_deref(), Some("Q-5-13"));
    }

    /// Helper: project-relative `unmatched_md_files` results, sorted.
    fn unmatched_with(project_dir: &Path, patterns: &[&str]) -> Vec<String> {
        let patterns: Vec<RawGlob> = patterns
            .iter()
            .map(|p| RawGlob::new(*p, SourceInfo::generated(By::programmatic_config())))
            .collect();
        let output_dir = project_dir.join("_site");
        let exts = RenderableExtensions::fixed();
        let config = DiscoveryConfig {
            project_dir,
            output_dir: &output_dir,
            render_patterns: &patterns,
            renderable_extensions: &exts,
        };
        let mut rels: Vec<String> = unmatched_md_files(&config, &native())
            .unwrap()
            .iter()
            .map(|p| {
                p.strip_prefix(project_dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/")
            })
            .collect();
        rels.sort();
        rels
    }

    /// With no `render:` key, every renderable `.md` is an opt-in
    /// candidate the author may be missing — but files the built-in
    /// exclusions reject are not (they could never render anyway).
    #[test]
    fn unmatched_md_reports_optin_candidates() {
        let (_t, dir) = md_fixture();
        assert_eq!(
            unmatched_with(&dir, &[]),
            vec![
                "news/NEWS.md".to_string(),
                "notes.md".to_string(),
                "posts/note.md".to_string(),
            ]
        );
    }

    /// Excluded `.md` (README, agent files, underscore) never counts
    /// as unmatched: suggesting the user opt in a file that cannot
    /// render would be a lie.
    #[test]
    fn unmatched_md_skips_builtin_exclusions() {
        let temp = TempDir::new().unwrap();
        let dir = canonical(temp.path());
        write_file(&dir.join("guide.md"), "# g\n");
        write_file(&dir.join("README.md"), "# r\n");
        write_file(&dir.join("CLAUDE.md"), "# c\n");
        write_file(&dir.join("_frag.md"), "# f\n");
        assert_eq!(unmatched_with(&dir, &[]), vec!["guide.md".to_string()]);
    }

    /// Once patterns select the `.md` files, nothing is unmatched;
    /// partially-selecting patterns leave the remainder.
    #[test]
    fn unmatched_md_shrinks_as_patterns_match() {
        let (_t, dir) = md_fixture();
        assert!(unmatched_with(&dir, &["**/*.qmd", "**/*.md"]).is_empty());
        assert_eq!(
            unmatched_with(&dir, &["posts/*.md"]),
            vec!["news/NEWS.md".to_string(), "notes.md".to_string()]
        );
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

        let exts = RenderableExtensions::fixed();
        let config = DiscoveryConfig {
            project_dir: &project_dir,
            output_dir: &project_dir,
            render_patterns: &[],
            renderable_extensions: &exts,
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
        let exts = RenderableExtensions::fixed();
        let config = DiscoveryConfig {
            project_dir: &project_dir,
            output_dir: &output_dir,
            render_patterns: &[],
            renderable_extensions: &exts,
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

    // ── Engine-claimed extensions: T6 / T6b / T7 re-port + D1 / D2 ──────────
    //
    // Re-ported from the branch's pre-merge discovery tests against main's
    // rewritten module (walk_sources / select_from_walk / is_renderable_source
    // replaced walk_qmd / is_renderable_qmd), plus the two tests D1 and D2
    // require. Without these the engine-claimed path has NO coverage on
    // either side of the merge.

    /// Project-relative, forward-slashed, sorted.
    fn rels_of(files: &[PathBuf], project_dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = files
            .iter()
            .map(|p| {
                p.strip_prefix(project_dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/")
            })
            .collect();
        v.sort();
        v
    }

    /// T6: an engine-claimed extension obeys the SAME exclusion rules that
    /// already govern `.qmd` (underscore prefix, dot prefix, output dir), and
    /// a non-member extension (`.ipynb`) stays excluded.
    ///
    /// This binds that engine extensions flow through the same predicate, not
    /// a bypass branch. It needs an explicit `render:` pattern now: engine
    /// claims no longer widen the default set, so with no patterns there would
    /// be nothing to apply exclusions to.
    #[test]
    fn t6_engine_extension_obeys_the_same_exclusions() {
        let temp = TempDir::new().unwrap();
        let project_dir = canonical(temp.path());

        write_file(&project_dir.join("a.echo"), "content\n");
        write_file(&project_dir.join("_draft.echo"), "draft\n");
        write_file(&project_dir.join(".hidden.echo"), "hidden\n");
        write_file(&project_dir.join("out/b.echo"), "stale\n");
        write_file(&project_dir.join("notebook.ipynb"), "{}\n");

        // `out/` doubles as the output dir, so this exercises the output-dir
        // exclusion in the same run as the underscore/dot exclusions and the
        // non-member-extension exclusion.
        let output_dir = project_dir.join("out");
        let exts = RenderableExtensions::new(vec!["echo".to_string()]);
        let patterns = vec![RawGlob::new(
            "**/*.echo",
            SourceInfo::generated(By::programmatic_config()),
        )];
        let config = DiscoveryConfig {
            project_dir: &project_dir,
            output_dir: &output_dir,
            render_patterns: &patterns,
            renderable_extensions: &exts,
        };

        let files = discover_project_files(&config, &native()).unwrap();
        assert_eq!(
            rels_of(&files, &project_dir),
            vec!["a.echo".to_string()],
            "only a.echo survives: _draft.echo / .hidden.echo / out/b.echo / \
             notebook.ipynb must all stay excluded"
        );
    }

    /// T6b (pattern path): same fixture, but an explicit `render: ["*.echo"]`
    /// routes through the pattern branch rather than the bare default.
    #[test]
    fn t6b_engine_extension_admitted_via_explicit_pattern() {
        let temp = TempDir::new().unwrap();
        let project_dir = canonical(temp.path());
        write_file(&project_dir.join("a.echo"), "content\n");
        write_file(&project_dir.join("notebook.ipynb"), "{}\n");

        let output_dir = project_dir.join("_site");
        let exts = RenderableExtensions::new(vec!["echo".to_string()]);
        let patterns = vec![RawGlob::new(
            "*.echo",
            SourceInfo::generated(By::programmatic_config()),
        )];
        let config = DiscoveryConfig {
            project_dir: &project_dir,
            output_dir: &output_dir,
            render_patterns: &patterns,
            renderable_extensions: &exts,
        };

        let files = discover_project_files(&config, &native()).unwrap();
        assert_eq!(
            rels_of(&files, &project_dir),
            vec!["a.echo".to_string()],
            "an explicit *.echo pattern selects the claimed file; .ipynb is not \
             a member and stays out"
        );
    }

    /// T7: a non-member extension is excluded even when a pattern names it
    /// explicitly. Membership is the gate; a pattern cannot widen it.
    #[test]
    fn t7_non_member_extension_excluded_even_when_pattern_names_it() {
        let temp = TempDir::new().unwrap();
        let project_dir = canonical(temp.path());
        write_file(&project_dir.join("notebook.ipynb"), "{}\n");
        write_file(&project_dir.join("a.echo"), "content\n");

        let output_dir = project_dir.join("_site");
        let exts = RenderableExtensions::new(vec!["echo".to_string()]);
        let patterns = vec![RawGlob::new(
            "*.ipynb",
            SourceInfo::generated(By::programmatic_config()),
        )];
        let config = DiscoveryConfig {
            project_dir: &project_dir,
            output_dir: &output_dir,
            render_patterns: &patterns,
            renderable_extensions: &exts,
        };

        let files = discover_project_files(&config, &native()).unwrap();
        assert!(
            files.is_empty(),
            "`.ipynb` is claimed by no engine, so naming it in `render:` must \
             not make it an input; got {:?}",
            rels_of(&files, &project_dir)
        );
    }

    /// Quarto 2 auto-discovers `**/*.qmd` and nothing else.
    ///
    /// Supersedes D1 of the ts-engine-extensions merge runbook, which had
    /// engine-claimed extensions widen the default pattern set. One rule now
    /// covers every non-`.qmd` input — `.md`, `.ipynb`, percent/spin scripts,
    /// and engine-contributed extensions alike: it renders only when a
    /// `project.render` pattern matches it.
    ///
    /// The rule is deliberately blunt. Deciding whether a `.py` in the tree is
    /// a percent script, or a `.echo` is a document rather than a fixture,
    /// requires opening it — which is what Quarto 1 did, and what makes
    /// installing an extension able to change silently which files a project
    /// renders. Listing them is one line and cannot surprise anyone.
    ///
    /// Named revert: restore the widening loop in `effective_render_patterns`
    /// and this goes RED.
    #[test]
    fn claimed_extension_is_not_auto_discovered() {
        let temp = TempDir::new().unwrap();
        let project_dir = canonical(temp.path());
        write_file(&project_dir.join("a.echo"), "content\n");
        write_file(&project_dir.join("index.qmd"), "# x\n");
        write_file(&project_dir.join("notes.md"), "notes\n");
        write_file(&project_dir.join("notebook.ipynb"), "{}\n");

        let output_dir = project_dir.join("_site");
        let exts = RenderableExtensions::new(vec!["echo".to_string()]);
        let config = DiscoveryConfig {
            project_dir: &project_dir,
            output_dir: &output_dir,
            render_patterns: &[], // ← no `project.render` key at all
            renderable_extensions: &exts,
        };

        let files = discover_project_files(&config, &native()).unwrap();
        let rels = rels_of(&files, &project_dir);

        assert_eq!(
            rels,
            vec!["index.qmd".to_string()],
            "with no `render:` key ONLY `**/*.qmd` is discovered — an \
             engine-claimed `.echo`, an opt-in `.md`, and an `.ipynb` all \
             stay out; got {rels:?}"
        );
    }

    /// The default pattern set is exactly `**/*.qmd`, whatever the engines
    /// claim. Unit-level companion to the test above.
    #[test]
    fn default_pattern_set_is_qmd_only_regardless_of_claims() {
        // `effective_render_patterns` no longer takes the extension set at
        // all — reverting D1 reverted the signature change D1 required. That
        // the claims cannot reach this function is the guarantee; building a
        // set here would only be able to test that an unused argument is
        // unused.
        let effective = effective_render_patterns(&[]);
        let raws: Vec<&str> = effective.iter().map(|g| g.raw.as_str()).collect();
        assert_eq!(
            raws,
            vec![DEFAULT_RENDER_PATTERN],
            "no claimed extension contributes a synthetic default glob"
        );
    }

    /// A user-written positive pattern is authoritative and replaces the
    /// default outright — including the `**/*.qmd` half. This is why any
    /// advice to add `render:` has to say "keep `**/*.qmd` too".
    #[test]
    fn a_positive_user_pattern_replaces_the_default_entirely() {
        let user = vec![RawGlob::new(
            "docs/**/*.qmd",
            SourceInfo::generated(By::programmatic_config()),
        )];
        let effective = effective_render_patterns(&user);
        let raws: Vec<&str> = effective.iter().map(|g| g.raw.as_str()).collect();
        assert_eq!(
            raws,
            vec!["docs/**/*.qmd"],
            "an explicit positive pattern is authoritative"
        );
    }

    /// **D2** — a dynamic claimer (`claims_files: None`) contributes no
    /// discovery wildcard, and emits no warning.
    ///
    /// Discovery only ever sees paths, so an engine that decides by
    /// inspecting content cannot contribute a static extension. It falls
    /// through silently, mirroring the language-claim precedent in
    /// `engine::resolution` (Pass-1 returns `None`, observability is a trace
    /// target, not a diagnostic). Do not invent a warning for this.
    #[test]
    fn d2_dynamic_claimer_contributes_no_wildcard_and_no_warning() {
        use crate::extension::types::{EngineContribution, claimed_file_extensions};

        let dynamic = EngineContribution::External {
            path: PathBuf::from("/ext/dyn-engine.js"),
            extension_yml_path: PathBuf::from("/ext/_extension.yml"),
            name: Some("dyn-engine".to_string()),
            claims: None,
            file_extensions: None,
            claims_files: None, // ← decides by inspecting content
        };

        assert!(
            claimed_file_extensions(&dynamic).is_empty(),
            "a dynamic claimer declares no static extension"
        );

        // Therefore it contributes no renderable member, and the default
        // pattern set is unchanged.
        let effective = effective_render_patterns(&[]);
        let raws: Vec<&str> = effective.iter().map(|g| g.raw.as_str()).collect();
        assert_eq!(
            raws,
            vec![DEFAULT_RENDER_PATTERN],
            "a dynamic claimer must not widen the default pattern set"
        );

        // And the fall-through is SILENT: discovery emits diagnostics only via
        // `render_pattern_diagnostics`, which is called with the *user's*
        // patterns. An empty user list yields nothing to warn about.
        let temp = TempDir::new().unwrap();
        let project_dir = canonical(temp.path());
        write_file(&project_dir.join("index.qmd"), "# x\n");
        let diags = render_pattern_diagnostics(&project_dir, &[], &[]);
        assert!(
            diags.is_empty(),
            "the dynamic-claimer fall-through must be silent; got {diags:?}"
        );
    }
}
