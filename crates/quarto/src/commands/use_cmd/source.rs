//! Reading a brand out of a fetched or local source directory
//! (bd-1vlw8, Phase 7).
//!
//! By the time this runs, `quarto-source-fetch` has produced a
//! directory: either a local path the user pointed at, or an extracted
//! archive rooted at whatever the archive actually contained. This
//! module answers three questions about it:
//!
//! 1. Is there a brand file, and does it parse?
//! 2. Which other files does that brand reference?
//! 3. Given the answer to 2, where should the brand land in the
//!    project — the root, or `_brand/`?
//!
//! Validation happens **before** anything is planned, let alone
//! written. A source whose `_brand.yml` does not parse is refused with
//! the parser's own complaint rather than copied into the user's
//! project to fail later at render time.

use std::path::{Path, PathBuf};

use quarto_brand::{Brand, BrandFont, BrandFontFileEntry, BrandLogoResource, LogoEntry};

use crate::commands::common::plan::CommandFailure;

/// Brand file spellings, in probe order.
const BRAND_FILENAMES: [&str; 2] = ["_brand.yml", "_brand.yaml"];

/// A validated brand found in a source directory.
#[derive(Debug)]
pub struct SourceBrand {
    /// Absolute path of the brand file within the source.
    pub brand_file: PathBuf,
    /// Files the brand references, relative to the brand file's
    /// directory. Only local, in-tree, existing files.
    pub assets: Vec<PathBuf>,
}

impl SourceBrand {
    /// Where this brand should land in the project.
    ///
    /// A lone brand file goes to the project root as `_brand.yml` —
    /// the layout Quarto 1 users recognize, and the one people write by
    /// hand. A brand that carries logos or font files goes to `_brand/`
    /// so those assets do not scatter across the project root.
    ///
    /// Quarto 2 writes the `brand:` key either way, so this is purely a
    /// tidiness choice, not a discovery requirement.
    pub fn destination(&self) -> Destination {
        if self.assets.is_empty() {
            Destination::ProjectRoot
        } else {
            Destination::BrandDirectory
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destination {
    /// `<project>/_brand.yml`
    ProjectRoot,
    /// `<project>/_brand/_brand.yml`, with assets alongside.
    BrandDirectory,
}

impl Destination {
    /// The path written into `_quarto.yml`'s `brand:` key.
    pub fn declared_path(self) -> &'static str {
        match self {
            Destination::ProjectRoot => "_brand.yml",
            Destination::BrandDirectory => "_brand/_brand.yml",
        }
    }

    /// Project-relative directory the files are written under.
    pub fn directory(self) -> Option<&'static str> {
        match self {
            Destination::ProjectRoot => None,
            Destination::BrandDirectory => Some("_brand"),
        }
    }
}

/// Find and validate the brand in `source`.
pub fn read_source_brand(source: &Path, target_label: &str) -> Result<SourceBrand, CommandFailure> {
    let brand_file = BRAND_FILENAMES
        .iter()
        .map(|name| source.join(name))
        .find(|p| p.is_file())
        .ok_or_else(|| no_brand_file_failure(source, target_label))?;

    let text = std::fs::read_to_string(&brand_file).map_err(|e| {
        CommandFailure::new(
            "Could not read the brand file",
            format!("{}: {e}", brand_file.display()),
        )
    })?;

    // Parse before planning anything. `quarto-brand` uses
    // `deny_unknown_fields`, so a typo'd key is caught here rather than
    // becoming a render-time surprise in the user's project.
    // Parse as the unified form ({light:, dark:} color values allowed
    // — the file is copied verbatim, so unified content stays intact);
    // asset collection reads fonts and logos, which the split carries
    // identically in both halves, so the light half suffices.
    let brand = quarto_brand::UnifiedBrand::from_yaml_str(&text).map_err(|e| {
        CommandFailure::new(
            format!("{target_label} does not contain a valid brand"),
            format!(
                "{} could not be parsed: {e}\n\nNothing was written.",
                brand_file.display()
            ),
        )
    })?;

    let brand = brand.split().light;
    let brand_dir = brand_file.parent().unwrap_or(source);
    let assets = collect_assets(&brand, brand_dir);

    Ok(SourceBrand { brand_file, assets })
}

/// Build the "no brand here" failure, with a better message when the
/// source looks like a Quarto 1 brand *extension*.
fn no_brand_file_failure(source: &Path, target_label: &str) -> CommandFailure {
    if let Some(extension_brand) = quarto1_brand_extension_file(source) {
        return CommandFailure::new(
            format!("{target_label} is a Quarto 1 brand extension"),
            format!(
                "It declares its brand as `contributes.metadata.project.brand: {extension_brand}` \
                 in an _extension.yml. Quarto 2 has no extension system yet, so `q2 use brand` \
                 cannot install it.\n\nYou can still use the brand by hand: copy {extension_brand} \
                 into your project as _brand.yml and add `brand: _brand.yml` to _quarto.yml."
            ),
        );
    }

    CommandFailure::new(
        format!("No brand file found in {target_label}"),
        format!(
            "Looked for {} in {}. A brand source must contain one of those at its top level.",
            BRAND_FILENAMES.join(" or "),
            source.display()
        ),
    )
}

/// Detect a Quarto 1 brand extension, returning the brand path it
/// declares.
///
/// We do not support these (decision 12 in the plan) — the layout
/// presumes an `_extensions/` system Quarto 2 does not have. Detecting
/// one is cheap and turns a baffling "no brand file found" into an
/// error that explains itself.
fn quarto1_brand_extension_file(source: &Path) -> Option<String> {
    for name in ["_extension.yml", "_extension.yaml"] {
        let path = source.join(name);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(&text) else {
            continue;
        };
        if let Some(brand) = value
            .get("contributes")
            .and_then(|c| c.get("metadata"))
            .and_then(|m| m.get("project"))
            .and_then(|p| p.get("brand"))
            .and_then(|b| b.as_str())
        {
            return Some(brand.to_string());
        }
    }
    None
}

/// Collect the local files a brand references.
///
/// Mirrors Quarto 1's `extractBrandFilePaths` (`brand.ts:134-212`), but
/// over the typed model instead of untyped probing — every field below
/// is a real field on `quarto_brand::types`, so a schema change is a
/// compile error here rather than a silently missed asset.
///
/// Three filters apply, in order: remote URLs are skipped (nothing to
/// copy), paths that escape the brand file's directory are skipped
/// (same escape concern as an archive entry — a brand file is
/// attacker-controlled too), and paths that do not exist are skipped
/// (a brand may reference a file it does not ship).
fn collect_assets(brand: &Brand, brand_dir: &Path) -> Vec<PathBuf> {
    let mut declared: Vec<String> = Vec::new();

    if let Some(logo) = &brand.logo {
        for entry in [&logo.small, &logo.medium, &logo.large]
            .into_iter()
            .flatten()
        {
            match entry {
                LogoEntry::Single(resource) => declared.push(resource.path().to_string()),
                LogoEntry::LightDark { light, dark } => {
                    for side in [light, dark].into_iter().flatten() {
                        declared.push(side.path().to_string());
                    }
                }
            }
        }
        if let Some(images) = &logo.images {
            for resource in images.values() {
                match resource {
                    BrandLogoResource::Path(p) => declared.push(p.clone()),
                    BrandLogoResource::Explicit(e) => declared.push(e.path.clone()),
                }
            }
        }
    }

    if let Some(typography) = &brand.typography {
        for font in &typography.fonts {
            // Only `source: file` fonts ship bytes. Google/Bunny fonts
            // are fetched by the browser and system fonts are already
            // installed; neither has anything to copy.
            if let BrandFont::File(file_font) = font {
                for entry in &file_font.files {
                    match entry {
                        BrandFontFileEntry::Path(p) => declared.push(p.clone()),
                        BrandFontFileEntry::Explicit { path, .. } => declared.push(path.clone()),
                    }
                }
            }
        }
    }

    let mut assets: Vec<PathBuf> = Vec::new();
    for path in declared {
        if is_remote(&path) {
            continue;
        }
        let Some(relative) = safe_relative(&path) else {
            continue;
        };
        if brand_dir.join(&relative).is_file() && !assets.contains(&relative) {
            assets.push(relative);
        }
    }
    assets.sort();
    assets
}

fn is_remote(path: &str) -> bool {
    path.starts_with("http://") || path.starts_with("https://") || path.starts_with("data:")
}

/// Accept only plain relative paths, on the same reasoning as archive
/// entry names: a brand file we just downloaded is untrusted input, and
/// `logo: ../../../etc/passwd` would otherwise be copied into the
/// user's project.
fn safe_relative(path: &str) -> Option<PathBuf> {
    if path.is_empty() || path.starts_with('/') || path.starts_with('\\') {
        return None;
    }
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return None;
    }
    let mut out = PathBuf::new();
    for segment in path.split(['/', '\\']) {
        match segment {
            "" | "." => continue,
            ".." => return None,
            other => out.push(other),
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn source_with(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for (name, content) in files {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        dir
    }

    #[test]
    fn a_lone_brand_file_targets_the_project_root() {
        let src = source_with(&[("_brand.yml", "color:\n  primary: red\n")]);
        let brand = read_source_brand(src.path(), "the source").unwrap();
        assert!(brand.assets.is_empty());
        assert_eq!(brand.destination(), Destination::ProjectRoot);
        assert_eq!(brand.destination().declared_path(), "_brand.yml");
    }

    #[test]
    fn the_yaml_spelling_is_accepted() {
        let src = source_with(&[("_brand.yaml", "color:\n  primary: red\n")]);
        let brand = read_source_brand(src.path(), "the source").unwrap();
        assert!(brand.brand_file.ends_with("_brand.yaml"));
    }

    #[test]
    fn a_brand_with_a_logo_targets_the_brand_directory() {
        let src = source_with(&[
            ("_brand.yml", "logo:\n  small: logo.png\n"),
            ("logo.png", "not really a png"),
        ]);
        let brand = read_source_brand(src.path(), "the source").unwrap();
        assert_eq!(brand.assets, [PathBuf::from("logo.png")]);
        assert_eq!(brand.destination(), Destination::BrandDirectory);
        assert_eq!(brand.destination().declared_path(), "_brand/_brand.yml");
    }

    #[test]
    fn light_dark_logos_and_named_images_are_collected() {
        let src = source_with(&[
            (
                "_brand.yml",
                "logo:\n  small:\n    light: light.png\n    dark: dark.png\n  \
                 images:\n    hero: hero.svg\n",
            ),
            ("light.png", "x"),
            ("dark.png", "x"),
            ("hero.svg", "x"),
        ]);
        let brand = read_source_brand(src.path(), "the source").unwrap();
        assert_eq!(
            brand.assets,
            [
                PathBuf::from("dark.png"),
                PathBuf::from("hero.svg"),
                PathBuf::from("light.png")
            ]
        );
    }

    #[test]
    fn only_file_source_fonts_contribute_assets() {
        let src = source_with(&[
            (
                "_brand.yml",
                "typography:\n  fonts:\n    - family: Open Sans\n      source: google\n    \
                 - family: Mine\n      source: file\n      files:\n        - fonts/mine.woff2\n",
            ),
            ("fonts/mine.woff2", "x"),
        ]);
        let brand = read_source_brand(src.path(), "the source").unwrap();
        assert_eq!(
            brand.assets,
            [PathBuf::from("fonts").join("mine.woff2")],
            "a google-sourced font ships no bytes to copy"
        );
    }

    #[test]
    fn remote_and_missing_asset_paths_are_skipped() {
        let src = source_with(&[(
            "_brand.yml",
            "logo:\n  small: https://example.com/logo.png\n  medium: absent.png\n",
        )]);
        let brand = read_source_brand(src.path(), "the source").unwrap();
        assert!(brand.assets.is_empty());
        assert_eq!(brand.destination(), Destination::ProjectRoot);
    }

    #[test]
    fn escaping_asset_paths_are_refused() {
        // A downloaded brand file is untrusted input just like the
        // archive that carried it.
        let src = source_with(&[
            ("_brand.yml", "logo:\n  small: ../../secret.png\n"),
            ("secret.png", "x"),
        ]);
        let brand = read_source_brand(src.path(), "the source").unwrap();
        assert!(
            brand.assets.is_empty(),
            "a traversing asset path must not be collected: {:?}",
            brand.assets
        );
    }

    #[test]
    fn an_invalid_brand_is_refused_before_anything_is_planned() {
        let src = source_with(&[("_brand.yml", "color:\n  nonsense_key: red\n")]);
        let err = read_source_brand(src.path(), "the source")
            .expect_err("an unknown key must be refused");
        let text = err.0.to_text(None);
        assert!(text.contains("valid brand"), "{text}");
        assert!(text.contains("Nothing was written"), "{text}");
    }

    #[test]
    fn a_source_with_no_brand_says_what_it_looked_for() {
        let src = source_with(&[("README.md", "hi")]);
        let err = read_source_brand(src.path(), "org/repo").expect_err("no brand here");
        let text = err.0.to_text(None);
        assert!(
            text.contains("_brand.yml") && text.contains("_brand.yaml"),
            "{text}"
        );
        assert!(text.contains("org/repo"), "{text}");
    }

    #[test]
    fn a_quarto1_brand_extension_gets_its_own_explanation() {
        let src = source_with(&[
            (
                "_extension.yml",
                "title: Acme Brand\ncontributes:\n  metadata:\n    project:\n      brand: brand.yml\n",
            ),
            ("brand.yml", "color:\n  primary: red\n"),
        ]);
        let err = read_source_brand(src.path(), "org/acme-brand")
            .expect_err("brand extensions are out of scope");
        let text = err.0.to_text(None);
        assert!(text.contains("brand extension"), "{text}");
        assert!(
            text.contains("brand.yml"),
            "the error should name the file so the user can copy it: {text}"
        );
    }
}
