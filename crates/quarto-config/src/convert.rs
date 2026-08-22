//! Conversion from YAML types to ConfigValue.
//!
//! This module provides conversion from `YamlWithSourceInfo` to `ConfigValue`,
//! extracting merge operations and interpretation hints from YAML tags.

// `config_value_from_yaml` has no production caller (see its doc comment),
// but it is kept `pub(crate)` + `#[allow(dead_code)]` rather than
// `#[cfg(test)]` so a plain `cargo build --workspace` still type-checks it
// against its hand-maintained lockstep partner, `yaml_to_config_value`. The
// imports below are used unconditionally as a result.
use crate::tag::parse_tag;
use crate::types::{ConfigMapEntry, ConfigValue, ConfigValueKind, Interpretation, MergeOp};
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_yaml::YamlWithSourceInfo;
use yaml_rust2::Yaml;

/// Convert a `YamlWithSourceInfo` to a `ConfigValue`.
///
/// This function recursively converts a YAML tree to a config tree, extracting
/// merge operations and interpretation hints from YAML tags.
///
/// **Crate-internal.** Kept only for quarto-config's own span-preservation
/// tests (the bd-2mxo / bd-9yh3pzfu tests in `materialize.rs`'s `mod spans`,
/// which need real quarto-yaml-derived spans, not `SourceInfo::for_test()`
/// stand-ins). It has **no production caller** — production YAML conversion
/// goes through `pampa::pandoc::yaml_to_config_value`, which quarto-config
/// cannot depend on (pampa depends on quarto-config, not the reverse, so a
/// dev-dependency the other way would be a cycle). Because of that layering,
/// this function **must be kept in lockstep** with `yaml_to_config_value` on
/// content provenance by hand — see the `content_source_info` handling below,
/// which mirrors `pampa::pandoc::meta::yaml_to_config_value_at` exactly.
///
/// # Arguments
///
/// * `yaml` - The source-tracked YAML value
/// * `diagnostics` - Collector for errors and warnings from tag parsing
///
/// # Returns
///
/// A `ConfigValue` with merge semantics extracted from tags.
/// Check `diagnostics` for any errors or warnings that occurred during conversion.
///
/// `pub(crate)` + `#[allow(dead_code)]` rather than `#[cfg(test)]`: this
/// function has no production caller (see above), but gating it to test-only
/// builds would mean a plain `cargo build --workspace` no longer type-checks
/// it against its lockstep partner, `yaml_to_config_value` — exactly the kind
/// of drift this function exists to catch. `#[allow(dead_code)]` suppresses
/// the lint that `pub(crate)` alone would trip in ordinary (non-test) builds.
#[allow(dead_code)]
pub(crate) fn config_value_from_yaml(
    yaml: YamlWithSourceInfo,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> ConfigValue {
    // Extract tag information if present
    let parsed_tag = if let Some((tag_str, tag_source)) = &yaml.tag {
        parse_tag(tag_str, tag_source, diagnostics)
    } else {
        Default::default()
    };

    // Determine the merge operation (default depends on value type)
    let default_merge_op = MergeOp::Concat;
    let merge_op = parsed_tag.merge_op.unwrap_or(default_merge_op);

    let interpretation = parsed_tag.interpretation;
    let source_info = yaml.source_info.clone();

    // Convert based on the YAML value type
    if yaml.is_array() {
        // Convert array
        let (items, _) = yaml.into_array().expect("checked is_array");
        let config_items: Vec<ConfigValue> = items
            .into_iter()
            .map(|item| config_value_from_yaml(item, diagnostics))
            .collect();

        ConfigValue {
            value: ConfigValueKind::Array(config_items),
            source_info,
            merge_op,
        }
    } else if yaml.is_hash() {
        // Convert hash/map with key source tracking
        let (entries, _) = yaml.into_hash().expect("checked is_hash");
        let config_entries: Vec<ConfigMapEntry> = entries
            .into_iter()
            .filter_map(|entry| {
                // Only include entries with string keys
                entry.key.yaml.as_str().map(|key| ConfigMapEntry {
                    key: key.to_string(),
                    key_source: entry.key.source_info.clone(),
                    value: config_value_from_yaml(entry.value, diagnostics),
                })
            })
            .collect();

        ConfigValue {
            value: ConfigValueKind::Map(config_entries),
            source_info,
            merge_op,
        }
    } else {
        // Provenance of the *decoded* content, when derivation ran (see
        // `YamlWithSourceInfo::content_source_info`'s contract). `source_info`
        // above describes the raw source text — quote delimiters, per-line
        // block-scalar decoding strips — so pairing a decoded string with it
        // and doing offset arithmetic drifts by whatever decoding removed.
        // Read here, before the match below moves `yaml.yaml`, mirroring
        // `pampa::pandoc::meta::yaml_to_config_value_at` exactly (this
        // function must stay in lockstep with it — see the doc comment above).
        let content_source_info = yaml.content_source_info().cloned();

        // q2 has already established this node is a scalar; if it's also a
        // string scalar with no content provenance, that's a desync report,
        // not a silent fallback (see `content_provenance_desync_warning`'s
        // doc comment). Mirrors
        // `pampa::pandoc::meta::content_provenance_desync_warning` — the two
        // must stay in lockstep for the same reason `config_value_from_yaml`
        // itself does (see the doc comment above).
        if matches!(yaml.yaml, Yaml::String(_)) && content_source_info.is_none() {
            diagnostics.push(content_provenance_desync_warning(&source_info));
        }

        // Scalar value - handle interpretation to create the right variant
        match (&yaml.yaml, interpretation) {
            // String with interpretation tag creates the appropriate variant
            (Yaml::String(s), Some(Interpretation::Path)) => ConfigValue {
                value: ConfigValueKind::Path(s.clone()),
                source_info,
                merge_op,
            },
            (Yaml::String(s), Some(Interpretation::Glob)) => ConfigValue {
                value: ConfigValueKind::Glob(s.clone()),
                source_info,
                merge_op,
            },
            (Yaml::String(s), Some(Interpretation::Expr)) => ConfigValue {
                value: ConfigValueKind::Expr(s.clone()),
                source_info,
                merge_op,
            },
            // Note: Interpretation::Markdown and Interpretation::PlainString are not
            // handled here because they require the markdown parser which is not
            // available in this crate. They will be handled by pampa when creating
            // document metadata. For now, we keep them as Scalar.
            _ => ConfigValue {
                value: match yaml.yaml {
                    // Scoped to `Yaml::String`, matching the scoping rule in
                    // `pampa::pandoc::meta::scalar_string_with_content_provenance`:
                    // no consumer here does sub-offset arithmetic into a
                    // non-string scalar, so every other scalar kind keeps
                    // storing `None`.
                    Yaml::String(s) => {
                        scalar_string_with_content_provenance(s, &content_source_info)
                    }
                    other => ConfigValueKind::scalar(other),
                },
                source_info,
                merge_op,
            },
        }
    }
}

/// Build a `Scalar(String)` carrying the decoded-content provenance derived
/// by the YAML reader, when available. Mirrors
/// `pampa::pandoc::meta::scalar_string_with_content_provenance` — the two
/// must stay in lockstep (see the module-level doc comment on
/// `config_value_from_yaml`).
///
/// Deliberately scoped to `Yaml::String`: see the call site's comment for
/// why every other scalar kind keeps constructing via `ConfigValueKind::scalar`
/// with no provenance.
///
/// `#[allow(dead_code)]` rather than `#[cfg(test)]`, for the same reason as
/// `config_value_from_yaml`: its only caller is that function, and it must
/// stay type-checked in every build.
#[allow(dead_code)]
fn scalar_string_with_content_provenance(
    s: String,
    content_source_info: &Option<quarto_source_map::SourceInfo>,
) -> ConfigValueKind {
    match content_source_info {
        Some(csi) => ConfigValueKind::scalar_with_provenance(Yaml::String(s), csi.clone()),
        None => ConfigValueKind::scalar(Yaml::String(s)),
    }
}

/// Build the consistency warning for a `Yaml::String` scalar whose
/// `content_source_info` came back `None`. Mirrors
/// `pampa::pandoc::meta::content_provenance_desync_warning` — the two must
/// stay in lockstep for the same reason `config_value_from_yaml` does (see
/// its doc comment).
///
/// `#[allow(dead_code)]` rather than `#[cfg(test)]`, for the same reason as
/// `config_value_from_yaml`: its only caller is that function, and it must
/// stay type-checked in every build.
#[allow(dead_code)]
fn content_provenance_desync_warning(
    source_info: &quarto_source_map::SourceInfo,
) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning("YAML string scalar has no content provenance")
        .with_location(source_info.clone())
        .problem(
            "`YamlWithSourceInfo::content_source_info()` returned `None` for a node already \
             established to be a `Yaml::String` scalar",
        )
        .add_detail(
            "Expected only for a hand-built test fixture (no derivation ran) or an unresolved \
             alias; otherwise this is a `quarto-yaml` provenance-derivation desync",
        )
        .add_hint(
            "Falling back to the raw node span, which may misalign a caret pointed at decoded content",
        )
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_source_map::SourceInfo;

    fn make_scalar(value: &str) -> YamlWithSourceInfo {
        // `.with_content_provenance(..)`: a hand-built node derives no
        // content provenance by construction. Attach a synthetic one (the
        // same shape these tests already pair with a `SourceInfo::for_test()`
        // raw span — see `span_assert.rs`'s module docs) so these
        // general-purpose fixtures don't trip the desync warning tested
        // separately in `test_convert_string_scalar_without_content_provenance_warns`.
        YamlWithSourceInfo::new_scalar(Yaml::String(value.into()), SourceInfo::for_test())
            .with_content_provenance(SourceInfo::for_test())
    }

    fn make_scalar_with_tag(value: &str, tag: &str) -> YamlWithSourceInfo {
        YamlWithSourceInfo::new_scalar_with_tag(
            Yaml::String(value.into()),
            SourceInfo::for_test(),
            Some((tag.to_string(), SourceInfo::for_test())),
        )
        .with_content_provenance(SourceInfo::for_test())
    }

    #[test]
    fn test_convert_scalar() {
        let mut diagnostics = Vec::new();
        let yaml = make_scalar("hello");
        let config = config_value_from_yaml(yaml, &mut diagnostics);

        assert!(diagnostics.is_empty());
        assert!(config.is_scalar());
        assert_eq!(config.merge_op, MergeOp::Concat);
        assert_eq!(config.as_yaml().unwrap().as_str(), Some("hello"));
    }

    #[test]
    fn test_convert_scalar_with_prefer_tag() {
        let mut diagnostics = Vec::new();
        let yaml = make_scalar_with_tag("hello", "prefer");
        let config = config_value_from_yaml(yaml, &mut diagnostics);

        assert!(diagnostics.is_empty());
        assert!(config.is_scalar());
        assert_eq!(config.merge_op, MergeOp::Prefer);
    }

    #[test]
    fn test_convert_scalar_with_md_tag() {
        let mut diagnostics = Vec::new();
        let yaml = make_scalar_with_tag("**bold**", "md");
        let config = config_value_from_yaml(yaml, &mut diagnostics);

        assert!(diagnostics.is_empty());
        // Note: Markdown interpretation is deferred, so it's still a Scalar
        assert!(matches!(config.value, ConfigValueKind::Scalar { .. }));
    }

    #[test]
    fn test_convert_scalar_with_combined_tag() {
        let mut diagnostics = Vec::new();
        let yaml = make_scalar_with_tag("**bold**", "prefer_md");
        let config = config_value_from_yaml(yaml, &mut diagnostics);

        assert!(diagnostics.is_empty());
        assert_eq!(config.merge_op, MergeOp::Prefer);
        // Markdown interpretation is deferred
        assert!(matches!(config.value, ConfigValueKind::Scalar { .. }));
    }

    #[test]
    fn test_convert_array() {
        let mut diagnostics = Vec::new();

        let items = vec![make_scalar("a"), make_scalar("b")];
        let yaml = YamlWithSourceInfo::new_array(
            Yaml::Array(vec![Yaml::String("a".into()), Yaml::String("b".into())]),
            SourceInfo::for_test(),
            items,
        );

        let config = config_value_from_yaml(yaml, &mut diagnostics);

        assert!(diagnostics.is_empty());
        assert!(config.is_array());
        assert_eq!(config.as_array().unwrap().len(), 2);
        assert_eq!(config.merge_op, MergeOp::Concat);
    }

    #[test]
    fn test_convert_hash() {
        let mut diagnostics = Vec::new();

        let key =
            YamlWithSourceInfo::new_scalar(Yaml::String("name".into()), SourceInfo::for_test());
        let value = make_scalar("value");
        let entry = quarto_yaml::YamlHashEntry::new(
            key,
            value,
            SourceInfo::for_test(),
            SourceInfo::for_test(),
            SourceInfo::for_test(),
        );

        let mut hash = yaml_rust2::yaml::Hash::new();
        hash.insert(Yaml::String("name".into()), Yaml::String("value".into()));

        let yaml =
            YamlWithSourceInfo::new_hash(Yaml::Hash(hash), SourceInfo::for_test(), vec![entry]);

        let config = config_value_from_yaml(yaml, &mut diagnostics);

        assert!(diagnostics.is_empty());
        assert!(config.is_map());
        assert_eq!(config.as_map_entries().unwrap().len(), 1);
        assert!(config.contains_key("name"));
    }

    #[test]
    fn test_convert_with_invalid_tag_produces_diagnostic() {
        let mut diagnostics = Vec::new();
        let yaml = make_scalar_with_tag("hello", "prefer_concat"); // Conflicting merge ops
        let _config = config_value_from_yaml(yaml, &mut diagnostics);

        assert!(!diagnostics.is_empty());
        assert!(diagnostics[0].code.as_deref() == Some("Q-1-28"));
    }

    // =========== END-TO-END INTEGRATION TESTS ===========

    /// Test end-to-end: parse YAML with quarto_yaml, convert to ConfigValue
    #[test]
    fn test_e2e_parse_and_convert_with_prefer_tag() {
        let yaml_content = "theme: !prefer cosmo";
        let yaml = quarto_yaml::parse(yaml_content).expect("parse failed");

        let mut diagnostics = Vec::new();
        let config = config_value_from_yaml(yaml, &mut diagnostics);

        assert!(diagnostics.is_empty());
        assert!(config.is_map());

        let theme = config.get("theme").expect("theme not found");
        assert_eq!(theme.merge_op, MergeOp::Prefer);
        assert_eq!(theme.as_yaml().unwrap().as_str(), Some("cosmo"));
    }

    #[test]
    fn test_e2e_parse_and_convert_with_md_tag() {
        let yaml_content = "description: !md \"**bold** text\"";
        let yaml = quarto_yaml::parse(yaml_content).expect("parse failed");

        let mut diagnostics = Vec::new();
        let config = config_value_from_yaml(yaml, &mut diagnostics);

        assert!(diagnostics.is_empty());

        let desc = config.get("description").expect("description not found");
        // Markdown interpretation is deferred, so it's still a Scalar
        assert!(matches!(desc.value, ConfigValueKind::Scalar { .. }));
    }

    #[test]
    fn test_e2e_parse_and_convert_with_path_tag() {
        let yaml_content = "file: !path ./data/input.csv";
        let yaml = quarto_yaml::parse(yaml_content).expect("parse failed");

        let mut diagnostics = Vec::new();
        let config = config_value_from_yaml(yaml, &mut diagnostics);

        assert!(diagnostics.is_empty());

        let file = config.get("file").expect("file not found");
        // Path interpretation creates Path variant
        assert!(matches!(file.value, ConfigValueKind::Path(_)));
        assert_eq!(file.as_str(), Some("./data/input.csv"));
    }

    #[test]
    fn test_e2e_parse_and_convert_with_glob_tag() {
        let yaml_content = "pattern: !glob \"*.qmd\"";
        let yaml = quarto_yaml::parse(yaml_content).expect("parse failed");

        let mut diagnostics = Vec::new();
        let config = config_value_from_yaml(yaml, &mut diagnostics);

        assert!(diagnostics.is_empty());

        let pattern = config.get("pattern").expect("pattern not found");
        assert!(matches!(pattern.value, ConfigValueKind::Glob(_)));
        assert_eq!(pattern.as_str(), Some("*.qmd"));
    }

    #[test]
    fn test_e2e_parse_and_convert_with_expr_tag() {
        let yaml_content = "value: !expr params$threshold";
        let yaml = quarto_yaml::parse(yaml_content).expect("parse failed");

        let mut diagnostics = Vec::new();
        let config = config_value_from_yaml(yaml, &mut diagnostics);

        assert!(diagnostics.is_empty());

        let value = config.get("value").expect("value not found");
        assert!(matches!(value.value, ConfigValueKind::Expr(_)));
        assert_eq!(value.as_str(), Some("params$threshold"));
    }

    #[test]
    fn test_e2e_parse_and_convert_nested_with_tags() {
        let yaml_content = r#"
format:
  html:
    theme: !prefer darkly
    toc: true
  pdf:
    documentclass: !str article
"#;
        let yaml = quarto_yaml::parse(yaml_content).expect("parse failed");

        let mut diagnostics = Vec::new();
        let config = config_value_from_yaml(yaml, &mut diagnostics);

        assert!(diagnostics.is_empty());

        // Navigate to nested values
        let format = config.get("format").expect("format not found");
        let html = format.get("html").expect("html not found");
        let theme = html.get("theme").expect("theme not found");

        assert_eq!(theme.merge_op, MergeOp::Prefer);
        assert_eq!(theme.as_yaml().unwrap().as_str(), Some("darkly"));

        let pdf = format.get("pdf").expect("pdf not found");
        let docclass = pdf.get("documentclass").expect("documentclass not found");

        // !str keeps it as Scalar
        assert!(matches!(docclass.value, ConfigValueKind::Scalar { .. }));
    }

    #[test]
    fn test_e2e_unknown_tag_produces_warning() {
        // Use "unknowntag" (no underscore) to avoid Q-1-26 invalid character error
        let yaml_content = "value: !unknowntag hello";
        let yaml = quarto_yaml::parse(yaml_content).expect("parse failed");

        let mut diagnostics = Vec::new();
        let config = config_value_from_yaml(yaml, &mut diagnostics);

        // Should have a warning (Q-1-21) but not an error
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code.as_deref(), Some("Q-1-21"));

        // Value should still be converted
        let value = config.get("value").expect("value not found");
        assert_eq!(value.as_yaml().unwrap().as_str(), Some("hello"));
    }

    #[test]
    fn test_e2e_parse_combined_tag_with_underscore() {
        // Test that combined tags with underscore work end-to-end
        let yaml_content = "title: !prefer_md \"**Override Title**\"";
        let yaml = quarto_yaml::parse(yaml_content).expect("parse failed");

        let mut diagnostics = Vec::new();
        let config = config_value_from_yaml(yaml, &mut diagnostics);

        assert!(
            diagnostics.is_empty(),
            "Expected no diagnostics, got: {:?}",
            diagnostics
        );

        let title = config.get("title").expect("title not found");
        assert_eq!(title.merge_op, MergeOp::Prefer);
        // Markdown interpretation is deferred
        assert!(matches!(title.value, ConfigValueKind::Scalar { .. }));
        assert_eq!(
            title.as_yaml().unwrap().as_str(),
            Some("**Override Title**")
        );
    }

    #[test]
    fn test_e2e_parse_concat_path_combined() {
        // Test concat_path combined tag
        let yaml_content = "files: !concat_path ./data.csv";
        let yaml = quarto_yaml::parse(yaml_content).expect("parse failed");

        let mut diagnostics = Vec::new();
        let config = config_value_from_yaml(yaml, &mut diagnostics);

        assert!(diagnostics.is_empty());

        let files = config.get("files").expect("files not found");
        assert_eq!(files.merge_op, MergeOp::Concat);
        // Path interpretation creates Path variant
        assert!(matches!(files.value, ConfigValueKind::Path(_)));
    }

    // ── content provenance threading (task C4b) ─────────────────────
    //
    // The five `mod spans` tests in `materialize.rs` assert through
    // `ConfigValue.source_info` — the *raw* node span, which the threading
    // in this file's `else` branch does not touch. Nothing else in this
    // crate reads `content_source_info` off a value this converter
    // produces, so without these two tests the threading hunk above is
    // unbound: it can be reverted and the whole suite stays green. Parse
    // real YAML per `span_assert.rs`'s module docs (`:39-45`) — a
    // `SourceInfo::for_test()` fixture would make a wrong span
    // indistinguishable from a right one.

    #[test]
    fn test_quoted_string_content_provenance_excludes_the_quotes() {
        // Raw byte layout of `k: "hello"\n`: the quote delimiters sit at
        // offsets 3 and 9; `content_source_info` must point at 4..9
        // ("hello"), not the quote-inclusive raw span. This is the exact
        // bug this epic exists to fix, so asserting merely `Some(..)`
        // would not discriminate it from the bug.
        let yaml_content = "k: \"hello\"\n";
        let yaml = quarto_yaml::parse_file(yaml_content, "fixture.yml").expect("valid yaml");

        let mut diagnostics = Vec::new();
        let config = config_value_from_yaml(yaml, &mut diagnostics);
        assert!(diagnostics.is_empty());

        let k = config.get("k").expect("k not found");
        let content_source_info = match &k.value {
            ConfigValueKind::Scalar {
                content_source_info,
                ..
            } => content_source_info
                .clone()
                .expect("quoted string scalar should carry content provenance"),
            other => panic!("expected Scalar, got {other:?}"),
        };

        let ctx = crate::span_assert::context_for("fixture.yml", yaml_content);
        let span = crate::span_assert::resolve_span(&content_source_info, &ctx)
            .unwrap_or_else(|p| panic!("content_source_info should resolve, got: {p}"));

        // The discriminating assertion: were provenance the raw
        // quote-inclusive span, this would resolve to `"hello"` with the
        // `"` delimiters (at offsets 3 and 9) included. 4..9 is the
        // content run.
        assert_eq!(span.text, "hello");
    }

    #[test]
    fn test_non_string_scalar_has_no_content_provenance() {
        // The threading is deliberately scoped to `Yaml::String` (see the
        // call site's comment in `config_value_from_yaml`). A non-string
        // scalar must keep storing `None`, so a later widening of the
        // scope would be caught here.
        let yaml_content = "k: 42\n";
        let yaml = quarto_yaml::parse_file(yaml_content, "fixture.yml").expect("valid yaml");

        let mut diagnostics = Vec::new();
        let config = config_value_from_yaml(yaml, &mut diagnostics);
        assert!(diagnostics.is_empty());

        let k = config.get("k").expect("k not found");
        match &k.value {
            ConfigValueKind::Scalar {
                content_source_info,
                yaml,
            } => {
                assert!(
                    matches!(yaml, Yaml::Integer(42)),
                    "expected Yaml::Integer(42), got {yaml:?}"
                );
                assert_eq!(
                    *content_source_info, None,
                    "non-string scalars must not carry content provenance"
                );
            }
            other => panic!("expected Scalar, got {other:?}"),
        }
    }

    // ── the desync/no-derivation warning (task C6, bd-yaml-provenance) ──
    //
    // `config_value_from_yaml` has no production caller (see its doc
    // comment), but it must stay in lockstep with
    // `pampa::pandoc::meta::yaml_to_config_value_at` by hand, including on
    // this warning. These two tests are what make that binding revertible
    // on this side: reverting the call to `content_provenance_desync_warning`
    // turns the positive test red, and widening the rule beyond
    // `Yaml::String` would turn the negative test red.

    #[test]
    fn test_convert_string_scalar_without_content_provenance_warns() {
        // `YamlWithSourceInfo::new_scalar` yields `content_source_info: None`
        // by construction (no derivation ran) — this is the injection seam
        // the warning exists to report on, built directly (not via
        // `make_scalar`, which now attaches synthetic provenance).
        let yaml = YamlWithSourceInfo::new_scalar(
            Yaml::String("no provenance".into()),
            SourceInfo::for_test(),
        );
        let mut diagnostics = Vec::new();
        let config = config_value_from_yaml(yaml, &mut diagnostics);

        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly one warning, got {diagnostics:?}"
        );
        assert_eq!(
            diagnostics[0].kind,
            quarto_error_reporting::DiagnosticKind::Warning,
            "must be a warning, not an error"
        );
        assert!(
            diagnostics[0].code.is_none(),
            "no Q- code: this is an internal consistency signal, not user-actionable"
        );
        // Non-fatal: the returned ConfigValue is still usable.
        assert!(matches!(
            config.value,
            ConfigValueKind::Scalar {
                yaml: Yaml::String(_),
                ..
            }
        ));
    }

    #[test]
    fn test_convert_non_string_scalar_without_content_provenance_does_not_warn() {
        // The rule is scoped to `Yaml::String`; a non-string scalar's
        // `None` is correct and must not warn.
        let yaml = YamlWithSourceInfo::new_scalar(Yaml::Integer(7), SourceInfo::for_test());
        let mut diagnostics = Vec::new();
        let config = config_value_from_yaml(yaml, &mut diagnostics);

        assert!(
            diagnostics.is_empty(),
            "non-string scalar's None must not warn, got {diagnostics:?}"
        );
        assert!(matches!(
            config.value,
            ConfigValueKind::Scalar {
                yaml: Yaml::Integer(7),
                ..
            }
        ));
    }

    #[test]
    fn test_map_key_source_tracking() {
        let yaml_content = "name: value";
        let yaml = quarto_yaml::parse(yaml_content).expect("parse failed");

        let mut diagnostics = Vec::new();
        let config = config_value_from_yaml(yaml, &mut diagnostics);

        assert!(diagnostics.is_empty());

        let entries = config.as_map_entries().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key, "name");
        // Key source should have position information
        // (exact values depend on YAML parser, just verify it's present)
    }
}
