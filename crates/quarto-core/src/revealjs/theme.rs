/*
 * revealjs/theme.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Resolve a revealjs `theme:` configuration into SCSS layers for compilation.
 */

//! Resolve the `theme:` front-matter for a reveal deck into the SCSS theme
//! layers (and load paths) that `quarto_sass::compile_reveal_theme_css`
//! consumes.
//!
//! `theme:` may be:
//! - absent / null → the `default` (white-equivalent) theme,
//! - a string → one built-in name (`dracula`, `dark`, …; `white`/`black`
//!   alias to `default`/`dark`) or a path to a user `.scss` file,
//! - an array → several entries layered left→right (e.g. `[dark, custom.scss]`),
//!   where later entries win (their `!default`s and rules override earlier ones).
//!
//! Built-in names resolve to the embedded reveal themes
//! (`quarto_sass::load_reveal_theme_layer`); anything else is treated as a user
//! `.scss` file resolved against the document directory
//! (`quarto_sass::load_custom_theme`). This mirrors Quarto 1's resolution while
//! keeping the reveal-specific name set (Bootstrap's `ThemeConfig` would reject
//! reveal theme names).

use std::path::{Path, PathBuf};

use quarto_pandoc_types::ConfigValue;
use quarto_sass::{SassError, SassLayer, ThemeContext};
use quarto_system_runtime::SystemRuntime;

/// The resolved reveal theme: ordered SCSS layers plus any extra load paths
/// (directories of user `.scss` themes, for `@use`/`@import` resolution).
#[derive(Debug, Default)]
pub struct RevealThemeResolution {
    pub layers: Vec<SassLayer>,
    pub load_paths: Vec<PathBuf>,
}

/// Read the `theme:` entries from merged metadata as a list of names/paths.
///
/// Returns `["default"]` when `theme:` is absent, null, empty, or an
/// unexpected shape (so a deck always gets the white-equivalent default).
fn theme_entries(meta: &ConfigValue) -> Vec<String> {
    match meta.get("theme") {
        None => vec!["default".to_string()],
        Some(value) if value.is_null() => vec!["default".to_string()],
        Some(value) => {
            if let Some(items) = value.as_array() {
                let names: Vec<String> = items.iter().filter_map(|i| i.as_plain_text()).collect();
                if names.is_empty() {
                    vec!["default".to_string()]
                } else {
                    names
                }
            } else if let Some(s) = value.as_plain_text() {
                vec![s]
            } else {
                vec!["default".to_string()]
            }
        }
    }
}

/// Resolve a deck's `theme:` into SCSS theme layers + load paths.
///
/// `document_dir` is the directory of the input `.qmd` (user `.scss` themes
/// resolve against it); `runtime` provides cross-platform file access.
pub fn resolve_reveal_theme(
    meta: &ConfigValue,
    document_dir: &Path,
    runtime: &dyn SystemRuntime,
) -> Result<RevealThemeResolution, SassError> {
    let context = ThemeContext::new(document_dir.to_path_buf(), runtime);
    let mut resolution = RevealThemeResolution::default();

    for entry in theme_entries(meta) {
        if quarto_sass::resolve_reveal_theme_name(&entry).is_some() {
            // A built-in reveal theme (or `white`/`black` alias).
            resolution
                .layers
                .push(quarto_sass::load_reveal_theme_layer(&entry)?);
        } else {
            // Otherwise treat the entry as a path to a user `.scss` theme,
            // resolved against the document directory.
            let (layer, dir) = quarto_sass::load_custom_theme(Path::new(&entry), &context)?;
            resolution.layers.push(layer);
            resolution.load_paths.push(dir);
        }
    }

    Ok(resolution)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_source_map::SourceInfo;
    use quarto_system_runtime::NativeRuntime;

    fn s(v: &str) -> ConfigValue {
        ConfigValue::new_string(v, SourceInfo::for_test())
    }
    fn meta_with_theme(value: ConfigValue) -> ConfigValue {
        let mut m = ConfigValue::new_map(vec![], SourceInfo::for_test());
        m.insert_path(&["theme"], value);
        m
    }

    #[test]
    fn entries_default_when_absent_or_null() {
        let empty = ConfigValue::new_map(vec![], SourceInfo::for_test());
        assert_eq!(theme_entries(&empty), vec!["default".to_string()]);
    }

    #[test]
    fn entries_string_and_array() {
        assert_eq!(
            theme_entries(&meta_with_theme(s("dracula"))),
            vec!["dracula".to_string()]
        );
        let arr = ConfigValue::new_array(vec![s("dark"), s("custom.scss")], SourceInfo::for_test());
        assert_eq!(
            theme_entries(&meta_with_theme(arr)),
            vec!["dark".to_string(), "custom.scss".to_string()]
        );
    }

    #[test]
    fn resolve_builtin_single_no_load_path() {
        let rt = NativeRuntime::new();
        let r = resolve_reveal_theme(&meta_with_theme(s("dracula")), Path::new("."), &rt).unwrap();
        assert_eq!(r.layers.len(), 1);
        assert!(r.load_paths.is_empty(), "built-in themes need no load path");
    }

    #[test]
    fn resolve_array_with_aliases() {
        let rt = NativeRuntime::new();
        // white→default, black→dark — both resolve to built-in layers.
        let arr = ConfigValue::new_array(vec![s("white"), s("black")], SourceInfo::for_test());
        let r = resolve_reveal_theme(&meta_with_theme(arr), Path::new("."), &rt).unwrap();
        assert_eq!(r.layers.len(), 2);
    }

    #[test]
    fn resolve_default_when_absent() {
        let rt = NativeRuntime::new();
        let empty = ConfigValue::new_map(vec![], SourceInfo::for_test());
        let r = resolve_reveal_theme(&empty, Path::new("."), &rt).unwrap();
        assert_eq!(r.layers.len(), 1, "absent theme → the default layer");
    }

    #[test]
    fn resolve_unknown_name_errors() {
        let rt = NativeRuntime::new();
        // Not a built-in and not an existing file → error (treated as a
        // missing user `.scss`).
        let r = resolve_reveal_theme(
            &meta_with_theme(s("no-such-theme")),
            Path::new("/nonexistent-dir-xyz"),
            &rt,
        );
        assert!(r.is_err());
    }
}
