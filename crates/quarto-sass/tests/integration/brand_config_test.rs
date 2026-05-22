//! Tests for `_brand.yml` integration into `ThemeSpec` and
//! `ThemeConfig`. Phase 5 of the brand-yml plan.

use std::path::PathBuf;

use quarto_pandoc_types::{ConfigMapEntry, ConfigValue, ConfigValueKind, MergeOp};
use quarto_sass::{ThemeConfig, ThemeSpec};
use quarto_source_map::SourceInfo;
use yaml_rust2::Yaml;

// ── ThemeSpec ───────────────────────────────────────────────────────

#[test]
fn theme_spec_parses_brand_token() {
    let spec = ThemeSpec::parse("brand").expect("parse brand");
    assert!(spec.is_brand(), "spec = {spec:?}");
}

#[test]
fn theme_spec_brand_scss_still_parses_as_custom_path() {
    // A file named `brand.scss` should be treated as a custom path,
    // not as the brand marker. The extension takes precedence.
    let spec = ThemeSpec::parse("brand.scss").expect("parse");
    assert!(spec.is_custom());
    assert!(!spec.is_brand());
}

#[test]
fn theme_spec_brand_is_neither_builtin_nor_custom() {
    let spec = ThemeSpec::parse("brand").expect("parse brand");
    assert!(!spec.is_builtin());
    assert!(!spec.is_custom());
    assert!(spec.is_brand());
}

// ── ThemeConfig with bare `brand:` key (no theme array) ─────────────

fn flattened_config(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
    let map_entries = entries
        .into_iter()
        .map(|(k, v)| ConfigMapEntry {
            key: k.to_string(),
            key_source: SourceInfo::for_test(),
            value: v,
        })
        .collect();
    ConfigValue {
        value: ConfigValueKind::Map(map_entries),
        source_info: SourceInfo::for_test(),
        merge_op: MergeOp::Concat,
    }
}

fn scalar_string(s: &str) -> ConfigValue {
    ConfigValue {
        value: ConfigValueKind::Scalar(Yaml::String(s.to_string())),
        source_info: SourceInfo::for_test(),
        merge_op: MergeOp::Concat,
    }
}

fn array_strings(items: &[&str]) -> ConfigValue {
    ConfigValue {
        value: ConfigValueKind::Array(items.iter().map(|s| scalar_string(s)).collect()),
        source_info: SourceInfo::for_test(),
        merge_op: MergeOp::Concat,
    }
}

#[test]
fn brand_key_as_string_path_produces_brand_ref() {
    let config = flattened_config(vec![("brand", scalar_string("_brand.yml"))]);
    let theme_config = ThemeConfig::from_config_value(&config).expect("from_config_value");
    let brand_ref = theme_config
        .brand_ref
        .as_ref()
        .expect("brand_ref should be Some");
    match brand_ref {
        quarto_brand::BrandRef::Path(p) => assert_eq!(p, &PathBuf::from("_brand.yml")),
        _ => panic!("expected Path variant"),
    }
}

#[test]
fn brand_key_alone_auto_injects_brand_into_theme_list() {
    let config = flattened_config(vec![("brand", scalar_string("_brand.yml"))]);
    let theme_config = ThemeConfig::from_config_value(&config).unwrap();
    assert_eq!(theme_config.themes.len(), 1);
    assert!(
        theme_config.themes[0].is_brand(),
        "expected auto-injected Brand spec, got {:?}",
        theme_config.themes
    );
}

#[test]
fn brand_token_in_theme_array_with_brand_key() {
    let config = flattened_config(vec![
        ("theme", array_strings(&["cosmo", "brand", "custom.scss"])),
        ("brand", scalar_string("_brand.yml")),
    ]);
    let theme_config = ThemeConfig::from_config_value(&config).unwrap();
    assert_eq!(theme_config.themes.len(), 3);
    assert!(theme_config.themes[0].is_builtin());
    assert!(theme_config.themes[1].is_brand());
    assert!(theme_config.themes[2].is_custom());
    assert!(theme_config.brand_ref.is_some());
}

#[test]
fn brand_token_in_theme_array_without_brand_key_errors() {
    // `theme: [..., brand, ...]` but no `brand:` configured — the user
    // named a thing that doesn't exist.
    let config = flattened_config(vec![("theme", array_strings(&["cosmo", "brand"]))]);
    let err = ThemeConfig::from_config_value(&config).expect_err("should error");
    let msg = err.to_string();
    assert!(
        msg.to_lowercase().contains("brand"),
        "error should mention brand: {msg}"
    );
}

#[test]
fn brand_key_with_existing_brand_token_no_double_inject() {
    let config = flattened_config(vec![
        ("theme", array_strings(&["brand"])),
        ("brand", scalar_string("_brand.yml")),
    ]);
    let theme_config = ThemeConfig::from_config_value(&config).unwrap();
    assert_eq!(theme_config.themes.len(), 1);
    assert!(theme_config.themes[0].is_brand());
}

// ── inline brand block ──────────────────────────────────────────────

#[test]
fn brand_key_as_inline_map_produces_inline_ref() {
    use yaml_rust2::Yaml;
    let mut color_map = yaml_rust2::yaml::Hash::new();
    color_map.insert(Yaml::String("primary".into()), Yaml::String("#abc".into()));
    let mut brand_map = yaml_rust2::yaml::Hash::new();
    brand_map.insert(Yaml::String("color".into()), Yaml::Hash(color_map));

    let brand_value = ConfigValue {
        value: ConfigValueKind::Scalar(Yaml::Hash(brand_map)),
        source_info: SourceInfo::for_test(),
        merge_op: MergeOp::Concat,
    };

    let config = flattened_config(vec![("brand", brand_value)]);
    let theme_config = ThemeConfig::from_config_value(&config).unwrap();
    let brand_ref = theme_config
        .brand_ref
        .as_ref()
        .expect("brand_ref should be Some");
    assert!(matches!(brand_ref, quarto_brand::BrandRef::Inline(_)));
}

// ── ThemeConfig::resolve ────────────────────────────────────────────

#[test]
fn resolve_no_brand_produces_resolved_with_brand_none() {
    let config = flattened_config(vec![("theme", scalar_string("cosmo"))]);
    let theme_config = ThemeConfig::from_config_value(&config).unwrap();
    let resolved = theme_config
        .resolve(
            &quarto_system_runtime::NativeRuntime::new(),
            std::path::Path::new("/tmp"),
        )
        .expect("resolve");
    assert!(resolved.brand.is_none());
    assert_eq!(resolved.themes.len(), 1);
}

#[test]
fn resolve_inline_brand_parses_typed_brand() {
    use yaml_rust2::Yaml;
    let mut color_map = yaml_rust2::yaml::Hash::new();
    color_map.insert(Yaml::String("primary".into()), Yaml::String("#abc".into()));
    let mut brand_map = yaml_rust2::yaml::Hash::new();
    brand_map.insert(Yaml::String("color".into()), Yaml::Hash(color_map));

    let brand_value = ConfigValue {
        value: ConfigValueKind::Scalar(Yaml::Hash(brand_map)),
        source_info: SourceInfo::for_test(),
        merge_op: MergeOp::Concat,
    };

    let config = flattened_config(vec![("brand", brand_value)]);
    let theme_config = ThemeConfig::from_config_value(&config).unwrap();
    let resolved = theme_config
        .resolve(
            &quarto_system_runtime::NativeRuntime::new(),
            std::path::Path::new("/tmp"),
        )
        .expect("resolve");
    let brand = resolved.brand.expect("resolved brand");
    let color = brand.color.expect("color");
    assert_eq!(color.primary.as_deref(), Some("#abc"));
}

#[test]
fn resolve_path_brand_reads_from_runtime() {
    // Write a brand fixture to a tempdir and resolve against it.
    let dir = tempfile::tempdir().expect("tempdir");
    let brand_path = dir.path().join("_brand.yml");
    std::fs::write(&brand_path, "color:\n  primary: \"#def\"\n").unwrap();

    let config = flattened_config(vec![("brand", scalar_string("_brand.yml"))]);
    let theme_config = ThemeConfig::from_config_value(&config).unwrap();
    let resolved = theme_config
        .resolve(&quarto_system_runtime::NativeRuntime::new(), dir.path())
        .expect("resolve");
    let brand = resolved.brand.expect("brand");
    let color = brand.color.expect("color");
    assert_eq!(color.primary.as_deref(), Some("#def"));
}
