//! Tests for [`ResolvedBrand`] — rebasing brand-relative paths to
//! project-relative form.
//!
//! Paths inside a `_brand.yml` are written relative to that file's own
//! directory. Consumers (the website favicon, bd-97yc; the navbar logo,
//! bd-hp3tx) need them relative to the project root instead. Quarto 1
//! does this eagerly in `Brand.resolvePath`
//! (`external-sources/quarto-cli/src/core/brand/brand.ts:247`) using a
//! `relative(projectDir, brandDir)` prefix; these tests pin the Q2
//! on-demand equivalent.

use std::path::{Path, PathBuf};

use quarto_brand::{Brand, ResolvedBrand};

fn brand(yaml: &str) -> Brand {
    Brand::from_yaml_str(yaml).expect("parse")
}

/// A brand loaded from `<project>/<subdir>/_brand.yml`.
fn resolved_in(subdir: &str, yaml: &str) -> (ResolvedBrand, PathBuf) {
    let project = PathBuf::from("/project");
    let dir = if subdir.is_empty() {
        project.clone()
    } else {
        project.join(subdir)
    };
    (ResolvedBrand::new(brand(yaml), Some(dir)), project)
}

const SMALL_LOGO: &str = "logo:\n  small: logo.png\n";

// ── favicon_relative_to ────────────────────────────────────────────

#[test]
fn favicon_at_project_root_is_unchanged() {
    let (resolved, project) = resolved_in("", SMALL_LOGO);
    assert_eq!(
        resolved.favicon_relative_to(&project),
        Some("logo.png".to_string())
    );
}

/// The case that distinguishes real rebasing from passing the raw YAML
/// path through: at the project root the two forms coincide.
#[test]
fn favicon_in_subdirectory_gains_the_subdirectory_prefix() {
    let (resolved, project) = resolved_in("_brand", SMALL_LOGO);
    assert_eq!(
        resolved.favicon_relative_to(&project),
        Some("_brand/logo.png".to_string())
    );
}

#[test]
fn favicon_in_nested_subdirectory_gains_the_full_prefix() {
    let (resolved, project) = resolved_in("assets/branding", SMALL_LOGO);
    assert_eq!(
        resolved.favicon_relative_to(&project),
        Some("assets/branding/logo.png".to_string())
    );
}

/// A logo path may itself be relative — the prefix and the path compose.
#[test]
fn favicon_path_with_own_subdirectory_composes_with_prefix() {
    let (resolved, project) = resolved_in("_brand", "logo:\n  small: images/logo.png\n");
    assert_eq!(
        resolved.favicon_relative_to(&project),
        Some("_brand/images/logo.png".to_string())
    );
}

/// A brand outside the project resolves to a `..`-style path rather
/// than an absolute one.
#[test]
fn favicon_in_sibling_directory_resolves_upward() {
    let resolved = ResolvedBrand::new(brand(SMALL_LOGO), Some(PathBuf::from("/shared/branding")));
    assert_eq!(
        resolved.favicon_relative_to(Path::new("/project")),
        Some("../shared/branding/logo.png".to_string())
    );
}

/// An inline `brand:` block has no file, so its paths are already
/// written relative to the project.
#[test]
fn favicon_from_inline_brand_is_unchanged() {
    let resolved = ResolvedBrand::new(brand(SMALL_LOGO), None);
    assert_eq!(
        resolved.favicon_relative_to(Path::new("/project")),
        Some("logo.png".to_string())
    );
}

// ── paths that must survive verbatim ───────────────────────────────

#[test]
fn favicon_external_url_is_never_rebased() {
    let (resolved, project) =
        resolved_in("_brand", "logo:\n  small: https://cdn.example.com/l.png\n");
    assert_eq!(
        resolved.favicon_relative_to(&project),
        Some("https://cdn.example.com/l.png".to_string())
    );
}

#[test]
fn favicon_protocol_relative_url_is_never_rebased() {
    let (resolved, project) = resolved_in("_brand", "logo:\n  small: //cdn.example.com/l.png\n");
    assert_eq!(
        resolved.favicon_relative_to(&project),
        Some("//cdn.example.com/l.png".to_string())
    );
}

#[test]
fn favicon_rooted_path_is_never_rebased() {
    let (resolved, project) = resolved_in("_brand", "logo:\n  small: /assets/logo.png\n");
    assert_eq!(
        resolved.favicon_relative_to(&project),
        Some("/assets/logo.png".to_string())
    );
}

// ── absent / undecidable favicons ──────────────────────────────────

#[test]
fn no_small_logo_yields_no_favicon() {
    let (resolved, project) = resolved_in("_brand", "logo:\n  large: big.png\n");
    assert_eq!(resolved.favicon_relative_to(&project), None);
}

/// `Brand::favicon` declines to pick a side of a light/dark pair, and
/// rebasing must not paper over that (bd-v5z8w owns the choice).
#[test]
fn light_dark_small_logo_yields_no_favicon() {
    let (resolved, project) = resolved_in(
        "_brand",
        "logo:\n  small:\n    light: l.png\n    dark: d.png\n",
    );
    assert_eq!(resolved.favicon_relative_to(&project), None);
}

// ── logo_resource_relative_to (the seam bd-hp3tx will use) ─────────

#[test]
fn named_logo_resource_is_rebased_and_keeps_alt_text() {
    let (resolved, project) = resolved_in(
        "_brand",
        "logo:\n  medium:\n    path: logo-med.png\n    alt: \"Acme\"\n",
    );
    let logo = resolved
        .logo_resource_relative_to("medium", &project)
        .expect("medium logo");
    assert_eq!(logo.path(), "_brand/logo-med.png");
    assert_eq!(
        logo.alt(),
        Some("Acme"),
        "rebasing must not drop alt text — the navbar needs it"
    );
}

#[test]
fn extra_logo_image_is_reachable_by_name() {
    let (resolved, project) = resolved_in(
        "_brand",
        "logo:\n  images:\n    icon:\n      path: icon.svg\n      alt: \"Icon\"\n",
    );
    let logo = resolved
        .logo_resource_relative_to("icon", &project)
        .expect("named image");
    assert_eq!(logo.path(), "_brand/icon.svg");
    assert_eq!(logo.alt(), Some("Icon"));
}

#[test]
fn unknown_logo_name_yields_none() {
    let (resolved, project) = resolved_in("_brand", SMALL_LOGO);
    assert!(
        resolved
            .logo_resource_relative_to("nonexistent", &project)
            .is_none()
    );
}

/// Named sizes and `logo.images.*` are separate namespaces, as in Q1's
/// `getLogo` / `getLogoResource`. A named size that exists but is a
/// light/dark pair must yield `None`, not silently fall through to an
/// `images` entry that happens to share the name.
#[test]
fn light_dark_named_size_does_not_fall_through_to_images() {
    let (resolved, project) = resolved_in(
        "_brand",
        "logo:\n  \
         images:\n    \
         small:\n      \
         path: decoy.png\n  \
         small:\n    \
         light: l.png\n    \
         dark: d.png\n",
    );
    assert!(
        resolved
            .logo_resource_relative_to("small", &project)
            .is_none(),
        "a light/dark `small` has no single resource; the `images.small` \
         decoy must not stand in for it"
    );
}

// ── path_prefix_relative_to ────────────────────────────────────────

#[test]
fn prefix_is_empty_at_project_root() {
    let (resolved, project) = resolved_in("", SMALL_LOGO);
    assert_eq!(
        resolved.path_prefix_relative_to(&project),
        PathBuf::from("")
    );
}

#[test]
fn prefix_is_empty_for_inline_brand() {
    let resolved = ResolvedBrand::new(brand(SMALL_LOGO), None);
    assert_eq!(
        resolved.path_prefix_relative_to(Path::new("/project")),
        PathBuf::from("")
    );
}

#[test]
fn prefix_names_the_brand_subdirectory() {
    let (resolved, project) = resolved_in("_brand", SMALL_LOGO);
    assert_eq!(
        resolved.path_prefix_relative_to(&project),
        PathBuf::from("_brand")
    );
}
