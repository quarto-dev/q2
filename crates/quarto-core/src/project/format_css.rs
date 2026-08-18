/*
 * project/format_css.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * User-declared stylesheet (`css:` / `format.html.css`) path marking
 * and the Q-5-29 missing-file diagnostic.
 * bd-format-css-not-copied-crn3bjdz.
 */

//! Merge-time marking of user-declared stylesheet paths.
//!
//! A `css:` entry is authored relative to the file that declares it:
//! `_quarto.yml` entries are project-root-relative, `_metadata.yml`
//! entries are relative to that file's directory, and document
//! front-matter entries are relative to the document. After the
//! metadata merge flattens these layers, that provenance is gone — so
//! each layer's entries are normalized *during* the merge, while the
//! layer's base directory is still known ([`mark_css_path_values`]).
//!
//! Marking is existence-driven, mirroring the extension-fragment
//! machinery (`FRAGMENT_PATH_PATTERNS`) and Q1's
//! `toInputRelativePaths`: a string that names an existing file
//! becomes a [`ConfigValueKind::Path`] holding the equivalent
//! document-relative path; external URLs and strings that name no
//! file pass through untouched. The downstream
//! [`FormatCssTransform`](crate::transforms::FormatCssTransform)
//! consumes only marked (`Path`-kind) entries — copy intents and
//! per-page hrefs — and never diagnoses; missing files are diagnosed
//! here, at the declaration site, so a project-wide mistake warns
//! once per declaring layer instead of once per rendered page:
//!
//! - project config: [`missing_project_css_diagnostics`], called once
//!   per project render by the orchestrator;
//! - directory metadata and document front matter: the diagnostics
//!   returned by [`mark_css_path_values`], pushed into the declaring
//!   document's own render diagnostics.

use std::path::{Path, PathBuf};

use quarto_config::resolve_format_config;
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::config_value::ConfigValueKind;
use quarto_system_runtime::SystemRuntime;

use crate::project::ProjectContext;

/// Apply `f` to a `css:` value's string-bearing leaves: the scalar
/// forms directly, or each item of the array form.
fn for_each_css_entry_mut(css: &mut ConfigValue, mut f: impl FnMut(&mut ConfigValue)) {
    match &mut css.value {
        ConfigValueKind::Array(items) => {
            for item in items {
                f(item);
            }
        }
        _ => f(css),
    }
}

/// Immutable counterpart of [`for_each_css_entry_mut`].
fn for_each_css_entry(css: &ConfigValue, mut f: impl FnMut(&ConfigValue)) {
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

/// Mark existing user-declared stylesheets in one metadata layer as
/// document-relative `Path` values.
///
/// `layer_base` is the directory the layer's relative paths are
/// authored against (project root for `_quarto.yml`, the metadata
/// file's directory for `_metadata.yml`, the document's directory for
/// front matter). Entries that are already `Path`-kind (explicit
/// `!path`, extension fragments) are left alone — the merge machinery
/// already rebases those.
///
/// Returns a Q-5-29 warning for each entry that names no file. The
/// caller decides whether to surface them: per-document layers push
/// them into the document's diagnostics; the project layer drops them
/// because [`missing_project_css_diagnostics`] reports the same
/// entries once per project render instead of once per page.
pub(crate) fn mark_css_path_values(
    metadata: &mut ConfigValue,
    layer_base: &Path,
    project_dir: &Path,
    document_dir: &Path,
    runtime: &dyn SystemRuntime,
) -> Vec<DiagnosticMessage> {
    let mut diagnostics = Vec::new();
    let Some(css) = metadata.get_mut("css") else {
        return diagnostics;
    };
    for_each_css_entry_mut(css, |entry| {
        if matches!(entry.value, ConfigValueKind::Path(_)) {
            return;
        }
        let Some(declared) = entry.as_plain_text() else {
            return;
        };
        if quarto_util::is_external_url(&declared) {
            return;
        }
        let source = candidate_path(&declared, layer_base, project_dir);
        if runtime.is_file(&source).unwrap_or(false) {
            let doc_relative =
                pathdiff::diff_paths(&source, document_dir).unwrap_or_else(|| source.clone());
            entry.value = ConfigValueKind::Path(quarto_util::to_forward_slashes(&doc_relative));
        } else {
            diagnostics.push(missing_css_diagnostic(&declared, entry));
        }
    });
    diagnostics
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
    for_each_css_entry(css, |entry| {
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

    fn css_map(entries: Vec<ConfigValue>) -> ConfigValue {
        let css = if entries.len() == 1 {
            entries.into_iter().next().unwrap()
        } else {
            ConfigValue::new_array(entries, SourceInfo::for_test())
        };
        ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "css".to_string(),
                key_source: SourceInfo::for_test(),
                value: css,
            }],
            SourceInfo::for_test(),
        )
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
        let diags = mark_css_path_values(
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
        let diags = mark_css_path_values(
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
            matches!(entry.value, ConfigValueKind::Scalar(_)),
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
        let diags = mark_css_path_values(
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
        assert!(matches!(items[1].value, ConfigValueKind::Scalar(_)));
        assert_eq!(
            items[1].as_plain_text().as_deref(),
            Some("https://example.com/x.css")
        );
        assert!(matches!(items[2].value, ConfigValueKind::Scalar(_)));
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
        let diags = mark_css_path_values(
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
        let diags = mark_css_path_values(
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
}
