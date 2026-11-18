/*
 * version_marker.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * The Quarto-version declaration in `_quarto.yml` (bd-gw8fmaz3).
 */

//! Does a project declare that it targets Quarto 2?
//!
//! Quarto 2 is not fully backwards compatible with Quarto 1, so a
//! project should say which major version it is written for. The
//! declaration is Quarto 1's own `quarto-required` key with a range
//! that admits Quarto 2 and excludes Quarto 1:
//!
//! ```yaml
//! # _quarto.yml
//! quarto-required: "^2"
//! ```
//!
//! Every Quarto 1 since 1.4 refuses to render such a project, because
//! its version does not satisfy the range. Released Quarto 1 versions
//! cannot change, so a `_quarto.yml` *without* the declaration has to be
//! presumed to belong to a Quarto 1 project; q2 warns about it.
//!
//! Only the project's own `_quarto.yml` (or `_quarto.yaml`) is read —
//! not profile overlays, `_quarto.yml.local`, `_metadata.yml`, document
//! front matter or `-M`. Whether a project targets Quarto 2 is a
//! property of the project, so it is a property of one file; and it
//! keeps `-M quarto-required=…` from becoming a habitual way to silence
//! the check. The value is captured at config-parse time as
//! [`ProjectConfig::declared_quarto_required`].
//!
//! Single-file renders and synthesized directory projects (no real
//! `_quarto.yml`) are never checked.
//!
//! Design and Quarto 1 survey:
//! `claude-notes/plans/2026-11-17-project-quarto-version-marker.md`.

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::ConfigValue;
use quarto_util::quarto_required::classify_quarto_required;

use super::ProjectConfig;

/// Q-5-53: an explicit `_quarto.yml` without `quarto-required`.
pub const CODE_MISSING: &str = "Q-5-53";
/// Q-5-54: a `quarto-required` range admitting both Quarto 1 and 2.
pub const CODE_SPANS_BOTH: &str = "Q-5-54";
/// Q-5-55: a `quarto-required` range admitting no Quarto 2 version.
pub const CODE_EXCLUDES_QUARTO2: &str = "Q-5-55";
/// Q-5-56: a `quarto-required` value that is not a version range.
pub const CODE_INVALID: &str = "Q-5-56";

/// The declaration q2 recommends; quoted because `^` and `>=` read
/// better quoted, and Quarto 1 requires a string.
const RECOMMENDED: &str = "quarto-required: \"^2\"";

/// The warnings, if any, about the project's Quarto-version
/// declaration. Empty for a project that targets Quarto 2 only, and for
/// configs that were not parsed from a real `_quarto.yml`.
pub fn version_marker_diagnostics(config: &ProjectConfig) -> Vec<DiagnosticMessage> {
    let Some(config_path) = &config.config_path else {
        return Vec::new();
    };
    let file_name = config_path
        .file_name()
        .map_or_else(|| "_quarto.yml".into(), |n| n.to_string_lossy());

    let Some(value) = &config.declared_quarto_required else {
        return vec![missing(&file_name)];
    };

    let Some(range) = value.as_plain_text() else {
        return vec![invalid(
            &file_name,
            "the value is not a string".to_string(),
            value,
        )];
    };

    match classify_quarto_required(&range) {
        Err(e) => vec![invalid(&file_name, e.reason, value)],
        Ok(coverage) if coverage.targets_quarto2_only() => Vec::new(),
        Ok(coverage) if coverage.quarto1 && coverage.quarto2 => {
            vec![spans_both(&file_name, &range, value)]
        }
        Ok(coverage) => vec![excludes_quarto2(
            &file_name,
            &range,
            coverage.quarto1,
            value,
        )],
    }
}

fn missing(file_name: &str) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning(format!("`{file_name}` does not declare a Quarto version"))
        .with_code(CODE_MISSING)
        .problem(format!(
            "Without `quarto-required`, `{file_name}` is presumed to belong to a Quarto 1 \
             project, and Quarto 1 will render it without complaint."
        ))
        .add_hint(format!(
            "If this project is meant for Quarto 2, add `{RECOMMENDED}` to `{file_name}`?"
        ))
        .build()
}

fn spans_both(file_name: &str, range: &str, value: &ConfigValue) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning(format!(
        "`quarto-required: \"{range}\"` admits both Quarto 1 and Quarto 2"
    ))
    .with_code(CODE_SPANS_BOTH)
    .problem(format!(
        "The range in `{file_name}` does not say which major version this project is \
         written for, so both Quarto 1 and Quarto 2 will render it."
    ))
    .add_hint(format!(
        "For a Quarto 2 project, write `{RECOMMENDED}`; for a Quarto 1 project, a range \
         below 2, such as `\"^1.7\"`?"
    ))
    .with_location(value.source_info.clone())
    .build()
}

fn excludes_quarto2(
    file_name: &str,
    range: &str,
    admits_quarto1: bool,
    value: &ConfigValue,
) -> DiagnosticMessage {
    let problem = if admits_quarto1 {
        format!(
            "The range in `{file_name}` declares a Quarto 1 project. Quarto 2 renders it \
             anyway, but the output may differ from what Quarto 1 produces."
        )
    } else {
        format!("The range in `{file_name}` admits neither Quarto 1 nor Quarto 2.")
    };
    DiagnosticMessageBuilder::warning(format!(
        "`quarto-required: \"{range}\"` does not admit Quarto 2"
    ))
    .with_code(CODE_EXCLUDES_QUARTO2)
    .problem(problem)
    .add_hint(format!(
        "If this project has been moved to Quarto 2, write `{RECOMMENDED}`?"
    ))
    .with_location(value.source_info.clone())
    .build()
}

fn invalid(file_name: &str, reason: String, value: &ConfigValue) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning("Invalid `quarto-required` value")
        .with_code(CODE_INVALID)
        .problem(format!(
            "`quarto-required` in `{file_name}` must be a version range string; {reason}."
        ))
        .add_hint(format!("For a Quarto 2 project, write `{RECOMMENDED}`?"))
        .with_location(value.source_info.clone())
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::ProjectContext;
    use quarto_system_runtime::NativeRuntime;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    /// A project directory holding `files` (relative path → contents)
    /// plus an `index.qmd`.
    fn project(files: &[(&str, &str)]) -> TempDir {
        let temp = TempDir::new().unwrap();
        for (name, content) in files {
            fs::write(temp.path().join(name), content).unwrap();
        }
        fs::write(temp.path().join("index.qmd"), "# Hello\n").unwrap();
        temp
    }

    fn diagnostics_for(dir: &Path, profile: Option<&[String]>) -> Vec<DiagnosticMessage> {
        let runtime = NativeRuntime::new();
        let ctx = ProjectContext::discover_with_profile(dir.join("index.qmd"), &runtime, profile)
            .unwrap();
        version_marker_diagnostics(&ctx.config)
    }

    fn codes(diagnostics: &[DiagnosticMessage]) -> Vec<&str> {
        diagnostics
            .iter()
            .map(|d| d.code.as_deref().unwrap_or("<none>"))
            .collect()
    }

    #[test]
    fn missing_marker_warns_q_5_53_without_a_location() {
        let temp = project(&[("_quarto.yml", "project:\n  type: default\n")]);
        let diagnostics = diagnostics_for(temp.path(), None);
        assert_eq!(codes(&diagnostics), ["Q-5-53"]);
        let d = &diagnostics[0];
        assert_eq!(d.title, "`_quarto.yml` does not declare a Quarto version");
        assert!(d.location.is_none());
        assert_eq!(
            d.hints[0].as_str(),
            "If this project is meant for Quarto 2, add `quarto-required: \"^2\"` to `_quarto.yml`?"
        );
    }

    #[test]
    fn a_config_without_a_project_key_is_still_checked() {
        let temp = project(&[("_quarto.yml", "title: Hello\n")]);
        assert_eq!(codes(&diagnostics_for(temp.path(), None)), ["Q-5-53"]);
    }

    #[test]
    fn the_yaml_spelling_is_named_in_the_message() {
        let temp = project(&[("_quarto.yaml", "title: Hello\n")]);
        let diagnostics = diagnostics_for(temp.path(), None);
        assert_eq!(
            diagnostics[0].title,
            "`_quarto.yaml` does not declare a Quarto version"
        );
    }

    #[test]
    fn a_quarto2_range_is_silent() {
        for range in ["\"^2\"", "\">=2\"", "^2", "\">=2.0.0 <3\""] {
            let temp = project(&[("_quarto.yml", &format!("quarto-required: {range}\n"))]);
            assert!(
                diagnostics_for(temp.path(), None).is_empty(),
                "{range} should be silent"
            );
        }
    }

    #[test]
    fn the_yaml_spelling_with_a_marker_is_silent() {
        let temp = project(&[("_quarto.yaml", "quarto-required: \"^2\"\n")]);
        assert!(diagnostics_for(temp.path(), None).is_empty());
    }

    #[test]
    fn a_range_spanning_both_majors_warns_q_5_54_at_the_value() {
        let temp = project(&[("_quarto.yml", "quarto-required: \">=1.4\"\n")]);
        let diagnostics = diagnostics_for(temp.path(), None);
        assert_eq!(codes(&diagnostics), ["Q-5-54"]);
        assert_eq!(
            diagnostics[0].title,
            "`quarto-required: \">=1.4\"` admits both Quarto 1 and Quarto 2"
        );
        assert!(diagnostics[0].location.is_some());
    }

    #[test]
    fn a_quarto1_range_warns_q_5_55() {
        let temp = project(&[("_quarto.yml", "quarto-required: \"~1.7\"\n")]);
        let diagnostics = diagnostics_for(temp.path(), None);
        assert_eq!(codes(&diagnostics), ["Q-5-55"]);
        assert_eq!(
            diagnostics[0].title,
            "`quarto-required: \"~1.7\"` does not admit Quarto 2"
        );
        assert!(diagnostics[0].location.is_some());
    }

    #[test]
    fn a_range_admitting_neither_major_warns_q_5_55() {
        let temp = project(&[("_quarto.yml", "quarto-required: \"^3\"\n")]);
        assert_eq!(codes(&diagnostics_for(temp.path(), None)), ["Q-5-55"]);
    }

    #[test]
    fn malformed_and_non_string_values_warn_q_5_56() {
        for value in ["\"foo\"", "2", "\"\"", "[\"^2\"]", "true"] {
            let temp = project(&[("_quarto.yml", &format!("quarto-required: {value}\n"))]);
            let diagnostics = diagnostics_for(temp.path(), None);
            assert_eq!(codes(&diagnostics), ["Q-5-56"], "{value}");
            assert!(diagnostics[0].location.is_some(), "{value}");
        }
    }

    #[test]
    fn a_marker_in_a_profile_overlay_does_not_count() {
        let temp = project(&[
            ("_quarto.yml", "title: Hello\n"),
            ("_quarto-q2.yml", "quarto-required: \"^2\"\n"),
        ]);
        let diagnostics = diagnostics_for(temp.path(), Some(&["q2".to_string()]));
        assert_eq!(codes(&diagnostics), ["Q-5-53"]);
    }

    #[test]
    fn a_marker_in_quarto_yml_local_does_not_count() {
        let temp = project(&[
            ("_quarto.yml", "title: Hello\n"),
            ("_quarto.yml.local", "quarto-required: \"^2\"\n"),
        ]);
        assert_eq!(codes(&diagnostics_for(temp.path(), None)), ["Q-5-53"]);
    }

    #[test]
    fn a_profile_overlay_cannot_override_the_base_marker() {
        let temp = project(&[
            ("_quarto.yml", "quarto-required: \"^2\"\n"),
            ("_quarto-old.yml", "quarto-required: \"^1.7\"\n"),
        ]);
        assert!(diagnostics_for(temp.path(), Some(&["old".to_string()])).is_empty());
    }

    #[test]
    fn a_marker_in_metadata_yml_does_not_count() {
        let temp = project(&[
            ("_quarto.yml", "title: Hello\n"),
            ("_metadata.yml", "quarto-required: \"^2\"\n"),
        ]);
        assert_eq!(codes(&diagnostics_for(temp.path(), None)), ["Q-5-53"]);
    }

    #[test]
    fn single_file_renders_are_not_checked() {
        let temp = TempDir::new().unwrap();
        let qmd = temp.path().join("doc.qmd");
        fs::write(&qmd, "# Hello\n").unwrap();
        let runtime = NativeRuntime::new();
        let ctx = ProjectContext::discover(&qmd, &runtime).unwrap();
        assert!(ctx.config.config_path.is_none());
        assert!(version_marker_diagnostics(&ctx.config).is_empty());
    }

    #[test]
    fn synthesized_directory_projects_are_not_checked() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("doc.qmd"), "# Hello\n").unwrap();
        let runtime = NativeRuntime::new();
        let ctx = ProjectContext::discover(temp.path(), &runtime).unwrap();
        assert!(ctx.config.config_path.is_none());
        assert!(version_marker_diagnostics(&ctx.config).is_empty());
    }

    #[test]
    fn codes_are_registered_in_catalog() {
        for code in [
            CODE_MISSING,
            CODE_SPANS_BOTH,
            CODE_EXCLUDES_QUARTO2,
            CODE_INVALID,
        ] {
            assert!(
                quarto_error_catalog::ERROR_CATALOG.get(code).is_some(),
                "{code} must be registered in the quarto-error-catalog"
            );
        }
    }

    #[test]
    fn default_configs_are_not_checked() {
        assert!(version_marker_diagnostics(&ProjectConfig::default()).is_empty());
    }
}
