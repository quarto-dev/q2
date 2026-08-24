//! Unified `_brand.yml` light/dark splitting through
//! `ThemeConfig::resolve_variants` (bd-unified-brand-split-ep49amad,
//! GH #580).
//!
//! A single brand whose color/typography values carry `{light:,
//! dark:}` pairs must (a) parse, (b) synthesize a dark theme variant
//! when the brand content enables dark mode (Q1's `enablesDarkMode`),
//! and (c) hand each variant its half of the split brand. The
//! two-file `brand: {light:, dark:}` form keeps its config-time
//! synthesis and now also takes the matching half of each file.

use std::path::Path;

use quarto_pandoc_types::{ConfigMapEntry, ConfigValue, ConfigValueKind, MergeOp};
use quarto_sass::ThemeConfig;
use quarto_source_map::SourceInfo;
use quarto_system_runtime::NativeRuntime;
use yaml_rust2::Yaml;

// ── config-construction helpers (same shapes as brand_config_test) ──

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

fn map_config(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
    // Same construction; a nested map value (e.g. the `{light:, dark:}`
    // pair form of `brand:` / `highlight-style:`).
    flattened_config(entries)
}

fn scalar_string(s: &str) -> ConfigValue {
    ConfigValue {
        value: ConfigValueKind::scalar(Yaml::String(s.to_string())),
        source_info: SourceInfo::for_test(),
        merge_op: MergeOp::Concat,
    }
}

/// The unified `_brand.yml` used by most tests: a background pair.
const UNIFIED_BRAND_YAML: &str =
    "color:\n  background:\n    light: \"#b22221\"\n    dark: \"#22b221\"\n";

fn write_brand(dir: &Path, name: &str, contents: &str) {
    std::fs::write(dir.join(name), contents).unwrap();
}

fn resolve(config: &ConfigValue, dir: &Path) -> quarto_sass::ResolvedVariants {
    ThemeConfig::from_config_value(config)
        .expect("config parses")
        .resolve_variants(&NativeRuntime::new(), dir)
        .expect("resolve_variants")
}

fn background_of(rb: &Option<quarto_brand::ResolvedBrand>) -> Option<String> {
    rb.as_ref()?.brand.color.as_ref()?.background.clone()
}

// ── content-driven dark-variant synthesis ───────────────────────────

#[test]
fn unified_brand_file_synthesizes_dark_variant() {
    let dir = tempfile::tempdir().unwrap();
    write_brand(dir.path(), "_brand.yml", UNIFIED_BRAND_YAML);
    let config = flattened_config(vec![
        ("theme", scalar_string("cosmo")),
        ("brand", scalar_string("_brand.yml")),
    ]);
    let rv = resolve(&config, dir.path());

    let dark = rv
        .config
        .dark
        .as_ref()
        .expect("brand dark values must synthesize a dark variant");
    assert!(
        !dark.is_default,
        "a unified brand has no ordering signal; light stays the author default"
    );
    assert_eq!(
        dark.themes.len(),
        rv.config.themes.len(),
        "synthesized dark variant clones the light theme list"
    );
    assert!(
        dark.themes.iter().any(quarto_sass::ThemeSpec::is_brand),
        "synthesized dark variant must carry the brand token"
    );

    assert_eq!(background_of(&rv.light_brand).as_deref(), Some("#b22221"));
    assert_eq!(background_of(&rv.dark_brand).as_deref(), Some("#22b221"));
}

#[test]
fn unified_brand_all_plain_stays_single_variant() {
    let dir = tempfile::tempdir().unwrap();
    write_brand(
        dir.path(),
        "_brand.yml",
        "color:\n  background: \"#b22221\"\n",
    );
    let config = flattened_config(vec![
        ("theme", scalar_string("cosmo")),
        ("brand", scalar_string("_brand.yml")),
    ]);
    let rv = resolve(&config, dir.path());
    assert!(
        rv.config.dark.is_none(),
        "no dark values → no synthesized dark variant"
    );
    assert_eq!(background_of(&rv.light_brand).as_deref(), Some("#b22221"));
    assert!(rv.dark_brand.is_none());
}

#[test]
fn unified_brand_light_only_pair_stays_single_variant() {
    let dir = tempfile::tempdir().unwrap();
    write_brand(
        dir.path(),
        "_brand.yml",
        "color:\n  background:\n    light: \"#b22221\"\n",
    );
    let config = flattened_config(vec![
        ("theme", scalar_string("cosmo")),
        ("brand", scalar_string("_brand.yml")),
    ]);
    let rv = resolve(&config, dir.path());
    assert!(
        rv.config.dark.is_none(),
        "a light-only pair does not enable dark mode"
    );
}

#[test]
fn theme_none_suppresses_brand_and_synthesis() {
    // `theme: none` drops the brand per the existing mutual-exclusion
    // rule; brand content must not resurrect a dark variant.
    let dir = tempfile::tempdir().unwrap();
    write_brand(dir.path(), "_brand.yml", UNIFIED_BRAND_YAML);
    let config = flattened_config(vec![
        ("theme", scalar_string("none")),
        ("brand", scalar_string("_brand.yml")),
    ]);
    let rv = resolve(&config, dir.path());
    assert!(rv.config.dark.is_none());
    assert!(rv.light_brand.is_none());
    assert!(rv.dark_brand.is_none());
}

#[test]
fn explicit_theme_pair_uses_brand_halves_without_double_synthesis() {
    let dir = tempfile::tempdir().unwrap();
    write_brand(dir.path(), "_brand.yml", UNIFIED_BRAND_YAML);
    let theme = map_config(vec![
        ("light", scalar_string("cosmo")),
        ("dark", scalar_string("darkly")),
    ]);
    let config = flattened_config(vec![
        ("theme", theme),
        ("brand", scalar_string("_brand.yml")),
    ]);
    let rv = resolve(&config, dir.path());

    let dark = rv.config.dark.as_ref().expect("declared dark variant");
    assert!(
        dark.themes
            .iter()
            .any(|t| t.as_builtin().map(|b| b.name()) == Some("darkly")),
        "declared dark theme list wins (no synthesized clone of the light list)"
    );
    assert_eq!(background_of(&rv.light_brand).as_deref(), Some("#b22221"));
    assert_eq!(background_of(&rv.dark_brand).as_deref(), Some("#22b221"));
}

// ── two-file form (existing seam) takes matching halves ─────────────

#[test]
fn two_file_brand_resolves_each_variants_file() {
    let dir = tempfile::tempdir().unwrap();
    write_brand(
        dir.path(),
        "light-brand.yml",
        "color:\n  background: \"#b22221\"\n",
    );
    write_brand(
        dir.path(),
        "dark-brand.yml",
        "color:\n  background: \"#22b221\"\n",
    );
    let brand = map_config(vec![
        ("light", scalar_string("light-brand.yml")),
        ("dark", scalar_string("dark-brand.yml")),
    ]);
    let config = flattened_config(vec![("theme", scalar_string("cosmo")), ("brand", brand)]);
    let rv = resolve(&config, dir.path());

    assert!(rv.config.dark.is_some(), "two-file form enables dark mode");
    assert_eq!(background_of(&rv.light_brand).as_deref(), Some("#b22221"));
    assert_eq!(background_of(&rv.dark_brand).as_deref(), Some("#22b221"));
}

#[test]
fn two_file_brand_with_pairs_takes_matching_half() {
    // A file named by the dark side that itself uses `{light:, dark:}`
    // values contributes its DARK half (more permissive than Q1, which
    // rejects pairs inside a mode-specific file; noted in bd-qnylgu69).
    let dir = tempfile::tempdir().unwrap();
    write_brand(
        dir.path(),
        "light-brand.yml",
        "color:\n  background: \"#b22221\"\n",
    );
    write_brand(
        dir.path(),
        "dark-brand.yml",
        "color:\n  background:\n    light: \"#fefefd\"\n    dark: \"#22b221\"\n",
    );
    let brand = map_config(vec![
        ("light", scalar_string("light-brand.yml")),
        ("dark", scalar_string("dark-brand.yml")),
    ]);
    let config = flattened_config(vec![("theme", scalar_string("cosmo")), ("brand", brand)]);
    let rv = resolve(&config, dir.path());
    assert_eq!(background_of(&rv.dark_brand).as_deref(), Some("#22b221"));
}

#[test]
fn single_file_brand_dark_variant_gets_dark_half_when_theme_pair_declared() {
    // `brand: file` + `theme: {light:, dark:}`: the dark variant falls
    // back to the light brand ref; it must receive the DARK half of
    // the split, not a copy of the light half.
    let dir = tempfile::tempdir().unwrap();
    write_brand(
        dir.path(),
        "_brand.yml",
        "color:\n  background:\n    light: \"#b22221\"\n  foreground: \"#333332\"\n",
    );
    let theme = map_config(vec![
        ("light", scalar_string("cosmo")),
        ("dark", scalar_string("darkly")),
    ]);
    let config = flattened_config(vec![
        ("theme", theme),
        ("brand", scalar_string("_brand.yml")),
    ]);
    let rv = resolve(&config, dir.path());
    // background is light-only → omitted from the dark half...
    assert_eq!(background_of(&rv.dark_brand), None);
    // ...while the plain foreground reaches both halves.
    let dark_fg = rv
        .dark_brand
        .as_ref()
        .and_then(|rb| rb.brand.color.as_ref())
        .and_then(|c| c.foreground.clone());
    assert_eq!(dark_fg.as_deref(), Some("#333332"));
}

// ── inline unified brand ────────────────────────────────────────────

#[test]
fn inline_unified_brand_synthesizes_dark_variant() {
    let mut pair = yaml_rust2::yaml::Hash::new();
    pair.insert(Yaml::String("light".into()), Yaml::String("#b22221".into()));
    pair.insert(Yaml::String("dark".into()), Yaml::String("#22b221".into()));
    let mut color_map = yaml_rust2::yaml::Hash::new();
    color_map.insert(Yaml::String("background".into()), Yaml::Hash(pair));
    let mut brand_map = yaml_rust2::yaml::Hash::new();
    brand_map.insert(Yaml::String("color".into()), Yaml::Hash(color_map));
    let brand_value = ConfigValue {
        value: ConfigValueKind::scalar(Yaml::Hash(brand_map)),
        source_info: SourceInfo::for_test(),
        merge_op: MergeOp::Concat,
    };

    let config = flattened_config(vec![
        ("theme", scalar_string("cosmo")),
        ("brand", brand_value),
    ]);
    let rv = resolve(&config, Path::new("."));
    assert!(rv.config.dark.is_some());
    assert_eq!(background_of(&rv.light_brand).as_deref(), Some("#b22221"));
    assert_eq!(background_of(&rv.dark_brand).as_deref(), Some("#22b221"));
}

// ── highlight-style flows to the synthesized dark variant ───────────

#[test]
fn scalar_adaptive_highlight_reaches_synthesized_dark_variant() {
    let dir = tempfile::tempdir().unwrap();
    write_brand(dir.path(), "_brand.yml", UNIFIED_BRAND_YAML);
    let config = flattened_config(vec![
        ("theme", scalar_string("cosmo")),
        ("brand", scalar_string("_brand.yml")),
        ("highlight-style", scalar_string("a11y")),
    ]);
    let rv = resolve(&config, dir.path());
    assert_eq!(
        rv.config.highlight_style.as_ref().map(|h| h.name.as_str()),
        Some("a11y-light")
    );
    assert_eq!(
        rv.config
            .dark
            .as_ref()
            .and_then(|d| d.highlight_style.as_ref())
            .map(|h| h.name.as_str()),
        Some("a11y-dark"),
        "the synthesized dark variant must get the dark-resolved adaptive palette"
    );
}

#[test]
fn pair_highlight_style_dark_value_reaches_synthesized_dark_variant() {
    let dir = tempfile::tempdir().unwrap();
    write_brand(dir.path(), "_brand.yml", UNIFIED_BRAND_YAML);
    // `dracula` is deliberately NON-adaptive: the pair's dark value
    // must arrive verbatim (an adaptive name like `github` would be
    // resolved to `github-dark`, same as in the declared-pair path).
    let highlight = map_config(vec![
        ("light", scalar_string("a11y")),
        ("dark", scalar_string("dracula")),
    ]);
    let config = flattened_config(vec![
        ("theme", scalar_string("cosmo")),
        ("brand", scalar_string("_brand.yml")),
        ("highlight-style", highlight),
    ]);
    let rv = resolve(&config, dir.path());
    assert_eq!(
        rv.config
            .dark
            .as_ref()
            .and_then(|d| d.highlight_style.as_ref())
            .map(|h| h.name.as_str()),
        Some("dracula"),
        "the pair's dark: highlight value must not be dropped when the dark \
         variant is synthesized from brand content"
    );
}
