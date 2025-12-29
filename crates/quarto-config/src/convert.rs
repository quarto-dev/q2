//! Conversion from YAML types to ConfigValue.
//!
//! This module provides conversion from `YamlWithSourceInfo` to `ConfigValue`,
//! extracting merge operations and interpretation hints from YAML tags.

use crate::tag::parse_tag;
use crate::types::{ConfigMapEntry, ConfigValue, ConfigValueKind, Interpretation, MergeOp};
use quarto_error_reporting::DiagnosticMessage;
use quarto_yaml::YamlWithSourceInfo;
use yaml_rust2::Yaml;

/// Convert a `YamlWithSourceInfo` to a `ConfigValue`.
///
/// This function recursively converts a YAML tree to a config tree, extracting
/// merge operations and interpretation hints from YAML tags.
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
pub fn config_value_from_yaml(
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
                value: ConfigValueKind::Scalar(yaml.yaml),
                source_info,
                merge_op,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_source_map::SourceInfo;

    fn make_scalar(value: &str) -> YamlWithSourceInfo {
        YamlWithSourceInfo::new_scalar(Yaml::String(value.into()), SourceInfo::default())
    }

    fn make_scalar_with_tag(value: &str, tag: &str) -> YamlWithSourceInfo {
        YamlWithSourceInfo::new_scalar_with_tag(
            Yaml::String(value.into()),
            SourceInfo::default(),
            Some((tag.to_string(), SourceInfo::default())),
        )
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
        assert!(matches!(config.value, ConfigValueKind::Scalar(_)));
    }

    #[test]
    fn test_convert_scalar_with_combined_tag() {
        let mut diagnostics = Vec::new();
        let yaml = make_scalar_with_tag("**bold**", "prefer_md");
        let config = config_value_from_yaml(yaml, &mut diagnostics);

        assert!(diagnostics.is_empty());
        assert_eq!(config.merge_op, MergeOp::Prefer);
        // Markdown interpretation is deferred
        assert!(matches!(config.value, ConfigValueKind::Scalar(_)));
    }

    #[test]
    fn test_convert_array() {
        let mut diagnostics = Vec::new();

        let items = vec![make_scalar("a"), make_scalar("b")];
        let yaml = YamlWithSourceInfo::new_array(
            Yaml::Array(vec![Yaml::String("a".into()), Yaml::String("b".into())]),
            SourceInfo::default(),
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
            YamlWithSourceInfo::new_scalar(Yaml::String("name".into()), SourceInfo::default());
        let value = make_scalar("value");
        let entry = quarto_yaml::YamlHashEntry::new(
            key,
            value,
            SourceInfo::default(),
            SourceInfo::default(),
            SourceInfo::default(),
        );

        let mut hash = yaml_rust2::yaml::Hash::new();
        hash.insert(Yaml::String("name".into()), Yaml::String("value".into()));

        let yaml =
            YamlWithSourceInfo::new_hash(Yaml::Hash(hash), SourceInfo::default(), vec![entry]);

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
        assert!(matches!(desc.value, ConfigValueKind::Scalar(_)));
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
        assert!(matches!(docclass.value, ConfigValueKind::Scalar(_)));
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
        assert!(matches!(title.value, ConfigValueKind::Scalar(_)));
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
