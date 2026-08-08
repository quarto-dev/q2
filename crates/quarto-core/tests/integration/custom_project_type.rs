/*
 * tests/integration/custom_project_type.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Extension-contributed custom project types (bd-ad7i1pc6, Phase 3).
 */

//! Custom project types resolved from `contributes.project`.
//!
//! A `_quarto.yml` `project.type` that is not a built-in name resolves
//! against extensions discovered at project-config time
//! (`discover_project_extensions`). The matching extension's
//! `contributes.project` is a `_quarto.yml` *fragment*: its
//! `project.type` names the built-in base type the project actually
//! renders as, and the rest merges **under** the user's config —
//! user wins scalar conflicts, arrays concat (extension entries
//! first), and a user value tagged `!prefer` replaces the extension's
//! outright. No Q1-style special cases: Quarto 2's standard merge
//! semantics apply uniformly.

use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::error::QuartoError;
use quarto_core::project::{ProjectContext, ProjectKind};
use quarto_error_reporting::DiagnosticKind;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// Build a project in a tempdir: `_quarto.yml` plus
/// `(extension-dir-relative-to-_extensions, _extension.yml content)`
/// pairs, then discover it.
fn discover_with_extensions(
    quarto_yml: &str,
    extensions: &[(&str, &str)],
) -> (quarto_core::error::Result<ProjectContext>, TempDir) {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("_quarto.yml"), quarto_yml).unwrap();
    std::fs::write(tmp.path().join("index.qmd"), "# Hello\n").unwrap();
    for (rel_dir, manifest) in extensions {
        let dir = tmp.path().join("_extensions").join(rel_dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("_extension.yml"), manifest).unwrap();
    }
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = ProjectContext::discover(tmp.path(), runtime.as_ref());
    (result, tmp)
}

const FANCYSITE: &str = r#"
title: Fancy Site
contributes:
  project:
    project:
      type: website
    website:
      title: "From The Extension"
      bread-crumbs: true
    format:
      html:
        include-in-header:
          - ext-header.html
"#;

fn parse_error(result: quarto_core::error::Result<ProjectContext>) -> quarto_core::ParseError {
    match result {
        Err(QuartoError::Parse(pe)) => pe,
        Err(other) => panic!("expected QuartoError::Parse, got: {other:?}"),
        Ok(_) => panic!("expected discovery to fail"),
    }
}

/// String value at a metadata path, for merge assertions.
fn meta_str(project: &ProjectContext, path: &[&str]) -> Option<String> {
    let mut current = project.config.metadata.as_ref()?;
    for key in path {
        current = current.get(key)?;
    }
    current.as_str().map(|s| s.to_string())
}

// ── Resolution ──────────────────────────────────────────────────────

#[test]
fn custom_type_resolves_to_website_base() {
    let (result, _tmp) = discover_with_extensions(
        "project:\n  type: fancysite\n",
        &[("acme/fancysite", FANCYSITE)],
    );
    let project = result.expect("custom type backed by an extension must resolve");

    assert_eq!(project.project_kind(), ProjectKind::Website);
    let custom = project
        .config
        .custom_project_type
        .as_ref()
        .expect("custom_project_type must be recorded");
    assert_eq!(custom.name, "fancysite");
    assert_eq!(custom.extension_id, "acme/fancysite");
    assert!(custom.extension_dir.ends_with("_extensions/acme/fancysite"));

    // Website base drives downstream defaults: output-dir is `_site`.
    assert!(
        project.output_dir.ends_with("_site"),
        "website base must give the _site output-dir default; got {}",
        project.output_dir.display()
    );
    // The merged metadata carries the *base* type so every downstream
    // consumer sees an ordinary website project.
    assert_eq!(
        meta_str(&project, &["project", "type"]).as_deref(),
        Some("website")
    );
}

#[test]
fn custom_type_label_names_extension_and_base() {
    let (result, _tmp) = discover_with_extensions(
        "project:\n  type: fancysite\n",
        &[("acme/fancysite", FANCYSITE)],
    );
    let project = result.unwrap();
    assert_eq!(project.project_type_label(), "fancysite (website)");

    let (result, _tmp) = discover_with_extensions("project:\n  type: website\n", &[]);
    assert_eq!(result.unwrap().project_type_label(), "website");
}

#[test]
fn exact_org_match_beats_name_only() {
    let other = r#"
contributes:
  project:
    project:
      type: default
"#;
    let (result, _tmp) = discover_with_extensions(
        "project:\n  type: acme/fancysite\n",
        &[("acme/fancysite", FANCYSITE), ("other/fancysite", other)],
    );
    let project = result.expect("org-qualified type must resolve");
    assert_eq!(project.project_kind(), ProjectKind::Website);
    assert_eq!(
        project
            .config
            .custom_project_type
            .as_ref()
            .unwrap()
            .extension_id,
        "acme/fancysite"
    );
    assert!(project.config.config_diagnostics.is_empty());
}

#[test]
fn ambiguous_name_only_match_warns_and_prefers_orgless() {
    let orgless = r#"
contributes:
  project:
    project:
      type: default
"#;
    let (result, _tmp) = discover_with_extensions(
        "project:\n  type: fancysite\n",
        &[("fancysite", orgless), ("acme/fancysite", FANCYSITE)],
    );
    let project = result.expect("ambiguous name must still resolve");
    // Org-less extension wins (Q1 parity), so base is `default`.
    assert_eq!(project.project_kind(), ProjectKind::Default);
    assert_eq!(
        project
            .config
            .custom_project_type
            .as_ref()
            .unwrap()
            .extension_id,
        "fancysite"
    );
    let warnings: Vec<_> = project
        .config
        .config_diagnostics
        .iter()
        .filter(|d| d.code.as_deref() == Some("Q-16-8"))
        .collect();
    assert_eq!(warnings.len(), 1, "must warn about the ambiguity");
    assert_eq!(warnings[0].kind, DiagnosticKind::Warning);
    let text = warnings[0].to_text(None);
    assert!(
        text.contains("fancysite") && text.contains("acme/fancysite"),
        "warning must name the chosen and the shadowed extension; got: {text}"
    );
}

// ── Merge semantics ─────────────────────────────────────────────────

#[test]
fn user_scalar_wins_extension_fills_gaps() {
    let (result, _tmp) = discover_with_extensions(
        "project:\n  type: fancysite\nwebsite:\n  title: \"Mine\"\n",
        &[("acme/fancysite", FANCYSITE)],
    );
    let project = result.unwrap();
    // Collision: user wins.
    assert_eq!(
        meta_str(&project, &["website", "title"]).as_deref(),
        Some("Mine")
    );
    // Gap: extension fills.
    let breadcrumbs = project
        .config
        .metadata
        .as_ref()
        .and_then(|m| m.get("website"))
        .and_then(|w| w.get("bread-crumbs"))
        .and_then(|b| b.as_bool());
    assert_eq!(breadcrumbs, Some(true));
}

#[test]
fn arrays_concat_extension_entries_first() {
    let (result, _tmp) = discover_with_extensions(
        "project:\n  type: fancysite\nformat:\n  html:\n    include-in-header:\n      - user-header.html\n",
        &[("acme/fancysite", FANCYSITE)],
    );
    let project = result.unwrap();
    let items: Vec<String> = project
        .config
        .metadata
        .as_ref()
        .and_then(|m| m.get("format"))
        .and_then(|f| f.get("html"))
        .and_then(|h| h.get("include-in-header"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|i| i.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(items, vec!["ext-header.html", "user-header.html"]);
}

#[test]
#[ignore = "bd-43lc07w1: quarto-yaml drops !prefer tags on sequences/mappings, so the \
            tag never reaches the merge; un-ignore when the upstream fix ships"]
fn user_prefer_replaces_extension_array() {
    // No Q1-style special-casing: a project that wants to *replace* an
    // extension-contributed list uses Quarto 2's `!prefer`.
    let (result, _tmp) = discover_with_extensions(
        "project:\n  type: fancysite\nformat:\n  html:\n    include-in-header: !prefer\n      - user-header.html\n",
        &[("acme/fancysite", FANCYSITE)],
    );
    let project = result.unwrap();
    let items: Vec<String> = project
        .config
        .metadata
        .as_ref()
        .and_then(|m| m.get("format"))
        .and_then(|f| f.get("html"))
        .and_then(|h| h.get("include-in-header"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|i| i.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(items, vec!["user-header.html"]);
}

#[test]
fn extension_render_globs_concat_with_users() {
    let ext = r#"
contributes:
  project:
    project:
      type: website
      render:
        - "**/*.qmd"
"#;
    let (result, _tmp) = discover_with_extensions(
        "project:\n  type: fancysite\n  render:\n    - index.qmd\n",
        &[("acme/fancysite", ext)],
    );
    let project = result.unwrap();
    let patterns: Vec<&str> = project
        .config
        .render_patterns
        .iter()
        .map(|g| g.raw.as_str())
        .collect();
    assert_eq!(patterns, vec!["**/*.qmd", "index.qmd"]);
}

#[test]
fn detect_is_stripped_from_merged_config() {
    let ext = r#"
contributes:
  project:
    project:
      type: website
      detect:
        - ["fancy.config.json"]
"#;
    let (result, _tmp) =
        discover_with_extensions("project:\n  type: fancysite\n", &[("acme/fancysite", ext)]);
    let project = result.unwrap();
    assert!(
        project
            .config
            .metadata
            .as_ref()
            .and_then(|m| m.get("project"))
            .and_then(|p| p.get("detect"))
            .is_none(),
        "`project.detect` is bootstrap-only and must not leak into the merged config"
    );
}

// ── Failure modes ───────────────────────────────────────────────────

#[test]
fn unknown_type_with_no_matching_extension_lists_candidates() {
    let (result, _tmp) = discover_with_extensions(
        "project:\n  type: nonexistent\n",
        &[("acme/fancysite", FANCYSITE)],
    );
    let pe = parse_error(result);
    assert_eq!(pe.diagnostics[0].code.as_deref(), Some("Q-5-17"));
    let text = pe.render();
    assert!(
        text.contains("acme/fancysite"),
        "error must list project-contributing extensions that were found; got: {text}"
    );
}

#[test]
fn extension_without_project_contribution_does_not_match() {
    let shortcode_only = r#"
contributes:
  shortcodes:
    - hello.lua
"#;
    let (result, _tmp) = discover_with_extensions(
        "project:\n  type: fancysite\n",
        &[("acme/fancysite", shortcode_only)],
    );
    let pe = parse_error(result);
    assert_eq!(pe.diagnostics[0].code.as_deref(), Some("Q-5-17"));
}

#[test]
fn base_type_book_is_a_q_16_7_error() {
    let ext = r#"
contributes:
  project:
    project:
      type: book
"#;
    let (result, _tmp) =
        discover_with_extensions("project:\n  type: fancybook\n", &[("acme/fancybook", ext)]);
    let pe = parse_error(result);
    assert_eq!(pe.diagnostics[0].code.as_deref(), Some("Q-16-7"));
    assert_eq!(pe.diagnostics[0].kind, DiagnosticKind::Error);
    let text = pe.render();
    assert!(
        text.contains("book") && text.contains("not yet"),
        "error must say book base types are not yet supported; got: {text}"
    );
}

#[test]
fn chained_custom_base_is_a_q_16_7_error() {
    let ext = r#"
contributes:
  project:
    project:
      type: other-custom-type
"#;
    let (result, _tmp) =
        discover_with_extensions("project:\n  type: fancysite\n", &[("acme/fancysite", ext)]);
    let pe = parse_error(result);
    assert_eq!(pe.diagnostics[0].code.as_deref(), Some("Q-16-7"));
    let text = pe.render();
    assert!(
        text.contains("other-custom-type"),
        "error must name the invalid base type; got: {text}"
    );
}

#[test]
fn missing_base_type_defaults_with_q_16_9_warning() {
    let ext = r#"
contributes:
  project:
    website:
      bread-crumbs: true
"#;
    let (result, _tmp) =
        discover_with_extensions("project:\n  type: fancysite\n", &[("acme/fancysite", ext)]);
    let project = result.expect("missing base type defaults to `default` (Q1 parity)");
    assert_eq!(project.project_kind(), ProjectKind::Default);
    let warnings: Vec<_> = project
        .config
        .config_diagnostics
        .iter()
        .filter(|d| d.code.as_deref() == Some("Q-16-9"))
        .collect();
    assert_eq!(warnings.len(), 1, "must warn about the missing base type");
    assert_eq!(warnings[0].kind, DiagnosticKind::Warning);
}
