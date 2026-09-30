//! `resolve_brand_variants` (bd-67i2z57f): resolving both halves of a
//! brand for consumers that pick their mode per document (Typst's
//! `brand-mode`) rather than compiling a light/dark CSS pair.
//!
//! Unlike `ThemeConfig::resolve_variants` (exercised in
//! `brand_light_dark_test`), there is no theme/highlight-style
//! machinery here and no "does the brand content enable dark mode"
//! gate — the dark half is always returned alongside the light half,
//! since `quarto_brand::SplitBrand` guarantees both halves exist once
//! a brand is configured.

use std::path::Path;

use quarto_pandoc_types::{ConfigMapEntry, ConfigValue, ConfigValueKind, MergeOp};
use quarto_sass::resolve_brand_variants;
use quarto_source_map::SourceInfo;
use quarto_system_runtime::NativeRuntime;
use yaml_rust2::Yaml;

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
    flattened_config(entries)
}

fn scalar_string(s: &str) -> ConfigValue {
    ConfigValue {
        value: ConfigValueKind::scalar(Yaml::String(s.to_string())),
        source_info: SourceInfo::for_test(),
        merge_op: MergeOp::Concat,
    }
}

fn write_brand(dir: &Path, name: &str, contents: &str) {
    std::fs::write(dir.join(name), contents).unwrap();
}

fn background_of(rb: &Option<quarto_brand::ResolvedBrand>) -> Option<String> {
    rb.as_ref()?.brand.color.as_ref()?.background.clone()
}

/// No `brand:` key at all: both halves absent, not an error.
#[test]
fn no_brand_yields_neither_half() {
    let config = flattened_config(vec![]);
    let (light, dark) =
        resolve_brand_variants(&config, &NativeRuntime::new(), Path::new(".")).unwrap();
    assert!(light.is_none());
    assert!(dark.is_none());
}

/// A unified `_brand.yml` with a `{light:, dark:}` pair resolves both
/// halves — the case `resolve_brand` (single-variant) drops entirely.
#[test]
fn unified_brand_pair_resolves_both_halves() {
    let dir = tempfile::tempdir().unwrap();
    write_brand(
        dir.path(),
        "_brand.yml",
        "color:\n  background:\n    light: \"#b22221\"\n    dark: \"#22b221\"\n",
    );
    let config = flattened_config(vec![("brand", scalar_string("_brand.yml"))]);
    let (light, dark) = resolve_brand_variants(&config, &NativeRuntime::new(), dir.path()).unwrap();
    assert_eq!(background_of(&light).as_deref(), Some("#b22221"));
    assert_eq!(background_of(&dark).as_deref(), Some("#22b221"));
}

/// An all-plain brand (no `dark:` side anywhere) still yields a dark
/// half — equal to the light half, per `SplitBrand`'s "both halves
/// always exist" contract. This is the bd-67i2z57f fix: previously
/// the typst path never resolved a dark half at all, so a document
/// with `brand-mode: dark` and an all-plain brand would have seen
/// `brand[dark]` as entirely absent instead of matching `brand[light]`.
#[test]
fn all_plain_brand_dark_half_equals_light_half() {
    let dir = tempfile::tempdir().unwrap();
    write_brand(
        dir.path(),
        "_brand.yml",
        "color:\n  background: \"#b22221\"\n",
    );
    let config = flattened_config(vec![("brand", scalar_string("_brand.yml"))]);
    let (light, dark) = resolve_brand_variants(&config, &NativeRuntime::new(), dir.path()).unwrap();
    assert_eq!(background_of(&light).as_deref(), Some("#b22221"));
    assert_eq!(background_of(&dark).as_deref(), Some("#b22221"));
}

/// Two-file `brand: {light:, dark:}` form: each half comes from its
/// own file.
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
    let config = flattened_config(vec![("brand", brand)]);
    let (light, dark) = resolve_brand_variants(&config, &NativeRuntime::new(), dir.path()).unwrap();
    assert_eq!(background_of(&light).as_deref(), Some("#b22221"));
    assert_eq!(background_of(&dark).as_deref(), Some("#22b221"));
}

/// A two-file form whose dark file itself uses a `{light:, dark:}`
/// pair contributes its DARK half — same "matching half" rule as
/// `ThemeConfig::resolve_variants`.
#[test]
fn two_file_brand_with_pairs_takes_matching_half() {
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
    let config = flattened_config(vec![("brand", brand)]);
    let (_, dark) = resolve_brand_variants(&config, &NativeRuntime::new(), dir.path()).unwrap();
    assert_eq!(background_of(&dark).as_deref(), Some("#22b221"));
}

/// A single-file brand with only a `light:` side declared on the
/// `brand:` map falls back to the same file for `dark:`, and still
/// gets that file's own dark half (not a copy of the light half).
#[test]
fn single_file_brand_falls_back_to_same_file_for_dark() {
    let dir = tempfile::tempdir().unwrap();
    write_brand(
        dir.path(),
        "_brand.yml",
        "color:\n  background:\n    light: \"#b22221\"\n  foreground: \"#333332\"\n",
    );
    let config = flattened_config(vec![("brand", scalar_string("_brand.yml"))]);
    let (_, dark) = resolve_brand_variants(&config, &NativeRuntime::new(), dir.path()).unwrap();
    // background is light-only → omitted from the dark half...
    assert_eq!(background_of(&dark), None);
    // ...while the plain foreground reaches both halves.
    let dark_fg = dark
        .as_ref()
        .and_then(|rb| rb.brand.color.as_ref())
        .and_then(|c| c.foreground.clone());
    assert_eq!(dark_fg.as_deref(), Some("#333332"));
}
