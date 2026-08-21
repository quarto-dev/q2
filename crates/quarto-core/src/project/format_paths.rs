/*
 * project/format_paths.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Merge-time declaration-site marking of path-shaped format keys
 * (`css`, `theme`, the `include-*` slots) and the Q-5-29 css
 * missing-file diagnostic. bd-format-css-not-copied-crn3bjdz
 * (css), bd-oejuizi9 / GH #455 (theme + include slots).
 */

//! Merge-time marking of user-declared path-shaped format values.
//!
//! A path in a format key is authored relative to the file that
//! declares it: `_quarto.yml` entries are project-root-relative,
//! `_metadata.yml` entries are relative to that file's directory, and
//! document front-matter entries are relative to the document
//! (contract: `claude-notes/designs/path-resolution-model.md`, rule 1).
//! After the metadata merge flattens these layers, that provenance is
//! gone — so each layer's entries are normalized *during* the merge,
//! while the layer's base directory is still known
//! ([`mark_format_path_values`]). A leading `/` anchors at the
//! project root regardless of the declaring layer (rule 2).
//!
//! Marked values become [`ConfigValueKind::Path`] holding the
//! equivalent **document-relative** path, so downstream consumers
//! that join the consuming document's directory
//! ([`IncludeResolveStage`](crate::stage::stages::IncludeResolveStage)'s
//! `doc_dir.join`, `quarto-sass`'s `ThemeContext::resolve_path`, the
//! [`FormatCssTransform`](crate::transforms::FormatCssTransform))
//! read them correctly with no change of their own.
//!
//! The key table ([`FORMAT_PATH_KEYS`]) is the in-tree seed of the
//! contract's unified path-shaped-key registry. Per-key marking
//! policy:
//!
//! - `css` — existence-driven, mirroring the extension-fragment
//!   machinery (`FRAGMENT_PATH_PATTERNS`) and Q1's
//!   `toInputRelativePaths`; entries that name no file pass through
//!   untouched and are diagnosed **at the declaration site** (Q-5-29),
//!   so a project-wide mistake warns once per declaring layer instead
//!   of once per rendered page: project config via
//!   [`missing_project_css_diagnostics`] (called once per project
//!   render by the orchestrator); directory metadata and front matter
//!   via the diagnostics returned by [`mark_format_path_values`],
//!   pushed into the declaring document's own render diagnostics.
//! - `theme` — existence-driven and **silent**: a non-file string is
//!   normally a built-in theme name (`cosmo`), not a mistake. A
//!   typo'd custom path stays Scalar and fails at theme load exactly
//!   as before.
//! - `include-in-header` / `include-before-body` / `include-after-body`
//!   — **unconditional**: these strings are always file paths, so
//!   even a missing file is rebased, and the resolve-time Q-5-4 then
//!   reports the declaration-resolved location instead of a bogus
//!   doc-dir join. Smart-include maps have only their `file:` value
//!   marked; `text:` is literal content, never a path.

use std::path::{Path, PathBuf};

use quarto_config::resolve_format_config;
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::config_value::ConfigValueKind;
use quarto_system_runtime::SystemRuntime;

use crate::project::ProjectContext;

/// How a key's entries are marked when the declared string does not
/// name an existing file (see the module docs for the rationale).
#[derive(Clone, Copy, PartialEq)]
enum MarkPolicy {
    /// Mark only existing files; return a Q-5-29 per miss (`css`).
    ExistenceDiagnose,
    /// Mark only existing files; silent otherwise (`theme` — built-in
    /// names are the common case).
    ExistenceSilent,
    /// Mark every string entry (`include-*` — always paths).
    Always,
}

/// The string-bearing shapes a key's value can take.
#[derive(Clone, Copy)]
enum KeyForms {
    /// Scalar, or an array of scalars (`css`).
    Entries,
    /// Scalar, array, or a `{light:, dark:}`-style map whose values
    /// are scalars/arrays (`theme`).
    Theme,
    /// Scalar or array; array items may be smart-include maps, whose
    /// `file:` value (only) is a path (`include-*`).
    Include,
}

/// The path-shaped format keys marked at each metadata-merge layer —
/// the in-tree seed of the contract's unified path-shaped-key
/// registry (`claude-notes/designs/path-resolution-model.md`).
/// Residual same-class keys (`template`, `template-partials`,
/// `filters`, …) are tracked in bd-hjv5o; adding one is one row here
/// plus its form handling.
const FORMAT_PATH_KEYS: &[(&str, MarkPolicy, KeyForms)] = &[
    ("css", MarkPolicy::ExistenceDiagnose, KeyForms::Entries),
    ("theme", MarkPolicy::ExistenceSilent, KeyForms::Theme),
    ("include-in-header", MarkPolicy::Always, KeyForms::Include),
    ("include-before-body", MarkPolicy::Always, KeyForms::Include),
    ("include-after-body", MarkPolicy::Always, KeyForms::Include),
];

/// Apply `f` to a value's string-bearing leaves: the scalar forms
/// directly, or each item of the array form.
fn for_each_entry_mut(css: &mut ConfigValue, mut f: impl FnMut(&mut ConfigValue)) {
    match &mut css.value {
        ConfigValueKind::Array(items) => {
            for item in items {
                f(item);
            }
        }
        _ => f(css),
    }
}

/// Immutable counterpart of [`for_each_entry_mut`].
fn for_each_entry(css: &ConfigValue, mut f: impl FnMut(&ConfigValue)) {
    match &css.value {
        ConfigValueKind::Array(items) => {
            for item in items {
                f(item);
            }
        }
        _ => f(css),
    }
}

/// Resolve a declared css string against its layer.
///
/// A leading `/` means site-root-relative (Decision 4 of
/// bd-root-relative-paths-design-fc5pvkcv), so it anchors at the
/// project root regardless of which layer declared it; anything else
/// anchors at the declaring layer's own base directory.
fn candidate_path(declared: &str, layer_base: &Path, project_dir: &Path) -> PathBuf {
    match declared.strip_prefix('/') {
        Some(rest) => project_dir.join(rest),
        None => layer_base.join(declared),
    }
}

/// The per-layer directories and runtime a marking pass resolves
/// against.
struct MarkCtx<'a> {
    layer_base: &'a Path,
    project_dir: &'a Path,
    document_dir: &'a Path,
    runtime: &'a dyn SystemRuntime,
}

/// Mark path-shaped format values in one metadata layer as
/// document-relative `Path` values ([`FORMAT_PATH_KEYS`]).
///
/// `layer_base` is the directory the layer's relative paths are
/// authored against (project root for `_quarto.yml`, the metadata
/// file's directory for `_metadata.yml`, the document's directory for
/// front matter). Entries that are already `Path`-kind (explicit
/// `!path`, extension fragments) are left alone — the merge machinery
/// already rebases those.
///
/// Returns a Q-5-29 warning for each `css` entry that names no file
/// (the other keys never diagnose here; see the module docs). The
/// caller decides whether to surface them: per-document layers push
/// them into the document's diagnostics; the project layer drops them
/// because [`missing_project_css_diagnostics`] reports the same
/// entries once per project render instead of once per page.
pub(crate) fn mark_format_path_values(
    metadata: &mut ConfigValue,
    layer_base: &Path,
    project_dir: &Path,
    document_dir: &Path,
    runtime: &dyn SystemRuntime,
) -> Vec<DiagnosticMessage> {
    let mut diagnostics = Vec::new();
    let ctx = MarkCtx {
        layer_base,
        project_dir,
        document_dir,
        runtime,
    };
    for (key, policy, forms) in FORMAT_PATH_KEYS {
        let Some(value) = metadata.get_mut(key) else {
            continue;
        };
        match forms {
            KeyForms::Entries => for_each_entry_mut(value, |entry| {
                mark_entry(entry, *policy, &ctx, &mut diagnostics);
            }),
            KeyForms::Theme => match &mut value.value {
                // `{light:, dark:}`-style map: each variant is itself
                // a scalar or array of theme entries.
                ConfigValueKind::Map(entries) => {
                    for map_entry in entries {
                        for_each_entry_mut(&mut map_entry.value, |entry| {
                            mark_entry(entry, *policy, &ctx, &mut diagnostics);
                        });
                    }
                }
                _ => for_each_entry_mut(value, |entry| {
                    mark_entry(entry, *policy, &ctx, &mut diagnostics);
                }),
            },
            KeyForms::Include => for_each_entry_mut(value, |entry| {
                if matches!(entry.value, ConfigValueKind::Map(_)) {
                    // Smart-include object: only `file:` is a path;
                    // `text:` is literal content.
                    if let Some(file) = entry.get_mut("file") {
                        mark_entry(file, *policy, &ctx, &mut diagnostics);
                    }
                } else {
                    mark_entry(entry, *policy, &ctx, &mut diagnostics);
                }
            }),
        }
    }
    diagnostics
}

/// Mark one string entry according to `policy` (see [`MarkPolicy`]).
fn mark_entry(
    entry: &mut ConfigValue,
    policy: MarkPolicy,
    ctx: &MarkCtx<'_>,
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    if matches!(entry.value, ConfigValueKind::Path(_)) {
        return;
    }
    let Some(declared) = entry.as_plain_text() else {
        return;
    };
    if quarto_util::is_external_url(&declared) {
        return;
    }
    let source = candidate_path(&declared, ctx.layer_base, ctx.project_dir);
    match policy {
        MarkPolicy::Always => {}
        MarkPolicy::ExistenceSilent => {
            if !ctx.runtime.is_file(&source).unwrap_or(false) {
                return;
            }
        }
        MarkPolicy::ExistenceDiagnose => {
            if !ctx.runtime.is_file(&source).unwrap_or(false) {
                diagnostics.push(missing_css_diagnostic(&declared, entry));
                return;
            }
        }
    }
    let doc_relative =
        pathdiff::diff_paths(&source, ctx.document_dir).unwrap_or_else(|| source.clone());
    entry.value = ConfigValueKind::Path(quarto_util::to_forward_slashes(&doc_relative));
}

/// Build the Q-5-29 warning for a declared stylesheet whose file does
/// not exist. The `<link>` is still emitted verbatim (a visibly
/// broken stylesheet beats a silently absent one — the favicon
/// posture), and nothing is copied.
fn missing_css_diagnostic(declared: &str, entry: &ConfigValue) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning(format!("`css` refers to missing file '{declared}'"))
        .with_code("Q-5-29")
        .with_location(entry.source_info.clone())
        .problem(format!(
            "The declared stylesheet `{declared}` does not exist, so it was \
             not copied into the output. Every page linking it will request \
             a file that is not there. The render continued without it."
        ))
        .add_hint(
            "Check the path in the `css` entry (it is resolved relative to \
             the file that declares it), or add the missing stylesheet.",
        )
        .build()
}

/// Project-render-level check of the project config's own `css`
/// entries, run **once** by the orchestrator (so a `_quarto.yml`
/// mistake does not warn once per rendered page).
///
/// Checks every non-URL entry — including explicit `!path` values,
/// which the per-layer marking deliberately skips — against the
/// project root.
pub fn missing_project_css_diagnostics(
    project: &ProjectContext,
    base_format: &str,
    runtime: &dyn SystemRuntime,
) -> Vec<DiagnosticMessage> {
    let mut diagnostics = Vec::new();
    let Some(metadata) = project.config.metadata.as_ref() else {
        return diagnostics;
    };
    let flattened = resolve_format_config(metadata, base_format);
    let Some(css) = flattened.get("css") else {
        return diagnostics;
    };
    for_each_entry(css, |entry| {
        let Some(declared) = entry.as_plain_text() else {
            return;
        };
        if quarto_util::is_external_url(&declared) {
            return;
        }
        let source = candidate_path(&declared, &project.dir, &project.dir);
        if !runtime.is_file(&source).unwrap_or(false) {
            diagnostics.push(missing_css_diagnostic(&declared, entry));
        }
    });
    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_source_map::SourceInfo;
    use quarto_system_runtime::NativeRuntime;

    fn scalar(s: &str) -> ConfigValue {
        ConfigValue::new_string(s, SourceInfo::for_test())
    }

    fn key_map(key: &str, entries: Vec<ConfigValue>) -> ConfigValue {
        let value = if entries.len() == 1 {
            entries.into_iter().next().unwrap()
        } else {
            ConfigValue::new_array(entries, SourceInfo::for_test())
        };
        ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: key.to_string(),
                key_source: SourceInfo::for_test(),
                value,
            }],
            SourceInfo::for_test(),
        )
    }

    fn css_map(entries: Vec<ConfigValue>) -> ConfigValue {
        key_map("css", entries)
    }

    /// Existing file → Path-kind, rebased project dir → document dir.
    #[test]
    fn marks_existing_file_document_relative() {
        let temp = tempfile::TempDir::new().unwrap();
        let project = temp.path().canonicalize().unwrap();
        std::fs::write(project.join("styles.css"), "x").unwrap();
        let doc_dir = project.join("deep/deeper");
        std::fs::create_dir_all(&doc_dir).unwrap();

        let mut meta = css_map(vec![scalar("styles.css")]);
        let diags = mark_format_path_values(
            &mut meta,
            &project,
            &project,
            &doc_dir,
            &NativeRuntime::new(),
        );
        assert!(diags.is_empty(), "existing file must not diagnose");
        let css = meta.get("css").unwrap();
        assert_eq!(
            css.value,
            ConfigValueKind::Path("../../styles.css".to_string()),
            "expected Path kind rebased to the document dir"
        );
    }

    /// Missing file → untouched value + one Q-5-29 naming the entry.
    #[test]
    fn missing_file_diagnoses_and_leaves_value() {
        let temp = tempfile::TempDir::new().unwrap();
        let project = temp.path().canonicalize().unwrap();

        let mut meta = css_map(vec![scalar("nope.css")]);
        let diags = mark_format_path_values(
            &mut meta,
            &project,
            &project,
            &project,
            &NativeRuntime::new(),
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("Q-5-29"));
        assert!(diags[0].title.contains("nope.css"));
        let entry = meta.get("css").unwrap();
        assert!(
            matches!(entry.value, ConfigValueKind::Scalar { .. }),
            "missing entry must stay Scalar"
        );
        assert_eq!(
            entry.as_plain_text().as_deref(),
            Some("nope.css"),
            "missing entry must stay verbatim"
        );
    }

    /// Array form: each item marked independently; URL passthrough.
    #[test]
    fn array_marks_items_independently_and_skips_urls() {
        let temp = tempfile::TempDir::new().unwrap();
        let project = temp.path().canonicalize().unwrap();
        std::fs::write(project.join("a.css"), "x").unwrap();

        let mut meta = css_map(vec![
            scalar("a.css"),
            scalar("https://example.com/x.css"),
            scalar("gone.css"),
        ]);
        let diags = mark_format_path_values(
            &mut meta,
            &project,
            &project,
            &project,
            &NativeRuntime::new(),
        );
        assert_eq!(diags.len(), 1, "only the missing local file diagnoses");
        assert!(diags[0].title.contains("gone.css"));
        let items = meta.get("css").unwrap().as_array().unwrap();
        assert_eq!(items[0].value, ConfigValueKind::Path("a.css".to_string()));
        assert!(matches!(items[1].value, ConfigValueKind::Scalar { .. }));
        assert_eq!(
            items[1].as_plain_text().as_deref(),
            Some("https://example.com/x.css")
        );
        assert!(matches!(items[2].value, ConfigValueKind::Scalar { .. }));
        assert_eq!(items[2].as_plain_text().as_deref(), Some("gone.css"));
    }

    /// A leading `/` anchors at the project root even when the layer
    /// base is a subdirectory (Decision 4: site-root-relative).
    #[test]
    fn rooted_entry_anchors_at_project_root() {
        let temp = tempfile::TempDir::new().unwrap();
        let project = temp.path().canonicalize().unwrap();
        std::fs::write(project.join("styles.css"), "x").unwrap();
        let doc_dir = project.join("deep");
        std::fs::create_dir_all(&doc_dir).unwrap();

        let mut meta = css_map(vec![scalar("/styles.css")]);
        let diags = mark_format_path_values(
            &mut meta,
            &doc_dir, // layer base is the doc dir — must not be used
            &project,
            &doc_dir,
            &NativeRuntime::new(),
        );
        assert!(diags.is_empty());
        assert_eq!(
            meta.get("css").unwrap().value,
            ConfigValueKind::Path("../styles.css".to_string())
        );
    }

    /// Explicit `!path` values are the merge machinery's business —
    /// marking must not touch them.
    #[test]
    fn explicit_path_kind_left_alone() {
        let temp = tempfile::TempDir::new().unwrap();
        let project = temp.path().canonicalize().unwrap();

        let mut meta = css_map(vec![ConfigValue::new_path(
            "already/rebased.css".to_string(),
            SourceInfo::for_test(),
        )]);
        let diags = mark_format_path_values(
            &mut meta,
            &project,
            &project,
            &project,
            &NativeRuntime::new(),
        );
        assert!(diags.is_empty());
        assert_eq!(
            meta.get("css").unwrap().value,
            ConfigValueKind::Path("already/rebased.css".to_string())
        );
    }

    // === include-* slots: unconditional marking (bd-oejuizi9) ===

    /// An include entry is marked even when the file does not exist —
    /// no diagnostic here; Q-5-4 fires at resolve time with the
    /// declaration-resolved path.
    #[test]
    fn include_entry_marked_unconditionally() {
        let temp = tempfile::TempDir::new().unwrap();
        let project = temp.path().canonicalize().unwrap();
        std::fs::write(project.join("hdr.html"), "x").unwrap();
        let doc_dir = project.join("sub");
        std::fs::create_dir_all(&doc_dir).unwrap();

        for declared in ["hdr.html", "missing.html"] {
            let mut meta = key_map("include-in-header", vec![scalar(declared)]);
            let diags = mark_format_path_values(
                &mut meta,
                &project,
                &project,
                &doc_dir,
                &NativeRuntime::new(),
            );
            assert!(diags.is_empty(), "include marking never diagnoses");
            assert_eq!(
                meta.get("include-in-header").unwrap().value,
                ConfigValueKind::Path(format!("../{declared}")),
                "entry '{declared}' must be rebased regardless of existence"
            );
        }
    }

    /// Smart-include maps: `file:` is a path and gets marked; `text:`
    /// is literal content and must never be touched.
    #[test]
    fn include_map_marks_file_not_text() {
        let temp = tempfile::TempDir::new().unwrap();
        let project = temp.path().canonicalize().unwrap();
        let doc_dir = project.join("sub");
        std::fs::create_dir_all(&doc_dir).unwrap();

        let file_entry = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "file".to_string(),
                key_source: SourceInfo::for_test(),
                value: scalar("hdr.html"),
            }],
            SourceInfo::for_test(),
        );
        let text_entry = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "text".to_string(),
                key_source: SourceInfo::for_test(),
                value: scalar("hdr.html"),
            }],
            SourceInfo::for_test(),
        );
        let mut meta = key_map("include-in-header", vec![file_entry, text_entry]);
        let diags = mark_format_path_values(
            &mut meta,
            &project,
            &project,
            &doc_dir,
            &NativeRuntime::new(),
        );
        assert!(diags.is_empty());
        let items = meta.get("include-in-header").unwrap().as_array().unwrap();
        assert_eq!(
            items[0].get("file").unwrap().value,
            ConfigValueKind::Path("../hdr.html".to_string()),
            "file: value must be marked"
        );
        assert!(
            matches!(
                items[1].get("text").unwrap().value,
                ConfigValueKind::Scalar { .. }
            ),
            "text: value must stay untouched even when it looks like a path"
        );
    }

    /// A leading `/` on an include entry anchors at the project root
    /// (contract rule 2 in filesystem space; bd-rdcvjy2s).
    #[test]
    fn rooted_include_entry_anchors_at_project_root() {
        let temp = tempfile::TempDir::new().unwrap();
        let project = temp.path().canonicalize().unwrap();
        let doc_dir = project.join("sub");
        std::fs::create_dir_all(&doc_dir).unwrap();

        let mut meta = key_map("include-in-header", vec![scalar("/hdr.html")]);
        let diags = mark_format_path_values(
            &mut meta,
            &doc_dir, // layer base must not be used for rooted entries
            &project,
            &doc_dir,
            &NativeRuntime::new(),
        );
        assert!(diags.is_empty());
        assert_eq!(
            meta.get("include-in-header").unwrap().value,
            ConfigValueKind::Path("../hdr.html".to_string())
        );
    }

    // === theme: existence-driven, silent (bd-oejuizi9) ===

    /// Built-in theme names name no file: untouched, and — unlike
    /// css — no diagnostic.
    #[test]
    fn theme_builtin_name_untouched_and_silent() {
        let temp = tempfile::TempDir::new().unwrap();
        let project = temp.path().canonicalize().unwrap();

        let mut meta = key_map("theme", vec![scalar("cosmo")]);
        let diags = mark_format_path_values(
            &mut meta,
            &project,
            &project,
            &project,
            &NativeRuntime::new(),
        );
        assert!(diags.is_empty(), "theme marking must never diagnose");
        assert!(
            matches!(
                meta.get("theme").unwrap().value,
                ConfigValueKind::Scalar { .. }
            ),
            "builtin name must stay Scalar"
        );
    }

    /// Mixed array: the builtin stays, the existing custom scss is
    /// rebased to the document dir.
    #[test]
    fn theme_array_marks_existing_scss_only() {
        let temp = tempfile::TempDir::new().unwrap();
        let project = temp.path().canonicalize().unwrap();
        std::fs::write(project.join("custom.scss"), "x").unwrap();
        let doc_dir = project.join("sub");
        std::fs::create_dir_all(&doc_dir).unwrap();

        let mut meta = key_map("theme", vec![scalar("cosmo"), scalar("custom.scss")]);
        let diags = mark_format_path_values(
            &mut meta,
            &project,
            &project,
            &doc_dir,
            &NativeRuntime::new(),
        );
        assert!(diags.is_empty());
        let items = meta.get("theme").unwrap().as_array().unwrap();
        assert!(matches!(items[0].value, ConfigValueKind::Scalar { .. }));
        assert_eq!(
            items[1].value,
            ConfigValueKind::Path("../custom.scss".to_string())
        );
    }

    /// `{light:, dark:}` map form: each variant's entries are walked.
    #[test]
    fn theme_light_dark_map_recursed() {
        let temp = tempfile::TempDir::new().unwrap();
        let project = temp.path().canonicalize().unwrap();
        std::fs::write(project.join("light.scss"), "x").unwrap();
        std::fs::write(project.join("dark.scss"), "x").unwrap();
        let doc_dir = project.join("sub");
        std::fs::create_dir_all(&doc_dir).unwrap();

        let theme = ConfigValue::new_map(
            vec![
                ConfigMapEntry {
                    key: "light".to_string(),
                    key_source: SourceInfo::for_test(),
                    value: ConfigValue::new_array(
                        vec![scalar("cosmo"), scalar("light.scss")],
                        SourceInfo::for_test(),
                    ),
                },
                ConfigMapEntry {
                    key: "dark".to_string(),
                    key_source: SourceInfo::for_test(),
                    value: scalar("dark.scss"),
                },
            ],
            SourceInfo::for_test(),
        );
        let mut meta = key_map("theme", vec![theme]);
        let diags = mark_format_path_values(
            &mut meta,
            &project,
            &project,
            &doc_dir,
            &NativeRuntime::new(),
        );
        assert!(diags.is_empty());
        let theme = meta.get("theme").unwrap();
        let light = theme.get("light").unwrap().as_array().unwrap();
        assert!(matches!(light[0].value, ConfigValueKind::Scalar { .. }));
        assert_eq!(
            light[1].value,
            ConfigValueKind::Path("../light.scss".to_string())
        );
        assert_eq!(
            theme.get("dark").unwrap().value,
            ConfigValueKind::Path("../dark.scss".to_string())
        );
    }
}
