//! Phase 0 test #9b — `(cli, yaml) → resolved` mode resolution matrix.
//!
//! Pure unit test on the public resolver function so "silent override
//! on CLI/YAML conflict" can't regress. Plus a small `RenderContext`
//! integration assertion that resolved-`Off`/`None` never installs a
//! `GitBlameProvider`.
//!
//! The E2E CLI test (Phase 0 test #9) lives in
//! `crates/quarto/tests/attribution_cli_e2e.rs` because it drives the
//! `q2` binary.

use quarto_core::Format;
use quarto_core::attribution::mode::{AttributionMode, resolve_attribution_mode};
use quarto_core::project::{DocumentInfo, ProjectConfig, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};

// ===========================================================================
// Pure resolution function — all eight cases.
// ===========================================================================

#[test]
fn resolve_attribution_mode_returns_none_when_both_absent() {
    assert_eq!(resolve_attribution_mode(None, None), None);
}

#[test]
fn resolve_attribution_mode_yaml_off_with_no_cli() {
    assert_eq!(
        resolve_attribution_mode(None, Some(AttributionMode::Off)),
        Some(AttributionMode::Off)
    );
}

#[test]
fn resolve_attribution_mode_yaml_git_with_no_cli() {
    assert_eq!(
        resolve_attribution_mode(None, Some(AttributionMode::Git)),
        Some(AttributionMode::Git)
    );
}

#[test]
fn resolve_attribution_mode_cli_off_with_no_yaml() {
    assert_eq!(
        resolve_attribution_mode(Some(AttributionMode::Off), None),
        Some(AttributionMode::Off)
    );
}

/// The escape-hatch case the prior review explicitly called out:
/// `--attribution=off` on the CLI must win over `attribution: git`
/// in project YAML.
#[test]
fn resolve_attribution_mode_cli_off_beats_yaml_git() {
    assert_eq!(
        resolve_attribution_mode(Some(AttributionMode::Off), Some(AttributionMode::Git)),
        Some(AttributionMode::Off),
        "CLI `--attribution=off` is the escape-hatch override"
    );
}

#[test]
fn resolve_attribution_mode_cli_git_with_no_yaml() {
    assert_eq!(
        resolve_attribution_mode(Some(AttributionMode::Git), None),
        Some(AttributionMode::Git)
    );
}

#[test]
fn resolve_attribution_mode_cli_git_beats_yaml_off() {
    assert_eq!(
        resolve_attribution_mode(Some(AttributionMode::Git), Some(AttributionMode::Off)),
        Some(AttributionMode::Git),
        "symmetric case: CLI overrides YAML in both directions"
    );
}

#[test]
fn resolve_attribution_mode_cli_git_yaml_git_trivial_agreement() {
    assert_eq!(
        resolve_attribution_mode(Some(AttributionMode::Git), Some(AttributionMode::Git)),
        Some(AttributionMode::Git)
    );
}

// ===========================================================================
// Integration: resolved `Off`/`None` must NOT install a GitBlameProvider.
// ===========================================================================

#[test]
fn render_context_default_has_no_attribution_provider() {
    let dir = std::env::temp_dir().join("attribution-cli-#9b");
    let project = ProjectContext {
        dir: dir.clone(),
        config: ProjectConfig::default(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path(dir.join("test.qmd"))],
        output_dir: dir.clone(),
    };
    let doc = DocumentInfo::from_path(dir.join("test.qmd"));
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let ctx = RenderContext::new(&project, &doc, &format, &binaries);

    assert!(
        ctx.attribution_provider.is_none(),
        "unflagged default: no provider installed"
    );
    assert!(
        ctx.attribution_data.is_none(),
        "unflagged default: sidecar empty"
    );
}
