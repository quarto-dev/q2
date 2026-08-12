/*
 * diagnostic_policy.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Per-code diagnostic policy read from merged document metadata.
 */

//! Warning suppression (bd-lone-bracket-diagnostic-mxu41qbt).
//!
//! A project can declare that a diagnostic q2 emits is not, for that
//! project, a problem:
//!
//! ```yaml
//! # _quarto.yml, or a document's front matter
//! diagnostics:
//!   Q-2-49: off
//! ```
//!
//! …or, preferably, with a reason:
//!
//! ```yaml
//! diagnostics:
//!   Q-2-49:
//!     level: off
//!     reason: "bare spans are hooks for our annotate.lua filter"
//! ```
//!
//! # Why this exists
//!
//! Without suppression, "some project might use this construct
//! deliberately" is an unanswerable objection to *any* diagnostic on an
//! ambiguous-but-usually-wrong construct — the project that means it has
//! no way to say so. The cost of honoring that objection is paid by every
//! reader of every *other* project, silently. With suppression the cost
//! inverts: the few projects that mean it opt out once, in one place.
//! `Q-2-49` (a lone bare `[text]`, whose brackets q2 deletes) is the
//! diagnostic this mechanism was built to unblock.
//!
//! # Where it applies
//!
//! Resolution happens in `MetadataMergeStage`, so precedence
//! (project → directory → document) is whatever the metadata merge already
//! decided — no separate precedence rules. Application happens in
//! [`crate::pipeline::run_pipeline`], the one point every per-document
//! diagnostic passes through on its way to *any* frontend, so suppression
//! works identically under `quarto render`, `q2 preview`, and hub-client.
//!
//! Because application happens inside the render, it necessarily precedes
//! `--strict`'s warnings-to-errors promotion at the CLI summary boundary
//! (`ProjectRenderSummary::promote_warnings_to_errors`). A suppressed
//! warning therefore stays suppressed under `--strict` rather than
//! reappearing as an error, which is the intended ordering: suppression is
//! the author's statement about *what counts as a problem*; strict mode is
//! a statement about *what to do with problems*.
//!
//! # Deliberate limits
//!
//! - **Errors are never suppressed.** Silencing an error means shipping
//!   broken output with no signal at all — the exact failure mode this
//!   feature exists to prevent.
//! - **Only diagnostics that carry a code can be suppressed.** Roughly
//!   25–30 warnings in the tree are still built with a bare
//!   `DiagnosticMessage::warning(...)` and no `.with_code(...)`; those are
//!   invisible to any code-keyed policy. `bd-m2w7a` tracks the backfill.
//! - **Project-*scoped* diagnostics are out of reach in v1.**
//!   `ProjectRenderSummary::project_diagnostics` and the config-diagnostic
//!   `eprintln!` path in `quarto render` do not flow through
//!   `run_pipeline`. Note this is *not* the same as "suppression written
//!   in `_quarto.yml` doesn't work" — that works, because project config
//!   is the first layer of the per-document metadata merge.

use hashlink::LinkedHashMap;
use quarto_error_reporting::{DiagnosticKind, DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::{ConfigValue, ConfigValueKind};
use quarto_source_map::SourceInfo;
use yaml_rust2::Yaml;

/// The metadata key a policy is read from.
const METADATA_KEY: &str = "diagnostics";

/// `Q-5-27` — an entry under `diagnostics:` that could not be understood.
const CODE_INVALID_ENTRY: &str = "Q-5-27";

/// What a policy says to do with one diagnostic code.
///
/// Only [`PolicyLevel::Off`] exists today. The config shape is a per-code
/// map (rather than a flat `suppress:` list) specifically so that
/// `warning` / `error` levels can be added here later without a second,
/// competing config key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyLevel {
    /// Drop diagnostics carrying this code.
    Off,
}

/// One code's entry in a [`DiagnosticPolicy`].
#[derive(Debug, Clone)]
pub struct PolicyEntry {
    /// What to do with diagnostics carrying this code.
    pub level: PolicyLevel,
    /// Why the author suppressed it. Optional, but strongly encouraged —
    /// it is the difference between a suppression that can be reviewed a
    /// year later and one that can only be deleted and re-discovered.
    pub reason: Option<String>,
    /// Where the entry was written, for future diagnostics about the
    /// policy itself (unused-suppression reporting, unknown codes).
    pub source: SourceInfo,
}

/// A resolved per-code diagnostic policy for one document.
///
/// `LinkedHashMap` rather than `HashMap`: entries are lookup-only today,
/// but the deferred unused-suppression report (bd-91rgxmav) will iterate
/// them to produce user-visible output, and author-config order is the
/// order that report should use.
#[derive(Debug, Clone, Default)]
pub struct DiagnosticPolicy {
    entries: LinkedHashMap<String, PolicyEntry>,
}

impl DiagnosticPolicy {
    /// True when the policy has nothing to say (the overwhelmingly common
    /// case — applying it is then a no-op).
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The entry for one code, if any.
    pub fn entry(&self, code: &str) -> Option<&PolicyEntry> {
        self.entries.get(code)
    }

    /// Read a policy out of merged document metadata.
    ///
    /// Returns the policy plus diagnostics about the policy *itself* —
    /// malformed entries are reported rather than ignored, because q2 has
    /// no config schema layer to catch them (an unrecognized key in
    /// `_quarto.yml` is otherwise silently dropped, which would turn a
    /// typo'd suppression into a mystery).
    pub fn from_metadata(meta: &ConfigValue) -> (Self, Vec<DiagnosticMessage>) {
        let mut entries = LinkedHashMap::new();
        let mut diagnostics = Vec::new();

        let Some(section) = meta.get(METADATA_KEY) else {
            return (Self { entries }, diagnostics);
        };

        let Some(map_entries) = section.as_map_entries() else {
            diagnostics.push(invalid_entry(
                format!(
                    "`{METADATA_KEY}:` must be a map of error code to level, \
                     e.g. `{METADATA_KEY}:\\n  Q-2-49: off`."
                ),
                &section.source_info,
            ));
            return (Self { entries }, diagnostics);
        };

        for map_entry in map_entries {
            match parse_entry(&map_entry.value) {
                Ok(entry) => {
                    entries.insert(map_entry.key.clone(), entry);
                }
                Err(message) => diagnostics.push(invalid_entry(
                    format!("`{METADATA_KEY}.{}`: {message}", map_entry.key),
                    &map_entry.key_source,
                )),
            }
        }

        (Self { entries }, diagnostics)
    }

    /// Drop every suppressed diagnostic from `diagnostics`, in place.
    ///
    /// Errors are retained regardless of policy.
    pub fn apply(&self, diagnostics: &mut Vec<DiagnosticMessage>) {
        if self.entries.is_empty() {
            return;
        }
        diagnostics.retain(|diagnostic| !self.suppresses(diagnostic));
    }

    /// Whether this policy silences one specific diagnostic.
    fn suppresses(&self, diagnostic: &DiagnosticMessage) -> bool {
        if diagnostic.kind == DiagnosticKind::Error {
            return false;
        }
        let Some(code) = diagnostic.code.as_deref() else {
            return false;
        };
        matches!(
            self.entries.get(code).map(|entry| entry.level),
            Some(PolicyLevel::Off)
        )
    }
}

/// Parse one `diagnostics.<CODE>` value: either the short form (`off`) or
/// the long form (`{level: off, reason: "…"}`).
fn parse_entry(value: &ConfigValue) -> Result<PolicyEntry, String> {
    if value.as_map_entries().is_some() {
        let level_value = value
            .get("level")
            .ok_or_else(|| "the long form requires a `level:` key.".to_string())?;
        let level = parse_level(level_value)?;
        let reason = value.get("reason").and_then(|r| r.as_plain_text());
        return Ok(PolicyEntry {
            level,
            reason,
            source: value.source_info.clone(),
        });
    }

    Ok(PolicyEntry {
        level: parse_level(value)?,
        reason: None,
        source: value.source_info.clone(),
    })
}

/// Parse a level scalar.
///
/// `off` is accepted both as the string `"off"` and as a boolean `false`,
/// because YAML 1.1 resolves a bare `off` to `false` — an author who
/// writes the documented spelling must not be told it is invalid.
fn parse_level(value: &ConfigValue) -> Result<PolicyLevel, String> {
    if matches!(&value.value, ConfigValueKind::Scalar(Yaml::Boolean(false))) {
        return Ok(PolicyLevel::Off);
    }
    // `as_plain_text` (not `as_str`) because a bare YAML string in
    // front-matter context is stored as `PandocInlines`, for which
    // `as_str` returns `None`.
    match value.as_plain_text().as_deref() {
        Some("off") => Ok(PolicyLevel::Off),
        Some(other) => Err(format!(
            "`{other}` is not a supported level; the only supported level is `off`."
        )),
        None => Err("expected `off`, or a map with a `level:` key.".to_string()),
    }
}

fn invalid_entry(message: String, source: &SourceInfo) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning(message)
        .with_code(CODE_INVALID_ENTRY)
        .with_location(source.clone())
        .add_hint(
            "Write `Q-2-49: off`, or give a reason with `Q-2-49: {level: off, reason: \"…\"}`?",
        )
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_source_map::{By, SourceInfo};

    /// Build a `ConfigValue` map from YAML-ish source so the tests read
    /// like the config they describe.
    fn meta(yaml: &str) -> ConfigValue {
        let docs = yaml_rust2::YamlLoader::load_from_str(yaml).expect("test YAML must parse");
        yaml_to_config(&docs[0])
    }

    fn yaml_to_config(yaml: &Yaml) -> ConfigValue {
        let source = SourceInfo::generated(By::programmatic_config());
        let kind = match yaml {
            Yaml::Hash(hash) => ConfigValueKind::Map(
                hash.iter()
                    .map(|(k, v)| quarto_pandoc_types::ConfigMapEntry {
                        key: k.as_str().expect("test keys are strings").to_string(),
                        key_source: SourceInfo::generated(By::programmatic_config()),
                        value: yaml_to_config(v),
                    })
                    .collect(),
            ),
            Yaml::Array(items) => {
                ConfigValueKind::Array(items.iter().map(yaml_to_config).collect())
            }
            scalar => ConfigValueKind::Scalar(scalar.clone()),
        };
        ConfigValue {
            value: kind,
            source_info: source,
            merge_op: quarto_pandoc_types::MergeOp::default(),
        }
    }

    fn warning(code: &str) -> DiagnosticMessage {
        DiagnosticMessage::warning(format!("warning {code}")).with_code(code)
    }

    #[test]
    fn absent_key_yields_an_empty_policy() {
        let (policy, diagnostics) = DiagnosticPolicy::from_metadata(&meta("title: hello"));
        assert!(policy.is_empty());
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn short_form_suppresses() {
        let (policy, diagnostics) =
            DiagnosticPolicy::from_metadata(&meta("diagnostics:\n  Q-2-49: off"));
        assert!(diagnostics.is_empty(), "got {diagnostics:?}");
        assert_eq!(
            policy.entry("Q-2-49").map(|e| e.level),
            Some(PolicyLevel::Off)
        );
        assert_eq!(policy.entry("Q-2-49").unwrap().reason, None);
    }

    /// YAML 1.1 resolves a bare `off` to boolean `false`. The documented
    /// spelling must work regardless of which side of that the loader
    /// lands on.
    #[test]
    fn bare_off_parsed_as_boolean_false_still_means_off() {
        let value = yaml_to_config(&Yaml::Boolean(false));
        assert_eq!(parse_level(&value), Ok(PolicyLevel::Off));
    }

    #[test]
    fn long_form_captures_the_reason() {
        let (policy, diagnostics) = DiagnosticPolicy::from_metadata(&meta(
            "diagnostics:\n  Q-2-49:\n    level: off\n    reason: filter hook",
        ));
        assert!(diagnostics.is_empty(), "got {diagnostics:?}");
        let entry = policy.entry("Q-2-49").expect("entry must exist");
        assert_eq!(entry.level, PolicyLevel::Off);
        assert_eq!(entry.reason.as_deref(), Some("filter hook"));
    }

    #[test]
    fn apply_drops_only_the_suppressed_code() {
        let (policy, _) = DiagnosticPolicy::from_metadata(&meta("diagnostics:\n  Q-2-49: off"));
        let mut diagnostics = vec![warning("Q-2-49"), warning("Q-2-45"), warning("Q-2-49")];
        policy.apply(&mut diagnostics);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code.as_deref(), Some("Q-2-45"));
    }

    /// The safety property: suppressing an error would ship broken output
    /// with no signal, which is the failure this whole feature prevents.
    #[test]
    fn errors_are_never_suppressed() {
        let (policy, _) = DiagnosticPolicy::from_metadata(&meta("diagnostics:\n  Q-2-49: off"));
        let mut diagnostics = vec![
            DiagnosticMessage::error("boom").with_code("Q-2-49"),
            warning("Q-2-49"),
        ];
        policy.apply(&mut diagnostics);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, DiagnosticKind::Error);
    }

    #[test]
    fn uncoded_diagnostics_are_never_suppressed() {
        let (policy, _) = DiagnosticPolicy::from_metadata(&meta("diagnostics:\n  Q-2-49: off"));
        let mut diagnostics = vec![DiagnosticMessage::warning("no code here")];
        policy.apply(&mut diagnostics);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn empty_policy_application_is_a_no_op() {
        let policy = DiagnosticPolicy::default();
        let mut diagnostics = vec![warning("Q-2-49")];
        policy.apply(&mut diagnostics);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn unsupported_level_is_reported_not_ignored() {
        let (policy, diagnostics) =
            DiagnosticPolicy::from_metadata(&meta("diagnostics:\n  Q-2-49: error"));
        assert!(policy.is_empty());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code.as_deref(), Some(CODE_INVALID_ENTRY));
        assert!(
            diagnostics[0].title.contains("Q-2-49"),
            "the diagnostic must name the offending code; got {:?}",
            diagnostics[0].title
        );
    }

    #[test]
    fn long_form_without_level_is_reported() {
        let (policy, diagnostics) = DiagnosticPolicy::from_metadata(&meta(
            "diagnostics:\n  Q-2-49:\n    reason: no level given",
        ));
        assert!(policy.is_empty());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code.as_deref(), Some(CODE_INVALID_ENTRY));
    }

    #[test]
    fn non_map_section_is_reported() {
        let (policy, diagnostics) =
            DiagnosticPolicy::from_metadata(&meta("diagnostics:\n  - Q-2-49"));
        assert!(policy.is_empty());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code.as_deref(), Some(CODE_INVALID_ENTRY));
    }

    /// One bad entry must not discard the good ones alongside it.
    #[test]
    fn a_bad_entry_does_not_void_its_neighbours() {
        let (policy, diagnostics) =
            DiagnosticPolicy::from_metadata(&meta("diagnostics:\n  Q-2-49: off\n  Q-2-45: shout"));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            policy.entry("Q-2-49").map(|e| e.level),
            Some(PolicyLevel::Off)
        );
        assert!(policy.entry("Q-2-45").is_none());
    }
}
