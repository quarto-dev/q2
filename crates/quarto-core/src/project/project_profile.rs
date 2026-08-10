/*
 * project_profile.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Project profiles: activation resolution (bd-fu16z22k).
 */

//! Project profiles — activation resolution and `profile:` config
//! extraction (bd-fu16z22k).
//!
//! ⚠️ **Terminology**: this module implements *project profiles* (the
//! Quarto 1 feature: `--profile`, `QUARTO_PROFILE`,
//! `_quarto-<name>.yml` overlays). It is unrelated to
//! [`crate::document_profile::DocumentProfile`], the pass-1 document
//! summary, or to its cache under the `"profiles"` namespace. Code in
//! this module never uses a bare `profiles` identifier for the active
//! set — it is always `active_config_profiles` or `ActiveProfile`.
//!
//! This module is the pure core: profile-string parsing, strict name
//! validation, `profile:` key extraction (with stripping), and the
//! activation-precedence algorithm. It performs no I/O; discovery of
//! `_quarto-<name>.yml` overlay files and their merging live in
//! [`super::ProjectContext::parse_config`] (Phase 1 of the plan at
//! `claude-notes/plans/2026-08-10-project-profiles-port.md`).
//!
//! # Quarto 1 semantics ported here
//!
//! The activation-precedence chain (first non-empty source wins):
//! 1. `--profile` CLI values ([`ProfileSource::CliArg`]) — *replaces*
//!    `QUARTO_PROFILE`, never merges with it (Q1 parity);
//! 2. the `QUARTO_PROFILE` environment variable;
//! 3. `QUARTO_PROFILE` defined in `_environment.local` /
//!    `_environment` (the "dotenv bootstrap"; wired in Phase 3);
//! 4. `profile.default` in `_quarto.yml.local`;
//! 5. `profile.default` in `_quarto.yml`.
//!
//! Then group expansion runs regardless of which source won: for each
//! group in `profile.group` (honored from `_quarto.yml` only), if no
//! member is active, the group's **first** member is appended. Group
//! defaults come after explicit selections, giving them lower overlay
//! precedence ("first-listed wins" — see the plan's precedence
//! decision).
//!
//! # Deliberate divergences from Quarto 1 (strictness)
//!
//! Q1 silently tolerates malformed input in this area; Q2 diagnoses
//! it (see the plan's divergence table):
//! - profile names are validated against
//!   [`is_valid_profile_name`] (Q-5-21 error);
//! - a fully-empty explicit selection (`--profile ""`,
//!   `QUARTO_PROFILE=" , "`) is a Q-5-21 error instead of silently
//!   meaning "no profiles" (an *unset or empty-string* env var is
//!   still "no selection", so `QUARTO_PROFILE= q2 render` unsets);
//! - shape errors under `profile:` (mixed-shape `group`, non-string
//!   entries, unknown keys, non-map value) are Q-5-20 errors instead
//!   of being silently ignored;
//! - `profile.group` in `_quarto.yml.local` and any `profile:` key in
//!   a `_quarto-<name>.yml` overlay are inert in Q1; Q2 warns
//!   (Q-5-22) and strips them;
//! - duplicate names are dropped (first occurrence wins) instead of
//!   being processed twice.

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::{ConfigValue, ConfigValueKind};
use quarto_source_map::SourceInfo;

/// The environment variable holding the active project-profile list
/// (comma/space-separated). Read at activation time; exported to
/// child processes (engines, render scripts) in Phase 2 — never
/// written into this process's environment.
pub const QUARTO_PROFILE_VAR: &str = "QUARTO_PROFILE";

/// Typed contents of a `profile:` config key after strict validation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectProfileConfig {
    /// `profile.default`: profiles activated when no higher-priority
    /// source selects any. A bare string normalizes to one element.
    pub default: Vec<String>,
    /// `profile.group`: groups of mutually-exclusive profiles; at
    /// least one member of each group is always active (the first
    /// member is the group's default). A flat list of strings
    /// normalizes to a single group.
    pub groups: Vec<Vec<String>>,
}

/// Where a `profile:` key was found, which controls what is honored
/// and what is diagnosed by [`extract_profile_config`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileKeySite {
    /// `_quarto.yml`: both `default` and `group` are honored.
    BaseConfig,
    /// `_quarto.yml.local`: only `default` is honored; a `group` key
    /// draws a Q-5-22 warning (Q1 reads groups from the base config
    /// only).
    LocalConfig,
    /// A `_quarto-<name>.yml` overlay: the whole `profile:` key is
    /// inert (no recursion in Q1) — Q-5-22 warning, nothing honored.
    Overlay,
}

/// Which activation source put a profile into the active set.
/// Recorded for the `-v` echo and for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileSource {
    /// `--profile` on the command line.
    CliArg,
    /// The `QUARTO_PROFILE` environment variable.
    EnvVar,
    /// `QUARTO_PROFILE` defined in `_environment` /
    /// `_environment.local` (dotenv bootstrap; Phase 3).
    EnvironmentFile,
    /// `profile.default` in `_quarto.yml.local`.
    LocalConfigDefault,
    /// `profile.default` in `_quarto.yml`.
    ConfigDefault,
    /// Appended by group expansion (`profile.group` first member).
    GroupDefault,
}

impl ProfileSource {
    /// Human-readable origin for the `-v` echo and diagnostics.
    pub fn describe(self) -> &'static str {
        match self {
            ProfileSource::CliArg => "--profile",
            ProfileSource::EnvVar => "QUARTO_PROFILE",
            ProfileSource::EnvironmentFile => "QUARTO_PROFILE (from environment file)",
            ProfileSource::LocalConfigDefault => "profile.default (_quarto.yml.local)",
            ProfileSource::ConfigDefault => "profile.default (_quarto.yml)",
            ProfileSource::GroupDefault => "profile.group default",
        }
    }
}

/// One active project profile with its activation provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveProfile {
    pub name: String,
    pub source: ProfileSource,
}

/// Inputs to [`resolve_active_profiles`], in precedence order.
#[derive(Debug)]
pub struct ProfileResolutionInputs<'a> {
    /// `--profile` values (each may itself be a comma/space-separated
    /// list). `Some` means the flag was given, even with no usable
    /// names — an explicitly-empty selection is an error, not a
    /// fall-through.
    pub cli: Option<&'a [String]>,
    /// The real `QUARTO_PROFILE` environment variable. An empty
    /// string is treated as unset.
    pub env_var: Option<&'a str>,
    /// `QUARTO_PROFILE` from `_environment.local` / `_environment`
    /// (Phase 3 wires this; until then callers pass `None`).
    pub env_file: Option<&'a str>,
    /// `profile.default` extracted from `_quarto.yml.local`.
    pub local_default: &'a [String],
    /// The `profile:` config from `_quarto.yml`.
    pub config: &'a ProjectProfileConfig,
}

impl Default for ProfileResolutionInputs<'_> {
    fn default() -> Self {
        static EMPTY: ProjectProfileConfig = ProjectProfileConfig {
            default: Vec::new(),
            groups: Vec::new(),
        };
        Self {
            cli: None,
            env_var: None,
            env_file: None,
            local_default: &[],
            config: &EMPTY,
        }
    }
}

/// Split a `QUARTO_PROFILE`-style string on commas and/or spaces
/// (Q1's `/[ ,]+/`), dropping empty segments and duplicate names
/// (first occurrence wins). Colons are **not** separators — Q1
/// parity; a `a:b` segment survives as one (invalid) name for
/// [`is_valid_profile_name`] to reject with a targeted hint.
pub fn parse_profile_string(s: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for token in s.split([' ', ',']) {
        if token.is_empty() {
            continue;
        }
        if !names.iter().any(|n| n == token) {
            names.push(token.to_string());
        }
    }
    names
}

/// Strict profile-name check: `[A-Za-z0-9][A-Za-z0-9._-]*`
/// (filename-safe, no leading `.`, no whitespace, ASCII-only).
/// Decided 2026-08-10; see the plan's divergence table.
pub fn is_valid_profile_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphanumeric()
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Extract — and **strip** — the top-level `profile:` key from a
/// parsed project config, validating strictly.
///
/// The key is removed from `metadata` even when malformed, so no
/// downstream consumer (metadata merge, writers) ever sees it — Q1
/// deletes it from the base config too (`initializeProfileConfig`).
/// `file_label` names the file in diagnostics (e.g. `_quarto.yml`).
///
/// Diagnostics: Q-5-20 (shape), Q-5-21 (names), Q-5-22 (key at a
/// site where it is inert). Error-severity diagnostics mean the
/// returned config omits the offending entries; the caller decides
/// whether to abort (parse_config does).
pub fn extract_profile_config(
    metadata: &mut ConfigValue,
    site: ProfileKeySite,
    file_label: &str,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> ProjectProfileConfig {
    let Some(entry) = take_top_level_entry(metadata, "profile") else {
        return ProjectProfileConfig::default();
    };

    if site == ProfileKeySite::Overlay {
        diagnostics.push(
            DiagnosticMessageBuilder::warning("`profile:` has no effect in a profile overlay")
                .with_code("Q-5-22")
                .problem(format!(
                    "The `profile:` key in `{file_label}` is ignored. Profile overlay \
                     files never contribute profile configuration — `profile.default` \
                     and `profile.group` are read from `_quarto.yml` (and \
                     `profile.default` from `_quarto.yml.local`)."
                ))
                .with_location(entry.key_source)
                .build(),
        );
        return ProjectProfileConfig::default();
    }

    let ConfigValueKind::Map(entries) = &entry.value.value else {
        diagnostics.push(
            DiagnosticMessageBuilder::error("`profile:` must be a mapping")
                .with_code("Q-5-20")
                .problem(format!(
                    "The `profile:` key in `{file_label}` must be a mapping with the \
                     keys `default` and/or `group`, not a bare value."
                ))
                .add_hint(
                    "To set the profiles used when none are requested, write \
                     `profile:` / `  default: <name>`.",
                )
                .with_location(entry.value.source_info)
                .build(),
        );
        return ProjectProfileConfig::default();
    };

    let mut config = ProjectProfileConfig::default();
    for e in entries {
        match e.key.as_str() {
            "default" => {
                config.default = extract_default_names(&e.value, file_label, diagnostics);
            }
            "group" => {
                if site == ProfileKeySite::LocalConfig {
                    diagnostics.push(
                        DiagnosticMessageBuilder::warning(
                            "`profile.group` has no effect in `_quarto.yml.local`",
                        )
                        .with_code("Q-5-22")
                        .problem(format!(
                            "The `profile.group` key in `{file_label}` is ignored: \
                             profile groups are read from `_quarto.yml` only; \
                             `{file_label}` contributes only `profile.default`."
                        ))
                        .with_location(e.key_source.clone())
                        .build(),
                    );
                } else {
                    config.groups = extract_groups(&e.value, file_label, diagnostics);
                }
            }
            unknown => {
                diagnostics.push(
                    DiagnosticMessageBuilder::error(format!(
                        "Unknown key `{unknown}` under `profile:`"
                    ))
                    .with_code("Q-5-20")
                    .problem(format!(
                        "`profile.{unknown}` in `{file_label}` is not a recognized \
                         key. The `profile:` mapping accepts only `default` and \
                         `group`."
                    ))
                    .with_location(e.key_source.clone())
                    .build(),
                );
            }
        }
    }
    config
}

/// Remove and return the top-level entry named `key` from a map-shaped
/// [`ConfigValue`]. Returns `None` when `metadata` is not a map or has
/// no such entry.
fn take_top_level_entry(
    metadata: &mut ConfigValue,
    key: &str,
) -> Option<quarto_pandoc_types::ConfigMapEntry> {
    let ConfigValueKind::Map(entries) = &mut metadata.value else {
        return None;
    };
    let idx = entries.iter().position(|e| e.key == key)?;
    Some(entries.remove(idx))
}

/// Parse `profile.default`: a string scalar or a list of strings,
/// each strictly validated. Malformed entries are dropped with a
/// diagnostic; valid entries are kept (the error severity aborts the
/// render upstream regardless).
fn extract_default_names(
    value: &ConfigValue,
    file_label: &str,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Vec<String> {
    let scalars: Vec<&ConfigValue> = if let Some(arr) = value.as_array() {
        arr.iter().collect()
    } else {
        vec![value]
    };
    let mut names = Vec::new();
    for scalar in scalars {
        let Some(name) = scalar.as_str() else {
            diagnostics.push(
                DiagnosticMessageBuilder::error("`profile.default` entries must be strings")
                    .with_code("Q-5-20")
                    .problem(format!(
                        "`profile.default` in `{file_label}` must be a profile name \
                         or a list of profile names; this entry is not a string."
                    ))
                    .with_location(scalar.source_info.clone())
                    .build(),
            );
            continue;
        };
        if validate_profile_name_diagnosed(
            name,
            Some(scalar.source_info.clone()),
            &format!("`profile.default` in `{file_label}`"),
            diagnostics,
        ) && !names.iter().any(|n| n == name)
        {
            names.push(name.to_string());
        }
    }
    names
}

/// Parse `profile.group`: a flat list of strings (one group) or a
/// list of lists of strings (many groups). A mixed-shape list is a
/// Q-5-20 error yielding no groups (Q1 silently yields none); an
/// empty group or a group with an invalid member is dropped with a
/// diagnostic while other groups survive.
fn extract_groups(
    value: &ConfigValue,
    file_label: &str,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Vec<Vec<String>> {
    let Some(items) = value.as_array() else {
        diagnostics.push(
            DiagnosticMessageBuilder::error("`profile.group` must be a list")
                .with_code("Q-5-20")
                .problem(format!(
                    "`profile.group` in `{file_label}` must be a list of profile \
                     names (one group) or a list of such lists (several groups)."
                ))
                .with_location(value.source_info.clone())
                .build(),
        );
        return Vec::new();
    };

    let all_scalar = items.iter().all(|i| !i.is_array());
    let all_lists = items.iter().all(|i| i.is_array());
    if !all_scalar && !all_lists {
        diagnostics.push(
            DiagnosticMessageBuilder::error("`profile.group` mixes names and lists")
                .with_code("Q-5-20")
                .problem(format!(
                    "`profile.group` in `{file_label}` mixes bare profile names with \
                     lists. Write either one flat list of names (a single group) or \
                     a list of lists (one per group)."
                ))
                .with_location(value.source_info.clone())
                .build(),
        );
        return Vec::new();
    }

    let group_values: Vec<&ConfigValue> = if all_lists {
        items.iter().collect()
    } else {
        vec![value]
    };

    let mut groups = Vec::new();
    for group_value in group_values {
        let members = group_value.as_array().unwrap_or_default();
        if members.is_empty() {
            diagnostics.push(
                DiagnosticMessageBuilder::error("Empty profile group")
                    .with_code("Q-5-20")
                    .problem(format!(
                        "A group in `profile.group` in `{file_label}` is empty. Each \
                         group needs at least one profile name — the first member is \
                         the group's default."
                    ))
                    .with_location(group_value.source_info.clone())
                    .build(),
            );
            continue;
        }
        let mut names = Vec::new();
        let mut valid = true;
        for member in members {
            let Some(name) = member.as_str() else {
                diagnostics.push(
                    DiagnosticMessageBuilder::error("`profile.group` members must be strings")
                        .with_code("Q-5-20")
                        .problem(format!(
                            "A group member in `profile.group` in `{file_label}` is \
                             not a string."
                        ))
                        .with_location(member.source_info.clone())
                        .build(),
                );
                valid = false;
                continue;
            };
            if !validate_profile_name_diagnosed(
                name,
                Some(member.source_info.clone()),
                &format!("`profile.group` in `{file_label}`"),
                diagnostics,
            ) {
                valid = false;
                continue;
            }
            names.push(name.to_string());
        }
        // A group with any invalid member is dropped wholesale: its
        // first-member-default semantics can't be trusted anymore.
        if valid {
            groups.push(names);
        }
    }
    groups
}

/// Validate one profile name, emitting a Q-5-21 error when invalid.
/// Returns whether the name is valid.
fn validate_profile_name_diagnosed(
    name: &str,
    span: Option<SourceInfo>,
    origin: &str,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> bool {
    if is_valid_profile_name(name) {
        return true;
    }
    let mut builder = DiagnosticMessageBuilder::error("Invalid project profile name")
        .with_code("Q-5-21")
        .problem(format!(
            "`{name}` (from {origin}) is not a valid profile name. Profile names \
             must start with an ASCII letter or digit and contain only ASCII \
             letters, digits, `.`, `_`, and `-` — they name files such as \
             `_quarto-<name>.yml`."
        ));
    if name.contains(':') {
        builder = builder.add_hint(
            "Profiles are separated by commas (for example `a,b`), not colons — \
             was this meant as a list?",
        );
    }
    if let Some(span) = span {
        builder = builder.with_location(span);
    }
    diagnostics.push(builder.build());
    false
}

/// Resolve the active project-profile set from all activation
/// sources. Pure: no I/O, no process-environment reads — callers
/// supply every input ([`ProfileResolutionInputs`]).
///
/// Returns profiles in **activation order** (explicit selections
/// first, group defaults appended). Overlay merging must give the
/// first-listed profile the highest precedence among profiles.
pub fn resolve_active_profiles(
    inputs: &ProfileResolutionInputs,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Vec<ActiveProfile> {
    // Explicit selection: the first source that applies wins outright
    // (Q1 parity: `--profile` *replaces* `QUARTO_PROFILE`, etc.). A
    // source that was explicitly given but yields no usable names
    // still counts as "applied" — with a Q-5-21 error — so a broken
    // selection never silently falls through to a different one.
    let explicit: Vec<ActiveProfile> = if let Some(cli) = inputs.cli {
        parse_explicit_source(&cli.join(","), ProfileSource::CliArg, diagnostics)
    } else if let Some(env) = nonempty(inputs.env_var) {
        parse_explicit_source(env, ProfileSource::EnvVar, diagnostics)
    } else if let Some(env_file) = nonempty(inputs.env_file) {
        parse_explicit_source(env_file, ProfileSource::EnvironmentFile, diagnostics)
    } else if !inputs.local_default.is_empty() {
        named(inputs.local_default, ProfileSource::LocalConfigDefault)
    } else if !inputs.config.default.is_empty() {
        named(&inputs.config.default, ProfileSource::ConfigDefault)
    } else {
        Vec::new()
    };

    // Group expansion (groups come pre-validated and non-empty from
    // extract_profile_config): every group must have an active
    // member; otherwise its first member is appended *after* the
    // explicit selection, giving it lower overlay precedence under
    // first-listed-wins.
    let mut active = explicit;
    for group in &inputs.config.groups {
        if !group
            .iter()
            .any(|member| active.iter().any(|a| &a.name == member))
        {
            active.push(ActiveProfile {
                name: group[0].clone(),
                source: ProfileSource::GroupDefault,
            });
        }
    }
    active
}

/// Treat an empty string as an unset variable (`QUARTO_PROFILE= q2
/// render` unsets), unlike a separator-only string, which is
/// "set but empty" and an error in [`parse_explicit_source`].
fn nonempty(s: Option<&str>) -> Option<&str> {
    s.filter(|s| !s.is_empty())
}

/// Parse one explicit selection string (CLI or environment), validate
/// each name (Q-5-21, span-less — these names were not written in
/// YAML), and error when the selection was given but names out empty.
fn parse_explicit_source(
    raw: &str,
    source: ProfileSource,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Vec<ActiveProfile> {
    let names = parse_profile_string(raw);
    if names.is_empty() {
        diagnostics.push(
            DiagnosticMessageBuilder::error("Empty project profile selection")
                .with_code("Q-5-21")
                .problem(format!(
                    "{} was given but contains no profile names. To render with \
                     no profiles active, omit it entirely.",
                    source.describe()
                ))
                .build(),
        );
        return Vec::new();
    }
    names
        .into_iter()
        .filter(|name| validate_profile_name_diagnosed(name, None, source.describe(), diagnostics))
        .map(|name| ActiveProfile { name, source })
        .collect()
}

/// Wrap already-validated default names with their source.
fn named(names: &[String], source: ProfileSource) -> Vec<ActiveProfile> {
    names
        .iter()
        .map(|name| ActiveProfile {
            name: name.clone(),
            source,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_error_reporting::DiagnosticKind;

    fn config_value_from_yaml(yaml: &str) -> ConfigValue {
        use pampa::pandoc::yaml_to_config_value;
        use pampa::utils::diagnostic_collector::DiagnosticCollector;
        use quarto_config::InterpretationContext;
        let parsed = quarto_yaml::parse_file(yaml, "_quarto.yml").expect("valid yaml");
        let mut diagnostics = DiagnosticCollector::new();
        yaml_to_config_value(
            parsed,
            InterpretationContext::ProjectConfig,
            &mut diagnostics,
        )
    }

    fn names(active: &[ActiveProfile]) -> Vec<&str> {
        active.iter().map(|p| p.name.as_str()).collect()
    }

    fn errors(diags: &[DiagnosticMessage]) -> Vec<&DiagnosticMessage> {
        diags
            .iter()
            .filter(|d| d.kind == DiagnosticKind::Error)
            .collect()
    }

    fn codes(diags: &[DiagnosticMessage]) -> Vec<String> {
        diags
            .iter()
            .filter_map(|d| d.code.clone())
            .collect::<Vec<_>>()
    }

    // ── parse_profile_string ────────────────────────────────────────

    #[test]
    fn parse_splits_on_commas() {
        assert_eq!(parse_profile_string("a,b"), vec!["a", "b"]);
    }

    #[test]
    fn parse_splits_on_spaces() {
        // Q1 parity: /[ ,]+/ — spaces separate too.
        assert_eq!(parse_profile_string("a b"), vec!["a", "b"]);
    }

    #[test]
    fn parse_collapses_separator_runs_and_trims_edges() {
        // Q1 returns ["", "a", "b"] for " a,b" (a real bug we fix):
        // edge separators must not produce empty names.
        assert_eq!(parse_profile_string(" a,, b , c "), vec!["a", "b", "c"]);
    }

    #[test]
    fn parse_colon_is_not_a_separator() {
        // Q1 parity: "a:b" is one (about-to-be-rejected) name, not two.
        assert_eq!(parse_profile_string("a:b"), vec!["a:b"]);
    }

    #[test]
    fn parse_empty_and_separator_only_yield_no_names() {
        assert!(parse_profile_string("").is_empty());
        assert!(parse_profile_string(" , ,").is_empty());
    }

    #[test]
    fn parse_drops_duplicates_first_occurrence_wins() {
        // Divergence from Q1 (which merges `_quarto-a.yml` twice for
        // "a,b,a"): duplicates are dropped, order of first
        // occurrence preserved.
        assert_eq!(parse_profile_string("a,b,a"), vec!["a", "b"]);
    }

    // ── is_valid_profile_name ───────────────────────────────────────

    #[test]
    fn valid_names_accepted() {
        for name in [
            "production",
            "dev2",
            "a",
            "advanced-docs",
            "v1.2",
            "a_b",
            "A",
        ] {
            assert!(is_valid_profile_name(name), "{name:?} must be valid");
        }
    }

    #[test]
    fn invalid_names_rejected() {
        for name in [
            "",        // empty
            ".hidden", // leading dot (dotfile)
            "-x",      // leading dash (option-like)
            "_x",      // leading underscore (first char must be alnum)
            "a/b",     // path separator
            "a\\b",    // path separator (windows)
            "a:b",     // colon (Q1 users expecting a separator)
            "a b",     // whitespace (unreachable via parse, reachable via YAML)
            "café",    // non-ASCII
        ] {
            assert!(!is_valid_profile_name(name), "{name:?} must be invalid");
        }
    }

    // ── extract_profile_config: base config ─────────────────────────

    #[test]
    fn extract_absent_key_is_default_and_silent() {
        let mut meta = config_value_from_yaml("project:\n  type: website\n");
        let mut diags = Vec::new();
        let config = extract_profile_config(
            &mut meta,
            ProfileKeySite::BaseConfig,
            "_quarto.yml",
            &mut diags,
        );
        assert_eq!(config, ProjectProfileConfig::default());
        assert!(diags.is_empty());
        assert!(meta.get("project").is_some(), "other keys untouched");
    }

    #[test]
    fn extract_string_default_normalizes_to_one_element() {
        let mut meta = config_value_from_yaml("profile:\n  default: dev\n");
        let mut diags = Vec::new();
        let config = extract_profile_config(
            &mut meta,
            ProfileKeySite::BaseConfig,
            "_quarto.yml",
            &mut diags,
        );
        assert_eq!(config.default, vec!["dev"]);
        assert!(config.groups.is_empty());
        assert!(diags.is_empty());
    }

    #[test]
    fn extract_list_default_preserves_order() {
        let mut meta = config_value_from_yaml("profile:\n  default: [advanced, production]\n");
        let mut diags = Vec::new();
        let config = extract_profile_config(
            &mut meta,
            ProfileKeySite::BaseConfig,
            "_quarto.yml",
            &mut diags,
        );
        assert_eq!(config.default, vec!["advanced", "production"]);
        assert!(diags.is_empty());
    }

    #[test]
    fn extract_strips_profile_key_from_metadata() {
        let mut meta =
            config_value_from_yaml("project:\n  type: website\nprofile:\n  default: dev\n");
        let mut diags = Vec::new();
        extract_profile_config(
            &mut meta,
            ProfileKeySite::BaseConfig,
            "_quarto.yml",
            &mut diags,
        );
        assert!(
            meta.get("profile").is_none(),
            "profile: must be stripped so downstream consumers never see it"
        );
        assert!(meta.get("project").is_some(), "other keys survive");
    }

    #[test]
    fn extract_flat_group_is_single_group() {
        let mut meta = config_value_from_yaml("profile:\n  group: [basic, advanced]\n");
        let mut diags = Vec::new();
        let config = extract_profile_config(
            &mut meta,
            ProfileKeySite::BaseConfig,
            "_quarto.yml",
            &mut diags,
        );
        assert_eq!(config.groups, vec![vec!["basic", "advanced"]]);
        assert!(diags.is_empty());
    }

    #[test]
    fn extract_nested_groups() {
        let mut meta = config_value_from_yaml("profile:\n  group:\n    - [a, b]\n    - [c, d]\n");
        let mut diags = Vec::new();
        let config = extract_profile_config(
            &mut meta,
            ProfileKeySite::BaseConfig,
            "_quarto.yml",
            &mut diags,
        );
        assert_eq!(config.groups, vec![vec!["a", "b"], vec!["c", "d"]]);
        assert!(diags.is_empty());
    }

    #[test]
    fn extract_mixed_shape_group_is_q_5_20_error_with_span() {
        // Q1 silently yields ZERO groups for [a, [b, c]]; we error.
        let mut meta = config_value_from_yaml("profile:\n  group:\n    - a\n    - [b, c]\n");
        let mut diags = Vec::new();
        let config = extract_profile_config(
            &mut meta,
            ProfileKeySite::BaseConfig,
            "_quarto.yml",
            &mut diags,
        );
        assert!(config.groups.is_empty());
        let errs = errors(&diags);
        assert_eq!(errs.len(), 1, "got: {:?}", codes(&diags));
        assert_eq!(errs[0].code.as_deref(), Some("Q-5-20"));
        assert!(
            errs[0].location.is_some(),
            "shape errors must carry the YAML span"
        );
    }

    #[test]
    fn extract_unknown_key_under_profile_is_q_5_20_error() {
        // Q1 has a closed schema here; Q2 has no schema layer, so
        // this closed-object check is explicit.
        let mut meta = config_value_from_yaml("profile:\n  default: dev\n  defualt: prod\n");
        let mut diags = Vec::new();
        let config = extract_profile_config(
            &mut meta,
            ProfileKeySite::BaseConfig,
            "_quarto.yml",
            &mut diags,
        );
        assert_eq!(config.default, vec!["dev"], "valid keys still honored");
        let errs = errors(&diags);
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_deref(), Some("Q-5-20"));
        let text = errs[0].to_text(None);
        assert!(
            text.contains("defualt"),
            "must name the unknown key: {text}"
        );
        assert!(errs[0].location.is_some());
    }

    #[test]
    fn extract_non_string_default_entry_is_q_5_20_error() {
        let mut meta = config_value_from_yaml("profile:\n  default: [1, dev]\n");
        let mut diags = Vec::new();
        let config = extract_profile_config(
            &mut meta,
            ProfileKeySite::BaseConfig,
            "_quarto.yml",
            &mut diags,
        );
        // The malformed entry is dropped, the valid one kept; the
        // error aborts the render upstream anyway.
        assert_eq!(config.default, vec!["dev"]);
        let errs = errors(&diags);
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_deref(), Some("Q-5-20"));
        assert!(errs[0].location.is_some());
    }

    #[test]
    fn extract_non_map_profile_value_is_q_5_20_error_and_stripped() {
        // Q1: `ld.isObject` fails → silently ignored. We error.
        let mut meta = config_value_from_yaml("profile: dev\n");
        let mut diags = Vec::new();
        let config = extract_profile_config(
            &mut meta,
            ProfileKeySite::BaseConfig,
            "_quarto.yml",
            &mut diags,
        );
        assert_eq!(config, ProjectProfileConfig::default());
        let errs = errors(&diags);
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_deref(), Some("Q-5-20"));
        assert!(
            meta.get("profile").is_none(),
            "stripped even when malformed"
        );
    }

    #[test]
    fn extract_invalid_name_in_default_is_q_5_21_error_with_span() {
        let mut meta = config_value_from_yaml("profile:\n  default: bad/name\n");
        let mut diags = Vec::new();
        let config = extract_profile_config(
            &mut meta,
            ProfileKeySite::BaseConfig,
            "_quarto.yml",
            &mut diags,
        );
        assert!(config.default.is_empty());
        let errs = errors(&diags);
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_deref(), Some("Q-5-21"));
        assert!(errs[0].location.is_some());
    }

    #[test]
    fn extract_invalid_name_in_group_is_q_5_21_error() {
        let mut meta = config_value_from_yaml("profile:\n  group: [ok, .bad]\n");
        let mut diags = Vec::new();
        let config = extract_profile_config(
            &mut meta,
            ProfileKeySite::BaseConfig,
            "_quarto.yml",
            &mut diags,
        );
        // A group with an invalid member is dropped wholesale: its
        // first-member-default semantics can't be trusted anymore.
        assert!(config.groups.is_empty());
        assert_eq!(
            codes(&errors(&diags).into_iter().cloned().collect::<Vec<_>>()),
            vec!["Q-5-21"]
        );
    }

    #[test]
    fn extract_empty_group_is_q_5_20_error() {
        // An empty group has no first member to use as default.
        let mut meta = config_value_from_yaml("profile:\n  group:\n    - []\n    - [a, b]\n");
        let mut diags = Vec::new();
        let config = extract_profile_config(
            &mut meta,
            ProfileKeySite::BaseConfig,
            "_quarto.yml",
            &mut diags,
        );
        assert_eq!(config.groups, vec![vec!["a", "b"]], "valid group kept");
        let errs = errors(&diags);
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_deref(), Some("Q-5-20"));
    }

    // ── extract_profile_config: local config / overlay sites ────────

    #[test]
    fn extract_local_config_honors_default_only() {
        let mut meta = config_value_from_yaml("profile:\n  default: dev\n  group: [a, b]\n");
        let mut diags = Vec::new();
        let config = extract_profile_config(
            &mut meta,
            ProfileKeySite::LocalConfig,
            "_quarto.yml.local",
            &mut diags,
        );
        assert_eq!(config.default, vec!["dev"]);
        assert!(
            config.groups.is_empty(),
            "groups are base-config-only (Q1 parity)"
        );
        assert_eq!(diags.len(), 1, "got: {:?}", codes(&diags));
        assert_eq!(diags[0].kind, DiagnosticKind::Warning);
        assert_eq!(diags[0].code.as_deref(), Some("Q-5-22"));
        let text = diags[0].to_text(None);
        assert!(
            text.contains("_quarto.yml.local"),
            "warning must name the file: {text}"
        );
    }

    #[test]
    fn extract_overlay_profile_key_is_q_5_22_warning_and_inert() {
        let mut meta = config_value_from_yaml("profile:\n  default: dev\nfoo: 1\n");
        let mut diags = Vec::new();
        let config = extract_profile_config(
            &mut meta,
            ProfileKeySite::Overlay,
            "_quarto-prod.yml",
            &mut diags,
        );
        assert_eq!(config, ProjectProfileConfig::default(), "nothing honored");
        assert!(meta.get("profile").is_none(), "stripped from the overlay");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].kind, DiagnosticKind::Warning);
        assert_eq!(diags[0].code.as_deref(), Some("Q-5-22"));
        let text = diags[0].to_text(None);
        assert!(text.contains("_quarto-prod.yml"), "got: {text}");
    }

    #[test]
    fn extract_overlay_without_profile_key_is_silent() {
        let mut meta = config_value_from_yaml("format:\n  html:\n    toc: true\n");
        let mut diags = Vec::new();
        let config = extract_profile_config(
            &mut meta,
            ProfileKeySite::Overlay,
            "_quarto-prod.yml",
            &mut diags,
        );
        assert_eq!(config, ProjectProfileConfig::default());
        assert!(diags.is_empty());
    }

    // ── resolve_active_profiles ─────────────────────────────────────

    fn resolve(inputs: &ProfileResolutionInputs) -> (Vec<ActiveProfile>, Vec<DiagnosticMessage>) {
        let mut diags = Vec::new();
        let active = resolve_active_profiles(inputs, &mut diags);
        (active, diags)
    }

    #[test]
    fn resolve_nothing_is_empty_and_silent() {
        let config = ProjectProfileConfig::default();
        let (active, diags) = resolve(&ProfileResolutionInputs {
            config: &config,
            ..Default::default()
        });
        assert!(active.is_empty());
        assert!(diags.is_empty());
    }

    #[test]
    fn resolve_cli_beats_env() {
        let config = ProjectProfileConfig::default();
        let cli = vec!["a".to_string()];
        let (active, diags) = resolve(&ProfileResolutionInputs {
            cli: Some(&cli),
            env_var: Some("b"),
            config: &config,
            ..Default::default()
        });
        assert_eq!(
            names(&active),
            vec!["a"],
            "--profile replaces QUARTO_PROFILE"
        );
        assert_eq!(active[0].source, ProfileSource::CliArg);
        assert!(diags.is_empty());
    }

    #[test]
    fn resolve_env_beats_env_file_beats_local_beats_config_default() {
        let config = ProjectProfileConfig {
            default: vec!["d".to_string()],
            groups: Vec::new(),
        };
        let local = vec!["c".to_string()];

        let (active, _) = resolve(&ProfileResolutionInputs {
            env_var: Some("a"),
            env_file: Some("b"),
            local_default: &local,
            config: &config,
            ..Default::default()
        });
        assert_eq!(names(&active), vec!["a"]);
        assert_eq!(active[0].source, ProfileSource::EnvVar);

        let (active, _) = resolve(&ProfileResolutionInputs {
            env_file: Some("b"),
            local_default: &local,
            config: &config,
            ..Default::default()
        });
        assert_eq!(names(&active), vec!["b"]);
        assert_eq!(active[0].source, ProfileSource::EnvironmentFile);

        let (active, _) = resolve(&ProfileResolutionInputs {
            local_default: &local,
            config: &config,
            ..Default::default()
        });
        assert_eq!(names(&active), vec!["c"]);
        assert_eq!(active[0].source, ProfileSource::LocalConfigDefault);

        let (active, _) = resolve(&ProfileResolutionInputs {
            config: &config,
            ..Default::default()
        });
        assert_eq!(names(&active), vec!["d"]);
        assert_eq!(active[0].source, ProfileSource::ConfigDefault);
    }

    #[test]
    fn resolve_cli_values_split_and_combine() {
        // Both `--profile a,b --profile c` and `--profile "a b"` work.
        let config = ProjectProfileConfig::default();
        let cli = vec!["a,b".to_string(), "c".to_string()];
        let (active, diags) = resolve(&ProfileResolutionInputs {
            cli: Some(&cli),
            config: &config,
            ..Default::default()
        });
        assert_eq!(names(&active), vec!["a", "b", "c"]);
        assert!(diags.is_empty());
    }

    #[test]
    fn resolve_explicitly_empty_cli_is_q_5_21_error() {
        let config = ProjectProfileConfig::default();
        let cli = vec![String::new()];
        let (active, diags) = resolve(&ProfileResolutionInputs {
            cli: Some(&cli),
            config: &config,
            ..Default::default()
        });
        assert!(active.is_empty());
        let errs = errors(&diags);
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_deref(), Some("Q-5-21"));
    }

    #[test]
    fn resolve_separator_only_env_var_is_q_5_21_error() {
        // QUARTO_PROFILE=" , " is set-but-empty: an error, not a
        // silent no-op (divergence from Q1, which yields ["",""]).
        let config = ProjectProfileConfig::default();
        let (active, diags) = resolve(&ProfileResolutionInputs {
            env_var: Some(" , "),
            config: &config,
            ..Default::default()
        });
        assert!(active.is_empty());
        let errs = errors(&diags);
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_deref(), Some("Q-5-21"));
    }

    #[test]
    fn resolve_empty_string_env_var_is_unset() {
        // `QUARTO_PROFILE= q2 render` must mean "no selection" and
        // fall through to defaults, matching shell conventions.
        let config = ProjectProfileConfig {
            default: vec!["d".to_string()],
            groups: Vec::new(),
        };
        let (active, diags) = resolve(&ProfileResolutionInputs {
            env_var: Some(""),
            config: &config,
            ..Default::default()
        });
        assert_eq!(names(&active), vec!["d"]);
        assert!(diags.is_empty());
    }

    #[test]
    fn resolve_invalid_cli_name_is_q_5_21_error() {
        let config = ProjectProfileConfig::default();
        let cli = vec!["good,bad/name".to_string()];
        let (active, diags) = resolve(&ProfileResolutionInputs {
            cli: Some(&cli),
            config: &config,
            ..Default::default()
        });
        assert_eq!(names(&active), vec!["good"], "valid names still resolve");
        let errs = errors(&diags);
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_deref(), Some("Q-5-21"));
        let text = errs[0].to_text(None);
        assert!(text.contains("bad/name"), "must name the offender: {text}");
    }

    #[test]
    fn resolve_colon_name_hints_about_separators() {
        let config = ProjectProfileConfig::default();
        let (_, diags) = resolve(&ProfileResolutionInputs {
            env_var: Some("a:b"),
            config: &config,
            ..Default::default()
        });
        let errs = errors(&diags);
        assert_eq!(errs.len(), 1);
        let text = errs[0].to_text(None);
        assert!(
            text.contains("comma"),
            "a colon-separated list deserves a targeted hint: {text}"
        );
    }

    #[test]
    fn resolve_group_appends_first_member_when_none_active() {
        let config = ProjectProfileConfig {
            default: Vec::new(),
            groups: vec![vec!["basic".to_string(), "advanced".to_string()]],
        };
        let (active, diags) = resolve(&ProfileResolutionInputs {
            config: &config,
            ..Default::default()
        });
        assert_eq!(names(&active), vec!["basic"]);
        assert_eq!(active[0].source, ProfileSource::GroupDefault);
        assert!(diags.is_empty());
    }

    #[test]
    fn resolve_group_satisfied_by_explicit_selection() {
        let config = ProjectProfileConfig {
            default: Vec::new(),
            groups: vec![vec!["basic".to_string(), "advanced".to_string()]],
        };
        let (active, _) = resolve(&ProfileResolutionInputs {
            env_var: Some("advanced"),
            config: &config,
            ..Default::default()
        });
        assert_eq!(
            names(&active),
            vec!["advanced"],
            "no group default appended"
        );
    }

    #[test]
    fn resolve_group_default_appends_after_explicit() {
        // Appended AFTER explicit selections → lower overlay
        // precedence under first-listed-wins.
        let config = ProjectProfileConfig {
            default: Vec::new(),
            groups: vec![vec!["fmt-a".to_string(), "fmt-b".to_string()]],
        };
        let (active, _) = resolve(&ProfileResolutionInputs {
            env_var: Some("production"),
            config: &config,
            ..Default::default()
        });
        assert_eq!(names(&active), vec!["production", "fmt-a"]);
        assert_eq!(active[1].source, ProfileSource::GroupDefault);
    }

    #[test]
    fn resolve_multiple_groups_each_contribute() {
        let config = ProjectProfileConfig {
            default: Vec::new(),
            groups: vec![
                vec!["a1".to_string(), "a2".to_string()],
                vec!["b1".to_string(), "b2".to_string()],
            ],
        };
        let (active, _) = resolve(&ProfileResolutionInputs {
            env_var: Some("b2"),
            config: &config,
            ..Default::default()
        });
        assert_eq!(names(&active), vec!["b2", "a1"]);
    }

    #[test]
    fn resolve_groups_apply_even_with_cli_selection() {
        // Q1 parity: group invariants hold regardless of how the
        // explicit selection was made.
        let config = ProjectProfileConfig {
            default: Vec::new(),
            groups: vec![vec!["basic".to_string(), "advanced".to_string()]],
        };
        let cli = vec!["production".to_string()];
        let (active, _) = resolve(&ProfileResolutionInputs {
            cli: Some(&cli),
            config: &config,
            ..Default::default()
        });
        assert_eq!(names(&active), vec!["production", "basic"]);
    }

    #[test]
    fn resolve_config_default_and_groups_compose() {
        // default supplies the explicit set; groups still enforced.
        let config = ProjectProfileConfig {
            default: vec!["docs".to_string()],
            groups: vec![vec!["basic".to_string(), "advanced".to_string()]],
        };
        let (active, _) = resolve(&ProfileResolutionInputs {
            config: &config,
            ..Default::default()
        });
        assert_eq!(names(&active), vec!["docs", "basic"]);
    }

    #[test]
    fn resolve_dedups_across_sources() {
        let config = ProjectProfileConfig {
            default: Vec::new(),
            groups: vec![vec!["a".to_string(), "b".to_string()]],
        };
        let (active, _) = resolve(&ProfileResolutionInputs {
            env_var: Some("a,a"),
            config: &config,
            ..Default::default()
        });
        assert_eq!(names(&active), vec!["a"], "dup dropped, group satisfied");
    }

    // ── error catalog registration ──────────────────────────────────

    #[test]
    fn project_profile_error_codes_are_registered_in_catalog() {
        for code in ["Q-5-19", "Q-5-20", "Q-5-21", "Q-5-22"] {
            assert!(
                quarto_error_catalog::ERROR_CATALOG.get(code).is_some(),
                "{code} must be registered in the quarto-error-catalog"
            );
        }
    }
}
