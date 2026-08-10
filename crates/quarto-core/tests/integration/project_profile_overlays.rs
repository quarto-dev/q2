/*
 * tests/integration/project_profile_overlays.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Project-profile config overlays (bd-fu16z22k, Phase 1).
 */

//! `_quarto-<name>.yml` overlay discovery and merging contract.
//!
//! Covers: overlay merge semantics (scalar override, map deep-merge,
//! array concat, `!prefer`), first-listed-wins ordering,
//! `_quarto.yml.local` as the highest-priority layer and as a
//! `profile.default` source, activation from `profile.default` /
//! `profile.group`, the Q-5-19 unknown-profile warning, Q-5-22 inert
//! `profile:` keys in overlays, hard Q-5-20/21 errors aborting
//! discovery, and FileId/span integrity for diagnostics anchored in
//! overlay files.
//!
//! ⚠️ "Profile" here means *project profiles* (`--profile`,
//! `QUARTO_PROFILE`), not `DocumentProfile` (the pass-1 summary).

use std::path::Path;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::error::QuartoError;
use quarto_core::project::ProjectContext;
use quarto_error_reporting::{DiagnosticKind, DiagnosticMessage};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// Write a project fixture and discover it with an explicit profile
/// selection (`None` = no `--profile`; the process environment is not
/// consulted because tests must not depend on the caller's env).
fn discover_with(
    files: &[(&str, &str)],
    selection: Option<&[&str]>,
) -> (quarto_core::error::Result<ProjectContext>, TempDir) {
    let tmp = TempDir::new().unwrap();
    for (name, content) in files {
        std::fs::write(tmp.path().join(name), content).unwrap();
    }
    std::fs::write(tmp.path().join("index.qmd"), "# Hello\n").unwrap();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let owned: Option<Vec<String>> = selection.map(|s| s.iter().map(|p| p.to_string()).collect());
    let result =
        ProjectContext::discover_with_profile(tmp.path(), runtime.as_ref(), owned.as_deref());
    (result, tmp)
}

fn ok(result: quarto_core::error::Result<ProjectContext>) -> ProjectContext {
    result.expect("discovery must succeed")
}

fn parse_error(result: quarto_core::error::Result<ProjectContext>) -> quarto_core::ParseError {
    match result {
        Err(QuartoError::Parse(pe)) => pe,
        Err(other) => panic!("expected QuartoError::Parse, got: {other:?}"),
        Ok(_) => panic!("expected discovery to fail"),
    }
}

/// The merged metadata value at `path`, as a string.
fn meta_str(project: &ProjectContext, path: &[&str]) -> Option<String> {
    project
        .config
        .metadata
        .as_ref()
        .and_then(|m| m.get_path(path))
        .and_then(|v| v.as_plain_text())
}

fn diags_with_code<'a>(diags: &'a [DiagnosticMessage], code: &str) -> Vec<&'a DiagnosticMessage> {
    diags
        .iter()
        .filter(|d| d.code.as_deref() == Some(code))
        .collect()
}

fn active_names(project: &ProjectContext) -> Vec<&str> {
    project
        .config
        .active_config_profiles
        .iter()
        .map(|p| p.name.as_str())
        .collect()
}

// ── merge semantics ─────────────────────────────────────────────────

#[test]
fn overlay_scalar_overrides_base() {
    let (result, _tmp) = discover_with(
        &[
            ("_quarto.yml", "execute:\n  freeze: true\n"),
            ("_quarto-prod.yml", "execute:\n  freeze: false\n"),
        ],
        Some(&["prod"]),
    );
    let project = ok(result);
    let freeze = project
        .config
        .metadata
        .as_ref()
        .and_then(|m| m.get_path(&["execute", "freeze"]))
        .and_then(|v| v.as_bool());
    assert_eq!(freeze, Some(false), "overlay scalar must win over base");
}

#[test]
fn overlay_map_deep_merges_with_base() {
    let (result, _tmp) = discover_with(
        &[
            ("_quarto.yml", "format:\n  html:\n    toc: true\n"),
            (
                "_quarto-prod.yml",
                "format:\n  html:\n    code-fold: true\n",
            ),
        ],
        Some(&["prod"]),
    );
    let project = ok(result);
    let get_bool = |path: &[&str]| {
        project
            .config
            .metadata
            .as_ref()
            .and_then(|m| m.get_path(path))
            .and_then(|v| v.as_bool())
    };
    assert_eq!(
        get_bool(&["format", "html", "toc"]),
        Some(true),
        "base key survives"
    );
    assert_eq!(
        get_bool(&["format", "html", "code-fold"]),
        Some(true),
        "overlay key added"
    );
}

#[test]
fn overlay_array_concats_by_default() {
    // Deliberate divergence from Q1 (union-with-dedup): Q2's Concat
    // appends. Documented in the plan's divergence table.
    let (result, _tmp) = discover_with(
        &[
            ("_quarto.yml", "extras: [alpha]\n"),
            ("_quarto-prod.yml", "extras: [beta]\n"),
        ],
        Some(&["prod"]),
    );
    let project = ok(result);
    let extras: Vec<String> = project
        .config
        .metadata
        .as_ref()
        .and_then(|m| m.get("extras"))
        .and_then(|v| v.as_array().map(|a| a.to_vec()))
        .map(|a| a.iter().filter_map(|v| v.as_plain_text()).collect())
        .unwrap_or_default();
    assert_eq!(extras, vec!["alpha", "beta"]);
}

#[test]
fn overlay_prefer_tag_replaces_array() {
    let (result, _tmp) = discover_with(
        &[
            ("_quarto.yml", "extras: [alpha]\n"),
            ("_quarto-prod.yml", "extras: !prefer [beta]\n"),
        ],
        Some(&["prod"]),
    );
    let project = ok(result);
    let extras: Vec<String> = project
        .config
        .metadata
        .as_ref()
        .and_then(|m| m.get("extras"))
        .and_then(|v| v.as_array().map(|a| a.to_vec()))
        .map(|a| a.iter().filter_map(|v| v.as_plain_text()).collect())
        .unwrap_or_default();
    assert_eq!(extras, vec!["beta"], "!prefer must replace, not append");
}

#[test]
fn first_listed_profile_wins_conflicts() {
    let (result, _tmp) = discover_with(
        &[
            ("_quarto.yml", "winner: base\n"),
            ("_quarto-a.yml", "winner: profile-a\n"),
            ("_quarto-b.yml", "winner: profile-b\n"),
        ],
        Some(&["a", "b"]),
    );
    let project = ok(result);
    assert_eq!(
        meta_str(&project, &["winner"]).as_deref(),
        Some("profile-a"),
        "the FIRST-listed profile must win conflicts (Q1 parity)"
    );
    assert_eq!(active_names(&project), vec!["a", "b"]);
}

#[test]
fn local_config_overrides_profiles() {
    let (result, _tmp) = discover_with(
        &[
            ("_quarto.yml", "winner: base\n"),
            ("_quarto-prod.yml", "winner: prod\n"),
            ("_quarto.yml.local", "winner: local\n"),
        ],
        Some(&["prod"]),
    );
    let project = ok(result);
    assert_eq!(
        meta_str(&project, &["winner"]).as_deref(),
        Some("local"),
        "_quarto.yml.local is the highest-priority layer"
    );
}

#[test]
fn local_config_applies_without_profiles_too() {
    let (result, _tmp) = discover_with(
        &[
            ("_quarto.yml", "winner: base\n"),
            ("_quarto.yml.local", "winner: local\n"),
        ],
        None,
    );
    let project = ok(result);
    assert_eq!(meta_str(&project, &["winner"]).as_deref(), Some("local"));
}

#[test]
fn overlay_yml_preferred_over_yaml() {
    let (result, _tmp) = discover_with(
        &[
            ("_quarto.yml", "winner: base\n"),
            ("_quarto-p.yml", "winner: from-yml\n"),
            ("_quarto-p.yaml", "winner: from-yaml\n"),
        ],
        Some(&["p"]),
    );
    let project = ok(result);
    assert_eq!(
        meta_str(&project, &["winner"]).as_deref(),
        Some("from-yml"),
        ".yml must be preferred when both extensions exist (Q1 parity)"
    );
}

#[test]
fn overlay_for_inactive_profile_is_ignored() {
    let (result, _tmp) = discover_with(
        &[
            ("_quarto.yml", "winner: base\n"),
            ("_quarto-other.yml", "winner: other\n"),
        ],
        None,
    );
    let project = ok(result);
    assert_eq!(meta_str(&project, &["winner"]).as_deref(), Some("base"));
    assert!(project.config.config_diagnostics.is_empty());
    assert!(active_names(&project).is_empty());
}

// ── activation from config ──────────────────────────────────────────

#[test]
fn profile_default_in_base_activates_overlay() {
    let (result, _tmp) = discover_with(
        &[
            ("_quarto.yml", "winner: base\nprofile:\n  default: prod\n"),
            ("_quarto-prod.yml", "winner: prod\n"),
        ],
        None,
    );
    let project = ok(result);
    assert_eq!(meta_str(&project, &["winner"]).as_deref(), Some("prod"));
    assert_eq!(active_names(&project), vec!["prod"]);
    assert!(
        project
            .config
            .metadata
            .as_ref()
            .is_some_and(|m| m.get("profile").is_none()),
        "profile: must be stripped from the merged metadata"
    );
}

#[test]
fn group_first_member_activates_when_none_selected() {
    let (result, _tmp) = discover_with(
        &[
            (
                "_quarto.yml",
                "winner: base\nprofile:\n  group: [basic, advanced]\n",
            ),
            ("_quarto-basic.yml", "winner: basic\n"),
            ("_quarto-advanced.yml", "winner: advanced\n"),
        ],
        None,
    );
    let project = ok(result);
    assert_eq!(meta_str(&project, &["winner"]).as_deref(), Some("basic"));
    assert_eq!(active_names(&project), vec!["basic"]);
}

#[test]
fn group_satisfied_by_explicit_selection() {
    let (result, _tmp) = discover_with(
        &[
            (
                "_quarto.yml",
                "winner: base\nprofile:\n  group: [basic, advanced]\n",
            ),
            ("_quarto-basic.yml", "winner: basic\n"),
            ("_quarto-advanced.yml", "winner: advanced\n"),
        ],
        Some(&["advanced"]),
    );
    let project = ok(result);
    assert_eq!(meta_str(&project, &["winner"]).as_deref(), Some("advanced"));
    assert_eq!(active_names(&project), vec!["advanced"]);
}

#[test]
fn local_profile_default_beats_base_default() {
    let (result, _tmp) = discover_with(
        &[
            ("_quarto.yml", "winner: base\nprofile:\n  default: a\n"),
            ("_quarto.yml.local", "profile:\n  default: b\n"),
            ("_quarto-a.yml", "winner: from-a\n"),
            ("_quarto-b.yml", "winner: from-b\n"),
        ],
        None,
    );
    let project = ok(result);
    assert_eq!(
        meta_str(&project, &["winner"]).as_deref(),
        Some("from-b"),
        "_quarto.yml.local's profile.default must beat _quarto.yml's"
    );
    assert_eq!(active_names(&project), vec!["b"]);
}

#[test]
fn explicit_selection_beats_config_defaults() {
    let (result, _tmp) = discover_with(
        &[
            ("_quarto.yml", "winner: base\nprofile:\n  default: a\n"),
            ("_quarto-a.yml", "winner: from-a\n"),
            ("_quarto-b.yml", "winner: from-b\n"),
        ],
        Some(&["b"]),
    );
    let project = ok(result);
    assert_eq!(meta_str(&project, &["winner"]).as_deref(), Some("from-b"));
    assert_eq!(active_names(&project), vec!["b"]);
}

// ── profile-aware project fields ────────────────────────────────────

#[test]
fn project_fields_are_profile_aware() {
    let (result, tmp) = discover_with(
        &[
            ("_quarto.yml", "project:\n  render:\n    - index.qmd\n"),
            (
                "_quarto-prod.yml",
                "project:\n  output-dir: _prod\n  pre-render: gen.py\n",
            ),
        ],
        Some(&["prod"]),
    );
    let project = ok(result);
    assert_eq!(
        project.config.output_dir.as_deref(),
        Some(Path::new("_prod")),
        "project.output-dir from the overlay must take effect"
    );
    assert_eq!(project.config.pre_render_scripts.len(), 1);
    assert_eq!(project.config.pre_render_scripts[0].command, "gen.py");
    assert_eq!(
        project.output_dir,
        tmp.path().canonicalize().unwrap().join("_prod"),
        "the resolved ProjectContext.output_dir must honor the overlay"
    );
}

// ── diagnostics ─────────────────────────────────────────────────────

#[test]
fn profile_key_in_overlay_warns_q_5_22_and_is_stripped() {
    let (result, _tmp) = discover_with(
        &[
            ("_quarto.yml", "winner: base\n"),
            (
                "_quarto-prod.yml",
                "winner: prod\nprofile:\n  default: other\n",
            ),
        ],
        Some(&["prod"]),
    );
    let project = ok(result);
    let warnings = diags_with_code(&project.config.config_diagnostics, "Q-5-22");
    assert_eq!(
        warnings.len(),
        1,
        "got: {:?}",
        project.config.config_diagnostics
    );
    assert_eq!(warnings[0].kind, DiagnosticKind::Warning);
    assert!(
        project
            .config
            .metadata
            .as_ref()
            .is_some_and(|m| m.get("profile").is_none()),
        "the overlay's profile: key must not leak into merged metadata"
    );
    // The inert `default: other` must not have activated anything.
    assert_eq!(active_names(&project), vec!["prod"]);
}

#[test]
fn q_5_22_warning_span_binds_to_the_overlay_file() {
    let (result, tmp) = discover_with(
        &[
            ("_quarto.yml", "winner: base\n"),
            ("_quarto-prod.yml", "profile:\n  default: other\n"),
        ],
        Some(&["prod"]),
    );
    let project = ok(result);
    let warnings = diags_with_code(&project.config.config_diagnostics, "Q-5-22");
    assert_eq!(warnings.len(), 1);
    let location = warnings[0]
        .location
        .as_ref()
        .expect("Q-5-22 must carry the overlay span");

    // Span integrity: the location's FileId must re-derive from the
    // overlay file's path — bind_config_source must pick the overlay,
    // not `_quarto.yml` (bd-m6wmztln discipline).
    let mut ctx = quarto_source_map::SourceContext::new();
    let candidates: Vec<&Path> = project
        .config
        .config_path
        .iter()
        .map(PathBuf::as_path)
        .chain(
            project
                .config
                .profile_config_paths
                .iter()
                .map(PathBuf::as_path),
        )
        .collect();
    let matched = quarto_core::config_sources::bind_config_source(&mut ctx, location, candidates);
    let overlay_path = tmp.path().canonicalize().unwrap().join("_quarto-prod.yml");
    assert_eq!(
        matched,
        Some(overlay_path.as_path()),
        "the diagnostic's FileId must bind to _quarto-prod.yml"
    );
}

#[test]
fn unknown_profile_warns_q_5_19() {
    let (result, _tmp) = discover_with(&[("_quarto.yml", "winner: base\n")], Some(&["produciton"]));
    let project = ok(result);
    let warnings = diags_with_code(&project.config.config_diagnostics, "Q-5-19");
    assert_eq!(
        warnings.len(),
        1,
        "got: {:?}",
        project.config.config_diagnostics
    );
    assert_eq!(warnings[0].kind, DiagnosticKind::Warning);
    let text = warnings[0].to_text(None);
    assert!(text.contains("produciton"), "must name the profile: {text}");
}

#[test]
fn declared_profile_without_files_does_not_warn() {
    // A profile that exists only for conditional content is declared
    // via profile.group (or default) to silence Q-5-19.
    let (result, _tmp) = discover_with(
        &[(
            "_quarto.yml",
            "winner: base\nprofile:\n  group: [basic, advanced]\n",
        )],
        Some(&["advanced"]),
    );
    let project = ok(result);
    assert!(
        diags_with_code(&project.config.config_diagnostics, "Q-5-19").is_empty(),
        "declared profiles must not warn: {:?}",
        project.config.config_diagnostics
    );
}

#[test]
fn environment_file_silences_q_5_19() {
    let (result, _tmp) = discover_with(
        &[
            ("_quarto.yml", "winner: base\n"),
            ("_environment-prod", "OMP_NUM_THREADS=16\n"),
        ],
        Some(&["prod"]),
    );
    let project = ok(result);
    assert!(
        diags_with_code(&project.config.config_diagnostics, "Q-5-19").is_empty(),
        "an _environment-<name> file makes the profile known: {:?}",
        project.config.config_diagnostics
    );
}

#[test]
fn matched_overlay_does_not_warn_q_5_19() {
    let (result, _tmp) = discover_with(
        &[
            ("_quarto.yml", "winner: base\n"),
            ("_quarto-prod.yml", "winner: prod\n"),
        ],
        Some(&["prod"]),
    );
    let project = ok(result);
    assert!(diags_with_code(&project.config.config_diagnostics, "Q-5-19").is_empty());
}

// ── hard errors ─────────────────────────────────────────────────────

#[test]
fn mixed_shape_group_aborts_discovery_with_span() {
    let (result, _tmp) = discover_with(
        &[("_quarto.yml", "profile:\n  group:\n    - a\n    - [b, c]\n")],
        None,
    );
    let pe = parse_error(result);
    assert!(
        pe.diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("Q-5-20")),
        "got: {:?}",
        pe.diagnostics
    );
    let text = pe.render();
    assert!(
        text.contains("_quarto.yml"),
        "the error must render a snippet from _quarto.yml: {text}"
    );
}

#[test]
fn invalid_profile_name_in_selection_aborts_discovery() {
    let (result, _tmp) = discover_with(&[("_quarto.yml", "winner: base\n")], Some(&["bad/name"]));
    let pe = parse_error(result);
    assert!(
        pe.diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("Q-5-21")),
        "got: {:?}",
        pe.diagnostics
    );
}

#[test]
fn overlay_yaml_parse_error_aborts_discovery() {
    let (result, _tmp) = discover_with(
        &[
            ("_quarto.yml", "winner: base\n"),
            ("_quarto-prod.yml", "winner: [unclosed\n"),
        ],
        Some(&["prod"]),
    );
    assert!(
        result.is_err(),
        "a malformed overlay must abort, matching Q1's loud failure"
    );
}

// ── bookkeeping ─────────────────────────────────────────────────────

use std::path::PathBuf;

#[test]
fn profile_config_paths_record_files_actually_read() {
    let (result, tmp) = discover_with(
        &[
            ("_quarto.yml", "winner: base\n"),
            ("_quarto-a.yml", "winner: a\n"),
            ("_quarto.yml.local", "winner: local\n"),
        ],
        // `b` has no overlay file: it must not appear in the paths.
        Some(&["a", "b"]),
    );
    let project = ok(result);
    let root = tmp.path().canonicalize().unwrap();
    assert_eq!(
        project.config.profile_config_paths,
        vec![root.join("_quarto-a.yml"), root.join("_quarto.yml.local")],
        "paths of the overlay + local files actually read, in merge order"
    );
    assert_eq!(active_names(&project), vec!["a", "b"]);
}
